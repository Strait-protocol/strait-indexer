//! Bitcoin tunnel deposit watcher backed by the BitcoinKitV1 precompile.
//!
//! Instead of parsing raw Bitcoin RPC responses for OP_RETURN data, we call
//! BitcoinKitV1 on Hemi directly. This gives us:
//!
//!   - `transactionExists(txId)` — cheap existence check before fetching
//!   - `getTxConfirmations(txId)` — confirmation count without a Bitcoin node
//!   - `getTransactionByTxId(txId)` — full tx including Output.isOpReturn and
//!     Output.opReturnData, which contains the Hemi destination address
//!   - `getUTXOsForBitcoinAddress(addr, page, size)` — watch custody addresses
//!     for new UTXOs instead of scanning every Bitcoin block
//!
//! The Bitcoin RPC node is still used for new-block detection and for
//! computing reorg windows (see `reorg.rs`). BitcoinKit handles verification
//! and data extraction.
//!
//! FIXME: The exact byte encoding of `opReturnData` for Hemi tunnel deposits
//! (i.e. how the Hemi destination address is encoded) must be confirmed with
//! Hemi documentation before the `parse_hemi_destination` function below can
//! be finalised.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use alloy::primitives::B256;
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use tracing::{debug, info, warn};

use strait_core::{
    error::{Result, StraitError},
    types::{Address, BitcoinAddress, BitcoinTxid},
};

use crate::contracts::{IBitcoinKitV1, UTXO, addresses};

// ============================================================================
// BitcoinKit caller
// ============================================================================

/// Thin wrapper around the BitcoinKitV1 precompile for deposit verification
/// and OP_RETURN data extraction.
pub struct BitcoinKitCaller {
    provider: Arc<dyn Provider>,
    contract: alloy::primitives::Address,
}

impl BitcoinKitCaller {
    /// Create a caller for mainnet BitcoinKitV1.
    pub fn mainnet(provider: Arc<dyn Provider>) -> Self {
        Self { provider, contract: addresses::HEMI_BITCOIN_KIT_V1 }
    }

    /// Create a caller for Hemi Sepolia (BitcoinKit v0).
    pub fn testnet(provider: Arc<dyn Provider>) -> Self {
        Self { provider, contract: addresses::HEMI_SEPOLIA_BITCOIN_KIT_V0 }
    }

    /// Create a caller for an explicitly-configured BitcoinKit address.
    pub fn new(provider: Arc<dyn Provider>, contract: alloy::primitives::Address) -> Self {
        Self { provider, contract }
    }

    /// Current Bitcoin tip height as seen by Hemi (via `getLastHeader`).
    pub async fn get_tip_height(&self) -> Result<u32> {
        let call = IBitcoinKitV1::getLastHeaderCall {};
        let result = self.call(call.abi_encode()).await?;
        let decoded = IBitcoinKitV1::getLastHeaderCall::abi_decode_returns(&result, false)
            .map_err(|e| StraitError::Parse(format!("getLastHeader decode: {e}")))?;
        Ok(decoded._0.height)
    }

    /// Check whether a Bitcoin txid exists in the chain as seen by Hemi.
    pub async fn transaction_exists(&self, txid: &BitcoinTxid) -> Result<bool> {
        let call = IBitcoinKitV1::transactionExistsCall { txId: B256::from(txid.0) };
        let result = self.call(call.abi_encode()).await?;
        let decoded = IBitcoinKitV1::transactionExistsCall::abi_decode_returns(&result, false)
            .map_err(|e| StraitError::Parse(format!("transactionExists decode: {e}")))?;
        Ok(decoded.exists)
    }

    /// Get the number of Bitcoin confirmations for a txid.
    pub async fn get_confirmations(&self, txid: &BitcoinTxid) -> Result<u32> {
        let call = IBitcoinKitV1::getTxConfirmationsCall { txId: B256::from(txid.0) };
        let result = self.call(call.abi_encode()).await?;
        let decoded = IBitcoinKitV1::getTxConfirmationsCall::abi_decode_returns(&result, false)
            .map_err(|e| StraitError::Parse(format!("getTxConfirmations decode: {e}")))?;
        Ok(decoded.confirmations)
    }

