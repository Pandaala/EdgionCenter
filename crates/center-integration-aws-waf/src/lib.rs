//! Default-off production composition for AWS WAFv2.
//!
//! This crate is the only layer that may create an AWS SDK transport. It keeps
//! ambient credentials, account generation authority and write admission out
//! of the Admin DTO layer.

mod ownership;

use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_adapter_aws_waf::{
    AwsWafAction, AwsWafAdapter, AwsWafApi, AwsWafAssociation, AwsWafAssociationTarget,
    AwsWafIpAddressVersion, AwsWafIpSet, AwsWafIpSetId, AwsWafIpSetReference, AwsWafLockToken,
    AwsWafManagedRuleGroup, AwsWafManagedRuleOverride, AwsWafManagedRuleOverrideAction,
    AwsWafMutationDispatch, AwsWafRateAggregateKey, AwsWafRegionalResourceKind, AwsWafRule,
    AwsWafRuleOwner, AwsWafScope, AwsWafSdkApi, AwsWafStatement, AwsWafVisibilityConfig,
    AwsWafWebAcl, AwsWafWebAclId,
};
use edgion_center_adapter_credential_files::{
    CredentialPurpose, MountedCredentialResolver, ResolveCredentialRequest,
};
use edgion_center_app::api::aws_waf::{
    AwsWafActionDto, AwsWafAddressVersionDto, AwsWafAdminError, AwsWafAdminService,
    AwsWafAssociationDto, AwsWafCapacityDto, AwsWafCatalogDto, AwsWafDefaultActionDto,
    AwsWafDeleteRequest, AwsWafIpSetCreateRequest, AwsWafIpSetDto, AwsWafIpSetReferenceDto,
    AwsWafIpSetUpdateRequest, AwsWafManagedExceptionRequest, AwsWafManagedRuleOverrideActionDto,
    AwsWafRegionalAssociationRequest, AwsWafRegionalDetachRequest, AwsWafRegionalResourceKindDto,
    AwsWafRuleDto, AwsWafRuleOwnershipDto, AwsWafRuleSecurityWeakenRequest, AwsWafRuleWriteRequest,
    AwsWafScopeDto, AwsWafStatementDto, AwsWafVisibilityDto, AwsWafWebAclCreateRequest,
    AwsWafWebAclDetailDto, AwsWafWebAclDto, AwsWafWebAclSecurityWeakenRequest,
    AwsWafWebAclUpdateRequest, SharedAwsWafAdminService,
};
use edgion_center_core::{
    CloudProvider, CloudResourceId, CoreError, CoreResult, CredentialRef, CredentialSource,
    NormalizedProviderError, ProviderAccount, ProviderAccountScope, ProviderAccountStore,
    ProviderErrorCategory,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 120;
const DEFAULT_GLOBAL_CONCURRENCY: usize = 4;
const MAX_GLOBAL_CONCURRENCY: usize = 16;
const DEFAULT_ACCOUNT_CONCURRENCY: usize = 1;
const MAX_ACCOUNT_CONCURRENCY: usize = 2;
const MAX_TRACKED_ACCOUNTS: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AwsWafConfig {
    pub read_enabled: bool,
    pub write_enabled: bool,
    pub attach_enabled: bool,
    pub detach_enabled: bool,
    pub security_weaken_enabled: bool,
    /// Mounted 32-byte HMAC key used only to prove Center rule ownership.
    pub ownership_hmac_key_ref: Option<String>,
    /// A composition-owned ceiling. Provider CheckCapacity remains mandatory;
    /// a missing or zero ceiling rejects every write.
    pub account_wcu_ceiling: u32,
    pub operation_timeout_secs: u64,
    pub global_concurrency: usize,
    pub per_account_concurrency: usize,
}

impl Default for AwsWafConfig {
    fn default() -> Self {
        Self {
            read_enabled: false,
            write_enabled: false,
            attach_enabled: false,
            detach_enabled: false,
            security_weaken_enabled: false,
            ownership_hmac_key_ref: None,
            account_wcu_ceiling: 0,
            operation_timeout_secs: DEFAULT_TIMEOUT_SECS,
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            per_account_concurrency: DEFAULT_ACCOUNT_CONCURRENCY,
        }
    }
}

impl AwsWafConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=MAX_TIMEOUT_SECS).contains(&self.operation_timeout_secs)
            || !(1..=MAX_GLOBAL_CONCURRENCY).contains(&self.global_concurrency)
            || !(1..=MAX_ACCOUNT_CONCURRENCY).contains(&self.per_account_concurrency)
            || self.per_account_concurrency > self.global_concurrency
        {
            return Err("invalid_aws_waf_limits");
        }
        if self.write_enabled && self.account_wcu_ceiling == 0 {
            return Err("aws_waf_wcu_ceiling_required");
        }
        if self.enabled()
            && self
                .ownership_hmac_key_ref
                .as_deref()
                .is_none_or(|value| CredentialRef::new(value.to_string()).is_err())
        {
            return Err("aws_waf_ownership_hmac_key_required");
        }
        Ok(())
    }
    pub fn enabled(&self) -> bool {
        self.read_enabled
            || self.write_enabled
            || self.attach_enabled
            || self.detach_enabled
            || self.security_weaken_enabled
    }
}

/// Composes the default-off AWS WAF inventory service. It accepts only an AWS
/// ProviderAccount backed by ambient credentials; every request verifies the
/// AWS identity with STS before WAF calls and rechecks the persisted account
/// generation before returning data.
pub fn compose_aws_waf_admin(
    config: &AwsWafConfig,
    account_store: Option<Arc<dyn ProviderAccountStore>>,
    mounted_resolver: Option<Arc<MountedCredentialResolver>>,
) -> CoreResult<Option<SharedAwsWafAdminService>> {
    config
        .validate()
        .map_err(|message| CoreError::Conflict(message.to_string()))?;
    if !config.enabled() {
        return Ok(None);
    }
    let account_store = account_store.ok_or_else(|| {
        CoreError::Conflict("AWS WAF requires a provider account store".to_string())
    })?;
    let ownership_key_ref = CredentialRef::new(
        config
            .ownership_hmac_key_ref
            .clone()
            .expect("validated ownership key ref"),
    )?;
    let mounted_resolver = mounted_resolver.ok_or_else(|| {
        CoreError::Conflict("AWS WAF requires mounted ownership credentials".to_string())
    })?;
    Ok(Some(Arc::new(AwsWafReadService {
        account_store,
        mounted_resolver,
        ownership_key_ref,
        write_enabled: config.write_enabled,
        attach_enabled: config.attach_enabled,
        detach_enabled: config.detach_enabled,
        security_weaken_enabled: config.security_weaken_enabled,
        account_wcu_ceiling: config.account_wcu_ceiling,
        timeout: Duration::from_secs(config.operation_timeout_secs),
        global: Arc::new(Semaphore::new(config.global_concurrency)),
        per_account_concurrency: config.per_account_concurrency,
        accounts: Mutex::new(HashMap::new()),
        api_factory: Arc::new(ProductionApiFactory),
    })))
}

#[async_trait]
trait ApiFactory: Send + Sync {
    async fn build(&self) -> Result<Arc<dyn AwsWafApi>, NormalizedProviderError>;
}

struct ProductionApiFactory;

