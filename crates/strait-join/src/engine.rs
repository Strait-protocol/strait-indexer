//! Join engine — consumes raw events from all three chain ingesters and
//! produces TunnelTransfer lifecycle updates.
//!
//! # Keystone anchoring (BTC routes)
//!
//! BTC→Hemi transfers advance from INITIATED → ANCHORED when a
//! `PopKeystoneAnchored` event covers the Hemi mint block.
//!
//! `PopAnchor::covers_block(mint_block)` is the authoritative check:
//!   window = (keystone_block - 25, keystone_block]  (exclusive lower, inclusive upper)
//!
//! When `PayoutRoundExecuted(blockRewarded)` fires on `PoPPayoutsV2`, the engine
//! fans out to all in-flight transfers and advances those whose mint block is covered.
//! ETH→Hemi uses OP Stack finality — no PoP wait required.

use chrono::Utc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use strait_core::{
    config::KEYSTONE_FREQUENCY,
    error::{Result, StraitError},
    events::{BitcoinEvent, EthereumEvent, HemiEvent, RawEvent},
    types::{
        Address, Asset, BitcoinTxid, BlockHash, Chain, ChainAddress, ChainTransaction, ChainTxHash,
        PopAnchor, ReorgEvent, TunnelDirection, TunnelRoute, TunnelStatus, TunnelTransfer, TxHash,
    },
};
use strait_store::{Database, TunnelTransferRepo};

use crate::{matcher::EventMatcher, state::TransferState};

/// Keystone frequency — must match PoPPayoutsV2.KEYSTONE_FREQUENCY = 25.
const KEYSTONE_FREQ: u64 = KEYSTONE_FREQUENCY as u64;

/// Compute the keystone block that covers Hemi block `n`.
/// Returns the smallest multiple of 25 that is >= n.
pub fn keystone_for(block: u64) -> u64 {
    let rem = block % KEYSTONE_FREQ;
    if rem == 0 { block } else { block + (KEYSTONE_FREQ - rem) }
}

/// An update emitted by the engine to the store layer.
#[derive(Debug, Clone)]
pub enum TunnelTransferUpdate {
    Created(strait_core::types::TunnelTransfer),
    StatusChanged {
        id: Uuid,
        new_status: TunnelStatus,
        updated_at: chrono::DateTime<Utc>,
    },
    Retracted {
        id: Uuid,
        reason: String,
        retracted_at: chrono::DateTime<Utc>,
    },
    DestinationConfirmed {
        id: Uuid,
        destination_tx: ChainTransaction,
    },
    /// A PoP keystone anchored this transfer to Bitcoin.
    /// The transfer's Hemi mint block fell within the keystone window.
    PopAnchored {
        id: Uuid,
        keystone_block: u64,
        pop_score: u64,
        anchored_at: chrono::DateTime<Utc>,
    },
}

/// Consumes raw events from all chain ingesters and produces
/// `TunnelTransferUpdate` records for the store layer.
pub struct JoinEngine {
    event_rx: mpsc::Receiver<RawEvent>,
    update_tx: mpsc::Sender<TunnelTransferUpdate>,
    state: TransferState,
    matcher: EventMatcher,
    /// Most recent keystone block confirmed as PoP-anchored.
    last_anchored_keystone: u64,
    /// DB handle for the keystone anchoring fallback — picks up BTC→Hemi
    /// deposits that were INITIATED in a prior session (not in in-memory state).
    /// `None` in unit tests (no real Postgres available).
    db: Option<Database>,
}

impl JoinEngine {
    pub fn new(
        event_rx: mpsc::Receiver<RawEvent>,
        update_tx: mpsc::Sender<TunnelTransferUpdate>,
        db: Database,
    ) -> Self {
        Self {
            event_rx,
            update_tx,
            state: TransferState::new(),
            matcher: EventMatcher::new(Default::default()),
            last_anchored_keystone: 0,
            db: Some(db),
        }
    }

    /// Test-only constructor — skips the DB-backed keystone anchoring fallback.
    #[cfg(test)]
    fn new_for_test(
        event_rx: mpsc::Receiver<RawEvent>,
        update_tx: mpsc::Sender<TunnelTransferUpdate>,
    ) -> Self {
        Self {
            event_rx,
            update_tx,
            state: TransferState::new(),
            matcher: EventMatcher::new(Default::default()),
            last_anchored_keystone: 0,
            db: None,
        }
    }

    /// Run the engine forever, consuming events until the channel closes.
    pub async fn run(mut self) -> Result<()> {
        info!("Join engine started");
        while let Some(event) = self.event_rx.recv().await {
            if let Err(e) = self.process(event).await {
                warn!("Join engine error: {}", e);
            }
        }
        info!("Join engine shutting down — event channel closed");
        Ok(())
    }

