//! Cross-chain event matcher.
//!
//! Matching rules:
//! - BTC → Hemi: `BitcoinEvent::TunnelDeposit` matched with `HemiEvent::TunnelMint`
//!   via `source_txid` on the mint (primary) or address+amount+time window (fallback).
//! - ETH → Hemi: `EthereumEvent::TunnelLock` matched with `HemiEvent::TunnelMint`
//!   via address+amount+time window.
//! - Hemi → ETH: `HemiEvent::TunnelBurn` matched with `EthereumEvent::TunnelRelease`.

use std::collections::HashMap;
use chrono::{DateTime, Utc};
use strait_core::{
    config::{
        BTC_DEPOSIT_MATCH_WINDOW_SECS, ETH_DEPOSIT_MATCH_WINDOW_SECS,
        ETH_WITHDRAWAL_PROOF_WINDOW_SECS,
    },
    events::{BitcoinEvent, EthereumEvent, HemiEvent, RawEvent},
    types::BitcoinTxid,
};

/// Configuration for matching behavior.
#[derive(Debug, Clone)]
pub struct MatcherConfig {
    /// Maximum time gap allowed between a BTC deposit and its Hemi mint (seconds).
    /// Confirmed: Bitcoin tunnel takes ~1 hour (6 confirmations). Default: 2 hours.
    pub btc_deposit_match_window_secs: i64,
    /// Maximum time gap allowed between an ETH lock and its Hemi mint (seconds).
    /// Confirmed: ETH tunnel takes ~2 minutes. Default: 5 minutes.
    pub eth_deposit_match_window_secs: i64,
    /// Tolerance for amount matching (fraction, e.g. 0.01 = 1%).
    /// Accounts for fees that may reduce the received amount slightly.
    pub amount_tolerance: f64,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self {
            btc_deposit_match_window_secs: BTC_DEPOSIT_MATCH_WINDOW_SECS,
            eth_deposit_match_window_secs: ETH_DEPOSIT_MATCH_WINDOW_SECS,
            amount_tolerance: 0.01,
        }
    }
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct MatchKey {
    address: String,
    asset: String,
}

#[derive(Debug, Clone)]
struct PendingEvent {
    raw: RawEvent,
    address: String,
    asset: String,
    amount: f64,
    timestamp: DateTime<Utc>,
    source_txid: Option<BitcoinTxid>,
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub source: RawEvent,
    pub destination: RawEvent,
    pub direction: MatchDirection,
    pub amount: f64,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchDirection {
    BtcToHemi,
    EthToHemi,
    HemiToEth,
}

pub struct EventMatcher {
    config: MatcherConfig,
    pending_btc_deposits: HashMap<MatchKey, Vec<PendingEvent>>,
    pending_eth_locks: HashMap<MatchKey, Vec<PendingEvent>>,
    pending_hemi_mints_by_txid: HashMap<BitcoinTxid, Vec<PendingEvent>>,
    pending_hemi_mints_by_addr: HashMap<MatchKey, Vec<PendingEvent>>,
    pending_hemi_burns: HashMap<MatchKey, Vec<PendingEvent>>,
    /// Latest on-chain event timestamp observed — the clock used for pruning,
    /// so pruning works during historical backfill as well as live tailing.
    latest_seen: Option<DateTime<Utc>>,
}

impl EventMatcher {
    pub fn new(config: MatcherConfig) -> Self {
        Self {
            config,
            pending_btc_deposits: HashMap::new(),
            pending_eth_locks: HashMap::new(),
            pending_hemi_mints_by_txid: HashMap::new(),
            pending_hemi_mints_by_addr: HashMap::new(),
            pending_hemi_burns: HashMap::new(),
            latest_seen: None,
        }
    }

