//! HTTP handler for unified password login.
//!
//! `POST /api/v1/auth/login` accepts credentials and tries, in order:
//!   1. the `users` table (bcrypt) when DB-user login is enabled, then
//!   2. the single configured `local_auth` admin.
//!
//! On success it issues the SAME signed JWT + httpOnly cookie as `local_auth`'s
//! login (via the shared [`crate::common::local_auth::handlers::issue_login_response`]),
//! so the token format and the `unified_auth` validation path are identical
//! regardless of which credential source matched. `logout` and `me` are
//! credential-source-agnostic and reused as-is from `local_auth` (see
//! [`crate::common::db_auth::add_unified_auth_routes`]).

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};

use crate::common::api::ApiResponse;
use crate::common::local_auth::handlers::{issue_login_response, LoginRequest, LoginResponse};
use crate::common::local_auth::middleware::LocalAuthState;
use crate::store::Store;

/// State for the unified login handler.
///
/// `store` is `Some` only when DB-user login is enabled. `local` carries the
/// JWT signing secret / cookie settings and the single-admin credential (when
/// `single_admin_enabled`). At least one of (`store.is_some()`,
/// `single_admin_enabled`) must hold for any login to succeed; the assembly
/// only mounts this handler when that is the case.
#[derive(Clone)]
pub struct UnifiedLoginState {
    /// The user store. `Some` when `db_auth` is enabled.
    pub store: Option<Arc<Store>>,
    /// Signing material + (optional) single-admin credential.
    pub local: Arc<LocalAuthState>,
    /// Whether the single configured `local_auth` admin is a valid credential
    /// source. When false, `local` is signing-only (placeholder credential).
    pub single_admin_enabled: bool,
}

/// Run a bcrypt verify off the async runtime.
///
/// Returns `Ok(true)`/`Ok(false)` for verify outcomes (a malformed stored hash
/// collapses to `Ok(false)`), and `Err(())` only when the blocking task panics.
async fn bcrypt_verify(password: String, hash: String) -> Result<bool, ()> {
    match tokio::task::spawn_blocking(move || bcrypt::verify(&password, &hash)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(_)) => Ok(false),
        Err(_) => Err(()),
    }
}