    async fn process(&mut self, event: RawEvent) -> Result<()> {
        match &event {
            // Reorgs — handle before matching so we retract before processing new events
            RawEvent::Bitcoin(BitcoinEvent::BlockReorg { affected_from_block, old_tip, new_tip, depth }) => {
                let re = ReorgEvent {
                    chain: Chain::Bitcoin,
                    depth: *depth,
                    old_tip: *old_tip,
                    new_tip: *new_tip,
                    affected_from_block: *affected_from_block,
                    detected_at: Utc::now(),
                };
                self.handle_reorg(Chain::Bitcoin, *affected_from_block, re).await?;
            }
            RawEvent::Hemi(HemiEvent::BlockReorg { affected_from_block, old_tip, new_tip, depth }) => {
                let re = ReorgEvent {
                    chain: Chain::Hemi,
                    depth: *depth,
                    old_tip: *old_tip,
                    new_tip: *new_tip,
                    affected_from_block: *affected_from_block,
                    detected_at: Utc::now(),
                };
                self.handle_reorg(Chain::Hemi, *affected_from_block, re).await?;
            }
            RawEvent::Ethereum(EthereumEvent::BlockReorg { affected_from_block, old_tip, new_tip, depth }) => {
                let re = ReorgEvent {
                    chain: Chain::Ethereum,
                    depth: *depth,
                    old_tip: *old_tip,
                    new_tip: *new_tip,
                    affected_from_block: *affected_from_block,
                    detected_at: Utc::now(),
                };
                self.handle_reorg(Chain::Ethereum, *affected_from_block, re).await?;
            }

            // PoP keystone anchoring
            RawEvent::Hemi(HemiEvent::PopKeystoneAnchored { keystone_block, pop_score, .. }) => {
                self.handle_keystone_anchored(*keystone_block, *pop_score).await?;
            }

            // Terminal Hemi tunnel events are self-contained transfer records — a
            // mint carries its source Bitcoin txid, a burn carries its destination.
            // Materialize a TunnelTransfer directly so it is persisted and becomes
            // eligible for PoP keystone anchoring, without waiting on the source-side
            // ingester. The matcher (below) handles cross-chain enrichment once
            // BitcoinEvent / EthereumEvent source legs are produced.
            RawEvent::Hemi(HemiEvent::TunnelMint { .. } | HemiEvent::TunnelBurn { .. }) => {
                // Only ETH-route legs need the matcher (BTC routes converge via their
                // txid on the direct path). Feeding BTC mints here would leave them
                // pending forever, so gate on the ETH-route shape.
                let eth_route = matches!(
                    &event,
                    RawEvent::Hemi(HemiEvent::TunnelMint { source_txid: None, .. })
                        | RawEvent::Hemi(HemiEvent::TunnelBurn {
                            destination: ChainAddress::Evm(_),
                            ..
                        })
                );
                if let RawEvent::Hemi(hemi) = &event {
                    if let Some(transfer) = transfer_from_hemi_event(hemi) {
                        self.register_transfer(transfer).await?;
                    }
                }
                // Feed the matcher so a later Ethereum L1 leg enriches this transfer.
                if eth_route {
                    if let Some(m) = self.matcher.process_event(event) {
                        self.handle_match(m).await?;
                    }
                }
            }

            // A Bitcoin custody deposit. For BTC routes the transfer id is derived
            // from the Bitcoin txid, so this and the later Hemi `DepositConfirmed`
            // mint converge on the same row — the deposit creates it early (with the
            // real Bitcoin block), the mint enriches it with the Hemi destination.
            RawEvent::Bitcoin(BitcoinEvent::TunnelDeposit { .. }) => {
                if let RawEvent::Bitcoin(btc) = &event {
                    if let Some(transfer) = transfer_from_btc_deposit(btc) {
                        self.register_transfer(transfer).await?;
                    }
                }
            }

            // Cross-chain matching (Ethereum source legs / other Bitcoin events)
            _ => {
                if let Some(m) = self.matcher.process_event(event) {
                    self.handle_match(m).await?;
                }
            }
        }
        Ok(())
    }

    // ── Keystone anchoring ────────────────────────────────────────────────────

