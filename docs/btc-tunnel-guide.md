# Bitcoin Tunnel Developer Guide

Deep dive into the BTC ↔ Hemi tunnel: vault discovery, OP_RETURN encoding, and the deposit/withdrawal lifecycle, with code examples.

> Source of truth: [`hemilabs/bitcoin-tunnel-contracts`](https://github.com/hemilabs/bitcoin-tunnel-contracts)

---

## Contracts

| Contract | Role |
|---|---|
| `BitcoinTunnelManager` | Central hub. All indexable events. Manages vault registry, mints/burns hBTC. |
| `SimpleBitcoinVault` | Per-operator vault. Holds collateral, owns a Bitcoin custody address. |
| `BTCToken` | hBTC ERC-20 (8 decimals). Minted/burned only by `BitcoinTunnelManager`. |

**Addresses:**

```
BitcoinTunnelManager (mainnet):  0xEAcA824F46c000fB89403846Bb57e6b913321081
BitcoinTunnelManager (testnet):  0x8221CFD3Eca3c5F9FA27b2AE774151642f1C449e
```

---

## Discovering vaults

There is no single deposit address. Each vault has its own Bitcoin custody address. To track all deposits, you must first discover all vaults.

### Option 1 — Read the vault registry

```solidity
interface IBitcoinTunnelManager {
    function vaultCounter() external view returns (uint32);
    function vaults(uint32 index) external view returns (address);
}
```

```javascript
// ethers.js — enumerate all vaults
const manager = new ethers.Contract(MANAGER_ADDRESS, MANAGER_ABI, provider);
const count = await manager.vaultCounter();

const vaults = [];
for (let i = 0; i < count; i++) {
    vaults.push(await manager.vaults(i));
}
```

### Option 2 — Watch VaultCreated events

```solidity
event VaultCreated(
    address indexed setupAdmin,
    address indexed operatorAdmin,
    address indexed vaultAddress
);
```

```javascript
// Backfill historical vaults + subscribe to new ones
const filter = manager.filters.VaultCreated();
const events = await manager.queryFilter(filter, START_BLOCK, "latest");
const vaultAddresses = events.map(e => e.args.vaultAddress);

manager.on("VaultCreated", (setupAdmin, operatorAdmin, vaultAddress) => {
    console.log("New vault:", vaultAddress);
    // add vaultAddress to your watch set
});
```

---

## OP_RETURN encoding

A BTC deposit transaction must include an OP_RETURN output encoding the recipient's Hemi EVM address. The vault contract parses this from `output.script` (the full script including the OP_RETURN opcode), **not** from a decoded data field.

### Two supported formats

Confirmed from `SimpleBitcoinVaultUTXOLogicHelper.sol`:

| Format | Script length | Layout |
|---|---|---|
| Raw bytes | 22 bytes | `0x6a` `0x14` + 20 raw address bytes |
| ASCII hex | 42 bytes | `0x6a` `0x28` + 40 ASCII hex characters |

- `0x6a` = `OP_RETURN`
- `0x14` = `OP_PUSHBYTES_20` (push 20 bytes)
- `0x28` = `OP_PUSHBYTES_40` (push 40 bytes)

The OP_RETURN output must be within the **first 8 outputs** of the transaction.

### Building the OP_RETURN (deposit side)

```javascript
// Format 1 — raw 20-byte address (most compact)
function buildOpReturnRaw(hemiAddress) {
    const addr = hemiAddress.replace(/^0x/, "");        // 40 hex chars
    const addrBytes = Buffer.from(addr, "hex");          // 20 bytes
    return Buffer.concat([
        Buffer.from([0x6a, 0x14]),                       // OP_RETURN OP_PUSHBYTES_20
        addrBytes,
    ]);
}

// Format 2 — ASCII hex (human-readable in block explorers)
function buildOpReturnAscii(hemiAddress) {
    const addr = hemiAddress.replace(/^0x/, "");         // "abc123..." 40 chars
    const asciiBytes = Buffer.from(addr, "ascii");        // 40 bytes
    return Buffer.concat([
        Buffer.from([0x6a, 0x28]),                       // OP_RETURN OP_PUSHBYTES_40
        asciiBytes,
    ]);
}
```

### Parsing the OP_RETURN (indexer side)

```rust
/// Parse the Hemi EVM destination from an OP_RETURN output script.
/// Pass output.script (full script with 0x6a prefix), not opReturnData.
pub fn parse_hemi_destination(script: &[u8]) -> Option<[u8; 20]> {
    if script.first() != Some(&0x6a) {
        return None; // not an OP_RETURN
    }
    match script.len() {
        // Format 1: 0x6a 0x14 <20 raw bytes>
        22 => {
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&script[2..22]);
            Some(addr)
        }
        // Format 2: 0x6a 0x28 <40 ASCII hex bytes>
        42 => {
            let hex_str = std::str::from_utf8(&script[2..42]).ok()?;
            let decoded = hex::decode(hex_str).ok()?;
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&decoded);
            Some(addr)
        }
        _ => None,
    }
}
```

---

## Deposit lifecycle (BTC → Hemi)

```
┌─────────────────────────────────────────────────────────────┐
│ 1. User sends BTC to vault custody address                  │
│    + OP_RETURN with their Hemi address (first 8 outputs)    │
└─────────────────────────────────────────────────────────────┘
                          │  ~1 hour (6 confirmations)
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. confirmDeposit(vaultIndex, txid, outputIndex, extraInfo) │
│    called by anyone on BitcoinTunnelManager                 │
└─────────────────────────────────────────────────────────────┘
                          │  BitcoinKit verifies on-chain
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. DepositConfirmed event + hBTC minted to recipient        │
│    depositTxId = the Bitcoin txid (cross-chain join key)    │
└─────────────────────────────────────────────────────────────┘
```

### Indexing DepositConfirmed

```solidity
event DepositConfirmed(
    address indexed vault,
    address indexed recipient,
    bytes32 indexed depositTxId,   // ← Bitcoin txid, primary join key
    uint256 depositSats,           // gross amount deposited
    uint256 netSatsAfterFee        // hBTC actually minted
);
```

```javascript
manager.on("DepositConfirmed", (vault, recipient, depositTxId, depositSats, netSats) => {
    // depositTxId lets you correlate with the Bitcoin deposit transaction
    console.log(`Deposit: ${netSats} sats → ${recipient}, btc tx ${depositTxId}`);
});
```

---

## Withdrawal lifecycle (Hemi → BTC)

```
┌─────────────────────────────────────────────────────────────┐
│ 1. initiateWithdrawal(vaultIndex, btcAddress, amount)       │
│    → hBTC burned immediately                                 │
│    → WithdrawalInitiated event (uuid assigned)              │
└─────────────────────────────────────────────────────────────┘
                          │  operator processes
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Operator sends BTC to user's address                     │
│    + OP_RETURN encoding the uuid                             │
└─────────────────────────────────────────────────────────────┘
                          │  if operator fails to pay
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. challengeWithdrawal(uuid, extraInfo)                     │
│    → hBTC re-minted to original withdrawer                  │
└─────────────────────────────────────────────────────────────┘
```

### The uuid join key

```solidity
event WithdrawalInitiated(
    address indexed vault,
    address indexed withdrawer,
    string  indexed btcAddress,    // ⚠ HASHED — original string not recoverable
    uint256 withdrawalSats,
    uint256 netSatsAfterFee,
    uint64  uuid                   // ← cross-chain join key (non-indexed)
);
```

The `uuid` encodes the vault index and a vault-specific counter:

```javascript
// Decompose a uuid
function decomposeUuid(uuid) {
    const vaultIndex = Number(BigInt(uuid) >> 32n);
    const vaultUuid  = Number(BigInt(uuid) & 0xFFFFFFFFn);
    return { vaultIndex, vaultUuid };
}

// Example: uuid = 8589934594
// vaultIndex = 2, vaultUuid = 2
```

The operator encodes this `uuid` in the OP_RETURN of the Bitcoin payout transaction, which is how you correlate the on-Hemi withdrawal with the on-Bitcoin payout.

> **Why is btcAddress unrecoverable?** Solidity hashes `indexed` dynamic types (strings, bytes) with keccak256 before storing them in the log topic. Only the hash is on-chain. To get the destination Bitcoin address, you must track the operator's payout transaction (matched via the uuid OP_RETURN).

---

## Fees

Each vault sets its own deposit and withdrawal fees within protocol-enforced bounds. The fee is the difference between gross (`depositSats` / `withdrawalSats`) and net (`netSatsAfterFee`). Operators claim collected fees as hBTC via `mintVaultFees()`.
