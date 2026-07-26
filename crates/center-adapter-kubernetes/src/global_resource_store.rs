use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use edgion_center_core::{
    global_resource_for_create, global_resource_for_replace, CoreError, CoreResult, GlobalResource,
    GlobalResourceCreateResult, GlobalResourceDesired, GlobalResourceId, GlobalResourcePage,
    GlobalResourcePageRequest, GlobalResourceReplaceResult, GlobalResourceStore,
    GlobalResourcesConfig,
};
use kube::{
    api::{ListParams, PostParams},
    Api, Client,
};
use sha2::{Digest, Sha256};

use crate::global_resource_crd::{EdgionGlobalResource, EdgionGlobalResourceSpec};

const MANAGED_BY_LABEL: &str = "app.kubernetes.io/managed-by";
const LIST_CHUNK_SIZE: u32 = 500;
const MAX_CONFLICT_RETRIES: usize = 8;

#[derive(Debug)]
enum ResourceError {
    Conflict,
    NotFound,
    Other,
}

impl From<kube::Error> for ResourceError {
    fn from(error: kube::Error) -> Self {
        match &error {
            kube::Error::Api(response) if response.code == 409 => Self::Conflict,
            kube::Error::Api(response) if response.code == 404 => Self::NotFound,
            _ => Self::Other,
        }
    }
}

#[async_trait]
trait GlobalResourceResources: Send + Sync {
    async fn get(&self, name: &str) -> Result<Option<EdgionGlobalResource>, ResourceError>;
    async fn list(&self) -> Result<Vec<EdgionGlobalResource>, ResourceError>;
    async fn create(
        &self,
        value: &EdgionGlobalResource,
    ) -> Result<EdgionGlobalResource, ResourceError>;
    async fn replace(
        &self,
        name: &str,
        value: &EdgionGlobalResource,
    ) -> Result<EdgionGlobalResource, ResourceError>;
}

struct KubernetesGlobalResourceResources {
    resources: Api<EdgionGlobalResource>,
}

#[async_trait]
impl GlobalResourceResources for KubernetesGlobalResourceResources {
    async fn get(&self, name: &str) -> Result<Option<EdgionGlobalResource>, ResourceError> {
        self.resources.get_opt(name).await.map_err(Into::into)
    }

    async fn list(&self) -> Result<Vec<EdgionGlobalResource>, ResourceError> {
        let mut items = Vec::new();
        let mut continuation = None;
        loop {
            let mut params = ListParams::default().limit(LIST_CHUNK_SIZE);
            params.continue_token = continuation;
            let page = self
                .resources
                .list(&params)
                .await
                .map_err(ResourceError::from)?;
            continuation = page.metadata.continue_;
            items.extend(page.items);
            if continuation.is_none() {
                return Ok(items);
            }
        }
    }

    async fn create(
        &self,
        value: &EdgionGlobalResource,
    ) -> Result<EdgionGlobalResource, ResourceError> {
        self.resources
            .create(&PostParams::default(), value)
            .await
            .map_err(Into::into)
    }

    async fn replace(
        &self,
        name: &str,
        value: &EdgionGlobalResource,
    ) -> Result<EdgionGlobalResource, ResourceError> {
        self.resources
            .replace(name, &PostParams::default(), value)
            .await
            .map_err(Into::into)
    }
}

#[derive(Clone)]
pub struct KubernetesGlobalResourceStore {
    resources: Arc<dyn GlobalResourceResources>,
    config: GlobalResourcesConfig,
}

impl KubernetesGlobalResourceStore {
    pub fn new(client: Client, config: GlobalResourcesConfig) -> CoreResult<Self> {
        config.validate().map_err(CoreError::Conflict)?;
        Ok(Self {
            resources: Arc::new(KubernetesGlobalResourceResources {
                resources: Api::all(client),
            }),
            config,
        })
    }

    #[cfg(test)]
    fn with_resources(
        resources: Arc<dyn GlobalResourceResources>,
        config: GlobalResourcesConfig,
    ) -> Self {
        Self { resources, config }
    }

