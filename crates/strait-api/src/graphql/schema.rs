//! GraphQL schema: types, the Query root, and the schema builder.

use async_graphql::{
    Context, Enum, EmptyMutation, EmptySubscription, Object, Schema, SimpleObject,
};
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use strait_store::{AnalyticsBucketRow, Database, TunnelTransferRepo, TunnelTransferRow};

/// The assembled Strait GraphQL schema type.
pub type StraitSchema = Schema<Query, EmptyMutation, EmptySubscription>;

/// A cross-chain tunnel transfer, as indexed by Strait.
#[derive(SimpleObject)]
pub struct Transfer {
    /// Deterministic id derived from the Hemi tx hash + log index.
    pub id: Uuid,
    /// Asset symbol: BTC, ETH, or an ERC-20 symbol.
    pub asset: String,
    /// IN (into Hemi) or OUT (out of Hemi).
    pub direction: String,
    /// BTC_TO_HEMI | HEMI_TO_BTC | ETH_TO_HEMI | HEMI_TO_ETH.
    pub route: String,
    /// Amount in atomic units (satoshis or wei), as a plain decimal string.
    pub amount: String,
    pub sender: String,
    pub recipient: String,
    /// INITIATED | ANCHORED | FINALIZED | FAILED | REORGED.
    pub status: String,

    pub source_chain: String,
    pub source_tx_hash: String,
    pub source_block: i64,
    pub source_timestamp: DateTime<Utc>,

    pub dest_chain: Option<String>,
    pub dest_tx_hash: Option<String>,
    pub dest_block: Option<i64>,

    /// Network fee on the source leg, atomic units (wei or sats) as a string, or null.
    pub source_fee: Option<String>,
    /// Network fee on the destination leg, atomic units (wei or sats) as a string, or null.
    pub dest_fee: Option<String>,

    /// Whether this transfer has been PoP-anchored to Bitcoin.
    pub pop_anchored: bool,
    /// The Hemi keystone block (multiple of 25) that anchored it.
    pub pop_keystone_block: Option<i64>,
    /// Aggregate PoP score of the anchoring keystone (0 = anchored, no publications).
    pub pop_score: Option<i64>,
    pub pop_anchored_at: Option<DateTime<Utc>>,

    pub initiated_at: DateTime<Utc>,
    pub finalized_at: Option<DateTime<Utc>>,
}

impl From<TunnelTransferRow> for Transfer {
    fn from(r: TunnelTransferRow) -> Self {
        Self {
            id: r.id,
            asset: r.asset,
            direction: r.direction,
            route: r.route,
            // Plain integer form, never scientific notation.
            amount: r.amount.with_scale(0).to_string(),
            sender: r.sender,
            recipient: r.recipient,
            status: r.status,
            source_chain: r.source_chain,
            source_tx_hash: r.source_tx_hash,
            source_block: r.source_block,
            source_timestamp: r.source_timestamp,
            dest_chain: r.dest_chain,
            dest_tx_hash: r.dest_tx_hash,
            dest_block: r.dest_block,
            source_fee: r.source_fee.map(|d| d.with_scale(0).to_string()),
            dest_fee: r.dest_fee.map(|d| d.with_scale(0).to_string()),
            pop_anchored: r.pop_anchored,
            pop_keystone_block: r.pop_keystone_block,
            pop_score: r.pop_score,
            pop_anchored_at: r.pop_anchored_at,
            initiated_at: r.initiated_at,
            finalized_at: r.finalized_at,
        }
    }
}

