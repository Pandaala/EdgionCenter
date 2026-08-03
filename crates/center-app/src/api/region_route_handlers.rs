//! RegionRouteOverride handlers and shared helpers for Center aggregation handlers.
//!
//! # Center RegionRouteOverride API contract
//!
//! Every endpoint here reads or writes the override documents fed by the
//! federation EdgionConfigData watch. The RegionRoute base table lives in
//! `EdgionPlugins` and is owned by the business teams, so Center never
//! aggregates it.
//!
//! | Method | Path                                             | Description                                                 |
//! |--------|--------------------------------------------------|-------------------------------------------------------------|
//! | GET    | `/api/v1/center/region-route-overrides`          | Watch-fed RegionRouteOverride rows across controllers        |
//! | POST   | `/api/v1/center/region-route-overrides/failover` | Set `failoverTo` on one region, on every online controller   |
//! | POST   | `/api/v1/center/region-route-overrides/sync`     | Copy one controller's override document to selected targets  |
//!
//! All writes go through the shared `config_data_ops::write_config_data` core.
//!
//! # There is exactly one override dimension
//!
//! An earlier model also carried a Service dimension
//! (`ServiceRegionRouteOverride`, with its own list/failover/sync endpoints and
//! its own dashboard tab). Edgion never had that type: RegionRoute config is
//! attached per HTTPRoute/GRPCRoute rule through an `EdgionPlugins`
//! ExtensionRef filter, and each `EdgionPlugins` object carries its own
//! `overrideRef`. Per-service failover is therefore already expressible with
//! `RegionRouteOverride` alone: scope is a property of which object references
//! a document, not of the document's own type. The Service surface was removed
//! rather than left permanently empty. Do not reintroduce it.
//!
//! `failover` pins the `/spec/data/type` its target must carry, so it can only
//! ever rewrite a `RegionRouteOverride`. `sync` has no equivalent guard — it
//! validates the source row (which comes from the type-classified metadata
//! store) but replaces each target's whole `/spec/data` without checking what
//! that target currently is.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::config_data_ops::{write_config_data, OutcomeState, WriteOutcome};
use super::ApiState;

// ============= Failover Request Type =============

/// Strongly-typed body for `/api/v1/center/region-route-overrides/failover`.
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
    pub region_name: String,
    /// Empty string = clear failover.
    pub failover_to: String,
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

// ============= RegionRouteOverride Handlers =============

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

pub async fn region_route_override_sync(
    State(state): State<ApiState>,
    Json(req): Json<OverrideSyncRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    sync_watched_override(&state, req).await
}

