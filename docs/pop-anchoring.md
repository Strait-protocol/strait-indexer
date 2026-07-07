# PoP Anchoring & Bitcoin Finality

Hemi anchors its blocks to Bitcoin through Proof-of-Publication (PoP). This is what gives Hemi "Bitcoin-grade" finality. This guide explains how it works and how to verify that a specific Hemi transaction is Bitcoin-final.

| Network | `PoPPayoutsV2` Address | Notes |
|---|---|---|
| Hemi Sepolia | `0x4a3b61C586DB4CD219E85aC0697b66916c7457AB` | Testnet |
| Hemi Mainnet (canonical) | `0x9a23ab7cb11cfb96e577da52a6ad5211ff24434b` | Used by Strait (`HEMI_POP_PAYOUTS_CONTRACT`) |
| Hemi Mainnet (first deployment) | `0x9417dd2eba413cfc11e8d8e368c007bfa1385a40` | Earlier deployment, same owner |

> Source: [`hemilabs/pop-payouts`](https://github.com/hemilabs/pop-payouts)

---

## Mainnet deployment status (as of June 2026)

Two `PoPPayoutsV2` contracts were deployed on Hemi Mainnet by the Hemi team (owner `0xE067Dd6965bd87C81AbE658ed42FC02eB41d5Bd3`):

- **First deployment** — `0x9417dd2eba413cfc11e8d8e368c007bfa1385a40`, block 3,497,671, factory `0xf9705145175800f6f2e4a81261a4cb5406da6023`
- **Canonical deployment** — `0x9a23ab7cb11cfb96e577da52a6ad5211ff24434b`, block 3,497,724, factory `0x92f03ea43ee029dbd28b63029d6f07e1efdb7a1a`

**`mintPoPRewards()` has never been called on either contract** — `lastBlockRewarded = 0` on both as of June 30, 2026. PoP payouts are not yet activated on mainnet.

Practical impact: BTC→Hemi deposits already reach `FINALIZED` status at Hemi mint — the user has their hBTC regardless of PoP activation. The `popAnchored` field will remain `false` on all deposits until the Hemi team activates the system by calling `mintPoPRewards()` for the first time. Strait is wired to watch `0x9a23ab7cb11cfb96e577da52a6ad5211ff24434b` via `HEMI_POP_PAYOUTS_CONTRACT` and will begin updating `popAnchored` automatically once `PayoutRoundExecuted` events start firing.

**Open questions for the Hemi team:**
- Which deployment is considered canonical for integrators?
- When will PoP payout activation happen?
- Who calls `mintPoPRewards()` — the sequencer, a multisig, or an automated keeper?

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
    // All BTC→Hemi deposits with mint block in (blockRewarded-25, blockRewarded]
    // are now Bitcoin-anchored. Status stays FINALIZED — only popAnchored flips.
    for (const transfer of pendingTransfers) {
        if (keystoneCovers(Number(blockRewarded), transfer.hemiMintBlock)) {
            transfer.popAnchored = true;
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

## Finality model

Strait separates **transfer completion** from **Bitcoin-grade anchoring**:

```
BTC deposit observed        → INITIATED     (~immediate)
Hemi mint confirmed         → FINALIZED     (~minutes, Hemi consensus)
PoP keystone fires          → popAnchored=true  (~90 min, Bitcoin-anchored)
```

`FINALIZED` means the recipient has their hBTC — the transfer is complete. `popAnchored=true` is an additional signal that the Hemi mint block is anchored to Bitcoin and safe to settle against with Bitcoin-grade finality.

For most consumer use cases (wallets, bridge UIs), `FINALIZED` is sufficient. For high-value settlement (lending collateral, treasury, compliance), check `popAnchored=true` as well.

```javascript
// Wait for funds available
if (transfer.status === "FINALIZED") {
    showFundsAvailable(transfer);
}

// Also wait for Bitcoin-grade finality
if (transfer.status === "FINALIZED" && transfer.popAnchored) {
    releaseLoanCollateral(transfer);
}
```

The `popKeystoneBlock` and `popScore` fields carry the specific keystone and PoP quality score when anchored.

---

## What you cannot get from events

The specific Bitcoin transaction that carried a PoP publication arrives as **calldata** to `mintPoPRewards()`, not as an indexed event. If you need the exact Bitcoin txid of the PoP publication, you must decode the transaction calldata rather than relying on event logs. For most consumers, the keystone-level "anchored: true/false" signal is sufficient.
