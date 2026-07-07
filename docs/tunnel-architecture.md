# Hemi Tunnel Architecture

A complete tunnel transfer touches three blockchains with different finality semantics. This document explains how the pieces fit together.

---

## Overview

Hemi is an EVM-compatible chain that embeds a full Bitcoin node inside its execution environment (via hVM). This enables trust-minimized bridging between Bitcoin, Hemi, and Ethereum.

Two distinct tunnel mechanisms exist:

| Mechanism | Routes | Contract |
|---|---|---|
| **Bitcoin tunnel** | BTC ↔ Hemi | `BitcoinTunnelManager` + `SimpleBitcoinVault` |
| **ETH/ERC-20 tunnel** | ETH/ERC-20 ↔ Hemi | OP Stack `L2StandardBridge` |

---

## Bitcoin tunnel

### Architecture

```
Bitcoin blockchain
  └── Vault custody address (watched by SimpleBitcoinVault operator)
         │
         │  User sends BTC + OP_RETURN with their Hemi address
         ▼
Hemi blockchain
  └── BitcoinTunnelManager (0xEAcA...)
         ├── confirmDeposit(vaultIndex, txid, outputIndex, extraInfo)
         │     → verifies via BitcoinKit precompile
         │     → emits DepositConfirmed(vault, recipient, depositTxId, ...)
         │     → mints hBTC to recipient
         └── SimpleBitcoinVault (per operator)
               └── holds collateral, manages custody address
```

### Key concepts

**Vaults**: There is no single tunnel address. Each `SimpleBitcoinVault` instance has its own Bitcoin custody address. Operators create vaults by calling `BitcoinTunnelManager.createVault()`. A `VaultCreated` event is emitted for each one.

**hBTC**: The `BTCToken` ERC-20 contract (8 decimals, satoshi precision) is deployed by `BitcoinTunnelManager`. Only `BitcoinTunnelManager` can mint or burn it.

**Collateralization**: Vaults are over-collateralized with an ERC-20 token. If collateral falls below the liquidation threshold, the vault is liquidated.

### Deposit flow (BTC → Hemi)

```
1. User selects a vault and sends BTC to its custody address on Bitcoin.
   The transaction must include an OP_RETURN encoding their Hemi EVM address.

2. After 6 Bitcoin confirmations (~1 hour), anyone calls:
   BitcoinTunnelManager.confirmDeposit(vaultIndex, txid, outputIndex, extraInfo)

3. The contract calls BitcoinKit precompile to verify:
   - The txid exists and is confirmed on Bitcoin
   - The output at outputIndex pays to the vault custody script
   - The OP_RETURN output encodes a valid EVM address

4. On success: DepositConfirmed event emitted, hBTC minted to recipient.
```

### Withdrawal flow (Hemi → BTC)

```
1. User calls:
   BitcoinTunnelManager.initiateWithdrawal(vaultIndex, btcAddress, amount)
   → hBTC is burned immediately
   → WithdrawalInitiated event emitted (uuid = vaultIndex << 32 | vaultSpecificUUID)

2. Vault operator sends BTC to the user's Bitcoin address.
   Payout tx includes an OP_RETURN encoding the 4-byte vault-specific uuid.

3. Operator calls SimpleBitcoinVault.finalizeWithdrawal(txid, withdrawalIndex)
   → Records the Bitcoin txid in vault state (currentSweepUTXO).
   → No event is emitted — state change only.

4. If operator does not pay within the deadline, user calls:
   BitcoinTunnelManager.challengeWithdrawal(uuid, extraInfo)
   → On success: hBTC re-minted to original withdrawer.
```

**Indexer note:** Because `finalizeWithdrawal` emits no events, Strait detects payouts via two polling phases in `BtcPayoutWatcher`:
- **Phase 2**: BitcoinKit UTXO scan on recipient address (works while UTXO is unspent)
- **Phase 3**: `currentSweepUTXO()` call on each vault contract (works regardless of UTXO state)

See [`btc-tunnel-guide.md`](btc-tunnel-guide.md) for the full finalization flow.

### Events to index

```solidity
// BitcoinTunnelManager
event VaultCreated(address indexed setupAdmin, address indexed operatorAdmin, address indexed vaultAddress);
event DepositConfirmed(address indexed vault, address indexed recipient, bytes32 indexed depositTxId, uint256 depositSats, uint256 netSatsAfterFee);
event WithdrawalInitiated(address indexed vault, address indexed withdrawer, string indexed btcAddress, uint256 withdrawalSats, uint256 netSatsAfterFee, uint64 uuid);
event WithdrawalChallengeSuccess(address indexed vault, address indexed withdrawer, uint64 indexed uuid);
```

> **Note**: `btcAddress` in `WithdrawalInitiated` is `indexed` — it is stored as its keccak256 hash in the topic and the original string is **not recoverable** from the log. Use the `uuid` as the cross-chain join key for Hemi→BTC withdrawals.

