//! Compatibility re-export for fed-sync gRPC type definitions.
//!
//! The wire contract (`proto`) is regenerated from the copied
//! `proto/fed_sync.proto`. Runtime behavior and peer-identity verification live
//! in `edgion-center-runtime`.

pub use edgion_center_runtime::federation::proto;
