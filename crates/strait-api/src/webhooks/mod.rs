//! Webhook dispatcher and subscription management.
//!
//! See docs/webhooks-implementation-plan.md for the design. `registry` owns
//! registration/validation and secret generation; `dispatcher` owns outbox
//! enqueue and the background delivery loop.

pub mod dispatcher;
pub mod registry;