    /// Process a raw event and return a MatchResult if one is found.
    pub fn process_event(&mut self, event: RawEvent) -> Option<MatchResult> {
        // Advance the pruning clock and expire pending events that can no longer
        // match anything — without this the pending maps grow without bound.
        if let Some(t) = event_block_time(&event) {
            let now = self.latest_seen.map_or(t, |seen| seen.max(t));
            self.latest_seen = Some(now);
            self.prune_stale(now);
        }

        // Clone so we can destructure for matching while still owning the original.
        match event.clone() {
            RawEvent::Bitcoin(btc) => self.process_btc_event(event, &btc),
            RawEvent::Hemi(hemi)   => self.process_hemi_event(event, &hemi),
            RawEvent::Ethereum(eth) => self.process_eth_event(event, &eth),
        }
    }

    /// Drop pending events older than their route's match window (with 2x margin;
    /// burns wait out the OP Stack proof window). Expired entries could never
    /// match again — the time-window check would reject them anyway.
    fn prune_stale(&mut self, now: DateTime<Utc>) {
        fn prune(map: &mut HashMap<MatchKey, Vec<PendingEvent>>, now: DateTime<Utc>, max_age: i64) {
            map.retain(|_, v| {
                v.retain(|e| (now - e.timestamp).num_seconds() <= max_age);
                !v.is_empty()
            });
        }
        let btc_age = self.config.btc_deposit_match_window_secs * 2;
        let eth_age = self.config.eth_deposit_match_window_secs * 2;
        // Burns pair with an L1 release that lands after the ~1-day challenge window.
        let burn_age = ETH_WITHDRAWAL_PROOF_WINDOW_SECS * 2;
        prune(&mut self.pending_btc_deposits, now, btc_age);
        prune(&mut self.pending_eth_locks, now, eth_age);
        prune(&mut self.pending_hemi_mints_by_addr, now, eth_age);
        prune(&mut self.pending_hemi_burns, now, burn_age);
        self.pending_hemi_mints_by_txid.retain(|_, v| {
            v.retain(|e| (now - e.timestamp).num_seconds() <= btc_age);
            !v.is_empty()
        });
    }

    fn process_btc_event(&mut self, event: RawEvent, btc: &BitcoinEvent) -> Option<MatchResult> {
        let BitcoinEvent::TunnelDeposit { to_address, amount_sats, txid, block_time, .. } = btc
        else {
            return None;
        };

        let address = to_address.to_string();
        let amount  = *amount_sats as f64;
        let tolerance = self.config.amount_tolerance;

        let pending = PendingEvent {
            raw: event,
            address: address.clone(),
            asset: "BTC".to_string(),
            amount,
            timestamp: *block_time,
            source_txid: Some(txid.clone()),
        };

        // Try direct txid match against pending Hemi mints.
        if let Some(mints) = self.pending_hemi_mints_by_txid.get_mut(txid) {
            if let Some(pos) = mints.iter().position(|m| amounts_match(pending.amount, m.amount, tolerance)) {
                let mint = mints.remove(pos);
                if mints.is_empty() { self.pending_hemi_mints_by_txid.remove(txid); }
                return Some(MatchResult {
                    source: pending.raw,
                    destination: mint.raw,
                    direction: MatchDirection::BtcToHemi,
                    amount: pending.amount,
                    address: pending.address,
                });
            }
        }

        // Fallback: address+asset match.
        let key = MatchKey { address: address.clone(), asset: "BTC".to_string() };
        if let Some(mints) = self.pending_hemi_mints_by_addr.get_mut(&key) {
            let gap = self.config.btc_deposit_match_window_secs;
            if let Some(pos) = mints.iter().position(|m| {
                amounts_match(pending.amount, m.amount, tolerance)
                    && within_time_window(pending.timestamp, m.timestamp, gap)
            }) {
                let mint = mints.remove(pos);
                if mints.is_empty() { self.pending_hemi_mints_by_addr.remove(&key); }
                return Some(MatchResult {
                    source: pending.raw,
                    destination: mint.raw,
                    direction: MatchDirection::BtcToHemi,
                    amount: pending.amount,
                    address: pending.address,
                });
            }
        }

        self.pending_btc_deposits.entry(key).or_default().push(pending);
        None
    }

