# Strait Analytics — Implementation Plan

## Goal

A standalone analytics product — **not a page inside the indexer dashboard** — showing
tunnel activity over time: transaction counts and USD-denominated volume broken down
by day / week / month, plus which route (`BTC_TO_HEMI`, `HEMI_TO_BTC`, `ETH_TO_HEMI`,
`HEMI_TO_ETH`) dominates in a given window. Multiple chart types, comparable across
time windows, filterable by network (mainnet/testnet).

## Project shape: separate app, separate concern

Strait is heading toward a product suite (per `files (3)/03-strait-wedge-to-platform-strategy.md`
— tunnel indexing as the wedge, broader data platform as the destination), so this
is built as its own top-level project from day one rather than nested under the
existing dashboard:

```
strait/
├── crates/            # indexer backend — unchanged, untouched by this work
├── frontend/           # indexer/tunnel-explorer dashboard — unchanged
├── analytics/          # NEW — standalone Next.js app, own package.json, own deploy
│   ├── app/
│   ├── lib/
│   │   ├── strait.ts    # GraphQL client — reads the same Strait API over HTTP
│   │   └── prices.ts    # USD price fetch + cache (new, see below)
│   └── package.json
└── docs/
```

No shared workspace/package linking between `frontend/` and `analytics/` — they're
independent codebases that both happen to consume the same Strait GraphQL API over
plain HTTP. That's the entire integration surface, which is the point: the indexer
stays an indexer (data + API, nothing product-specific bolted on), and each
consumer product (`frontend` today, `analytics` next) owns its own presentation and
domain logic — including price conversion, which does **not** belong in `strait-api`.

## Do we need a webhook?

**No.** Webhooks (still unbuilt — see README "Open Items") push events to external
subscribers in real time. Analytics is the opposite shape: pull pre-aggregated
historical data on page load/refresh. `strait-api`'s existing GraphQL surface is the
right transport, extended with aggregation queries (below) — `analytics/` is just
another GraphQL client, same as `frontend/` is today.

## What `strait-api` needs to add (still lives in the indexer, still just data)

Today `strait-api` exposes row-level `transfers`/`transfer`/`searchTransfers` and a
single global `stats` snapshot — no time dimension, no volume, no route breakdown.
Add, additively, to the existing schema:

```graphql
enum TimeWindow { LAST_24H LAST_7D LAST_30D ALL_TIME }
enum Granularity { DAY WEEK MONTH }

type AnalyticsBucket {
  bucketStart: DateTime!
  route: String!
  asset: String!           # BTC | ETH | ERC20 — never summed across assets server-side
  transferCount: Int!
  volume: String!           # atomic units (sats / wei) — USD conversion happens in analytics/, not here
}

type RouteBreakdown {
  route: String!
  transferCount: Int!
  share: Float!             # 0.0-1.0 of total in window
}

extend type Query {
  analyticsSeries(window: TimeWindow!, granularity: Granularity!): [AnalyticsBucket!]!
  routeBreakdown(window: TimeWindow!): [RouteBreakdown!]!
}
```

Backed by one aggregation query in a new `strait-store/src/analytics.rs`:

```sql
SELECT
  date_trunc($1, initiated_at) AS bucket,   -- 'day' | 'week' | 'month'
  route, asset,
  COUNT(*)    AS transfer_count,
  SUM(amount) AS volume
FROM tunnel_transfers
WHERE initiated_at >= $2
GROUP BY bucket, route, asset
ORDER BY bucket ASC;
```

plus a cheap route-share query for `routeBreakdown`. No new table needed at current
data volumes; the roadmap already earmarks a ClickHouse cold-path (Phase 4) if this
ever needs to scale past a single indexed Postgres query — don't build a rollup
table speculatively now.

**strait-api stays dumb on purpose**: it returns atomic-unit volume per asset, full
stop. It does not know about USD, does not call any price API, does not carry a
price-feed dependency. That logic lives entirely in `analytics/`.

## USD price conversion (lives in `analytics/`, not in the indexer)

Source: **free HTTP price API** (CoinGecko). Two different lookups, because
historical accuracy matters for a "volume over time" chart — using today's BTC
price to value a transfer from three months ago would misrepresent the chart:

1. **Historical daily prices** — CoinGecko's `/coins/{id}/market_chart/range`
   (free tier) returns a price series for an arbitrary date range. Fetch once per
   asset per window, keyed by day, and convert each `AnalyticsBucket`'s atomic
   volume to USD using the price for that bucket's date.
