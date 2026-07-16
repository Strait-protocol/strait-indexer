//! Webhook fan-out with retry.
//!
//! Durable-outbox model (docs/webhooks-implementation-plan.md):
//!
//! * [`enqueue`] — called by strait-node's store writer after every successful
//!   `tunnel_transfers` write. Matches active subscriptions against the
//!   transfer's route/asset/status and INSERTs one PENDING `webhook_deliveries`
//!   row per match. No HTTP here — the hot path stays fast.
//!
//! * [`run_dispatch_loop`] — an independent background task. Polls for due
//!   deliveries (atomic claim with a short lease, so a crash mid-delivery just
//!   means the row comes due again), POSTs each one with an HMAC-SHA256
//!   signature, and reschedules failures with exponential backoff until the
//!   attempt budget is exhausted.
//!
//! Delivery is **at-least-once**: consumers must dedupe on the `delivery_id`
//! header or tolerate replays.

use std::time::Duration;

use chrono::Utc;
use futures::StreamExt;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{debug, error, info, warn};

use strait_core::error::Result;
use strait_store::{Database, DueDelivery, TunnelTransferRow, WebhookRepo};

/// Event type strings, as sent in the payload's `event` field and the
/// `X-Strait-Event` header.
pub mod event {
    pub const CREATED: &str = "transfer.created";
    pub const STATUS_CHANGED: &str = "transfer.status_changed";
    pub const RETRACTED: &str = "transfer.retracted";
    pub const DESTINATION_CONFIRMED: &str = "transfer.destination_confirmed";
    pub const POP_ANCHORED: &str = "transfer.pop_anchored";
}

/// Retry schedule: delay before attempt N+1 after attempt N fails (clamps to
/// the last entry). Long tail on purpose — a subscriber being down for an hour
/// shouldn't cost them the event.
const BACKOFF: &[Duration] = &[
    Duration::from_secs(10),
    Duration::from_secs(60),
    Duration::from_secs(10 * 60),
    Duration::from_secs(60 * 60),
    Duration::from_secs(6 * 60 * 60),
    Duration::from_secs(24 * 60 * 60),
];

/// Attempts before a delivery is marked FAILED permanently.
const MAX_ATTEMPTS: i32 = 8;

/// How long the poller sleeps when there was nothing to deliver.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Max deliveries claimed per poll tick.
const CLAIM_BATCH: i64 = 50;

/// Max concurrent in-flight POSTs within a batch.
const CONCURRENCY: usize = 8;

/// Per-request timeout for delivery POSTs.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Match `transfer` against the active subscriptions and enqueue one PENDING
/// delivery per match. Called from the store writer after a successful DB
/// write; must never block on network I/O.
pub async fn enqueue(db: &Database, event_type: &str, transfer: &TunnelTransferRow) -> Result<()> {
    let repo = WebhookRepo::new(db);
    let subs = repo
        .active_matching(&transfer.route, &transfer.asset, &transfer.status)
        .await?;
    if subs.is_empty() {
        return Ok(());
    }

    let payload = serde_json::json!({
        "event": event_type,
        "timestamp": Utc::now(),
        "transfer": transfer,
    });

    for sub in subs {
        let delivery_id = repo
            .enqueue_delivery(sub.id, transfer.id, event_type, &payload)
            .await?;
        debug!(
            %delivery_id,
            subscription = %sub.id,
            transfer = %transfer.id,
            event = event_type,
            "webhook delivery enqueued"
        );
    }
    Ok(())
}

