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

## Known mainnet vault custody addresses (June 2026)

`BitcoinTunnelManager` (mainnet): `0xEAcA824F46c000fB89403846Bb57e6b913321081`

9 vaults exist as of June 2026. The 7 unique Bitcoin custody addresses below are configured in `BITCOIN_TUNNEL_ADDRESSES` in `.env`.

| Vault Index | Hemi Contract Address | Bitcoin Custody Address |
|---|---|---|
| 0 | `0x3DA10b74bD339E69c1dE9408020cE640B012E8CC` | `18AVmm853HVhibPHMc3JRLXMynzKAbj6Po` |
| 1 | `0xeCF9C248FC63857e217214dAa82C1083cE8645D9` | `1CY4RxCxmzDC1W1iL9edAtJF2CTGeaJMbC` |
| 2 | `0x13ca60FeFBe278F34bbAC50cAa121802474FCa43` | `12LcfeGZYzbiUqcLq1UvmMdtKFNa4niLEZ` |
| 3 | `0xaabd93f4324eaB9e2Df736FF17eA22C9Eb239B10` | _(none — vault not yet configured)_ |
| 4 | `0x96aA8D0DEEE02bD3F283e6896F57e2206A42A581` | `16NuSCxDVCAXbKs9GRbjbHXbwGXu3tnPSo` |
| 5 | `0x654cE308839484a8a199354FAaED286E7B0C3a02` | `16NuSCxDVCAXbKs9GRbjbHXbwGXu3tnPSo` (same as vault 4) |
| 6 | `0x3A29d25c255D3C5Be67fAA105936c21a0251FA2a` | `1GawhMSUVu3bgRiNmejbVTBjpwBygGWSqf` |
| 7 | `0x5E6AbAD42E63cd7E8CE156fB8a8F0a3aEE464E33` | `bc1q4lpa9d5zxehge7vx86784gcxy23hc3xwp3gl422venswe6pvhh5qpn9xfj` |
| 8 | `0x58f7B8D7A7291AaECE0FEbb39aA4E877387e61E4` | `1QDhzsteETKuw1M5kWHEjzaAmHSGhpH8zr` |

**Notes:**

- **Vault 3** has no Bitcoin custody address yet — it has not been configured by the operator.
- **Vaults 4 and 5** share the same Bitcoin address (`16NuSCxDVCAXbKs9GRbjbHXbwGXu3tnPSo`). Deposits to that address are disambiguated by the `DepositConfirmed` event's `vault` field, not the Bitcoin address alone.
- The `CustodyWatcher` uses the **BitcoinKit precompile on Hemi** to poll these addresses — no native Bitcoin node is required. `BITCOIN_RPC_URL` in `.env` is vestigial and only needed if you want direct Bitcoin RPC access for other purposes.
- Without `BITCOIN_TUNNEL_ADDRESSES` set, BTC→Hemi deposits are still captured via Hemi's `DepositConfirmed` event but only appear after ~6 BTC confirmations (~1 hour). With addresses set, deposits appear as soon as the UTXO hits a custody address.
- **Warning:** new vaults may be added over time. Monitor `VaultCreated` events or re-query `vaultCounter()` periodically and update `BITCOIN_TUNNEL_ADDRESSES` to keep the watch-set current.

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
│    → Strait marks transfer FINALIZED                        │
└─────────────────────────────────────────────────────────────┘
                          │  ~90 min (optional, async)
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. PoP keystone anchors the mint block to Bitcoin           │
│    → Strait sets popAnchored=true (status stays FINALIZED)  │
└─────────────────────────────────────────────────────────────┘
```

**Finality model:** A deposit reaches `FINALIZED` as soon as the hBTC mint is confirmed on Hemi — the user has their funds. Bitcoin-grade finality (`popAnchored=true`) arrives asynchronously (~90 minutes) once a PoP keystone covers the mint block. The two are tracked separately so callers can gate on whichever guarantee they need.

**Failure states:** A `BTC_TO_HEMI` deposit can reach `REORGED` if the Hemi mint transaction is rolled back by a Hemi chain reorganization before the deposit is indexed as FINALIZED. The Bitcoin-side transaction is unaffected — the user's BTC is still locked in the custody address and a new `DepositConfirmed` event will be emitted when the transaction is re-included. If you display transfer status to users, treat `REORGED` as retriable, not permanent.

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
│    + OP_RETURN encoding the vault-specific uuid (4 bytes)   │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Operator calls finalizeWithdrawal(txid, withdrawalIndex) │
│    on SimpleBitcoinVault — records the Bitcoin txid         │
│    No event is emitted. State stored silently in contract.  │
└─────────────────────────────────────────────────────────────┘
                          │  if operator fails to pay within deadline
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. challengeWithdrawal(uuid, extraInfo)                     │
│    → WithdrawalChallengeSuccess emitted                     │
│    → hBTC re-minted to original withdrawer                  │
│    → Strait marks transfer FAILED                           │
└─────────────────────────────────────────────────────────────┘
```

### Indexer finalization — two complementary approaches

`SimpleBitcoinVault.finalizeWithdrawal(bytes32 txid, uint32 withdrawalIndex)` emits **no events**. Strait detects the payout via two fallback phases run on every poll cycle:

**Phase 2 — UTXO polling** (`BtcPayoutWatcher`)

Watches the withdrawal recipient's Bitcoin address via the BitcoinKit precompile (`getUTXOsForBitcoinAddress`). When an unspent output appears, the OP_RETURN is read to confirm the vault-specific uuid matches. Sets the transfer FINALIZED with the Bitcoin txid.

- Works for fresh, still-unspent payouts
- Fails if the recipient spends the UTXO before the watcher polls (typically 60-second window)

**Phase 3 — Vault sweep txid** (`BtcPayoutWatcher.check_vault_sweeps`)

Calls `currentSweepUTXO()` (selector `0xe9beef3d`) on each `SimpleBitcoinVault` contract. The vault stores the Bitcoin txid of its most recent confirmed sweep directly in contract storage, so payout detection works even when the UTXO has already been spent.

```javascript
// Read the latest sweep txid for a vault
const CURRENT_SWEEP_UTXO_SELECTOR = "0xe9beef3d";
const result = await provider.call({ to: vaultAddress, data: CURRENT_SWEEP_UTXO_SELECTOR });
const sweepTxid = result; // bytes32 Bitcoin txid, zero if no sweep yet
```

The OP_RETURN in the sweep transaction carries the 4-byte vault-specific uuid (big-endian `uint32`) to identify which withdrawal was paid:

```
OP_RETURN script: 0x6a 0x04 <4 bytes big-endian vaultSpecificUuid>
Example:          6a 04 00 00 03 38   →  vaultUuid = 824
```

Configure the vault contract addresses for Phase 3 via `HEMI_VAULT_CONTRACTS` in `.env` (ordered by vault index, comma-separated).

**Remaining blind spot:** If Phase 2 misses a payout (UTXO already spent) AND the vault's `currentSweepUTXO` has since been overwritten by a newer sweep, the withdrawal stays INITIATED indefinitely. This requires either historical sweep tracking or scanning Bitcoin transactions directly.

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
