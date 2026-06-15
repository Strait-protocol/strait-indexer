//! In-memory transfer state tracker.
//!
//! Tracks pending TunnelTransfers through their lifecycle, matching
//! source and destination events across chains.

use std::collections::HashMap;
use chrono::Utc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use strait_core::{
    error::StraitError,
    types::{BlockRef, Chain, ChainTxHash, TunnelStatus, TunnelTransfer},
};

/// Tracks in-flight transfers and the latest chain tip for each chain.
#[derive(Debug)]
pub struct TransferState {
    /// Active transfers keyed by their UUID.
    transfers: HashMap<Uuid, TunnelTransfer>,
    /// Index: source tx hash → transfer id, for fast lookup when a destination event arrives.
    source_tx_index: HashMap<ChainTxHash, Uuid>,
    /// Latest known block per chain.
    chain_tips: HashMap<Chain, BlockRef>,
}

impl TransferState {
    pub fn new() -> Self {
        Self {
            transfers: HashMap::new(),
            source_tx_index: HashMap::new(),
            chain_tips: HashMap::new(),
        }
    }

    /// Insert a newly created transfer (INITIATED).
    pub fn insert(&mut self, transfer: TunnelTransfer) {
        let id = transfer.id;
        let source_hash = transfer.source_tx.hash;
        self.source_tx_index.insert(source_hash, id);
        debug!(transfer_id = %id, "transfer inserted into state");
        self.transfers.insert(id, transfer);
    }

    /// Look up a transfer by its source transaction hash.
    pub fn get_by_source_tx(&self, hash: &ChainTxHash) -> Option<&TunnelTransfer> {
        self.source_tx_index
            .get(hash)
            .and_then(|id| self.transfers.get(id))
    }

    /// Get a transfer by ID.
    pub fn get(&self, id: &Uuid) -> Option<&TunnelTransfer> {
        self.transfers.get(id)
    }

    /// Get a mutable reference to a transfer by ID.
    pub fn get_mut(&mut self, id: &Uuid) -> Option<&mut TunnelTransfer> {
        self.transfers.get_mut(id)
    }

    /// Update a transfer's status.
    pub fn update_status(&mut self, id: &Uuid, status: TunnelStatus) -> Result<(), StraitError> {
        let transfer = self
            .transfers
            .get_mut(id)
            .ok_or_else(|| StraitError::TransferNotFound(*id))?;

        let old_status = transfer.status.clone();
        transfer.status = status.clone();

        // If finalized or failed, set finalized_at
        match &status {
            TunnelStatus::Finalized | TunnelStatus::Failed { .. } | TunnelStatus::Reorged { .. } => {
                transfer.finalized_at = Some(Utc::now());
            }
            _ => {}
        }

        info!(
            transfer_id = %id,
            old_status = %old_status,
            new_status = %status,
            "transfer status updated"
        );

        Ok(())
    }

    /// Mark a transfer as anchored (PoP proof or relay observed).
    pub fn anchor(&mut self, id: &Uuid) -> Result<(), StraitError> {
        self.update_status(id, TunnelStatus::Anchored)
    }

    /// Mark a transfer as finalized (destination tx confirmed).
    pub fn finalize(&mut self, id: &Uuid) -> Result<(), StraitError> {
        self.update_status(id, TunnelStatus::Finalized)
    }

    /// Mark a transfer as failed.
    pub fn fail(&mut self, id: &Uuid, reason: String) -> Result<(), StraitError> {
        self.update_status(id, TunnelStatus::Failed { reason })
    }

    /// Get all transfers in a given status.
    pub fn get_by_status(&self, status: &TunnelStatus) -> Vec<&TunnelTransfer> {
        self.transfers
            .values()
            .filter(|t| {
                std::mem::discriminant(&t.status) == std::mem::discriminant(status)
            })
            .collect()
    }

    /// Get all pending (INITIATED) transfers awaiting anchoring.
    pub fn get_pending(&self) -> Vec<&TunnelTransfer> {
        self.get_by_status(&TunnelStatus::Initiated)
    }

    /// Get all anchored transfers awaiting finalization.
    pub fn get_anchored(&self) -> Vec<&TunnelTransfer> {
        self.get_by_status(&TunnelStatus::Anchored)
    }

