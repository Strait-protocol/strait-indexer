//! Bitcoin finality lookup for any Hemi block, from PoP keystone publications.
//!
//! Shared by the GraphQL `popFinality` query and `GET /pop/finality/:hemi_block`.

use async_graphql::{Enum, SimpleObject};
use chrono::{DateTime, Utc};
use serde::Serialize;

use strait_core::error::Result;
use strait_store::{Database, PopRepo};

/// Confirmations at which Strait marks transfers Bitcoin-final (the node's
/// `POP_MIN_CONFIRMATIONS` default).
pub const BITCOIN_FINAL_CONFIRMATIONS: i64 = 6;
/// Hemi's own "superfinality" threshold (`bfgapi.BitcoinSuperFinality`).
pub const SUPER_FINAL_CONFIRMATIONS: i64 = 10;

#[derive(Enum, Copy, Clone, Eq, PartialEq, Serialize, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PopFinalityStatus {
    /// A valid Bitcoin publication anchors this block (see `confirmations`).
    Anchored,
    /// Inside the synced range, but no publication covers it yet. Recent
    /// blocks wait for the next PoP publication batch.
    Pending,
    /// Before the synced range: an earlier publication may exist that Strait
    /// hasn't read, so it can't name the first anchor.
    NotCovered,
}

/// The Bitcoin anchor for a Hemi block.
#[derive(SimpleObject, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PopFinality {
    pub hemi_block: i64,
    pub status: PopFinalityStatus,
    /// The published keystone that anchors the block (>= `hemi_block`; often a
    /// later keystone, since miners publish in batches).
    pub keystone_block: Option<i64>,
    pub bitcoin_txid: Option<String>,
    pub bitcoin_vout: Option<i32>,
    pub bitcoin_block: Option<i64>,
    pub bitcoin_block_time: Option<DateTime<Utc>>,
    /// Bitcoin confirmations of the publication, as of Strait's last sync.
    pub confirmations: Option<i64>,
    /// `confirmations >= 6`: the threshold Strait uses for `popAnchored`.
    pub bitcoin_final: bool,
    /// `confirmations >= 10`: Hemi's superfinality threshold.
    pub super_final: bool,
    /// Lowest Hemi block this lookup can answer for.
    pub covered_from_hemi_block: Option<i64>,
    /// Bitcoin height up to which the PoP miners' publications are fully read.
    pub synced_bitcoin_height: Option<i64>,
}

pub async fn lookup(db: &Database, hemi_block: i64) -> Result<PopFinality> {
    let repo = PopRepo::new(db);
    let cov = repo.coverage().await?;
    let mut out = PopFinality {
        hemi_block,
        status: PopFinalityStatus::NotCovered,
        keystone_block: None,
        bitcoin_txid: None,
        bitcoin_vout: None,
        bitcoin_block: None,
        bitcoin_block_time: None,
        confirmations: None,
        bitcoin_final: false,
        super_final: false,
        covered_from_hemi_block: cov.min_hemi_block,
        synced_bitcoin_height: cov.synced_height,
    };
    match cov.min_hemi_block {
        Some(min) if hemi_block >= min => {}
        _ => return Ok(out),
    }
    out.status = PopFinalityStatus::Pending;
    if let Some(f) = repo.finality_for_hemi_block(hemi_block).await? {
        out.status = PopFinalityStatus::Anchored;
        out.keystone_block = Some(f.keystone_block);
        out.bitcoin_txid = Some(f.btc_txid);
        out.bitcoin_vout = Some(f.vout);
        out.bitcoin_block = Some(f.btc_block_height);
        out.bitcoin_block_time = Some(f.btc_block_time);
        out.confirmations = Some(f.confirmations);
        out.bitcoin_final = f.confirmations >= BITCOIN_FINAL_CONFIRMATIONS;
        out.super_final = f.confirmations >= SUPER_FINAL_CONFIRMATIONS;
    }
    Ok(out)
}
