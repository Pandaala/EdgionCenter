//! Admin HTTP API for edgion-center.
//!
//! Listeners:
//!   - Admin API (http_addr / 12201):  business routes + auth middleware
//!   - Probe     (probe_addr / 12200): GET /health, GET /ready (no auth)
//!   - Metrics   (metrics_addr / 12290): GET /metrics (no auth)
//!
//! Admin routes:
//!   GET  /api/v1/server-info                              → public platform and capability discovery
//!   GET  /api/v1/controllers                              → list all controller summaries
//!   GET  /api/v1/clusters                                 → list distinct cluster names
//!   POST /api/v1/controllers/{id}/reload                  → send reload command
//!   GET  /api/v1/center/region-routes                              → aggregated effective region routes (unified)
//!   POST /api/v1/center/region-routes/failover                     → fan-out failover to all online controllers (unified)
//!   GET  /api/v1/center/region-routes/consistency                  → cross-controller consistency check (unified, online-only)
//!   GET  /api/v1/center/cluster-region-routes                      → 308 redirect → /api/v1/center/region-routes
//!   GET  /api/v1/center/service-region-routes                      → 308 redirect → /api/v1/center/region-routes
//!   POST /api/v1/center/cluster-region-routes/failover             → 308 redirect → /api/v1/center/region-routes/failover
//!   POST /api/v1/center/service-region-routes/failover             → 308 redirect → /api/v1/center/region-routes/failover
//!   GET  /api/v1/center/cluster-region-routes/consistency          → 308 redirect → /api/v1/center/region-routes/consistency
//!   GET  /api/v1/center/service-region-routes/consistency          → 308 redirect → /api/v1/center/region-routes/consistency
//!   GET    /api/v1/center/global-connection-ip-restrictions                        → aggregated GlobalConnectionIpRestriction list from MetaDataStore
//!   GET    /api/v1/center/global-connection-ip-restrictions/{ns}/{name}            → single GlobalConnectionIpRestriction detail
//!   PATCH  /api/v1/center/global-connection-ip-restrictions/{ns}/{name}/active-profile → switch active profile (fan-out Selector PUT to target controllers)
//!   GET    /api/v1/center/global-connection-ip-restrictions/consistency            → consistency detection across controllers
//!   GET    /api/v1/center/admin/users                              → list users (with role ids + names; no password_hash)
//!   POST   /api/v1/center/admin/users                              → create user (bcrypt password; optional role bindings)
//!   PATCH  /api/v1/center/admin/users/{id}                         → partial update (status / password reset / role rebind)
//!   DELETE /api/v1/center/admin/users/{id}                         → delete user
//!   GET    /api/v1/center/admin/roles                              → list roles (each with permission_keys)
//!   POST   /api/v1/center/admin/roles                              → create role (optional permission set)
//!   PUT    /api/v1/center/admin/roles/{id}/permissions            → replace a role's permission set
//!   DELETE /api/v1/center/admin/roles/{id}                         → delete role (FK cascade removes bindings)
//!   GET    /api/v1/center/admin/permission-catalog                → grouped permission catalog for the matrix UI
//!   GET  /api/v1/center/admin/watch-status                          → watch cache sync status per controller
//!   GET  /api/v1/center/admin/metadata-store                         → metadata store key summary
//!   GET  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones   → Cloudflare zone inventory
//!   GET  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id} → Cloudflare zone detail
//!   GET  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets → Cloudflare RRset inventory
//!   GET  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets/{record_type} → Cloudflare RRset detail
//!   GET  /api/v1/center/cloud/provider-capabilities/accounts/{account_id} → sanitized capability snapshot
//!   ANY  /api/v1/proxy/{controller_id}/*rest                       → proxy HTTP request to controller

use axum::{
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, HeaderName, StatusCode, Uri},
    response::IntoResponse,
    routing::{any, delete, get, patch, post},
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;

/// Headers that are safe and necessary for the Controller's in-process Admin
/// router. Center authentication credentials and proxy/hop-by-hop metadata must
/// never cross the federation trust boundary.
const PROXY_REQUEST_HEADER_ALLOWLIST: &[&str] =
    &["accept", "content-type", "if-match", "user-agent"];

/// Response metadata that may be reflected from a Controller onto the Center
/// origin. In particular, `set-cookie` is intentionally absent: a Controller
/// must never be able to create, replace, or clear a Center browser session.
const PROXY_RESPONSE_HEADER_ALLOWLIST: &[&str] = &[
    "cache-control",
    "content-language",
    "content-length",
    "content-type",
    "etag",
    "last-modified",
    "location",
    "retry-after",
    "x-request-id",
];

