//! PoP scanner: reads Hemi keystone publications straight from Bitcoin and marks
//! transfers Bitcoin-final, independent of the PoPPayoutsV2 reward contract.
//!
//! Each tick:
//!  1. **Reorg check**: compare the last few scanned block hashes with Esplora;
//!     on a mismatch, drop scanned blocks from the fork point (publications cascade).
//!  2. **Scan**: walk Bitcoin forward from the last scanned height (or
//!     `POP_SCAN_START_HEIGHT`), decoding `OP_RETURN "HEMI" + keystone` outputs.
//!     Heights stay contiguous, which the finality query relies on.
//!  3. **Validate**: check each new publication against the canonical Hemi block
//!     it names (block hash / parent hash / state root prefixes). Forged or
//!     non-canonical publications are marked invalid and never anchor anything.
//!  4. **Anchor**: mark transfers whose Hemi block is covered by a publication
//!     with enough confirmations, and enqueue `transfer.pop_anchored` webhooks.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use alloy::providers::Provider;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use strait_api::webhooks::dispatcher::{self, event};
use strait_bitcoin::pop::{scan_raw_block, EsploraClient, KeystoneAbrev};
use strait_core::error::{Result, StraitError};
use strait_store::{Database, NewPublication, NewScannedBlock, PopRepo, TunnelTransferRepo};

/// Blocks scanned per tick while catching up; the tick then validates and
/// anchors, so a long backfill makes visible progress as it goes.
const SCAN_BATCH: u64 = 30;
/// Recent scanned blocks re-checked against Esplora for a Bitcoin reorg.
const REORG_DEPTH: i64 = 6;
/// Publications validated per tick.
const VALIDATE_BATCH: i64 = 500;
/// Sleep between ticks once caught up with the Bitcoin tip (~1 block / 10 min).
const IDLE_INTERVAL: Duration = Duration::from_secs(60);

pub struct PopScanner {
    esplora: EsploraClient,
    hemi_provider: Arc<dyn Provider>,
    db: Database,
    start_height: u64,
    min_confirmations: i64,
    delay: Duration,
}

impl PopScanner {
    pub fn new(
        esplora: EsploraClient,
        hemi_provider: Arc<dyn Provider>,
        db: Database,
        start_height: u64,
        min_confirmations: u32,
        delay_ms: u64,
    ) -> Self {
        Self {
            esplora,
            hemi_provider,
            db,
            start_height,
            min_confirmations: i64::from(min_confirmations.max(1)),
            delay: Duration::from_millis(delay_ms),
        }
    }

