//! Bitcoin-side PoP publications: the known PoP miners' Bitcoin transactions,
//! the Hemi keystones published in them, and the queries that turn them into
//! Bitcoin finality.
//!
//! Finality rule: a published keystone commits to the keystone chain before it,
//! so Hemi block N is anchored by the **earliest Bitcoin publication of any valid
//! keystone >= N**. That is often a later keystone than N's own, because miners
//! publish in batches and not every keystone is published.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use strait_core::error::{Result, StraitError};

use crate::db::Database;

/// Bitcoin blocks after the start height before the lookup will answer. A
/// keystone published in the first day may also have been published earlier,
/// before the start height, so blocks up to the highest keystone seen in that
/// first day are reported as outside coverage rather than answered wrongly.
const COVERAGE_WARMUP_BLOCKS: i64 = 144;

pub struct NewPublication {
    pub btc_txid: String,
    pub vout: i32,
    pub btc_block_height: i64,
    pub btc_block_hash: String,
    pub btc_block_time: DateTime<Utc>,
    pub miner_address: String,
    pub keystone_block: i64,
    pub l1_block: i64,
    pub ep_hash: String,
    pub raw_keystone: String,
}

/// A publication awaiting validation against the canonical Hemi chain.
#[derive(Debug, Clone, FromRow)]
pub struct UnvalidatedPublication {
    pub btc_txid: String,
    pub vout: i32,
    pub keystone_block: i64,
    pub raw_keystone: String,
}

/// Sync progress.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PopSyncState {
    pub start_height: i64,
    pub start_time: DateTime<Utc>,
    /// Every known miner's history is complete up to this Bitcoin height.
    pub synced_height: Option<i64>,
    pub tip_height: Option<i64>,
}

/// The Bitcoin anchor for a Hemi block.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PopFinalityRow {
    /// The keystone whose publication anchors the block (>= the block).
    pub keystone_block: i64,
    pub btc_txid: String,
    pub vout: i32,
    pub btc_block_height: i64,
    pub btc_block_time: DateTime<Utc>,
    /// Bitcoin confirmations of the publication at the last sync.
    pub confirmations: i64,
}

/// What the sync has covered so far.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PopCoverage {
    pub start_height: Option<i64>,
    pub synced_height: Option<i64>,
    pub tip_height: Option<i64>,
    /// Lowest Hemi block the lookup answers for (see `COVERAGE_WARMUP_BLOCKS`).
    pub min_hemi_block: Option<i64>,
}

pub struct PopRepo<'a> {
    pool: &'a PgPool,
}