mod audit;
pub mod cloudflare_dns;
mod consistency_handlers;
mod global_connection_ip_restriction_handlers;
pub mod provider_accounts;
pub mod provider_capabilities;
pub mod provider_credential_inspections;
mod region_route_handlers;
mod roles;
#[cfg(feature = "password-auth")]
mod users;
pub mod web;

use crate::aggregator::ResourceAggregator;
use crate::commander::Commander;
use crate::common::api::{ApiResponse, ListResponse};
use crate::common::fed_sync::proto::command_request::Command;
use crate::common::fed_sync::proto::ReloadCommand;
use crate::fed_sync::registry::ControllerRegistry;
use crate::metadata_store::CenterMetaDataStore;
use crate::proxy::ProxyForwarder;
use crate::watch_cache::CenterSyncClient;

#[derive(Clone)]
pub struct ApiState {
    pub aggregator: Arc<ResourceAggregator>,
    pub commander: Arc<Commander>,
    pub proxy: Arc<ProxyForwarder>,
    pub controller_directory: Option<Arc<dyn edgion_center_core::ControllerDirectory>>,
    pub controller_evictor: Arc<dyn edgion_center_runtime::eviction::ControllerEviction>,
    pub user_admin: Option<Arc<dyn edgion_center_core::UserAdmin>>,
    pub role_admin: Option<Arc<dyn edgion_center_core::RoleAdmin>>,
    pub audit_reader: Option<Arc<dyn edgion_center_core::AuditReader>>,
    /// Optional SDK-free Cloudflare DNS read service. Provider clients and credentials remain
    /// behind the composition boundary.
    pub cloudflare_dns_admin: Option<cloudflare_dns::SharedCloudflareDnsAdminService>,
    /// Optional secret-free provider account desired-state store.
    pub provider_account_store: Option<Arc<dyn edgion_center_core::ProviderAccountStore>>,
    /// Optional capability snapshot store. Admin handlers only perform exact-key reads.
    pub capability_snapshot_store: Option<Arc<dyn edgion_center_core::CapabilitySnapshotStore>>,
    /// Optional bounded credential inspection orchestration. Provider clients
    /// and resolved credentials remain behind this runtime service.
    pub credential_inspection_service:
        Option<edgion_center_runtime::cloud::CredentialInspectionService>,
    pub metadata_store: Arc<CenterMetaDataStore>,
    pub sync_client: Arc<CenterSyncClient>,
    /// Needed by Admin DELETE to cascade eviction into the fed-sync registry.
    /// MetaDataStore is cleaned via `sync_client.plugin_metadata.remove_controller`
    /// (triggers `CenterConfHandler::controller_removed`), not directly.
    pub registry: ControllerRegistry,
    /// Live readiness of required platform dependencies.
    pub platform_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Configured authorization mode (`allow_all` / `rbac`), exposed as
    /// descriptive compatibility metadata. Feature availability comes only
    /// from `capabilities`.
    pub authz_mode: edgion_center_core::AuthzMode,
    pub platform_mode: edgion_center_core::CenterMode,
    /// Capabilities resolved from the adapters actually composed at startup.
    pub capabilities: edgion_center_core::CenterCapabilities,
}

