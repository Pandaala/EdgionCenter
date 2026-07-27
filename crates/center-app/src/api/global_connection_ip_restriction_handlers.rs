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

use crate::api::config_data_ops::{write_config_data, OutcomeState, WriteOutcome};
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

            let mut conflicts = Vec::new();
            if online_profiles.len() != online.len() {
                conflicts.push("presence".to_string());
            }
            let distinct: HashSet<&str> = online_profiles.iter().map(String::as_str).collect();
            if distinct.len() > 1 {
                conflicts.push("activeProfile".to_string());
            }
            let consistent = conflicts.is_empty();

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
    /// The shared write core's outcome for this controller, when the result
    /// came from [`write_config_data`] — e.g. `patch_active_profile`. Added
    /// alongside the existing `detail`/`error`/`status_code` fields (rather
    /// than replacing them) so existing callers of this fan-out response
    /// shape are unaffected; new callers can read `outcome.state` for the
    /// same `converged`/`superseded`/`accepted`/... vocabulary the failover
    /// endpoint reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<WriteOutcome>,
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

/// Report whether the Selector's `/spec/data/config/active` already equals
/// `active_profile`. Used both as the write core's idempotent-skip check and
/// its convergence predicate. Mirrors `region_route_handlers::
/// region_failover_matches`: `false` (not an error) whenever the document
/// isn't a matching Selector at all — the mutate side is what surfaces that
/// as an error.
fn selector_active_matches(document: &serde_json::Value, active_profile: &str) -> bool {
    document
        .pointer("/spec/data/type")
        .and_then(serde_json::Value::as_str)
        == Some("Selector")
        && document
            .pointer("/spec/data/config/active")
            .and_then(serde_json::Value::as_str)
            == Some(active_profile)
}

/// Set `/spec/data/config/active` to `active_profile`.
///
/// The Controller update API performs whole-resource replacement. Mutating
/// the live cached document in place (rather than building a fresh one)
/// preserves labels, annotations, visibility, resourceVersion, and fields
/// introduced by newer Edgion versions.
fn set_selector_active(
    document: &mut serde_json::Value,
    active_profile: &str,
) -> Result<(), String> {
    let data = document
        .pointer_mut("/spec/data")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| "Selector document is missing spec.data".to_string())?;
    if data.get("type").and_then(serde_json::Value::as_str) != Some("Selector") {
        return Err("referenced EdgionConfigData is not a Selector".to_string());
    }
    let config = data
        .get_mut("config")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| "Selector document is missing spec.data.config".to_string())?;
    config.insert(
        "active".to_string(),
        serde_json::Value::String(active_profile.to_string()),
    );
    Ok(())
}

/// Classify a [`WriteOutcome`] into `detail`/`error`, using the same split
/// [`region_route_handlers::failover_response`] uses: `Converged`,
/// `Superseded`, `Accepted`, and `Unknown` all mean the write landed on that
/// controller, so they populate `detail`, not `error`; only `Failed` and
/// `Conflict` mean nothing was applied, so only those populate `error`. The
/// full outcome is always attached so callers that want the richer
/// `converged`/`superseded`/... vocabulary can read `outcome.state` instead.
fn controller_op_result_from_outcome(outcome: WriteOutcome) -> ControllerOpResult {
    let is_failure = matches!(outcome.state, OutcomeState::Failed | OutcomeState::Conflict);
    ControllerOpResult {
        controller_id: outcome.controller_id.clone(),
        detail: (!is_failure).then(|| outcome_state_label(outcome.state).to_string()),
        error: is_failure.then(|| {
            outcome
                .reason
                .clone()
                .unwrap_or_else(|| outcome_state_label(outcome.state).to_string())
        }),
        status_code: None,
        outcome: Some(outcome),
    }
}

fn outcome_state_label(state: OutcomeState) -> &'static str {
    match state {
        OutcomeState::Converged => "converged",
        OutcomeState::Superseded => "superseded",
        OutcomeState::Accepted => "accepted",
        OutcomeState::Conflict => "conflict",
        OutcomeState::Failed => "failed",
        OutcomeState::Unknown => "unknown",
    }
}

