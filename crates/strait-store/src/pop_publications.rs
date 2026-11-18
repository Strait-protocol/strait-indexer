//! Bitcoin-side PoP publications: scanned Bitcoin blocks, the Hemi keystones
//! published in them, and the queries that turn them into Bitcoin finality.
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

/// Blocks scanned before the earliest keystone the lookup will answer for. A
/// keystone published in the first day of scanning may also have been published
/// earlier, before scanning began, so blocks up to the highest keystone seen in
/// that first day are reported as outside coverage rather than answered wrongly.
const COVERAGE_WARMUP_BLOCKS: i64 = 144;

/// A Bitcoin block to record as scanned, with the publications it contains.
pub struct NewScannedBlock {
    pub height: i64,
    pub hash: String,
    pub time: DateTime<Utc>,
    pub publications: Vec<NewPublication>,
}

pub struct NewPublication {
    pub btc_txid: String,
    pub vout: i32,
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

/// The Bitcoin anchor for a Hemi block.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PopFinalityRow {
    /// The keystone whose publication anchors the block (>= the block).
    pub keystone_block: i64,
    pub btc_txid: String,
    pub vout: i32,
    pub btc_block_height: i64,
    pub btc_block_time: DateTime<Utc>,
    /// Bitcoin confirmations of the publication, counted to the scanned tip.
    pub confirmations: i64,
}

/// What the scanner has covered so far.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PopCoverage {
    pub first_height: Option<i64>,
    pub tip_height: Option<i64>,
    /// Lowest Hemi block the finality lookup answers for (see `COVERAGE_WARMUP_BLOCKS`).
    pub min_hemi_block: Option<i64>,
}

pub struct PopRepo<'a> {
    pool: &'a PgPool,
}

impl<'a> PopRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { pool: db.pool() }
    }

    /// Highest scanned Bitcoin block, if any.
    pub async fn scanned_tip(&self) -> Result<Option<(i64, String)>> {
        sqlx::query_as::<_, (i64, String)>(
            "SELECT height, hash FROM pop_scanned_blocks ORDER BY height DESC LIMIT 1",
        )
        .fetch_optional(self.pool)
        .await
        .map_err(StraitError::Database)
    }

    /// The most recent `n` scanned blocks, newest first (for reorg checks).
    pub async fn recent_scanned(&self, n: i64) -> Result<Vec<(i64, String)>> {
        sqlx::query_as::<_, (i64, String)>(
            "SELECT height, hash FROM pop_scanned_blocks ORDER BY height DESC LIMIT $1",
        )
        .bind(n)
        .fetch_all(self.pool)
        .await
        .map_err(StraitError::Database)
    }

    /// Drop scanned blocks from `height` up (a Bitcoin reorg). Their publications
    /// cascade. Returns the number of blocks removed.
    pub async fn delete_from(&self, height: i64) -> Result<u64> {
        sqlx::query("DELETE FROM pop_scanned_blocks WHERE height >= $1")
            .bind(height)
            .execute(self.pool)
            .await
            .map(|r| r.rows_affected())
            .map_err(StraitError::Database)
    }

    /// Record a scanned block and its publications atomically.
    pub async fn insert_block(&self, b: &NewScannedBlock) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(StraitError::Database)?;
        sqlx::query(
            "INSERT INTO pop_scanned_blocks (height, hash, block_time, publications)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (height) DO NOTHING",
        )
        .bind(b.height)
        .bind(&b.hash)
        .bind(b.time)
        .bind(b.publications.len() as i32)
        .execute(&mut *tx)
        .await
        .map_err(StraitError::Database)?;

        for p in &b.publications {
            sqlx::query(
                "INSERT INTO pop_publications
                     (btc_txid, vout, btc_block_height, btc_block_time, keystone_block,
                      l1_block, ep_hash, raw_keystone)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (btc_txid, vout) DO NOTHING",
            )
            .bind(&p.btc_txid)
            .bind(p.vout)
            .bind(b.height)
            .bind(b.time)
            .bind(p.keystone_block)
            .bind(p.l1_block)
            .bind(&p.ep_hash)
            .bind(&p.raw_keystone)
            .execute(&mut *tx)
            .await
            .map_err(StraitError::Database)?;
        }
        tx.commit().await.map_err(StraitError::Database)
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
    /// Only transfers whose Hemi block came after scanning began (with a margin
    /// for Bitcoin timestamp skew) are eligible, so the earliest publication is
    /// known to be inside the scanned range. Nothing is anchored past a
    /// publication that is still awaiting validation. Returns the anchored ids.
    pub async fn anchor_transfers(&self, min_confirmations: i64) -> Result<Vec<Uuid>> {
        sqlx::query_scalar::<_, Uuid>(
            r#"
            WITH cov AS (
                SELECT MIN(block_time) AS start_time, MAX(height) AS tip FROM pop_scanned_blocks
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
                CROSS JOIN cov
                CROSS JOIN LATERAL (
                    SELECT btc_txid, btc_block_height, btc_block_time, keystone_block
                    FROM pop_publications
                    WHERE valid AND keystone_block >= t.hemi_block
                    ORDER BY btc_block_height, keystone_block, btc_txid
                    LIMIT 1
                ) p
                WHERE t.hemi_block > 0
                  AND t.hemi_time >= cov.start_time + INTERVAL '2 hours'
                  AND cov.tip - p.btc_block_height + 1 >= $1
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

    /// Scan coverage, including the lowest Hemi block the lookup answers for.
    pub async fn coverage(&self) -> Result<PopCoverage> {
        sqlx::query_as::<_, PopCoverage>(
            r#"
            WITH r AS (SELECT MIN(height) AS first_height, MAX(height) AS tip_height
                       FROM pop_scanned_blocks)
            SELECT r.first_height, r.tip_height,
                   (SELECT MAX(p.keystone_block) + 1 FROM pop_publications p
                     WHERE p.valid AND p.btc_block_height < r.first_height + $1) AS min_hemi_block
            FROM r
            "#,
        )
        .bind(COVERAGE_WARMUP_BLOCKS)
        .fetch_one(self.pool)
        .await
        .map_err(StraitError::Database)
    }

    /// The Bitcoin anchor for Hemi block `hemi_block`, or `None` if no valid
    /// publication covers it yet. Callers must check `coverage().min_hemi_block`
    /// first: below it, an earlier publication may exist before the scanned range.
    pub async fn finality_for_hemi_block(&self, hemi_block: i64) -> Result<Option<PopFinalityRow>> {
        sqlx::query_as::<_, PopFinalityRow>(
            r#"
            SELECT p.keystone_block, p.btc_txid, p.vout, p.btc_block_height, p.btc_block_time,
                   (SELECT MAX(height) FROM pop_scanned_blocks) - p.btc_block_height + 1 AS confirmations
            FROM pop_publications p
            WHERE p.valid AND p.keystone_block >= $1
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