    pub async fn run(self) -> Result<()> {
        info!(
            start_height = self.start_height,
            min_confirmations = self.min_confirmations,
            "PoP scanner started (Bitcoin keystone publications)"
        );
        loop {
            let caught_up = match self.tick().await {
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, "PoP scanner tick failed");
                    true // back off as if idle
                }
            };
            if caught_up {
                sleep(IDLE_INTERVAL).await;
            }
        }
    }

    /// One pass. Returns true once the scan has reached the Bitcoin tip.
    async fn tick(&self) -> Result<bool> {
        let tip = self.esplora.tip_height().await?;
        self.check_reorg().await?;
        let caught_up = self.scan_batch(tip).await?;
        self.validate().await?;
        self.anchor().await?;
        Ok(caught_up)
    }

    async fn check_reorg(&self) -> Result<()> {
        let repo = PopRepo::new(&self.db);
        let mut fork_at: Option<i64> = None;
        for (height, hash) in repo.recent_scanned(REORG_DEPTH).await? {
            let remote = self.esplora.block_hash(height as u64).await?;
            if remote == hash {
                break; // everything at and below this height is still canonical
            }
            fork_at = Some(height);
        }
        if let Some(h) = fork_at {
            let removed = repo.delete_from(h).await?;
            warn!(from_height = h, removed, "Bitcoin reorg — dropped scanned PoP blocks for rescan");
        }
        Ok(())
    }

    async fn scan_batch(&self, tip: u64) -> Result<bool> {
        let repo = PopRepo::new(&self.db);
        let next = match repo.scanned_tip().await? {
            Some((h, _)) => h as u64 + 1,
            None => self.start_height,
        };
        if next > tip {
            return Ok(true);
        }
        let end = tip.min(next + SCAN_BATCH - 1);
        for height in next..=end {
            let hash = self.esplora.block_hash(height).await?;
            let raw = self.esplora.raw_block(&hash).await?;
            let block = scan_raw_block(&raw)?;
            if block.hash != hash {
                return Err(StraitError::Parse(format!(
                    "Bitcoin block {height}: decoded hash {} != expected {hash}",
                    block.hash
                )));
            }
            let publications: Vec<NewPublication> = block
                .publications
                .iter()
                .map(|p| NewPublication {
                    btc_txid: p.txid.clone(),
                    vout: p.vout as i32,
                    keystone_block: i64::from(p.keystone.l2_block),
                    l1_block: i64::from(p.keystone.l1_block),
                    ep_hash: hex::encode(p.keystone.ep_hash),
                    raw_keystone: hex::encode(p.keystone.to_bytes()),
                })
                .collect();
            if !publications.is_empty() {
                let lo = publications.iter().map(|p| p.keystone_block).min().unwrap_or(0);
                let hi = publications.iter().map(|p| p.keystone_block).max().unwrap_or(0);
                info!(height, count = publications.len(), keystones = %format!("{lo}..={hi}"), "PoP publications found on Bitcoin");
            } else if height % 100 == 0 {
                info!(height, tip, "PoP scan progress");
            }
            repo.insert_block(&NewScannedBlock {
                height: height as i64,
                hash,
                time: block.time,
                publications,
            })
            .await?;
            sleep(self.delay).await;
        }
        Ok(end >= tip)
    }

    async fn validate(&self) -> Result<()> {
        let repo = PopRepo::new(&self.db);
        let pending = repo.unvalidated(VALIDATE_BATCH).await?;
        if pending.is_empty() {
            return Ok(());
        }
        // One Hemi block fetch per keystone, however many miners published it.
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
                if !valid {
                    warn!(txid = %p.btc_txid, keystone_block, "PoP publication does not match the canonical Hemi block — ignored");
                    bad += 1;
                } else {
                    ok += 1;
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
    async fn scans_validates_and_anchors_real_mainnet_publications() {
        let url = std::env::var("POP_TEST_DATABASE_URL").expect("POP_TEST_DATABASE_URL");
        let db = strait_store::init_database(&url).await.unwrap();
        sqlx::query("TRUNCATE pop_scanned_blocks, pop_publications, tunnel_transfers CASCADE")
            .execute(db.pool())
            .await
            .unwrap();

        let utc = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap().and_utc();
        // Real Hemi block times (rpc.hemi.network).
        let a = insert_transfer(&db, "HEMI_TO_ETH", 5_757_610, utc("2026-11-17 23:27:11")).await;
        let d = insert_transfer(&db, "ETH_TO_HEMI", 5_757_700, utc("2026-11-17 23:45:11")).await;
        // Before the scan window (+2h margin): must not be anchored.
        let c = insert_transfer(&db, "HEMI_TO_ETH", 5_757_300, utc("2026-11-17 20:00:00")).await;

        let scanner = PopScanner::new(
            EsploraClient::new("https://mempool.space/api").unwrap(),
            crate::build_provider("https://rpc.hemi.network/rpc").unwrap(),
            db.clone(),
            // Mined ~20:3x UTC: A's Hemi block (23:27) clears the 2h start margin.
            976_318,
            6,
            150,
        );
        // Public Esplora drops the odd request; the real loop retries, so do we.
        let mut caught_up = false;
        for _ in 0..20 {
            match scanner.tick().await {
                Ok(true) => {
                    caught_up = true;
                    break;
                }
                Ok(false) => {}
                Err(e) => println!("tick error (retrying): {e}"),
            }
        }
        assert!(caught_up, "scanner never reached the Bitcoin tip");
        // A final pass validates and anchors whatever the last scan added.
        scanner.tick().await.unwrap();

        let repo = TunnelTransferRepo::new(&db);
        let ra = repo.get(a).await.unwrap().unwrap();
        let rd = repo.get(d).await.unwrap().unwrap();
        let rc = repo.get(c).await.unwrap().unwrap();
        println!("A: anchored={} keystone={:?} btc={:?} {:?}", ra.pop_anchored, ra.pop_keystone_block, ra.pop_btc_block, ra.pop_btc_txid);
        println!("D: anchored={} keystone={:?} btc={:?} {:?}", rd.pop_anchored, rd.pop_keystone_block, rd.pop_btc_block, rd.pop_btc_txid);
        println!("C: anchored={}", rc.pop_anchored);

        assert!(ra.pop_anchored);
        assert_eq!(ra.pop_keystone_block, Some(5_757_625));
        assert_eq!(ra.pop_btc_block, Some(976_349));
        assert!(rd.pop_anchored);
        assert_eq!(rd.pop_keystone_block, Some(5_757_700));
        assert_eq!(rd.pop_btc_txid.as_deref(), Some("06a408ff4007cd3c89b5f65f1e07222d0f8065c1053cfcbad8751d964b254bd5"));
        assert!(!rc.pop_anchored, "transfer before the scan window must not be anchored");

        let invalid: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pop_publications WHERE valid = FALSE")
            .fetch_one(db.pool()).await.unwrap();
        let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pop_publications WHERE valid IS NULL")
            .fetch_one(db.pool()).await.unwrap();
        println!("publications invalid={invalid} unvalidated={pending}");
        assert_eq!(pending, 0);

        // The raw query answers for A's block…
        let f = PopRepo::new(&db).finality_for_hemi_block(5_757_610).await.unwrap().unwrap();
        assert_eq!((f.keystone_block, f.btc_block_height), (5_757_625, 976_349));
        // …but the public lookup withholds it: with under a day scanned, every
        // keystone seen is inside the warm-up window, so earlier publications
        // can't be ruled out.
        let l = lookup(&db, 5_757_610).await.unwrap();
        println!("lookup 5757610: {l:?}");
        assert_eq!(l.status, PopFinalityStatus::NotCovered);
        assert_eq!(lookup(&db, 100).await.unwrap().status, PopFinalityStatus::NotCovered);
        assert_eq!(lookup(&db, 99_999_999).await.unwrap().status, PopFinalityStatus::Pending);
    }
}
