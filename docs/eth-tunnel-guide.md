# ETH / ERC-20 Tunnel Developer Guide

The ETH and ERC-20 tunnel uses the standard OP Stack bridge. If you've worked with Optimism, Base, or any OP Stack chain, this will be familiar.

---

## Contracts

| Contract | Network | Address |
|---|---|---|
| `L2StandardBridge` | Hemi Mainnet + Sepolia | `0x4200000000000000000000000000000000000010` |
| `L1StandardBridgeProxy` | Ethereum Mainnet | `0x5eaa10F99e7e6D177eF9F74E519E319aa49f191e` |
| `L1StandardBridgeProxy` | Ethereum Sepolia | `0xc94b1BEe63A3e101FE5F71C80F912b4F4b055925` |

`L2StandardBridge` lives at the well-known OP Stack predeploy address `0x4200...0010`. The proxy resolves to implementation `0xC0d3c0d3c0D3c0d3C0D3c0D3C0d3C0D3C0D30010`.

---

## Events

```solidity
interface IStandardBridge {
    // ETH deposits: Ethereum → Hemi
    event ETHBridgeFinalized(address indexed from, address indexed to, uint256 amount, bytes extraData);
    // ETH withdrawals initiated: Hemi → Ethereum
    event ETHBridgeInitiated(address indexed from, address indexed to, uint256 amount, bytes extraData);

    // ERC-20 deposits: Ethereum → Hemi
    event ERC20BridgeFinalized(address indexed localToken, address indexed remoteToken, address indexed from, address to, uint256 amount, bytes extraData);
    // ERC-20 withdrawals initiated: Hemi → Ethereum
    event ERC20BridgeInitiated(address indexed localToken, address indexed remoteToken, address indexed from, address to, uint256 amount, bytes extraData);

    // Legacy OP Stack events (still emitted by some versions)
    event DepositFinalized(address indexed l1Token, address indexed l2Token, address indexed from, address to, uint256 amount, bytes extraData);
    event WithdrawalInitiated(address indexed l1Token, address indexed l2Token, address indexed from, address to, uint256 amount, bytes extraData);
}

interface IOptimismPortal {
    // Emitted when proveWithdrawalTransaction is called — starts the ~1 day challenge window.
    event WithdrawalProven(bytes32 indexed withdrawalHash, address indexed from, address indexed to);
    // Emitted when finalizeWithdrawalTransaction is called, after the window elapses.
    event WithdrawalFinalized(bytes32 indexed withdrawalHash, bool success);
}
```

---

## Deposit flow (ETH → Hemi)

```
Ethereum                                  Hemi
────────                                  ────
depositETH()                       ~2min
  └─ ETHBridgeInitiated  ──────────────►  ETHBridgeFinalized
                                            └─ ETH credited to recipient
```

```javascript
// Deposit ETH from Ethereum to Hemi
const l1Bridge = new ethers.Contract(L1_BRIDGE_ADDRESS, BRIDGE_ABI, signer);

const tx = await l1Bridge.depositETH(
    200000,      // L2 gas limit
    "0x",        // extra data
    { value: ethers.parseEther("0.1") }
);
await tx.wait();
// Watch for ETHBridgeFinalized on Hemi to confirm completion (~2 min)
```

### Indexing both sides

```javascript
// On Ethereum — deposit initiated
l1Bridge.on("ETHBridgeInitiated", (from, to, amount, extraData, event) => {
    recordDepositInitiated({ from, to, amount, txHash: event.log.transactionHash });
});

// On Hemi — deposit finalized
const l2Bridge = new ethers.Contract(L2_BRIDGE_ADDRESS, BRIDGE_ABI, hemiProvider);
l2Bridge.on("ETHBridgeFinalized", (from, to, amount, extraData, event) => {
    recordDepositFinalized({ from, to, amount, txHash: event.log.transactionHash });
});
```

---

## Withdrawal flow (Hemi → ETH)

