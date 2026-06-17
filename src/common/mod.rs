//! Shared infrastructure copied (a minimal subset) from the Edgion monorepo's
//! `core::common`, plus a re-implemented auth layer (3rd-party OIDC + local
//! users). Only the surface Center actually uses is pulled in here.

pub mod api;
pub mod audit;
pub mod authz;
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
pub mod db_auth;
pub mod local_auth;
pub mod unified_auth;

/// Current Unix time in milliseconds. Returns 0 if the system clock is before
/// the Unix epoch (mirrors the previously open-coded `unwrap_or(0)` semantics).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::now_ms;
    #[test]
    fn now_ms_is_after_2020() {
        assert!(now_ms() > 1_577_836_800_000);
    }
    #[test]
    fn now_ms_is_monotonic_nondecreasing() {
        let a = now_ms();
        let b = now_ms();
        assert!(b >= a);
    }
}
