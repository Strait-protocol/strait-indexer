//! Read-only diagnostic: print every log in a transfer's source/dest tx receipts,
//! to see why `backfill_erc20_symbols` couldn't find an ERC20Bridge log for it.
//!
//! ```bash
//! DATABASE_URL='postgres://...' HEMI_RPC_URL='https://...' ETH_RPC_URL='https://...' \
//!     cargo run -p strait-evm --example diagnose_row -- <transfer-uuid>
//! ```

use alloy::primitives::B256;
use alloy::providers::{Provider, ProviderBuilder};
use sqlx::Row;

use strait_store::init_database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let id = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: diagnose_row <transfer-uuid>"))?;

    let database_url = std::env::var("DATABASE_URL")?;
    let hemi_rpc_url = std::env::var("HEMI_RPC_URL")?;
    let eth_rpc_url = std::env::var("ETH_RPC_URL")?;

    let db = init_database(&database_url).await?;
    let hemi_provider = ProviderBuilder::new().on_http(hemi_rpc_url.parse()?).boxed();
    let eth_provider = ProviderBuilder::new().on_http(eth_rpc_url.parse()?).boxed();

    let row = sqlx::query(
        "SELECT asset, direction, route, amount::text, status, \
                source_chain, source_tx_hash, source_block, \
                dest_chain, dest_tx_hash, dest_block \
         FROM tunnel_transfers WHERE id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(db.pool())
    .await?;

    println!(
        "asset={:?} direction={} route={} amount={} status={}",
        row.try_get::<String, _>("asset")?,
        row.try_get::<String, _>("direction")?,
        row.try_get::<String, _>("route")?,
        row.try_get::<String, _>("amount")?,
        row.try_get::<String, _>("status")?,
    );

    for (chain_col, tx_col, block_col) in [
        ("source_chain", "source_tx_hash", "source_block"),
        ("dest_chain", "dest_tx_hash", "dest_block"),
    ] {
        let chain: Option<String> = row.try_get(chain_col)?;
        let tx: Option<String> = row.try_get(tx_col)?;
        let block: Option<i64> = row.try_get(block_col).ok().flatten();
        println!("\n-- {chain_col}={chain:?} {tx_col}={tx:?} {block_col}={block:?}");

        let (Some(chain), Some(tx)) = (chain, tx) else { continue };
        let provider: &dyn Provider = match chain.as_str() {
            "HEMI" => &hemi_provider,
            "ETHEREUM" => &eth_provider,
            other => {
                println!("  (chain {other} — no EVM provider, skipping)");
                continue;
            }
        };

        let Ok(hash) = tx.parse::<B256>() else {
            println!("  (bad tx hash format)");
            continue;
        };
        match strait_evm::contracts::fetch_receipt_logs(provider, hash).await {
            Some(logs) => {
                println!("  receipt found, {} log(s)", logs.len());
                for (address, topics) in logs {
                    println!("  log: address={address:?} topics={topics:?}");
                }
            }
            None => println!("  receipt not found or RPC call failed"),
        }
    }

    Ok(())
}
