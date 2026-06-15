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
   Payout tx must include an OP_RETURN encoding the uuid.

3. If operator does not pay within the deadline, user calls:
   BitcoinTunnelManager.challengeWithdrawal(uuid, extraInfo)
   → On success: hBTC re-minted to original withdrawer.
```

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

```
1. User calls L2StandardBridge.withdraw() on Hemi.
   → ETHBridgeInitiated event emitted on Hemi.

2. ~7 day challenge period (OP Stack fault proof window).
   → ETHBridgeFinalized event emitted on Ethereum.
```

### Events to index

```solidity
// L2StandardBridge on Hemi (and L1StandardBridgeProxy on Ethereum)
event ETHBridgeFinalized(address indexed from, address indexed to, uint256 amount, bytes extraData);
event ETHBridgeInitiated(address indexed from, address indexed to, uint256 amount, bytes extraData);
event ERC20BridgeFinalized(address indexed localToken, address indexed remoteToken, address indexed from, address to, uint256 amount, bytes extraData);
event ERC20BridgeInitiated(address indexed localToken, address indexed remoteToken, address indexed from, address to, uint256 amount, bytes extraData);
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
