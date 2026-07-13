//! Tunnel contract ABIs and log decoders.
//!
//! Two bridge mechanisms are in play:
//!
//! - **ETH/ERC-20 routes** — OP Stack L2StandardBridge at the well-known
//!   address 0x4200000000000000000000000000000000000010 on Hemi, and the
//!   corresponding L1StandardBridgeProxy on Ethereum. Event signatures
//!   confirmed from the Hemi explorer.
//!
//! - **BTC routes** — A separate Hemi-specific tunnel contract. The event
//!   shapes here are derived from Hemi's architecture; the contract address
//!   must still be confirmed from Hemi documentation.
//!   FIXME: confirm BTC tunnel contract address with Hemi docs.
//!
//! - **BitcoinKitV1** — A read-only precompile on Hemi that exposes Bitcoin
//!   chain state. Has NO events — only view functions. Confirmed from Hemi
//!   mainnet explorer. Used by the Bitcoin watcher to verify deposits and
//!   read OP_RETURN data without relying solely on a Bitcoin RPC node.

use alloy::primitives::{Address, B256, U256};
use alloy::sol;
use strait_core::error::{Result, StraitError};

// ============================================================================
// OP Stack Standard Bridge (ETH / ERC-20 routes)
// Confirmed from Hemi mainnet explorer — L2StandardBridge implementation at
// 0xC0d3c0d3c0D3c0d3C0D3c0D3C0d3C0D3C0D30010
// ============================================================================

sol! {
    /// OP Stack L2StandardBridge — handles ETH and ERC-20 tunnel transfers.
    /// Deployed at 0x4200000000000000000000000000000000000010 on Hemi (and
    /// Hemi Sepolia). The proxy resolves to the implementation above.
    interface IStandardBridge {

        // ETH deposits: Ethereum → Hemi
        event ETHBridgeFinalized(
            address indexed from,
            address indexed to,
            uint256 amount,
            bytes extraData
        );

        // ETH withdrawals initiated: Hemi → Ethereum
        event ETHBridgeInitiated(
            address indexed from,
            address indexed to,
            uint256 amount,
            bytes extraData
        );

        // ERC-20 deposits: Ethereum → Hemi
        event ERC20BridgeFinalized(
            address indexed localToken,
            address indexed remoteToken,
            address indexed from,
            address to,
            uint256 amount,
            bytes extraData
        );

        // ERC-20 withdrawals initiated: Hemi → Ethereum
        event ERC20BridgeInitiated(
            address indexed localToken,
            address indexed remoteToken,
            address indexed from,
            address to,
            uint256 amount,
            bytes extraData
        );

        // Legacy deposit event (older OP Stack versions, still emitted)
        event DepositFinalized(
            address indexed l1Token,
            address indexed l2Token,
            address indexed from,
            address to,
            uint256 amount,
            bytes extraData
        );

        // Legacy withdrawal event
        event WithdrawalInitiated(
            address indexed l1Token,
            address indexed l2Token,
            address indexed from,
            address to,
            uint256 amount,
            bytes extraData
        );
    }
}

// ============================================================================
// BitcoinTunnelManager — central BTC tunnel hub (confirmed from repo + explorer)
//
// Mainnet:  0xEAcA824F46c000fB89403846Bb57e6b913321081
// Testnet:  0x8221CFD3Eca3c5F9FA27b2AE774151642f1C449e
//
// Architecture (from hemilabs/bitcoin-tunnel-contracts):
//   - BitcoinTunnelManager is the single interaction point for all BTC tunnel ops.
//   - It manages multiple SimpleBitcoinVault instances, each with its own Bitcoin
//     custody address. Vaults are created by operators and tracked by vaultIndex.
//   - DepositConfirmed is the primary join key event for BTC→Hemi: it carries
//     the Bitcoin depositTxId which is the cross-chain match key.
//   - WithdrawalInitiated is the Hemi-side signal for Hemi→BTC outflows.
//   - VaultCreated must be watched to discover vault custody addresses dynamically.
//
// hBTC token (BTCToken.sol) is a standard ERC20 deployed by BitcoinTunnelManager.
// Decimals: 8 (matches native Bitcoin satoshi precision).
// ============================================================================

