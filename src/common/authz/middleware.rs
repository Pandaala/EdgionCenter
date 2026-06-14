//! Authorization middleware.
//!
//! Applied INSIDE `unified_auth` (so the `UnifiedAuthClaims` it injects are
//! visible here) and AROUND both the shared auth routes and business routes
//! (so `/auth/me` can read the resolved permissions). For each request it:
//!
//! 1. Reads `UnifiedAuthClaims` from extensions. If absent — a public skip-path
//!    such as login/status reached without a token — it runs the next layer
//!    with no enforcement and no injection.
//! 2. Otherwise builds a [`Principal`], resolves a [`PermissionSet`] via the
//!    installed [`AuthzStore`], and injects the set into request extensions so
//!    downstream handlers (notably `/auth/me`) can report it.
//! 3. Enforces: if `route_permission(method, path)` is `Some(key)` and the set
//!    does not contain it, responds `403`; otherwise continues.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

use super::{catalog, AuthzStore, PermissionSet, Principal};
use crate::common::unified_auth::{AuthProvider, UnifiedAuthClaims};

/// Build the 403 JSON body returned when a caller lacks the required key.
fn forbidden_response(required: &str) -> Response {
    let body = Json(serde_json::json!({
        "success": false,
        "error": "forbidden: missing permission",
        "required_permission": required,
    }));
    (StatusCode::FORBIDDEN, body).into_response()
}