---

## ETH/ERC-20 tunnel

### Architecture

```
Ethereum blockchain
  └── L1StandardBridgeProxy (0x5eaa...)
         │  ETHBridgeInitiated / ERC20BridgeInitiated
         ▼
Hemi blockchain
  └── L2StandardBridge (0x4200...0010)
         │  ETHBridgeFinalized / ERC20BridgeFinalized
         ▼
  hToken minted to recipient on Hemi
```

This is a standard OP Stack bridge. Hemi inherits the full OP Stack bridge mechanics.

### Deposit flow (ETH → Hemi)

```
1. User calls L1StandardBridgeProxy.depositETH() on Ethereum.
   → ETHBridgeInitiated event emitted on Ethereum.

2. OP Stack relayer picks up the message.
   → ETHBridgeFinalized event emitted on Hemi (~2 minutes).
   → ETH credited to recipient on Hemi.
```

### Withdrawal flow (Hemi → ETH)

Two steps, both requiring someone to actively submit a transaction — the challenge window
does not start until step 2 happens, so an un-proven withdrawal can sit indefinitely.

```
1. User calls L2StandardBridge.withdraw() on Hemi.
   → ETHBridgeInitiated event emitted on Hemi. hBTC/ETH burned; nothing has
     happened on Ethereum yet.

2. Anyone calls OptimismPortal.proveWithdrawalTransaction(tx, l2OutputIndex,
   outputRootProof, withdrawalProof) on Ethereum, once the L2 output root
   covering this withdrawal's block has been posted (~1 hour after step 1).
   → WithdrawalProven event emitted on Ethereum.
   → The ~1 day challenge window starts here, not at step 1.

3. ~1 day challenge period elapses (Hemi's fault-proof window — shortened
   from the standard OP Stack 7 days by anchoring L2 output-root finality
   to Bitcoin via PoP; see "PoP anchoring" below).

4. Anyone calls OptimismPortal.finalizeWithdrawalTransaction(tx).
   → WithdrawalFinalized event emitted on OptimismPortal.
   → ETHBridgeFinalized event emitted on L1StandardBridgeProxy; funds released.
```

**Indexer note:** Strait watches `WithdrawalProven` on `OptimismPortal` (when
`ETH_OPT_PORTAL_CONTRACT` is configured) to advance a withdrawal from `INITIATED` to
`PROVING`, and `ETHBridgeFinalized` on L1 to advance `PROVING` → `FINALIZED`. Strait does
not submit the proof itself — today this is a manual step for the withdrawing user (or
a relayer acting on their behalf).

### Events to index

```solidity
// L2StandardBridge on Hemi (and L1StandardBridgeProxy on Ethereum)
event ETHBridgeFinalized(address indexed from, address indexed to, uint256 amount, bytes extraData);
event ETHBridgeInitiated(address indexed from, address indexed to, uint256 amount, bytes extraData);
event ERC20BridgeFinalized(address indexed localToken, address indexed remoteToken, address indexed from, address to, uint256 amount, bytes extraData);
event ERC20BridgeInitiated(address indexed localToken, address indexed remoteToken, address indexed from, address to, uint256 amount, bytes extraData);

// OptimismPortal on Ethereum — the two-step withdrawal proof/finalize gate
event WithdrawalProven(bytes32 indexed withdrawalHash, address indexed from, address indexed to);
event WithdrawalFinalized(bytes32 indexed withdrawalHash, bool success);
```

---

## PoP anchoring

Hemi's finality is anchored to Bitcoin via Proof-of-Publication (PoP). Every 25 Hemi blocks (~5 minutes) is a **keystone**. PoP miners publish keystone commitments to Bitcoin. When enough miners have published, the `PoPPayoutsV2` contract emits:

```solidity
event PayoutRoundExecuted(uint64 indexed blockRewarded, uint256 rewardPool, uint256 popScore);
```

Any Hemi block in `(blockRewarded - 25, blockRewarded]` is now Bitcoin-anchored.

See [pop-anchoring.md](pop-anchoring.md) for the full guide.

---

## Cross-chain join keys

The hardest part of indexing the Hemi tunnels is correlating events across three chains. There is no single shared identifier in the raw logs. The join keys are:

| Route | Join key | Source |
|---|---|---|
| BTC → Hemi | `depositTxId` (Bitcoin txid) | Indexed topic in `DepositConfirmed` |
| Hemi → BTC | `uuid` (vaultIndex << 32 \| vaultUUID) | Non-indexed field in `WithdrawalInitiated` |
| ETH → Hemi | sender + amount + timestamp window | No shared id — match heuristically |
| Hemi → ETH | sender + amount + timestamp window | No shared id — match heuristically |

For BTC routes, `depositTxId` in `DepositConfirmed` is the Bitcoin transaction ID — this is the primary join key and allows exact matching between the on-Bitcoin deposit and the on-Hemi mint.
