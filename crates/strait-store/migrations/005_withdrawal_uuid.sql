-- Migration 005: withdrawal uuid
--
-- The Hemi→BTC WithdrawalInitiated event carries a uuid (vaultIndex << 32 | vaultUUID).
-- The operator encodes the 4-byte vaultUUID in the OP_RETURN of the Bitcoin payout, so
-- storing it lets the payout watcher match a payout to its exact withdrawal — instead of
-- the ambiguous recipient+amount heuristic (which mis-attributes identical withdrawals).
-- Null for non-BTC-withdrawal routes.

ALTER TABLE tunnel_transfers
    ADD COLUMN IF NOT EXISTS withdrawal_uuid BIGINT;