sol! {
    /// BitcoinTunnelManager — confirmed ABI from hemilabs/bitcoin-tunnel-contracts.
    interface IBitcoinTunnelManager {

        /// Emitted when a new vault is deployed under this tunnel manager.
        /// Strait watches this to discover vault custody addresses to track.
        event VaultCreated(
            address indexed setupAdmin,
            address indexed operatorAdmin,
            address indexed vaultAddress
        );

        /// Emitted when a BTC deposit is confirmed on Hemi (BTC → Hemi).
        /// depositTxId is the Bitcoin txid — the primary cross-chain join key.
        /// netSatsAfterFee is the amount of hBTC minted to recipient.
        event DepositConfirmed(
            address indexed vault,
            address indexed recipient,
            bytes32 indexed depositTxId,
            uint256 depositSats,
            uint256 netSatsAfterFee
        );

        /// Emitted when a user initiates a BTC withdrawal (Hemi → BTC).
        /// uuid encodes (vaultIndex << 32 | vaultSpecificUUID).
        event WithdrawalInitiated(
            address indexed vault,
            address indexed withdrawer,
            string indexed btcAddress,
            uint256 withdrawalSats,
            uint256 netSatsAfterFee,
            uint64 uuid
        );

        /// Emitted when a withdrawal challenge succeeds (operator failed to pay).
        /// hBTC is re-minted to the original withdrawer.
        event WithdrawalChallengeSuccess(
            address indexed vault,
            address indexed withdrawer,
            uint64 indexed uuid
        );

        /// Confirm a BTC deposit and mint hBTC to the EVM address in the OP_RETURN.
        function confirmDeposit(
            uint32 vaultIndex,
            bytes32 txid,
            uint256 outputIndex,
            bytes memory extraInfo
        ) external returns (bool successful);

        /// Initiate a BTC withdrawal — burns hBTC and registers the withdrawal.
        function initiateWithdrawal(
            uint32 vaultIndex,
            string memory btcAddress,
            uint256 amount
        ) external returns (uint256 feeSats, uint64 uuid);

        /// Challenge a withdrawal that was not fulfilled within the deadline.
        function challengeWithdrawal(
            uint64 uuid,
            bytes memory extraInfo
        ) external returns (bool success);

        /// Get the vault contract at a given index.
        function vaults(uint32 index) external view returns (address);

        /// Total number of vaults created.
        function vaultCounter() external view returns (uint32);

        /// The hBTC ERC-20 token contract deployed by this manager.
        function btcTokenContract() external view returns (address);
    }
}

// ============================================================================
// PoPPayoutsV2 — PoP anchoring contract (confirmed from hemilabs/pop-payouts)
//
// Testnet:  0x4a3b61C586DB4CD219E85aC0697b66916c7457AB  (Hemi Sepolia)
// Mainnet:  not yet indexed — confirm from Hemi explorer
//
// One keystone every 25 Hemi blocks (~5 minutes). When mintPoPRewards() is
// called by the sequencer for keystone K, it emits PayoutRoundExecuted(K).
// All Hemi blocks in (K-25, K] are then considered PoP-anchored on Bitcoin.
//
// Strait uses this to advance BTC→Hemi transfers from INITIATED → ANCHORED:
//   anchored when PayoutRoundExecuted(blockRewarded >= ceil(mint_block/25)*25).
// ============================================================================

