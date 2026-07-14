//! HTTP handlers for /api/v1/center/global-connection-ip-restrictions endpoints.
//!
//! Surviving surface (D7 architecture, git-owned GIR base config):
//!   GET   /api/v1/center/global-connection-ip-restrictions                        → list (read)
//!   GET   /api/v1/center/global-connection-ip-restrictions/{ns}/{name}            → get (read)
//!   PATCH .../{ns}/{name}/active-profile                                           → switch active profile (fan-out Selector PUT)
//!   GET   .../global-connection-ip-restrictions/consistency                        → consistency check (read)
//!
//! Base CRUD (create/update/delete/enable/sync) has been retired; GIR base config
//! is now git-owned via EdgionStreamPlugins and must be modified through GitOps.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::api::consistency_handlers::ConsistencyResult;
use crate::api::ApiState;
use crate::common::api::ApiResponse;
use crate::common::observe::fed_metrics;
use crate::metadata_store::EffectiveGirView;

/// Classify a fan-out outcome into the `result` label value emitted on
/// `edgion_fed_fanout_total{op,result}`. "ok" means every target
/// succeeded, "fail" means every target failed, "partial" is the mixed
/// case. Zero-target calls are treated as "ok" since there is nothing
/// that could have failed.
fn fanout_result_label(success_count: usize, failed_count: usize) -> &'static str {
    if failed_count == 0 {
        fed_metrics::labels::fanout_result::OK
    } else if success_count == 0 {
        fed_metrics::labels::fanout_result::FAIL
    } else {
        fed_metrics::labels::fanout_result::PARTIAL
    }
}

/// Effective GIR aggregated view returned by the read endpoints.
/// Populated from the background poller's snapshot (`metadata_store.gir_effective`).
/// Replaces the old `CenterGlobalIpRestrictionView` which relied on the dead
/// `global_ip_restrictions` fed-sync feed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CenterGirAggregatedView {
    pub namespace: String,
    pub plugin_name: String,
    /// Per-controller effective GIR view keyed by controller_id.
    pub controllers: HashMap<String, EffectiveGirView>,
    pub online_controller_ids: Vec<String>,
}

async fn online_controllers(state: &ApiState) -> edgion_center_core::CoreResult<Vec<String>> {
    state.online_controller_ids().await
}

/// `GET /api/v1/center/global-connection-ip-restrictions`
///
/// Returns all GIR effective views aggregated across controllers, read from the
/// background-poller snapshot (`metadata_store.gir_effective`).
pub async fn list_global_ip_restrictions(
    State(state): State<ApiState>,
) -> Result<
    Json<ApiResponse<Vec<CenterGirAggregatedView>>>,
    (StatusCode, Json<ApiResponse<Vec<CenterGirAggregatedView>>>),
> {
    state.require_effective_read_model().map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::err_body(error.to_string())),
        )
    })?;
    let entries = state.metadata_store.list_gir_effective();
    let online = online_controllers(&state).await.map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::err_body(error.to_string())),
        )
    })?;
    let items: Vec<_> = entries
        .into_iter()
        .map(|e| CenterGirAggregatedView {
            namespace: e.namespace,
            plugin_name: e.plugin_name,
            controllers: e.controllers,
            online_controller_ids: online.clone(),
        })
        .collect();
    Ok(Json(ApiResponse::ok_body(items)))
}

/// `GET /api/v1/center/global-connection-ip-restrictions/{ns}/{name}`
///
/// `name` is matched against `plugin_name` in the effective GIR store.
pub async fn get_global_ip_restriction(
    State(state): State<ApiState>,
    Path((ns, name)): Path<(String, String)>,
) -> Result<Json<ApiResponse<CenterGirAggregatedView>>, (StatusCode, Json<ApiResponse<()>>)> {
    state.require_effective_read_model().map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::err_body(error.to_string())),
        )
    })?;
    let Some(entry) = state
        .metadata_store
        .list_gir_effective()
        .into_iter()
        .find(|v| v.namespace == ns && v.plugin_name == name)
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::err_body(format!(
                "GlobalConnectionIpRestriction {}/{} not found",
                ns, name
            ))),
        ));
    };
    let online = online_controllers(&state).await.map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::err_body(error.to_string())),
        )
    })?;
    Ok(Json(ApiResponse::ok_body(CenterGirAggregatedView {
        namespace: entry.namespace,
        plugin_name: entry.plugin_name,
        controllers: entry.controllers,
        online_controller_ids: online,
    })))
}

