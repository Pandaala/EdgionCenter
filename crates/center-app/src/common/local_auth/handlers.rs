//! HTTP handlers for local authentication endpoints.
//!
//! Provides:
//! - `POST /api/v1/auth/login`  — validate credentials, return JWT
//! - `POST /api/v1/auth/logout` — clear the httpOnly auth cookie
//! - `GET /api/v1/auth/me`      — return authenticated user info from JWT claims

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::common::api::ApiResponse;
use crate::common::unified_auth::UnifiedAuthState;

/// JWT claims used by local auth tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAuthClaims {
    /// Subject (username).
    pub sub: String,
    /// Issued-at timestamp (Unix seconds).
    pub iat: usize,
    /// Expiry timestamp (Unix seconds).
    pub exp: usize,
}

/// Login request body.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Login response body.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// Signed JWT to use as `Authorization: Bearer <token>`.
    pub token: String,
    /// Token lifetime in seconds.
    pub expires_in: u64,
}

pub use crate::common::auth::session::{me_handler, MeResponse};

pub use super::status::{AuthStatus, AuthStatusResponse};

/// POST /api/v1/auth/login
///
/// Validates username + password against the stored config and returns a signed JWT.
pub async fn login_handler(
    State(state): State<Arc<UnifiedAuthState>>,
    Json(req): Json<LoginRequest>,
) -> Response {
    // Return 503 when the local auth provider is not yet populated.
    let Some(local_state) = state.local.load_full() else {
        use axum::http::HeaderValue;
        let body: Json<ApiResponse<()>> = Json(ApiResponse::err_body(
            "authentication subsystem initializing, retry later".to_string(),
        ));
        let mut resp = (StatusCode::SERVICE_UNAVAILABLE, body).into_response();
        resp.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("10"));
        return resp;
    };

    // Single timing-safe credential check (always runs exactly one bcrypt verify:
    // the real hash on a username match, a dummy hash otherwise) off the async
    // runtime via spawn_blocking. A task panic is the only internal-error case.
    let checker = local_state.clone();
    let username = req.username.clone();
    let password = req.password.clone();
    let ok = match tokio::task::spawn_blocking(move || {
        checker.verify_single_admin(&username, &password)
    })
    .await
    {
        Ok(v) => v,
        Err(_) => {
            tracing::error!(component = "local_auth", "bcrypt task panicked");
            let body: Json<ApiResponse<LoginResponse>> = Json(ApiResponse::err_body(
                "Internal authentication error".to_string(),
            ));
            return (StatusCode::INTERNAL_SERVER_ERROR, body).into_response();
        }
    };

    if !ok {
        tracing::debug!(
            component = "local_auth",
            "Login failed: invalid username or password"
        );
        let body: Json<ApiResponse<LoginResponse>> = Json(ApiResponse::err_body(
            "Invalid username or password".to_string(),
        ));
        return (StatusCode::UNAUTHORIZED, body).into_response();
    }

    tracing::info!(
        component = "local_auth",
        username = %local_state.username,
        "Login successful"
    );
    issue_login_response(&local_state, &local_state.username)
}

impl crate::common::local_auth::middleware::LocalAuthState {
    /// Timing-safe single-admin credential check.
    ///
    /// ALWAYS runs exactly one bcrypt verify — against the real `password_hash`
    /// when `username` matches the configured admin, against `dummy_hash`
    /// otherwise — so response time does not reveal whether the username exists.
    /// Returns `true` only when the username matches AND the password verifies.
    /// A bcrypt error (e.g. a malformed stored hash) is treated as a failed
    /// verify (`false`), never as success.
    ///
    /// Synchronous and CPU-bound: callers on an async runtime should invoke it
    /// inside `spawn_blocking`.
    #[doc(hidden)]
    pub fn verify_single_admin(&self, username: &str, password: &str) -> bool {
        let username_matches = username == self.username;
        let hash = if username_matches {
            &self.password_hash
        } else {
            &self.dummy_hash
        };
        let verified = bcrypt::verify(password, hash).unwrap_or(false);
        username_matches && verified
    }
}

