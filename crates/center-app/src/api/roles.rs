//! Role administration endpoints — RBAC / db_auth user management: CRUD over
//! the `roles` table and
//! its `role_permissions` bindings, plus the grouped permission catalog the
//! matrix UI renders. All `/roles` routes and `/permission-catalog` gate on the
//! `roles:manage` permission key (see
//! [`crate::common::authz::catalog::route_permission`]).
//!
//! Any `permission_key` written into `role_permissions` is validated against
//! [`crate::common::authz::catalog::all_keys`] first; an unknown key is
//! rejected with 400 so arbitrary strings never enter the store. When the
//! the role capability is unavailable these routes are not mounted; the
//! catalog is mounted alongside the capability rather than inferred from mode.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::{ApiResponse, ListResponse};
use crate::common::authz::catalog;

/// Serde view of a role row plus its permission keys. camelCase per convention.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDto {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub permission_keys: Vec<String>,
}

/// Body for `POST /roles`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub permission_keys: Option<Vec<String>>,
}

/// Body for `PUT /roles/{id}/permissions`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPermissionsRequest {
    pub permission_keys: Vec<String>,
}

fn db_unavailable_list() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ListResponse::<RoleDto>::error(
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

/// Whether an `anyhow::Error` from a Store create wraps a genuine UNIQUE
/// constraint violation (vs. any other DB failure). Works across both sqlx
/// backends: `is_unique_violation()` is provided by the `DatabaseError` trait
/// for SQLite and MySQL alike.
/// Reject (400) if any key is not in `catalog::all_keys()`. Returns the
/// offending key in the error so the caller can fix its request.
#[allow(clippy::result_large_err)]
fn validate_keys(keys: &[String]) -> Result<(), axum::response::Response> {
    if let Some(bad) = keys.iter().find(|k| !catalog::is_known_key(k)) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<String>::err_body(format!(
                "unknown permission key: '{bad}'"
            ))),
        )
            .into_response());
    }
    Ok(())
}

impl From<edgion_center_core::RoleRecord> for RoleDto {
    fn from(role: edgion_center_core::RoleRecord) -> Self {
        Self {
            id: role.id,
            name: role.name,
            description: role.description,
            permission_keys: role.permission_keys,
        }
    }
}

/// `GET /api/v1/center/admin/roles` — list roles, each with its permission keys.
pub async fn list_roles_handler(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(admin) = &state.role_admin else {
        return db_unavailable_list();
    };
    let roles = match admin.list_roles().await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ListResponse::<RoleDto>::error(e.to_string())),
            )
                .into_response();
        }
    };
    let dtos = roles.into_iter().map(RoleDto::from).collect();
    Json(ListResponse::success(dtos)).into_response()
}

/// `POST /api/v1/center/admin/roles` — create a role, optionally setting its
/// permissions. 400 on empty name or an unknown permission key, 409 on a
/// duplicate name.
pub async fn create_role_handler(
    State(state): State<ApiState>,
    Json(req): Json<CreateRoleRequest>,
) -> impl IntoResponse {
    let Some(admin) = &state.role_admin else {
        return db_unavailable();
    };
    if req.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<String>::err_body(
                "name must not be empty".to_string(),
            )),
        )
            .into_response();
    }
    let keys = req.permission_keys.unwrap_or_default();
    if let Err(resp) = validate_keys(&keys) {
        return resp;
    }

    let description = req.description.unwrap_or_default();
    let id = match admin
        .create_role(edgion_center_core::CreateRole {
            name: req.name.clone(),
            description,
            permission_keys: keys,
        })
        .await
    {
        Ok(id) => id,
        // A duplicate name violates the UNIQUE constraint → 409. Any other DB
        // failure is a genuine internal error → 500 (not a spurious "duplicate").
        Err(edgion_center_core::CoreError::Conflict(_)) => {
            return (
                StatusCode::CONFLICT,
                Json(ApiResponse::<String>::err_body(format!(
                    "role '{}' already exists",
                    req.name
                ))),
            )
                .into_response();
        }
        Err(e) => return internal_err(e),
    };

    (StatusCode::CREATED, Json(ApiResponse::ok_body(id))).into_response()
}

/// `PUT /api/v1/center/admin/roles/{id}/permissions` — replace a role's
/// permission set. 400 if any key is unknown.
pub async fn set_role_permissions_handler(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(req): Json<SetPermissionsRequest>,
) -> impl IntoResponse {
    let Some(admin) = &state.role_admin else {
        return db_unavailable();
    };
    if let Err(resp) = validate_keys(&req.permission_keys) {
        return resp;
    }
    match admin.set_permissions(id, req.permission_keys).await {
        Ok(()) => (StatusCode::OK, Json(ApiResponse::ok_body("ok".to_string()))).into_response(),
        Err(e) => internal_err(e),
    }
}

