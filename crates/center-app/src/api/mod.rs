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
//!   POST /api/v1/controllers/{id}/reload                  → reload via the proxy tunnel, reported as a terminal outcome
//!   GET  /api/v1/center/global-resources/catalog          → supported kinds, namespaces, and clusters
//!   GET  /api/v1/center/global-resources/resources/{kind} → grouped cross-cluster inventory
//!   GET  /api/v1/center/global-resources/resources/{kind}/{namespace}/{name} → exact fenced cluster detail
//!   GET  /api/v1/center/region-route-overrides                     → watch-fed RegionRouteOverride rows across controllers
//!   POST /api/v1/center/region-route-overrides/failover            → set failoverTo on a RegionRouteOverride on every online controller
//!   POST /api/v1/center/region-route-overrides/sync                → copy one controller's RegionRouteOverride to selected targets
//!   GET    /api/v1/center/admin/users                              → list users (with role ids + names; no password_hash)
//!   POST   /api/v1/center/admin/users                              → create user (bcrypt password; optional role bindings)
//!   PATCH  /api/v1/center/admin/users/{id}                         → partial update (status / password reset / role rebind)
//!   DELETE /api/v1/center/admin/users/{id}                         → delete user
//!   GET    /api/v1/center/admin/roles                              → list roles (each with permission_keys)
//!   POST   /api/v1/center/admin/roles                              → create role (optional permission set)
//!   PUT    /api/v1/center/admin/roles/{id}/permissions            → replace a role's permission set
//!   DELETE /api/v1/center/admin/roles/{id}                         → delete role (FK cascade removes bindings)
//!   GET    /api/v1/center/admin/permission-catalog                → grouped permission catalog for the matrix UI
//!   GET  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones   → Cloudflare zone inventory
//!   POST /api/v1/center/cloudflare/dns/accounts/{account_id}/zones   → create a Cloudflare zone
//!   GET  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id} → Cloudflare zone detail
//!   DELETE /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id} → delete one Cloudflare zone
//!   GET  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets → Cloudflare RRset inventory
//!   GET  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets/{record_type} → Cloudflare RRset detail
//!   PUT  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets/{record_type} → create/replace one Cloudflare RRset
//!   PUT  /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets/{record_type}/remote-control → create/replace one remotely marked Cloudflare RRset
//!   DELETE /api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets/{record_type} → delete one Cloudflare RRset
//!   GET  /api/v1/center/aws/route53/accounts/{account_id}/hosted-zones → Route 53 public hosted-zone inventory
//!   GET  /api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id} → Route 53 hosted-zone detail
//!   GET  /api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id}/record-sets → Route 53 RRset inventory
//!   GET  /api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id}/record-sets/{record_type} → Route 53 RRset detail
//!   GET  /api/v1/center/cloud/provider-capabilities/accounts/{account_id} → sanitized capability snapshot
//!   ANY  /api/v1/proxy/{controller_id}/*rest                       → proxy HTTP request to controller

#[cfg(feature = "password-auth")]
use axum::routing::patch;
use axum::{
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, HeaderName, StatusCode, Uri},
    response::IntoResponse,
    routing::{any, delete, get, post},
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
pub mod config_data_ops;
mod global_resources;
pub mod provider_accounts;
pub mod provider_capabilities;
pub mod provider_credential_inspections;
mod region_route_handlers;
pub mod reload_ops;
mod roles;
pub mod route53_dns;
#[cfg(feature = "password-auth")]
mod users;
pub mod web;

use crate::aggregator::ResourceAggregator;
use crate::common::api::{ApiResponse, ListResponse};
use crate::fed_sync::registry::ControllerRegistry;
use crate::metadata_store::CenterMetaDataStore;
use crate::proxy::ProxyForwarder;
use crate::watch_cache::CenterSyncClient;

