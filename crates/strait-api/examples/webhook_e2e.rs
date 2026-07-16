//! End-to-end webhook verification harness.
//!
//! Exercises the full pipeline against a real database: local callback
//! listener → subscription insert → outbox enqueue → dispatch loop → HMAC
//! verification → retry scheduling for an unreachable subscriber → cleanup.
//!
//! Inserts the test subscription directly via `WebhookRepo` (bypassing the
//! registration endpoint's SSRF guard, which would rightly reject a loopback
//! callback URL) and deletes everything it created on the way out.
//!
//! Deliberately uses `Database::connect` (NOT `init_database`): running
//! migrations from a feature branch against a shared database records
//! migrations the deployed binary doesn't have, which crash-loops it on
//! restart.
//!
//! ```bash
//! DATABASE_URL='postgres://...' cargo run -p strait-api --example webhook_e2e
//! ```

use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::Router;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::mpsc;

use strait_api::webhooks::dispatcher::{self, event};
use strait_store::{CreateWebhookSubscription, Database, TunnelTransferRepo, WebhookRepo};

type Received = (HeaderMap, Bytes);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    // No migrations on purpose — see module docs.
    let db = Database::connect(&url, 2).await?;

    // ── Local callback listener on a random port ─────────────────────────────
    let (got_tx, mut got_rx) = mpsc::channel::<Received>(16);
    let listener_state = Arc::new(got_tx);
    let app = Router::new()
        .route(
            "/hook",
            post(
                |State(tx): State<Arc<mpsc::Sender<Received>>>, headers: HeaderMap, body: Bytes| async move {
                    let _ = tx.send((headers, body)).await;
                    "ok"
                },
            ),
        )
        .with_state(listener_state);
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = tcp.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(tcp, app).await;
    });
    println!("listener on {addr}");

    // ── Test subscriptions ───────────────────────────────────────────────────
    let repo = WebhookRepo::new(&db);
    let secret = "e2e-test-signing-secret".to_string();
    let token = "e2e-test-management-token".to_string();
    let sub_ok = repo
        .create(CreateWebhookSubscription {
            url: format!("http://{addr}/hook"),
            signing_secret: secret.clone(),
            management_token: token.clone(),
            routes: None,
            assets: None,
            statuses: None,
        })
        .await?;
    println!("created subscription {} -> {}", sub_ok.id, sub_ok.url);

    // A real transfer row to notify about.
    let transfer = TunnelTransferRepo::new(&db)
        .list(1, 0)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no transfers in DB to test with"))?;
    println!("using transfer {} ({} {})", transfer.id, transfer.route, transfer.asset);

    // ── Phase 1: successful delivery ─────────────────────────────────────────
    dispatcher::enqueue(&db, event::STATUS_CHANGED, &transfer).await?;
    println!("enqueued; starting dispatch loop");
    {
        let db = db.clone();
        tokio::spawn(async move { dispatcher::run_dispatch_loop(db).await });
    }

    let (headers, body) = tokio::time::timeout(Duration::from_secs(30), got_rx.recv())
        .await
        .map_err(|_| anyhow::anyhow!("FAIL: no delivery arrived within 30s"))?
        .ok_or_else(|| anyhow::anyhow!("listener channel closed"))?;

    // Verify signature over the exact received bytes.
    let sig_header = headers
        .get("x-strait-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("FAIL: missing X-Strait-Signature"))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(&body);
    let expect = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    anyhow::ensure!(
        sig_header == expect,
        "FAIL: signature mismatch: got {sig_header}, want {expect}"
    );
    let evt = headers
        .get("x-strait-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    anyhow::ensure!(evt == event::STATUS_CHANGED, "FAIL: wrong event header {evt}");
    let payload: serde_json::Value = serde_json::from_slice(&body)?;
    anyhow::ensure!(
        payload["transfer"]["id"] == serde_json::json!(transfer.id.to_string()),
        "FAIL: payload transfer id mismatch"
    );
    println!("PASS: delivery received, HMAC + headers + payload verified");

    // Delivery row should be DELIVERED shortly after.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let (status, attempts): (String, i32) = sqlx::query_as(
        "SELECT status, attempt_count FROM webhook_deliveries WHERE subscription_id = $1",
    )
    .bind(sub_ok.id)
    .fetch_one(db.pool())
    .await?;
    anyhow::ensure!(
        status == "DELIVERED" && attempts == 1,
        "FAIL: outbox row not settled (status={status}, attempts={attempts})"
    );
    println!("PASS: outbox row DELIVERED after {attempts} attempt");

    // ── Phase 2: unreachable subscriber schedules a retry ────────────────────
    let sub_bad = repo
        .create(CreateWebhookSubscription {
            url: "http://127.0.0.1:9/hook".to_string(), // discard port — refused
            signing_secret: secret.clone(),
            management_token: token.clone(),
            routes: None,
            assets: None,
            statuses: None,
        })
        .await?;
    dispatcher::enqueue(&db, event::STATUS_CHANGED, &transfer).await?;
    println!("enqueued to unreachable subscriber; waiting for the failed attempt");
    tokio::time::sleep(Duration::from_secs(8)).await;

    let (status, attempts, last_error, due_in): (String, i32, Option<String>, f64) = sqlx::query_as(
        "SELECT status, attempt_count, last_error,
                EXTRACT(EPOCH FROM (next_attempt_at - NOW()))::float8
         FROM webhook_deliveries WHERE subscription_id = $1",
    )
    .bind(sub_bad.id)
    .fetch_one(db.pool())
    .await?;
    anyhow::ensure!(
        status == "PENDING" && attempts >= 1 && last_error.is_some() && due_in > 0.0,
        "FAIL: retry not scheduled (status={status}, attempts={attempts}, err={last_error:?}, due_in={due_in})"
    );
    println!(
        "PASS: failed delivery rescheduled (attempts={attempts}, retry in {due_in:.0}s, err={:?})",
        last_error.unwrap()
    );

    // The healthy subscriber also received the second event (at-least-once fan-out).
    let second = tokio::time::timeout(Duration::from_secs(15), got_rx.recv()).await;
    anyhow::ensure!(second.is_ok(), "FAIL: second event never reached healthy subscriber");
    println!("PASS: healthy subscriber also received the second event");

    // ── Delivery history (backs GET /webhooks/:id/deliveries) ───────────────
    let ok_history = repo.recent_deliveries(sub_ok.id, 20).await?;
    anyhow::ensure!(
        ok_history.len() == 2 && ok_history.iter().all(|d| d.status == "DELIVERED"),
        "FAIL: expected 2 DELIVERED rows for healthy sub, got {ok_history:?}"
    );
    anyhow::ensure!(
        ok_history.iter().all(|d| d.response_ms.is_some()),
        "FAIL: response_ms not recorded on delivered attempts"
    );
    let bad_history = repo.recent_deliveries(sub_bad.id, 20).await?;
    anyhow::ensure!(
        bad_history.len() == 1 && bad_history[0].response_ms.is_some(),
        "FAIL: expected 1 attempted row with response_ms for unreachable sub, got {bad_history:?}"
    );
    println!(
        "PASS: delivery history lists attempts with response times ({} ms on last success)",
        ok_history[0].response_ms.unwrap()
    );

    // ── Cleanup (deliveries cascade with their subscription) ─────────────────
    anyhow::ensure!(repo.delete(sub_ok.id, &token).await?, "cleanup: sub_ok gone?");
    anyhow::ensure!(repo.delete(sub_bad.id, &token).await?, "cleanup: sub_bad gone?");
    let leftover: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM webhook_deliveries WHERE subscription_id IN ($1, $2)",
    )
    .bind(sub_ok.id)
    .bind(sub_bad.id)
    .fetch_one(db.pool())
    .await?;
    anyhow::ensure!(leftover == 0, "cleanup: {leftover} delivery rows left behind");
    println!("PASS: cleanup complete — all E2E checks passed");
    Ok(())
}
