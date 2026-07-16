use std::sync::Arc;

use async_trait::async_trait;
use edgion_center_core::{
    ControllerDirectory, ControllerId, ControllerPhase, ControllerRecord, ControllerRegistration,
    ControllerRuntimeObservation, CoreError, CoreResult, EvictionOutcome, EvictionResult,
    EvictionTarget, OfflineOutcome, SessionId,
};
use kube::{
    api::{ListParams, PostParams},
    Api, Client, ResourceExt,
};
use sha2::{Digest, Sha256};

use crate::{
    EdgionController, EdgionControllerPhase, EdgionControllerSpec, EdgionControllerStatus,
};

const MAX_CONFLICT_RETRIES: usize = 8;

/// Generate a deterministic DNS-label name and retain a digest suffix even for
/// already-valid ids. The canonical id remains in spec and is checked on every
/// update, making a hash collision fail closed rather than alias two streams.
pub fn controller_resource_name(controller_id: &str) -> String {
    let mut prefix = String::with_capacity(50);
    let mut previous_dash = false;
    for character in controller_id.chars() {
        let mapped = if character.is_ascii_alphanumeric() {
            character.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if previous_dash || prefix.is_empty() {
                continue;
            }
            previous_dash = true;
        } else {
            previous_dash = false;
        }
        prefix.push(mapped);
        if prefix.len() == 50 {
            break;
        }
    }
    while prefix.ends_with('-') {
        prefix.pop();
    }
    if prefix.is_empty() {
        prefix.push_str("controller");
    }
    let digest = hex::encode(Sha256::digest(controller_id.as_bytes()));
    format!("{prefix}-{}", &digest[..12])
}

#[derive(Debug)]
enum ResourceError {
    Conflict,
    Other(String),
}

impl From<kube::Error> for ResourceError {
    fn from(error: kube::Error) -> Self {
        match &error {
            kube::Error::Api(response) if response.code == 409 => Self::Conflict,
            _ => Self::Other(error.to_string()),
        }
    }
}

#[async_trait]
trait ControllerResources: Send + Sync {
    async fn get(&self, name: &str) -> Result<Option<EdgionController>, ResourceError>;
    async fn create(&self, resource: &EdgionController) -> Result<EdgionController, ResourceError>;
    async fn replace(
        &self,
        name: &str,
        resource: &EdgionController,
    ) -> Result<EdgionController, ResourceError>;
    async fn replace_status(
        &self,
        name: &str,
        resource: &EdgionController,
    ) -> Result<EdgionController, ResourceError>;
    async fn list(&self) -> Result<Vec<EdgionController>, ResourceError>;
}

struct KubernetesResources {
    api: Api<EdgionController>,
}

#[async_trait]
impl ControllerResources for KubernetesResources {
    async fn get(&self, name: &str) -> Result<Option<EdgionController>, ResourceError> {
        self.api.get_opt(name).await.map_err(Into::into)
    }

    async fn create(&self, resource: &EdgionController) -> Result<EdgionController, ResourceError> {
        self.api
            .create(&PostParams::default(), resource)
            .await
            .map_err(Into::into)
    }

    async fn replace(
        &self,
        name: &str,
        resource: &EdgionController,
    ) -> Result<EdgionController, ResourceError> {
        self.api
            .replace(name, &PostParams::default(), resource)
            .await
            .map_err(Into::into)
    }

    async fn replace_status(
        &self,
        name: &str,
        resource: &EdgionController,
    ) -> Result<EdgionController, ResourceError> {
        let body = serde_json::to_vec(resource)
            .map_err(|error| ResourceError::Other(error.to_string()))?;
        self.api
            .replace_status(name, &PostParams::default(), body)
            .await
            .map_err(Into::into)
    }

    async fn list(&self) -> Result<Vec<EdgionController>, ResourceError> {
        self.api
            .list(&ListParams::default())
            .await
            .map(|list| list.items)
            .map_err(Into::into)
    }
}

/// Kubernetes implementation of the platform-neutral controller directory.
#[derive(Clone)]
pub struct KubernetesControllerDirectory {
    resources: Arc<dyn ControllerResources>,
}

impl KubernetesControllerDirectory {
    pub fn new(client: Client, namespace: &str) -> Self {
        Self {
            resources: Arc::new(KubernetesResources {
                api: Api::namespaced(client, namespace),
            }),
        }
    }

