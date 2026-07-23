//! User administration endpoints — RBAC / db_auth user management: CRUD over
//! the `users` table and
//! its `user_roles` bindings. All routes mount under
//! `/api/v1/center/admin/users` and gate on the `users:manage` permission key
//! (see [`crate::common::authz::catalog::route_permission`]).
//!
//! Password hashing and persistence are owned by the selected `UserAdmin`
//! adapter; password hashes never cross the core port or reach this HTTP layer.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::{ApiResponse, ListResponse};

/// Serde view of a user row. camelCase to match the existing DTO convention.
/// Deliberately omits `password_hash` — it is never exposed to a client.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDto {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub status: String,
    pub created_at: i64,
    pub role_ids: Vec<i64>,
    pub role_names: Vec<String>,
}

/// Body for `POST /users`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub role_ids: Option<Vec<i64>>,
}

/// Body for `PATCH /users/{id}`. Every field is optional; only the provided
/// ones take effect.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub role_ids: Option<Vec<i64>>,
}

/// 503 response shared by every handler when the DB is not configured.
fn db_unavailable_list() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ListResponse::<UserDto>::error(
            "database not configured".to_string(),
        )),
    )
        .into_response()
}

fn db_unavailable() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiResponse::<String>::err_body(
            "database not configured".to_string(),
        )),
    )
        .into_response()
}

fn internal_err(e: impl std::fmt::Display) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse::<String>::err_body(e.to_string())),
    )
        .into_response()
}

/// Allowed values for a user's `status`. A PATCH carrying any other value is
/// rejected with 400 rather than persisting an arbitrary string.
const ALLOWED_STATUSES: &[&str] = &["active", "disabled"];

/// Whether an `anyhow::Error` from a Store create wraps a genuine UNIQUE
/// constraint violation (vs. any other DB failure — disk-full, connection-loss,
/// schema error). Works across both sqlx backends: `is_unique_violation()` is
/// provided by the `DatabaseError` trait for SQLite and MySQL alike.
impl From<edgion_center_core::UserRecord> for UserDto {
    fn from(user: edgion_center_core::UserRecord) -> Self {
        Self {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            status: user.status,
            created_at: user.created_at,
            role_ids: user.role_ids,
            role_names: user.role_names,
        }
    }
}

/// `GET /api/v1/center/admin/users` — list users with their bound roles.
pub async fn list_users_handler(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(admin) = &state.user_admin else {
        return db_unavailable_list();
    };
    let users = match admin.list_users().await {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ListResponse::<UserDto>::error(e.to_string())),
            )
                .into_response();
        }
    };
    let dtos = users.into_iter().map(UserDto::from).collect();
    Json(ListResponse::success(dtos)).into_response()
}

/// `POST /api/v1/center/admin/users` — create a user (bcrypt-hash the password,
/// optionally bind roles). 400 on empty username/password, 409 on duplicate.
pub async fn create_user_handler(
    State(state): State<ApiState>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    let Some(admin) = &state.user_admin else {
        return db_unavailable();
    };
    if req.username.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<String>::err_body(
                "username must not be empty".to_string(),
            )),
        )
            .into_response();
    }
    if req.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<String>::err_body(
                "password must not be empty".to_string(),
            )),
        )
            .into_response();
    }

    let id = match admin
        .create_user(edgion_center_core::CreateUser {
            username: req.username.clone(),
            password: req.password,
            display_name: req.display_name.unwrap_or_default(),
            role_ids: req.role_ids.unwrap_or_default(),
        })
        .await
    {
        Ok(id) => id,
        // Lost a create race against the UNIQUE constraint → 409. Any other DB
        // failure is a genuine internal error → 500 (not a spurious "duplicate").
        Err(edgion_center_core::CoreError::Conflict(_)) => {
            return (
                StatusCode::CONFLICT,
                Json(ApiResponse::<String>::err_body(format!(
                    "username '{}' already exists",
                    req.username
                ))),
            )
                .into_response();
        }
        Err(e) => return internal_err(e),
    };

    (StatusCode::CREATED, Json(ApiResponse::ok_body(id))).into_response()
}

