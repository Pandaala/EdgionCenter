//! Read-only planning orchestration for durable GlobalResources.
//!
//! A plan uses one Controller directory snapshot, resolves an authoritative
//! session or owner fence for every selected cluster, and performs only fresh
//! detail GETs. Observed payloads live only for the duration of this call.

use crate::federation::registry::ControllerRegistry;
use crate::global_resources::{
    request_target, resolve_authoritative_targets, stable_revision, ClusterResolutionState,
    ResolvedTarget, TargetResolutionMode,
};
use crate::poll::ControllerHttpClient;
use edgion_center_core::{
    normalized_global_resource_desired, plan_global_resource, resolve_global_resource_targets,
    ControllerDirectory, ControllerOwnerLocator, CoreError, GlobalResource, GlobalResourceId,
    GlobalResourcePlan, GlobalResourceResolvedTarget, GlobalResourceTargetObservation,
    GlobalResourceTargetResolutionState, GlobalResourcesConfig, GLOBAL_RESOURCE_KINDS,
};
use futures::{stream::FuturesUnordered, StreamExt};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

const DEFAULT_PLAN_FANOUT_CONCURRENCY: usize = 8;
const DEFAULT_PLAN_DEADLINE: Duration = Duration::from_secs(15);
const DEFAULT_DETAIL_DEADLINE: Duration = Duration::from_secs(5);
const MAX_PLAN_TARGETS: usize = 100;
const MAX_DETAIL_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalResourceApplyOutcome {
    Created,
    Updated,
    Noop,
    Conflict,
    Denied,
    Offline,
    RateLimited,
    Failed,
    Unknown,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalResourceApplyTargetResult {
    pub cluster: String,
    pub outcome: GlobalResourceApplyOutcome,
    pub status_code: Option<u32>,
    pub resource_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalResourceApplyResult {
    pub global_resource_id: GlobalResourceId,
    pub generation: u64,
    pub plan_revision: String,
    pub targets: Vec<GlobalResourceApplyTargetResult>,
}

fn classify_apply_response(
    cluster: String,
    response: Result<crate::poll::ControllerHttpResponse, String>,
) -> GlobalResourceApplyTargetResult {
    let Ok(response) = response else {
        return GlobalResourceApplyTargetResult {
            cluster,
            outcome: GlobalResourceApplyOutcome::Unknown,
            status_code: None,
            resource_version: None,
        };
    };
    let outcome = match response.status_code {
        200..=299 => {
            if response.status_code == 201 {
                GlobalResourceApplyOutcome::Created
            } else {
                GlobalResourceApplyOutcome::Updated
            }
        }
        401 | 403 => GlobalResourceApplyOutcome::Denied,
        409 => GlobalResourceApplyOutcome::Conflict,
        429 => GlobalResourceApplyOutcome::RateLimited,
        408 | 500..=599 => GlobalResourceApplyOutcome::Unknown,
        _ => GlobalResourceApplyOutcome::Failed,
    };
    let resource_version = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|v| {
            v.pointer("/metadata/resourceVersion")
                .and_then(Value::as_str)
                .map(String::from)
        });
    GlobalResourceApplyTargetResult {
        cluster,
        outcome,
        status_code: Some(response.status_code),
        resource_version,
    }
}

/// Runtime service consumed by the Admin API for read-only plan generation.
pub struct GlobalResourcePlanService {
    directory: Arc<dyn ControllerDirectory>,
    client: Arc<dyn ControllerHttpClient>,
    resolution_mode: TargetResolutionMode,
    config: GlobalResourcesConfig,
    fanout: Arc<Semaphore>,
    total_deadline: Duration,
    detail_deadline: Duration,
}

impl GlobalResourcePlanService {
    pub fn new_standalone(
        directory: Arc<dyn ControllerDirectory>,
        registry: ControllerRegistry,
        client: Arc<dyn ControllerHttpClient>,
        config: GlobalResourcesConfig,
    ) -> Self {
        Self::new(
            directory,
            client,
            TargetResolutionMode::Standalone(registry),
            config,
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
            TargetResolutionMode::Kubernetes(owner_locator),
            config,
        )
    }

    fn new(
        directory: Arc<dyn ControllerDirectory>,
        client: Arc<dyn ControllerHttpClient>,
        resolution_mode: TargetResolutionMode,
        config: GlobalResourcesConfig,
    ) -> Self {
        Self {
            directory,
            client,
            resolution_mode,
            config,
            fanout: Arc::new(Semaphore::new(DEFAULT_PLAN_FANOUT_CONCURRENCY)),
            total_deadline: DEFAULT_PLAN_DEADLINE,
            detail_deadline: DEFAULT_DETAIL_DEADLINE,
        }
    }

    pub fn config(&self) -> &GlobalResourcesConfig {
        &self.config
    }

    pub async fn apply_plan(
        &self,
        resource: &GlobalResource,
        plan: &GlobalResourcePlan,
    ) -> Result<GlobalResourceApplyResult, CoreError> {
        resource.validate(&self.config)?;
        let (fresh_plan, owner_routes) = tokio::time::timeout(
            self.total_deadline,
            self.plan_with_routes(resource, Some(&plan.binding.ordered_targets)),
        )
        .await
        .map_err(|_| CoreError::Adapter("GlobalResource apply plan deadline exceeded".into()))??;
        if fresh_plan != *plan {
            return Err(CoreError::Conflict(
                "global resource plan evidence is stale".to_string(),
            ));
        }
        plan.binding.validate_exact(
            resource,
            &self.config,
            &plan.targets,
            &plan.binding.membership_revision,
        )?;
        let desired = normalized_global_resource_desired(resource, &self.config)?;
        let name = resource
            .desired
            .template_document
            .pointer("/metadata/name")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::Conflict("invalid GlobalResource name".into()))?;
        let namespace = resource.desired.target_namespace.as_str();
        let path_kind = GLOBAL_RESOURCE_KINDS
            .iter()
            .find(|entry| entry.kind == resource.desired.resource_kind)
            .ok_or_else(|| CoreError::Conflict("invalid GlobalResource kind".into()))?
            .path_kind;
        let mut tasks = FuturesUnordered::new();
        for target in plan.targets.iter().cloned() {
            let client = self.client.clone();
            let desired = desired.clone();
            let semaphore = self.fanout.clone();
            let owner_route = owner_routes.get(&target.cluster).cloned();
            tasks.push(async move {
                let Ok(_permit) = semaphore.acquire_owned().await else {
                    return GlobalResourceApplyTargetResult {
                        cluster: target.cluster,
                        outcome: GlobalResourceApplyOutcome::Invalid,
                        status_code: None,
                        resource_version: None,
                    };
                };
                let cluster = target.cluster.clone();
                let Some(controller_id) = target.controller_id.clone() else {
                    return GlobalResourceApplyTargetResult {
                        cluster,
                        outcome: GlobalResourceApplyOutcome::Offline,
                        status_code: None,
                        resource_version: None,
                    };
                };
                let (method, path, mut document) = match target.state {
                    edgion_center_core::GlobalResourcePlanState::Create => (
                        "POST",
                        format!("/api/v1/namespaced/{path_kind}/{namespace}"),
                        desired,
                    ),
                    edgion_center_core::GlobalResourcePlanState::Update => (
                        "PUT",
                        format!("/api/v1/namespaced/{path_kind}/{namespace}/{name}"),
                        desired,
                    ),
                    edgion_center_core::GlobalResourcePlanState::Noop => {
                        return GlobalResourceApplyTargetResult {
                            cluster,
                            outcome: GlobalResourceApplyOutcome::Noop,
                            status_code: None,
                            resource_version: target.observed_resource_version,
                        }
                    }
                    _ => {
                        return GlobalResourceApplyTargetResult {
                            cluster,
                            outcome: GlobalResourceApplyOutcome::Invalid,
                            status_code: None,
                            resource_version: None,
                        }
                    }
                };
                if method == "PUT" {
                    if let Some(version) = target.observed_resource_version.clone() {
                        if let Some(metadata) =
                            document.get_mut("metadata").and_then(Value::as_object_mut)
                        {
                            metadata.insert("resourceVersion".into(), Value::String(version));
                        }
                    }
                }
                let body = match serde_json::to_vec(&document) {
                    Ok(body) => body,
                    Err(_) => {
                        return GlobalResourceApplyTargetResult {
                            cluster,
                            outcome: GlobalResourceApplyOutcome::Invalid,
                            status_code: None,
                            resource_version: None,
                        }
                    }
                };
                let response = if let Some(route) = owner_route.as_ref() {
                    client
                        .request_fenced(
                            &controller_id,
                            method.into(),
                            path,
                            HashMap::from([(
                                String::from("content-type"),
                                String::from("application/json"),
                            )]),
                            body,
                            route,
                        )
                        .await
                } else if let Some(session) = target.controller_session_id.as_deref() {
                    client
                        .request_session_fenced(
                            &controller_id,
                            method.into(),
                            path,
                            HashMap::from([(
                                String::from("content-type"),
                                String::from("application/json"),
                            )]),
                            body,
                            session,
                        )
                        .await
                } else {
                    Err(String::from("missing controller session fence"))
                };
                classify_apply_response(cluster, response)
            });
        }
        let mut targets = Vec::new();
        tokio::time::timeout(self.total_deadline, async {
            while let Some(result) = tasks.next().await {
                targets.push(result);
            }
        })
        .await
        .map_err(|_| CoreError::Adapter("GlobalResource apply deadline exceeded".into()))?;
        targets.sort_by(|a, b| a.cluster.cmp(&b.cluster));
        Ok(GlobalResourceApplyResult {
            global_resource_id: resource.id.clone(),
            generation: resource.generation,
            plan_revision: plan.binding.plan_revision.clone(),
            targets,
        })
    }

    /// Generate a fresh read-only plan. `requested_clusters`, when supplied,
    /// may only narrow the durable selector and never expand it.
    pub async fn plan(
        &self,
        resource: &GlobalResource,
        requested_clusters: Option<&[String]>,
    ) -> Result<GlobalResourcePlan, CoreError> {
        tokio::time::timeout(
            self.total_deadline,
            self.plan_with_routes(resource, requested_clusters),
        )
        .await
        .map_err(|_| CoreError::Adapter("GlobalResource plan deadline exceeded".to_string()))?
        .map(|(plan, _)| plan)
    }

    async fn plan_with_routes(
        &self,
        resource: &GlobalResource,
        requested_clusters: Option<&[String]>,
    ) -> Result<
        (
            GlobalResourcePlan,
            BTreeMap<String, edgion_center_core::ControllerOwnerRoute>,
        ),
        CoreError,
    > {
        resource.validate(&self.config)?;

        // This is deliberately the only directory read in one planning call.
        let records = self.directory.list().await?;
        let selector_targets =
            resolve_global_resource_targets(&resource.desired.target_selector, &records)?;
        let selected = narrow_targets(selector_targets, requested_clusters)?;
        if selected.len() > MAX_PLAN_TARGETS {
            return Err(CoreError::Conflict(format!(
                "GlobalResource plan cannot target more than {MAX_PLAN_TARGETS} clusters"
            )));
        }

        let requested = selected
            .iter()
            .map(|target| target.cluster.as_str())
            .collect::<HashSet<_>>();
        let authoritative =
            resolve_authoritative_targets(records, &self.resolution_mode, &requested).await?;
        let owner_routes = authoritative
            .iter()
            .filter_map(|target| {
                target
                    .owner_route
                    .clone()
                    .map(|route| (target.resolution.cluster.clone(), route))
            })
            .collect::<BTreeMap<_, _>>();
        let targets = merge_authoritative_targets(selected, authoritative);
        let membership_revision = stable_revision(&MembershipEvidence::from(targets.as_slice()));
        let observations = self
            .observe_targets(resource, &targets, &owner_routes)
            .await;
        let plan = plan_global_resource(
            resource,
            &self.config,
            &targets,
            &observations,
            &membership_revision,
        )?;
        Ok((plan, owner_routes))
    }

    async fn observe_targets(
        &self,
        resource: &GlobalResource,
        targets: &[GlobalResourceResolvedTarget],
        owner_routes: &BTreeMap<String, edgion_center_core::ControllerOwnerRoute>,
    ) -> BTreeMap<String, GlobalResourceTargetObservation> {
        let name = resource
            .desired
            .template_document
            .pointer("/metadata/name")
            .and_then(Value::as_str)
            .expect("validated GlobalResource has metadata.name")
            .to_string();
        let namespace = resource.desired.target_namespace.clone();
        let path_kind = GLOBAL_RESOURCE_KINDS
            .iter()
            .find(|entry| entry.kind == resource.desired.resource_kind)
            .expect("validated GlobalResource kind exists in catalog")
            .path_kind;
        let target_by_cluster = targets
            .iter()
            .filter(|target| target.state == GlobalResourceTargetResolutionState::Available)
            .map(|target| {
                (
                    target.cluster.clone(),
                    (
                        target
                            .controller_id
                            .clone()
                            .expect("available target has id"),
                        target.controller_session_id.clone(),
                        target.ownership_fence.clone(),
                    ),
                )
            })
            .collect::<HashMap<_, _>>();

        let mut tasks = FuturesUnordered::new();
        for target in targets
            .iter()
            .filter(|target| target.state == GlobalResourceTargetResolutionState::Available)
        {
            let cluster = target.cluster.clone();
            let (controller_id, session_id, ownership_fence) = target_by_cluster
                .get(&cluster)
                .expect("available target evidence exists")
                .clone();
            let owner_route = ownership_fence.and_then(|_| owner_routes.get(&cluster).cloned());
            let client = self.client.clone();
            let fanout = self.fanout.clone();
            let path = format!("/api/v1/namespaced/{path_kind}/{namespace}/{name}");
            let deadline = self.detail_deadline;
            tasks.push(async move {
                let permit = fanout
                    .acquire_owned()
                    .await
                    .expect("GlobalResource planner semaphore closed");
                let observation = tokio::time::timeout(
                    deadline,
                    request_target(
                        client.as_ref(),
                        &controller_id,
                        session_id.as_deref(),
                        owner_route.as_ref(),
                        path,
                    ),
                )
                .await
                .map_or(GlobalResourceTargetObservation::Indeterminate, |response| {
                    classify_detail_response(response)
                });
                drop(permit);
                (cluster, observation)
            });
        }

        let mut observations = BTreeMap::new();
        while let Some((cluster, observation)) = tasks.next().await {
            observations.insert(cluster, observation);
        }
        observations
    }
}

