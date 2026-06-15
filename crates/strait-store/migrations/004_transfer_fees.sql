-- Migration 004: per-leg network fees
--
-- Records the gas/fee spent on each leg of a transfer, in that chain's atomic unit:
--   * EVM legs (Hemi, Ethereum): wei  (gasUsed * effectiveGasPrice from the receipt)
--   * Bitcoin legs:              sats (sum of inputs - sum of outputs)
-- Nullable: a leg that hasn't been observed yet (or predates this migration) is NULL.

ALTER TABLE tunnel_transfers
    ADD COLUMN IF NOT EXISTS source_fee NUMERIC(78, 0),
    ADD COLUMN IF NOT EXISTS dest_fee   NUMERIC(78, 0);
