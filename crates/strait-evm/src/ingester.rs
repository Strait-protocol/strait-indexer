//! EVM chain ingester for processing Hemi tunnel contract events.
//!
//! Watches for TunnelIn, TunnelOut, TunnelOutComplete, and PoP events
//! from the Hemi tunnel contracts and converts them to raw events.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use alloy::primitives::{Address as AlloyAddress, B256, U256};
use alloy::providers::Provider;
use alloy::rpc::types::Log;
use alloy::sol_types::{SolCall, SolEvent};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;
use std::collections::BTreeMap;
use tracing::{debug, error, info, instrument, warn};

use strait_core::{
    config::EvmChainConfig,
    error::{Result, StraitError},
    events::{RawEvent, HemiEvent, EthereumEvent},
    types::{Address, Asset, BitcoinTxid, Chain, ChainAddress, BitcoinAddress, TxHash},
};
use strait_store::{CheckpointRepo, Database, TunnelTransferRepo, UpsertCheckpoint};

use crate::contracts::{IBitcoinTunnelManager, IPoPPayoutsV2, IStandardBridge, topics};
use crate::reorg::ReorgDetector;

/// EVM chain ingester that watches for tunnel contract events.
pub struct EvmIngester {
    config: EvmChainConfig,
    /// Which chain this ingester serves — used as the checkpoint key.
    chain: Chain,
    provider: Arc<dyn Provider>,
    reorg_detector: ReorgDetector,
    event_tx: mpsc::Sender<RawEvent>,
    /// Store handle for reading/writing the ingestion checkpoint.
    db: Database,
    /// Cache of resolved ERC-20 symbol/decimals, keyed by token contract address —
    /// avoids re-fetching metadata for every event from the same token.
    erc20_metadata_cache: Mutex<HashMap<Address, (String, u8)>>,
}