/// Aggregate indexer statistics.
#[derive(SimpleObject)]
pub struct Stats {
    /// Total number of indexed transfers (all time).
    pub total_transfers: i64,
    /// Transfers currently in INITIATED state.
    pub initiated: i64,
    /// Transfers currently in ANCHORED state (BTC routes, PoP-anchored).
    pub anchored: i64,
    /// HEMI_TO_ETH withdrawals with their withdrawal proven — 1-day challenge window open.
    pub proving: i64,
    /// Transfers that reached FINALIZED state.
    pub finalized: i64,
    /// Transfers in a terminal FAILED state.
    pub failed: i64,
    /// Transfers rolled back due to a reorg.
    pub reorged: i64,
    /// Transfers that have been PoP-anchored to Bitcoin (across all statuses).
    pub pop_anchored: i64,
}

/// How far back an analytics query looks.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum TimeWindow {
    Last24h,
    Last7d,
    Last30d,
    AllTime,
}

impl TimeWindow {
    /// Lower bound on `initiated_at`, or `None` for all-time (no lower bound).
    fn since(self) -> Option<DateTime<Utc>> {
        match self {
            TimeWindow::Last24h => Some(Utc::now() - Duration::hours(24)),
            TimeWindow::Last7d => Some(Utc::now() - Duration::days(7)),
            TimeWindow::Last30d => Some(Utc::now() - Duration::days(30)),
            TimeWindow::AllTime => None,
        }
    }
}

/// Time-bucket size for `analyticsSeries`.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum Granularity {
    Day,
    Week,
    Month,
}

impl Granularity {
    /// The `date_trunc` field name this maps to.
    fn as_sql(self) -> &'static str {
        match self {
            Granularity::Day => "day",
            Granularity::Week => "week",
            Granularity::Month => "month",
        }
    }
}

/// Transfer count + volume for one route/asset within one time bucket.
/// `volume` is atomic units (sats / wei) as a plain decimal string — USD
/// conversion is a client-side concern, not the indexer's.
#[derive(SimpleObject)]
pub struct AnalyticsBucket {
    pub bucket_start: DateTime<Utc>,
    pub route: String,
    pub asset: String,
    pub transfer_count: i64,
    pub volume: String,
}

impl From<AnalyticsBucketRow> for AnalyticsBucket {
    fn from(r: AnalyticsBucketRow) -> Self {
        Self {
            bucket_start: r.bucket_start,
            route: r.route,
            asset: r.asset,
            transfer_count: r.transfer_count,
            volume: r.volume.with_scale(0).to_string(),
        }
    }
}

/// Share of total transfers a route accounts for within a window.
#[derive(SimpleObject)]
pub struct RouteBreakdown {
    pub route: String,
    pub transfer_count: i64,
    /// 0.0-1.0 of total transfers in the window.
    pub share: f64,
}

pub struct Query;

#[Object]
impl Query {
    /// List indexed transfers, most recent first. `limit` is clamped to 1..=500.
    async fn transfers(
        &self,
        ctx: &Context<'_>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> async_graphql::Result<Vec<Transfer>> {
        let db = db(ctx)?;
        let repo = TunnelTransferRepo::new(db);
        let limit = limit.unwrap_or(50).clamp(1, 500);
        let offset = offset.unwrap_or(0).max(0);
        let rows = repo.list(limit, offset).await.map_err(to_gql)?;
        Ok(rows.into_iter().map(Transfer::from).collect())
    }