    #[cfg(test)]
    fn with_resources(resources: Arc<dyn ControllerResources>) -> Self {
        Self { resources }
    }

    fn adapter_error(error: ResourceError) -> CoreError {
        match error {
            ResourceError::Conflict => {
                CoreError::Conflict("Kubernetes resourceVersion conflict".to_string())
            }
            ResourceError::Other(message) => CoreError::Adapter(message),
        }
    }

    async fn get_or_create(
        &self,
        name: &str,
        spec: EdgionControllerSpec,
    ) -> Result<EdgionController, ResourceError> {
        if let Some(mut existing) = self.resources.get(name).await? {
            // Preserve the canonical-id collision guard. The caller verifies
            // and reports the conflict before any mutable fields are touched.
            if existing.spec.controller_id != spec.controller_id {
                return Ok(existing);
            }
            if existing.spec != spec {
                existing.spec = spec;
                return self.resources.replace(name, &existing).await;
            }
            return Ok(existing);
        }
        let resource = EdgionController::new(name, spec);
        self.resources.create(&resource).await
    }

    fn verify_identity(resource: &EdgionController, id: &ControllerId) -> CoreResult<()> {
        if resource.spec.controller_id == id.as_str() {
            Ok(())
        } else {
            Err(CoreError::Conflict(format!(
                "Kubernetes name collision: {} maps both {:?} and {:?}",
                resource.name_any(),
                resource.spec.controller_id,
                id.as_str()
            )))
        }
    }

    async fn replace_status_with_retry<F>(
        &self,
        id: &ControllerId,
        initial_spec: EdgionControllerSpec,
        mut transition: F,
    ) -> CoreResult<bool>
    where
        F: FnMut(Option<&EdgionControllerStatus>, Option<i64>) -> Option<EdgionControllerStatus>,
    {
        let name = controller_resource_name(id.as_str());
        for _ in 0..MAX_CONFLICT_RETRIES {
            let mut resource = match self.get_or_create(&name, initial_spec.clone()).await {
                Ok(resource) => resource,
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            };
            Self::verify_identity(&resource, id)?;
            let generation = resource.metadata.generation;
            let Some(status) = transition(resource.status.as_ref(), generation) else {
                return Ok(false);
            };
            resource.status = Some(status);
            match self.resources.replace_status(&name, &resource).await {
                Ok(_) => return Ok(true),
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(format!(
            "controller projection for {} remained conflicted after {} attempts",
            id, MAX_CONFLICT_RETRIES
        )))
    }
}

fn time_from_millis(
    unix_ms: i64,
) -> CoreResult<k8s_openapi::apimachinery::pkg::apis::meta::v1::Time> {
    chrono::DateTime::from_timestamp_millis(unix_ms)
        .map(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time)
        .ok_or_else(|| {
            CoreError::Adapter(format!(
                "observed timestamp {unix_ms} is outside RFC3339 range"
            ))
        })
}

#[async_trait]
impl ControllerDirectory for KubernetesControllerDirectory {
    async fn upsert_registration(&self, registration: ControllerRegistration) -> CoreResult<()> {
        let last_seen_time = time_from_millis(registration.observed_at_unix_ms)?;
        let spec = EdgionControllerSpec {
            controller_id: registration.controller_id.to_string(),
            cluster: registration.cluster.clone(),
            environments: registration.environments.clone(),
            tags: registration.tags.clone(),
        };
        let projected = self
            .replace_status_with_retry(&registration.controller_id, spec, |current, generation| {
                let stale = match registration.ownership_fence.as_ref() {
                    Some(fence) => {
                        current.is_some_and(|status| status.ownership_epoch >= fence.epoch)
                    }
                    None => current.is_some_and(|status| {
                        status.ownership_epoch > 0
                            || status.observed_at_unix_ms >= registration.observed_at_unix_ms
                    }),
                };
                if stale {
                    return None;
                }
                Some(EdgionControllerStatus {
                    phase: EdgionControllerPhase::Online,
                    session_id: Some(registration.session_id.to_string()),
                    cluster: registration.cluster.clone(),
                    environments: registration.environments.clone(),
                    tags: registration.tags.clone(),
                    connected_replica: registration.connected_replica.clone(),
                    ownership_token: registration
                        .ownership_fence
                        .as_ref()
                        .map(|fence| fence.token.clone()),
                    ownership_epoch: registration
                        .ownership_fence
                        .as_ref()
                        .map_or(0, |fence| fence.epoch),
                    sync_version: None,
                    watch_server_id: None,
                    resource_count: None,
                    stats_updated_unix_ms: None,
                    watch_updated_unix_ms: None,
                    last_seen_time: last_seen_time.clone(),
                    observed_at_unix_ms: registration.observed_at_unix_ms,
                    evicted: false,
                    observed_generation: generation,
                })
            })
            .await?;
        if projected {
            Ok(())
        } else {
            Err(CoreError::Conflict(format!(
                "stale controller registration revision for {}",
                registration.controller_id
            )))
        }
    }

