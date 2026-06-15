-- Migration 002: tunnel_transfers
--
-- The unified cross-chain transfer record, mirroring strait_core::types::TunnelTransfer.
-- This is the table the join engine writes to and the API reads from. It supersedes
-- the Bitcoin-centric `transfers` table for tunnel indexing and carries the PoP /
-- Bitcoin-finality fields (the deferred "Layer 2" of the PoP anchoring spec).

CREATE TABLE IF NOT EXISTS tunnel_transfers (
    id              UUID PRIMARY KEY,

    asset           TEXT NOT NULL,                 -- BTC | ETH | <erc20 symbol>
    direction       TEXT NOT NULL,                 -- IN | OUT
    route           TEXT NOT NULL,                 -- BTC_TO_HEMI | HEMI_TO_BTC | ETH_TO_HEMI | HEMI_TO_ETH
    amount          NUMERIC(78, 0) NOT NULL,       -- satoshis or wei (atomic units)
    sender          TEXT NOT NULL,
    recipient       TEXT NOT NULL,
    status          TEXT NOT NULL,                 -- INITIATED | ANCHORED | FINALIZED | FAILED | REORGED

    -- Source transaction (the leg that initiated the transfer)
    source_chain        TEXT NOT NULL,             -- BITCOIN | HEMI | ETHEREUM
    source_tx_hash      TEXT NOT NULL,
    source_block        BIGINT NOT NULL,
    source_timestamp    TIMESTAMPTZ NOT NULL,

    -- Destination transaction (filled when the counterpart leg confirms)
    dest_chain          TEXT,
    dest_tx_hash        TEXT,
    dest_block          BIGINT,

    -- PoP / Bitcoin finality
    pop_anchored        BOOLEAN NOT NULL DEFAULT FALSE,
    pop_keystone_block  BIGINT,
    pop_score           BIGINT,
    pop_anchored_at     TIMESTAMPTZ,

    initiated_at    TIMESTAMPTZ NOT NULL,
    finalized_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_status        ON tunnel_transfers(status);
CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_route         ON tunnel_transfers(route);
CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_source_tx     ON tunnel_transfers(source_tx_hash);
CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_dest_tx       ON tunnel_transfers(dest_tx_hash) WHERE dest_tx_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_recipient     ON tunnel_transfers(recipient);
CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_pop_keystone  ON tunnel_transfers(pop_keystone_block) WHERE pop_keystone_block IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_created       ON tunnel_transfers(created_at DESC);

-- Reuse the updated_at trigger function defined in 001_initial.sql.
DROP TRIGGER IF EXISTS update_tunnel_transfers_updated_at ON tunnel_transfers;
CREATE TRIGGER update_tunnel_transfers_updated_at
    BEFORE UPDATE ON tunnel_transfers
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
