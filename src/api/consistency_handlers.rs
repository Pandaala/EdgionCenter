//! Consistency detection endpoints for Center.
//!
//! GET /api/v1/center/region-routes/consistency
//!   Compares RegionRoute effective views across online controllers and reports conflicts.
//!   Offline controllers are excluded to avoid false positives during initial sync or
//!   after a controller drops.
//!
//! Legacy paths redirect with 308 Permanent Redirect:
//! - GET /api/v1/center/cluster-region-routes/consistency → /api/v1/center/region-routes/consistency
//! - GET /api/v1/center/service-region-routes/consistency → /api/v1/center/region-routes/consistency

use std::collections::HashSet;

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::common::api::ApiResponse;

use super::ApiState;

// ---------------------------------------------------------------------------
// Report DTOs
// ---------------------------------------------------------------------------

/// One row in the consistency response — mirrors the shape used by the GIR
/// consistency endpoint so callers get a uniform contract.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsistencyResult {
    pub namespace: String,
    /// Route identifier: `plugin_name` when alias is absent, `plugin_name/alias` otherwise.
    pub name: String,
    pub consistent: bool,
    /// Number of online controllers that reported this route key.
    pub controller_count: usize,
    /// Field names that differ across online controllers (empty when consistent).
    pub conflicts: Vec<String>,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/v1/center/region-routes/consistency`
///
/// For each (namespace, plugin, alias) route key, restricts the per-controller
/// effective views to controllers that are currently online, then checks whether
/// all online controllers agree on the effective `regions` table.  A row is
/// reported as inconsistent when two or more online controllers disagree.
pub async fn region_routes_consistency(
    State(state): State<ApiState>,
) -> Json<ApiResponse<Vec<ConsistencyResult>>> {
    // Collect the set of currently-online controller IDs.
    let online: HashSet<String> = state
        .aggregator
        .controller_summaries()
        .into_iter()
        .filter(|s| s.online)
        .map(|s| s.controller_id)
        .collect();

    let routes = state.metadata_store.list_region_routes();
    let items: Vec<ConsistencyResult> = routes
        .into_iter()
        .map(|view| {
            // Build a human-readable name from plugin + optional alias.
            let name = match &view.alias {
                Some(alias) => format!("{}/{}", view.plugin_name, alias),
                None => view.plugin_name.clone(),
            };

            // Collect the regions values only for online controllers.
            let online_regions: Vec<&serde_json::Value> = view
                .controllers
                .iter()
                .filter(|(ctrl_id, _)| online.contains(*ctrl_id))
                .map(|(_, eff)| &eff.regions)
                .collect();

            if online_regions.len() <= 1 {
                // Zero or one online controller — divergence is impossible.
                return ConsistencyResult {
                    namespace: view.namespace,
                    name,
                    consistent: true,
                    controller_count: online_regions.len(),
                    conflicts: Vec::new(),
                };
            }

            // Compare serialized JSON; identical Value serializations mean agreement.
            let distinct: HashSet<String> = online_regions
                .iter()
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .collect();
            let consistent = distinct.len() <= 1;
            let conflicts = if consistent {
                Vec::new()
            } else {
                vec!["regions".to_string()]
            };

            ConsistencyResult {
                namespace: view.namespace,
                name,
                consistent,
                controller_count: online_regions.len(),
                conflicts,
            }
        })
        .collect();

    Json(ApiResponse::ok_body(items))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregator::ResourceAggregator;
    use crate::api::ApiState;
    use crate::commander::Commander;
    use crate::common::fed_sync::proto::RegisterRequest;
    use crate::config::AuthzMode;
    use crate::fed_sync::registry::ControllerRegistry;
    use crate::metadata_store::{CenterMetaDataStore, EffectiveRegionRouteView};
    use crate::proxy::ProxyForwarder;
    use crate::watch_cache::{CenterSyncClient, CenterWatchCacheRegistry};
    use axum::extract::State;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn test_api_state() -> ApiState {
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
            db: None,
            metadata_store,
            sync_client,
            registry,
            db_required: false,
            authz_mode: AuthzMode::AllowAll,
            db_auth_enabled: false,
        }
    }

    fn make_register_info(controller_id: &str) -> RegisterRequest {
        RegisterRequest {
            controller_id: controller_id.to_string(),
            cluster: "test-cluster".to_string(),
            env: vec![],
            tag: vec![],
            supported_kinds: vec![],
        }
    }

    fn make_region_route(regions: serde_json::Value) -> EffectiveRegionRouteView {
        EffectiveRegionRouteView {
            namespace: "default".into(),
            plugin_name: "rr1".into(),
            alias: None,
            my_region: "east".into(),
            regions,
            override_ref: None,
            override_applied: false,
        }
    }

    /// Two ONLINE controllers report divergent `regions` for the same route key.
    /// The handler must emit exactly one conflict row.
    #[tokio::test]
    async fn region_consistency_flags_divergent_controllers() {
        let state = test_api_state();

        // Seed both controllers as online.
        state
            .aggregator
            .set_controller_info("ctrl-a", make_register_info("ctrl-a"));
        state
            .aggregator
            .set_controller_info("ctrl-b", make_register_info("ctrl-b"));

        // Same route key, different `regions` values.
        state.metadata_store.replace_region_routes(
            "ctrl-a",
            vec![make_region_route(serde_json::json!({"east": "self", "west": "east"}))],
        );
        state.metadata_store.replace_region_routes(
            "ctrl-b",
            vec![make_region_route(serde_json::json!({"east": "self", "west": "west"}))],
        );

        let Json(resp) = region_routes_consistency(State(state)).await;
        assert!(resp.success, "response must be success:true");
        let data = resp.data.expect("data must be present");
        assert_eq!(data.len(), 1, "one route key with a conflict");
        assert!(!data[0].consistent, "route must be reported as inconsistent");
        assert_eq!(data[0].controller_count, 2, "two online controllers");
        assert!(
            data[0].conflicts.contains(&"regions".to_string()),
            "conflict field must be 'regions'"
        );
    }

    /// Same divergence but ctrl-b is marked OFFLINE — should be excluded, leaving
    /// only ctrl-a online.  A single online controller cannot produce a conflict.
    #[tokio::test]
    async fn region_consistency_excludes_offline() {
        let state = test_api_state();

        // ctrl-a online; ctrl-b registered then immediately marked offline.
        state
            .aggregator
            .set_controller_info("ctrl-a", make_register_info("ctrl-a"));
        state
            .aggregator
            .set_controller_info("ctrl-b", make_register_info("ctrl-b"));
        state.aggregator.mark_offline("ctrl-b");

        // Same divergent routes as the previous test.
        state.metadata_store.replace_region_routes(
            "ctrl-a",
            vec![make_region_route(serde_json::json!({"east": "self", "west": "east"}))],
        );
        state.metadata_store.replace_region_routes(
            "ctrl-b",
            vec![make_region_route(serde_json::json!({"east": "self", "west": "west"}))],
        );

        let Json(resp) = region_routes_consistency(State(state)).await;
        assert!(resp.success, "response must be success:true");
        let data = resp.data.expect("data must be present");
        assert_eq!(data.len(), 1, "one route key");
        assert!(data[0].consistent, "only one online controller => consistent");
        assert_eq!(data[0].controller_count, 1, "only ctrl-a is online");
        assert!(
            data[0].conflicts.is_empty(),
            "no conflicts when single online controller"
        );
    }
}
