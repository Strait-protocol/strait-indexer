-- Migration 009: webhooks
--
-- Push notifications for external consumers (docs/webhooks-implementation-plan.md).
--
-- * webhook_subscriptions — one row per registered callback URL, with optional
--   route/asset/status filters. `signing_secret` is the HMAC-SHA256 key for
--   payload signatures (returned once at registration, never again);
--   `management_token` gates GET/DELETE on the subscription.
--
-- * webhook_deliveries — the durable outbox. The store writer INSERTs a PENDING
--   row per matching subscription whenever a transfer changes; a background
--   dispatch loop polls due rows, POSTs them, and either marks DELIVERED or
--   reschedules with backoff until the attempt budget is exhausted (FAILED).
--   Deliveries survive a process crash/restart — nothing is lost in-flight.

CREATE TABLE IF NOT EXISTS webhook_subscriptions (
    id                UUID PRIMARY KEY,
    url               TEXT NOT NULL,
    signing_secret    TEXT NOT NULL,             -- hex, HMAC-SHA256 key for payload signatures
    management_token  TEXT NOT NULL,             -- hex, required to GET/DELETE this subscription

    -- Filters: NULL or empty array = match everything on that dimension.
    routes            TEXT[],                    -- e.g. {HEMI_TO_BTC, ETH_TO_HEMI}
    assets            TEXT[],                    -- e.g. {BTC, ETH, HEMI}
    statuses          TEXT[],                    -- e.g. {FINALIZED, FAILED}

    active            BOOLEAN NOT NULL DEFAULT TRUE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_webhook_subscriptions_active
    ON webhook_subscriptions(active) WHERE active;

-- Reuse the updated_at trigger function defined in 001_initial.sql.
DROP TRIGGER IF EXISTS update_webhook_subscriptions_updated_at ON webhook_subscriptions;
CREATE TRIGGER update_webhook_subscriptions_updated_at
    BEFORE UPDATE ON webhook_subscriptions
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Migration 001 created a legacy `webhook_deliveries` audit-log table (BIGSERIAL
-- id, webhook_id, response_code/success) that no code ever wrote to. It clashes
-- with the outbox shape below, so drop it — but *only* the legacy shape
-- (detected by its webhook_id column), never the new outbox table. This keeps
-- the migration idempotent: re-running it over an existing outbox (e.g. one
-- created by pasting this file manually before the deploy) preserves its rows.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'webhook_deliveries' AND column_name = 'webhook_id'
    ) THEN
        DROP TABLE webhook_deliveries;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id                UUID PRIMARY KEY,
    subscription_id   UUID NOT NULL REFERENCES webhook_subscriptions(id) ON DELETE CASCADE,
    transfer_id       UUID NOT NULL,
    event_type        TEXT NOT NULL,             -- transfer.created | transfer.status_changed | ...
    payload           JSONB NOT NULL,

    status            TEXT NOT NULL DEFAULT 'PENDING',  -- PENDING | DELIVERED | FAILED
    attempt_count     INT  NOT NULL DEFAULT 0,
    next_attempt_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_error        TEXT,
    -- Round-trip time of the most recent POST attempt, for the manage UI.
    response_ms       INT,
    delivered_at      TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Cover databases where the outbox table was created from an earlier revision
-- of this file (e.g. pasted manually before deploy) without response_ms.
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS response_ms INT;

-- The dispatch loop's scan: due PENDING rows, oldest due first.
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_due
    ON webhook_deliveries(next_attempt_at) WHERE status = 'PENDING';

-- The manage UI's "recent deliveries for this subscription" listing.
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_sub
    ON webhook_deliveries(subscription_id, created_at DESC);