/// Authorization middleware. See the module docs for the full contract.
///
/// AuthBypass invariant: when `unified_auth` honors an `AuthBypass` extension it
/// returns early WITHOUT injecting `UnifiedAuthClaims`. Such a request reaches
/// this middleware with no claims and so takes the no-claims branch below —
/// passing through with NO authz enforcement at all. AuthBypass therefore means
/// full trust by design. No Center admin route injects AuthBypass today; this
/// note records the invariant so a future bypass path isn't added blindly.
pub async fn authz_middleware(
    State(authz): State<Arc<dyn AuthzStore>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // No authenticated claims (public skip-path without a token, or an
    // AuthBypass'd request — see the fn doc) → pass through untouched: no
    // enforcement, no injection.
    let principal = {
        let Some(claims) = request.extensions().get::<UnifiedAuthClaims>() else {
            return next.run(request).await;
        };
        // Borrow the claims: copy only the provider tag and clone only `sub`,
        // never the (potentially large) `claims: serde_json::Value`. The borrow
        // is dropped at the end of this block, before `next.run`.
        let provider = match claims.provider {
            AuthProvider::Oidc => "oidc",
            AuthProvider::Local => "local",
        };
        Principal {
            subject: claims.sub.clone().unwrap_or_else(|| "<unknown>".to_string()),
            provider: provider.to_string(),
        }
    };

    let perms = authz.permissions_for(&principal).await;

    // Enforcement against the route's required key.
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    match catalog::route_permission(&method, &path) {
        Some(key) => {
            if !perms.contains(key) {
                tracing::debug!(
                    component = "authz",
                    subject = %principal.subject,
                    provider = %principal.provider,
                    method = %method,
                    path = %path,
                    required = key,
                    "authorization denied: missing permission"
                );
                return forbidden_response(key);
            }
        }
        None => {
            // Unmapped route. Fail CLOSED for business paths: a future business
            // route nobody added to `route_permission` must NOT be silently
            // reachable by everyone once real RBAC lands. Superusers (an `all`
            // set — notably LITE's AllowAll) still pass, so lite is unchanged
            // and an admin role keeps working. Public auth routes are not
            // business paths and always pass.
            if catalog::is_business_path(&path) && !perms.is_all() {
                tracing::debug!(
                    component = "authz",
                    subject = %principal.subject,
                    provider = %principal.provider,
                    method = %method,
                    path = %path,
                    "authorization denied: unmapped business route (fail-closed)"
                );
                return forbidden_response("<unmapped-business-route>");
            }
        }
    }

    // Make the resolved permissions available to downstream handlers.
    request.extensions_mut().insert(perms);
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::authz::allow_all::AllowAllAuthz;
    use axum::{body::to_bytes, http::Request, middleware, routing::get, Router};
    use serde_json::Value;
    use tower::ServiceExt;

    /// A store that grants no permissions, for the deny path.
    struct EmptyAuthz;
    #[async_trait::async_trait]
    impl AuthzStore for EmptyAuthz {
        async fn permissions_for(&self, _p: &Principal) -> PermissionSet {
            PermissionSet::from_keys(Vec::<String>::new())
        }
    }

    /// Inject a `UnifiedAuthClaims` (simulating unified_auth) before authz runs.
    fn claims_injecting_layer(router: Router) -> Router {
        router.layer(middleware::from_fn(
            |mut req: Request<Body>, next: Next| async move {
                req.extensions_mut().insert(UnifiedAuthClaims {
                    provider: AuthProvider::Local,
                    sub: Some("tester".to_string()),
                    iss: None,
                    claims: serde_json::Value::Null,
                });
                next.run(req).await
            },
        ))
    }

    fn app_with(authz: Arc<dyn AuthzStore>, inner: Router) -> Router {
        // authz inner, claims-injection outer — mirrors unified_auth wrapping authz.
        let with_authz = inner.layer(middleware::from_fn_with_state(authz, authz_middleware));
        claims_injecting_layer(with_authz)
    }

    #[tokio::test]
    async fn denies_without_key() {
        // A mapped route (GET /api/v1/controllers → controllers:read) with an
        // empty permission set must be rejected with 403.
        let inner = Router::new().route("/api/v1/controllers", get(|| async { "ok" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(Request::builder().uri("/api/v1/controllers").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn allows_with_all() {
        // AllowAll → 200, and the PermissionSet must be injected into extensions
        // (the handler reads it back and echoes the count).
        let inner = Router::new().route(
            "/api/v1/controllers",
            get(|axum::Extension(perms): axum::Extension<PermissionSet>| async move {
                format!("{}", perms.materialize().len())
            }),
        );
        let app = app_with(Arc::new(AllowAllAuthz), inner);
        let resp = app
            .oneshot(Request::builder().uri("/api/v1/controllers").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let count: usize = String::from_utf8(body.to_vec()).unwrap().parse().unwrap();
        assert_eq!(
            count,
            catalog::all_keys().len(),
            "injected PermissionSet must materialize the full catalog"
        );
    }

    #[tokio::test]
    async fn no_required_key_passes() {
        // An unmapped NON-business path (route_permission == None, and not under
        // /api/v1/) passes even with an empty set.
        let inner = Router::new().route("/auth/me", get(|| async { "me" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(Request::builder().uri("/auth/me").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_route_passes_without_key() {
        // A public auth route under /api/v1/auth/ is unmapped (None) but is NOT a
        // business path → it must pass even for an empty (non-superuser) set.
        let inner = Router::new().route("/api/v1/auth/status", get(|| async { "status" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(Request::builder().uri("/api/v1/auth/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unmapped_business_route_denied_for_non_superuser() {
        // A future business route nobody mapped (route_permission == None but it
        // IS a business path) must fail CLOSED for a non-superuser set.
        let inner =
            Router::new().route("/api/v1/center/something-new", get(|| async { "new" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/center/something-new")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn unmapped_business_route_allowed_for_superuser() {
        // Same unmapped business route, but a superuser (AllowAll, all=true) set
        // still reaches it → lite tier is unchanged.
        let inner =
            Router::new().route("/api/v1/center/something-new", get(|| async { "new" }));
        let app = app_with(Arc::new(AllowAllAuthz), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/center/something-new")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn forbidden_body_is_json() {
        let inner = Router::new().route("/api/v1/controllers", get(|| async { "ok" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(Request::builder().uri("/api/v1/controllers").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v.get("success").and_then(|x| x.as_bool()), Some(false));
        assert_eq!(
            v.get("required_permission").and_then(|x| x.as_str()),
            Some("controllers:read")
        );
    }
}