#[async_trait]
impl ApiFactory for ProductionApiFactory {
    async fn build(&self) -> Result<Arc<dyn AwsWafApi>, NormalizedProviderError> {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        AwsWafSdkApi::new(&config)
            .await
            .map(|api| Arc::new(api) as Arc<dyn AwsWafApi>)
    }
}

struct RequestAuthority {
    account: ProviderAccount,
    ownership_key: ownership::OwnershipKey,
    ownership_key_revision: String,
}

struct AwsWafReadService {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    ownership_key_ref: CredentialRef,
    write_enabled: bool,
    attach_enabled: bool,
    detach_enabled: bool,
    security_weaken_enabled: bool,
    account_wcu_ceiling: u32,
    timeout: Duration,
    global: Arc<Semaphore>,
    per_account_concurrency: usize,
    accounts: Mutex<HashMap<CloudResourceId, Weak<Semaphore>>>,
    api_factory: Arc<dyn ApiFactory>,
}

impl AwsWafReadService {
    async fn prepare(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<(AwsWafAdapter, RequestAuthority), AwsWafAdminError> {
        let account = self
            .account_store
            .get(account_id)
            .await
            .map_err(|_| AwsWafAdminError::Unavailable)?
            .ok_or(AwsWafAdminError::NotFound)?;
        validate_account(account_id, &account)?;
        let resolved = self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: account_id,
                provider: &CloudProvider::Aws,
                purpose: CredentialPurpose::AwsWafOwnershipHmac,
                credential_ref: &self.ownership_key_ref,
            })
            .await
            .map_err(|_| AwsWafAdminError::Unavailable)?;
        let ownership_key_revision = resolved.revision().as_str().to_string();
        let ownership_key = resolved
            .with_bytes(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .map(ownership::OwnershipKey::new)
            .ok_or(AwsWafAdminError::Unavailable)?;
        let api = self
            .api_factory
            .build()
            .await
            .map_err(|_| AwsWafAdminError::Unavailable)?;
        let adapter = AwsWafAdapter::new(
            account.metadata.id.clone(),
            account.metadata.generation,
            &account.spec,
            api,
        )
        .map_err(|_| AwsWafAdminError::Unavailable)?;
        Ok((
            adapter,
            RequestAuthority {
                account,
                ownership_key,
                ownership_key_revision,
            },
        ))
    }

    async fn authority_is_current(&self, authority: &RequestAuthority) -> bool {
        if !matches!(
            self.account_store.get(&authority.account.metadata.id).await,
            Ok(Some(current)) if current == authority.account
        ) {
            return false;
        }
        matches!(self.mounted_resolver.resolve(ResolveCredentialRequest {
            provider_account_id: &authority.account.metadata.id,
            provider: &CloudProvider::Aws,
            purpose: CredentialPurpose::AwsWafOwnershipHmac,
            credential_ref: &self.ownership_key_ref,
        }).await, Ok(resolved) if resolved.revision().as_str() == authority.ownership_key_revision)
    }

    fn account_semaphore(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<Arc<Semaphore>, AwsWafAdminError> {
        let mut accounts = self
            .accounts
            .lock()
            .map_err(|_| AwsWafAdminError::Unavailable)?;
        if let Some(existing) = accounts.get(account_id).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        accounts.retain(|_, value| value.strong_count() > 0);
        if accounts.len() >= MAX_TRACKED_ACCOUNTS {
            return Err(AwsWafAdminError::Unavailable);
        }
        let semaphore = Arc::new(Semaphore::new(self.per_account_concurrency));
        accounts.insert(account_id.clone(), Arc::downgrade(&semaphore));
        Ok(semaphore)
    }

    async fn admission(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit), AwsWafAdminError> {
        let account = self
            .account_semaphore(account_id)?
            .acquire_owned()
            .await
            .map_err(|_| AwsWafAdminError::Unavailable)?;
        let global = self
            .global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| AwsWafAdminError::Unavailable)?;
        Ok((account, global))
    }

    async fn read<T>(
        &self,
        account: &CloudResourceId,
        operation: impl std::future::Future<Output = Result<T, AwsWafAdminError>>,
    ) -> Result<T, AwsWafAdminError> {
        tokio::time::timeout(self.timeout, async {
            let (_account, _global) = self.admission(account).await?;
            operation.await
        })
        .await
        .unwrap_or(Err(AwsWafAdminError::Unavailable))
    }

    /// A deadline before provider dispatch is retryable. Once the adapter has
    /// marked a provider mutation, the result is ambiguous and must be
    /// observed before any retry.
    async fn mutation<T>(
        &self,
        account: &CloudResourceId,
        dispatch: AwsWafMutationDispatch,
        operation: impl std::future::Future<Output = Result<T, AwsWafAdminError>>,
    ) -> Result<T, AwsWafAdminError> {
        self.mutation_allowed(account, dispatch, self.write_enabled, operation)
            .await
    }

    async fn mutation_allowed<T>(
        &self,
        account: &CloudResourceId,
        dispatch: AwsWafMutationDispatch,
        allowed: bool,
        operation: impl std::future::Future<Output = Result<T, AwsWafAdminError>>,
    ) -> Result<T, AwsWafAdminError> {
        if !allowed {
            return Err(AwsWafAdminError::Unavailable);
        }
        await_mutation(self.timeout, dispatch, async {
            let (_account, _global) = self.admission(account).await?;
            operation.await
        })
        .await
    }

    async fn mutate_acl(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        lock_token: &str,
        security_weaken: bool,
        change: impl FnOnce(AwsWafWebAcl) -> Result<AwsWafWebAcl, AwsWafAdminError>,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        if security_weaken && !self.security_weaken_enabled {
            return Err(AwsWafAdminError::Unavailable);
        }
        let scope = scope_from_dto(scope)?;
        let id = AwsWafWebAclId::new(id.to_string()).map_err(|_| AwsWafAdminError::Invalid)?;
        let lock_token = lock_token.to_string();
        let dispatch = AwsWafMutationDispatch::default();
        self.mutation(&account, dispatch.clone(), async {
            let (adapter, authority) = self.prepare(&account).await?;
            let current = adapter
                .get_web_acl(scope, &id)
                .await
                .map_err(map_provider_error)?
                .ok_or(AwsWafAdminError::NotFound)?;
            if current.lock_token.as_str() != lock_token {
                return Err(AwsWafAdminError::Conflict);
            }
            let desired = change(current.clone())?;
            let updated = adapter
                .update_web_acl_tracked(current.revision(), desired, &dispatch)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::UnknownOutcome);
            }
            acl_detail_dto(updated, &authority)
        })
        .await
    }

    /// Rebuilds an ACL from a fresh provider read. Center ownership is proved
    /// only by the HMAC-bound provider rule name; an unverified name is always
    /// preserved as external, even if it uses the Center prefix.
    async fn mutate_center_rules(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        lock_token: &str,
        security_weaken: bool,
        change: impl FnOnce(
            &AwsWafWebAcl,
            &RequestAuthority,
            &[edgion_center_adapter_aws_waf::AwsWafManagedRuleGroupCatalogEntry],
            &mut Vec<AwsWafRule>,
        ) -> Result<(), AwsWafAdminError>,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        if security_weaken && !self.security_weaken_enabled {
            return Err(AwsWafAdminError::Unavailable);
        }
        let scope = scope_from_dto(scope)?;
        let id = AwsWafWebAclId::new(id.to_string()).map_err(|_| AwsWafAdminError::Invalid)?;
        let lock_token = lock_token.to_string();
        let dispatch = AwsWafMutationDispatch::default();
        self.mutation(&account, dispatch.clone(), async {
            let (adapter, authority) = self.prepare(&account).await?;
            let current = adapter
                .get_web_acl(scope.clone(), &id)
                .await
                .map_err(map_provider_error)?
                .ok_or(AwsWafAdminError::NotFound)?;
            if current.lock_token.as_str() != lock_token {
                return Err(AwsWafAdminError::Conflict);
            }
            let catalog = adapter
                .managed_rule_group_catalog(scope.clone())
                .await
                .map_err(map_provider_error)?;
            let (external, mut center) = split_rules(&current, &authority)?;
            change(&current, &authority, &catalog, &mut center)?;
            validate_center_rules(&center)?;
            let mut desired = current.clone();
            // `external` retains the exact provider order. Center rules are a
            // separate managed set and cannot overwrite a provider-owned rule.
            desired.rules = external.into_iter().chain(center).collect();
            validate_rule_set(&desired.rules)?;
            let capacity = adapter
                .check_capacity(scope, &desired.rules, self.account_wcu_ceiling)
                .await
                .map_err(map_provider_error)?;
            desired.capacity = capacity.required_wcu;
            let updated = adapter
                .update_web_acl_tracked(current.revision(), desired, &dispatch)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::UnknownOutcome);
            }
            acl_detail_dto(updated, &authority)
        })
        .await
    }
}

