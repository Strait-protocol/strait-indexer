//! Clear the ingestion checkpoint for one chain so the next run starts from
//! `*_START_BLOCK` (or the tip) instead of resuming — used to trigger a backfill.
//!
//! ```bash
//! DATABASE_URL='postgres://...' cargo run -p strait-store --example reset_checkpoint hemi
//! # chain: bitcoin | hemi | ethereum
//! ```

use strait_core::types::Chain;
use strait_store::{init_database, CheckpointRepo};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arg = std::env::args().nth(1).unwrap_or_default();
    let chain = match arg.to_lowercase().as_str() {
        "bitcoin" | "btc" => Chain::Bitcoin,
        "hemi" => Chain::Hemi,
        "ethereum" | "eth" => Chain::Ethereum,
        other => anyhow::bail!("unknown chain '{other}' — use: bitcoin | hemi | ethereum"),
    };

    let url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let db = init_database(&url).await?;
    let removed = CheckpointRepo::new(&db).delete(chain).await?;

    if removed {
        println!("cleared {chain} checkpoint — next run will honor *_START_BLOCK / tip");
    } else {
        println!("no {chain} checkpoint to clear");
    }
    Ok(())
}