    async fn mark_offline(
        &self,
        id: &ControllerId,
        observed_session: &SessionId,
        ownership_fence: Option<&edgion_center_core::OwnershipFence>,
        observed_at_unix_ms: i64,
    ) -> CoreResult<OfflineOutcome> {
        let last_seen_time = time_from_millis(observed_at_unix_ms)?;
        let spec = EdgionControllerSpec {
            controller_id: id.to_string(),
            cluster: String::new(),
            environments: Vec::new(),
            tags: Vec::new(),
        };
        let changed = self
            .replace_status_with_retry(id, spec, |current, generation| {
                let not_current = current.is_some_and(|status| {
                    if status.evicted {
                        return true;
                    }
                    match ownership_fence {
                        Some(fence) if status.ownership_epoch < fence.epoch => false,
                        Some(fence) if status.ownership_epoch == fence.epoch => {
                            status.session_id.as_deref() != Some(observed_session.as_str())
                                || status.ownership_token.as_deref() != Some(fence.token.as_str())
                        }
                        Some(_) => true,
                        None => {
                            status.ownership_epoch > 0
                                || status.session_id.as_deref() != Some(observed_session.as_str())
                                || status.observed_at_unix_ms > observed_at_unix_ms
                        }
                    }
                });
                if not_current {
                    return None;
                }
                let current = current.cloned();
                Some(EdgionControllerStatus {
                    phase: EdgionControllerPhase::Offline,
                    session_id: Some(observed_session.to_string()),
                    cluster: current
                        .as_ref()
                        .map(|s| s.cluster.clone())
                        .unwrap_or_default(),
                    environments: current
                        .as_ref()
                        .map(|s| s.environments.clone())
                        .unwrap_or_default(),
                    tags: current.as_ref().map(|s| s.tags.clone()).unwrap_or_default(),
                    connected_replica: None,
                    ownership_token: ownership_fence.map(|fence| fence.token.clone()),
                    ownership_epoch: ownership_fence.map_or(0, |fence| fence.epoch),
                    sync_version: current.as_ref().and_then(|status| status.sync_version),
                    watch_server_id: current
                        .as_ref()
                        .and_then(|status| status.watch_server_id.clone()),
                    resource_count: current.as_ref().and_then(|status| status.resource_count),
                    stats_updated_unix_ms: current
                        .as_ref()
                        .and_then(|status| status.stats_updated_unix_ms),
                    watch_updated_unix_ms: current
                        .as_ref()
                        .and_then(|status| status.watch_updated_unix_ms),
                    last_seen_time: last_seen_time.clone(),
                    observed_at_unix_ms: current.as_ref().map_or(observed_at_unix_ms, |status| {
                        status.observed_at_unix_ms.max(observed_at_unix_ms)
                    }),
                    evicted: false,
                    observed_generation: generation,
                })
            })
            .await?;
        Ok(if changed {
            OfflineOutcome::Marked
        } else {
            OfflineOutcome::NotCurrent
        })
    }

