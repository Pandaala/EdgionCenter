//! Provider-neutral authenticated-session endpoint.

#[cfg(test)]
#[path = "../../../tests/unit/common/auth/session_test.rs"]
mod tests;

use std::sync::Arc;

use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;

use crate::common::{
    api::ApiResponse,
    authz::PermissionSet,
    unified_auth::{AuthProvider, UnifiedAuthClaims, UnifiedAuthState},
};

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub username: String,
    /// Permission keys resolved by the authorization middleware.
    pub permissions: Vec<String>,
    #[serde(rename = "authProvider")]
    pub auth_provider: AuthProvider,
    #[serde(rename = "logoutPath", skip_serializing_if = "Option::is_none")]
    pub logout_path: Option<String>,
    /// Clear any coexisting password cookie before handing off external logout.
    #[serde(rename = "localLogoutAvailable")]
    pub local_logout_available: bool,
}

/// Return the identity and permissions established by the composed
/// authentication and authorization middleware. This endpoint is independent
/// of the credential provider and is therefore present in OIDC-only builds.
pub async fn me_handler(
    State(state): State<Arc<UnifiedAuthState>>,
    axum::Extension(claims): axum::Extension<UnifiedAuthClaims>,
    perms: Option<axum::Extension<PermissionSet>>,
) -> impl IntoResponse {
    let permissions = perms
        .map(|axum::Extension(value)| value.materialize())
        .unwrap_or_default();
    Json(ApiResponse::ok_body(MeResponse {
        username: claims.sub.unwrap_or_default(),
        permissions,
        #[cfg(feature = "password-auth")]
        local_logout_available: state.local.load().is_some(),
        #[cfg(not(feature = "password-auth"))]
        local_logout_available: false,
        logout_path: if claims.provider == AuthProvider::Oidc {
            state
                .oidc
                .as_ref()
                .and_then(|oidc| oidc.logout_path.clone())
        } else {
            None
        },
        auth_provider: claims.provider,
    }))
}
