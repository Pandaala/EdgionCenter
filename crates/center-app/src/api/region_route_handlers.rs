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
use std::collections::HashMap;

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

// ============= Failover Handlers =============

/// `POST /api/v1/center/region-routes/failover`
///
/// Fans out `POST /api/v1/cluster-region-routes/failover` to all online controllers
/// with the same request body.  Both the old cluster- and service- paths redirect
/// (308) to this unified endpoint.
///
/// Response: `{ modified: N, failed: N }`
pub async fn region_route_failover(
    State(state): State<ApiState>,
    Json(req): Json<FailoverRequest>,
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
        use crate::commander::Commander;
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
}
