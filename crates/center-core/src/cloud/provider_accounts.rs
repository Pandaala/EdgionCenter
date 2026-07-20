//! Persistence contract for secret-free provider account desired state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    CloudResource, CloudResourceId, CloudResourceMetadata, CloudResourceStatus, DeletionPolicy,
    ManagementPolicy, ProviderAccount, ProviderAccountSpec,
};
use crate::{CoreError, CoreResult};

const MAX_PROVIDER_ACCOUNT_PAGE_SIZE: u16 = 100;
const MAX_DISPLAY_NAME_BYTES: usize = 256;
const MAX_OWNER_BYTES: usize = 256;
const MAX_LABELS: usize = 64;
const MAX_LABEL_KEY_BYTES: usize = 128;
const MAX_LABEL_VALUE_BYTES: usize = 256;
const MAX_CREDENTIAL_REF_BYTES: usize = 512;
const MAX_DESIRED_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_PROVIDER_ACCOUNT_GENERATION: u64 = i64::MAX as u64;

/// Caller-controlled ProviderAccount desired state. Persistence assigns the
/// generation and always starts with empty observed status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccountDesired {
    pub display_name: String,
    pub owner: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub management_policy: ManagementPolicy,
    #[serde(default)]
    pub deletion_policy: DeletionPolicy,
    pub spec: ProviderAccountSpec,
}

impl ProviderAccountDesired {
    pub fn validate(&self) -> CoreResult<()> {
        validate_text(
            &self.display_name,
            "provider account display name",
            MAX_DISPLAY_NAME_BYTES,
            false,
        )?;
        if let Some(owner) = self.owner.as_deref() {
            validate_text(owner, "provider account owner", MAX_OWNER_BYTES, false)?;
        }
        if self.labels.len() > MAX_LABELS {
            return Err(CoreError::Conflict(
                "provider account has too many labels".to_string(),
            ));
        }
        for (key, value) in &self.labels {
            validate_text(
                key,
                "provider account label key",
                MAX_LABEL_KEY_BYTES,
                false,
            )?;
            validate_text(
                value,
                "provider account label value",
                MAX_LABEL_VALUE_BYTES,
                true,
            )?;
        }
        if self.deletion_policy != DeletionPolicy::Retain {
            return Err(CoreError::Conflict(
                "provider account deletion policy must retain the provider account".to_string(),
            ));
        }
        self.spec.validate()?;
        for credential_ref in self.spec.credential_source.credential_refs() {
            credential_ref.validate()?;
            if credential_ref.as_str().len() > MAX_CREDENTIAL_REF_BYTES {
                return Err(CoreError::Conflict(
                    "provider account credential reference is too long".to_string(),
                ));
            }
        }
        let encoded = serde_json::to_vec(self)
            .map_err(|_| CoreError::Conflict("provider account desired state is invalid".into()))?;
        if encoded.len() > MAX_DESIRED_PAYLOAD_BYTES {
            return Err(CoreError::Conflict(
                "provider account desired state exceeds its persistence limit".to_string(),
            ));
        }
        Ok(())
    }

    fn from_account(account: &ProviderAccount) -> Self {
        Self {
            display_name: account.metadata.display_name.clone(),
            owner: account.metadata.owner.clone(),
            labels: account.metadata.labels.clone(),
            management_policy: account.metadata.management_policy,
            deletion_policy: account.metadata.deletion_policy,
            spec: account.spec.clone(),
        }
    }
}

fn validate_text(
    value: &str,
    kind: &'static str,
    max_bytes: usize,
    allow_empty: bool,
) -> CoreResult<()> {
    if (!allow_empty && value.is_empty())
        || value.len() > max_bytes
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(CoreError::Conflict(format!("{kind} is invalid")));
    }
    Ok(())
}