impl<'a> PopRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { pool: db.pool() }
    }

    pub async fn state(&self) -> Result<Option<PopSyncState>> {
        sqlx::query_as::<_, PopSyncState>(
            "SELECT start_height, start_time, synced_height, tip_height FROM pop_sync_state",
        )
        .fetch_optional(self.pool)
        .await
        .map_err(StraitError::Database)
    }

    /// Record the configured start. A changed start height resets every miner's
    /// progress so history is re-read from the new start.
    pub async fn init_state(&self, start_height: i64, start_time: DateTime<Utc>) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(StraitError::Database)?;
        let previous: Option<i64> =
            sqlx::query_scalar("SELECT start_height FROM pop_sync_state FOR UPDATE")
                .fetch_optional(&mut *tx)
                .await
                .map_err(StraitError::Database)?;
        match previous {
            Some(h) if h == start_height => {}
            Some(_) => {
                sqlx::query(
                    "UPDATE pop_sync_state SET start_height = $1, start_time = $2,
                            synced_height = NULL, updated_at = NOW()",
                )
                .bind(start_height)
                .bind(start_time)
                .execute(&mut *tx)
                .await
                .map_err(StraitError::Database)?;
                sqlx::query("UPDATE pop_miners SET synced_height = NULL, updated_at = NOW()")
                    .execute(&mut *tx)
                    .await
                    .map_err(StraitError::Database)?;
            }
            None => {
                sqlx::query("INSERT INTO pop_sync_state (start_height, start_time) VALUES ($1, $2)")
                    .bind(start_height)
                    .bind(start_time)
                    .execute(&mut *tx)
                    .await
                    .map_err(StraitError::Database)?;
            }
        }
        tx.commit().await.map_err(StraitError::Database)
    }

    pub async fn ensure_miner(&self, address: &str) -> Result<()> {
        sqlx::query("INSERT INTO pop_miners (address) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(address)
            .execute(self.pool)
            .await
            .map_err(StraitError::Database)?;
        Ok(())
    }

    pub async fn miner_synced_height(&self, address: &str) -> Result<Option<i64>> {
        sqlx::query_scalar::<_, Option<i64>>("SELECT synced_height FROM pop_miners WHERE address = $1")
            .bind(address)
            .fetch_optional(self.pool)
            .await
            .map(Option::flatten)
            .map_err(StraitError::Database)
    }

    pub async fn set_miner_synced_height(&self, address: &str, height: i64) -> Result<()> {
        sqlx::query("UPDATE pop_miners SET synced_height = $2, updated_at = NOW() WHERE address = $1")
            .bind(address)
            .bind(height)
            .execute(self.pool)
            .await
            .map_err(StraitError::Database)?;
        Ok(())
    }

    /// Set the height up to which all configured miners are synced, and the tip.
    pub async fn set_synced(&self, synced_height: Option<i64>, tip_height: i64) -> Result<()> {
        sqlx::query(
            "UPDATE pop_sync_state SET synced_height = $1, tip_height = $2, updated_at = NOW()",
        )
        .bind(synced_height)
        .bind(tip_height)
        .execute(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(())
    }

    /// Insert a publication, or move it if a Bitcoin reorg re-mined it elsewhere.
    pub async fn upsert_publication(&self, p: &NewPublication) -> Result<()> {
        sqlx::query(
            "INSERT INTO pop_publications
                 (btc_txid, vout, btc_block_height, btc_block_hash, btc_block_time, miner_address,
                  keystone_block, l1_block, ep_hash, raw_keystone)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (btc_txid, vout) DO UPDATE
                SET btc_block_height = EXCLUDED.btc_block_height,
                    btc_block_hash   = EXCLUDED.btc_block_hash,
                    btc_block_time   = EXCLUDED.btc_block_time",
        )
        .bind(&p.btc_txid)
        .bind(p.vout)
        .bind(p.btc_block_height)
        .bind(&p.btc_block_hash)
        .bind(p.btc_block_time)
        .bind(&p.miner_address)
        .bind(p.keystone_block)
        .bind(p.l1_block)
        .bind(&p.ep_hash)
        .bind(&p.raw_keystone)
        .execute(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(())
    }

    /// Publications not yet checked against the canonical Hemi chain.
    pub async fn unvalidated(&self, limit: i64) -> Result<Vec<UnvalidatedPublication>> {
        sqlx::query_as::<_, UnvalidatedPublication>(
            "SELECT btc_txid, vout, keystone_block, raw_keystone
             FROM pop_publications WHERE valid IS NULL
             ORDER BY keystone_block LIMIT $1",
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)
    }

    pub async fn set_valid(&self, btc_txid: &str, vout: i32, valid: bool) -> Result<()> {
        sqlx::query("UPDATE pop_publications SET valid = $3 WHERE btc_txid = $1 AND vout = $2")
            .bind(btc_txid)
            .bind(vout)
            .bind(valid)
            .execute(self.pool)
            .await
            .map_err(StraitError::Database)?;
        Ok(())
    }

    /// Mark transfers Bitcoin-final from Bitcoin publications. A transfer's Hemi
    /// block is its source block (withdrawals) or destination block (deposits,
    /// i.e. the mint). It is anchored by the earliest valid publication of a
    /// keystone >= that block, once that publication has `min_confirmations`.
    ///
    /// Only publications inside the fully synced range count, and only transfers
    /// whose Hemi block came after the start height (with a margin for Bitcoin
    /// timestamp skew) are eligible, so the earliest publication is known.
    /// Nothing is anchored past a publication still awaiting validation.
    /// Returns the anchored ids.
    pub async fn anchor_transfers(&self, min_confirmations: i64) -> Result<Vec<Uuid>> {
        sqlx::query_scalar::<_, Uuid>(
            r#"
            WITH s AS (
                SELECT start_time, synced_height, tip_height FROM pop_sync_state
                WHERE synced_height IS NOT NULL AND tip_height IS NOT NULL
            ),
            t AS (
                SELECT id,
                       CASE WHEN source_chain = 'HEMI' THEN source_block
                            WHEN dest_chain   = 'HEMI' THEN dest_block END AS hemi_block,
                       CASE WHEN source_chain = 'HEMI' THEN source_timestamp
                            WHEN dest_chain   = 'HEMI' THEN finalized_at END AS hemi_time
                FROM tunnel_transfers
                WHERE pop_anchored = FALSE AND status <> 'REORGED'
            ),
            cand AS (
                SELECT t.id, p.btc_txid, p.btc_block_height, p.btc_block_time, p.keystone_block
                FROM t
                CROSS JOIN s
                CROSS JOIN LATERAL (
                    SELECT btc_txid, btc_block_height, btc_block_time, keystone_block
                    FROM pop_publications
                    WHERE valid AND keystone_block >= t.hemi_block
                      AND btc_block_height <= s.synced_height
                    ORDER BY btc_block_height, keystone_block, btc_txid
                    LIMIT 1
                ) p
                WHERE t.hemi_block > 0
                  AND t.hemi_time >= s.start_time + INTERVAL '2 hours'
                  AND s.tip_height - p.btc_block_height + 1 >= $1
                  AND NOT EXISTS (
                      SELECT 1 FROM pop_publications q
                      WHERE q.valid IS NULL
                        AND q.keystone_block >= t.hemi_block
                        AND q.btc_block_height <= p.btc_block_height
                  )
            )
            UPDATE tunnel_transfers tt
               SET pop_anchored       = TRUE,
                   pop_keystone_block = cand.keystone_block,
                   pop_btc_txid       = cand.btc_txid,
                   pop_btc_block      = cand.btc_block_height,
                   pop_anchored_at    = cand.btc_block_time,
                   pop_score          = NULL,
                   updated_at         = NOW()
              FROM cand
             WHERE tt.id = cand.id
            RETURNING tt.id
            "#,
        )
        .bind(min_confirmations)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)
    }

    /// Sync coverage, including the lowest Hemi block the lookup answers for.
    pub async fn coverage(&self) -> Result<PopCoverage> {
        let row = sqlx::query_as::<_, PopCoverage>(
            r#"
            SELECT s.start_height, s.synced_height, s.tip_height,
                   (SELECT MAX(p.keystone_block) + 1 FROM pop_publications p
                     WHERE p.valid AND p.btc_block_height < s.start_height + $1) AS min_hemi_block
            FROM pop_sync_state s
            "#,
        )
        .bind(COVERAGE_WARMUP_BLOCKS)
        .fetch_optional(self.pool)
        .await
        .map_err(StraitError::Database)?;
        Ok(row.unwrap_or(PopCoverage {
            start_height: None,
            synced_height: None,
            tip_height: None,
            min_hemi_block: None,
        }))
    }

    /// The Bitcoin anchor for Hemi block `hemi_block`, or `None` if no valid
    /// publication in the synced range covers it yet. Callers must check
    /// `coverage().min_hemi_block` first: below it, an earlier publication may
    /// exist before the start height.
    pub async fn finality_for_hemi_block(&self, hemi_block: i64) -> Result<Option<PopFinalityRow>> {
        sqlx::query_as::<_, PopFinalityRow>(
            r#"
            SELECT p.keystone_block, p.btc_txid, p.vout, p.btc_block_height, p.btc_block_time,
                   s.tip_height - p.btc_block_height + 1 AS confirmations
            FROM pop_publications p
            CROSS JOIN pop_sync_state s
            WHERE p.valid AND p.keystone_block >= $1
              AND p.btc_block_height <= s.synced_height
              AND NOT EXISTS (
                  SELECT 1 FROM pop_publications q
                  WHERE q.valid IS NULL AND q.keystone_block >= $1
                    AND q.btc_block_height <= p.btc_block_height
              )
            ORDER BY p.btc_block_height, p.keystone_block, p.btc_txid
            LIMIT 1
            "#,
        )
        .bind(hemi_block)
        .fetch_optional(self.pool)
        .await
        .map_err(StraitError::Database)
    }
}
