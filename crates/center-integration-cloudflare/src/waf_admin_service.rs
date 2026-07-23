//! Production composition for bounded Cloudflare Zone WAF operations.
//!
//! This service is deliberately independent from Cloudflare DNS composition.
//! It resolves one mounted API token for one exact ProviderAccount, builds a
//! fresh account-bound client for every operation, and never retries a
//! provider mutation after dispatch.

use std::{
    collections::{BTreeSet, HashMap},
    str,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_adapter_cloudflare::{
    CloudflareApi, CloudflareApiResult, CloudflareApiToken, CloudflareBatchRequest,
    CloudflareBatchResult, CloudflareCreateZoneRequest, CloudflareDeleteZoneAck, CloudflareDnssec,
    CloudflareHttpApi, CloudflarePage, CloudflareRecord, CloudflareWafAction as AdapterAction,
    CloudflareWafApi, CloudflareWafCustomRuleSpec, CloudflareWafExpression,
    CloudflareWafManagedExceptionSpec, CloudflareWafManagedRuleOverride,
    CloudflareWafManagedRuleSpec, CloudflareWafOwnedRuleDefinition,
    CloudflareWafPhase as AdapterPhase, CloudflareWafPhaseAvailability as AdapterPhaseAvailability,
    CloudflareWafRateLimitCharacteristic, CloudflareWafRateLimitRuleSpec,
    CloudflareWafRulePosition, CloudflareWafRuleRef, CloudflareWafRuleSpec,
    CloudflareWafRulesetRevision, CloudflareWafSecurityWeakeningIntent, CloudflareZone,
    CloudflareZoneWafAdapter, CloudflareZoneWafApi,
};
use edgion_center_adapter_credential_files::{
    ensure_distinct_authorities, CredentialPurpose, MountedCredentialResolver,
    ResolveCredentialRequest, ResolvedCredential,
};
use edgion_center_app::api::cloudflare_waf::{
    CloudflareManagedRuleOverrideDto, CloudflareManagedRuleOverrideView,
    CloudflareRateLimitCharacteristicDto, CloudflareWafAction, CloudflareWafAdminError,
    CloudflareWafAdminService, CloudflareWafInventoryDto, CloudflareWafMutation,
    CloudflareWafMutationResult, CloudflareWafOwnership, CloudflareWafPhase,
    CloudflareWafPhaseAvailability, CloudflareWafRuleDefinitionDto, CloudflareWafRuleDto,
    CloudflareWafRulePositionDto, CloudflareWafRulesetDto, CloudflareWafVersionGuardDto,
    SharedCloudflareWafAdminService,
};
use edgion_center_core::{
    CloudProvider, CloudResourceId, CoreError, CoreResult, CredentialRef, CredentialSource,
    DnsZoneId, DnssecDesiredState, ManagementPolicy, NormalizedProviderError, ProviderAccount,
    ProviderAccountScope, ProviderAccountStore, ProviderErrorCategory,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 120;
const DEFAULT_GLOBAL_CONCURRENCY: usize = 4;
const MAX_GLOBAL_CONCURRENCY: usize = 32;
const DEFAULT_ACCOUNT_CONCURRENCY: usize = 1;
const MAX_ACCOUNT_CONCURRENCY: usize = 4;
const MAX_TRACKED_ACCOUNTS: usize = 1_024;
const SECURITY_WEAKENING_INTENT: &str = "confirmed";

/// Strict Cloudflare Zone WAF composition. Read and write routes are enabled
/// independently and both remain off in every base deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloudflareWafConfig {
    pub read_enabled: bool,
    pub write_enabled: bool,
    /// Active signing key for Cloudflare WAF ownership bindings.
    pub ownership_key_ref: Option<String>,
    /// Verification-only ownership key during an explicit bounded rotation.
    pub ownership_fallback_key_ref: Option<String>,
    pub operation_timeout_secs: u64,
    pub global_concurrency: usize,
    pub per_account_concurrency: usize,
}

impl Default for CloudflareWafConfig {
    fn default() -> Self {
        Self {
            read_enabled: false,
            write_enabled: false,
            ownership_key_ref: None,
            ownership_fallback_key_ref: None,
            operation_timeout_secs: DEFAULT_TIMEOUT_SECS,
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            per_account_concurrency: DEFAULT_ACCOUNT_CONCURRENCY,
        }
    }
}

/// Composes a WAF service only when either independently gated route family is
/// enabled. The caller advertises the same individual route bits from this
/// exact configuration and service result.
pub fn compose_waf_admin(
    config: &CloudflareWafConfig,
    account_store: Option<Arc<dyn ProviderAccountStore>>,
    mounted_resolver: Option<Arc<MountedCredentialResolver>>,
) -> CoreResult<Option<SharedCloudflareWafAdminService>> {
    if !config.read_enabled && !config.write_enabled {
        return Ok(None);
    }
    if !(1..=MAX_TIMEOUT_SECS).contains(&config.operation_timeout_secs)
        || !(1..=MAX_GLOBAL_CONCURRENCY).contains(&config.global_concurrency)
        || !(1..=MAX_ACCOUNT_CONCURRENCY).contains(&config.per_account_concurrency)
        || config.per_account_concurrency > config.global_concurrency
    {
        return Err(CoreError::Conflict(
            "Cloudflare WAF limits are invalid".into(),
        ));
    }
    let ownership_key_ref =
        CredentialRef::new(config.ownership_key_ref.clone().ok_or_else(|| {
            CoreError::Conflict("Cloudflare WAF ownership key is required".into())
        })?)?;
    let ownership_fallback_key_ref = config
        .ownership_fallback_key_ref
        .clone()
        .map(CredentialRef::new)
        .transpose()?;
    if ownership_fallback_key_ref.as_ref() == Some(&ownership_key_ref) {
        return Err(CoreError::Conflict(
            "Cloudflare WAF ownership key references must be distinct".into(),
        ));
    }
    let account_store = account_store.ok_or_else(|| {
        CoreError::Conflict("Cloudflare WAF requires a provider account store".into())
    })?;
    let mounted_resolver = mounted_resolver
        .ok_or_else(|| CoreError::Conflict("Cloudflare WAF requires mounted credentials".into()))?;
    Ok(Some(Arc::new(CloudflareWafService {
        account_store,
        mounted_resolver,
        ownership_key_ref,
        ownership_fallback_key_ref,
        timeout: Duration::from_secs(config.operation_timeout_secs),
        global: Arc::new(Semaphore::new(config.global_concurrency)),
        per_account_concurrency: config.per_account_concurrency,
        accounts: Mutex::new(HashMap::new()),
        api_factory: Arc::new(ProductionApiFactory),
    })))
}

trait ApiFactory: Send + Sync {
    fn build(
        &self,
        token: CloudflareApiToken,
    ) -> Result<Arc<dyn CloudflareZoneWafApi>, NormalizedProviderError>;
}

struct ProductionApiFactory;

impl ApiFactory for ProductionApiFactory {
    fn build(
        &self,
        token: CloudflareApiToken,
    ) -> Result<Arc<dyn CloudflareZoneWafApi>, NormalizedProviderError> {
        CloudflareHttpApi::new(token).map(|api| Arc::new(api) as Arc<dyn CloudflareZoneWafApi>)
    }
}

/// Records the instant immediately before a provider mutation is issued. A
/// timeout or authority change after that instant is necessarily ambiguous.
struct DispatchTrackingApi {
    inner: Arc<dyn CloudflareZoneWafApi>,
    dispatched: Arc<AtomicBool>,
}

#[async_trait]
impl CloudflareApi for DispatchTrackingApi {
    async fn create_zone(
        &self,
        request: &CloudflareCreateZoneRequest,
    ) -> CloudflareApiResult<CloudflareZone> {
        self.dispatched.store(true, Ordering::SeqCst);
        self.inner.create_zone(request).await
    }

    async fn get_zone(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareZone>> {
        self.inner.get_zone(zone_id).await
    }

    async fn delete_zone(&self, zone_id: &str) -> CloudflareApiResult<CloudflareDeleteZoneAck> {
        self.dispatched.store(true, Ordering::SeqCst);
        self.inner.delete_zone(zone_id).await
    }

    async fn get_dnssec(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareDnssec>> {
        self.inner.get_dnssec(zone_id).await
    }

    async fn patch_dnssec(
        &self,
        zone_id: &str,
        desired: DnssecDesiredState,
    ) -> CloudflareApiResult<CloudflareDnssec> {
        self.dispatched.store(true, Ordering::SeqCst);
        self.inner.patch_dnssec(zone_id, desired).await
    }

    async fn list_zones(
        &self,
        account_id: &str,
        page: u32,
        per_page: u32,
    ) -> CloudflareApiResult<CloudflarePage<CloudflareZone>> {
        self.inner.list_zones(account_id, page, per_page).await
    }

    async fn list_records(
        &self,
        zone_id: &str,
        page: u32,
        per_page: u32,
    ) -> CloudflareApiResult<CloudflarePage<CloudflareRecord>> {
        self.inner.list_records(zone_id, page, per_page).await
    }

    async fn batch_records(
        &self,
        zone_id: &str,
        request: &CloudflareBatchRequest,
    ) -> CloudflareApiResult<CloudflareBatchResult> {
        self.dispatched.store(true, Ordering::SeqCst);
        self.inner.batch_records(zone_id, request).await
    }
}

#[async_trait]
impl CloudflareWafApi for DispatchTrackingApi {
    async fn get_zone_waf_entrypoint(
        &self,
        zone_id: &str,
        phase: AdapterPhase,
    ) -> CloudflareApiResult<Option<edgion_center_adapter_cloudflare::CloudflareWafRuleset>> {
        self.inner.get_zone_waf_entrypoint(zone_id, phase).await
    }

    async fn create_zone_waf_entrypoint(
        &self,
        zone_id: &str,
        phase: AdapterPhase,
        rule: &edgion_center_adapter_cloudflare::CloudflareWafRulePayload,
    ) -> CloudflareApiResult<edgion_center_adapter_cloudflare::CloudflareWafRuleset> {
        self.dispatched.store(true, Ordering::SeqCst);
        self.inner
            .create_zone_waf_entrypoint(zone_id, phase, rule)
            .await
    }

    async fn create_zone_waf_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule: &edgion_center_adapter_cloudflare::CloudflareWafRulePayload,
    ) -> CloudflareApiResult<edgion_center_adapter_cloudflare::CloudflareWafRuleset> {
        self.dispatched.store(true, Ordering::SeqCst);
        self.inner
            .create_zone_waf_rule(zone_id, ruleset_id, rule)
            .await
    }

    async fn update_zone_waf_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
        rule: &edgion_center_adapter_cloudflare::CloudflareWafRulePayload,
    ) -> CloudflareApiResult<edgion_center_adapter_cloudflare::CloudflareWafRuleset> {
        self.dispatched.store(true, Ordering::SeqCst);
        self.inner
            .update_zone_waf_rule(zone_id, ruleset_id, rule_id, rule)
            .await
    }

    async fn reorder_zone_waf_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
        position: &CloudflareWafRulePosition,
    ) -> CloudflareApiResult<edgion_center_adapter_cloudflare::CloudflareWafRuleset> {
        self.dispatched.store(true, Ordering::SeqCst);
        self.inner
            .reorder_zone_waf_rule(zone_id, ruleset_id, rule_id, position)
            .await
    }

    async fn delete_zone_waf_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
    ) -> CloudflareApiResult<edgion_center_adapter_cloudflare::CloudflareWafRuleset> {
        self.dispatched.store(true, Ordering::SeqCst);
        self.inner
            .delete_zone_waf_rule(zone_id, ruleset_id, rule_id)
            .await
    }
}

