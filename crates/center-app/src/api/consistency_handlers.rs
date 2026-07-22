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
use axum::{http::StatusCode, Json};
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
    let online: HashSet<String> = state
        .online_controller_ids()
        .await
        .map_err(|error| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ApiResponse::err_body(error.to_string())),
            )
        })?
        .into_iter()
        .collect();

    let routes = state.metadata_store.list_region_routes();
    let items: Vec<ConsistencyResult> = routes
        .into_iter()
        .map(|view| {
            // Build a human-readable name from plugin + optional alias.
            let name = match &view.alias {
                Some(alias) => format!("{}/{} (#{})", view.plugin_name, alias, view.entry_index),
                None => format!("{} (#{})", view.plugin_name, view.entry_index),
            };

            let online_views: Vec<_> = view
                .controllers
                .iter()
                .filter(|(ctrl_id, _)| online.contains(*ctrl_id))
                .map(|(_, eff)| eff)
                .collect();

            let mut conflicts = Vec::new();
            if online_views.len() != online.len() {
                conflicts.push("presence".to_string());
            }
            let mut differs = |field: &str, values: Vec<String>| {
                if values.into_iter().collect::<HashSet<_>>().len() > 1 {
                    conflicts.push(field.to_string());
                }
            };
            differs(
                "regions",
                online_views.iter().map(|v| v.regions.to_string()).collect(),
            );
            differs(
                "keyGet",
                online_views.iter().map(|v| v.key_get.to_string()).collect(),
            );
            differs(
                "hashKeyGet",
                online_views
                    .iter()
                    .map(|v| format!("{:?}", v.hash_key_get))
                    .collect(),
            );
            differs(
                "hashCalc",
                online_views
                    .iter()
                    .map(|v| format!("{:?}", v.hash_calc))
                    .collect(),
            );
            differs(
                "routeRules",
                online_views
                    .iter()
                    .map(|v| v.route_rules.to_string())
                    .collect(),
            );
            differs(
                "routeByKeyConfMatch",
                online_views
                    .iter()
                    .map(|v| format!("{:?}", v.route_by_key_conf_match))
                    .collect(),
            );
            differs(
                "dyeHeaders",
                online_views
                    .iter()
                    .map(|v| format!("{:?}", v.dye_headers))
                    .collect(),
            );
            differs(
                "overrideRef",
                online_views
                    .iter()
                    .map(|v| format!("{:?}", v.override_ref))
                    .collect(),
            );
            differs(
                "overrideApplied",
                online_views
                    .iter()
                    .map(|v| v.override_applied.to_string())
                    .collect(),
            );
            let consistent = conflicts.is_empty();

            ConsistencyResult {
                namespace: view.namespace,
                name,
                consistent,
                controller_count: online_views.len(),
                conflicts,
            }
        })
        .collect();

    Ok(Json(ApiResponse::ok_body(items)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregator::{ControllerInfo, ResourceAggregator};
    use crate::api::ApiState;
    use crate::commander::Commander;
    use crate::fed_sync::registry::ControllerRegistry;
    use crate::metadata_store::{CenterMetaDataStore, EffectiveRegionRouteView};
    use crate::proxy::ProxyForwarder;
    use crate::watch_cache::{CenterSyncClient, CenterWatchCacheRegistry};
    use axum::extract::State;
    use edgion_center_core::AuthzMode;
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

    fn make_register_info(controller_id: &str) -> ControllerInfo {
        ControllerInfo {
            controller_id: controller_id.to_string(),
            cluster: "test-cluster".to_string(),
            environments: vec![],
            tags: vec![],
        }
    }

    fn make_region_route(regions: serde_json::Value) -> EffectiveRegionRouteView {
        EffectiveRegionRouteView {
            namespace: "default".into(),
            plugin_name: "rr1".into(),
            alias: None,
            entry_index: 0,
            my_region: "east".into(),
            regions,
            key_get: serde_json::json!([]),
            hash_key_get: None,
            hash_calc: None,
            route_rules: serde_json::json!([]),
            route_by_key_conf_match: None,
            dye_headers: None,
            override_ref: None,
            override_applied: false,
            service_usages: Vec::new(),
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
            vec![make_region_route(
                serde_json::json!({"east": "self", "west": "east"}),
            )],
        );
        state.metadata_store.replace_region_routes(
            "ctrl-b",
            vec![make_region_route(
                serde_json::json!({"east": "self", "west": "west"}),
            )],
        );

        let Json(resp) = match region_routes_consistency(State(state)).await {
            Ok(response) => response,
            Err(_) => panic!("membership lookup should succeed"),
        };
        assert!(resp.success, "response must be success:true");
        let data = resp.data.expect("data must be present");
        assert_eq!(data.len(), 1, "one route key with a conflict");
        assert!(
            !data[0].consistent,
            "route must be reported as inconsistent"
        );
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
            vec![make_region_route(
                serde_json::json!({"east": "self", "west": "east"}),
            )],
        );
        state.metadata_store.replace_region_routes(
            "ctrl-b",
            vec![make_region_route(
                serde_json::json!({"east": "self", "west": "west"}),
            )],
        );

        let Json(resp) = match region_routes_consistency(State(state)).await {
            Ok(response) => response,
            Err(_) => panic!("membership lookup should succeed"),
        };
        assert!(resp.success, "response must be success:true");
        let data = resp.data.expect("data must be present");
        assert_eq!(data.len(), 1, "one route key");
        assert!(
            data[0].consistent,
            "only one online controller => consistent"
        );
        assert_eq!(data[0].controller_count, 1, "only ctrl-a is online");
        assert!(
            data[0].conflicts.is_empty(),
            "no conflicts when single online controller"
        );
    }

    #[tokio::test]
    async fn region_consistency_flags_missing_online_controller() {
        let state = test_api_state();
        state
            .aggregator
            .set_controller_info("ctrl-a", make_register_info("ctrl-a"));
        state
            .aggregator
            .set_controller_info("ctrl-b", make_register_info("ctrl-b"));
        state
            .metadata_store
            .replace_region_routes("ctrl-a", vec![make_region_route(serde_json::json!([]))]);

        let Json(resp) = match region_routes_consistency(State(state)).await {
            Ok(response) => response,
            Err(_) => panic!("consistency response should succeed"),
        };
        let row = &resp.data.expect("data")[0];
        assert!(!row.consistent);
        assert!(row.conflicts.contains(&"presence".to_string()));
    }

    #[tokio::test]
    async fn region_consistency_allows_local_region_and_usage_differences() {
        let state = test_api_state();
        state
            .aggregator
            .set_controller_info("ctrl-a", make_register_info("ctrl-a"));
        state
            .aggregator
            .set_controller_info("ctrl-b", make_register_info("ctrl-b"));
        let mut east = make_region_route(serde_json::json!([]));
        east.my_region = "east".into();
        east.service_usages
            .push(crate::metadata_store::RegionRouteServiceUsage {
                route_kind: "HTTPRoute".into(),
                route_namespace: "east".into(),
                route_name: "api".into(),
                rule_index: 0,
                backend_services: Vec::new(),
            });
        let mut west = make_region_route(serde_json::json!([]));
        west.my_region = "west".into();
        state
            .metadata_store
            .replace_region_routes("ctrl-a", vec![east]);
        state
            .metadata_store
            .replace_region_routes("ctrl-b", vec![west]);

        let Json(resp) = match region_routes_consistency(State(state)).await {
            Ok(response) => response,
            Err(_) => panic!("consistency response should succeed"),
        };
        let row = &resp.data.expect("data")[0];
        assert!(row.consistent);
        assert!(row.conflicts.is_empty());
    }
}
