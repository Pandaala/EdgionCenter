//! Region Route handlers and shared helpers for Center aggregation handlers.
//!
//! # Center RegionRoute API contract (frozen — web/P4 builds against these URLs)
//!
//! ## Primary (unified) endpoints
//!
//! | Method | Path                                          | Description                                          |
//! |--------|-----------------------------------------------|------------------------------------------------------|
//! | GET    | `/api/v1/center/region-routes`                | Aggregated effective region routes (MetaDataStore snapshot) |
//! | POST   | `/api/v1/center/region-routes/failover`       | Patch `failoverTo` on a RegionRouteOverride; fans out to all online controllers |
//! | POST   | `/api/v1/center/region-routes/sync`           | Copy one controller's base and override documents to selected online controllers |
//! | GET    | `/api/v1/center/region-routes/consistency`    | Cross-controller consistency check (online controllers only) |
//!
//! ## Legacy paths (308 permanent redirect to unified endpoints above)
//!
//! | Method | Legacy path                                              | Redirects to                                    |
//! |--------|----------------------------------------------------------|-------------------------------------------------|
//! | GET    | `/api/v1/center/cluster-region-routes`                   | `/api/v1/center/region-routes`                  |
//! | GET    | `/api/v1/center/service-region-routes`                   | `/api/v1/center/region-routes`                  |
//! | POST   | `/api/v1/center/cluster-region-routes/failover`          | `/api/v1/center/region-routes/failover`         |
//! | POST   | `/api/v1/center/service-region-routes/failover`          | `/api/v1/center/region-routes/failover`         |
//! | GET    | `/api/v1/center/cluster-region-routes/consistency`       | `/api/v1/center/region-routes/consistency`      |
//! | GET    | `/api/v1/center/service-region-routes/consistency`       | `/api/v1/center/region-routes/consistency`      |
//!
//! New callers MUST use the unified paths. Legacy paths exist only for backward compatibility
//! and may be removed in a future major release.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::config_data_ops::{write_config_data, OutcomeState, WriteOutcome};
use super::ApiState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CenterRegionRouteAggregatedView {
    #[serde(flatten)]
    route: crate::metadata_store::CenterRegionRouteView,
    online_controller_ids: Vec<String>,
}

// ============= Failover Request Type =============

/// Strongly-typed body for `/api/v1/center/{cluster,service}-region-routes/failover`.
///
/// The failover endpoint can only change the targeted region's `failoverTo` field.
/// Any extra field (e.g. `myRegion`, `spec`, full PM YAML) is rejected at the
/// deserialization stage, so the contract is enforced by the type system rather
/// than by runtime checks.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FailoverRequest {
    pub namespace: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub plugin_name: Option<String>,
    #[serde(default)]
    pub entry_index: Option<usize>,
    pub region_name: String,
    /// Empty string = clear failover.
    pub failover_to: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegionRouteSyncRequest {
    pub namespace: String,
    pub plugin_name: String,
    pub entry_index: usize,
    pub source_controller_id: String,
    /// Empty means every other online controller.
    #[serde(default)]
    pub target_controller_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegionRouteSyncTargetResult {
    controller_id: String,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverrideSyncRequest {
    pub namespace: String,
    pub name: String,
    pub source_controller_id: String,
    #[serde(default)]
    pub target_controller_ids: Vec<String>,
}

// ============= RegionRoute MetaDataStore Handlers =============

/// `GET /api/v1/center/region-routes`
///
/// Returns aggregated effective region routes across all controllers,
/// drawn from the background poller's snapshot in `CenterMetaDataStore`.
///
/// Legacy paths `/api/v1/center/cluster-region-routes` and
/// `/api/v1/center/service-region-routes` redirect (308) to this endpoint.
pub async fn list_region_routes(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    state.require_effective_read_model().map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "success": false, "message": error.to_string() })),
        )
    })?;
    let online_controller_ids = state.online_controller_ids().await.map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "success": false, "message": error.to_string() })),
        )
    })?;
    let data: Vec<_> = state
        .metadata_store
        .list_region_routes()
        .into_iter()
        .map(|route| CenterRegionRouteAggregatedView {
            route,
            online_controller_ids: online_controller_ids.clone(),
        })
        .collect();
    Ok(Json(serde_json::json!({ "success": true, "data": data })))
}

pub async fn list_region_route_overrides(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let online_controller_ids = state.online_controller_ids().await.map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "success": false, "message": error.to_string() })),
        )
    })?;
    Ok(Json(serde_json::json!({
        "success": true,
        "data": state.metadata_store.list_region_route_overrides(),
        "onlineControllerIds": online_controller_ids,
    })))
}

pub async fn list_service_region_route_overrides(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let online_controller_ids = state.online_controller_ids().await.map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "success": false, "message": error.to_string() })),
        )
    })?;
    Ok(Json(serde_json::json!({
        "success": true,
        "data": state.metadata_store.list_service_region_route_overrides(),
        "onlineControllerIds": online_controller_ids,
    })))
}

pub async fn region_route_override_sync(
    State(state): State<ApiState>,
    Json(req): Json<OverrideSyncRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    sync_watched_override(&state, req, false).await
}

pub async fn service_region_route_override_sync(
    State(state): State<ApiState>,
    Json(req): Json<OverrideSyncRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    sync_watched_override(&state, req, true).await
}

