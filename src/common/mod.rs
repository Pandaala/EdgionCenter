//! Shared infrastructure copied (a minimal subset) from the Edgion monorepo's
//! `core::common`, plus a re-implemented auth layer (3rd-party OIDC + local
//! users). Only the surface Center actually uses is pulled in here.

pub mod api;
pub mod audit;
pub mod conf_sync;
pub mod config;
pub mod fed_sync;
pub mod grpc_tls;
pub mod metadata_conf_handler;
pub mod observe;
pub mod startup;

// Auth stack copied from core::common as the integration plumbing; the OIDC
// validation core is replaced with the `openidconnect` crate in Phase 4b.
pub mod auth;
pub mod local_auth;
pub mod unified_auth;
