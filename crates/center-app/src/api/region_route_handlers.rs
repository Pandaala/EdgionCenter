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
    pub name: String,
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
    let data = state.metadata_store.list_region_routes();
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
    let result = fan_out_failover(
        &state,
        "/api/v1/cluster-region-routes/failover".to_string(),
        &req,
    )
    .await;
    match result {
        Ok((modified, failed)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "data": { "modified": modified, "failed": failed },
            })),
        ),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "success": false, "error": error.to_string() })),
        ),
    }
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
    req: &FailoverRequest,
) -> edgion_center_core::CoreResult<(usize, usize)> {
    let body_bytes = match serde_json::to_vec(req) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(
                component = "center",
                error = %e,
                "FailoverRequest serialization failed unexpectedly"
            );
            unreachable!("FailoverRequest contains only String fields; serialization cannot fail");
        }
    };
    let online = state.online_controller_ids().await?;

    let futs = online.iter().map(|controller_id| {
        let proxy = state.proxy.clone();
        let controller_id = controller_id.clone();
        let path = path.clone();
        let body = body_bytes.clone();
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
    let failed = results.iter().filter(|&&ok| !ok).count();
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
            my_region: "east".into(),
            regions: serde_json::json!([]),
            override_ref: None,
            override_applied: false,
        };
        state
            .metadata_store
            .replace_region_routes("ctrl-a", vec![route]);
        let Json(v) = list_region_routes(State(state)).await.unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["data"].as_array().unwrap().len(), 1);
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

    /// With no online controllers, `region_route_failover` must return
    /// `{success:true, data:{modified:0, failed:0}}` rather than an error.
    #[tokio::test]
    async fn region_route_failover_no_online_controllers_returns_modified_zero() {
        let state = test_api_state();
        let req = FailoverRequest {
            namespace: "default".into(),
            name: "rr-override".into(),
            region_name: "east".into(),
            failover_to: "west".into(),
        };
        let (status, Json(v)) = region_route_failover(State(state), Json(req)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["success"], true, "response must be success:true");
        assert_eq!(
            v["data"]["modified"], 0,
            "no online controllers => modified:0"
        );
        assert_eq!(v["data"]["failed"], 0, "no online controllers => failed:0");
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