    /// Fetch all UTXOs for a Bitcoin custody address (paginated, page size 50).
    pub async fn get_utxos_for_address(
        &self,
        btc_address: &str,
    ) -> Result<Vec<UTXO>> {
        let mut all = Vec::new();
        let page_size: u32 = 50;
        let mut page: u32 = 0;

        loop {
            let call = IBitcoinKitV1::getUTXOsForBitcoinAddressCall {
                btcAddress: btc_address.to_string(),
                pageNumber: page,
                pageSize: page_size,
            };
            let result = self.call(call.abi_encode()).await?;
            let decoded = IBitcoinKitV1::getUTXOsForBitcoinAddressCall::abi_decode_returns(
                &result, false,
            )
            .map_err(|e| StraitError::Parse(format!("getUTXOs decode: {e}")))?;

            let count = decoded._0.len();
            all.extend(decoded._0);

            if count < page_size as usize {
                break;
            }
            page += 1;
        }

        Ok(all)
    }

    /// Read the OP_RETURN payload from a Bitcoin transaction by txid.
    ///
    /// Uses `getTransactionByTxId` and scans its outputs for `isOpReturn == true`
    /// OR a script starting with `0x6a` (the OP_RETURN opcode). The script-prefix
    /// fallback guards against BitcoinKit bugs where `isOpReturn` is not set even
    /// though the output is genuinely OP_RETURN (observed in vault sweep txs on mainnet).
    pub async fn get_op_return_data(&self, txid: &BitcoinTxid) -> Result<Option<Vec<u8>>> {
        let call = IBitcoinKitV1::getTransactionByTxIdCall { txId: B256::from(txid.0) };
        let result = self.call(call.abi_encode()).await?;
        let tx = IBitcoinKitV1::getTransactionByTxIdCall::abi_decode_returns(&result, false)
            .map_err(|e| StraitError::Parse(format!("getTransactionByTxId decode: {e}")))?
            ._0;

        for output in tx.outputs {
            if output.isOpReturn || output.script.first() == Some(&0x6a) {
                return Ok(Some(output.script.to_vec()));
            }
        }
        Ok(None)
    }

    /// Bitcoin transaction fee in sats = Σ input values − Σ output values.
    ///
    /// Best-effort: returns `None` unless BitcoinKit reports a *complete* set of
    /// inputs and outputs (an incomplete set would understate the fee).
    pub async fn get_tx_fee_sats(&self, txid: &BitcoinTxid) -> Result<Option<u64>> {
        let call = IBitcoinKitV1::getTransactionByTxIdCall { txId: B256::from(txid.0) };
        let result = self.call(call.abi_encode()).await?;
        let tx = IBitcoinKitV1::getTransactionByTxIdCall::abi_decode_returns(&result, false)
            .map_err(|e| StraitError::Parse(format!("getTransactionByTxId decode: {e}")))?
            ._0;

        if !tx.containsAllInputs || !tx.containsAllOutputs {
            return Ok(None);
        }
        let total_in: u128 = tx.inputs.iter().map(|i| i.inValue.saturating_to::<u128>()).sum();
        let total_out: u128 = tx.outputs.iter().map(|o| o.outValue.saturating_to::<u128>()).sum();
        Ok(total_in.checked_sub(total_out).map(|f| f as u64))
    }

    /// Execute a raw eth_call against the BitcoinKit precompile.
    async fn call(&self, data: Vec<u8>) -> Result<Vec<u8>> {
        use alloy::rpc::types::TransactionRequest;

        let req = TransactionRequest::default()
            .to(self.contract)
            .input(data.into());

        let result = self.provider
            .call(&req)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("BitcoinKit call failed: {e}")))?;

        Ok(result.to_vec())
    }
}

// ============================================================================
// OP_RETURN decoder
// ============================================================================

/// Parse the Hemi EVM destination address from a raw OP_RETURN output script.
///
/// Encoding confirmed from hemilabs/bitcoin-tunnel-contracts source
/// (SimpleBitcoinVaultUTXOLogicHelper.sol). The function parses `output.script`
/// (the full Bitcoin output script including the OP_RETURN opcode), NOT
/// `output.opReturnData`. Two formats are supported:
///
///   Format 1 — 22-byte script:
///     `0x6a` (OP_RETURN) + `0x14` (OP_PUSHBYTES_20) + <20 raw address bytes>
///
///   Format 2 — 42-byte script:
///     `0x6a` (OP_RETURN) + `0x28` (OP_PUSHBYTES_40) + <40 ASCII hex address bytes>
///
/// The OP_RETURN output must be within the first 8 outputs of the transaction
/// (vault code caps the scan at outputMaxLen = min(outputs.len(), 8)).
///
/// Note: pass `output.script`, not `output.opReturnData`.
pub fn parse_hemi_destination(script: &[u8]) -> Option<Address> {
    // Must start with OP_RETURN opcode (0x6a)
    if script.first() != Some(&0x6a) {
        return None;
    }

    match script.len() {
        // Format 1: 0x6a 0x14 <20 raw bytes> = 22 bytes total
        22 => {
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&script[2..22]);
            Some(Address(addr))
        }
        // Format 2: 0x6a 0x28 <40 ASCII hex bytes> = 42 bytes total
        42 => {
            let hex_bytes = &script[2..42];
            let hex_str = std::str::from_utf8(hex_bytes).ok()?;
            let decoded = hex::decode(hex_str).ok()?;
            if decoded.len() != 20 {
                return None;
            }
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&decoded);
            Some(Address(addr))
        }
        n => {
            warn!(bytes = n, "OP_RETURN script length does not match either known Hemi tunnel format (22 or 42 bytes)");
            None
        }
    }
}