sol! {
    interface IPoPPayoutsV2 {
        /// Fired once per keystone block when mintPoPRewards() is called.
        /// blockRewarded is always a multiple of 25 (KEYSTONE_FREQUENCY).
        /// popScore == 0 means no miners published but the round still executed.
        event PayoutRoundExecuted(
            uint64 indexed blockRewarded,
            uint256 rewardPool,
            uint256 popScore
        );

        /// Fired when missed keystones are backfilled with zero publications.
        event RoundsBackfilled(
            uint64 indexed startBlock,
            uint64 indexed endBlock,
            uint256 count
        );

        /// The most recently rewarded keystone block.
        /// Any Hemi block <= lastBlockRewarded is PoP-anchored.
        function lastBlockRewarded() external view returns (uint64);

        /// Query a specific payout round by index.
        /// rounds[i].blockHeight is the keystone block for that round.
        function rounds(uint256 index) external view returns (
            uint64 blockHeight,
            uint256 totalPoPScore,
            uint256 rewardPool
        );
    }
}

// ============================================================================
// OptimismPortal — OP Stack withdrawal proving + finalization
//
// Hemi mainnet OptimismPortal:  confirm from Hemi explorer (ETH_OPT_PORTAL_CONTRACT env)
// Hemi testnet OptimismPortal:  confirm from Sepolia explorer
//
// Two-step withdrawal flow:
//   1. proveWithdrawalTransaction  → emits WithdrawalProven  → 1-day challenge window
//   2. finalizeWithdrawalTransaction → emits ETHBridgeFinalized on L1StandardBridge
//
// Strait watches WithdrawalProven to advance HEMI_TO_ETH withdrawals INITIATED → PROVING.
// ETHBridgeFinalized (already watched on L1StandardBridge) advances PROVING → FINALIZED.
// ============================================================================

sol! {
    interface IOptimismPortal {
        /// Emitted when proveWithdrawalTransaction is called.
        /// Starts the 1-day challenge window for the withdrawal.
        event WithdrawalProven(
            bytes32 indexed withdrawalHash,
            address indexed from,
            address indexed to
        );

        /// Emitted when finalizeWithdrawalTransaction is called (after the window).
        event WithdrawalFinalized(bytes32 indexed withdrawalHash, bool success);
    }
}

// ============================================================================
// BitcoinKitV1 precompile interface
// Confirmed from Hemi mainnet explorer (verified, Solidity 0.8.26).
// Mainnet:  0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12
// Testnet:  0xeC9fa5daC1118963933e1A675a4EEA0009b7f215  (v0 — slightly different API)
//
// This contract has NO events — it is a pure read precompile that exposes
// Bitcoin chain state directly from Hemi's embedded Bitcoin node.
// Use it to:
//   - Verify a Bitcoin txid exists and is confirmed
//   - Read OP_RETURN outputs (Output.isOpReturn / Output.opReturnData) to
//     extract the Hemi destination address encoded in tunnel deposits
//   - Watch custody addresses for new UTXOs instead of polling Bitcoin RPC
// ============================================================================

sol! {
    /// Minimal ERC-20 metadata interface, used to resolve the symbol/decimals of
    /// a bridged token from its contract address (the standard bridge events only
    /// carry the address, not the symbol).
    interface IERC20Metadata {
        function symbol() external view returns (string memory);
        function decimals() external view returns (uint8);
    }
}

/// Best-effort `symbol()` call against an ERC-20 contract. Returns `None` on any
/// RPC/decode failure (nonstandard token, transient error) rather than erroring —
/// callers should fall back to an empty symbol.
pub async fn fetch_erc20_symbol(
    provider: &dyn alloy::providers::Provider,
    token: Address,
) -> Option<String> {
    use alloy::rpc::types::TransactionRequest;
    use alloy::sol_types::SolCall;

    let calldata = IERC20Metadata::symbolCall {}.abi_encode();
    let req = TransactionRequest::default().to(token).input(calldata.into());
    let raw = provider.call(&req).await.ok()?;
    IERC20Metadata::symbolCall::abi_decode_returns(&raw, false)
        .ok()
        .map(|r| r._0)
}

