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

> Open `http://<host>:8080/graphql` in a browser to explore the schema interactively
> with GraphiQL.

---

## 2. The `Transfer` object

A `Transfer` is the core resource. Every field below is what Strait records for a
single tunnel transfer.

| GraphQL field (camelCase) | REST field (snake_case) | Type | Meaning |
|---|---|---|---|
| `id` | `id` | UUID | Deterministic id derived from the Hemi tx hash + log index. Stable — safe to use as a key. |
| `asset` | `asset` | String | `BTC`, `ETH`, or an ERC-20 symbol. |
| `direction` | `direction` | Enum | `IN` (into Hemi) or `OUT` (out of Hemi). |
| `route` | `route` | Enum | `BTC_TO_HEMI` \| `HEMI_TO_BTC` \| `ETH_TO_HEMI` \| `HEMI_TO_ETH`. |
| `amount` | `amount` | String | **Atomic units** (satoshis or wei) as a plain decimal string. See [§5](#5-amounts). |
| `sender` | `sender` | String | Origin address (EVM `0x…` or a Bitcoin address). |
| `recipient` | `recipient` | String | Destination address. |
| `status` | `status` | Enum | `INITIATED` \| `ANCHORED` \| `FINALIZED` \| `FAILED` \| `REORGED`. |
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

---

## 3. The transfer lifecycle

`status` is what you poll for. The progression depends on the route:

```
ETH_TO_HEMI  (deposit) :  INITIATED ─────────────► FINALIZED
BTC_TO_HEMI  (deposit) :  INITIATED ──► ANCHORED ─► FINALIZED
HEMI_TO_ETH  (withdraw):  INITIATED ─────────────► FINALIZED   (after L1 challenge window)
HEMI_TO_BTC  (withdraw):  INITIATED ──► ANCHORED ─► FINALIZED
```

- `INITIATED` — the transfer has been seen on its source chain.
- `ANCHORED` — (BTC routes) committed to Bitcoin via Proof-of-Proof. `popAnchored=true`.
- `FINALIZED` — complete and irreversible.
- `FAILED` / `REORGED` — terminal failure / rolled back by a chain reorg.

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

# Aggregate stats.
{ stats { totalTransfers } }
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
- **`popAnchored` / `ANCHORED` / PoP fields** depend on PoP keystone ingestion. If
  PoP anchoring isn't wired in your deployment, these stay `false`/`null` and BTC
  deposits remain `INITIATED` — don't gate UX on `ANCHORED` unless you've confirmed
  it's live.
- **Withdrawals** stay `INITIATED` until their destination leg is matched (ETH
  withdrawals also wait out the ~7-day OP-Stack challenge window).

---

## 7. Real-time updates (polling)

There is **no subscription/webhook endpoint yet** — poll. Recommended pattern:

```
1. Submit a bridge tx; compute/record nothing special — just the recipient address.
2. Poll transfersByRecipient(recipient) every ~10–15s.
3. Find your transfer (match on amount + route + recent initiatedAt, or store its id).
4. Stop when status == FINALIZED (or FAILED / REORGED).
```

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
