//! Repository for the unified `tunnel_transfers` table.
//!
//! This is the table the join engine writes to (via `TunnelTransferUpdate`) and the
//! API reads from. It mirrors `strait_core::types::TunnelTransfer`, including the
//! PoP / Bitcoin-finality fields.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::types::BigDecimal;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use strait_core::error::{Result, StraitError};
use strait_core::types::{TunnelStatus, TunnelTransfer};

use crate::db::Database;

/// A row of the `tunnel_transfers` table, as returned to API consumers.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TunnelTransferRow {
    pub id: Uuid,
    pub asset: String,
    pub direction: String,
    pub route: String,
    #[serde(serialize_with = "serialize_plain_decimal")]
    pub amount: BigDecimal,
    pub sender: String,
    pub recipient: String,
    pub status: String,

    pub source_chain: String,
    pub source_tx_hash: String,
    pub source_block: i64,
    pub source_timestamp: DateTime<Utc>,

    pub dest_chain: Option<String>,
    pub dest_tx_hash: Option<String>,
    pub dest_block: Option<i64>,

    /// Per-leg network fee in the chain's atomic unit (wei for EVM, sats for BTC).
    #[serde(serialize_with = "serialize_opt_plain_decimal")]
    pub source_fee: Option<BigDecimal>,
    #[serde(serialize_with = "serialize_opt_plain_decimal")]
    pub dest_fee: Option<BigDecimal>,
    /// BTC withdrawal uuid, used to match the Bitcoin payout by its OP_RETURN.
    pub withdrawal_uuid: Option<i64>,

    pub pop_anchored: bool,
    pub pop_keystone_block: Option<i64>,
    pub pop_score: Option<i64>,
    pub pop_anchored_at: Option<DateTime<Utc>>,

    pub initiated_at: DateTime<Utc>,
    pub finalized_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One time-bucketed row from `analytics_series`: transfer count and total
/// volume for a given route/asset within a `date_trunc`'d window.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AnalyticsBucketRow {
    pub bucket_start: DateTime<Utc>,
    pub route: String,
    pub asset: String,
    pub transfer_count: i64,
    pub volume: BigDecimal,
}

/// Repository over `tunnel_transfers`.
pub struct TunnelTransferRepo<'a> {
    pool: &'a PgPool,
}