    /// Fan out a `PopKeystoneAnchored` event to all in-flight transfers.
    ///
    /// Uses `PopAnchor::covers_block` as the single authoritative check —
    /// window is (keystone_block - 25, keystone_block] (exclusive, inclusive).
    async fn handle_keystone_anchored(&mut self, keystone_block: u64, pop_score: u64) -> Result<()> {
        if keystone_block <= self.last_anchored_keystone {
            debug!(keystone_block, "Duplicate or out-of-order keystone, skipping");
            return Ok(());
        }
        self.last_anchored_keystone = keystone_block;

        let anchor = PopAnchor {
            keystone_block,
            pop_score,
            reward_pool: 0,
            observed_at: Utc::now(),
        };

        info!(
            keystone_block,
            pop_score,
            window = format!("({}, {}]", keystone_block.saturating_sub(PopAnchor::KEYSTONE_FREQUENCY), keystone_block),
            "PopKeystoneAnchored — checking in-flight transfers"
        );

        // Collect transfers whose Hemi mint block is covered by this keystone.
        let to_anchor: Vec<Uuid> = self.state
            .transfers_initiated_with_hemi_mint()
            .into_iter()
            .filter(|(_, mint_block)| anchor.covers_block(*mint_block))
            .map(|(id, _)| id)
            .collect();

        let anchored_at = Utc::now();

        // In-memory fan-out — covers transfers seen in this session.
        let mut anchored_in_memory: std::collections::HashSet<uuid::Uuid> =
            std::collections::HashSet::with_capacity(to_anchor.len());
        for id in to_anchor {
            self.state.anchor(&id).map_err(|e| StraitError::Internal(e.to_string()))?;
            self.state.set_pop_anchor(&id, keystone_block, pop_score, anchored_at);

            self.emit(TunnelTransferUpdate::PopAnchored {
                id,
                keystone_block,
                pop_score,
                anchored_at,
            }).await?;
            self.emit(TunnelTransferUpdate::StatusChanged {
                id,
                new_status: TunnelStatus::Anchored,
                updated_at: anchored_at,
            }).await?;

            info!(transfer_id = %id, keystone_block, "Transfer INITIATED → ANCHORED via PoP keystone");
            anchored_in_memory.insert(id);
        }

        // DB-backed fallback: after a node restart the in-memory state is empty, so
        // BTC→Hemi deposits that were INITIATED in a prior session are invisible to
        // the in-memory fan-out above. Query the DB for deposits in this keystone's
        // window and anchor any that weren't already handled.
        let Some(ref db) = self.db else { return Ok(()); };
        let repo = TunnelTransferRepo::new(db);
        match repo.list_initiated_btc_deposits_in_keystone_window(keystone_block as i64).await {
            Ok(rows) => {
                let db_count = rows.len();
                for row in rows {
                    if anchored_in_memory.contains(&row.id) {
                        continue; // already handled by in-memory path
                    }
                    self.emit(TunnelTransferUpdate::PopAnchored {
                        id: row.id,
                        keystone_block,
                        pop_score,
                        anchored_at,
                    }).await?;
                    self.emit(TunnelTransferUpdate::StatusChanged {
                        id: row.id,
                        new_status: TunnelStatus::Anchored,
                        updated_at: anchored_at,
                    }).await?;
                    info!(
                        transfer_id = %row.id,
                        keystone_block,
                        "BTC→Hemi deposit INITIATED → ANCHORED via DB-backed keystone fallback"
                    );
                }
                if db_count > 0 {
                    debug!(keystone_block, count = db_count, "DB-backed keystone fallback checked");
                }
            }
            Err(e) => warn!(
                error = %e,
                keystone_block,
                "DB-backed keystone anchoring fallback failed — in-session transfers were still anchored"
            ),
        }

        Ok(())
    }

    // ── Match handling ────────────────────────────────────────────────────────

    async fn handle_match(&mut self, m: crate::matcher::MatchResult) -> Result<()> {
        debug!(direction = ?m.direction, amount = m.amount, "Cross-chain match found");

        // The Hemi leg is the authoritative transfer record (deduplicated by
        // deterministic id in `register_transfer`); the matched Ethereum leg enriches
        // it with the real L1 source (deposits) or release (withdrawals).
        let hemi = [&m.source, &m.destination].into_iter().find_map(|e| match e {
            RawEvent::Hemi(h) => Some(h),
            _ => None,
        });
        let eth = [&m.source, &m.destination].into_iter().find_map(|e| match e {
            RawEvent::Ethereum(x) => Some(x),
            _ => None,
        });

        let Some(hemi) = hemi else { return Ok(()); };
        let Some(mut transfer) = transfer_from_hemi_event(hemi) else { return Ok(()); };
        if let Some(eth) = eth {
            enrich_with_eth_leg(&mut transfer, eth);
        }
        self.register_transfer(transfer).await
    }