#[async_trait]
impl AwsWafAdminService for AwsWafReadService {
    async fn list_web_acls(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
    ) -> Result<Vec<AwsWafWebAclDto>, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        self.read(&account, async {
            let (adapter, authority) = self.prepare(&account).await?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::Unavailable);
            }
            let result = adapter.inventory(scope).await.map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::Unavailable);
            }
            Ok(result.into_iter().map(web_acl_dto).collect())
        })
        .await
    }

    async fn list_ip_sets(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
    ) -> Result<Vec<AwsWafIpSetDto>, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        self.read(&account, async {
            let (adapter, authority) = self.prepare(&account).await?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::Unavailable);
            }
            let result = adapter
                .inventory_ip_sets(scope)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::Unavailable);
            }
            Ok(result.into_iter().map(ip_set_dto).collect())
        })
        .await
    }

    async fn create_ip_set(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        request: AwsWafIpSetCreateRequest,
    ) -> Result<AwsWafIpSetDto, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        let addresses = bounded_addresses(request.addresses)?;
        let dispatch = AwsWafMutationDispatch::default();
        self.mutation(&account, dispatch.clone(), async {
            let (adapter, authority) = self.prepare(&account).await?;
            let Some(ProviderAccountScope::Aws { account_id }) =
                authority.account.spec.scope.as_ref()
            else {
                return Err(AwsWafAdminError::Invalid);
            };
            let created = adapter
                .create_ip_set_tracked(
                    AwsWafIpSet {
                        // AWS chooses both values. The adapter validates this
                        // typed placeholder before the one create dispatch.
                        id: AwsWafIpSetId::new("placeholder")
                            .map_err(|_| AwsWafAdminError::Invalid)?,
                        name: request.name,
                        arn: placeholder_ip_set_arn(&scope, account_id),
                        scope,
                        address_version: address_version_from_dto(request.address_version),
                        addresses,
                        lock_token: AwsWafLockToken::new("placeholder")
                            .map_err(|_| AwsWafAdminError::Invalid)?,
                    },
                    &dispatch,
                )
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::UnknownOutcome);
            }
            Ok(ip_set_dto(created))
        })
        .await
    }

    async fn update_ip_set(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        request: AwsWafIpSetUpdateRequest,
    ) -> Result<AwsWafIpSetDto, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        let id = AwsWafIpSetId::new(id.to_string()).map_err(|_| AwsWafAdminError::Invalid)?;
        let lock_token = request.lock_token;
        let addresses = bounded_addresses(request.addresses)?;
        let dispatch = AwsWafMutationDispatch::default();
        self.mutation(&account, dispatch.clone(), async {
            let (adapter, authority) = self.prepare(&account).await?;
            let current = adapter
                .get_ip_set(scope, &id)
                .await
                .map_err(map_provider_error)?
                .ok_or(AwsWafAdminError::NotFound)?;
            if current.lock_token.as_str() != lock_token {
                return Err(AwsWafAdminError::Conflict);
            }
            let mut desired = current.clone();
            desired.addresses = addresses;
            let updated = adapter
                .update_ip_set_tracked(current.revision(), desired, &dispatch)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::UnknownOutcome);
            }
            Ok(ip_set_dto(updated))
        })
        .await
    }

    async fn delete_ip_set(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        request: AwsWafDeleteRequest,
    ) -> Result<(), AwsWafAdminError> {
        if !self.security_weaken_enabled {
            return Err(AwsWafAdminError::Unavailable);
        }
        let scope = scope_from_dto(scope)?;
        let id = AwsWafIpSetId::new(id.to_string()).map_err(|_| AwsWafAdminError::Invalid)?;
        let lock_token = request.lock_token;
        let dispatch = AwsWafMutationDispatch::default();
        self.mutation(&account, dispatch.clone(), async {
            let (adapter, authority) = self.prepare(&account).await?;
            let current = adapter
                .get_ip_set(scope, &id)
                .await
                .map_err(map_provider_error)?
                .ok_or(AwsWafAdminError::NotFound)?;
            if current.lock_token.as_str() != lock_token {
                return Err(AwsWafAdminError::Conflict);
            }
            adapter
                .delete_ip_set_tracked(current.revision(), &dispatch)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::UnknownOutcome);
            }
            Ok(())
        })
        .await
    }

    async fn managed_catalog(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
    ) -> Result<Vec<AwsWafCatalogDto>, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        self.read(&account, async {
            let (adapter, authority) = self.prepare(&account).await?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::Unavailable);
            }
            let result = adapter
                .managed_rule_group_catalog(scope)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::Unavailable);
            }
            Ok(result
                .into_iter()
                .map(|value| AwsWafCatalogDto {
                    vendor_name: value.vendor_name,
                    name: value.name,
                    versions: value.versions.into_iter().collect(),
                })
                .collect())
        })
        .await
    }

    async fn get_web_acl(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        let id = AwsWafWebAclId::new(id.to_string()).map_err(|_| AwsWafAdminError::Invalid)?;
        self.read(&account, async {
            let (adapter, authority) = self.prepare(&account).await?;
            let value = adapter
                .get_web_acl(scope, &id)
                .await
                .map_err(map_provider_error)?
                .ok_or(AwsWafAdminError::NotFound)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::Unavailable);
            }
            acl_detail_dto(value, &authority)
        })
        .await
    }

    async fn create_web_acl(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        request: AwsWafWebAclCreateRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        let dispatch = AwsWafMutationDispatch::default();
        self.mutation(&account, dispatch.clone(), async {
            let (adapter, authority) = self.prepare(&account).await?;
            let created = adapter
                .create_web_acl_tracked(
                    edgion_center_adapter_aws_waf::AwsWafCreateWebAclRequest {
                        name: request.name,
                        scope,
                        default_action: default_action_from_dto(request.default_action),
                        visibility: visibility_from_dto(request.visibility),
                        rules: Vec::new(),
                        capability: edgion_center_adapter_aws_waf::AwsWafCapabilityEvidence {
                            managed_rule_groups: Default::default(),
                            challenge_allowed: false,
                            captcha_allowed: false,
                            maximum_wcu: self.account_wcu_ceiling,
                        },
                    },
                    &dispatch,
                )
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::UnknownOutcome);
            }
            acl_detail_dto(created, &authority)
        })
        .await
    }

    async fn update_web_acl(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        request: AwsWafWebAclUpdateRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        self.mutate_acl(account, scope, id, &request.lock_token, false, |mut acl| {
            acl.visibility = visibility_from_dto(request.visibility);
            Ok(acl)
        })
        .await
    }

    async fn security_weaken_web_acl(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        request: AwsWafWebAclSecurityWeakenRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        self.mutate_acl(account, scope, id, &request.lock_token, true, |mut acl| {
            acl.default_action = default_action_from_dto(request.default_action);
            Ok(acl)
        })
        .await
    }

    async fn delete_web_acl(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        request: AwsWafDeleteRequest,
    ) -> Result<(), AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        let id = AwsWafWebAclId::new(id.to_string()).map_err(|_| AwsWafAdminError::Invalid)?;
        let dispatch = AwsWafMutationDispatch::default();
        self.mutation(&account, dispatch.clone(), async {
            let (adapter, authority) = self.prepare(&account).await?;
            let acl = adapter
                .get_web_acl(scope, &id)
                .await
                .map_err(map_provider_error)?
                .ok_or(AwsWafAdminError::NotFound)?;
            if acl.lock_token.as_str() != request.lock_token {
                return Err(AwsWafAdminError::Conflict);
            }
            if !adapter
                .list_associations(acl.scope.clone(), &acl.id)
                .await
                .map_err(map_provider_error)?
                .is_empty()
            {
                return Err(AwsWafAdminError::Conflict);
            }
            adapter
                .delete_web_acl_tracked(acl.revision(), &dispatch)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::UnknownOutcome);
            }
            Ok(())
        })
        .await
    }

    async fn list_rules(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
    ) -> Result<Vec<AwsWafRuleDto>, AwsWafAdminError> {
        Ok(self.get_web_acl(account, scope, id).await?.rules)
    }

    async fn create_rule(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        request: AwsWafRuleWriteRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        let lock_token = request.lock_token.clone();
        self.mutate_center_rules(
            account,
            scope,
            id,
            &lock_token,
            false,
            |acl, authority, catalog, rules| {
                if rules
                    .iter()
                    .any(|rule| owned_reference(rule) == Some(request.reference.as_str()))
                {
                    return Err(AwsWafAdminError::Conflict);
                }
                let candidate = owned_rule(request, acl, authority, catalog)?;
                if matches!(candidate.statement, AwsWafStatement::ManagedRuleGroup(_))
                    && candidate.managed_override_action
                        != Some(AwsWafManagedRuleOverrideAction::None)
                {
                    return Err(AwsWafAdminError::Invalid);
                }
                rules.push(candidate);
                Ok(())
            },
        )
        .await
    }

    async fn update_rule(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        reference: &str,
        request: AwsWafRuleWriteRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        let lock_token = request.lock_token.clone();
        let reference = reference.to_string();
        self.mutate_center_rules(
            account,
            scope,
            id,
            &lock_token,
            false,
            move |acl, authority, catalog, rules| {
                let position = rules
                    .iter()
                    .position(|rule| owned_reference(rule) == Some(reference.as_str()))
                    .ok_or(AwsWafAdminError::NotFound)?;
                let replacement = owned_rule(request, acl, authority, catalog)?;
                if !managed_override_update_allowed(&rules[position], &replacement)
                    || weakens_action(rules[position].action, replacement.action)
                {
                    return Err(AwsWafAdminError::Invalid);
                }
                rules[position] = replacement;
                Ok(())
            },
        )
        .await
    }

    async fn delete_rule(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        reference: &str,
        request: AwsWafDeleteRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        let lock_token = request.lock_token.clone();
        let reference = reference.to_string();
        self.mutate_center_rules(
            account,
            scope,
            id,
            &lock_token,
            true,
            move |_, _, _, rules| {
                let position = rules
                    .iter()
                    .position(|rule| owned_reference(rule) == Some(reference.as_str()))
                    .ok_or(AwsWafAdminError::NotFound)?;
                rules.remove(position);
                Ok(())
            },
        )
        .await
    }

    async fn security_weaken_rule(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        reference: &str,
        request: AwsWafRuleSecurityWeakenRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        let lock_token = request.lock_token.clone();
        let reference = reference.to_string();
        self.mutate_center_rules(
            account,
            scope,
            id,
            &lock_token,
            true,
            move |_, _, _, rules| {
                let rule = rules
                    .iter_mut()
                    .find(|rule| owned_reference(rule) == Some(reference.as_str()))
                    .ok_or(AwsWafAdminError::NotFound)?;
                match &rule.statement {
                    AwsWafStatement::ManagedRuleGroup(_) => {
                        if request.action.is_some()
                            || request.managed_override_action
                                != Some(AwsWafManagedRuleOverrideActionDto::Count)
                            || rule.managed_override_action
                                != Some(AwsWafManagedRuleOverrideAction::None)
                        {
                            return Err(AwsWafAdminError::Invalid);
                        }
                        rule.managed_override_action = Some(AwsWafManagedRuleOverrideAction::Count);
                    }
                    _ => {
                        if request.managed_override_action.is_some() {
                            return Err(AwsWafAdminError::Invalid);
                        }
                        let action =
                            action_from_dto(request.action.ok_or(AwsWafAdminError::Invalid)?)?;
                        if !weakens_action(rule.action, action) {
                            return Err(AwsWafAdminError::Invalid);
                        }
                        rule.action = action;
                    }
                }
                Ok(())
            },
        )
        .await
    }

    async fn apply_managed_exception(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        reference: &str,
        request: AwsWafManagedExceptionRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        let lock_token = request.lock_token.clone();
        let reference = reference.to_string();
        self.mutate_center_rules(
            account,
            scope,
            id,
            &lock_token,
            true,
            move |_, _, _, rules| {
                let rule = rules
                    .iter_mut()
                    .find(|rule| owned_reference(rule) == Some(reference.as_str()))
                    .ok_or(AwsWafAdminError::NotFound)?;
                let AwsWafStatement::ManagedRuleGroup(group) = &mut rule.statement else {
                    return Err(AwsWafAdminError::Invalid);
                };
                if let Some(excluded_rules) = request.excluded_rules {
                    group.excluded_rules = excluded_rules.into_iter().collect();
                }
                if let Some(rule_action_overrides) = request.rule_action_overrides {
                    group.rule_action_overrides = rule_action_overrides
                        .into_iter()
                        .map(|item| {
                            Ok(AwsWafManagedRuleOverride {
                                name: item.name,
                                action: action_from_dto(item.action)?,
                            })
                        })
                        .collect::<Result<_, AwsWafAdminError>>()?;
                }
                Ok(())
            },
        )
        .await
    }

    async fn list_associations(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
    ) -> Result<Vec<AwsWafAssociationDto>, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        let id = AwsWafWebAclId::new(id.to_string()).map_err(|_| AwsWafAdminError::Invalid)?;
        self.read(&account, async {
            let (adapter, authority) = self.prepare(&account).await?;
            let result = adapter
                .list_associations(scope, &id)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::Unavailable);
            }
            result.into_iter().map(association_dto).collect()
        })
        .await
    }

    async fn associate_regional_resource(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        id: &str,
        request: AwsWafRegionalAssociationRequest,
    ) -> Result<Vec<AwsWafAssociationDto>, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        let AwsWafScope::Regional { .. } = scope else {
            return Err(AwsWafAdminError::Invalid);
        };
        let id = AwsWafWebAclId::new(id.to_string()).map_err(|_| AwsWafAdminError::Invalid)?;
        let target = AwsWafAssociationTarget {
            resource_arn: request.resource_arn,
            resource_kind: regional_kind_from_dto(request.resource_kind),
        };
        let dispatch = AwsWafMutationDispatch::default();
        self.mutation_allowed(&account, dispatch.clone(), self.attach_enabled, async {
            let (adapter, authority) = self.prepare(&account).await?;
            adapter
                .associate_regional_resource_tracked(scope.clone(), target, id.clone(), &dispatch)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::UnknownOutcome);
            }
            adapter
                .list_associations(scope, &id)
                .await
                .map_err(map_provider_error)?
                .into_iter()
                .map(association_dto)
                .collect()
        })
        .await
    }

    async fn disassociate_regional_resource(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        request: AwsWafRegionalDetachRequest,
    ) -> Result<(), AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        let AwsWafScope::Regional { .. } = scope else {
            return Err(AwsWafAdminError::Invalid);
        };
        let target = AwsWafAssociationTarget {
            resource_arn: request.resource_arn,
            resource_kind: regional_kind_from_dto(request.resource_kind),
        };
        let dispatch = AwsWafMutationDispatch::default();
        self.mutation_allowed(&account, dispatch.clone(), self.detach_enabled, async {
            let (adapter, authority) = self.prepare(&account).await?;
            adapter
                .disassociate_regional_resource_tracked(scope, target, &dispatch)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::UnknownOutcome);
            }
            Ok(())
        })
        .await
    }

    async fn check_capacity(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
        request: edgion_center_app::api::aws_waf::AwsWafCapacityRequest,
    ) -> Result<AwsWafCapacityDto, AwsWafAdminError> {
        let scope = scope_from_dto(scope)?;
        if request.rules.len() > 1_500 {
            return Err(AwsWafAdminError::Invalid);
        }
        self.read(&account, async {
            let (adapter, authority) = self.prepare(&account).await?;
            let catalog = adapter
                .managed_rule_group_catalog(scope.clone())
                .await
                .map_err(map_provider_error)?;
            let rules = request
                .rules
                .into_iter()
                .map(|rule| capacity_rule(rule, &catalog))
                .collect::<Result<Vec<_>, _>>()?;
            let observation = adapter
                .check_capacity(scope, &rules, self.account_wcu_ceiling)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(AwsWafAdminError::Unavailable);
            }
            Ok(AwsWafCapacityDto {
                required_wcu: observation.required_wcu,
                allowed: true,
                reason: "within_configured_wcu_ceiling".to_string(),
            })
        })
        .await
    }
}