fn internal_error() -> Response {
    let body: Json<ApiResponse<LoginResponse>> =
        Json(ApiResponse::err_body("Internal authentication error".to_string()));
    (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
}

fn unauthorized() -> Response {
    let body: Json<ApiResponse<LoginResponse>> =
        Json(ApiResponse::err_body("Invalid username or password".to_string()));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

/// `POST /api/v1/auth/login` (unified).
///
/// Order of credential sources:
///   1. **DB users** (when `state.store` is `Some`): if the user exists, is
///      `active`, and the password verifies → success. If the user is missing,
///      a dummy bcrypt still runs (timing). A DB error is a 500, not an auth
///      outcome. A found-but-rejected user falls through to step 2.
///   2. **Single admin** (when `state.single_admin_enabled`): timing-safe
///      `verify_single_admin` → success on match.
///   3. Otherwise a uniform 401, after a dummy bcrypt so the no-source path is
///      not measurably faster.
///
/// On success the issued token's `sub` is the submitted username.
///
/// A DB user and the single admin SHOULD NOT share a username: if they collide,
/// supplying the single-admin password authenticates as that subject even if the
/// DB row is disabled/rotated (the break-glass single admin is independent of the
/// DB — a found-but-rejected DB user in step 1 falls through to the step 2 admin).
pub async fn unified_login_handler(State(state): State<UnifiedLoginState>, Json(req): Json<LoginRequest>) -> Response {
    // Step 1: DB-user login.
    if let Some(store) = &state.store {
        match store.get_user_by_username(&req.username).await {
            Ok(Some(user)) => {
                let active = user.status == "active";
                match bcrypt_verify(req.password.clone(), user.password_hash.clone()).await {
                    Ok(verified) => {
                        if active && verified {
                            tracing::info!(component = "db_auth", username = %req.username, "Login successful (db user)");
                            return issue_login_response(&state.local, &req.username);
                        }
                        // Found but inactive or wrong password: fall through.
                    }
                    Err(()) => {
                        tracing::error!(component = "db_auth", "bcrypt task panicked");
                        return internal_error();
                    }
                }
            }
            Ok(None) => {
                // Unknown user: run a dummy bcrypt to equalize timing, then fall through.
                let _ = bcrypt_verify(req.password.clone(), state.local.dummy_hash.clone()).await;
            }
            Err(e) => {
                tracing::error!(component = "db_auth", error = %e, "user lookup failed");
                return internal_error();
            }
        }
    }

    // Step 2: single-admin login.
    if state.single_admin_enabled {
        let checker = state.local.clone();
        let username = req.username.clone();
        let password = req.password.clone();
        match tokio::task::spawn_blocking(move || checker.verify_single_admin(&username, &password)).await {
            Ok(true) => {
                tracing::info!(component = "db_auth", username = %req.username, "Login successful (single admin)");
                return issue_login_response(&state.local, &req.username);
            }
            Ok(false) => {}
            Err(_) => {
                tracing::error!(component = "db_auth", "bcrypt task panicked");
                return internal_error();
            }
        }
    }

    // Step 3: uniform 401. Run a dummy bcrypt so a request that reaches a handler
    // with no usable source is not constant-time-distinguishable.
    let _ = bcrypt_verify(req.password.clone(), state.local.dummy_hash.clone()).await;
    tracing::debug!(component = "db_auth", username = %req.username, "Login failed");
    unauthorized()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::local_auth::LocalAuthConfig;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use axum::routing::post;
    use tower::ServiceExt;

    /// Build a signing-capable `LocalAuthState` carrying the given single-admin
    /// credential (used only when `single_admin_enabled`).
    fn local_state(username: &str, password: &str) -> Arc<LocalAuthState> {
        LocalAuthState::from_config(&LocalAuthConfig {
            username: username.to_string(),
            password: password.to_string(),
            jwt_secret: "a_long_enough_jwt_secret_value_abcdef".to_string(),
            ..LocalAuthConfig::default()
        })
    }

    fn app(state: UnifiedLoginState) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/auth/login", post(unified_login_handler))
            .with_state(state)
    }

    async fn login(state: UnifiedLoginState, username: &str, password: &str) -> axum::http::Response<Body> {
        let body = serde_json::json!({ "username": username, "password": password }).to_string();
        app(state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn seed_user(store: &Store, username: &str, password: &str) -> i64 {
        let hash = bcrypt::hash(password, bcrypt::DEFAULT_COST).unwrap();
        store.create_user(username, &hash, username).await.unwrap()
    }

    /// When a username exists in BOTH the DB and as the single admin (with
    /// different passwords), the DB password authenticates (DB path is tried first).
    #[tokio::test]
    async fn unified_login_db_user_wins() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        seed_user(&store, "shared", "db-password-123").await;
        let state = UnifiedLoginState {
            store: Some(store),
            local: local_state("shared", "admin-password-123"),
            single_admin_enabled: true,
        };
        let resp = login(state, "shared", "db-password-123").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let cookie = resp
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .expect("login must set the auth cookie");
        assert!(cookie.to_str().unwrap().contains("edgion_token="));
    }

    /// A user absent from the DB but matching the single admin authenticates
    /// via the fallback.
    #[tokio::test]
    async fn unified_login_falls_back_to_admin() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let state = UnifiedLoginState {
            store: Some(store),
            local: local_state("admin", "admin-password-123"),
            single_admin_enabled: true,
        };
        let resp = login(state, "admin", "admin-password-123").await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// A disabled DB user with the correct password is rejected (and there is no
    /// matching single admin to fall back to).
    #[tokio::test]
    async fn unified_login_db_inactive_rejected() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let uid = seed_user(&store, "alice", "correct horse battery").await;
        store.set_user_status(uid, "disabled").await.unwrap();
        let state = UnifiedLoginState {
            store: Some(store),
            local: local_state("admin", "admin-password-123"),
            single_admin_enabled: true,
        };
        let resp = login(state, "alice", "correct horse battery").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Unknown user + wrong admin credentials → uniform 401.
    #[tokio::test]
    async fn unified_login_uniform_401() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        seed_user(&store, "alice", "correct horse battery").await;
        let state = UnifiedLoginState {
            store: Some(store),
            local: local_state("admin", "admin-password-123"),
            single_admin_enabled: true,
        };
        let resp = login(state, "ghost", "whatever").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// With no store, only the single admin can log in.
    #[tokio::test]
    async fn unified_login_no_store_admin_only() {
        let state = UnifiedLoginState {
            store: None,
            local: local_state("admin", "admin-password-123"),
            single_admin_enabled: true,
        };
        // Right admin credentials -> 200.
        let resp = login(state.clone(), "admin", "admin-password-123").await;
        assert_eq!(resp.status(), StatusCode::OK);
        // Wrong password -> 401.
        let resp = login(state, "admin", "nope").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
