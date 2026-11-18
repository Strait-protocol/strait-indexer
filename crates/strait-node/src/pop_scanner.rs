//! PoP scanner: reads Hemi keystone publications from the PoP miners' Bitcoin
//! transactions and marks transfers Bitcoin-final, independent of the
//! PoPPayoutsV2 reward contract.
//!
//! Rather than downloading every Bitcoin block, it follows the transaction
//! history of the known miner wallets (`POP_MINER_ADDRESSES`) through Esplora:
//! a few MB to backfill, then about one small request a minute per miner.
//! Publications from wallets not in that list are not seen.
//!
//! Each tick:
//!  1. **Sync**: for each miner, read its confirmed transactions newest-first
//!     back to where the last sync ended (minus a reorg margin), or to
//!     `POP_SCAN_START_HEIGHT` on the first run, storing `OP_RETURN "HEMI"`
//!     publications. Re-read publications are moved if a reorg re-mined them.
//!  2. **Validate**: check each new publication against the canonical Hemi block
//!     it names (block hash / parent hash / state root prefixes). Forged or
//!     non-canonical publications are marked invalid and never anchor anything.
//!  3. **Anchor**: mark transfers covered by a publication with enough
//!     confirmations, and enqueue `transfer.pop_anchored` webhooks.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use alloy::providers::Provider;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use strait_api::webhooks::dispatcher::{self, event};
use strait_bitcoin::pop::{EsploraClient, KeystoneAbrev};
use strait_core::error::Result;
use strait_store::{Database, NewPublication, PopRepo, TunnelTransferRepo};

/// Blocks re-read below a miner's last synced height, to pick up publications
/// that a Bitcoin reorg moved.
const REORG_MARGIN: i64 = 6;
/// Publications validated per tick.
const VALIDATE_BATCH: i64 = 500;
/// Sleep between ticks (~1 Bitcoin block / 10 min).
const TICK_INTERVAL: Duration = Duration::from_secs(60);

pub struct PopScanner {
    esplora: EsploraClient,
    hemi_provider: Arc<dyn Provider>,
    db: Database,
    miners: Vec<String>,
    start_height: u64,
    min_confirmations: i64,
    page_delay: Duration,
}

impl PopScanner {
    pub fn new(
        esplora: EsploraClient,
        hemi_provider: Arc<dyn Provider>,
        db: Database,
        miners: Vec<String>,
        start_height: u64,
        min_confirmations: u32,
        page_delay_ms: u64,
    ) -> Self {
        Self {
            esplora,
            hemi_provider,
            db,
            miners,
            start_height,
            min_confirmations: i64::from(min_confirmations.max(1)),
            page_delay: Duration::from_millis(page_delay_ms),
        }
    }

    pub async fn run(self) -> Result<()> {
        info!(
            miners = ?self.miners,
            start_height = self.start_height,
            min_confirmations = self.min_confirmations,
            "PoP scanner started (miner wallets → Bitcoin keystone publications)"
        );
        while let Err(e) = self.init().await {
            warn!(error = %e, "PoP scanner init failed — retrying");
            sleep(TICK_INTERVAL).await;
        }
        loop {
            if let Err(e) = self.tick().await {
                warn!(error = %e, "PoP scanner tick failed");
            }
            sleep(TICK_INTERVAL).await;
        }
    }

    async fn init(&self) -> Result<()> {
        let start_time = self.esplora.block_time(self.start_height).await?;
        let repo = PopRepo::new(&self.db);
        repo.init_state(self.start_height as i64, start_time).await?;
        for m in &self.miners {
            repo.ensure_miner(m).await?;
        }
        Ok(())
    }

    async fn tick(&self) -> Result<()> {
        let tip = self.esplora.tip_height().await? as i64;
        let repo = PopRepo::new(&self.db);
        // The synced height is the lowest across miners: a miner whose sync
        // failed holds it back, so its missing history can't skew "earliest".
        let mut synced: Option<i64> = Some(tip);
        for miner in &self.miners {
            match self.sync_miner(miner, tip).await {
                Ok(()) => {}
                Err(e) => warn!(miner, error = %e, "PoP miner sync failed — will retry"),
            }
            let h = repo.miner_synced_height(miner).await?;
            synced = match (synced, h) {
                (Some(a), Some(b)) => Some(a.min(b)),
                _ => None,
            };
        }
        repo.set_synced(synced, tip).await?;
        self.validate().await?;
        self.anchor().await
    }