/// Best-effort `decimals()` call against an ERC-20 contract. Returns `None` on any
/// RPC/decode failure — callers should fall back to a default (e.g. 18).
pub async fn fetch_erc20_decimals(
    provider: &dyn alloy::providers::Provider,
    token: Address,
) -> Option<u8> {
    use alloy::rpc::types::TransactionRequest;
    use alloy::sol_types::SolCall;

    let calldata = IERC20Metadata::decimalsCall {}.abi_encode();
    let req = TransactionRequest::default().to(token).input(calldata.into());
    let raw = provider.call(&req).await.ok()?;
    IERC20Metadata::decimalsCall::abi_decode_returns(&raw, false)
        .ok()
        .map(|r| r._0)
}

/// A log entry as returned by `eth_getTransactionReceipt`, decoded independently
/// of the full receipt shape.
#[derive(Debug, serde::Deserialize)]
struct RawLog {
    address: Address,
    topics: Vec<B256>,
}

/// A minimal, permissive view of `eth_getTransactionReceipt`'s result — just the
/// `logs` field. OP-Stack deposit transactions (Hemi's L1→L2 mint txs) use
/// transaction type `0x7e`, which `alloy_rpc_types_eth::TransactionReceipt`'s
/// `type` enum (pinned to standard `0x0`-`0x4`) doesn't recognize, so decoding a
/// full typed receipt for those errors out before we ever reach the logs. This
/// struct only pulls `logs`, ignoring every other field (including `type`), so it
/// decodes fine regardless of transaction type.
#[derive(Debug, serde::Deserialize)]
struct RawReceiptLogs {
    logs: Vec<RawLog>,
}

/// Fetch just the `(address, topics)` of every log in `tx_hash`'s receipt, via a
/// raw JSON-RPC call rather than `Provider::get_transaction_receipt` — see
/// `RawReceiptLogs` for why. Returns `None` if the tx isn't found or the RPC call
/// fails.
pub async fn fetch_receipt_logs(
    provider: &dyn alloy::providers::Provider,
    tx_hash: B256,
) -> Option<Vec<(Address, Vec<B256>)>> {
    let receipt: RawReceiptLogs = provider
        .client()
        .request("eth_getTransactionReceipt", (tx_hash,))
        .await
        .ok()?;
    Some(receipt.logs.into_iter().map(|l| (l.address, l.topics)).collect())
}

sol! {
    /// SimpleBitcoinVault — one vault contract per Bitcoin custody address on Hemi.
    /// The BTC payout watcher calls currentSweepUTXO() to detect BTC payouts
    /// without requiring the UTXO to remain unspent.
    interface ISimpleBitcoinVault {
        /// The Bitcoin txid (bytes32) of the most recent sweep transaction confirmed
        /// by the vault operator via finalizeWithdrawal(). Returns zero if no sweep
        /// has been processed yet.
        function currentSweepUTXO() external view returns (bytes32);
    }
}