    fn adapter_error(error: ResourceError) -> CoreError {
        match error {
            ResourceError::Conflict => {
                CoreError::Conflict("Kubernetes resourceVersion conflict".to_string())
            }
            ResourceError::NotFound => {
                CoreError::Adapter("Kubernetes global resource was not found".to_string())
            }
            ResourceError::Other => {
                CoreError::Adapter("Kubernetes global resource request failed".to_string())
            }
        }
    }

    fn verify_id(resource: &EdgionGlobalResource, expected: &GlobalResourceId) -> CoreResult<()> {
        if resource.spec.global_resource_id.as_bytes() != expected.as_str().as_bytes() {
            return Err(CoreError::Adapter(
                "global resource CRD name collision".to_string(),
            ));
        }
        let expected_name = global_resource_resource_name(expected)?;
        if resource.metadata.name.as_deref() != Some(expected_name.as_str()) {
            return Err(CoreError::Adapter(
                "global resource CRD name does not match its identity".to_string(),
            ));
        }
        Ok(())
    }

    fn core(&self, resource: &EdgionGlobalResource) -> CoreResult<GlobalResource> {
        let metadata_generation = resource.metadata.generation.ok_or_else(|| {
            CoreError::Adapter("global resource CRD omitted metadata.generation".to_string())
        })?;
        if metadata_generation != resource.spec.generation {
            return Err(CoreError::Adapter(
                "global resource CRD metadata generation does not match intent generation"
                    .to_string(),
            ));
        }
        let value = resource.spec.to_core(&self.config).map_err(|message| {
            CoreError::Adapter(format!("invalid global resource CRD: {message}"))
        })?;
        Self::verify_id(resource, &value.id)?;
        Ok(value)
    }

    fn new_resource(name: &str, resource: &GlobalResource) -> CoreResult<EdgionGlobalResource> {
        let spec = EdgionGlobalResourceSpec::new(resource).map_err(CoreError::Conflict)?;
        let mut value = EdgionGlobalResource::new(name, spec);
        value.metadata.labels = Some(BTreeMap::from([(
            MANAGED_BY_LABEL.to_string(),
            "edgion-center".to_string(),
        )]));
        Ok(value)
    }

    async fn load(&self, id: &GlobalResourceId) -> CoreResult<Option<EdgionGlobalResource>> {
        let name = global_resource_resource_name(id)?;
        let resource = self
            .resources
            .get(&name)
            .await
            .map_err(Self::adapter_error)?;
        if let Some(resource) = resource.as_ref() {
            Self::verify_id(resource, id)?;
        }
        Ok(resource)
    }
}

#[async_trait]
impl GlobalResourceStore for KubernetesGlobalResourceStore {
    async fn create(
        &self,
        id: &GlobalResourceId,
        desired: &GlobalResourceDesired,
        actor: &str,
        now_unix_ms: i64,
    ) -> CoreResult<GlobalResourceCreateResult> {
        let intent = global_resource_for_create(
            id.clone(),
            desired.clone(),
            actor,
            now_unix_ms,
            &self.config,
        )?;
        let name = global_resource_resource_name(id)?;
        let resource = Self::new_resource(&name, &intent)?;
        match self.resources.create(&resource).await {
            Ok(created) => {
                Self::verify_id(&created, id)?;
                Ok(GlobalResourceCreateResult::Created(Box::new(
                    self.core(&created)?,
                )))
            }
            Err(ResourceError::Conflict) => {
                let existing = self
                    .resources
                    .get(&name)
                    .await
                    .map_err(Self::adapter_error)?;
                let Some(existing) = existing else {
                    return Err(CoreError::Conflict(
                        "global resource create conflicted with a disappearing resource"
                            .to_string(),
                    ));
                };
                Self::verify_id(&existing, id)?;
                self.core(&existing)?;
                Ok(GlobalResourceCreateResult::AlreadyExists)
            }
            Err(error) => Err(Self::adapter_error(error)),
        }
    }

    async fn get(&self, id: &GlobalResourceId) -> CoreResult<Option<GlobalResource>> {
        id.validate()?;
        self.load(id)
            .await?
            .as_ref()
            .map(|resource| self.core(resource))
            .transpose()
    }

