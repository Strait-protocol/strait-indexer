//! Core domain types for Strait tunnel indexer.
//!
//! These types represent the unified cross-chain data model that every other crate depends on.

use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

// ============================================================================
// Newtype wrappers — never use raw strings for chain-specific identifiers
// ============================================================================

/// EVM transaction hash (32 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TxHash(pub [u8; 32]);

impl TxHash {
    /// Create from a hex string (with or without 0x prefix)
    pub fn from_hex(hex: &str) -> Result<Self, hex::FromHexError> {
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        let bytes = hex::decode(hex)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Convert to hex string with 0x prefix
    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.0))
    }
}

impl fmt::Display for TxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl From<[u8; 32]> for TxHash {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&str> for TxHash {
    type Error = hex::FromHexError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_hex(s)
    }
}

/// Bitcoin transaction ID (32 bytes, little-endian stored)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BitcoinTxid(pub [u8; 32]);

impl BitcoinTxid {
    /// Create from a hex string (Bitcoin uses little-endian display)
    pub fn from_hex(hex: &str) -> Result<Self, hex::FromHexError> {
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        let bytes = hex::decode(hex)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Convert to hex string (little-endian, as Bitcoin displays)
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Display for BitcoinTxid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl From<[u8; 32]> for BitcoinTxid {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&str> for BitcoinTxid {
    type Error = hex::FromHexError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_hex(s)
    }
}

/// Block hash (32 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockHash(pub [u8; 32]);

impl BlockHash {
    /// Create from a hex string
    pub fn from_hex(hex: &str) -> Result<Self, hex::FromHexError> {
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        let bytes = hex::decode(hex)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Display for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl From<[u8; 32]> for BlockHash {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&str> for BlockHash {
    type Error = hex::FromHexError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_hex(s)
    }
}

/// EVM address (20 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Address(pub [u8; 20]);

impl Address {
    /// Create from a hex string (with or without 0x prefix)
    pub fn from_hex(hex: &str) -> Result<Self, hex::FromHexError> {
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        let bytes = hex::decode(hex)?;
        if bytes.len() != 20 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Convert to hex string with 0x prefix
    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.0))
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl From<[u8; 20]> for Address {
    fn from(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&str> for Address {
    type Error = hex::FromHexError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_hex(s)
    }
}

/// Bitcoin address (bech32 or legacy) — kept as String due to variable length
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BitcoinAddress(pub String);

impl BitcoinAddress {
    pub fn new(address: impl Into<String>) -> Self {
        Self(address.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Create from bytes (converts to string)
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, std::str::Utf8Error> {
        let s = std::str::from_utf8(bytes)?;
        Ok(Self(s.to_string()))
    }
}

impl fmt::Display for BitcoinAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for BitcoinAddress {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for BitcoinAddress {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

// ============================================================================
// Core domain objects
// ============================================================================

/// The central domain object representing a complete tunnel transfer lifecycle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelTransfer {
    pub id: Uuid,
    pub asset: Asset,
    pub direction: TunnelDirection,
    pub route: TunnelRoute,
    pub amount: BigDecimal,
    pub sender: ChainAddress,
    pub recipient: ChainAddress,
    pub status: TunnelStatus,
    pub initiated_at: DateTime<Utc>,
    pub finalized_at: Option<DateTime<Utc>>,
    pub source_tx: ChainTransaction,
    pub destination_tx: Option<ChainTransaction>,
    /// Network fee paid on the source leg, in that chain's atomic unit (wei or sats).
    pub source_fee: Option<BigDecimal>,
    /// Network fee paid on the destination leg, in that chain's atomic unit (wei or sats).
    pub dest_fee: Option<BigDecimal>,
    /// BTC withdrawal uuid (vaultIndex << 32 | vaultUUID); `None` for other routes.
    /// Used to match the Bitcoin payout (which echoes the vaultUUID in its OP_RETURN).
    pub withdrawal_uuid: Option<u64>,
    /// Whether this transfer has been PoP-anchored to Bitcoin.
    pub pop_anchored: bool,
    /// The Hemi keystone block (multiple of 25) that anchored this transfer.
    pub pop_keystone_block: Option<u64>,
    /// Aggregate PoP score of the anchoring keystone. 0 = no publications but still anchored.
    pub pop_score: Option<u64>,
    /// When the anchoring keystone was observed by Strait.
    pub pop_anchored_at: Option<DateTime<Utc>>,
    pub reorg_events: Vec<ReorgEvent>,
}

/// Tunnel transfer status lifecycle
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TunnelStatus {
    Initiated,
    /// PoP proof observed for BTC routes; relay observed for ETH routes
    Anchored,
    Finalized,
    Failed {
        reason: String,
    },
    Reorged {
        retracted_at: DateTime<Utc>,
    },
}

impl fmt::Display for TunnelStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initiated => write!(f, "INITIATED"),
            Self::Anchored => write!(f, "ANCHORED"),
            Self::Finalized => write!(f, "FINALIZED"),
            Self::Failed { reason } => write!(f, "FAILED: {}", reason),
            Self::Reorged { retracted_at } => write!(f, "REORGED at {}", retracted_at),
        }
    }
}

/// Direction of the tunnel transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TunnelDirection {
    /// Into Hemi (from Bitcoin or Ethereum)
    In,
    /// Out of Hemi (to Bitcoin or Ethereum)
    Out,
}

impl fmt::Display for TunnelDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::In => write!(f, "IN"),
            Self::Out => write!(f, "OUT"),
        }
    }
}

/// The specific route a tunnel transfer takes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TunnelRoute {
    BtcToHemi,
    HemiToBtc,
    EthToHemi,
    HemiToEth,
}