sol! {
    /// BitcoinKitV1 — read-only precompile exposing Bitcoin state on Hemi.
    interface IBitcoinKitV1 {

        /// Get all UTXOs for a Bitcoin address (paginated).
        function getUTXOsForBitcoinAddress(
            string calldata btcAddress,
            uint32 pageNumber,
            uint32 pageSize
        ) external view returns (UTXO[] memory);

        /// Get the number of confirmations for a Bitcoin transaction.
        function getTxConfirmations(bytes32 txId) external view returns (uint32 confirmations);

        /// Get the balance of a Bitcoin address in satoshis.
        function getBitcoinAddressBalance(string calldata btcAddress) external view returns (uint64 balance);

        /// Get the locking script for a Bitcoin address.
        function getScriptForAddress(string calldata btcAddress) external view returns (bytes memory script);

        /// Check if a Bitcoin address is valid.
        function isAddressValid(string calldata btcAddress) external view returns (bool);

        /// Check if a Bitcoin transaction exists in the chain.
        function transactionExists(bytes32 txId) external view returns (bool exists);

        /// Get a full transaction by txid.
        /// The Transaction struct contains an outputs array; each Output has
        /// isOpReturn (bool) and opReturnData (bytes) fields.
        function getTransactionByTxId(bytes32 txId) external view returns (Transaction memory);

        /// Get all inputs for a transaction — more efficient than getTransactionByTxId
        /// when only the inputs are needed.
        function getTransactionInputsByTxId(bytes32 txId) external view returns (Input[] memory);

        /// Get all outputs for a transaction — more efficient than getTransactionByTxId
        /// when only the outputs are needed. Use this when scanning for OP_RETURN outputs.
        function getTransactionOutputsByTxId(bytes32 txId) external view returns (Output[] memory);

        /// Get the number of inputs in a transaction.
        function getTxInputCount(bytes32 txId) external view returns (uint32 txInputCount);

        /// Get the number of outputs in a transaction.
        function getTxOutputCount(bytes32 txId) external view returns (uint32 txOutputCount);

        /// Get a specific input of a transaction.
        function getSpecificTxInput(bytes32 txId, uint32 inputIndex) external view returns (Input memory);

        /// Get a specific output of a transaction.
        /// Use this with the known OP_RETURN output index to read opReturnData
        /// without fetching the entire transaction.
        function getSpecificTxOutput(bytes32 txId, uint32 outputIndex) external view returns (Output memory);

        /// Get the most recent Bitcoin block header seen by Hemi.
        function getLastHeader() external view returns (BitcoinHeader memory);

        /// Get the Bitcoin block header at a specific height.
        function getHeaderN(uint32 height) external view returns (BitcoinHeader memory);
    }

    // -------------------------------------------------------------------------
    // Structs (from BitcoinKitStructs.sol, confirmed from source)
    // -------------------------------------------------------------------------

    struct UTXO {
        bytes32 txId;
        uint256 index;
        uint256 value;           // in satoshis
        bytes scriptPubKey;
    }

    struct Transaction {
        bytes32 containingBlockHash;
        uint256 transactionVersion;
        uint256 size;
        uint256 vSize;
        uint256 lockTime;
        Input[] inputs;
        Output[] outputs;
        uint256 totalInputs;
        uint256 totalOutputs;
        bool containsAllInputs;
        bool containsAllOutputs;
    }

    struct Input {
        uint256 witnessElements;
        uint256 inValue;
        bytes32 inputTxId;
        uint256 sourceIndex;
        bytes scriptSig;
        uint256 sequence;
        uint256 fullScriptSigLength;
        bool containsFullScriptSig;
    }

    /// Details of the transaction that spent an output.
    /// Populated only when Output.isSpent == true.
    struct SpentDetail {
        bytes32 spendingTxId; // Transaction that spent this output
        uint256 inputIndex;   // Index of the input in the spending transaction
    }

    struct Output {
        uint256 outValue;        // in satoshis
        bytes script;
        string outputAddress;
        bool isOpReturn;
        bytes opReturnData;      // raw OP_RETURN payload — contains the Hemi
                                 // destination address for tunnel deposits
                                 // FIXME: confirm exact byte encoding with Hemi docs
        bool isSpent;
        uint256 fullScriptLength;
        bool containsFullScript;
        SpentDetail spentDetail; // Populated when isSpent == true
    }

    struct BitcoinHeader {
        uint32 height;
        bytes32 blockHash;
        uint32 version;
        bytes32 previousBlockHash;
        bytes32 merkleRoot;
        uint32 timestamp;
        uint32 bits;
        uint32 nonce;
    }
}

// ============================================================================
// Event decoder structs
// ============================================================================

/// Decoded ETHBridgeFinalized event (ETH deposit arriving on Hemi).
#[derive(Debug, Clone)]
pub struct EthBridgeFinalizedEvent {
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub extra_data: Vec<u8>,
}

