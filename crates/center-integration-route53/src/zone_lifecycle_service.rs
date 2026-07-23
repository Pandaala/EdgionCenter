use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_adapter_credential_files::{
    CredentialPurpose, MountedCredentialResolver, ResolveCredentialRequest,
};
use edgion_center_adapter_route53::{
    Route53Api, Route53ChangeBatch, Route53ChangeInfo, Route53CreateHostedZoneRequest,
    Route53CreateHostedZoneResult, Route53CursorKey, Route53DnsAdapter, Route53DnssecInfo,
    Route53HostedZone, Route53HostedZonePage, Route53LifecycleTokenKey, Route53RecordCursor,
    Route53RecordPage,
};
use edgion_center_app::api::route53_dns::{
    Route53DnsAdminError, Route53ZoneDeleteRequest, Route53ZoneLifecycleAdminService,
    Route53ZoneLifecycleMutationDto, Route53ZoneLifecycleObservationDto,
    SharedRoute53ZoneLifecycleAdminService,
};
use edgion_center_core::{
    AbsoluteDnsName, CloudProvider, CloudResourceId, CoreError, CoreResult, CredentialRef,
    CredentialSource, DnsZoneId, DnsZoneRef, ManagementPolicy, NormalizedProviderError,
    ProviderAccount, ProviderAccountScope, ProviderAccountStore, ProviderErrorCategory,
    ZoneCreationRequest, ZoneLifecycleProvider, ZoneVisibility,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::dns_admin_service::{ApiFactory, ProductionApiFactory};

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 180;
const DEFAULT_GLOBAL_CONCURRENCY: usize = 2;
const MAX_GLOBAL_CONCURRENCY: usize = 8;
const DEFAULT_ACCOUNT_CONCURRENCY: usize = 1;
const MAX_ACCOUNT_CONCURRENCY: usize = 1;
const MAX_TRACKED_ACCOUNTS: usize = 1_024;

/// Strict, independently default-off Route 53 hosted-zone lifecycle composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Route53ZoneLifecycleConfig {
    pub enabled: bool,
    pub cursor_key_ref: Option<String>,
    pub lifecycle_token_key_ref: Option<String>,
    pub operation_timeout_secs: u64,
    pub global_concurrency: usize,
    pub per_account_concurrency: usize,
}

impl Default for Route53ZoneLifecycleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cursor_key_ref: None,
            lifecycle_token_key_ref: None,
            operation_timeout_secs: DEFAULT_TIMEOUT_SECS,
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            per_account_concurrency: DEFAULT_ACCOUNT_CONCURRENCY,
        }
    }
}

pub fn compose_zone_lifecycle_admin(
    config: &Route53ZoneLifecycleConfig,
    account_store: Option<Arc<dyn ProviderAccountStore>>,
    mounted_resolver: Option<Arc<MountedCredentialResolver>>,
) -> CoreResult<Option<SharedRoute53ZoneLifecycleAdminService>> {
    let Some((cursor_key_ref, lifecycle_token_key_ref)) = validated_config(config)? else {
        return Ok(None);
    };
    let account_store = account_store.ok_or_else(|| {
        CoreError::Conflict("Route 53 zone lifecycle requires a provider account store".into())
    })?;
    let mounted_resolver = mounted_resolver.ok_or_else(|| {
        CoreError::Conflict("Route 53 zone lifecycle requires mounted credentials".into())
    })?;
    Ok(Some(Arc::new(Route53ZoneLifecycleService {
        account_store,
        mounted_resolver,
        cursor_key_ref,
        lifecycle_token_key_ref,
        timeout: Duration::from_secs(config.operation_timeout_secs),
        global: Arc::new(Semaphore::new(config.global_concurrency)),
        per_account_concurrency: config.per_account_concurrency,
        accounts: Mutex::new(HashMap::new()),
        api_factory: Arc::new(ProductionApiFactory),
    })))
}

struct Route53ZoneLifecycleService {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    cursor_key_ref: CredentialRef,
    lifecycle_token_key_ref: CredentialRef,
    timeout: Duration,
    global: Arc<Semaphore>,
    per_account_concurrency: usize,
    accounts: Mutex<HashMap<CloudResourceId, Weak<Semaphore>>>,
    api_factory: Arc<dyn ApiFactory>,
}

#[derive(Clone)]
struct RequestAuthority {
    account: ProviderAccount,
    cursor_revision: String,
    lifecycle_revision: String,
}

