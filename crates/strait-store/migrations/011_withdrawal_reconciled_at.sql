-- Migration 011: reconciler rotation cursor for Hemi→ETH withdrawals
--
-- The withdrawal reconciler re-checks pending withdrawals against the L1
-- OptimismPortal every tick, but only a bounded batch per tick. ~27% of
-- withdrawals are never proven and stay INITIATED indefinitely, so ordering by
-- recency alone would starve older rows. `reconciled_at` records the last check;
-- each tick takes the least-recently-checked rows first.

ALTER TABLE tunnel_transfers
    ADD COLUMN IF NOT EXISTS reconciled_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_eth_withdrawal_reconcile
    ON tunnel_transfers(reconciled_at NULLS FIRST)
    WHERE route = 'HEMI_TO_ETH'
      AND status IN ('INITIATED', 'PROVING')
      AND withdrawal_hash IS NOT NULL;