async fn sync_watched_override(
    state: &ApiState,
    req: OverrideSyncRequest,
    service: bool,
) -> (StatusCode, Json<serde_json::Value>) {
    let online = match state.online_controller_ids().await {
        Ok(online) if !online.is_empty() => online,
        Ok(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "success": false, "error": "no online controllers" })),
            );
        }
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "success": false, "error": error.to_string() })),
            );
        }
    };
    let online_set: HashSet<_> = online.iter().cloned().collect();
    let row = if service {
        state
            .metadata_store
            .list_service_region_route_overrides()
            .into_iter()
            .find(|row| row.namespace == req.namespace && row.name == req.name)
    } else {
        state
            .metadata_store
            .list_region_route_overrides()
            .into_iter()
            .find(|row| row.namespace == req.namespace && row.name == req.name)
    };
    let Some(row) = row else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "success": false, "error": "override not found" })),
        );
    };
    if !row.controllers.contains_key(&req.source_controller_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "success": false, "error": "source Controller does not have this override" }),
            ),
        );
    }
    let mut seen = HashSet::new();
    let targets: Vec<_> = if req.target_controller_ids.is_empty() {
        online
            .into_iter()
            .filter(|id| id != &req.source_controller_id)
            .collect()
    } else {
        req.target_controller_ids
            .into_iter()
            .filter(|id| id != &req.source_controller_id && seen.insert(id.clone()))
            .collect()
    };
    if targets.is_empty() || targets.iter().any(|id| !online_set.contains(id)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "success": false, "error": "targets must be online Controllers other than the source" }),
            ),
        );
    }
    // The source document comes from the SOURCE controller's own local watch
    // cache — never from `metadata_store`'s derived override view (used only
    // for the existence/membership check above) and never from a proxied
    // GET. `row` already established that the source controller reports
    // this override; this is the same document, read the same way the
    // write core reads its own CAS precondition.
    let key = format!("{}/{}", req.namespace, req.name);
    let Some(source_document) = state
        .sync_client
        .plugin_metadata
        .raw_document(&req.source_controller_id, &key)
    else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "success": false,
                "error": "source document is not in the local watch cache"
            })),
        );
    };
    let Some(source_data) = source_document.pointer("/spec/data").cloned() else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "success": false,
                "error": "source document is missing spec.data"
            })),
        );
    };
    let mutate = |document: &mut serde_json::Value| -> Result<(), String> {
        replace_spec_data(document, &source_data)
    };
    let predicate = |document: &serde_json::Value| -> bool {
        document.pointer("/spec/data") == Some(&source_data)
    };

    let outcomes: Vec<WriteOutcome> =
        futures::future::join_all(targets.iter().map(|controller_id| {
            write_config_data(
                state,
                controller_id,
                &req.namespace,
                &req.name,
                &mutate,
                &predicate,
            )
        }))
        .await;

    failover_response(outcomes)
}

/// Replace `/spec/data` wholesale with `source_data`. Used by
/// `sync_watched_override`'s `mutate` closure: row-level sync copies the
/// entire `spec.data` payload from the source controller's document, not
/// individual fields.
fn replace_spec_data(
    document: &mut serde_json::Value,
    source_data: &serde_json::Value,
) -> Result<(), String> {
    let spec = document
        .get_mut("spec")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| "target resource is missing spec".to_string())?;
    spec.insert("data".to_string(), source_data.clone());
    Ok(())
}

// ============= Configuration Sync Handler =============

