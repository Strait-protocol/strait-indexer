-- Migration 010: asset decimals + OP Stack withdrawal hash
--
-- 1. `asset_decimals` / `asset_contract` — the ingester already resolves an
--    ERC-20's decimals on-chain, but only the symbol was persisted, so every
--    consumer had to guess (and guessed 18, which misformats cbBTC / WBTC at 8 and
--    USDC / USDT at 6). NULL on rows written before this migration; the API fills
--    those from a known-symbol table.
--
-- 2. `withdrawal_hash` / `proven_at` — HEMI_TO_ETH withdrawals are matched to the
--    L1 OptimismPortal's `WithdrawalProven` event by the withdrawal hash emitted in
--    the Hemi-side `MessagePassed` log. (The event's `to` is always the
--    L1CrossDomainMessenger, never the user, so recipient matching can't work.)
--    Filled by the withdrawal reconciler in strait-node.

ALTER TABLE tunnel_transfers
    ADD COLUMN IF NOT EXISTS asset_decimals  SMALLINT,
    ADD COLUMN IF NOT EXISTS asset_contract  TEXT,
    ADD COLUMN IF NOT EXISTS withdrawal_hash TEXT,
    ADD COLUMN IF NOT EXISTS proven_at       TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_withdrawal_hash
    ON tunnel_transfers(withdrawal_hash) WHERE withdrawal_hash IS NOT NULL;

-- Backs the reconciler's sweeps over pending Hemi→ETH withdrawals.
CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_pending_eth_withdrawals
    ON tunnel_transfers(initiated_at)
    WHERE route = 'HEMI_TO_ETH' AND status IN ('INITIATED', 'PROVING');