    async fn list(&self) -> CoreResult<Vec<ControllerRecord>> {
        let mut records = Vec::new();
        for resource in self.resources.list().await.map_err(Self::adapter_error)? {
            let Some(status) = resource.status else {
                continue;
            };
            if status.evicted {
                continue;
            }
            let controller_id = ControllerId::new(resource.spec.controller_id)?;
            let online = status.phase == EdgionControllerPhase::Online;
            records.push(ControllerRecord {
                controller_id,
                current_session_id: if online {
                    status.session_id.map(SessionId::new).transpose()?
                } else {
                    None
                },
                cluster: status.cluster,
                environments: status.environments,
                tags: status.tags,
                connected_replica: if online {
                    status.connected_replica
                } else {
                    None
                },
                ownership_fence: if online && status.ownership_epoch > 0 {
                    status
                        .ownership_token
                        .map(|token| edgion_center_core::OwnershipFence {
                            token,
                            epoch: status.ownership_epoch,
                        })
                } else {
                    None
                },
                sync_version: status.sync_version,
                watch_server_id: status.watch_server_id,
                resource_count: status.resource_count,
                stats_updated_unix_ms: status.stats_updated_unix_ms,
                watch_updated_unix_ms: status.watch_updated_unix_ms,
                phase: match status.phase {
                    EdgionControllerPhase::Online => ControllerPhase::Online,
                    EdgionControllerPhase::Offline => ControllerPhase::Offline,
                    EdgionControllerPhase::Stale => ControllerPhase::Stale,
                },
                last_seen_unix_ms: status.observed_at_unix_ms,
            });
        }
        records.sort_by(|left, right| {
            left.controller_id
                .as_str()
                .cmp(right.controller_id.as_str())
        });
        Ok(records)
    }

    async fn project_runtime(&self, observation: ControllerRuntimeObservation) -> CoreResult<bool> {
        let observed_time = time_from_millis(observation.observed_at_unix_ms)?;
        let spec = EdgionControllerSpec {
            controller_id: observation.controller_id.to_string(),
            cluster: String::new(),
            environments: Vec::new(),
            tags: Vec::new(),
        };
        self.replace_status_with_retry(&observation.controller_id, spec, |current, generation| {
            let current = current?;
            if current.evicted
                || current.phase != EdgionControllerPhase::Online
                || current.session_id.as_deref() != Some(observation.session_id.as_str())
            {
                return None;
            }
            match observation.ownership_fence.as_ref() {
                Some(fence)
                    if current.ownership_epoch == fence.epoch
                        && current.ownership_token.as_deref() == Some(fence.token.as_str()) => {}
                None if current.ownership_epoch == 0 => {}
                _ => return None,
            }
            let mut projected = current.clone();
            if let Some(sync_version) = observation.sync_version {
                projected.sync_version = Some(sync_version);
            }
            if let Some(server_id) = observation.watch_server_id.as_ref() {
                projected.watch_server_id = Some(server_id.clone());
            }
            if let Some(resource_count) = observation.resource_count {
                projected.resource_count = Some(resource_count);
            }
            if let Some(updated_at) = observation.stats_updated_unix_ms {
                projected.stats_updated_unix_ms = Some(updated_at);
            }
            if let Some(updated_at) = observation.watch_updated_unix_ms {
                projected.watch_updated_unix_ms = Some(updated_at);
            }
            if observation.observed_at_unix_ms > projected.observed_at_unix_ms {
                projected.observed_at_unix_ms = observation.observed_at_unix_ms;
                projected.last_seen_time = observed_time.clone();
            }
            projected.observed_generation = generation;
            Some(projected)
        })
        .await
    }

