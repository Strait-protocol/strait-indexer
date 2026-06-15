//! Seed a few representative tunnel transfers for dashboard development/demos.
//!
//! ```bash
//! DATABASE_URL='postgres://...' cargo run -p strait-store --example seed
//! ```
//!
//! Idempotent (fixed ids). Remove with `cargo run -p strait-store --example db_reset`
//! (which clears all data) or delete the three `seed-*` ids by hand.

use chrono::{Duration, Utc};
use strait_store::{init_database, TunnelTransferRepo};
use strait_core::types::*;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
fn transfer(
    id: &str,
    asset: Asset,
    direction: TunnelDirection,
    route: TunnelRoute,
    amount: u64,
    sender: ChainAddress,
    recipient: ChainAddress,
    status: TunnelStatus,
    source: ChainTransaction,
    destination: Option<ChainTransaction>,
    pop_keystone: Option<u64>,
) -> TunnelTransfer {
    let now = Utc::now();
    TunnelTransfer {
        id: Uuid::parse_str(id).unwrap(),
        asset,
        direction,
        route,
        amount: bigdecimal::BigDecimal::from(amount),
        source_fee: None,
        dest_fee: None,
        withdrawal_uuid: None,
        sender,
        recipient,
        status,
        initiated_at: now - Duration::minutes(20),
        finalized_at: None,
        source_tx: source,
        destination_tx: destination,
        pop_anchored: pop_keystone.is_some(),
        pop_keystone_block: pop_keystone,
        pop_score: pop_keystone.map(|_| 4200),
        pop_anchored_at: pop_keystone.map(|_| now),
        reorg_events: vec![],
    }
}

fn btc_tx(block: u64) -> ChainTransaction {
    ChainTransaction {
        chain: Chain::Bitcoin,
        hash: ChainTxHash::Bitcoin(BitcoinTxid([0x11; 32])),
        block_number: block,
        block_hash: BlockHash([0; 32]),
        timestamp: Utc::now(),
        confirmations: 6,
    }
}

fn hemi_tx(block: u64) -> ChainTransaction {
    ChainTransaction {
        chain: Chain::Hemi,
        hash: ChainTxHash::Evm(TxHash([0x22; 32])),
        block_number: block,
        block_hash: BlockHash([0; 32]),
        timestamp: Utc::now(),
        confirmations: 3,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let db = init_database(&url).await?;
    let repo = TunnelTransferRepo::new(&db);

    let evm = |b: u8| ChainAddress::Evm(Address([b; 20]));
    let btc = |s: &str| ChainAddress::Bitcoin(BitcoinAddress::new(s));

    let transfers = vec![
        // BTC → Hemi, anchored to Bitcoin via PoP.
        transfer(
            "5eed0001-0000-4000-8000-000000000001",
            Asset::Btc,
            TunnelDirection::In,
            TunnelRoute::BtcToHemi,
            50_000_000, // 0.5 BTC
            btc("btctx:demo"),
            evm(0xab),
            TunnelStatus::Anchored,
            btc_tx(800_121),
            Some(hemi_tx(6_037_628)),
            Some(6_037_650),
        ),
        // ETH → Hemi, finalized.
        transfer(
            "5eed0002-0000-4000-8000-000000000002",
            Asset::Eth,
            TunnelDirection::In,
            TunnelRoute::EthToHemi,
            130_000_000_000_000_000, // 0.13 ETH
            evm(0xcd),
            evm(0xcd),
            TunnelStatus::Finalized,
            ChainTransaction {
                chain: Chain::Ethereum,
                hash: ChainTxHash::Evm(TxHash([0x33; 32])),
                block_number: 0,
                block_hash: BlockHash([0; 32]),
                timestamp: Utc::now(),
                confirmations: 12,
            },
            Some(hemi_tx(6_037_500)),
            Some(6_037_500),
        ),
        // Hemi → BTC withdrawal, still initiated.
        transfer(
            "5eed0003-0000-4000-8000-000000000003",
            Asset::Btc,
            TunnelDirection::Out,
            TunnelRoute::HemiToBtc,
            10_000_000, // 0.1 BTC
            evm(0xef),
            btc("tb1qdemo..."),
            TunnelStatus::Initiated,
            hemi_tx(6_037_700),
            None,
            None,
        ),
    ];

    for t in &transfers {
        repo.upsert(t).await?;
        println!("seeded {} ({})", t.id, t.route);
    }
    println!("done — {} transfers", transfers.len());
    Ok(())
}
