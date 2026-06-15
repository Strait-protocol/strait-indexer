# Strait Documentation

Technical reference for the Hemi tunnel ecosystem — contracts, events, Bitcoin integration, and PoP anchoring. Based on confirmed sources: Hemi explorer, hemilabs/bitcoin-tunnel-contracts, hemilabs/pop-payouts, and direct contract verification.

---

## Contents

| Document | What it covers |
|---|---|
| [contract-addresses.md](contract-addresses.md) | All confirmed contract addresses across Hemi mainnet, Hemi Sepolia, and Ethereum. Includes full hBK interface reference. |
| [tunnel-architecture.md](tunnel-architecture.md) | How the three-chain tunnel system works end-to-end. The right place to start. |
| [btc-tunnel-guide.md](btc-tunnel-guide.md) | Bitcoin tunnel deep dive: vaults, OP_RETURN encoding, deposit and withdrawal flows, code examples. |
| [eth-tunnel-guide.md](eth-tunnel-guide.md) | ETH/ERC-20 tunnel: L2StandardBridge events, deposit and withdrawal flows, code examples. |
| [bitcoinkit-reference.md](bitcoinkit-reference.md) | BitcoinKit precompile cookbook — reading Bitcoin state from Hemi smart contracts, with Solidity and ethers.js examples. |
| [pop-anchoring.md](pop-anchoring.md) | PoP anchoring and Bitcoin finality: how PoPPayoutsV2 works, keystone windows, how to verify a transaction is Bitcoin-final. |
| [api-integration.md](api-integration.md) | **For app developers:** integrate the Strait read API (GraphQL + REST) — the `Transfer` schema, queries, the transfer lifecycle, amounts, and code examples. |

---

## Quick reference

### Tunnel contract addresses

| Contract | Network | Address |
|---|---|---|
| `BitcoinTunnelManager` | Hemi Mainnet | `0xEAcA824F46c000fB89403846Bb57e6b913321081` |
| `BitcoinTunnelManager` | Hemi Sepolia | `0x8221CFD3Eca3c5F9FA27b2AE774151642f1C449e` |
| `L2StandardBridge` | Hemi Mainnet + Sepolia | `0x4200000000000000000000000000000000000010` |
| `L1StandardBridgeProxy` | Ethereum Mainnet | `0x5eaa10F99e7e6D177eF9F74E519E319aa49f191e` |
| `L1StandardBridgeProxy` | Ethereum Sepolia | `0xc94b1BEe63A3e101FE5F71C80F912b4F4b055925` |
| `BitcoinKitV1` | Hemi Mainnet | `0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12` |
| `BitcoinKit v0` | Hemi Sepolia | `0xeC9fa5daC1118963933e1A675a4EEA0009b7f215` |
| `PoPPayoutsV2` | Hemi Sepolia | `0x4a3b61C586DB4CD219E85aC0697b66916c7457AB` |

### Key facts

- Bitcoin tunnel: **6 BTC confirmations** required before hBTC is minted (~1 hour)
- ETH tunnel: **~2 minutes** from Ethereum lock to Hemi mint
- PoP anchoring: **every 25 Hemi blocks** (~5 minutes), ~90 min to Bitcoin finality
- hBTC decimals: **8** (satoshi precision, matches native Bitcoin)
- Keystone frequency: **25 Hemi blocks** = one PoP anchoring window
