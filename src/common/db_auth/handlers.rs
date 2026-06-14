//! HTTP handlers for FULL-tier database-backed authentication.
//!
//! Provides the DB-user login handler (`POST /api/v1/auth/login`) used when
//! `access.mode = full`. It authenticates against the `users` table (bcrypt)
//! instead of the single configured admin, then issues the SAME signed JWT +
//! httpOnly cookie as lite's `local_auth` login (via the shared
//! [`crate::common::local_auth::handlers::issue_login_response`]). The token
//! format and the `unified_auth` validation path are identical across tiers —
//! only the credential source differs.
//!
//! `logout` and `me` are credential-source-agnostic and are reused as-is from
//! `local_auth` (see [`crate::common::db_auth::add_db_auth_routes`]).

use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
};

use crate::common::api::ApiResponse;
use crate::common::local_auth::handlers::{issue_login_response, LoginRequest, LoginResponse};
use crate::common::unified_auth::UnifiedAuthState;
use crate::store::Store;

/// State for the DB-backed login handler: the shared auth state (for the JWT
/// signing secret / cookie settings carried by its local provider) plus the
/// `Store` that holds the `users` table.
#[derive(Clone)]
pub struct DbAuthState {
    pub auth: Arc<UnifiedAuthState>,
    pub store: Arc<Store>,
}

/// `POST /api/v1/auth/login` (FULL tier)
///
/// Authenticates `{ username, password }` against the `users` table:
/// - 503 if the local auth provider (signing secret) is not yet populated.
/// - 401 if the user does not exist, is not `active`, or the password is wrong.
/// - 200 + signed JWT + httpOnly cookie on success (`sub` = username).
///
/// Timing-attack mitigation mirrors `local_auth`: a bcrypt verify always runs
/// (against the real hash when the user exists, against a dummy hash otherwise)
/// so response time does not reveal whether a username exists.
pub async fn db_login_handler(State(state): State<DbAuthState>, Json(req): Json<LoginRequest>) -> Response {
    // The local provider carries the JWT signing secret / cookie settings. When
    // it is not yet populated, fail with 503 (matches local_auth's login).
    let Some(local_state) = state.auth.local.load_full() else {
        let body: Json<ApiResponse<()>> = Json(ApiResponse::err_body(
            "authentication subsystem initializing, retry later".to_string(),
        ));
        let mut resp = (StatusCode::SERVICE_UNAVAILABLE, body).into_response();
        resp.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("10"));
        return resp;
    };

    // Look up the user. A DB error is an internal failure (500), not an auth
    // outcome — surfacing it as 401 would be misleading and could mask outages.
    let user = match state.store.get_user_by_username(&req.username).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(component = "db_auth", error = %e, "user lookup failed");
            let body: Json<ApiResponse<LoginResponse>> =
                Json(ApiResponse::err_body("Internal authentication error".to_string()));
            return (StatusCode::INTERNAL_SERVER_ERROR, body).into_response();
        }
    };

    // Always run a bcrypt verify (real hash if the user exists, dummy otherwise)
    // to equalize timing and prevent username enumeration. Success additionally
    // requires the user to exist AND be active.
    let active = matches!(&user, Some(u) if u.status == "active");
    let candidate_hash = match &user {
        Some(u) => u.password_hash.clone(),
        None => local_state.dummy_hash.clone(),
    };
    let password = req.password.clone();
    let verify_result = match tokio::task::spawn_blocking(move || bcrypt::verify(&password, &candidate_hash)).await {
        Ok(r) => r,
        Err(_) => {
            tracing::error!(component = "db_auth", "bcrypt task panicked");
            let body: Json<ApiResponse<LoginResponse>> =
                Json(ApiResponse::err_body("Internal authentication error".to_string()));
            return (StatusCode::INTERNAL_SERVER_ERROR, body).into_response();
        }
    };

    let password_ok = matches!(verify_result, Ok(true));
    if active && password_ok {
        tracing::info!(component = "db_auth", username = %req.username, "Login successful");
        // sub = username; token + cookie identical to the lite local_auth login.
        return issue_login_response(&local_state, &req.username);
    }

    // Uniform 401 for unknown user, inactive user, and wrong password — no
    // distinction leaked to the client.
    if let Some(e) = verify_result.err() {
        // A malformed stored hash surfaces here; log it but still return 401.
        tracing::debug!(component = "db_auth", error = %e, "bcrypt verify error during login");
    }
    tracing::debug!(component = "db_auth", username = %req.username, active, "Login failed");
    let body: Json<ApiResponse<LoginResponse>> =
        Json(ApiResponse::err_body("Invalid username or password".to_string()));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::local_auth::middleware::LocalAuthState;
    use crate::common::local_auth::LocalAuthConfig;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use axum::routing::post;
    use tower::ServiceExt;

    /// Build a `DbAuthState` over an in-memory store with the local provider
    /// (signing secret) populated, returning the state and the store handle.
    async fn state_with_store() -> (DbAuthState, Arc<Store>) {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let auth = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        let mut cfg = LocalAuthConfig::default();
        cfg.username = "unused".to_string();
        cfg.password = "unused-but-nonempty".to_string();
        cfg.jwt_secret = "a_long_enough_jwt_secret_value_abcdef".to_string();
        auth.update_local(LocalAuthState::from_config(&cfg));
        (
            DbAuthState {
                auth,
                store: store.clone(),
            },
            store,
        )
    }

    fn app(state: DbAuthState) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/auth/login", post(db_login_handler))
            .with_state(state)
    }

    async fn login(state: DbAuthState, username: &str, password: &str) -> axum::http::Response<Body> {
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

    #[tokio::test]
    async fn login_succeeds_for_active_user() {
        let (state, store) = state_with_store().await;
        seed_user(&store, "alice", "correct horse battery").await;
        let resp = login(state, "alice", "correct horse battery").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let cookie = resp
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .expect("login must set the auth cookie");
        assert!(cookie.to_str().unwrap().contains("edgion_token="));
    }

    #[tokio::test]
    async fn login_rejects_wrong_password() {
        let (state, store) = state_with_store().await;
        seed_user(&store, "alice", "correct horse battery").await;
        let resp = login(state, "alice", "wrong").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_rejects_unknown_user() {
        let (state, _store) = state_with_store().await;
        let resp = login(state, "ghost", "whatever").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_rejects_inactive_user() {
        let (state, store) = state_with_store().await;
        let uid = seed_user(&store, "alice", "correct horse battery").await;
        store.set_user_status(uid, "disabled").await.unwrap();
        // Correct password, but the account is disabled -> 401.
        let resp = login(state, "alice", "correct horse battery").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_503_when_provider_not_ready() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let auth = UnifiedAuthState::from_configs(None, None, true, "test").unwrap();
        let state = DbAuthState { auth, store };
        let resp = login(state, "alice", "x").await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