    fn process_hemi_event(&mut self, event: RawEvent, hemi: &HemiEvent) -> Option<MatchResult> {
        match hemi {
            HemiEvent::TunnelMint { source_txid, to, asset, amount, block_time, .. } => {
                let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
                let addr_str   = hex::encode(to.0);
                let asset_str  = format!("{:?}", asset);
                let tolerance  = self.config.amount_tolerance;

                let pending = PendingEvent {
                    raw: event,
                    address: addr_str.clone(),
                    asset: asset_str.clone(),
                    amount: amount_f64,
                    // On-chain block time, not ingestion time — keeps the time-window
                    // check meaningful during historical backfill.
                    timestamp: *block_time,
                    source_txid: source_txid.clone(),
                };

                // Primary: txid-based match (BTC route)
                if let Some(ref src_txid) = source_txid {
                    // Search pending BTC deposits for one with this txid
                    let deposit = self.pending_btc_deposits
                        .values_mut()
                        .flat_map(|v| v.iter().enumerate())
                        .find(|(_, d)| d.source_txid.as_ref() == Some(src_txid)
                            && amounts_match(d.amount, amount_f64, tolerance))
                        .map(|(i, d)| (i, d.address.clone(), d.asset.clone()));

                    if let Some((idx, dep_addr, dep_asset)) = deposit {
                        let key = MatchKey { address: dep_addr, asset: dep_asset };
                        if let Some(deposits) = self.pending_btc_deposits.get_mut(&key) {
                            let dep = deposits.remove(idx);
                            if deposits.is_empty() { self.pending_btc_deposits.remove(&key); }
                            return Some(MatchResult {
                                source: dep.raw,
                                destination: pending.raw,
                                direction: MatchDirection::BtcToHemi,
                                amount: amount_f64,
                                address: pending.address,
                            });
                        }
                    }

                    self.pending_hemi_mints_by_txid
                        .entry(src_txid.clone())
                        .or_default()
                        .push(pending);
                } else {
                    // Fallback: addr+asset match (ETH route)
                    let key = MatchKey { address: addr_str.clone(), asset: asset_str.clone() };
                    let gap = self.config.eth_deposit_match_window_secs;
                    if let Some(locks) = self.pending_eth_locks.get_mut(&key) {
                        if let Some(pos) = locks.iter().position(|l| {
                            amounts_match(l.amount, amount_f64, tolerance)
                                && within_time_window(l.timestamp, pending.timestamp, gap)
                        }) {
                            let lock = locks.remove(pos);
                            if locks.is_empty() { self.pending_eth_locks.remove(&key); }
                            return Some(MatchResult {
                                source: lock.raw,
                                destination: pending.raw,
                                direction: MatchDirection::EthToHemi,
                                amount: amount_f64,
                                address: pending.address,
                            });
                        }
                    }
                    self.pending_hemi_mints_by_addr.entry(key).or_default().push(pending);
                }
                None
            }

            // Hemi → ETH: TunnelBurn waits for a later EthereumEvent::TunnelRelease.
            // A burn is always the *first* leg of a withdrawal (the L1 release
            // follows the OP Stack challenge window), so there is nothing to match
            // against yet — never pair a burn with a pending TunnelLock (an L1
            // *deposit*), which would corrupt the withdrawal's source leg.
            HemiEvent::TunnelBurn { from, asset, amount, block_time, .. } => {
                let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
                let addr_str   = hex::encode(from.0);
                let asset_str  = format!("{:?}", asset);
                let key = MatchKey { address: addr_str.clone(), asset: asset_str.clone() };

                self.pending_hemi_burns.entry(key).or_default().push(PendingEvent {
                    raw: event,
                    address: addr_str,
                    asset: asset_str,
                    amount: amount_f64,
                    timestamp: *block_time,
                    source_txid: None,
                });
                None
            }

            _ => None,
        }
    }