fn scope_from_dto(scope: AwsWafScopeDto) -> Result<AwsWafScope, AwsWafAdminError> {
    let value = match scope {
        AwsWafScopeDto::Cloudfront => AwsWafScope::Cloudfront,
        AwsWafScopeDto::Regional { region } => AwsWafScope::Regional { region },
    };
    value.validate().map_err(|_| AwsWafAdminError::Invalid)?;
    Ok(value)
}

fn scope_dto(scope: AwsWafScope) -> AwsWafScopeDto {
    match scope {
        AwsWafScope::Cloudfront => AwsWafScopeDto::Cloudfront,
        AwsWafScope::Regional { region } => AwsWafScopeDto::Regional { region },
    }
}

fn web_acl_dto(value: edgion_center_adapter_aws_waf::AwsWafWebAcl) -> AwsWafWebAclDto {
    AwsWafWebAclDto {
        id: value.id.as_str().to_string(),
        name: value.name,
        arn: value.arn,
        scope: scope_dto(value.scope),
        capacity: value.capacity,
        lock_token_present: true,
    }
}

fn ip_set_dto(value: edgion_center_adapter_aws_waf::AwsWafIpSet) -> AwsWafIpSetDto {
    AwsWafIpSetDto {
        id: value.id.as_str().to_string(),
        name: value.name,
        arn: value.arn,
        scope: scope_dto(value.scope),
        address_version: match value.address_version {
            AwsWafIpAddressVersion::Ipv4 => "ipv4",
            AwsWafIpAddressVersion::Ipv6 => "ipv6",
        }
        .to_string(),
        addresses: value.addresses.into_iter().collect(),
        lock_token: value.lock_token.as_str().to_string(),
    }
}

