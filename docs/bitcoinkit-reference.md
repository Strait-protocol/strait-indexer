# BitcoinKit (hBK) Reference

BitcoinKit is a precompile that exposes live Bitcoin chain state to Hemi smart contracts and off-chain consumers. It lets you read Bitcoin balances, UTXOs, transactions, and headers directly from Hemi — no external Bitcoin RPC node, no oracle, no trust assumption.

| Network | Address | Version |
|---|---|---|
| Hemi Mainnet | `0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12` | v1 |
| Hemi Sepolia | `0xeC9fa5daC1118963933e1A675a4EEA0009b7f215` | v0 |

> Source: [`hemilabs/bitcoin-tunnel-contracts`](https://github.com/hemilabs/bitcoin-tunnel-contracts) (`contracts/bitcoinkit/`)

---

## Precompiles

The wrapper contract dispatches to fixed precompile addresses:

| Function | Precompile | Returns |
|---|---|---|
| `getBitcoinAddressBalance` | `0x40` | Balance in satoshis |
| `getUTXOsForBitcoinAddress` | `0x41` | UTXO set (paginated) |
| `getTransactionByTxId` | `0x42` | Full transaction |
| `getTxConfirmations` | `0x43` | Confirmation count |
| `getLastHeader` | `0x44` | Latest Bitcoin header |
| `getHeaderN` | `0x45` | Header at height |
| `getScriptForAddress` (`BtcAddrToScript`) | `0x46` | Locking script for an address |

All precompiles support P2PKH, P2SH, P2WPKH, P2WSH, and P2TR address formats.

---

## Interface

```solidity
interface IBitcoinKit {
    function getUTXOsForBitcoinAddress(string calldata btcAddress, uint256 pageNumber, uint256 pageSize) external view returns (UTXO[] memory);
    function getTxConfirmations(bytes32 txId) external view returns (uint32 confirmations);
    function getBitcoinAddressBalance(string calldata btcAddress) external view returns (uint256 balance);
    function getTransactionByTxId(bytes32 txId) external view returns (Transaction memory);
    function getTransactionInputsByTxId(bytes32 txId) external view returns (Input[] memory);
    function getTransactionOutputsByTxId(bytes32 txId) external view returns (Output[] memory);
    function getScriptForAddress(string calldata btcAddress) external view returns (bytes memory script);
    function getLastHeader() external view returns (BitcoinHeader memory);
    function getHeaderN(uint32 height) external view returns (BitcoinHeader memory);
}
```

---

## Structs

```solidity
struct UTXO {
    bytes32 txId;
    uint256 index;
    uint256 value;          // satoshis
    bytes   scriptPubKey;
}

struct Transaction {
    bytes32  containingBlockHash;
    uint256  transactionVersion;
    uint256  size;
    uint256  vSize;
    uint256  lockTime;
    Input[]  inputs;
    Output[] outputs;
    uint256  totalInputs;
    uint256  totalOutputs;
    bool     containsAllInputs;
    bool     containsAllOutputs;
}

struct Output {
    uint256     outValue;       // satoshis
    bytes       script;         // full locking script (incl. OP_RETURN opcode)
    string      outputAddress;
    bool        isOpReturn;
    bytes       opReturnData;   // raw OP_RETURN payload
    bool        isSpent;
    uint256     fullScriptLength;
    bool        containsFullScript;
    SpentDetail spentDetail;    // populated when isSpent == true
}

struct SpentDetail {
    bytes32 spendingTxId;       // tx that spent this output
    uint256 inputIndex;
}

struct BitcoinHeader {
    uint32  height;
    bytes32 blockHash;
    uint32  version;
    bytes32 previousBlockHash;
    bytes32 merkleRoot;
    uint32  timestamp;
    uint32  bits;
    uint32  nonce;
}
```

---

## Recipes

### Check if a Bitcoin deposit is confirmed

```solidity
function isDepositReady(bytes32 txId, uint32 minConfirmations) external view returns (bool) {
    IBitcoinKit kit = IBitcoinKit(0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12);
    return kit.getTxConfirmations(txId) >= minConfirmations;
}
```

### Read OP_RETURN data from a transaction

The most common indexing operation — find the OP_RETURN output and read its script.

```solidity
function getOpReturnScript(bytes32 txId) external view returns (bytes memory) {
    IBitcoinKit kit = IBitcoinKit(0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12);
    Output[] memory outputs = kit.getTransactionOutputsByTxId(txId);

    for (uint i = 0; i < outputs.length && i < 8; i++) {
        if (outputs[i].isOpReturn) {
            return outputs[i].script;   // pass to your address parser
        }
    }
    return "";
}
```

> Use `getTransactionOutputsByTxId` when you only need outputs — it is cheaper than fetching the full `Transaction`.

### Off-chain via eth_call (ethers.js)

```javascript
const KIT_ADDRESS = "0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12";
const kit = new ethers.Contract(KIT_ADDRESS, BITCOINKIT_ABI, hemiProvider);

// Confirmation count
const confirmations = await kit.getTxConfirmations("0x" + bitcoinTxid);

// Full transaction with outputs
const tx = await kit.getTransactionByTxId("0x" + bitcoinTxid);
for (const output of tx.outputs) {
    if (output.isOpReturn) {
        console.log("OP_RETURN script:", output.script);
    }
}

// Watch a custody address for UTXOs (page 0, 50 per page)
const utxos = await kit.getUTXOsForBitcoinAddress(custodyAddress, 0, 50);
```

### Via cast (Foundry)

```bash
# Confirmation count for a Bitcoin txid
cast call 0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12 \
  "getTxConfirmations(bytes32)(uint32)" \
  0x<bitcoin_txid> \
  --rpc-url https://rpc.hemi.network/rpc

# Balance of a Bitcoin address (satoshis)
cast call 0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12 \
  "getBitcoinAddressBalance(string)(uint256)" \
  "bc1q..." \
  --rpc-url https://rpc.hemi.network/rpc
```

---

## Why this matters for indexing

Traditional Bitcoin bridges require running a separate Bitcoin full node and trusting it. BitcoinKit lets you verify Bitcoin state from within the deterministic Hemi EVM — the same state every Hemi node agrees on.

For a tunnel indexer, this means you can:

- **Detect deposits** without scanning every Bitcoin block — query `getUTXOsForBitcoinAddress` on each vault custody address
- **Verify confirmations** with `getTxConfirmations` instead of tracking the Bitcoin chain tip yourself
- **Read OP_RETURN data** with `getTransactionOutputsByTxId` instead of parsing raw Bitcoin transactions
- **Trace spends** via `Output.spentDetail` to detect when a tunnel UTXO is consumed

This collapses the "run a Bitcoin node + a Hemi node + reconcile them" problem into "query one Hemi RPC."