impl ApiState {
    /// Returns `true` when Center is ready to handle all API requests.
    ///
    /// The composition root owns the dependency checks and updates the shared
    /// bit when their health changes.
    pub fn is_ready(&self) -> bool {
        self.platform_ready
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Return the durable global Controller membership when a platform
    /// directory is composed. Tests and deliberately minimal compositions may
    /// omit the directory and retain the original in-memory behavior.
    pub async fn controller_summaries(
        &self,
    ) -> edgion_center_core::CoreResult<Vec<crate::aggregator::ControllerSummary>> {
        let local = self.aggregator.controller_summaries();
        let Some(directory) = &self.controller_directory else {
            let mut summaries = local;
            for summary in &mut summaries {
                summary.last_seen_secs_ago =
                    self.registry.last_seen_secs_ago(&summary.controller_id);
            }
            return Ok(summaries);
        };

        let enrichments: std::collections::HashMap<_, _> = local
            .into_iter()
            .map(|summary| (summary.controller_id.clone(), summary))
            .collect();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        Ok(directory
            .list()
            .await?
            .into_iter()
            .map(|record| {
                let id = record.controller_id.to_string();
                let enrichment = enrichments.get(&id);
                crate::aggregator::ControllerSummary {
                    controller_id: id,
                    cluster: record.cluster,
                    env: record.environments,
                    tag: record.tags,
                    online: record.phase == edgion_center_core::ControllerPhase::Online,
                    key_count: record
                        .resource_count
                        .or_else(|| enrichment.and_then(|summary| summary.key_count)),
                    stats_updated_secs_ago: record
                        .stats_updated_unix_ms
                        .map(|updated_at| now_ms.saturating_sub(updated_at) as u64 / 1_000),
                    last_seen_secs_ago: Some(
                        now_ms.saturating_sub(record.last_seen_unix_ms) as u64 / 1_000,
                    ),
                }
            })
            .collect())
    }

    pub async fn online_controller_ids(&self) -> edgion_center_core::CoreResult<Vec<String>> {
        Ok(self
            .controller_summaries()
            .await?
            .into_iter()
            .filter(|summary| summary.online)
            .map(|summary| summary.controller_id)
            .collect())
    }

    pub fn require_effective_read_model(&self) -> edgion_center_core::CoreResult<()> {
        if self.platform_mode == edgion_center_core::CenterMode::Kubernetes && !self.is_ready() {
            return Err(edgion_center_core::CoreError::Adapter(
                "global effective read model is not ready".to_string(),
            ));
        }
        Ok(())
    }
}

pub fn router(mut state: ApiState) -> Router {
    // Advertise only capabilities that are actually composed. Keeping this
    // effective value in state also makes `/server-info` and route mounting
    // report the same surface when a composition is incomplete.
    state.capabilities.cloudflare_dns_read &= state.cloudflare_dns_admin.is_some();
    state.capabilities.provider_account_admin &= state.provider_account_store.is_some();
    state.capabilities.provider_capability_read &=
        state.provider_account_store.is_some() && state.capability_snapshot_store.is_some();
    state.capabilities.provider_credential_inspection &=
        state.credential_inspection_service.is_some();
    let capabilities = state.capabilities.clone();
    let mut app = Router::new()
        // Center-specific endpoints
        .route("/api/v1/server-info", get(server_info))
        .route("/api/v1/controllers", get(list_controllers))
        .route("/api/v1/clusters", get(list_clusters))
        .route("/api/v1/controllers/{id}/reload", post(reload_controller))
        // MetaDataStore-backed RegionRoute endpoints
        .route(
            "/api/v1/center/region-routes",
            get(region_route_handlers::list_region_routes),
        )
        // Legacy paths redirect permanently (308) to the unified endpoint above.
        .route(
            "/api/v1/center/cluster-region-routes",
            get(|| async { axum::response::Redirect::permanent("/api/v1/center/region-routes") }),
        )
        .route(
            "/api/v1/center/service-region-routes",
            get(|| async { axum::response::Redirect::permanent("/api/v1/center/region-routes") }),
        )
        // RegionRoute failover (unified endpoint; legacy paths redirect 308)
        .route(
            "/api/v1/center/region-routes/failover",
            post(region_route_handlers::region_route_failover),
        )
        .route(
            "/api/v1/center/cluster-region-routes/failover",
            post(|| async {
                axum::response::Redirect::permanent("/api/v1/center/region-routes/failover")
            }),
        )
        .route(
            "/api/v1/center/service-region-routes/failover",
            post(|| async {
                axum::response::Redirect::permanent("/api/v1/center/region-routes/failover")
            }),
        )
        // RegionRoute consistency (unified endpoint; legacy paths redirect 308)
        .route(
            "/api/v1/center/region-routes/consistency",
            get(consistency_handlers::region_routes_consistency),
        )
        .route(
            "/api/v1/center/cluster-region-routes/consistency",
            get(|| async {
                axum::response::Redirect::permanent("/api/v1/center/region-routes/consistency")
            }),
        )
        .route(
            "/api/v1/center/service-region-routes/consistency",
            get(|| async {
                axum::response::Redirect::permanent("/api/v1/center/region-routes/consistency")
            }),
        )
        // GlobalConnectionIpRestriction endpoints (read + active-profile write only; base CRUD retired)
        .route(
            "/api/v1/center/global-connection-ip-restrictions",
            get(global_connection_ip_restriction_handlers::list_global_ip_restrictions),
        )
        .route(
            "/api/v1/center/global-connection-ip-restrictions/{ns}/{name}",
            get(global_connection_ip_restriction_handlers::get_global_ip_restriction),
        )
        .route(
            "/api/v1/center/global-connection-ip-restrictions/{ns}/{name}/active-profile",
            patch(global_connection_ip_restriction_handlers::patch_active_profile),
        )
        .route(
            "/api/v1/center/global-connection-ip-restrictions/consistency",
            get(global_connection_ip_restriction_handlers::global_ip_restrictions_consistency),
        )
        // Watch cache admin endpoints
        .route("/api/v1/center/admin/watch-status", get(watch_status))
        .route(
            "/api/v1/center/admin/metadata-store",
            get(metadata_store_status),
        )
        // HTTP proxy to controllers
        .route("/api/v1/proxy/{controller_id}/{*rest}", any(proxy_handler));

    if capabilities.controller_history {
        app = app
            .route(
                "/api/v1/center/admin/controllers",
                get(list_admin_controllers),
            )
            .route(
                "/api/v1/center/admin/controllers/{id}",
                delete(delete_admin_controller),
            );
    }
    #[cfg(feature = "password-auth")]
    if capabilities.user_admin {
        app = app
            .route(
                "/api/v1/center/admin/users",
                get(users::list_users_handler).post(users::create_user_handler),
            )
            .route(
                "/api/v1/center/admin/users/{id}",
                patch(users::update_user_handler).delete(users::delete_user_handler),
            );
    }
    if capabilities.role_admin {
        app = app
            .route(
                "/api/v1/center/admin/roles",
                get(roles::list_roles_handler).post(roles::create_role_handler),
            )
            .route(
                "/api/v1/center/admin/roles/{id}",
                delete(roles::delete_role_handler),
            )
            .route(
                "/api/v1/center/admin/roles/{id}/permissions",
                axum::routing::put(roles::set_role_permissions_handler),
            )
            .route(
                "/api/v1/center/admin/permission-catalog",
                get(roles::permission_catalog_handler),
            );
    }
    if capabilities.audit_query {
        app = app.route(
            "/api/v1/center/admin/audit-logs",
            get(audit::audit_list_handler),
        );
    }
    if capabilities.cloudflare_dns_read && state.cloudflare_dns_admin.is_some() {
        app = app
            .route(
                "/api/v1/center/cloudflare/dns/accounts/{account_id}/zones",
                get(cloudflare_dns::list_zones),
            )
            .route(
                "/api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}",
                get(cloudflare_dns::get_zone),
            )
            .route(
                "/api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets",
                get(cloudflare_dns::list_record_sets),
            )
            .route(
                "/api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets/{record_type}",
                get(cloudflare_dns::get_record_set),
            );
    }
    if capabilities.provider_account_admin && state.provider_account_store.is_some() {
        let provider_account_routes = Router::new()
            .route(
                "/api/v1/center/cloud/provider-accounts",
                get(provider_accounts::list).post(provider_accounts::create),
            )
            .route(
                "/api/v1/center/cloud/provider-accounts/{account_id}",
                get(provider_accounts::get).put(provider_accounts::replace),
            )
            .layer(axum::extract::DefaultBodyLimit::max(70 * 1024));
        app = app.merge(provider_account_routes);
    }
    if capabilities.provider_capability_read
        && state.provider_account_store.is_some()
        && state.capability_snapshot_store.is_some()
    {
        app = app.route(
            "/api/v1/center/cloud/provider-capabilities/accounts/{account_id}",
            get(provider_capabilities::get),
        );
    }
    if capabilities.provider_credential_inspection && state.credential_inspection_service.is_some()
    {
        app = app.route(
            "/api/v1/center/cloud/provider-credential-inspections/accounts/{account_id}/refresh",
            post(provider_credential_inspections::refresh),
        );
    }

    app.with_state(state)
}

/// Dedicated liveness/readiness router (own listener).
pub fn create_probe_router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(ready_check))
        .with_state(state)
}