/// Issue a signed HS256 JWT + httpOnly auth cookie for an authenticated
/// `username`, using the signing secret / expiry / cookie settings carried by
/// `local_state`.
///
/// Shared by the single-admin login ([`login_handler`]) and the unified login
/// (`crate::common::db_auth::handlers::unified_login_handler`) so the token
/// format and `Set-Cookie` shape are byte-for-byte identical regardless of the
/// credential source — the same `unified_auth` local validation path accepts
/// both. The only difference is the credential source (config admin vs. `users`
/// table); the issued token is the same.
#[doc(hidden)]
pub fn issue_login_response(
    local_state: &crate::common::local_auth::middleware::LocalAuthState,
    username: &str,
) -> Response {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as usize;
    let expiry_secs = local_state.jwt_expiry_hours * 3600;
    let claims = LocalAuthClaims {
        sub: username.to_string(),
        iat: now,
        exp: now + expiry_secs as usize,
    };

    let encoding_key = EncodingKey::from_secret(local_state.jwt_secret.as_bytes());
    match encode(
        &Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &encoding_key,
    ) {
        Ok(token) => {
            // `Secure` keeps this bearer-equivalent JWT cookie off plaintext http://.
            // Operator-configurable (default true) for non-TLS dev; see LocalAuthConfig.
            let secure_attr = if local_state.cookie_secure {
                "; Secure"
            } else {
                ""
            };
            let cookie_value = format!(
                "edgion_token={}; HttpOnly; SameSite=Strict; Path=/{}; Max-Age={}",
                token, secure_attr, expiry_secs
            );
            let response = LoginResponse {
                token,
                expires_in: expiry_secs,
            };
            let mut resp = Json(ApiResponse::ok_body(response)).into_response();
            resp.headers_mut()
                .insert(header::SET_COOKIE, cookie_value.parse().unwrap());
            resp
        }
        Err(e) => {
            tracing::error!(component = "local_auth", error = %e, "JWT encode error");
            let body: Json<ApiResponse<LoginResponse>> = Json(ApiResponse::err_body(
                "Failed to generate token".to_string(),
            ));
            (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
        }
    }
}

/// POST /api/v1/auth/logout
///
/// Clears the httpOnly auth cookie by setting Max-Age=0. Reads `cookie_secure`
/// from `LocalAuthState` so the clearing cookie carries the same `Secure`
/// attribute as the login cookie — a `Secure` clearing cookie must not be sent
/// when the login cookie was non-`Secure` (and vice versa), otherwise the
/// browser would refuse the clear over plain HTTP. No readiness guard: logout
/// must work regardless of provider state, so it falls back to secure-by-default
/// when the local provider is not yet populated.
pub async fn logout_handler(State(state): State<Arc<UnifiedAuthState>>) -> Response {
    let cookie_secure = state
        .local
        .load_full()
        .map(|s| s.cookie_secure)
        .unwrap_or(true);
    let secure_attr = if cookie_secure { "; Secure" } else { "" };
    let cookie_value = format!(
        "edgion_token=; HttpOnly; SameSite=Strict; Path=/{}; Max-Age=0",
        secure_attr
    );
    let mut resp = Json(ApiResponse::<()>::ok_body(())).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, cookie_value.parse().unwrap());
    resp
}

#[cfg(test)]
mod handler_tests {
    use super::*;
    use crate::common::local_auth::middleware::LocalAuthState;
    use crate::common::unified_auth::UnifiedAuthState;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn valid_local() -> crate::common::local_auth::LocalAuthConfig {
        crate::common::local_auth::LocalAuthConfig {
            enabled: true,
            username: "admin".to_string(),
            password: "a_long_enough_password_123".to_string(),
            jwt_secret: "a_long_enough_jwt_secret_value_abcdef".to_string(),
            ..crate::common::local_auth::LocalAuthConfig::default()
        }
    }

    fn app_with_unified(state: Arc<UnifiedAuthState>) -> axum::Router {
        use axum::routing::{get, post};
        axum::Router::new()
            .route("/api/v1/auth/login", post(login_handler))
            .route("/api/v1/auth/logout", post(logout_handler))
            .route("/api/v1/auth/me", get(me_handler))
            .with_state(state)
    }

