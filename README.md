# Strait

**Real-time, reorg-safe tunnel indexer for Hemi Network.**

Strait tracks assets moving through Hemi's trust-minimized bridges between Bitcoin, Hemi, and Ethereum. It ingests all three chains, joins related events into unified `TunnelTransfer` records with complete lifecycles — including **Bitcoin-anchored finality** via Hemi's PoP keystones — and serves that data via REST, GraphQL, and a web dashboard.

---

## What It Does

A complete tunnel flow touches at least three blockchains — each with different finality semantics and no shared identifier in raw form. Strait's job is to:

1. **Ingest** raw events from Bitcoin, Hemi, and Ethereum independently and concurrently
2. **Join** related cross-chain events into a single `TunnelTransfer` record using a stateful engine
3. **Track lifecycle** — `INITIATED → ANCHORED → FINALIZED` — with explicit handling for reorgs and failures
4. **Serve** the unified dataset via REST + GraphQL and a web dashboard, with a complete audit log

Supported tunnel routes:

| Route | Direction | Description |
|---|---|---|
| `BTC_TO_HEMI` | In | Bitcoin deposit → Hemi `DepositConfirmed` → hBTC minted |
| `HEMI_TO_BTC` | Out | `WithdrawalInitiated` on Hemi → BTC payout by vault operator |
| `ETH_TO_HEMI` | In | Ethereum `ETHBridgeInitiated` → Hemi `ETHBridgeFinalized` |
| `HEMI_TO_ETH` | Out | Hemi `ETHBridgeInitiated` → Ethereum release |

---

## Project Status

Strait runs end-to-end today: a single `strait-node` binary ingests Hemi + Ethereum
(and Bitcoin via BitcoinKit), joins events into `TunnelTransfer` records, persists them
to Postgres/Supabase, and serves them over REST + GraphQL with a web dashboard on top.

| Area | Status |
|---|---|
| Hemi + Ethereum EVM ingesters (live, reorg-aware) | ✅ Built |
| Join engine — `TunnelTransfer` lifecycle, PoP anchoring | ✅ Built |
| Bitcoin custody watcher (BitcoinKit, OP_RETURN decode) | ✅ Built — all 9 mainnet vault custody addresses configured |
| Postgres/Supabase persistence + auto-migrations | ✅ Built |
| Checkpointing / resumability / historical backfill | ✅ Built |
| REST (`/transfers`) + GraphQL (`/graphql`) API | ✅ Built |
| Web dashboard (Next.js tunnel explorer + finality timeline) | ✅ Built |
| Webhooks (push notifications) | ✅ HMAC-signed, durable outbox with retry, route/asset/status filters |
| Vault auto-discovery for the BTC watch-set | ✅ Vault addresses discovered and configured (9 vaults, June 2026) |

The GraphQL schema and setup steps documented below reflect what is **actually
implemented**. Sections marked _Planned_ are design targets, not yet shipped.

---

## Contract Addresses