struct AuthorityFence {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    cursor_key_ref: CredentialRef,
    lifecycle_token_key_ref: CredentialRef,
    expected: RequestAuthority,
}

impl AuthorityFence {
    async fn is_current(&self) -> bool {
        let Ok(Some(account)) = self
            .account_store
            .get(&self.expected.account.metadata.id)
            .await
        else {
            return false;
        };
        if account != self.expected.account {
            return false;
        }
        let cursor = resolve_revision(
            &self.mounted_resolver,
            &account,
            &self.cursor_key_ref,
            CredentialPurpose::Route53DnsCursorHmac,
        )
        .await;
        let lifecycle = resolve_revision(
            &self.mounted_resolver,
            &account,
            &self.lifecycle_token_key_ref,
            CredentialPurpose::Route53ZoneLifecycleHmac,
        )
        .await;
        matches!(
            (cursor, lifecycle),
            (Ok(cursor), Ok(lifecycle))
                if cursor == self.expected.cursor_revision
                    && lifecycle == self.expected.lifecycle_revision
        ) && matches!(
            self.account_store.get(&account.metadata.id).await,
            Ok(Some(final_account)) if final_account == account
        )
    }
}

struct DispatchTrackingApi {
    inner: Arc<dyn Route53Api>,
    authority: Arc<AuthorityFence>,
    dispatched: Arc<AtomicBool>,
}

#[async_trait]
impl Route53Api for DispatchTrackingApi {
    fn verified_account_id(&self) -> &str {
        self.inner.verified_account_id()
    }

    async fn create_hosted_zone(
        &self,
        request: &Route53CreateHostedZoneRequest,
    ) -> Result<Route53CreateHostedZoneResult, NormalizedProviderError> {
        self.before_dispatch().await?;
        self.inner.create_hosted_zone(request).await
    }
    async fn get_hosted_zone(
        &self,
        zone_id: &str,
    ) -> Result<Option<Route53HostedZone>, NormalizedProviderError> {
        self.inner.get_hosted_zone(zone_id).await
    }
    async fn list_hosted_zones(
        &self,
        marker: Option<&str>,
        max_items: u16,
    ) -> Result<Route53HostedZonePage, NormalizedProviderError> {
        self.inner.list_hosted_zones(marker, max_items).await
    }
    async fn list_record_sets(
        &self,
        zone_id: &str,
        cursor: Option<&Route53RecordCursor>,
        max_items: u16,
    ) -> Result<Route53RecordPage, NormalizedProviderError> {
        self.inner
            .list_record_sets(zone_id, cursor, max_items)
            .await
    }
    async fn change_record_sets(
        &self,
        zone_id: &str,
        batch: &Route53ChangeBatch,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        self.inner.change_record_sets(zone_id, batch).await
    }
    async fn get_change(
        &self,
        change_id: &str,
    ) -> Result<Option<Route53ChangeInfo>, NormalizedProviderError> {
        self.inner.get_change(change_id).await
    }
    async fn delete_hosted_zone(
        &self,
        zone_id: &str,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        self.before_dispatch().await?;
        self.inner.delete_hosted_zone(zone_id).await
    }
    async fn get_dnssec(
        &self,
        zone_id: &str,
    ) -> Result<Route53DnssecInfo, NormalizedProviderError> {
        self.inner.get_dnssec(zone_id).await
    }
    async fn enable_hosted_zone_dnssec(
        &self,
        zone_id: &str,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        self.inner.enable_hosted_zone_dnssec(zone_id).await
    }
    async fn disable_hosted_zone_dnssec(
        &self,
        zone_id: &str,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        self.inner.disable_hosted_zone_dnssec(zone_id).await
    }
}

impl DispatchTrackingApi {
    async fn before_dispatch(&self) -> Result<(), NormalizedProviderError> {
        if !self.authority.is_current().await {
            return Err(provider_error(
                ProviderErrorCategory::Validation,
                "route53_authority_changed_before_dispatch",
            ));
        }
        self.dispatched.store(true, Ordering::Release);
        Ok(())
    }
}

