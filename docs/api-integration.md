# Integrating the Strait API

Strait indexes every cross-chain transfer through Hemi's Bitcoin and Ethereum
tunnels and serves it as a read API. This guide is for developers building on top
of that data — wallets, bridge UIs, portfolio trackers, notification bots, or any
app that needs to know the state of a tunnel transfer.

- **No SDK required.** The API is plain HTTP (GraphQL + REST); integrate with any
  language using a normal HTTP client.
- **Two interfaces:** a **GraphQL** endpoint (richer — filter by recipient, fetch a
  single transfer, aggregate stats) and a small **REST** API (simplest to curl).

---

## 1. Base URL & endpoints

The node serves the API on `API_HOST:API_PORT` (default `0.0.0.0:8080`).

| Method & path | Interface | Purpose |
|---|---|---|
| `POST /graphql` | GraphQL | Execute queries |
| `GET  /graphql` | GraphQL | In-browser GraphiQL playground |
| `GET  /transfers?limit=&offset=` | REST | List transfers (most recent first) |
| `GET  /transfers/:id` | REST | One transfer by UUID |
| `GET  /health` | REST | Liveness |
| `GET  /health/db` | REST | Database connectivity |
| `POST /webhooks` | REST | Register a webhook subscription (see [§10](#10-webhooks)) |
| `GET  /webhooks/:id` | REST | Inspect a subscription (`X-Management-Token`) |
| `GET  /webhooks/:id/deliveries` | REST | Last 20 delivery attempts (`X-Management-Token`) |
| `DELETE /webhooks/:id` | REST | Remove a subscription (`X-Management-Token`) |

> Open `http://<host>:8080/graphql` in a browser to explore the schema interactively
> with GraphiQL.

---

## 2. The `Transfer` object

A `Transfer` is the core resource. Every field below is what Strait records for a
single tunnel transfer.

| GraphQL field (camelCase) | REST field (snake_case) | Type | Meaning |
|---|---|---|---|
| `id` | `id` | UUID | Deterministic id derived from the Hemi tx hash + log index. Stable — safe to use as a key. |
| `asset` | `asset` | String | `BTC`, `ETH`, or an ERC-20 symbol. **Not implied by `route`** — see the note below. |
| `direction` | `direction` | Enum | `IN` (into Hemi) or `OUT` (out of Hemi). |
| `route` | `route` | Enum | `BTC_TO_HEMI` \| `HEMI_TO_BTC` \| `ETH_TO_HEMI` \| `HEMI_TO_ETH`. |
| `amount` | `amount` | String | **Atomic units** (satoshis or wei) as a plain decimal string. See [§5](#5-amounts). |
| `sender` | `sender` | String | Origin address (EVM `0x…` or a Bitcoin address). See the `BTC_TO_HEMI` caveat in [§6](#6-field-caveats). |
| `recipient` | `recipient` | String | Destination address. |
| `status` | `status` | Enum | `INITIATED` \| `FINALIZED` \| `FAILED` \| `REORGED`. |
| `sourceChain` | `source_chain` | Enum | `BITCOIN` \| `HEMI` \| `ETHEREUM`. |
| `sourceTxHash` | `source_tx_hash` | String | Tx hash on the source chain. |
| `sourceBlock` | `source_block` | Int | Source block height (`0` = source leg not yet observed; see [§6](#6-field-caveats)). |
| `sourceTimestamp` | `source_timestamp` | DateTime | ISO-8601 UTC. |
| `destChain` | `dest_chain` | Enum? | Destination chain, or `null` if not yet known. |
| `destTxHash` | `dest_tx_hash` | String? | Destination tx hash, or `null`. |
| `destBlock` | `dest_block` | Int? | Destination block height, or `null`. |
| `popAnchored` | `pop_anchored` | Bool | Whether the transfer is PoP-anchored to Bitcoin. |
| `popKeystoneBlock` | `pop_keystone_block` | Int? | Hemi keystone block (multiple of 25) that anchored it. |
| `popScore` | `pop_score` | Int? | Aggregate PoP score of the anchoring keystone. |
| `popAnchoredAt` | `pop_anchored_at` | DateTime? | When it anchored. |
| `initiatedAt` | `initiated_at` | DateTime | When the transfer was first observed. |
| `finalizedAt` | `finalized_at` | DateTime? | When it reached `FINALIZED`, or `null`. |
| — | `created_at`, `updated_at` | DateTime | REST only — row bookkeeping timestamps. |

> **`route` is a path, not a currency.** `ETH_TO_HEMI` and `HEMI_TO_ETH` cover every
> asset the OP Stack standard bridge tunnels between Ethereum and Hemi — native ETH,
> the HEMI token, WBTC, cbBTC, and any other bridged ERC-20 — not just `ETH`. Two
> transfers on the same route can legitimately have different `asset` values. Always
> read `asset` for the display unit; never infer it from `route` or `direction`.
> `BTC_TO_HEMI` / `HEMI_TO_BTC` are the one exception — those routes only ever carry
> native `BTC` (there's no BTC-side ERC-20 equivalent).

---

## 3. The transfer lifecycle

`status` is what you poll for. All routes go directly from `INITIATED` to `FINALIZED`:

```
ETH_TO_HEMI  (deposit) :  INITIATED ─────────────► FINALIZED
BTC_TO_HEMI  (deposit) :  INITIATED ─────────────► FINALIZED   (at Hemi mint)
HEMI_TO_ETH  (withdraw):  INITIATED ─────────────► FINALIZED   (after L1 challenge window)
HEMI_TO_BTC  (withdraw):  INITIATED ─────────────► FINALIZED   (when BTC payout detected)
```

- `INITIATED` — the transfer has been seen on its source chain but not yet complete.
- `FINALIZED` — complete. The recipient has their funds.
- `FAILED` — terminal failure. For `HEMI_TO_BTC`: the operator did not pay the Bitcoin payout within the deadline and a `WithdrawalChallengeSuccess` event fired on `BitcoinTunnelManager` — the user's hBTC is automatically re-minted to them by the contract. The transfer is terminal; no further action is needed. Users who receive a FAILED `HEMI_TO_BTC` should see their hBTC balance restored on Hemi.
- `REORGED` — the source-chain transaction was rolled back by a chain reorganization before the transfer completed. The user's funds are safe on the source chain (a reorg undoes the transaction, so the tokens were never spent), but the transfer will not proceed. The user must re-initiate the transfer from scratch. REORGED can occur on any chain — Bitcoin (rare, deep reorgs only), Ethereum, or Hemi — if a block containing the initiating transaction is orphaned.

**PoP anchoring is separate from `FINALIZED`.** For `BTC_TO_HEMI` deposits, the transfer reaches `FINALIZED` as soon as the hBTC mint is confirmed on Hemi — the user has their funds. Bitcoin-grade finality (anchoring to the Bitcoin chain via Hemi's Proof-of-Publication system) is tracked independently by the `popAnchored` / `popKeystoneBlock` fields and does not gate `FINALIZED`. When a PoP keystone covers a deposit's mint block, `popAnchored` flips to `true` while `status` stays `FINALIZED`.

A robust integration treats `FINALIZED` as "done" and anything else as "in flight."

---

## 4. Queries

### GraphQL

```graphql
# List recent transfers (newest first). limit clamped to 1..=500 (default 50).
{
  transfers(limit: 20, offset: 0) {
    id route asset amount status sourceTxHash destTxHash initiatedAt finalizedAt
  }
}

# A single transfer by id.
{
  transfer(id: "a2ce3b2d-7110-520c-8999-d21d1f88d1e5") {
    id route status popAnchored destChain destBlock recipient
  }
}

# All transfers for a recipient address (case-insensitive). The query for a wallet UI.
{
  transfersByRecipient(recipient: "0x64eac284be878ad740fbf5b0eb3827f49825951f", limit: 50) {
    id route asset amount status finalizedAt
  }
}

# Search by address / tx hash / id, with optional status & route filters.
{
  searchTransfers(query: "0xabc123…", status: "FINALIZED", route: "HEMI_TO_BTC") {
    id amount status finalizedAt
  }
}

# Aggregate stats — optionally scoped to a time window.
{ stats { totalTransfers finalized failed } }
{ stats(window: LAST_24H) { totalTransfers finalized } }

# Time-bucketed analytics: transfer count + volume per route/asset.
# window: LAST_24H | LAST_7D | LAST_30D | ALL_TIME · granularity: DAY | WEEK | MONTH
# volume is atomic units (sats/wei) per asset — convert client-side, never sum across assets.
{
  analyticsSeries(window: LAST_30D, granularity: DAY) {
    bucketStart route asset transferCount volume
  }
}

# Which route dominates a window (share is 0–1 of total transfers).
{ routeBreakdown(window: LAST_7D) { route transferCount share } }
```

All list queries accept `limit` (1–500, default 50) and `offset` (default 0).

### REST

```bash
# List
curl 'http://localhost:8080/transfers?limit=20&offset=0'

# One transfer
curl 'http://localhost:8080/transfers/a2ce3b2d-7110-520c-8999-d21d1f88d1e5'
```

REST responses wrap the payload: `{ "transfers": [...], "limit": 20, "offset": 0 }`
and `{ "transfer": {...} }`. Fields are **snake_case** (vs camelCase in GraphQL).

---

## 5. Amounts

`amount` is always in the asset's **smallest atomic unit**, as a string (never a
float — avoids precision loss):

| Asset | Atomic unit | Divisor for display |
|---|---|---|
| BTC | satoshi | `10^8` |
| ETH | wei | `10^18` |
| ERC-20 | token base unit | `10^(token decimals)` |

```js
// BTC: 99500 sats → 0.000995 BTC
const btc = Number(amount) / 1e8;
// ETH: 130000000000000000 wei → 0.13 ETH
const eth = BigInt(amount); // use BigInt / a decimal lib for wei
```

> **ERC-20 decimals:** Strait stores the raw on-chain amount. Tokens are not always
> 18 decimals — apply the specific token's `decimals()` before display, or a
> small-decimal token can look like `0`.

---

## 6. Field caveats (read before depending on a field)

These reflect what is reliably populated **today**:

- **`sourceBlock == 0` on an `ETH_TO_HEMI` transfer** means the Ethereum-L1 deposit
  leg hasn't been matched yet — treat the source tx as *pending*, not real. (The
  dashboard shows it as "L1 deposit not yet matched.") The **destination** (Hemi
  mint) tx is real.
- **BTC recipient on `HEMI_TO_BTC`** is recovered from the withdrawal transaction's
  calldata. If recovery fails it falls back to a `withdrawal-uuid-<n>` placeholder.
- **`sender` on `BTC_TO_HEMI`** is always a `btctx:<txid>` placeholder, never a real
  Bitcoin address. Bitcoin is UTXO-based — a deposit transaction can spend from
  multiple input addresses, so there's no single unambiguous "sender" the way an EVM
  tx has a `from`. Rather than guess, Strait stores a stable identifier keyed to the
  deposit's own txid. This is permanent, not a placeholder awaiting enrichment — don't
  poll waiting for it to become a real address.
- **`popAnchored` / PoP fields** are set when a PoP keystone covers a `BTC_TO_HEMI`
  deposit's Hemi mint block. PoP anchoring is not required for `FINALIZED` — a deposit
  reaches `FINALIZED` at mint, and `popAnchored` upgrades to `true` independently when
  the keystone fires. If PoP is not yet active on the network, `popAnchored` stays
  `false` but `status` is still `FINALIZED`.
- **Withdrawals** stay `INITIATED` until their destination leg is matched. ETH
  withdrawals go through two on-chain steps before `FINALIZED`: someone must call
  `proveWithdrawalTransaction` on Ethereum (advances to `PROVING` — Strait does not do
  this automatically, and an un-proven withdrawal waits indefinitely, not just ~1 day),
  then Hemi's ~1 day challenge window elapses (shortened from the standard OP Stack
  7 days by anchoring finality to Bitcoin via PoP) before `finalizeWithdrawalTransaction`
  releases the funds.

---

## 7. Real-time updates (polling or webhooks)

For push notifications, register a webhook ([§10](#10-webhooks)). If you'd rather
poll (or need the state right now rather than on change), the recommended pattern:

```
1. Submit a bridge tx. You already know its tx hash — use it, don't fuzzy-match.
2. Poll searchTransfers(query: "<your source tx hash>") every ~10–15s.
3. Stop when status == FINALIZED (or FAILED / REORGED). Cache the returned `id`
   for any follow-up queries (e.g. transfer(id: ...)) once you have it.
```

**Use your known tx hash, not amount + route + time matching.** `searchTransfers` does a
substring match across `id`, `sender`, `recipient`, and both tx hashes, so
`searchTransfers(query: "0xabc123...")` finds your transfer directly and unambiguously.
Matching by amount/route/timestamp instead is fragile — it's the same kind of heuristic
Strait's own cross-chain event matcher uses internally, and it's exactly the sort of thing
that breaks when two transfers of the same amount land close together. Don't reimplement
it client-side when you already have a better key.

If you don't have a tx hash yet (e.g. you only know the user's wallet address), fall back
to `transfersByRecipient(recipient)` and disambiguate by `initiatedAt` recency.

A new transfer typically appears within seconds of finality on the source chain
(after the indexer's confirmation buffer), so a 10–15s poll is plenty.

---

## 8. End-to-end examples

### TypeScript / JavaScript (GraphQL)

```ts
async function transfersFor(recipient: string) {
  const res = await fetch("http://localhost:8080/graphql", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      query: `query($r: String!) {
        transfersByRecipient(recipient: $r, limit: 50) {
          id route asset amount status finalizedAt
        }
      }`,
      variables: { r: recipient },
    }),
  });
  const { data } = await res.json();
  return data.transfersByRecipient;
}
```

### Python (REST)

```python
import requests
r = requests.get("http://localhost:8080/transfers", params={"limit": 20})
for t in r.json()["transfers"]:
    print(t["route"], t["status"], t["amount"], t["recipient"])
```

### curl (GraphQL)

```bash
curl -s http://localhost:8080/graphql \
  -H 'content-type: application/json' \
  --data '{"query":"{ stats { totalTransfers } }"}'
```

---

## 9. Conventions & limits

- **Pagination:** `limit` is clamped to `1..=500` (default 50); `offset` defaults to 0.
- **Ordering:** lists are newest-first by `initiatedAt`.
- **Casing:** GraphQL = camelCase, REST = snake_case.
- **Errors:** GraphQL returns a top-level `errors` array; REST uses HTTP status codes
  (`404` unknown id, `500` server error, `503` from `/health/db` when the DB is down)
  with `{ "error": "…" }`.
- **Stability:** `id`, `route`, `status`, and the atomic-unit `amount` are stable
  contracts. Treat enum sets as potentially additive over time.

---

## 10. Webhooks

Push notifications for transfer lifecycle events — an HMAC-signed JSON POST to
your URL whenever a matching transfer changes. Backed by a durable outbox, so a
node restart never drops a delivery. Delivery is **at-least-once**: dedupe on
the `X-Strait-Delivery` header if a replay would hurt you.

### Registering

```bash
curl -X POST http://localhost:8080/webhooks \
  -H 'content-type: application/json' \
  -d '{
    "url": "https://example.com/strait-hook",
    "routes":   ["HEMI_TO_BTC", "HEMI_TO_ETH"],
    "assets":   ["BTC", "ETH"],
    "statuses": ["FINALIZED", "FAILED"]
  }'
```

Filters are optional — omit a dimension to match everything on it. The URL must
be public `http(s)`; loopback/private/link-local hosts are rejected.

The `201` response contains two credentials **returned only once**:

- `signing_secret` — HMAC-SHA256 key used to sign every delivery to you.
- `management_token` — required (as the `X-Management-Token` header) to
  `GET /webhooks/:id` (inspect) or `DELETE /webhooks/:id` (unsubscribe).

Losing them means registering a new webhook — the API never discloses them again.

### Deliveries

Each event is a POST with headers:

| Header | Meaning |
|---|---|
| `X-Strait-Signature` | `sha256=<hex HMAC-SHA256 of the raw request body under your signing_secret>` |
| `X-Strait-Event` | `transfer.created` \| `transfer.status_changed` \| `transfer.pop_anchored` \| `transfer.retracted` |
| `X-Strait-Delivery` | Unique delivery id — your dedupe key |

Body: `{ "event": "...", "timestamp": "...", "transfer": { ... } }` where
`transfer` is the same snake_case shape as `GET /transfers` rows (see [§2](#2-the-transfer-object)).

Respond with any `2xx` within 10 seconds to acknowledge. Anything else (or a
timeout) schedules a retry with exponential backoff — 10s, 1m, 10m, 1h, 6h, then
24h — up to 8 attempts before the delivery is marked permanently failed.

To debug your receiver, list the last 20 attempts (event, status, attempt count,
response time in ms, last error):

```bash
curl http://localhost:8080/webhooks/<id>/deliveries -H 'X-Management-Token: <token>'
```

The explorer's **/webhooks** page does the same in a browser — register,
inspect delivery history, delete — if you'd rather not curl.

### Verifying the signature

Always verify before trusting a payload — anyone who discovers your endpoint URL
can POST fake events to it; only Strait knows your `signing_secret`.

```js
import { createHmac, timingSafeEqual } from "node:crypto";

function verify(rawBody /* Buffer */, signatureHeader, secret) {
  const expected = "sha256=" + createHmac("sha256", secret).update(rawBody).digest("hex");
  return timingSafeEqual(Buffer.from(signatureHeader), Buffer.from(expected));
}
```

```python
import hashlib, hmac

def verify(raw_body: bytes, signature_header: str, secret: str) -> bool:
    expected = "sha256=" + hmac.new(secret.encode(), raw_body, hashlib.sha256).hexdigest()
    return hmac.compare_digest(signature_header, expected)
```

Sign-and-compare must use the **raw request bytes** — re-serializing the parsed
JSON can reorder keys and break the digest.

**What the two crypto calls are doing** (`node:crypto` is built into Node — no
npm package needed):

- `createHmac("sha256", secret)` recomputes the signature Strait attached:
  HMAC-SHA256 of the body under your `signing_secret`. Only a holder of the
  secret can produce a valid signature, and changing even one byte of the body
  changes it completely — so a match proves the payload came from Strait and
  wasn't tampered with. Without this check, anyone who discovers your endpoint
  URL could POST a fake `"status": "FINALIZED"` event and your backend would
  believe it.
- `timingSafeEqual` compares the two signatures in **constant time**. A plain
  `===` short-circuits at the first mismatched character, so failures return a
  few nanoseconds faster or slower depending on *how much* of a forged
  signature was correct — enough signal, over many requests, to reconstruct a
  valid signature byte by byte (a timing attack). `timingSafeEqual` always
  compares every byte, so response timing leaks nothing. (Python's
  `hmac.compare_digest` is the same idea.)

### Storing credentials & handling subscriptions

**Register one subscription per service, not per end-user.** Strait doesn't
know about your users — it notifies *you* about transfers. A wallet with 10,000
users runs **one** subscription (per environment) pointed at its backend; when
a delivery arrives, match `transfer.recipient` (or `sender`) against your own
users table to decide who to notify. Don't create a webhook per user — you'd
be managing thousands of secrets for no gain, and every subscription receives
its own copy of every matching event anyway.

**Store the three values from registration like API keys:**

| Value | Secret? | Where to keep it |
|---|---|---|
| `id` | No | Config/env — needed for `GET`/`DELETE /webhooks/:id` |
| `signing_secret` | **Yes** | Env var or secret manager (`STRAIT_SIGNING_SECRET`) — your receiver reads it to verify deliveries |
| `management_token` | **Yes** | Secret manager — only needed when inspecting or deleting the subscription |

Never in the repo, never in client-side code — the signing secret is only
useful server-side anyway, since verification happens where deliveries land.

**Environments:** register separately for staging and production, each with its
own URL and its own secrets. Filters can differ too (e.g. staging subscribes to
everything, production only to `FINALIZED`/`FAILED`).

**Rotation and loss:** the API never re-discloses secrets. To rotate, register
a new subscription, let your receiver accept both secrets during the cutover,
then `DELETE` the old one. If you lose the `management_token`, you can't delete
the subscription yourself — keep returning `2xx` from your receiver (and ignore
the events) so it doesn't burn retries, and ask the operator to remove the row.

### Receiving deliveries in your backend

The two rules every receiver must follow:

1. **Verify over the raw request bytes** — parse JSON only *after* the HMAC
   check. Body-parsing middleware that re-serializes will break the digest.
2. **Acknowledge fast** — return `2xx` within 10s, then do your real work
   (DB writes, notifications) asynchronously. A slow handler looks like a
   failure and triggers a retry, which you'll then process twice.

**Express:**

```js
import express from "express";
import { createHmac, timingSafeEqual } from "node:crypto";

const app = express();
const SECRET = process.env.STRAIT_SIGNING_SECRET;

// express.raw (NOT express.json) so we can verify the exact bytes.
app.post("/strait-hook", express.raw({ type: "application/json" }), (req, res) => {
  const sig = req.get("X-Strait-Signature") ?? "";
  const expected = "sha256=" + createHmac("sha256", SECRET).update(req.body).digest("hex");
  if (sig.length !== expected.length ||
      !timingSafeEqual(Buffer.from(sig), Buffer.from(expected))) {
    return res.status(401).send("bad signature");
  }

  res.status(200).send("ok"); // ack first — work after

  const deliveryId = req.get("X-Strait-Delivery"); // your dedupe key
  const { event, transfer } = JSON.parse(req.body);
  if (event === "transfer.status_changed" && transfer.status === "FINALIZED") {
    // e.g. mark the user's bridge as complete, send a push notification…
  }
});
```

**Next.js (App Router route handler):**

```ts
// app/api/strait-hook/route.ts
import { createHmac, timingSafeEqual } from "node:crypto";

export async function POST(req: Request) {
  const raw = Buffer.from(await req.arrayBuffer()); // raw bytes, not req.json()
  const sig = req.headers.get("x-strait-signature") ?? "";
  const expected = "sha256=" +
    createHmac("sha256", process.env.STRAIT_SIGNING_SECRET!).update(raw).digest("hex");
  if (sig.length !== expected.length ||
      !timingSafeEqual(Buffer.from(sig), Buffer.from(expected))) {
    return new Response("bad signature", { status: 401 });
  }

  const deliveryId = req.headers.get("x-strait-delivery"); // your dedupe key
  const { event, transfer } = JSON.parse(raw.toString());
  // handle the event (keep it quick, or hand off to a queue)…
  return new Response("ok");
}
```

### Recommended integration pattern (webhook + poll reconciliation)

For a bridge UI or wallet backend tracking user transfers end-to-end:

1. **On submit** — your user submits the bridge tx; you know its source tx
   hash. Store a `pending` row in your own DB keyed by that hash.
2. **On webhook** — verify the signature, dedupe on `X-Strait-Delivery`, match
   `transfer.source_tx_hash` (or `transfer.id`) to your row, update its status,
   notify the user. Subscribe with `statuses: ["FINALIZED", "FAILED"]` if
   that's all you act on.
3. **Reconcile** — webhooks are at-least-once, but if your endpoint is down
   longer than the retry window (~1.5 days) a delivery can be permanently
   failed. Run a periodic sweep (cron every ~10 min) over your still-`pending`
   rows and resolve them by polling:

```ts
async function fetchByTxHash(txHash: string) {
  const res = await fetch("http://localhost:8080/graphql", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      query: `query($q: String) {
        searchTransfers(query: $q, limit: 1) {
          id status route asset amount destTxHash finalizedAt
        }
      }`,
      variables: { q: txHash },
    }),
  });
  const { data } = await res.json();
  return data?.searchTransfers?.[0] ?? null;
}
```

The webhook gives you low latency; the sweep guarantees you never miss a
terminal state. Both read the same records, so they can share handling code.