/// `POST /api/v1/center/region-routes/sync`
///
/// Copies the selected source Controller's complete EdgionPlugins document and
/// its referenced RegionRouteOverride document to explicit targets (or every
/// other online Controller when targets are omitted). Controller federation
/// RBAC remains the final authority for both resource writes.
pub async fn region_route_sync(
    State(state): State<ApiState>,
    Json(req): Json<RegionRouteSyncRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let online = match state.online_controller_ids().await {
        Ok(online) if !online.is_empty() => online,
        Ok(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "success": false, "error": "no online controllers" })),
            );
        }
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "success": false, "error": error.to_string() })),
            );
        }
    };
    let online_set: HashSet<_> = online.iter().cloned().collect();
    if !online_set.contains(&req.source_controller_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "source Controller is not online"
            })),
        );
    }

    let route = state
        .metadata_store
        .list_region_routes()
        .into_iter()
        .find(|route| {
            route.namespace == req.namespace
                && route.plugin_name == req.plugin_name
                && route.entry_index == req.entry_index
        });
    let Some(route) = route else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "success": false, "error": "RegionRoute entry not found" })),
        );
    };
    if !route.controllers.contains_key(&req.source_controller_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "source Controller does not report this RegionRoute"
            })),
        );
    }

    let mut seen = HashSet::new();
    let targets: Vec<String> = if req.target_controller_ids.is_empty() {
        online
            .into_iter()
            .filter(|id| id != &req.source_controller_id)
            .collect()
    } else {
        req.target_controller_ids
            .iter()
            .filter(|id| *id != &req.source_controller_id && seen.insert((*id).clone()))
            .cloned()
            .collect()
    };
    if targets.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "success": false, "error": "no target Controllers selected" }),
            ),
        );
    }
    if let Some(offline) = targets.iter().find(|id| !online_set.contains(*id)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": format!("target Controller is not online: {offline}")
            })),
        );
    }

    let plugin = match read_resource(
        &state,
        &req.source_controller_id,
        "edgionplugins",
        &req.namespace,
        &req.plugin_name,
    )
    .await
    {
        Ok(Some(document)) => document,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "success": false,
                    "error": "source EdgionPlugins document was not found"
                })),
            );
        }
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "success": false, "error": error })),
            );
        }
    };
    let override_identity =
        match region_route_override_identity(&plugin, req.entry_index, &req.namespace) {
            Ok(identity) => identity,
            Err(error) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({ "success": false, "error": error })),
                );
            }
        };
    let override_document = if let Some((namespace, name)) = &override_identity {
        match read_resource(
            &state,
            &req.source_controller_id,
            "edgionconfigdata",
            namespace,
            name,
        )
        .await
        {
            Ok(Some(document)) => Some(document),
            Ok(None) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({
                        "success": false,
                        "error": format!(
                            "source RegionRouteOverride was not found: {namespace}/{name}"
                        )
                    })),
                );
            }
            Err(error) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "success": false, "error": error })),
                );
            }
        }
    } else {
        None
    };

    let futures = targets.into_iter().map(|controller_id| {
        let state = state.clone();
        let plugin = plugin.clone();
        let plugin_namespace = req.namespace.clone();
        let plugin_name = req.plugin_name.clone();
        let override_identity = override_identity.clone();
        let override_document = override_document.clone();
        async move {
            let result = async {
                // Write the optional dependency first. A failed base write then leaves
                // only an unused override, never a base route with a dangling reference.
                if let (Some((namespace, name)), Some(document)) =
                    (override_identity, override_document)
                {
                    upsert_resource(
                        &state,
                        &controller_id,
                        "edgionconfigdata",
                        &namespace,
                        &name,
                        &document,
                    )
                    .await
                    .map_err(|error| format!("RegionRouteOverride sync failed: {error}"))?;
                }
                upsert_resource(
                    &state,
                    &controller_id,
                    "edgionplugins",
                    &plugin_namespace,
                    &plugin_name,
                    &plugin,
                )
                .await
                .map_err(|error| format!("EdgionPlugins sync failed: {error}"))
            }
            .await;
            RegionRouteSyncTargetResult {
                controller_id,
                success: result.is_ok(),
                error: result.err(),
            }
        }
    });
    let results = futures::future::join_all(futures).await;
    let modified = results.iter().filter(|result| result.success).count();
    let failed = results.len() - modified;
    let status = if failed == 0 {
        StatusCode::OK
    } else if modified == 0 {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::MULTI_STATUS
    };
    (
        status,
        Json(serde_json::json!({
            "success": failed == 0,
            "data": {
                "modified": modified,
                "failed": failed,
                "targets": results,
            }
        })),
    )
}

fn resource_path(kind: &str, namespace: &str, name: &str) -> String {
    format!("/api/v1/namespaced/{kind}/{namespace}/{name}")
}

async fn read_resource(
    state: &ApiState,
    controller_id: &str,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<Option<serde_json::Value>, String> {
    let response = state
        .proxy
        .forward(
            controller_id,
            "GET".to_string(),
            resource_path(kind, namespace, name),
            HashMap::new(),
            Vec::new(),
        )
        .await
        .map_err(|(status, error)| format!("read dispatch failed ({status}): {error}"))?;
    match response.status_code {
        200..=299 => serde_json::from_slice(&response.body)
            .map(Some)
            .map_err(|error| format!("Controller returned invalid resource JSON: {error}")),
        404 => Ok(None),
        status => Err(format!(
            "read returned status {status}: {}",
            response_detail(&response.body)
        )),
    }
}

async fn upsert_resource(
    state: &ApiState,
    controller_id: &str,
    kind: &str,
    namespace: &str,
    name: &str,
    source: &serde_json::Value,
) -> Result<(), String> {
    let current = read_resource(state, controller_id, kind, namespace, name).await?;
    let mut document = source.clone();
    prepare_resource_document(
        &mut document,
        namespace,
        name,
        current.as_ref().and_then(|value| {
            value
                .pointer("/metadata/resourceVersion")
                .and_then(serde_json::Value::as_str)
        }),
    )?;

    let (method, path) = if current.is_some() {
        ("PUT", resource_path(kind, namespace, name))
    } else {
        ("POST", format!("/api/v1/namespaced/{kind}/{namespace}"))
    };
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());
    if let Some(resource_version) = document
        .pointer("/metadata/resourceVersion")
        .and_then(serde_json::Value::as_str)
    {
        headers.insert("if-match".to_string(), format!("\"{resource_version}\""));
    }
    let body = serde_json::to_vec(&document)
        .map_err(|error| format!("failed to serialize resource: {error}"))?;
    let response = state
        .proxy
        .forward(controller_id, method.to_string(), path, headers, body)
        .await
        .map_err(|(status, error)| format!("write dispatch failed ({status}): {error}"))?;
    if (200..300).contains(&response.status_code) {
        Ok(())
    } else {
        Err(format!(
            "write returned status {}: {}",
            response.status_code,
            response_detail(&response.body)
        ))
    }
}

