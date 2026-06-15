//! Axum HTTP server: health checks and a read API over indexed transfers.
//!
//! Exposes:
//!   GET /health         — liveness
//!   GET /health/db      — database connectivity
//!   GET /transfers      — list indexed transfers (?limit=&offset=)
//!   GET /transfers/:id  — fetch one transfer by UUID

use std::net::SocketAddr;
use std::sync::Arc;

use async_graphql::http::GraphiQLSource;
use async_graphql_axum::GraphQL;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use serde::Deserialize;
use serde_json::json;
use tracing::info;
use uuid::Uuid;

use strait_core::config::ApiConfig;
use strait_store::{Database, TunnelTransferRepo};

/// Shared HTTP state — a cloneable handle to the database pool.
#[derive(Clone)]
struct AppState {
    db: Arc<Database>,
}

/// Build the router and serve until the process is shut down.
pub async fn serve(config: ApiConfig, db: Database) -> anyhow::Result<()> {
    // GraphQL schema carries its own DB handle as context data.
    let schema = crate::graphql::build_schema(db.clone());
    let state = AppState { db: Arc::new(db) };

    let app = Router::new()
        .route("/health", get(health))
        .route("/health/db", get(health_db))
        .route("/transfers", get(list_transfers))
        .route("/transfers/:id", get(get_transfer))
        // GET serves the GraphiQL playground; POST executes GraphQL queries.
        .route(
            "/graphql",
            get(graphiql).post_service(GraphQL::new(schema)),
        )
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid API bind address: {e}"))?;

    info!("API server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Liveness probe.
async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "strait-node" }))
}

/// GraphiQL in-browser playground for the `/graphql` endpoint.
async fn graphiql() -> impl IntoResponse {
    Html(GraphiQLSource::build().endpoint("/graphql").finish())
}

/// Readiness probe — checks the database is reachable.
async fn health_db(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.health_check().await {
        Ok(_) => (StatusCode::OK, Json(json!({ "database": "ok" }))),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "database": "error", "detail": e.to_string() })),
        ),
    }
}

/// Pagination query parameters for `GET /transfers`.
#[derive(Debug, Deserialize)]
struct ListParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

/// List indexed transfers, most recent first.
async fn list_transfers(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let repo = TunnelTransferRepo::new(state.db.as_ref());
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);

    match repo.list(limit, offset).await {
        Ok(transfers) => (
            StatusCode::OK,
            Json(json!({ "transfers": transfers, "limit": limit, "offset": offset })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Fetch a single transfer by UUID.
async fn get_transfer(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    let repo = TunnelTransferRepo::new(state.db.as_ref());

    match repo.get(id).await {
        Ok(Some(transfer)) => {
            (StatusCode::OK, Json(json!({ "transfer": transfer }))).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "transfer not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