fn acl_detail_dto(
    value: AwsWafWebAcl,
    authority: &RequestAuthority,
) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
    let Some(ProviderAccountScope::Aws { account_id }) = authority.account.spec.scope.as_ref()
    else {
        return Err(AwsWafAdminError::Invalid);
    };
    let mut references = std::collections::BTreeSet::new();
    let mut rules = Vec::with_capacity(value.rules.len());
    for rule in value.rules {
        let reference = authority.ownership_key.verify(
            authority.account.metadata.id.as_str(),
            account_id,
            &value.scope,
            &value.name,
            &rule.name,
        );
        if let Some(reference) = &reference {
            if !references.insert(reference.clone()) {
                return Err(AwsWafAdminError::Conflict);
            }
        }
        rules.push(rule_dto(rule, reference)?);
    }
    Ok(AwsWafWebAclDetailDto {
        id: value.id.as_str().to_string(),
        name: value.name,
        arn: value.arn,
        scope: scope_dto(value.scope),
        default_action: match value.default_action {
            edgion_center_adapter_aws_waf::AwsWafDefaultAction::Allow => {
                AwsWafDefaultActionDto::Allow
            }
            edgion_center_adapter_aws_waf::AwsWafDefaultAction::Block => {
                AwsWafDefaultActionDto::Block
            }
        },
        visibility: visibility_dto(value.visibility),
        capacity: value.capacity,
        lock_token: value.lock_token.as_str().to_string(),
        rules,
    })
}
fn rule_dto(
    value: AwsWafRule,
    reference: Option<String>,
) -> Result<AwsWafRuleDto, AwsWafAdminError> {
    let name = reference.clone().unwrap_or_else(|| value.name.clone());
    Ok(AwsWafRuleDto {
        name,
        priority: value.priority,
        action: (!matches!(value.statement, AwsWafStatement::ManagedRuleGroup(_)))
            .then_some(action_dto(value.action)),
        statement: statement_dto(value.statement)?,
        visibility: visibility_dto(value.visibility),
        ownership: if reference.is_some() {
            AwsWafRuleOwnershipDto::CenterOwned
        } else {
            AwsWafRuleOwnershipDto::External
        },
        managed_override_action: value.managed_override_action.map(|action| match action {
            AwsWafManagedRuleOverrideAction::None => AwsWafManagedRuleOverrideActionDto::None,
            AwsWafManagedRuleOverrideAction::Count => AwsWafManagedRuleOverrideActionDto::Count,
        }),
        reference,
    })
}
fn visibility_dto(
    value: edgion_center_adapter_aws_waf::AwsWafVisibilityConfig,
) -> AwsWafVisibilityDto {
    AwsWafVisibilityDto {
        cloudwatch_metrics_enabled: value.cloudwatch_metrics_enabled,
        sampled_requests_enabled: value.sampled_requests_enabled,
        metric_name: value.metric_name,
    }
}
fn visibility_from_dto(value: AwsWafVisibilityDto) -> AwsWafVisibilityConfig {
    AwsWafVisibilityConfig {
        cloudwatch_metrics_enabled: value.cloudwatch_metrics_enabled,
        sampled_requests_enabled: value.sampled_requests_enabled,
        metric_name: value.metric_name,
    }
}
fn default_action_from_dto(
    value: AwsWafDefaultActionDto,
) -> edgion_center_adapter_aws_waf::AwsWafDefaultAction {
    match value {
        AwsWafDefaultActionDto::Allow => edgion_center_adapter_aws_waf::AwsWafDefaultAction::Allow,
        AwsWafDefaultActionDto::Block => edgion_center_adapter_aws_waf::AwsWafDefaultAction::Block,
    }
}
fn action_dto(value: AwsWafAction) -> AwsWafActionDto {
    match value {
        AwsWafAction::Allow => AwsWafActionDto::Allow,
        AwsWafAction::Block => AwsWafActionDto::Block,
        AwsWafAction::Count => AwsWafActionDto::Count,
        AwsWafAction::Challenge => AwsWafActionDto::Challenge,
        AwsWafAction::Captcha => AwsWafActionDto::Captcha,
    }
}
fn statement_dto(value: AwsWafStatement) -> Result<AwsWafStatementDto, AwsWafAdminError> {
    Ok(match value {
        AwsWafStatement::ManagedRuleGroup(group) => AwsWafStatementDto::ManagedRuleGroup {
            vendor_name: group.vendor_name,
            name: group.name,
            version: group.version,
            excluded_rules: group.excluded_rules.into_iter().collect(),
            rule_action_overrides: group
                .rule_action_overrides
                .into_iter()
                .map(
                    |value| edgion_center_app::api::aws_waf::AwsWafManagedRuleOverrideDto {
                        name: value.name,
                        action: action_dto(value.action),
                    },
                )
                .collect(),
        },
        AwsWafStatement::IpSetReference(value) => {
            AwsWafStatementDto::IpSetReference { arn: value.arn }
        }
        AwsWafStatement::RateBased {
            limit,
            aggregate_key: AwsWafRateAggregateKey::Ip,
            scope_down_ip_set,
        } => AwsWafStatementDto::RateBased {
            limit,
            scope_down_ip_set: scope_down_ip_set
                .map(|value| AwsWafIpSetReferenceDto { arn: value.arn }),
        },
    })
}
fn association_dto(value: AwsWafAssociation) -> Result<AwsWafAssociationDto, AwsWafAdminError> {
    Ok(AwsWafAssociationDto {
        resource_arn: value.target.resource_arn,
        resource_kind: match value.target.resource_kind {
            AwsWafRegionalResourceKind::ApplicationLoadBalancer => {
                AwsWafRegionalResourceKindDto::ApplicationLoadBalancer
            }
            AwsWafRegionalResourceKind::ApiGatewayStage => {
                AwsWafRegionalResourceKindDto::ApiGatewayStage
            }
            AwsWafRegionalResourceKind::AppSyncApi => AwsWafRegionalResourceKindDto::AppSyncApi,
            AwsWafRegionalResourceKind::CognitoUserPool => {
                AwsWafRegionalResourceKindDto::CognitoUserPool
            }
        },
        web_acl_id: value.web_acl_id.as_str().to_string(),
        target_deployment_authority: "aws_waf_regional".to_string(),
    })
}