fn narrow_targets(
    selector_targets: Vec<GlobalResourceResolvedTarget>,
    requested_clusters: Option<&[String]>,
) -> Result<Vec<GlobalResourceResolvedTarget>, CoreError> {
    let Some(requested_clusters) = requested_clusters else {
        return Ok(selector_targets);
    };
    let requested = requested_clusters.iter().cloned().collect::<BTreeSet<_>>();
    if requested.len() > MAX_PLAN_TARGETS {
        return Err(CoreError::Conflict(format!(
            "GlobalResource plan cannot target more than {MAX_PLAN_TARGETS} clusters"
        )));
    }
    let allowed = selector_targets
        .iter()
        .map(|target| target.cluster.as_str())
        .collect::<HashSet<_>>();
    if let Some(cluster) = requested
        .iter()
        .find(|cluster| !allowed.contains(cluster.as_str()))
    {
        return Err(CoreError::Conflict(format!(
            "requested cluster '{cluster}' is outside the GlobalResource selector"
        )));
    }
    Ok(selector_targets
        .into_iter()
        .filter(|target| requested.contains(&target.cluster))
        .collect())
}

fn merge_authoritative_targets(
    selected: Vec<GlobalResourceResolvedTarget>,
    authoritative: Vec<ResolvedTarget>,
) -> Vec<GlobalResourceResolvedTarget> {
    let authoritative = authoritative
        .into_iter()
        .map(|target| (target.resolution.cluster.clone(), target))
        .collect::<BTreeMap<_, _>>();
    selected
        .into_iter()
        .map(|selected| {
            if selected.state == GlobalResourceTargetResolutionState::Indeterminate {
                return selected;
            }
            authoritative
                .get(&selected.cluster)
                .map(core_target_from_authoritative)
                .unwrap_or(selected)
        })
        .collect()
}