    async fn evict(&self, id: &ControllerId) -> CoreResult<EvictionResult> {
        let name = controller_resource_name(id.as_str());
        for _ in 0..MAX_CONFLICT_RETRIES {
            let Some(mut resource) = self
                .resources
                .get(&name)
                .await
                .map_err(Self::adapter_error)?
            else {
                return Ok(EvictionResult {
                    outcome: EvictionOutcome::AlreadyAbsent,
                    target: None,
                });
            };
            Self::verify_identity(&resource, id)?;
            let Some(mut status) = resource.status.clone() else {
                return Ok(EvictionResult {
                    outcome: EvictionOutcome::AlreadyAbsent,
                    target: None,
                });
            };
            if status.evicted {
                return Ok(EvictionResult {
                    outcome: EvictionOutcome::AlreadyAbsent,
                    target: None,
                });
            }
            let target = EvictionTarget {
                session_id: status.session_id.clone().map(SessionId::new).transpose()?,
                connected_replica: status.connected_replica.clone(),
                ownership_fence: if status.ownership_epoch > 0 {
                    status
                        .ownership_token
                        .clone()
                        .map(|token| edgion_center_core::OwnershipFence {
                            token,
                            epoch: status.ownership_epoch,
                        })
                } else {
                    None
                },
            };
            status.evicted = true;
            status.phase = EdgionControllerPhase::Offline;
            status.session_id = None;
            status.connected_replica = None;
            resource.status = Some(status);
            match self.resources.replace_status(&name, &resource).await {
                Ok(_) => {
                    return Ok(EvictionResult {
                        outcome: EvictionOutcome::Evicted,
                        target: Some(target),
                    })
                }
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(format!(
            "controller eviction for {} remained conflicted after {} attempts",
            id, MAX_CONFLICT_RETRIES
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, pin::pin, sync::Mutex};

    use http::{Request, Response};
    use kube::client::Body;
    use kube::CustomResourceExt;
    use tower_test::mock;

    use super::*;

    #[derive(Default)]
    struct FakeResources {
        state: Mutex<FakeState>,
    }

    #[derive(Default)]
    struct FakeState {
        resources: BTreeMap<String, EdgionController>,
        revision: u64,
        status_conflicts_remaining: usize,
    }

    impl FakeResources {
        fn conflict_next_status_updates(&self, count: usize) {
            self.state.lock().unwrap().status_conflicts_remaining = count;
        }
    }

    #[async_trait]
    impl ControllerResources for FakeResources {
        async fn get(&self, name: &str) -> Result<Option<EdgionController>, ResourceError> {
            Ok(self.state.lock().unwrap().resources.get(name).cloned())
        }

        async fn create(
            &self,
            resource: &EdgionController,
        ) -> Result<EdgionController, ResourceError> {
            let mut state = self.state.lock().unwrap();
            let name = resource.name_any();
            if state.resources.contains_key(&name) {
                return Err(ResourceError::Conflict);
            }
            state.revision += 1;
            let mut created = resource.clone();
            created.status = None;
            created.metadata.resource_version = Some(state.revision.to_string());
            state.resources.insert(name, created.clone());
            Ok(created)
        }

        async fn replace(
            &self,
            name: &str,
            resource: &EdgionController,
        ) -> Result<EdgionController, ResourceError> {
            let mut state = self.state.lock().unwrap();
            let current = state
                .resources
                .get(name)
                .cloned()
                .ok_or_else(|| ResourceError::Other("missing".to_string()))?;
            if current.metadata.resource_version != resource.metadata.resource_version {
                return Err(ResourceError::Conflict);
            }
            state.revision += 1;
            let mut replaced = resource.clone();
            replaced.metadata.resource_version = Some(state.revision.to_string());
            replaced.metadata.generation = Some(
                current
                    .metadata
                    .generation
                    .unwrap_or(1)
                    .saturating_add(i64::from(current.spec != resource.spec)),
            );
            state.resources.insert(name.to_string(), replaced.clone());
            Ok(replaced)
        }

        async fn replace_status(
            &self,
            name: &str,
            resource: &EdgionController,
        ) -> Result<EdgionController, ResourceError> {
            let mut state = self.state.lock().unwrap();
            if state.status_conflicts_remaining > 0 {
                state.status_conflicts_remaining -= 1;
                return Err(ResourceError::Conflict);
            }
            let current = state
                .resources
                .get(name)
                .cloned()
                .ok_or_else(|| ResourceError::Other("missing".to_string()))?;
            if current.metadata.resource_version != resource.metadata.resource_version {
                return Err(ResourceError::Conflict);
            }
            state.revision += 1;
            let mut replaced = current;
            replaced.status = resource.status.clone();
            replaced.metadata.resource_version = Some(state.revision.to_string());
            state.resources.insert(name.to_string(), replaced.clone());
            Ok(replaced)
        }

        async fn list(&self) -> Result<Vec<EdgionController>, ResourceError> {
            Ok(self
                .state
                .lock()
                .unwrap()
                .resources
                .values()
                .cloned()
                .collect())
        }
    }

    fn registration(id: &str, session: &str, revision: i64) -> ControllerRegistration {
        ControllerRegistration {
            controller_id: ControllerId::new(id).unwrap(),
            session_id: SessionId::new(session).unwrap(),
            cluster: "cluster-a".to_string(),
            environments: vec!["prod".to_string()],
            tags: vec!["east".to_string()],
            connected_replica: Some("center-0".to_string()),
            ownership_fence: None,
            observed_at_unix_ms: revision,
        }
    }

    fn fenced_registration(
        id: &str,
        session: &str,
        epoch: u64,
        observed_at: i64,
    ) -> ControllerRegistration {
        ControllerRegistration {
            ownership_fence: Some(edgion_center_core::OwnershipFence {
                token: format!("token-{epoch}"),
                epoch,
            }),
            ..registration(id, session, observed_at)
        }
    }

    #[test]
    fn generated_names_are_dns_safe_bounded_and_collision_resistant() {
        let first = controller_resource_name("Cluster_A/controller/0");
        let second = controller_resource_name("cluster-a-controller-0");
        assert!(first.len() <= 63);
        assert!(first
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        assert_ne!(first, second);
        assert_eq!(first, controller_resource_name("Cluster_A/controller/0"));
    }

    #[test]
    fn generated_crd_has_status_subresource_and_expected_scope() {
        let crd = EdgionController::crd();
        assert_eq!(crd.spec.scope, "Namespaced");
        assert_eq!(crd.spec.group, "center.edgion.io");
        let version = &crd.spec.versions[0];
        assert_eq!(version.name, "v1alpha1");
        assert!(version
            .subresources
            .as_ref()
            .and_then(|s| s.status.as_ref())
            .is_some());
        let schema = serde_json::to_value(crd).unwrap();
        assert!(schema
            .pointer("/spec/versions/0/schema/openAPIV3Schema/properties/spec/additionalProperties")
            .is_none());
        assert_eq!(
            schema.pointer("/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/controllerId/maxLength"),
            Some(&serde_json::json!(253))
        );
        assert_eq!(
            schema.pointer("/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/lastSeenTime/format"),
            Some(&serde_json::Value::String("date-time".to_string()))
        );
        let validations = schema
            .pointer("/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/controllerId/x-kubernetes-validations")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(validations
            .iter()
            .any(|value| value["rule"] == "self != ''"));
        assert!(validations
            .iter()
            .any(|value| value["rule"] == "self == oldSelf"));
    }

    #[tokio::test]
    async fn directory_retries_conflicts_and_fences_stale_sessions() {
        let resources = Arc::new(FakeResources::default());
        resources.conflict_next_status_updates(2);
        let directory = KubernetesControllerDirectory::with_resources(resources);
        directory
            .upsert_registration(registration("c1", "s1", 10))
            .await
            .unwrap();
        directory
            .upsert_registration(registration("c1", "s2", 20))
            .await
            .unwrap();
        assert!(directory
            .upsert_registration(registration("c1", "stale", 19))
            .await
            .is_err());
        assert_eq!(
            directory
                .mark_offline(
                    &ControllerId::new("c1").unwrap(),
                    &SessionId::new("s1").unwrap(),
                    None,
                    10
                )
                .await
                .unwrap(),
            OfflineOutcome::NotCurrent
        );
        let listed = directory.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].current_session_id.as_ref().unwrap().as_str(),
            "s2"
        );
    }

    #[tokio::test]
    async fn registration_refreshes_operator_visible_spec_metadata() {
        let resources = Arc::new(FakeResources::default());
        let directory = KubernetesControllerDirectory::with_resources(resources.clone());
        directory
            .upsert_registration(registration("c1", "s1", 10))
            .await
            .unwrap();
        let mut updated = registration("c1", "s2", 20);
        updated.cluster = "cluster-b".to_string();
        updated.environments = vec!["staging".to_string()];
        updated.tags = vec!["west".to_string()];
        directory.upsert_registration(updated).await.unwrap();

        let resource = resources
            .get(&controller_resource_name("c1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource.spec.cluster, "cluster-b");
        assert_eq!(resource.spec.environments, vec!["staging"]);
        assert_eq!(resource.spec.tags, vec!["west"]);
        assert_eq!(resource.metadata.generation, Some(2));
    }

    #[tokio::test]
    async fn runtime_projection_is_session_and_ownership_fenced() {
        let directory =
            KubernetesControllerDirectory::with_resources(Arc::new(FakeResources::default()));
        let id = ControllerId::new("c1").unwrap();
        directory
            .upsert_registration(fenced_registration("c1", "current", 2, 10))
            .await
            .unwrap();

        let stale = ControllerRuntimeObservation {
            controller_id: id.clone(),
            session_id: SessionId::new("old").unwrap(),
            ownership_fence: Some(edgion_center_core::OwnershipFence {
                token: "token-1".to_string(),
                epoch: 1,
            }),
            sync_version: Some(99),
            watch_server_id: Some("stale-server".to_string()),
            resource_count: Some(99),
            stats_updated_unix_ms: Some(20),
            watch_updated_unix_ms: Some(20),
            observed_at_unix_ms: 20,
        };
        assert!(!directory.project_runtime(stale).await.unwrap());

        let current = ControllerRuntimeObservation {
            controller_id: id,
            session_id: SessionId::new("current").unwrap(),
            ownership_fence: Some(edgion_center_core::OwnershipFence {
                token: "token-2".to_string(),
                epoch: 2,
            }),
            sync_version: Some(7),
            watch_server_id: Some("server-7".to_string()),
            resource_count: Some(42),
            stats_updated_unix_ms: Some(25),
            watch_updated_unix_ms: Some(30),
            observed_at_unix_ms: 30,
        };
        assert!(directory.project_runtime(current).await.unwrap());
        let record = directory.list().await.unwrap().remove(0);
        assert_eq!(record.sync_version, Some(7));
        assert_eq!(record.watch_server_id.as_deref(), Some("server-7"));
        assert_eq!(record.resource_count, Some(42));
        assert_eq!(record.stats_updated_unix_ms, Some(25));
        assert_eq!(record.watch_updated_unix_ms, Some(30));
        assert_eq!(record.last_seen_unix_ms, 30);
    }

    #[tokio::test]
    async fn offline_tombstone_and_eviction_require_strictly_newer_registration() {
        let resources = Arc::new(FakeResources::default());
        let directory = KubernetesControllerDirectory::with_resources(resources.clone());
        let id = ControllerId::new("c1").unwrap();
        let session = SessionId::new("s1").unwrap();
        assert_eq!(
            directory
                .mark_offline(&id, &session, None, 10)
                .await
                .unwrap(),
            OfflineOutcome::Marked
        );
        assert!(directory
            .upsert_registration(registration("c1", "s1", 10))
            .await
            .is_err());
        assert_eq!(
            directory.list().await.unwrap()[0].phase,
            ControllerPhase::Offline
        );
        directory
            .upsert_registration(registration("c1", "s2", 11))
            .await
            .unwrap();
        assert_eq!(
            directory.list().await.unwrap()[0].phase,
            ControllerPhase::Online
        );
        assert_eq!(
            directory.evict(&id).await.unwrap().outcome,
            EvictionOutcome::Evicted
        );
        assert!(directory.list().await.unwrap().is_empty());
        assert_eq!(
            directory.evict(&id).await.unwrap().outcome,
            EvictionOutcome::AlreadyAbsent
        );
        assert!(directory
            .upsert_registration(registration("c1", "s2", 11))
            .await
            .is_err());
        assert!(directory.list().await.unwrap().is_empty());
        directory
            .upsert_registration(registration("c1", "s3", 12))
            .await
            .unwrap();
        assert_eq!(
            directory.list().await.unwrap()[0].phase,
            ControllerPhase::Online
        );
    }

    #[tokio::test]
    async fn fencing_epoch_beats_skewed_wall_time_and_stale_offline_never_mutates() {
        let directory =
            KubernetesControllerDirectory::with_resources(Arc::new(FakeResources::default()));
        let id = ControllerId::new("c1").unwrap();
        directory
            .upsert_registration(fenced_registration("c1", "old", 1, 1_000_000))
            .await
            .unwrap();
        directory
            .upsert_registration(fenced_registration("c1", "new", 2, 10))
            .await
            .unwrap();
        let stale_fence = edgion_center_core::OwnershipFence {
            token: "token-1".to_string(),
            epoch: 1,
        };
        assert_eq!(
            directory
                .mark_offline(
                    &id,
                    &SessionId::new("old").unwrap(),
                    Some(&stale_fence),
                    2_000_000,
                )
                .await
                .unwrap(),
            OfflineOutcome::NotCurrent
        );
        let records = directory.list().await.unwrap();
        assert_eq!(records[0].phase, ControllerPhase::Online);
        assert_eq!(
            records[0].current_session_id.as_ref().unwrap().as_str(),
            "new"
        );
        let eviction = directory.evict(&id).await.unwrap();
        let target = eviction.target.expect("online fenced target");
        assert_eq!(target.session_id.as_ref().unwrap().as_str(), "new");
        assert_eq!(target.ownership_fence.unwrap().epoch, 2);
    }

    #[tokio::test]
    async fn newer_fenced_offline_supersedes_old_projection_after_ambiguous_upsert() {
        let directory =
            KubernetesControllerDirectory::with_resources(Arc::new(FakeResources::default()));
        let id = ControllerId::new("c1").unwrap();
        directory
            .upsert_registration(fenced_registration("c1", "old", 1, 1_000))
            .await
            .unwrap();
        let new_fence = edgion_center_core::OwnershipFence {
            token: "token-2".to_string(),
            epoch: 2,
        };
        assert_eq!(
            directory
                .mark_offline(&id, &SessionId::new("new").unwrap(), Some(&new_fence), 10,)
                .await
                .unwrap(),
            OfflineOutcome::Marked
        );
        assert_eq!(
            directory.list().await.unwrap()[0].phase,
            ControllerPhase::Offline
        );
    }

    #[tokio::test]
    async fn deterministic_name_collision_fails_closed() {
        let resources = Arc::new(FakeResources::default());
        let name = controller_resource_name("c1");
        let forged = EdgionController::new(
            &name,
            EdgionControllerSpec {
                controller_id: "different".to_string(),
                cluster: String::new(),
                environments: Vec::new(),
                tags: Vec::new(),
            },
        );
        resources.create(&forged).await.unwrap();
        let directory = KubernetesControllerDirectory::with_resources(resources);
        let error = directory
            .upsert_registration(registration("c1", "s1", 1))
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::Conflict(_)));
    }

    #[tokio::test]
    async fn real_kube_client_uses_namespaced_resource_and_status_subresource() {
        let (service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let server = tokio::spawn(async move {
            let mut handle = pin!(handle);
            let name = controller_resource_name("c1");

            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), http::Method::GET);
            assert_eq!(
                request.uri().path(),
                format!("/apis/center.edgion.io/v1alpha1/namespaces/management/edgioncontrollers/{name}")
            );
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "apiVersion": "v1",
                            "kind": "Status",
                            "status": "Failure",
                            "reason": "NotFound",
                            "message": "not found",
                            "code": 404
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            );

            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), http::Method::POST);
            assert_eq!(
                request.uri().path(),
                "/apis/center.edgion.io/v1alpha1/namespaces/management/edgioncontrollers"
            );
            let mut created = EdgionController::new(
                &name,
                EdgionControllerSpec {
                    controller_id: "c1".to_string(),
                    cluster: "cluster-a".to_string(),
                    environments: vec!["prod".to_string()],
                    tags: vec!["east".to_string()],
                },
            );
            created.metadata.resource_version = Some("1".to_string());
            send.send_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&created).unwrap()))
                    .unwrap(),
            );

            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), http::Method::PUT);
            assert_eq!(
                request.uri().path(),
                format!("/apis/center.edgion.io/v1alpha1/namespaces/management/edgioncontrollers/{name}/status")
            );
            created.status = Some(EdgionControllerStatus {
                phase: EdgionControllerPhase::Online,
                session_id: Some("s1".to_string()),
                cluster: "cluster-a".to_string(),
                environments: vec!["prod".to_string()],
                tags: vec!["east".to_string()],
                connected_replica: Some("center-0".to_string()),
                ownership_token: None,
                ownership_epoch: 0,
                sync_version: None,
                watch_server_id: None,
                resource_count: None,
                stats_updated_unix_ms: None,
                watch_updated_unix_ms: None,
                last_seen_time: time_from_millis(10).unwrap(),
                observed_at_unix_ms: 10,
                evicted: false,
                observed_generation: None,
            });
            created.metadata.resource_version = Some("2".to_string());
            send.send_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&created).unwrap()))
                    .unwrap(),
            );
        });

        let client = Client::new(service, "management");
        let directory = KubernetesControllerDirectory::new(client, "management");
        directory
            .upsert_registration(registration("c1", "s1", 10))
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn out_of_range_observation_time_fails_closed() {
        let directory =
            KubernetesControllerDirectory::with_resources(Arc::new(FakeResources::default()));
        let error = directory
            .upsert_registration(registration("c1", "s1", i64::MAX))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("outside RFC3339 range"));
    }
}
