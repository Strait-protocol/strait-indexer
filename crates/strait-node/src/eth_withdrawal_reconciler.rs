//! Hemi→ETH withdrawal reconciler: advances HEMI_TO_ETH withdrawals from
//! INITIATED → PROVING once they're proven on the L1 OptimismPortal.
//!
//! The OP Stack identifies a withdrawal only by its `withdrawalHash`, which is
//! emitted on Hemi by the L2ToL1MessagePasser's `MessagePassed` log in the same tx
//! as the bridge's `ETHBridgeInitiated` / `ERC20BridgeInitiated`. The L1-side
//! `WithdrawalProven(hash, from, to)` can't be matched by address: `from`/`to` are
//! always the L2/L1 CrossDomainMessengers, never the user.
//!
//! Each tick:
//!  1. **Hash fill**: for pending withdrawals without a `withdrawal_hash`, read the
//!     Hemi tx receipt and record the `MessagePassed` withdrawal hash.
//!  2. **Reconcile**: for pending withdrawals with a hash (least-recently checked
//!     first), call `provenWithdrawals(hash)` → PROVING (with `proven_at`), then
//!     `finalizedWithdrawals(hash)` → locate the `WithdrawalFinalized` log by hash
//!     and settle as FINALIZED (or FAILED if the relay reverted) with the claim tx
//!     as the destination leg. Each transition enqueues `transfer.status_changed`.
//!
//! State-based rather than event-based on purpose: it backfills withdrawals proven
//! before this ran, and a missed log or a node restart can't strand a transfer.
//! This is exact (keyed on the hash) and complements the recipient + amount match
//! on L1StandardBridge `*BridgeFinalized`, which misses some claims and left
//! transfers stuck in PROVING.

use std::sync::Arc;
use std::time::Duration;

use alloy::primitives::{Address, B256};
use std::str::FromStr;

use alloy::providers::Provider;
use alloy::eips::BlockId;
use alloy::rpc::types::{Filter, TransactionRequest};
use alloy::sol_types::{SolCall, SolEvent};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use strait_core::error::{Result, StraitError};
use strait_evm::contracts::{IL2ToL1MessagePasser, IOptimismPortal, L2_TO_L1_MESSAGE_PASSER};
use strait_store::{Database, TunnelTransferRepo};

/// Max withdrawals handled per phase per tick — bounds RPC usage while a large
/// backlog (e.g. the first run after deploy) drains over several ticks.
const BATCH: i64 = 100;
/// Spacing between consecutive RPC calls, to stay under public-RPC rate limits.
const CALL_SPACING: Duration = Duration::from_millis(150);

/// An L1 `WithdrawalFinalized` occurrence for one withdrawal.
struct Finalization {
    success: bool,
    tx_hash: B256,
    block: u64,
    time: DateTime<Utc>,
    fee: Option<BigDecimal>,
}

pub struct EthWithdrawalReconciler {
    hemi_provider: Arc<dyn Provider>,
    eth_provider: Arc<dyn Provider>,
    portal: Address,
    message_passer: Address,
    db: Database,
    poll_interval: Duration,
}

impl EthWithdrawalReconciler {
    pub fn new(
        hemi_provider: Arc<dyn Provider>,
        eth_provider: Arc<dyn Provider>,
        portal: Address,
        db: Database,
        poll_interval: Duration,
    ) -> Self {
        Self {
            hemi_provider,
            eth_provider,
            portal,
            message_passer: L2_TO_L1_MESSAGE_PASSER.parse().expect("valid predeploy address"),
            db,
            poll_interval,
        }
    }

    pub async fn run(self) -> Result<()> {
        info!(
            portal = %self.portal,
            poll_secs = self.poll_interval.as_secs(),
            "Hemi→ETH withdrawal reconciler started"
        );
        loop {
            if let Err(e) = self.fill_withdrawal_hashes().await {
                warn!("withdrawal hash fill failed: {e}");
            }
            if let Err(e) = self.reconcile_pending().await {
                warn!("withdrawal reconcile failed: {e}");
            }
            sleep(self.poll_interval).await;
        }
    }