    /// Insert (or refresh) a transfer in state and emit a `Created` update for the
    /// store layer. Idempotent: the transfer id is derived deterministically from the
    /// Hemi tx, so re-delivery upserts the same row rather than duplicating it.
    async fn register_transfer(&mut self, transfer: TunnelTransfer) -> Result<()> {
        let id = transfer.id;
        let route = transfer.route;
        let is_new = self.state.get(&id).is_none();
        self.state.insert(transfer.clone());
        self.emit(TunnelTransferUpdate::Created(transfer)).await?;
        if is_new {
            info!(transfer_id = %id, %route, "transfer created from Hemi tunnel event");
        } else {
            debug!(transfer_id = %id, %route, "transfer re-observed, refreshed");
        }
        Ok(())
    }

    // ── Reorg handling ────────────────────────────────────────────────────────

    async fn handle_reorg(
        &mut self,
        chain: Chain,
        affected_from_block: u64,
        reorg_event: ReorgEvent,
    ) -> Result<()> {
        warn!(chain = %chain, affected_from_block, "Reorg — retracting affected transfers");

        self.state.handle_reorg(&chain, affected_from_block, reorg_event);

        // Emit retraction for any transfer now marked REORGED
        let retracted: Vec<Uuid> = self.state
            .transfers_by_status_discriminant(std::mem::discriminant(
                &TunnelStatus::Reorged { retracted_at: Utc::now() }
            ))
            .into_iter()
            .map(|t| t.id)
            .collect();

        for id in retracted {
            self.emit(TunnelTransferUpdate::Retracted {
                id,
                reason: format!("Reorg on {chain} from block {affected_from_block}"),
                retracted_at: Utc::now(),
            }).await?;
        }

        Ok(())
    }

    async fn emit(&self, update: TunnelTransferUpdate) -> Result<()> {
        self.update_tx.send(update).await
            .map_err(|e| StraitError::Internal(format!("Failed to emit update: {}", e)))
    }
}

/// Derive a stable transfer id from the Hemi tx hash and log index, so the same
/// on-chain event always maps to the same transfer row (idempotent persistence).
fn deterministic_id(tx_hash: &TxHash, log_index: u32) -> uuid::Uuid {
    let name = format!("{tx_hash}-{log_index}");
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, name.as_bytes())
}

/// Derive a stable transfer id from a Bitcoin txid. Used for BTC routes so the
/// Bitcoin custody deposit and the Hemi `DepositConfirmed` mint — which both know
/// the same txid — produce the same transfer row.
fn btc_transfer_id(txid: &BitcoinTxid) -> uuid::Uuid {
    let name = format!("btc:{txid}");
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, name.as_bytes())
}

/// Build a preliminary `TunnelTransfer` from a Bitcoin custody deposit (BTC → Hemi).
/// The Hemi destination tx is filled in later when the `DepositConfirmed` mint lands.
fn transfer_from_btc_deposit(event: &BitcoinEvent) -> Option<TunnelTransfer> {
    let BitcoinEvent::TunnelDeposit {
        txid,
        amount_sats,
        to_address,
        hemi_destination,
        block_number,
        block_time,
        ..
    } = event
    else {
        return None;
    };

    let now = *block_time;
    let recipient = match hemi_destination {
        Some(addr) => ChainAddress::Evm(Address(addr.0)),
        // Unparseable OP_RETURN — recipient unknown until the Hemi mint enriches it.
        None => ChainAddress::Bitcoin(to_address.clone()),
    };

    Some(TunnelTransfer {
        id: btc_transfer_id(txid),
        asset: Asset::Btc,
        direction: TunnelDirection::In,
        route: TunnelRoute::BtcToHemi,
        amount: bigdecimal::BigDecimal::from(*amount_sats),
        sender: ChainAddress::Bitcoin(strait_core::types::BitcoinAddress::new(format!(
            "btctx:{txid}"
        ))),
        recipient,
        status: TunnelStatus::Initiated,
        initiated_at: now,
        finalized_at: None,
        source_tx: ChainTransaction {
            chain: Chain::Bitcoin,
            hash: ChainTxHash::Bitcoin(*txid),
            block_number: *block_number,
            block_hash: BlockHash([0u8; 32]),
            timestamp: *block_time,
            confirmations: 0,
        },
        destination_tx: None,
        source_fee: None,
        dest_fee: None,
        withdrawal_uuid: None,
        pop_anchored: false,
        pop_keystone_block: None,
        pop_score: None,
        pop_anchored_at: None,
        reorg_events: vec![],
    })
}

