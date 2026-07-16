//! Axum HTTP server: health checks and a read API over indexed transfers.
//!
//! Exposes:
//!   GET    /health         — liveness
//!   GET    /health/db      — database connectivity
//!   GET    /transfers      — list indexed transfers (?limit=&offset=)
//!   GET    /transfers/:id  — fetch one transfer by UUID
//!   POST   /webhooks       — register a webhook subscription
//!   GET    /webhooks/:id   — inspect a subscription (X-Management-Token)
//!   DELETE /webhooks/:id   — remove a subscription (X-Management-Token)

use std::net::SocketAddr;
use std::sync::Arc;

use async_graphql::http::GraphiQLSource;
use async_graphql_axum::GraphQL;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use uuid::Uuid;

use strait_core::config::ApiConfig;
use strait_store::{Database, TunnelTransferRepo, WebhookRepo};

use crate::webhooks::registry::{self, RegisterRequest, SubscriptionView};

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

    // Strait is a public API — reads are open to any origin, and the webhook
    // management endpoints are self-authorizing via per-subscription tokens.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/health/db", get(health_db))
        .route("/transfers", get(list_transfers))
        .route("/transfers/:id", get(get_transfer))
        .route("/webhooks", post(register_webhook))
        .route("/webhooks/:id", get(get_webhook).delete(delete_webhook))
        .route("/webhooks/:id/deliveries", get(list_webhook_deliveries))
        // GET serves the GraphiQL playground; POST executes GraphQL queries.
        .route(
            "/graphql",
            get(graphiql).post_service(GraphQL::new(schema)),
        )
        .with_state(state)
        .layer(cors);

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

// ── Webhook management ───────────────────────────────────────────────────────

/// Register a webhook subscription. The response is the only time the
/// signing secret and management token are ever disclosed.
async fn register_webhook(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    if let Err(reason) = registry::validate(&req) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": reason }))).into_response();
    }

    match registry::register(state.db.as_ref(), req).await {
        Ok(created) => (StatusCode::CREATED, Json(json!({ "webhook": created }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Pull the management token out of the `X-Management-Token` header.
fn management_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-management-token")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .filter(|t| !t.is_empty())
}

/// Inspect a subscription (metadata only — never the secret or token).
/// Unknown id and wrong token are both 404 so ids can't be probed.
async fn get_webhook(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(token) = management_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing X-Management-Token header" })),
        )
            .into_response();
    };

    match WebhookRepo::new(state.db.as_ref())
        .get_by_id_and_token(id, &token)
        .await
    {
        Ok(Some(row)) => (
            StatusCode::OK,
            Json(json!({ "webhook": SubscriptionView::from(row) })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "webhook not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Recent delivery attempts for a subscription (newest first, up to 20) —
/// backs the manage UI's attempt history. Same token gating as `get_webhook`.
/// The stored payload is omitted from the response to keep it small; use the
/// `transfer_id` with `GET /transfers/:id` for the full record.
async fn list_webhook_deliveries(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(token) = management_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing X-Management-Token header" })),
        )
            .into_response();
    };

    let repo = WebhookRepo::new(state.db.as_ref());
    match repo.get_by_id_and_token(id, &token).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "webhook not found" })),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }

    match repo.recent_deliveries(id, 20).await {
        Ok(rows) => {
            let deliveries: Vec<_> = rows
                .into_iter()
                .map(|d| {
                    json!({
                        "id": d.id,
                        "transfer_id": d.transfer_id,
                        "event_type": d.event_type,
                        "status": d.status,
                        "attempt_count": d.attempt_count,
                        "response_ms": d.response_ms,
                        "last_error": d.last_error,
                        "next_attempt_at": d.next_attempt_at,
                        "delivered_at": d.delivered_at,
                        "created_at": d.created_at,
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({ "deliveries": deliveries }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Delete a subscription (pending deliveries cascade with it).
async fn delete_webhook(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(token) = management_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing X-Management-Token header" })),
        )
            .into_response();
    };

    match WebhookRepo::new(state.db.as_ref()).delete(id, &token).await {
        Ok(true) => (StatusCode::OK, Json(json!({ "deleted": true }))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "webhook not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
