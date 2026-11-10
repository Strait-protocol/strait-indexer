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
//!  2. **Proof check**: for INITIATED withdrawals with a hash, call
//!     `provenWithdrawals(hash)` on the portal; a non-zero timestamp means proven,
//!     so the transfer moves to PROVING (with `proven_at`) and a
//!     `transfer.status_changed` webhook is enqueued.
//!
//! State-based rather than event-based on purpose: it backfills withdrawals proven
//! before this ran, and a missed log or a node restart can't strand a transfer.
//! PROVING → FINALIZED stays with the L1StandardBridge `*BridgeFinalized` path.

use std::sync::Arc;
use std::time::Duration;

use alloy::primitives::{Address, B256};
use alloy::providers::Provider;
use alloy::rpc::types::TransactionRequest;
use alloy::sol_types::SolCall;
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
            if let Err(e) = self.check_proofs().await {
                warn!("withdrawal proof check failed: {e}");
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

    /// Phase 2: ask the portal whether each INITIATED withdrawal has been proven.
    async fn check_proofs(&self) -> Result<()> {
        use strait_api::webhooks::dispatcher::{self, event};

        let repo = TunnelTransferRepo::new(&self.db);
        let candidates = repo.eth_withdrawals_awaiting_proof(BATCH).await?;
        let mut advanced = 0usize;
        for w in &candidates {
            let Some(hash) = w.withdrawal_hash.as_deref().and_then(|h| h.parse::<B256>().ok()) else {
                continue;
            };
            sleep(CALL_SPACING).await;
            let proven_at = match self.proven_at(hash).await {
                Ok(Some(t)) => t,
                Ok(None) => continue,
                Err(e) => {
                    debug!(transfer = %w.id, "provenWithdrawals call failed: {e}");
                    continue;
                }
            };
            if !repo.set_eth_withdrawal_proving(w.id, proven_at).await? {
                continue; // raced with finalization — nothing changed
            }
            advanced += 1;
            info!(
                transfer = %w.id,
                %proven_at,
                "HEMI_TO_ETH withdrawal INITIATED → PROVING (challenge window open)"
            );
            match repo.get(w.id).await {
                Ok(Some(row)) => {
                    if let Err(e) = dispatcher::enqueue(&self.db, event::STATUS_CHANGED, &row).await {
                        error!(transfer = %w.id, "Webhook enqueue failed: {e}");
                    }
                }
                Ok(None) => {}
                Err(e) => error!(transfer = %w.id, "Webhook enqueue lookup failed: {e}"),
            }
        }
        if advanced > 0 {
            info!(advanced, checked = candidates.len(), "Hemi→ETH withdrawals now PROVING");
        }
        Ok(())
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