impl fmt::Display for TunnelRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BtcToHemi => write!(f, "BTC_TO_HEMI"),
            Self::HemiToBtc => write!(f, "HEMI_TO_BTC"),
            Self::EthToHemi => write!(f, "ETH_TO_HEMI"),
            Self::HemiToEth => write!(f, "HEMI_TO_ETH"),
        }
    }
}

/// Asset type being tunneled
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum Asset {
    Btc,
    Eth,
    Erc20 {
        contract: Address,
        symbol: String,
        decimals: u8,
    },
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Btc => write!(f, "BTC"),
            Self::Eth => write!(f, "ETH"),
            Self::Erc20 { symbol, .. } => write!(f, "{}", symbol),
        }
    }
}

/// Chain-specific address (EVM or Bitcoin)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "chain")]
pub enum ChainAddress {
    Evm(Address),
    Bitcoin(BitcoinAddress),
}

impl fmt::Display for ChainAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evm(addr) => write!(f, "{}", addr),
            Self::Bitcoin(addr) => write!(f, "{}", addr),
        }
    }
}

/// A blockchain transaction record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainTransaction {
    pub chain: Chain,
    pub hash: ChainTxHash,
    pub block_number: u64,
    pub block_hash: BlockHash,
    pub timestamp: DateTime<Utc>,
    pub confirmations: u32,
}

/// Chain identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Chain {
    Bitcoin,
    Hemi,
    Ethereum,
}

impl fmt::Display for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bitcoin => write!(f, "BITCOIN"),
            Self::Hemi => write!(f, "HEMI"),
            Self::Ethereum => write!(f, "ETHEREUM"),
        }
    }
}

/// Chain-specific transaction hash (Bitcoin or EVM)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "chain")]
pub enum ChainTxHash {
    Bitcoin(BitcoinTxid),
    Evm(TxHash),
}

impl fmt::Display for ChainTxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bitcoin(txid) => write!(f, "{}", txid),
            Self::Evm(hash) => write!(f, "{}", hash),
        }
    }
}

/// A PoP anchoring record from PoPPayoutsV2.PayoutRoundExecuted.
///
/// One anchor per Hemi keystone (every 25 blocks). When observed, all Hemi
/// blocks in `(keystone_block - 25, keystone_block]` are Bitcoin-anchored.
/// A `pop_score` of 0 means no miners published but the round still anchors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopAnchor {
    /// The Hemi keystone block anchored (always a multiple of 25).
    pub keystone_block: u64,
    /// Aggregate PoP score across all publications of this keystone.
    pub pop_score: u64,
    /// HEMI reward pool paid out for this round (in atomic units).
    pub reward_pool: u64,
    /// When Strait observed this anchoring event.
    pub observed_at: DateTime<Utc>,
}

impl PopAnchor {
    /// Keystone frequency, matching PoPPayoutsV2.KEYSTONE_FREQUENCY = 25.
    pub const KEYSTONE_FREQUENCY: u64 = 25;