    fn process_eth_event(&mut self, event: RawEvent, eth: &EthereumEvent) -> Option<MatchResult> {
        match eth {
            EthereumEvent::TunnelLock { from, asset, amount, block_time, .. } => {
                let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
                let addr_str   = hex::encode(from.0);
                let asset_str  = format!("{:?}", asset);
                let key = MatchKey { address: addr_str.clone(), asset: asset_str.clone() };
                let tolerance  = self.config.amount_tolerance;
                let gap        = self.config.eth_deposit_match_window_secs;

                let pending = PendingEvent {
                    raw: event,
                    address: addr_str,
                    asset: asset_str,
                    amount: amount_f64,
                    timestamp: *block_time,
                    source_txid: None,
                };

                // Try to match against a pending Hemi mint (ETH route)
                if let Some(mints) = self.pending_hemi_mints_by_addr.get_mut(&key) {
                    if let Some(pos) = mints.iter().position(|m| {
                        amounts_match(m.amount, amount_f64, tolerance)
                            && within_time_window(m.timestamp, pending.timestamp, gap)
                    }) {
                        let mint = mints.remove(pos);
                        if mints.is_empty() { self.pending_hemi_mints_by_addr.remove(&key); }
                        return Some(MatchResult {
                            source: pending.raw,
                            destination: mint.raw,
                            direction: MatchDirection::EthToHemi,
                            amount: amount_f64,
                            address: pending.address,
                        });
                    }
                }

                self.pending_eth_locks.entry(key).or_default().push(pending);
                None
            }

            EthereumEvent::TunnelRelease { to, asset, amount, .. } => {
                let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
                let addr_str   = hex::encode(to.0);
                let asset_str  = format!("{:?}", asset);
                let key = MatchKey { address: addr_str.clone(), asset: asset_str.clone() };
                let tolerance  = self.config.amount_tolerance;

                // Try to match against a pending Hemi burn
                if let Some(burns) = self.pending_hemi_burns.get_mut(&key) {
                    if let Some(pos) = burns.iter().position(|b| {
                        amounts_match(b.amount, amount_f64, tolerance)
                    }) {
                        let burn = burns.remove(pos);
                        if burns.is_empty() { self.pending_hemi_burns.remove(&key); }
                        return Some(MatchResult {
                            source: burn.raw,
                            destination: event,
                            direction: MatchDirection::HemiToEth,
                            amount: amount_f64,
                            address: addr_str,
                        });
                    }
                }

                None
            }

            _ => None,
        }
    }

    pub fn pending_count(&self) -> PendingCount {
        PendingCount {
            btc_deposits: self.pending_btc_deposits.values().map(|v| v.len()).sum(),
            eth_locks: self.pending_eth_locks.values().map(|v| v.len()).sum(),
            hemi_mints: self.pending_hemi_mints_by_txid.values().map(|v| v.len()).sum::<usize>()
                + self.pending_hemi_mints_by_addr.values().map(|v| v.len()).sum::<usize>(),
            hemi_burns: self.pending_hemi_burns.values().map(|v| v.len()).sum(),
        }
    }