/// `GET /api/v1/center/global-connection-ip-restrictions/consistency`
///
/// For each (namespace, plugin_name) GIR key, restricts to online controllers and
/// checks whether all agree on the effective `active_profile`. Offline controllers
/// are excluded to avoid false positives during initial sync or after a drop.
pub async fn global_ip_restrictions_consistency(
    State(state): State<ApiState>,
) -> Result<
    Json<ApiResponse<Vec<ConsistencyResult>>>,
    (StatusCode, Json<ApiResponse<Vec<ConsistencyResult>>>),
> {
    state.require_effective_read_model().map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::err_body(error.to_string())),
        )
    })?;
    // Collect the set of currently-online controller IDs.
    let online: HashSet<String> = online_controllers(&state)
        .await
        .map_err(|error| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ApiResponse::err_body(error.to_string())),
            )
        })?
        .into_iter()
        .collect();

    let entries = state.metadata_store.list_gir_effective();
    let items: Vec<_> = entries
        .into_iter()
        .map(|e| {
            // Collect active_profile values only from online controllers.
            let online_profiles: Vec<String> = e
                .controllers
                .iter()
                .filter(|(ctrl_id, _)| online.contains(*ctrl_id))
                .map(|(_, gir)| gir.active_profile.clone())
                .collect();

            if online_profiles.len() <= 1 {
                // Zero or one online controller — divergence is impossible.
                return ConsistencyResult {
                    namespace: e.namespace,
                    name: e.plugin_name,
                    consistent: true,
                    controller_count: online_profiles.len(),
                    conflicts: Vec::new(),
                };
            }

            let distinct: HashSet<&str> = online_profiles.iter().map(String::as_str).collect();
            let consistent = distinct.len() <= 1;
            let conflicts = if consistent {
                Vec::new()
            } else {
                vec!["activeProfile".to_string()]
            };

            ConsistencyResult {
                namespace: e.namespace,
                name: e.plugin_name,
                consistent,
                controller_count: online_profiles.len(),
                conflicts,
            }
        })
        .collect();
    Ok(Json(ApiResponse::ok_body(items)))
}

// =====================================================================
// Fan-out helpers and response types (shared by write endpoints)
// =====================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerOpResult {
    pub controller_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FanOutResponse {
    pub success: Vec<ControllerOpResult>,
    pub failed: Vec<ControllerOpResult>,
    pub warnings: Vec<String>,
}

/// Resolve `controllers: ["all"]` or `["ctrl-a","ctrl-b"]` into (targets, warnings).
/// `["all"]` → all online controllers. Offline explicit IDs stay in targets (the write will fail them).
async fn resolve_targets(
    state: &ApiState,
    requested: &[String],
) -> edgion_center_core::CoreResult<(Vec<String>, Vec<String>)> {
    if requested.len() == 1 && requested[0] == "all" {
        let summaries = state.controller_summaries().await?;
        let online = summaries
            .iter()
            .filter(|summary| summary.online)
            .map(|summary| summary.controller_id.clone())
            .collect();
        let warnings: Vec<String> = summaries
            .into_iter()
            .filter(|summary| !summary.online)
            .map(|summary| format!("{} offline, skipped", summary.controller_id))
            .collect();
        Ok((online, warnings))
    } else {
        Ok((requested.to_vec(), Vec::new()))
    }
}