/// Poll the outbox forever, delivering due webhooks. Spawned as its own task in
/// strait-node alongside the ingesters and store writer.
pub async fn run_dispatch_loop(db: Database) {
    info!(
        poll_secs = POLL_INTERVAL.as_secs(),
        max_attempts = MAX_ATTEMPTS,
        "Webhook dispatcher started"
    );

    let client = match reqwest::Client::builder().timeout(HTTP_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            error!("failed to build webhook HTTP client — dispatcher disabled: {e}");
            return;
        }
    };

    loop {
        let claimed = match WebhookRepo::new(&db).claim_due_deliveries(CLAIM_BATCH).await {
            Ok(rows) => rows,
            Err(e) => {
                warn!("webhook outbox poll failed: {e}");
                tokio::time::sleep(POLL_INTERVAL).await;
                continue;
            }
        };

        if claimed.is_empty() {
            tokio::time::sleep(POLL_INTERVAL).await;
            continue;
        }

        futures::stream::iter(claimed)
            .for_each_concurrent(CONCURRENCY, |delivery| {
                let client = client.clone();
                let db = db.clone();
                async move {
                    deliver_one(&client, &db, delivery).await;
                }
            })
            .await;
        // Immediately re-poll: a full batch likely means more rows are due.
    }
}

/// POST one delivery and settle its outbox row (delivered / retry / failed).
async fn deliver_one(client: &reqwest::Client, db: &Database, d: DueDelivery) {
    let repo = WebhookRepo::new(db);

    // Sign the exact bytes we send — receivers verify HMAC over the raw body.
    let body = d.payload.to_string();
    let signature = sign(&d.signing_secret, body.as_bytes());

    let started = std::time::Instant::now();
    let result = client
        .post(&d.url)
        .header("content-type", "application/json")
        .header("X-Strait-Signature", format!("sha256={signature}"))
        .header("X-Strait-Event", &d.event_type)
        .header("X-Strait-Delivery", d.id.to_string())
        .body(body)
        .send()
        .await;
    let response_ms = Some(started.elapsed().as_millis().min(i32::MAX as u128) as i32);

    let outcome: std::result::Result<(), String> = match result {
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) => Err(format!("subscriber returned HTTP {}", resp.status())),
        Err(e) => Err(format!("request failed: {e}")),
    };

    match outcome {
        Ok(()) => {
            if let Err(e) = repo.mark_delivered(d.id, response_ms).await {
                warn!(delivery = %d.id, "failed to mark webhook delivered: {e}");
            } else {
                info!(delivery = %d.id, url = %d.url, event = %d.event_type, "webhook delivered");
            }
        }
        Err(reason) => {
            // d.attempt_count is the count *before* this attempt.
            let attempts_done = d.attempt_count + 1;
            if attempts_done >= MAX_ATTEMPTS {
                warn!(
                    delivery = %d.id, url = %d.url, attempts = attempts_done,
                    "webhook delivery failed permanently: {reason}"
                );
                if let Err(e) = repo.mark_failed_permanently(d.id, &reason, response_ms).await {
                    warn!(delivery = %d.id, "failed to mark webhook FAILED: {e}");
                }
            } else {
                let idx = (d.attempt_count as usize).min(BACKOFF.len() - 1);
                let next = Utc::now()
                    + chrono::Duration::from_std(BACKOFF[idx])
                        .unwrap_or_else(|_| chrono::Duration::seconds(60));
                debug!(
                    delivery = %d.id, url = %d.url, attempt = attempts_done,
                    retry_at = %next, "webhook delivery failed, retrying: {reason}"
                );
                if let Err(e) = repo.mark_retry(d.id, &reason, next, response_ms).await {
                    warn!(delivery = %d.id, "failed to schedule webhook retry: {e}");
                }
            }
        }
    }
}

/// Hex HMAC-SHA256 of `body` under `secret` (the subscription's signing key,
/// itself a hex string — the HMAC key is its raw UTF-8 bytes, matching what
/// subscribers were handed at registration).
fn sign(secret: &str, body: &[u8]) -> String {
    // HMAC accepts any key length; new_from_slice on Hmac is infallible.
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_stable_hex() {
        let sig = sign("deadbeef", b"{\"a\":1}");
        assert_eq!(sig.len(), 64);
        assert_eq!(sig, sign("deadbeef", b"{\"a\":1}"));
        assert_ne!(sig, sign("deadbeef", b"{\"a\":2}"));
        assert_ne!(sig, sign("other-key", b"{\"a\":1}"));
    }
}