/// Decoded ETHBridgeInitiated event (ETH withdrawal leaving Hemi).
#[derive(Debug, Clone)]
pub struct EthBridgeInitiatedEvent {
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub extra_data: Vec<u8>,
}

/// Decoded ERC20BridgeFinalized event (ERC-20 deposit arriving on Hemi).
#[derive(Debug, Clone)]
pub struct Erc20BridgeFinalizedEvent {
    pub local_token: Address,
    pub remote_token: Address,
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub extra_data: Vec<u8>,
}

/// Decoded ERC20BridgeInitiated event (ERC-20 withdrawal leaving Hemi).
#[derive(Debug, Clone)]
pub struct Erc20BridgeInitiatedEvent {
    pub local_token: Address,
    pub remote_token: Address,
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub extra_data: Vec<u8>,
}

/// Decoded DepositConfirmed event (BTC deposit confirmed on Hemi, hBTC minted).
/// `deposit_tx_id` is the Bitcoin txid — primary cross-chain join key.
#[derive(Debug, Clone)]
pub struct DepositConfirmedEvent {
    pub vault: Address,
    pub recipient: Address,
    pub deposit_tx_id: [u8; 32],
    pub deposit_sats: U256,
    pub net_sats_after_fee: U256,
}

/// Decoded WithdrawalInitiated event (Hemi→BTC withdrawal registered).
/// `uuid` encodes (vaultIndex << 32 | vaultSpecificUUID).
#[derive(Debug, Clone)]
pub struct BtcWithdrawalInitiatedEvent {
    pub vault: Address,
    pub withdrawer: Address,
    pub btc_address: String,
    pub withdrawal_sats: U256,
    pub net_sats_after_fee: U256,
    pub uuid: u64,
}

/// Decoded VaultCreated event — needed to discover vault custody addresses.
#[derive(Debug, Clone)]
pub struct VaultCreatedEvent {
    pub setup_admin: Address,
    pub operator_admin: Address,
    pub vault_address: Address,
}

// ============================================================================
// Topic selectors
// ============================================================================

/// Event topic selectors for log filtering.
pub mod topics {
    use alloy::primitives::{keccak256, B256};

    // Standard bridge events (ETH / ERC-20 routes)
    pub fn eth_bridge_finalized() -> B256 {
        keccak256(b"ETHBridgeFinalized(address,address,uint256,bytes)")
    }
    pub fn eth_bridge_initiated() -> B256 {
        keccak256(b"ETHBridgeInitiated(address,address,uint256,bytes)")
    }
    pub fn erc20_bridge_finalized() -> B256 {
        keccak256(b"ERC20BridgeFinalized(address,address,address,address,uint256,bytes)")
    }
    pub fn erc20_bridge_initiated() -> B256 {
        keccak256(b"ERC20BridgeInitiated(address,address,address,address,uint256,bytes)")
    }
    pub fn deposit_finalized() -> B256 {
        keccak256(b"DepositFinalized(address,address,address,address,uint256,bytes)")
    }
    pub fn withdrawal_initiated() -> B256 {
        keccak256(b"WithdrawalInitiated(address,address,address,address,uint256,bytes)")
    }

    // BitcoinTunnelManager events (BTC routes) — confirmed from hemilabs/bitcoin-tunnel-contracts
    pub fn deposit_confirmed() -> B256 {
        keccak256(b"DepositConfirmed(address,address,bytes32,uint256,uint256)")
    }
    pub fn btc_withdrawal_initiated() -> B256 {
        keccak256(b"WithdrawalInitiated(address,address,string,uint256,uint256,uint64)")
    }
    pub fn vault_created() -> B256 {
        keccak256(b"VaultCreated(address,address,address)")
    }
    pub fn withdrawal_challenge_success() -> B256 {
        keccak256(b"WithdrawalChallengeSuccess(address,address,uint64)")
    }

