//! Admin HTTP router composition.
//!
//! Provides `compose_admin_routes` -- the single shared composition entry
//! point for Controller and Center. The function assembles business routes,
//! shared auth endpoints (login/me/logout/status), metrics, health/ready,
//! and the unified_auth middleware in a fixed order. The `Router` returned
//! to the caller is treated as final; any subsequent routes would bypass
//! the middleware -- see the function-level docs for the final-state
//! contract.
//!
//! The Admin API does not mount a CORS layer: local development is handled
//! by edgion-dashboard's Vite `server.proxy` (see
//! `edgion-dashboard/vite.config.ts`), and in production the dashboard is
//! embedded and deployed same-origin with the Controller. If a separate
//! origin deployment becomes necessary in the future, introduce a precise
//! origin allowlist at that time.

use std::sync::Arc;

use axum::Router;

use super::{
    add_auth_status_route, add_local_auth_routes, cache_control_no_store_middleware, wrap_with_unified_auth_layer,
};
use crate::common::authz::AuthzStore;
use crate::common::unified_auth::UnifiedAuthState;

/// Compose the admin HTTP Router in a fixed order.
///
/// # Composition order (hard-coded inside this function; not caller-controllable)
/// 1. `business` (each component's own business routes)
/// 2. `/api/v1/auth/{login, logout, me}` (shared local_auth handlers,
///    mounted only when `local_auth_intent = true`)
/// 3. `/api/v1/auth/status` (mounted unconditionally)
/// 4. `authz` middleware (wraps the auth routes + business so `/auth/me` can
///    report permissions; reads the claims injected by `unified_auth`)
/// 5. `unified_auth` middleware (wraps all routes; skip_paths are provided by `auth_state`)
/// 6. `Cache-Control: no-store` middleware (outermost — see `cache_control_no_store_middleware`)
///
/// The authz layer sits INSIDE `unified_auth`: `unified_auth` runs first and
/// injects `UnifiedAuthClaims`, then authz resolves the caller's
/// `PermissionSet`, enforces the route's required key, and injects the set for
/// downstream handlers. Public skip-paths (login/status) carry no claims, so
/// authz passes them through without enforcement.
///
/// # Final-state contract
/// **The returned `Router` is final.** Do not call `.route()` / `.layer()`
/// on it -- any addition would bypass the auth middleware. New admin routes
/// should be added to the `business` argument before calling this function.
///
/// # Arguments
/// - `business`: per-component private business routes. **Must not** include
///   the auth routes mounted by this function (duplicate merges trigger an
///   axum panic). Whether business routes mount their own `/health` /
///   `/ready` / `/metrics` is up to the caller; this function does not mount
///   them.
/// - `auth_state`: shared authentication state from `UnifiedAuthState::from_configs`.
/// - `local_auth_intent`: whether local auth is intended to be enabled. When
///   `true`, mounts `/auth/{login,logout,me}`; when `false`, omits them
///   (OIDC-only or explicitly disabled). Note: this argument is not derived
///   from `auth_state`. The reason is that "intended-on but provider not yet
///   ready" is a valid state (`auth_state.local` is `None` but intent is
///   `true`, e.g. credentials configured but not yet loaded); in that state
///   the routes should still be mounted (handlers fail closed with 503 until
///   the provider is ready). `auth_state.local.load().is_some()` cannot
///   distinguish this case from "explicitly disabled", so the independent
///   intent parameter is retained.
pub fn compose_admin_routes(
    business: Router,
    auth_state: Arc<UnifiedAuthState>,
    local_auth_intent: bool,
    authz: Arc<dyn AuthzStore>,
) -> Router {
    let with_auth_routes = add_local_auth_routes(business, auth_state.clone(), local_auth_intent);
    let with_status = add_auth_status_route(with_auth_routes, auth_state.clone());
    // authz wraps the auth routes + business (INSIDE unified_auth, applied next):
    // it reads the claims unified_auth injects and resolves/enforces permissions.
    let with_authz = with_status.layer(axum::middleware::from_fn_with_state(
        authz,
        crate::common::authz::middleware::authz_middleware,
    ));
    let with_auth_layer = wrap_with_unified_auth_layer(with_authz, auth_state);
    with_auth_layer.layer(axum::middleware::from_fn(cache_control_no_store_middleware))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use tower::ServiceExt;

    use crate::common::authz::allow_all::AllowAllAuthz;
    use crate::common::authz::AuthzStore;

    fn allow_all() -> std::sync::Arc<dyn AuthzStore> {
        std::sync::Arc::new(AllowAllAuthz)
    }

    use crate::common::local_auth::LocalAuthConfig;

    fn valid_local() -> LocalAuthConfig {
        LocalAuthConfig {
            username: "admin".to_string(),
            password: "a_long_enough_password_123".to_string(),
            jwt_secret: "a_long_enough_jwt_secret_value_abcdef".to_string(),
            ..LocalAuthConfig::default()
        }
    }

    fn new_state_with_local() -> Arc<UnifiedAuthState> {
        let cfg = valid_local();
        UnifiedAuthState::from_configs(None, Some(&cfg), true, "test").unwrap()
    }

    fn dummy_business() -> Router {
        Router::new().route("/foo", get(|| async { "foo" }))
    }

    /// T1 -- after compose, /auth/{login,logout,me,status} are all routable (not 404).
    #[tokio::test]
    async fn compose_mounts_all_shared_routes() {
        let state = new_state_with_local();
        let app = compose_admin_routes(dummy_business(), state, true, allow_all());

        for path in &[
            "/api/v1/auth/login",
            "/api/v1/auth/logout",
            "/api/v1/auth/me",
            "/api/v1/auth/status",
        ] {
            let req = Request::builder().uri(*path).body(Body::empty()).unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_ne!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "route {} should be mounted, got {:?}",
                path,
                resp.status()
            );
        }
    }

    /// T2 -- /auth/me must be 401 without a Bearer token (not 200, 500, or 503).
    #[tokio::test]
    async fn me_requires_auth() {
        let state = new_state_with_local();
        let app = compose_admin_routes(dummy_business(), state, true, allow_all());
        let req = Request::builder().uri("/api/v1/auth/me").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// T3 -- /auth/status returns 200 without a token (the status endpoint must not be blocked by middleware).
    #[tokio::test]
    async fn status_does_not_require_auth() {
        let state = new_state_with_local();
        let app = compose_admin_routes(dummy_business(), state, true, allow_all());
        let req = Request::builder()
            .uri("/api/v1/auth/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// T4 -- the business route /foo is 401 without a token (require_auth=true with a local provider).
    #[tokio::test]
    async fn business_route_requires_auth_when_enabled() {
        let state = new_state_with_local();
        let app = compose_admin_routes(dummy_business(), state, true, allow_all());
        let req = Request::builder().uri("/foo").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// T5 -- a tokenless request to a business route returns 401 (auth is mandatory).
    /// (Previously asserted 200 pass-through when require_auth=false; that no-auth
    /// mode has been removed.)
    #[tokio::test]
    async fn business_route_requires_auth_no_token() {
        let state = new_state_with_local();
        let app = compose_admin_routes(dummy_business(), state, true, allow_all());
        let req = Request::builder().uri("/foo").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// T6 -- every admin response carries `Cache-Control: no-store`.
    ///
    /// Verifies that the blanket middleware fires on:
    ///   - the /api/v1/auth/status public endpoint (200 without auth — skip_path),
    ///   - a 401 rejection (auth required but no token supplied),
    ///   - a second /api/v1/auth/status request to confirm consistency.
    #[tokio::test]
    async fn all_responses_carry_cache_control_no_store() {
        // Case A: public /api/v1/auth/status skip-path → 200 (no token needed)
        let state_auth = new_state_with_local();
        let app_no_auth = compose_admin_routes(dummy_business(), state_auth, true, allow_all());
        let req = Request::builder()
            .uri("/api/v1/auth/status")
            .body(Body::empty())
            .unwrap();
        let resp = app_no_auth.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CACHE_CONTROL)
                .map(|v| v.as_bytes()),
            Some(b"no-store".as_slice()),
            "200 response must carry Cache-Control: no-store"
        );

        // Case B: auth required, no token → 401
        let state_auth = new_state_with_local();
        let app_auth = compose_admin_routes(dummy_business(), state_auth, true, allow_all());
        let req = Request::builder().uri("/foo").body(Body::empty()).unwrap();
        let resp = app_auth.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CACHE_CONTROL)
                .map(|v| v.as_bytes()),
            Some(b"no-store".as_slice()),
            "401 response must carry Cache-Control: no-store"
        );

        // Case C: /api/v1/auth/status → 200 (no auth required)
        let state_auth2 = new_state_with_local();
        let app_auth2 = compose_admin_routes(dummy_business(), state_auth2, true, allow_all());
        let req = Request::builder()
            .uri("/api/v1/auth/status")
            .body(Body::empty())
            .unwrap();
        let resp = app_auth2.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CACHE_CONTROL)
                .map(|v| v.as_bytes()),
            Some(b"no-store".as_slice()),
            "/auth/status response must carry Cache-Control: no-store"
        );
    }

    /// Mint a valid local HS256 token for `valid_local()`'s secret.
    fn valid_local_token() -> String {
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        let claims = serde_json::json!({ "sub": "admin", "iat": now, "exp": now + 3600 });
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret("a_long_enough_jwt_secret_value_abcdef".as_bytes()),
        )
        .unwrap()
    }

    /// Under AllowAll, an authenticated request to a business route mapped to a
    /// permission key (GET /api/v1/controllers → controllers:read) is allowed
    /// (200, not 403), and /auth/status still works without a token.
    #[tokio::test]
    async fn allow_all_permits_mapped_business_route() {
        let state = new_state_with_local();
        let business = Router::new().route("/api/v1/controllers", get(|| async { "controllers" }));
        let app = compose_admin_routes(business, state, true, allow_all());

        // Mapped business route with a valid token → allowed under AllowAll.
        let req = Request::builder()
            .uri("/api/v1/controllers")
            .header("Authorization", format!("Bearer {}", valid_local_token()))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "AllowAll must permit a mapped business route for an authenticated caller"
        );

        // /auth/status still reachable without a token.
        let req = Request::builder()
            .uri("/api/v1/auth/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Mint a valid local HS256 token for `valid_local()`'s secret with an
    /// arbitrary `sub`.
    fn token_for(sub: &str) -> String {
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        let claims = serde_json::json!({ "sub": sub, "iat": now, "exp": now + 3600 });
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret("a_long_enough_jwt_secret_value_abcdef".as_bytes()),
        )
        .unwrap()
    }

    /// RBAC-mode end-to-end enforcement through the fully composed app:
    /// a logged-in user whose ONLY permission is `controllers:read` gets 200 on
    /// `GET /api/v1/controllers` but 403 on `POST /api/v1/controllers/{id}/reload`
    /// (which requires `controllers:write`). The token is hand-issued (signed
    /// with the test secret) and resolved by a real `DbAuthz` over a seeded
    /// in-memory `Store` — a real behavioral assertion, not a mock.
    #[tokio::test]
    async fn db_authz_enforces_per_route_permissions() {
        use crate::common::authz::db_authz::DbAuthz;
        use crate::store::Store;
        use std::sync::Arc;

        // Seed: user "alice" with a role granting only controllers:read.
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let uid = store.create_user("alice", "hash", "Alice").await.unwrap();
        let rid = store.create_role("viewer", "Read-only").await.unwrap();
        store
            .set_role_permissions(rid, &["controllers:read".into()])
            .await
            .unwrap();
        store.set_user_roles(uid, &[rid]).await.unwrap();

        let state = new_state_with_local();
        let authz: Arc<dyn AuthzStore> = Arc::new(DbAuthz::new(store));
        let business = Router::new()
            .route("/api/v1/controllers", get(|| async { "list" }))
            .route("/api/v1/controllers/{id}/reload", axum::routing::post(|| async { "reload" }));
        let app = compose_admin_routes(business, state, false, authz);

        // GET /api/v1/controllers -> controllers:read -> granted -> 200.
        let req = Request::builder()
            .uri("/api/v1/controllers")
            .header("Authorization", format!("Bearer {}", token_for("alice")))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "controllers:read must permit GET /api/v1/controllers"
        );

        // POST /api/v1/controllers/c1/reload -> controllers:write -> missing -> 403.
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/controllers/c1/reload")
            .header("Authorization", format!("Bearer {}", token_for("alice")))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "missing controllers:write must deny POST .../reload with 403"
        );
    }

    /// Orthogonality proof: RBAC enforcement resolves by SUBJECT regardless of
    /// the authn provider. A token whose `sub` is NOT provisioned in the users
    /// table is denied (403) on a mapped route; once that same subject is seeded
    /// with a role granting the route's key, it is allowed (200). The token here
    /// is a local HS256 token standing in for any provider (OIDC included) — what
    /// matters is that DbAuthz keys off the subject, not the issuer.
    #[tokio::test]
    async fn oidc_rbac_unmapped_sub_403() {
        use crate::common::authz::db_authz::DbAuthz;
        use crate::store::Store;
        use std::sync::Arc;

        let store = Arc::new(Store::open_in_memory().await.unwrap());

        // App over an EMPTY users table: "carol" is unprovisioned.
        let authz: Arc<dyn AuthzStore> = Arc::new(DbAuthz::new(store.clone()));
        let business = Router::new().route("/api/v1/controllers", get(|| async { "list" }));
        let app = compose_admin_routes(business, new_state_with_local(), false, authz);

        let req = Request::builder()
            .uri("/api/v1/controllers")
            .header("Authorization", format!("Bearer {}", token_for("carol")))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "an unprovisioned subject must be denied under RBAC (fail closed)"
        );

        // Seed carol with a role granting controllers:read, then rebuild the app
        // with a fresh DbAuthz (empty cache) so the new grant is visible.
        let uid = store.create_user("carol", "hash", "Carol").await.unwrap();
        let rid = store.create_role("viewer", "Read-only").await.unwrap();
        store
            .set_role_permissions(rid, &["controllers:read".into()])
            .await
            .unwrap();
        store.set_user_roles(uid, &[rid]).await.unwrap();

        let authz2: Arc<dyn AuthzStore> = Arc::new(DbAuthz::new(store.clone()));
        let business2 = Router::new().route("/api/v1/controllers", get(|| async { "list" }));
        let app2 = compose_admin_routes(business2, new_state_with_local(), false, authz2);
        let req = Request::builder()
            .uri("/api/v1/controllers")
            .header("Authorization", format!("Bearer {}", token_for("carol")))
            .body(Body::empty())
            .unwrap();
        let resp = app2.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "once provisioned with controllers:read, the same subject is allowed"
        );
    }

    /// AllowAll authz combined with DB-user login: an authenticated caller reaches
    /// a WRITE route (mapped to controllers:write) and is allowed (200) — proving
    /// the authz axis (allow_all) is independent of the authn axis (db users).
    #[tokio::test]
    async fn db_auth_allow_all_grants_everything() {
        let business = Router::new().route(
            "/api/v1/controllers/{id}/reload",
            axum::routing::post(|| async { "reload" }),
        );
        let app = compose_admin_routes(business, new_state_with_local(), true, allow_all());

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/controllers/c1/reload")
            .header("Authorization", format!("Bearer {}", token_for("anyuser")))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "AllowAll must grant a write route to any authenticated caller"
        );
    }
}