    async fn list(&self, page: &GlobalResourcePageRequest) -> CoreResult<GlobalResourcePage> {
        page.validate()?;
        let mut resources = self
            .resources
            .list()
            .await
            .map_err(Self::adapter_error)?
            .iter()
            .map(|resource| self.core(resource))
            .collect::<CoreResult<Vec<_>>>()?;
        resources.sort_by(|left, right| {
            left.id
                .as_str()
                .as_bytes()
                .cmp(right.id.as_str().as_bytes())
        });
        if let Some(after) = page.after.as_ref() {
            resources
                .retain(|resource| resource.id.as_str().as_bytes() > after.as_str().as_bytes());
        }
        let has_more = resources.len() > usize::from(page.limit);
        resources.truncate(usize::from(page.limit));
        let result = GlobalResourcePage {
            next: has_more.then(|| {
                resources
                    .last()
                    .expect("non-empty page when an extra item exists")
                    .id
                    .clone()
            }),
            items: resources,
        };
        result.validate(page, &self.config)?;
        Ok(result)
    }

    async fn replace_if_generation(
        &self,
        id: &GlobalResourceId,
        expected_generation: u64,
        desired: &GlobalResourceDesired,
        actor: &str,
        now_unix_ms: i64,
    ) -> CoreResult<GlobalResourceReplaceResult> {
        id.validate()?;
        if expected_generation == 0 || expected_generation >= i64::MAX as u64 {
            return Err(CoreError::Conflict(
                "global resource expected generation is outside the persistence range".to_string(),
            ));
        }
        let name = global_resource_resource_name(id)?;
        for _ in 0..MAX_CONFLICT_RETRIES {
            let Some(mut resource) = self.load(id).await? else {
                return Ok(GlobalResourceReplaceResult::NotFound);
            };
            let current = self.core(&resource)?;
            if current.generation != expected_generation {
                return Ok(GlobalResourceReplaceResult::GenerationMismatch {
                    actual_generation: current.generation,
                });
            }
            let next = global_resource_for_replace(
                &current,
                desired.clone(),
                actor,
                now_unix_ms,
                &self.config,
            )?;
            resource.spec = EdgionGlobalResourceSpec::new(&next).map_err(CoreError::Conflict)?;
            match self.resources.replace(&name, &resource).await {
                Ok(stored) => {
                    Self::verify_id(&stored, id)?;
                    return Ok(GlobalResourceReplaceResult::Stored(Box::new(
                        self.core(&stored)?,
                    )));
                }
                Err(ResourceError::Conflict) => continue,
                Err(ResourceError::NotFound) => {
                    return Ok(GlobalResourceReplaceResult::NotFound);
                }
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(
            "Kubernetes global resource update remained contended".to_string(),
        ))
    }
}

pub fn global_resource_resource_name(id: &GlobalResourceId) -> CoreResult<String> {
    id.validate()?;
    let digest = Sha256::digest(id.as_str().as_bytes());
    Ok(format!("global-resource-{}", hex::encode(&digest[..20])))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeSet, HashMap},
        sync::Mutex,
    };

    use edgion_center_core::{
        GlobalResourceAdoptionPolicy, GlobalResourceInventoryKind, GlobalResourcePrunePolicy,
        GlobalResourceSyncMode, GlobalResourceSyncPolicy, GlobalResourceTargetSelector,
    };
    use serde_json::json;

    use super::*;

    #[derive(Default)]
    struct MemoryResources {
        values: Mutex<HashMap<String, EdgionGlobalResource>>,
        revision: Mutex<u64>,
        replace_conflicts: Mutex<u32>,
    }

    impl MemoryResources {
        fn next_revision(&self) -> String {
            let mut revision = self.revision.lock().unwrap();
            *revision += 1;
            revision.to_string()
        }
    }

    #[async_trait]
    impl GlobalResourceResources for MemoryResources {
        async fn get(&self, name: &str) -> Result<Option<EdgionGlobalResource>, ResourceError> {
            Ok(self.values.lock().unwrap().get(name).cloned())
        }

        async fn list(&self) -> Result<Vec<EdgionGlobalResource>, ResourceError> {
            Ok(self.values.lock().unwrap().values().cloned().collect())
        }

        async fn create(
            &self,
            value: &EdgionGlobalResource,
        ) -> Result<EdgionGlobalResource, ResourceError> {
            let name = value.metadata.name.clone().unwrap();
            let mut values = self.values.lock().unwrap();
            if values.contains_key(&name) {
                return Err(ResourceError::Conflict);
            }
            let mut stored = value.clone();
            stored.metadata.resource_version = Some(self.next_revision());
            stored.metadata.generation = Some(1);
            values.insert(name, stored.clone());
            Ok(stored)
        }

        async fn replace(
            &self,
            name: &str,
            value: &EdgionGlobalResource,
        ) -> Result<EdgionGlobalResource, ResourceError> {
            let mut values = self.values.lock().unwrap();
            let Some(current) = values.get(name) else {
                return Err(ResourceError::NotFound);
            };
            if current.metadata.resource_version != value.metadata.resource_version {
                return Err(ResourceError::Conflict);
            }
            let mut replace_conflicts = self.replace_conflicts.lock().unwrap();
            if *replace_conflicts > 0 {
                *replace_conflicts -= 1;
                let current = values.get_mut(name).unwrap();
                current.metadata.resource_version = Some(self.next_revision());
                return Err(ResourceError::Conflict);
            }
            let mut stored = value.clone();
            stored.metadata.resource_version = Some(self.next_revision());
            stored.metadata.generation = Some(current.metadata.generation.unwrap_or(0) + 1);
            values.insert(name.to_string(), stored.clone());
            Ok(stored)
        }
    }

    fn desired(name: &str) -> GlobalResourceDesired {
        GlobalResourceDesired {
            display_name: format!("Desired {name}"),
            resource_kind: GlobalResourceInventoryKind::EdgionConfigData,
            target_namespace: "edgion-data".to_string(),
            template_document: json!({
                "apiVersion": "edgion.io/v1alpha1",
                "kind": "EdgionConfigData",
                "metadata": {"name": name, "namespace": "edgion-data"},
                "data": {"type": "IpList", "items": ["192.0.2.0/24"]}
            }),
            target_selector: GlobalResourceTargetSelector::ClusterIds {
                cluster_ids: BTreeSet::from(["cluster-a".to_string()]),
            },
            sync_policy: GlobalResourceSyncPolicy {
                mode: GlobalResourceSyncMode::Manual,
                adoption: GlobalResourceAdoptionPolicy::Never,
                prune: GlobalResourcePrunePolicy::Retain,
            },
        }
    }

    fn make_store(resources: Arc<MemoryResources>) -> KubernetesGlobalResourceStore {
        KubernetesGlobalResourceStore::with_resources(resources, GlobalResourcesConfig::default())
    }

    #[tokio::test]
    async fn create_get_replace_and_generation_mismatch_are_fenced() {
        let resources = Arc::new(MemoryResources::default());
        let store = make_store(resources);
        let id = GlobalResourceId::new("shared-list").unwrap();
        let created = store
            .create(&id, &desired("shared-list"), "alice", 100)
            .await
            .unwrap();
        let GlobalResourceCreateResult::Created(created) = created else {
            panic!("resource was not created");
        };
        assert_eq!(created.generation, 1);
        assert!(matches!(
            store
                .create(&id, &desired("shared-list"), "bob", 200)
                .await
                .unwrap(),
            GlobalResourceCreateResult::AlreadyExists
        ));
        let replaced = store
            .replace_if_generation(&id, 1, &desired("shared-list-v2"), "bob", 200)
            .await
            .unwrap();
        let GlobalResourceReplaceResult::Stored(replaced) = replaced else {
            panic!("resource was not replaced");
        };
        assert_eq!(replaced.generation, 2);
        assert_eq!(replaced.created_by, "alice");
        assert_eq!(replaced.updated_by, "bob");
        assert!(matches!(
            store
                .replace_if_generation(&id, 1, &desired("stale"), "carol", 300)
                .await
                .unwrap(),
            GlobalResourceReplaceResult::GenerationMismatch {
                actual_generation: 2
            }
        ));
    }

    #[tokio::test]
    async fn metadata_only_conflict_retries_only_unchanged_generation() {
        let resources = Arc::new(MemoryResources::default());
        let store = make_store(resources.clone());
        let id = GlobalResourceId::new("retry-list").unwrap();
        store
            .create(&id, &desired("retry-list"), "alice", 100)
            .await
            .unwrap();
        *resources.replace_conflicts.lock().unwrap() = 1;
        let result = store
            .replace_if_generation(&id, 1, &desired("retry-list-v2"), "bob", 200)
            .await
            .unwrap();
        assert!(matches!(result, GlobalResourceReplaceResult::Stored(_)));
        assert_eq!(store.get(&id).await.unwrap().unwrap().generation, 2);
    }

    #[tokio::test]
    async fn identical_intent_advances_generation_but_preserves_desired_revision() {
        let resources = Arc::new(MemoryResources::default());
        let store = make_store(resources);
        let id = GlobalResourceId::new("identical-list").unwrap();
        let desired = desired("identical-list");
        let GlobalResourceCreateResult::Created(created) =
            store.create(&id, &desired, "alice", 100).await.unwrap()
        else {
            panic!("resource was not created");
        };
        let GlobalResourceReplaceResult::Stored(replaced) = store
            .replace_if_generation(&id, 1, &desired, "alice", 100)
            .await
            .unwrap()
        else {
            panic!("resource was not replaced");
        };
        assert_eq!(replaced.generation, 2);
        assert_eq!(replaced.desired_revision, created.desired_revision);
    }

    #[tokio::test]
    async fn list_uses_strict_identity_order_and_exclusive_pages() {
        let resources = Arc::new(MemoryResources::default());
        let store = make_store(resources);
        for id in ["c", "a", "b"] {
            let id = GlobalResourceId::new(id).unwrap();
            store
                .create(&id, &desired(id.as_str()), "alice", 100)
                .await
                .unwrap();
        }
        let first = store
            .list(&GlobalResourcePageRequest {
                limit: 2,
                after: None,
            })
            .await
            .unwrap();
        assert_eq!(
            first
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(first.next.as_ref().map(GlobalResourceId::as_str), Some("b"));
        let second = store
            .list(&GlobalResourcePageRequest {
                limit: 2,
                after: first.next,
            })
            .await
            .unwrap();
        assert_eq!(
            second
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["c"]
        );
        assert!(second.next.is_none());
    }

    #[tokio::test]
    async fn malformed_identity_and_generation_are_rejected() {
        let resources = Arc::new(MemoryResources::default());
        let store = make_store(resources.clone());
        let id = GlobalResourceId::new("valid-id").unwrap();
        store
            .create(&id, &desired("valid-id"), "alice", 100)
            .await
            .unwrap();
        let name = global_resource_resource_name(&id).unwrap();
        {
            let mut values = resources.values.lock().unwrap();
            let resource = values.get_mut(&name).unwrap();
            resource.spec.global_resource_id = "different-id".to_string();
        }
        assert!(store.get(&id).await.is_err());

        let resources = Arc::new(MemoryResources::default());
        let store = make_store(resources.clone());
        store
            .create(&id, &desired("valid-id"), "alice", 100)
            .await
            .unwrap();
        {
            let mut values = resources.values.lock().unwrap();
            values.get_mut(&name).unwrap().metadata.generation = Some(2);
        }
        assert!(store.get(&id).await.is_err());

        let resources = Arc::new(MemoryResources::default());
        let store = make_store(resources.clone());
        store
            .create(&id, &desired("valid-id"), "alice", 100)
            .await
            .unwrap();
        {
            let mut values = resources.values.lock().unwrap();
            values.get_mut(&name).unwrap().spec.desired_revision =
                format!("sha256:{}", "0".repeat(64));
        }
        assert!(store.get(&id).await.is_err());
    }

    #[test]
    fn resource_name_is_dns_safe_and_identity_sensitive() {
        let upper = GlobalResourceId::new("account.a").unwrap();
        let lower = GlobalResourceId::new("account-a").unwrap();
        let upper_name = global_resource_resource_name(&upper).unwrap();
        assert!(upper_name.len() <= 63);
        assert_ne!(upper_name, global_resource_resource_name(&lower).unwrap());
    }
}
