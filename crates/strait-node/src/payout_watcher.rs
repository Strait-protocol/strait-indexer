//! Bitcoin payout watcher — finalizes Hemi→BTC withdrawals once their Bitcoin
//! payout is observed.
//!
//! Withdrawals have no Hemi-side "fulfilled" event: the operator sends BTC to the
//! user's address out on Bitcoin. We detect that payout in two ways:
//!
//! **Phase 2 — UTXO polling**: Watches the withdrawal recipient's Bitcoin address via
//! BitcoinKit on Hemi for a matching UTXO. Works for freshly-paid, still-unspent outputs.
//! Limited by BitcoinKit only exposing *unspent* outputs — a payout already spent by the
//! recipient before we poll cannot be observed this way.
//!
//! **Phase 3 — Vault sweep txid**: Calls `currentSweepUTXO()` on each SimpleBitcoinVault
//! contract. The vault stores the Bitcoin txid of its last confirmed sweep, so we can
//! detect a payout regardless of UTXO availability. The OP_RETURN in the sweep tx carries
//! the vault-specific withdrawal UUID to identify which withdrawal was paid.
//!
//! Best-effort by design — historical sweeps overwritten by newer ones are only catchable
//! via Phase 2 while the UTXO is unspent.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use alloy::primitives::{Address, B256};
use alloy::providers::Provider;
use alloy::rpc::types::TransactionRequest;
use alloy::sol_types::SolCall;
use chrono::Utc;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use strait_bitcoin::BitcoinKitCaller;
use strait_core::error::Result;
use strait_core::types::BitcoinTxid;
use strait_evm::contracts::{IBitcoinTunnelManager, ISimpleBitcoinVault};
use strait_store::{Database, TunnelTransferRepo};
use strait_store::tunnel_transfers::TunnelTransferRow;

/// Polls pending Hemi→BTC withdrawals and finalizes each one whose Bitcoin payout
/// is observed via BitcoinKit or vault sweep detection.
pub struct BtcPayoutWatcher {
    caller: BitcoinKitCaller,
    /// Hemi EVM provider — used to retry recovering the BTC destination address
    /// from initiateWithdrawal calldata and to call currentSweepUTXO on vaults.
    hemi_provider: Arc<dyn Provider>,
    db: Database,
    poll_interval: Duration,
    min_confirmations: u32,
    /// Ordered list of SimpleBitcoinVault addresses (index 0 = vault 0).
    /// Empty means Phase 3 sweep detection is disabled.
    vault_contracts: Vec<Address>,
    /// Last-seen currentSweepUTXO per vault index — avoids re-processing the same sweep.
    last_sweep: HashMap<u32, [u8; 32]>,
}