impl Route53ZoneLifecycleService {
    async fn prepare(
        &self,
        account_id: &CloudResourceId,
        dispatched: Arc<AtomicBool>,
    ) -> Result<(Route53DnsAdapter, Arc<AuthorityFence>), Route53DnsAdminError> {
        let account = self
            .account_store
            .get(account_id)
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?
            .ok_or(Route53DnsAdminError::NotFound)?;
        validate_lifecycle_account(account_id, &account)?;
        let (cursor_key, cursor_revision) = self.resolve_cursor_key(&account).await?;
        let (lifecycle_key, lifecycle_revision) = self.resolve_lifecycle_key(&account).await?;
        let authority = Arc::new(AuthorityFence {
            account_store: self.account_store.clone(),
            mounted_resolver: self.mounted_resolver.clone(),
            cursor_key_ref: self.cursor_key_ref.clone(),
            lifecycle_token_key_ref: self.lifecycle_token_key_ref.clone(),
            expected: RequestAuthority {
                account: account.clone(),
                cursor_revision,
                lifecycle_revision,
            },
        });
        let api = self
            .api_factory
            .build()
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        let api = Arc::new(DispatchTrackingApi {
            inner: api,
            authority: authority.clone(),
            dispatched,
        });
        let adapter = Route53DnsAdapter::new_with_lifecycle_key(
            account.metadata.id.clone(),
            &account.spec,
            api,
            cursor_key,
            lifecycle_key,
        )
        .map_err(|_| Route53DnsAdminError::Unavailable)?;
        Ok((adapter, authority))
    }

    async fn resolve_cursor_key(
        &self,
        account: &ProviderAccount,
    ) -> Result<(Route53CursorKey, String), Route53DnsAdminError> {
        let (bytes, revision) = self
            .resolve_key(
                account,
                &self.cursor_key_ref,
                CredentialPurpose::Route53DnsCursorHmac,
            )
            .await?;
        Ok((
            Route53CursorKey::new(bytes).map_err(|_| Route53DnsAdminError::Unavailable)?,
            revision,
        ))
    }

    async fn resolve_lifecycle_key(
        &self,
        account: &ProviderAccount,
    ) -> Result<(Route53LifecycleTokenKey, String), Route53DnsAdminError> {
        let (bytes, revision) = self
            .resolve_key(
                account,
                &self.lifecycle_token_key_ref,
                CredentialPurpose::Route53ZoneLifecycleHmac,
            )
            .await?;
        Ok((
            Route53LifecycleTokenKey::new(bytes).map_err(|_| Route53DnsAdminError::Unavailable)?,
            revision,
        ))
    }

    async fn resolve_key(
        &self,
        account: &ProviderAccount,
        credential_ref: &CredentialRef,
        purpose: CredentialPurpose,
    ) -> Result<([u8; 32], String), Route53DnsAdminError> {
        let resolved = self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: &account.metadata.id,
                provider: &CloudProvider::Aws,
                purpose,
                credential_ref,
            })
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        let revision = resolved.revision().as_str().to_owned();
        let bytes = resolved
            .with_bytes(|value| <[u8; 32]>::try_from(value).ok())
            .ok_or(Route53DnsAdminError::Unavailable)?;
        Ok((bytes, revision))
    }

    fn account_semaphore(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<Arc<Semaphore>, Route53DnsAdminError> {
        let mut accounts = self
            .accounts
            .lock()
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        if let Some(existing) = accounts.get(account_id).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        accounts.retain(|_, value| value.strong_count() > 0);
        if accounts.len() >= MAX_TRACKED_ACCOUNTS {
            return Err(Route53DnsAdminError::Unavailable);
        }
        let semaphore = Arc::new(Semaphore::new(self.per_account_concurrency));
        accounts.insert(account_id.clone(), Arc::downgrade(&semaphore));
        Ok(semaphore)
    }

    async fn admission(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit), Route53DnsAdminError> {
        let account = self
            .account_semaphore(account_id)?
            .acquire_owned()
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        let global = self
            .global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        Ok((account, global))
    }

    async fn run<T>(
        &self,
        account_id: &CloudResourceId,
        mutation: bool,
        work: impl AsyncWork<T>,
    ) -> Result<T, Route53DnsAdminError> {
        let dispatched = Arc::new(AtomicBool::new(false));
        let result = tokio::time::timeout(self.timeout, async {
            let (_account, _global) = self.admission(account_id).await?;
            let (adapter, authority) = self.prepare(account_id, dispatched.clone()).await?;
            let result = work
                .run(adapter)
                .await
                .map_err(|error| map_error(error, dispatched.load(Ordering::Acquire)))?;
            if !authority.is_current().await {
                return Err(if mutation {
                    Route53DnsAdminError::UnknownOutcome
                } else {
                    Route53DnsAdminError::Unavailable
                });
            }
            Ok(result)
        })
        .await;
        match result {
            Ok(value) => value,
            Err(_) if mutation && dispatched.load(Ordering::Acquire) => {
                Err(Route53DnsAdminError::UnknownOutcome)
            }
            Err(_) => Err(Route53DnsAdminError::Unavailable),
        }
    }
}

