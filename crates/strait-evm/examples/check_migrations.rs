//! Read-only: print the `_sqlx_migrations` table so we can diagnose a
//! "migration N was previously applied but is missing in the resolved
//! migrations" error without a psql client on hand.
//!
//! ```bash
//! DATABASE_URL='postgres://...' cargo run -p strait-evm --example check_migrations
//! ```

use sqlx::Row;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL")?;
    let pool = PgPoolOptions::new().connect(&database_url).await?;

    let rows = sqlx::query(
        "SELECT version, description, success, checksum, installed_on \
         FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await?;

    for row in rows {
        let version: i64 = row.try_get("version")?;
        let description: String = row.try_get("description")?;
        let success: bool = row.try_get("success")?;
        let checksum: Vec<u8> = row.try_get("checksum")?;
        println!(
            "version={version} description={description:?} success={success} checksum={}",
            hex::encode(&checksum)
        );
    }
    Ok(())
}