async fn sync_watched_override(
    state: &ApiState,
    req: OverrideSyncRequest,
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
    let row = state
        .metadata_store
        .list_region_route_overrides()
        .into_iter()
        .find(|row| row.namespace == req.namespace && row.name == req.name);
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

// ============= Failover Handlers =============

/// `POST /api/v1/center/region-route-overrides/failover`
///
/// Writes `failoverTo` on the identified `namespace`/`name` `EdgionConfigData`
/// through the shared `config_data_ops::write_config_data` core on every online
/// controller. Response `data` carries the `modified`/`failed` counts alongside
/// the full per-controller `outcomes` list — see [`failover_response`].
pub async fn region_route_failover(
    State(state): State<ApiState>,
    Json(req): Json<FailoverRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    direct_override_failover(&state, req, REGION_OVERRIDE_TYPE).await
}

/// The only `/spec/data/type` this endpoint may write.
const REGION_OVERRIDE_TYPE: &str = "RegionRouteOverride";

/// Report whether the document's `/spec/data/type` is exactly `expected`.
/// Exact and case-sensitive, matching how the watch read model classifies a
/// document — a case variant or typo is a different type, not a near miss.
fn config_data_type_is(document: &serde_json::Value, expected: &str) -> bool {
    document
        .pointer("/spec/data/type")
        .and_then(serde_json::Value::as_str)
        == Some(expected)
}

/// Writes the requested `failoverTo` to every online controller through the
/// shared [`write_config_data`] core: for each controller, locate `region_name`
/// under `/spec/data/config/regions` in the cached `EdgionConfigData` document
/// and set its `failoverTo`, then observe convergence locally.
///
/// `expected_type` keeps the write inside its own resource type: a request
/// identifies its target by `namespace`/`name` alone, and the watch cache holds
/// every `EdgionConfigData` type, so without the check a failover could rewrite
/// an unrelated document that merely shares the name. The check runs in BOTH
/// closures deliberately — the write core evaluates `predicate` first as an
/// idempotent skip, so a `mutate`-only check would be bypassed whenever the
/// wrong-type document already happened to carry the requested `failoverTo`.
async fn direct_override_failover(
    state: &ApiState,
    req: FailoverRequest,
    expected_type: &str,
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
        if !config_data_type_is(document, expected_type) {
            return Err(format!(
                "document is not a {expected_type}: spec.data.type is {}",
                document
                    .pointer("/spec/data/type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("absent")
            ));
        }
        set_region_failover_to(document, &req.region_name, &req.failover_to)
    };
    let predicate = |document: &serde_json::Value| -> bool {
        config_data_type_is(document, expected_type)
            && region_failover_matches(document, &req.region_name, &req.failover_to)
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

/// How one [`OutcomeState`] counts toward the aggregate failover response.
///
/// The classification exists so the response is derived from an exhaustive
/// `match` instead of a `matches!` predicate plus subtraction: a seventh
/// `OutcomeState` must not be able to fall silently into the "landed" bin.
/// Adding a state without extending [`OutcomeClass::of`] is a compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutcomeClass {
    /// The write landed and the intent is currently in effect.
    Converged,
    /// The write landed, but convergence is not confirmed to be in effect
    /// right now (`Superseded`, `Accepted`, `Unknown`).
    Landed,
    /// Nothing was applied: the CAS precondition was rejected (409).
    Conflicted,
    /// Nothing was applied, for any other reason.
    Failed,
}

impl OutcomeClass {
    fn of(state: OutcomeState) -> Self {
        match state {
            OutcomeState::Converged => OutcomeClass::Converged,
            OutcomeState::Superseded | OutcomeState::Accepted | OutcomeState::Unknown => {
                OutcomeClass::Landed
            }
            OutcomeState::Conflict => OutcomeClass::Conflicted,
            OutcomeState::Failed => OutcomeClass::Failed,
        }
    }

    /// Whether the write was applied on that controller. Exhaustive for the
    /// same reason [`OutcomeClass::of`] is: a new class must not default into
    /// either bin.
    fn landed(self) -> bool {
        match self {
            OutcomeClass::Converged | OutcomeClass::Landed => true,
            OutcomeClass::Conflicted | OutcomeClass::Failed => false,
        }
    }
}

/// Aggregate per-controller [`WriteOutcome`]s into the failover HTTP response:
/// 200 when every controller converged, 409 when every controller rejected the
/// CAS precondition, 502 when nothing landed anywhere for any other reason,
/// 207 for anything genuinely mixed in between. `modified`/`failed` are kept
/// alongside the outcome list for existing web callers, split by whether the
/// write actually landed on that controller — NOT by whether convergence was
/// observed:
///   - `modified`: `Converged`, `Superseded`, `Accepted`, `Unknown` — all of
///     these had a 2xx write accepted by the Controller; `Unknown` only means
///     this replica could not *observe* convergence, not that the write
///     failed.
///   - `failed`: `Failed`, `Conflict` — nothing was applied on that
///     controller. A `Conflict` (409) means the CAS precondition was
///     rejected outright, so it must not be counted as modified.
///
/// The status is driven by the same classification as the counters, so a
/// batch counted as entirely `failed` can never be reported as 207
/// "multi-status". An all-`Conflict` batch reports 409 rather than 502: every
/// Controller answered correctly and rejected a stale write, so the operator
/// needs "refresh and retry", not an upstream-failure signal.
fn failover_response(outcomes: Vec<WriteOutcome>) -> (StatusCode, Json<serde_json::Value>) {
    let classes: Vec<OutcomeClass> = outcomes
        .iter()
        .map(|outcome| OutcomeClass::of(outcome.state))
        .collect();
    let failed = classes.iter().filter(|class| !class.landed()).count();
    let modified = classes.len() - failed;
    let all = |target: OutcomeClass| classes.iter().all(|class| *class == target);
    let status = if all(OutcomeClass::Converged) {
        StatusCode::OK
    } else if all(OutcomeClass::Conflicted) {
        StatusCode::CONFLICT
    } else if classes.iter().all(|class| !class.landed()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiState;
    use axum::extract::State;
    use axum::Json;
    use std::collections::HashMap;
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
            authz_mode: AuthzMode::AllowAll,
            platform_mode: edgion_center_core::CenterMode::Standalone,
            capabilities: edgion_center_core::CenterCapabilities::for_mode(
                edgion_center_core::CenterMode::Standalone,
            ),
        }
    }

    /// The federation watch feeds every `EdgionConfigData` type into the store,
    /// but this list surface projects exactly one. A document of another type
    /// must not appear in it.
    #[tokio::test]
    async fn override_list_is_fed_only_by_the_watched_resource_type() {
        use crate::watch_cache::CenterConfHandler;
        let state = test_api_state();
        state.metadata_store.full_set(
            "ctrl-a",
            &HashMap::from([
                (
                    "shop/shared".to_string(),
                    Arc::new(serde_json::json!({
                        "metadata": {"namespace": "shop", "name": "shared"},
                        "spec": {
                            "data": {
                                "type": "RegionRouteOverride",
                                "config": {"regions": [{"name": "east"}]}
                            }
                        }
                    })),
                ),
                (
                    "shop/checkout".to_string(),
                    Arc::new(serde_json::json!({
                        "metadata": {"namespace": "shop", "name": "checkout"},
                        "spec": {
                            "data": {
                                "type": "KeyList",
                                "config": {"keys": ["tenant-a"]}
                            }
                        }
                    })),
                ),
            ]),
        );
        let Json(region) = list_region_route_overrides(State(state)).await.unwrap();
        assert_eq!(region["data"].as_array().unwrap().len(), 1);
        assert_eq!(region["data"][0]["namespace"], "shop");
        assert_eq!(region["data"][0]["name"], "shared");
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
                                "type": "RegionRouteOverride",
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

    /// A failover request identifies its target by `namespace`/`name` alone,
    /// and the watch cache holds every `EdgionConfigData` type — so only the
    /// `/spec/data/type` check keeps the write inside `RegionRouteOverride`.
    ///
    /// The fixture is deliberately adversarial: a `Misc` document whose config
    /// is region-table-shaped AND already carries the requested `failoverTo`.
    /// Without the guard the write core's idempotent skip would evaluate the
    /// predicate to true and report `converged` before `mutate` ever runs, so
    /// a `mutate`-only check would not catch this.
    #[tokio::test]
    async fn failover_refuses_a_document_of_another_config_data_type() {
        use crate::aggregator::ControllerInfo;

        let state = test_api_state();
        state.aggregator.set_controller_info(
            "ctrl-a",
            ControllerInfo {
                controller_id: "ctrl-a".into(),
                cluster: "cluster-a".into(),
                environments: Vec::new(),
                tags: Vec::new(),
            },
        );
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
                                "type": "Misc",
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

        let req = FailoverRequest {
            namespace: "default".into(),
            name: "rr-override".into(),
            region_name: "east".into(),
            failover_to: "west".into(),
        };
        let (status, Json(v)) = region_route_failover(State(state), Json(req)).await;

        // Nothing landed anywhere, and not because of a CAS conflict -> 502.
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(v["data"]["modified"], 0);
        assert_eq!(v["data"]["failed"], 1);
        let outcome = &v["data"]["outcomes"][0];
        assert_eq!(outcome["state"], "failed");
        assert_eq!(
            outcome["reason"],
            "document is not a RegionRouteOverride: spec.data.type is Misc"
        );
    }

    #[test]
    fn config_data_type_is_matches_exactly() {
        let document = serde_json::json!({
            "spec": {"data": {"type": "RegionRouteOverride"}}
        });
        assert!(config_data_type_is(&document, REGION_OVERRIDE_TYPE));
        assert!(!config_data_type_is(&document, "Misc"));
        // Case variants and a missing type are different types, not near misses.
        assert!(!config_data_type_is(
            &serde_json::json!({"spec": {"data": {"type": "regionrouteoverride"}}}),
            REGION_OVERRIDE_TYPE
        ));
        assert!(!config_data_type_is(
            &serde_json::json!({"spec": {"data": {}}}),
            REGION_OVERRIDE_TYPE
        ));
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

    fn outcome(controller_id: &str, state: OutcomeState) -> WriteOutcome {
        WriteOutcome {
            controller_id: controller_id.into(),
            state,
            reason: None,
            observed: None,
            convergence_ms: None,
        }
    }

    /// The status must be driven by the same landed/not-landed classification
    /// as the counters. A batch counted as entirely `failed` is not "mixed",
    /// so it must never report 207; and an all-`Conflict` batch is a stale-view
    /// rejection (409), not an upstream failure (502).
    #[test]
    fn failover_response_status_matches_the_outcome_classification() {
        let cases = [
            (
                vec![OutcomeState::Converged, OutcomeState::Converged],
                StatusCode::OK,
            ),
            (
                vec![OutcomeState::Conflict, OutcomeState::Conflict],
                StatusCode::CONFLICT,
            ),
            (
                vec![OutcomeState::Failed, OutcomeState::Failed],
                StatusCode::BAD_GATEWAY,
            ),
            (
                vec![OutcomeState::Conflict, OutcomeState::Failed],
                StatusCode::BAD_GATEWAY,
            ),
            (
                vec![OutcomeState::Converged, OutcomeState::Accepted],
                StatusCode::MULTI_STATUS,
            ),
            (
                vec![OutcomeState::Superseded, OutcomeState::Unknown],
                StatusCode::MULTI_STATUS,
            ),
            (
                vec![OutcomeState::Accepted, OutcomeState::Conflict],
                StatusCode::MULTI_STATUS,
            ),
        ];
        for (states, expected) in cases {
            let outcomes = states
                .iter()
                .enumerate()
                .map(|(index, state)| outcome(&format!("ctrl-{index}"), *state))
                .collect();
            let (status, _) = failover_response(outcomes);
            assert_eq!(status, expected, "unexpected status for {states:?}");
        }
    }

    /// Every state must land in exactly one counting bin, and the two bins
    /// must always sum to the outcome count — the property the old
    /// `matches!`-plus-subtraction split could not guarantee for a state it
    /// did not name.
    #[test]
    fn failover_response_counts_every_state_in_exactly_one_bin() {
        for state in [
            OutcomeState::Converged,
            OutcomeState::Superseded,
            OutcomeState::Accepted,
            OutcomeState::Conflict,
            OutcomeState::Failed,
            OutcomeState::Unknown,
        ] {
            let (_, Json(value)) = failover_response(vec![outcome("ctrl-a", state)]);
            let modified = value["data"]["modified"].as_u64().expect("modified count");
            let failed = value["data"]["failed"].as_u64().expect("failed count");
            assert_eq!(
                modified + failed,
                1,
                "{state:?} was counted twice or not at all"
            );
            let landed = matches!(
                state,
                OutcomeState::Converged
                    | OutcomeState::Superseded
                    | OutcomeState::Accepted
                    | OutcomeState::Unknown
            );
            assert_eq!(modified == 1, landed, "{state:?} landed in the wrong bin");
        }
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
}
