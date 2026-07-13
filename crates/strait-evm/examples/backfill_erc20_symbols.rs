//! One-off backfill for `tunnel_transfers` rows written before the ERC-20
//! symbol/decimals resolution fix — those rows have `asset = ''` because the
//! ingester used to hardcode an empty symbol for any non-native-ETH bridged
//! token (see `EvmIngester::resolve_erc20_metadata`).
//!
//! For each affected row, re-fetches the source (or dest, if source doesn't
//! carry the bridge log) transaction receipt, recovers the token contract
//! address from the `ERC20Bridge{Initiated,Finalized}` log's indexed
//! `localToken` topic, resolves its symbol on-chain, and updates the row.
//! Rows whose token can't be resolved are left untouched and reported.
//!
//! ```bash
//! DATABASE_URL='postgres://...' HEMI_RPC_URL='https://...' ETH_RPC_URL='https://...' \
//!     cargo run -p strait-evm --example backfill_erc20_symbols
//! ```

use std::collections::HashMap;

use alloy::primitives::{Address as AlloyAddress, B256};
use alloy::providers::{Provider, ProviderBuilder};
use sqlx::Row;

use strait_evm::contracts::{fetch_erc20_symbol, topics};
use strait_store::init_database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let hemi_rpc_url =
        std::env::var("HEMI_RPC_URL").map_err(|_| anyhow::anyhow!("HEMI_RPC_URL must be set"))?;
    let eth_rpc_url =
        std::env::var("ETH_RPC_URL").map_err(|_| anyhow::anyhow!("ETH_RPC_URL must be set"))?;

    let db = init_database(&database_url).await?;
    let hemi_provider = build_provider(&hemi_rpc_url)?;
    let eth_provider = build_provider(&eth_rpc_url)?;

    let rows = sqlx::query(
        "SELECT id, source_chain, source_tx_hash, dest_chain, dest_tx_hash \
         FROM tunnel_transfers WHERE asset = ''",
    )
    .fetch_all(db.pool())
    .await?;

    println!("{} row(s) with an unresolved ERC-20 symbol", rows.len());

    let mut symbol_cache: HashMap<AlloyAddress, Option<String>> = HashMap::new();
    let (mut fixed, mut skipped) = (0, 0);

    for row in rows {
        let id: uuid::Uuid = row.try_get("id")?;
        let legs = [
            (row.try_get::<String, _>("source_chain")?, row.try_get::<String, _>("source_tx_hash")?),
        ]
        .into_iter()
        .chain(
            match (
                row.try_get::<Option<String>, _>("dest_chain")?,
                row.try_get::<Option<String>, _>("dest_tx_hash")?,
            ) {
                (Some(chain), Some(tx)) => vec![(chain, tx)],
                _ => vec![],
            },
        );

        let mut resolved = None;
        for (chain, tx_hash) in legs {
            let provider: &dyn Provider = match chain.as_str() {
                "HEMI" => &*hemi_provider,
                "ETHEREUM" => &*eth_provider,
                _ => continue, // BTC legs never carry an ERC-20 bridge log
            };
            if let Some(token) = find_local_token(provider, &tx_hash).await {
                resolved = Some((provider, token));
                break;
            }
        }

        let Some((provider, token)) = resolved else {
            println!("  skip {id}: no ERC20Bridge log found on either leg");
            skipped += 1;
            continue;
        };

        let symbol = match symbol_cache.get(&token) {
            Some(cached) => cached.clone(),
            None => {
                let s = fetch_erc20_symbol(provider, token).await;
                symbol_cache.insert(token, s.clone());
                s
            }
        };

        let Some(symbol) = symbol.filter(|s| !s.is_empty()) else {
            println!("  skip {id}: symbol() call failed for token {token}");
            skipped += 1;
            continue;
        };

        sqlx::query("UPDATE tunnel_transfers SET asset = $1 WHERE id = $2")
            .bind(&symbol)
            .bind(id)
            .execute(db.pool())
            .await?;
        println!("  fixed {id}: asset = {symbol} (token {token})");
        fixed += 1;
    }

    println!("done — fixed {fixed}, skipped {skipped}");
    Ok(())
}

/// Fetch `tx_hash`'s receipt and return the `localToken` address from its
/// `ERC20Bridge{Initiated,Finalized}` log, if any. Uses a raw JSON-RPC call
/// (`fetch_receipt_logs`) rather than the typed `get_transaction_receipt`, since
/// OP-Stack deposit transactions (type `0x7e`) fail typed receipt decoding.
async fn find_local_token(provider: &dyn Provider, tx_hash: &str) -> Option<AlloyAddress> {
    let hash: B256 = tx_hash.parse().ok()?;
    let logs = strait_evm::contracts::fetch_receipt_logs(provider, hash).await?;
    logs.into_iter().find_map(|(_, t)| {
        let topic0 = *t.first()?;
        if topic0 == topics::erc20_bridge_finalized() || topic0 == topics::erc20_bridge_initiated() {
            t.get(1).map(|t| AlloyAddress::from_slice(&t.0[12..]))
        } else {
            None
        }
    })
}

fn build_provider(rpc_url: &str) -> anyhow::Result<std::sync::Arc<dyn Provider>> {
    let provider = ProviderBuilder::new().on_http(rpc_url.parse()?).boxed();
    Ok(std::sync::Arc::new(provider))
}
