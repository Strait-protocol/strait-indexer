//! Strait storage layer
//! 
//! Provides PostgreSQL-backed storage for indexed data, POP proofs, and chain state.

pub mod db;
pub mod transfers;
pub mod tunnel_transfers;
pub mod checkpoints;
pub mod events;
pub mod pop_proofs;
pub mod webhooks;

pub use db::Database;
pub use transfers::{Transfer, TransferRepo, CreateTransfer};
pub use tunnel_transfers::{status_str, AnalyticsBucketRow, TunnelTransferRepo, TunnelTransferRow};
pub use checkpoints::{Checkpoint, CheckpointRepo, UpsertCheckpoint};
pub use events::{IndexedEvent, EventRepo, CreateEvent};
pub use pop_proofs::{PopProof, PopProofRepo, PopProofStatus, CreatePopProof};
pub use webhooks::{
    CreateWebhookSubscription, DueDelivery, WebhookDeliveryRow, WebhookRepo, WebhookSubscriptionRow,
};

use strait_core::error::Result;

/// Initialize the database with migrations
pub async fn init_database(database_url: &str) -> Result<Database> {
    let db = Database::connect(database_url, 10).await?;
    db.migrate().await?;
    Ok(db)
}
