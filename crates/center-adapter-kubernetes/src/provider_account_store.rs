use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use edgion_center_core::{
    validate_stored_provider_account, CloudResourceId, CoreError, CoreResult, ProviderAccount,
    ProviderAccountCreateResult, ProviderAccountDesired, ProviderAccountPage,
    ProviderAccountPageRequest, ProviderAccountReplaceResult, ProviderAccountStore,
};
use kube::{api::PostParams, Api, Client};
use sha2::{Digest, Sha256};

use crate::provider_account_crd::{EdgionProviderAccount, EdgionProviderAccountSpec};

const MANAGED_BY_LABEL: &str = "app.kubernetes.io/managed-by";
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
trait ProviderAccountResources: Send + Sync {
    async fn get(&self, name: &str) -> Result<Option<EdgionProviderAccount>, ResourceError>;
    async fn list(&self) -> Result<Vec<EdgionProviderAccount>, ResourceError>;
    async fn create(
        &self,
        value: &EdgionProviderAccount,
    ) -> Result<EdgionProviderAccount, ResourceError>;
    async fn replace(
        &self,
        name: &str,
        value: &EdgionProviderAccount,
    ) -> Result<EdgionProviderAccount, ResourceError>;
}

struct KubernetesProviderAccountResources {
    accounts: Api<EdgionProviderAccount>,
}

#[async_trait]
impl ProviderAccountResources for KubernetesProviderAccountResources {
    async fn get(&self, name: &str) -> Result<Option<EdgionProviderAccount>, ResourceError> {
        self.accounts.get_opt(name).await.map_err(Into::into)
    }

    async fn list(&self) -> Result<Vec<EdgionProviderAccount>, ResourceError> {
        self.accounts
            .list(&Default::default())
            .await
            .map(|list| list.items)
            .map_err(Into::into)
    }

    async fn create(
        &self,
        value: &EdgionProviderAccount,
    ) -> Result<EdgionProviderAccount, ResourceError> {
        self.accounts
            .create(&PostParams::default(), value)
            .await
            .map_err(Into::into)
    }

    async fn replace(
        &self,
        name: &str,
        value: &EdgionProviderAccount,
    ) -> Result<EdgionProviderAccount, ResourceError> {
        self.accounts
            .replace(name, &PostParams::default(), value)
            .await
            .map_err(Into::into)
    }
}

#[derive(Clone)]
pub struct KubernetesProviderAccountStore {
    resources: Arc<dyn ProviderAccountResources>,
}

impl KubernetesProviderAccountStore {
    pub fn new(client: Client, namespace: &str) -> CoreResult<Self> {
        if namespace.trim().is_empty() || namespace.chars().any(char::is_control) {
            return Err(CoreError::Adapter(
                "Kubernetes provider account namespace must be non-empty".to_string(),
            ));
        }
        Ok(Self {
            resources: Arc::new(KubernetesProviderAccountResources {
                accounts: Api::namespaced(client, namespace),
            }),
        })
    }

    #[cfg(test)]
    fn with_resources(resources: Arc<dyn ProviderAccountResources>) -> Self {
        Self { resources }
    }

    fn adapter_error(error: ResourceError) -> CoreError {
        match error {
            ResourceError::Conflict => {
                CoreError::Conflict("Kubernetes resourceVersion conflict".to_string())
            }
            ResourceError::NotFound => {
                CoreError::Adapter("Kubernetes provider account was not found".to_string())
            }
            ResourceError::Other => {
                CoreError::Adapter("Kubernetes provider account request failed".to_string())
            }
        }
    }

    fn verify_id(resource: &EdgionProviderAccount, expected: &CloudResourceId) -> CoreResult<()> {
        if resource.spec.account_id.as_bytes() != expected.as_str().as_bytes() {
            return Err(CoreError::Adapter(
                "provider account CRD name collision".to_string(),
            ));
        }
        let expected_name = provider_account_resource_name(expected)?;
        if resource.metadata.name.as_deref() != Some(expected_name.as_str()) {
            return Err(CoreError::Adapter(
                "provider account CRD name does not match its account identity".to_string(),
            ));
        }
        Ok(())
    }

    fn core(resource: &EdgionProviderAccount) -> CoreResult<ProviderAccount> {
        let metadata_generation = resource.metadata.generation.ok_or_else(|| {
            CoreError::Adapter("provider account CRD omitted metadata.generation".to_string())
        })?;
        if metadata_generation != resource.spec.desired_generation {
            return Err(CoreError::Adapter(
                "provider account CRD generation does not match desired generation".to_string(),
            ));
        }
        let account = resource.spec.to_core().map_err(|message| {
            CoreError::Adapter(format!("invalid provider account CRD: {message}"))
        })?;
        Self::verify_id(resource, &account.metadata.id)?;
        validate_stored_provider_account(&account).map_err(|_| {
            CoreError::Adapter("provider account CRD contains invalid stored state".to_string())
        })?;
        Ok(account)
    }