    /// Returns true if `hemi_block` falls within this keystone's anchoring window.
    ///
    /// Window: `(keystone_block - 25, keystone_block]`
    /// — exclusive lower bound, inclusive upper bound.
    pub fn covers_block(&self, hemi_block: u64) -> bool {
        let window_start = self.keystone_block.saturating_sub(Self::KEYSTONE_FREQUENCY);
        hemi_block > window_start && hemi_block <= self.keystone_block
    }
}

/// Record of a chain reorganization event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorgEvent {
    pub chain: Chain,
    pub depth: u32,
    pub old_tip: BlockHash,
    pub new_tip: BlockHash,
    pub affected_from_block: u64,
    pub detected_at: DateTime<Utc>,
}

/// Block reference with height, hash, and timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRef {
    pub height: u64,
    pub hash: String,
    pub timestamp: u64,
}

/// Satoshi amount (Bitcoin's smallest unit)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Satoshi(pub u64);

impl Satoshi {
    pub fn from_u64(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for Satoshi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type alias for BitcoinTxid as Txid
pub type Txid = BitcoinTxid;

/// Type alias for Address as EvmAddress
pub type EvmAddress = Address;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_hash_from_hex() {
        let hex = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let hash = TxHash::from_hex(hex).unwrap();
        assert_eq!(hash.to_hex(), hex);

        // Without 0x prefix
        let hex_no_prefix = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let hash2 = TxHash::from_hex(hex_no_prefix).unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_tx_hash_invalid_length() {
        let result = TxHash::from_hex("0x1234");
        assert!(result.is_err());
    }

    #[test]
    fn test_bitcoin_txid_from_hex() {
        let hex = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let txid = BitcoinTxid::from_hex(hex).unwrap();
        assert_eq!(txid.to_hex(), hex);
    }

    #[test]
    fn test_address_from_hex() {
        let hex = "0x1234567890abcdef1234567890abcdef12345678";
        let addr = Address::from_hex(hex).unwrap();
        assert_eq!(addr.to_hex(), hex);
    }

    #[test]
    fn test_address_invalid_length() {
        let result = Address::from_hex("0x1234");
        assert!(result.is_err());
    }

    #[test]
    fn test_bitcoin_address() {
        let addr = BitcoinAddress::new("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh");
        assert_eq!(addr.as_str(), "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh");
        assert_eq!(
            addr.to_string(),
            "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh"
        );
    }

    #[test]
    fn test_tunnel_status_display() {
        assert_eq!(TunnelStatus::Initiated.to_string(), "INITIATED");
        assert_eq!(TunnelStatus::Anchored.to_string(), "ANCHORED");
        assert_eq!(TunnelStatus::Finalized.to_string(), "FINALIZED");
        assert_eq!(
            TunnelStatus::Failed {
                reason: "timeout".to_string()
            }
            .to_string(),
            "FAILED: timeout"
        );
    }

    #[test]
    fn test_tunnel_direction_display() {
        assert_eq!(TunnelDirection::In.to_string(), "IN");
        assert_eq!(TunnelDirection::Out.to_string(), "OUT");
    }

    #[test]
    fn test_tunnel_route_display() {
        assert_eq!(TunnelRoute::BtcToHemi.to_string(), "BTC_TO_HEMI");
        assert_eq!(TunnelRoute::HemiToBtc.to_string(), "HEMI_TO_BTC");
        assert_eq!(TunnelRoute::EthToHemi.to_string(), "ETH_TO_HEMI");
        assert_eq!(TunnelRoute::HemiToEth.to_string(), "HEMI_TO_ETH");
    }

    #[test]
    fn test_asset_display() {
        assert_eq!(Asset::Btc.to_string(), "BTC");
        assert_eq!(Asset::Eth.to_string(), "ETH");
        assert_eq!(
            Asset::Erc20 {
                contract: Address([0; 20]),
                symbol: "USDC".to_string(),
                decimals: 6
            }
            .to_string(),
            "USDC"
        );
    }

    #[test]
    fn test_chain_display() {
        assert_eq!(Chain::Bitcoin.to_string(), "BITCOIN");
        assert_eq!(Chain::Hemi.to_string(), "HEMI");
        assert_eq!(Chain::Ethereum.to_string(), "ETHEREUM");
    }

    #[test]
    fn test_serde_roundtrip() {
        let tx_hash =
            TxHash::from_hex("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
                .unwrap();

        let json = serde_json::to_string(&tx_hash).unwrap();
        let deserialized: TxHash = serde_json::from_str(&json).unwrap();
        assert_eq!(tx_hash, deserialized);
    }
}
