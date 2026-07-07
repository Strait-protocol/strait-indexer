# Contract Addresses

> **TL;DR:** This document lists contract addresses for Hemi Mainnet, Hemi Sepolia, and commonly used protocols. It also includes the full hBK (Bitcoin Kit) smart contract reference — structs, interface, and precompile addresses — which Strait uses to verify Bitcoin deposits and read OP_RETURN data directly from Hemi.

---

## Hemi Mainnet

### L1 Hemi Contracts

Core Hemi contracts deployed on **Ethereum Mainnet**.

| Contract Name | Contract Address |
|---|---|
| `AddressManager` | `0xA5F37791378c55941a52B4dCb70Be4D8D09f5e43` |
| `AnchorStateRegistryProxy` | `0xF44007EAF2faFdD8bA8d3551F23CD2b879F54677` |
| `DelayedWETHProxy` | `0xc5627348Dbc9179cFb5a24C8199635770Ea575A3` |
| `DisputeGameFactoryProxy` | `0x5442d0ddB33B396879D2d016A9ad09ad122562C3` |
| `L1CrossDomainMessengerProxy` | `0xF005dFb08377faD44588Af68d0884D272A6fb050` |
| `L1ERC721BridgeProxy` | `0xa446331bD28cbe0186A983a27C528f566B6bedE0` |
| `L1StandardBridgeProxy` | `0x5eaa10F99e7e6D177eF9F74E519E319aa49f191e` |
| `L2OutputOracleProxy` | `0x6daF3a3497D8abdFE12915aDD9829f83A79C0d51` |
| `Mips` | `0x42Ff661af011939f699D67bd021d237eBcBA9c2A` |
| `OptimismMintableERC20FactoryProxy` | `0x0262fEDC4A98f94dDB90CeF0E058644d8409342C` |
| `OptimismPortalProxy` | `0x39a0005415256B9863aFE2d55Edcf75ECc3A4D7e` |
| `OptimismPortal2` | `0x04dcfE50e43823A1D8f6e3Fbb8af10BfB7Ebb634` |
| `PreimageOracle` | `0x613F36BE58Ba712B37474F4B82484D680D24ed20` |
| `ProtocolVersionsProxy` | `0x13Cb1B6e69Ec8fF6a5C8823d1e8dc78CCCf3Ce48` |
| `ProxyAdmin` | `0xbE81A9D662422f667F634f3Fc301e2E360FeFB30` |
| `SafeProxyFactory` | `0xa6B71E26C5e0845f74c812102Ca7114b6a896AB2` |
| `SafeSingleton` | `0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552` |
| `SuperchainConfigProxy` | `0x15144FB8621cB3c4ED3DB223c173ffb58C8D2aB8` |
| `SystemConfigProxy` | `0x5ae68684D9179A8053883f1Df599Ea7Fb35303c3` |
| `SystemOwnerSafe` | `0x8434dc705e4B729405Dd66C94DfC62bc3825Ea69` |

---

### L2 Hemi Contracts

Core Hemi contracts deployed on **Hemi**.