impl BtcPayoutWatcher {
    pub fn new(
        caller: BitcoinKitCaller,
        hemi_provider: Arc<dyn Provider>,
        db: Database,
        poll_interval_secs: u64,
        min_confirmations: u32,
        vault_contracts: Vec<Address>,
    ) -> Self {
        Self {
            caller,
            hemi_provider,
            db,
            // Payouts confirm over minutes — polling faster just burdens the Hemi RPC.
            poll_interval: Duration::from_secs(poll_interval_secs.max(60)),
            min_confirmations,
            vault_contracts,
            last_sweep: HashMap::new(),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!(
            min_confirmations = self.min_confirmations,
            poll_secs = self.poll_interval.as_secs(),
            vault_count = self.vault_contracts.len(),
            "BTC payout watcher started"
        );
        loop {
            if let Err(e) = self.tick().await {
                warn!(error = %e, "BTC payout watcher tick failed");
            }
            sleep(self.poll_interval).await;
        }
    }

    async fn tick(&mut self) -> Result<()> {
        // Phase 1: Retry BTC address recovery for withdrawals with placeholder recipients.
        if let Err(e) = self.retry_placeholder_addresses().await {
            warn!(error = %e, "BTC address recovery retry phase failed");
        }

        let repo = TunnelTransferRepo::new(&self.db);
        let pending = repo.list_pending_btc_payouts(200).await?;

        // Phase 2: Check UTXOs for withdrawals with real BTC addresses.
        for w in &pending {
            if !is_real_btc_address(&w.recipient) {
                continue;
            }
            sleep(Duration::from_millis(400)).await;
            let Some(uuid) = w.withdrawal_uuid else {
                debug!(transfer = %w.id, "skipping HEMI_TO_BTC withdrawal — no uuid recorded");
                continue;
            };
            let want_vault_uuid = (uuid as u64 & 0xffff_ffff) as u32;

            let utxos = match self.caller.get_utxos_for_address(&w.recipient).await {
                Ok(u) => u,
                Err(e) => {
                    warn!(recipient = %w.recipient, error = %e, "UTXO lookup failed — skipping");
                    continue;
                }
            };

            for utxo in utxos {
                let txid = BitcoinTxid(utxo.txId.into());

                let payout_uuid = match self.caller.get_op_return_data(&txid).await {
                    Ok(Some(script)) => decode_payout_vault_uuid(&script),
                    _ => None,
                };
                if payout_uuid != Some(want_vault_uuid) {
                    continue;
                }

                let confs = self.caller.get_confirmations(&txid).await.unwrap_or(0);
                if confs < self.min_confirmations {
                    continue;
                }

                let fee_sats = self.caller.get_tx_fee_sats(&txid).await.ok().flatten();
                let dest_fee = fee_sats.map(bigdecimal::BigDecimal::from);

                let txid_hex = hex::encode(txid.0);
                repo.set_btc_payout(w.id, &txid_hex, None, dest_fee, Utc::now()).await?;
                info!(
                    transfer = %w.id,
                    recipient = %w.recipient,
                    uuid,
                    sats = utxo.value.saturating_to::<u64>(),
                    fee_sats = ?fee_sats,
                    confirmations = confs,
                    txid = %txid_hex,
                    "Hemi→BTC withdrawal FINALIZED — Bitcoin payout matched by uuid"
                );
                break;
            }
        }

        // Phase 3: Vault sweep txid detection.
        // Calls currentSweepUTXO() on each vault; if it changed since last poll,
        // reads the sweep's OP_RETURN to find which withdrawal was paid and finalizes it.
        // Works even when the recipient has already spent the payout UTXO.
        if !self.vault_contracts.is_empty() {
            self.check_vault_sweeps(&pending).await?;
        }

        Ok(())
    }

    async fn check_vault_sweeps(&mut self, pending: &[TunnelTransferRow]) -> Result<()> {
        // Group pending withdrawals by vault_index (upper 32 bits of uuid).
        let mut by_vault: HashMap<u32, Vec<&TunnelTransferRow>> = HashMap::new();
        for w in pending {
            if !is_real_btc_address(&w.recipient) {
                continue;
            }
            let Some(uuid) = w.withdrawal_uuid else { continue };
            let vault_idx = ((uuid as u64) >> 32) as u32;
            by_vault.entry(vault_idx).or_default().push(w);
        }

        if by_vault.is_empty() {
            return Ok(());
        }

        let repo = TunnelTransferRepo::new(&self.db);

        for (vault_idx, withdrawals) in &by_vault {
            let vault_idx = *vault_idx;
            let vault_addr = match self.vault_contracts.get(vault_idx as usize) {
                Some(a) => *a,
                None => {
                    debug!(vault_idx, "vault index out of range — skipping sweep check");
                    continue;
                }
            };

            sleep(Duration::from_millis(200)).await;

            let sweep_bytes = match fetch_current_sweep_utxo(&self.hemi_provider, vault_addr).await {
                Ok(b) => b,
                Err(e) => {
                    warn!(vault_idx, error = %e, "currentSweepUTXO call failed");
                    continue;
                }
            };

            if sweep_bytes == [0u8; 32] {
                continue; // vault has no sweep yet
            }
            if self.last_sweep.get(&vault_idx) == Some(&sweep_bytes) {
                continue; // already processed this sweep
            }

            let txid = BitcoinTxid(sweep_bytes);

            sleep(Duration::from_millis(200)).await;

            let op_return = match self.caller.get_op_return_data(&txid).await {
                Ok(Some(d)) => d,
                Ok(None) => {
                    // No OP_RETURN — can't match, mark seen to avoid re-querying.
                    self.last_sweep.insert(vault_idx, sweep_bytes);
                    continue;
                }
                Err(e) => {
                    warn!(vault_idx, error = %e, "OP_RETURN lookup for sweep txid failed");
                    continue;
                }
            };

            let sweep_vault_uuid = match decode_payout_vault_uuid(&op_return) {
                Some(u) => u,
                None => {
                    self.last_sweep.insert(vault_idx, sweep_bytes);
                    continue;
                }
            };

            // Find the withdrawal matching this sweep's vault-specific UUID.
            let mut finalized = false;
            for w in withdrawals {
                let Some(uuid) = w.withdrawal_uuid else { continue };
                let want = (uuid as u64 & 0xffff_ffff) as u32;
                if want != sweep_vault_uuid {
                    continue;
                }

                let confs = self.caller.get_confirmations(&txid).await.unwrap_or(0);
                if confs < self.min_confirmations {
                    debug!(
                        transfer = %w.id,
                        vault_idx,
                        confirmations = confs,
                        needed = self.min_confirmations,
                        "sweep txid matched but not yet confirmed — will retry"
                    );
                    // Don't update last_sweep — re-check next poll.
                    finalized = true; // matched, just pending confirmation
                    break;
                }

                let txid_hex = hex::encode(sweep_bytes);
                let fee_sats = self.caller.get_tx_fee_sats(&txid).await.ok().flatten();
                let dest_fee = fee_sats.map(bigdecimal::BigDecimal::from);

                repo.set_btc_payout(w.id, &txid_hex, None, dest_fee, Utc::now()).await?;
                info!(
                    transfer = %w.id,
                    vault_idx,
                    sweep_uuid = sweep_vault_uuid,
                    confirmations = confs,
                    txid = %txid_hex,
                    "Hemi→BTC withdrawal FINALIZED — detected via vault sweep txid"
                );
                self.last_sweep.insert(vault_idx, sweep_bytes);
                finalized = true;
                break;
            }

            if !finalized {
                // No pending withdrawal matched this sweep — mark seen.
                self.last_sweep.insert(vault_idx, sweep_bytes);
            }
        }

        Ok(())
    }

    /// For each INITIATED Hemi→BTC withdrawal whose BTC address could not be recovered
    /// at indexing time (stored as `withdrawal-uuid-N`), attempt to decode the real
    /// address from the initiateWithdrawal transaction calldata.
    async fn retry_placeholder_addresses(&self) -> Result<()> {
        let repo = TunnelTransferRepo::new(&self.db);
        let pending = repo.list_btc_withdrawals_needing_recipient(50).await?;
        if pending.is_empty() {
            return Ok(());
        }
        debug!(count = pending.len(), "Retrying BTC address recovery for placeholder withdrawals");

        for w in pending {
            sleep(Duration::from_millis(400)).await;

            let tx_bytes = match hex::decode(w.source_tx_hash.trim_start_matches("0x")) {
                Ok(b) if b.len() == 32 => b,
                _ => {
                    debug!(transfer = %w.id, "invalid source_tx_hash — skipping address retry");
                    continue;
                }
            };
            let tx_hash = B256::from_slice(&tx_bytes);

            let tx = match self.hemi_provider.get_transaction_by_hash(tx_hash).await {
                Ok(Some(tx)) => tx,
                Ok(None) => {
                    debug!(transfer = %w.id, "tx not found for placeholder withdrawal");
                    continue;
                }
                Err(e) => {
                    debug!(transfer = %w.id, error = %e, "tx fetch failed for placeholder withdrawal");
                    continue;
                }
            };

            let decoded =
                match IBitcoinTunnelManager::initiateWithdrawalCall::abi_decode(tx.input.as_ref(), false) {
                    Ok(d) => d,
                    Err(_) => {
                        debug!(
                            transfer = %w.id,
                            "initiateWithdrawal calldata decode failed — may be a different entry point"
                        );
                        continue;
                    }
                };

            let btc_addr = decoded.btcAddress;
            if !is_real_btc_address(&btc_addr) {
                debug!(transfer = %w.id, addr = %btc_addr, "decoded btcAddress is not a valid BTC address");
                continue;
            }

            match repo.update_btc_withdrawal_recipient(w.id, &btc_addr).await {
                Ok(()) => info!(
                    transfer = %w.id,
                    btc_address = %btc_addr,
                    "Recovered real BTC address for Hemi→BTC withdrawal — payout matching now active"
                ),
                Err(e) => warn!(transfer = %w.id, error = %e, "Failed to update BTC withdrawal recipient"),
            }
        }
        Ok(())
    }
}

/// Call `currentSweepUTXO()` on a SimpleBitcoinVault contract.
/// Returns the Bitcoin txid as a 32-byte array, or an error on RPC failure.
async fn fetch_current_sweep_utxo(provider: &Arc<dyn Provider>, vault: Address) -> Result<[u8; 32]> {
    use strait_core::error::StraitError;

    let calldata = ISimpleBitcoinVault::currentSweepUTXOCall {}.abi_encode();
    let req = TransactionRequest::default()
        .to(vault)
        .input(calldata.into());

    let raw = provider
        .call(&req)
        .await
        .map_err(|e| StraitError::EvmProvider(format!("currentSweepUTXO call failed: {e}")))?;

    let result = ISimpleBitcoinVault::currentSweepUTXOCall::abi_decode_returns(&raw, false)
        .map_err(|e| StraitError::Parse(format!("currentSweepUTXO decode failed: {e}")))?;

    Ok(result._0.into())
}

/// True for a real Bitcoin address — i.e. not the `withdrawal-uuid-…` placeholder
/// that's stored when the recipient couldn't be recovered from the calldata.
fn is_real_btc_address(s: &str) -> bool {
    if s.starts_with("withdrawal-uuid-") {
        return false;
    }
    matches!(s.chars().next(), Some('b' | 't' | '1' | '3' | 'm' | 'n' | '2'))
}

/// Decode the 4-byte big-endian vaultUUID from a payout OP_RETURN script
/// (`0x6a 0x04 <4 bytes>`, confirmed from a real mainnet payout). Returns `None`
/// if the script isn't that shape.
fn decode_payout_vault_uuid(script: &[u8]) -> Option<u32> {
    if script.len() >= 6 && script[0] == 0x6a && script[1] == 0x04 {
        Some(u32::from_be_bytes([script[2], script[3], script[4], script[5]]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_payout_vault_uuid() {
        // Real mainnet payout OP_RETURN: 6a 04 00 00 03 27 → vaultUUID 807.
        assert_eq!(decode_payout_vault_uuid(&[0x6a, 0x04, 0, 0, 0x03, 0x27]), Some(807));
        assert_eq!(decode_payout_vault_uuid(&[0x6a, 0x14, 0xAB]), None); // wrong push len
        assert_eq!(decode_payout_vault_uuid(&[0x00]), None);             // not OP_RETURN
    }

    #[test]
    fn placeholder_recipients_are_skipped() {
        assert!(!is_real_btc_address("withdrawal-uuid-804583"));
        assert!(is_real_btc_address("bc1qwql2auj2u56sk87n2p8g464nnswp6qarkcy5tk"));
        assert!(is_real_btc_address("tb1qexampletestnetaddr"));
    }
}