impl<'a> TunnelTransferRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { pool: db.pool() }
    }

    /// Insert a transfer, or update its mutable fields if it already exists.
    ///
    /// Idempotent on `id` — the engine derives a deterministic id from the Hemi
    /// tx hash, so re-delivery (restart / reorg replay) updates rather than dupes.
    pub async fn upsert(&self, t: &TunnelTransfer) -> Result<()> {
        let (dest_chain, dest_tx_hash, dest_block) = match &t.destination_tx {
            Some(tx) => (
                Some(tx.chain.to_string()),
                Some(tx.hash.to_string()),
                Some(tx.block_number as i64),
            ),
            None => (None, None, None),
        };

        sqlx::query(
            r#"
            INSERT INTO tunnel_transfers (
                id, asset, direction, route, amount, sender, recipient, status,
                source_chain, source_tx_hash, source_block, source_timestamp,
                dest_chain, dest_tx_hash, dest_block,
                pop_anchored, pop_keystone_block, pop_score, pop_anchored_at,
                initiated_at, finalized_at,
                source_fee, dest_fee, withdrawal_uuid
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24)
            ON CONFLICT (id) DO UPDATE SET
                -- Never let a late INITIATED event regress an advanced status —
                -- except a REORGED row, which a re-observed event revives (the
                -- event re-confirmed on the canonical chain after the reorg).
                status             = CASE WHEN EXCLUDED.status = 'INITIATED'
                                           AND tunnel_transfers.status <> 'REORGED'
                                          THEN tunnel_transfers.status ELSE EXCLUDED.status END,
                asset              = EXCLUDED.asset,
                direction          = EXCLUDED.direction,
                route              = EXCLUDED.route,
                amount             = EXCLUDED.amount,
                sender             = EXCLUDED.sender,
                recipient          = EXCLUDED.recipient,
                -- Keep the real (non-zero) Bitcoin block from whichever leg has it.
                source_block       = GREATEST(EXCLUDED.source_block, tunnel_transfers.source_block),
                source_timestamp   = LEAST(EXCLUDED.source_timestamp, tunnel_transfers.source_timestamp),
                -- Fill the destination leg once it confirms.
                dest_chain         = COALESCE(EXCLUDED.dest_chain, tunnel_transfers.dest_chain),
                dest_tx_hash       = COALESCE(EXCLUDED.dest_tx_hash, tunnel_transfers.dest_tx_hash),
                dest_block         = COALESCE(EXCLUDED.dest_block, tunnel_transfers.dest_block),
                pop_anchored       = EXCLUDED.pop_anchored OR tunnel_transfers.pop_anchored,
                pop_keystone_block = COALESCE(EXCLUDED.pop_keystone_block, tunnel_transfers.pop_keystone_block),
                pop_score          = COALESCE(EXCLUDED.pop_score, tunnel_transfers.pop_score),
                pop_anchored_at    = COALESCE(EXCLUDED.pop_anchored_at, tunnel_transfers.pop_anchored_at),
                finalized_at       = COALESCE(EXCLUDED.finalized_at, tunnel_transfers.finalized_at),
                source_fee         = COALESCE(EXCLUDED.source_fee, tunnel_transfers.source_fee),
                dest_fee           = COALESCE(EXCLUDED.dest_fee, tunnel_transfers.dest_fee),
                withdrawal_uuid    = COALESCE(EXCLUDED.withdrawal_uuid, tunnel_transfers.withdrawal_uuid),
                updated_at         = NOW()
            "#,
        )
        .bind(t.id)
        .bind(t.asset.to_string())
        .bind(t.direction.to_string())
        .bind(t.route.to_string())
        .bind(t.amount.clone())
        .bind(t.sender.to_string())
        .bind(t.recipient.to_string())
        .bind(status_str(&t.status))
        .bind(t.source_tx.chain.to_string())
        .bind(t.source_tx.hash.to_string())
        .bind(t.source_tx.block_number as i64)
        .bind(t.source_tx.timestamp)
        .bind(dest_chain)
        .bind(dest_tx_hash)
        .bind(dest_block)
        .bind(t.pop_anchored)
        .bind(t.pop_keystone_block.map(|v| v as i64))
        .bind(t.pop_score.map(|v| v as i64))
        .bind(t.pop_anchored_at)
        .bind(t.initiated_at)
        .bind(t.finalized_at)
        .bind(t.source_fee.clone())
        .bind(t.dest_fee.clone())
        .bind(t.withdrawal_uuid.map(|v| v as i64))
        .execute(self.pool)
        .await
        .map_err(StraitError::Database)?;

        Ok(())
    }

    /// Update a transfer's status (and finalized_at, if provided).
    pub async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        finalized_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE tunnel_transfers
             SET status = $2, finalized_at = COALESCE($3, finalized_at), updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .bind(finalized_at)
        .execute(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(())
    }

    /// Record PoP anchoring on a transfer. Sets the pop_* fields; does not change
    /// status (BTC→Hemi deposits are already FINALIZED before PoP fires).
    pub async fn set_pop_anchor(
        &self,
        id: Uuid,
        keystone_block: i64,
        pop_score: i64,
        anchored_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE tunnel_transfers
             SET pop_anchored = TRUE,
                 pop_keystone_block = $2,
                 pop_score = $3,
                 pop_anchored_at = $4,
                 updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(keystone_block)
        .bind(pop_score)
        .bind(anchored_at)
        .execute(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(())
    }

    /// Find the oldest INITIATED HEMI_TO_ETH withdrawal for a recipient address —
    /// used when `WithdrawalProven` fires to advance the transfer to PROVING.
    ///
    /// `WithdrawalProven` carries no amount, so matching is by recipient only.
    /// FIFO ordering (ASC by initiated_at) handles same-user concurrent withdrawals.
    pub async fn find_eth_withdrawal_for_proving(
        &self,
        recipient: &str,
    ) -> Result<Option<TunnelTransferRow>> {
        let row = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers
             WHERE route = 'HEMI_TO_ETH'
               AND status = 'INITIATED'
               AND lower(recipient) = lower($1)
             ORDER BY initiated_at ASC
             LIMIT 1",
        )
        .bind(recipient)
        .fetch_optional(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(row)
    }

    /// Advance a HEMI_TO_ETH withdrawal to PROVING once its withdrawal has been
    /// proven on the OptimismPortal (1-day challenge window now open).
    /// The `WHERE status = 'INITIATED'` guard makes this idempotent.
    pub async fn set_eth_withdrawal_proving(
        &self,
        id: Uuid,
        proven_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE tunnel_transfers
             SET status     = 'PROVING',
                 updated_at = $2
             WHERE id = $1
               AND status = 'INITIATED'",
        )
        .bind(id)
        .bind(proven_at)
        .execute(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(())
    }

    /// Find a Hemi→ETH withdrawal in INITIATED or PROVING status matching the
    /// Ethereum recipient address and exact amount — used by the DB-backed
    /// finalization fallback when ETHBridgeFinalized fires on L1.
    ///
    /// Returns the oldest matching transfer (ASC by initiated_at) so that repeated
    /// withdrawals of the same amount are finalized in initiation order.
    pub async fn find_pending_eth_withdrawal(
        &self,
        recipient: &str,
        amount: &BigDecimal,
    ) -> Result<Option<TunnelTransferRow>> {
        let row = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers
             WHERE route = 'HEMI_TO_ETH'
               AND status IN ('INITIATED', 'PROVING')
               AND lower(recipient) = lower($1)
               AND amount = $2
             ORDER BY initiated_at ASC
             LIMIT 1",
        )
        .bind(recipient)
        .bind(amount)
        .fetch_optional(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(row)
    }

    /// Mark a Hemi→ETH withdrawal FINALIZED once its L1 release tx is confirmed.
    pub async fn set_eth_withdrawal_finalized(
        &self,
        id: Uuid,
        dest_tx_hash: &str,
        dest_block: Option<i64>,
        dest_fee: Option<BigDecimal>,
        finalized_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE tunnel_transfers
             SET status       = 'FINALIZED',
                 dest_chain   = 'ETHEREUM',
                 dest_tx_hash = $2,
                 dest_block   = COALESCE($3, dest_block),
                 dest_fee     = COALESCE($4, dest_fee),
                 finalized_at = $5,
                 updated_at   = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(dest_tx_hash)
        .bind(dest_block)
        .bind(dest_fee)
        .bind(finalized_at)
        .execute(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(())
    }

    /// Find INITIATED BTC→Hemi deposits whose Hemi mint block is covered by the
    /// given keystone window `(keystone_block - 25, keystone_block]`.
    ///
    /// Used as a DB-backed fallback for PoP keystone anchoring after a node restart —
    /// when the JoinEngine's in-memory state is empty, pending deposits would otherwise
    /// be silently skipped by the keystone fan-out.
    pub async fn list_initiated_btc_deposits_in_keystone_window(
        &self,
        keystone_block: i64,
    ) -> Result<Vec<TunnelTransferRow>> {
        let window_start = keystone_block.saturating_sub(25);
        let rows = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers
             WHERE route = 'BTC_TO_HEMI'
               AND pop_anchored = FALSE
               AND dest_chain = 'HEMI'
               AND dest_block > $1
               AND dest_block <= $2",
        )
        .bind(window_start)
        .bind(keystone_block)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows)
    }

    /// Find BTC→Hemi deposits not yet PoP-anchored whose mint block is strictly before
    /// `before_block` — meaning their keystone window has already passed without being
    /// caught by the in-session or per-keystone DB fallback (e.g. the node was down when
    /// the keystone fired). Used by the startup catch-up to retroactively set pop fields.
    pub async fn list_stale_initiated_btc_deposits(
        &self,
        before_block: i64,
    ) -> Result<Vec<TunnelTransferRow>> {
        let rows = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers
             WHERE route = 'BTC_TO_HEMI'
               AND pop_anchored = FALSE
               AND dest_chain = 'HEMI'
               AND dest_block IS NOT NULL
               AND dest_block <= $1",
        )
        .bind(before_block)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows)
    }

    /// Find an INITIATED HEMI_TO_BTC withdrawal by its on-chain uuid.
    /// Used by the join engine to mark a withdrawal FAILED when
    /// `WithdrawalChallengeSuccess` fires (operator did not pay on time).
    pub async fn find_initiated_btc_withdrawal_by_uuid(&self, uuid: i64) -> Result<Option<TunnelTransferRow>> {
        let row = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers
             WHERE route = 'HEMI_TO_BTC'
               AND status = 'INITIATED'
               AND withdrawal_uuid = $1
             LIMIT 1",
        )
        .bind(uuid)
        .fetch_optional(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(row)
    }

    /// List Hemi→BTC withdrawals whose BTC address recovery failed at indexing time —
    /// INITIATED transfers with a `withdrawal-uuid-N` placeholder recipient but a
    /// known uuid. The payout watcher retries calldata decoding for these.
    pub async fn list_btc_withdrawals_needing_recipient(
        &self,
        limit: i64,
    ) -> Result<Vec<TunnelTransferRow>> {
        let rows = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers
             WHERE route = 'HEMI_TO_BTC'
               AND status = 'INITIATED'
               AND withdrawal_uuid IS NOT NULL
               AND recipient LIKE 'withdrawal-uuid-%'
             ORDER BY initiated_at ASC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows)
    }

    /// Update the BTC recipient for a Hemi→BTC withdrawal that had a placeholder.
    /// Called by the payout watcher after recovering the real address from calldata.
    pub async fn update_btc_withdrawal_recipient(&self, id: Uuid, btc_address: &str) -> Result<()> {
        sqlx::query(
            "UPDATE tunnel_transfers SET recipient = $2, updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(btc_address)
        .execute(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(())
    }

    /// List Hemi→BTC withdrawals still awaiting their Bitcoin payout (status
    /// `INITIATED`), most recent first — candidates for payout matching.
    pub async fn list_pending_btc_payouts(&self, limit: i64) -> Result<Vec<TunnelTransferRow>> {
        let rows = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers
             WHERE route = 'HEMI_TO_BTC' AND status = 'INITIATED'
             ORDER BY initiated_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows)
    }

    /// Record the observed Bitcoin payout for a Hemi→BTC withdrawal and finalize it.
    pub async fn set_btc_payout(
        &self,
        id: Uuid,
        payout_txid: &str,
        payout_block: Option<i64>,
        dest_fee: Option<BigDecimal>,
        finalized_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE tunnel_transfers
             SET status = 'FINALIZED',
                 dest_chain = 'BITCOIN',
                 dest_tx_hash = $2,
                 dest_block = COALESCE($3, dest_block),
                 dest_fee = COALESCE($4, dest_fee),
                 finalized_at = $5,
                 updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(payout_txid)
        .bind(payout_block)
        .bind(dest_fee)
        .bind(finalized_at)
        .execute(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(())
    }

    /// Mark every transfer whose source or destination leg sits at or above
    /// `from_block` on `chain` as REORGED. Returns the affected ids.
    ///
    /// Called by the join engine when an ingester reports a reorg. Rows revive
    /// automatically if their events are re-observed on the canonical chain:
    /// the upsert lets a re-delivered event overwrite REORGED.
    pub async fn mark_reorged(&self, chain: &str, from_block: i64) -> Result<Vec<Uuid>> {
        let rows = sqlx::query_as::<_, (Uuid,)>(
            "UPDATE tunnel_transfers
             SET status = 'REORGED', updated_at = NOW()
             WHERE status <> 'REORGED'
               AND ((source_chain = $1 AND source_block >= $2)
                 OR (dest_chain  = $1 AND dest_block  >= $2))
             RETURNING id",
        )
        .bind(chain)
        .bind(from_block)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// List transfers, most recent first.
    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<TunnelTransferRow>> {
        let rows = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows)
    }

    /// List transfers addressed to a given recipient (case-insensitive), most recent first.
    pub async fn list_by_recipient(
        &self,
        recipient: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TunnelTransferRow>> {
        let rows = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers
             WHERE lower(recipient) = lower($1)
             ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(recipient)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows)
    }

    /// Search transfers by free text (id / sender / recipient / source or dest tx
    /// hash, case-insensitive substring) plus optional status and route filters.
    /// Any argument may be `None`; all present filters are ANDed.
    pub async fn search(
        &self,
        query: Option<&str>,
        status: Option<&str>,
        route: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TunnelTransferRow>> {
        let rows = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers
             WHERE ($1::text IS NULL OR (
                     id::text ILIKE '%'||$1||'%'
                     OR sender ILIKE '%'||$1||'%'
                     OR recipient ILIKE '%'||$1||'%'
                     OR source_tx_hash ILIKE '%'||$1||'%'
                     OR dest_tx_hash ILIKE '%'||$1||'%'
                   ))
               AND ($2::text IS NULL OR status = $2)
               AND ($3::text IS NULL OR route = $3)
             ORDER BY created_at DESC
             LIMIT $4 OFFSET $5",
        )
        .bind(query)
        .bind(status)
        .bind(route)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows)
    }

    /// Fetch a single transfer by id.
    pub async fn get(&self, id: Uuid) -> Result<Option<TunnelTransferRow>> {
        let row = sqlx::query_as::<_, TunnelTransferRow>(
            "SELECT * FROM tunnel_transfers WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(row)
    }

    /// Delete a transfer by id. Returns true if a row was removed.
    pub async fn delete(&self, id: Uuid) -> Result<bool> {
        let res = sqlx::query("DELETE FROM tunnel_transfers WHERE id = $1")
            .bind(id)
            .execute(self.pool)
            .await
            .map_err(StraitError::Database)?;
        Ok(res.rows_affected() > 0)
    }

    /// Number of indexed transfers with `initiated_at >= since` (`None` = all-time).
    pub async fn count(&self, since: Option<DateTime<Utc>>) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM tunnel_transfers WHERE $1::timestamptz IS NULL OR initiated_at >= $1",
        )
        .bind(since)
        .fetch_one(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(row.0)
    }

    /// Per-status counts within `since` (`None` = all-time).
    pub async fn count_by_status(&self, since: Option<DateTime<Utc>>) -> Result<std::collections::HashMap<String, i64>> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT status, COUNT(*)::bigint FROM tunnel_transfers
             WHERE $1::timestamptz IS NULL OR initiated_at >= $1
             GROUP BY status",
        )
        .bind(since)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows.into_iter().collect())
    }

    /// Time-bucketed transfer count + volume per route/asset, backing the
    /// `analyticsSeries` GraphQL query.
    ///
    /// `granularity` must be one of `date_trunc`'s field names ("day" / "week" /
    /// "month" — validated by the caller via the `Granularity` GraphQL enum, never
    /// user-supplied free text). `since` filters to `initiated_at >= since`;
    /// `None` means all-time (no lower bound).
    pub async fn analytics_series(
        &self,
        since: Option<DateTime<Utc>>,
        granularity: &str,
    ) -> Result<Vec<AnalyticsBucketRow>> {
        let rows = sqlx::query_as::<_, AnalyticsBucketRow>(
            "SELECT
                date_trunc($1, initiated_at) AS bucket_start,
                route,
                asset,
                COUNT(*)::bigint AS transfer_count,
                SUM(amount)      AS volume
             FROM tunnel_transfers
             WHERE $2::timestamptz IS NULL OR initiated_at >= $2
             GROUP BY bucket_start, route, asset
             ORDER BY bucket_start ASC",
        )
        .bind(granularity)
        .bind(since)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows)
    }

    /// Per-route transfer counts within a window, backing the `routeBreakdown`
    /// GraphQL query. `since` of `None` means all-time.
    pub async fn route_breakdown(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT route, COUNT(*)::bigint
             FROM tunnel_transfers
             WHERE $1::timestamptz IS NULL OR initiated_at >= $1
             GROUP BY route",
        )
        .bind(since)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(rows)
    }

    /// Number of transfers PoP-anchored to Bitcoin, within `since` (`None` = all-time).
    pub async fn count_pop_anchored(&self, since: Option<DateTime<Utc>>) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*)::bigint FROM tunnel_transfers
             WHERE pop_anchored = true
               AND ($1::timestamptz IS NULL OR initiated_at >= $1)",
        )
        .bind(since)
        .fetch_one(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(row.0)
    }
}