| Contract Name | Contract Address |
|---|---|
| `WETH9` | [`0x4200000000000000000000000000000000000006`](https://explorer.hemi.xyz/token/0x4200000000000000000000000000000000000006) |
| `OptimismMintableERC20Factory` | [`0x4200000000000000000000000000000000000012`](https://explorer.hemi.xyz/address/0x4200000000000000000000000000000000000012) |
| `OptimismMintableERC721Factory` | [`0x4200000000000000000000000000000000000017`](https://explorer.hemi.xyz/address/0x4200000000000000000000000000000000000017) |
| `L2StandardBridge` | [`0x4200000000000000000000000000000000000010`](https://explorer.hemi.xyz/address/0x4200000000000000000000000000000000000010) |
| `L2ERC721Bridge` | `0x4200000000000000000000000000000000000014` |

---

### Utilities Contracts

Commonly used utility contracts deployed on **Hemi**.

| Contract Name | Contract Address |
|---|---|
| `BitcoinKit v1` | `0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12` |

### PoPPayoutsV2

Two deployments exist on Hemi Mainnet (owner `0xE067Dd6965bd87C81AbE658ed42FC02eB41d5Bd3`). Strait uses the canonical deployment.

| Deployment | Contract Address | Deployed Block | Factory |
|---|---|---|---|
| Canonical (used by Strait) | `0x9a23ab7cb11cfb96e577da52a6ad5211ff24434b` | 3,497,724 | `0x92f03ea43ee029dbd28b63029d6f07e1efdb7a1a` |
| First deployment | `0x9417dd2eba413cfc11e8d8e368c007bfa1385a40` | 3,497,671 | `0xf9705145175800f6f2e4a81261a4cb5406da6023` |

> **Activation status (June 2026):** `mintPoPRewards()` has never been called — `lastBlockRewarded = 0` on both contracts. PoP payouts are not yet active on mainnet.

---

### hVM Precompiles

For hVM precompile contract addresses, visit the [hVM Feature Summary](https://docs.hemi.xyz/building-bitcoin-apps/hemi-virtual-machine-hvm/feature-summary).

---

### Token Contracts

For a full list of token contract addresses deployed on **Hemi**, visit [`hemilabs/token-list`](https://github.com/hemilabs/token-list/blob/master/src/hemi.tokenlist.json).

---

## Hemi Sepolia

### L1 Hemi Contracts

Core Hemi contracts deployed on **Sepolia**.

| Contract Name | Contract Address |
|---|---|
| `AddressManager` | [`0x23f0022354241FDb721Dc43E7897d7Af662A2995`](https://sepolia.etherscan.io/address/0x23f0022354241fdb721dc43e7897d7af662a2995) |
| `L1CrossDomainMessengerProxy` | [`0x9bCCCf1d222539c4C47E4C6f5749e4d5fA33215c`](https://sepolia.etherscan.io/address/0x9bcccf1d222539c4c47e4c6f5749e4d5fa33215c) |
| `L2OutputOracleProxy` | [`0x032d1e1dd960A4B027a9a35FF8B2b672E333Bc27`](https://sepolia.etherscan.io/address/0x032d1e1dd960a4b027a9a35ff8b2b672e333bc27) |
| `OptimismPortalProxy` | [`0xB6f9579980aE46f61217A99145645341E49E2516`](https://sepolia.etherscan.io/address/0xB6f9579980aE46f61217A99145645341E49E2516) |
| `ProtocolVersionsProxy` | [`0xBD869d97B85C450d396215c5E1a81bbFA4545e23`](https://sepolia.etherscan.io/address/0xBD869d97B85C450d396215c5E1a81bbFA4545e23) |
| `DisputeGameFactoryProxy` | [`0x4cb8fdc8E1A8Ad01369F9a159C67c8be794a98FA`](https://sepolia.etherscan.io/address/0x4cb8fdc8E1A8Ad01369F9a159C67c8be794a98FA) |
| `L1StandardBridgeProxy` | [`0xc94b1BEe63A3e101FE5F71C80F912b4F4b055925`](https://sepolia.etherscan.io/address/0xc94b1bee63a3e101fe5f71c80f912b4f4b055925) |
| `OptimismMintableERC20FactoryProxy` | [`0xb4bCe3efD3282Da4eEC69429966a85f92298799B`](https://sepolia.etherscan.io/address/0xb4bCe3efD3282Da4eEC69429966a85f92298799B) |
| `ProxyAdmin` | [`0xc43ED1E8D70d0e5801514833fAD3D93Ba16Da4Aa`](https://sepolia.etherscan.io/address/0xc43ED1E8D70d0e5801514833fAD3D93Ba16Da4Aa) |
| `L1ERC721BridgeProxy` | [`0xa5ba2558B41F34f0B5Cc4eD389386201a3D31AEc`](https://sepolia.etherscan.io/address/0xa5ba2558b41f34f0b5cc4ed389386201a3d31aec) |
| `SystemConfigProxy` | [`0xfa73580F4D72294Ae9EE3DAaC36D8bF111B37Ce9`](https://sepolia.etherscan.io/address/0xfa73580F4D72294Ae9EE3DAaC36D8bF111B37Ce9) |

---

### L2 Hemi Contracts

Core Hemi contracts deployed on **Hemi Sepolia**.

| Contract Name | Contract Address |
|---|---|
| `L2ToL1MessagePasser` | [`0x4200000000000000000000000000000000000016`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000016) |
| `L2CrossDomainMessenger` | [`0x4200000000000000000000000000000000000007`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000007) |
| `L2StandardBridge` | [`0x4200000000000000000000000000000000000010`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000010) |
| `L2ERC721Bridge` | [`0x4200000000000000000000000000000000000014`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000014) |
| `SequencerFeeVault` | [`0x4200000000000000000000000000000000000011`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000011) |
| `OptimismMintableERC20Factory` | [`0x4200000000000000000000000000000000000012`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000012) |
| `OptimismMintableERC721Factory` | [`0x4200000000000000000000000000000000000017`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000017) |
| `L1Block` | [`0x4200000000000000000000000000000000000015`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000015) |
| `GasPriceOracle` | [`0x420000000000000000000000000000000000000F`](https://optimistic.etherscan.io/address/0x420000000000000000000000000000000000000F) |
| `ProxyAdmin` | [`0x4200000000000000000000000000000000000018`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000018) |
| `BaseFeeVault` | [`0x4200000000000000000000000000000000000019`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000019) |
| `L1FeeVault` | [`0x420000000000000000000000000000000000001A`](https://optimistic.etherscan.io/address/0x420000000000000000000000000000000000001A) |
| `GovernanceToken` | [`0x4200000000000000000000000000000000000042`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000042) |
| `SchemaRegistry` | [`0x4200000000000000000000000000000000000020`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000020) |
| `EAS` | [`0x4200000000000000000000000000000000000021`](https://optimistic.etherscan.io/address/0x4200000000000000000000000000000000000021) |

---

### Utilities Contracts

Commonly used utility contracts deployed on **Hemi Sepolia**.

| Contract Name | Contract Address |
|---|---|
| `BitcoinKit v0` | [`0xeC9fa5daC1118963933e1A675a4EEA0009b7f215`](https://testnet.explorer.hemi.xyz/address/0xeC9fa5daC1118963933e1A675a4EEA0009b7f215?tab=read_contract) |

---

### hVM Precompiles

For hVM precompile contract addresses, visit the [hVM Feature Summary](https://docs.hemi.xyz/building-bitcoin-apps/hemi-virtual-machine-hvm/feature-summary).

---

### Token Contracts

For a full list of token contract addresses deployed on **Hemi Sepolia**, visit [`hemilabs/token-list`](https://github.com/hemilabs/token-list/blob/master/src/hemi.tokenlist.json).

---

## hBK — Hemi Bitcoin Kit

The Bitcoin Kit (hBK) smart contract provides access to live Bitcoin chain state from within Hemi smart contracts and the Strait indexer. Strait uses it to verify Bitcoin deposits, count confirmations, and read OP_RETURN payloads — without relying solely on a Bitcoin RPC node.

| Network | Address |
|---|---|
| **Hemi Mainnet** | [`0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12`](https://explorer.hemi.xyz/address/0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12) |
| **Hemi Sepolia (testnet)** | [`0xeC9fa5daC1118963933e1A675a4EEA0009b7f215`](https://testnet.explorer.hemi.xyz/address/0xeC9fa5daC1118963933e1A675a4EEA0009b7f215) |

> **Phase 0 scope:** Script/address balances, UTXO set, full transactions with output availability, confirmation count, and block headers (latest and by height).

---

### Struct Definitions

#### `UTXO`

```solidity
struct UTXO {
    bytes32 txId;         // Transaction ID
    uint256 index;        // Output index
    uint256 value;        // Value in satoshis
    bytes   scriptPubKey; // Locking script
}
```

#### `Transaction`

```solidity
struct Transaction {
    bytes32  containingBlockHash; // Hash of the block containing this tx
    uint256  transactionVersion;
    uint256  size;
    uint256  vSize;               // Virtual size (segwit-adjusted)
    uint256  lockTime;
    Input[]  inputs;
    Output[] outputs;
    uint256  totalInputs;         // Total inputs in original tx (may exceed inputs array)
    uint256  totalOutputs;        // Total outputs in original tx (may exceed outputs array)
    bool     containsAllInputs;
    bool     containsAllOutputs;
}
```

#### `Input`

```solidity
struct Input {
    uint256 inValue;              // Value spent by this input (satoshis)
    bytes32 inputTxId;            // Source transaction ID
    uint256 sourceIndex;          // Output index in the source transaction
    bytes   scriptSig;            // Unlocking script
    uint256 sequence;
    uint256 fullScriptSigLength;
    bool    containsFullScriptSig;
}
```

#### `Output`

```solidity
struct Output {
    uint256     outValue;         // Value in satoshis
    bytes       script;           // Locking script
    string      outputAddress;    // Decoded address (if standard)
    bool        isOpReturn;       // True if this is an OP_RETURN output
    bytes       opReturnData;     // Raw OP_RETURN payload — used by Strait to extract
                                  // the Hemi destination address from tunnel deposits
    bool        isSpent;
    uint256     fullScriptLength;
    bool        containsFullScript;
    SpentDetail spentDetail;      // Populated when isSpent == true
}
```

#### `SpentDetail`

```solidity
struct SpentDetail {
    bytes32 spendingTxId; // Transaction that spent this output
    uint256 inputIndex;   // Index of the input in the spending transaction
}
```

#### `BitcoinHeader`

```solidity
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

### `IBitcoinKit` Interface

```solidity
interface IBitcoinKit {
    function getUTXOsForBitcoinAddress(
        string calldata btcAddress,
        uint256 pageNumber,
        uint256 pageSize
    ) external view returns (UTXO[] memory);

    function getTxConfirmations(bytes32 txId)
        external view returns (uint32 confirmations);

    function getBitcoinAddressBalance(string calldata btcAddress)
        external view returns (uint256 balance);

    function getTransactionByTxId(bytes32 txId)
        external view returns (Transaction memory);

    function getTransactionInputsByTxId(bytes32 txId)
        external view returns (Input[] memory);

    function getTransactionOutputsByTxId(bytes32 txId)
        external view returns (Output[] memory);

    function getLastHeader()
        external view returns (BitcoinHeader memory);

    function getHeaderN(uint32 height)
        external view returns (BitcoinHeader memory);
}
```

---

### Method Reference

| Method | Precompile | Description |
|---|---|---|
| `getBitcoinAddressBalance(address)` | `0x40` | Balance of a Bitcoin address in satoshis |
| `getUTXOsForBitcoinAddress(address, page, size)` | `0x41` | Paginated UTXO set for an address |
| `getTransactionByTxId(txId)` | `0x42` | Full transaction including all inputs and outputs |
| `getTxConfirmations(txId)` | `0x43` | Confirmation count for a transaction |
| `getLastHeader()` | `0x44` | Most recent Bitcoin block header seen by Hemi |
| `getHeaderN(height)` | `0x45` | Bitcoin block header at a specific height |

---

### Strait usage notes

- **Deposit detection**: `getUTXOsForBitcoinAddress` polls tunnel custody addresses for new UTXOs instead of scanning every Bitcoin block.
- **OP_RETURN parsing**: `getTransactionByTxId` returns `Output.isOpReturn` and `Output.opReturnData` — Strait iterates outputs to extract the Hemi destination address encoded in tunnel deposit transactions.
- **Confirmation gating**: `getTxConfirmations` provides the confirmation count used to enforce the `BITCOIN_CONFIRMATION_DEPTH` threshold before emitting a `TunnelDeposit` event.
- **Note**: `getBitcoinAddressBalance` returns `uint256` in the interface above (vs `uint64` in the v1 on-chain source). Use the on-chain ABI as the source of truth for the deployed version.

---

## Bitcoin Vault Custody Addresses

`BitcoinTunnelManager` (mainnet): `0xEAcA824F46c000fB89403846Bb57e6b913321081`

9 vaults total as of June 2026. Each vault is a `SimpleBitcoinVault` contract on Hemi with its own Bitcoin custody address. Vault 3 has not been configured with a Bitcoin address yet. Vaults 4 and 5 share the same Bitcoin address.

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

These 7 unique custody addresses are configured in `BITCOIN_TUNNEL_ADDRESSES` in `.env`. See [`docs/btc-tunnel-guide.md`](btc-tunnel-guide.md) for details on how the `CustodyWatcher` uses them.

---

## SimpleBitcoinVault — payout detection interface

Each vault contract exposes `currentSweepUTXO()`, which returns the Bitcoin txid of the most recently confirmed withdrawal sweep. Strait's `BtcPayoutWatcher` uses this to finalize Hemi→BTC withdrawals when the payout UTXO has already been spent.

```solidity
interface ISimpleBitcoinVault {
    /// Returns the Bitcoin txid (bytes32) of the most recent sweep transaction
    /// confirmed by the vault operator via finalizeWithdrawal().
    /// Returns zero (bytes32(0)) if no sweep has been processed yet.
    function currentSweepUTXO() external view returns (bytes32);
}
```

Selector: `0xe9beef3d`

Configure via `HEMI_VAULT_CONTRACTS` in `.env` — a comma-separated list of vault EVM addresses in vault-index order (index 0 first):

```env
HEMI_VAULT_CONTRACTS=0x3da10b74...,0xecf9c248...,0x13ca60fe,...
```

If `HEMI_VAULT_CONTRACTS` is empty or unset, Phase 3 sweep detection is disabled and only Phase 2 UTXO polling runs.