2. **Current price** — for "live" headline stat tiles (e.g. "$X total volume,
   updated just now"), a simple `/simple/price` call is enough.

Caching is required, not optional — free-tier CoinGecko rate limits are tight
(~10-30 calls/min), and this app will be requesting the same historical ranges
repeatedly across page loads:

- Historical prices for **past, closed days never change** — cache them
  indefinitely (a small local store: SQLite file, or even a JSON file behind a
  Next.js route handler with `revalidate: false` after first fetch — no need for
  Postgres here, this is a separate app with its own minimal state).
- Current/today's price can use a short revalidate window (e.g. 60s) since it's
  only for a "right now" display, not historical accuracy.
- If a CoinGecko call fails or rate-limits, fall back to the last cached price
  rather than showing an error — a slightly stale USD estimate beats a broken chart.

`analytics/lib/prices.ts` owns this entirely; `analytics/lib/strait.ts` (the GraphQL
client, structurally similar to `frontend/lib/strait.ts` but a separate file, no
shared package) only fetches atomic volumes. The two are combined at render time:
`usdVolume = atomicVolume / 10^decimals * priceForBucketDate`.

## Frontend design (`analytics/app/`)

Chart types, matched to what each metric needs (invoke the `dataviz` skill when
implementing — no charting library exists in this new app yet, so palette/form
choices start from scratch rather than inheriting `frontend/`'s existing style):

| Metric | Chart | Why |
|---|---|---|
| Transaction count over time | Line/area chart, per-route stacked or multi-series | Trend + composition at once |
| USD volume over time | Line/area chart, single series (now that USD unifies BTC + ETH + ERC-20) | This is the chart cross-asset summing was blocked on before — USD conversion is exactly what unlocks it |
| Route breakdown for selected window | Horizontal bar or donut (pick one) | Small fixed cardinality (4 routes) |
| Status distribution | Stacked bar | Same data shape as the indexer dashboard's funnel, different app/visual language |
| Headline stat tiles (total transfers, total USD volume, most-used route) | Stat tiles with sparkline | Standard overview-page pattern |

Controls: `TimeWindow` selector (24h/7d/30d/all-time) and `Granularity` selector
(day/week/month), and a network toggle (mainnet/testnet) — same concept as
`frontend/`'s `NetworkSwitcher.tsx`, reimplemented here rather than shared, per the
decoupling goal.

## Phased plan

**Phase 1 — `strait-api` additions (indexer side, small and additive)**
- `strait-store/src/analytics.rs`: the two aggregation queries
- `analyticsSeries` / `routeBreakdown` added to GraphQL schema + resolvers
- DB-backed integration tests (existing `#[ignore]`d pattern)

**Phase 2 — scaffold `analytics/`**
- New Next.js app, own `package.json`, own `.env` (`STRAIT_API_URL` /
  `STRAIT_TESTNET_API_URL`, `COINGECKO_API_KEY` if using a paid tier later)
- `lib/strait.ts` (GraphQL client), `lib/prices.ts` (CoinGecko fetch + cache)
- Decide local cache mechanism for historical prices (lean toward a small SQLite
  file via a lightweight driver, avoids standing up Postgres for a cache-only need)

**Phase 3 — build the page**
- Chart set from the table above (Recharts is the default pick unless there's a
  reason otherwise — no extra build config needed)
- Wire `TimeWindow`/`Granularity`/network controls as URL search params

**Phase 4 — polish**
- Testnet: near-zero data today — confirm empty/zero states render sanely
- Rate-limit/backoff handling for CoinGecko; verify stale-price fallback behavior
- Confirm `strait-api` aggregation query performance as mainnet data grows

## Open questions

1. **CoinGecko free-tier limits** — confirm current rate limits and whether an API
   key is required even for the free tier (CoinGecko has tightened this before);
   budget for a paid tier if `analytics/` gets real traffic.
2. **ERC-20 pricing** — BTC and ETH have obvious CoinGecko IDs; each bridged ERC-20
   needs its own CoinGecko coin ID mapped from its contract address (or a
   token-list lookup) before it can be priced. Start with BTC/ETH only; add ERC-20
   pricing once specific tokens are actually seen crossing the tunnel.
3. **Bucket timezone** — Postgres `date_trunc` defaults to UTC; confirm that's the
   intended bucketing (probably yes, avoids DST edge cases) before both `strait-api`
   and CoinGecko's daily buckets need to agree on day boundaries.
4. **Deployment** — where does `analytics/` get hosted? Same target as `frontend/`
   (e.g. Vercel) or something else — affects whether the price cache can be a local
   SQLite file (fine for a single long-lived server) or needs to be a hosted store
   (needed if deployed to a serverless/edge platform with no persistent disk).