/// Serialize a `BigDecimal` as a plain integer string (e.g. "130000000000000000")
/// rather than `BigDecimal`'s default scientific form ("13e+16"). Tunnel amounts are
/// always integers (satoshis / wei), so forcing scale 0 is lossless.
fn serialize_plain_decimal<S: serde::Serializer>(
    amount: &BigDecimal,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&amount.with_scale(0).to_string())
}

/// Like `serialize_plain_decimal` but for an optional fee — `null` when absent.
fn serialize_opt_plain_decimal<S: serde::Serializer>(
    amount: &Option<BigDecimal>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match amount {
        Some(d) => serializer.serialize_str(&d.with_scale(0).to_string()),
        None => serializer.serialize_none(),
    }
}

/// Canonical string form of a transfer status (matches the values written by the engine).
pub fn status_str(status: &TunnelStatus) -> &'static str {
    match status {
        TunnelStatus::Initiated => "INITIATED",
        TunnelStatus::Anchored => "ANCHORED",
        TunnelStatus::Proving => "PROVING",
        TunnelStatus::Finalized => "FINALIZED",
        TunnelStatus::Failed { .. } => "FAILED",
        TunnelStatus::Reorged { .. } => "REORGED",
    }
}

#[cfg(test)]
mod db_tests {
    //! Full persist → read round-trip against a real Postgres / Supabase database.
    //!
    //! Skips automatically unless `DATABASE_URL` is set, so it is safe in CI without
    //! a database. Run it against Supabase with:
    //!
    //! ```bash
    //! DATABASE_URL='postgres://...supabase.com:5432/postgres?sslmode=require' \
    //!   cargo test -p strait-store -- --ignored db_roundtrip --nocapture
    //! ```
    use super::*;
    use chrono::Utc;
    use strait_core::types::*;

