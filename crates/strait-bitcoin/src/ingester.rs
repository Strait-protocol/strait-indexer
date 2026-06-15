//! Bitcoin chain ingester.
//!
//! Drives the [`CustodyWatcher`](crate::watcher::CustodyWatcher): on each poll it
//! reads new UTXOs on the watched tunnel custody addresses (via the BitcoinKit
//! precompile on Hemi) and emits a `BitcoinEvent::TunnelDeposit` for each new
//! deposit into the shared event channel consumed by the join engine.

use std::time::Duration;

use chrono::Utc;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{error, info, warn};

use strait_core::{
    error::Result,
    events::{BitcoinEvent, RawEvent},
    types::BlockHash,
};

use crate::watcher::CustodyWatcher;

/// Polls Bitcoin custody addresses and emits tunnel deposit events.
pub struct BitcoinIngester {
    watcher: CustodyWatcher,
    event_tx: mpsc::Sender<RawEvent>,
    poll_interval: Duration,
}

impl BitcoinIngester {
    pub fn new(
        watcher: CustodyWatcher,
        event_tx: mpsc::Sender<RawEvent>,
        poll_interval_secs: u64,
    ) -> Self {
        Self {
            watcher,
            event_tx,
            poll_interval: Duration::from_secs(poll_interval_secs.max(1)),
        }
    }

    /// Run the watcher forever, emitting deposit events as they appear.
    pub async fn run(mut self) -> Result<()> {
        info!("Starting Bitcoin custody watcher");
        loop {
            match self.watcher.poll_new_deposits().await {
                Ok(candidates) => {
                    for c in candidates {
                        let event = RawEvent::Bitcoin(BitcoinEvent::TunnelDeposit {
                            txid: c.txid,
                            vout: c.vout,
                            to_address: c.to_address,
                            amount_sats: c.amount_sats,
                            op_return_data: None,
                            hemi_destination: Some(c.hemi_destination),
                            block_number: c.block_height,
                            block_hash: BlockHash([0u8; 32]),
                            block_time: Utc::now(),
                        });
                        if self.event_tx.send(event).await.is_err() {
                            warn!("event channel closed — stopping Bitcoin ingester");
                            return Ok(());
                        }
                    }
                }
                Err(e) => error!("Bitcoin custody poll failed: {e}"),
            }
            sleep(self.poll_interval).await;
        }
    }
}
