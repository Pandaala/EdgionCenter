//! Controller capability and namespace preflight for GlobalResources.
//!
//! The preflight is read-only. Effective verbs come from the Controller's
//! `/api/v1/access` self-introspection endpoint, while namespace reachability is
//! verified with bounded namespaced list requests through the federation proxy.

mod budget;
mod cache;

use crate::federation::registry::ControllerRegistry;
use crate::poll::ControllerHttpClient;
use budget::{InventoryBudget, InventoryBudgetLimits};
use cache::BoundedTtlCache;
use edgion_center_core::{
    ControllerDirectory, ControllerOwnerLocator, ControllerOwnerRoute, ControllerPhase,
    ControllerRecord, CoreError, GlobalResourceInventoryKind, GlobalResourcesConfig,
    GLOBAL_RESOURCE_KINDS,
};
use futures::{stream::FuturesUnordered, StreamExt};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::Semaphore;

const DEFAULT_LIST_PAGE_SIZE: usize = 200;
const DEFAULT_MAX_ITEMS_PER_CLUSTER: usize = 10_000;
const DEFAULT_MAX_PAGES_PER_NAMESPACE: usize = 100;
const DEFAULT_FANOUT_CONCURRENCY: usize = 8;
const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(2);
const DEFAULT_MAX_TARGET_CLUSTERS: usize = 100;
const DEFAULT_MAX_UPSTREAM_BODY_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_MAX_SINGLE_FLIGHTS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterResolutionState {
    Available,
    Offline,
    Ambiguous,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterResolution {
    pub cluster: String,
    pub state: ClusterResolutionState,
    pub controller_id: Option<String>,
    pub candidates: Vec<String>,
}

#[derive(Clone)]
pub(crate) enum TargetResolutionMode {
    Standalone(ControllerRegistry),
    Kubernetes(Arc<dyn ControllerOwnerLocator>),
}

#[derive(Clone)]
pub(crate) struct ResolvedTarget {
    pub(crate) resolution: ClusterResolution,
    pub(crate) session_id: Option<String>,
    pub(crate) owner_route: Option<ControllerOwnerRoute>,
}

#[derive(Clone, Copy)]
struct InventoryScanLimits {
    page_size: usize,
    max_pages_per_namespace: usize,
    max_items_per_cluster: usize,
}

pub fn resolve_cluster_targets(records: Vec<ControllerRecord>) -> Vec<ClusterResolution> {
    let mut grouped = BTreeMap::<String, Vec<ControllerRecord>>::new();
    for record in records {
        if !record.cluster.trim().is_empty() {
            grouped
                .entry(record.cluster.clone())
                .or_default()
                .push(record);
        }
    }
    grouped
        .into_iter()
        .map(|(cluster, records)| {
            let mut known = records
                .iter()
                .map(|record| record.controller_id.to_string())
                .collect::<Vec<_>>();
            known.sort();
            let mut eligible = records
                .iter()
                .filter(|record| {
                    record.phase == ControllerPhase::Online && record.current_session_id.is_some()
                })
                .map(|record| record.controller_id.to_string())
                .collect::<Vec<_>>();
            eligible.sort();
            match eligible.as_slice() {
                [controller_id] => ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Available,
                    controller_id: Some(controller_id.clone()),
                    candidates: eligible,
                },
                [] => ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Offline,
                    controller_id: None,
                    candidates: known,
                },
                _ => ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Ambiguous,
                    controller_id: None,
                    candidates: eligible,
                },
            }
        })
        .collect()
}