    // The same real Hemi testnet ETHBridgeFinalized event used in the engine tests:
    // 0.13 ETH bridged into Hemi at block 6037628.
    fn real_transfer() -> TunnelTransfer {
        let now = Utc::now();
        let tx = TxHash::from_hex(
            "0x2b320473e4f679f8d763bbb6aee617f8b42b1a9367e623ec1c5bc6e45eedc19a",
        )
        .unwrap();
        let to = Address::from_hex("0x065213232a03b6ef9c51e0c3250096474bb515b8").unwrap();
        TunnelTransfer {
            id: Uuid::parse_str("4ca93d2a-8b7c-567c-80ee-473de55b38e8").unwrap(),
            asset: Asset::Eth,
            direction: TunnelDirection::In,
            route: TunnelRoute::EthToHemi,
            amount: BigDecimal::from(130_000_000_000_000_000u64),
            source_fee: None,
            dest_fee: None,
            withdrawal_uuid: None,
            sender: ChainAddress::Evm(to),
            recipient: ChainAddress::Evm(to),
            status: TunnelStatus::Initiated,
            initiated_at: now,
            finalized_at: None,
            source_tx: ChainTransaction {
                chain: Chain::Ethereum,
                hash: ChainTxHash::Evm(tx),
                block_number: 0,
                block_hash: BlockHash([0u8; 32]),
                timestamp: now,
                confirmations: 0,
            },
            destination_tx: Some(ChainTransaction {
                chain: Chain::Hemi,
                hash: ChainTxHash::Evm(tx),
                block_number: 6_037_628,
                block_hash: BlockHash([0u8; 32]),
                timestamp: now,
                confirmations: 0,
            }),
            pop_anchored: false,
            pop_keystone_block: None,
            pop_score: None,
            pop_anchored_at: None,
            reorg_events: vec![],
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL; run explicitly to verify the DB round-trip"]
    async fn db_roundtrip_real_testnet_transfer() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set — skipping DB round-trip");
            return;
        };

        let db = crate::init_database(&url)
            .await
            .expect("connect + run migrations");
        let repo = TunnelTransferRepo::new(&db);
        let t = real_transfer();

        // Clean any prior run, then persist.
        let _ = sqlx::query("DELETE FROM tunnel_transfers WHERE id = $1")
            .bind(t.id)
            .execute(db.pool())
            .await;
        repo.upsert(&t).await.expect("upsert");

        // Read it back — this is exactly what GET /transfers/:id returns.
        let got = repo.get(t.id).await.expect("get").expect("row should exist");
        assert_eq!(got.route, "ETH_TO_HEMI");
        assert_eq!(got.amount, BigDecimal::from(130_000_000_000_000_000u64));
        assert_eq!(got.amount.with_scale(0).to_string(), "130000000000000000");
        assert_eq!(
            got.recipient.to_lowercase(),
            "0x065213232a03b6ef9c51e0c3250096474bb515b8"
        );
        assert_eq!(got.status, "INITIATED");
        assert_eq!(got.dest_block, Some(6_037_628));
        assert_eq!(got.dest_chain.as_deref(), Some("HEMI"));

        // PoP-anchor it and confirm finality fields land.
        repo.set_pop_anchor(t.id, 6_037_650, 4200, Utc::now())
            .await
            .expect("set_pop_anchor");
        let anchored = repo.get(t.id).await.unwrap().unwrap();
        assert!(anchored.pop_anchored);
        assert_eq!(anchored.pop_keystone_block, Some(6_037_650));
        assert_eq!(anchored.pop_score, Some(4200));
        assert_eq!(anchored.status, "ANCHORED");

        // It shows up in the list endpoint's query.
        let list = repo.list(50, 0).await.unwrap();
        assert!(list.iter().any(|r| r.id == t.id));

        // Cleanup.
        sqlx::query("DELETE FROM tunnel_transfers WHERE id = $1")
            .bind(t.id)
            .execute(db.pool())
            .await
            .unwrap();

        eprintln!("DB round-trip OK — transfer {} persisted, anchored, listed", t.id);
    }