/// Build a `TunnelTransfer` from a terminal Hemi tunnel event.
///
/// - `TunnelMint` → an inbound transfer (BTC_TO_HEMI / ETH_TO_HEMI). The Hemi mint
///   is recorded as the destination tx so the PoP keystone fan-out can anchor it.
/// - `TunnelBurn` → an outbound transfer (HEMI_TO_BTC / HEMI_TO_ETH). The Hemi burn
///   is the source tx; the destination payout is filled in later.
///
/// Returns `None` for non-tunnel Hemi events (reorg, keystone).
fn transfer_from_hemi_event(event: &HemiEvent) -> Option<TunnelTransfer> {
    match event {
        HemiEvent::TunnelMint {
            tx_hash,
            asset,
            amount,
            to,
            source_txid,
            block_number,
            block_time,
            log_index,
            gas_fee,
        } => {
            let now = *block_time;
            let route = if matches!(asset, Asset::Btc) {
                TunnelRoute::BtcToHemi
            } else {
                TunnelRoute::EthToHemi
            };

            // Source leg: known for BTC (the deposit txid); for ETH only the Hemi
            // finalization is observed today, so the source tx hash mirrors it until
            // an Ethereum-side ingester provides the real L1 lock.
            let (source_chain, source_hash, sender) = match source_txid {
                Some(txid) => (
                    Chain::Bitcoin,
                    ChainTxHash::Bitcoin(*txid),
                    ChainAddress::Bitcoin(strait_core::types::BitcoinAddress::new(format!(
                        "btctx:{txid}"
                    ))),
                ),
                None => (
                    Chain::Ethereum,
                    ChainTxHash::Evm(*tx_hash),
                    ChainAddress::Evm(Address(to.0)),
                ),
            };

            // BTC routes key on the Bitcoin txid so the deposit and the mint
            // converge; ETH routes key on the Hemi mint tx.
            let id = match source_txid {
                Some(txid) => btc_transfer_id(txid),
                None => deterministic_id(tx_hash, *log_index),
            };

            // An ETH/ERC-20 deposit finalizing on Hemi means the funds have arrived —
            // the transfer is complete. A BTC mint is only INITIATED until a PoP
            // keystone anchors it to Bitcoin.
            let (status, finalized_at) = match route {
                TunnelRoute::EthToHemi => (TunnelStatus::Finalized, Some(now)),
                _ => (TunnelStatus::Initiated, None),
            };

            Some(TunnelTransfer {
                id,
                asset: asset.clone(),
                direction: TunnelDirection::In,
                route,
                amount: amount.clone(),
                sender,
                recipient: ChainAddress::Evm(Address(to.0)),
                status,
                initiated_at: now,
                finalized_at,
                source_tx: ChainTransaction {
                    chain: source_chain,
                    hash: source_hash,
                    block_number: 0,
                    block_hash: BlockHash([0u8; 32]),
                    timestamp: now,
                    confirmations: 0,
                },
                destination_tx: Some(ChainTransaction {
                    chain: Chain::Hemi,
                    hash: ChainTxHash::Evm(*tx_hash),
                    block_number: *block_number,
                    block_hash: BlockHash([0u8; 32]),
                    timestamp: now,
                    confirmations: 0,
                }),
                source_fee: None,
                dest_fee: gas_fee.clone(),
                withdrawal_uuid: None,
                pop_anchored: false,
                pop_keystone_block: None,
                pop_score: None,
                pop_anchored_at: None,
                reorg_events: vec![],
            })
        }

        HemiEvent::TunnelBurn {
            tx_hash,
            asset,
            amount,
            from,
            destination,
            block_number,
            block_time,
            log_index,
            gas_fee,
            uuid,
        } => {
            let now = *block_time;
            let route = match destination {
                ChainAddress::Bitcoin(_) => TunnelRoute::HemiToBtc,
                ChainAddress::Evm(_) => TunnelRoute::HemiToEth,
            };

            Some(TunnelTransfer {
                id: deterministic_id(tx_hash, *log_index),
                asset: asset.clone(),
                direction: TunnelDirection::Out,
                route,
                amount: amount.clone(),
                sender: ChainAddress::Evm(Address(from.0)),
                recipient: destination.clone(),
                status: TunnelStatus::Initiated,
                initiated_at: now,
                finalized_at: None,
                source_tx: ChainTransaction {
                    chain: Chain::Hemi,
                    hash: ChainTxHash::Evm(*tx_hash),
                    block_number: *block_number,
                    block_hash: BlockHash([0u8; 32]),
                    timestamp: now,
                    confirmations: 0,
                },
                destination_tx: None,
                source_fee: gas_fee.clone(),
                dest_fee: None,
                withdrawal_uuid: *uuid,
                pop_anchored: false,
                pop_keystone_block: None,
                pop_score: None,
                pop_anchored_at: None,
                reorg_events: vec![],
            })
        }

        _ => None,
    }
}