/// Fan-out a HTTP op to N controllers in parallel, collecting per-controller outcomes.
///
/// Note: `proxy.forward` returns `Result<HttpProxyResponse, (StatusCode, String)>`.
/// `HttpProxyResponse.status_code` is `u32` (from proto uint32).
async fn fan_out_http(
    state: &ApiState,
    controllers: Vec<String>,
    method: &str,
    path: String,
    body: Vec<u8>,
) -> Vec<ControllerOpResult> {
    let futs = controllers.into_iter().map(|cid| {
        let proxy = state.proxy.clone();
        let method = method.to_string();
        let path = path.clone();
        let body = body.clone();
        async move {
            let mut headers = HashMap::new();
            headers.insert("content-type".to_string(), "application/json".to_string());
            match proxy.forward(&cid, method, path, headers, body).await {
                Ok(r) if r.status_code >= 200 && r.status_code < 300 => ControllerOpResult {
                    controller_id: cid,
                    detail: Some(format!("status {}", r.status_code)),
                    error: None,
                    status_code: Some(r.status_code as u16),
                },
                Ok(r) => ControllerOpResult {
                    controller_id: cid,
                    detail: None,
                    error: Some(format!("HTTP {}", r.status_code)),
                    status_code: Some(r.status_code as u16),
                },
                Err((status, msg)) => ControllerOpResult {
                    controller_id: cid,
                    detail: None,
                    error: Some(msg),
                    status_code: Some(status.as_u16()),
                },
            }
        }
    });
    futures::future::join_all(futs).await
}

fn partition(
    results: Vec<ControllerOpResult>,
) -> (Vec<ControllerOpResult>, Vec<ControllerOpResult>) {
    let (success, failed): (Vec<_>, Vec<_>) = results.into_iter().partition(|r| r.error.is_none());
    (success, failed)
}

/// Map a fan-out outcome to an HTTP status. When every target failed (success
/// empty, failed non-empty) we surface `502 Bad Gateway` so automated callers
/// can gate retries on the status line without parsing the nested body. Any
/// partial success — and the zero-target case — stays `200 OK`.
fn fanout_status(success: &[ControllerOpResult], failed: &[ControllerOpResult]) -> StatusCode {
    if success.is_empty() && !failed.is_empty() {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::OK
    }
}

/// Build the PUT path and JSON body for a Selector EdgionConfigData update.
///
/// Returns `(path, json_body)` where:
/// - `path` is `/api/v1/namespaced/edgionconfigdata/{ns}/{name}`
/// - `json_body` is a full EdgionConfigData document with `spec.enable:true` and
///   `spec.data = {"type":"Selector","config":{"active":<active_profile>}}`
fn build_selector_put(ns: &str, name: &str, active_profile: &str) -> (String, String) {
    let path = format!("/api/v1/namespaced/edgionconfigdata/{}/{}", ns, name);
    let doc = serde_json::json!({
        "apiVersion": "edgion.io/v1",
        "kind": "EdgionConfigData",
        "metadata": { "name": name, "namespace": ns },
        "spec": {
            "enable": true,
            "data": {
                "type": "Selector",
                "config": { "active": active_profile }
            }
        }
    });
    let json = serde_json::to_string(&doc).expect("serde_json serialize");
    (path, json)
}

// =====================================================================
// PATCH /api/v1/center/global-connection-ip-restrictions/{ns}/{name}/active-profile
// =====================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchActiveProfileRequest {
    pub active_profile: String,
    pub controllers: Vec<String>,
}