struct CloudflareWafService {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    ownership_key_ref: CredentialRef,
    ownership_fallback_key_ref: Option<CredentialRef>,
    timeout: Duration,
    global: Arc<Semaphore>,
    per_account_concurrency: usize,
    accounts: Mutex<HashMap<CloudResourceId, Weak<Semaphore>>>,
    api_factory: Arc<dyn ApiFactory>,
}

struct RequestAuthority {
    account: ProviderAccount,
    token_revision: String,
    ownership_revision: String,
    ownership_fallback_revision: Option<String>,
}

struct ResolvedWafAuthorities {
    token: CloudflareApiToken,
    token_revision: String,
    ownership: edgion_center_adapter_cloudflare::CloudflareWafOwnershipKeyRing,
    ownership_revision: String,
    ownership_fallback_revision: Option<String>,
}

impl CloudflareWafService {
    async fn prepare(
        &self,
        account_id: &CloudResourceId,
        dispatched: Arc<AtomicBool>,
    ) -> Result<(CloudflareZoneWafAdapter, RequestAuthority), CloudflareWafAdminError> {
        let account = self
            .account_store
            .get(account_id)
            .await
            .map_err(|_| CloudflareWafAdminError::Unavailable)?
            .ok_or(CloudflareWafAdminError::NotFound)?;
        validate_account(account_id, &account)?;
        let authorities = self.resolve_authorities(&account).await?;
        let api = self
            .api_factory
            .build(authorities.token)
            .map_err(|_| CloudflareWafAdminError::Unavailable)?;
        let api: Arc<dyn CloudflareZoneWafApi> = Arc::new(DispatchTrackingApi {
            inner: api,
            dispatched,
        });
        let adapter = CloudflareZoneWafAdapter::new(
            account.metadata.id.clone(),
            &account.spec,
            authorities.ownership,
            api,
        )
        .map_err(map_provider_error)?;
        Ok((
            adapter,
            RequestAuthority {
                account,
                token_revision: authorities.token_revision,
                ownership_revision: authorities.ownership_revision,
                ownership_fallback_revision: authorities.ownership_fallback_revision,
            },
        ))
    }