    /// `/auth/me` reports the injected `PermissionSet` (LITE → full catalog) and
    /// the username from the claims. Builds a router that injects both a claims
    /// extension and an `AllowAll` permission set ahead of the handler.
    #[tokio::test]
    async fn me_reports_permissions_from_injected_set() {
        use crate::common::authz::PermissionSet;
        use crate::common::unified_auth::{AuthProvider, UnifiedAuthClaims};
        use axum::routing::get;

        let app = axum::Router::new()
            .route("/api/v1/auth/me", get(me_handler))
            .layer(axum::middleware::from_fn(
                |mut req: Request<Body>, next: axum::middleware::Next| async move {
                    req.extensions_mut().insert(UnifiedAuthClaims {
                        provider: AuthProvider::Local,
                        sub: Some("admin".to_string()),
                        iss: None,
                        groups: Vec::new(),
                        claims: serde_json::Value::Null,
                    });
                    req.extensions_mut().insert(PermissionSet::from_keys(
                        crate::common::authz::catalog::all_keys()
                            .iter()
                            .map(|key| (*key).to_string()),
                    ));
                    next.run(req).await
                },
            ));

        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let data = v.get("data").expect("data field");
        assert_eq!(data.get("username").and_then(|x| x.as_str()), Some("admin"));
        let perms = data
            .get("permissions")
            .and_then(|x| x.as_array())
            .expect("permissions array");
        assert_eq!(
            perms.len(),
            crate::common::authz::catalog::all_keys().len(),
            "LITE /auth/me must report the full permission catalog"
        );
        assert!(perms.iter().any(|p| p.as_str() == Some("controllers:read")));
    }

    /// When no `PermissionSet` is injected, `/auth/me` reports an empty list.
    #[tokio::test]
    async fn me_reports_empty_permissions_when_absent() {
        use crate::common::unified_auth::{AuthProvider, UnifiedAuthClaims};
        use axum::routing::get;

        let app = axum::Router::new()
            .route("/api/v1/auth/me", get(me_handler))
            .layer(axum::middleware::from_fn(
                |mut req: Request<Body>, next: axum::middleware::Next| async move {
                    req.extensions_mut().insert(UnifiedAuthClaims {
                        provider: AuthProvider::Local,
                        sub: Some("admin".to_string()),
                        iss: None,
                        groups: Vec::new(),
                        claims: serde_json::Value::Null,
                    });
                    next.run(req).await
                },
            ));

        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let perms = v
            .get("data")
            .and_then(|d| d.get("permissions"))
            .and_then(|x| x.as_array())
            .expect("permissions array");
        assert!(
            perms.is_empty(),
            "permissions must be empty when no set injected"
        );
    }

    #[test]
    fn verify_single_admin_matrix() {
        let state = LocalAuthState::from_config(&valid_local());
        // Right username + right password -> true.
        assert!(state.verify_single_admin("admin", "a_long_enough_password_123"));
        // Right username + wrong password -> false.
        assert!(!state.verify_single_admin("admin", "wrong"));
        // Unknown username (runs the dummy-hash bcrypt) -> false, even if the
        // password happens to equal the admin's.
        assert!(!state.verify_single_admin("ghost", "a_long_enough_password_123"));
    }

    #[tokio::test]
    async fn login_returns_503_when_local_not_ready() {
        let state = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        let app = app_with_unified(state);
        let body = serde_json::json!({"username":"admin","password":"x"}).to_string();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let retry = resp
            .headers()
            .get("retry-after")
            .expect("Retry-After header");
        assert_eq!(retry.to_str().unwrap(), "10");
    }