fn capacity_rule(
    value: AwsWafRuleWriteRequest,
    catalog: &[edgion_center_adapter_aws_waf::AwsWafManagedRuleGroupCatalogEntry],
) -> Result<AwsWafRule, AwsWafAdminError> {
    let is_managed_rule_group =
        matches!(value.statement, AwsWafStatementDto::ManagedRuleGroup { .. });
    let (action, managed_override_action) = if is_managed_rule_group {
        if value.action.is_some()
            || !matches!(
                value.managed_override_action,
                Some(
                    AwsWafManagedRuleOverrideActionDto::None
                        | AwsWafManagedRuleOverrideActionDto::Count
                )
            )
        {
            return Err(AwsWafAdminError::Invalid);
        }
        // The legacy field is never sent for a managed rule (see rule_dto),
        // but the internal rule shape keeps an action for non-managed rules.
        (
            AwsWafAction::Count,
            Some(
                match value.managed_override_action.expect("checked above") {
                    AwsWafManagedRuleOverrideActionDto::None => {
                        AwsWafManagedRuleOverrideAction::None
                    }
                    AwsWafManagedRuleOverrideActionDto::Count => {
                        AwsWafManagedRuleOverrideAction::Count
                    }
                },
            ),
        )
    } else {
        if value.managed_override_action.is_some() {
            return Err(AwsWafAdminError::Invalid);
        }
        (
            action_from_dto(value.action.ok_or(AwsWafAdminError::Invalid)?)?,
            None,
        )
    };
    let statement = match value.statement {
        AwsWafStatementDto::ManagedRuleGroup {
            vendor_name,
            name,
            version,
            excluded_rules,
            rule_action_overrides,
        } => {
            let known = catalog
                .iter()
                .find(|entry| entry.vendor_name == vendor_name && entry.name == name)
                .filter(|entry| {
                    version
                        .as_ref()
                        .is_none_or(|version| entry.versions.contains(version))
                })
                .ok_or(AwsWafAdminError::Invalid)?;
            let _ = known;
            AwsWafStatement::ManagedRuleGroup(AwsWafManagedRuleGroup {
                vendor_name,
                name,
                version,
                capacity: 1,
                excluded_rules: excluded_rules.into_iter().collect(),
                rule_action_overrides: rule_action_overrides
                    .into_iter()
                    .map(|item| {
                        Ok(AwsWafManagedRuleOverride {
                            name: item.name,
                            action: action_from_dto(item.action)?,
                        })
                    })
                    .collect::<Result<_, AwsWafAdminError>>()?,
            })
        }
        AwsWafStatementDto::IpSetReference { arn } => {
            AwsWafStatement::IpSetReference(AwsWafIpSetReference { arn })
        }
        AwsWafStatementDto::RateBased {
            limit,
            scope_down_ip_set,
        } => AwsWafStatement::RateBased {
            limit,
            aggregate_key: AwsWafRateAggregateKey::Ip,
            scope_down_ip_set: scope_down_ip_set.map(|item| AwsWafIpSetReference { arn: item.arn }),
        },
    };
    Ok(AwsWafRule {
        name: value.name,
        priority: value.priority,
        action,
        statement,
        visibility: AwsWafVisibilityConfig {
            cloudwatch_metrics_enabled: value.visibility.cloudwatch_metrics_enabled,
            sampled_requests_enabled: value.visibility.sampled_requests_enabled,
            metric_name: value.visibility.metric_name,
        },
        owner: AwsWafRuleOwner::Center {
            reference: value.reference,
        },
        managed_override_action,
    })
}

