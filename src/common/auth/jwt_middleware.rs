use std::sync::Arc;

use super::config::AdminAuthConfig;
use super::oidc::OidcProvider;

/// Admin OIDC auth state shared with `unified_auth`.
///
/// Thin wrapper around the `openidconnect`-backed [`OidcProvider`]; the
/// validation policy (audiences / issuers / allowed algorithms / clock skew)
/// lives on the provider.
pub struct AuthMiddlewareState {
    pub provider: Arc<OidcProvider>,
}

impl AuthMiddlewareState {
    /// Build state from config. Returns `Err` on HTTP-client construction
    /// failure or an unparseable issuer URL (instead of `process::exit`).
    pub fn from_config(config: &AdminAuthConfig) -> Result<Arc<Self>, String> {
        let provider = OidcProvider::from_config(config)?;
        Ok(Arc::new(Self { provider }))
    }
}