    async fn resolve_authorities(
        &self,
        account: &ProviderAccount,
    ) -> Result<ResolvedWafAuthorities, CloudflareWafAdminError> {
        let CredentialSource::StaticSecret { credential_ref } = &account.spec.credential_source
        else {
            return Err(CloudflareWafAdminError::InvalidRequest);
        };
        let token_authority = self
            .resolve_authority(
                account,
                CredentialPurpose::CloudflareApiToken,
                credential_ref,
            )
            .await?;
        let ownership_authority = self
            .resolve_authority(
                account,
                CredentialPurpose::CloudflareWafOwnershipHmac,
                &self.ownership_key_ref,
            )
            .await?;
        let fallback_authority = match &self.ownership_fallback_key_ref {
            Some(reference) => Some(
                self.resolve_authority(
                    account,
                    CredentialPurpose::CloudflareWafOwnershipHmac,
                    reference,
                )
                .await?,
            ),
            None => None,
        };
        let mut distinct = vec![&token_authority, &ownership_authority];
        if let Some(fallback) = &fallback_authority {
            distinct.push(fallback);
        }
        ensure_distinct_authorities(&distinct).map_err(|_| CloudflareWafAdminError::Unavailable)?;
        let token = token_authority
            .with_bytes(|bytes| str::from_utf8(bytes).map(str::to_owned))
            .map_err(|_| CloudflareWafAdminError::Unavailable)?;
        let token =
            CloudflareApiToken::new(token).map_err(|_| CloudflareWafAdminError::Unavailable)?;
        let active_ownership_key = ownership_key(&ownership_authority)?;
        let fallback_key = fallback_authority.as_ref().map(ownership_key).transpose()?;
        let ownership = edgion_center_adapter_cloudflare::CloudflareWafOwnershipKeyRing::new(
            *active_ownership_key,
            fallback_key.as_deref().copied(),
        )
        .map_err(|_| CloudflareWafAdminError::Unavailable)?;
        Ok(ResolvedWafAuthorities {
            token,
            token_revision: token_authority.revision().as_str().to_owned(),
            ownership,
            ownership_revision: ownership_authority.revision().as_str().to_owned(),
            ownership_fallback_revision: fallback_authority
                .as_ref()
                .map(|authority| authority.revision().as_str().to_owned()),
        })
    }

    async fn authority_is_current(&self, authority: &RequestAuthority) -> bool {
        let Ok(Some(current)) = self.account_store.get(&authority.account.metadata.id).await else {
            return false;
        };
        if current != authority.account {
            return false;
        }
        let Ok(current_authorities) = self.resolve_authorities(&current).await else {
            return false;
        };
        if current_authorities.token_revision != authority.token_revision
            || current_authorities.ownership_revision != authority.ownership_revision
            || current_authorities.ownership_fallback_revision
                != authority.ownership_fallback_revision
        {
            return false;
        }
        matches!(
            self.account_store.get(&authority.account.metadata.id).await,
            Ok(Some(final_account)) if final_account == authority.account
        )
    }

