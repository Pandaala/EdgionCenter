use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use edgion_center_core::{
    validate_write, CapabilityDiscoveryFence, CapabilitySnapshotKey, CapabilitySnapshotStore,
    CapabilityStoreWrite, CloudResourceId, CoreError, CoreResult, DiscoveryToken,
    ProviderCapabilitySnapshot,
};
use kube::{api::PostParams, Api, Client};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::cloud_capability_crd::{
    EdgionCapabilityAuthorityStatus, EdgionProviderCapabilitySnapshot,
    EdgionProviderCapabilitySnapshotSpec, EdgionProviderCapabilitySnapshotStatus,
};

const MAX_CONFLICT_RETRIES: usize = 8;
const ACCOUNT_LABEL: &str = "center.edgion.io/provider-account-hash";

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
trait CapabilityResources: Send + Sync {
    async fn get(
        &self,
        name: &str,
    ) -> Result<Option<EdgionProviderCapabilitySnapshot>, ResourceError>;
    async fn list(&self) -> Result<Vec<EdgionProviderCapabilitySnapshot>, ResourceError>;
    async fn create(
        &self,
        value: &EdgionProviderCapabilitySnapshot,
    ) -> Result<EdgionProviderCapabilitySnapshot, ResourceError>;
    async fn replace_status(
        &self,
        name: &str,
        value: &EdgionProviderCapabilitySnapshot,
    ) -> Result<EdgionProviderCapabilitySnapshot, ResourceError>;
}

struct KubernetesCapabilityResources {
    snapshots: Api<EdgionProviderCapabilitySnapshot>,
}

#[async_trait]
impl CapabilityResources for KubernetesCapabilityResources {
    async fn get(
        &self,
        name: &str,
    ) -> Result<Option<EdgionProviderCapabilitySnapshot>, ResourceError> {
        self.snapshots.get_opt(name).await.map_err(Into::into)
    }

    async fn list(&self) -> Result<Vec<EdgionProviderCapabilitySnapshot>, ResourceError> {
        self.snapshots
            .list(&Default::default())
            .await
            .map(|list| list.items)
            .map_err(Into::into)
    }

    async fn create(
        &self,
        value: &EdgionProviderCapabilitySnapshot,
    ) -> Result<EdgionProviderCapabilitySnapshot, ResourceError> {
        self.snapshots
            .create(&PostParams::default(), value)
            .await
            .map_err(Into::into)
    }

    async fn replace_status(
        &self,
        name: &str,
        value: &EdgionProviderCapabilitySnapshot,
    ) -> Result<EdgionProviderCapabilitySnapshot, ResourceError> {
        let body =
            serde_json::to_vec(value).map_err(|error| ResourceError::Other(error.to_string()))?;
        self.snapshots
            .replace_status(name, &PostParams::default(), body)
            .await
            .map_err(Into::into)
    }
}

/// Kubernetes-native capability snapshot storage.
///
/// One CRD contains both the active discovery authority and its snapshot.
/// Kubernetes `resourceVersion` therefore provides the only CAS needed across
/// replicas; this store deliberately does not coordinate through a Lease.
#[derive(Clone)]
pub struct KubernetesCapabilitySnapshotStore {
    resources: Arc<dyn CapabilityResources>,
}

impl KubernetesCapabilitySnapshotStore {
    pub fn new(client: Client, namespace: &str) -> CoreResult<Self> {
        if namespace.trim().is_empty() || namespace.chars().any(char::is_control) {
            return Err(CoreError::Adapter(
                "Kubernetes capability snapshot namespace must be non-empty".to_string(),
            ));
        }
        Ok(Self {
            resources: Arc::new(KubernetesCapabilityResources {
                snapshots: Api::namespaced(client, namespace),
            }),
        })
    }

