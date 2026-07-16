# Webhooks for external Strait consumers

## Context

`strait-api` currently only supports polling (`GET /transfers`, GraphQL
`transfers`/`searchTransfers`) — README.md's "Webhooks _(planned)_" section says
push notifications are "on the roadmap but not yet implemented." External
consumers (wallets, notification bots) building on Strait need push notifications
instead of polling every ~10-15s for a specific transfer's status.

The scaffolding for this already exists but was never filled in:
- `crates/strait-api/src/webhooks/{mod,registry,dispatcher}.rs` — 3-6 line stub
  files, each saying "Stub — will be implemented in Step 8."
- `crates/strait-api/Cargo.toml` already depends on `reqwest`, `hmac`, `sha2`,
  `hex` for exactly this purpose (crate description: "GraphQL and webhook HTTP
  server for Strait").
- `crates/strait-core/src/error.rs` already has a `WebhookDeliveryFailed` variant.
- `crates/strait-store/examples/db_reset.rs` already references a
  `webhook_deliveries` table name.

This plan fills in that scaffolding. Decisions confirmed with the user before
implementation: **durable outbox + background poller** (not fire-and-forget) for
delivery, and a **per-webhook management token** (not open/no-auth) for managing
a subscription.

## Design

**Two new tables** (migration `009_webhooks.sql`, following the style of
`003_chain_checkpoints.sql` and `002_tunnel_transfers.sql`):

- `webhook_subscriptions` — `id UUID PK`, `url TEXT`, `signing_secret TEXT`
  (HMAC key, returned once at creation), `management_token TEXT` (required to
  read/delete this row later), `routes TEXT[]`/`assets TEXT[]`/`statuses TEXT[]`
  (NULL/empty = no filter on that dimension = matches everything), `active BOOL`,
  `created_at`/`updated_at` (reuse `update_updated_at_column()` trigger from
  `001_initial.sql`, same as `tunnel_transfers`).
- `webhook_deliveries` — the outbox. `id UUID PK`, `subscription_id UUID FK`,
  `transfer_id UUID`, `event_type TEXT`, `payload JSONB`, `status TEXT`
  (`PENDING`/`DELIVERED`/`FAILED`), `attempt_count INT`, `next_attempt_at
  TIMESTAMPTZ`, `last_error TEXT`, `delivered_at TIMESTAMPTZ`, `created_at`.
  Partial index on `(next_attempt_at) WHERE status = 'PENDING'` for the poller's scan.

**Flow**: `strait-node`'s `store_writer` (in `crates/strait-node/src/main.rs`)
already sits right after every successful `tunnel_transfers` write. After each
successful write, it re-fetches the row via the existing
`TunnelTransferRepo::get(id)` and calls a new
`strait_api::webhooks::dispatcher::enqueue(&db, event_type, &row)` — this just
matches active subscriptions (route/asset/status filter, array containment SQL)
and `INSERT`s one `PENDING` delivery row per match. Fast, non-blocking, no HTTP
calls in the hot path.

A **second, independent tokio task** — `webhooks::dispatcher::run_dispatch_loop`,
spawned into the same `JoinSet` in `main.rs` alongside the ingesters/join
engine/store_writer — polls `webhook_deliveries` for due rows (`FOR UPDATE SKIP
LOCKED`, future-proofs against running >1 instance), delivers them concurrently
(bounded `for_each_concurrent`, `futures` crate — already a workspace dep),
HMAC-signs each payload (`hmac`+`sha2`, already deps) into an
`X-Strait-Signature` header, POSTs with a 10s timeout, and on failure schedules a
retry with backoff (10s, 1m, 10m, 1h, 6h, 24h — longer tail than the EVM
ingester's rate-limit backoff, since "subscriber's server down for an hour"
shouldn't drop the event) up to ~8 attempts before marking `FAILED` permanently.

**REST endpoints**, mounted in `crates/strait-api/src/server.rs` next to the
existing `/transfers` routes (same `AppState`/`State(state)` pattern):
- `POST /webhooks` — body `{ url, routes?, assets?, statuses? }`. Validates
  `url` is `http(s)` and rejects localhost/private/link-local hosts (basic SSRF
  guard on user-supplied URLs). Generates `signing_secret` + `management_token`
  (32 random bytes, hex — needs the `rand` crate, currently only a transitive
  dep, so add `rand = "0.8"` to `strait-api/Cargo.toml` and the workspace root).
  Response includes both secrets **once** — never returned again.
- `GET /webhooks/:id` — requires `X-Management-Token` header matching. Returns
  metadata only (url, filters, active, created_at) — no secret/token.
- `DELETE /webhooks/:id` — same token requirement.

**New `strait-store` module**, `crates/strait-store/src/webhooks.rs`, following
the `CheckpointRepo` pattern (`struct WebhookRepo<'a> { pool: &'a PgPool }`,
`pub fn new(db: &'a Database) -> Self`), exported from `lib.rs` next to the
other repos. Methods: `create`, `get_by_id_and_token`, `delete`,
`active_matching(route, asset, status)`, `enqueue_delivery`,
`fetch_due_deliveries`, `mark_delivered`, `mark_retry`, `mark_failed_permanently`.

## Files touched

- `crates/strait-store/migrations/009_webhooks.sql` — new
- `crates/strait-store/src/webhooks.rs` — new (repo)
- `crates/strait-store/src/lib.rs` — export the new repo/row types
- `crates/strait-api/src/webhooks/registry.rs` — fill in (URL validation, secret/token
  generation, CRUD queries via `WebhookRepo`)
- `crates/strait-api/src/webhooks/dispatcher.rs` — fill in (`enqueue`,
  `run_dispatch_loop`, HMAC signing, backoff schedule)
- `crates/strait-api/src/server.rs` — add `POST/GET/DELETE /webhooks[/:id]` routes
  and their handlers (same shape as `list_transfers`/`get_transfer`)
- `crates/strait-api/Cargo.toml` — add `rand`
- `Cargo.toml` (workspace root) — pin `rand` version in `[workspace.dependencies]`
- `crates/strait-node/src/main.rs` — `store_writer`: call `dispatcher::enqueue`
  after each successful write; spawn `dispatcher::run_dispatch_loop` into the
  existing `JoinSet`
- `README.md` — flip the "Webhooks _(planned)_" section to document the real
  endpoints, headers, and payload shape
- `docs/api-integration.md` — add a `§10 Webhooks` section (registration,
  signature verification example, retry/backoff behavior)

## Verification

- `cargo check --workspace --all-targets` after each crate's changes land.
- End-to-end against production-shaped local flow: run migration, `POST
  /webhooks` with a filter, use a local HTTP listener as the callback URL,
  trigger a status change and confirm delivery arrives with a valid HMAC
  signature.
- Verify the signature independently: compute `HMAC-SHA256(signing_secret,
  raw_body)` and compare hex digest to `X-Strait-Signature`.
- Point the callback URL at a closed port and confirm the `webhook_deliveries`
  row's `attempt_count`/`next_attempt_at` advance correctly and it isn't lost
  after restarting `strait-node`.