fn prepare_resource_document(
    document: &mut serde_json::Value,
    namespace: &str,
    name: &str,
    resource_version: Option<&str>,
) -> Result<(), String> {
    let object = document
        .as_object_mut()
        .ok_or_else(|| "source resource is not a JSON object".to_string())?;
    object.remove("status");
    let metadata = object
        .get_mut("metadata")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| "source resource is missing metadata".to_string())?;
    for field in [
        "creationTimestamp",
        "deletionGracePeriodSeconds",
        "deletionTimestamp",
        "generateName",
        "generation",
        "managedFields",
        "selfLink",
        "uid",
    ] {
        metadata.remove(field);
    }
    metadata.insert(
        "namespace".to_string(),
        serde_json::Value::String(namespace.to_string()),
    );
    metadata.insert(
        "name".to_string(),
        serde_json::Value::String(name.to_string()),
    );
    match resource_version {
        Some(value) => {
            metadata.insert(
                "resourceVersion".to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        None => {
            metadata.remove("resourceVersion");
        }
    }
    Ok(())
}

fn region_route_override_identity(
    plugin: &serde_json::Value,
    entry_index: usize,
    default_namespace: &str,
) -> Result<Option<(String, String)>, String> {
    let entry = plugin
        .pointer(&format!("/spec/requestPlugins/{entry_index}"))
        .ok_or_else(|| format!("source EdgionPlugins has no requestPlugins[{entry_index}]"))?;
    if entry.get("type").and_then(serde_json::Value::as_str) != Some("RegionRoute") {
        return Err(format!(
            "source requestPlugins[{entry_index}] is not a RegionRoute"
        ));
    }
    let Some(reference) = entry.pointer("/config/overrideRef") else {
        return Ok(None);
    };
    let name = reference
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "RegionRoute overrideRef is missing name".to_string())?;
    let namespace = reference
        .get("namespace")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_namespace);
    Ok(Some((namespace.to_string(), name.to_string())))
}

fn response_detail(body: &[u8]) -> String {
    let detail = String::from_utf8_lossy(body);
    detail.chars().take(500).collect()
}

// ============= Failover Handlers =============

/// `POST /api/v1/center/region-routes/failover`
///
/// The request shape selects one of two different code paths:
///
/// - `region_name` alone (no `plugin_name`/`entry_index`): delegates to
///   [`direct_override_failover`], which writes `failoverTo` on the
///   identified `namespace`/`name` `EdgionConfigData` directly through the
///   shared `config_data_ops::write_config_data` core on every online
///   controller. Response `data` carries the `modified`/`failed` counts
///   alongside the full per-controller `outcomes` list — see
///   [`failover_response`].
/// - `plugin_name` + `entry_index`: resolves each online controller's
///   permitted `RegionRouteOverride` reference from `list_region_routes()`
///   and fans out `POST /api/v1/cluster-region-routes/failover` to each
///   Controller's dedicated endpoint (see [`fan_out_failover`]).
///   `list_region_routes()` is empty in production, so this branch is
///   effectively dead; it is kept only until CCI-09 removes it. Response
///   `data` is `{ modified: N, failed: N }`.
///
/// Both the old cluster- and service- paths redirect (308) to this unified
/// endpoint.
pub async fn region_route_failover(
    State(state): State<ApiState>,
    Json(req): Json<FailoverRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.plugin_name.is_none() && req.entry_index.is_none() {
        return direct_override_failover(&state, req).await;
    }
    let online = match state.online_controller_ids().await {
        Ok(online) if !online.is_empty() => online,
        Ok(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "success": false, "error": "no online controllers" })),
            );
        }
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "success": false, "error": error.to_string() })),
            );
        }
    };
    let mut failed_before_dispatch = 0;
    let targets = if let (Some(plugin_name), Some(entry_index)) =
        (&req.plugin_name, req.entry_index)
    {
        let route = state
            .metadata_store
            .list_region_routes()
            .into_iter()
            .find(|route| {
                route.namespace == req.namespace
                    && route.plugin_name == *plugin_name
                    && route.entry_index == entry_index
            });
        let Some(route) = route else {
            return (
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::json!({ "success": false, "error": "RegionRoute entry not found" }),
                ),
            );
        };
        online
            .into_iter()
            .filter_map(|controller_id| {
                let reference = route
                    .controllers
                    .get(&controller_id)
                    .and_then(|view| view.override_ref.as_ref())
                    .filter(|reference| reference.permitted);
                match reference {
                    Some(reference) => Some((
                        controller_id,
                        ControllerFailoverRequest {
                            namespace: reference.namespace.clone(),
                            name: reference.name.clone(),
                            region_name: req.region_name.clone(),
                            failover_to: req.failover_to.clone(),
                        },
                    )),
                    None => {
                        failed_before_dispatch += 1;
                        None
                    }
                }
            })
            .collect()
    } else {
        online
            .into_iter()
            .map(|controller_id| {
                (
                    controller_id,
                    ControllerFailoverRequest {
                        namespace: req.namespace.clone(),
                        name: req.name.clone(),
                        region_name: req.region_name.clone(),
                        failover_to: req.failover_to.clone(),
                    },
                )
            })
            .collect()
    };
    let result = fan_out_failover(
        &state,
        "/api/v1/cluster-region-routes/failover".to_string(),
        targets,
        failed_before_dispatch,
    )
    .await;
    match result {
        Ok((modified, failed)) => {
            let status = if failed == 0 {
                StatusCode::OK
            } else if modified == 0 {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::MULTI_STATUS
            };
            (
                status,
                Json(serde_json::json!({
                    "success": failed == 0,
                    "data": { "modified": modified, "failed": failed },
                })),
            )
        }
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "success": false, "error": error.to_string() })),
        ),
    }
}

pub async fn service_region_route_failover(
    State(state): State<ApiState>,
    Json(req): Json<FailoverRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    direct_override_failover(&state, req).await
}

