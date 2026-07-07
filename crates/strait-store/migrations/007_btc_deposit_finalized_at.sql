-- Backfill finalized_at for BTC→Hemi deposits that were upgraded to FINALIZED by
-- migration 006. initiated_at is set to the Hemi mint block time, which is the
-- correct finalization timestamp for these transfers.
UPDATE tunnel_transfers
   SET finalized_at = initiated_at,
       updated_at   = NOW()
 WHERE route = 'BTC_TO_HEMI'
   AND status = 'FINALIZED'
   AND finalized_at IS NULL;
