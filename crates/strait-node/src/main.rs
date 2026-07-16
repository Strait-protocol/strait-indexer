//! Strait Node — wires all components into a single running indexer process.
//!
//! Pipeline:
//!
//! ```text
//!   Hemi RPC ─┐
//!             ├─► EvmIngester ──┐
//!   Eth  RPC ─┘                 │  RawEvent
//!                               ▼
//!                          JoinEngine ──► TunnelTransferUpdate ──► store writer
//!                                                                       │
//!   Postgres / Supabase ◄─────────────────────────────────────────────┘
//!                               ▲
//!                          HTTP API (axum) ── /health, /transfers
//! ```

mod payout_watcher;

use std::sync::Arc;

use alloy::providers::{Provider, ProviderBuilder};
use anyhow::Context;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use strait_bitcoin::{BitcoinIngester, BitcoinKitCaller, CustodyWatcher};
use strait_core::config::AppConfig;
use strait_core::events::RawEvent;
use strait_core::types::Chain;
use strait_evm::ingester::EvmIngester;
use strait_join::engine::{JoinEngine, TunnelTransferUpdate};
use strait_store::{init_database, status_str, Database, TunnelTransferRepo};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Tracing ──────────────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            // Clean operational default: meaningful events only. Per-block tracing
            // lives at DEBUG — opt in with e.g. RUST_LOG=strait_evm=debug.
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")),
        )
        .init();

    info!("Strait node starting up");

    // 1. Configuration
    let config = AppConfig::load().context("failed to load configuration")?;
    info!(
        hemi_chain = config.hemi.chain_id,
        eth_chain = config.ethereum.chain_id,
        "Configuration loaded"
    );

    // 2. Database + migrations (Supabase-ready)
    let db = init_database(&config.database.url)
        .await
        .context("database init / migrations failed")?;
    info!("Database connected and migrations applied");

    // 3. Channels between stages
    let (event_tx, event_rx) = mpsc::channel::<RawEvent>(2048);
    let (update_tx, update_rx) = mpsc::channel::<TunnelTransferUpdate>(2048);

    // 4. RPC providers
    let hemi_provider = build_provider(&config.hemi.rpc_url).context("invalid HEMI_RPC_URL")?;
    let eth_provider = build_provider(&config.ethereum.rpc_url).context("invalid ETH_RPC_URL")?;

    // 5. EVM ingesters (Hemi + Ethereum) feeding the shared event channel.
    // BitcoinKit precompile calls (UTXO lookups, tx outputs) are read-only eth_calls
    // that work on the public Hemi RPC. Use a separate provider so custody watcher
    // and payout watcher don't burn the premium QuickNode quota used by eth_getLogs.
    let bitcoin_kit_provider: Arc<dyn Provider> = match &config.bitcoin.bitcoin_kit_rpc_url {
        Some(url) => build_provider(url).context("invalid HEMI_BITCOIN_KIT_RPC_URL")?,
        None => hemi_provider.clone(),
    };
    // Payout watcher gets its own provider so custody watcher and payout watcher
    // hit independent rate-limit pools. Falls back to bitcoin_kit_provider if unset.
    let payout_kit_provider: Arc<dyn Provider> = match &config.bitcoin.payout_kit_rpc_url {
        Some(url) => build_provider(url).context("invalid HEMI_PAYOUT_KIT_RPC_URL")?,
        None => bitcoin_kit_provider.clone(),
    };
    let hemi_ingester = EvmIngester::new(
        config.hemi.clone(),
        Chain::Hemi,
        hemi_provider,
        db.clone(),
        event_tx.clone(),
    );
    let eth_ingester = EvmIngester::new(
        config.ethereum.clone(),
        Chain::Ethereum,
        eth_provider,
        db.clone(),
        event_tx.clone(),
    );

    // 6. Bitcoin custody watcher (optional — only if custody addresses are configured)
    let bitcoin_ingester = build_bitcoin_ingester(&config, bitcoin_kit_provider, event_tx.clone());

    // Drop the spare sender so the event channel closes once the ingesters stop.
    drop(event_tx);

    // 7. Join engine — consumes RawEvent, emits TunnelTransferUpdate
    let engine = JoinEngine::new(event_rx, update_tx, db.clone());

    // 8. Spawn every long-running component
    let mut tasks = tokio::task::JoinSet::new();

    tasks.spawn(async move {
        if let Err(e) = hemi_ingester.run().await {
            error!("Hemi ingester exited: {e}");
        }
    });
    tasks.spawn(async move {
        if let Err(e) = eth_ingester.run().await {
            error!("Ethereum ingester exited: {e}");
        }
    });
    if let Some(btc_ingester) = bitcoin_ingester {
        tasks.spawn(async move {
            if let Err(e) = btc_ingester.run().await {
                error!("Bitcoin ingester exited: {e}");
            }
        });
    }
    // Bitcoin payout watcher — finalizes Hemi→BTC withdrawals once their payout is
    // observed. Runs unconditionally (it reads recipients from the DB; no custody
    // addresses needed) and reads Bitcoin state through BitcoinKit on Hemi.
    {
        let caller = build_bitcoin_kit_caller(&config, payout_kit_provider.clone());
        let db = db.clone();
        let interval = config.bitcoin.poll_interval_secs;
        let confs = config.bitcoin.confirmation_depth;
        let vault_contracts: Vec<alloy::primitives::Address> = config.bitcoin.vault_contracts
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        if vault_contracts.is_empty() {
            warn!("HEMI_VAULT_CONTRACTS not set — vault sweep detection disabled (Phase 3)");
        }
        tasks.spawn(async move {
            let watcher = payout_watcher::BtcPayoutWatcher::new(caller, payout_kit_provider, db, interval, confs, vault_contracts);
            if let Err(e) = watcher.run().await {
                error!("BTC payout watcher exited: {e}");
            }
        });
    }
    tasks.spawn(async move {
        if let Err(e) = engine.run().await {
            error!("Join engine exited: {e}");
        }
    });
    {
        let db = db.clone();
        tasks.spawn(async move {
            store_writer(db, update_rx).await;
        });
    }
    {
        let api_config = config.api.clone();
        let db = db.clone();
        tasks.spawn(async move {
            if let Err(e) = strait_api::server::serve(api_config, db).await {
                error!("API server exited: {e}");
            }
        });
    }
    // Webhook dispatcher — drains the webhook_deliveries outbox that the store
    // writer fills, POSTing HMAC-signed payloads to registered subscribers.
    {
        let db = db.clone();
        tasks.spawn(async move {
            strait_api::webhooks::dispatcher::run_dispatch_loop(db).await;
        });
    }

    info!(
        api = %format!("http://{}:{}", config.api.host, config.api.port),
        "All components started — indexing live. Press Ctrl-C to stop."
    );

    // 8. Run until Ctrl-C. If an individual component task exits (e.g. a chain's
    //    RPC is misconfigured), log it and keep the rest of the node running —
    //    one bad ingester must not take down the API or the healthy chains.
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received — stopping");
                break;
            }
            joined = tasks.join_next() => {
                match joined {
                    Some(Ok(())) => warn!("A component task exited — other components keep running"),
                    Some(Err(e)) => error!("A component task panicked: {e} — other components keep running"),
                    None => {
                        warn!("All component tasks have exited — nothing left to do, stopping");
                        break;
                    }
                }
            }
        }
    }

    tasks.shutdown().await;
    info!("Strait node stopped");
    Ok(())
}

