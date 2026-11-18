-- Migration 012: Bitcoin-side PoP publications
--
-- PoP miners publish Hemi keystones (every 25th Hemi block) to Bitcoin in
-- OP_RETURN outputs, whether or not the PoPPayoutsV2 reward contract is active.
-- Strait follows the transaction history of the known PoP miner wallets
-- (POP_MINER_ADDRESSES) and uses their publications to mark transfers
-- Bitcoin-final, and to answer "which Bitcoin tx anchors Hemi block N?".
--
-- pop_sync_state.synced_height is the Bitcoin height up to which every known
-- miner's history has been read completely. The finality queries only trust
-- publications at or below it, so "the earliest publication of any keystone
-- >= N" can't be answered from a partially read history.

CREATE TABLE IF NOT EXISTS pop_sync_state (
    id             BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    -- POP_SCAN_START_HEIGHT and that block's time: publications before it are
    -- not collected, so blocks near it can't be answered for.
    start_height   BIGINT NOT NULL,
    start_time     TIMESTAMPTZ NOT NULL,
    -- Every miner's history is complete up to here.
    synced_height  BIGINT,
    -- Bitcoin tip at the last sync, for counting confirmations.
    tip_height     BIGINT,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS pop_miners (
    address        TEXT PRIMARY KEY,
    -- This miner's history is complete up to here (NULL: never synced).
    synced_height  BIGINT,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS pop_publications (
    btc_txid         TEXT NOT NULL,
    vout             INT NOT NULL,
    btc_block_height BIGINT NOT NULL,
    btc_block_hash   TEXT NOT NULL,
    btc_block_time   TIMESTAMPTZ NOT NULL,
    miner_address    TEXT NOT NULL,
    -- Hemi keystone block this publication is for (multiple of 25).
    keystone_block   BIGINT NOT NULL,
    l1_block         BIGINT NOT NULL,
    -- First 12 bytes of the keystone block hash, hex.
    ep_hash          TEXT NOT NULL,
    -- Checked against the canonical Hemi block: NULL = not yet, TRUE = matches,
    -- FALSE = forged or non-canonical (never used for anchoring).
    valid            BOOLEAN,
    -- Full publication payload (hex) for re-validation and auditing.
    raw_keystone     TEXT NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (btc_txid, vout)
);

CREATE INDEX IF NOT EXISTS idx_pop_publications_valid_keystone
    ON pop_publications(keystone_block, btc_block_height) WHERE valid;
CREATE INDEX IF NOT EXISTS idx_pop_publications_unvalidated
    ON pop_publications(keystone_block) WHERE valid IS NULL;

-- The Bitcoin proof behind a transfer's PoP anchoring.
ALTER TABLE tunnel_transfers
    ADD COLUMN IF NOT EXISTS pop_btc_txid  TEXT,
    ADD COLUMN IF NOT EXISTS pop_btc_block BIGINT;