// ============================================================================
// Custody address watcher
// ============================================================================

/// Watches a set of Bitcoin tunnel custody addresses for new UTXOs using
/// BitcoinKitV1, emitting deposit candidates for the ingester to process.
pub struct CustodyWatcher {
    caller: BitcoinKitCaller,
    addresses: HashSet<String>,
    /// Txids already processed — prevents re-emitting on subsequent polls.
    seen: HashSet<[u8; 32]>,
    /// False until the first poll has seeded `seen` with pre-existing UTXOs.
    initialized: bool,
}

impl CustodyWatcher {
    pub fn new(caller: BitcoinKitCaller, addresses: Vec<String>) -> Self {
        Self {
            caller,
            addresses: addresses.into_iter().collect(),
            seen: HashSet::new(),
            initialized: false,
        }
    }

    /// On the very first call, enumerate all currently-unspent UTXOs across every
    /// watched address and mark them seen without processing them. These are
    /// pre-existing deposits already captured by the Hemi EVM ingester via
    /// DepositConfirmed. Marking them seen upfront avoids a burst of
    /// getTransactionByTxId calls on startup that would saturate the rate limit.
    ///
    /// Returns true only if ALL addresses were successfully seeded. On partial
    /// failure (any 429/error), returns false so the next poll retries the init
    /// rather than treating unseeded addresses' UTXOs as new deposits.
    async fn initialize_seen(&mut self) -> bool {
        let mut all_ok = true;
        let mut total = 0usize;
        for addr in &self.addresses.clone() {
            // Pace seed calls: 400ms gap = 2.5 calls/sec, well under the 300 req/min
            // (5 req/s) public Hemi RPC limit even when the EVM ingester fires concurrently.
            tokio::time::sleep(Duration::from_millis(400)).await;
            let utxos = match self.caller.get_utxos_for_address(addr).await {
                Ok(u) => u,
                Err(e) => {
                    warn!(address = %addr, error = %e, "Seed-seen UTXO fetch failed — will retry on next poll");
                    all_ok = false;
                    continue;
                }
            };
            for utxo in &utxos {
                self.seen.insert(utxo.txId.into());
            }
            total += utxos.len();
        }
        if all_ok {
            info!(utxos_skipped = total, "Custody watcher initialized — pre-existing UTXOs marked seen, watching for new deposits only");
            self.initialized = true;
        } else {
            warn!(seeded_so_far = total, "Seed-seen incomplete — retrying failed addresses on next poll");
        }
        all_ok
    }