/// Dedicated Prometheus metrics router (own listener, stateless handler).
pub fn create_metrics_router() -> Router {
    Router::new().route(
        "/metrics",
        get(crate::common::observe::metrics_api::metrics_handler),
    )
}

async fn health_check() -> impl IntoResponse {
    Json(ApiResponse::ok_body("ok".to_string()))
}

/// Readiness check endpoint - returns 200 OK only when Center is fully operational.
///
/// Returns 503 when `database.enabled = true` but the metadata store failed to open
/// at startup, leaving DB-backed endpoints permanently degraded.
async fn ready_check(State(state): State<ApiState>) -> impl IntoResponse {
    if state.is_ready() {
        (StatusCode::OK, Json(ApiResponse::ok_body("ok".to_string())))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::<String>::err_body(
                "platform dependencies unavailable; Center is not ready".to_string(),
            )),
        )
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfoResponse {
    mode: String,
    /// Authorization mode: `"allow_all"` or `"rbac"`. The dashboard uses this to
    /// decide whether to surface user/role management (only meaningful under
    /// `rbac`, where permissions are enforced per subject).
    authz_mode: edgion_center_core::AuthzMode,
    /// Whether DB-backed user login is enabled (`db_auth.enabled`).
    db_auth_enabled: bool,
    platform_mode: edgion_center_core::CenterMode,
    capabilities: edgion_center_core::CenterCapabilities,
}