/// `DELETE /api/v1/center/admin/roles/{id}` — delete (FK cascade removes its
/// `user_roles` / `role_permissions` bindings).
pub async fn delete_role_handler(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let Some(admin) = &state.role_admin else {
        return db_unavailable();
    };
    match admin.delete_role(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal_err(e),
    }
}

/// `GET /api/v1/center/admin/permission-catalog` — the grouped permission
/// catalog for the matrix UI. Static (no DB needed); always serves.
pub async fn permission_catalog_handler() -> impl IntoResponse {
    Json(ApiResponse::ok_body(catalog::catalog_groups()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::sync::Arc;

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
        let role_admin = db.map(|store| {
            Arc::new(edgion_center_adapter_sql::SqlAdmin::new(store))
                as Arc<dyn edgion_center_core::RoleAdmin>
        });
        ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            commander,
            proxy,
            controller_directory: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: None,
            role_admin: role_admin.clone(),
            audit_reader: None,
            cloudflare_dns_admin: None,
            cloudflare_dns_write_admin: None,
            route53_dns_admin: None,
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
                false,
                role_admin.is_some(),
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
    async fn roles_create_and_set_permissions() {
        let db = Arc::new(Store::open_in_memory().await.unwrap());
        let state = state_with_db(Some(db.clone()));

        // Create with an initial permission set.
        let resp = create_role_handler(
            State(state.clone()),
            Json(CreateRoleRequest {
                name: "ops".to_string(),
                description: Some("Operators".to_string()),
                permission_keys: Some(vec![
                    "controllers:read".to_string(),
                    "controllers:write".to_string(),
                ]),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let id = body_json(resp).await["data"].as_i64().unwrap();

        // GET reflects the permission keys.
        let resp = list_roles_handler(State(state.clone()))
            .await
            .into_response();
        let json = body_json(resp).await;
        let role = json["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"].as_i64() == Some(id))
            .unwrap();
        assert_eq!(role["name"], "ops");
        assert_eq!(role["description"], "Operators");
        let keys: Vec<String> = role["permissionKeys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            keys,
            vec![
                "controllers:read".to_string(),
                "controllers:write".to_string()
            ]
        );

        // PUT replaces the set.
        let resp = set_role_permissions_handler(
            State(state.clone()),
            Path(id),
            Json(SetPermissionsRequest {
                permission_keys: vec!["audit:read".to_string()],
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            db.role_permissions(id).await.unwrap(),
            vec!["audit:read".to_string()]
        );
    }

    #[tokio::test]
    async fn role_permission_rejects_unknown_key() {
        let db = Arc::new(Store::open_in_memory().await.unwrap());
        let state = state_with_db(Some(db.clone()));

        // On create.
        let resp = create_role_handler(
            State(state.clone()),
            Json(CreateRoleRequest {
                name: "bad".to_string(),
                description: None,
                permission_keys: Some(vec!["controllers:read".to_string(), "made:up".to_string()]),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Nothing must have been persisted for the rejected create.
        assert!(db.list_roles().await.unwrap().is_empty());

        // On set-permissions.
        let id = db.create_role("ok", "").await.unwrap();
        let resp = set_role_permissions_handler(
            State(state),
            Path(id),
            Json(SetPermissionsRequest {
                permission_keys: vec!["totally:bogus".to_string()],
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(db.role_permissions(id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn roles_duplicate_returns_409() {
        let db = Arc::new(Store::open_in_memory().await.unwrap());
        let state = state_with_db(Some(db));
        let mk = || CreateRoleRequest {
            name: "ops".to_string(),
            description: None,
            permission_keys: None,
        };
        let resp = create_role_handler(State(state.clone()), Json(mk()))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::CREATED);
        // Same name again → a genuine UNIQUE violation maps to 409 (not 500).
        let resp = create_role_handler(State(state), Json(mk()))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn permission_catalog_covers_all_keys() {
        use std::collections::BTreeSet;
        let resp = permission_catalog_handler().await.into_response();
        let json = body_json(resp).await;
        let groups = json["data"].as_array().unwrap();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for g in groups {
            assert!(g["group"].is_string());
            for k in g["keys"].as_array().unwrap() {
                seen.insert(k.as_str().unwrap().to_string());
            }
        }
        let expected: BTreeSet<String> =
            catalog::all_keys().iter().map(|s| s.to_string()).collect();
        assert_eq!(
            seen, expected,
            "catalog endpoint must cover exactly all_keys()"
        );
    }

    #[tokio::test]
    async fn db_disabled_returns_503() {
        let state = state_with_db(None);
        let resp = list_roles_handler(State(state)).await.into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