    fn new_resource(
        name: &str,
        account_id: &CloudResourceId,
        generation: u64,
        desired: &ProviderAccountDesired,
    ) -> CoreResult<EdgionProviderAccount> {
        let spec = EdgionProviderAccountSpec::new(account_id, generation, desired)
            .map_err(CoreError::Conflict)?;
        let mut resource = EdgionProviderAccount::new(name, spec);
        resource.metadata.labels = Some(BTreeMap::from([(
            MANAGED_BY_LABEL.to_string(),
            "edgion-center".to_string(),
        )]));
        Ok(resource)
    }

    async fn load(
        &self,
        account_id: &CloudResourceId,
    ) -> CoreResult<Option<EdgionProviderAccount>> {
        let name = provider_account_resource_name(account_id)?;
        let resource = self
            .resources
            .get(&name)
            .await
            .map_err(Self::adapter_error)?;
        if let Some(resource) = resource.as_ref() {
            Self::verify_id(resource, account_id)?;
        }
        Ok(resource)
    }
}

#[async_trait]
impl ProviderAccountStore for KubernetesProviderAccountStore {
    async fn create(
        &self,
        account_id: &CloudResourceId,
        desired: &ProviderAccountDesired,
    ) -> CoreResult<ProviderAccountCreateResult> {
        account_id.validate()?;
        desired.validate()?;
        let name = provider_account_resource_name(account_id)?;
        let resource = Self::new_resource(&name, account_id, 1, desired)?;
        match self.resources.create(&resource).await {
            Ok(created) => {
                Self::verify_id(&created, account_id)?;
                Ok(ProviderAccountCreateResult::Created(Box::new(Self::core(
                    &created,
                )?)))
            }
            Err(ResourceError::Conflict) => {
                let existing = self
                    .resources
                    .get(&name)
                    .await
                    .map_err(Self::adapter_error)?;
                let Some(existing) = existing else {
                    return Err(CoreError::Conflict(
                        "provider account create conflicted with a disappearing resource"
                            .to_string(),
                    ));
                };
                Self::verify_id(&existing, account_id)?;
                Self::core(&existing)?;
                Ok(ProviderAccountCreateResult::AlreadyExists)
            }
            Err(error) => Err(Self::adapter_error(error)),
        }
    }

    async fn get(&self, account_id: &CloudResourceId) -> CoreResult<Option<ProviderAccount>> {
        account_id.validate()?;
        self.load(account_id)
            .await?
            .as_ref()
            .map(Self::core)
            .transpose()
    }

    async fn list(&self, page: &ProviderAccountPageRequest) -> CoreResult<ProviderAccountPage> {
        page.validate()?;
        let mut accounts = self
            .resources
            .list()
            .await
            .map_err(Self::adapter_error)?
            .iter()
            .map(Self::core)
            .collect::<CoreResult<Vec<_>>>()?;
        accounts.sort_by(|left, right| {
            left.metadata
                .id
                .as_str()
                .as_bytes()
                .cmp(right.metadata.id.as_str().as_bytes())
        });
        if let Some(after) = page.after.as_ref() {
            accounts.retain(|account| {
                account.metadata.id.as_str().as_bytes() > after.as_str().as_bytes()
            });
        }
        let has_more = accounts.len() > usize::from(page.limit);
        accounts.truncate(usize::from(page.limit));
        let result = ProviderAccountPage {
            next: has_more.then(|| {
                accounts
                    .last()
                    .expect("non-empty page when an extra item exists")
                    .metadata
                    .id
                    .clone()
            }),
            items: accounts,
        };
        result.validate(page)?;
        Ok(result)
    }