async fn server_info(State(state): State<ApiState>) -> impl IntoResponse {
    Json(ApiResponse::ok_body(ServerInfoResponse {
        mode: "center".to_string(),
        authz_mode: state.authz_mode,
        db_auth_enabled: state.capabilities.password_login,
        platform_mode: state.platform_mode,
        capabilities: state.capabilities.clone(),
    }))
}

async fn list_controllers(State(state): State<ApiState>) -> impl IntoResponse {
    match state.controller_summaries().await {
        Ok(summaries) => Json(ListResponse::success(summaries)).into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ListResponse::<crate::aggregator::ControllerSummary>::error(
                error.to_string(),
            )),
        )
            .into_response(),
    }
}

async fn list_clusters(State(state): State<ApiState>) -> impl IntoResponse {
    match state.controller_summaries().await {
        Ok(summaries) => {
            let mut clusters: Vec<String> = summaries
                .into_iter()
                .map(|summary| summary.cluster)
                .collect();
            clusters.sort();
            clusters.dedup();
            Json(ListResponse::success(clusters)).into_response()
        }
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ListResponse::<String>::error(error.to_string())),
        )
            .into_response(),
    }
}

async fn reload_controller(
    State(state): State<ApiState>,
    Path(id_raw): Path<String>,
) -> impl IntoResponse {
    let id = id_raw.replace('~', "/");
    match state
        .commander
        .send_command(&id, Command::Reload(ReloadCommand {}))
        .await
    {
        Ok(resp) if resp.success => (StatusCode::OK, Json(ApiResponse::ok_body("ok".to_string()))),
        Ok(resp) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<String>::err_body(resp.message)),
        ),
        Err(e) => {
            let status = if e.to_string().contains("timed out") {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(ApiResponse::<String>::err_body(e.to_string())))
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminControllerDto {
    controller_id: String,
    cluster: String,
    env: Vec<String>,
    tag: Vec<String>,
    online: bool,
    last_seen_at: i64,
}

impl From<edgion_center_core::ControllerRecord> for AdminControllerDto {
    fn from(r: edgion_center_core::ControllerRecord) -> Self {
        Self {
            controller_id: r.controller_id.as_str().to_string(),
            cluster: r.cluster,
            env: r.environments,
            tag: r.tags,
            online: r.phase == edgion_center_core::ControllerPhase::Online,
            last_seen_at: r.last_seen_unix_ms / 1000,
        }
    }
}

async fn list_admin_controllers(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(directory) = &state.controller_directory else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ListResponse::<AdminControllerDto>::error(
                "controller history unavailable".to_string(),
            )),
        )
            .into_response();
    };
    match directory.list().await {
        Ok(rows) => {
            let dtos: Vec<AdminControllerDto> =
                rows.into_iter().map(AdminControllerDto::from).collect();
            Json(ListResponse::success(dtos)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ListResponse::<AdminControllerDto>::error(e.to_string())),
        )
            .into_response(),
    }
}

async fn delete_admin_controller(
    State(state): State<ApiState>,
    Path(id_raw): Path<String>,
) -> impl IntoResponse {
    let Some(directory) = &state.controller_directory else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::<String>::err_body(
                "controller management unavailable".to_string(),
            )),
        )
            .into_response();
    };
    let id = id_raw.replace('~', "/");

    let controller_id = match edgion_center_core::ControllerId::new(id) {
        Ok(id) => id,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<String>::err_body(error.to_string())),
            )
                .into_response();
        }
    };
    match directory.evict(&controller_id).await {
        Ok(eviction) => match state
            .controller_evictor
            .evict_live(&controller_id, eviction.target.as_ref())
            .await
        {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ApiResponse::<String>::err_body(format!(
                    "durable eviction succeeded but live owner cleanup failed: {error}"
                ))),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<String>::err_body(e.to_string())),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WatchControllerStatus {
    controller_id: String,
    sync_version: u64,
    server_id: String,
}

