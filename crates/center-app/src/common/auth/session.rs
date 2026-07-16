//! Provider-neutral authenticated-session endpoint.

use axum::{response::IntoResponse, Json};
use serde::Serialize;

use crate::common::{api::ApiResponse, authz::PermissionSet, unified_auth::UnifiedAuthClaims};

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub username: String,
    /// Permission keys resolved by the authorization middleware.
    pub permissions: Vec<String>,
}

/// Return the identity and permissions established by the composed
/// authentication and authorization middleware. This endpoint is independent
/// of the credential provider and is therefore present in OIDC-only builds.
pub async fn me_handler(
    axum::Extension(claims): axum::Extension<UnifiedAuthClaims>,
    perms: Option<axum::Extension<PermissionSet>>,
) -> impl IntoResponse {
    let permissions = perms
        .map(|axum::Extension(value)| value.materialize())
        .unwrap_or_default();
    Json(ApiResponse::ok_body(MeResponse {
        username: claims.sub.unwrap_or_default(),
        permissions,
    }))
}
