//! Fire a test webhook event: enqueues a `transfer.status_changed` delivery
//! for the most recent indexed transfer, to every matching active
//! subscription. Pair with `serve_local` (whose dispatch loop will pick it up
//! within ~5s and POST it) to test a registered endpoint without waiting for
//! real on-chain activity.
//!
//! ```bash
//! DATABASE_URL='postgres://...' cargo run -p strait-api --example fire_test_webhook
//! ```

use strait_api::webhooks::dispatcher::{self, event};
use strait_store::{Database, TunnelTransferRepo};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    // No migrations on purpose — see serve_local.rs.
    let db = Database::connect(&url, 2).await?;

    let transfer = TunnelTransferRepo::new(&db)
        .list(1, 0)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no transfers in DB"))?;

    println!(
        "enqueueing {} for transfer {} ({} {} {})",
        event::STATUS_CHANGED,
        transfer.id,
        transfer.route,
        transfer.asset,
        transfer.status
    );
    dispatcher::enqueue(&db, event::STATUS_CHANGED, &transfer).await?;
    println!("done — a running dispatcher (serve_local / strait-node) will deliver it within ~5s");
    Ok(())
}