    async fn sync_miner(&self, miner: &str, tip: i64) -> Result<()> {
        let repo = PopRepo::new(&self.db);
        let start = self.start_height as i64;
        let floor = match repo.miner_synced_height(miner).await? {
            Some(h) => (h - REORG_MARGIN).max(start),
            None => start,
        };
        let (mut after, mut pages, mut found) = (None::<String>, 0u32, 0usize);
        loop {
            let page = self.esplora.address_txs(miner, after.as_deref()).await?;
            let Some(last) = page.last() else { break };
            let oldest = last.block_height as i64;
            after = Some(last.txid.clone());
            pages += 1;
            for tx in &page {
                if (tx.block_height as i64) < start {
                    continue;
                }
                for (vout, k) in &tx.publications {
                    repo.upsert_publication(&NewPublication {
                        btc_txid: tx.txid.clone(),
                        vout: *vout as i32,
                        btc_block_height: tx.block_height as i64,
                        btc_block_hash: tx.block_hash.clone(),
                        btc_block_time: tx.block_time,
                        miner_address: miner.to_string(),
                        keystone_block: i64::from(k.l2_block),
                        l1_block: i64::from(k.l1_block),
                        ep_hash: hex::encode(k.ep_hash),
                        raw_keystone: hex::encode(k.to_bytes()),
                    })
                    .await?;
                    found += 1;
                }
            }
            if oldest < floor {
                break;
            }
            if pages % 20 == 0 {
                info!(miner, pages, oldest_height = oldest, floor, "PoP backfill progress");
            }
            sleep(self.page_delay).await;
        }
        repo.set_miner_synced_height(miner, tip).await?;
        if found > 0 {
            info!(miner, pages, publications = found, synced_to = tip, "PoP miner synced");
        }
        Ok(())
    }

    async fn validate(&self) -> Result<()> {
        let repo = PopRepo::new(&self.db);
        let pending = repo.unvalidated(VALIDATE_BATCH).await?;
        if pending.is_empty() {
            return Ok(());
        }
        // One Hemi block fetch per keystone, however many times it was published.
        let mut by_keystone: BTreeMap<i64, Vec<_>> = BTreeMap::new();
        for p in pending {
            by_keystone.entry(p.keystone_block).or_default().push(p);
        }
        let (mut ok, mut bad) = (0usize, 0usize);
        for (keystone_block, pubs) in by_keystone {
            let block = match self.hemi_provider.get_block_by_number((keystone_block as u64).into(), false).await {
                Ok(Some(b)) => b,
                Ok(None) => {
                    debug!(keystone_block, "Hemi block not available yet — validation deferred");
                    continue;
                }
                Err(e) => {
                    warn!(keystone_block, error = %e, "Hemi block fetch failed — validation deferred");
                    continue;
                }
            };
            let hash: [u8; 32] = block.header.hash.0;
            let parent: [u8; 32] = block.header.parent_hash.0;
            let root: [u8; 32] = block.header.state_root.0;
            for p in pubs {
                let valid = hex::decode(&p.raw_keystone)
                    .ok()
                    .and_then(|b| KeystoneAbrev::from_slice(&b))
                    .is_some_and(|k| k.matches_hemi_block(&hash, &parent, &root));
                if valid {
                    ok += 1;
                } else {
                    warn!(txid = %p.btc_txid, keystone_block, "PoP publication does not match the canonical Hemi block — ignored");
                    bad += 1;
                }
                repo.set_valid(&p.btc_txid, p.vout, valid).await?;
            }
        }
        debug!(valid = ok, invalid = bad, "PoP publications validated");
        Ok(())
    }