    /// Phase 1: record `withdrawal_hash` from each pending withdrawal's Hemi receipt.
    async fn fill_withdrawal_hashes(&self) -> Result<()> {
        let repo = TunnelTransferRepo::new(&self.db);
        let pending = repo.eth_withdrawals_missing_hash(BATCH).await?;
        if pending.is_empty() {
            return Ok(());
        }
        let mut filled = 0usize;
        for w in &pending {
            sleep(CALL_SPACING).await;
            let Ok(tx_hash) = w.source_tx_hash.parse::<B256>() else {
                debug!(transfer = %w.id, tx = %w.source_tx_hash, "unparseable source tx hash");
                continue;
            };
            match self.withdrawal_hash_for_tx(tx_hash).await {
                Ok(Some(hash)) => {
                    repo.set_withdrawal_hash(w.id, &format!("{hash:#x}")).await?;
                    filled += 1;
                }
                Ok(None) => debug!(transfer = %w.id, "no unique MessagePassed log in receipt yet"),
                Err(e) => debug!(transfer = %w.id, "receipt lookup failed: {e}"),
            }
        }
        if filled > 0 {
            info!(filled, of = pending.len(), "recorded Hemi→ETH withdrawal hashes");
        }
        Ok(())
    }

    /// The `withdrawalHash` from the single `MessagePassed` log in a Hemi tx.
    /// `None` when the receipt isn't available yet, or when the tx passed zero or
    /// several messages (a batched tx — ambiguous which one is this transfer).
    async fn withdrawal_hash_for_tx(&self, tx_hash: B256) -> Result<Option<B256>> {
        let receipt = self
            .hemi_provider
            .get_transaction_receipt(tx_hash)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("get_transaction_receipt: {e}")))?;
        let Some(receipt) = receipt else { return Ok(None) };

        let hashes: Vec<B256> = receipt
            .inner
            .logs()
            .iter()
            .filter(|l| l.address() == self.message_passer)
            .filter_map(|l| l.log_decode::<IL2ToL1MessagePasser::MessagePassed>().ok())
            .map(|l| l.inner.data.withdrawalHash)
            .collect();
        match hashes.as_slice() {
            [h] => Ok(Some(*h)),
            [] => Ok(None),
            many => {
                warn!(tx = %tx_hash, count = many.len(), "multiple MessagePassed logs in one tx — skipping");
                Ok(None)
            }
        }
    }

    /// Phase 2: bring each pending withdrawal up to date with the L1 portal —
    /// INITIATED → PROVING when proven, then → FINALIZED / FAILED when claimed.
    /// Rows rotate by `reconciled_at`, so a backlog larger than one batch (e.g.
    /// never-proven withdrawals, ~27% of all) is still fully revisited.
    async fn reconcile_pending(&self) -> Result<()> {
        let repo = TunnelTransferRepo::new(&self.db);
        let rows = repo.eth_withdrawals_to_reconcile(BATCH).await?;
        let (mut proving, mut settled, mut failed) = (0usize, 0usize, 0usize);
        let mut first_err: Option<String> = None;
        for w in &rows {
            let Some(hash) = w.withdrawal_hash.as_deref().and_then(|h| h.parse::<B256>().ok()) else {
                continue;
            };
            match self.reconcile_one(&repo, w.id, &w.status, hash).await {
                Ok((p, s)) => {
                    proving += p as usize;
                    settled += s as usize;
                }
                Err(e) => {
                    failed += 1;
                    first_err.get_or_insert_with(|| format!("{}: {e}", w.id));
                }
            }
            repo.touch_reconciled(w.id).await?;
        }
        if proving + settled > 0 {
            info!(proving, settled, checked = rows.len(), "Hemi→ETH withdrawals reconciled");
        }
        if let Some(err) = first_err {
            warn!(failed, checked = rows.len(), first_error = %err, "Hemi→ETH withdrawal reconcile errors");
        }
        Ok(())
    }

    /// Returns (moved to PROVING, settled as FINALIZED/FAILED).
    async fn reconcile_one(
        &self,
        repo: &TunnelTransferRepo<'_>,
        id: uuid::Uuid,
        status: &str,
        hash: B256,
    ) -> Result<(bool, bool)> {
        let mut moved_to_proving = false;
        if status == "INITIATED" {
            sleep(CALL_SPACING).await;
            let Some(proven_at) = self.proven_at(hash).await? else {
                return Ok((false, false)); // not proven ⇒ can't be finalized either
            };
            if repo.set_eth_withdrawal_proving(id, proven_at).await? {
                moved_to_proving = true;
                info!(transfer = %id, %proven_at, "HEMI_TO_ETH withdrawal INITIATED → PROVING");
                self.notify(repo, id).await;
            }
        }

        sleep(CALL_SPACING).await;
        if !self.is_finalized(hash).await? {
            return Ok((moved_to_proving, false));
        }
        // Claimed on L1: find the claim tx for the destination leg.
        let Some(row) = repo.get(id).await? else { return Ok((moved_to_proving, false)) };
        let from_ts = row.proven_at.unwrap_or(row.initiated_at);
        let Some(fin) = self.find_finalization(hash, from_ts).await? else {
            // Raced: finalized at `latest` but not when re-read at head. Next rotation.
            return Ok((moved_to_proving, false));
        };
        let changed = repo
            .settle_eth_withdrawal(
                id,
                fin.success,
                &format!("{:#x}", fin.tx_hash),
                fin.block as i64,
                fin.fee,
                fin.time,
            )
            .await?;
        if changed {
            info!(
                transfer = %id,
                tx = %fin.tx_hash,
                success = fin.success,
                "HEMI_TO_ETH withdrawal settled from L1 WithdrawalFinalized"
            );
            self.notify(repo, id).await;
        }
        Ok((moved_to_proving, changed))
    }

    /// Enqueue a `transfer.status_changed` webhook for the row's current state.
    async fn notify(&self, repo: &TunnelTransferRepo<'_>, id: uuid::Uuid) {
        use strait_api::webhooks::dispatcher::{self, event};
        match repo.get(id).await {
            Ok(Some(row)) => {
                if let Err(e) = dispatcher::enqueue(&self.db, event::STATUS_CHANGED, &row).await {
                    error!(transfer = %id, "Webhook enqueue failed: {e}");
                }
            }
            Ok(None) => {}
            Err(e) => error!(transfer = %id, "Webhook enqueue lookup failed: {e}"),
        }
    }

    async fn is_finalized(&self, hash: B256) -> Result<bool> {
        self.is_finalized_at(hash, BlockId::latest()).await
    }

    /// `finalizedWithdrawals(hash)` as of `block` (historical reads need an
    /// archive-capable RPC — standard on Alchemy / QuickNode / Infura).
    async fn is_finalized_at(&self, hash: B256, block: BlockId) -> Result<bool> {
        let calldata = IOptimismPortal::finalizedWithdrawalsCall { withdrawalHash: hash }.abi_encode();
        let req = TransactionRequest::default().to(self.portal).input(calldata.into());
        let raw = self
            .eth_provider
            .call(&req)
            .block(block)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("finalizedWithdrawals@{block:?}: {e}")))?;
        let ret = IOptimismPortal::finalizedWithdrawalsCall::abi_decode_returns(&raw, false)
            .map_err(|e| StraitError::Parse(format!("finalizedWithdrawals decode: {e}")))?;
        Ok(ret._0)
    }

    /// Locate the portal's `WithdrawalFinalized` log for `hash`.
    ///
    /// `finalizedWithdrawals(hash)` flips false → true exactly in the claim block,
    /// so a binary search over historical state (~18 `eth_call`s) finds that block,
    /// and a single-block `eth_getLogs` returns the log. This works under any
    /// provider's getLogs range cap (Alchemy's free tier allows only 10 blocks),
    /// unlike scanning forward in chunks.
    async fn find_finalization(
        &self,
        hash: B256,
        from_ts: DateTime<Utc>,
    ) -> Result<Option<Finalization>> {
        let head = self
            .eth_provider
            .get_block_number()
            .await
            .map_err(|e| StraitError::EvmProvider(format!("eth_blockNumber: {e}")))?;
        // Lower bound: a block at or before `from_ts` (the claim can't precede the
        // proof). 12s slots; missed slots only make the true block later, and the
        // margin covers clock skew. Verified below rather than trusted.
        let secs_back = (Utc::now() - from_ts).num_seconds().max(0) as u64;
        let mut lo = head.saturating_sub(secs_back / 12 + 1_000);
        if self.is_finalized_at(hash, BlockId::number(lo)).await? {
            lo = 0; // estimate overshot — search the whole history instead
        }
        let mut hi = head;
        if !self.is_finalized_at(hash, BlockId::number(hi)).await? {
            return Ok(None);
        }
        // Invariant: not finalized at `lo`, finalized at `hi`.
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            sleep(CALL_SPACING).await;
            if self.is_finalized_at(hash, BlockId::number(mid)).await? {
                hi = mid;
            } else {
                lo = mid;
            }
        }

        let filter = Filter::new()
            .address(self.portal)
            .event_signature(IOptimismPortal::WithdrawalFinalized::SIGNATURE_HASH)
            .topic1(hash)
            .from_block(hi)
            .to_block(hi);
        let logs = self
            .eth_provider
            .get_logs(&filter)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("eth_getLogs@{hi}: {e}")))?;
        let Some(log) = logs.first() else {
            return Err(StraitError::Internal(format!(
                "finalized at block {hi} but no WithdrawalFinalized log there"
            )));
        };
        let decoded = log
            .log_decode::<IOptimismPortal::WithdrawalFinalized>()
            .map_err(|e| StraitError::Parse(format!("WithdrawalFinalized decode: {e}")))?;
        let tx_hash = log
            .transaction_hash
            .ok_or_else(|| StraitError::Internal("log without tx hash".into()))?;
        Ok(Some(Finalization {
            success: decoded.inner.data.success,
            tx_hash,
            block: hi,
            time: self.block_time(hi).await?,
            fee: self.tx_fee(tx_hash).await,
        }))
    }

    async fn block_time(&self, block: u64) -> Result<DateTime<Utc>> {
        let b = self
            .eth_provider
            .get_block_by_number(block.into(), false)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("eth_getBlockByNumber: {e}")))?
            .ok_or_else(|| StraitError::EvmProvider(format!("block {block} not found")))?;
        DateTime::from_timestamp(b.header.timestamp as i64, 0)
            .ok_or_else(|| StraitError::Parse("bad block timestamp".into()))
    }

    /// Claim-tx network fee in wei, best-effort.
    async fn tx_fee(&self, tx: B256) -> Option<BigDecimal> {
        let r = self.eth_provider.get_transaction_receipt(tx).await.ok()??;
        let fee = (r.gas_used as u128).checked_mul(r.effective_gas_price)?;
        BigDecimal::from_str(&fee.to_string()).ok()
    }

    /// When `hash` was proven on the portal, or `None` if it hasn't been.
    async fn proven_at(&self, hash: B256) -> Result<Option<DateTime<Utc>>> {
        let calldata = IOptimismPortal::provenWithdrawalsCall { withdrawalHash: hash }.abi_encode();
        let req = TransactionRequest::default().to(self.portal).input(calldata.into());
        let raw = self
            .eth_provider
            .call(&req)
            .await
            .map_err(|e| StraitError::EvmProvider(format!("provenWithdrawals: {e}")))?;
        let ret = IOptimismPortal::provenWithdrawalsCall::abi_decode_returns(&raw, false)
            .map_err(|e| StraitError::Parse(format!("provenWithdrawals decode: {e}")))?;
        let ts = u64::try_from(ret.timestamp).unwrap_or(0);
        if ts == 0 {
            return Ok(None);
        }
        Ok(DateTime::from_timestamp(ts as i64, 0))
    }
}
