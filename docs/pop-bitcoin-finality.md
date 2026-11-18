# Bitcoin finality from PoP publications

Strait reads Hemi's Proof-of-Proof (PoP) keystone publications **directly from Bitcoin** and uses them to:

1. mark transfers **Bitcoin-final** (`popAnchored`), with the Bitcoin transaction as proof, and
2. answer, for **any Hemi block**, which Bitcoin transaction anchors it and how deeply.

This does not depend on the `PoPPayoutsV2` reward contract. Payouts have never run on mainnet (`lastBlockRewarded() = 0` on both deployments as of November 2026), but miners publish keystones to Bitcoin regardless.

---

## How PoP publications look on Bitcoin

Every 25th Hemi block is a **keystone**. PoP miners publish keystones in Bitcoin transactions as a single `OP_RETURN` output:

```text
6a 4c 50   "HEMI"   L2KeystoneAbrev (76 bytes)
OP_RETURN  PUSHDATA1(80)
```

| Bytes (after `HEMI`) | Field | Meaning |
|---|---|---|
| `0` | `version` | Currently `1` |
| `1..5` | `l1_block` | Ethereum origin block (big-endian) |
| `5..9` | `l2_block` | **The Hemi keystone block** (big-endian, a multiple of 25) |
| `9..20` | `parent_ep_hash` | First 11 bytes of the keystone block's parent hash |
| `20..32` | `prev_keystone_ep_hash` | First 12 bytes of the previous keystone's block hash |
| `32..64` | `state_root` | The keystone block's state root |
| `64..76` | `ep_hash` | First 12 bytes of the keystone block's hash |

Source: `hemilabs/heminetwork` (`hemi/pop/pop.go`, `hemi/hemi.go`, MIT). Confirmed on mainnet: Bitcoin block 976,349 contains 11 publications, keystones 5,757,525–5,757,775.

Miners publish in **batches**: many Bitcoin blocks contain none, and then one contains several. Not every keystone is published.

### Who publishes
Every publication observed on mainnet (130+ across 30+ Bitcoin blocks, November 2026) came from one wallet: `17Fv7httdZvqMubbvMkQ9KLPiTSPobxAqF`, which had made over 5,000 publication transactions. Strait follows the miner wallets listed in `POP_MINER_ADDRESSES` instead of downloading every Bitcoin block.

**Trade-off:** publications from wallets not in that list are not seen. If another wallet published a keystone first, Strait would name a *later* publication as the anchor. That's still a real Bitcoin proof, just with fewer confirmations than the true first one: Strait can understate finality this way, never overstate it. Add new miner wallets to `POP_MINER_ADDRESSES` as they appear.

## The finality rule

A published keystone commits to the keystone chain before it. So:

> Hemi block **N** is anchored by the **earliest Bitcoin publication of any valid keystone ≥ N**.

That is often a *later* keystone than N's own. Its Bitcoin confirmations are N's Bitcoin finality.

| Threshold | Confirmations | Used for |
|---|---|---|
| Bitcoin-final | 6 (`POP_MIN_CONFIRMATIONS`) | Setting `popAnchored` on transfers |
| Super-final | 10 | Hemi's own threshold (`bfgapi.BitcoinSuperFinality`); reported by the lookup |

## Validation

A publication is only trusted after Strait checks it against the canonical Hemi block it names: `ep_hash`, `parent_ep_hash` and `state_root` must match that block's hash, parent hash and state root. Forged or non-canonical publications are stored with `valid = false` and never anchor anything.

## How the scanner works

The node's PoP scanner (`crates/strait-node/src/pop_scanner.rs`) loops:

