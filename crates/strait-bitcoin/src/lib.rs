//! Strait Bitcoin — Bitcoin chain ingester for the Strait tunnel indexer.

pub mod contracts;
pub mod ingester;
pub mod reorg;
pub mod watcher;

pub use ingester::BitcoinIngester;
pub use watcher::{BitcoinKitCaller, CustodyWatcher, DepositCandidate};
