//! Drop all Strait schema objects so migrations can be applied from a clean slate.
//!
//! Useful when a database has partially-applied objects (e.g. a migration was run
//! by hand and failed midway) and `sqlx` refuses to re-run migration 001.
//!
//! ```bash
//! DATABASE_URL='postgres://...' cargo run -p strait-store --example db_reset
//! ```
//!
//! Destructive: removes all indexed data. Intended for development databases.

use strait_store::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let db = Database::connect(&url, 2).await?;

    let statements = [
        "DROP TABLE IF EXISTS tunnel_transfers CASCADE",
        "DROP TABLE IF EXISTS webhook_deliveries CASCADE",
        "DROP TABLE IF EXISTS webhook_subscriptions CASCADE",
        "DROP TABLE IF EXISTS pop_proofs CASCADE",
        "DROP TABLE IF EXISTS events CASCADE",
        "DROP TABLE IF EXISTS checkpoints CASCADE",
        "DROP TABLE IF EXISTS transfers CASCADE",
        "DROP TYPE IF EXISTS chain_type CASCADE",
        "DROP TYPE IF EXISTS transfer_status CASCADE",
        "DROP TYPE IF EXISTS tunnel_direction CASCADE",
        "DROP TYPE IF EXISTS pop_proof_status CASCADE",
        "DROP FUNCTION IF EXISTS update_updated_at_column CASCADE",
        "DROP TABLE IF EXISTS _sqlx_migrations CASCADE",
    ];

    for stmt in statements {
        sqlx::query(stmt).execute(db.pool()).await?;
        println!("ok: {stmt}");
    }

    println!("reset complete — schema is clean");
    Ok(())
}