/// Overlay a matched Ethereum L1 leg onto a Hemi-derived transfer.
///
/// - `TunnelLock` (ETH→Hemi): the real L1 deposit replaces the mirrored source leg.
/// - `TunnelRelease` (Hemi→ETH): the L1 release fills the destination leg and finalizes.
fn enrich_with_eth_leg(t: &mut TunnelTransfer, eth: &EthereumEvent) {
    match eth {
        EthereumEvent::TunnelLock { tx_hash, from, block_number, block_time, gas_fee, .. } => {
            t.source_tx = ChainTransaction {
                chain: Chain::Ethereum,
                hash: ChainTxHash::Evm(*tx_hash),
                block_number: *block_number,
                block_hash: BlockHash([0u8; 32]),
                timestamp: *block_time,
                confirmations: 0,
            };
            t.sender = ChainAddress::Evm(Address(from.0));
            t.source_fee = gas_fee.clone();
            // The L1 lock is the real initiation moment for an ETH→Hemi deposit.
            t.initiated_at = *block_time;
        }
        EthereumEvent::TunnelRelease { tx_hash, to, block_number, block_time, gas_fee, .. } => {
            t.destination_tx = Some(ChainTransaction {
                chain: Chain::Ethereum,
                hash: ChainTxHash::Evm(*tx_hash),
                block_number: *block_number,
                block_hash: BlockHash([0u8; 32]),
                timestamp: *block_time,
                confirmations: 0,
            });
            t.recipient = ChainAddress::Evm(Address(to.0));
            t.dest_fee = gas_fee.clone();
            t.status = TunnelStatus::Finalized;
            t.finalized_at = Some(*block_time);
        }
        EthereumEvent::BlockReorg { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keystone_for_exact_multiple() {
        assert_eq!(keystone_for(0), 0);
        assert_eq!(keystone_for(25), 25);
        assert_eq!(keystone_for(50), 50);
        assert_eq!(keystone_for(100), 100);
    }

    #[test]
    fn test_keystone_for_between_multiples() {
        assert_eq!(keystone_for(1), 25);
        assert_eq!(keystone_for(24), 25);
        assert_eq!(keystone_for(26), 50);
        assert_eq!(keystone_for(49), 50);
    }

    #[test]
    fn test_keystone_for_real_blocks() {
        // 12350 = 25 * 494 — it IS a keystone, so it covers itself
        assert_eq!(keystone_for(12350), 12350);
        // Block 12351 (one past a keystone) waits for the next keystone 12375
        assert_eq!(keystone_for(12351), 12375);
        // Block 12374 (one before a keystone) waits for keystone 12375
        assert_eq!(keystone_for(12374), 12375);
        // Block exactly on keystone 12375 is covered by it
        assert_eq!(keystone_for(12375), 12375);
        // Block 12376 waits for keystone 12400
        assert_eq!(keystone_for(12376), 12400);
    }

    #[test]
    fn test_keystone_coverage() {
        // keystone 12375 covers blocks (12350, 12375]
        let keystone = 12375u64;
        let prev = keystone - KEYSTONE_FREQ;
        assert!(12351 > prev && 12351 <= keystone); // covered
        assert!(12375 > prev && 12375 <= keystone); // covered (on keystone)
        assert!(!(12350 > prev && 12350 <= keystone)); // NOT covered (== prev)
        assert!(!(12376 > prev && 12376 <= keystone)); // NOT covered (next keystone)
    }

    // ── PopAnchor.covers_block boundary tests ─────────────────────────────────
    // These tests are the authoritative specification for which Hemi mint blocks
    // get anchored by a given keystone. If these pass, the logic is correct.

    fn make_anchor(keystone_block: u64) -> PopAnchor {
        PopAnchor {
            keystone_block,
            pop_score: 5000,
            reward_pool: 1_000_000,
            observed_at: Utc::now(),
        }
    }

    #[test]
    fn covers_block_window_boundaries() {
        // Keystone at 100, window is (75, 100]
        let anchor = make_anchor(100);
        let cases: &[(u64, bool)] = &[
            (74, false), // well before window
            (75, false), // exactly at window_start — NOT covered (exclusive lower bound)
            (76, true),  // one past window_start — covered
            (99, true),  // one before keystone — covered
            (100, true), // exactly on keystone_block — covered (inclusive upper bound)
            (101, false), // one past keystone_block — NOT covered
            (200, false), // well after window
        ];
        for (mint_block, expected) in cases {
            assert_eq!(
                anchor.covers_block(*mint_block),
                *expected,
                "keystone=100, mint_block={mint_block}: expected covers={expected}"
            );
        }
    }

    #[test]
    fn covers_block_keystone_at_frequency_boundary() {
        // Keystone at exactly 25 (first possible keystone), window is (0, 25]
        let anchor = make_anchor(25);
        assert!(!anchor.covers_block(0));  // saturating_sub(25) = 0, so 0 > 0 is false
        assert!(anchor.covers_block(1));
        assert!(anchor.covers_block(25));
        assert!(!anchor.covers_block(26));
    }

    #[test]
    fn covers_block_at_keystone_zero_saturates() {
        // Keystone at 0 (degenerate): window_start saturates to 0
        // covers_block(0) = 0 > 0 && 0 <= 0 = false — correctly excluded
        let anchor = make_anchor(0);
        assert!(!anchor.covers_block(0));
        assert!(!anchor.covers_block(1));
    }

    #[test]
    fn consecutive_keystones_cover_all_blocks_without_overlap() {
        // Blocks 1..=50 should be covered by exactly one of keystone 25 or keystone 50
        let k25 = make_anchor(25);
        let k50 = make_anchor(50);

        for block in 1u64..=50 {
            let in_25 = k25.covers_block(block);
            let in_50 = k50.covers_block(block);
            assert!(
                in_25 ^ in_50,
                "block {block} should be in exactly one keystone (25={in_25}, 50={in_50})"
            );
        }
    }

    // ── Real testnet event → transfer ─────────────────────────────────────────
    // Values captured from a live Hemi testnet ETHBridgeFinalized log:
    //   tx     0x2b320473e4f679f8d763bbb6aee617f8b42b1a9367e623ec1c5bc6e45eedc19a
    //   block  6037628, log_index 1, 0.13 ETH bridged into Hemi.
    // The deterministic id below was computed independently (uuid5 of NAMESPACE_OID
    // over "<tx>-<log_index>") — it must match what the engine derives.

    const REAL_TX: &str = "0x2b320473e4f679f8d763bbb6aee617f8b42b1a9367e623ec1c5bc6e45eedc19a";
    const REAL_TO: &str = "0x065213232a03b6ef9c51e0c3250096474bb515b8";
    const REAL_BLOCK: u64 = 6_037_628;
    const REAL_AMOUNT: u64 = 130_000_000_000_000_000; // 0.13 ETH in wei
    const REAL_ID: &str = "4ca93d2a-8b7c-567c-80ee-473de55b38e8";
    const REAL_KEYSTONE: u64 = 6_037_650; // keystone_for(6037628)

    fn real_mint_event() -> RawEvent {
        RawEvent::Hemi(HemiEvent::TunnelMint {
            tx_hash: TxHash::from_hex(REAL_TX).unwrap(),
            asset: strait_core::types::Asset::Eth,
            amount: bigdecimal::BigDecimal::from(REAL_AMOUNT),
            to: Address::from_hex(REAL_TO).unwrap(),
            source_txid: None,
            block_number: REAL_BLOCK,
            block_time: Utc::now(),
            log_index: 1,
            gas_fee: None,
        })
    }

    #[test]
    fn real_testnet_eth_bridge_event_builds_expected_transfer() {
        let RawEvent::Hemi(ev) = real_mint_event() else { unreachable!() };
        let t = transfer_from_hemi_event(&ev).expect("should build a transfer");

        assert_eq!(t.id, uuid::Uuid::parse_str(REAL_ID).unwrap());
        assert_eq!(t.route, TunnelRoute::EthToHemi);
        assert_eq!(t.direction, TunnelDirection::In);
        assert_eq!(t.amount, bigdecimal::BigDecimal::from(REAL_AMOUNT));
        assert_eq!(
            t.recipient,
            ChainAddress::Evm(Address::from_hex(REAL_TO).unwrap())
        );
        // An ETH/ERC-20 deposit finalizing on Hemi is complete.
        assert!(matches!(t.status, TunnelStatus::Finalized));
        assert!(t.finalized_at.is_some());
        let dest = t.destination_tx.as_ref().expect("mint records a Hemi dest tx");
        assert_eq!(dest.chain, Chain::Hemi);
        assert_eq!(dest.block_number, REAL_BLOCK);
    }

    #[tokio::test]
    async fn engine_creates_then_pop_anchors_real_testnet_event() {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(32);
        let (update_tx, mut update_rx) = tokio::sync::mpsc::channel(32);
        let handle = tokio::spawn(JoinEngine::new_for_test(event_rx, update_tx).run());

        // 1. A BTC→Hemi mint at the real testnet block → engine emits Created (INITIATED).
        //    PoP anchoring applies to BTC routes (ETH routes finalize immediately).
        event_tx
            .send(RawEvent::Hemi(HemiEvent::TunnelMint {
                tx_hash: TxHash::from_hex(REAL_TX).unwrap(),
                asset: strait_core::types::Asset::Btc,
                amount: bigdecimal::BigDecimal::from(REAL_AMOUNT),
                to: Address::from_hex(REAL_TO).unwrap(),
                source_txid: Some(BitcoinTxid([3u8; 32])),
                block_number: REAL_BLOCK,
                block_time: Utc::now(),
                log_index: 1,
                gas_fee: None,
            }))
            .await
            .unwrap();
        let id = match update_rx.recv().await.unwrap() {
            TunnelTransferUpdate::Created(t) => {
                assert_eq!(t.route, TunnelRoute::BtcToHemi);
                assert!(matches!(t.status, TunnelStatus::Initiated));
                t.id
            }
            other => panic!("expected Created, got {other:?}"),
        };

        // 2. The keystone covering block 6037628 fires → PoP anchored on Bitcoin.
        event_tx
            .send(RawEvent::Hemi(HemiEvent::PopKeystoneAnchored {
                hemi_tx_hash: TxHash([0u8; 32]),
                keystone_block: REAL_KEYSTONE,
                reward_pool: 0,
                pop_score: 4200,
                block_number: REAL_KEYSTONE + 1,
                log_index: 0,
            }))
            .await
            .unwrap();

        match update_rx.recv().await.unwrap() {
            TunnelTransferUpdate::PopAnchored { id: aid, keystone_block, pop_score, .. } => {
                assert_eq!(aid, id);
                assert_eq!(keystone_block, REAL_KEYSTONE);
                assert_eq!(pop_score, 4200);
            }
            other => panic!("expected PopAnchored, got {other:?}"),
        }
        match update_rx.recv().await.unwrap() {
            TunnelTransferUpdate::StatusChanged { id: sid, new_status, .. } => {
                assert_eq!(sid, id);
                assert!(matches!(new_status, TunnelStatus::Anchored));
            }
            other => panic!("expected StatusChanged, got {other:?}"),
        }

        drop(event_tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn btc_deposit_and_mint_converge_on_one_transfer() {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(32);
        let (update_tx, mut update_rx) = tokio::sync::mpsc::channel(32);
        let handle = tokio::spawn(JoinEngine::new_for_test(event_rx, update_tx).run());

        let txid = BitcoinTxid([7u8; 32]);
        let dest = Address::from_hex("0x065213232a03b6ef9c51e0c3250096474bb515b8").unwrap();

        // 1. Bitcoin custody deposit observed first (with the real Bitcoin block).
        event_tx
            .send(RawEvent::Bitcoin(BitcoinEvent::TunnelDeposit {
                txid,
                vout: 0,
                to_address: strait_core::types::BitcoinAddress::new("bc1qvault"),
                amount_sats: 100_000_000,
                op_return_data: None,
                hemi_destination: Some(dest),
                block_number: 800_000,
                block_hash: BlockHash([0u8; 32]),
                block_time: Utc::now(),
            }))
            .await
            .unwrap();

        let btc_id = match update_rx.recv().await.unwrap() {
            TunnelTransferUpdate::Created(t) => {
                assert_eq!(t.route, TunnelRoute::BtcToHemi);
                assert_eq!(t.source_tx.block_number, 800_000);
                assert!(t.destination_tx.is_none());
                assert_eq!(t.recipient, ChainAddress::Evm(dest));
                t.id
            }
            other => panic!("expected Created, got {other:?}"),
        };

        // 2. The Hemi DepositConfirmed mint for the same Bitcoin txid.
        event_tx
            .send(RawEvent::Hemi(HemiEvent::TunnelMint {
                tx_hash: TxHash([9u8; 32]),
                asset: Asset::Btc,
                amount: bigdecimal::BigDecimal::from(99_900_000u64),
                to: dest,
                source_txid: Some(txid),
                block_number: 6_037_628,
                block_time: Utc::now(),
                log_index: 0,
                gas_fee: None,
            }))
            .await
            .unwrap();

        match update_rx.recv().await.unwrap() {
            TunnelTransferUpdate::Created(t) => {
                // Same row — the deposit and mint converge on the Bitcoin-txid id.
                assert_eq!(t.id, btc_id, "deposit and mint must converge on one transfer");
                assert_eq!(t.route, TunnelRoute::BtcToHemi);
                let dest_tx = t.destination_tx.as_ref().expect("mint fills Hemi destination");
                assert_eq!(dest_tx.chain, Chain::Hemi);
                assert_eq!(dest_tx.block_number, 6_037_628);
            }
            other => panic!("expected Created, got {other:?}"),
        }

        drop(event_tx);
        let _ = handle.await;
    }
}