    // PoPPayoutsV2 events
    pub fn payout_round_executed() -> B256 {
        keccak256(b"PayoutRoundExecuted(uint64,uint256,uint256)")
    }
    pub fn rounds_backfilled() -> B256 {
        keccak256(b"RoundsBackfilled(uint64,uint64,uint256)")
    }

    // OptimismPortal events
    pub fn withdrawal_proven() -> B256 {
        keccak256(b"WithdrawalProven(bytes32,address,address)")
    }
    pub fn withdrawal_finalized() -> B256 {
        keccak256(b"WithdrawalFinalized(bytes32,bool)")
    }
}

// ============================================================================
// Contract addresses
// ============================================================================

pub mod addresses {
    use alloy::primitives::Address;

    // ---- L2StandardBridge on Hemi (mainnet + Sepolia) ----
    // Confirmed: well-known OP Stack address, verified on Hemi explorer.

    /// L2StandardBridge on Hemi mainnet and Hemi Sepolia.
    pub const HEMI_L2_STANDARD_BRIDGE: Address =
        alloy::primitives::address!("4200000000000000000000000000000000000010");

    // ---- L1StandardBridgeProxy on Ethereum ----
    // Confirmed from the Hemi contract addresses doc.

    /// L1StandardBridgeProxy on Ethereum mainnet.
    pub const ETH_L1_STANDARD_BRIDGE: Address =
        alloy::primitives::address!("5eaa10F99e7e6D177eF9F74E519E319aa49f191e");

    /// L1StandardBridgeProxy on Sepolia (Hemi testnet counterpart).
    pub const ETH_SEPOLIA_L1_STANDARD_BRIDGE: Address =
        alloy::primitives::address!("c94b1BEe63A3e101FE5F71C80F912b4F4b055925");

    // ---- BitcoinKit precompile ----
    // Confirmed from Hemi explorer (verified source).

    /// BitcoinKit v1 on Hemi mainnet.
    pub const HEMI_BITCOIN_KIT_V1: Address =
        alloy::primitives::address!("7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12");

    /// BitcoinKit v0 on Hemi Sepolia (testnet).
    pub const HEMI_SEPOLIA_BITCOIN_KIT_V0: Address =
        alloy::primitives::address!("eC9fa5daC1118963933e1A675a4EEA0009b7f215");

    // ---- PoPPayoutsV2 ----
    // Confirmed from Hemi Sepolia explorer + hemilabs/pop-payouts repo.

    /// PoPPayoutsV2 on Hemi Sepolia (testnet). Confirmed from explorer.
    pub const HEMI_SEPOLIA_POP_PAYOUTS_V2: Address =
        alloy::primitives::address!("4a3b61C586DB4CD219E85aC0697b66916c7457AB");

    // Two PoPPayoutsV2 contracts were deployed on Hemi mainnet by the Hemi team
    // (owner 0xE067Dd6965bd87C81AbE658ed42FC02eB41d5Bd3) at blocks ~3,497,671 and
    // ~3,497,724. mintPoPRewards() has not yet been called on either — PoP payouts
    // are not yet activated. The second deployment is used as the canonical address;
    // update HEMI_POP_PAYOUTS_CONTRACT in .env if the Hemi team activates the first.
    //
    // Factory for first deployment:  0xf9705145175800f6f2e4a81261a4cb5406da6023 (block 3,497,671)
    // Factory for second deployment: 0x92f03ea43ee029dbd28b63029d6f07e1efdb7a1a (block 3,497,724)

    /// PoPPayoutsV2 deployment #1 on Hemi mainnet (block 3,497,671). Likely superseded by #2.
    pub const HEMI_POP_PAYOUTS_V2_FIRST: Address =
        alloy::primitives::address!("9417dd2eba413cfc11e8d8e368c007bfa1385a40");

    /// PoPPayoutsV2 deployment #2 on Hemi mainnet (block 3,497,724). Use this as canonical.
    pub const HEMI_POP_PAYOUTS_V2: Address =
        alloy::primitives::address!("9a23ab7cb11cfb96e577da52a6ad5211ff24434b");