impl EvmIngester {
    /// Create a new EVM ingester for `chain`.
    pub fn new(
        config: EvmChainConfig,
        chain: Chain,
        provider: Arc<dyn Provider>,
        db: Database,
        event_tx: mpsc::Sender<RawEvent>,
    ) -> Self {
        let reorg_detector = ReorgDetector::new(config.confirmation_depth as u64);

        Self {
            config,
            chain,
            provider,
            reorg_detector,
            event_tx,
            db,
            erc20_metadata_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Run the ingester, watching for new blocks and processing events.
    #[instrument(skip(self), fields(chain = %self.config.chain_id))]
    pub async fn run(mut self) -> Result<()> {
        info!("Starting EVM ingester for chain {}", self.config.chain_id);

        let mut last_block = self.get_start_block().await?;
        info!("Starting from block {}", last_block);

        // Exponential backoff state for rate-limit (429) errors.
        // On first 429: wait 10s. Each subsequent: double, capped at 5 minutes.
        // Resets to 0 on any successful poll.
        let mut rate_limit_backoff_secs: u64 = 0;

        loop {
            match self.process_new_blocks(&last_block).await {
                Ok(new_last) => {
                    last_block = new_last;
                    rate_limit_backoff_secs = 0;
                    sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
                }
                Err(e) => {
                    if is_rate_limit_error(&e) {
                        rate_limit_backoff_secs = if rate_limit_backoff_secs == 0 {
                            10
                        } else {
                            (rate_limit_backoff_secs * 2).min(300)
                        };
                        warn!(
                            backoff_secs = rate_limit_backoff_secs,
                            "Rate limit (429) — backing off before retry"
                        );
                        sleep(Duration::from_secs(rate_limit_backoff_secs)).await;
                    } else {
                        error!("Error processing blocks: {}", e);
                        sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }
    }

    /// Determine the block to resume from (returns the last *processed* block;
    /// the run loop continues at the next block).
    ///
    /// Priority: persisted checkpoint → configured `start_block` (backfill) →
    /// near the chain tip.
    async fn get_start_block(&mut self) -> Result<u64> {
        // 1. Resume from a persisted checkpoint if one exists.
        let repo = CheckpointRepo::new(&self.db);
        if let Some(cp) = repo.get(self.chain).await? {
            if cp.block_height > 0 {
                info!(
                    chain = %self.chain,
                    block = cp.block_height,
                    "Resuming from persisted checkpoint"
                );
                // Seed the reorg detector with the checkpoint hash so a reorg
                // that happened while the node was down is caught on the first
                // poll (the recorded hash won't match the canonical chain).
                if !cp.block_hash.is_empty() {
                    self.reorg_detector
                        .record_block_with_hash(cp.block_height as u64, cp.block_hash.clone());
                }
                return Ok(cp.block_height as u64);
            }
        }

        // 2. Backfill from a configured start block (process start_block onward).
        if self.config.start_block > 0 {
            info!(chain = %self.chain, block = self.config.start_block, "Starting backfill from configured start_block");
            return Ok(self.config.start_block.saturating_sub(1));
        }

        // 3. Otherwise start near the chain tip.
        let latest = self.provider
            .get_block_number()
            .await
            .map_err(|e| StraitError::EvmProvider(format!("Failed to get block number: {}", e)))?;
        Ok(latest.saturating_sub(self.config.confirmation_depth as u64))
    }

    /// Persist the ingestion checkpoint for this chain.
    async fn save_checkpoint(&self, block_num: u64, block_hash: B256) -> Result<()> {
        let repo = CheckpointRepo::new(&self.db);
        repo.upsert(UpsertCheckpoint {
            chain: self.chain,
            block_height: block_num as i64,
            block_hash: block_hash.to_string(),
        })
        .await?;
        Ok(())
    }

    /// Process new blocks and emit events.
    async fn process_new_blocks(&mut self, last_block: &u64) -> Result<u64> {
        let latest = self.provider
            .get_block_number()
            .await
            .map_err(|e| StraitError::EvmProvider(format!("Failed to get block number: {}", e)))?;

        let confirmed = latest.saturating_sub(self.config.confirmation_depth as u64);

        if confirmed <= *last_block {
            debug!("No new confirmed blocks (latest={}, confirmed={})", latest, confirmed);
            return Ok(*last_block);
        }

        // Reorg check: compare the recorded per-window block hashes against the
        // canonical chain. On divergence, retract affected transfers, roll the
        // checkpoint back, and resume scanning from just before the fork.
        if let Some(affected_from) = self
            .reorg_detector
            .find_reorg_point(&*self.provider)
            .await?
        {
            return self.handle_reorg_detected(affected_from).await;
        }

        // Scan in windows: one get_logs call covers up to LOG_RANGE blocks, and we only
        // fetch block headers for the (few) blocks that actually carry tunnel events.
        // This keeps RPC usage far below per-block scanning — important on rate-limited
        // public endpoints — and we checkpoint once per window (re-processing a window
        // after a crash is harmless, as all writes are idempotent upserts).
        let log_range = self.config.log_range.max(1);
        let backfilling = confirmed.saturating_sub(*last_block) > log_range;
        let mut new_last = *last_block;
        while new_last < confirmed {
            let from = new_last + 1;
            let to = (from + log_range - 1).min(confirmed);
            let to_hash = self.process_block_range(from, to).await?;
            self.save_checkpoint(to, to_hash).await?;
            self.reorg_detector
                .record_block_with_hash(to, to_hash.to_string());
            new_last = to;
            // Throttle during backfill to avoid hammering the RPC.
            if backfilling && new_last < confirmed {
                sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
            }
        }

        // Heartbeat so a caught-up node is visibly alive (range scanning is otherwise
        // silent for blocks with no tunnel events).
        info!("indexed up to block {} (chain tip {})", new_last, latest);
        Ok(new_last)
    }

    /// React to a detected reorg: emit a `BlockReorg` event (so the join engine
    /// retracts affected transfers), roll the persisted checkpoint back to just
    /// before the fork, and return the new resume block so the run loop
    /// re-scans the affected range against the canonical chain.
    async fn handle_reorg_detected(&mut self, affected_from: u64) -> Result<u64> {
        let resume = affected_from.saturating_sub(1);
        let depth = self
            .reorg_detector
            .highest_processed()
            .unwrap_or(affected_from)
            .saturating_sub(resume) as u32;

        warn!(
            chain = %self.chain,
            affected_from,
            resume,
            depth,
            "Reorg detected — retracting affected transfers and re-scanning"
        );

        // Best-effort tip hashes for the reorg event (informational).
        let new_tip = self
            .fetch_block_header(resume)
            .await
            .map(|(h, _)| strait_core::types::BlockHash(h.0))
            .unwrap_or(strait_core::types::BlockHash([0u8; 32]));
        let old_tip = strait_core::types::BlockHash([0u8; 32]);

        let event = match self.chain {
            Chain::Hemi => RawEvent::Hemi(HemiEvent::BlockReorg {
                old_tip,
                new_tip,
                depth,
                affected_from_block: affected_from,
            }),
            Chain::Ethereum => RawEvent::Ethereum(EthereumEvent::BlockReorg {
                old_tip,
                new_tip,
                depth,
                affected_from_block: affected_from,
            }),
            Chain::Bitcoin => {
                return Err(StraitError::Internal(
                    "EvmIngester cannot serve the Bitcoin chain".into(),
                ))
            }
        };
        self.event_tx
            .send(event)
            .await
            .map_err(|_| StraitError::Internal("event channel closed".into()))?;

        // Roll the persisted checkpoint back so a crash mid-rescan resumes
        // from before the fork, then drop the invalidated detector entries.
        let resume_hash = self
            .fetch_block_header(resume)
            .await
            .map(|(h, _)| h.to_string())
            .unwrap_or_default();
        let repo = CheckpointRepo::new(&self.db);
        repo.rollback(self.chain, resume as i64, &resume_hash).await?;
        self.reorg_detector.handle_reorg(affected_from).await?;
        if !resume_hash.is_empty() {
            self.reorg_detector
                .record_block_with_hash(resume, resume_hash);
        }

        Ok(resume)
    }

    /// Scan a window of blocks `[from, to]` with a single get_logs call, process any
    /// tunnel events in chain order, and return the hash of block `to` (for the checkpoint).
    #[instrument(skip(self), fields(from = from, to = to))]
    async fn process_block_range(&self, from: u64, to: u64) -> Result<B256> {
        let mut logs = self.get_tunnel_logs(from, to).await?;
        // get_logs returns in chain order, but sort defensively before processing.
        logs.sort_by_key(|l| (l.block_number.unwrap_or(0), l.log_index.unwrap_or(0)));
        if !logs.is_empty() {
            info!("found {} tunnel event(s) in blocks {}-{}", logs.len(), from, to);
        }

        // Fetch each relevant block's header once (timestamp + hash). Most blocks in the
        // window carry no tunnel events, so this is a handful of calls, not one per block.
        let mut headers: BTreeMap<u64, (B256, DateTime<Utc>)> = BTreeMap::new();
        for log in &logs {
            let bn = log.block_number.unwrap_or(0);
            if let std::collections::btree_map::Entry::Vacant(e) = headers.entry(bn) {
                e.insert(self.fetch_block_header(bn).await?);
            }
        }

        for log in logs {
            let bn = log.block_number.unwrap_or(0);
            let (block_hash, block_time) =
                headers.get(&bn).copied().unwrap_or((B256::ZERO, Utc::now()));
            self.process_log(log, bn, block_hash, block_time).await?;
        }

        // The checkpoint needs the hash of `to`; reuse it if it carried events, else fetch.
        match headers.get(&to) {
            Some((hash, _)) => Ok(*hash),
            None => Ok(self.fetch_block_header(to).await?.0),
        }
    }

    /// Fetch a single block's hash and timestamp.
    async fn fetch_block_header(&self, block_num: u64) -> Result<(B256, DateTime<Utc>)> {
        let block = self.provider
            .get_block_by_number(block_num.into(), false)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("Failed to get block: {}", e)))?
            .ok_or_else(|| StraitError::EvmProvider(format!("Block {} not found", block_num)))?;
        let hash = block.header.hash;
        let time = DateTime::from_timestamp(block.header.timestamp as i64, 0)
            .unwrap_or_else(Utc::now);
        Ok((hash, time))
    }

    /// Fetch the gas fee spent on a transaction (wei = gasUsed * effectiveGasPrice).
    /// Best-effort — returns None if the receipt can't be fetched.
    async fn fetch_gas_fee(&self, tx_hash: B256) -> Option<bigdecimal::BigDecimal> {
        let receipt = self.provider.get_transaction_receipt(tx_hash).await.ok()??;
        let fee = U256::from(receipt.gas_used)
            .checked_mul(U256::from(receipt.effective_gas_price))?;
        u256_to_bigdecimal(fee).ok()
    }

    /// Resolve an ERC-20 token's symbol and decimals from its contract, caching the
    /// result so repeat events for the same token don't re-hit the RPC. Falls back to
    /// an empty symbol / 18 decimals (best-effort) if either call fails — e.g. a
    /// nonstandard token or a transient RPC error shouldn't block ingestion.
    async fn resolve_erc20_metadata(&self, contract: Address) -> (String, u8) {
        if let Some(cached) = self.erc20_metadata_cache.lock().await.get(&contract) {
            return cached.clone();
        }

        let token_addr = AlloyAddress::from(contract.0);
        let symbol = crate::contracts::fetch_erc20_symbol(&*self.provider, token_addr)
            .await
            .unwrap_or_default();
        let decimals = crate::contracts::fetch_erc20_decimals(&*self.provider, token_addr)
            .await
            .unwrap_or(18);

        let metadata = (symbol, decimals);
        self.erc20_metadata_cache
            .lock()
            .await
            .insert(contract, metadata.clone());
        metadata
    }

    /// Get tunnel-contract logs across a window of blocks `[from_block, to_block]`.
    async fn get_tunnel_logs(&self, from_block: u64, to_block: u64) -> Result<Vec<Log>> {
        // Watch the ETH/ERC-20 tunnel (L2StandardBridge / L1 bridge) and, on Hemi,
        // the BTC tunnel (BitcoinTunnelManager) — DepositConfirmed / WithdrawalInitiated
        // for BTC routes are emitted there, not on the standard bridge.
        let mut addresses: Vec<AlloyAddress> = Vec::with_capacity(2);
        addresses.push(
            self.config.tunnel_contract.parse().map_err(|e| {
                StraitError::Parse(format!("Invalid tunnel contract address: {}", e))
            })?,
        );
        if let Some(btc) = self.config.btc_tunnel_contract.as_deref() {
            if !btc.is_empty() {
                addresses.push(btc.parse().map_err(|e| {
                    StraitError::Parse(format!("Invalid BTC tunnel contract address: {}", e))
                })?);
            }
        }
        if let Some(portal) = self.config.opt_portal_contract.as_deref() {
            if !portal.is_empty() {
                addresses.push(portal.parse().map_err(|e| {
                    StraitError::Parse(format!("Invalid OptimismPortal contract address: {}", e))
                })?);
            }
        }
        if let Some(pop) = self.config.pop_payouts_contract.as_deref() {
            if !pop.is_empty() {
                addresses.push(pop.parse().map_err(|e| {
                    StraitError::Parse(format!("Invalid PoPPayoutsV2 contract address: {}", e))
                })?);
            }
        }

        let filter = alloy::rpc::types::Filter::new()
            .address(addresses)
            .from_block(from_block)
            .to_block(to_block);
        
        let logs = self.provider
            .get_logs(&filter)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("Failed to get logs: {}", e)))?;
        
        Ok(logs)
    }

    /// Process a single log entry.
    ///
    /// Dispatches to either the OP Stack StandardBridge handlers (ETH/ERC-20
    /// routes) or the BTC tunnel handlers depending on the event topic.
    async fn process_log(
        &self,
        log: Log,
        block_num: u64,
        _block_hash: B256,
        block_time: DateTime<Utc>,
    ) -> Result<()> {
        let tx_hash = log.transaction_hash
            .ok_or_else(|| StraitError::Parse("Missing transaction hash".into()))?;

        let log_index = log.log_index.unwrap_or(0) as u32;
        let log_topics: Vec<B256> = log.topics().to_vec();
        let data = &log.data().data;

        if log_topics.is_empty() {
            return Ok(());
        }

        let topic0 = log_topics[0];

        // ── OP Stack StandardBridge events (ETH / ERC-20 routes) ─────────────
        //
        // alloy-sol-types 0.3 exposes `SolEvent::decode_data(data, validate)`
        // which decodes only the non-indexed portion of a log. Indexed fields
        // are extracted manually from the topics array.

        if topic0 == topics::eth_bridge_finalized() {
            // ETHBridgeFinalized(indexed from, indexed to, uint256 amount, bytes extraData)
            if let (Some(from_t), Some(to_t)) = (log_topics.get(1), log_topics.get(2)) {
                let from = addr_from_topic(from_t);
                let to   = addr_from_topic(to_t);
                if let Ok(decoded) = IStandardBridge::ETHBridgeFinalized::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    self.handle_eth_deposit(from, to, decoded.amount, tx_hash, block_num, log_index, block_time).await?;
                }
            }
        } else if topic0 == topics::eth_bridge_initiated() {
            // ETHBridgeInitiated(indexed from, indexed to, uint256 amount, bytes extraData)
            if let (Some(from_t), Some(to_t)) = (log_topics.get(1), log_topics.get(2)) {
                let from = addr_from_topic(from_t);
                let to   = addr_from_topic(to_t);
                if let Ok(decoded) = IStandardBridge::ETHBridgeInitiated::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    self.handle_eth_withdrawal(from, to, decoded.amount, tx_hash, block_num, log_index, block_time).await?;
                }
            }
        } else if topic0 == topics::erc20_bridge_finalized() {
            // ERC20BridgeFinalized(indexed localToken, indexed remoteToken, indexed from,
            //                      address to, uint256 amount, bytes extraData)
            if let (Some(lt), Some(rt), Some(ft)) =
                (log_topics.get(1), log_topics.get(2), log_topics.get(3))
            {
                let local_token  = addr_from_topic(lt);
                let remote_token = addr_from_topic(rt);
                let from         = addr_from_topic(ft);
                if let Ok(decoded) = IStandardBridge::ERC20BridgeFinalized::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    let to = Address(decoded.to.into());
                    self.handle_erc20_deposit(
                        local_token, remote_token, from, to,
                        decoded.amount, tx_hash, block_num, log_index, block_time,
                    ).await?;
                }
            }
        } else if topic0 == topics::erc20_bridge_initiated() {
            // ERC20BridgeInitiated(indexed localToken, indexed remoteToken, indexed from,
            //                      address to, uint256 amount, bytes extraData)
            if let (Some(lt), Some(rt), Some(ft)) =
                (log_topics.get(1), log_topics.get(2), log_topics.get(3))
            {
                let local_token  = addr_from_topic(lt);
                let remote_token = addr_from_topic(rt);
                let from         = addr_from_topic(ft);
                if let Ok(decoded) = IStandardBridge::ERC20BridgeInitiated::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    let to = Address(decoded.to.into());
                    self.handle_erc20_withdrawal(
                        local_token, remote_token, from, to,
                        decoded.amount, tx_hash, block_num, log_index, block_time,
                    ).await?;
                }
            }

        // ── BitcoinTunnelManager events (BTC routes) ──────────────────────────
        // Confirmed from hemilabs/bitcoin-tunnel-contracts source.

        } else if topic0 == topics::deposit_confirmed() {
            // DepositConfirmed(indexed vault, indexed recipient, indexed depositTxId,
            //                  uint256 depositSats, uint256 netSatsAfterFee)
            // depositTxId is the Bitcoin txid — the primary cross-chain join key.
            if let (Some(vault_t), Some(recipient_t), Some(txid_t)) =
                (log_topics.get(1), log_topics.get(2), log_topics.get(3))
            {
                let recipient = addr_from_topic(recipient_t);
                let deposit_tx_id: [u8; 32] = txid_t.0;
                if let Ok(decoded) = IBitcoinTunnelManager::DepositConfirmed::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    self.handle_btc_deposit_confirmed(
                        addr_from_topic(vault_t), recipient, deposit_tx_id,
                        decoded.netSatsAfterFee, tx_hash, block_num, log_index, block_time,
                    ).await?;
                }
            }
        } else if topic0 == topics::btc_withdrawal_initiated() {
            // WithdrawalInitiated(indexed vault, indexed withdrawer, indexed btcAddress,
            //                     uint256 withdrawalSats, uint256 netSatsAfterFee, uint64 uuid)
            // Note: btcAddress is indexed so it is keccak256-hashed in topics[3] —
            // the original string cannot be recovered from the log. The uuid in the
            // non-indexed data is the correct cross-chain join key for this withdrawal.
            if let Some(withdrawer_t) = log_topics.get(2) {
                let withdrawer = addr_from_topic(withdrawer_t);
                if let Ok(decoded) = IBitcoinTunnelManager::WithdrawalInitiated::decode_raw_log(
                    log_topics.iter().copied(), data, false,
                ) {
                    self.handle_btc_withdrawal_initiated(
                        withdrawer, decoded.netSatsAfterFee, decoded.uuid,
                        tx_hash, block_num, log_index, block_time,
                    ).await?;
                }
            }
        } else if topic0 == topics::withdrawal_challenge_success() {
            // WithdrawalChallengeSuccess(indexed vault, indexed withdrawer, indexed uuid)
            // Operator failed to pay within the deadline — hBTC re-minted to withdrawer.
            // uuid is indexed (uint64 in topic[3]).
            if let Some(withdrawer_t) = log_topics.get(2) {
                let withdrawer = addr_from_topic(withdrawer_t);
                let uuid = log_topics.get(3)
                    .map(|t| u64::from_be_bytes(t.0[24..32].try_into().unwrap_or([0u8; 8])))
                    .unwrap_or(0);
                info!(
                    uuid,
                    withdrawer = %hex::encode(withdrawer.0),
                    tx = %hex::encode(tx_hash.0),
                    "WithdrawalChallengeSuccess — operator failed to pay, withdrawal FAILED"
                );
                self.event_tx.send(RawEvent::Hemi(HemiEvent::WithdrawalChallengeSuccess {
                    uuid,
                    withdrawer,
                    tx_hash: TxHash(tx_hash.0),
                    block_number: block_num,
                    block_time,
                    log_index,
                })).await.map_err(|_| StraitError::Internal("event channel closed".into()))?;
            }

        // ── PoPPayoutsV2 events ────────────────────────────────────────────────

        } else if topic0 == topics::payout_round_executed() {
            // PayoutRoundExecuted(indexed blockRewarded, uint256 rewardPool, uint256 popScore)
            // All Hemi blocks in (blockRewarded-25, blockRewarded] are now PoP-anchored.
            let keystone_block = log_topics.get(1)
                .map(|t| u64::from_be_bytes(t.0[24..32].try_into().unwrap_or([0u8; 8])))
                .unwrap_or(0);
            if let Ok(decoded) = IPoPPayoutsV2::PayoutRoundExecuted::decode_raw_log(
                log_topics.iter().copied(), data, false,
            ) {
                self.handle_keystone_anchored(
                    keystone_block,
                    decoded.popScore.saturating_to::<u64>(),
                    tx_hash,
                    block_num,
                ).await?;
            }
        } else if topic0 == topics::vault_created() {
            // VaultCreated(indexed setupAdmin, indexed operatorAdmin, indexed vaultAddress)
            // Track new vaults so the CustodyWatcher can watch their Bitcoin addresses.
            if let Some(vault_t) = log_topics.get(3) {
                let vault_address = addr_from_topic(vault_t);
                info!(vault = %hex::encode(vault_address.0), "New BTC tunnel vault created — add to watched addresses");
            }

        // ── OptimismPortal events (HEMI_TO_ETH two-step withdrawal) ──────────
        //
        // WithdrawalProven fires when proveWithdrawalTransaction is called, starting
        // the 1-day OP Stack challenge window. Only watched when ETH_OPT_PORTAL_CONTRACT
        // is configured (not a standard bridge address).

        } else if topic0 == topics::withdrawal_proven() {
            // WithdrawalProven(indexed bytes32 withdrawalHash, indexed address from, indexed address to)
            if let (Some(hash_t), Some(from_t), Some(to_t)) =
                (log_topics.get(1), log_topics.get(2), log_topics.get(3))
            {
                let withdrawal_hash = TxHash(hash_t.0);
                let from = addr_from_topic(from_t);
                let to   = addr_from_topic(to_t);
                self.handle_withdrawal_proven(withdrawal_hash, from, to, tx_hash, block_num, log_index, block_time).await?;
            }
        } else {
            debug!("Unknown event topic {:?}, skipping", topic0);
        }

        Ok(())
    }

    // ── StandardBridge handlers (ETH / ERC-20 routes) ────────────────────────

    async fn handle_eth_deposit(
        &self,
        from: Address,
        to: Address,
        amount: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
        block_time: DateTime<Utc>,
    ) -> Result<()> {
        info!(from = %hex::encode(from.0), to = %hex::encode(to.0), %amount, "ETHBridgeFinalized (deposit on Hemi)");
        let gas_fee = self.fetch_gas_fee(tx_hash).await;
        let amount_bd = u256_to_bigdecimal(amount)?;
        let event = if self.chain == Chain::Ethereum {
            // On L1, ETHBridgeFinalized is the withdrawal release — funds returned to the user.
            // First emit the TunnelRelease event so the in-memory matcher can finalize same-session
            // withdrawals (where the Hemi burn and the L1 release happen without a node restart).
            let release = RawEvent::Ethereum(EthereumEvent::TunnelRelease {
                tx_hash: TxHash(tx_hash.0),
                asset: Asset::Eth,
                amount: amount_bd.clone(),
                to,
                block_number: block_num,
                block_time,
                log_index,
                gas_fee: gas_fee.clone(),
            });

            // DB-backed fallback: if the Hemi burn happened in a previous node session (common
            // — the OP Stack challenge period is ~1 day), the in-memory matcher's pending_hemi_burns
            // map is empty and the TunnelRelease would be silently dropped.  Query the DB directly
            // so restarts don't permanently strand Hemi→ETH withdrawals at INITIATED.
            let recipient = format!("0x{}", hex::encode(to.0));
            let dest_tx_hex = format!("0x{}", hex::encode(tx_hash.0));
            let repo = TunnelTransferRepo::new(&self.db);
            match repo.find_pending_eth_withdrawal(&recipient, &amount_bd).await {
                Ok(Some(transfer)) => {
                    match repo
                        .set_eth_withdrawal_finalized(
                            transfer.id,
                            &dest_tx_hex,
                            Some(block_num as i64),
                            gas_fee,
                            block_time,
                        )
                        .await
                    {
                        Ok(()) => info!(
                            transfer_id = %transfer.id,
                            dest_tx = %dest_tx_hex,
                            block = block_num,
                            "Hemi→ETH withdrawal FINALIZED via DB-backed L1 release match"
                        ),
                        Err(e) => warn!(error = %e, "DB finalization write failed for ETH withdrawal"),
                    }
                }
                Ok(None) => {
                    // No pending INITIATED transfer found — either it was already finalized
                    // by the in-memory matcher this session, or this is an unknown release.
                    debug!(recipient = %recipient, amount = %amount_bd, "No pending HEMI_TO_ETH transfer found for L1 release");
                }
                Err(e) => warn!(error = %e, "DB lookup failed for pending ETH withdrawal"),
            }

            release
        } else {
            RawEvent::Hemi(HemiEvent::TunnelMint {
                tx_hash: TxHash(tx_hash.0),
                asset: Asset::Eth,
                amount: amount_bd,
                to,
                source_txid: None,
                block_number: block_num,
                block_time,
                log_index,
                gas_fee,
            })
        };
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    async fn handle_eth_withdrawal(
        &self,
        from: Address,
        to: Address,
        amount: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
        block_time: DateTime<Utc>,
    ) -> Result<()> {
        info!(from = %hex::encode(from.0), to = %hex::encode(to.0), %amount, "ETHBridgeInitiated (withdrawal from Hemi)");
        let gas_fee = self.fetch_gas_fee(tx_hash).await;
        let amount_bd = u256_to_bigdecimal(amount)?;
        let event = if self.chain == Chain::Ethereum {
            // On L1, an "initiated" bridge event is a deposit being locked for L2.
            RawEvent::Ethereum(EthereumEvent::TunnelLock {
                tx_hash: TxHash(tx_hash.0),
                asset: Asset::Eth,
                amount: amount_bd,
                from,
                block_number: block_num,
                block_time,
                log_index,
                gas_fee,
            })
        } else {
            RawEvent::Hemi(HemiEvent::TunnelBurn {
                tx_hash: TxHash(tx_hash.0),
                asset: Asset::Eth,
                amount: amount_bd,
                from,
                destination: ChainAddress::Evm(to),
                block_number: block_num,
                block_time,
                log_index,
                gas_fee,
                uuid: None,
            })
        };
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    async fn handle_erc20_deposit(
        &self,
        local_token: Address,
        _remote_token: Address,
        from: Address,
        to: Address,
        amount: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
        block_time: DateTime<Utc>,
    ) -> Result<()> {
        info!(
            token = %hex::encode(local_token.0),
            from = %hex::encode(from.0),
            to = %hex::encode(to.0),
            %amount,
            "ERC20BridgeFinalized (deposit on Hemi)"
        );
        let gas_fee = self.fetch_gas_fee(tx_hash).await;
        let amount_bd = u256_to_bigdecimal(amount)?;
        let (symbol, decimals) = self.resolve_erc20_metadata(local_token).await;
        let asset = Asset::Erc20 { contract: local_token, symbol, decimals };
        let event = if self.chain == Chain::Ethereum {
            // On L1, a "finalized" bridge event is a withdrawal released to the user.
            RawEvent::Ethereum(EthereumEvent::TunnelRelease {
                tx_hash: TxHash(tx_hash.0),
                asset,
                amount: amount_bd,
                to,
                block_number: block_num,
                block_time,
                log_index,
                gas_fee,
            })
        } else {
            RawEvent::Hemi(HemiEvent::TunnelMint {
                tx_hash: TxHash(tx_hash.0),
                asset,
                amount: amount_bd,
                to,
                source_txid: None,
                block_number: block_num,
                block_time,
                log_index,
                gas_fee,
            })
        };
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    async fn handle_erc20_withdrawal(
        &self,
        local_token: Address,
        _remote_token: Address,
        from: Address,
        to: Address,
        amount: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
        block_time: DateTime<Utc>,
    ) -> Result<()> {
        info!(
            token = %hex::encode(local_token.0),
            from = %hex::encode(from.0),
            to = %hex::encode(to.0),
            %amount,
            "ERC20BridgeInitiated (withdrawal from Hemi)"
        );
        let gas_fee = self.fetch_gas_fee(tx_hash).await;
        let amount_bd = u256_to_bigdecimal(amount)?;
        let (symbol, decimals) = self.resolve_erc20_metadata(local_token).await;
        let asset = Asset::Erc20 { contract: local_token, symbol, decimals };
        let event = if self.chain == Chain::Ethereum {
            // On L1, an "initiated" bridge event is a deposit being locked for L2.
            RawEvent::Ethereum(EthereumEvent::TunnelLock {
                tx_hash: TxHash(tx_hash.0),
                asset,
                amount: amount_bd,
                from,
                block_number: block_num,
                block_time,
                log_index,
                gas_fee,
            })
        } else {
            RawEvent::Hemi(HemiEvent::TunnelBurn {
                tx_hash: TxHash(tx_hash.0),
                asset,
                amount: amount_bd,
                from,
                destination: ChainAddress::Evm(to),
                block_number: block_num,
                block_time,
                log_index,
                gas_fee,
                uuid: None,
            })
        };
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    // ── PoPPayoutsV2 handler ──────────────────────────────────────────────────

    /// Handle PayoutRoundExecuted — signals that all Hemi blocks in
    /// (keystone_block - 25, keystone_block] are PoP-anchored on Bitcoin.
    async fn handle_keystone_anchored(
        &self,
        keystone_block: u64,
        pop_score: u64,
        tx_hash: B256,
        block_num: u64,
    ) -> Result<()> {
        info!(
            keystone_block,
            pop_score,
            "PayoutRoundExecuted — Hemi blocks anchored on Bitcoin"
        );
        let event = RawEvent::Hemi(HemiEvent::PopKeystoneAnchored {
            hemi_tx_hash: TxHash(tx_hash.0),
            keystone_block,
            reward_pool: 0,
            pop_score,
            block_number: block_num,
            log_index: 0,
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    // ── BitcoinTunnelManager handlers (BTC routes) ────────────────────────────

    /// Handle DepositConfirmed (BTC → Hemi): Bitcoin deposit confirmed, hBTC minted.
    /// `deposit_tx_id` is the Bitcoin txid — primary cross-chain join key.
    async fn handle_btc_deposit_confirmed(
        &self,
        _vault: Address,
        recipient: Address,
        deposit_tx_id: [u8; 32],
        net_sats: U256,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
        block_time: DateTime<Utc>,
    ) -> Result<()> {
        info!(
            recipient = %hex::encode(recipient.0),
            bitcoin_txid = %hex::encode(deposit_tx_id),
            net_sats = %net_sats,
            "DepositConfirmed — BTC deposited and hBTC minted on Hemi"
        );
        let event = RawEvent::Hemi(HemiEvent::TunnelMint {
            tx_hash: TxHash(tx_hash.0),
            asset: Asset::Btc,
            amount: u256_to_bigdecimal(net_sats)?,
            to: recipient,
            source_txid: Some(BitcoinTxid(deposit_tx_id)),
            block_number: block_num,
            block_time,
            log_index,
            gas_fee: self.fetch_gas_fee(tx_hash).await,
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    /// Handle WithdrawalInitiated (Hemi → BTC): hBTC burned, BTC payout pending.
    ///
    /// The `btcAddress` field in the event is `indexed`, so it is keccak256-hashed
    /// in the topic and the original string is unrecoverable from the log. The `uuid`
    /// (vaultIndex << 32 | vaultSpecificUUID) is the cross-chain join key: the
    /// Bitcoin payout transaction includes an OP_RETURN encoding this uuid.
    async fn handle_btc_withdrawal_initiated(
        &self,
        withdrawer: Address,
        net_sats: U256,
        uuid: u64,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
        block_time: DateTime<Utc>,
    ) -> Result<()> {
        // The btcAddress is `indexed` in the event (only a keccak hash survives in the
        // topic), but the originating initiateWithdrawal(uint32,string,uint256) call
        // carries it in cleartext — recover it from the transaction calldata so the
        // transfer has the real Bitcoin recipient (and the Bitcoin payout can be matched).
        let destination = match self.recover_withdrawal_btc_address(tx_hash).await {
            Some(addr) => ChainAddress::Bitcoin(BitcoinAddress::new(addr)),
            None => ChainAddress::Bitcoin(BitcoinAddress::new(format!("withdrawal-uuid-{uuid}"))),
        };

        info!(
            withdrawer = %hex::encode(withdrawer.0),
            %net_sats,
            uuid,
            recipient = ?destination,
            "WithdrawalInitiated — hBTC burned, BTC payout registered"
        );
        let event = RawEvent::Hemi(HemiEvent::TunnelBurn {
            tx_hash: TxHash(tx_hash.0),
            asset: Asset::Btc,
            amount: u256_to_bigdecimal(net_sats)?,
            from: withdrawer,
            destination,
            block_number: block_num,
            block_time,
            log_index,
            gas_fee: self.fetch_gas_fee(tx_hash).await,
            uuid: Some(uuid),
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))
    }

    /// Recover the plaintext BTC withdrawal address from the originating
    /// `initiateWithdrawal` transaction's calldata. Returns `None` if the tx can't
    /// be fetched or its calldata isn't a recognised `initiateWithdrawal` call.
    async fn recover_withdrawal_btc_address(&self, tx_hash: B256) -> Option<String> {
        let tx = self.provider.get_transaction_by_hash(tx_hash).await.ok()??;
        let decoded =
            IBitcoinTunnelManager::initiateWithdrawalCall::abi_decode(tx.input.as_ref(), false)
                .ok()?;
        Some(decoded.btcAddress)
    }

    // ── OptimismPortal handler (HEMI_TO_ETH two-step withdrawal) ─────────────

    /// Handle WithdrawalProven — `proveWithdrawalTransaction` was called on OptimismPortal,
    /// starting the 1-day OP Stack challenge window for a HEMI_TO_ETH withdrawal.
    ///
    /// Emits a `WithdrawalProven` event to the channel and directly advances the
    /// matching DB transfer from INITIATED → PROVING.
    ///
    /// `WithdrawalProven` carries no amount, so matching is by `to` (recipient) address
    /// only — FIFO ordering (oldest INITIATED first) handles same-user concurrent
    /// withdrawals correctly.
    async fn handle_withdrawal_proven(
        &self,
        withdrawal_hash: TxHash,
        from: Address,
        to: Address,
        tx_hash: B256,
        block_num: u64,
        log_index: u32,
        block_time: DateTime<Utc>,
    ) -> Result<()> {
        info!(
            withdrawal_hash = %withdrawal_hash,
            from = %hex::encode(from.0),
            to   = %hex::encode(to.0),
            "WithdrawalProven — OP Stack 1-day challenge window started"
        );

        let event = RawEvent::Ethereum(EthereumEvent::WithdrawalProven {
            withdrawal_hash,
            from,
            to,
            tx_hash: TxHash(tx_hash.0),
            block_number: block_num,
            block_time,
            log_index,
        });
        self.event_tx.send(event).await
            .map_err(|e| StraitError::Internal(format!("Failed to send event: {}", e)))?;

        // DB-backed advance: find the oldest INITIATED HEMI_TO_ETH withdrawal for
        // this recipient and advance it to PROVING.
        let recipient = format!("0x{}", hex::encode(to.0));
        let repo = TunnelTransferRepo::new(&self.db);
        match repo.find_eth_withdrawal_for_proving(&recipient).await {
            Ok(Some(transfer)) => {
                match repo.set_eth_withdrawal_proving(transfer.id, block_time).await {
                    Ok(()) => info!(
                        transfer_id = %transfer.id,
                        block = block_num,
                        "HEMI_TO_ETH withdrawal INITIATED → PROVING (challenge window open)"
                    ),
                    Err(e) => warn!(error = %e, "DB proving write failed for ETH withdrawal"),
                }
            }
            Ok(None) => {
                debug!(
                    recipient = %recipient,
                    "WithdrawalProven: no INITIATED HEMI_TO_ETH withdrawal found in DB"
                );
            }
            Err(e) => warn!(error = %e, "DB lookup failed for withdrawal proving"),
        }

        Ok(())
    }
}

/// Returns true if the error is an HTTP 429 / rate-limit response from the RPC.
/// Used to distinguish rate limiting from real failures so we can back off properly.
fn is_rate_limit_error(e: &StraitError) -> bool {
    let s = e.to_string();
    s.contains("429") || s.contains("rate limit") || s.contains("rate_limit") || s.contains("-32005")
}

/// Extract an EVM address from a 32-byte log topic (right-padded to 32 bytes).
fn addr_from_topic(topic: &alloy::primitives::B256) -> Address {
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&topic.0[12..32]);
    Address(addr)
}

/// Convert U256 to BigDecimal.
fn u256_to_bigdecimal(value: U256) -> Result<BigDecimal> {
    let s = value.to_string();
    s.parse::<BigDecimal>()
        .map_err(|e| StraitError::Parse(format!("Failed to parse U256 as BigDecimal: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_u256_to_bigdecimal() {
        let value = U256::from(100000000u64); // 1 BTC in satoshis
        let bd = u256_to_bigdecimal(value).unwrap();
        assert_eq!(bd, BigDecimal::from(100000000u64));
    }
}