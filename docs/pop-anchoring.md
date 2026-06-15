# PoP Anchoring & Bitcoin Finality

Hemi anchors its blocks to Bitcoin through Proof-of-Publication (PoP). This is what gives Hemi "Bitcoin-grade" finality. This guide explains how it works and how to verify that a specific Hemi transaction is Bitcoin-final.

| Network | `PoPPayoutsV2` Address |
|---|---|
| Hemi Sepolia | `0x4a3b61C586DB4CD219E85aC0697b66916c7457AB` |
| Hemi Mainnet | _confirm from explorer_ |

> Source: [`hemilabs/pop-payouts`](https://github.com/hemilabs/pop-payouts)

---

## How PoP anchoring works

1. Every **25 Hemi blocks** (~5 minutes) is a **keystone**.
2. PoP miners publish a commitment to each keystone onto the Bitcoin blockchain.
3. Once published and Bitcoin-confirmed, the Hemi sequencer calls `mintPoPRewards()` on `PoPPayoutsV2` to reward the miners.
4. This emits `PayoutRoundExecuted(blockRewarded, ...)`.
5. When that event fires, **every Hemi block in `(blockRewarded - 25, blockRewarded]` is now anchored to Bitcoin.**

```
Hemi blocks:   ...─────[ keystone window ]─────[ next window ]──...
                       (K-25, K]                (K, K+25]
                          │                         │
PoP publication:    keystone K              keystone K+25
                    to Bitcoin                to Bitcoin
                          │                         │
                  PayoutRoundExecuted(K)    PayoutRoundExecuted(K+25)
```

---

## Key constants

```
KEYSTONE_FREQUENCY     = 25 Hemi blocks   (~5 min per keystone)
BITCOIN_FINALITY_DELAY = 9 BTC blocks     (~90 min anchoring window)
```

A transfer is Bitcoin-final approximately 90 minutes after its Hemi mint — fast Hemi confirmation in seconds, full Bitcoin anchoring in ~90 minutes.

---

## The event

```solidity
event PayoutRoundExecuted(
    uint64  indexed blockRewarded,   // always a multiple of 25
    uint256 rewardPool,              // HEMI paid to miners (atomic units)
    uint256 popScore                 // aggregate PoP quality score
);
```

> **`popScore == 0` does NOT mean unanchored.** If no miners published a keystone, the sequencer still processes the round and emits the event with `popScore = 0`. The block range is still considered anchored for finality purposes — the score only affects miner rewards.

---

## Verifying a transaction is Bitcoin-final

### The window check

A Hemi transaction at block `N` is anchored by the keystone `K` where:

```
K = ceil(N / 25) * 25
```

The transaction is anchored once `PayoutRoundExecuted` has fired with `blockRewarded >= K`.

```javascript
function keystoneFor(hemiBlock) {
    const KEYSTONE_FREQUENCY = 25;
    const rem = hemiBlock % KEYSTONE_FREQUENCY;
    return rem === 0 ? hemiBlock : hemiBlock + (KEYSTONE_FREQUENCY - rem);
}

// A keystone's window is (keystoneBlock - 25, keystoneBlock]
//   exclusive lower bound, inclusive upper bound
function keystoneCovers(keystoneBlock, hemiBlock) {
    const windowStart = Math.max(0, keystoneBlock - 25);
    return hemiBlock > windowStart && hemiBlock <= keystoneBlock;
}
```

### Method 1 — read `lastBlockRewarded` (cheapest)

```solidity
function lastBlockRewarded() external view returns (uint64);
```

Any Hemi block `<= lastBlockRewarded` is anchored.

```javascript
const pop = new ethers.Contract(POP_ADDRESS, POP_ABI, hemiProvider);
const lastRewarded = await pop.lastBlockRewarded();

const myKeystone = keystoneFor(myTxBlock);
const isAnchored = myKeystone <= lastRewarded;
```

```bash
cast call 0x4a3b61C586DB4CD219E85aC0697b66916c7457AB \
  "lastBlockRewarded()(uint64)" \
  --rpc-url https://testnet.rpc.hemi.network/rpc
```

### Method 2 — watch the event (real-time)

```javascript
pop.on("PayoutRoundExecuted", (blockRewarded, rewardPool, popScore) => {
    // All pending transfers with mint block in (blockRewarded-25, blockRewarded]
    // are now Bitcoin-final.
    for (const transfer of pendingTransfers) {
        if (keystoneCovers(Number(blockRewarded), transfer.hemiMintBlock)) {
            transfer.status = "FINALIZED";
            transfer.popKeystoneBlock = Number(blockRewarded);
            transfer.popScore = Number(popScore);
        }
    }
});
```

### Method 3 — query the rounds array (audit trail)

```solidity
function rounds(uint256 index) external view returns (
    uint64  blockHeight,    // the keystone block
    uint256 totalPoPScore,
    uint256 rewardPool
);
```

Each executed round is stored. Binary-search by `blockHeight` to find the round covering your block.

---

## Boundary conditions (important)

The window is **exclusive on the lower bound, inclusive on the upper bound**: `(K-25, K]`.

For keystone `K = 100`, window is `(75, 100]`:

| Mint block | Covered? | Reason |
|---|---|---|
| 75 | No | exactly at window start — excluded |
| 76 | Yes | first block in window |
| 100 | Yes | exactly on keystone — included |
| 101 | No | belongs to next keystone (125) |

This boundary is what guarantees every block is covered by **exactly one** keystone — no gaps, no double-counting.

```javascript
// Verification: blocks 1..50 are each covered by exactly one of keystone 25 or 50
for (let block = 1; block <= 50; block++) {
    const in25 = keystoneCovers(25, block);
    const in50 = keystoneCovers(50, block);
    console.assert(in25 !== in50, `block ${block} must be in exactly one keystone`);
}
```

---

## Two-stage finality model

This is the data no generic EVM indexer can produce for Hemi:

```
BTC deposit observed        → INITIATED   (~immediate)
Hemi mint confirmed         → ANCHORED    (~minutes, Hemi consensus)
PoP keystone fires          → FINALIZED   (~90 min, Bitcoin-anchored)
```

`FINALIZED` means the transaction is anchored to Bitcoin and safe to settle against. For a lending protocol, a treasury, or a compliance tool, the difference between "confirmed on Hemi" and "anchored to Bitcoin" is the difference between probabilistic and Bitcoin-grade finality.

---

## What you cannot get from events

The specific Bitcoin transaction that carried a PoP publication arrives as **calldata** to `mintPoPRewards()`, not as an indexed event. If you need the exact Bitcoin txid of the PoP publication, you must decode the transaction calldata rather than relying on event logs. For most consumers, the keystone-level "anchored: true/false" signal is sufficient.