/// Fan out the active-profile switch to every target through the shared
/// `config_data_ops::write_config_data` core: for each controller, set
/// `/spec/data/config/active` on its own Selector document (namespace/name
/// resolved per-controller from `selector_refs`) and observe convergence
/// locally, exactly like `region_route_handlers::direct_override_failover`
/// does for RegionRoute failover.
async fn fan_out_selector_update(
    state: &ApiState,
    controllers: Vec<(String, String, String)>,
    active_profile: String,
) -> Vec<ControllerOpResult> {
    let futs = controllers
        .into_iter()
        .map(|(controller_id, namespace, name)| {
            let state = state.clone();
            let active_profile = active_profile.clone();
            async move {
                let mutate = |document: &mut serde_json::Value| -> Result<(), String> {
                    set_selector_active(document, &active_profile)
                };
                let predicate = |document: &serde_json::Value| -> bool {
                    selector_active_matches(document, &active_profile)
                };
                let outcome = write_config_data(
                    &state,
                    &controller_id,
                    &namespace,
                    &name,
                    &mutate,
                    &predicate,
                )
                .await;
                controller_op_result_from_outcome(outcome)
            }
        });
    futures::future::join_all(futs).await
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
    let selector_refs = {
        let gir_row = state
            .metadata_store
            .list_gir_effective()
            .into_iter()
            .find(|v| v.namespace == ns && v.plugin_name == name);
        let refs: HashMap<String, crate::metadata_store::EffectiveConfigDataRef> = gir_row
            .map(|row| {
                row.controllers
                    .into_iter()
                    .filter_map(|(controller_id, view)| {
                        view.active_profile_ref
                            .filter(|reference| reference.permitted)
                            .map(|reference| (controller_id, reference))
                    })
                    .collect()
            })
            .unwrap_or_default();
        match refs.is_empty() {
            false => refs,
            true => {
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
    if targets.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::err_body(
                "no online controllers matched the request".to_string(),
            )),
        );
    }
    let mut selector_targets = Vec::new();
    let mut results = Vec::new();
    for controller_id in targets {
        if let Some(reference) = selector_refs.get(&controller_id) {
            let selector_namespace = if reference.namespace.is_empty() {
                &ns
            } else {
                &reference.namespace
            };
            selector_targets.push((
                controller_id,
                selector_namespace.to_string(),
                reference.name.clone(),
            ));
        } else {
            results.push(ControllerOpResult {
                controller_id,
                detail: None,
                error: Some(
                    "controller has no active-profile selector in its effective view".to_string(),
                ),
                status_code: None,
                outcome: None,
            });
        }
    }
    results.extend(fan_out_selector_update(&state, selector_targets, req.active_profile).await);
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

    #[tokio::test]
    async fn global_ip_consistency_flags_missing_online_controller() {
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

        let Json(resp) = match global_ip_restrictions_consistency(State(state)).await {
            Ok(response) => response,
            Err(_) => panic!("membership lookup should succeed"),
        };
        let data = resp.data.expect("data must be present");
        assert_eq!(data.len(), 1);
        assert!(!data[0].consistent);
        assert_eq!(data[0].controller_count, 1);
        assert!(data[0].conflicts.contains(&"presence".to_string()));
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

    /// Selector updates must preserve the complete live resource and change only
    /// the active profile, because the downstream API performs full replacement.
    #[test]
    fn set_selector_active_preserves_metadata_and_unknown_fields() {
        let original = serde_json::json!({
            "apiVersion": "edgion.io/v1",
            "kind": "EdgionConfigData",
            "metadata": {
                "name": "my-selector",
                "namespace": "prod-ns",
                "resourceVersion": "42",
                "labels": { "owner": "gitops" },
                "annotations": { "future": "preserve" }
            },
            "spec": {
                "enable": false,
                "visibility": "Namespace",
                "futureSpec": true,
                "data": {
                    "type": "Selector",
                    "config": { "active": "open", "futureConfig": 7 }
                }
            }
        });
        let mut updated = original.clone();
        set_selector_active(&mut updated, "strict").expect("update Selector");
        assert_eq!(
            updated.pointer("/spec/data/config/active"),
            Some(&serde_json::json!("strict"))
        );
        let mut expected = original;
        expected["spec"]["data"]["config"]["active"] = serde_json::json!("strict");
        assert_eq!(updated, expected);
    }

    #[test]
    fn set_selector_active_rejects_non_selector_data() {
        let mut document = serde_json::json!({
            "spec": { "data": { "type": "Yaml", "config": {} } }
        });
        assert_eq!(
            set_selector_active(&mut document, "strict").unwrap_err(),
            "referenced EdgionConfigData is not a Selector"
        );
    }

    #[test]
    fn selector_active_matches_is_false_for_non_selector_data() {
        let document = serde_json::json!({
            "spec": { "data": { "type": "Yaml", "config": { "active": "strict" } } }
        });
        assert!(
            !selector_active_matches(&document, "strict"),
            "a non-Selector document must never report a match, even with a coincidentally equal field"
        );
    }

    /// The active-profile switch (Task 4) must now write through the shared
    /// `config_data_ops::write_config_data` core: each `ControllerOpResult`
    /// carries a `WriteOutcome` (`outcome.state`), and a target whose
    /// Selector already has the requested `active` profile is reported as
    /// an idempotent skip (`converged`, no write dispatched) rather than
    /// going through a blind GET+PUT.
    #[tokio::test]
    async fn patch_active_profile_reports_outcome_and_idempotent_skip() {
        let state = test_api_state();
        let mut view = make_gir_view("gir1", "strict");
        view.active_profile_ref = Some(crate::metadata_store::EffectiveConfigDataRef {
            namespace: "default".to_string(),
            name: "selector".to_string(),
            permitted: true,
        });
        state
            .metadata_store
            .replace_gir("ctrl-a", vec![view.clone()]);
        state.metadata_store.replace_gir("ctrl-b", vec![view]);

        let selector_doc = |active: &str, version: &str| {
            serde_json::json!({
                "metadata": {
                    "namespace": "default",
                    "name": "selector",
                    "resourceVersion": version
                },
                "spec": {
                    "data": {
                        "type": "Selector",
                        "config": { "active": active }
                    }
                }
            })
        };
        // ctrl-a's Selector already has the requested profile, seeded
        // through the local watch cache that `write_config_data` actually
        // reads -> idempotent skip.
        state
            .sync_client
            .plugin_metadata
            .get_or_create("ctrl-a")
            .replace_all(
                vec![("default/selector".to_string(), selector_doc("open", "5"))],
                1,
                "server-1".to_string(),
            );
        // ctrl-b is a valid target but has no cached Selector document at
        // all -> the write core must fail it before ever dispatching.

        let (status, Json(response)) = patch_active_profile(
            State(state),
            Path(("default".to_string(), "gir1".to_string())),
            Json(PatchActiveProfileRequest {
                active_profile: "open".to_string(),
                controllers: vec!["ctrl-a".to_string(), "ctrl-b".to_string()],
            }),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::OK,
            "GIR fan-out stays 200 on partial failure"
        );
        let data = response.data.expect("data must be present");
        assert_eq!(data.success.len(), 1);
        assert_eq!(data.failed.len(), 1);

        let ctrl_a = data
            .success
            .iter()
            .find(|result| result.controller_id == "ctrl-a")
            .expect("ctrl-a must be in success");
        let outcome = ctrl_a.outcome.as_ref().expect("outcome must be attached");
        assert_eq!(
            outcome.state,
            crate::api::config_data_ops::OutcomeState::Converged
        );
        assert_eq!(
            outcome.convergence_ms,
            Some(0),
            "idempotent skip must report convergence without a write"
        );

        let ctrl_b = data
            .failed
            .iter()
            .find(|result| result.controller_id == "ctrl-b")
            .expect("ctrl-b must be in failed");
        assert!(ctrl_b.error.is_some());
        let ctrl_b_outcome = ctrl_b.outcome.as_ref().expect("outcome must be attached");
        assert_eq!(
            ctrl_b_outcome.state,
            crate::api::config_data_ops::OutcomeState::Failed
        );
    }

    #[tokio::test]
    async fn patch_active_profile_rejects_zero_online_targets() {
        let state = test_api_state();
        let mut view = make_gir_view("gir1", "strict");
        view.active_profile_ref = Some(crate::metadata_store::EffectiveConfigDataRef {
            namespace: "default".to_string(),
            name: "selector".to_string(),
            permitted: true,
        });
        state.metadata_store.replace_gir("ctrl-a", vec![view]);

        let (status, response) = patch_active_profile(
            State(state),
            Path(("default".to_string(), "gir1".to_string())),
            Json(PatchActiveProfileRequest {
                active_profile: "open".to_string(),
                controllers: vec!["all".to_string()],
            }),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!response.success);
    }
}
