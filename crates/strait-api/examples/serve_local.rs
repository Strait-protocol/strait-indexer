//! Local test server for the webhook flow: serves the full HTTP API (including
//! the /webhooks endpoints) AND runs the webhook dispatch loop, without the
//! chain ingesters — pair it with the frontend on localhost to exercise
//! register → manage → deliver end-to-end.
//!
//! Unlike `serve.rs`, this connects with `Database::connect` and does NOT run
//! migrations: applying a feature branch's migrations to a shared database
//! records versions the currently-deployed binary doesn't have, which makes it
//! crash-loop on restart. Schema must already be in place (it is, if the
//! webhooks tables were created manually or the branch is deployed).
//!
//! ```bash
//! DATABASE_URL='postgres://...' cargo run -p strait-api --example serve_local
//! # → API on http://127.0.0.1:8080, dispatcher polling every 5s
//! ```

use strait_core::config::ApiConfig;
use strait_store::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let db = Database::connect(&url, 5).await?;

    let api = ApiConfig {
        host: std::env::var("API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        port: std::env::var("API_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080),
    };

    // Dispatcher drains the webhook outbox so registered URLs actually get POSTs.
    {
        let db = db.clone();
        tokio::spawn(async move {
            strait_api::webhooks::dispatcher::run_dispatch_loop(db).await;
        });
    }

    strait_api::server::serve(api, db).await
}