fn core_target_from_authoritative(target: &ResolvedTarget) -> GlobalResourceResolvedTarget {
    let state = match target.resolution.state {
        ClusterResolutionState::Available => GlobalResourceTargetResolutionState::Available,
        ClusterResolutionState::Offline => GlobalResourceTargetResolutionState::Offline,
        ClusterResolutionState::Ambiguous => GlobalResourceTargetResolutionState::Ambiguous,
        ClusterResolutionState::Indeterminate => GlobalResourceTargetResolutionState::Indeterminate,
    };
    GlobalResourceResolvedTarget {
        cluster: target.resolution.cluster.clone(),
        state,
        controller_id: target.resolution.controller_id.clone(),
        controller_session_id: target.session_id.clone(),
        ownership_fence: target
            .owner_route
            .as_ref()
            .map(|route| route.ownership_fence.clone()),
        candidates: target.resolution.candidates.clone(),
    }
}

fn classify_detail_response(
    response: Result<crate::poll::ControllerHttpResponse, String>,
) -> GlobalResourceTargetObservation {
    let Ok(response) = response else {
        return GlobalResourceTargetObservation::Indeterminate;
    };
    if response.body.len() > MAX_DETAIL_BODY_BYTES {
        return GlobalResourceTargetObservation::Invalid;
    }
    match response.status_code {
        404 => GlobalResourceTargetObservation::Missing,
        401 | 403 => GlobalResourceTargetObservation::Denied,
        status if (200..300).contains(&status) => serde_json::from_slice::<Value>(&response.body)
            .ok()
            .filter(Value::is_object)
            .map(GlobalResourceTargetObservation::Present)
            .unwrap_or(GlobalResourceTargetObservation::Invalid),
        status if status >= 500 || status == 408 || status == 429 => {
            GlobalResourceTargetObservation::Indeterminate
        }
        _ => GlobalResourceTargetObservation::Invalid,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MembershipEvidence<'a> {
    targets: &'a [GlobalResourceResolvedTarget],
}

impl<'a> From<&'a [GlobalResourceResolvedTarget]> for MembershipEvidence<'a> {
    fn from(targets: &'a [GlobalResourceResolvedTarget]) -> Self {
        Self { targets }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::federation::proto::RegisterRequest;
    use crate::poll::ControllerHttpResponse;
    use async_trait::async_trait;
    use edgion_center_core::{
        global_resource_for_create, ControllerId, ControllerOwnerRoute, ControllerPhase,
        ControllerRecord, ControllerRegistration, ControllerRuntimeObservation, CoreResult,
        EvictionResult, GlobalResourceDesired, GlobalResourceId, GlobalResourceInventoryKind,
        GlobalResourceSyncPolicy, GlobalResourceTargetSelector, OfflineOutcome, OwnershipFence,
        SessionId,
    };
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::mpsc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum RecordedFence {
        Session(String),
        Owner(ControllerOwnerRoute),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedRequest {
        controller_id: String,
        method: String,
        path: String,
        fence: RecordedFence,
    }

    struct FakeDirectory {
        records: Vec<ControllerRecord>,
        lists: AtomicUsize,
    }

    #[async_trait]
    impl ControllerDirectory for FakeDirectory {
        async fn upsert_registration(&self, _: ControllerRegistration) -> CoreResult<()> {
            unreachable!("planner is read-only")
        }

        async fn mark_offline(
            &self,
            _: &ControllerId,
            _: &SessionId,
            _: Option<&OwnershipFence>,
            _: i64,
        ) -> CoreResult<OfflineOutcome> {
            unreachable!("planner is read-only")
        }

        async fn list(&self) -> CoreResult<Vec<ControllerRecord>> {
            self.lists.fetch_add(1, Ordering::SeqCst);
            Ok(self.records.clone())
        }

        async fn project_runtime(&self, _: ControllerRuntimeObservation) -> CoreResult<bool> {
            unreachable!("planner is read-only")
        }

        async fn evict(&self, _: &ControllerId) -> CoreResult<EvictionResult> {
            unreachable!("planner is read-only")
        }
    }

    struct RecordingClient {
        response: Mutex<Option<Result<ControllerHttpResponse, String>>>,
        calls: Mutex<Vec<RecordedRequest>>,
    }

    impl RecordingClient {
        fn with_response(status_code: u32, body: impl Into<Vec<u8>>) -> Self {
            Self {
                response: Mutex::new(Some(Ok(ControllerHttpResponse {
                    status_code,
                    body: body.into(),
                }))),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn take_response(&self) -> Result<ControllerHttpResponse, String> {
            self.response
                .lock()
                .take()
                .expect("one planner request expected")
        }
    }

    #[async_trait]
    impl ControllerHttpClient for RecordingClient {
        async fn request(
            &self,
            _: &str,
            _: String,
            _: String,
            _: HashMap<String, String>,
            _: Vec<u8>,
        ) -> Result<ControllerHttpResponse, String> {
            panic!("planner must use a fenced request")
        }

        async fn request_fenced(
            &self,
            controller_id: &str,
            method: String,
            path: String,
            _: HashMap<String, String>,
            _: Vec<u8>,
            expected_owner: &ControllerOwnerRoute,
        ) -> Result<ControllerHttpResponse, String> {
            self.calls.lock().push(RecordedRequest {
                controller_id: controller_id.to_string(),
                method,
                path,
                fence: RecordedFence::Owner(expected_owner.clone()),
            });
            self.take_response()
        }

        async fn request_session_fenced(
            &self,
            controller_id: &str,
            method: String,
            path: String,
            _: HashMap<String, String>,
            _: Vec<u8>,
            expected_session_id: &str,
        ) -> Result<ControllerHttpResponse, String> {
            self.calls.lock().push(RecordedRequest {
                controller_id: controller_id.to_string(),
                method,
                path,
                fence: RecordedFence::Session(expected_session_id.to_string()),
            });
            self.take_response()
        }
    }

    struct FixedOwnerLocator {
        route: ControllerOwnerRoute,
        calls: AtomicUsize,
    }

    struct ConcurrencyClient {
        active: AtomicUsize,
        maximum: AtomicUsize,
        calls: AtomicUsize,
        delay: Duration,
    }

    struct ActiveRequest<'a>(&'a AtomicUsize);

    impl Drop for ActiveRequest<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl ControllerHttpClient for ConcurrencyClient {
        async fn request(
            &self,
            _: &str,
            _: String,
            _: String,
            _: HashMap<String, String>,
            _: Vec<u8>,
        ) -> Result<ControllerHttpResponse, String> {
            panic!("planner must use a fenced request")
        }

        async fn request_fenced(
            &self,
            _: &str,
            method: String,
            _: String,
            _: HashMap<String, String>,
            _: Vec<u8>,
            _: &ControllerOwnerRoute,
        ) -> Result<ControllerHttpResponse, String> {
            assert_eq!(method, "GET");
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            let _active = ActiveRequest(&self.active);
            tokio::time::sleep(self.delay).await;
            Ok(ControllerHttpResponse {
                status_code: 404,
                body: Vec::new(),
            })
        }
    }

    struct PendingClient {
        dropped: Arc<AtomicBool>,
    }

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl ControllerHttpClient for PendingClient {
        async fn request(
            &self,
            _: &str,
            _: String,
            _: String,
            _: HashMap<String, String>,
            _: Vec<u8>,
        ) -> Result<ControllerHttpResponse, String> {
            panic!("planner must use a fenced request")
        }

        async fn request_fenced(
            &self,
            _: &str,
            method: String,
            _: String,
            _: HashMap<String, String>,
            _: Vec<u8>,
            _: &ControllerOwnerRoute,
        ) -> Result<ControllerHttpResponse, String> {
            assert_eq!(method, "GET");
            let _drop_flag = DropFlag(self.dropped.clone());
            std::future::pending().await
        }
    }

    #[async_trait]
    impl ControllerOwnerLocator for FixedOwnerLocator {
        async fn locate(&self, _: &ControllerId) -> CoreResult<Option<ControllerOwnerRoute>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(self.route.clone()))
        }
    }

    fn record(
        controller_id: &str,
        cluster: &str,
        session_id: &str,
        owner: Option<(&str, OwnershipFence)>,
    ) -> ControllerRecord {
        ControllerRecord {
            controller_id: ControllerId::new(controller_id).unwrap(),
            current_session_id: Some(SessionId::new(session_id).unwrap()),
            cluster: cluster.to_string(),
            environments: vec!["production".to_string()],
            tags: vec!["edge".to_string()],
            connected_replica: owner.as_ref().map(|(holder, _)| (*holder).to_string()),
            ownership_fence: owner.map(|(_, fence)| fence),
            sync_version: None,
            watch_server_id: None,
            resource_count: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: None,
            phase: ControllerPhase::Online,
            last_seen_unix_ms: 1,
        }
    }

    fn resource(selector: GlobalResourceTargetSelector) -> GlobalResource {
        let config = GlobalResourcesConfig::default();
        global_resource_for_create(
            GlobalResourceId::new("shared-ip-list").unwrap(),
            GlobalResourceDesired {
                display_name: "Shared IP list".to_string(),
                resource_kind: GlobalResourceInventoryKind::EdgionConfigData,
                target_namespace: "edgion-data".to_string(),
                template_document: serde_json::json!({
                    "apiVersion": "edgion.io/v1",
                    "kind": "EdgionConfigData",
                    "metadata": {
                        "name": "shared-ip-list",
                        "namespace": "edgion-data"
                    },
                    "spec": {}
                }),
                target_selector: selector,
                sync_policy: GlobalResourceSyncPolicy::default(),
            },
            "tester",
            1,
            &config,
        )
        .unwrap()
    }

    fn register_request(controller_id: &str, cluster: &str) -> RegisterRequest {
        RegisterRequest {
            controller_id: controller_id.to_string(),
            cluster: cluster.to_string(),
            env: Vec::new(),
            tag: Vec::new(),
            supported_kinds: Vec::new(),
        }
    }

    #[tokio::test]
    async fn kubernetes_plan_get_uses_exact_owner_route_from_the_same_resolution() {
        let fence = OwnershipFence {
            token: "fence-a".to_string(),
            epoch: 7,
        };
        let route = ControllerOwnerRoute {
            holder: "center-a".to_string(),
            endpoint: "https://center-a.internal".to_string(),
            ownership_fence: fence.clone(),
        };
        let directory = Arc::new(FakeDirectory {
            records: vec![record(
                "controller-a",
                "cluster-a",
                "session-a",
                Some(("center-a", fence.clone())),
            )],
            lists: AtomicUsize::new(0),
        });
        let locator = Arc::new(FixedOwnerLocator {
            route: route.clone(),
            calls: AtomicUsize::new(0),
        });
        let client = Arc::new(RecordingClient::with_response(404, Vec::new()));
        let service = GlobalResourcePlanService::new_kubernetes(
            directory.clone(),
            locator.clone(),
            client.clone(),
            GlobalResourcesConfig::default(),
        );

        let plan = service
            .plan(&resource(GlobalResourceTargetSelector::All), None)
            .await
            .unwrap();

        assert_eq!(
            plan.targets[0].state,
            edgion_center_core::GlobalResourcePlanState::Create
        );
        assert_eq!(plan.targets[0].ownership_fence.as_ref(), Some(&fence));
        assert_eq!(directory.lists.load(Ordering::SeqCst), 1);
        assert_eq!(locator.calls.load(Ordering::SeqCst), 1);
        let calls = client.calls.lock();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "GET");
        assert_eq!(calls[0].fence, RecordedFence::Owner(route));
    }

    #[tokio::test]
    async fn standalone_plan_get_uses_exact_session_and_never_mutates() {
        let directory = Arc::new(FakeDirectory {
            records: vec![record("controller-a", "cluster-a", "session-a", None)],
            lists: AtomicUsize::new(0),
        });
        let registry = ControllerRegistry::new();
        let (tx, _rx) = mpsc::channel(1);
        registry.register(
            "controller-a".to_string(),
            register_request("controller-a", "cluster-a"),
            tx,
            "session-a".to_string(),
        );
        let client = Arc::new(RecordingClient::with_response(404, Vec::new()));
        let service = GlobalResourcePlanService::new_standalone(
            directory.clone(),
            registry,
            client.clone(),
            GlobalResourcesConfig::default(),
        );

        let plan = service
            .plan(&resource(GlobalResourceTargetSelector::All), None)
            .await
            .unwrap();

        assert_eq!(
            plan.targets[0].controller_session_id.as_deref(),
            Some("session-a")
        );
        assert_eq!(directory.lists.load(Ordering::SeqCst), 1);
        let calls = client.calls.lock();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "GET");
        assert_eq!(
            calls[0].fence,
            RecordedFence::Session("session-a".to_string())
        );
    }

    #[test]
    fn detail_statuses_are_classified_without_parsing_error_messages() {
        let response = |status_code, body: &[u8]| {
            classify_detail_response(Ok(ControllerHttpResponse {
                status_code,
                body: body.to_vec(),
            }))
        };
        assert_eq!(
            response(404, b"arbitrary"),
            GlobalResourceTargetObservation::Missing
        );
        assert_eq!(
            response(403, b"arbitrary"),
            GlobalResourceTargetObservation::Denied
        );
        assert_eq!(
            response(503, b"arbitrary"),
            GlobalResourceTargetObservation::Indeterminate
        );
        assert_eq!(
            response(400, b"arbitrary"),
            GlobalResourceTargetObservation::Invalid
        );
        assert_eq!(
            response(200, b"not-json"),
            GlobalResourceTargetObservation::Invalid
        );
    }

    #[test]
    fn requested_targets_can_only_narrow_the_selector() {
        let selected = vec![GlobalResourceResolvedTarget {
            cluster: "cluster-a".to_string(),
            state: GlobalResourceTargetResolutionState::Offline,
            controller_id: None,
            controller_session_id: None,
            ownership_fence: None,
            candidates: Vec::new(),
        }];
        assert!(narrow_targets(selected.clone(), Some(&["cluster-b".to_string()])).is_err());
        assert_eq!(
            narrow_targets(selected, Some(&["cluster-a".to_string()]))
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn planning_fanout_is_bounded() {
        let fence = OwnershipFence {
            token: "shared-fence".to_string(),
            epoch: 1,
        };
        let route = ControllerOwnerRoute {
            holder: "center-a".to_string(),
            endpoint: "https://center-a.internal".to_string(),
            ownership_fence: fence.clone(),
        };
        let records = (0..20)
            .map(|index| {
                record(
                    &format!("controller-{index}"),
                    &format!("cluster-{index:02}"),
                    &format!("session-{index}"),
                    Some(("center-a", fence.clone())),
                )
            })
            .collect();
        let directory = Arc::new(FakeDirectory {
            records,
            lists: AtomicUsize::new(0),
        });
        let locator = Arc::new(FixedOwnerLocator {
            route,
            calls: AtomicUsize::new(0),
        });
        let client = Arc::new(ConcurrencyClient {
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(5),
        });
        let service = GlobalResourcePlanService::new_kubernetes(
            directory,
            locator,
            client.clone(),
            GlobalResourcesConfig::default(),
        );

        let plan = service
            .plan(&resource(GlobalResourceTargetSelector::All), None)
            .await
            .unwrap();

        assert_eq!(plan.targets.len(), 20);
        assert_eq!(client.calls.load(Ordering::SeqCst), 20);
        assert!(client.maximum.load(Ordering::SeqCst) <= DEFAULT_PLAN_FANOUT_CONCURRENCY);
        assert!(client.maximum.load(Ordering::SeqCst) > 1);
        assert_eq!(client.active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn total_deadline_cancels_in_flight_observations() {
        let fence = OwnershipFence {
            token: "fence-a".to_string(),
            epoch: 1,
        };
        let route = ControllerOwnerRoute {
            holder: "center-a".to_string(),
            endpoint: "https://center-a.internal".to_string(),
            ownership_fence: fence.clone(),
        };
        let directory = Arc::new(FakeDirectory {
            records: vec![record(
                "controller-a",
                "cluster-a",
                "session-a",
                Some(("center-a", fence)),
            )],
            lists: AtomicUsize::new(0),
        });
        let locator = Arc::new(FixedOwnerLocator {
            route,
            calls: AtomicUsize::new(0),
        });
        let dropped = Arc::new(AtomicBool::new(false));
        let client = Arc::new(PendingClient {
            dropped: dropped.clone(),
        });
        let mut service = GlobalResourcePlanService::new_kubernetes(
            directory,
            locator,
            client,
            GlobalResourcesConfig::default(),
        );
        service.total_deadline = Duration::from_millis(10);
        service.detail_deadline = Duration::from_secs(1);

        assert!(service
            .plan(&resource(GlobalResourceTargetSelector::All), None)
            .await
            .is_err());
        assert!(dropped.load(Ordering::SeqCst));
    }
}