fn owned_reference(rule: &AwsWafRule) -> Option<&str> {
    match &rule.owner {
        AwsWafRuleOwner::Center { reference } => Some(reference),
        AwsWafRuleOwner::External => None,
    }
}

/// A normal update may preserve a previous managed `Count` override or
/// strengthen it back to `None`; only introducing `Count` is a weakening and
/// must use the separate security-weaken endpoint.
fn managed_override_update_allowed(current: &AwsWafRule, desired: &AwsWafRule) -> bool {
    match (&current.statement, &desired.statement) {
        (AwsWafStatement::ManagedRuleGroup(_), AwsWafStatement::ManagedRuleGroup(_)) => {
            !(current.managed_override_action == Some(AwsWafManagedRuleOverrideAction::None)
                && desired.managed_override_action == Some(AwsWafManagedRuleOverrideAction::Count))
        }
        (AwsWafStatement::ManagedRuleGroup(_), _) | (_, AwsWafStatement::ManagedRuleGroup(_)) => {
            false
        }
        _ => true,
    }
}

fn split_rules(
    acl: &AwsWafWebAcl,
    authority: &RequestAuthority,
) -> Result<(Vec<AwsWafRule>, Vec<AwsWafRule>), AwsWafAdminError> {
    let Some(ProviderAccountScope::Aws { account_id }) = authority.account.spec.scope.as_ref()
    else {
        return Err(AwsWafAdminError::Invalid);
    };
    let mut external = Vec::new();
    let mut center = Vec::new();
    let mut references = BTreeSet::new();
    for mut rule in acl.rules.clone() {
        let reference = authority.ownership_key.verify(
            authority.account.metadata.id.as_str(),
            account_id,
            &acl.scope,
            &acl.name,
            &rule.name,
        );
        if let Some(reference) = reference {
            if !references.insert(reference.clone()) {
                return Err(AwsWafAdminError::Conflict);
            }
            rule.owner = AwsWafRuleOwner::Center { reference };
            center.push(rule);
        } else {
            rule.owner = AwsWafRuleOwner::External;
            external.push(rule);
        }
    }
    Ok((external, center))
}

fn owned_rule(
    value: AwsWafRuleWriteRequest,
    acl: &AwsWafWebAcl,
    authority: &RequestAuthority,
    catalog: &[edgion_center_adapter_aws_waf::AwsWafManagedRuleGroupCatalogEntry],
) -> Result<AwsWafRule, AwsWafAdminError> {
    let Some(ProviderAccountScope::Aws { account_id }) = authority.account.spec.scope.as_ref()
    else {
        return Err(AwsWafAdminError::Invalid);
    };
    let reference = value.reference.clone();
    let mut rule = capacity_rule(value, catalog)?;
    rule.name = authority
        .ownership_key
        .rule_name(
            authority.account.metadata.id.as_str(),
            account_id,
            &acl.scope,
            &acl.name,
            &reference,
        )
        .ok_or(AwsWafAdminError::Invalid)?;
    rule.owner = AwsWafRuleOwner::Center { reference };
    Ok(rule)
}

fn validate_center_rules(rules: &[AwsWafRule]) -> Result<(), AwsWafAdminError> {
    let mut references = BTreeSet::new();
    let mut priorities = BTreeSet::new();
    for rule in rules {
        let reference = owned_reference(rule).ok_or(AwsWafAdminError::Invalid)?;
        if !references.insert(reference.to_string()) || !priorities.insert(rule.priority) {
            return Err(AwsWafAdminError::Conflict);
        }
    }
    Ok(())
}

fn validate_rule_set(rules: &[AwsWafRule]) -> Result<(), AwsWafAdminError> {
    let mut priorities = BTreeSet::new();
    let mut names = BTreeSet::new();
    if rules
        .iter()
        .any(|rule| !priorities.insert(rule.priority) || !names.insert(rule.name.clone()))
    {
        return Err(AwsWafAdminError::Conflict);
    }
    Ok(())
}

fn action_from_dto(value: AwsWafActionDto) -> Result<AwsWafAction, AwsWafAdminError> {
    Ok(match value {
        AwsWafActionDto::Allow => AwsWafAction::Allow,
        AwsWafActionDto::Block => AwsWafAction::Block,
        AwsWafActionDto::Count => AwsWafAction::Count,
        // Challenge/CAPTCHA require entitlement evidence that this bounded
        // service does not currently obtain from AWS. Never guess it.
        AwsWafActionDto::Challenge | AwsWafActionDto::Captcha => {
            return Err(AwsWafAdminError::Invalid)
        }
    })
}

fn weakens_action(current: AwsWafAction, desired: AwsWafAction) -> bool {
    fn rank(action: AwsWafAction) -> u8 {
        match action {
            AwsWafAction::Allow => 0,
            AwsWafAction::Count => 1,
            AwsWafAction::Challenge | AwsWafAction::Captcha => 2,
            AwsWafAction::Block => 3,
        }
    }
    rank(desired) < rank(current)
}

fn bounded_addresses(value: Vec<String>) -> Result<BTreeSet<String>, AwsWafAdminError> {
    if value.is_empty() || value.len() > 10_000 {
        return Err(AwsWafAdminError::Invalid);
    }
    let requested = value.len();
    let addresses = value.into_iter().collect::<BTreeSet<_>>();
    // Duplicates are rejected rather than silently changing the requested
    // set, and typed adapter validation checks every CIDR/version afterwards.
    if addresses.len() != requested {
        return Err(AwsWafAdminError::Invalid);
    }
    Ok(addresses)
}

fn placeholder_ip_set_arn(scope: &AwsWafScope, account: &str) -> String {
    let (region, scope_segment) = match scope {
        AwsWafScope::Cloudfront => ("us-east-1", "global"),
        AwsWafScope::Regional { region } => (region.as_str(), "regional"),
    };
    format!("arn:aws:wafv2:{region}:{account}:{scope_segment}/ipset/placeholder")
}
fn address_version_from_dto(value: AwsWafAddressVersionDto) -> AwsWafIpAddressVersion {
    match value {
        AwsWafAddressVersionDto::Ipv4 => AwsWafIpAddressVersion::Ipv4,
        AwsWafAddressVersionDto::Ipv6 => AwsWafIpAddressVersion::Ipv6,
    }
}
fn regional_kind_from_dto(value: AwsWafRegionalResourceKindDto) -> AwsWafRegionalResourceKind {
    match value {
        AwsWafRegionalResourceKindDto::ApplicationLoadBalancer => {
            AwsWafRegionalResourceKind::ApplicationLoadBalancer
        }
        AwsWafRegionalResourceKindDto::ApiGatewayStage => {
            AwsWafRegionalResourceKind::ApiGatewayStage
        }
        AwsWafRegionalResourceKindDto::AppSyncApi => AwsWafRegionalResourceKind::AppSyncApi,
        AwsWafRegionalResourceKindDto::CognitoUserPool => {
            AwsWafRegionalResourceKind::CognitoUserPool
        }
    }
}

fn validate_account(
    requested_account: &CloudResourceId,
    account: &ProviderAccount,
) -> Result<(), AwsWafAdminError> {
    if &account.metadata.id != requested_account
        || account.spec.provider != CloudProvider::Aws
        || account.spec.credential_source != CredentialSource::Ambient
        || !matches!(account.spec.scope, Some(ProviderAccountScope::Aws { .. }))
    {
        return Err(AwsWafAdminError::Invalid);
    }
    Ok(())
}

