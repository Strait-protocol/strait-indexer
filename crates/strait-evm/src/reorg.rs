//! EVM reorg handling.
//!
//! Detects and handles chain reorganizations on EVM chains by tracking
//! block hashes and comparing them against the canonical chain.

use std::collections::VecDeque;

use alloy::providers::Provider;
use tracing::{debug, info, warn};

use strait_core::error::{Result, StraitError};

/// Maximum number of block hashes to keep in the reorg detection window.
const DEFAULT_REORG_WINDOW: usize = 128;

/// Tracks block hashes to detect chain reorganizations.
///
/// EVM reorgs happen when the chain switches to a different fork,
/// invalidating previously confirmed blocks. This detector maintains
/// a sliding window of recent block hashes and compares them against
/// the canonical chain to detect discrepancies.
pub struct ReorgDetector {
    /// Number of confirmations required before processing.
    required_confirmations: u64,
    /// Sliding window of (block_height, block_hash) pairs.
    block_hashes: VecDeque<(u64, String)>,
    /// Maximum size of the sliding window.
    window_size: usize,
}

impl ReorgDetector {
    /// Returns the number of confirmations required before processing.
    pub fn required_confirmations(&self) -> u64 {
        self.required_confirmations
    }

    /// Create a new reorg detector with the given confirmation requirement.
    pub fn new(required_confirmations: u64) -> Self {
        Self {
            required_confirmations,
            block_hashes: VecDeque::with_capacity(DEFAULT_REORG_WINDOW),
            window_size: DEFAULT_REORG_WINDOW,
        }
    }