/// Writes the requested `failoverTo` to every online controller through the
/// shared [`write_config_data`] core: for each controller, locate `region_name`
/// under `/spec/data/config/regions` in the cached `EdgionConfigData` document
/// and set its `failoverTo`, then observe convergence locally. Region and
/// service overrides are the same resource kind (`edgionconfigdata`) from the
/// write core's point of view, so both callers share this path.
async fn direct_override_failover(
    state: &ApiState,
    req: FailoverRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let online = match state.online_controller_ids().await {
        Ok(online) if !online.is_empty() => online,
        Ok(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "success": false, "error": "no online controllers" })),
            );
        }
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "success": false, "error": error.to_string() })),
            );
        }
    };

    let mutate = |document: &mut serde_json::Value| -> Result<(), String> {
        set_region_failover_to(document, &req.region_name, &req.failover_to)
    };
    let predicate = |document: &serde_json::Value| -> bool {
        region_failover_matches(document, &req.region_name, &req.failover_to)
    };

    let outcomes: Vec<WriteOutcome> =
        futures::future::join_all(online.iter().map(|controller_id| {
            write_config_data(
                state,
                controller_id,
                &req.namespace,
                &req.name,
                &mutate,
                &predicate,
            )
        }))
        .await;

    failover_response(outcomes)
}

/// Aggregate per-controller [`WriteOutcome`]s into the failover HTTP response:
/// 200 when every controller converged, 502 when every controller failed,
/// 207 for anything mixed in between. `modified`/`failed` are kept alongside
/// the outcome list for existing web callers, split by whether the write
/// actually landed on that controller — NOT by whether convergence was
/// observed:
///   - `modified`: `Converged`, `Superseded`, `Accepted`, `Unknown` — all of
///     these had a 2xx write accepted by the Controller; `Unknown` only means
///     this replica could not *observe* convergence, not that the write
///     failed.
///   - `failed`: `Failed`, `Conflict` — nothing was applied on that
///     controller. A `Conflict` (409) means the CAS precondition was
///     rejected outright, so it must not be counted as modified.
fn failover_response(outcomes: Vec<WriteOutcome>) -> (StatusCode, Json<serde_json::Value>) {
    let failed = outcomes
        .iter()
        .filter(|outcome| matches!(outcome.state, OutcomeState::Failed | OutcomeState::Conflict))
        .count();
    let modified = outcomes.len() - failed;
    let status = if outcomes
        .iter()
        .all(|outcome| outcome.state == OutcomeState::Converged)
    {
        StatusCode::OK
    } else if outcomes
        .iter()
        .all(|outcome| outcome.state == OutcomeState::Failed)
    {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::MULTI_STATUS
    };
    (
        status,
        Json(serde_json::json!({
            "success": status == StatusCode::OK,
            "data": {
                "modified": modified,
                "failed": failed,
                "outcomes": outcomes,
            },
        })),
    )
}

/// JSON pointer to the regions array inside an `EdgionConfigData` document's
/// failover-relevant config. Centralized so a future path change cannot be
/// applied to the mutate side and missed on the predicate side (or vice
/// versa) — see [`region_index`], which is the single lookup both use.
const REGIONS_POINTER: &str = "/spec/data/config/regions";

/// Index of the region named `region_name` under [`REGIONS_POINTER`], or
/// `None` if the document has no such array or no region with that name.
/// Both [`region_failover_matches`] (predicate) and [`set_region_failover_to`]
/// (mutate) resolve the region through this one function, so the lookup rule
/// — path plus match field — lives in exactly one place.
fn region_index(document: &serde_json::Value, region_name: &str) -> Option<usize> {
    document
        .pointer(REGIONS_POINTER)?
        .as_array()?
        .iter()
        .position(|region| {
            region.get("name").and_then(serde_json::Value::as_str) == Some(region_name)
        })
}

