//! Chain checkpoint tracking for sync state

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};

use strait_core::error::{Result, StraitError};
use strait_core::types::Chain;

use crate::db::Database;

/// Checkpoint record for tracking chain sync state
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Checkpoint {
    pub id: i32,
    pub chain: String,
    pub block_height: i64,
    pub block_hash: String,
    pub updated_at: DateTime<Utc>,
}

/// Parameters for creating or updating a checkpoint
#[derive(Debug, Clone)]
pub struct UpsertCheckpoint {
    pub chain: Chain,
    pub block_height: i64,
    pub block_hash: String,
}

/// Checkpoint repository
pub struct CheckpointRepo<'a> {
    pool: &'a PgPool,
}

impl<'a> CheckpointRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { pool: db.pool() }
    }

    /// Get checkpoint for a chain
    pub async fn get(&self, chain: Chain) -> Result<Option<Checkpoint>> {
        let chain_name = chain_to_string(chain);
        
        let checkpoint = sqlx::query_as::<_, Checkpoint>(
            "SELECT * FROM checkpoints WHERE chain = $1"
        )
        .bind(chain_name)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(checkpoint)
    }

    /// Upsert checkpoint (insert or update)
    pub async fn upsert(&self, params: UpsertCheckpoint) -> Result<Checkpoint> {
        let chain_name = chain_to_string(params.chain);
        
        let checkpoint = sqlx::query_as::<_, Checkpoint>(
            r#"
            INSERT INTO checkpoints (chain, block_height, block_hash)
            VALUES ($1, $2, $3)
            ON CONFLICT (chain) DO UPDATE
            SET block_height = GREATEST(checkpoints.block_height, EXCLUDED.block_height),
                block_hash   = EXCLUDED.block_hash,
                updated_at   = NOW()
            RETURNING *
            "#
        )
        .bind(chain_name)
        .bind(params.block_height)
        .bind(&params.block_hash)
        .fetch_one(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(checkpoint)
    }

    /// Get all checkpoints
    pub async fn get_all(&self) -> Result<Vec<Checkpoint>> {
        let checkpoints = sqlx::query_as::<_, Checkpoint>(
            "SELECT * FROM checkpoints ORDER BY chain"
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(checkpoints)
    }

    /// Delete checkpoint for a chain
    pub async fn delete(&self, chain: Chain) -> Result<bool> {
        let chain_name = chain_to_string(chain);
        
        let result = sqlx::query(
            "DELETE FROM checkpoints WHERE chain = $1"
        )
        .bind(chain_name)
        .execute(self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;

        Ok(result.rows_affected() > 0)
    }
}

/// Convert Chain enum to database string
fn chain_to_string(chain: Chain) -> &'static str {
    match chain {
        Chain::Bitcoin => "bitcoin",
        Chain::Ethereum => "ethereum",
        Chain::Hemi => "hemi",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_to_string() {
        assert_eq!(chain_to_string(Chain::Bitcoin), "bitcoin");
        assert_eq!(chain_to_string(Chain::Ethereum), "ethereum");
        assert_eq!(chain_to_string(Chain::Hemi), "hemi");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL; verifies checkpoint persistence/resume against a live DB"]
    async fn db_checkpoint_persist_and_resume() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set — skipping");
            return;
        };
        let db = crate::init_database(&url).await.expect("connect + migrate");
        let repo = CheckpointRepo::new(&db);
        let _ = repo.delete(Chain::Hemi).await;
        let _ = repo.delete(Chain::Ethereum).await;

        // First persist.
        repo.upsert(UpsertCheckpoint {
            chain: Chain::Hemi,
            block_height: 100,
            block_hash: "0xaaa".into(),
        })
        .await
        .unwrap();
        let cp = repo.get(Chain::Hemi).await.unwrap().unwrap();
        assert_eq!(cp.chain, "hemi");
        assert_eq!(cp.block_height, 100);

        // Advance.
        repo.upsert(UpsertCheckpoint {
            chain: Chain::Hemi,
            block_height: 250,
            block_hash: "0xbbb".into(),
        })
        .await
        .unwrap();
        assert_eq!(repo.get(Chain::Hemi).await.unwrap().unwrap().block_height, 250);

        // A lower height must not regress the checkpoint (GREATEST).
        repo.upsert(UpsertCheckpoint {
            chain: Chain::Hemi,
            block_height: 200,
            block_hash: "0xccc".into(),
        })
        .await
        .unwrap();
        assert_eq!(repo.get(Chain::Hemi).await.unwrap().unwrap().block_height, 250);

        // Hemi and Ethereum are tracked independently.
        repo.upsert(UpsertCheckpoint {
            chain: Chain::Ethereum,
            block_height: 7,
            block_hash: "0xeee".into(),
        })
        .await
        .unwrap();
        assert_eq!(repo.get(Chain::Ethereum).await.unwrap().unwrap().block_height, 7);
        assert_eq!(repo.get(Chain::Hemi).await.unwrap().unwrap().block_height, 250);

        repo.delete(Chain::Hemi).await.unwrap();
        repo.delete(Chain::Ethereum).await.unwrap();
        eprintln!("DB checkpoint persist/resume OK");
    }
}