/// `PATCH /api/v1/center/global-connection-ip-restrictions/{ns}/{name}/active-profile`
///
/// Resolves the active-profile Selector for the GIR `(ns, name)` from the effective store,
/// then fan-outs a PUT of the Selector EdgionConfigData to the target controllers.
/// Center patches an EXISTING Selector only (D7 architecture); if the GIR has no
/// `active_profile_ref` configured, returns 400 — it does not create a Selector.
pub async fn patch_active_profile(
    State(state): State<ApiState>,
    Path((ns, name)): Path<(String, String)>,
    Json(req): Json<PatchActiveProfileRequest>,
) -> (StatusCode, Json<ApiResponse<FanOutResponse>>) {
    if let Err(error) = state.require_effective_read_model() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::err_body(error.to_string())),
        );
    }
    // Resolve which Selector EdgionConfigData to patch by reading the effective GIR view.
    // The active_profile_ref field names the Selector in the same namespace as the GIR plugin.
    let selector_name = {
        let gir_row = state
            .metadata_store
            .list_gir_effective()
            .into_iter()
            .find(|v| v.namespace == ns && v.plugin_name == name);
        let selector_ref = gir_row.and_then(|row| {
            // All controllers share the same base spec; take the first non-None ref.
            row.controllers
                .into_values()
                .find_map(|v| v.active_profile_ref)
        });
        match selector_ref {
            Some(s) => s,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse::err_body(
                        "no active-profile selector configured".to_string(),
                    )),
                );
            }
        }
    };

    let (targets, warnings) = match resolve_targets(&state, &req.controllers).await {
        Ok(resolved) => resolved,
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ApiResponse::err_body(error.to_string())),
            );
        }
    };
    let (path, body_str) = build_selector_put(&ns, &selector_name, &req.active_profile);
    let body = body_str.into_bytes();
    let results = fan_out_http(&state, targets, "PUT", path, body).await;
    let (success, failed) = partition(results);
    fed_metrics::record_fanout(
        fed_metrics::labels::fanout_op::PATCH_PROFILE,
        fanout_result_label(success.len(), failed.len()),
    );
    let status = fanout_status(&success, &failed);
    (
        status,
        Json(ApiResponse::ok_body(FanOutResponse {
            success,
            failed,
            warnings,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata_store::EffectiveGirView;
    use axum::extract::State;

    /// Minimal `ApiState` for handler tests — mirrors the builder in `region_route_handlers.rs`.
    fn test_api_state() -> ApiState {
        use crate::aggregator::ResourceAggregator;
        use crate::commander::Commander;
        use crate::fed_sync::registry::ControllerRegistry;
        use crate::metadata_store::CenterMetaDataStore;
        use crate::proxy::ProxyForwarder;
        use crate::watch_cache::{CenterSyncClient, CenterWatchCacheRegistry};
        use edgion_center_core::AuthzMode;
        use parking_lot::Mutex;
        use std::collections::HashMap;
        use std::sync::Arc;

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

    fn make_register_info(controller_id: &str) -> crate::aggregator::ControllerInfo {
        crate::aggregator::ControllerInfo {
            controller_id: controller_id.to_string(),
            cluster: "test-cluster".to_string(),
            environments: vec![],
            tags: vec![],
        }
    }

    fn make_gir_view(plugin_name: &str, active_profile: &str) -> EffectiveGirView {
        EffectiveGirView {
            namespace: "default".into(),
            plugin_name: plugin_name.into(),
            enable: true,
            active_profile: active_profile.into(),
            profiles: serde_json::json!({}),
            active_profile_ref: None,
            selector_applied: false,
        }
    }

    /// Seed one GIR into the effective store and assert `list_global_ip_restrictions`
    /// returns it aggregated by (ns, plugin_name) with the controller entry present.
    #[tokio::test]
    async fn list_global_ip_restrictions_returns_aggregated() {
        let state = test_api_state();
        state
            .metadata_store
            .replace_gir("ctrl-a", vec![make_gir_view("gir1", "strict")]);
        let Json(resp) = match list_global_ip_restrictions(State(state)).await {
            Ok(response) => response,
            Err(_) => panic!("membership lookup should succeed"),
        };
        assert!(resp.success, "response must be success:true");
        let data = resp.data.expect("data must be present");
        assert_eq!(data.len(), 1, "one GIR key");
        assert_eq!(data[0].plugin_name, "gir1");
        assert_eq!(data[0].namespace, "default");
        assert_eq!(data[0].controllers.len(), 1);
        assert!(data[0].controllers.contains_key("ctrl-a"));
    }

    /// `get_global_ip_restriction` must return 404 when the effective store has no entry.
    #[tokio::test]
    async fn get_global_ip_restriction_not_found_returns_404() {
        let state = test_api_state();
        let result = get_global_ip_restriction(
            State(state),
            Path(("default".to_string(), "nonexistent".to_string())),
        )
        .await;
        assert!(result.is_err(), "must return 404 for missing GIR");
        // Use if-let to extract the status code without requiring Debug on the Ok type.
        if let Err((status, _)) = result {
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
    }

    /// `get_global_ip_restriction` must return the view when the entry exists.
    #[tokio::test]
    async fn get_global_ip_restriction_found_returns_view() {
        let state = test_api_state();
        state
            .metadata_store
            .replace_gir("ctrl-a", vec![make_gir_view("gir1", "strict")]);
        let result = get_global_ip_restriction(
            State(state),
            Path(("default".to_string(), "gir1".to_string())),
        )
        .await;
        assert!(result.is_ok(), "must succeed when entry exists");
        // Use if-let to extract the response without requiring Debug on the Err type.
        if let Ok(Json(resp)) = result {
            assert!(resp.success);
            let view = resp.data.expect("data must be present");
            assert_eq!(view.plugin_name, "gir1");
            assert!(view.controllers.contains_key("ctrl-a"));
        }
    }

    /// Two online controllers with divergent `active_profile` for the same GIR key
    /// must be reported as inconsistent.
    #[tokio::test]
    async fn global_ip_consistency_flags_divergent_online_controllers() {
        let state = test_api_state();
        state
            .aggregator
            .set_controller_info("ctrl-a", make_register_info("ctrl-a"));
        state
            .aggregator
            .set_controller_info("ctrl-b", make_register_info("ctrl-b"));

        state
            .metadata_store
            .replace_gir("ctrl-a", vec![make_gir_view("gir1", "strict")]);
        state
            .metadata_store
            .replace_gir("ctrl-b", vec![make_gir_view("gir1", "open")]);

        let Json(resp) = match global_ip_restrictions_consistency(State(state)).await {
            Ok(response) => response,
            Err(_) => panic!("membership lookup should succeed"),
        };
        assert!(resp.success, "response must be success:true");
        let data = resp.data.expect("data must be present");
        assert_eq!(data.len(), 1, "one GIR key with a conflict");
        assert!(
            !data[0].consistent,
            "divergent active_profile => inconsistent"
        );
        assert_eq!(data[0].controller_count, 2);
        assert!(data[0].conflicts.contains(&"activeProfile".to_string()));
    }

    /// Offline ctrl-b must be excluded; only ctrl-a (online) is considered,
    /// so there can be no conflict.
    #[tokio::test]
    async fn global_ip_consistency_excludes_offline_controllers() {
        let state = test_api_state();
        state
            .aggregator
            .set_controller_info("ctrl-a", make_register_info("ctrl-a"));
        state
            .aggregator
            .set_controller_info("ctrl-b", make_register_info("ctrl-b"));
        state.aggregator.mark_offline("ctrl-b");

        state
            .metadata_store
            .replace_gir("ctrl-a", vec![make_gir_view("gir1", "strict")]);
        state
            .metadata_store
            .replace_gir("ctrl-b", vec![make_gir_view("gir1", "open")]);

        let Json(resp) = match global_ip_restrictions_consistency(State(state)).await {
            Ok(response) => response,
            Err(_) => panic!("membership lookup should succeed"),
        };
        assert!(resp.success, "response must be success:true");
        let data = resp.data.expect("data must be present");
        assert_eq!(data.len(), 1, "one GIR key");
        assert!(data[0].consistent, "offline ctrl-b excluded => consistent");
        assert_eq!(data[0].controller_count, 1, "only ctrl-a is online");
        assert!(
            data[0].conflicts.is_empty(),
            "no conflicts with single online controller"
        );
    }

    /// `build_selector_put` must produce the correct path and a JSON body that
    /// describes a Selector EdgionConfigData with the given active profile.
    #[test]
    fn build_selector_put_path_and_body() {
        let (path, json) = build_selector_put("prod-ns", "my-selector", "strict");
        assert_eq!(
            path,
            "/api/v1/namespaced/edgionconfigdata/prod-ns/my-selector"
        );
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            v.pointer("/kind").and_then(|x| x.as_str()),
            Some("EdgionConfigData"),
            "/kind must be EdgionConfigData"
        );
        assert_eq!(
            v.pointer("/spec/data/type").and_then(|x| x.as_str()),
            Some("Selector"),
            "/spec/data/type must be Selector"
        );
        assert_eq!(
            v.pointer("/spec/data/config/active")
                .and_then(|x| x.as_str()),
            Some("strict"),
            "/spec/data/config/active must match the requested profile"
        );
    }
}