    async fn resolve_authority(
        &self,
        account: &ProviderAccount,
        purpose: CredentialPurpose,
        credential_ref: &CredentialRef,
    ) -> Result<ResolvedCredential, CloudflareWafAdminError> {
        self.mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: &account.metadata.id,
                provider: &CloudProvider::Cloudflare,
                purpose,
                credential_ref,
            })
            .await
            .map_err(|_| CloudflareWafAdminError::Unavailable)
    }

    fn account_semaphore(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<Arc<Semaphore>, CloudflareWafAdminError> {
        let mut accounts = self
            .accounts
            .lock()
            .map_err(|_| CloudflareWafAdminError::Unavailable)?;
        if let Some(existing) = accounts.get(account_id).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        accounts.retain(|_, value| value.strong_count() > 0);
        if accounts.len() >= MAX_TRACKED_ACCOUNTS {
            return Err(CloudflareWafAdminError::Unavailable);
        }
        let semaphore = Arc::new(Semaphore::new(self.per_account_concurrency));
        accounts.insert(account_id.clone(), Arc::downgrade(&semaphore));
        Ok(semaphore)
    }

    async fn inventory(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> Result<CloudflareWafInventoryDto, CloudflareWafAdminError> {
        let dispatched = Arc::new(AtomicBool::new(false));
        let result = tokio::time::timeout(self.timeout, async {
            let account_limit = self.account_semaphore(account_id)?;
            let _account_permit = acquire(account_limit).await?;
            let _global_permit = acquire(self.global.clone()).await?;
            let (adapter, authority) = self.prepare(account_id, dispatched).await?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareWafAdminError::Unavailable);
            }
            let result = adapter
                .inventory(zone_id.as_str())
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareWafAdminError::Unavailable);
            }
            map_inventory(account_id, zone_id, result)
        })
        .await;
        result.unwrap_or(Err(CloudflareWafAdminError::Unavailable))
    }

    async fn execute_mutation(
        &self,
        mutation: CloudflareWafMutation,
    ) -> Result<CloudflareWafMutationResult, CloudflareWafAdminError> {
        let (account_id, zone_id) = mutation_scope(&mutation);
        let account_id = account_id.clone();
        let zone_id = zone_id.clone();
        let dispatched = Arc::new(AtomicBool::new(false));
        let marker = dispatched.clone();
        let result = tokio::time::timeout(self.timeout, async {
            let account_limit = self.account_semaphore(&account_id)?;
            let _account_permit = acquire(account_limit).await?;
            let _global_permit = acquire(self.global.clone()).await?;
            let (adapter, authority) = self.prepare(&account_id, marker).await?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareWafAdminError::Unavailable);
            }
            let result = apply_mutation(&adapter, zone_id.as_str(), mutation).await;
            if dispatched.load(Ordering::SeqCst) {
                if !self.authority_is_current(&authority).await {
                    return Err(CloudflareWafAdminError::UnknownOutcome);
                }
                return result
                    .map(|receipt| map_receipt(account_id.clone(), zone_id.clone(), receipt))
                    .map_err(map_provider_error);
            }
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareWafAdminError::Unavailable);
            }
            match result {
                Err(error) => Err(map_provider_error(error)),
                Ok(_) => Err(CloudflareWafAdminError::UnknownOutcome),
            }
        })
        .await;
        classify_timeout(result, &dispatched)
    }
}

fn ownership_key(
    authority: &ResolvedCredential,
) -> Result<Zeroizing<[u8; 32]>, CloudflareWafAdminError> {
    authority
        .with_bytes(|bytes| {
            if bytes.len() != 32 || bytes.iter().all(|byte| *byte == 0) {
                return None;
            }
            let mut key = [0_u8; 32];
            key.copy_from_slice(bytes);
            Some(Zeroizing::new(key))
        })
        .ok_or(CloudflareWafAdminError::Unavailable)
}