    #[cfg(test)]
    fn with_resources(resources: Arc<dyn CapabilityResources>) -> Self {
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

    fn verify_key(
        resource: &EdgionProviderCapabilitySnapshot,
        expected: &CapabilitySnapshotKey,
    ) -> CoreResult<()> {
        let actual = resource.spec.to_key().map_err(|message| {
            CoreError::Conflict(format!("invalid capability snapshot CRD key: {message}"))
        })?;
        if &actual != expected {
            return Err(CoreError::Conflict(
                "capability snapshot CRD name collision".to_string(),
            ));
        }
        Ok(())
    }

    fn authority(
        status: &EdgionProviderCapabilitySnapshotStatus,
    ) -> CoreResult<Option<CapabilityDiscoveryFence>> {
        if status.last_discovery_epoch < 0 {
            return Err(CoreError::Conflict(
                "capability snapshot CRD has a negative discovery epoch".to_string(),
            ));
        }
        let authority = status
            .authority
            .as_ref()
            .map(EdgionCapabilityAuthorityStatus::to_core)
            .transpose()
            .map_err(|message| {
                CoreError::Conflict(format!("invalid capability snapshot authority: {message}"))
            })?;
        if authority
            .as_ref()
            .is_some_and(|fence| fence.discovery_epoch as i64 != status.last_discovery_epoch)
        {
            return Err(CoreError::Conflict(
                "capability snapshot authority is not the latest epoch".to_string(),
            ));
        }
        Ok(authority)
    }

    fn snapshot(
        key: &CapabilitySnapshotKey,
        status: &EdgionProviderCapabilitySnapshotStatus,
    ) -> CoreResult<Option<ProviderCapabilitySnapshot>> {
        let Some(value) = status.snapshot_json.as_ref() else {
            return Ok(None);
        };
        let snapshot: ProviderCapabilitySnapshot = serde_json::from_str(value).map_err(|_| {
            CoreError::Conflict("capability snapshot CRD contains an invalid payload".to_string())
        })?;
        validate_write(key, &snapshot.fence.clone(), &snapshot)?;
        Ok(Some(snapshot))
    }

    fn new_resource(name: &str, key: &CapabilitySnapshotKey) -> EdgionProviderCapabilitySnapshot {
        let mut resource = EdgionProviderCapabilitySnapshot::new(
            name,
            EdgionProviderCapabilitySnapshotSpec::from_key(key),
        );
        resource.metadata.labels = Some(BTreeMap::from([(
            ACCOUNT_LABEL.to_string(),
            digest(key.provider_account_id.as_str()),
        )]));
        resource
    }

    async fn load_for_key(
        &self,
        key: &CapabilitySnapshotKey,
    ) -> CoreResult<Option<EdgionProviderCapabilitySnapshot>> {
        let name = capability_snapshot_resource_name(key)?;
        let resource = self
            .resources
            .get(&name)
            .await
            .map_err(Self::adapter_error)?;
        if let Some(resource) = resource.as_ref() {
            Self::verify_key(resource, key)?;
        }
        Ok(resource)
    }

    async fn invalidate_key(
        &self,
        key: &CapabilitySnapshotKey,
        generation: u64,
        revision: Option<&str>,
    ) -> CoreResult<()> {
        let name = capability_snapshot_resource_name(key)?;
        for _ in 0..MAX_CONFLICT_RETRIES {
            let Some(mut resource) = self
                .resources
                .get(&name)
                .await
                .map_err(Self::adapter_error)?
            else {
                return Ok(());
            };
            Self::verify_key(&resource, key)?;
            let Some(status) = resource.status.as_mut() else {
                return Ok(());
            };
            let active_matches = match Self::authority(status) {
                Ok(authority) => authority.as_ref().is_some_and(|fence| {
                    fence.provider_account_generation == generation
                        && fence.credential_revision.as_deref() == revision
                }),
                // Malformed authority is unusable and safe to revoke while
                // repairing this account's coordination state.
                Err(_) => true,
            };
            let snapshot_matches = match status.snapshot_json.as_ref() {
                Some(value) => {
                    match serde_json::from_str::<ProviderCapabilitySnapshot>(value) {
                        Ok(snapshot) => {
                            snapshot.fence.provider_account_generation == generation
                                && snapshot.fence.credential_revision.as_deref() == revision
                        }
                        // A corrupt payload must not prevent revocation of an
                        // exactly stale active writer. It is safe to discard
                        // because it cannot be a valid committed snapshot.
                        Err(_) => true,
                    }
                }
                None => false,
            };
            if !active_matches && !snapshot_matches {
                return Ok(());
            }
            if active_matches {
                // Revoking the authority fences a discovery that was already
                // in flight when credential rotation was observed. A negative
                // epoch can only come from legacy or externally corrupted
                // status, so normalize it to the empty-state baseline.
                status.authority = None;
                status.last_discovery_epoch = if status.last_discovery_epoch < 0 {
                    0
                } else {
                    status.last_discovery_epoch.checked_add(1).ok_or_else(|| {
                        CoreError::Conflict("capability discovery epoch exhausted".to_string())
                    })?
                };
            }
            if snapshot_matches {
                status.snapshot_json = None;
            }
            match self.resources.replace_status(&name, &resource).await {
                Ok(_) => return Ok(()),
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(
            "capability snapshot invalidation remained conflicted".to_string(),
        ))
    }
}

#[async_trait]
impl CapabilitySnapshotStore for KubernetesCapabilitySnapshotStore {
    async fn get(
        &self,
        key: &CapabilitySnapshotKey,
    ) -> CoreResult<Option<ProviderCapabilitySnapshot>> {
        key.validate()?;
        let Some(resource) = self.load_for_key(key).await? else {
            return Ok(None);
        };
        let Some(status) = resource.status.as_ref() else {
            return Ok(None);
        };
        Self::snapshot(key, status)
    }

    async fn begin_discovery(
        &self,
        key: &CapabilitySnapshotKey,
        provider_account_generation: u64,
        credential_revision: Option<&str>,
    ) -> CoreResult<CapabilityDiscoveryFence> {
        key.validate()?;
        let name = capability_snapshot_resource_name(key)?;
        for _ in 0..MAX_CONFLICT_RETRIES {
            let mut resource = match self
                .resources
                .get(&name)
                .await
                .map_err(Self::adapter_error)?
            {
                Some(resource) => {
                    Self::verify_key(&resource, key)?;
                    resource
                }
                None => match self.resources.create(&Self::new_resource(&name, key)).await {
                    Ok(resource) => resource,
                    Err(ResourceError::Conflict) => continue,
                    Err(error) => return Err(Self::adapter_error(error)),
                },
            };
            let last_epoch = resource
                .status
                .as_ref()
                .map(|status| {
                    if status.last_discovery_epoch < 0 {
                        return Err(CoreError::Conflict(
                            "capability discovery epoch is negative".to_string(),
                        ));
                    }
                    u64::try_from(status.last_discovery_epoch).map_err(|_| {
                        CoreError::Conflict("capability discovery epoch is negative".to_string())
                    })
                })
                .transpose()?
                .unwrap_or_default();
            let discovery_epoch = last_epoch.checked_add(1).ok_or_else(|| {
                CoreError::Conflict("capability discovery epoch exhausted".to_string())
            })?;
            let fence = CapabilityDiscoveryFence {
                provider_account_generation,
                credential_revision: credential_revision.map(str::to_string),
                discovery_epoch,
                discovery_token: DiscoveryToken::new(Uuid::new_v4().to_string())?,
            };
            fence.validate()?;
            let committed_snapshot = resource
                .status
                .as_ref()
                .and_then(|status| status.snapshot_json.clone());
            resource.status = Some(EdgionProviderCapabilitySnapshotStatus {
                last_discovery_epoch: i64::try_from(discovery_epoch).map_err(|_| {
                    CoreError::Conflict("capability discovery epoch exhausted".to_string())
                })?,
                authority: Some(EdgionCapabilityAuthorityStatus::from_core(&fence)),
                snapshot_json: committed_snapshot,
            });
            match self.resources.replace_status(&name, &resource).await {
                Ok(_) => return Ok(fence),
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(
            "capability discovery begin remained conflicted".to_string(),
        ))
    }

    async fn put_if_current(
        &self,
        key: &CapabilitySnapshotKey,
        expected_fence: &CapabilityDiscoveryFence,
        snapshot: &ProviderCapabilitySnapshot,
    ) -> CoreResult<CapabilityStoreWrite> {
        validate_write(key, expected_fence, snapshot)?;
        let payload = serde_json::to_string(snapshot)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        let name = capability_snapshot_resource_name(key)?;
        for _ in 0..MAX_CONFLICT_RETRIES {
            let Some(mut resource) = self.load_for_key(key).await? else {
                return Ok(CapabilityStoreWrite::FenceLost);
            };
            let Some(status) = resource.status.as_mut() else {
                return Ok(CapabilityStoreWrite::FenceLost);
            };
            if Self::authority(status)?.as_ref() != Some(expected_fence) {
                return Ok(CapabilityStoreWrite::FenceLost);
            }
            if let Ok(Some(existing)) = Self::snapshot(key, status) {
                if existing.fence == *expected_fence {
                    if &existing == snapshot {
                        return Ok(CapabilityStoreWrite::Stored);
                    }
                    return Err(CoreError::Conflict(
                        "capability discovery fence cannot commit two different snapshots"
                            .to_string(),
                    ));
                }
            }
            status.snapshot_json = Some(payload.clone());
            match self.resources.replace_status(&name, &resource).await {
                Ok(_) => return Ok(CapabilityStoreWrite::Stored),
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(
            "capability snapshot write remained conflicted".to_string(),
        ))
    }

    async fn invalidate_account_revision(
        &self,
        account_id: &CloudResourceId,
        stale_provider_account_generation: u64,
        stale_credential_revision: Option<&str>,
    ) -> CoreResult<()> {
        account_id.validate()?;
        CapabilityDiscoveryFence {
            provider_account_generation: stale_provider_account_generation,
            credential_revision: stale_credential_revision.map(str::to_string),
            discovery_epoch: 1,
            discovery_token: DiscoveryToken::new("invalidation-validation")?,
        }
        .validate()?;
        // List all objects for correctness. A mutable metadata label cannot be
        // the sole authority for exact stale-revision invalidation.
        let resources = self.resources.list().await.map_err(Self::adapter_error)?;
        let mut first_error = None;
        for resource in resources {
            if resource.spec.provider_account_id != account_id.as_str() {
                continue;
            }
            let key = match resource.spec.to_key() {
                Ok(key) => key,
                Err(message) => {
                    first_error.get_or_insert_with(|| {
                        CoreError::Conflict(format!(
                            "invalid capability snapshot CRD key: {message}"
                        ))
                    });
                    continue;
                }
            };
            if &key.provider_account_id != account_id {
                first_error.get_or_insert_with(|| {
                    CoreError::Conflict(
                        "capability snapshot account identity is inconsistent".to_string(),
                    )
                });
                continue;
            }
            if let Err(error) = self
                .invalidate_key(
                    &key,
                    stale_provider_account_generation,
                    stale_credential_revision,
                )
                .await
            {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

pub fn capability_snapshot_resource_name(key: &CapabilitySnapshotKey) -> CoreResult<String> {
    key.validate()?;
    let encoded = serde_json::to_vec(key).map_err(|error| CoreError::Adapter(error.to_string()))?;
    Ok(format!("cloudcap-{}", digest(&encoded)))
}

fn digest(value: impl AsRef<[u8]>) -> String {
    let encoded = hex::encode(Sha256::digest(value.as_ref()));
    encoded[..32].to_string()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
    };

    use edgion_center_core::{
        cloud_test_support::{
            assert_exact_revision_invalidation, assert_roundtrip_and_fencing,
            assert_scope_isolation,
        },
        CapabilityScope, ProviderRegion,
    };
    use kube::ResourceExt;
    use tokio::sync::Barrier;

    use super::*;

    #[derive(Default)]
    struct MemoryResources {
        values: Mutex<HashMap<String, EdgionProviderCapabilitySnapshot>>,
        revision: Mutex<u64>,
        replace_gate: Mutex<Option<(Arc<Barrier>, usize)>>,
        conflicts: AtomicUsize,
    }

    impl MemoryResources {
        fn next_revision(&self) -> String {
            let mut revision = self.revision.lock().unwrap();
            *revision += 1;
            revision.to_string()
        }

        fn synchronize_next_replaces(&self, participants: usize) {
            *self.replace_gate.lock().unwrap() =
                Some((Arc::new(Barrier::new(participants)), participants));
        }

        fn conflict_count(&self) -> usize {
            self.conflicts.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl CapabilityResources for MemoryResources {
        async fn get(
            &self,
            name: &str,
        ) -> Result<Option<EdgionProviderCapabilitySnapshot>, ResourceError> {
            Ok(self.values.lock().unwrap().get(name).cloned())
        }

        async fn list(&self) -> Result<Vec<EdgionProviderCapabilitySnapshot>, ResourceError> {
            Ok(self.values.lock().unwrap().values().cloned().collect())
        }

        async fn create(
            &self,
            value: &EdgionProviderCapabilitySnapshot,
        ) -> Result<EdgionProviderCapabilitySnapshot, ResourceError> {
            let name = value.name_any();
            let mut values = self.values.lock().unwrap();
            if values.contains_key(&name) {
                return Err(ResourceError::Conflict);
            }
            let mut value = value.clone();
            value.metadata.resource_version = Some(self.next_revision());
            values.insert(name, value.clone());
            Ok(value)
        }

        async fn replace_status(
            &self,
            name: &str,
            value: &EdgionProviderCapabilitySnapshot,
        ) -> Result<EdgionProviderCapabilitySnapshot, ResourceError> {
            let barrier = {
                let mut gate = self.replace_gate.lock().unwrap();
                match gate.as_mut() {
                    Some((barrier, remaining)) => {
                        let barrier = barrier.clone();
                        *remaining -= 1;
                        if *remaining == 0 {
                            *gate = None;
                        }
                        Some(barrier)
                    }
                    None => None,
                }
            };
            if let Some(barrier) = barrier {
                barrier.wait().await;
            }
            let mut values = self.values.lock().unwrap();
            let Some(current) = values.get(name) else {
                return Err(ResourceError::Conflict);
            };
            if current.metadata.resource_version != value.metadata.resource_version {
                self.conflicts.fetch_add(1, Ordering::Relaxed);
                return Err(ResourceError::Conflict);
            }
            let mut value = value.clone();
            value.metadata.resource_version = Some(self.next_revision());
            values.insert(name.to_string(), value.clone());
            Ok(value)
        }
    }

    fn store() -> (KubernetesCapabilitySnapshotStore, Arc<MemoryResources>) {
        let resources = Arc::new(MemoryResources::default());
        (
            KubernetesCapabilitySnapshotStore::with_resources(resources.clone()),
            resources,
        )
    }

    #[test]
    fn names_are_bounded_deterministic_and_scope_specific() {
        let account = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("Tenant/A").unwrap(),
            scope: CapabilityScope::Account,
        };
        let region = CapabilitySnapshotKey {
            provider_account_id: account.provider_account_id.clone(),
            scope: CapabilityScope::Region {
                region: ProviderRegion::new("us-east-1").unwrap(),
            },
        };
        assert_eq!(
            capability_snapshot_resource_name(&account).unwrap(),
            capability_snapshot_resource_name(&account).unwrap()
        );
        assert_ne!(
            capability_snapshot_resource_name(&account).unwrap(),
            capability_snapshot_resource_name(&region).unwrap()
        );
        assert!(capability_snapshot_resource_name(&account).unwrap().len() <= 63);
    }

    #[tokio::test]
    async fn shared_conformance_passes() {
        let (store, _) = store();
        assert_roundtrip_and_fencing(&store, "kube-a").await;
        assert_scope_isolation(&store, "kube-b").await;
        assert_exact_revision_invalidation(&store, "kube-c").await;
    }

    #[tokio::test]
    async fn one_fence_cannot_commit_two_different_results() {
        let (store, _) = store();
        assert_roundtrip_and_fencing(&store, "kube-immutable").await;
        let key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("kube-immutable-roundtrip").unwrap(),
            scope: CapabilityScope::Account,
        };
        let committed = store.get(&key).await.unwrap().unwrap();
        assert_eq!(
            store
                .put_if_current(&key, &committed.fence, &committed)
                .await
                .unwrap(),
            CapabilityStoreWrite::Stored
        );
        let mut different = committed.clone();
        different.discovered_at_unix_ms += 1;
        assert!(store
            .put_if_current(&key, &different.fence, &different)
            .await
            .is_err());
        assert_eq!(store.get(&key).await.unwrap(), Some(committed));
    }

    #[tokio::test]
    async fn full_key_collision_is_rejected() {
        let (store, resources) = store();
        let expected = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("collision-a").unwrap(),
            scope: CapabilityScope::Account,
        };
        let other = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("collision-b").unwrap(),
            scope: CapabilityScope::Account,
        };
        let name = capability_snapshot_resource_name(&expected).unwrap();
        let mut forged = KubernetesCapabilitySnapshotStore::new_resource(&name, &other);
        forged.metadata.resource_version = Some("1".to_string());
        resources.values.lock().unwrap().insert(name, forged);
        assert!(store.get(&expected).await.is_err());
        assert!(store.begin_discovery(&expected, 1, None).await.is_err());
    }

    #[tokio::test]
    async fn malformed_committed_payload_can_be_replaced_by_a_new_authority() {
        let (store, resources) = store();
        assert_roundtrip_and_fencing(&store, "kube-recovery").await;
        let key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("kube-recovery-roundtrip").unwrap(),
            scope: CapabilityScope::Account,
        };
        let committed = store.get(&key).await.unwrap().unwrap();
        let name = capability_snapshot_resource_name(&key).unwrap();
        resources
            .values
            .lock()
            .unwrap()
            .get_mut(&name)
            .unwrap()
            .status
            .as_mut()
            .unwrap()
            .snapshot_json = Some("{not-json".to_string());
        assert!(store.get(&key).await.is_err());

        let fence = store
            .begin_discovery(&key, 3, Some("recovered"))
            .await
            .unwrap();
        let mut recovered = committed;
        recovered.fence = fence.clone();
        recovered.discovered_at_unix_ms += 1;
        assert_eq!(
            store
                .put_if_current(&key, &fence, &recovered)
                .await
                .unwrap(),
            CapabilityStoreWrite::Stored
        );
        assert_eq!(store.get(&key).await.unwrap(), Some(recovered));
    }

    #[tokio::test]
    async fn malformed_object_does_not_prevent_other_scope_invalidation() {
        let (store, resources) = store();
        assert_roundtrip_and_fencing(&store, "kube-isolation").await;
        let key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("kube-isolation-roundtrip").unwrap(),
            scope: CapabilityScope::Account,
        };
        let mut malformed = KubernetesCapabilitySnapshotStore::new_resource("malformed", &key);
        malformed.spec.scope.scope_type = "invalid".to_string();
        malformed.metadata.resource_version = Some("malformed-rv".to_string());
        resources
            .values
            .lock()
            .unwrap()
            .insert("malformed".to_string(), malformed);

        assert!(store
            .invalidate_account_revision(&key.provider_account_id, 2, Some("revision-b"))
            .await
            .is_err());
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn invalidation_repairs_a_negative_discovery_epoch() {
        let (store, resources) = store();
        let key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("kube-negative-epoch").unwrap(),
            scope: CapabilityScope::Account,
        };
        store.begin_discovery(&key, 1, None).await.unwrap();
        let name = capability_snapshot_resource_name(&key).unwrap();
        resources
            .values
            .lock()
            .unwrap()
            .get_mut(&name)
            .unwrap()
            .status
            .as_mut()
            .unwrap()
            .last_discovery_epoch = -100;

        store
            .invalidate_account_revision(&key.provider_account_id, 1, None)
            .await
            .unwrap();
        let repaired = store.begin_discovery(&key, 2, None).await.unwrap();
        assert_eq!(repaired.discovery_epoch, 1);
    }

    #[tokio::test]
    async fn concurrent_begins_allocate_unique_epochs_and_only_latest_writes() {
        let (store, resources) = store();
        assert_roundtrip_and_fencing(&store, "kube-concurrent").await;
        let key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("kube-concurrent-roundtrip").unwrap(),
            scope: CapabilityScope::Account,
        };
        let template = store.get(&key).await.unwrap().unwrap();
        resources.synchronize_next_replaces(16);
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let store = store.clone();
            let key = key.clone();
            tasks.push(tokio::spawn(async move {
                store.begin_discovery(&key, 3, Some("concurrent")).await
            }));
        }
        let mut fences = Vec::new();
        for task in tasks {
            fences.push(task.await.unwrap().unwrap());
        }
        assert!(resources.conflict_count() >= 15);
        fences.sort_by_key(|fence| fence.discovery_epoch);
        for pair in fences.windows(2) {
            assert_eq!(pair[1].discovery_epoch, pair[0].discovery_epoch + 1);
            assert_ne!(pair[1].discovery_token, pair[0].discovery_token);
        }

        let winner = fences.pop().unwrap();
        for stale in fences {
            let mut value = template.clone();
            value.fence = stale.clone();
            value.discovered_at_unix_ms = 20_000 + stale.discovery_epoch as i64;
            assert_eq!(
                store.put_if_current(&key, &stale, &value).await.unwrap(),
                CapabilityStoreWrite::FenceLost
            );
        }
        let mut value = template;
        value.fence = winner.clone();
        value.discovered_at_unix_ms = 20_000 + winner.discovery_epoch as i64;
        assert_eq!(
            store.put_if_current(&key, &winner, &value).await.unwrap(),
            CapabilityStoreWrite::Stored
        );
    }
}
