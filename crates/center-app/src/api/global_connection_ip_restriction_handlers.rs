//! HTTP handlers for /api/v1/center/global-connection-ip-restrictions endpoints.
//!
//! Remaining surface (read-only):
//!   GET   /api/v1/center/global-connection-ip-restrictions                        → list (read)
//!   GET   /api/v1/center/global-connection-ip-restrictions/{ns}/{name}            → get (read)
//!   GET   .../global-connection-ip-restrictions/consistency                        → consistency check (read)
//!
//! Base CRUD (create/update/delete/enable/sync) has been retired; GIR base config
//! is git-owned via EdgionStreamPlugins and must be modified through GitOps.
//!
//! The Selector active-profile switch (`PATCH .../{ns}/{name}/active-profile`) has
//! been removed: switching a profile is a plain write to a Selector
//! `EdgionConfigData`, which the generic ConfigData write path already covers, so a
//! GIR-specific fan-out endpoint was a second spelling of the same operation. The
//! Center-side management of this shared plugin is deliberately dropped for now;
//! whatever replaces it is a future design question, not a re-source of this code.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::api::consistency_handlers::ConsistencyResult;
use crate::api::ApiState;
use crate::common::api::ApiResponse;
use crate::metadata_store::EffectiveGirView;

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
}