    /// Find all transfers whose Hemi destination tx block falls in [from, to].
    /// Used by the join engine to anchor transfers when a keystone fires.
    pub fn transfers_in_hemi_block_range(&self, from: u64, to: u64) -> Vec<&TunnelTransfer> {
        self.transfers.values().filter(|t| {
            t.destination_tx
                .as_ref()
                .map(|tx| tx.chain == Chain::Hemi && tx.block_number >= from && tx.block_number <= to)
                .unwrap_or(false)
        }).collect()
    }

    /// Return (id, hemi_mint_block) for all INITIATED transfers that have a
    /// confirmed Hemi destination block — the set the keystone fan-out checks.
    pub fn transfers_initiated_with_hemi_mint(&self) -> Vec<(uuid::Uuid, u64)> {
        self.transfers.values()
            .filter(|t| matches!(t.status, TunnelStatus::Initiated))
            .filter_map(|t| {
                t.destination_tx.as_ref()
                    .filter(|tx| tx.chain == Chain::Hemi)
                    .map(|tx| (t.id, tx.block_number))
            })
            .collect()
    }

    /// Record PoP anchoring fields on a transfer after it has been anchored.
    pub fn set_pop_anchor(
        &mut self,
        id: &uuid::Uuid,
        keystone_block: u64,
        pop_score: u64,
        anchored_at: chrono::DateTime<chrono::Utc>,
    ) {
        if let Some(t) = self.transfers.get_mut(id) {
            t.pop_anchored = true;
            t.pop_keystone_block = Some(keystone_block);
            t.pop_score = Some(pop_score);
            t.pop_anchored_at = Some(anchored_at);
        }
    }

    /// Find transfers by status discriminant (matches any variant of the same enum arm).
    pub fn transfers_by_status_discriminant(
        &self,
        discriminant: std::mem::Discriminant<TunnelStatus>,
    ) -> Vec<&TunnelTransfer> {
        self.transfers.values()
            .filter(|t| std::mem::discriminant(&t.status) == discriminant)
            .collect()
    }

    /// Alias for engine compatibility.
    pub fn transfers_by_status(&self, status: &TunnelStatus) -> Vec<&TunnelTransfer> {
        self.get_by_status(status)
    }

    /// Update the latest known block for a chain.
    pub fn update_chain_tip(&mut self, chain: Chain, tip: BlockRef) {
        debug!(chain = %chain, height = tip.height, "chain tip updated");
        self.chain_tips.insert(chain, tip);
    }

    /// Get the latest known block height for a chain.
    pub fn chain_tip_height(&self, chain: &Chain) -> Option<u64> {
        self.chain_tips.get(chain).map(|b| b.height)
    }

    /// Remove a completed transfer from active tracking.
    pub fn remove(&mut self, id: &Uuid) -> Option<TunnelTransfer> {
        let transfer = self.transfers.remove(id);
        if let Some(ref t) = transfer {
            // Clean up the source tx index
            let source_hash = t.source_tx.hash;
            self.source_tx_index.remove(&source_hash);
        }
        transfer
    }

    /// Number of active transfers being tracked.
    pub fn active_count(&self) -> usize {
        self.transfers.len()
    }

    /// Handle a reorg event — mark affected transfers as REORGED.
    pub fn handle_reorg(
        &mut self,
        chain: &Chain,
        affected_from_block: u64,
        reorg_event: strait_core::types::ReorgEvent,
    ) {
        let affected_ids: Vec<Uuid> = self
            .transfers
            .iter()
            .filter(|(_, t)| {
                // Check if this transfer's source or destination tx is on the reorged chain
                // and was confirmed in a block >= affected_from_block
                let source_on_chain = t.source_tx.chain == *chain;
                let source_block = t.source_tx.block_number;
                let dest_on_chain = t
                    .destination_tx
                    .as_ref()
                    .map(|tx| tx.chain == *chain)
                    .unwrap_or(false);
                let dest_block = t
                    .destination_tx
                    .as_ref()
                    .map(|tx| tx.block_number)
                    .unwrap_or(0);

                (source_on_chain && source_block >= affected_from_block)
                    || (dest_on_chain && dest_block >= affected_from_block)
            })
            .map(|(id, _)| *id)
            .collect();

        for id in affected_ids {
            warn!(
                transfer_id = %id,
                chain = %chain,
                affected_from_block = affected_from_block,
                "marking transfer as reorged"
            );

            if let Some(transfer) = self.transfers.get_mut(&id) {
                transfer.reorg_events.push(reorg_event.clone());
                transfer.status = TunnelStatus::Reorged {
                    retracted_at: Utc::now(),
                };
                transfer.finalized_at = Some(Utc::now());
            }
        }
    }
}