#[async_trait]
trait AsyncWork<T>: Send {
    async fn run(self, adapter: Route53DnsAdapter) -> Result<T, NormalizedProviderError>;
}

struct CreateWork(ZoneCreationRequest);
#[async_trait]
impl AsyncWork<Route53ZoneLifecycleMutationDto> for CreateWork {
    async fn run(
        self,
        adapter: Route53DnsAdapter,
    ) -> Result<Route53ZoneLifecycleMutationDto, NormalizedProviderError> {
        Route53ZoneLifecycleMutationDto::from_core(adapter.create_zone(&self.0).await?).map_err(
            |_| {
                provider_error(
                    ProviderErrorCategory::UnknownOutcome,
                    "route53_invalid_lifecycle_receipt",
                )
            },
        )
    }
}

struct ObserveWork {
    zone_id: DnsZoneId,
    apex: AbsoluteDnsName,
}
#[async_trait]
impl AsyncWork<Route53ZoneLifecycleObservationDto> for ObserveWork {
    async fn run(
        self,
        adapter: Route53DnsAdapter,
    ) -> Result<Route53ZoneLifecycleObservationDto, NormalizedProviderError> {
        let zone = DnsZoneRef {
            provider_account_id: adapter.center_account_id().clone(),
            provider: CloudProvider::Aws,
            zone_id: self.zone_id,
            apex: self.apex,
            visibility: ZoneVisibility::Public,
        };
        let value = adapter.observe_zone(&zone).await?.ok_or_else(|| {
            provider_error(ProviderErrorCategory::NotFound, "route53_zone_not_found")
        })?;
        Route53ZoneLifecycleObservationDto::from_core(value).map_err(|_| {
            provider_error(
                ProviderErrorCategory::UnknownOutcome,
                "route53_invalid_lifecycle_observation",
            )
        })
    }
}

struct DeleteWork {
    zone_id: DnsZoneId,
    request: Route53ZoneDeleteRequest,
}
#[async_trait]
impl AsyncWork<Route53ZoneLifecycleMutationDto> for DeleteWork {
    async fn run(
        self,
        adapter: Route53DnsAdapter,
    ) -> Result<Route53ZoneLifecycleMutationDto, NormalizedProviderError> {
        let zone = DnsZoneRef {
            provider_account_id: adapter.center_account_id().clone(),
            provider: CloudProvider::Aws,
            zone_id: self.zone_id,
            apex: self.request.apex,
            visibility: ZoneVisibility::Public,
        };
        Route53ZoneLifecycleMutationDto::from_core(
            adapter
                .delete_zone_with_exact_guard(&zone, &self.request.revision)
                .await?,
        )
        .map_err(|_| {
            provider_error(
                ProviderErrorCategory::UnknownOutcome,
                "route53_invalid_lifecycle_receipt",
            )
        })
    }
}

#[async_trait]
impl Route53ZoneLifecycleAdminService for Route53ZoneLifecycleService {
    async fn create_zone(
        &self,
        account_id: &CloudResourceId,
        request: &ZoneCreationRequest,
    ) -> Result<Route53ZoneLifecycleMutationDto, Route53DnsAdminError> {
        if request.provider_account_id != *account_id
            || request.provider != CloudProvider::Aws
            || request.visibility != ZoneVisibility::Public
        {
            return Err(Route53DnsAdminError::InvalidRequest);
        }
        self.run(account_id, true, CreateWork(request.clone()))
            .await
    }
    async fn observe_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        apex: &AbsoluteDnsName,
    ) -> Result<Route53ZoneLifecycleObservationDto, Route53DnsAdminError> {
        self.run(
            account_id,
            false,
            ObserveWork {
                zone_id: zone_id.clone(),
                apex: apex.clone(),
            },
        )
        .await
    }
    async fn delete_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        request: &Route53ZoneDeleteRequest,
    ) -> Result<Route53ZoneLifecycleMutationDto, Route53DnsAdminError> {
        self.run(
            account_id,
            true,
            DeleteWork {
                zone_id: zone_id.clone(),
                request: request.clone(),
            },
        )
        .await
    }
}

