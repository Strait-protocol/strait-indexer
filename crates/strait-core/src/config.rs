//! Configuration loading for Strait.
//!
//! Loads configuration from environment variables with `.env` file fallback.
//! Uses the `config` crate for layered configuration with sensible defaults.

use crate::error::{Result, StraitError};
use serde::Deserialize;

/// Top-level application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub bitcoin: BitcoinConfig,
    pub hemi: EvmChainConfig,
    pub ethereum: EvmChainConfig,
    pub database: DatabaseConfig,
    pub api: ApiConfig,
}

/// Bitcoin chain configuration
#[derive(Debug, Clone, Deserialize)]
pub struct BitcoinConfig {
    /// Bitcoin RPC URL. Legacy/optional: all Bitcoin state is currently read
    /// through the BitcoinKit precompile on Hemi, so no Bitcoin node is needed.
    #[serde(default)]
    pub rpc_url: String,
    /// Bitcoin RPC username (legacy/optional, see `rpc_url`).
    #[serde(default)]
    pub rpc_user: String,
    /// Bitcoin RPC password (legacy/optional, see `rpc_url`).
    #[serde(default)]
    pub rpc_password: String,
    /// Comma-separated list of tunnel custody addresses to watch
    pub tunnel_addresses: Vec<String>,
    /// Number of confirmations before considering a block final
    #[serde(default = "default_bitcoin_confirmation_depth")]
    pub confirmation_depth: u32,
    /// Poll interval in seconds for checking new blocks
    #[serde(default = "default_bitcoin_poll_interval")]
    pub poll_interval_secs: u64,
    /// Hemi RPC URL used exclusively by the custody watcher BitcoinKit precompile calls
    /// (getUTXOsForBitcoinAddress). Defaults to HEMI_RPC_URL if unset.
    /// Set via HEMI_BITCOIN_KIT_RPC_URL.
    #[serde(default)]
    pub bitcoin_kit_rpc_url: Option<String>,
    /// Separate Hemi RPC URL used exclusively by the payout watcher BitcoinKit calls
    /// (getUTXOsForBitcoinAddress, getTransactionByTxId, currentSweepUTXO).
    /// Defaults to bitcoin_kit_rpc_url → HEMI_RPC_URL if unset.
    /// Set via HEMI_PAYOUT_KIT_RPC_URL to spread load across endpoints.
    #[serde(default)]
    pub payout_kit_rpc_url: Option<String>,
    /// Ordered list of SimpleBitcoinVault contract addresses on Hemi (index 0 first).
    /// Used by the payout watcher to call currentSweepUTXO() per vault, detecting
    /// BTC payouts without depending on UTXO availability.
    /// Set via HEMI_VAULT_CONTRACTS (comma-separated EVM addresses).
    #[serde(default)]
    pub vault_contracts: Vec<String>,
}

