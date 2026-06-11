//! Fed-sync gRPC type definitions and shared identity helpers.
//!
//! The wire contract (`proto`) is regenerated from the copied
//! `proto/fed_sync.proto`. `spiffe` is the SPIFFE peer-identity verification
//! used to authenticate controllers over the mTLS stream. The fed-sync SERVER
//! runtime logic lives under `crate::server` (moved with Center in Phase 3).

pub mod proto;
pub mod spiffe;
