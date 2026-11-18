-- Migration 012: Bitcoin-side PoP publications
--
-- PoP miners publish Hemi keystones (every 25th Hemi block) to Bitcoin in
-- OP_RETURN outputs, whether or not the PoPPayoutsV2 reward contract is active.
-- Strait scans Bitcoin blocks for these publications and uses them to mark
-- transfers Bitcoin-final, and to answer "which Bitcoin tx anchors Hemi block N?".
--
-- pop_scanned_blocks is contiguous from the configured start height: the scanner
-- only ever appends the next height, and a Bitcoin reorg deletes from the fork
-- point up (cascading to its publications). That contiguity is what makes "the
-- earliest publication of any keystone >= N" a correct query.

CREATE TABLE IF NOT EXISTS pop_scanned_blocks (
    height       BIGINT PRIMARY KEY,
    hash         TEXT NOT NULL,
    block_time   TIMESTAMPTZ NOT NULL,
    publications INT NOT NULL DEFAULT 0,
    scanned_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS pop_publications (
    btc_txid         TEXT NOT NULL,
    vout             INT NOT NULL,
    btc_block_height BIGINT NOT NULL REFERENCES pop_scanned_blocks(height) ON DELETE CASCADE,
    btc_block_time   TIMESTAMPTZ NOT NULL,
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
