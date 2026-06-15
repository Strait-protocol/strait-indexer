-- Migration 003: chain_checkpoints
--
-- Replace the original checkpoints table with one keyed by a TEXT chain name.
-- The original (001) used the `chain_type` enum which only had 'bitcoin' and
-- 'evm' — but Strait indexes two EVM chains (Hemi and Ethereum) that must be
-- tracked independently. This table records, per chain, the last block whose
-- events have been fully ingested, so the node resumes from there on restart.

DROP TABLE IF EXISTS checkpoints CASCADE;

CREATE TABLE checkpoints (
    id           SERIAL PRIMARY KEY,
    chain        TEXT NOT NULL UNIQUE,        -- bitcoin | hemi | ethereum
    block_height BIGINT NOT NULL DEFAULT 0,
    block_hash   TEXT NOT NULL DEFAULT '',
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