/// EVM chain configuration (used for both Hemi and Ethereum)
#[derive(Debug, Clone, Deserialize)]
pub struct EvmChainConfig {
    /// RPC URL for the chain
    pub rpc_url: String,
    /// Chain ID (43111 for Hemi mainnet, 743111 for Hemi testnet, 1 for Ethereum mainnet, 11155111 for Sepolia)
    pub chain_id: u64,
    /// ETH/ERC-20 tunnel contract — L2StandardBridge (0x4200…0010 on Hemi),
    /// L1StandardBridgeProxy on Ethereum.
    pub tunnel_contract: String,
    /// BTC tunnel contract — BitcoinTunnelManager on Hemi (emits DepositConfirmed /
    /// WithdrawalInitiated). Watched in addition to `tunnel_contract`. None on Ethereum.
    #[serde(default)]
    pub btc_tunnel_contract: Option<String>,
    /// BitcoinKit precompile address on Hemi.
    /// Mainnet: 0x7007dd1C09527B92AEcd8Ae6570B73d09E0B8F12 (v1)
    /// Testnet: 0xeC9fa5daC1118963933e1A675a4EEA0009b7f215 (v0)
    /// Only relevant for the Hemi chain config — ignored for Ethereum.
    #[serde(default)]
    pub bitcoin_kit_contract: Option<String>,
    /// OptimismPortal contract address on Ethereum.
    /// When set, the Ethereum ingester watches for WithdrawalProven events to
    /// advance HEMI_TO_ETH withdrawals from INITIATED → PROVING (1-day window).
    /// Set via ETH_OPT_PORTAL_CONTRACT. Only relevant for the Ethereum chain config.
    #[serde(default)]
    pub opt_portal_contract: Option<String>,
    /// PoPPayoutsV2 contract on Hemi. When set, the Hemi ingester watches for
    /// PayoutRoundExecuted events to advance BTC→Hemi transfers from INITIATED → ANCHORED.
    /// Set via HEMI_POP_PAYOUTS_CONTRACT. Not relevant for the Ethereum chain config.
    #[serde(default)]
    pub pop_payouts_contract: Option<String>,
    /// Block number to start indexing from
    #[serde(default = "default_start_block")]
    pub start_block: u64,
    /// Number of confirmations before considering a block final
    #[serde(default = "default_evm_confirmation_depth")]
    pub confirmation_depth: u32,
    /// Poll interval in milliseconds for checking new blocks
    #[serde(default = "default_evm_poll_interval")]
    pub poll_interval_ms: u64,
    /// Maximum block range per eth_getLogs request. Free-tier providers (e.g.
    /// Alchemy free) cap this at 10; paid tiers typically allow 500–2000.
    #[serde(default = "default_log_range")]
    pub log_range: u64,
}

/// Database configuration
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// PostgreSQL connection string
    pub url: String,
    /// Maximum number of database connections
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

/// API server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    /// Host to bind the API server to
    #[serde(default = "default_api_host")]
    pub host: String,
    /// Port to bind the API server to
    #[serde(default = "default_api_port")]
    pub port: u16,
}

// ============================================================================
// Confirmed timing constants (from Hemi documentation)
// ============================================================================

/// Hemi keystone frequency — every 25 Hemi blocks a keystone is produced.
/// Must match PoPPayoutsV2.KEYSTONE_FREQUENCY = 25.
pub const KEYSTONE_FREQUENCY: u32 = 25;

/// Bitcoin tunnel: 6 confirmations required before minting on Hemi (~1 hour).
pub const BTC_CONFIRMATION_DEPTH: u32 = 6;

/// Hemi: faster finality, 3 confirmations sufficient.
pub const HEMI_CONFIRMATION_DEPTH: u32 = 3;

/// Ethereum: 12 confirmations for standard finality.
pub const ETH_CONFIRMATION_DEPTH: u32 = 12;

/// ETH→Hemi deposit: representative token mints within ~2 minutes of Ethereum lock.
/// Used as the match window for pairing TunnelLock with TunnelMint events.
pub const ETH_DEPOSIT_MATCH_WINDOW_SECS: i64 = 300; // 5 min — 2.5x the observed ~2 min

/// Hemi→ETH withdrawal: up to ~40 minutes to achieve Bitcoin finality,
/// plus up to 24 hours for proof submission. The join engine keeps a
/// withdrawal in ANCHORED state for this long before marking it FAILED.
pub const ETH_WITHDRAWAL_PROOF_WINDOW_SECS: i64 = 90_000; // 25 hours

/// BTC→Hemi deposit: 6 Bitcoin confirmations at ~10 min/block = ~1 hour.
/// Used as the match window for pairing TunnelDeposit with TunnelMint events.
pub const BTC_DEPOSIT_MATCH_WINDOW_SECS: i64 = 7_200; // 2 hours — 2x the observed ~1 hour

/// Hemi→BTC withdrawal: up to ~12 hours for custodian verification.
pub const BTC_WITHDRAWAL_WINDOW_SECS: i64 = 50_400; // 14 hours — safety margin

// ============================================================================
// Default values
// ============================================================================

fn default_bitcoin_confirmation_depth() -> u32 {
    BTC_CONFIRMATION_DEPTH
}

fn default_bitcoin_poll_interval() -> u64 {
    60
}

fn default_start_block() -> u64 {
    0
}

fn default_evm_confirmation_depth() -> u32 {
    ETH_CONFIRMATION_DEPTH
}