    /// Fetch a single transfer by its id.
    async fn transfer(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<Option<Transfer>> {
        let db = db(ctx)?;
        let repo = TunnelTransferRepo::new(db);
        Ok(repo.get(id).await.map_err(to_gql)?.map(Transfer::from))
    }

    /// List transfers addressed to a recipient (case-insensitive).
    async fn transfers_by_recipient(
        &self,
        ctx: &Context<'_>,
        recipient: String,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> async_graphql::Result<Vec<Transfer>> {
        let db = db(ctx)?;
        let repo = TunnelTransferRepo::new(db);
        let limit = limit.unwrap_or(50).clamp(1, 500);
        let offset = offset.unwrap_or(0).max(0);
        let rows = repo
            .list_by_recipient(&recipient, limit, offset)
            .await
            .map_err(to_gql)?;
        Ok(rows.into_iter().map(Transfer::from).collect())
    }

    /// Search transfers by id / address / tx hash (case-insensitive), with optional
    /// `status` and `route` filters. All provided filters are combined (AND).
    async fn search_transfers(
        &self,
        ctx: &Context<'_>,
        query: Option<String>,
        status: Option<String>,
        route: Option<String>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> async_graphql::Result<Vec<Transfer>> {
        let db = db(ctx)?;
        let repo = TunnelTransferRepo::new(db);
        let limit = limit.unwrap_or(50).clamp(1, 500);
        let offset = offset.unwrap_or(0).max(0);
        // Treat blank strings as "no filter".
        let norm = |s: Option<String>| s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let (query, status, route) = (norm(query), norm(status), norm(route));
        let rows = repo
            .search(query.as_deref(), status.as_deref(), route.as_deref(), limit, offset)
            .await
            .map_err(to_gql)?;
        Ok(rows.into_iter().map(Transfer::from).collect())
    }

    /// Aggregate indexer statistics.
    async fn stats(&self, ctx: &Context<'_>) -> async_graphql::Result<Stats> {
        let db = db(ctx)?;
        let repo = TunnelTransferRepo::new(db);
        let (total, by_status, pop) = tokio::try_join!(
            repo.count(),
            repo.count_by_status(),
            repo.count_pop_anchored(),
        )
        .map_err(to_gql)?;
        let n = |s: &str| *by_status.get(s).unwrap_or(&0);
        Ok(Stats {
            total_transfers: total,
            initiated: n("INITIATED"),
            anchored: n("ANCHORED"),
            proving: n("PROVING"),
            finalized: n("FINALIZED"),
            failed: n("FAILED"),
            reorged: n("REORGED"),
            pop_anchored: pop,
        })
    }

    /// Time-bucketed transfer count + volume per route/asset within `window`,
    /// bucketed at `granularity`. Volume is atomic units — USD conversion
    /// happens client-side.
    async fn analytics_series(
        &self,
        ctx: &Context<'_>,
        window: TimeWindow,
        granularity: Granularity,
    ) -> async_graphql::Result<Vec<AnalyticsBucket>> {
        let db = db(ctx)?;
        let repo = TunnelTransferRepo::new(db);
        let rows = repo
            .analytics_series(window.since(), granularity.as_sql())
            .await
            .map_err(to_gql)?;
        Ok(rows.into_iter().map(AnalyticsBucket::from).collect())
    }

    /// Per-route share of total transfers within `window`.
    async fn route_breakdown(
        &self,
        ctx: &Context<'_>,
        window: TimeWindow,
    ) -> async_graphql::Result<Vec<RouteBreakdown>> {
        let db = db(ctx)?;
        let repo = TunnelTransferRepo::new(db);
        let rows = repo.route_breakdown(window.since()).await.map_err(to_gql)?;
        let total: i64 = rows.iter().map(|(_, count)| count).sum();
        Ok(rows
            .into_iter()
            .map(|(route, transfer_count)| RouteBreakdown {
                route,
                transfer_count,
                share: if total > 0 {
                    transfer_count as f64 / total as f64
                } else {
                    0.0
                },
            })
            .collect())
    }
}

/// Build the schema, injecting the database handle as shared context data.
pub fn build_schema(db: Database) -> StraitSchema {
    Schema::build(Query, EmptyMutation, EmptySubscription)
        .data(db)
        .finish()
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn db<'a>(ctx: &Context<'a>) -> async_graphql::Result<&'a Database> {
    ctx.data::<Database>()
}

fn to_gql(e: strait_core::error::StraitError) -> async_graphql::Error {
    async_graphql::Error::new(e.to_string())
}

#[cfg(test)]
mod tests {
    //! Executes real GraphQL queries against a live database.
    //! Skips unless `DATABASE_URL` is set. Run with:
    //!
    //! ```bash
    //! DATABASE_URL='postgres://...' cargo test -p strait-api graphql_live -- --ignored --nocapture
    //! ```
    use super::*;
    use chrono::Utc;
    use strait_core::types::*;

    // The same real Hemi testnet transfer used elsewhere (0.13 ETH into Hemi).
    fn real_transfer() -> TunnelTransfer {
        let now = Utc::now();
        let tx = TxHash::from_hex(
            "0x2b320473e4f679f8d763bbb6aee617f8b42b1a9367e623ec1c5bc6e45eedc19a",
        )
        .unwrap();
        let to = Address::from_hex("0x065213232a03b6ef9c51e0c3250096474bb515b8").unwrap();
        TunnelTransfer {
            id: Uuid::parse_str("4ca93d2a-8b7c-567c-80ee-473de55b38e8").unwrap(),
            asset: Asset::Eth,
            direction: TunnelDirection::In,
            route: TunnelRoute::EthToHemi,
            amount: bigdecimal::BigDecimal::from(130_000_000_000_000_000u64),
            sender: ChainAddress::Evm(to),
            recipient: ChainAddress::Evm(to),
            status: TunnelStatus::Initiated,
            initiated_at: now,
            finalized_at: None,
            source_tx: ChainTransaction {
                chain: Chain::Ethereum,
                hash: ChainTxHash::Evm(tx),
                block_number: 0,
                block_hash: BlockHash([0u8; 32]),
                timestamp: now,
                confirmations: 0,
            },
            destination_tx: Some(ChainTransaction {
                chain: Chain::Hemi,
                hash: ChainTxHash::Evm(tx),
                block_number: 6_037_628,
                block_hash: BlockHash([0u8; 32]),
                timestamp: now,
                confirmations: 0,
            }),
            source_fee: None,
            dest_fee: None,
            withdrawal_uuid: None,
            pop_anchored: false,
            pop_keystone_block: None,
            pop_score: None,
            pop_anchored_at: None,
            reorg_events: vec![],
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL; verifies GraphQL end-to-end against a live DB"]
    async fn graphql_live_queries_real_transfer() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("DATABASE_URL not set — skipping GraphQL live test");
            return;
        };
        let db = strait_store::init_database(&url).await.expect("connect + migrate");
        let repo = TunnelTransferRepo::new(&db);
        let t = real_transfer();
        let id = t.id;

        // Seed.
        let _ = repo.delete(id).await;
        repo.upsert(&t).await.expect("seed transfer");

        // Execute a real GraphQL query through the schema.
        let schema = build_schema(db.clone());
        let query = r#"
            {
              stats { totalTransfers }
              transfer(id: "4ca93d2a-8b7c-567c-80ee-473de55b38e8") {
                id route asset amount status popAnchored destChain destBlock recipient
              }
              transfersByRecipient(recipient: "0x065213232a03b6ef9c51e0c3250096474bb515b8") {
                id route
              }
            }
        "#;
        let resp = schema.execute(query).await;
        assert!(resp.errors.is_empty(), "graphql errors: {:?}", resp.errors);

        let data = resp.data.into_json().unwrap();
        assert_eq!(data["transfer"]["route"], "ETH_TO_HEMI");
        assert_eq!(data["transfer"]["asset"], "ETH");
        assert_eq!(data["transfer"]["amount"], "130000000000000000");
        assert_eq!(data["transfer"]["status"], "INITIATED");
        assert_eq!(data["transfer"]["destChain"], "HEMI");
        assert_eq!(data["transfer"]["destBlock"], 6_037_628);
        assert_eq!(data["transfer"]["popAnchored"], false);
        assert!(data["stats"]["totalTransfers"].as_i64().unwrap() >= 1);
        assert!(data["transfersByRecipient"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["route"] == "ETH_TO_HEMI"));

        // Cleanup.
        repo.delete(id).await.expect("cleanup");
        eprintln!("GraphQL live OK: {data}");
    }
}