#[derive(Clone)]
pub struct ApiState {
    pub aggregator: Arc<ResourceAggregator>,
    pub proxy: Arc<ProxyForwarder>,
    pub controller_directory: Option<Arc<dyn edgion_center_core::ControllerDirectory>>,
    pub controller_evictor: Arc<dyn edgion_center_runtime::eviction::ControllerEviction>,
    pub user_admin: Option<Arc<dyn edgion_center_core::UserAdmin>>,
    pub role_admin: Option<Arc<dyn edgion_center_core::RoleAdmin>>,
    pub audit_reader: Option<Arc<dyn edgion_center_core::AuditReader>>,
    /// Optional SDK-free Cloudflare DNS read service. Provider clients and credentials remain
    /// behind the composition boundary.
    pub cloudflare_dns_admin: Option<cloudflare_dns::SharedCloudflareDnsAdminService>,
    /// Optional SDK-free Cloudflare DNS synchronous write service.
    pub cloudflare_dns_write_admin: Option<cloudflare_dns::SharedCloudflareDnsWriteAdminService>,
    /// Optional SDK-free Route 53 DNS read service.
    pub route53_dns_admin: Option<route53_dns::SharedRoute53DnsAdminService>,
    /// Optional SDK-free Route 53 synchronous RRset write service.
    pub route53_dns_write_admin: Option<route53_dns::SharedRoute53DnsWriteAdminService>,
    /// Optional SDK-free Route 53 public hosted-zone lifecycle service.
    pub route53_zone_lifecycle_admin: Option<route53_dns::SharedRoute53ZoneLifecycleAdminService>,
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
                let online = record.phase == edgion_center_core::ControllerPhase::Online;
                let key_count = record
                    .resource_count
                    .or_else(|| enrichment.and_then(|summary| summary.key_count));
                let per_kind = record
                    .resource_counts_by_kind
                    .or_else(|| enrichment.and_then(|summary| summary.per_kind.clone()));
                let stats_updated_secs_ago = record
                    .stats_updated_unix_ms
                    .map(|updated_at| now_ms.saturating_sub(updated_at) as u64 / 1_000)
                    .or_else(|| enrichment.and_then(|summary| summary.stats_updated_secs_ago));
                crate::aggregator::ControllerSummary {
                    controller_id: id,
                    cluster: record.cluster,
                    env: record.environments,
                    tag: record.tags,
                    online,
                    key_count,
                    per_kind,
                    stats_updated_secs_ago,
                    stats_state: crate::aggregator::StatsState::derive(key_count.is_some(), online),
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
}

#[cfg(test)]
impl Default for ApiState {
    fn default() -> Self {
        use crate::watch_cache::CenterWatchCacheRegistry;
        use parking_lot::Mutex;

        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let proxy = Arc::new(ProxyForwarder::new(
            registry.clone(),
            Arc::new(Mutex::new(std::collections::HashMap::new())),
            5,
        ));

        Self {
            aggregator: Arc::new(ResourceAggregator::new()),
            proxy,
            controller_directory: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: None,
            role_admin: None,
            audit_reader: None,
            cloudflare_dns_admin: None,
            cloudflare_dns_write_admin: None,
            route53_dns_admin: None,
            route53_dns_write_admin: None,
            route53_zone_lifecycle_admin: None,
            provider_account_store: None,
            capability_snapshot_store: None,
            credential_inspection_service: None,
            metadata_store,
            sync_client,
            registry,
            platform_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            authz_mode: edgion_center_core::AuthzMode::AllowAll,
            platform_mode: edgion_center_core::CenterMode::Standalone,
            capabilities: edgion_center_core::CenterCapabilities::for_mode(
                edgion_center_core::CenterMode::Standalone,
            ),
        }
    }
}

pub fn router(mut state: ApiState) -> Router {
    // Advertise only capabilities that are actually composed. Keeping this
    // effective value in state also makes `/server-info` and route mounting
    // report the same surface when a composition is incomplete.
    state.capabilities.cloudflare_dns_read &= state.cloudflare_dns_admin.is_some();
    state.capabilities.cloudflare_dns_write &= state.cloudflare_dns_write_admin.is_some();
    state.capabilities.route53_dns_read &= state.route53_dns_admin.is_some();
    state.capabilities.route53_dns_write &= state.route53_dns_write_admin.is_some();
    state.capabilities.route53_zone_lifecycle &= state.route53_zone_lifecycle_admin.is_some();
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
        // Watch-fed RegionRouteOverride endpoints
        .route(
            "/api/v1/center/region-route-overrides",
            get(region_route_handlers::list_region_route_overrides),
        )
        .route(
            "/api/v1/center/region-route-overrides/failover",
            post(region_route_handlers::region_route_failover),
        )
        .route(
            "/api/v1/center/region-route-overrides/sync",
            post(region_route_handlers::region_route_override_sync),
        )
        // HTTP proxy to controllers. Body cap mirrors the Controller-side
        // 1 MiB federation proxy limit so oversized writes fail locally.
        .route(
            "/api/v1/proxy/{controller_id}/{*rest}",
            any(proxy_handler).layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)),
        );

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
    if capabilities.global_resources_inventory {
        app = app
            .route(
                "/api/v1/center/global-resources/catalog",
                get(global_resources::catalog),
            )
            .route(
                "/api/v1/center/global-resources/resources/{kind}",
                get(global_resources::list),
            )
            .route(
                "/api/v1/center/global-resources/resources/{kind}/{namespace}/{name}",
                get(global_resources::detail),
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
    if capabilities.cloudflare_dns_write && state.cloudflare_dns_write_admin.is_some() {
        let cloudflare_dns_write_routes = Router::new()
            .route(
                "/api/v1/center/cloudflare/dns/accounts/{account_id}/zones",
                post(cloudflare_dns::create_zone),
            )
            .route(
                "/api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}",
                delete(cloudflare_dns::delete_zone),
            )
            .route(
                "/api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets/{record_type}",
                axum::routing::put(cloudflare_dns::put_record_set)
                    .delete(cloudflare_dns::delete_record_set),
            )
            .route(
                "/api/v1/center/cloudflare/dns/accounts/{account_id}/zones/{zone_id}/record-sets/{record_type}/remote-control",
                axum::routing::put(cloudflare_dns::put_remote_record_set),
            )
            .layer(axum::extract::DefaultBodyLimit::max(64 * 1024));
        app = app.merge(cloudflare_dns_write_routes);
    }
    if capabilities.route53_dns_read && state.route53_dns_admin.is_some() {
        app = app
            .route(
                "/api/v1/center/aws/route53/accounts/{account_id}/hosted-zones",
                get(route53_dns::list_zones),
            )
            .route(
                "/api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id}",
                get(route53_dns::get_zone),
            )
            .route(
                "/api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id}/record-sets",
                get(route53_dns::list_record_sets),
            )
            .route(
                "/api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id}/record-sets/{record_type}",
                get(route53_dns::get_record_set),
            );
    }
    if capabilities.route53_dns_write && state.route53_dns_write_admin.is_some() {
        let route53_write_routes = Router::new()
            .route(
                "/api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id}/record-sets/{record_type}",
                axum::routing::put(route53_dns::put_record_set)
                    .delete(route53_dns::delete_record_set),
            )
            .route(
                "/api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id}/change-batches",
                post(route53_dns::apply_change_batch),
            )
            .route(
                "/api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id}/changes/{receipt}",
                get(route53_dns::get_change),
            )
            .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024));
        app = app.merge(route53_write_routes);
    }
    if capabilities.route53_zone_lifecycle && state.route53_zone_lifecycle_admin.is_some() {
        let route53_lifecycle_routes = Router::new()
            .route(
                "/api/v1/center/aws/route53/accounts/{account_id}/hosted-zones",
                post(route53_dns::create_zone),
            )
            .route(
                "/api/v1/center/aws/route53/accounts/{account_id}/hosted-zones/{zone_id}/lifecycle",
                get(route53_dns::observe_zone_lifecycle).delete(route53_dns::delete_zone),
            )
            .layer(axum::extract::DefaultBodyLimit::max(64 * 1024));
        app = app.merge(route53_lifecycle_routes);
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

    app.layer(axum::middleware::from_fn(
        crate::common::observe::cloud_metrics::cloud_metrics_middleware,
    ))
    .with_state(state)
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

/// `POST /api/v1/controllers/{id}/reload` — dispatch a reload over the
/// federation tunnel and report whether it actually completed.
///
/// The Controller's own `200` only ever means "queued" (`request_reload()` is
/// fire-and-forget), so reporting it as success made a wedged reload
/// indistinguishable from a finished one. The terminal state is determined by
/// [`reload_ops`] instead; see its module docs.
async fn reload_controller(
    State(state): State<ApiState>,
    Path(id_raw): Path<String>,
) -> impl IntoResponse {
    let id = id_raw.replace('~', "/");
    match reload_ops::reload_controller(&state, &id).await {
        Ok(outcome) => reload_outcome_response(outcome),
        // Never reached the Controller (unknown/offline controller, stale
        // ownership, tunnel timeout). Passed through with the proxy's own
        // status rather than flattened into a reload outcome.
        Err((status, message)) => {
            (status, Json(ApiResponse::<String>::err_body(message))).into_response()
        }
    }
}

/// HTTP status for one [`reload_ops::ReloadOutcome`].
///
/// Exhaustive on purpose, in the same spirit as
/// `region_route_handlers::OutcomeClass`: a seventh `OutcomeState` must be a
/// compile error here rather than silently defaulting into a success or a
/// failure bucket.
fn reload_outcome_status(outcome: &reload_ops::ReloadOutcome) -> StatusCode {
    use config_data_ops::OutcomeState;
    match outcome.state {
        OutcomeState::Converged => StatusCode::OK,
        // The reload was accepted by the Controller but is not confirmed
        // complete — 202 keeps it out of the client's error path so the
        // dashboard can render the distinction instead of an error toast.
        // `Superseded` is unreachable on this path (a reload overwrites no
        // document) and is grouped here only to keep the match exhaustive.
        OutcomeState::Accepted | OutcomeState::Unknown | OutcomeState::Superseded => {
            StatusCode::ACCEPTED
        }
        // A follower Controller refused it: retrying this address is
        // pointless, and the body carries the leader's.
        OutcomeState::Conflict => StatusCode::CONFLICT,
        OutcomeState::Failed => match outcome.retry_after_secs {
            // A transient refusal (already queued, or the config center not
            // ready) must keep its 503 all the way to the client instead of
            // collapsing into 502 — retrying is the action either way.
            Some(_) => StatusCode::SERVICE_UNAVAILABLE,
            None => StatusCode::BAD_GATEWAY,
        },
    }
}

fn reload_outcome_response(outcome: reload_ops::ReloadOutcome) -> axum::response::Response {
    let status = reload_outcome_status(&outcome);
    let retry_after = outcome.retry_after_secs;
    let body = ApiResponse {
        success: status.is_success(),
        error: if status.is_success() {
            None
        } else {
            outcome.reason.clone()
        },
        data: Some(outcome),
    };
    let mut response = (status, Json(body)).into_response();
    // Echo the transient hint so a non-browser client sees the same retry
    // semantics the Controller expressed.
    if let Some(seconds) = retry_after {
        if let Ok(value) = axum::http::HeaderValue::from_str(&seconds.to_string()) {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, value);
        }
    }
    response
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

    struct UnavailableRoute53Dns;

    #[async_trait::async_trait]
    impl route53_dns::Route53DnsAdminService for UnavailableRoute53Dns {
        async fn list_zones(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &route53_dns::Route53ZonePageRequest,
        ) -> Result<route53_dns::Route53ZonePageDto, route53_dns::Route53DnsAdminError> {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }

        async fn get_zone(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::DnsZoneId,
        ) -> Result<route53_dns::Route53ZoneDto, route53_dns::Route53DnsAdminError> {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }

        async fn list_record_sets(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::DnsZoneId,
            _: &route53_dns::Route53RecordPageRequest,
        ) -> Result<route53_dns::Route53RecordPageDto, route53_dns::Route53DnsAdminError> {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }

        async fn get_record_set(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::DnsZoneId,
            _: &route53_dns::Route53RecordSetKey,
        ) -> Result<route53_dns::Route53RecordSetDto, route53_dns::Route53DnsAdminError> {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }
    }

    struct UnavailableRoute53DnsWrite;

    #[async_trait::async_trait]
    impl route53_dns::Route53DnsWriteAdminService for UnavailableRoute53DnsWrite {
        async fn put_record_set(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::DnsZoneId,
            _: &route53_dns::Route53RecordSetKey,
            _: &route53_dns::Route53RecordSetPutRequest,
        ) -> Result<route53_dns::Route53ChangeReceiptDto, route53_dns::Route53DnsAdminError>
        {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }

        async fn delete_record_set(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::DnsZoneId,
            _: &route53_dns::Route53RecordSetKey,
            _: &route53_dns::Route53RecordSetDeleteRequest,
        ) -> Result<route53_dns::Route53ChangeReceiptDto, route53_dns::Route53DnsAdminError>
        {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }

        async fn apply_change_batch(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::DnsZoneId,
            _: &route53_dns::Route53RecordChangeBatchRequest,
        ) -> Result<route53_dns::Route53ChangeReceiptDto, route53_dns::Route53DnsAdminError>
        {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }

        async fn get_change(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::DnsZoneId,
            _: &edgion_center_core::DnsChangeId,
        ) -> Result<route53_dns::Route53ChangeReceiptDto, route53_dns::Route53DnsAdminError>
        {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }
    }

    struct UnavailableRoute53ZoneLifecycle;

    #[async_trait::async_trait]
    impl route53_dns::Route53ZoneLifecycleAdminService for UnavailableRoute53ZoneLifecycle {
        async fn create_zone(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::ZoneCreationRequest,
        ) -> Result<route53_dns::Route53ZoneLifecycleMutationDto, route53_dns::Route53DnsAdminError>
        {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }

        async fn observe_zone(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::DnsZoneId,
            _: &edgion_center_core::AbsoluteDnsName,
        ) -> Result<
            route53_dns::Route53ZoneLifecycleObservationDto,
            route53_dns::Route53DnsAdminError,
        > {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }

        async fn delete_zone(
            &self,
            _: &edgion_center_core::CloudResourceId,
            _: &edgion_center_core::DnsZoneId,
            _: &route53_dns::Route53ZoneDeleteRequest,
        ) -> Result<route53_dns::Route53ZoneLifecycleMutationDto, route53_dns::Route53DnsAdminError>
        {
            Err(route53_dns::Route53DnsAdminError::Unavailable)
        }
    }

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
        ApiState {
            authz_mode,
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
            ..ApiState::default()
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
            resource_counts_by_kind: None,
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

    /// A minimal directory record in the SQL-adapter shape: `resource_count`
    /// and `stats_updated_unix_ms` are never persisted there (the SQL adapter
    /// does not carry runtime diagnostics), so every diagnostic field must
    /// fall back to whatever the in-memory aggregator holds for the same
    /// controller id.
    fn directory_record_without_stats(
        controller_id: &str,
        phase: ControllerPhase,
    ) -> ControllerRecord {
        ControllerRecord {
            controller_id: ControllerId::new(controller_id).unwrap(),
            current_session_id: Some(SessionId::new("session-1").unwrap()),
            cluster: "cluster-a".to_string(),
            environments: vec!["prod".to_string()],
            tags: vec![],
            connected_replica: None,
            ownership_fence: None,
            sync_version: None,
            watch_server_id: None,
            resource_count: None,
            resource_counts_by_kind: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: None,
            phase,
            last_seen_unix_ms: 1,
        }
    }

    #[tokio::test]
    async fn summaries_fall_back_to_the_aggregator_when_the_directory_lacks_stats() {
        let mut state = state_with_authz_mode(AuthzMode::Rbac, false);
        state.platform_mode = edgion_center_core::CenterMode::Kubernetes;

        let aggregator = ResourceAggregator::new();
        aggregator.set_controller_info(
            "cluster-a/controller-0",
            crate::aggregator::ControllerInfo {
                controller_id: "cluster-a/controller-0".to_string(),
                cluster: "cluster-a".to_string(),
                environments: vec!["prod".to_string()],
                tags: vec![],
            },
        );
        let mut per_kind = std::collections::BTreeMap::new();
        per_kind.insert("Pod".to_string(), 4u32);
        per_kind.insert("Service".to_string(), 1u32);
        aggregator.update_stats("cluster-a/controller-0", Some(per_kind), 5);
        state.aggregator = Arc::new(aggregator);

        state.controller_directory = Some(Arc::new(GlobalDirectory(vec![
            directory_record_without_stats("cluster-a/controller-0", ControllerPhase::Online),
        ])));

        let summaries = state.controller_summaries().await.unwrap();
        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert_eq!(summary.key_count, Some(5));
        assert_eq!(summary.stats_updated_secs_ago, Some(0));
        let per_kind = summary
            .per_kind
            .as_ref()
            .expect("per_kind falls back to the aggregator");
        assert_eq!(per_kind.get("Pod"), Some(&4));
        assert_eq!(per_kind.get("Service"), Some(&1));
        assert_eq!(summary.stats_state, crate::aggregator::StatsState::Fresh);
    }

    #[tokio::test]
    async fn summaries_report_missing_and_stale_states() {
        let mut state = state_with_authz_mode(AuthzMode::Rbac, false);
        state.platform_mode = edgion_center_core::CenterMode::Kubernetes;

        // No stats anywhere (directory nor aggregator) -> Missing, regardless
        // of online/offline phase.
        let missing =
            directory_record_without_stats("cluster-a/controller-missing", ControllerPhase::Online);

        // Offline record that DOES carry counts (the K8s adapter preserves
        // counts on mark_offline) -> Stale, counts retained.
        let mut stale = directory_record_without_stats(
            "cluster-a/controller-offline",
            ControllerPhase::Offline,
        );
        stale.resource_count = Some(9);
        let mut counts_by_kind = std::collections::BTreeMap::new();
        counts_by_kind.insert("Pod".to_string(), 9u32);
        stale.resource_counts_by_kind = Some(counts_by_kind);
        stale.stats_updated_unix_ms = Some(1);

        state.controller_directory = Some(Arc::new(GlobalDirectory(vec![missing, stale])));

        let summaries = state.controller_summaries().await.unwrap();
        assert_eq!(summaries.len(), 2);
        let by_id = |id: &str| summaries.iter().find(|s| s.controller_id == id).unwrap();

        let missing = by_id("cluster-a/controller-missing");
        assert_eq!(missing.stats_state, crate::aggregator::StatsState::Missing);
        assert_eq!(missing.key_count, None);
        assert_eq!(missing.per_kind, None);

        let stale = by_id("cluster-a/controller-offline");
        assert_eq!(stale.stats_state, crate::aggregator::StatsState::Stale);
        assert!(!stale.online);
        assert_eq!(stale.key_count, Some(9));
        assert!(stale.per_kind.is_some());
    }

    /// The last cell of the stats 2×2: the directory carries no counts and the
    /// record is offline, but the in-memory aggregator still holds counts from
    /// this replica's own session. The counts must survive the fallback and be
    /// labelled `Stale` — deriving `Missing` here would erase numbers Center
    /// can still render.
    #[tokio::test]
    async fn summaries_report_stale_when_only_the_aggregator_has_counts_for_an_offline_controller()
    {
        let mut state = state_with_authz_mode(AuthzMode::Rbac, false);
        state.platform_mode = edgion_center_core::CenterMode::Kubernetes;

        let aggregator = ResourceAggregator::new();
        aggregator.set_controller_info(
            "cluster-a/controller-0",
            crate::aggregator::ControllerInfo {
                controller_id: "cluster-a/controller-0".to_string(),
                cluster: "cluster-a".to_string(),
                environments: vec!["prod".to_string()],
                tags: vec![],
            },
        );
        let mut per_kind = std::collections::BTreeMap::new();
        per_kind.insert("Pod".to_string(), 4u32);
        aggregator.update_stats("cluster-a/controller-0", Some(per_kind), 4);
        state.aggregator = Arc::new(aggregator);

        state.controller_directory = Some(Arc::new(GlobalDirectory(vec![
            directory_record_without_stats("cluster-a/controller-0", ControllerPhase::Offline),
        ])));

        let summaries = state.controller_summaries().await.unwrap();
        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert!(!summary.online);
        assert_eq!(summary.key_count, Some(4));
        assert_eq!(
            summary.per_kind.as_ref().and_then(|kinds| kinds.get("Pod")),
            Some(&4)
        );
        assert_eq!(summary.stats_state, crate::aggregator::StatsState::Stale);
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
            "/api/v1/center/global-resources/catalog",
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

    /// The proxy route must reject request bodies above the Controller's
    /// 1 MiB federation cap locally, instead of letting axum's 2 MiB default
    /// through only for the Controller to 413 it after a full tunnel trip.
    #[tokio::test]
    async fn proxy_route_rejects_body_over_controller_limit() {
        use tower::ServiceExt;

        let app = router(state_with_authz_mode(AuthzMode::AllowAll, false));
        let oversized = vec![b'x'; 1024 * 1024 + 1];
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("PUT")
                    .uri("/api/v1/proxy/cluster~c0/api/v1/namespaced/edgionconfigdata/ns/name")
                    .header("content-type", "application/yaml")
                    .body(axum::body::Body::from(oversized))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// Reload rides the proxy tunnel now. With no live federation session the
    /// ProxyForwarder answers 404 (unknown controller), which must pass
    /// through instead of the old Commander 500/504 mapping.
    #[tokio::test]
    async fn reload_uses_proxy_tunnel_and_maps_unknown_controller_to_404() {
        use tower::ServiceExt;

        let app = router(state_with_authz_mode(AuthzMode::AllowAll, false));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/controllers/cluster~c0/reload")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// Every reload outcome must map to a status the client can act on, and
    /// the mapping must not collapse states that call for different operator
    /// actions: `converged` is the only 200, "landed but unconfirmed" stays in
    /// the success family as 202 so the dashboard renders the distinction
    /// instead of an error toast, a follower's refusal keeps its 409, and a
    /// transient refusal keeps its 503 with the retry hint.
    #[test]
    fn reload_outcome_status_separates_every_operator_action() {
        use config_data_ops::OutcomeState;
        use reload_ops::ReloadOutcome;

        fn outcome(state: OutcomeState) -> ReloadOutcome {
            ReloadOutcome {
                controller_id: "cluster/c0".to_string(),
                state,
                reason: None,
                server_id: None,
                convergence_ms: None,
                retry_after_secs: None,
                leader: None,
            }
        }

        assert_eq!(
            reload_outcome_status(&outcome(OutcomeState::Converged)),
            StatusCode::OK
        );
        for state in [OutcomeState::Accepted, OutcomeState::Unknown] {
            assert_eq!(
                reload_outcome_status(&outcome(state)),
                StatusCode::ACCEPTED,
                "{state:?} landed and must not read as an error"
            );
        }
        assert_eq!(
            reload_outcome_status(&outcome(OutcomeState::Conflict)),
            StatusCode::CONFLICT
        );
        assert_eq!(
            reload_outcome_status(&outcome(OutcomeState::Failed)),
            StatusCode::BAD_GATEWAY
        );
        let transient = ReloadOutcome {
            retry_after_secs: Some(5),
            ..outcome(OutcomeState::Failed)
        };
        assert_eq!(
            reload_outcome_status(&transient),
            StatusCode::SERVICE_UNAVAILABLE,
            "a transient refusal is not a bad gateway"
        );
    }

    /// Deliberate contract change, recorded so it is not mistaken for a bug:
    /// the old handler echoed the Controller's status for *any* non-2xx
    /// (`StatusCode::from_u16(resp.status_code)`). Now only 409 and 503 keep
    /// their own status — every other upstream rejection is reported as 502
    /// with the upstream status and body preserved in `reason`. A Controller
    /// status is a statement about the Controller's own request, not about
    /// Center's, and echoing e.g. a 403 made a Center-side authorization
    /// failure indistinguishable from a Controller-side one.
    #[tokio::test]
    async fn other_upstream_rejections_collapse_to_502_with_the_detail_in_reason() {
        use config_data_ops::OutcomeState;
        use reload_ops::ReloadOutcome;

        let outcome = ReloadOutcome {
            controller_id: "cluster/c0".to_string(),
            state: OutcomeState::Failed,
            reason: Some("reload returned status 403: verb reload not granted".to_string()),
            server_id: None,
            convergence_ms: None,
            retry_after_secs: None,
            leader: None,
        };

        let response = reload_outcome_response(outcome);
        assert_eq!(
            response.status(),
            StatusCode::BAD_GATEWAY,
            "an upstream 403 must not be echoed as Center's own 403"
        );
        assert!(
            response
                .headers()
                .get(axum::http::header::RETRY_AFTER)
                .is_none(),
            "only a transient refusal may carry a retry hint"
        );

        // Collapsing the status is only defensible because the upstream detail
        // survives in the body — otherwise a 403 would become an unattributable
        // 502. Assert it reaches the wire, not merely the struct.
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("outcome body is small and fully buffered");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("outcome body is JSON");
        assert_eq!(body["success"], serde_json::json!(false));
        let reason = body["data"]["reason"]
            .as_str()
            .expect("failed carries a reason");
        assert!(reason.contains("403"), "got {reason}");
        assert!(reason.contains("verb reload not granted"), "got {reason}");
        assert_eq!(
            body["error"],
            serde_json::json!(reason),
            "the reason must also surface on the generic `error` field clients read"
        );
    }

    /// A transient refusal must carry `Retry-After` on the Center response
    /// too, so a non-browser client sees the same retry semantics the
    /// Controller expressed rather than a bare 503.
    #[test]
    fn transient_reload_failure_echoes_retry_after() {
        use config_data_ops::OutcomeState;
        use reload_ops::ReloadOutcome;

        let response = reload_outcome_response(ReloadOutcome {
            controller_id: "cluster/c0".to_string(),
            state: OutcomeState::Failed,
            reason: Some("already in progress".to_string()),
            server_id: None,
            convergence_ms: None,
            retry_after_secs: Some(5),
            leader: None,
        });

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            Some("5")
        );
    }

    /// GlobalResources is served entirely from the federation watch read
    /// model now (CCI-04): the route gate depends only on the capability
    /// flag, with no `global_resources` fan-out service to compose.
    #[tokio::test]
    async fn global_resources_routes_mount_without_a_fanout_service() {
        use tower::ServiceExt;

        let path = "/api/v1/center/global-resources/catalog";
        let mut state = state_with_authz_mode(AuthzMode::AllowAll, false);
        state.capabilities.global_resources_inventory = true;
        let response = router(state)
            .oneshot(
                axum::http::Request::builder()
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn route53_capability_and_service_jointly_control_route_mounting() {
        use tower::ServiceExt;

        let path = "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones";
        let mut absent = state_with_authz_mode(AuthzMode::AllowAll, false);
        absent.capabilities.route53_dns_read = true;
        let response = router(absent)
            .oneshot(
                axum::http::Request::builder()
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let mut present = state_with_authz_mode(AuthzMode::AllowAll, false);
        present.capabilities.route53_dns_read = true;
        present.route53_dns_admin = Some(Arc::new(UnavailableRoute53Dns));
        let app = router(present);
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
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let server_info = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/server-info")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(server_info).await;
        assert_eq!(json["data"]["capabilities"]["route53DnsRead"], true);
    }

    #[tokio::test]
    async fn route53_write_capability_and_service_jointly_control_routes() {
        use tower::ServiceExt;

        let path = "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z0123456789ABCDEF/record-sets/A?owner=www.example.com";
        let body = r#"{"guard":{"type":"must_not_exist"},"desired":{"ttl":{"type":"seconds","seconds":60},"values":[{"type":"A","address":"192.0.2.1"}],"aliasTarget":null,"routingPolicy":null,"healthCheckId":null}}"#;
        let mut absent = state_with_authz_mode(AuthzMode::AllowAll, false);
        absent.capabilities.route53_dns_write = true;
        let response = router(absent)
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::PUT)
                    .uri(path)
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let mut present = state_with_authz_mode(AuthzMode::AllowAll, false);
        present.capabilities.route53_dns_write = true;
        present.route53_dns_write_admin = Some(Arc::new(UnavailableRoute53DnsWrite));
        let app = router(present);
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::PUT)
                    .uri(path)
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let json = body_json(
            app.oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/server-info")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(json["data"]["capabilities"]["route53DnsWrite"], true);
    }

    #[tokio::test]
    async fn route53_zone_lifecycle_capability_and_service_jointly_control_routes() {
        use tower::ServiceExt;

        let path = "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones";
        let body = r#"{"apex":"example.com","idempotencyKey":"create-1"}"#;
        let mut absent = state_with_authz_mode(AuthzMode::AllowAll, false);
        absent.capabilities.route53_zone_lifecycle = true;
        let response = router(absent)
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri(path)
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let mut present = state_with_authz_mode(AuthzMode::AllowAll, false);
        present.capabilities.route53_zone_lifecycle = true;
        present.route53_zone_lifecycle_admin = Some(Arc::new(UnavailableRoute53ZoneLifecycle));
        let app = router(present);
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri(path)
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let json = body_json(
            app.oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/server-info")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(json["data"]["capabilities"]["route53ZoneLifecycle"], true);
    }

    #[tokio::test]
    async fn route53_read_and_write_methods_merge_on_the_record_detail_path() {
        use tower::ServiceExt;

        let path = "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z0123456789ABCDEF/record-sets/A?owner=www.example.com";
        let mut state = state_with_authz_mode(AuthzMode::AllowAll, false);
        state.capabilities.route53_dns_read = true;
        state.capabilities.route53_dns_write = true;
        state.route53_dns_admin = Some(Arc::new(UnavailableRoute53Dns));
        state.route53_dns_write_admin = Some(Arc::new(UnavailableRoute53DnsWrite));
        let app = router(state);

        let get = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get.status(), StatusCode::SERVICE_UNAVAILABLE);

        let put = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::PUT)
                    .uri(path)
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(
                        r#"{"guard":{"type":"must_not_exist"},"desired":{"ttl":{"type":"seconds","seconds":60},"values":[{"type":"A","address":"192.0.2.1"}],"aliasTarget":null,"routingPolicy":null,"healthCheckId":null}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put.status(), StatusCode::SERVICE_UNAVAILABLE);

        let json = body_json(
            app.oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/server-info")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(json["data"]["capabilities"]["route53DnsRead"], true);
        assert_eq!(json["data"]["capabilities"]["route53DnsWrite"], true);
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