pub(crate) async fn resolve_authoritative_targets(
    records: Vec<ControllerRecord>,
    mode: &TargetResolutionMode,
    requested: &HashSet<&str>,
) -> Result<Vec<ResolvedTarget>, CoreError> {
    let mut ids = HashSet::new();
    let mut grouped = BTreeMap::<String, Vec<ControllerRecord>>::new();
    for record in records {
        if !ids.insert(record.controller_id.to_string()) {
            return Err(CoreError::Conflict(format!(
                "duplicate Controller directory entry {}",
                record.controller_id
            )));
        }
        if record.cluster.is_empty() || record.cluster.trim() != record.cluster {
            return Err(CoreError::Conflict(format!(
                "Controller directory entry {} has an invalid cluster identity",
                record.controller_id
            )));
        }
        if !requested.is_empty() && !requested.contains(record.cluster.as_str()) {
            continue;
        }
        grouped
            .entry(record.cluster.clone())
            .or_default()
            .push(record);
    }
    if grouped.len() > DEFAULT_MAX_TARGET_CLUSTERS {
        return Err(CoreError::Conflict(format!(
            "GlobalResources inventory cannot target more than {DEFAULT_MAX_TARGET_CLUSTERS} clusters"
        )));
    }

    let mut output = Vec::with_capacity(grouped.len());
    for (cluster, records) in grouped {
        let mut known = records
            .iter()
            .map(|record| record.controller_id.to_string())
            .collect::<Vec<_>>();
        known.sort();
        let mut eligible = Vec::<(String, String, Option<ControllerOwnerRoute>)>::new();
        let mut indeterminate = Vec::new();
        for record in records {
            if record.phase != ControllerPhase::Online {
                continue;
            }
            let Some(session_id) = record.current_session_id.as_ref() else {
                indeterminate.push(record.controller_id.to_string());
                continue;
            };
            match mode {
                TargetResolutionMode::Standalone(registry) => {
                    if registry
                        .is_dispatchable_session(record.controller_id.as_str(), session_id.as_str())
                    {
                        eligible.push((
                            record.controller_id.to_string(),
                            session_id.to_string(),
                            None,
                        ));
                    } else {
                        indeterminate.push(record.controller_id.to_string());
                    }
                }
                TargetResolutionMode::Kubernetes(locator) => {
                    let Some(expected_holder) = record.connected_replica.as_deref() else {
                        indeterminate.push(record.controller_id.to_string());
                        continue;
                    };
                    let Some(expected_fence) = record.ownership_fence.as_ref() else {
                        indeterminate.push(record.controller_id.to_string());
                        continue;
                    };
                    if expected_fence.token.is_empty() || expected_fence.epoch == 0 {
                        indeterminate.push(record.controller_id.to_string());
                        continue;
                    }
                    match tokio::time::timeout(
                        Duration::from_secs(2),
                        locator.locate(&record.controller_id),
                    )
                    .await
                    {
                        Ok(Ok(Some(route)))
                            if route.holder == expected_holder
                                && route.ownership_fence == *expected_fence =>
                        {
                            eligible.push((
                                record.controller_id.to_string(),
                                session_id.to_string(),
                                Some(route),
                            ));
                        }
                        _ => indeterminate.push(record.controller_id.to_string()),
                    }
                }
            }
        }
        eligible.sort_by(|left, right| left.0.cmp(&right.0));
        indeterminate.sort();
        let target = if !indeterminate.is_empty() {
            let mut candidates = eligible
                .iter()
                .map(|entry| entry.0.clone())
                .chain(indeterminate)
                .collect::<Vec<_>>();
            candidates.sort();
            ResolvedTarget {
                resolution: ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Indeterminate,
                    controller_id: None,
                    candidates,
                },
                session_id: None,
                owner_route: None,
            }
        } else if eligible.len() > 1 {
            ResolvedTarget {
                resolution: ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Ambiguous,
                    controller_id: None,
                    candidates: eligible.iter().map(|entry| entry.0.clone()).collect(),
                },
                session_id: None,
                owner_route: None,
            }
        } else if let Some((controller_id, session_id, owner_route)) = eligible.pop() {
            ResolvedTarget {
                resolution: ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Available,
                    controller_id: Some(controller_id.clone()),
                    candidates: vec![controller_id],
                },
                session_id: Some(session_id),
                owner_route,
            }
        } else {
            ResolvedTarget {
                resolution: ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Offline,
                    controller_id: None,
                    candidates: known,
                },
                session_id: None,
                owner_route: None,
            }
        };
        output.push(target);
    }
    Ok(output)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryNamespaceError {
    pub namespace: String,
    pub code: InventoryErrorCode,
    pub status: Option<u16>,
    pub retryable: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryErrorCode {
    ClusterNotFound,
    ControllerOffline,
    ControllerAmbiguous,
    OwnershipIndeterminate,
    UpstreamUnavailable,
    UpstreamRejected,
    PayloadTooLarge,
    InvalidUpstreamResponse,
    BudgetExceeded,
    PaginationInvalid,
    PaginationLimit,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterInventory {
    pub cluster: String,
    pub state: ClusterResolutionState,
    pub controller_id: Option<String>,
    pub candidates: Vec<String>,
    pub items: Vec<Value>,
    pub errors: Vec<InventoryNamespaceError>,
    pub complete: bool,
}

fn inventory_error(
    namespace: impl Into<String>,
    code: InventoryErrorCode,
    status: Option<u16>,
    retryable: bool,
    message: impl Into<String>,
) -> InventoryNamespaceError {
    InventoryNamespaceError {
        namespace: namespace.into(),
        code,
        status,
        retryable,
        message: message.into(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalResourcesInventory {
    pub catalog_revision: String,
    pub membership_revision: String,
    pub kind: GlobalResourceInventoryKind,
    pub generated_at_unix_ms: i64,
    pub clusters: Vec<ClusterInventory>,
}

#[derive(Debug, Deserialize)]
struct ControllerListEnvelope {
    success: bool,
    #[serde(default)]
    data: Option<Vec<Value>>,
    #[serde(default)]
    #[serde(alias = "continue_token")]
    continue_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

pub struct GlobalResourcesService {
    directory: Arc<dyn ControllerDirectory>,
    client: Arc<dyn ControllerHttpClient>,
    resolution_mode: TargetResolutionMode,
    config: GlobalResourcesConfig,
    fanout: Arc<Semaphore>,
    cache: BoundedTtlCache<String, GlobalResourcesInventory>,
    flights: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
    page_size: usize,
    max_pages_per_namespace: usize,
    max_items_per_cluster: usize,
}

impl GlobalResourcesService {
    pub fn new_standalone(
        directory: Arc<dyn ControllerDirectory>,
        registry: ControllerRegistry,
        client: Arc<dyn ControllerHttpClient>,
        config: GlobalResourcesConfig,
    ) -> Self {
        Self::new(
            directory,
            client,
            config,
            TargetResolutionMode::Standalone(registry),
        )
    }

    pub fn new_kubernetes(
        directory: Arc<dyn ControllerDirectory>,
        owner_locator: Arc<dyn ControllerOwnerLocator>,
        client: Arc<dyn ControllerHttpClient>,
        config: GlobalResourcesConfig,
    ) -> Self {
        Self::new(
            directory,
            client,
            config,
            TargetResolutionMode::Kubernetes(owner_locator),
        )
    }

    fn new(
        directory: Arc<dyn ControllerDirectory>,
        client: Arc<dyn ControllerHttpClient>,
        config: GlobalResourcesConfig,
        resolution_mode: TargetResolutionMode,
    ) -> Self {
        Self {
            directory,
            client,
            resolution_mode,
            config,
            fanout: Arc::new(Semaphore::new(DEFAULT_FANOUT_CONCURRENCY)),
            cache: BoundedTtlCache::new(32, 64 * 1024 * 1024, DEFAULT_CACHE_TTL)
                .expect("static GlobalResources cache limits are valid"),
            flights: Mutex::new(HashMap::new()),
            page_size: DEFAULT_LIST_PAGE_SIZE,
            max_pages_per_namespace: DEFAULT_MAX_PAGES_PER_NAMESPACE,
            max_items_per_cluster: DEFAULT_MAX_ITEMS_PER_CLUSTER,
        }
    }

    pub fn config(&self) -> &GlobalResourcesConfig {
        &self.config
    }

    pub async fn resolve_targets(
        &self,
    ) -> Result<Vec<ClusterResolution>, edgion_center_core::CoreError> {
        let requested = HashSet::new();
        Ok(self
            .resolved_targets(&requested)
            .await?
            .into_iter()
            .map(|target| target.resolution)
            .collect())
    }

    async fn resolved_targets(
        &self,
        requested: &HashSet<&str>,
    ) -> Result<Vec<ResolvedTarget>, CoreError> {
        resolve_authoritative_targets(
            self.directory.list().await?,
            &self.resolution_mode,
            requested,
        )
        .await
    }

    pub async fn preflight(
        &self,
        controller_id: &str,
    ) -> Result<ControllerGlobalResourcesPreflight, PreflightError> {
        preflight_controller(self.client.as_ref(), controller_id, &self.config).await
    }

    pub async fn preflight_cluster(
        &self,
        cluster: &str,
    ) -> Result<ControllerGlobalResourcesPreflight, PreflightError> {
        tokio::time::timeout(
            Duration::from_secs(15),
            self.preflight_cluster_with_deadline(cluster),
        )
        .await
        .map_err(|_| PreflightError {
            status: 504,
            message: "GlobalResources preflight deadline exceeded".to_string(),
        })?
    }

    async fn preflight_cluster_with_deadline(
        &self,
        cluster: &str,
    ) -> Result<ControllerGlobalResourcesPreflight, PreflightError> {
        let requested = HashSet::from([cluster]);
        let mut targets =
            self.resolved_targets(&requested)
                .await
                .map_err(|error| PreflightError {
                    status: 503,
                    message: error.to_string(),
                })?;
        let Some(target) = targets.pop() else {
            return Err(PreflightError {
                status: 404,
                message: "cluster was not found".to_string(),
            });
        };
        if target.resolution.state != ClusterResolutionState::Available {
            return Err(PreflightError {
                status: 409,
                message: "cluster has no authoritative Controller target".to_string(),
            });
        }
        let controller_id = target
            .resolution
            .controller_id
            .as_deref()
            .expect("available resolution has one Controller");
        preflight_resolved_controller(
            self.client.as_ref(),
            controller_id,
            &self.config,
            target.session_id.as_deref(),
            target.owner_route.as_ref(),
        )
        .await
    }

    pub async fn inventory(
        &self,
        kind: GlobalResourceInventoryKind,
        requested_clusters: &[String],
    ) -> Result<Arc<GlobalResourcesInventory>, edgion_center_core::CoreError> {
        tokio::time::timeout(
            Duration::from_secs(15),
            self.inventory_with_deadline(kind, requested_clusters),
        )
        .await
        .map_err(|_| CoreError::Adapter("GlobalResources inventory deadline exceeded".into()))?
    }

    /// Fresh-read one exact resource through the authoritative Controller
    /// session or ownership fence. Detail reads bypass the inventory cache.
    pub async fn get_resource(
        &self,
        kind: GlobalResourceInventoryKind,
        cluster: &str,
        namespace: &str,
        name: &str,
    ) -> Result<ClusterInventory, CoreError> {
        if !self
            .config
            .platform_namespaces
            .iter()
            .any(|configured| configured == namespace)
        {
            return Err(CoreError::Conflict(format!(
                "namespace '{namespace}' is not a configured GlobalResources platform namespace"
            )));
        }
        if !valid_resource_name(name) {
            return Err(CoreError::Conflict(
                "GlobalResources resource name is invalid".to_string(),
            ));
        }
        let requested = HashSet::from([cluster]);
        let mut targets = self.resolved_targets(&requested).await?;
        let target = targets.pop().unwrap_or_else(|| ResolvedTarget {
            resolution: ClusterResolution {
                cluster: cluster.to_string(),
                state: ClusterResolutionState::Offline,
                controller_id: None,
                candidates: Vec::new(),
            },
            session_id: None,
            owner_route: None,
        });
        if target.resolution.state != ClusterResolutionState::Available {
            return Ok(unavailable_cluster_inventory(target));
        }
        let path_kind = GLOBAL_RESOURCE_KINDS
            .iter()
            .find(|entry| entry.kind == kind)
            .expect("inventory kind must exist in the static catalog")
            .path_kind;
        let controller_id = target
            .resolution
            .controller_id
            .clone()
            .expect("available resolution has one Controller");
        let path = format!("/api/v1/namespaced/{path_kind}/{namespace}/{name}");
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            request_target(
                self.client.as_ref(),
                &controller_id,
                target.session_id.as_deref(),
                target.owner_route.as_ref(),
                path,
            ),
        )
        .await
        .map_err(|_| CoreError::Adapter("GlobalResources detail deadline exceeded".into()))?;
        let mut result = ClusterInventory {
            cluster: target.resolution.cluster,
            state: target.resolution.state,
            controller_id: Some(controller_id),
            candidates: target.resolution.candidates,
            items: Vec::new(),
            errors: Vec::new(),
            complete: true,
        };
        match response {
            Err(message) => {
                result.errors.push(inventory_error(
                    namespace,
                    InventoryErrorCode::UpstreamUnavailable,
                    None,
                    true,
                    message,
                ));
                result.complete = false;
            }
            Ok(response) if !(200..300).contains(&response.status_code) => {
                result.errors.push(inventory_error(
                    namespace,
                    InventoryErrorCode::UpstreamRejected,
                    u16::try_from(response.status_code).ok(),
                    response.status_code >= 500,
                    format!("Controller get returned status {}", response.status_code),
                ));
                result.complete = false;
            }
            Ok(response) if response.body.len() > DEFAULT_MAX_UPSTREAM_BODY_BYTES => {
                result.errors.push(inventory_error(
                    namespace,
                    InventoryErrorCode::PayloadTooLarge,
                    Some(413),
                    false,
                    "Controller get response exceeded 2 MiB",
                ));
                result.complete = false;
            }
            Ok(response) => match serde_json::from_slice::<Value>(&response.body) {
                Ok(item)
                    if valid_inventory_item(&item, kind.as_str(), namespace)
                        && item.pointer("/metadata/name").and_then(Value::as_str) == Some(name) =>
                {
                    result.items.push(item);
                }
                Ok(_) => {
                    result.errors.push(inventory_error(
                        namespace,
                        InventoryErrorCode::InvalidUpstreamResponse,
                        Some(502),
                        false,
                        "Controller returned an invalid or mismatched resource envelope",
                    ));
                    result.complete = false;
                }
                Err(error) => {
                    result.errors.push(inventory_error(
                        namespace,
                        InventoryErrorCode::InvalidUpstreamResponse,
                        Some(502),
                        false,
                        format!("invalid Controller get response: {error}"),
                    ));
                    result.complete = false;
                }
            },
        }
        Ok(result)
    }

    async fn inventory_with_deadline(
        &self,
        kind: GlobalResourceInventoryKind,
        requested_clusters: &[String],
    ) -> Result<Arc<GlobalResourcesInventory>, CoreError> {
        let mut cluster_key = requested_clusters.to_vec();
        cluster_key.sort();
        cluster_key.dedup();
        let requested: HashSet<&str> = requested_clusters.iter().map(String::as_str).collect();
        if requested.len() > DEFAULT_MAX_TARGET_CLUSTERS {
            return Err(CoreError::Conflict(format!(
                "GlobalResources inventory cannot target more than {DEFAULT_MAX_TARGET_CLUSTERS} clusters"
            )));
        }
        let mut resolutions = self
            .resolved_targets(&requested)
            .await?
            .into_iter()
            .filter(|entry| {
                requested.is_empty() || requested.contains(entry.resolution.cluster.as_str())
            })
            .collect::<Vec<_>>();
        if !requested.is_empty() {
            let resolved: HashSet<String> = resolutions
                .iter()
                .map(|entry| entry.resolution.cluster.clone())
                .collect();
            let mut missing = requested
                .iter()
                .filter(|cluster| !resolved.contains(**cluster))
                .map(|cluster| (*cluster).to_string())
                .collect::<Vec<_>>();
            missing.sort();
            resolutions.extend(missing.into_iter().map(|cluster| ResolvedTarget {
                resolution: ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Offline,
                    controller_id: None,
                    candidates: Vec::new(),
                },
                session_id: None,
                owner_route: None,
            }));
        }
        if resolutions.len() > DEFAULT_MAX_TARGET_CLUSTERS {
            return Err(CoreError::Conflict(format!(
                "GlobalResources inventory cannot target more than {DEFAULT_MAX_TARGET_CLUSTERS} clusters"
            )));
        }
        let membership = resolutions
            .iter()
            .map(|target| {
                (
                    target.resolution.cluster.as_str(),
                    &target.resolution.state,
                    target.resolution.controller_id.as_deref(),
                    target.session_id.as_deref(),
                    target
                        .owner_route
                        .as_ref()
                        .map(|route| route.holder.as_str()),
                    target
                        .owner_route
                        .as_ref()
                        .map(|route| route.ownership_fence.token.as_str()),
                    target
                        .owner_route
                        .as_ref()
                        .map(|route| route.ownership_fence.epoch),
                )
            })
            .collect::<Vec<_>>();
        let membership_revision = stable_revision(&membership);
        let cache_key = serde_json::to_string(&(
            self.config.revision(),
            kind,
            cluster_key,
            &membership_revision,
        ))
        .expect("inventory cache key is serializable");
        if let Some(hit) = self.cache.get(&cache_key) {
            return Ok(hit);
        }
        let flight = {
            let mut flights = self.flights.lock();
            flights.retain(|_, flight| flight.strong_count() > 0);
            if let Some(flight) = flights.get(&cache_key).and_then(Weak::upgrade) {
                flight
            } else {
                if flights.len() >= DEFAULT_MAX_SINGLE_FLIGHTS {
                    return Err(CoreError::Adapter(
                        "GlobalResources inventory concurrency limit reached".into(),
                    ));
                }
                let flight = Arc::new(tokio::sync::Mutex::new(()));
                flights.insert(cache_key.clone(), Arc::downgrade(&flight));
                flight
            }
        };
        let flight_guard = flight.lock().await;
        if let Some(hit) = self.cache.get(&cache_key) {
            return Ok(hit);
        }
        let path_kind = GLOBAL_RESOURCE_KINDS
            .iter()
            .find(|entry| entry.kind == kind)
            .expect("inventory kind must exist in the static catalog")
            .path_kind;
        let expected_kind = kind.as_str();
        let budget = InventoryBudget::new(InventoryBudgetLimits {
            max_items: 20_000,
            max_retained_bytes: 32 * 1024 * 1024,
            max_upstream_pages: 2_000,
        })
        .expect("static GlobalResources inventory budget is valid");
        let task_count = resolutions.len();
        let mut tasks = FuturesUnordered::new();
        for target in resolutions {
            let client = self.client.clone();
            let fanout = self.fanout.clone();
            let namespaces = self.config.platform_namespaces.clone();
            let page_size = self.page_size;
            let max_pages = self.max_pages_per_namespace;
            let max_items = self.max_items_per_cluster;
            let budget = budget.clone();
            tasks.push(async move {
                if target.resolution.state != ClusterResolutionState::Available {
                    return unavailable_cluster_inventory(target);
                }
                let _permit = fanout
                    .acquire_owned()
                    .await
                    .expect("GlobalResources fan-out semaphore closed");
                list_cluster_inventory(
                    client,
                    target,
                    expected_kind,
                    path_kind,
                    namespaces,
                    InventoryScanLimits {
                        page_size,
                        max_pages_per_namespace: max_pages,
                        max_items_per_cluster: max_items,
                    },
                    budget,
                )
                .await
            });
        }

        let mut clusters = Vec::with_capacity(task_count);
        while let Some(cluster) = tasks.next().await {
            clusters.push(cluster);
        }
        clusters.sort_by(|left, right| left.cluster.cmp(&right.cluster));
        let value = Arc::new(GlobalResourcesInventory {
            catalog_revision: self.config.revision(),
            membership_revision,
            kind,
            generated_at_unix_ms: unix_time_ms(),
            clusters,
        });
        let estimated_bytes = serde_json::to_vec(value.as_ref())
            .map(|bytes| bytes.len())
            .unwrap_or(64 * 1024 * 1024 + 1);
        self.cache
            .insert(cache_key.clone(), value.clone(), estimated_bytes);
        drop(flight_guard);
        Ok(value)
    }
}

async fn list_cluster_inventory(
    client: Arc<dyn ControllerHttpClient>,
    target: ResolvedTarget,
    expected_kind: &'static str,
    path_kind: &'static str,
    namespaces: Vec<String>,
    limits: InventoryScanLimits,
    budget: InventoryBudget,
) -> ClusterInventory {
    let session_id = target.session_id.clone();
    let owner_route = target.owner_route.clone();
    let controller_id = target
        .resolution
        .controller_id
        .clone()
        .expect("available resolution has one Controller");
    let mut result = ClusterInventory {
        cluster: target.resolution.cluster,
        state: target.resolution.state,
        controller_id: Some(controller_id.clone()),
        candidates: target.resolution.candidates,
        items: Vec::new(),
        errors: Vec::new(),
        complete: true,
    };
    for namespace in namespaces {
        let mut namespace_items = Vec::new();
        let mut namespace_reservations = Vec::new();
        let mut namespace_failed = false;
        let mut namespace_complete = false;
        let mut continuation: Option<String> = None;
        let mut seen_tokens = HashSet::new();
        for _ in 0..limits.max_pages_per_namespace {
            if let Err(error) = budget.try_consume_page() {
                result.errors.push(inventory_error(
                    namespace.clone(),
                    InventoryErrorCode::BudgetExceeded,
                    Some(413),
                    false,
                    error.to_string(),
                ));
                namespace_failed = true;
                break;
            }
            let mut path = format!(
                "/api/v1/namespaced/{path_kind}/{namespace}?limit={}",
                limits.page_size
            );
            if let Some(token) = continuation.as_deref() {
                let encoded: String =
                    url::form_urlencoded::byte_serialize(token.as_bytes()).collect();
                path.push_str("&continue=");
                path.push_str(&encoded);
            }
            let response = match request_target(
                client.as_ref(),
                &controller_id,
                session_id.as_deref(),
                owner_route.as_ref(),
                path,
            )
            .await
            {
                Ok(response) => response,
                Err(message) => {
                    result.errors.push(inventory_error(
                        namespace.clone(),
                        InventoryErrorCode::UpstreamUnavailable,
                        None,
                        true,
                        message,
                    ));
                    namespace_failed = true;
                    break;
                }
            };
            if !(200..300).contains(&response.status_code) {
                result.errors.push(inventory_error(
                    namespace.clone(),
                    InventoryErrorCode::UpstreamRejected,
                    u16::try_from(response.status_code).ok(),
                    response.status_code >= 500,
                    format!("Controller list returned status {}", response.status_code),
                ));
                namespace_failed = true;
                break;
            }
            if response.body.len() > DEFAULT_MAX_UPSTREAM_BODY_BYTES {
                result.errors.push(inventory_error(
                    namespace.clone(),
                    InventoryErrorCode::PayloadTooLarge,
                    Some(413),
                    false,
                    "Controller list response exceeded 2 MiB",
                ));
                namespace_failed = true;
                break;
            }
            let envelope: ControllerListEnvelope = match serde_json::from_slice(&response.body) {
                Ok(value) => value,
                Err(error) => {
                    result.errors.push(inventory_error(
                        namespace.clone(),
                        InventoryErrorCode::InvalidUpstreamResponse,
                        Some(502),
                        false,
                        format!("invalid Controller list response: {error}"),
                    ));
                    namespace_failed = true;
                    break;
                }
            };
            if !envelope.success {
                result.errors.push(inventory_error(
                    namespace.clone(),
                    InventoryErrorCode::InvalidUpstreamResponse,
                    Some(502),
                    false,
                    envelope
                        .error
                        .unwrap_or_else(|| "Controller list reported failure".to_string()),
                ));
                namespace_failed = true;
                break;
            }
            let Some(page_items) = envelope.data else {
                result.errors.push(inventory_error(
                    namespace.clone(),
                    InventoryErrorCode::InvalidUpstreamResponse,
                    Some(502),
                    false,
                    "Controller list response omitted data",
                ));
                namespace_failed = true;
                break;
            };
            let reservation =
                match budget.try_reserve_payload(page_items.len(), response.body.len()) {
                    Ok(reservation) => reservation,
                    Err(error) => {
                        result.errors.push(inventory_error(
                            namespace.clone(),
                            InventoryErrorCode::BudgetExceeded,
                            Some(413),
                            false,
                            error.to_string(),
                        ));
                        namespace_failed = true;
                        break;
                    }
                };
            if page_items
                .iter()
                .any(|item| !valid_inventory_item(item, expected_kind, &namespace))
            {
                result.errors.push(inventory_error(
                    namespace.clone(),
                    InventoryErrorCode::InvalidUpstreamResponse,
                    Some(502),
                    false,
                    "Controller returned an invalid resource envelope",
                ));
                namespace_failed = true;
                break;
            }
            namespace_items.extend(page_items);
            namespace_reservations.push(reservation);
            if result.items.len() + namespace_items.len() > limits.max_items_per_cluster {
                result.errors.push(inventory_error(
                    namespace.clone(),
                    InventoryErrorCode::BudgetExceeded,
                    Some(413),
                    false,
                    format!(
                        "cluster inventory exceeded {} items",
                        limits.max_items_per_cluster
                    ),
                ));
                namespace_failed = true;
                break;
            }
            // Older/current Controller APIs encode "no next page" as an empty
            // string, while the Center contract uses null. Treat both forms
            // as the end of the namespace scan.
            continuation = envelope.continue_token.filter(|token| !token.is_empty());
            let Some(token) = continuation.as_ref() else {
                namespace_complete = true;
                break;
            };
            if token.is_empty() || token.len() > 4096 {
                result.errors.push(inventory_error(
                    namespace.clone(),
                    InventoryErrorCode::PaginationInvalid,
                    Some(502),
                    false,
                    "Controller returned an invalid continuation token",
                ));
                namespace_failed = true;
                break;
            }
            if !seen_tokens.insert(token.clone()) {
                result.errors.push(inventory_error(
                    namespace.clone(),
                    InventoryErrorCode::PaginationInvalid,
                    Some(502),
                    false,
                    "Controller repeated a continuation token",
                ));
                namespace_failed = true;
                break;
            }
        }
        if !namespace_failed && !namespace_complete {
            result.errors.push(inventory_error(
                namespace.clone(),
                InventoryErrorCode::PaginationLimit,
                Some(413),
                false,
                format!(
                    "namespace pagination exceeded {} pages",
                    limits.max_pages_per_namespace
                ),
            ));
            namespace_failed = true;
        }
        if !namespace_failed {
            result.items.extend(namespace_items);
            for reservation in namespace_reservations {
                reservation.commit();
            }
        } else {
            result.complete = false;
        }
    }
    result
}

fn valid_inventory_item(item: &Value, expected_kind: &str, namespace: &str) -> bool {
    item.as_object().is_some()
        && item.get("kind").and_then(Value::as_str) == Some(expected_kind)
        && item
            .pointer("/metadata/name")
            .and_then(Value::as_str)
            .is_some_and(valid_resource_name)
        && item.pointer("/metadata/namespace").and_then(Value::as_str) == Some(namespace)
}

fn valid_resource_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
}

fn unavailable_cluster_inventory(target: ResolvedTarget) -> ClusterInventory {
    let (code, retryable) = match target.resolution.state {
        ClusterResolutionState::Offline if target.resolution.candidates.is_empty() => {
            (InventoryErrorCode::ClusterNotFound, false)
        }
        ClusterResolutionState::Offline => (InventoryErrorCode::ControllerOffline, true),
        ClusterResolutionState::Ambiguous => (InventoryErrorCode::ControllerAmbiguous, false),
        ClusterResolutionState::Indeterminate => (InventoryErrorCode::OwnershipIndeterminate, true),
        ClusterResolutionState::Available => unreachable!(),
    };
    ClusterInventory {
        cluster: target.resolution.cluster,
        state: target.resolution.state,
        controller_id: None,
        candidates: target.resolution.candidates,
        items: Vec::new(),
        errors: vec![inventory_error(
            "",
            code,
            None,
            retryable,
            "cluster has no authoritative Controller target",
        )],
        complete: false,
    }
}

pub(crate) async fn request_target(
    client: &dyn ControllerHttpClient,
    controller_id: &str,
    session_id: Option<&str>,
    owner_route: Option<&ControllerOwnerRoute>,
    path: String,
) -> Result<crate::poll::ControllerHttpResponse, String> {
    if let Some(route) = owner_route {
        return client
            .request_fenced(
                controller_id,
                "GET".to_string(),
                path,
                HashMap::new(),
                Vec::new(),
                route,
            )
            .await;
    }
    client
        .request_session_fenced(
            controller_id,
            "GET".to_string(),
            path,
            HashMap::new(),
            Vec::new(),
            session_id.ok_or_else(|| "resolved target has no session fence".to_string())?,
        )
        .await
}

fn unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(crate) fn stable_revision(value: &impl Serialize) -> String {
    let canonical = serde_json::to_vec(value).expect("revision input is serializable");
    let digest = Sha256::digest(canonical);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccessEnvelope {
    success: bool,
    data: ControllerAccessSnapshot,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerAccessSnapshot {
    pub schema_version: u8,
    pub revision: String,
    pub resources: Vec<ResourceAccess>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResourceAccess {
    pub kind: String,
    pub scope: String,
    pub verbs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NamespacePreflightState {
    Available,
    AccessDenied,
    BackendRejected,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamespacePreflight {
    pub namespace: String,
    pub kind: String,
    pub state: NamespacePreflightState,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindPreflight {
    pub kind: String,
    pub can_get: bool,
    pub can_list: bool,
    pub can_create: bool,
    pub can_update: bool,
    pub mutation_available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerGlobalResourcesPreflight {
    pub controller_id: String,
    pub access_revision: String,
    pub catalog_revision: String,
    pub inventory_available: bool,
    pub sync_available: bool,
    pub kinds: Vec<KindPreflight>,
    pub namespaces: Vec<NamespacePreflight>,
}

#[derive(Debug)]
pub struct PreflightError {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for PreflightError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} (status {})", self.message, self.status)
    }
}

impl std::error::Error for PreflightError {}

pub async fn preflight_controller(
    client: &dyn ControllerHttpClient,
    controller_id: &str,
    config: &GlobalResourcesConfig,
) -> Result<ControllerGlobalResourcesPreflight, PreflightError> {
    preflight_impl(client, controller_id, config, None, None, false).await
}

async fn preflight_resolved_controller(
    client: &dyn ControllerHttpClient,
    controller_id: &str,
    config: &GlobalResourcesConfig,
    session_id: Option<&str>,
    owner_route: Option<&ControllerOwnerRoute>,
) -> Result<ControllerGlobalResourcesPreflight, PreflightError> {
    preflight_impl(client, controller_id, config, session_id, owner_route, true).await
}

async fn preflight_impl(
    client: &dyn ControllerHttpClient,
    controller_id: &str,
    config: &GlobalResourcesConfig,
    session_id: Option<&str>,
    owner_route: Option<&ControllerOwnerRoute>,
    fenced: bool,
) -> Result<ControllerGlobalResourcesPreflight, PreflightError> {
    let access_response = preflight_request(
        client,
        controller_id,
        "/api/v1/access".to_string(),
        session_id,
        owner_route,
        fenced,
    )
    .await
    .map_err(|message| PreflightError {
        status: 503,
        message,
    })?;
    if !(200..300).contains(&access_response.status_code) {
        return Err(PreflightError {
            status: u16::try_from(access_response.status_code).unwrap_or(502),
            message: "Controller access introspection failed".to_string(),
        });
    }
    if access_response.body.len() > DEFAULT_MAX_UPSTREAM_BODY_BYTES {
        return Err(PreflightError {
            status: 413,
            message: "Controller access snapshot exceeded 2 MiB".to_string(),
        });
    }
    let envelope: AccessEnvelope =
        serde_json::from_slice(&access_response.body).map_err(|error| PreflightError {
            status: 502,
            message: format!("Controller returned an invalid access snapshot: {error}"),
        })?;
    if !envelope.success {
        return Err(invalid_access_contract(
            "response success flag is false".to_string(),
        ));
    }
    if envelope.data.schema_version != 1 {
        return Err(PreflightError {
            status: 502,
            message: format!(
                "unsupported Controller access schema version {}",
                envelope.data.schema_version
            ),
        });
    }
    if !is_sha256_revision(&envelope.data.revision) {
        return Err(invalid_access_contract(
            "revision must be sha256 followed by 64 lowercase hex characters".to_string(),
        ));
    }

    let kinds = evaluate_kind_access(&envelope.data)?;
    let mut namespaces =
        Vec::with_capacity(config.platform_namespaces.len() * GLOBAL_RESOURCE_KINDS.len());
    for namespace in &config.platform_namespaces {
        for (kind, catalog) in kinds.iter().zip(GLOBAL_RESOURCE_KINDS.iter()) {
            if !kind.can_list {
                namespaces.push(NamespacePreflight {
                    namespace: namespace.clone(),
                    kind: kind.kind.clone(),
                    state: NamespacePreflightState::AccessDenied,
                    detail: Some("Controller access snapshot denies list".to_string()),
                });
                continue;
            }
            let path = format!(
                "/api/v1/namespaced/{}/{namespace}?limit=1",
                catalog.path_kind
            );
            match preflight_request(client, controller_id, path, session_id, owner_route, fenced)
                .await
            {
                Ok(response) if response.body.len() > DEFAULT_MAX_UPSTREAM_BODY_BYTES => namespaces
                    .push(NamespacePreflight {
                        namespace: namespace.clone(),
                        kind: kind.kind.clone(),
                        state: NamespacePreflightState::Unavailable,
                        detail: Some("namespace/kind probe response exceeded 2 MiB".to_string()),
                    }),
                Ok(response) if (200..300).contains(&response.status_code) => {
                    namespaces.push(NamespacePreflight {
                        namespace: namespace.clone(),
                        kind: kind.kind.clone(),
                        state: NamespacePreflightState::Available,
                        detail: None,
                    })
                }
                Ok(response) if response.status_code == 401 || response.status_code == 403 => {
                    namespaces.push(NamespacePreflight {
                        namespace: namespace.clone(),
                        kind: kind.kind.clone(),
                        state: NamespacePreflightState::BackendRejected,
                        detail: Some(
                            "Controller backend rejected the namespace/kind probe".to_string(),
                        ),
                    });
                }
                Ok(response) => namespaces.push(NamespacePreflight {
                    namespace: namespace.clone(),
                    kind: kind.kind.clone(),
                    state: NamespacePreflightState::Unavailable,
                    detail: Some(format!(
                        "namespace/kind probe returned status {}",
                        response.status_code
                    )),
                }),
                Err(message) => namespaces.push(NamespacePreflight {
                    namespace: namespace.clone(),
                    kind: kind.kind.clone(),
                    state: NamespacePreflightState::Unavailable,
                    detail: Some(format!("namespace/kind probe failed: {message}")),
                }),
            }
        }
    }

    let all_namespaces_available = namespaces
        .iter()
        .all(|entry| entry.state == NamespacePreflightState::Available);
    let inventory_available =
        all_namespaces_available && kinds.iter().all(|kind| kind.can_get && kind.can_list);
    let sync_available =
        all_namespaces_available && kinds.iter().any(|kind| kind.mutation_available);

    Ok(ControllerGlobalResourcesPreflight {
        controller_id: controller_id.to_string(),
        access_revision: envelope.data.revision,
        catalog_revision: config.revision(),
        inventory_available,
        sync_available,
        kinds,
        namespaces,
    })
}

async fn preflight_request(
    client: &dyn ControllerHttpClient,
    controller_id: &str,
    path: String,
    session_id: Option<&str>,
    owner_route: Option<&ControllerOwnerRoute>,
    fenced: bool,
) -> Result<crate::poll::ControllerHttpResponse, String> {
    if fenced {
        request_target(client, controller_id, session_id, owner_route, path).await
    } else {
        client
            .request(
                controller_id,
                "GET".to_string(),
                path,
                HashMap::new(),
                Vec::new(),
            )
            .await
    }
}

fn evaluate_kind_access(
    snapshot: &ControllerAccessSnapshot,
) -> Result<Vec<KindPreflight>, PreflightError> {
    let mut by_kind = HashMap::<&str, HashSet<&str>>::new();
    for entry in &snapshot.resources {
        if entry.scope != "namespaced"
            && GLOBAL_RESOURCE_KINDS
                .iter()
                .any(|catalog| catalog.kind.as_str() == entry.kind)
        {
            return Err(invalid_access_contract(format!(
                "{} has unexpected scope {}",
                entry.kind, entry.scope
            )));
        }
        if by_kind
            .insert(
                entry.kind.as_str(),
                entry.verbs.iter().map(String::as_str).collect(),
            )
            .is_some()
        {
            return Err(invalid_access_contract(format!(
                "duplicate kind {}",
                entry.kind
            )));
        }
    }
    GLOBAL_RESOURCE_KINDS
        .iter()
        .map(|entry| {
            let verbs = by_kind.get(entry.kind.as_str()).ok_or_else(|| {
                invalid_access_contract(format!("missing kind {}", entry.kind.as_str()))
            })?;
            let allows = |verb| verbs.contains(verb);
            let can_get = allows("get");
            let can_create = allows("create");
            let can_update = allows("update");
            Ok(KindPreflight {
                kind: entry.kind.as_str().to_string(),
                can_get,
                can_list: allows("list"),
                can_create,
                can_update,
                mutation_available: can_get && can_create && can_update,
            })
        })
        .collect()
}

fn invalid_access_contract(message: String) -> PreflightError {
    PreflightError {
        status: 502,
        message: format!("invalid Controller access snapshot: {message}"),
    }
}

fn is_sha256_revision(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::federation::proto::RegisterRequest;
    use crate::poll::ControllerHttpResponse;
    use edgion_center_core::{ControllerId, OwnershipFence, SessionId};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::mpsc;

    struct FakeHttpClient {
        responses: Mutex<VecDeque<Result<ControllerHttpResponse, String>>>,
        calls: Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl ControllerHttpClient for FakeHttpClient {
        async fn request(
            &self,
            _controller_id: &str,
            method: String,
            path: String,
            _headers: HashMap<String, String>,
            _body: Vec<u8>,
        ) -> Result<ControllerHttpResponse, String> {
            self.calls.lock().push((method, path));
            self.responses.lock().pop_front().expect("fake response")
        }

        async fn request_session_fenced(
            &self,
            _controller_id: &str,
            method: String,
            path: String,
            _headers: HashMap<String, String>,
            _body: Vec<u8>,
            _expected_session_id: &str,
        ) -> Result<ControllerHttpResponse, String> {
            self.calls.lock().push((method, path));
            self.responses.lock().pop_front().expect("fake response")
        }
    }

    fn response(status_code: u32, body: impl Into<Vec<u8>>) -> ControllerHttpResponse {
        ControllerHttpResponse {
            status_code,
            body: body.into(),
        }
    }

    fn full_access_body() -> Vec<u8> {
        let resources = GLOBAL_RESOURCE_KINDS
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "kind": entry.kind.as_str(),
                    "scope": "namespaced",
                    "verbs": ["get", "list", "create", "update"]
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_vec(&serde_json::json!({
            "success": true,
            "data": {
                "schemaVersion": 1,
                "revision": format!("sha256:{}", "0".repeat(64)),
                "resources": resources
            }
        }))
        .unwrap()
    }

    fn record(id: &str, cluster: &str, phase: ControllerPhase) -> ControllerRecord {
        ControllerRecord {
            controller_id: ControllerId::new(id).unwrap(),
            current_session_id: (phase == ControllerPhase::Online)
                .then(|| SessionId::new(format!("session-{id}")).unwrap()),
            cluster: cluster.into(),
            environments: Vec::new(),
            tags: Vec::new(),
            connected_replica: None,
            ownership_fence: None,
            sync_version: None,
            watch_server_id: None,
            resource_count: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: None,
            phase,
            last_seen_unix_ms: 1,
        }
    }

    fn register_request(id: &str, cluster: &str) -> RegisterRequest {
        RegisterRequest {
            controller_id: id.to_string(),
            cluster: cluster.to_string(),
            env: Vec::new(),
            tag: Vec::new(),
            supported_kinds: Vec::new(),
        }
    }

    struct FixedOwnerLocator {
        route: Option<ControllerOwnerRoute>,
    }

    struct DropSignal(Arc<AtomicBool>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait::async_trait]
    impl ControllerOwnerLocator for FixedOwnerLocator {
        async fn locate(
            &self,
            _id: &ControllerId,
        ) -> Result<Option<ControllerOwnerRoute>, CoreError> {
            Ok(self.route.clone())
        }
    }

    #[test]
    fn cluster_resolution_requires_exactly_one_online_session() {
        let resolutions = resolve_cluster_targets(vec![
            record("a-1", "a", ControllerPhase::Online),
            record("b-1", "b", ControllerPhase::Online),
            record("b-2", "b", ControllerPhase::Online),
            record("c-1", "c", ControllerPhase::Offline),
            record("stale", "", ControllerPhase::Online),
        ]);
        assert_eq!(resolutions.len(), 3);
        assert_eq!(resolutions[0].state, ClusterResolutionState::Available);
        assert_eq!(resolutions[0].controller_id.as_deref(), Some("a-1"));
        assert_eq!(resolutions[1].state, ClusterResolutionState::Ambiguous);
        assert_eq!(resolutions[1].candidates, ["b-1", "b-2"]);
        assert_eq!(resolutions[2].state, ClusterResolutionState::Offline);
        assert_eq!(resolutions[2].candidates, ["c-1"]);
    }

    #[tokio::test]
    async fn standalone_resolution_rejects_a_stale_directory_session() {
        let registry = ControllerRegistry::new();
        let (tx, _rx) = mpsc::channel(1);
        registry.register(
            "controller-a".into(),
            register_request("controller-a", "cluster-a"),
            tx,
            "different-session".into(),
        );

        let targets = resolve_authoritative_targets(
            vec![record("controller-a", "cluster-a", ControllerPhase::Online)],
            &TargetResolutionMode::Standalone(registry),
            &HashSet::new(),
        )
        .await
        .unwrap();

        assert_eq!(
            targets[0].resolution.state,
            ClusterResolutionState::Indeterminate
        );
        assert!(targets[0].session_id.is_none());
    }

    #[tokio::test]
    async fn kubernetes_resolution_rejects_an_owner_fence_mismatch() {
        let mut directory_record = record("controller-a", "cluster-a", ControllerPhase::Online);
        directory_record.connected_replica = Some("center-a".into());
        directory_record.ownership_fence = Some(OwnershipFence {
            token: "expected".into(),
            epoch: 7,
        });
        let locator = Arc::new(FixedOwnerLocator {
            route: Some(ControllerOwnerRoute {
                holder: "center-a".into(),
                endpoint: "http://center-a".into(),
                ownership_fence: OwnershipFence {
                    token: "stale".into(),
                    epoch: 6,
                },
            }),
        });

        let targets = resolve_authoritative_targets(
            vec![directory_record],
            &TargetResolutionMode::Kubernetes(locator),
            &HashSet::new(),
        )
        .await
        .unwrap();

        assert_eq!(
            targets[0].resolution.state,
            ClusterResolutionState::Indeterminate
        );
        assert!(targets[0].owner_route.is_none());
    }

    #[test]
    fn weak_flight_entries_keep_waiters_together_without_owning_the_flight() {
        let flight = Arc::new(tokio::sync::Mutex::new(()));
        let weak = Arc::downgrade(&flight);
        let waiter = weak.upgrade().expect("waiter sees the active flight");

        drop(flight);
        let later = weak
            .upgrade()
            .expect("a later request joins the waiter's existing flight");
        assert!(Arc::ptr_eq(&waiter, &later));

        drop(waiter);
        drop(later);
        assert!(weak.upgrade().is_none());
    }

    #[tokio::test]
    async fn dropping_inventory_work_drops_all_in_flight_children() {
        let dropped = Arc::new(AtomicBool::new(false));
        let parent = {
            let dropped = dropped.clone();
            async move {
                let mut tasks = FuturesUnordered::new();
                tasks.push(async move {
                    let _signal = DropSignal(dropped);
                    std::future::pending::<()>().await;
                });
                tasks.next().await
            }
        };

        assert!(tokio::time::timeout(Duration::from_millis(1), parent)
            .await
            .is_err());
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn write_exposure_requires_both_create_and_update() {
        let snapshot = ControllerAccessSnapshot {
            schema_version: 1,
            revision: "sha256:test".into(),
            resources: vec![
                ResourceAccess {
                    kind: "HTTPRoute".into(),
                    scope: "namespaced".into(),
                    verbs: vec!["get".into(), "list".into(), "create".into()],
                },
                ResourceAccess {
                    kind: "EdgionConfigData".into(),
                    scope: "namespaced".into(),
                    verbs: vec![
                        "get".into(),
                        "list".into(),
                        "create".into(),
                        "update".into(),
                    ],
                },
                ResourceAccess {
                    kind: "GRPCRoute".into(),
                    scope: "namespaced".into(),
                    verbs: vec![],
                },
                ResourceAccess {
                    kind: "EdgionPlugins".into(),
                    scope: "namespaced".into(),
                    verbs: vec![],
                },
                ResourceAccess {
                    kind: "ReferenceGrant".into(),
                    scope: "namespaced".into(),
                    verbs: vec![],
                },
            ],
        };
        let kinds = evaluate_kind_access(&snapshot).unwrap();
        let http = kinds
            .iter()
            .find(|entry| entry.kind == "HTTPRoute")
            .unwrap();
        assert!(http.can_list && http.can_create && !http.mutation_available);
        let config_data = kinds
            .iter()
            .find(|entry| entry.kind == "EdgionConfigData")
            .unwrap();
        assert!(config_data.mutation_available);
        assert!(
            !kinds
                .iter()
                .find(|entry| entry.kind == "ReferenceGrant")
                .unwrap()
                .can_list
        );
    }

    #[test]
    fn malformed_access_contract_fails_closed() {
        let missing = ControllerAccessSnapshot {
            schema_version: 1,
            revision: "sha256:test".into(),
            resources: vec![],
        };
        assert_eq!(evaluate_kind_access(&missing).unwrap_err().status, 502);

        let duplicate = ControllerAccessSnapshot {
            schema_version: 1,
            revision: "sha256:test".into(),
            resources: vec![
                ResourceAccess {
                    kind: "HTTPRoute".into(),
                    scope: "namespaced".into(),
                    verbs: vec![],
                },
                ResourceAccess {
                    kind: "HTTPRoute".into(),
                    scope: "namespaced".into(),
                    verbs: vec![],
                },
            ],
        };
        assert!(evaluate_kind_access(&duplicate).is_err());
    }

    #[tokio::test]
    async fn preflight_is_read_only_and_probes_each_namespace_kind() {
        let mut responses = VecDeque::from([Ok(response(200, full_access_body()))]);
        for index in 0..10 {
            responses.push_back(Ok(response(
                if index == 9 { 403 } else { 200 },
                b"{}".to_vec(),
            )));
        }
        let client = FakeHttpClient {
            responses: Mutex::new(responses),
            calls: Mutex::new(Vec::new()),
        };
        let result =
            preflight_controller(&client, "controller-a", &GlobalResourcesConfig::default())
                .await
                .unwrap();
        assert_eq!(result.namespaces.len(), 10);
        assert!(!result.inventory_available);
        assert_eq!(
            result.namespaces.last().unwrap().state,
            NamespacePreflightState::BackendRejected
        );
        let calls = client.calls.lock();
        assert_eq!(calls.len(), 11);
        assert!(calls.iter().all(|(method, _)| method == "GET"));
        assert!(calls
            .iter()
            .skip(1)
            .all(|(_, path)| path.contains("?limit=1")));
    }

    #[tokio::test]
    async fn false_success_and_invalid_revision_fail_before_namespace_probes() {
        for body in [
            serde_json::json!({
                "success": false,
                "data": {
                    "schemaVersion": 1,
                    "revision": format!("sha256:{}", "0".repeat(64)),
                    "resources": []
                }
            }),
            serde_json::json!({
                "success": true,
                "data": {
                    "schemaVersion": 1,
                    "revision": "sha256:not-a-digest",
                    "resources": []
                }
            }),
        ] {
            let client = FakeHttpClient {
                responses: Mutex::new(VecDeque::from([Ok(response(
                    200,
                    serde_json::to_vec(&body).unwrap(),
                ))])),
                calls: Mutex::new(Vec::new()),
            };
            assert!(preflight_controller(
                &client,
                "controller-a",
                &GlobalResourcesConfig::default()
            )
            .await
            .is_err());
            assert_eq!(client.calls.lock().len(), 1);
        }
    }

    #[tokio::test]
    async fn preflight_rejects_an_oversized_access_snapshot_before_parsing() {
        let client = FakeHttpClient {
            responses: Mutex::new(VecDeque::from([Ok(response(
                200,
                vec![b' '; DEFAULT_MAX_UPSTREAM_BODY_BYTES + 1],
            ))])),
            calls: Mutex::new(Vec::new()),
        };
        let error =
            preflight_controller(&client, "controller-a", &GlobalResourcesConfig::default())
                .await
                .unwrap_err();
        assert_eq!(error.status, 413);
        assert_eq!(client.calls.lock().len(), 1);
    }

    #[test]
    fn membership_revision_changes_with_session_and_owner_fence() {
        let first = stable_revision(&[(
            "cluster-a",
            "controller-a",
            "session-a",
            "holder-a",
            "token-a",
            1_u64,
        )]);
        let new_session = stable_revision(&[(
            "cluster-a",
            "controller-a",
            "session-b",
            "holder-a",
            "token-a",
            1_u64,
        )]);
        let new_fence = stable_revision(&[(
            "cluster-a",
            "controller-a",
            "session-a",
            "holder-a",
            "token-b",
            2_u64,
        )]);
        assert_ne!(first, new_session);
        assert_ne!(first, new_fence);
    }

    #[tokio::test]
    async fn failed_namespace_discards_partial_pages() {
        let first_page = serde_json::to_vec(&serde_json::json!({
            "success": true,
            "data": [{
                "apiVersion": "gateway.networking.k8s.io/v1",
                "kind": "HTTPRoute",
                "metadata": {"name": "route-a", "namespace": "edgion-system"},
                "spec": {}
            }],
            "count": 1,
            "continue_token": "next"
        }))
        .unwrap();
        let client = Arc::new(FakeHttpClient {
            responses: Mutex::new(VecDeque::from([
                Ok(response(200, first_page)),
                Ok(response(500, b"{}".to_vec())),
            ])),
            calls: Mutex::new(Vec::new()),
        });
        let target = ResolvedTarget {
            resolution: ClusterResolution {
                cluster: "cluster-a".into(),
                state: ClusterResolutionState::Available,
                controller_id: Some("controller-a".into()),
                candidates: vec!["controller-a".into()],
            },
            session_id: Some("session-a".into()),
            owner_route: None,
        };
        let result = list_cluster_inventory(
            client,
            target,
            "HTTPRoute",
            "httproute",
            vec!["edgion-system".into()],
            InventoryScanLimits {
                page_size: 100,
                max_pages_per_namespace: 10,
                max_items_per_cluster: 100,
            },
            InventoryBudget::new(InventoryBudgetLimits {
                max_items: 100,
                max_retained_bytes: 1024 * 1024,
                max_upstream_pages: 10,
            })
            .unwrap(),
        )
        .await;
        assert!(result.items.is_empty());
        assert_eq!(result.errors[0].status, Some(500));
    }

    #[test]
    fn inventory_item_validation_binds_kind_and_namespace() {
        let valid = serde_json::json!({
            "kind": "ReferenceGrant",
            "metadata": {"name": "grant-a", "namespace": "edgion-data"}
        });
        assert!(valid_inventory_item(
            &valid,
            "ReferenceGrant",
            "edgion-data"
        ));
        assert!(!valid_inventory_item(
            &valid,
            "ReferenceGrant",
            "edgion-system"
        ));
    }
}
