-- Migration 008: analytics index
--
-- Supports the analyticsSeries/routeBreakdown GraphQL queries, which aggregate
-- volume with date_trunc(initiated_at) grouped by asset/route (optionally
-- filtered by status). No new table is needed — tunnel_transfers already
-- carries every column these queries read; this just gives the range scan
-- over initiated_at an index to use instead of a full table scan, with
-- asset/route/amount/status covered so the scan doesn't hit the heap.

CREATE INDEX IF NOT EXISTS idx_tunnel_transfers_analytics
    ON tunnel_transfers (initiated_at)
    INCLUDE (asset, route, amount, status);