    /// Poll all watched addresses and return any new UTXOs not yet seen.
    pub async fn poll_new_deposits(&mut self) -> Result<Vec<DepositCandidate>> {
        if !self.initialized {
            // Run seed-seen and always return early this cycle — success or not.
            // On success, the 60s poll sleep separates seed-seen from the first
            // real poll, preventing a 14-call burst (7 seed + 7 poll back-to-back)
            // that trips the 300 req/min rate limit. On failure, we retry next cycle.
            self.initialize_seen().await;
            return Ok(Vec::new());
        }

        let mut candidates = Vec::new();

        // Bitcoin tip height (best-effort) so we can derive each deposit's block.
        let tip_height = self.caller.get_tip_height().await.unwrap_or(0);

        for addr in &self.addresses.clone() {
            // Pace the 7 UTXO-list calls to avoid a burst that overlaps with the
            // Hemi EVM ingester and exceeds the 300 req/min rolling rate limit.
            tokio::time::sleep(Duration::from_millis(500)).await;
            // One bad/invalid custody address must not fail the whole poll —
            // log it and move on to the others.
            let utxos = match self.caller.get_utxos_for_address(addr).await {
                Ok(u) => u,
                Err(e) => {
                    warn!(address = %addr, error = %e, "Failed to read UTXOs for custody address — skipping");
                    continue;
                }
            };
            debug!(address = %addr, count = utxos.len(), "Polled UTXOs");

            for utxo in utxos {
                let txid: [u8; 32] = utxo.txId.into();

                if self.seen.contains(&txid) {
                    continue;
                }

                // Pace per-UTXO API calls to stay within the public Hemi RPC rate limit
                // (300 req/min). Without this, a custody address with many existing UTXOs
                // fires a burst of getTransactionByTxId calls on every poll, saturating
                // the limit and causing 429s for the other watchers.
                tokio::time::sleep(Duration::from_millis(150)).await;

                // Fetch OP_RETURN data to extract the Hemi destination.
                // On any error (including 429) mark as seen to prevent re-bursting on
                // the next poll. This skip is permanent for the txid (a restart
                // seeds all existing UTXOs as seen), but the transfer itself is
                // still captured via the Hemi DepositConfirmed event — only the
                // Bitcoin-side leg data (real BTC block, gross amount) is lost.
                let bitcoin_txid = BitcoinTxid(txid);
                let op_return = match self.caller.get_op_return_data(&bitcoin_txid).await {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(txid = %hex::encode(txid), error = %e, "OP_RETURN fetch failed — skipping UTXO");
                        self.seen.insert(txid);
                        continue;
                    }
                };

                let hemi_destination = op_return.as_deref().and_then(parse_hemi_destination);

                if hemi_destination.is_none() {
                    warn!(
                        txid = %hex::encode(txid),
                        "No parseable OP_RETURN on deposit UTXO — skipping"
                    );
                    self.seen.insert(txid);
                    continue;
                }

                let confirmations = match self.caller.get_confirmations(&bitcoin_txid).await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(txid = %hex::encode(txid), error = %e, "Confirmation fetch failed — skipping UTXO");
                        self.seen.insert(txid);
                        continue;
                    }
                };

                // Block height of the deposit tx ≈ tip - (confirmations - 1).
                let block_height = if confirmations > 0 {
                    (tip_height as u64).saturating_sub(confirmations as u64 - 1)
                } else {
                    0
                };

                info!(
                    txid = %hex::encode(txid),
                    amount_sats = %utxo.value,
                    confirmations,
                    block_height,
                    "New tunnel deposit candidate"
                );

                candidates.push(DepositCandidate {
                    txid: bitcoin_txid,
                    vout: utxo.index.saturating_to::<u32>(),
                    amount_sats: utxo.value.saturating_to::<u64>(),
                    to_address: BitcoinAddress::new(addr.clone()),
                    hemi_destination: hemi_destination.unwrap(),
                    block_height,
                    confirmations,
                });

                self.seen.insert(txid);
            }
        }

        Ok(candidates)
    }
}

/// A Bitcoin UTXO deposited to a tunnel custody address with the decoded
/// Hemi destination address from its OP_RETURN output.
#[derive(Debug, Clone)]
pub struct DepositCandidate {
    pub txid: BitcoinTxid,
    pub vout: u32,
    pub amount_sats: u64,
    pub to_address: BitcoinAddress,
    pub hemi_destination: Address,
    /// Bitcoin block height the deposit was mined in (0 if unconfirmed/unknown).
    pub block_height: u64,
    pub confirmations: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Format 1: 0x6a 0x14 <20 raw bytes> — confirmed from SimpleBitcoinVaultUTXOLogicHelper.sol
    #[test]
    fn test_format1_raw_20_bytes() {
        let mut script = vec![0x6a, 0x14]; // OP_RETURN OP_PUSHBYTES_20
        script.extend_from_slice(&[0xABu8; 20]);
        let result = parse_hemi_destination(&script).unwrap();
        assert_eq!(result.0, [0xABu8; 20]);
    }

    /// Format 2: 0x6a 0x28 <40 ASCII hex bytes> — confirmed from SimpleBitcoinVaultUTXOLogicHelper.sol
    #[test]
    fn test_format2_ascii_hex_40_bytes() {
        let addr = [0xCDu8; 20];
        let hex_str = hex::encode(addr); // "cdcdcdcd..."
        let mut script = vec![0x6a, 0x28]; // OP_RETURN OP_PUSHBYTES_40
        script.extend_from_slice(hex_str.as_bytes());
        let result = parse_hemi_destination(&script).unwrap();
        assert_eq!(result.0, addr);
    }

    #[test]
    fn test_missing_op_return_prefix_rejected() {
        // Script without 0x6a prefix
        let mut script = vec![0x14];
        script.extend_from_slice(&[0xABu8; 20]);
        assert!(parse_hemi_destination(&script).is_none());
    }

    #[test]
    fn test_unknown_length_rejected() {
        // 0x6a prefix but wrong length
        assert!(parse_hemi_destination(&[0x6a, 0x00, 0x01]).is_none());
    }
}