Unlike standard OP Stack chains, this is a **two-step, actively-triggered** process — the
challenge window does not start ticking until someone calls `proveWithdrawalTransaction`.
A withdrawal can sit indefinitely at "burned, not yet proven" if nobody submits the proof.

```
Hemi                          Ethereum (OptimismPortal)             Ethereum (L1StandardBridge)
────                          ─────────────────────────             ───────────────────────────
withdraw()
  └─ ETHBridgeInitiated
        │
        │  (anyone can call, once the L2 output
        │   root covering this block is posted)
        ▼
                          proveWithdrawalTransaction(tx, l2OutputIndex,
                                                      outputRootProof, withdrawalProof)
                            └─ WithdrawalProven
                                  │
                                  │  ~1 day challenge window (Hemi shortens the
                                  │  standard OP Stack 7-day window by anchoring
                                  │  finality to Bitcoin via PoP)
                                  ▼
                          finalizeWithdrawalTransaction(tx)
                            └─ WithdrawalFinalized ─────────────►  ETHBridgeFinalized
                                                                      └─ ETH released
```

1. **`withdraw()` on Hemi** — burns the ETH/ERC-20 and emits `ETHBridgeInitiated`. This is
   just the L2 side; nothing on Ethereum has happened yet.
2. **`proveWithdrawalTransaction` on `OptimismPortal`** — anyone (typically the withdrawing
   user, or a relayer on their behalf) submits a Merkle proof that the withdrawal is
   included in `L2ToL1MessagePasser`'s state, checked against the L2 output root posted to
   `OptimismPortal`. This can only be submitted once that output root exists on L1
   (usually within about an hour of the L2 block), but it is **not automatic** — someone
   has to send this transaction. Emits `WithdrawalProven`, which starts the challenge clock.
3. **~1 day challenge window** — Hemi's fault-proof window, shortened from the standard OP
   Stack 7 days because Hemi anchors L2 output-root finality to Bitcoin via PoP rather than
   relying solely on an optimistic dispute period. This part genuinely cannot be skipped —
   it is a security property of the chain, not a relayer/liveness delay.
4. **`finalizeWithdrawalTransaction` on `OptimismPortal`** — callable by anyone once the
   window elapses. Releases the funds and emits `WithdrawalFinalized` on `OptimismPortal`
   and `ETHBridgeFinalized` on the L1 `L1StandardBridgeProxy`.

> **If a withdrawal looks "stuck" for days**, check whether it has even been proven yet
> (step 2) before assuming the challenge window (step 3) is the bottleneck — an unproven
> withdrawal waits indefinitely, not just ~1 day.

---

## Matching deposits to mints

Unlike the BTC tunnel (which has the Bitcoin txid as a join key), the OP Stack bridge events do **not** carry a shared cross-chain identifier in a convenient indexed field. To correlate the Ethereum-side `ETHBridgeInitiated` with the Hemi-side `ETHBridgeFinalized`, match on:

- `from` address (same on both sides)
- `to` address
- `amount` (exact match — no fee deduction for the standard bridge)
- timestamp window (deposits finalize in ~2 minutes)

```rust
// Pseudocode for the matching heuristic
fn match_eth_deposit(initiated: &EthBridgeInitiated, finalized: &EthBridgeFinalized) -> bool {
    initiated.from == finalized.from
        && initiated.to == finalized.to
        && initiated.amount == finalized.amount
        && (finalized.timestamp - initiated.timestamp) < Duration::minutes(30)
}
```

For ERC-20, also match `localToken` / `remoteToken` to disambiguate transfers of different tokens with the same amount.

---

## ERC-20 token mapping

When indexing ERC-20 transfers, you need the L1↔L2 token mapping. The `ERC20Bridge*` events carry both:

- `localToken` — the token address on the chain emitting the event
- `remoteToken` — the corresponding token address on the other chain

For the canonical list of bridged tokens on Hemi, see [`hemilabs/token-list`](https://github.com/hemilabs/token-list/blob/master/src/hemi.tokenlist.json).