    /// Create a new reorg detector with a custom window size.
    pub fn with_window(required_confirmations: u64, window_size: usize) -> Self {
        Self {
            required_confirmations,
            block_hashes: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    /// Record a processed block hash.
    ///
    /// This should be called after successfully processing a block
    /// to add it to the reorg detection window.
    pub fn record_block(&mut self, height: u64) {
        // Note: We don't store the hash here because we need to fetch it
        // from the provider during detection. This just records that we
        // processed this block height.
        if self.block_hashes.len() >= self.window_size {
            self.block_hashes.pop_front();
        }
        self.block_hashes.push_back((height, String::new()));
    }

    /// Record a block with its hash for more accurate reorg detection.
    pub fn record_block_with_hash(&mut self, height: u64, hash: String) {
        if self.block_hashes.len() >= self.window_size {
            self.block_hashes.pop_front();
        }
        self.block_hashes.push_back((height, hash));
    }

    /// Check if a reorg has occurred at the given block height.
    ///
    /// Compares the block hash at the given height against what we
    /// previously recorded. If they differ, a reorg has occurred.
    pub async fn detect_reorg<P: Provider + ?Sized>(
        &self,
        provider: &P,
        last_block: u64,
    ) -> Result<bool> {
        // Find the recorded hash for this block height
        let recorded = self.block_hashes.iter().find(|(h, _)| *h == last_block);

        match recorded {
            Some((_, recorded_hash)) if !recorded_hash.is_empty() => {
                // Fetch current block hash from chain
                let current_hash = self.get_block_hash(provider, last_block).await?;

                if *recorded_hash != current_hash {
                    warn!(
                        block = last_block,
                        recorded = %recorded_hash,
                        current = %current_hash,
                        "Reorg detected: block hash mismatch"
                    );
                    return Ok(true);
                }
                Ok(false)
            }
            _ => {
                // No recorded hash for this height, can't detect reorg
                // This is normal for blocks we haven't processed yet
                Ok(false)
            }
        }
    }

    /// Check the newest recorded block against the canonical chain and, if it
    /// no longer matches, walk backwards to find the first affected height.
    ///
    /// Returns `Ok(None)` when the newest recorded hash still matches (the
    /// common case — one extra RPC call per poll). On a mismatch, returns
    /// `Ok(Some(h))` where `h` is the lowest recorded height whose hash
    /// diverges from the canonical chain: every block >= h must be re-scanned.
    pub async fn find_reorg_point<P: Provider + ?Sized>(
        &self,
        provider: &P,
    ) -> Result<Option<u64>> {
        // Fast path: newest recorded block still canonical → no reorg.
        let Some((newest, newest_hash)) = self
            .block_hashes
            .iter()
            .rev()
            .find(|(_, h)| !h.is_empty())
        else {
            return Ok(None); // nothing recorded yet
        };
        if self.get_block_hash(provider, *newest).await? == *newest_hash {
            return Ok(None);
        }

        // The tip diverged. Walk backwards to the newest block that still
        // matches. Only per-window checkpoints are recorded, so the true fork
        // point may lie between two recorded heights — everything above the
        // newest *matching* block must be treated as affected.
        let mut oldest_recorded = *newest;
        for (height, recorded_hash) in self.block_hashes.iter().rev().skip(1) {
            if recorded_hash.is_empty() {
                continue;
            }
            if self.get_block_hash(provider, *height).await? == *recorded_hash {
                return Ok(Some(*height + 1));
            }
            oldest_recorded = *height;
        }
        // Every recorded block mismatched — the reorg is at least as deep as
        // our window; re-scan from the oldest block we know about.
        Ok(Some(oldest_recorded))
    }

    /// Detect reorg by checking multiple recent blocks.
    ///
    /// More thorough than single-block detection. Checks all recorded
    /// blocks within the confirmation window.
    pub async fn detect_reorg_thorough<P: Provider + ?Sized>(&self, provider: &P) -> Result<Option<u64>> {
        for (height, recorded_hash) in self.block_hashes.iter().rev() {
            if recorded_hash.is_empty() {
                continue;
            }

            let current_hash = self.get_block_hash(provider, *height).await?;

            if *recorded_hash != current_hash {
                warn!(
                    block = height,
                    recorded = %recorded_hash,
                    current = %current_hash,
                    "Reorg detected during thorough check"
                );
                return Ok(Some(*height));
            }
        }

        Ok(None)
    }

    /// Handle a detected reorg.
    ///
    /// Removes all blocks at and above the reorg point from the
    /// tracking window. The ingester should re-process from the
    /// last known good block.
    pub async fn handle_reorg(&mut self, reorg_height: u64) -> Result<()> {
        info!(
            reorg_height,
            "Handling reorg: removing blocks at and above reorg height"
        );

        // Remove all blocks at or above the reorg height
        self.block_hashes.retain(|(height, _)| *height < reorg_height);

        debug!(
            remaining_blocks = self.block_hashes.len(),
            "Reorg handling complete"
        );

        Ok(())
    }

    /// Get the last known good block height (below all tracked blocks).
    ///
    /// Returns the height of the last block that was not affected by
    /// any detected reorg.
    pub fn last_good_height(&self) -> Option<u64> {
        self.block_hashes.front().map(|(height, _)| *height).map(|h| h.saturating_sub(1))
    }

    /// Get the highest processed block height.
    pub fn highest_processed(&self) -> Option<u64> {
        self.block_hashes.back().map(|(height, _)| *height)
    }

    /// Clear all tracked blocks.
    pub fn clear(&mut self) {
        self.block_hashes.clear();
    }

    /// Get the number of tracked blocks.
    pub fn tracked_count(&self) -> usize {
        self.block_hashes.len()
    }

    /// Fetch block hash from the provider.
    async fn get_block_hash<P: Provider + ?Sized>(&self, provider: &P, height: u64) -> Result<String> {
        let block = provider
            .get_block_by_number(height.into(), false)
            .await
            .map_err(|e| StraitError::Chain(format!("Failed to get block {}: {}", height, e)))?
            .ok_or_else(|| StraitError::Chain(format!("Block {} not found", height)))?;

        Ok(block.header.hash.to_string())
    }
}

impl Default for ReorgDetector {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_creation() {
        let detector = ReorgDetector::new(3);
        assert_eq!(detector.required_confirmations, 3);
        assert_eq!(detector.tracked_count(), 0);
    }

    #[test]
    fn test_detector_with_window() {
        let detector = ReorgDetector::with_window(6, 64);
        assert_eq!(detector.required_confirmations, 6);
        assert_eq!(detector.window_size, 64);
    }

    #[test]
    fn test_record_blocks() {
        let mut detector = ReorgDetector::new(1);

        detector.record_block(100);
        detector.record_block(101);
        detector.record_block(102);

        assert_eq!(detector.tracked_count(), 3);
        assert_eq!(detector.highest_processed(), Some(102));
    }

    #[test]
    fn test_record_with_hash() {
        let mut detector = ReorgDetector::new(1);

        detector.record_block_with_hash(100, "0xabc".to_string());
        detector.record_block_with_hash(101, "0xdef".to_string());

        assert_eq!(detector.tracked_count(), 2);
    }

    #[test]
    fn test_window_size_limit() {
        let mut detector = ReorgDetector::with_window(1, 3);

        detector.record_block(1);
        detector.record_block(2);
        detector.record_block(3);
        detector.record_block(4);

        // Should only keep the last 3
        assert_eq!(detector.tracked_count(), 3);
    }

    #[test]
    fn test_handle_reorg() {
        let mut detector = ReorgDetector::new(1);

        detector.record_block_with_hash(100, "0xaaa".to_string());
        detector.record_block_with_hash(101, "0xbbb".to_string());
        detector.record_block_with_hash(102, "0xccc".to_string());
        detector.record_block_with_hash(103, "0xddd".to_string());

        // Simulate reorg at block 102
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(detector.handle_reorg(102)).unwrap();

        // Should only have blocks below 102
        assert_eq!(detector.tracked_count(), 2);
        assert_eq!(detector.highest_processed(), Some(101));
    }

    #[test]
    fn test_clear() {
        let mut detector = ReorgDetector::new(1);

        detector.record_block(100);
        detector.record_block(101);

        detector.clear();

        assert_eq!(detector.tracked_count(), 0);
        assert_eq!(detector.highest_processed(), None);
    }

    #[test]
    fn test_last_good_height() {
        let mut detector = ReorgDetector::new(1);

        assert_eq!(detector.last_good_height(), None);

        detector.record_block(100);
        assert_eq!(detector.last_good_height(), Some(99));

        detector.record_block(101);
        assert_eq!(detector.last_good_height(), Some(99));
    }

    #[test]
    fn test_default() {
        let detector = ReorgDetector::default();
        assert_eq!(detector.required_confirmations, 1);
    }
}