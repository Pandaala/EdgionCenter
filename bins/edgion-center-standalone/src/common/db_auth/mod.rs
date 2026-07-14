//! Unified password login (DB users and/or the single `local_auth` admin).
//!
//! `POST /api/v1/auth/login` authenticates against the `users` table (bcrypt)
//! when DB-user login is enabled, falling back to the single configured admin.
//! Everything else about the token — the HS256 signing secret, the
//! `unified_auth` validation path, the `/logout` and `/me` endpoints — is reused
//! unchanged from the shared auth stack. The installed `AuthzStore` (`AllowAllAuthz` vs
//! `DbAuthz`) is an orthogonal axis selected separately by `authz.mode`.
//!
//! This module mounts the unified `/api/v1/auth/login` and reuses the existing
//! local `/logout` handler. The provider-neutral composer owns `/me`.

pub mod handlers;

use std::sync::Arc;

use axum::{routing::post, Router};

use crate::common::unified_auth::UnifiedAuthState;
pub use handlers::{unified_login_handler, UnifiedLoginState};

/// Mount the unified auth routes onto `business`:
/// - `POST /api/v1/auth/login`  -> unified login (DB users + single admin)
/// - `POST /api/v1/auth/logout` -> reused `local_auth` logout
///
/// Call this on the business router BEFORE `compose_admin_routes`, and pass
/// `local_auth_intent = false` to `compose_admin_routes` so it does NOT also
/// mount its own single-admin login/logout (the unified handler already owns
/// those paths; mounting them twice would panic axum on a duplicate route).
/// The routes still get wrapped by the authz and `unified_auth` layers (which
/// wrap the whole router); `login`/`logout` remain public via `unified_auth`'s
/// public-auth-endpoint skip list. The shared composer mounts provider-neutral
/// `/me`, which stays token-gated.
///
/// `auth_state` is required because the reused `logout_handler` reads
/// `cookie_secure` from the `UnifiedAuthState`'s local provider.
pub fn add_unified_auth_routes(
    business: Router,
    login_state: UnifiedLoginState,
    auth_state: Arc<UnifiedAuthState>,
) -> Router {
    use crate::common::local_auth::logout_handler;

    // Build each stateful route separately, then let the shared composer add
    // provider-neutral `/me`.
    let login: Router = Router::new()
        .route("/api/v1/auth/login", post(unified_login_handler))
        .with_state(login_state);
    let logout: Router = Router::new()
        .route("/api/v1/auth/logout", post(logout_handler))
        .with_state(auth_state);
    business.merge(login).merge(logout)
}