/// `PATCH /api/v1/center/admin/users/{id}` — partial update. Only the fields
/// present in the body act (status / password reset / role rebinding).
pub async fn update_user_handler(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    let Some(admin) = &state.user_admin else {
        return db_unavailable();
    };

    // Validate EVERY provided field up front, before performing any write, so a
    // partially-valid PATCH (e.g. {status, password:""}) never persists one field
    // and then 400s on another — the update is all-or-nothing.
    if let Some(status) = &req.status {
        if !ALLOWED_STATUSES.contains(&status.as_str()) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<String>::err_body(format!(
                    "invalid status '{status}' (allowed: active, disabled)"
                ))),
            )
                .into_response();
        }
    }
    if let Some(password) = &req.password {
        if password.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<String>::err_body(
                    "password must not be empty".to_string(),
                )),
            )
                .into_response();
        }
    }

    if let Err(e) = admin
        .update_user(
            id,
            edgion_center_core::UpdateUser {
                status: req.status,
                password: req.password,
                role_ids: req.role_ids,
            },
        )
        .await
    {
        return internal_err(e);
    }

    (StatusCode::OK, Json(ApiResponse::ok_body("ok".to_string()))).into_response()
}

/// `DELETE /api/v1/center/admin/users/{id}` — delete (FK cascade removes its
/// `user_roles` / `api_tokens`).
pub async fn delete_user_handler(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let Some(admin) = &state.user_admin else {
        return db_unavailable();
    };
    match admin.delete_user(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal_err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::sync::Arc;

    /// Build an `ApiState` whose only meaningful field is `db`; everything else
    /// is a default/empty construction sufficient for these handlers.
    fn state_with_db(db: Option<Arc<Store>>) -> ApiState {
        use crate::aggregator::ResourceAggregator;
        use crate::commander::Commander;
        use crate::fed_sync::registry::ControllerRegistry;
        use crate::metadata_store::CenterMetaDataStore;
        use crate::proxy::ProxyForwarder;
        use crate::watch_cache::{CenterSyncClient, CenterWatchCacheRegistry};
        use parking_lot::Mutex;
        use std::collections::HashMap;

        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let commander = Arc::new(Commander::new(
            registry.clone(),
            Arc::new(Mutex::new(HashMap::new())),
            5,
        ));
        let proxy = Arc::new(ProxyForwarder::new(
            registry.clone(),
            Arc::new(Mutex::new(HashMap::new())),
            5,
        ));
        let user_admin = db.map(|store| {
            Arc::new(edgion_center_adapter_sql::SqlAdmin::new(store))
                as Arc<dyn edgion_center_core::UserAdmin>
        });
        ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            commander,
            proxy,
            controller_directory: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: user_admin.clone(),
            role_admin: None,
            audit_reader: None,
            cloudflare_dns_admin: None,
            cloudflare_dns_write_admin: None,
            cloudflare_waf_admin: None,
            route53_dns_admin: None,
            route53_dns_write_admin: None,
            route53_zone_lifecycle_admin: None,
            cloudfront_admin: None,
            aws_waf_admin: None,
            provider_account_store: None,
            capability_snapshot_store: None,
            credential_inspection_service: None,
            metadata_store,
            sync_client,
            registry,
            platform_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            authz_mode: edgion_center_core::AuthzMode::Rbac,
            platform_mode: edgion_center_core::CenterMode::Standalone,
            capabilities: edgion_center_core::CenterCapabilities::resolved(
                user_admin.is_some(),
                false,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        }
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        if bytes.is_empty() {
            return serde_json::Value::Null;
        }
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn users_create_list_roundtrip() {
        let db = Arc::new(Store::open_in_memory().await.unwrap());
        let admin = db.create_role("admin", "Administrators").await.unwrap();
        let state = state_with_db(Some(db.clone()));

        let req = CreateUserRequest {
            username: "alice".to_string(),
            password: "s3cret-pass".to_string(),
            display_name: Some("Alice".to_string()),
            role_ids: Some(vec![admin]),
        };
        let resp = create_user_handler(State(state.clone()), Json(req))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = list_users_handler(State(state)).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let items = json["data"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        let u = &items[0];
        assert_eq!(u["username"], "alice");
        assert_eq!(u["displayName"], "Alice");
        assert_eq!(u["status"], "active");
        // Bound role reflected by id and name.
        assert_eq!(u["roleIds"][0], admin);
        assert_eq!(u["roleNames"][0], "admin");
        // password_hash must NEVER be exposed under any key spelling.
        assert!(u.get("passwordHash").is_none());
        assert!(u.get("password_hash").is_none());
        assert!(u.get("password").is_none());
    }

    #[tokio::test]
    async fn users_duplicate_returns_409() {
        let db = Arc::new(Store::open_in_memory().await.unwrap());
        let state = state_with_db(Some(db));
        let mk = || CreateUserRequest {
            username: "bob".to_string(),
            password: "pw-bob-123".to_string(),
            display_name: None,
            role_ids: None,
        };
        let resp = create_user_handler(State(state.clone()), Json(mk()))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = create_user_handler(State(state), Json(mk()))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn users_empty_username_or_password_400() {
        let db = Arc::new(Store::open_in_memory().await.unwrap());
        let state = state_with_db(Some(db));
        let resp = create_user_handler(
            State(state.clone()),
            Json(CreateUserRequest {
                username: "".to_string(),
                password: "x".to_string(),
                display_name: None,
                role_ids: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let resp = create_user_handler(
            State(state),
            Json(CreateUserRequest {
                username: "u".to_string(),
                password: "".to_string(),
                display_name: None,
                role_ids: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn users_patch_updates_status_password_roles() {
        let db = Arc::new(Store::open_in_memory().await.unwrap());
        let r1 = db.create_role("r1", "").await.unwrap();
        let r2 = db.create_role("r2", "").await.unwrap();
        let initial = bcrypt::hash("old-password", bcrypt::DEFAULT_COST).unwrap();
        let uid = db.create_user("carol", &initial, "Carol").await.unwrap();
        db.set_user_roles(uid, &[r1]).await.unwrap();
        let state = state_with_db(Some(db.clone()));

        let resp = update_user_handler(
            State(state),
            Path(uid),
            Json(UpdateUserRequest {
                status: Some("disabled".to_string()),
                password: Some("brand-new-pass".to_string()),
                role_ids: Some(vec![r2]),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        let user = db.get_user(uid).await.unwrap().unwrap();
        assert_eq!(user.status, "disabled");
        // Verify through the adapter-owned credential seam; raw hashes never
        // cross the SQL adapter boundary. Temporarily activate the test user
        // because disabled accounts intentionally reject every password.
        db.set_user_status(uid, "active").await.unwrap();
        let dummy = bcrypt::hash("dummy", bcrypt::DEFAULT_COST).unwrap();
        assert!(db
            .verify_user_credentials("carol", "brand-new-pass".to_string(), dummy.clone())
            .await
            .unwrap());
        assert!(!db
            .verify_user_credentials("carol", "old-password".to_string(), dummy)
            .await
            .unwrap());
        db.set_user_status(uid, "disabled").await.unwrap();
        // Roles replaced (r1 -> r2), not appended.
        assert_eq!(db.user_role_ids(uid).await.unwrap(), vec![r2]);
    }

    #[tokio::test]
    async fn patch_rejects_without_partial_apply() {
        let db = Arc::new(Store::open_in_memory().await.unwrap());
        let initial = bcrypt::hash("pw", bcrypt::DEFAULT_COST).unwrap();
        let uid = db.create_user("dave", &initial, "Dave").await.unwrap();
        assert_eq!(db.get_user(uid).await.unwrap().unwrap().status, "active");
        let state = state_with_db(Some(db.clone()));

        // {status:"disabled", password:""} — the empty password is invalid, so the
        // whole PATCH must 400 WITHOUT having applied the (valid) status first.
        let resp = update_user_handler(
            State(state),
            Path(uid),
            Json(UpdateUserRequest {
                status: Some("disabled".to_string()),
                password: Some("".to_string()),
                role_ids: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Status must be UNCHANGED — proving no partial write happened.
        assert_eq!(db.get_user(uid).await.unwrap().unwrap().status, "active");
    }

    #[tokio::test]
    async fn patch_rejects_unknown_status() {
        let db = Arc::new(Store::open_in_memory().await.unwrap());
        let initial = bcrypt::hash("pw", bcrypt::DEFAULT_COST).unwrap();
        let uid = db.create_user("erin", &initial, "Erin").await.unwrap();
        let state = state_with_db(Some(db.clone()));

        let resp = update_user_handler(
            State(state),
            Path(uid),
            Json(UpdateUserRequest {
                status: Some("bogus".to_string()),
                password: None,
                role_ids: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // The unknown status must NOT have been persisted.
        assert_eq!(db.get_user(uid).await.unwrap().unwrap().status, "active");
    }

    #[tokio::test]
    async fn db_disabled_returns_503() {
        let state = state_with_db(None);
        let resp = list_users_handler(State(state)).await.into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