1. **Sync:** for each miner wallet, reads its confirmed transactions (via Esplora, 25 per page, newest first) back to where the last sync ended, re-reading 6 blocks for Bitcoin reorgs. The first sync reads back to `POP_SCAN_START_HEIGHT`. Publications go into `pop_publications`; one a reorg moved is updated in place.
2. **Validate:** checks new publications against Hemi (one block fetch per keystone).
3. **Anchor:** sets `popAnchored`, `popKeystoneBlock`, `popBitcoinTxid`, `popBitcoinBlock` and `popAnchoredAt` (the Bitcoin block time) on covered transfers, and sends the `transfer.pop_anchored` webhook.

A transfer's Hemi block is its **source block** for withdrawals and its **mint block** (`destBlock`) for deposits. All four routes are covered.

### Coverage guards
The earliest publication must be inside the range Strait has read, or it could name the wrong one:

- Only publications at or below the **synced height** count: the Bitcoin height up to which *every* listed miner's history has been read. A miner whose sync failed holds it back.
- **Transfers** are only anchored if their Hemi block is at least 2 hours newer than the start height's Bitcoin block (a margin for Bitcoin timestamp skew).
- **The lookup** answers `NOT_COVERED` below `coveredFromHemiBlock`: the highest keystone published in the first day after the start height, plus one. Keystones published that early may also have been published before it.
- Nothing is anchored past a publication that is still awaiting validation.

---

## API

### Transfer fields

| GraphQL | REST | Type | Meaning |
|---|---|---|---|
| `popBitcoinTxid` | `pop_btc_txid` | String? | Bitcoin tx of the publication that anchors this transfer |
| `popBitcoinBlock` | `pop_btc_block` | Int? | Its Bitcoin block height |

`popKeystoneBlock` is the anchoring keystone (≥ the transfer's Hemi block). `popAnchoredAt` is the Bitcoin block time of the publication. `popScore` is `null` for transfers anchored this way.

### Lookup any Hemi block

```graphql
{
  popFinality(hemiBlock: 5757610) {
    status            # ANCHORED | PENDING | NOT_COVERED
    keystoneBlock     # 5757625
    bitcoinTxid
    bitcoinBlock      # 976349
    bitcoinBlockTime
    confirmations
    bitcoinFinal      # confirmations >= 6
    superFinal        # confirmations >= 10
    coveredFromHemiBlock
    syncedBitcoinHeight
  }
}
```

```bash
curl https://api.straitprotocol.com/pop/finality/5757610
# → { "finality": { "hemiBlock": 5757610, "status": "ANCHORED", "keystoneBlock": 5757625, ... } }
```

| `status` | Meaning |
|---|---|
| `ANCHORED` | A valid publication anchors the block. Check `confirmations` / `bitcoinFinal`. |
| `PENDING` | Inside the synced range, but no publication covers it yet. Recent blocks wait for the next batch. |
| `NOT_COVERED` | Before the synced range: Strait can't rule out an earlier publication. |

---

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `POP_ESPLORA_URL` | unset (disabled) | Esplora API root, e.g. `https://mempool.space/api` or `https://blockstream.info/api` |
| `POP_MINER_ADDRESSES` | required when enabled | Comma-separated PoP miner wallets. Mainnet: `17Fv7httdZvqMubbvMkQ9KLPiTSPobxAqF` |
| `POP_SCAN_START_HEIGHT` | required when enabled | Earliest Bitcoin block to read publications from. Pick one shortly before your oldest transfer. Mainnet: `951921` (2026-06-01). Changing it re-reads history from the new start. |
| `POP_MIN_CONFIRMATIONS` | `6` | Confirmations before transfers are marked Bitcoin-final |
| `POP_SCAN_DELAY_MS` | `1000` | Pause between history pages while backfilling |

For testnet, point `POP_ESPLORA_URL` at the Bitcoin network Hemi Sepolia publishes to, and use that network's miner wallets and start height.

### Cost
The first sync pages through each miner's history back to the start height: about 25 transactions per request, so a few hundred small requests (a few MB) and a few minutes. After that, each sync is typically one request per miner per minute. Nothing is downloaded per Bitcoin block.