fn default_evm_poll_interval() -> u64 {
    1000
}

fn default_log_range() -> u64 {
    100
}

fn default_max_connections() -> u32 {
    10
}

fn default_api_host() -> String {
    "0.0.0.0".to_string()
}

fn default_api_port() -> u16 {
    8080
}

// ============================================================================
// Environment helpers
// ============================================================================

/// Read an environment variable as a String, empty if unset.
fn env_str(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

/// Read an environment variable as `Some(String)`, `None` if unset or empty.
fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Read and parse an environment variable, falling back to `default` if unset
/// or unparseable.
fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

// ============================================================================
// Configuration loading
// ============================================================================

impl AppConfig {
    /// Load configuration from environment variables (with `.env` fallback).
    ///
    /// Reads the flat, conventionally-named variables documented in `.env.example`
    /// (e.g. `HEMI_RPC_URL`, `DATABASE_URL`, `BITCOIN_TUNNEL_ADDRESSES`). Optional
    /// values fall back to the defaults defined in this module.
    pub fn load() -> Result<Self> {
        // Load .env file if it exists (ignore if not found)
        let _ = dotenvy::dotenv();

        let config = AppConfig {
            bitcoin: BitcoinConfig {
                rpc_url: env_str("BITCOIN_RPC_URL"),
                rpc_user: env_str("BITCOIN_RPC_USER"),
                rpc_password: env_str("BITCOIN_RPC_PASSWORD"),
                tunnel_addresses: env_str("BITCOIN_TUNNEL_ADDRESSES")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                confirmation_depth: env_parse(
                    "BITCOIN_CONFIRMATION_DEPTH",
                    default_bitcoin_confirmation_depth(),
                ),
                poll_interval_secs: env_parse(
                    "BITCOIN_POLL_INTERVAL_SECS",
                    default_bitcoin_poll_interval(),
                ),
                bitcoin_kit_rpc_url: env_opt("HEMI_BITCOIN_KIT_RPC_URL"),
                payout_kit_rpc_url: env_opt("HEMI_PAYOUT_KIT_RPC_URL"),
                vault_contracts: env_str("HEMI_VAULT_CONTRACTS")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            },
            hemi: EvmChainConfig {
                rpc_url: env_str("HEMI_RPC_URL"),
                chain_id: env_parse("HEMI_CHAIN_ID", 43111),
                tunnel_contract: env_str("HEMI_TUNNEL_CONTRACT"),
                btc_tunnel_contract: env_opt("HEMI_BTC_TUNNEL_CONTRACT"),
                bitcoin_kit_contract: env_opt("HEMI_BITCOIN_KIT_CONTRACT"),
                opt_portal_contract: None,
                pop_payouts_contract: env_opt("HEMI_POP_PAYOUTS_CONTRACT"),
                start_block: env_parse("HEMI_START_BLOCK", default_start_block()),
                confirmation_depth: env_parse(
                    "HEMI_CONFIRMATION_DEPTH",
                    HEMI_CONFIRMATION_DEPTH,
                ),
                poll_interval_ms: env_parse("HEMI_POLL_INTERVAL_MS", default_evm_poll_interval()),
                log_range: env_parse("HEMI_LOG_RANGE", default_log_range()),
            },
            ethereum: EvmChainConfig {
                rpc_url: env_str("ETH_RPC_URL"),
                chain_id: env_parse("ETH_CHAIN_ID", 1),
                tunnel_contract: env_str("ETH_TUNNEL_CONTRACT"),
                btc_tunnel_contract: None,
                bitcoin_kit_contract: None,
                opt_portal_contract: env_opt("ETH_OPT_PORTAL_CONTRACT"),
                pop_payouts_contract: None,
                start_block: env_parse("ETH_START_BLOCK", default_start_block()),
                confirmation_depth: env_parse(
                    "ETH_CONFIRMATION_DEPTH",
                    default_evm_confirmation_depth(),
                ),
                poll_interval_ms: env_parse("ETH_POLL_INTERVAL_MS", default_evm_poll_interval()),
                log_range: env_parse("ETH_LOG_RANGE", default_log_range()),
            },
            database: DatabaseConfig {
                url: env_str("DATABASE_URL"),
                max_connections: env_parse("DATABASE_MAX_CONNECTIONS", default_max_connections()),
            },
            api: ApiConfig {
                host: env_opt("API_HOST").unwrap_or_else(default_api_host),
                port: env_parse("API_PORT", default_api_port()),
            },
        };

        // Validate required fields
        config.validate()?;

        Ok(config)
    }

    /// Validate that all required configuration is present.
    ///
    /// The Bitcoin RPC credentials are deliberately NOT required: all Bitcoin
    /// state is read through the BitcoinKit precompile on Hemi.
    fn validate(&self) -> Result<()> {
        if self.hemi.rpc_url.is_empty() {
            return Err(StraitError::MissingConfig(
                "HEMI_RPC_URL is required".to_string(),
            ));
        }
        if self.hemi.tunnel_contract.is_empty() {
            return Err(StraitError::MissingConfig(
                "HEMI_TUNNEL_CONTRACT is required".to_string(),
            ));
        }
        if self.ethereum.rpc_url.is_empty() {
            return Err(StraitError::MissingConfig(
                "ETH_RPC_URL is required".to_string(),
            ));
        }
        if self.ethereum.tunnel_contract.is_empty() {
            return Err(StraitError::MissingConfig(
                "ETH_TUNNEL_CONTRACT is required".to_string(),
            ));
        }
        if self.database.url.is_empty() {
            return Err(StraitError::MissingConfig(
                "DATABASE_URL is required".to_string(),
            ));
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        assert_eq!(default_bitcoin_confirmation_depth(), 6);
        assert_eq!(default_bitcoin_poll_interval(), 60);
        assert_eq!(default_start_block(), 0);
        assert_eq!(default_evm_confirmation_depth(), 12);
        assert_eq!(default_evm_poll_interval(), 1000);
        assert_eq!(default_max_connections(), 10);
        assert_eq!(default_api_host(), "0.0.0.0");
        assert_eq!(default_api_port(), 8080);
    }

    #[test]
    fn test_config_validation() {
        let mut config = AppConfig {
            bitcoin: BitcoinConfig {
                rpc_url: String::new(),
                rpc_user: "user".to_string(),
                rpc_password: "pass".to_string(),
                tunnel_addresses: vec![],
                confirmation_depth: 6,
                poll_interval_secs: 10,
                bitcoin_kit_rpc_url: None,
                payout_kit_rpc_url: None,
                vault_contracts: vec![],
            },
            hemi: EvmChainConfig {
                rpc_url: "https://testnet.rpc.hemi.network/rpc".to_string(),
                chain_id: 743111,
                tunnel_contract: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
                btc_tunnel_contract: Some("0x8221CFD3Eca3c5F9FA27b2AE774151642f1C449e".to_string()),
                bitcoin_kit_contract: Some("0xeC9fa5daC1118963933e1A675a4EEA0009b7f215".to_string()),
                opt_portal_contract: None,
                pop_payouts_contract: Some("0x4a3b61C586DB4CD219E85aC0697b66916c7457AB".to_string()),
                start_block: 0,
                confirmation_depth: 3,
                poll_interval_ms: 1000,
                log_range: 100,
            },
            ethereum: EvmChainConfig {
                rpc_url: "https://eth-sepolia.g.alchemy.com/v2/test".to_string(),
                chain_id: 11155111,
                tunnel_contract: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
                btc_tunnel_contract: None,
                bitcoin_kit_contract: None,
                opt_portal_contract: None,
                pop_payouts_contract: None,
                start_block: 0,
                confirmation_depth: 12,
                poll_interval_ms: 1000,
                log_range: 100,
            },
            database: DatabaseConfig {
                url: "postgres://postgres:password@localhost:5432/strait".to_string(),
                max_connections: 10,
            },
            api: ApiConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
            },
        };

        // Bitcoin RPC credentials are legacy/optional — everything is read via
        // BitcoinKit on Hemi — so an empty bitcoin.rpc_url must NOT fail.
        assert!(config.validate().is_ok());

        // A missing Hemi RPC URL is a hard error.
        config.hemi.rpc_url = String::new();
        assert!(config.validate().is_err());
    }
}