    pub fn clear(&mut self) {
        self.pending_btc_deposits.clear();
        self.pending_eth_locks.clear();
        self.pending_hemi_mints_by_txid.clear();
        self.pending_hemi_mints_by_addr.clear();
        self.pending_hemi_burns.clear();
    }
}

#[derive(Debug, Clone, Default)]
pub struct PendingCount {
    pub btc_deposits: usize,
    pub eth_locks: usize,
    pub hemi_mints: usize,
    pub hemi_burns: usize,
}

impl PendingCount {
    pub fn total(&self) -> usize {
        self.btc_deposits + self.eth_locks + self.hemi_mints + self.hemi_burns
    }
}

// ── Free helpers (avoid borrow conflicts inside closures) ─────────────────────

/// The on-chain block timestamp carried by a raw event, if it has one.
fn event_block_time(event: &RawEvent) -> Option<DateTime<Utc>> {
    match event {
        RawEvent::Bitcoin(BitcoinEvent::TunnelDeposit { block_time, .. })
        | RawEvent::Bitcoin(BitcoinEvent::TunnelWithdrawal { block_time, .. })
        | RawEvent::Hemi(HemiEvent::TunnelMint { block_time, .. })
        | RawEvent::Hemi(HemiEvent::TunnelBurn { block_time, .. })
        | RawEvent::Hemi(HemiEvent::WithdrawalChallengeSuccess { block_time, .. })
        | RawEvent::Ethereum(EthereumEvent::TunnelLock { block_time, .. })
        | RawEvent::Ethereum(EthereumEvent::TunnelRelease { block_time, .. })
        | RawEvent::Ethereum(EthereumEvent::WithdrawalProven { block_time, .. }) => {
            Some(*block_time)
        }
        _ => None,
    }
}

fn amounts_match(a: f64, b: f64, tolerance: f64) -> bool {
    if a == 0.0 || b == 0.0 { return false; }
    (a - b).abs() <= a.max(b) * tolerance
}

fn within_time_window(a: DateTime<Utc>, b: DateTime<Utc>, max_gap_secs: i64) -> bool {
    (a - b).num_seconds().abs() <= max_gap_secs
}

#[cfg(test)]
mod tests {
    use super::*;
    use bigdecimal::BigDecimal;
    use chrono::Utc;
    use strait_core::{
        events::BitcoinEvent,
        types::{Address, Asset, BitcoinAddress, BitcoinTxid, BlockHash, TxHash},
    };

    fn make_btc_deposit(txid_bytes: [u8; 32], addr: &str, amount_sats: u64) -> RawEvent {
        RawEvent::Bitcoin(BitcoinEvent::TunnelDeposit {
            txid: BitcoinTxid(txid_bytes),
            vout: 0,
            to_address: BitcoinAddress::new(addr),
            amount_sats,
            op_return_data: None,
            hemi_destination: None,
            block_number: 100,
            block_hash: BlockHash([0; 32]),
            block_time: Utc::now(),
        })
    }

    fn make_hemi_mint(source_txid: Option<BitcoinTxid>, to: Address, amount_sats: u64) -> RawEvent {
        RawEvent::Hemi(HemiEvent::TunnelMint {
            tx_hash: TxHash([0; 32]),
            asset: Asset::Btc,
            amount: BigDecimal::from(amount_sats),
            to,
            source_txid,
            block_number: 200,
            block_time: Utc::now(),
            log_index: 0,
            gas_fee: None,
        })
    }

    #[test]
    fn test_btc_deposit_then_hemi_mint_with_source_txid() {
        let mut matcher = EventMatcher::new(MatcherConfig::default());

        let txid = BitcoinTxid([1u8; 32]);
        let to   = Address([0xABu8; 20]);

        assert!(matcher.process_event(make_btc_deposit([1u8; 32], "bc1qtest", 100_000_000)).is_none());
        let result = matcher.process_event(make_hemi_mint(Some(txid), to, 100_000_000));
        assert!(result.is_some());
        assert_eq!(result.unwrap().direction, MatchDirection::BtcToHemi);
    }

    #[test]
    fn test_hemi_mint_first_then_btc_deposit() {
        let mut matcher = EventMatcher::new(MatcherConfig::default());

        let txid = BitcoinTxid([2u8; 32]);
        let to   = Address([0xCDu8; 20]);

        assert!(matcher.process_event(make_hemi_mint(Some(txid), to, 100_000_000)).is_none());
        let result = matcher.process_event(make_btc_deposit([2u8; 32], "bc1qtest", 100_000_000));
        assert!(result.is_some());
        assert_eq!(result.unwrap().direction, MatchDirection::BtcToHemi);
    }

    #[test]
    fn test_no_match_on_large_amount_difference() {
        let mut matcher = EventMatcher::new(MatcherConfig { amount_tolerance: 0.01, ..Default::default() });

        let txid = BitcoinTxid([3u8; 32]);
        let to   = Address([0xEFu8; 20]);

        assert!(matcher.process_event(make_btc_deposit([3u8; 32], "bc1qtest", 100_000_000)).is_none());
        assert!(matcher.process_event(make_hemi_mint(Some(txid), to, 200_000_000)).is_none());
        assert_eq!(matcher.pending_count().total(), 2);
    }