fn validated_config(
    config: &Route53ZoneLifecycleConfig,
) -> CoreResult<Option<(CredentialRef, CredentialRef)>> {
    if !config.enabled {
        if config != &Route53ZoneLifecycleConfig::default() {
            return Err(CoreError::Conflict(
                "disabled Route 53 zone lifecycle configuration must use defaults".into(),
            ));
        }
        return Ok(None);
    }
    if !(1..=MAX_TIMEOUT_SECS).contains(&config.operation_timeout_secs)
        || !(1..=MAX_GLOBAL_CONCURRENCY).contains(&config.global_concurrency)
        || !(1..=MAX_ACCOUNT_CONCURRENCY).contains(&config.per_account_concurrency)
    {
        return Err(CoreError::Conflict(
            "Route 53 zone lifecycle configuration is out of bounds".into(),
        ));
    }
    let cursor = config.cursor_key_ref.as_deref().ok_or_else(|| {
        CoreError::Conflict("Route 53 zone lifecycle cursor key is required".into())
    })?;
    let lifecycle = config
        .lifecycle_token_key_ref
        .as_deref()
        .ok_or_else(|| CoreError::Conflict("Route 53 lifecycle token key is required".into()))?;
    if cursor == lifecycle {
        return Err(CoreError::Conflict(
            "Route 53 lifecycle keys must be distinct".into(),
        ));
    }
    Ok(Some((
        CredentialRef::new(cursor)?,
        CredentialRef::new(lifecycle)?,
    )))
}

fn validate_lifecycle_account(
    account_id: &CloudResourceId,
    account: &ProviderAccount,
) -> Result<(), Route53DnsAdminError> {
    if account.metadata.id != *account_id
        || account.spec.provider != CloudProvider::Aws
        || account.metadata.management_policy != ManagementPolicy::Managed
        || account.spec.credential_source != CredentialSource::Ambient
        || !matches!(account.spec.scope, Some(ProviderAccountScope::Aws { .. }))
    {
        return Err(Route53DnsAdminError::Conflict);
    }
    Ok(())
}

async fn resolve_revision(
    resolver: &MountedCredentialResolver,
    account: &ProviderAccount,
    credential_ref: &CredentialRef,
    purpose: CredentialPurpose,
) -> Result<String, Route53DnsAdminError> {
    let value = resolver
        .resolve(ResolveCredentialRequest {
            provider_account_id: &account.metadata.id,
            provider: &CloudProvider::Aws,
            purpose,
            credential_ref,
        })
        .await
        .map_err(|_| Route53DnsAdminError::Unavailable)?;
    Ok(value.revision().as_str().to_owned())
}

fn map_error(error: NormalizedProviderError, dispatched: bool) -> Route53DnsAdminError {
    edgion_center_app::common::observe::cloud_metrics::record_provider_error(
        "aws",
        error.category(),
    );
    if dispatched || error.category() == ProviderErrorCategory::UnknownOutcome {
        return Route53DnsAdminError::UnknownOutcome;
    }
    match error.category() {
        ProviderErrorCategory::NotFound => Route53DnsAdminError::NotFound,
        ProviderErrorCategory::Conflict => Route53DnsAdminError::Conflict,
        ProviderErrorCategory::Validation => Route53DnsAdminError::InvalidRequest,
        ProviderErrorCategory::Authentication
        | ProviderErrorCategory::Authorization
        | ProviderErrorCategory::Quota
        | ProviderErrorCategory::Throttled
        | ProviderErrorCategory::Transient
        | ProviderErrorCategory::UnknownOutcome => Route53DnsAdminError::Unavailable,
    }
}

fn provider_error(category: ProviderErrorCategory, code: &'static str) -> NormalizedProviderError {
    NormalizedProviderError::new(category, code, code, None, None).expect("static provider error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_config_is_default_off_and_requires_distinct_authorities() {
        assert!(
            compose_zone_lifecycle_admin(&Route53ZoneLifecycleConfig::default(), None, None)
                .unwrap()
                .is_none()
        );
        let enabled = Route53ZoneLifecycleConfig {
            enabled: true,
            cursor_key_ref: Some("aws/route53-cursor".into()),
            lifecycle_token_key_ref: Some("aws/route53-lifecycle".into()),
            ..Default::default()
        };
        assert!(compose_zone_lifecycle_admin(&enabled, None, None).is_err());
        assert!(validated_config(&Route53ZoneLifecycleConfig {
            lifecycle_token_key_ref: Some("aws/route53-cursor".into()),
            ..enabled
        })
        .is_err());
    }
}