/// Report whether `region_name`'s `failoverTo` already equals `failover_to`.
/// Used both as the write core's idempotent-skip check and its convergence
/// predicate. Reports `false` (not an error) when the region is absent — the
/// mutate path ([`set_region_failover_to`]) is what surfaces a missing
/// region as an error.
fn region_failover_matches(
    document: &serde_json::Value,
    region_name: &str,
    failover_to: &str,
) -> bool {
    let Some(index) = region_index(document, region_name) else {
        return false;
    };
    document
        .pointer(REGIONS_POINTER)
        .and_then(serde_json::Value::as_array)
        .and_then(|regions| regions.get(index))
        .and_then(|region| region.get("failoverTo"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        == failover_to
}

/// Set `failoverTo` on the region named `region_name` under
/// [`REGIONS_POINTER`]. Errors (rather than silently no-oping) when the
/// region is absent, so the write core surfaces it as `Failed` for that
/// controller instead of reporting a false convergence.
fn set_region_failover_to(
    document: &mut serde_json::Value,
    region_name: &str,
    failover_to: &str,
) -> Result<(), String> {
    let index = region_index(document, region_name)
        .ok_or_else(|| format!("region '{region_name}' was not found under {REGIONS_POINTER}"))?;
    let region = document
        .pointer_mut(REGIONS_POINTER)
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|regions| regions.get_mut(index))
        .ok_or_else(|| format!("region '{region_name}' was not found under {REGIONS_POINTER}"))?;
    region["failoverTo"] = serde_json::Value::String(failover_to.to_string());
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ControllerFailoverRequest {
    namespace: String,
    name: String,
    region_name: String,
    failover_to: String,
}

/// Fan-out a POST request to all online controllers in parallel.
/// Returns `(modified_count, failed_count)`.
///
/// Body is re-serialized from the typed `FailoverRequest` here — Center never
/// forwards raw upstream bytes, ensuring controllers always receive a clean
/// 4-field payload regardless of how the operator structured the original request.
async fn fan_out_failover(
    state: &ApiState,
    path: String,
    targets: Vec<(String, ControllerFailoverRequest)>,
    failed_before_dispatch: usize,
) -> edgion_center_core::CoreResult<(usize, usize)> {
    let futs = targets.into_iter().map(|(controller_id, request)| {
        let proxy = state.proxy.clone();
        let path = path.clone();
        let body = serde_json::to_vec(&request)
            .expect("ControllerFailoverRequest contains only serializable fields");
        async move {
            let mut headers = HashMap::new();
            headers.insert("content-type".to_string(), "application/json".to_string());
            let result = proxy
                .forward(&controller_id, "POST".to_string(), path, headers, body)
                .await;
            match result {
                Ok(r) if r.status_code == 200 => {
                    tracing::debug!(
                        component = "center",
                        controller_id = %controller_id,
                        "RegionRoute failover forwarded successfully"
                    );
                    true
                }
                Ok(r) => {
                    tracing::warn!(
                        component = "center",
                        controller_id = %controller_id,
                        status = r.status_code,
                        "RegionRoute failover POST returned non-OK status"
                    );
                    false
                }
                Err((status, msg)) => {
                    tracing::warn!(
                        component = "center",
                        controller_id = %controller_id,
                        status = %status,
                        error = %msg,
                        "RegionRoute failover POST failed"
                    );
                    false
                }
            }
        }
    });

    let results = futures::future::join_all(futs).await;
    let modified = results.iter().filter(|&&ok| ok).count();
    let failed = results.iter().filter(|&&ok| !ok).count() + failed_before_dispatch;
    Ok((modified, failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiState;
    use axum::extract::State;
    use axum::Json;
    use std::sync::Arc;

    /// Minimal `ApiState` for handler tests; mirrors the builder in `src/api/mod.rs`.
    fn test_api_state() -> ApiState {
        use crate::aggregator::ResourceAggregator;
        use crate::fed_sync::registry::ControllerRegistry;
        use crate::metadata_store::CenterMetaDataStore;
        use crate::proxy::ProxyForwarder;
        use crate::watch_cache::{CenterSyncClient, CenterWatchCacheRegistry};
        use edgion_center_core::AuthzMode;
        use parking_lot::Mutex;
        use std::collections::HashMap;

        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let proxy = Arc::new(ProxyForwarder::new(
            registry.clone(),
            Arc::new(Mutex::new(HashMap::new())),
            5,
        ));
        ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            proxy,
            controller_directory: None,
            global_resource_sync: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: None,
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
            authz_mode: AuthzMode::AllowAll,
            platform_mode: edgion_center_core::CenterMode::Standalone,
            capabilities: edgion_center_core::CenterCapabilities::for_mode(
                edgion_center_core::CenterMode::Standalone,
            ),
        }
    }

    #[tokio::test]
    async fn list_region_routes_returns_aggregated() {
        use crate::metadata_store::EffectiveRegionRouteView;
        let state = test_api_state();
        let route = EffectiveRegionRouteView {
            namespace: "default".into(),
            plugin_name: "rr1".into(),
            alias: Some("primary".into()),
            entry_index: 0,
            my_region: "east".into(),
            regions: serde_json::json!([]),
            key_get: serde_json::json!([]),
            hash_key_get: None,
            hash_calc: None,
            route_rules: serde_json::json!([]),
            route_by_key_conf_match: None,
            dye: Some(serde_json::json!({
                "headerName": "X-Edgion-Dye",
                "headerValue": "canary"
            })),
            override_ref: None,
            override_applied: false,
            service_usages: Vec::new(),
        };
        state
            .metadata_store
            .replace_region_routes("ctrl-a", vec![route]);
        let Json(v) = list_region_routes(State(state)).await.unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["data"].as_array().unwrap().len(), 1);
        assert_eq!(v["data"][0]["onlineControllerIds"], serde_json::json!([]));
        assert_eq!(
            v["data"][0]["controllers"]["ctrl-a"]["dye"],
            serde_json::json!({
                "headerName": "X-Edgion-Dye",
                "headerValue": "canary"
            })
        );
        assert!(
            v["data"][0]["controllers"]["ctrl-a"]
                .get("dyeHeaders")
                .is_none(),
            "the stale dyeHeaders compatibility alias must not be serialized"
        );
    }

    #[tokio::test]
    async fn override_lists_are_fed_by_watched_resource_type() {
        use crate::watch_cache::CenterConfHandler;
        let state = test_api_state();
        state.metadata_store.full_set(
            "ctrl-a",
            &HashMap::from([(
                "shop/checkout".to_string(),
                Arc::new(serde_json::json!({
                    "metadata": {"namespace": "shop", "name": "checkout"},
                    "spec": {
                        "data": {
                            "type": "ServiceRegionRouteOverride",
                            "config": {"regions": [{"name": "east"}]}
                        }
                    }
                })),
            )]),
        );
        let Json(region) = list_region_route_overrides(State(state.clone()))
            .await
            .unwrap();
        let Json(service) = list_service_region_route_overrides(State(state))
            .await
            .unwrap();
        assert_eq!(region["data"].as_array().unwrap().len(), 0);
        assert_eq!(service["data"].as_array().unwrap().len(), 1);
        assert_eq!(service["data"][0]["namespace"], "shop");
        assert_eq!(service["data"][0]["name"], "checkout");
    }

    #[test]
    fn failover_request_rejects_unknown_fields() {
        let json = r#"{
            "namespace": "default",
            "name": "test-cluster-route",
            "regionName": "east",
            "failoverTo": "west",
            "myRegion": "evil"
        }"#;
        let result: Result<FailoverRequest, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "unknown field 'myRegion' must be rejected at center"
        );
    }

    #[test]
    fn failover_request_rejects_full_resource_payload() {
        // A real-world risk: someone tries to send a full EdgionConfigData YAML/JSON
        // instead of the compact 4-field failover body.
        let json = r#"{
            "namespace": "default",
            "name": "test-cluster-route",
            "regionName": "east",
            "failoverTo": "west",
            "spec": {"metadata": {"config": {"myRegion": "evil"}}}
        }"#;
        let result: Result<FailoverRequest, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "spec field must be rejected — center failover does not accept full resource payload"
        );
    }

    /// With no online controllers, failover must not claim success because no
    /// target accepted the requested state change.
    #[tokio::test]
    async fn region_route_failover_no_online_controllers_returns_unavailable() {
        let state = test_api_state();
        let req = FailoverRequest {
            namespace: "default".into(),
            name: "rr-override".into(),
            plugin_name: None,
            entry_index: None,
            region_name: "east".into(),
            failover_to: "west".into(),
        };
        let (status, Json(v)) = region_route_failover(State(state), Json(req)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(v["success"], false);
    }

    /// Failover now writes through the shared `config_data_ops::write_config_data`
    /// core instead of fanning out blind POSTs and sleeping. Two online
    /// controllers: one whose cached document already carries the requested
    /// `failoverTo` (idempotent skip -> Converged), one absent from the local
    /// watch cache entirely (-> Failed). The response must carry both
    /// per-controller outcomes and report 207 for the mixed result.
    #[tokio::test]
    async fn failover_reports_per_controller_outcomes() {
        use crate::aggregator::ControllerInfo;

        let state = test_api_state();

        let online_info = |controller_id: &str| ControllerInfo {
            controller_id: controller_id.to_string(),
            cluster: "cluster-a".into(),
            environments: Vec::new(),
            tags: Vec::new(),
        };
        state
            .aggregator
            .set_controller_info("ctrl-a", online_info("ctrl-a"));
        state
            .aggregator
            .set_controller_info("ctrl-b", online_info("ctrl-b"));

        // ctrl-a already has the requested failoverTo cached -> idempotent skip.
        state
            .sync_client
            .plugin_metadata
            .get_or_create("ctrl-a")
            .replace_all(
                vec![(
                    "default/rr-override".to_string(),
                    serde_json::json!({
                        "metadata": {
                            "namespace": "default",
                            "name": "rr-override",
                            "resourceVersion": "1"
                        },
                        "spec": {
                            "data": {
                                "config": {
                                    "regions": [{"name": "east", "failoverTo": "west"}]
                                }
                            }
                        }
                    }),
                )],
                1,
                "server-1".to_string(),
            );
        // ctrl-b is online but has no cached document for this key at all.

        let req = FailoverRequest {
            namespace: "default".into(),
            name: "rr-override".into(),
            plugin_name: None,
            entry_index: None,
            region_name: "east".into(),
            failover_to: "west".into(),
        };
        let (status, Json(v)) = region_route_failover(State(state), Json(req)).await;

        assert_eq!(status, StatusCode::MULTI_STATUS);
        assert_eq!(v["data"]["modified"], 1);
        assert_eq!(v["data"]["failed"], 1);
        let outcomes = v["data"]["outcomes"].as_array().expect("outcomes array");
        assert_eq!(outcomes.len(), 2);
        let outcome_for = |controller_id: &str| {
            outcomes
                .iter()
                .find(|outcome| outcome["controllerId"] == controller_id)
                .unwrap_or_else(|| panic!("missing outcome for {controller_id}"))
        };
        assert_eq!(outcome_for("ctrl-a")["state"], "converged");
        assert_eq!(outcome_for("ctrl-b")["state"], "failed");
    }

    /// A `Conflict` (409: the CAS precondition was rejected outright) must be
    /// counted as `failed`, not `modified` — nothing was actually applied on
    /// that controller. Exercises `failover_response` directly: reproducing
    /// a real 409 would require a live proxied session, which is exactly why
    /// `config_data_ops`'s own tests use `write_config_data_with_dispatch`'s
    /// injectable seam instead — `write_config_data` exposes no such seam to
    /// callers, by design.
    #[test]
    fn failover_response_counts_conflict_as_failed_not_modified() {
        let outcomes = vec![
            WriteOutcome {
                controller_id: "ctrl-a".into(),
                state: OutcomeState::Converged,
                reason: None,
                observed: None,
                convergence_ms: Some(0),
            },
            WriteOutcome {
                controller_id: "ctrl-b".into(),
                state: OutcomeState::Conflict,
                reason: Some("CAS precondition no longer matched".into()),
                observed: None,
                convergence_ms: None,
            },
        ];

        let (status, Json(v)) = failover_response(outcomes);

        assert_eq!(status, StatusCode::MULTI_STATUS);
        assert_eq!(
            v["data"]["modified"], 1,
            "only the Converged outcome counts as modified"
        );
        assert_eq!(
            v["data"]["failed"], 1,
            "a Conflict outcome must count as failed, since nothing landed"
        );
    }

    #[test]
    fn failover_request_roundtrip_canonical() {
        let json = r#"{"namespace":"default","name":"r","regionName":"east","failoverTo":"west"}"#;
        let req: FailoverRequest = serde_json::from_str(json).expect("canonical payload accepted");
        assert_eq!(req.namespace, "default");
        assert_eq!(req.region_name, "east");
        assert_eq!(req.failover_to, "west");
        // Re-serialize: only the 4 contract fields, nothing else.
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("\"regionName\":\"east\""));
        assert!(!s.contains("myRegion"));
        assert!(!s.contains("spec"));
    }

    #[test]
    fn prepare_resource_document_removes_server_owned_fields() {
        let mut document = serde_json::json!({
            "apiVersion": "edgion.io/v1",
            "kind": "EdgionPlugins",
            "metadata": {
                "name": "route",
                "namespace": "source",
                "resourceVersion": "10",
                "uid": "uid",
                "generation": 3,
                "managedFields": []
            },
            "spec": { "requestPlugins": [] },
            "status": { "conditions": [] }
        });
        prepare_resource_document(&mut document, "target", "route", Some("42")).unwrap();
        assert_eq!(document["metadata"]["namespace"], "target");
        assert_eq!(document["metadata"]["resourceVersion"], "42");
        assert!(document["metadata"].get("uid").is_none());
        assert!(document["metadata"].get("managedFields").is_none());
        assert!(document.get("status").is_none());
    }

    /// `sync_watched_override` (Task 4) must now write through the shared
    /// `config_data_ops::write_config_data` core and aggregate results with
    /// `failover_response`, exactly like `direct_override_failover` does:
    /// per-controller `outcomes` with a `state` field, and an idempotent
    /// skip (target already carries the source's `spec.data`) reported as
    /// `converged` without a write ever being dispatched.
    #[tokio::test]
    async fn override_sync_reports_per_controller_outcomes_and_idempotent_skip() {
        use crate::aggregator::ControllerInfo;

        let state = test_api_state();
        let online_info = |controller_id: &str| ControllerInfo {
            controller_id: controller_id.to_string(),
            cluster: "cluster-a".into(),
            environments: Vec::new(),
            tags: Vec::new(),
        };
        for id in ["ctrl-src", "ctrl-a", "ctrl-b"] {
            state.aggregator.set_controller_info(id, online_info(id));
        }

        let override_doc = |version: &str| {
            serde_json::json!({
                "metadata": {
                    "namespace": "default",
                    "name": "rr-override",
                    "resourceVersion": version
                },
                "spec": {
                    "data": {
                        "type": "RegionRouteOverride",
                        "config": { "regions": [{"name": "east", "hashRange": [0, 100]}] }
                    }
                }
            })
        };
        // Source document, seeded through the watch cache so both the local
        // cache (read by write_config_data) and the metadata_store's
        // derived override view (read by sync_watched_override's row
        // lookup) agree, exactly as the federation watch stream keeps them
        // in production.
        state
            .sync_client
            .plugin_metadata
            .get_or_create("ctrl-src")
            .replace_all(
                vec![("default/rr-override".to_string(), override_doc("1"))],
                1,
                "server-1".to_string(),
            );
        // ctrl-a already carries the same spec.data as the source -> the
        // write core's idempotent skip must fire without ever dispatching.
        state
            .sync_client
            .plugin_metadata
            .get_or_create("ctrl-a")
            .replace_all(
                vec![("default/rr-override".to_string(), override_doc("9"))],
                1,
                "server-1".to_string(),
            );
        // ctrl-b is online (a valid sync target) but has no cached document
        // for this key at all, so the write core must fail it before ever
        // dispatching.

        let req = OverrideSyncRequest {
            namespace: "default".into(),
            name: "rr-override".into(),
            source_controller_id: "ctrl-src".into(),
            target_controller_ids: vec!["ctrl-a".into(), "ctrl-b".into()],
        };
        let (status, Json(v)) = region_route_override_sync(State(state), Json(req)).await;

        assert_eq!(status, StatusCode::MULTI_STATUS);
        assert_eq!(v["data"]["modified"], 1);
        assert_eq!(v["data"]["failed"], 1);
        let outcomes = v["data"]["outcomes"].as_array().expect("outcomes array");
        assert_eq!(outcomes.len(), 2);
        let outcome_for = |controller_id: &str| {
            outcomes
                .iter()
                .find(|outcome| outcome["controllerId"] == controller_id)
                .unwrap_or_else(|| panic!("missing outcome for {controller_id}"))
        };
        assert_eq!(outcome_for("ctrl-a")["state"], "converged");
        assert_eq!(
            outcome_for("ctrl-a")["convergenceMs"],
            0,
            "idempotent skip must report convergence without a write"
        );
        assert_eq!(outcome_for("ctrl-b")["state"], "failed");
    }

    #[test]
    fn extracts_region_route_override_with_default_namespace() {
        let plugin = serde_json::json!({
            "spec": {
                "requestPlugins": [{
                    "type": "RegionRoute",
                    "config": { "overrideRef": { "name": "route-override" } }
                }]
            }
        });
        assert_eq!(
            region_route_override_identity(&plugin, 0, "shop").unwrap(),
            Some(("shop".to_string(), "route-override".to_string()))
        );
    }
}