    #[tokio::test]
    async fn login_succeeds_after_update_local() {
        let state = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        state.update_local(LocalAuthState::from_config(&valid_local()));
        let app = app_with_unified(state);
        let body = serde_json::json!({
            "username":"admin",
            "password":"a_long_enough_password_123"
        })
        .to_string();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Pin the wire format of `AuthStatus`. CLI bails on serde parse failure,
    /// so silent casing drift (e.g. accidentally dropping `rename_all` or
    /// changing it to `lowercase`) would break every CLI command end-to-end.
    #[test]
    fn auth_status_serde_wire_format_snake_case() {
        assert_eq!(
            serde_json::to_string(&AuthStatus::Ready).unwrap(),
            "\"ready\""
        );
        assert_eq!(
            serde_json::to_string(&AuthStatus::Initializing).unwrap(),
            "\"initializing\""
        );
        assert_eq!(
            serde_json::to_string(&AuthStatus::Disabled).unwrap(),
            "\"disabled\""
        );
        // And round-trip from wire:
        let r: AuthStatus = serde_json::from_str("\"ready\"").unwrap();
        assert_eq!(r, AuthStatus::Ready);
        let i: AuthStatus = serde_json::from_str("\"initializing\"").unwrap();
        assert_eq!(i, AuthStatus::Initializing);
        let d: AuthStatus = serde_json::from_str("\"disabled\"").unwrap();
        assert_eq!(d, AuthStatus::Disabled);
    }

    #[tokio::test]
    async fn login_rejects_wrong_username() {
        let state = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        state.update_local(LocalAuthState::from_config(&valid_local()));
        let app = app_with_unified(state);
        let body =
            serde_json::json!({"username":"wrong_user","password":"a_long_enough_password_123"})
                .to_string();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Run a successful login against `state` and return the `Set-Cookie` value.
    async fn login_cookie(state: Arc<UnifiedAuthState>) -> String {
        let app = app_with_unified(state);
        let body = serde_json::json!({
            "username":"admin",
            "password":"a_long_enough_password_123"
        })
        .to_string();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        resp.headers()
            .get(axum::http::header::SET_COOKIE)
            .expect("login must set a cookie")
            .to_str()
            .unwrap()
            .to_string()
    }

    /// Run logout against `state` and return the `Set-Cookie` value.
    async fn logout_cookie(state: Arc<UnifiedAuthState>) -> String {
        let app = app_with_unified(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/auth/logout")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        resp.headers()
            .get(axum::http::header::SET_COOKIE)
            .expect("logout must set a cookie")
            .to_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn logout_sets_cookie_max_age_zero() {
        let state = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        state.update_local(LocalAuthState::from_config(&valid_local()));
        let cookie = logout_cookie(state).await;
        assert!(
            cookie.contains("Max-Age=0"),
            "logout cookie should carry Max-Age=0 to clear client state: {cookie}"
        );
    }

    #[tokio::test]
    async fn login_cookie_has_secure_by_default() {
        let state = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        state.update_local(LocalAuthState::from_config(&valid_local()));
        let cookie = login_cookie(state).await;
        assert!(
            cookie.contains("; Secure"),
            "login cookie must carry Secure by default: {cookie}"
        );
    }

    #[tokio::test]
    async fn login_cookie_omits_secure_when_disabled() {
        let mut cfg = valid_local();
        cfg.cookie_secure = false;
        let state = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        state.update_local(LocalAuthState::from_config(&cfg));
        let cookie = login_cookie(state).await;
        // Match the exact attribute segment: the embedded JWT body could in
        // principle contain the bare substring "Secure".
        assert!(
            !cookie.contains("; Secure"),
            "login cookie must omit Secure when cookie_secure=false: {cookie}"
        );
    }

    #[tokio::test]
    async fn logout_cookie_secure_matches_config() {
        // Secure by default.
        let state = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        state.update_local(LocalAuthState::from_config(&valid_local()));
        let cookie = logout_cookie(state).await;
        assert!(
            cookie.contains("; Secure"),
            "logout clearing cookie must carry Secure: {cookie}"
        );

        // Omitted when disabled, so the browser accepts the clear over plain HTTP.
        let mut cfg = valid_local();
        cfg.cookie_secure = false;
        let state = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        state.update_local(LocalAuthState::from_config(&cfg));
        let cookie = logout_cookie(state).await;
        assert!(
            !cookie.contains("; Secure"),
            "logout cookie must omit Secure when disabled: {cookie}"
        );
    }
}
