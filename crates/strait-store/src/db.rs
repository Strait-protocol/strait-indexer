//! Database connection and initialization

use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
use sqlx::migrate::Migrator;
use std::str::FromStr;
use std::sync::OnceLock;

use strait_core::error::{Result, StraitError};

static MIGRATOR: OnceLock<Migrator> = OnceLock::new();

/// Get the embedded migrator
async fn get_migrator() -> &'static Migrator {
    MIGRATOR.get_or_init(|| {
        // Embed migrations at compile time using absolute path
        sqlx::migrate!("./migrations")
    })
}

/// Database connection pool wrapper
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Create a new database connection pool.
    ///
    /// Works with local Postgres and with Supabase. Supabase requires TLS, so the
    /// SSL mode is upgraded to at least `Require` unless the URL explicitly opts out
    /// (`sslmode=disable`, used for local development).
    ///
    /// When connecting through Supabase's transaction-mode connection pooler
    /// (PgBouncer, host contains `pooler.supabase.com` or port `6543`), the
    /// prepared-statement cache is disabled because that pooler does not support
    /// server-side prepared statements.
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self> {
        let mut opts = PgConnectOptions::from_str(database_url)
            .map_err(|e| StraitError::Database(e))?;

        // Supabase mandates SSL. Only leave SSL off if the URL explicitly disabled it.
        let explicitly_disabled = database_url.contains("sslmode=disable");
        if !explicitly_disabled && matches!(opts.get_ssl_mode(), PgSslMode::Prefer) {
            opts = opts.ssl_mode(PgSslMode::Require);
        }

        // The transaction-mode pooler (PgBouncer) cannot reuse prepared statements
        // across pooled connections, so disable statement caching for it.
        let host = opts.get_host().to_string();
        let uses_txn_pooler =
            host.contains("pooler.supabase.com") || opts.get_port() == 6543;
        if uses_txn_pooler {
            opts = opts.statement_cache_capacity(0);
        }

        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .min_connections(2)
            .acquire_timeout(std::time::Duration::from_secs(30))
            .idle_timeout(std::time::Duration::from_secs(300))
            .max_lifetime(std::time::Duration::from_secs(1800))
            .connect_with(opts)
            .await
            .map_err(|e| StraitError::Database(e))?;

        Ok(Self { pool })
    }

    /// Run database migrations
    pub async fn migrate(&self) -> Result<()> {
        let migrator = get_migrator().await;
        migrator
            .run(&self.pool)
            .await
            .map_err(|e| StraitError::Migration(e.to_string()))?;
        Ok(())
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Check database connectivity
    pub async fn health_check(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| StraitError::Database(e))?;
        Ok(())
    }

    /// Get current database size (for monitoring)
    pub async fn database_size(&self) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT pg_database_size(current_database())"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?;
        
        Ok(row.0)
    }

    /// Get table row counts (for monitoring)
    pub async fn table_stats(&self) -> Result<TableStats> {
        let transfers = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM transfers"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?
        .0;

        let events = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM events"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?
        .0;

        let pop_proofs = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM pop_proofs"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StraitError::Database(e))?
        .0;

        Ok(TableStats {
            transfers,
            events,
            pop_proofs,
        })
    }

    /// Close the connection pool gracefully
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Table statistics for monitoring
#[derive(Debug, Clone)]
pub struct TableStats {
    pub transfers: i64,
    pub events: i64,
    pub pop_proofs: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_stats_debug() {
        let stats = TableStats {
            transfers: 100,
            events: 500,
            pop_proofs: 50,
        };
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("transfers: 100"));
    }
}