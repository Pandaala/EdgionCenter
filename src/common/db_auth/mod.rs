//! FULL-tier database-backed authentication.
//!
//! When `access.mode = full`, login authenticates against the `users` table
//! (bcrypt) instead of the single configured admin. Everything else about the
//! token — the HS256 signing secret (from `[local_auth].jwt_secret`), the
//! `unified_auth` validation path, the `/logout` and `/me` endpoints — is
//! reused unchanged from `local_auth`. Only the login credential source and the
//! installed `AuthzStore` (`DbAuthz`) differ between the lite and full tiers.
//!
//! This module mounts a DB-backed `/api/v1/auth/login` and reuses the existing
//! `local_auth` `/logout` and `/me` handlers (which are credential-source
//! agnostic — `/me` reads the injected claims + `PermissionSet`).

pub mod handlers;

use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};

use crate::common::unified_auth::UnifiedAuthState;
use crate::store::Store;
use handlers::{db_login_handler, DbAuthState};

/// Mount the FULL-tier auth routes onto `router`:
/// - `POST /api/v1/auth/login`  -> DB-user login (bcrypt against `users`)
/// - `POST /api/v1/auth/logout` -> reused `local_auth` logout
/// - `GET  /api/v1/auth/me`     -> reused `local_auth` me
///
/// Call this on the business router BEFORE `compose_admin_routes`, and pass
/// `local_auth_intent = false` to `compose_admin_routes` so it does NOT also
/// mount the lite single-admin login. The routes still get wrapped by the authz
/// and `unified_auth` layers (which wrap the whole router), and `login`/`logout`
/// remain public via `unified_auth`'s public-auth-endpoint skip list while `me`
/// stays token-gated — identical to the lite mounting.
pub fn add_db_auth_routes(router: Router, auth_state: Arc<UnifiedAuthState>, store: Arc<Store>) -> Router {
    use crate::common::local_auth::{logout_handler, me_handler};

    let db_state = DbAuthState {
        auth: auth_state.clone(),
        store,
    };

    // `login` needs DbAuthState; `logout` needs the UnifiedAuthState; `me` needs
    // no state. Build each as its own fully-stated `Router<()>` then merge.
    let login: Router = Router::new()
        .route("/api/v1/auth/login", post(db_login_handler))
        .with_state(db_state);
    let logout: Router = Router::new()
        .route("/api/v1/auth/logout", post(logout_handler))
        .with_state(auth_state);
    let me: Router = Router::new().route("/api/v1/auth/me", get(me_handler));

    router.merge(login).merge(logout).merge(me)
}