    async fn anchor(&self) -> Result<()> {
        let ids = PopRepo::new(&self.db).anchor_transfers(self.min_confirmations).await?;
        if ids.is_empty() {
            return Ok(());
        }
        info!(count = ids.len(), "Transfers marked Bitcoin-final from PoP publications");
        let transfers = TunnelTransferRepo::new(&self.db);
        for id in ids {
            match transfers.get(id).await {
                Ok(Some(row)) => {
                    if let Err(e) = dispatcher::enqueue(&self.db, event::POP_ANCHORED, &row).await {
                        warn!(transfer = %id, error = %e, "pop_anchored webhook enqueue failed");
                    }
                }
                Ok(None) => {}
                Err(e) => warn!(transfer = %id, error = %e, "reload after anchoring failed"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! End-to-end against real mainnet data (mempool.space + rpc.hemi.network)
    //! and a disposable Postgres:
    //! `POP_TEST_DATABASE_URL=postgres://… cargo test -p strait-node pop_scanner -- --ignored --nocapture`
    use super::*;
    use strait_api::pop::{lookup, PopFinalityStatus};
    use uuid::Uuid;

    const MINER: &str = "17Fv7httdZvqMubbvMkQ9KLPiTSPobxAqF";

    async fn insert_transfer(db: &Database, route: &str, hemi_block: i64, hemi_time: chrono::DateTime<chrono::Utc>) -> Uuid {
        let id = Uuid::new_v4();
        let deposit = route.ends_with("TO_HEMI");
        let (src_chain, src_block, dest_chain, dest_block, finalized) = if deposit {
            ("ETHEREUM", 1i64, Some("HEMI"), Some(hemi_block), Some(hemi_time))
        } else {
            ("HEMI", hemi_block, None, None, None)
        };
        sqlx::query(
            "INSERT INTO tunnel_transfers
               (id, asset, direction, route, amount, sender, recipient, status,
                source_chain, source_tx_hash, source_block, source_timestamp,
                dest_chain, dest_block, initiated_at, finalized_at)
             VALUES ($1,'ETH',$2,$3,1,'0xa','0xb',$4,$5,'0xtx',$6,$7,$8,$9,$7,$10)",
        )
        .bind(id)
        .bind(if deposit { "IN" } else { "OUT" })
        .bind(route)
        .bind(if deposit { "FINALIZED" } else { "INITIATED" })
        .bind(src_chain)
        .bind(src_block)
        .bind(hemi_time)
        .bind(dest_chain)
        .bind(dest_block)
        .bind(finalized)
        .execute(db.pool())
        .await
        .unwrap();
        id
    }

    #[tokio::test]
    #[ignore]
    async fn syncs_validates_and_anchors_real_mainnet_publications() {
        let url = std::env::var("POP_TEST_DATABASE_URL").expect("POP_TEST_DATABASE_URL");
        let db = strait_store::init_database(&url).await.unwrap();
        sqlx::query("TRUNCATE pop_sync_state, pop_miners, pop_publications, tunnel_transfers CASCADE")
            .execute(db.pool())
            .await
            .unwrap();

        let utc = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap().and_utc();
        // Real Hemi block times (rpc.hemi.network).
        let a = insert_transfer(&db, "HEMI_TO_ETH", 5_757_610, utc("2026-11-17 23:27:11")).await;
        let d = insert_transfer(&db, "ETH_TO_HEMI", 5_757_700, utc("2026-11-17 23:45:11")).await;
        // Before the start height (+2h margin): must not be anchored.
        let c = insert_transfer(&db, "HEMI_TO_ETH", 5_757_300, utc("2026-11-17 20:00:00")).await;

        let scanner = PopScanner::new(
            EsploraClient::new("https://mempool.space/api").unwrap(),
            crate::build_provider("https://rpc.hemi.network/rpc").unwrap(),
            db.clone(),
            vec![MINER.to_string()],
            // Mined ~20:3x UTC: A's Hemi block (23:27) clears the 2h start margin.
            976_318,
            6,
            250,
        );
        scanner.init().await.unwrap();
        // Public Esplora drops the odd request; the real loop retries, so do we.
        let mut ok = false;
        for _ in 0..5 {
            match scanner.tick().await {
                Ok(()) => {
                    ok = true;
                    break;
                }
                Err(e) => println!("tick error (retrying): {e}"),
            }
        }
        assert!(ok, "no successful tick");
        // A second tick is the steady state: one page per miner.
        scanner.tick().await.unwrap();

        let state = PopRepo::new(&db).state().await.unwrap().unwrap();
        println!("sync state: {state:?}");
        assert_eq!(state.synced_height, state.tip_height);

        let repo = TunnelTransferRepo::new(&db);
        let ra = repo.get(a).await.unwrap().unwrap();
        let rd = repo.get(d).await.unwrap().unwrap();
        let rc = repo.get(c).await.unwrap().unwrap();
        println!("A: anchored={} keystone={:?} btc={:?} {:?}", ra.pop_anchored, ra.pop_keystone_block, ra.pop_btc_block, ra.pop_btc_txid);
        println!("D: anchored={} keystone={:?} btc={:?} {:?}", rd.pop_anchored, rd.pop_keystone_block, rd.pop_btc_block, rd.pop_btc_txid);
        println!("C: anchored={}", rc.pop_anchored);

        // Same answers the full block scan produced for these transfers.
        assert!(ra.pop_anchored);
        assert_eq!(ra.pop_keystone_block, Some(5_757_625));
        assert_eq!(ra.pop_btc_block, Some(976_349));
        assert_eq!(ra.pop_btc_txid.as_deref(), Some("4202e43a66a2e5aeea2be00fffcf09d53274b91df3a1e8ec2d68d64033c3031f"));
        assert!(rd.pop_anchored);
        assert_eq!(rd.pop_keystone_block, Some(5_757_700));
        assert_eq!(rd.pop_btc_txid.as_deref(), Some("06a408ff4007cd3c89b5f65f1e07222d0f8065c1053cfcbad8751d964b254bd5"));
        assert!(!rc.pop_anchored, "transfer before the start height must not be anchored");

        let (invalid, pending, total): (i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE valid = FALSE), COUNT(*) FILTER (WHERE valid IS NULL), COUNT(*)
             FROM pop_publications",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        println!("publications total={total} invalid={invalid} unvalidated={pending}");
        assert_eq!(pending, 0);
        assert!(total > 100);

        // The raw query answers for A's block…
        let f = PopRepo::new(&db).finality_for_hemi_block(5_757_610).await.unwrap().unwrap();
        assert_eq!((f.keystone_block, f.btc_block_height), (5_757_625, 976_349));
        // …but the public lookup withholds it: with under a day synced, every
        // keystone seen is inside the warm-up window, so earlier publications
        // can't be ruled out.
        assert_eq!(lookup(&db, 5_757_610).await.unwrap().status, PopFinalityStatus::NotCovered);
        assert_eq!(lookup(&db, 100).await.unwrap().status, PopFinalityStatus::NotCovered);
        assert_eq!(lookup(&db, 99_999_999).await.unwrap().status, PopFinalityStatus::Pending);
    }
}
