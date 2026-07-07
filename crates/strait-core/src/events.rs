//! Raw ingestion events emitted by chain ingesters before the join engine processes them.
//!
//! These events are separate from domain types to maintain a clean boundary between
//! ingestion and join layers.

use bigdecimal::BigDecimal;
use bitcoin::Txid;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{Address, Asset, BitcoinAddress, BitcoinTxid, BlockHash, ChainAddress, TxHash};

/// Chain identifier for multi-chain support.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChainId {
    Bitcoin,
    Evm(u64),
}

/// Block reference with height, hash, and timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRef {
    pub height: u64,
    pub hash: String,
    pub timestamp: u64,
}

/// Direction of a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    BitcoinToEvm,
    EvmToBitcoin,
}

/// A transfer event representing value movement between chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub direction: TransferDirection,
    pub sender: String,
    pub receiver: String,
    pub amount: u64,
    pub source_txid: String,
    pub dest_txid: String,
    pub log_index: u64,
    pub metadata: serde_json::Value,
}

/// Withdrawal completion event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalComplete {
    pub nonce: u64,
    pub bitcoin_txid: String,
    pub evm_txid: String,
}

/// Type of event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    Transfer(Transfer),
    WithdrawalComplete(WithdrawalComplete),
}

/// A chain event with full context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEvent {
    pub chain_id: ChainId,
    pub block: BlockRef,
    pub txid: Txid,
    pub kind: EventKind,
}

/// Raw event from any chain ingester
#[derive(Debug, Clone)]
pub enum RawEvent {
    Bitcoin(BitcoinEvent),
    Hemi(HemiEvent),
    Ethereum(EthereumEvent),
}

/// Events from the Bitcoin chain ingester
#[derive(Debug, Clone)]
pub enum BitcoinEvent {
    /// A deposit to a tunnel custody address
    TunnelDeposit {
        txid: BitcoinTxid,
        vout: u32,
        to_address: BitcoinAddress,
        amount_sats: u64,
        op_return_data: Option<Vec<u8>>,
        /// Hemi EVM destination decoded from the deposit's OP_RETURN, if parseable.
        hemi_destination: Option<Address>,
        block_number: u64,
        block_hash: BlockHash,
        block_time: DateTime<Utc>,
    },
    /// A withdrawal spending a previously-seen tunnel UTXO
    TunnelWithdrawal {
        txid: BitcoinTxid,
        from_address: BitcoinAddress,
        to_address: BitcoinAddress,
        amount_sats: u64,
        block_number: u64,
        block_hash: BlockHash,
        block_time: DateTime<Utc>,
    },
    /// A chain reorganization was detected
    BlockReorg {
        old_tip: BlockHash,
        new_tip: BlockHash,
        depth: u32,
        affected_from_block: u64,
    },
}

/// Events from the Hemi EVM chain ingester
#[derive(Debug, Clone)]
pub enum HemiEvent {
    /// A tunnel mint (BTC or ETH deposited into Hemi)
    TunnelMint {
        tx_hash: TxHash,
        asset: Asset,
        amount: BigDecimal,
        to: Address,
        /// Present for BTC routes — links to the Bitcoin deposit
        source_txid: Option<BitcoinTxid>,
        block_number: u64,
        /// On-chain block timestamp (the real time the tx was mined).
        block_time: DateTime<Utc>,
        log_index: u32,
        /// Gas spent on this Hemi tx (wei = gasUsed * effectiveGasPrice), if fetched.
        gas_fee: Option<BigDecimal>,
    },
    /// A tunnel burn (assets being withdrawn from Hemi)
    TunnelBurn {
        tx_hash: TxHash,
        asset: Asset,
        amount: BigDecimal,
        from: Address,
        destination: ChainAddress,
        block_number: u64,
        /// On-chain block timestamp (the real time the tx was mined).
        block_time: DateTime<Utc>,
        log_index: u32,
        /// Gas spent on this Hemi tx (wei = gasUsed * effectiveGasPrice), if fetched.
        gas_fee: Option<BigDecimal>,
        /// BTC withdrawal uuid (vaultIndex << 32 | vaultUUID); `None` for ETH routes.
        /// The 4-byte vaultUUID is echoed in the Bitcoin payout's OP_RETURN.
        uuid: Option<u64>,
    },
    /// Emitted when PoPPayoutsV2.PayoutRoundExecuted fires on Hemi.
    ///
    /// Signals that all Hemi blocks in (keystone_block - 25, keystone_block]
    /// are now PoP-anchored on Bitcoin. The join engine fans this out to all
    /// in-flight transfers and advances those whose mint block is covered.
    PopKeystoneAnchored {
        hemi_tx_hash: TxHash,
        /// The Hemi keystone block (multiple of 25) that was anchored.
        keystone_block: u64,
        /// HEMI reward pool paid out (atomic units).
        reward_pool: u64,
        /// Aggregate PoP score. 0 = no publications but still anchored.
        pop_score: u64,
        block_number: u64,
        log_index: u32,
    },
    /// Emitted when a HEMI_TO_BTC withdrawal challenge succeeds — the operator
    /// failed to pay within the deadline and the user's hBTC was re-minted.
    /// The withdrawal is considered failed/refunded.
    WithdrawalChallengeSuccess {
        uuid: u64,
        withdrawer: Address,
        tx_hash: TxHash,
        block_number: u64,
        block_time: DateTime<Utc>,
        log_index: u32,
    },
    /// A chain reorganization was detected
    BlockReorg {
        old_tip: BlockHash,
        new_tip: BlockHash,
        depth: u32,
        affected_from_block: u64,
    },
}