    // ---- BitcoinTunnelManager ----
    // Confirmed from Hemi explorer + hemilabs/bitcoin-tunnel-contracts repo.

    /// BitcoinTunnelManager on Hemi mainnet.
    pub const HEMI_BITCOIN_TUNNEL_MANAGER: Address =
        alloy::primitives::address!("EAcA824F46c000fB89403846Bb57e6b913321081");

    /// BitcoinTunnelManager on Hemi Sepolia (testnet).
    pub const HEMI_SEPOLIA_BITCOIN_TUNNEL_MANAGER: Address =
        alloy::primitives::address!("8221CFD3Eca3c5F9FA27b2AE774151642f1C449e");
}

// ============================================================================
// Utilities
// ============================================================================

pub fn bytes32_to_txid(bytes: &[u8; 32]) -> String {
    hex::encode(bytes)
}

pub fn txid_to_bytes32(txid: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(txid)
        .map_err(|e| StraitError::Parse(format!("Invalid txid hex: {}", e)))?;
    if bytes.len() != 32 {
        return Err(StraitError::Parse(format!(
            "Txid must be 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut result = [0u8; 32];
    result.copy_from_slice(&bytes);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_selectors_are_unique() {
        let selectors = [
            topics::eth_bridge_finalized(),
            topics::eth_bridge_initiated(),
            topics::erc20_bridge_finalized(),
            topics::erc20_bridge_initiated(),
            topics::deposit_confirmed(),
            topics::btc_withdrawal_initiated(),
            topics::withdrawal_challenge_success(),
            topics::vault_created(),
            topics::payout_round_executed(),
        ];
        for i in 0..selectors.len() {
            for j in (i + 1)..selectors.len() {
                assert_ne!(selectors[i], selectors[j], "Duplicate topic at indices {i} and {j}");
            }
        }
    }

    #[test]
    fn test_topic_selectors_are_deterministic() {
        assert_eq!(topics::eth_bridge_finalized(), topics::eth_bridge_finalized());
        assert_eq!(topics::deposit_confirmed(), topics::deposit_confirmed());
    }

    #[test]
    fn test_txid_conversion_roundtrip() {
        let txid_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let bytes = txid_to_bytes32(txid_hex).unwrap();
        assert_eq!(bytes32_to_txid(&bytes), txid_hex);
    }

    #[test]
    fn test_invalid_txid_rejected() {
        assert!(txid_to_bytes32("invalid").is_err());
        assert!(txid_to_bytes32("0123456789abcdef").is_err()); // too short
    }

    #[test]
    fn test_recover_btc_address_from_initiate_withdrawal_calldata() {
        use alloy::sol_types::SolCall;
        // Real Hemi-mainnet initiateWithdrawal(uint32,string,uint256) calldata.
        // The btcAddress is `indexed` (keccak-hashed) in the WithdrawalInitiated event,
        // but the originating call carries it in cleartext — this proves we can recover it.
        let calldata = hex::decode(concat!(
            "6fff6d2e",
            "0000000000000000000000000000000000000000000000000000000000000006",
            "0000000000000000000000000000000000000000000000000000000000000060",
            "00000000000000000000000000000000000000000000000000000000000927c0",
            "000000000000000000000000000000000000000000000000000000000000002a",
            "6263317177716c3261756a32753536736b38376e327038673436346e6e737770",
            "367161726b637935746b00000000000000000000000000000000000000000000",
        ))
        .unwrap();
        let decoded =
            IBitcoinTunnelManager::initiateWithdrawalCall::abi_decode(&calldata, false).unwrap();
        assert_eq!(decoded.vaultIndex, 6);
        assert_eq!(decoded.btcAddress, "bc1qwql2auj2u56sk87n2p8g464nnswp6qarkcy5tk");
        assert_eq!(decoded.amount, alloy::primitives::U256::from(600_000u64));
    }
}