    #[test]
    fn test_amount_within_tolerance_matches() {
        let mut matcher = EventMatcher::new(MatcherConfig { amount_tolerance: 0.02, ..Default::default() });

        let txid = BitcoinTxid([4u8; 32]);
        let to   = Address([0x11u8; 20]);

        assert!(matcher.process_event(make_btc_deposit([4u8; 32], "bc1qtest", 100_000_000)).is_none());
        // 1% difference — within 2% tolerance
        let result = matcher.process_event(make_hemi_mint(Some(txid), to, 101_000_000));
        assert!(result.is_some());
    }

    #[test]
    fn test_pending_count() {
        let mut matcher = EventMatcher::new(MatcherConfig::default());
        assert!(matcher.process_event(make_btc_deposit([5u8; 32], "bc1qtest", 100_000_000)).is_none());
        assert_eq!(matcher.pending_count().btc_deposits, 1);
        assert_eq!(matcher.pending_count().total(), 1);
    }

    /// Regression: a Hemi burn (withdrawal) must never pair with a pending L1
    /// TunnelLock (a deposit) from the same address with a similar amount — the
    /// release that pairs with a burn always arrives *after* the burn.
    #[test]
    fn test_burn_does_not_match_pending_lock() {
        let mut matcher = EventMatcher::new(MatcherConfig::default());
        let addr = Address([0x42u8; 20]);
        let amount = BigDecimal::from(1_000_000_000_000_000_000u64); // 1 ETH

        // Same user deposits 1 ETH on L1...
        let lock = RawEvent::Ethereum(EthereumEvent::TunnelLock {
            tx_hash: TxHash([1u8; 32]),
            asset: Asset::Eth,
            amount: amount.clone(),
            from: addr,
            block_number: 100,
            block_time: Utc::now(),
            log_index: 0,
            gas_fee: None,
        });
        assert!(matcher.process_event(lock).is_none());

        // ...then burns 1 ETH on Hemi to withdraw. This must NOT match the lock.
        let burn = RawEvent::Hemi(HemiEvent::TunnelBurn {
            tx_hash: TxHash([2u8; 32]),
            asset: Asset::Eth,
            amount,
            from: addr,
            destination: strait_core::types::ChainAddress::Evm(addr),
            block_number: 200,
            block_time: Utc::now(),
            log_index: 0,
            gas_fee: None,
            uuid: None,
        });
        assert!(matcher.process_event(burn).is_none());
        // Both stay pending, awaiting their real counterparts.
        assert_eq!(matcher.pending_count().eth_locks, 1);
        assert_eq!(matcher.pending_count().hemi_burns, 1);
    }

    /// Pending events past their match window are pruned as newer events arrive,
    /// so the pending maps cannot grow without bound.
    #[test]
    fn test_stale_pending_events_are_pruned() {
        let mut matcher = EventMatcher::new(MatcherConfig::default());

        // A deposit whose block time is far in the past.
        let old = RawEvent::Bitcoin(BitcoinEvent::TunnelDeposit {
            txid: BitcoinTxid([6u8; 32]),
            vout: 0,
            to_address: BitcoinAddress::new("bc1qold"),
            amount_sats: 100_000_000,
            op_return_data: None,
            hemi_destination: None,
            block_number: 100,
            block_hash: BlockHash([0; 32]),
            block_time: Utc::now() - chrono::Duration::seconds(BTC_DEPOSIT_MATCH_WINDOW_SECS * 3),
        });
        assert!(matcher.process_event(old).is_none());
        assert_eq!(matcher.pending_count().btc_deposits, 1);

        // A fresh deposit advances the pruning clock past the old one's window.
        assert!(matcher.process_event(make_btc_deposit([7u8; 32], "bc1qnew", 200_000_000)).is_none());
        assert_eq!(matcher.pending_count().btc_deposits, 1, "stale deposit should be pruned");
    }
}
