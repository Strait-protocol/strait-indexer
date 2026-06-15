-- Initial schema for Strait indexer
-- PostgreSQL 16+

-- ============================================================================
-- Extensions
-- ============================================================================
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ============================================================================
-- Enum types
-- ============================================================================
CREATE TYPE chain_type AS ENUM ('bitcoin', 'evm');
CREATE TYPE transfer_status AS ENUM (
    'pending',
    'bitcoin_sent',
    'bitcoin_confirmed',
    'evm_claimed',
    'completed',
    'failed'
);
CREATE TYPE tunnel_direction AS ENUM ('bitcoin_to_evm', 'evm_to_bitcoin');
CREATE TYPE pop_proof_status AS ENUM ('pending', 'verified', 'failed');

-- ============================================================================
-- Transfers table
-- ============================================================================
CREATE TABLE transfers (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    direction       tunnel_direction NOT NULL,
    status          transfer_status NOT NULL DEFAULT 'pending',
    
    -- Bitcoin side
    bitcoin_txid    VARCHAR(64),
    bitcoin_block   BIGINT,
    bitcoin_vout    INTEGER,
    bitcoin_amount  NUMERIC(20, 0),  -- satoshis
    
    -- EVM side
    evm_tx_hash     VARCHAR(66),
    evm_block       BIGINT,
    evm_log_index   INTEGER,
    evm_amount      NUMERIC(78, 0),  -- wei
    
    -- Tunnel metadata
    sender_address  VARCHAR(255) NOT NULL,
    receiver_address VARCHAR(255) NOT NULL,
    tunnel_id       VARCHAR(66),
    
    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    confirmed_at    TIMESTAMPTZ,
    claimed_at      TIMESTAMPTZ,
    
    -- Constraints
    CONSTRAINT transfers_bitcoin_txid_check 
        CHECK (bitcoin_txid IS NULL OR LENGTH(bitcoin_txid) = 64),
    CONSTRAINT transfers_evm_tx_hash_check 
        CHECK (evm_tx_hash IS NULL OR LENGTH(evm_tx_hash) = 66)
);

-- Indexes for common queries
CREATE INDEX idx_transfers_status ON transfers(status);
CREATE INDEX idx_transfers_bitcoin_txid ON transfers(bitcoin_txid) WHERE bitcoin_txid IS NOT NULL;
CREATE INDEX idx_transfers_evm_tx_hash ON transfers(evm_tx_hash) WHERE evm_tx_hash IS NOT NULL;
CREATE INDEX idx_transfers_sender ON transfers(sender_address);
CREATE INDEX idx_transfers_receiver ON transfers(receiver_address);
CREATE INDEX idx_transfers_created_at ON transfers(created_at);

-- ============================================================================
-- Checkpoints table (chain ingestion progress)
-- ============================================================================
CREATE TABLE checkpoints (
    chain           chain_type PRIMARY KEY,
    block_height    BIGINT NOT NULL DEFAULT 0,
    block_hash      VARCHAR(66),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Initialize default checkpoints
INSERT INTO checkpoints (chain, block_height) VALUES ('bitcoin', 0);
INSERT INTO checkpoints (chain, block_height) VALUES ('evm', 0);

-- ============================================================================
-- Events table (raw event log)
-- ============================================================================
CREATE TABLE events (
    id              BIGSERIAL PRIMARY KEY,
    chain           chain_type NOT NULL,
    block_height    BIGINT NOT NULL,
    block_hash      VARCHAR(66) NOT NULL,
    tx_hash         VARCHAR(66) NOT NULL,
    log_index       INTEGER,
    event_type      VARCHAR(100) NOT NULL,
    event_data      JSONB NOT NULL DEFAULT '{}',
    processed       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Deduplication: a UNIQUE *table constraint* cannot contain an expression like
-- COALESCE(), so enforce it with a unique expression index instead.
CREATE UNIQUE INDEX events_unique ON events (chain, tx_hash, COALESCE(log_index, -1));

-- Indexes
CREATE INDEX idx_events_chain_block ON events(chain, block_height);
CREATE INDEX idx_events_processed ON events(processed) WHERE NOT processed;
CREATE INDEX idx_events_tx_hash ON events(tx_hash);
CREATE INDEX idx_events_event_type ON events(event_type);

-- ============================================================================
-- POP proofs table
-- ============================================================================
CREATE TABLE pop_proofs (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    transfer_id     UUID NOT NULL REFERENCES transfers(id),
    status          pop_proof_status NOT NULL DEFAULT 'pending',
    
    -- Proof data
    bitcoin_txid    VARCHAR(66) NOT NULL,
    bitcoin_block   BIGINT NOT NULL,
    merkle_proof    BYTEA,
    block_header    BYTEA,
    
    -- Verification
    verified_at     TIMESTAMPTZ,
    verification_error TEXT,
    
    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pop_proofs_transfer ON pop_proofs(transfer_id);
CREATE INDEX idx_pop_proofs_status ON pop_proofs(status);

-- ============================================================================
-- Webhook deliveries table (audit log)
-- ============================================================================
CREATE TABLE webhook_deliveries (
    id              BIGSERIAL PRIMARY KEY,
    webhook_id      UUID NOT NULL,
    event_type      VARCHAR(100) NOT NULL,
    payload         JSONB NOT NULL,
    response_code   INTEGER,
    response_body   TEXT,
    delivered_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    success         BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_webhook_deliveries_webhook ON webhook_deliveries(webhook_id);
CREATE INDEX idx_webhook_deliveries_delivered ON webhook_deliveries(delivered_at);

-- ============================================================================
-- Updated_at trigger function
-- ============================================================================
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Apply triggers
CREATE TRIGGER update_transfers_updated_at
    BEFORE UPDATE ON transfers
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_checkpoints_updated_at
    BEFORE UPDATE ON checkpoints
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_pop_proofs_updated_at
    BEFORE UPDATE ON pop_proofs
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();