async fn watch_status(State(state): State<ApiState>) -> impl IntoResponse {
    if state.platform_mode == edgion_center_core::CenterMode::Kubernetes {
        let Some(directory) = &state.controller_directory else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ListResponse::<WatchControllerStatus>::error(
                    "global watch status unavailable".to_string(),
                )),
            )
                .into_response();
        };
        return match directory.list().await {
            Ok(records) => Json(ListResponse::success(
                records
                    .into_iter()
                    .map(|record| WatchControllerStatus {
                        controller_id: record.controller_id.to_string(),
                        sync_version: record.sync_version.unwrap_or_default(),
                        server_id: record.watch_server_id.unwrap_or_default(),
                    })
                    .collect(),
            ))
            .into_response(),
            Err(error) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ListResponse::<WatchControllerStatus>::error(
                    error.to_string(),
                )),
            )
                .into_response(),
        };
    }
    let entries = state.sync_client.plugin_metadata.list_controllers();
    let dtos: Vec<WatchControllerStatus> = entries
        .into_iter()
        .map(|(id, ver, sid)| WatchControllerStatus {
            controller_id: id,
            sync_version: ver,
            server_id: sid,
        })
        .collect();
    Json(ListResponse::success(dtos)).into_response()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MetaDataStoreStatus {
    region_routes: Vec<MetaDataStoreEntry>,
    global_connection_ip_restrictions: Vec<MetaDataStoreEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MetaDataStoreEntry {
    key: String,
    controller_count: usize,
}

async fn metadata_store_status(State(state): State<ApiState>) -> impl IntoResponse {
    let (region_routes, restrictions) = state.metadata_store.status_entries();
    Json(ApiResponse::ok_body(MetaDataStoreStatus {
        region_routes: region_routes
            .into_iter()
            .map(|(key, controller_count)| MetaDataStoreEntry {
                key,
                controller_count,
            })
            .collect(),
        global_connection_ip_restrictions: restrictions
            .into_iter()
            .map(|(key, controller_count)| MetaDataStoreEntry {
                key,
                controller_count,
            })
            .collect(),
    }))
}

async fn proxy_handler(
    State(state): State<ApiState>,
    Path((controller_id_raw, rest)): Path<(String, String)>,
    OriginalUri(original_uri): OriginalUri,
    method: axum::http::Method,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Frontend uses "~" instead of "/" in controller_id to avoid browser URL decoding issues
    let controller_id = controller_id_raw.replace('~', "/");

    let headers_map = proxy_request_headers(&headers);

    // Ensure path starts with / for forwarding
    let path = proxy_forward_path(rest, &original_uri);

    match state
        .proxy
        .forward(
            &controller_id,
            method.to_string(),
            path,
            headers_map,
            body.to_vec(),
        )
        .await
    {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status_code as u16)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

            let mut builder = axum::http::Response::builder().status(status);

            for (key, value) in proxy_response_headers(&resp.headers) {
                builder = builder.header(key, value);
            }

            builder
                .body(axum::body::Body::from(resp.body))
                .unwrap_or_else(|_| {
                    axum::http::Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(axum::body::Body::empty())
                        .unwrap()
                })
        }
        Err((status, message)) => {
            tracing::warn!(
                component = "center",
                controller_id = %controller_id,
                status = %status,
                error = %message,
                "Proxy request failed"
            );
            axum::http::Response::builder()
                .status(status)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&ApiResponse::<()>::err_body(message)).unwrap_or_default(),
                ))
                .unwrap_or_else(|_| {
                    axum::http::Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(axum::body::Body::empty())
                        .unwrap()
                })
        }
    }
}

fn header_is_allowed(name: &HeaderName, allowlist: &[&str]) -> bool {
    allowlist
        .iter()
        .any(|allowed| name.as_str().eq_ignore_ascii_case(allowed))
}

fn proxy_forward_path(rest: String, original_uri: &Uri) -> String {
    let mut path = if rest.starts_with('/') {
        rest
    } else {
        format!("/{rest}")
    };
    if let Some(query) = original_uri.query() {
        path.push('?');
        path.push_str(query);
    }
    path
}

