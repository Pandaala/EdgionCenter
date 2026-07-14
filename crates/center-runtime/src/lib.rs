//! Platform-independent EdgionCenter application runtime.
//!
//! Modules move here incrementally while the compatibility package remains
//! executable. This crate may depend on `center-core`, but never on SQL or
//! Kubernetes adapters.

pub mod aggregator;
pub mod metadata_store;
pub mod poll;
pub mod watch_cache;
