//! Bitcoin payout watcher — finalizes Hemi→BTC withdrawals once their Bitcoin
//! payout is observed.
//!
//! Withdrawals have no Hemi-side "fulfilled" event: the operator sends BTC to the
//! user's address out on Bitcoin. We detect that payout by watching the
//! withdrawal's recipient address through BitcoinKit on Hemi (no Bitcoin node
//! required) for a UTXO matching the net amount, then mark the transfer FINALIZED.
//!
//! Best-effort by design:
//!   - BitcoinKit exposes *unspent* outputs, so a payout already spent by the
//!     recipient before we poll cannot be observed. Frequent polling catches live
//!     payouts; already-spent historical ones are missed.
//!   - Matching is by recipient + amount (within a small tolerance). The uuid in
//!     the payout's OP_RETURN would be exact once its encoding is confirmed — we log
//!     any OP_RETURN found on a matched payout to help confirm that encoding.

use std::sync::Arc;
use std::time::Duration;

use alloy::primitives::B256;
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use chrono::Utc;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use strait_bitcoin::BitcoinKitCaller;
use strait_core::error::Result;
use strait_core::types::BitcoinTxid;
use strait_evm::contracts::IBitcoinTunnelManager;
use strait_store::{Database, TunnelTransferRepo};

/// Polls pending Hemi→BTC withdrawals and finalizes each one whose Bitcoin payout
/// is observed via BitcoinKit.
pub struct BtcPayoutWatcher {
    caller: BitcoinKitCaller,
    /// Hemi EVM provider — used to retry recovering the BTC destination address
    /// from initiateWithdrawal calldata for withdrawals that failed at index time.
    hemi_provider: Arc<dyn Provider>,
    db: Database,
    poll_interval: Duration,
    min_confirmations: u32,
}

impl BtcPayoutWatcher {
    pub fn new(
        caller: BitcoinKitCaller,
        hemi_provider: Arc<dyn Provider>,
        db: Database,
        poll_interval_secs: u64,
        min_confirmations: u32,
    ) -> Self {
        Self {
            caller,
            hemi_provider,
            db,
            // Payouts confirm over minutes — polling faster just burdens the Hemi RPC.
            poll_interval: Duration::from_secs(poll_interval_secs.max(60)),
            min_confirmations,
        }
    }

    pub async fn run(self) -> Result<()> {
        info!(
            min_confirmations = self.min_confirmations,
            poll_secs = self.poll_interval.as_secs(),
            "BTC payout watcher started"
        );
        loop {
            if let Err(e) = self.tick().await {
                warn!(error = %e, "BTC payout watcher tick failed");
            }
            sleep(self.poll_interval).await;
        }
    }

    async fn tick(&self) -> Result<()> {
        // Phase 1: Retry BTC address recovery for withdrawals with placeholder recipients.
        // At indexing time, recover_withdrawal_btc_address may have failed (rate limit,
        // transient RPC error). Re-try here on every poll cycle so these withdrawals
        // eventually get a real address and can be matched by the UTXO phase below.
        if let Err(e) = self.retry_placeholder_addresses().await {
            warn!(error = %e, "BTC address recovery retry phase failed");
        }

        // Phase 2: Check UTXOs for withdrawals with real BTC addresses.
        let repo = TunnelTransferRepo::new(&self.db);
        let pending = repo.list_pending_btc_payouts(200).await?;
        for w in pending {
            if !is_real_btc_address(&w.recipient) {
                continue; // placeholder recipient — nothing to watch yet
            }
            // Pace requests across the Hemi RPC to avoid rate-limit bursts.
            // With many pending withdrawals each requiring multiple eth_call lookups
            // (UTXOs, OP_RETURN, confirmations, fee), firing them all at once easily
            // fills a 300-req/60s rolling window. 200ms between withdrawals keeps
            // the burst spread across the full poll interval.
            sleep(Duration::from_millis(200)).await;
            // The withdrawal's 4-byte vaultUUID is echoed in the payout's OP_RETURN —
            // the deterministic key that disambiguates otherwise-identical withdrawals
            // (same recipient + amount). Without it, amount matching mis-attributes.
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

                // Match the payout to THIS exact withdrawal via its OP_RETURN vaultUUID.
                let payout_uuid = match self.caller.get_op_return_data(&txid).await {
                    Ok(Some(script)) => decode_payout_vault_uuid(&script),
                    _ => None,
                };
                if payout_uuid != Some(want_vault_uuid) {
                    continue;
                }

                let confs = self.caller.get_confirmations(&txid).await.unwrap_or(0);
                if confs < self.min_confirmations {
                    continue; // payout matched but not yet final
                }

                // Bitcoin network fee for the payout (Σ inputs − Σ outputs), best-effort.
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
        Ok(())
    }

    /// For each INITIATED Hemi→BTC withdrawal whose BTC address could not be recovered
    /// at indexing time (stored as `withdrawal-uuid-N`), attempt to decode the real
    /// address from the initiateWithdrawal transaction calldata.
    ///
    /// Failures at index time are almost always transient (RPC rate limit / timeout),
    /// so retrying here on every poll cycle lets placeholder withdrawals self-heal.
    async fn retry_placeholder_addresses(&self) -> Result<()> {
        let repo = TunnelTransferRepo::new(&self.db);
        let pending = repo.list_btc_withdrawals_needing_recipient(50).await?;
        if pending.is_empty() {
            return Ok(());
        }
        debug!(count = pending.len(), "Retrying BTC address recovery for placeholder withdrawals");

        for w in pending {
            sleep(Duration::from_millis(200)).await;

            // Parse the Hemi source tx hash stored in DB.
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

/// True for a real Bitcoin address — i.e. not the `withdrawal-uuid-…` placeholder
/// that's stored when the recipient couldn't be recovered from the calldata.
fn is_real_btc_address(s: &str) -> bool {
    if s.starts_with("withdrawal-uuid-") {
        return false;
    }
    // bech32 (bc1/tb1) or base58 (1/3 mainnet, m/n/2 testnet) prefixes.
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
