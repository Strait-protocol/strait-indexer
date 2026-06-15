//! Run only the Strait HTTP API (REST + GraphQL) against a database, without the
//! ingesters. Useful for developing the dashboard or exploring indexed data.
//!
//! ```bash
//! DATABASE_URL='postgres://...' cargo run -p strait-api --example serve
//! # → http://127.0.0.1:8080/graphql
//! ```

use strait_core::config::ApiConfig;
use strait_store::init_database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let db = init_database(&url).await?;

    let api = ApiConfig {
        host: std::env::var("API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        port: std::env::var("API_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080),
    };

    println!("Strait API on http://{}:{}  (GraphQL at /graphql)", api.host, api.port);
    strait_api::server::serve(api, db).await
}