/// Constructs the only ProviderAccount shape accepted by the desired-state
/// store. Adapters use this helper after atomically assigning a generation.
pub fn provider_account_from_desired(
    account_id: CloudResourceId,
    generation: u64,
    desired: &ProviderAccountDesired,
) -> CoreResult<ProviderAccount> {
    account_id.validate()?;
    desired.validate()?;
    if generation == 0 || generation > MAX_PROVIDER_ACCOUNT_GENERATION {
        return Err(CoreError::Conflict(
            "provider account generation is outside the persistence range".to_string(),
        ));
    }
    let account = ProviderAccount {
        metadata: CloudResourceMetadata {
            id: account_id,
            display_name: desired.display_name.clone(),
            owner: desired.owner.clone(),
            labels: desired.labels.clone(),
            generation,
            management_policy: desired.management_policy,
            deletion_policy: desired.deletion_policy,
        },
        spec: desired.spec.clone(),
        status: CloudResourceStatus::default(),
    };
    validate_stored_provider_account(&account)?;
    Ok(account)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountPageRequest {
    pub limit: u16,
    /// Exclusive keyset boundary. IDs are compared with exact byte ordering.
    pub after: Option<CloudResourceId>,
}

impl ProviderAccountPageRequest {
    pub fn validate(&self) -> CoreResult<()> {
        if self.limit == 0 || self.limit > MAX_PROVIDER_ACCOUNT_PAGE_SIZE {
            return Err(CoreError::Conflict(
                "provider account page size is invalid".to_string(),
            ));
        }
        if let Some(after) = self.after.as_ref() {
            after.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountPage {
    pub items: Vec<ProviderAccount>,
    /// Exclusive boundary to pass to the next request.
    pub next: Option<CloudResourceId>,
}

impl ProviderAccountPage {
    pub fn validate(&self, request: &ProviderAccountPageRequest) -> CoreResult<()> {
        request.validate()?;
        if self.items.len() > usize::from(request.limit) {
            return Err(CoreError::Conflict(
                "provider account response page exceeds the requested size".to_string(),
            ));
        }
        let mut previous = request.after.as_ref();
        for account in &self.items {
            validate_stored_provider_account(account)?;
            if previous
                .is_some_and(|id| id.as_str().as_bytes() >= account.metadata.id.as_str().as_bytes())
            {
                return Err(CoreError::Conflict(
                    "provider account response page is not strictly ordered".to_string(),
                ));
            }
            previous = Some(&account.metadata.id);
        }
        if let Some(next) = self.next.as_ref() {
            next.validate()?;
            if self.items.last().map(|account| &account.metadata.id) != Some(next) {
                return Err(CoreError::Conflict(
                    "provider account next boundary does not match the page".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderAccountCreateResult {
    Created(Box<ProviderAccount>),
    AlreadyExists,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderAccountReplaceResult {
    Stored(Box<ProviderAccount>),
    NotFound,
    GenerationMismatch { actual_generation: u64 },
}

/// Revalidates a complete persisted account, including desired-state bounds,
/// store-owned generation, and the first-slice empty-status invariant.
pub fn validate_stored_provider_account(account: &ProviderAccount) -> CoreResult<()> {
    account.metadata.id.validate()?;
    if account.metadata.generation == 0
        || account.metadata.generation > MAX_PROVIDER_ACCOUNT_GENERATION
    {
        return Err(CoreError::Conflict(
            "stored provider account generation is outside the persistence range".to_string(),
        ));
    }
    if account.status != CloudResourceStatus::default() {
        return Err(CoreError::Conflict(
            "provider account desired-state store does not accept status".to_string(),
        ));
    }
    ProviderAccountDesired::from_account(account).validate()?;
    CloudResource::ProviderAccount(account.clone()).validate()
}

#[async_trait::async_trait]
pub trait ProviderAccountStore: Send + Sync {
    /// Creates generation one and empty status. Duplicate identity never
    /// replaces the existing account.
    async fn create(
        &self,
        account_id: &CloudResourceId,
        desired: &ProviderAccountDesired,
    ) -> CoreResult<ProviderAccountCreateResult>;

    async fn get(&self, account_id: &CloudResourceId) -> CoreResult<Option<ProviderAccount>>;

    /// Returns accounts in exact byte ordering by Center-owned account ID. `next`
    /// is `Some(last_returned_id)` if and only if more results exist. Adapters
    /// should fetch `limit + 1` rows to establish that fact without ambiguity.
    async fn list(&self, page: &ProviderAccountPageRequest) -> CoreResult<ProviderAccountPage>;

    /// Replaces desired state only when the persisted generation exactly equals
    /// `expected_generation`. The store assigns the checked next generation and
    /// must not retry a stale write against newer state. `expected_generation`
    /// must be positive and strictly below `i64::MAX`, so the assigned next
    /// generation remains representable in every supported persistence mode.
    async fn replace_if_generation(
        &self,
        account_id: &CloudResourceId,
        expected_generation: u64,
        desired: &ProviderAccountDesired,
    ) -> CoreResult<ProviderAccountReplaceResult>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CloudProvider, CredentialRef, CredentialSource, ProviderAccountScope};

    fn desired() -> ProviderAccountDesired {
        ProviderAccountDesired {
            display_name: "Cloudflare production".to_string(),
            owner: Some("platform".to_string()),
            labels: BTreeMap::from([("environment".to_string(), "prod".to_string())]),
            management_policy: ManagementPolicy::ObserveOnly,
            deletion_policy: DeletionPolicy::Retain,
            spec: ProviderAccountSpec {
                provider: CloudProvider::Cloudflare,
                scope: Some(ProviderAccountScope::Cloudflare {
                    account_id: "0123456789abcdef0123456789abcdef".to_string(),
                }),
                credential_source: CredentialSource::StaticSecret {
                    credential_ref: CredentialRef::new("cloudflare/main").unwrap(),
                },
            },
        }
    }

    #[test]
    fn desired_state_builds_store_owned_generation_and_status() {
        let value = provider_account_from_desired(
            CloudResourceId::new("account-a").unwrap(),
            1,
            &desired(),
        )
        .unwrap();
        assert_eq!(value.metadata.generation, 1);
        assert_eq!(value.status, CloudResourceStatus::default());
        assert_eq!(value.spec, desired().spec);
        assert!(provider_account_from_desired(value.metadata.id, 0, &desired()).is_err());
        assert!(provider_account_from_desired(
            CloudResourceId::new("account-overflow").unwrap(),
            (i64::MAX as u64) + 1,
            &desired(),
        )
        .is_err());
        let mut invalid_stored = provider_account_from_desired(
            CloudResourceId::new("account-stored-overflow").unwrap(),
            i64::MAX as u64,
            &desired(),
        )
        .unwrap();
        invalid_stored.metadata.generation = (i64::MAX as u64) + 1;
        assert!(validate_stored_provider_account(&invalid_stored).is_err());
    }

    #[test]
    fn persistence_bounds_reject_oversized_or_unsafe_metadata_and_aliases() {
        let mut value = desired();
        value.display_name = "x".repeat(MAX_DISPLAY_NAME_BYTES + 1);
        assert!(value.validate().is_err());
        let mut value = desired();
        value.owner = Some(" owner".to_string());
        assert!(value.validate().is_err());
        let mut value = desired();
        value.labels = (0..=MAX_LABELS)
            .map(|index| (format!("key-{index}"), String::new()))
            .collect();
        assert!(value.validate().is_err());
        let mut value = desired();
        value.spec.credential_source = CredentialSource::StaticSecret {
            credential_ref: CredentialRef::new("x".repeat(MAX_CREDENTIAL_REF_BYTES + 1)).unwrap(),
        };
        assert!(value.validate().is_err());
        let mut value = desired();
        value.spec.credential_source = CredentialSource::Federated {
            subject_token_ref: None,
            target_principal: "p".repeat(MAX_DESIRED_PAYLOAD_BYTES),
            audience: None,
        };
        assert!(value.validate().is_err());
    }
}