async fn await_mutation<T>(
    timeout: Duration,
    dispatch: AwsWafMutationDispatch,
    operation: impl std::future::Future<Output = Result<T, AwsWafAdminError>>,
) -> Result<T, AwsWafAdminError> {
    match tokio::time::timeout(timeout, operation).await {
        Ok(result) => result,
        Err(_) if dispatch.was_dispatched() => Err(AwsWafAdminError::UnknownOutcome),
        Err(_) => Err(AwsWafAdminError::Unavailable),
    }
}

fn map_provider_error(error: NormalizedProviderError) -> AwsWafAdminError {
    edgion_center_app::common::observe::cloud_metrics::record_provider_error(
        "aws",
        error.category(),
    );
    match error.category() {
        ProviderErrorCategory::NotFound => AwsWafAdminError::NotFound,
        ProviderErrorCategory::Conflict => AwsWafAdminError::Conflict,
        ProviderErrorCategory::UnknownOutcome => AwsWafAdminError::UnknownOutcome,
        ProviderErrorCategory::Validation | ProviderErrorCategory::Authorization => {
            AwsWafAdminError::Invalid
        }
        _ => AwsWafAdminError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(statement: AwsWafStatementDto) -> AwsWafRuleWriteRequest {
        AwsWafRuleWriteRequest {
            reference: "rule-one".to_string(),
            lock_token: "lock".to_string(),
            name: "rule-one".to_string(),
            priority: 1,
            action: Some(AwsWafActionDto::Block),
            managed_override_action: None,
            statement,
            visibility: AwsWafVisibilityDto {
                cloudwatch_metrics_enabled: true,
                sampled_requests_enabled: false,
                metric_name: "rule-one".to_string(),
            },
        }
    }
    #[test]
    fn default_is_closed_and_writes_require_a_ceiling() {
        assert!(!AwsWafConfig::default().enabled());
        let mut config = AwsWafConfig {
            write_enabled: true,
            ..Default::default()
        };
        assert_eq!(config.validate(), Err("aws_waf_wcu_ceiling_required"));
        config.account_wcu_ceiling = 1500;
        config.ownership_hmac_key_ref = Some("aws/waf-owner".to_string());
        assert!(config.validate().is_ok());
    }

    #[tokio::test]
    async fn mutation_timeout_before_dispatch_is_retryable() {
        let before_dispatch = AwsWafMutationDispatch::default();
        let result = await_mutation(
            Duration::from_millis(1),
            before_dispatch,
            std::future::pending::<Result<(), AwsWafAdminError>>(),
        )
        .await;
        assert_eq!(result, Err(AwsWafAdminError::Unavailable));
    }

    #[test]
    fn regional_scope_is_typed_and_invalid_regions_fail_closed() {
        assert!(matches!(
            scope_from_dto(AwsWafScopeDto::Regional {
                region: "us-west-2".to_string()
            }),
            Ok(AwsWafScope::Regional { .. })
        ));
        assert_eq!(
            scope_from_dto(AwsWafScopeDto::Regional {
                region: "not a region".to_string()
            }),
            Err(AwsWafAdminError::Invalid)
        );
    }

    #[test]
    fn capacity_mapper_accepts_ip_and_rate_and_rejects_unknown_managed_or_entitlement_actions() {
        assert!(capacity_rule(
            write(AwsWafStatementDto::IpSetReference {
                arn: "arn:aws:wafv2:us-west-2:123456789012:regional/ipset/example".to_string(),
            }),
            &[]
        )
        .is_ok());
        assert!(capacity_rule(
            write(AwsWafStatementDto::RateBased {
                limit: 100,
                scope_down_ip_set: None
            }),
            &[]
        )
        .is_ok());
        assert_eq!(
            capacity_rule(
                write(AwsWafStatementDto::ManagedRuleGroup {
                    vendor_name: "AWS".to_string(),
                    name: "Unknown".to_string(),
                    version: None,
                    excluded_rules: Vec::new(),
                    rule_action_overrides: Vec::new()
                }),
                &[]
            ),
            Err(AwsWafAdminError::Invalid)
        );
        let mut challenge = write(AwsWafStatementDto::RateBased {
            limit: 100,
            scope_down_ip_set: None,
        });
        challenge.action = Some(AwsWafActionDto::Challenge);
        assert_eq!(
            capacity_rule(challenge, &[]),
            Err(AwsWafAdminError::Invalid)
        );
    }

    #[test]
    fn rule_mutation_rejects_priority_collisions_and_normal_weakening() {
        let first = capacity_rule(
            write(AwsWafStatementDto::RateBased {
                limit: 100,
                scope_down_ip_set: None,
            }),
            &[],
        )
        .unwrap();
        let second = first.clone();
        assert_eq!(
            validate_rule_set(&[first, second]),
            Err(AwsWafAdminError::Conflict)
        );
        assert!(weakens_action(AwsWafAction::Block, AwsWafAction::Count));
        assert!(!weakens_action(AwsWafAction::Count, AwsWafAction::Block));
    }

    #[test]
    fn managed_count_can_be_previewed_and_preserved_but_not_introduced_by_update() {
        let catalog = vec![
            edgion_center_adapter_aws_waf::AwsWafManagedRuleGroupCatalogEntry {
                vendor_name: "AWS".to_string(),
                name: "AWSManagedRulesCommonRuleSet".to_string(),
                versions: Default::default(),
            },
        ];
        let statement = AwsWafStatementDto::ManagedRuleGroup {
            vendor_name: "AWS".to_string(),
            name: "AWSManagedRulesCommonRuleSet".to_string(),
            version: None,
            excluded_rules: Vec::new(),
            rule_action_overrides: Vec::new(),
        };
        let mut counted = write(statement.clone());
        counted.action = None;
        counted.managed_override_action = Some(AwsWafManagedRuleOverrideActionDto::Count);
        let counted = capacity_rule(counted, &catalog).unwrap();
        assert_eq!(
            counted.managed_override_action,
            Some(AwsWafManagedRuleOverrideAction::Count)
        );
        assert!(managed_override_update_allowed(&counted, &counted));
        let mut none = write(statement);
        none.action = None;
        none.managed_override_action = Some(AwsWafManagedRuleOverrideActionDto::None);
        let none = capacity_rule(none, &catalog).unwrap();
        assert!(!managed_override_update_allowed(&none, &counted));
        assert!(managed_override_update_allowed(&counted, &none));
    }

    #[test]
    fn attachment_only_configuration_is_enabled_without_write_capacity() {
        let config = AwsWafConfig {
            attach_enabled: true,
            ownership_hmac_key_ref: Some("aws/waf-owner".to_string()),
            ..Default::default()
        };
        assert!(config.enabled());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn ip_set_write_input_rejects_empty_and_duplicate_addresses() {
        assert_eq!(
            bounded_addresses(Vec::new()),
            Err(AwsWafAdminError::Invalid)
        );
        assert_eq!(
            bounded_addresses(vec!["192.0.2.0/24".to_string(), "192.0.2.0/24".to_string()]),
            Err(AwsWafAdminError::Invalid)
        );
        assert_eq!(
            bounded_addresses(vec!["192.0.2.0/24".to_string()])
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            vec!["192.0.2.0/24".to_string()]
        );
    }
}