/// Events from the Ethereum chain ingester
#[derive(Debug, Clone)]
pub enum EthereumEvent {
    /// A tunnel lock (ETH or ERC-20 locked for tunneling to Hemi)
    TunnelLock {
        tx_hash: TxHash,
        asset: Asset,
        amount: BigDecimal,
        from: Address,
        block_number: u64,
        /// On-chain block timestamp (the real time the tx was mined).
        block_time: DateTime<Utc>,
        log_index: u32,
        /// Gas spent on this L1 tx (wei = gasUsed * effectiveGasPrice), if fetched.
        gas_fee: Option<BigDecimal>,
    },
    /// A tunnel release (assets released from Hemi to Ethereum)
    TunnelRelease {
        tx_hash: TxHash,
        asset: Asset,
        amount: BigDecimal,
        to: Address,
        block_number: u64,
        /// On-chain block timestamp (the real time the tx was mined).
        block_time: DateTime<Utc>,
        log_index: u32,
        /// Gas spent on this L1 tx (wei = gasUsed * effectiveGasPrice), if fetched.
        gas_fee: Option<BigDecimal>,
    },
    /// Emitted when `proveWithdrawalTransaction` is called on OptimismPortal.
    ///
    /// Signals that the 1-day OP Stack challenge window has started for a
    /// HEMI_TO_ETH withdrawal. The transfer advances INITIATED → PROVING.
    /// `withdrawal_hash` is the bytes32 identifier from the event (not a tx hash).
    WithdrawalProven {
        withdrawal_hash: TxHash,
        from: Address,
        to: Address,
        tx_hash: TxHash,
        block_number: u64,
        block_time: DateTime<Utc>,
        log_index: u32,
    },
    /// A chain reorganization was detected
    BlockReorg {
        old_tip: BlockHash,
        new_tip: BlockHash,
        depth: u32,
        affected_from_block: u64,
    },
}

impl RawEvent {
    /// Get the chain this event originated from
    pub fn chain(&self) -> crate::types::Chain {
        match self {
            Self::Bitcoin(_) => crate::types::Chain::Bitcoin,
            Self::Hemi(_) => crate::types::Chain::Hemi,
            Self::Ethereum(_) => crate::types::Chain::Ethereum,
        }
    }

    /// Check if this is a reorg event
    pub fn is_reorg(&self) -> bool {
        matches!(
            self,
            Self::Bitcoin(BitcoinEvent::BlockReorg { .. })
                | Self::Hemi(HemiEvent::BlockReorg { .. })
                | Self::Ethereum(EthereumEvent::BlockReorg { .. })
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_event_chain() {
        let btc_event = RawEvent::Bitcoin(BitcoinEvent::TunnelDeposit {
            txid: BitcoinTxid([0; 32]),
            vout: 0,
            to_address: BitcoinAddress::new("bc1qtest"),
            amount_sats: 100000000,
            op_return_data: None,
            hemi_destination: None,
            block_number: 100,
            block_hash: BlockHash([0; 32]),
            block_time: Utc::now(),
        });
        assert_eq!(btc_event.chain(), crate::types::Chain::Bitcoin);
        assert!(!btc_event.is_reorg());

        let reorg_event = RawEvent::Bitcoin(BitcoinEvent::BlockReorg {
            old_tip: BlockHash([0; 32]),
            new_tip: BlockHash([1; 32]),
            depth: 2,
            affected_from_block: 98,
        });
        assert!(reorg_event.is_reorg());
    }
}
