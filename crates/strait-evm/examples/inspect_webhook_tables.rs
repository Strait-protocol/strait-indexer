//! Read-only: dump the column layout and row counts of any pre-existing
//! webhook tables, to diagnose a migration-009 conflict with old scaffolding.

use sqlx::postgres::PgPoolOptions;
use sqlx::Row;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = PgPoolOptions::new()
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;

    let cols = sqlx::query(
        "SELECT table_name, column_name, data_type
         FROM information_schema.columns
         WHERE table_name IN ('webhook_deliveries', 'webhook_subscriptions')
         ORDER BY table_name, ordinal_position",
    )
    .fetch_all(&pool)
    .await?;

    if cols.is_empty() {
        println!("no webhook tables exist");
        return Ok(());
    }
    for c in &cols {
        println!(
            "{}.{} ({})",
            c.try_get::<String, _>("table_name")?,
            c.try_get::<String, _>("column_name")?,
            c.try_get::<String, _>("data_type")?,
        );
    }

    for table in ["webhook_deliveries", "webhook_subscriptions"] {
        if cols
            .iter()
            .any(|c| c.try_get::<String, _>("table_name").ok().as_deref() == Some(table))
        {
            let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await?;
            println!("{table}: {n} row(s)");
        }
    }
    Ok(())
}