#[async_trait]
impl CloudflareWafAdminService for CloudflareWafService {
    async fn read_inventory(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> Result<CloudflareWafInventoryDto, CloudflareWafAdminError> {
        self.inventory(account_id, zone_id).await
    }

    async fn mutate(
        &self,
        mutation: CloudflareWafMutation,
    ) -> Result<CloudflareWafMutationResult, CloudflareWafAdminError> {
        self.execute_mutation(mutation).await
    }
}

fn mutation_scope(mutation: &CloudflareWafMutation) -> (&CloudResourceId, &DnsZoneId) {
    match mutation {
        CloudflareWafMutation::CreateManagedRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::UpdateManagedRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::WeakenManagedRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::DeleteManagedRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::OrderManagedRules {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::SetManagedRuleException {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::CreateCustomRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::UpdateCustomRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::DeleteCustomRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::OrderCustomRules {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::WeakenCustomRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::CreateRateLimitRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::UpdateRateLimitRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::DeleteRateLimitRule {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::OrderRateLimitRules {
            account_id,
            zone_id,
            ..
        }
        | CloudflareWafMutation::WeakenRateLimitRule {
            account_id,
            zone_id,
            ..
        } => (account_id, zone_id),
    }
}

async fn apply_mutation(
    adapter: &CloudflareZoneWafAdapter,
    zone_id: &str,
    mutation: CloudflareWafMutation,
) -> edgion_center_adapter_cloudflare::CloudflareApiResult<
    edgion_center_adapter_cloudflare::CloudflareWafMutationReceipt,
> {
    match mutation {
        CloudflareWafMutation::CreateManagedRule { request, .. } => {
            let expected = optional_revision(&request.guard)?;
            adapter
                .create_rule(
                    zone_id,
                    expected.as_ref(),
                    &CloudflareWafRuleSpec::Managed(managed_spec(
                        &request.reference,
                        &request.description,
                        &request.expression,
                        &request.managed_ruleset_id,
                        &request.overrides,
                        true,
                        request.position.as_ref(),
                        None,
                    )?),
                )
                .await
        }
        CloudflareWafMutation::UpdateManagedRule {
            rule_id, request, ..
        } => {
            adapter
                .update_rule(
                    zone_id,
                    &revision(&request.guard)?,
                    &rule_id,
                    &CloudflareWafRuleSpec::Managed(managed_spec(
                        &request.reference,
                        &request.description,
                        &request.expression,
                        &request.managed_ruleset_id,
                        &request.overrides,
                        true,
                        None,
                        None,
                    )?),
                )
                .await
        }
        CloudflareWafMutation::WeakenManagedRule {
            rule_id, request, ..
        } => {
            adapter
                .update_rule(
                    zone_id,
                    &revision(&request.guard)?,
                    &rule_id,
                    &CloudflareWafRuleSpec::Managed(managed_spec(
                        &request.reference,
                        &request.description,
                        &request.expression,
                        &request.managed_ruleset_id,
                        &request.overrides,
                        request.enabled,
                        None,
                        Some(weakening_intent()?),
                    )?),
                )
                .await
        }
        CloudflareWafMutation::DeleteManagedRule {
            rule_id, request, ..
        } => {
            adapter
                .delete_rule(
                    zone_id,
                    AdapterPhase::Managed,
                    &revision(&request.guard)?,
                    &rule_id,
                    weakening_intent()?,
                )
                .await
        }
        CloudflareWafMutation::OrderManagedRules {
            rule_id, request, ..
        } => {
            adapter
                .reorder_rule(
                    zone_id,
                    AdapterPhase::Managed,
                    &revision(&request.guard)?,
                    &rule_id,
                    &position(request.position)?,
                )
                .await
        }
        CloudflareWafMutation::SetManagedRuleException { request, .. } => {
            adapter
                .create_managed_exception(
                    zone_id,
                    &revision(&request.guard)?,
                    &CloudflareWafManagedExceptionSpec {
                        reference: rule_ref(&request.reference)?,
                        description: request.description,
                        expression: expression(request.expression)?,
                        managed_ruleset_ids: request.managed_ruleset_ids.into_iter().collect(),
                        position: position(request.position)?,
                        weakening_intent: weakening_intent()?,
                    },
                )
                .await
        }
        CloudflareWafMutation::CreateCustomRule { request, .. } => {
            let expected = optional_revision(&request.guard)?;
            adapter
                .create_rule(
                    zone_id,
                    expected.as_ref(),
                    &CloudflareWafRuleSpec::Custom(custom_spec(
                        &request.reference,
                        &request.description,
                        &request.expression,
                        request.action,
                        true,
                        request.position.as_ref(),
                        None,
                    )?),
                )
                .await
        }
        CloudflareWafMutation::UpdateCustomRule {
            rule_id, request, ..
        } => {
            adapter
                .update_rule(
                    zone_id,
                    &revision(&request.guard)?,
                    &rule_id,
                    &CloudflareWafRuleSpec::Custom(custom_spec(
                        &request.reference,
                        &request.description,
                        &request.expression,
                        request.action,
                        true,
                        None,
                        None,
                    )?),
                )
                .await
        }
        CloudflareWafMutation::DeleteCustomRule {
            rule_id, request, ..
        } => {
            adapter
                .delete_rule(
                    zone_id,
                    AdapterPhase::Custom,
                    &revision(&request.guard)?,
                    &rule_id,
                    weakening_intent()?,
                )
                .await
        }
        CloudflareWafMutation::OrderCustomRules {
            rule_id, request, ..
        } => {
            adapter
                .reorder_rule(
                    zone_id,
                    AdapterPhase::Custom,
                    &revision(&request.guard)?,
                    &rule_id,
                    &position(request.position)?,
                )
                .await
        }
        CloudflareWafMutation::WeakenCustomRule {
            rule_id, request, ..
        } => {
            adapter
                .update_rule(
                    zone_id,
                    &revision(&request.guard)?,
                    &rule_id,
                    &CloudflareWafRuleSpec::Custom(custom_spec(
                        &request.reference,
                        &request.description,
                        &request.expression,
                        request.action,
                        request.enabled,
                        None,
                        Some(weakening_intent()?),
                    )?),
                )
                .await
        }
        CloudflareWafMutation::CreateRateLimitRule { request, .. } => {
            let expected = optional_revision(&request.guard)?;
            adapter
                .create_rule(
                    zone_id,
                    expected.as_ref(),
                    &CloudflareWafRuleSpec::RateLimit(rate_limit_spec(
                        &request.reference,
                        &request.description,
                        &request.expression,
                        request.action,
                        request.characteristics,
                        request.period_secs,
                        request.requests_per_period,
                        request.mitigation_timeout_secs,
                        true,
                        request.position.as_ref(),
                        None,
                    )?),
                )
                .await
        }
        CloudflareWafMutation::UpdateRateLimitRule {
            rule_id, request, ..
        } => {
            adapter
                .update_rule(
                    zone_id,
                    &revision(&request.guard)?,
                    &rule_id,
                    &CloudflareWafRuleSpec::RateLimit(rate_limit_spec(
                        &request.reference,
                        &request.description,
                        &request.expression,
                        request.action,
                        request.characteristics,
                        request.period_secs,
                        request.requests_per_period,
                        request.mitigation_timeout_secs,
                        true,
                        None,
                        None,
                    )?),
                )
                .await
        }
        CloudflareWafMutation::DeleteRateLimitRule {
            rule_id, request, ..
        } => {
            adapter
                .delete_rule(
                    zone_id,
                    AdapterPhase::RateLimit,
                    &revision(&request.guard)?,
                    &rule_id,
                    weakening_intent()?,
                )
                .await
        }
        CloudflareWafMutation::OrderRateLimitRules {
            rule_id, request, ..
        } => {
            adapter
                .reorder_rule(
                    zone_id,
                    AdapterPhase::RateLimit,
                    &revision(&request.guard)?,
                    &rule_id,
                    &position(request.position)?,
                )
                .await
        }
        CloudflareWafMutation::WeakenRateLimitRule {
            rule_id, request, ..
        } => {
            adapter
                .update_rule(
                    zone_id,
                    &revision(&request.guard)?,
                    &rule_id,
                    &CloudflareWafRuleSpec::RateLimit(rate_limit_spec(
                        &request.reference,
                        &request.description,
                        &request.expression,
                        request.action,
                        request.characteristics,
                        request.period_secs,
                        request.requests_per_period,
                        request.mitigation_timeout_secs,
                        request.enabled,
                        None,
                        Some(weakening_intent()?),
                    )?),
                )
                .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn managed_spec(
    reference: &str,
    description: &str,
    expression_value: &str,
    managed_ruleset_id: &str,
    overrides: &[CloudflareManagedRuleOverrideDto],
    enabled: bool,
    rule_position: Option<&CloudflareWafRulePositionDto>,
    weakening: Option<CloudflareWafSecurityWeakeningIntent>,
) -> Result<CloudflareWafManagedRuleSpec, NormalizedProviderError> {
    Ok(CloudflareWafManagedRuleSpec {
        reference: rule_ref(reference)?,
        description: description.to_owned(),
        expression: expression(expression_value.to_owned())?,
        managed_ruleset_id: managed_ruleset_id.to_owned(),
        overrides: overrides
            .iter()
            .map(|item| CloudflareWafManagedRuleOverride {
                managed_rule_id: item.managed_rule_id.clone(),
                action: item.action.map(action),
                enabled: item.enabled,
            })
            .collect(),
        enabled,
        position: rule_position.cloned().map(position).transpose()?,
        weakening_intent: weakening,
    })
}

fn custom_spec(
    reference: &str,
    description: &str,
    expression_value: &str,
    dto_action: CloudflareWafAction,
    enabled: bool,
    rule_position: Option<&CloudflareWafRulePositionDto>,
    weakening: Option<CloudflareWafSecurityWeakeningIntent>,
) -> Result<CloudflareWafCustomRuleSpec, NormalizedProviderError> {
    Ok(CloudflareWafCustomRuleSpec {
        reference: rule_ref(reference)?,
        description: description.to_owned(),
        expression: expression(expression_value.to_owned())?,
        action: action(dto_action),
        enabled,
        position: rule_position.cloned().map(position).transpose()?,
        weakening_intent: weakening,
    })
}

#[allow(clippy::too_many_arguments)]
fn rate_limit_spec(
    reference: &str,
    description: &str,
    expression_value: &str,
    dto_action: CloudflareWafAction,
    characteristics: BTreeSet<CloudflareRateLimitCharacteristicDto>,
    period_secs: u32,
    requests_per_period: u32,
    mitigation_timeout_secs: u32,
    enabled: bool,
    rule_position: Option<&CloudflareWafRulePositionDto>,
    weakening: Option<CloudflareWafSecurityWeakeningIntent>,
) -> Result<CloudflareWafRateLimitRuleSpec, NormalizedProviderError> {
    Ok(CloudflareWafRateLimitRuleSpec {
        reference: rule_ref(reference)?,
        description: description.to_owned(),
        expression: expression(expression_value.to_owned())?,
        action: action(dto_action),
        characteristics: characteristics.into_iter().map(characteristic).collect(),
        period_secs,
        requests_per_period,
        mitigation_timeout_secs,
        enabled,
        position: rule_position.cloned().map(position).transpose()?,
        weakening_intent: weakening,
    })
}

fn optional_revision(
    guard: &edgion_center_app::api::cloudflare_waf::CloudflareWafRulesetVersionGuardDto,
) -> Result<Option<CloudflareWafRulesetRevision>, NormalizedProviderError> {
    match (&guard.ruleset_id, &guard.ruleset_version) {
        (Some(id), Some(version)) => Ok(Some(CloudflareWafRulesetRevision {
            id: id.clone(),
            version: version.clone(),
        })),
        (None, None) => Ok(None),
        _ => Err(validation_error("cloudflare_waf_revision_guard_invalid")),
    }
}

fn revision(
    guard: &CloudflareWafVersionGuardDto,
) -> Result<CloudflareWafRulesetRevision, NormalizedProviderError> {
    Ok(CloudflareWafRulesetRevision {
        id: guard.ruleset_id.clone(),
        version: guard.ruleset_version.clone(),
    })
}

fn rule_ref(value: impl Into<String>) -> Result<CloudflareWafRuleRef, NormalizedProviderError> {
    CloudflareWafRuleRef::new(value)
}

fn expression(
    value: impl Into<String>,
) -> Result<CloudflareWafExpression, NormalizedProviderError> {
    CloudflareWafExpression::new(value)
}

fn weakening_intent() -> Result<CloudflareWafSecurityWeakeningIntent, NormalizedProviderError> {
    CloudflareWafSecurityWeakeningIntent::new(SECURITY_WEAKENING_INTENT)
}

fn position(
    value: CloudflareWafRulePositionDto,
) -> Result<CloudflareWafRulePosition, NormalizedProviderError> {
    Ok(match value {
        CloudflareWafRulePositionDto::First => CloudflareWafRulePosition::First,
        CloudflareWafRulePositionDto::Before { rule_id } => {
            CloudflareWafRulePosition::Before { rule_id }
        }
        CloudflareWafRulePositionDto::After { rule_id } => {
            CloudflareWafRulePosition::After { rule_id }
        }
        CloudflareWafRulePositionDto::Index { index } => CloudflareWafRulePosition::Index { index },
    })
}

fn action(value: CloudflareWafAction) -> AdapterAction {
    match value {
        CloudflareWafAction::Block => AdapterAction::Block,
        CloudflareWafAction::Challenge => AdapterAction::Challenge,
        CloudflareWafAction::ManagedChallenge => AdapterAction::ManagedChallenge,
        CloudflareWafAction::Log => AdapterAction::Log,
    }
}

fn characteristic(
    value: CloudflareRateLimitCharacteristicDto,
) -> CloudflareWafRateLimitCharacteristic {
    match value {
        CloudflareRateLimitCharacteristicDto::IpSource => {
            CloudflareWafRateLimitCharacteristic::IpSource
        }
        CloudflareRateLimitCharacteristicDto::Colo => CloudflareWafRateLimitCharacteristic::Colo,
    }
}

fn map_inventory(
    account_id: &CloudResourceId,
    zone_id: &DnsZoneId,
    inventory: edgion_center_adapter_cloudflare::CloudflareZoneWafInventory,
) -> Result<CloudflareWafInventoryDto, CloudflareWafAdminError> {
    if inventory.zone_id != zone_id.as_str() || inventory.phases.len() != 3 {
        return Err(CloudflareWafAdminError::InvalidProviderObservation);
    }
    let rulesets = inventory
        .phases
        .into_iter()
        .map(|phase| {
            let (ruleset_id, version) = phase
                .revision
                .map(|revision| (Some(revision.id), Some(revision.version)))
                .unwrap_or((None, None));
            CloudflareWafRulesetDto {
                provider_account_id: account_id.clone(),
                zone_id: zone_id.clone(),
                phase: map_phase(phase.phase),
                availability: map_availability(phase.availability),
                ruleset_id,
                version,
                rules: phase.rules.into_iter().map(map_rule).collect(),
            }
        })
        .collect();
    Ok(CloudflareWafInventoryDto { rulesets })
}

fn map_rule(
    value: edgion_center_adapter_cloudflare::CloudflareWafRuleInventoryItem,
) -> CloudflareWafRuleDto {
    let definition = value.definition.and_then(|definition| {
        map_definition(
            definition,
            value.center_reference.as_deref(),
            value.position,
        )
    });
    CloudflareWafRuleDto {
        rule_id: value.id,
        version: value.version,
        action: value.action,
        enabled: value.enabled,
        ownership: if value.center_owned {
            CloudflareWafOwnership::CenterOwned
        } else {
            CloudflareWafOwnership::ObserveOnly
        },
        position: value.position,
        definition,
    }
}

fn map_definition(
    value: CloudflareWafOwnedRuleDefinition,
    provider_reference: Option<&str>,
    position_index: usize,
) -> Option<CloudflareWafRuleDefinitionDto> {
    let reference = provider_reference?.to_owned();
    match value {
        CloudflareWafOwnedRuleDefinition::Managed {
            description,
            expression,
            managed_ruleset_id,
            overrides,
            ..
        } => Some(CloudflareWafRuleDefinitionDto::Managed {
            reference,
            description,
            expression: expression_to_string(expression)?,
            managed_ruleset_id,
            overrides: overrides
                .into_iter()
                .map(|item| CloudflareManagedRuleOverrideView {
                    managed_rule_id: item.managed_rule_id,
                    action: item.action.map(map_action),
                    enabled: item.enabled,
                })
                .collect(),
        }),
        CloudflareWafOwnedRuleDefinition::Custom {
            description,
            expression,
            action,
            ..
        } => Some(CloudflareWafRuleDefinitionDto::Custom {
            reference,
            description,
            expression: expression_to_string(expression)?,
            action: map_action(action),
        }),
        CloudflareWafOwnedRuleDefinition::RateLimit {
            description,
            expression,
            action,
            characteristics,
            period_secs,
            requests_per_period,
            mitigation_timeout_secs,
            ..
        } => Some(CloudflareWafRuleDefinitionDto::RateLimit {
            reference,
            description,
            expression: expression_to_string(expression)?,
            action: map_action(action),
            characteristics: characteristics
                .into_iter()
                .map(map_characteristic)
                .collect(),
            period_secs,
            requests_per_period,
            mitigation_timeout_secs,
        }),
        CloudflareWafOwnedRuleDefinition::ManagedException {
            description,
            expression,
            managed_ruleset_ids,
            ..
        } => Some(CloudflareWafRuleDefinitionDto::ManagedException {
            reference,
            description,
            expression: expression_to_string(expression)?,
            managed_ruleset_ids: managed_ruleset_ids.into_iter().collect(),
            position: CloudflareWafRulePositionDto::Index {
                index: u16::try_from(position_index.checked_add(1)?).ok()?,
            },
        }),
    }
}

fn expression_to_string(value: CloudflareWafExpression) -> Option<String> {
    serde_json::to_value(value)
        .ok()?
        .as_str()
        .map(ToOwned::to_owned)
}

fn map_phase(value: AdapterPhase) -> CloudflareWafPhase {
    match value {
        AdapterPhase::Managed => CloudflareWafPhase::Managed,
        AdapterPhase::Custom => CloudflareWafPhase::Custom,
        AdapterPhase::RateLimit => CloudflareWafPhase::RateLimit,
    }
}

fn map_availability(value: AdapterPhaseAvailability) -> CloudflareWafPhaseAvailability {
    match value {
        AdapterPhaseAvailability::Available => CloudflareWafPhaseAvailability::Available,
        AdapterPhaseAvailability::EntryPointAbsent => {
            CloudflareWafPhaseAvailability::EntryPointAbsent
        }
        AdapterPhaseAvailability::PermissionDenied => {
            CloudflareWafPhaseAvailability::PermissionDenied
        }
        AdapterPhaseAvailability::QuotaLimited => CloudflareWafPhaseAvailability::QuotaLimited,
        AdapterPhaseAvailability::Unavailable => CloudflareWafPhaseAvailability::Unavailable,
    }
}

fn map_action(value: AdapterAction) -> CloudflareWafAction {
    match value {
        AdapterAction::Block => CloudflareWafAction::Block,
        AdapterAction::Challenge => CloudflareWafAction::Challenge,
        AdapterAction::ManagedChallenge => CloudflareWafAction::ManagedChallenge,
        AdapterAction::Log => CloudflareWafAction::Log,
    }
}

fn map_characteristic(
    value: CloudflareWafRateLimitCharacteristic,
) -> CloudflareRateLimitCharacteristicDto {
    match value {
        CloudflareWafRateLimitCharacteristic::IpSource => {
            CloudflareRateLimitCharacteristicDto::IpSource
        }
        CloudflareWafRateLimitCharacteristic::Colo => CloudflareRateLimitCharacteristicDto::Colo,
    }
}

fn map_receipt(
    provider_account_id: CloudResourceId,
    zone_id: DnsZoneId,
    value: edgion_center_adapter_cloudflare::CloudflareWafMutationReceipt,
) -> CloudflareWafMutationResult {
    CloudflareWafMutationResult {
        provider_account_id,
        zone_id,
        phase: map_phase(value.phase),
        ruleset_id: value.revision.id,
        ruleset_version: value.revision.version,
        rule_id: value.rule_id,
        security_weakening_confirmed: value.security_weakening_confirmed,
    }
}

async fn acquire(
    semaphore: Arc<Semaphore>,
) -> Result<OwnedSemaphorePermit, CloudflareWafAdminError> {
    semaphore
        .acquire_owned()
        .await
        .map_err(|_| CloudflareWafAdminError::Unavailable)
}

fn classify_timeout<T>(
    result: Result<Result<T, CloudflareWafAdminError>, tokio::time::error::Elapsed>,
    dispatched: &AtomicBool,
) -> Result<T, CloudflareWafAdminError> {
    match result {
        Ok(result) => result,
        Err(_) if dispatched.load(Ordering::SeqCst) => Err(CloudflareWafAdminError::UnknownOutcome),
        Err(_) => Err(CloudflareWafAdminError::Unavailable),
    }
}

fn validate_account(
    requested_account_id: &CloudResourceId,
    account: &ProviderAccount,
) -> Result<(), CloudflareWafAdminError> {
    if &account.metadata.id != requested_account_id
        || account.metadata.management_policy != ManagementPolicy::Managed
        || account.spec.provider != CloudProvider::Cloudflare
        || !matches!(
            account.spec.scope.as_ref(),
            Some(ProviderAccountScope::Cloudflare { .. })
        )
        || !matches!(
            account.spec.credential_source,
            CredentialSource::StaticSecret { .. }
        )
    {
        return Err(CloudflareWafAdminError::InvalidRequest);
    }
    Ok(())
}

fn map_provider_error(error: NormalizedProviderError) -> CloudflareWafAdminError {
    edgion_center_app::common::observe::cloud_metrics::record_provider_error(
        "cloudflare",
        error.category(),
    );
    match error.category() {
        ProviderErrorCategory::Validation => CloudflareWafAdminError::InvalidRequest,
        ProviderErrorCategory::NotFound => CloudflareWafAdminError::NotFound,
        ProviderErrorCategory::Conflict => CloudflareWafAdminError::Conflict,
        ProviderErrorCategory::Authorization | ProviderErrorCategory::Authentication => {
            CloudflareWafAdminError::EntitlementDenied
        }
        ProviderErrorCategory::UnknownOutcome => CloudflareWafAdminError::UnknownOutcome,
        _ => CloudflareWafAdminError::Unavailable,
    }
}

fn validation_error(code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::Validation,
        code,
        "Cloudflare WAF request is invalid",
        None,
        None,
    )
    .expect("static normalized provider error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_default_off_and_rejects_unknown_fields() {
        let config = CloudflareWafConfig::default();
        assert!(!config.read_enabled);
        assert!(!config.write_enabled);
        assert!(serde_yaml::from_str::<CloudflareWafConfig>("unknown: true\n").is_err());
        let config: CloudflareWafConfig = serde_yaml::from_str(
            "read_enabled: true\nwrite_enabled: false\noperation_timeout_secs: 30\nglobal_concurrency: 4\nper_account_concurrency: 1\n",
        )
        .expect("bounded WAF config");
        assert!(config.read_enabled);
        assert!(!config.write_enabled);
    }

    #[test]
    fn disabled_composition_requires_no_provider_dependencies() {
        assert!(
            compose_waf_admin(&CloudflareWafConfig::default(), None, None)
                .expect("disabled composition")
                .is_none()
        );
    }

    #[test]
    fn enabled_composition_requires_account_store_and_mounted_credentials() {
        let config = CloudflareWafConfig {
            read_enabled: true,
            ..CloudflareWafConfig::default()
        };
        assert!(compose_waf_admin(&config, None, None).is_err());
    }
}