impl Default for TransferState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strait_core::types::{
        Address, Asset, BlockHash, ChainTransaction, ChainTxHash, Satoshi, TunnelDirection,
        TunnelRoute, TxHash,
    };
    use chrono::Utc;

    fn make_test_transfer() -> TunnelTransfer {
        TunnelTransfer {
            id: Uuid::new_v4(),
            asset: Asset::Btc,
            direction: TunnelDirection::In,
            route: TunnelRoute::BtcToHemi,
            amount: bigdecimal::BigDecimal::from(100_000),
            source_fee: None,
            dest_fee: None,
            withdrawal_uuid: None,
            sender: strait_core::types::ChainAddress::Bitcoin(
                strait_core::types::BitcoinAddress::new("bc1qtest"),
            ),
            recipient: strait_core::types::ChainAddress::Evm(Address([1u8; 20])),
            status: TunnelStatus::Initiated,
            initiated_at: Utc::now(),
            finalized_at: None,
            source_tx: ChainTransaction {
                chain: Chain::Bitcoin,
                hash: ChainTxHash::Bitcoin(strait_core::types::BitcoinTxid([2u8; 32])),
                block_number: 800_000,
                block_hash: BlockHash([3u8; 32]),
                timestamp: Utc::now(),
                confirmations: 1,
            },
            destination_tx: None,
            pop_anchored: false,
            pop_keystone_block: None,
            pop_score: None,
            pop_anchored_at: None,
            reorg_events: vec![],
        }
    }

    #[test]
    fn test_insert_and_get() {
        let mut state = TransferState::new();
        let transfer = make_test_transfer();
        let id = transfer.id;
        let source_hash = transfer.source_tx.hash;

        state.insert(transfer);
        assert!(state.get(&id).is_some());
        assert_eq!(state.active_count(), 1);

        // Lookup by source tx
        assert!(state.get_by_source_tx(&source_hash).is_some());
    }

    #[test]
    fn test_status_transitions() {
        let mut state = TransferState::new();
        let transfer = make_test_transfer();
        let id = transfer.id;
        state.insert(transfer);

        // INITIATED -> ANCHORED
        state.anchor(&id).unwrap();
        assert_eq!(state.get(&id).unwrap().status, TunnelStatus::Anchored);

        // ANCHORED -> FINALIZED
        state.finalize(&id).unwrap();
        assert_eq!(state.get(&id).unwrap().status, TunnelStatus::Finalized);
        assert!(state.get(&id).unwrap().finalized_at.is_some());
    }

    #[test]
    fn test_fail_transfer() {
        let mut state = TransferState::new();
        let transfer = make_test_transfer();
        let id = transfer.id;
        state.insert(transfer);

        state.fail(&id, "timeout".to_string()).unwrap();
        assert!(matches!(
            state.get(&id).unwrap().status,
            TunnelStatus::Failed { .. }
        ));
    }

    #[test]
    fn test_get_by_status() {
        let mut state = TransferState::new();

        let t1 = make_test_transfer();
        let t2 = make_test_transfer();
        let id2 = t2.id;

        state.insert(t1);
        state.insert(t2);
        state.anchor(&id2).unwrap();

        assert_eq!(state.get_pending().len(), 1);
        assert_eq!(state.get_anchored().len(), 1);
    }

    #[test]
    fn test_chain_tip() {
        let mut state = TransferState::new();
        assert!(state.chain_tip_height(&Chain::Bitcoin).is_none());

        state.update_chain_tip(
            Chain::Bitcoin,
            BlockRef {
                height: 800_000,
                hash: "abc".to_string(),
                timestamp: 1234567890,
            },
        );
        assert_eq!(state.chain_tip_height(&Chain::Bitcoin), Some(800_000));
    }

    #[test]
    fn test_remove() {
        let mut state = TransferState::new();
        let transfer = make_test_transfer();
        let id = transfer.id;
        state.insert(transfer);

        assert_eq!(state.active_count(), 1);
        state.remove(&id);
        assert_eq!(state.active_count(), 0);
        assert!(state.get(&id).is_none());
    }
}