/// Build the Bitcoin custody watcher ingester, if custody addresses are configured.
///
/// Reads Bitcoin state through the BitcoinKit precompile on Hemi (no separate
/// Bitcoin node required), using the configured `HEMI_BITCOIN_KIT_CONTRACT` address.
fn build_bitcoin_ingester(
    config: &AppConfig,
    hemi_provider: Arc<dyn Provider>,
    event_tx: tokio::sync::mpsc::Sender<RawEvent>,
) -> Option<BitcoinIngester> {
    if config.bitcoin.tunnel_addresses.is_empty() {
        info!("No BITCOIN_TUNNEL_ADDRESSES configured — Bitcoin custody watcher disabled");
        return None;
    }

    let caller = build_bitcoin_kit_caller(config, hemi_provider);
    let watcher = CustodyWatcher::new(caller, config.bitcoin.tunnel_addresses.clone());
    info!(
        addresses = config.bitcoin.tunnel_addresses.len(),
        "Bitcoin custody watcher enabled"
    );
    Some(BitcoinIngester::new(
        watcher,
        event_tx,
        config.bitcoin.poll_interval_secs,
    ))
}

/// Resolve the BitcoinKit precompile caller for the configured Hemi chain, using
/// `HEMI_BITCOIN_KIT_CONTRACT` if set, else the well-known mainnet/testnet address.
fn build_bitcoin_kit_caller(config: &AppConfig, hemi_provider: Arc<dyn Provider>) -> BitcoinKitCaller {
    match config
        .hemi
        .bitcoin_kit_contract
        .as_deref()
        .and_then(|s| s.parse().ok())
    {
        Some(addr) => BitcoinKitCaller::new(hemi_provider, addr),
        None if config.hemi.chain_id == 743111 => BitcoinKitCaller::testnet(hemi_provider),
        None => BitcoinKitCaller::mainnet(hemi_provider),
    }
}