    /// A BTC-route transfer: source is the Bitcoin deposit, dest is the Hemi mint.
    fn btc_transfer(
        id: Uuid,
        source_block: u64,
        dest_block: Option<u64>,
        amount: u64,
        status: TunnelStatus,
    ) -> TunnelTransfer {
        let now = Utc::now();
        let txid = BitcoinTxid([7u8; 32]);
        let to = Address::from_hex("0x065213232a03b6ef9c51e0c3250096474bb515b8").unwrap();
        TunnelTransfer {
            id,
            asset: Asset::Btc,
            direction: TunnelDirection::In,
            route: TunnelRoute::BtcToHemi,
            amount: BigDecimal::from(amount),
            source_fee: None,
            dest_fee: None,
            withdrawal_uuid: None,
            sender: ChainAddress::Bitcoin(BitcoinAddress::new("btctx:..")),
            recipient: ChainAddress::Evm(to),
            status,
            initiated_at: now,
            finalized_at: None,
            source_tx: ChainTransaction {
                chain: Chain::Bitcoin,
                hash: ChainTxHash::Bitcoin(txid),
                block_number: source_block,
                block_hash: BlockHash([0u8; 32]),
                timestamp: now,
                confirmations: 0,
            },
            destination_tx: dest_block.map(|b| ChainTransaction {
                chain: Chain::Hemi,
                hash: ChainTxHash::Evm(TxHash([9u8; 32])),
                block_number: b,
                block_hash: BlockHash([0u8; 32]),
                timestamp: now,
                confirmations: 0,
            }),
            pop_anchored: false,
            pop_keystone_block: None,
            pop_score: None,
            pop_anchored_at: None,
            reorg_events: vec![],
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL; verifies upsert enrichment (deposit + mint -> one row)"]
    async fn db_enrichment_deposit_then_mint_merge() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set — skipping");
            return;
        };
        let db = crate::init_database(&url).await.expect("connect + migrate");
        let repo = TunnelTransferRepo::new(&db);
        let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let _ = repo.delete(id).await;

        // Bitcoin deposit first: real Bitcoin block, gross amount, no dest yet.
        repo.upsert(&btc_transfer(id, 800_000, None, 100_000_000, TunnelStatus::Initiated))
            .await
            .unwrap();
        // Hemi mint second: Bitcoin block unknown (0), net amount, Hemi dest filled.
        repo.upsert(&btc_transfer(
            id,
            0,
            Some(6_037_628),
            99_900_000,
            TunnelStatus::Initiated,
        ))
        .await
        .unwrap();

        let got = repo.get(id).await.unwrap().unwrap();
        assert_eq!(got.source_block, 800_000, "real Bitcoin block must survive (GREATEST)");
        assert_eq!(got.dest_block, Some(6_037_628), "mint fills Hemi destination");
        assert_eq!(got.dest_chain.as_deref(), Some("HEMI"));
        assert_eq!(got.amount.with_scale(0).to_string(), "99900000", "net amount wins");
        assert_eq!(got.status, "INITIATED");

        repo.delete(id).await.unwrap();
        eprintln!("DB enrichment OK — deposit + mint merged into one row {id}");
    }
}