fn proxy_request_headers(headers: &HeaderMap) -> std::collections::HashMap<String, String> {
    headers
        .iter()
        .filter(|(name, _)| header_is_allowed(name, PROXY_REQUEST_HEADER_ALLOWLIST))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn proxy_response_headers(
    headers: &std::collections::HashMap<String, String>,
) -> Vec<(HeaderName, axum::http::HeaderValue)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            let name = HeaderName::try_from(name.as_str()).ok()?;
            if !header_is_allowed(&name, PROXY_RESPONSE_HEADER_ALLOWLIST) {
                return None;
            }
            let value = axum::http::HeaderValue::try_from(value.as_str()).ok()?;
            Some((name, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_center_core::{
        AuthzMode, ControllerDirectory, ControllerId, ControllerPhase, ControllerRecord,
        ControllerRegistration, CoreResult, EvictionResult, OfflineOutcome, OwnershipFence,
        SessionId,
    };

    #[test]
    fn proxy_request_headers_drop_credentials_and_hop_by_hop_metadata() {
        let headers = HeaderMap::from_iter([
            (
                axum::http::header::ACCEPT,
                "application/json".parse().unwrap(),
            ),
            (
                axum::http::header::CONTENT_TYPE,
                "application/yaml".parse().unwrap(),
            ),
            (axum::http::header::IF_MATCH, "\"42\"".parse().unwrap()),
            (
                axum::http::header::AUTHORIZATION,
                "Bearer center-secret".parse().unwrap(),
            ),
            (
                axum::http::header::COOKIE,
                "edgion_token=center-secret".parse().unwrap(),
            ),
            (
                axum::http::header::PROXY_AUTHORIZATION,
                "Basic secret".parse().unwrap(),
            ),
            (
                axum::http::header::CONNECTION,
                "keep-alive".parse().unwrap(),
            ),
            (
                HeaderName::from_static("x-forwarded-for"),
                "127.0.0.1".parse().unwrap(),
            ),
        ]);

        let forwarded = proxy_request_headers(&headers);
        assert_eq!(
            forwarded.get("accept").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(
            forwarded.get("content-type").map(String::as_str),
            Some("application/yaml")
        );
        assert_eq!(
            forwarded.get("if-match").map(String::as_str),
            Some("\"42\"")
        );
        for forbidden in [
            "authorization",
            "cookie",
            "proxy-authorization",
            "connection",
            "x-forwarded-for",
        ] {
            assert!(!forwarded.contains_key(forbidden), "forwarded {forbidden}");
        }
    }

    #[test]
    fn proxy_response_headers_never_reflect_cookies_or_hop_by_hop_metadata() {
        let source = std::collections::HashMap::from([
            ("content-type".to_string(), "application/json".to_string()),
            ("etag".to_string(), "\"revision-1\"".to_string()),
            (
                "set-cookie".to_string(),
                "edgion_token=attacker".to_string(),
            ),
            ("connection".to_string(), "keep-alive".to_string()),
            ("transfer-encoding".to_string(), "chunked".to_string()),
        ]);

        let forwarded = proxy_response_headers(&source)
            .into_iter()
            .map(|(name, _)| name.to_string())
            .collect::<std::collections::HashSet<_>>();
        assert!(forwarded.contains("content-type"));
        assert!(forwarded.contains("etag"));
        assert!(!forwarded.contains("set-cookie"));
        assert!(!forwarded.contains("connection"));
        assert!(!forwarded.contains("transfer-encoding"));
    }

    #[test]
    fn proxy_query_is_preserved_verbatim() {
        let uri: Uri =
            "/api/v1/proxy/east~controller/api/v1/namespaced/httproute?limit=20&continue=a%2Fb"
                .parse()
                .unwrap();
        let path = proxy_forward_path("api/v1/namespaced/httproute".to_string(), &uri);
        assert_eq!(path, "/api/v1/namespaced/httproute?limit=20&continue=a%2Fb");
    }

    struct GlobalDirectory(Vec<ControllerRecord>);

    #[async_trait::async_trait]
    impl ControllerDirectory for GlobalDirectory {
        async fn upsert_registration(&self, _: ControllerRegistration) -> CoreResult<()> {
            unreachable!()
        }
        async fn mark_offline(
            &self,
            _: &ControllerId,
            _: &SessionId,
            _: Option<&OwnershipFence>,
            _: i64,
        ) -> CoreResult<OfflineOutcome> {
            unreachable!()
        }
        async fn list(&self) -> CoreResult<Vec<ControllerRecord>> {
            Ok(self.0.clone())
        }
        async fn evict(&self, _: &ControllerId) -> CoreResult<EvictionResult> {
            unreachable!()
        }
    }

    /// Build an `ApiState` carrying `authz_mode` + `db_auth_enabled`; every other
    /// field is a minimal default sufficient for the stateless `server_info`
    /// handler.
    fn state_with_authz_mode(authz_mode: AuthzMode, db_auth_enabled: bool) -> ApiState {
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
        ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            commander,
            proxy,
            controller_directory: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: None,
            role_admin: None,
            audit_reader: None,
            cloudflare_dns_admin: None,
            provider_account_store: None,
            capability_snapshot_store: None,
            credential_inspection_service: None,
            metadata_store,
            sync_client,
            registry,
            platform_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            authz_mode,
            platform_mode: edgion_center_core::CenterMode::Standalone,
            capabilities: edgion_center_core::CenterCapabilities::resolved(
                false,
                false,
                false,
                false,
                false,
                false,
                db_auth_enabled,
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
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn server_info_reports_authz_and_db_auth() {
        // RBAC authz + DB-user login on: authzMode=rbac, dbAuthEnabled=true,
        // and the legacy accessMode key is gone from the wire.
        let resp = server_info(State(state_with_authz_mode(AuthzMode::Rbac, true)))
            .await
            .into_response();
        let json = body_json(resp).await;
        assert_eq!(json["data"]["mode"], "center");
        assert_eq!(json["data"]["authzMode"], "rbac");
        assert_eq!(json["data"]["dbAuthEnabled"], true);
        assert_eq!(json["data"]["platformMode"], "standalone");
        assert_eq!(json["data"]["capabilities"]["passwordLogin"], true);
        assert!(
            json["data"]["accessMode"].is_null(),
            "accessMode must be gone"
        );

        // AllowAll authz + DB-user login off: authzMode=allow_all, dbAuthEnabled=false.
        let resp = server_info(State(state_with_authz_mode(AuthzMode::AllowAll, false)))
            .await
            .into_response();
        let json = body_json(resp).await;
        assert_eq!(json["data"]["authzMode"], "allow_all");
        assert_eq!(json["data"]["dbAuthEnabled"], false);

        let mut kubernetes = state_with_authz_mode(AuthzMode::Rbac, false);
        kubernetes.platform_mode = edgion_center_core::CenterMode::Kubernetes;
        kubernetes.capabilities = edgion_center_core::CenterCapabilities::for_mode(
            edgion_center_core::CenterMode::Kubernetes,
        );
        let json = body_json(server_info(State(kubernetes)).await.into_response()).await;
        assert_eq!(json["data"]["platformMode"], "kubernetes");
        assert_eq!(json["data"]["capabilities"]["nativeRbac"], true);
        assert_eq!(json["data"]["capabilities"]["passwordLogin"], false);
    }

    #[test]
    fn readiness_tracks_live_platform_health() {
        let state = state_with_authz_mode(AuthzMode::Rbac, false);
        assert!(state.is_ready());
        state
            .platform_ready
            .store(false, std::sync::atomic::Ordering::Release);
        assert!(!state.is_ready());
    }

    #[tokio::test]
    async fn kubernetes_controller_reads_use_global_directory_without_local_session() {
        let mut state = state_with_authz_mode(AuthzMode::Rbac, false);
        state.platform_mode = edgion_center_core::CenterMode::Kubernetes;
        state.controller_directory = Some(Arc::new(GlobalDirectory(vec![ControllerRecord {
            controller_id: ControllerId::new("cluster-a/controller-0").unwrap(),
            current_session_id: Some(SessionId::new("session-1").unwrap()),
            cluster: "cluster-a".to_string(),
            environments: vec!["prod".to_string()],
            tags: vec!["east".to_string()],
            connected_replica: Some("center-a/uid-a".to_string()),
            ownership_fence: Some(OwnershipFence {
                token: "token-1".to_string(),
                epoch: 1,
            }),
            sync_version: Some(7),
            watch_server_id: Some("server-7".to_string()),
            resource_count: Some(42),
            stats_updated_unix_ms: Some(1),
            watch_updated_unix_ms: Some(1),
            phase: ControllerPhase::Online,
            last_seen_unix_ms: 1,
        }])));

        let summaries = state.controller_summaries().await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].controller_id, "cluster-a/controller-0");
        assert_eq!(summaries[0].key_count, Some(42));
        let response = list_clusters(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["data"], serde_json::json!(["cluster-a"]));
    }

    #[tokio::test]
    async fn unavailable_management_capabilities_do_not_mount_routes() {
        use tower::ServiceExt;

        let app = router(state_with_authz_mode(AuthzMode::AllowAll, false));
        for path in [
            "/api/v1/center/admin/users",
            "/api/v1/center/admin/roles",
            "/api/v1/center/admin/audit-logs",
            "/api/v1/center/admin/controllers",
        ] {
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(path)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[tokio::test]
    async fn kubernetes_capabilities_mount_history_but_not_sql_management() {
        use tower::ServiceExt;

        let mut state = state_with_authz_mode(AuthzMode::Rbac, false);
        state.platform_mode = edgion_center_core::CenterMode::Kubernetes;
        state.capabilities = edgion_center_core::CenterCapabilities::for_mode(
            edgion_center_core::CenterMode::Kubernetes,
        );
        let app = router(state);
        for path in [
            "/api/v1/center/admin/users",
            "/api/v1/center/admin/roles",
            "/api/v1/center/admin/audit-logs",
        ] {
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(path)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
        let history = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/center/admin/controllers")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(history.status(), StatusCode::NOT_FOUND);
    }
}