/// Build a read-only HTTP JSON-RPC provider for an EVM chain.
fn build_provider(rpc_url: &str) -> anyhow::Result<Arc<dyn Provider>> {
    // `.boxed()` erases the transport to `BoxTransport`, matching the
    // `Arc<dyn Provider>` (i.e. `dyn Provider<BoxTransport, Ethereum>`) the
    // ingester stores.
    let provider = ProviderBuilder::new()
        .on_http(rpc_url.parse().context("invalid RPC URL")?)
        .boxed();
    Ok(Arc::new(provider))
}

/// Persistence sink for transfer lifecycle updates emitted by the join engine.
///
/// Applies each update to the `tunnel_transfers` table: `Created` upserts the row,
/// `StatusChanged` / `Retracted` update its status, and `PopAnchored` records the
/// Bitcoin-finality fields.
async fn store_writer(db: Database, mut rx: mpsc::Receiver<TunnelTransferUpdate>) {
    use strait_api::webhooks::dispatcher::{self, event};

    info!("Store writer started");
    while let Some(update) = rx.recv().await {
        let repo = TunnelTransferRepo::new(&db);
        let result = match &update {
            TunnelTransferUpdate::Created(t) => repo.upsert(t).await,
            TunnelTransferUpdate::StatusChanged {
                id,
                new_status,
                updated_at,
            } => {
                repo.update_status(*id, status_str(new_status), Some(*updated_at))
                    .await
            }
            TunnelTransferUpdate::PopAnchored {
                id,
                keystone_block,
                pop_score,
                anchored_at,
            } => {
                repo.set_pop_anchor(*id, *keystone_block as i64, *pop_score as i64, *anchored_at)
                    .await
            }
            TunnelTransferUpdate::Retracted {
                id, retracted_at, ..
            } => repo.update_status(*id, "REORGED", Some(*retracted_at)).await,
            // Destination enrichment lands with the source-side ingesters.
            TunnelTransferUpdate::DestinationConfirmed { .. } => Ok(()),
            TunnelTransferUpdate::EthWithdrawalFinalized {
                id,
                dest_tx_hash,
                dest_block,
                dest_fee,
                finalized_at,
            } => {
                repo.set_eth_withdrawal_finalized(*id, dest_tx_hash, *dest_block, dest_fee.clone(), *finalized_at)
                    .await
            }
        };

        if let Err(e) = result {
            error!("Store writer failed to persist update: {e}");
            continue;
        }

        // Enqueue webhook deliveries for the persisted change (outbox insert
        // only — the dispatcher task does the actual HTTP). Failures here are
        // logged, never allowed to stall indexing.
        let (transfer_id, event_type) = match &update {
            TunnelTransferUpdate::Created(t) => (t.id, event::CREATED),
            TunnelTransferUpdate::StatusChanged { id, .. } => (*id, event::STATUS_CHANGED),
            TunnelTransferUpdate::PopAnchored { id, .. } => (*id, event::POP_ANCHORED),
            TunnelTransferUpdate::Retracted { id, .. } => (*id, event::RETRACTED),
            TunnelTransferUpdate::DestinationConfirmed { .. } => continue, // no write happened
            TunnelTransferUpdate::EthWithdrawalFinalized { id, .. } => {
                (*id, event::STATUS_CHANGED)
            }
        };
        match repo.get(transfer_id).await {
            Ok(Some(row)) => {
                if let Err(e) = dispatcher::enqueue(&db, event_type, &row).await {
                    error!(transfer = %transfer_id, "Webhook enqueue failed: {e}");
                }
            }
            Ok(None) => {} // row vanished (shouldn't happen) — nothing to notify
            Err(e) => error!(transfer = %transfer_id, "Webhook enqueue lookup failed: {e}"),
        }
    }
    info!("Store writer stopped — update channel closed");
}
