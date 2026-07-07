-- BTC→Hemi deposits are FINALIZED as soon as the Hemi mint tx is confirmed.
-- PoP anchoring is an additional security property tracked by pop_anchored/pop_keystone_block,
-- not a prerequisite for the transfer to be considered complete by the user.
--
-- Advance all existing BTC_TO_HEMI rows that are still INITIATED (mint confirmed but no PoP yet)
-- or ANCHORED (old terminal status before this change) to FINALIZED.

UPDATE tunnel_transfers
   SET status     = 'FINALIZED',
       updated_at = NOW()
 WHERE route = 'BTC_TO_HEMI'
   AND status IN ('INITIATED', 'ANCHORED')
   AND dest_tx_hash IS NOT NULL;