    async fn replace_if_generation(
        &self,
        account_id: &CloudResourceId,
        expected_generation: u64,
        desired: &ProviderAccountDesired,
    ) -> CoreResult<ProviderAccountReplaceResult> {
        account_id.validate()?;
        desired.validate()?;
        if expected_generation == 0 || expected_generation >= i64::MAX as u64 {
            return Err(CoreError::Conflict(
                "provider account expected generation is outside the persistence range".to_string(),
            ));
        }
        let name = provider_account_resource_name(account_id)?;
        for _ in 0..MAX_CONFLICT_RETRIES {
            let Some(mut resource) = self.load(account_id).await? else {
                return Ok(ProviderAccountReplaceResult::NotFound);
            };
            let current = Self::core(&resource)?;
            if current.metadata.generation != expected_generation {
                return Ok(ProviderAccountReplaceResult::GenerationMismatch {
                    actual_generation: current.metadata.generation,
                });
            }
            resource.spec =
                EdgionProviderAccountSpec::new(account_id, expected_generation + 1, desired)
                    .map_err(CoreError::Conflict)?;
            match self.resources.replace(&name, &resource).await {
                Ok(stored) => {
                    Self::verify_id(&stored, account_id)?;
                    return Ok(ProviderAccountReplaceResult::Stored(Box::new(Self::core(
                        &stored,
                    )?)));
                }
                Err(ResourceError::Conflict) => continue,
                Err(ResourceError::NotFound) => {
                    return Ok(ProviderAccountReplaceResult::NotFound);
                }
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(
            "Kubernetes provider account update remained contended".to_string(),
        ))
    }
}

pub fn provider_account_resource_name(account_id: &CloudResourceId) -> CoreResult<String> {
    account_id.validate()?;
    let digest = Sha256::digest(account_id.as_str().as_bytes());
    Ok(format!("provider-account-{}", hex::encode(&digest[..20])))
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use edgion_center_core::cloud_test_support::assert_provider_account_store_conformance;

    use super::*;

    #[derive(Default)]
    struct MemoryResources {
        values: Mutex<HashMap<String, EdgionProviderAccount>>,
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
    impl ProviderAccountResources for MemoryResources {
        async fn get(&self, name: &str) -> Result<Option<EdgionProviderAccount>, ResourceError> {
            Ok(self.values.lock().unwrap().get(name).cloned())
        }

        async fn list(&self) -> Result<Vec<EdgionProviderAccount>, ResourceError> {
            Ok(self.values.lock().unwrap().values().cloned().collect())
        }

        async fn create(
            &self,
            value: &EdgionProviderAccount,
        ) -> Result<EdgionProviderAccount, ResourceError> {
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
            value: &EdgionProviderAccount,
        ) -> Result<EdgionProviderAccount, ResourceError> {
            let mut values = self.values.lock().unwrap();
            let Some(current) = values.get(name) else {
                return Err(ResourceError::Conflict);
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

    #[tokio::test]
    async fn memory_store_passes_shared_conformance() {
        let resources = Arc::new(MemoryResources::default());
        let store = KubernetesProviderAccountStore::with_resources(resources);
        assert_provider_account_store_conformance(&store, "kubernetes").await;
    }

    #[tokio::test]
    async fn metadata_only_conflict_is_retried_without_false_generation_mismatch() {
        let resources = Arc::new(MemoryResources::default());
        let store = KubernetesProviderAccountStore::with_resources(resources.clone());
        let id = CloudResourceId::new("kubernetes/retry-account").unwrap();
        let first: ProviderAccountDesired = serde_json::from_value(serde_json::json!({
            "displayName": "Cloudflare first",
            "owner": null,
            "labels": {},
            "managementPolicy": "observe_only",
            "deletionPolicy": "retain",
            "spec": {
                "provider": "cloudflare",
                "scope": {"provider": "cloudflare", "account_id": "0123456789abcdef0123456789abcdef"},
                "credentialSource": {"type": "ambient"}
            }
        }))
        .unwrap();
        assert!(matches!(
            store.create(&id, &first).await.unwrap(),
            ProviderAccountCreateResult::Created(_)
        ));
        *resources.replace_conflicts.lock().unwrap() = 1;
        let mut second = first;
        second.display_name = "Cloudflare second".to_string();
        assert!(matches!(
            store.replace_if_generation(&id, 1, &second).await.unwrap(),
            ProviderAccountReplaceResult::Stored(_)
        ));
        assert_eq!(
            store.get(&id).await.unwrap().unwrap().metadata.generation,
            2
        );
    }

    #[tokio::test]
    async fn identical_desired_replacement_advances_both_generations() {
        let resources = Arc::new(MemoryResources::default());
        let store = KubernetesProviderAccountStore::with_resources(resources.clone());
        let id = CloudResourceId::new("kubernetes/identical-account").unwrap();
        let desired: ProviderAccountDesired = serde_json::from_value(serde_json::json!({
            "displayName": "Cloudflare identical",
            "owner": null,
            "labels": {},
            "managementPolicy": "observe_only",
            "deletionPolicy": "retain",
            "spec": {
                "provider": "cloudflare",
                "scope": {"provider": "cloudflare", "account_id": "0123456789abcdef0123456789abcdef"},
                "credentialSource": {"type": "ambient"}
            }
        }))
        .unwrap();
        store.create(&id, &desired).await.unwrap();
        let replaced = store.replace_if_generation(&id, 1, &desired).await.unwrap();
        let ProviderAccountReplaceResult::Stored(replaced) = replaced else {
            panic!("identical replacement was not stored");
        };
        assert_eq!(replaced.metadata.generation, 2);
        let name = provider_account_resource_name(&id).unwrap();
        let resource = resources
            .values
            .lock()
            .unwrap()
            .get(&name)
            .cloned()
            .unwrap();
        assert_eq!(resource.metadata.generation, Some(2));
        assert_eq!(resource.spec.desired_generation, 2);
    }

    #[test]
    fn resource_name_is_dns_safe_and_case_sensitive() {
        let upper = CloudResourceId::new("Account/A").unwrap();
        let lower = CloudResourceId::new("account/a").unwrap();
        let upper_name = provider_account_resource_name(&upper).unwrap();
        assert!(upper_name.len() <= 63);
        assert_ne!(upper_name, provider_account_resource_name(&lower).unwrap());
    }
}