All addresses confirmed from the Hemi explorer and the [`hemilabs/bitcoin-tunnel-contracts`](https://github.com/hemilabs/bitcoin-tunnel-contracts) repository.

### BTC Tunnel — `BitcoinTunnelManager`

| Network | Address |
|---|---|
| Hemi Mainnet | `0xEAcA824F46c000fB89403846Bb57e6b913321081` |
| Hemi Sepolia (testnet) | `0x8221CFD3Eca3c5F9FA27b2AE774151642f1C449e` |

### ETH/ERC-20 Tunnel — `L2StandardBridge` (OP Stack)

| Network | Address |
|---|---|
| Hemi Mainnet + Hemi Sepolia | `0x4200000000000000000000000000000000000010` |
| Ethereum Mainnet (L1) | `0x5eaa10F99e7e6D177eF9F74E519E319aa49f191e` |
| Ethereum Sepolia (L1) | `0xc94b1BEe63A3e101FE5F71C80F912b4F4b055925` |

### BitcoinKit Precompile

| Network | Address | Version |
|---|---|---|
| Hemi Mainnet | `0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12` | v1 |
| Hemi Sepolia | `0xeC9fa5daC1118963933e1A675a4EEA0009b7f215` | v0 |

### PoP Anchoring — `PoPPayoutsV2`

| Network | Address |
|---|---|
| Hemi Mainnet (canonical) | `0x9a23ab7cb11cfb96e577da52a6ad5211ff24434b` |
| Hemi Mainnet (first deployment) | `0x9417dd2eba413cfc11e8d8e368c007bfa1385a40` |
| Hemi Sepolia (testnet) | `0x4a3b61C586DB4CD219E85aC0697b66916c7457AB` |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  REST (/transfers)  │  GraphQL (/graphql + GraphiQL)  │  Web │
│                                                    dashboard │
└─────────────────────────────────────────────────────────────┘
                             │
┌─────────────────────────────────────────────────────────────┐
│  Postgres / Supabase  ←  strait-store                       │
└─────────────────────────────────────────────────────────────┘
                             │
┌─────────────────────────────────────────────────────────────┐
│  Join Engine  (strait-join)                                 │
│  State machine per transfer. Reorg-aware.                   │
└─────────────────────────────────────────────────────────────┘
           │                  │                  │
┌──────────────────┐  ┌──────────────┐  ┌───────────────────┐
│ Bitcoin Ingester │  │  Hemi EVM    │  │  Ethereum EVM     │
│ + CustodyWatcher │  │  Ingester    │  │  Ingester         │
│  (strait-btc)    │  │ (strait-evm) │  │  (strait-evm)     │
└──────────────────┘  └──────────────┘  └───────────────────┘
         │                    │
    BitcoinKit v1       BitcoinTunnelManager
    precompile          + L2StandardBridge
  (reads BTC state)    (emits tunnel events)
```

### Workspace Crates

| Crate | Role |
|---|---|
| `strait-core` | Domain types, events, config, timing constants, error types |
| `strait-bitcoin` | Bitcoin ingester — CustodyWatcher via BitcoinKit, OP_RETURN parsing, reorg detection |
| `strait-evm` | EVM ingester — handles both `BitcoinTunnelManager` (BTC) and `L2StandardBridge` (ETH/ERC-20) events |
| `strait-join` | Join engine — per-route state machine, cross-chain event matching, reorg retraction |
| `strait-store` | Database layer — SQLx/Postgres CRUD for transfers, events, proofs, checkpoints |
| `strait-api` | HTTP API — REST (`/transfers`) + GraphQL (`/graphql`, GraphiQL) on axum + async-graphql |
| `strait-node` | Binary entrypoint — wires all crates together, manages task lifecycle |

---

## BTC Tunnel Architecture

The BTC tunnel is not a single contract. It is a hub-and-spoke system:

- **`BitcoinTunnelManager`** — central contract. Emits all indexable events. Manages vault registry, mints/burns hBTC.
- **`SimpleBitcoinVault`** (multiple) — per-operator vaults, each with its own Bitcoin custody address. Operators compete on fees and availability.
- **`BTCToken` (hBTC)** — ERC-20 with 8 decimals (satoshi precision), deployed by `BitcoinTunnelManager`.

### BTC→Hemi deposit flow

```
1. User sends BTC to vault custody address on Bitcoin.
   Transaction must include an OP_RETURN output encoding their Hemi address.

2. Anyone calls confirmDeposit(vaultIndex, txid, outputIndex, extraInfo)
   on BitcoinTunnelManager.

3. BitcoinTunnelManager emits:
   DepositConfirmed(vault, recipient, depositTxId, depositSats, netSatsAfterFee)
   ← depositTxId is the Bitcoin txid — Strait's primary cross-chain join key.

4. hBTC is minted to recipient on Hemi.
```

### Hemi→BTC withdrawal flow

```
1. User calls initiateWithdrawal(vaultIndex, btcAddress, amount)
   on BitcoinTunnelManager. hBTC is burned immediately.

2. BitcoinTunnelManager emits:
   WithdrawalInitiated(vault, withdrawer, btcAddress[hashed], withdrawalSats, netSatsAfterFee, uuid)
   ← uuid (vaultIndex << 32 | vaultSpecificUUID) is the cross-chain join key.
   ← btcAddress is indexed (hashed in topic) — original string NOT recoverable from the log.

3. Vault operator sends BTC to the user's Bitcoin address.
   Payout transaction includes an OP_RETURN encoding the uuid.

4. Strait correlates the Hemi WithdrawalInitiated with the Bitcoin payout
   by matching the uuid from the Bitcoin OP_RETURN.
```

### OP_RETURN encoding (confirmed)

Two formats, parsed from `output.script` (not `opReturnData`):

| Format | Script length | Layout |
|---|---|---|
| Raw bytes | 22 bytes | `0x6a` `0x14` + 20 raw address bytes |
| ASCII hex | 42 bytes | `0x6a` `0x28` + 40 ASCII hex characters of the address |

The OP_RETURN output must be within the first 8 outputs of the transaction.

---

## PoP Anchoring — `PoPPayoutsV2`

Hemi anchors its blocks to Bitcoin through Proof-of-Publication (PoP) miners. Strait watches `PoPPayoutsV2` to advance BTC→Hemi transfers from `INITIATED` to `ANCHORED`.

### Contract

| Network | Address |
|---|---|
| Hemi Sepolia (testnet) | `0x4a3b61C586DB4CD219E85aC0697b66916c7457AB` |
| Hemi Mainnet (canonical) | `0x9a23ab7cb11cfb96e577da52a6ad5211ff24434b` |
| Hemi Mainnet (first deployment) | `0x9417dd2eba413cfc11e8d8e368c007bfa1385a40` |

Source: [`hemilabs/pop-payouts`](https://github.com/hemilabs/pop-payouts)

### How it works

Every **25 Hemi blocks** (~5 minutes) is a **keystone**. When PoP miners publish a keystone commitment to Bitcoin, the sequencer calls `mintPoPRewards()` on `PoPPayoutsV2`, which emits:

```solidity
event PayoutRoundExecuted(uint64 indexed blockRewarded, uint256 rewardPool, uint256 popScore);
```

`blockRewarded` is always a multiple of 25. All Hemi blocks in `(blockRewarded - 25, blockRewarded]` are now PoP-anchored on Bitcoin.

### Verification logic

A transfer whose Hemi mint landed at block `N` is anchored when:

```
keystone_for(N) = ceil(N / 25) * 25
anchored when: PayoutRoundExecuted(blockRewarded >= keystone_for(N))
```

**Example:**

```
Transfer DepositConfirmed at Hemi block 12351
  keystone_for(12351) = 12375

PayoutRoundExecuted(blockRewarded=12375) fires
  → covers blocks (12350, 12375]
  → block 12351 is included ✓
  → transfer advances to ANCHORED
```

### Checking anchoring status directly

```bash
# Query lastBlockRewarded — any block <= this value is PoP-anchored
cast call 0x4a3b61C586DB4CD219E85aC0697b66916c7457AB \
  "lastBlockRewarded()(uint64)" \
  --rpc-url https://testnet.rpc.hemi.network/rpc

# Example: returns 125000
# Transfer minted at block 124990 → keystone 125000
# 125000 <= 125000 → anchored ✓
# Transfer minted at block 125001 → keystone 125025
# 125025 > 125000 → not yet anchored
```

### Note on `popScore`

A `popScore` of `0` means no miners published that keystone to Bitcoin, but the sequencer still processed the round. **A zero score does not mean the keystone is unanchored** — it means there were no publications scored, but the round was still executed and the block range is still considered PoP-finalized for Strait's purposes.

---

## Domain Model

The central object is `TunnelTransfer`:

```
TunnelTransfer
  id              UUID — globally unique cross-chain identifier
  asset           BTC | ETH | ERC20
  direction       IN (to Hemi) | OUT (from Hemi)
  route           BTC_TO_HEMI | HEMI_TO_BTC | ETH_TO_HEMI | HEMI_TO_ETH
  amount          BigDecimal (satoshis for BTC, wei for ETH, token units for ERC20)
  sender          source-chain address
  recipient       destination-chain address
  status          INITIATED | ANCHORED | FINALIZED | FAILED | REORGED
  initiated_at    timestamp
  finalized_at    timestamp (null until finalized)
  source_tx       ChainTransaction (chain, hash, block, confirmations, timestamp)
  destination_tx  ChainTransaction (null until finalized)
  pop_proofs      [] keystone anchoring records (BTC routes) — one per PayoutRoundExecuted
  reorg_events    [] full audit log of reorgs that touched this transfer
```

### Transfer lifecycle by route

**BTC→Hemi:**
```
Bitcoin UTXO detected at custody address
  → INITIATED   (Strait sees the Bitcoin deposit via BitcoinKit)

DepositConfirmed emitted on Hemi (depositTxId is the join key)
  — transfer now has a Hemi destination block N

PoPPayoutsV2.PayoutRoundExecuted(blockRewarded=K) fires, where K = ceil(N/25)*25
  → ANCHORED    (keystone K covers block N — PoP-anchored on Bitcoin)
  — typically ~5-30 min after DepositConfirmed (next keystone boundary)

Bitcoin reorg covers the deposit block → REORGED (record preserved)
```

**Hemi→BTC:**
```
WithdrawalInitiated emitted on Hemi (uuid assigned)
  → INITIATED

Bitcoin payout tx detected (OP_RETURN uuid matches)
  → ANCHORED

Bitcoin payout reaches confirmation depth
  → FINALIZED   (up to ~12 hours for operator processing)
```

**ETH→Hemi:**
```
ETHBridgeInitiated / ERC20BridgeInitiated on Ethereum
  → INITIATED

ETHBridgeFinalized / ERC20BridgeFinalized on Hemi
  → FINALIZED   (~2 minutes typical)
```

**Hemi→ETH:**
```
ETHBridgeInitiated on Hemi
  → INITIATED

Release confirmed on Ethereum
  → FINALIZED   (~40 min + up to 24h proof submission window)
```

---

## API

### GraphQL

Served at `http://localhost:8080/graphql`, with an interactive **GraphiQL** playground at
the same URL in a browser. Field names are camelCase. This is the schema as implemented:

```graphql
type Query {
  transfers(limit: Int = 50, offset: Int = 0): [Transfer!]!
  transfer(id: UUID!): Transfer
  transfersByRecipient(recipient: String!, limit: Int = 50, offset: Int = 0): [Transfer!]!
  stats: Stats!
}

type Transfer {
  id: UUID!
  asset: String!            # BTC | ETH | ERC20
  direction: String!        # IN | OUT
  route: String!            # BTC_TO_HEMI | HEMI_TO_BTC | ETH_TO_HEMI | HEMI_TO_ETH
  amount: String!           # atomic units (sats for BTC, wei for ETH)
  sender: String!
  recipient: String!
  status: String!           # INITIATED | ANCHORED | FINALIZED | FAILED | REORGED
  sourceChain: String!
  sourceTxHash: String!
  sourceBlock: Int!
  sourceTimestamp: DateTime!
  destChain: String
  destTxHash: String
  destBlock: Int
  popAnchored: Boolean!     # anchored to Bitcoin via PoP keystone
  popKeystoneBlock: Int
  popScore: Int
  popAnchoredAt: DateTime
  initiatedAt: DateTime!
  finalizedAt: DateTime
}

type Stats { totalTransfers: Int! }
```

**Recent transfers:**

```graphql
{
  transfers(limit: 10) {
    id route asset amount status popAnchored popKeystoneBlock
    sourceChain sourceBlock destChain destBlock
  }
}
```

**One transfer by id:**

```graphql
{ transfer(id: "5eed0001-0000-4000-8000-000000000001") {
    route amount status popAnchored popKeystoneBlock popScore
    sourceTxHash destTxHash finalizedAt
} }
```

**A wallet's transfers (e.g. "show my bridges"):**

```graphql
{ transfersByRecipient(recipient: "0xab...") { route asset amount status } }
```

The REST endpoint `GET /transfers?limit=&offset=` returns the same records as JSON, and
`GET /health` is a liveness probe.

### Webhooks

Push notifications for transfer lifecycle events, HMAC-signed and filtered by
route/asset/status. Backed by a durable outbox (`webhook_deliveries`), so
deliveries survive restarts; failed POSTs retry with exponential backoff (10s →
24h, 8 attempts). Delivery is **at-least-once** — dedupe on the
`X-Strait-Delivery` header.

```bash
# Register (returns signing_secret + management_token ONCE — store them)
curl -X POST http://localhost:8080/webhooks \
  -H 'content-type: application/json' \
  -d '{"url": "https://example.com/hook", "routes": ["HEMI_TO_BTC"], "statuses": ["FINALIZED"]}'

# Inspect / delivery history / delete (requires the management token from registration)
curl http://localhost:8080/webhooks/<id> -H 'X-Management-Token: <token>'
curl http://localhost:8080/webhooks/<id>/deliveries -H 'X-Management-Token: <token>'
curl -X DELETE http://localhost:8080/webhooks/<id> -H 'X-Management-Token: <token>'
```

Or skip curl entirely — the explorer's `/webhooks` page registers and manages
subscriptions (including delivery history) in the browser.

Each delivery is a JSON POST with `X-Strait-Signature: sha256=<hex>` (HMAC-SHA256
of the raw body under your `signing_secret`), `X-Strait-Event` (e.g.
`transfer.status_changed`), and `X-Strait-Delivery` (unique id). Payload:
`{ "event", "timestamp", "transfer": { ...same shape as GET /transfers... } }`.
See [`docs/api-integration.md`](docs/api-integration.md) §10 for signature
verification examples.

---

## Setup

### Prerequisites

- **Rust** 1.75+ (2021 edition)
- **PostgreSQL 14+** — local, or a hosted **Supabase** project (see [`docs/supabase-setup.md`](docs/supabase-setup.md)). Migrations run automatically on startup; no `sqlx-cli` needed.
- **Hemi RPC endpoint** — mainnet `https://rpc.hemi.network/rpc` (keyless), testnet `https://testnet.rpc.hemi.network/rpc`
- **Ethereum RPC endpoint** — a keyless public node works (`https://ethereum-rpc.publicnode.com`), or your own Alchemy/Infura URL
- **Node.js 20+** — only if you want to run the dashboard
- A **Bitcoin node is optional** — Bitcoin state is read through the BitcoinKit precompile on Hemi

### Quick start

```bash
git clone https://github.com/Godbrand0/strait
cd strait && cp .env.example .env
```

Edit `.env` — the defaults target **Hemi mainnet** (keyless) and a keyless Ethereum node,
so the only value you must set is `DATABASE_URL`:

```bash
# Local Postgres:
DATABASE_URL=postgres://postgres:password@localhost:5432/strait?sslmode=disable
# …or a Supabase Session-pooler URL (see docs/supabase-setup.md):
# DATABASE_URL=postgres://postgres.<ref>:<pw>@aws-<n>-<region>.pooler.supabase.com:5432/postgres?sslmode=require

HEMI_RPC_URL=https://rpc.hemi.network/rpc       # keyless
ETH_RPC_URL=https://ethereum-rpc.publicnode.com  # keyless (swap in your own for production)

# Set to vault custody addresses to enable early deposit detection (see docs/btc-tunnel-guide.md)
BITCOIN_TUNNEL_ADDRESSES=
```

Then run the node — it connects, **applies migrations automatically**, and starts indexing:

```bash
cargo run -p strait-node        # add --release for production
# → API + GraphQL on http://localhost:8080 ,  GraphiQL at http://localhost:8080/graphql
```

`Ctrl+C` shuts down cleanly (tasks drain before exiting). A failing chain (e.g. a bad RPC)
is logged but does **not** take the node down — the API and healthy chains keep running.

**Backfill history** by pointing the start block at known activity (the ingester resumes
forward from there and persists checkpoints): set `HEMI_START_BLOCK` / `ETH_START_BLOCK`.

### Dashboard

A Next.js tunnel explorer lives in [`frontend/`](frontend) and reads the GraphQL API:

```bash
cd frontend
STRAIT_API_URL=http://localhost:8080/graphql npm install && npm run dev
# → http://localhost:3000/dashboard
```

The overview lists recent transfers with a live status funnel; each transfer has a
**finality-lifecycle** page (source deposit → Hemi mint → PoP keystone → finalized) with
outbound links to mempool.space / the Hemi explorer. No data yet? Seed a few demo rows:

```bash
DATABASE_URL='…' cargo run -p strait-store --example seed
```

### Mainnet addresses

```bash
HEMI_RPC_URL=https://rpc.hemi.network/rpc
HEMI_CHAIN_ID=43111
HEMI_TUNNEL_CONTRACT=0x4200000000000000000000000000000000000010
HEMI_BTC_TUNNEL_CONTRACT=0xEAcA824F46c000fB89403846Bb57e6b913321081
HEMI_BITCOIN_KIT_CONTRACT=0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12
HEMI_POP_PAYOUTS_CONTRACT=0x9a23ab7cb11cfb96e577da52a6ad5211ff24434b  # PoPPayoutsV2 mainnet canonical
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY
ETH_CHAIN_ID=1
ETH_TUNNEL_CONTRACT=0x5eaa10F99e7e6D177eF9F74E519E319aa49f191e
```

---

## Configuration Reference

| Variable | Default | Description |
|---|---|---|
| `BITCOIN_RPC_URL` | — | Bitcoin node RPC URL |
| `BITCOIN_RPC_USER` | — | RPC username |
| `BITCOIN_RPC_PASSWORD` | — | RPC password |
| `BITCOIN_TUNNEL_ADDRESSES` | — | Comma-separated vault custody addresses to watch |
| `BITCOIN_CONFIRMATION_DEPTH` | `6` | Blocks before a Bitcoin event is stable (~1 hour) |
| `HEMI_RPC_URL` | — | Hemi RPC endpoint |
| `HEMI_CHAIN_ID` | `43111` | Chain ID (743111 for testnet) |
| `HEMI_TUNNEL_CONTRACT` | `0x4200...0010` | L2StandardBridge — ETH/ERC-20 routes |
| `HEMI_BTC_TUNNEL_CONTRACT` | — | BitcoinTunnelManager — BTC routes |
| `HEMI_BITCOIN_KIT_CONTRACT` | — | BitcoinKit precompile |
| `HEMI_POP_PAYOUTS_CONTRACT` | — | PoPPayoutsV2 contract; watches PayoutRoundExecuted for BTC→Hemi PoP anchoring |
| `HEMI_START_BLOCK` | `0` | Block to begin indexing from |
| `HEMI_CONFIRMATION_DEPTH` | `3` | Blocks before a Hemi event is stable |
| `HEMI_LOG_RANGE` | `100` | Max block range per eth_getLogs (set to 5 for QuickNode Discover, 10 for Alchemy free) |
| `HEMI_POLL_INTERVAL_MS` | `1000` | Hemi poll interval in milliseconds (also used as backfill throttle sleep) |
| `ETH_RPC_URL` | — | Ethereum RPC endpoint |
| `ETH_CHAIN_ID` | `1` | Chain ID (11155111 for Sepolia) |
| `ETH_TUNNEL_CONTRACT` | — | L1StandardBridgeProxy on Ethereum |
| `ETH_START_BLOCK` | `0` | Block to begin indexing from |
| `ETH_CONFIRMATION_DEPTH` | `12` | Blocks before an Ethereum event is stable |
| `ETH_LOG_RANGE` | `100` | Max block range per eth_getLogs (set to 10 for Alchemy free tier) |
| `ETH_POLL_INTERVAL_MS` | `1000` | Ethereum poll interval in milliseconds |
| `DATABASE_URL` | — | PostgreSQL connection string |
| `API_HOST` | `0.0.0.0` | API bind address |
| `API_PORT` | `8080` | API bind port |

---

## Reorg Safety

Reorgs are treated as normal control flow, not edge cases:

- Each ingester maintains a rolling window of recent block hashes beyond its confirmation depth
- On each new block, the parent hash is verified against the stored hash for the prior block
- A mismatch triggers a backward walk to find the fork point, emitting a `BlockReorg` event
- The join engine retracts any in-flight transfers whose source transactions were reorged out
- Retractions are stored with a `reorg_at` timestamp — records are never hard-deleted

| Chain | Default confirmation depth | Approximate window |
|---|---|---|
| Bitcoin | 6 | ~1 hour |
| Hemi | 3 | ~30 seconds |
| Ethereum | 12 | ~3 minutes |

---

## Development

### Run tests

```bash
# Full suite — DB-backed tests are #[ignore]d and skipped without a database,
# so this needs no external services.
cargo test --workspace

# Opt into the live DB/GraphQL tests against a real Postgres/Supabase:
DATABASE_URL=postgres://... cargo test --workspace -- --ignored
```

### Structured logging

The default is clean (`info,sqlx=warn`). Opt into per-block / per-poll detail with `RUST_LOG`:

```bash
RUST_LOG=strait_evm=debug cargo run -p strait-node   # verbose ingester tracing
```

Key log lines to watch when verifying the indexer is working:

```
INFO strait_evm::ingester: DepositConfirmed — BTC deposited and hBTC minted on Hemi
INFO strait_evm::ingester: ETHBridgeFinalized (deposit on Hemi)
INFO strait_evm::ingester: New BTC tunnel vault created — add to watched addresses
INFO strait_evm::ingester: PayoutRoundExecuted — Hemi blocks anchored on Bitcoin keystone_block=12375 pop_score=342000
INFO strait_join::engine: Anchoring transfers count=3 keystone_block=12375
INFO strait_join::engine: Transfer INITIATED → ANCHORED transfer_id=550e8400-...
```

### Database migrations

Migrations in [`crates/strait-store/migrations/`](crates/strait-store/migrations) are
**applied automatically** when `strait-node` starts — there's nothing to run by hand. To
author a new one you can use [`sqlx-cli`](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli):

```bash
sqlx migrate add <name>   # scaffold a new migration (then edit the .sql)
```

---

## Open Items

| Item | Status |
|---|---|
| ETH/ERC-20 tunnel ABI | Confirmed — OP Stack `L2StandardBridge` events |
| BTC tunnel contract address + ABI | Confirmed — `BitcoinTunnelManager` from [`hemilabs/bitcoin-tunnel-contracts`](https://github.com/hemilabs/bitcoin-tunnel-contracts) |
| OP_RETURN encoding | Confirmed — 22-byte (raw) or 42-byte (ASCII hex) from `SimpleBitcoinVaultUTXOLogicHelper.sol` |
| PoP proof contract | Confirmed — `PoPPayoutsV2` via keystone anchoring from [`hemilabs/pop-payouts`](https://github.com/hemilabs/pop-payouts). Testnet: `0x4a3b61C586DB4CD219E85aC0697b66916c7457AB`. Mainnet: two deployments (see Contract Addresses). |
| `PoPPayoutsV2` mainnet address | Confirmed — two deployments exist, canonical: `0x9a23ab7cb11cfb96e577da52a6ad5211ff24434b`. `mintPoPRewards()` not yet called as of June 2026 — PoP payouts pending Hemi activation. |
| Reorg frequency in production | **Still open** — ask at Hemi office hour |

---

## License

MIT — see [LICENSE](LICENSE) for details.

---

## Documentation

In-depth developer guides live in [`docs/`](docs):

- [`docs/README.md`](docs/README.md) — documentation index
- [`docs/tunnel-architecture.md`](docs/tunnel-architecture.md) — how the three-chain tunnel system works end-to-end (start here)
- [`docs/contract-addresses.md`](docs/contract-addresses.md) — all confirmed contract addresses + BitcoinKit interface
- [`docs/btc-tunnel-guide.md`](docs/btc-tunnel-guide.md) — Bitcoin tunnel: vaults, OP_RETURN, deposit/withdrawal flows
- [`docs/eth-tunnel-guide.md`](docs/eth-tunnel-guide.md) — ETH/ERC-20 tunnel via L2StandardBridge
- [`docs/bitcoinkit-reference.md`](docs/bitcoinkit-reference.md) — reading Bitcoin state from Hemi via the BitcoinKit precompile
- [`docs/pop-anchoring.md`](docs/pop-anchoring.md) — PoP anchoring and Bitcoin finality
- [`docs/supabase-setup.md`](docs/supabase-setup.md) — hosting the database on Supabase
