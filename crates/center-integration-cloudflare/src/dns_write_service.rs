use std::{
    collections::HashMap,
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
    CloudflareBatchResult, CloudflareCreateZoneRequest, CloudflareDeleteZoneAck,
    CloudflareDnsSyncRecordOutcome, CloudflareDnsSyncWriter, CloudflareDnssec, CloudflareHttpApi,
    CloudflarePage, CloudflareRecord, CloudflareZone,
};
use edgion_center_adapter_credential_files::{
    CredentialPurpose, MountedCredentialResolver, ResolveCredentialRequest,
};
use edgion_center_app::api::cloudflare_dns::{
    CloudflareDnsAdminError, CloudflareDnsWriteAdminService, CloudflareRecordControlDto,
    CloudflareRecordMutationGuardDto, CloudflareRecordSetDeleteRequest, CloudflareRecordSetDto,
    CloudflareRecordSetKey, CloudflareRecordSetPutRequest, CloudflareZoneCreateRequest,
    CloudflareZoneDeleteRequest, CloudflareZoneDto, SharedCloudflareDnsWriteAdminService,
};
use edgion_center_core::{
    CloudProvider, CloudResourceId, CoreError, CoreResult, CredentialSource, DnsMutationGuard,
    DnsRecordChange, DnsZoneId, DnssecDesiredState, ManagementPolicy, NormalizedProviderError,
    ObservedDnsRecordSet, ProviderAccount, ProviderAccountScope, ProviderAccountStore,
    ProviderDnsRecordType, ProviderErrorCategory, ZoneVisibility,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::dns_admin::{map_record, map_zone};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 120;
const DEFAULT_GLOBAL_CONCURRENCY: usize = 4;
const MAX_GLOBAL_CONCURRENCY: usize = 32;
const DEFAULT_ACCOUNT_CONCURRENCY: usize = 1;
const MAX_ACCOUNT_CONCURRENCY: usize = 4;
const MAX_TRACKED_ACCOUNTS: usize = 1_024;

/// Strict, independently default-off switch for synchronous Cloudflare DNS writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloudflareDnsWriteConfig {
    pub enabled: bool,
    pub operation_timeout_secs: u64,
    pub global_concurrency: usize,
    pub per_account_concurrency: usize,
}

impl Default for CloudflareDnsWriteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            operation_timeout_secs: DEFAULT_TIMEOUT_SECS,
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            per_account_concurrency: DEFAULT_ACCOUNT_CONCURRENCY,
        }
    }
}

pub fn compose_dns_write_admin(
    config: &CloudflareDnsWriteConfig,
    account_store: Option<Arc<dyn ProviderAccountStore>>,
    mounted_resolver: Option<Arc<MountedCredentialResolver>>,
) -> CoreResult<Option<SharedCloudflareDnsWriteAdminService>> {
    if !config.enabled {
        return Ok(None);
    }
    if !(1..=MAX_TIMEOUT_SECS).contains(&config.operation_timeout_secs)
        || !(1..=MAX_GLOBAL_CONCURRENCY).contains(&config.global_concurrency)
        || !(1..=MAX_ACCOUNT_CONCURRENCY).contains(&config.per_account_concurrency)
        || config.per_account_concurrency > config.global_concurrency
    {
        return Err(CoreError::Conflict(
            "Cloudflare DNS write limits are invalid".into(),
        ));
    }
    let account_store = account_store.ok_or_else(|| {
        CoreError::Conflict("Cloudflare DNS write requires a provider account store".into())
    })?;
    let mounted_resolver = mounted_resolver.ok_or_else(|| {
        CoreError::Conflict("Cloudflare DNS write requires mounted credentials".into())
    })?;
    Ok(Some(Arc::new(CloudflareDnsWriteService {
        account_store,
        mounted_resolver,
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
    ) -> Result<Arc<dyn CloudflareApi>, NormalizedProviderError>;
}

struct ProductionApiFactory;

impl ApiFactory for ProductionApiFactory {
    fn build(
        &self,
        token: CloudflareApiToken,
    ) -> Result<Arc<dyn CloudflareApi>, NormalizedProviderError> {
        CloudflareHttpApi::new(token).map(|api| Arc::new(api) as Arc<dyn CloudflareApi>)
    }
}

struct DispatchTrackingApi {
    inner: Arc<dyn CloudflareApi>,
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

struct CloudflareDnsWriteService {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    timeout: Duration,
    global: Arc<Semaphore>,
    per_account_concurrency: usize,
    accounts: Mutex<HashMap<CloudResourceId, Weak<Semaphore>>>,
    api_factory: Arc<dyn ApiFactory>,
}

struct RequestAuthority {
    account: ProviderAccount,
    token_revision: String,
}

impl CloudflareDnsWriteService {
    async fn prepare(
        &self,
        account_id: &CloudResourceId,
        dispatched: Arc<AtomicBool>,
    ) -> Result<(CloudflareDnsSyncWriter, RequestAuthority), CloudflareDnsAdminError> {
        let account = self
            .account_store
            .get(account_id)
            .await
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?
            .ok_or(CloudflareDnsAdminError::NotFound)?;
        validate_account(account_id, &account)?;
        let (token, token_revision) = self.resolve_token(&account).await?;
        let api = self
            .api_factory
            .build(token)
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let api: Arc<dyn CloudflareApi> = Arc::new(DispatchTrackingApi {
            inner: api,
            dispatched,
        });
        let writer = CloudflareDnsSyncWriter::new(account.metadata.id.clone(), &account.spec, api)
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        Ok((
            writer,
            RequestAuthority {
                account,
                token_revision,
            },
        ))
    }

    async fn resolve_token(
        &self,
        account: &ProviderAccount,
    ) -> Result<(CloudflareApiToken, String), CloudflareDnsAdminError> {
        let CredentialSource::StaticSecret { credential_ref } = &account.spec.credential_source
        else {
            return Err(CloudflareDnsAdminError::InvalidRequest);
        };
        let resolved = self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: &account.metadata.id,
                provider: &CloudProvider::Cloudflare,
                purpose: CredentialPurpose::CloudflareApiToken,
                credential_ref,
            })
            .await
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let revision = resolved.revision().as_str().to_owned();
        let token = resolved
            .with_bytes(|bytes| str::from_utf8(bytes).map(str::to_owned))
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let token =
            CloudflareApiToken::new(token).map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        Ok((token, revision))
    }

    async fn authority_is_current(&self, authority: &RequestAuthority) -> bool {
        let Ok(Some(current)) = self.account_store.get(&authority.account.metadata.id).await else {
            return false;
        };
        if current != authority.account {
            return false;
        }
        let Ok((_, revision)) = self.resolve_token(&current).await else {
            return false;
        };
        if revision != authority.token_revision {
            return false;
        }
        matches!(
            self.account_store.get(&authority.account.metadata.id).await,
            Ok(Some(final_account)) if final_account == authority.account
        )
    }

    fn account_semaphore(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<Arc<Semaphore>, CloudflareDnsAdminError> {
        let mut accounts = self
            .accounts
            .lock()
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        if let Some(existing) = accounts.get(account_id).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        accounts.retain(|_, value| value.strong_count() > 0);
        if accounts.len() >= MAX_TRACKED_ACCOUNTS {
            return Err(CloudflareDnsAdminError::Unavailable);
        }
        let semaphore = Arc::new(Semaphore::new(self.per_account_concurrency));
        accounts.insert(account_id.clone(), Arc::downgrade(&semaphore));
        Ok(semaphore)
    }

    async fn put_record_set_inner(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
        request: &CloudflareRecordSetPutRequest,
        remote_caller_alias: Option<&str>,
    ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError> {
        let core_key = key.core();
        reject_soa_record_set(&core_key)?;
        let dispatched = Arc::new(AtomicBool::new(false));
        let marker = dispatched.clone();
        let result = tokio::time::timeout(self.timeout, async {
            let account_limit = self.account_semaphore(account_id)?;
            let _account_permit = acquire(account_limit).await?;
            let _global_permit = acquire(self.global.clone()).await?;
            let (writer, authority) = self.prepare(account_id, marker.clone()).await?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            let zone = observe_exact_zone(&writer, account_id, zone_id).await?;
            reject_apex_ns_record_set(&zone, &core_key)?;
            let desired = match remote_caller_alias {
                Some(alias) => request.remote_record_set(key, alias),
                None => request.record_set(key),
            }
            .map_err(|_| CloudflareDnsAdminError::InvalidRequest)?;
            let previous = writer
                .observe_record_set(&zone, &desired.key)
                .await
                .map_err(map_provider_error)?;
            let change = put_change(request, desired, previous)?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            let outcome = writer.apply_record_change(&zone, &change).await;
            if marker.load(Ordering::SeqCst) {
                if !self.authority_is_current(&authority).await {
                    return Err(CloudflareDnsAdminError::UnknownOutcome);
                }
                return match outcome {
                    Ok(CloudflareDnsSyncRecordOutcome::Present(observed)) => {
                        let dto = map_record(*observed)
                            .map_err(|_| CloudflareDnsAdminError::UnknownOutcome)?;
                        if remote_caller_alias.is_some_and(|alias| {
                            !matches!(
                                &dto.control,
                                CloudflareRecordControlDto::Remote { caller_alias }
                                    if caller_alias == alias
                            )
                        }) {
                            return Err(CloudflareDnsAdminError::UnknownOutcome);
                        }
                        Ok(dto)
                    }
                    Ok(CloudflareDnsSyncRecordOutcome::Deleted) => {
                        Err(CloudflareDnsAdminError::UnknownOutcome)
                    }
                    Err(error) => Err(map_provider_error(error)),
                };
            }
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            match outcome {
                Err(error) => Err(map_provider_error(error)),
                Ok(_) => Err(CloudflareDnsAdminError::UnknownOutcome),
            }
        })
        .await;
        classify_timeout(result, &dispatched)
    }
}

#[async_trait]
impl CloudflareDnsWriteAdminService for CloudflareDnsWriteService {
    async fn create_zone(
        &self,
        account_id: &CloudResourceId,
        request: &CloudflareZoneCreateRequest,
    ) -> Result<CloudflareZoneDto, CloudflareDnsAdminError> {
        let dispatched = Arc::new(AtomicBool::new(false));
        let dispatched_during_operation = dispatched.clone();
        let result = tokio::time::timeout(self.timeout, async {
            let account_limit = self.account_semaphore(account_id)?;
            let _account_permit = acquire(account_limit).await?;
            let _global_permit = acquire(self.global.clone()).await?;
            let (writer, authority) = self
                .prepare(account_id, dispatched_during_operation.clone())
                .await?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            let observed = writer
                .create_zone(&request.name)
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::UnknownOutcome);
            }
            map_post_dispatch_zone(observed)
        })
        .await;
        match result {
            Ok(result) => result,
            Err(_) if dispatched.load(Ordering::SeqCst) => {
                Err(CloudflareDnsAdminError::UnknownOutcome)
            }
            Err(_) => Err(CloudflareDnsAdminError::Unavailable),
        }
    }

    async fn delete_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        request: &CloudflareZoneDeleteRequest,
    ) -> Result<(), CloudflareDnsAdminError> {
        let dispatched = Arc::new(AtomicBool::new(false));
        let marker = dispatched.clone();
        let result = tokio::time::timeout(self.timeout, async {
            let account_limit = self.account_semaphore(account_id)?;
            let _account_permit = acquire(account_limit).await?;
            let _global_permit = acquire(self.global.clone()).await?;
            let (writer, authority) = self.prepare(account_id, marker.clone()).await?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            let guard = writer
                .preflight_zone_delete(zone_id, &request.confirm_name, &request.expected_revision)
                .await;
            let guard = guard.map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            let outcome = writer.delete_zone(guard).await;
            if marker.load(Ordering::SeqCst) {
                if !self.authority_is_current(&authority).await {
                    return Err(CloudflareDnsAdminError::UnknownOutcome);
                }
                return outcome.map_err(map_provider_error);
            }
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            outcome.map_err(map_provider_error)
        })
        .await;
        classify_timeout(result, &dispatched)
    }

    async fn put_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
        request: &CloudflareRecordSetPutRequest,
    ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError> {
        self.put_record_set_inner(account_id, zone_id, key, request, None)
            .await
    }

    async fn put_remote_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
        request: &CloudflareRecordSetPutRequest,
        caller_alias: &str,
    ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError> {
        self.put_record_set_inner(account_id, zone_id, key, request, Some(caller_alias))
            .await
    }

    async fn delete_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
        request: &CloudflareRecordSetDeleteRequest,
    ) -> Result<(), CloudflareDnsAdminError> {
        let core_key = key.core();
        reject_soa_record_set(&core_key)?;
        let dispatched = Arc::new(AtomicBool::new(false));
        let marker = dispatched.clone();
        let result = tokio::time::timeout(self.timeout, async {
            let account_limit = self.account_semaphore(account_id)?;
            let _account_permit = acquire(account_limit).await?;
            let _global_permit = acquire(self.global.clone()).await?;
            let (writer, authority) = self.prepare(account_id, marker.clone()).await?;
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            let zone = observe_exact_zone(&writer, account_id, zone_id).await?;
            reject_apex_ns_record_set(&zone, &core_key)?;
            let previous = writer
                .observe_record_set(&zone, &core_key)
                .await
                .map_err(map_provider_error)?
                .ok_or(CloudflareDnsAdminError::NotFound)?;
            if previous.revision != request.expected_revision {
                return Err(CloudflareDnsAdminError::Conflict);
            }
            let change = DnsRecordChange::Delete {
                guard: DnsMutationGuard::MatchObserved {
                    revision: request.expected_revision.clone(),
                },
                previous,
            };
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            let outcome = writer.apply_record_change(&zone, &change).await;
            if marker.load(Ordering::SeqCst) {
                if !self.authority_is_current(&authority).await {
                    return Err(CloudflareDnsAdminError::UnknownOutcome);
                }
                return match outcome {
                    Ok(CloudflareDnsSyncRecordOutcome::Deleted) => Ok(()),
                    Ok(CloudflareDnsSyncRecordOutcome::Present(_)) => {
                        Err(CloudflareDnsAdminError::UnknownOutcome)
                    }
                    Err(error) => Err(map_provider_error(error)),
                };
            }
            if !self.authority_is_current(&authority).await {
                return Err(CloudflareDnsAdminError::Unavailable);
            }
            match outcome {
                Err(error) => Err(map_provider_error(error)),
                Ok(_) => Err(CloudflareDnsAdminError::UnknownOutcome),
            }
        })
        .await;
        classify_timeout(result, &dispatched)
    }
}

fn classify_timeout<T>(
    result: Result<Result<T, CloudflareDnsAdminError>, tokio::time::error::Elapsed>,
    dispatched: &AtomicBool,
) -> Result<T, CloudflareDnsAdminError> {
    match result {
        Ok(result) => result,
        Err(_) if dispatched.load(Ordering::SeqCst) => Err(CloudflareDnsAdminError::UnknownOutcome),
        Err(_) => Err(CloudflareDnsAdminError::Unavailable),
    }
}

async fn observe_exact_zone(
    writer: &CloudflareDnsSyncWriter,
    account_id: &CloudResourceId,
    zone_id: &DnsZoneId,
) -> Result<edgion_center_core::DnsZoneRef, CloudflareDnsAdminError> {
    let observed = writer
        .observe_zone(zone_id)
        .await
        .map_err(map_provider_error)?
        .ok_or(CloudflareDnsAdminError::NotFound)?;
    if &observed.zone.provider_account_id != account_id
        || &observed.zone.zone_id != zone_id
        || observed.zone.provider != CloudProvider::Cloudflare
        || observed.zone.visibility != ZoneVisibility::Public
    {
        return Err(CloudflareDnsAdminError::InvalidProviderObservation);
    }
    Ok(observed.zone)
}

fn reject_soa_record_set(
    key: &edgion_center_core::DnsRecordSetKey,
) -> Result<(), CloudflareDnsAdminError> {
    if key.record_type == ProviderDnsRecordType::Soa {
        return Err(CloudflareDnsAdminError::InvalidRequest);
    }
    Ok(())
}

fn reject_apex_ns_record_set(
    zone: &edgion_center_core::DnsZoneRef,
    key: &edgion_center_core::DnsRecordSetKey,
) -> Result<(), CloudflareDnsAdminError> {
    if key.owner.as_str() == zone.apex.as_str() && key.record_type == ProviderDnsRecordType::Ns {
        return Err(CloudflareDnsAdminError::InvalidRequest);
    }
    Ok(())
}

fn put_change(
    request: &CloudflareRecordSetPutRequest,
    desired: edgion_center_core::ProviderDnsRecordSet,
    previous: Option<ObservedDnsRecordSet>,
) -> Result<DnsRecordChange, CloudflareDnsAdminError> {
    match (&request.guard, previous) {
        (CloudflareRecordMutationGuardDto::MustNotExist, None) => Ok(DnsRecordChange::Create {
            record_set: desired,
            guard: DnsMutationGuard::MustNotExist,
        }),
        (CloudflareRecordMutationGuardDto::MustNotExist, Some(_)) => {
            Err(CloudflareDnsAdminError::Conflict)
        }
        (CloudflareRecordMutationGuardDto::MatchRevision { revision }, Some(previous))
            if &previous.revision == revision =>
        {
            Ok(DnsRecordChange::Replace {
                previous,
                desired,
                guard: DnsMutationGuard::MatchObserved {
                    revision: revision.clone(),
                },
            })
        }
        (CloudflareRecordMutationGuardDto::MatchRevision { .. }, None) => {
            Err(CloudflareDnsAdminError::NotFound)
        }
        (CloudflareRecordMutationGuardDto::MatchRevision { .. }, Some(_)) => {
            Err(CloudflareDnsAdminError::Conflict)
        }
    }
}

async fn acquire(
    semaphore: Arc<Semaphore>,
) -> Result<OwnedSemaphorePermit, CloudflareDnsAdminError> {
    semaphore
        .acquire_owned()
        .await
        .map_err(|_| CloudflareDnsAdminError::Unavailable)
}

fn validate_account(
    requested_account_id: &CloudResourceId,
    account: &ProviderAccount,
) -> Result<(), CloudflareDnsAdminError> {
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
        return Err(CloudflareDnsAdminError::InvalidRequest);
    }
    Ok(())
}

fn map_provider_error(error: NormalizedProviderError) -> CloudflareDnsAdminError {
    edgion_center_app::common::observe::cloud_metrics::record_provider_error(
        "cloudflare",
        error.category(),
    );
    match error.category() {
        ProviderErrorCategory::Validation => CloudflareDnsAdminError::InvalidRequest,
        ProviderErrorCategory::NotFound => CloudflareDnsAdminError::NotFound,
        ProviderErrorCategory::Conflict => CloudflareDnsAdminError::Conflict,
        ProviderErrorCategory::UnknownOutcome => CloudflareDnsAdminError::UnknownOutcome,
        _ => CloudflareDnsAdminError::Unavailable,
    }
}

fn map_post_dispatch_zone(
    observed: edgion_center_adapter_cloudflare::ObservedCloudflareZone,
) -> Result<CloudflareZoneDto, CloudflareDnsAdminError> {
    map_zone(observed).map_err(|_| CloudflareDnsAdminError::UnknownOutcome)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::atomic::AtomicUsize,
    };

    use edgion_center_adapter_cloudflare::{
        CloudflareApiResult, CloudflareBatchRequest, CloudflareBatchResult,
        CloudflareCreateZoneRequest, CloudflareDeleteZoneAck, CloudflareDnssec,
        CloudflareDnssecStatus, CloudflarePage, CloudflareRecord, CloudflareZone,
        CloudflareZoneKind, CloudflareZoneStatus, ObservedCloudflareZone,
    };
    use edgion_center_adapter_credential_files::{
        MountedCredentialBinding, MountedCredentialConfig,
    };
    use edgion_center_core::{
        provider_account_from_desired, AbsoluteDnsName, CloudflareCnameFlattening,
        CloudflareProxyOptions, DeletionPolicy, DnsOwnerName, DnsRecordRevision, DnsZoneId,
        DnsZoneRef, DnssecDesiredState, ManagementPolicy, ProviderAccountCreateResult,
        ProviderAccountDesired, ProviderAccountPage, ProviderAccountPageRequest,
        ProviderAccountReplaceResult, ProviderAccountSpec, ZoneVisibility,
    };

    use super::*;

    const CENTER_ACCOUNT: &str = "cf-main";
    const NATIVE_ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
    const ZONE_ID: &str = "abcdef0123456789abcdef0123456789";

    struct Store(Mutex<Option<ProviderAccount>>);

    #[async_trait]
    impl ProviderAccountStore for Store {
        async fn create(
            &self,
            _: &CloudResourceId,
            _: &ProviderAccountDesired,
        ) -> CoreResult<ProviderAccountCreateResult> {
            unreachable!()
        }

        async fn get(&self, _: &CloudResourceId) -> CoreResult<Option<ProviderAccount>> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn list(&self, _: &ProviderAccountPageRequest) -> CoreResult<ProviderAccountPage> {
            unreachable!()
        }

        async fn replace_if_generation(
            &self,
            _: &CloudResourceId,
            _: u64,
            _: &ProviderAccountDesired,
        ) -> CoreResult<ProviderAccountReplaceResult> {
            unreachable!()
        }
    }

    fn account(generation: u64) -> ProviderAccount {
        provider_account_from_desired(
            CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
            generation,
            &ProviderAccountDesired {
                display_name: "Cloudflare main".into(),
                owner: None,
                labels: BTreeMap::new(),
                management_policy: ManagementPolicy::Managed,
                deletion_policy: DeletionPolicy::Retain,
                spec: ProviderAccountSpec {
                    provider: CloudProvider::Cloudflare,
                    scope: Some(ProviderAccountScope::Cloudflare {
                        account_id: NATIVE_ACCOUNT.into(),
                    }),
                    credential_source: CredentialSource::StaticSecret {
                        credential_ref: edgion_center_core::CredentialRef::new("cloudflare/token")
                            .unwrap(),
                    },
                },
            },
        )
        .unwrap()
    }

    async fn mounted() -> (tempfile::TempDir, Arc<MountedCredentialResolver>) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("revision.key"), [9_u8; 32]).unwrap();
        std::fs::write(directory.path().join("token"), b"token").unwrap();
        let resolver = MountedCredentialResolver::from_config(&MountedCredentialConfig {
            enabled: true,
            root_directory: Some(directory.path().to_string_lossy().into_owned()),
            revision_key_file: Some("revision.key".into()),
            bindings: vec![MountedCredentialBinding {
                credential_ref: "cloudflare/token".into(),
                provider_account_id: CENTER_ACCOUNT.into(),
                provider: CloudProvider::Cloudflare,
                purpose: CredentialPurpose::CloudflareApiToken,
                file: "token".into(),
            }],
        })
        .await
        .unwrap()
        .unwrap();
        (directory, Arc::new(resolver))
    }

    struct FakeApi {
        calls: AtomicUsize,
        delete_calls: AtomicUsize,
        zone_reads: AtomicUsize,
        record_reads: AtomicUsize,
        batch_calls: AtomicUsize,
        active: AtomicUsize,
        peak: AtomicUsize,
        delay: Duration,
        delete_delay: Duration,
        batch_delay: Duration,
        corrupt_remote_marker: AtomicBool,
        zone_present: AtomicBool,
        delete_ack_id: Mutex<String>,
        dnssec: Mutex<Option<CloudflareDnssec>>,
        records: Mutex<Vec<CloudflareRecord>>,
        rotate_store_during_preflight: Option<Arc<Store>>,
        rotate_store: Option<Arc<Store>>,
    }

    impl FakeApi {
        fn new(delay: Duration, rotate_store: Option<Arc<Store>>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                delete_calls: AtomicUsize::new(0),
                zone_reads: AtomicUsize::new(0),
                record_reads: AtomicUsize::new(0),
                batch_calls: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
                delay,
                delete_delay: Duration::ZERO,
                batch_delay: Duration::ZERO,
                corrupt_remote_marker: AtomicBool::new(false),
                zone_present: AtomicBool::new(true),
                delete_ack_id: Mutex::new(ZONE_ID.into()),
                dnssec: Mutex::new(None),
                records: Mutex::new(Vec::new()),
                rotate_store_during_preflight: None,
                rotate_store,
            }
        }

        fn record_peak(&self, active: usize) {
            self.peak.fetch_max(active, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl CloudflareApi for FakeApi {
        async fn create_zone(
            &self,
            request: &CloudflareCreateZoneRequest,
        ) -> CloudflareApiResult<CloudflareZone> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.record_peak(active);
            tokio::time::sleep(self.delay).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            if let Some(store) = &self.rotate_store {
                *store.0.lock().unwrap() = Some(account(2));
            }
            Ok(CloudflareZone {
                id: ZONE_ID.into(),
                account_id: request.account_id.clone(),
                name: request.name.clone(),
                kind: CloudflareZoneKind::Full,
                status: CloudflareZoneStatus::Active,
                name_servers: BTreeSet::from([AbsoluteDnsName::new("ns1.example.net").unwrap()]),
                modified_on: Some("revision".into()),
            })
        }

        async fn get_zone(&self, _: &str) -> CloudflareApiResult<Option<CloudflareZone>> {
            self.zone_reads.fetch_add(1, Ordering::SeqCst);
            if !self.zone_present.load(Ordering::SeqCst) {
                return Ok(None);
            }
            Ok(Some(CloudflareZone {
                id: ZONE_ID.into(),
                account_id: NATIVE_ACCOUNT.into(),
                name: "example.com".into(),
                kind: CloudflareZoneKind::Full,
                status: CloudflareZoneStatus::Active,
                name_servers: BTreeSet::from([AbsoluteDnsName::new("ns1.example.net").unwrap()]),
                modified_on: Some("revision".into()),
            }))
        }

        async fn delete_zone(&self, _: &str) -> CloudflareApiResult<CloudflareDeleteZoneAck> {
            self.delete_calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.record_peak(active);
            tokio::time::sleep(self.delete_delay).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            if let Some(store) = &self.rotate_store {
                *store.0.lock().unwrap() = Some(account(2));
            }
            Ok(CloudflareDeleteZoneAck {
                id: self.delete_ack_id.lock().unwrap().clone(),
            })
        }

        async fn get_dnssec(&self, _: &str) -> CloudflareApiResult<Option<CloudflareDnssec>> {
            if let Some(store) = &self.rotate_store_during_preflight {
                *store.0.lock().unwrap() = Some(account(2));
            }
            Ok(self.dnssec.lock().unwrap().clone())
        }

        async fn patch_dnssec(
            &self,
            _: &str,
            _: DnssecDesiredState,
        ) -> CloudflareApiResult<CloudflareDnssec> {
            unreachable!()
        }

        async fn list_zones(
            &self,
            _: &str,
            _: u32,
            _: u32,
        ) -> CloudflareApiResult<CloudflarePage<CloudflareZone>> {
            unreachable!()
        }

        async fn list_records(
            &self,
            _: &str,
            page: u32,
            _: u32,
        ) -> CloudflareApiResult<CloudflarePage<CloudflareRecord>> {
            self.record_reads.fetch_add(1, Ordering::SeqCst);
            Ok(CloudflarePage {
                items: self.records.lock().unwrap().clone(),
                page,
                total_pages: 1,
            })
        }

        async fn batch_records(
            &self,
            _: &str,
            request: &CloudflareBatchRequest,
        ) -> CloudflareApiResult<CloudflareBatchResult> {
            let batch_number = self.batch_calls.fetch_add(1, Ordering::SeqCst) + 1;
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.record_peak(active);
            tokio::time::sleep(self.batch_delay).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            let mut records = self.records.lock().unwrap();
            let deleted = records
                .iter()
                .filter(|record| request.deletes.iter().any(|delete| delete.id == record.id))
                .cloned()
                .collect::<Vec<_>>();
            records.retain(|record| !request.deletes.iter().any(|delete| delete.id == record.id));
            let mut posts = request
                .posts
                .iter()
                .enumerate()
                .map(|(index, post)| fake_record(post, batch_number * 100 + index))
                .collect::<Vec<_>>();
            if self.corrupt_remote_marker.load(Ordering::SeqCst) {
                for post in &mut posts {
                    post.tags.retain(|tag| !tag.starts_with("edgion-center-"));
                    post.tags
                        .insert(format!("edgion-center-remote:{}", "m".repeat(43)));
                }
            }
            records.extend(posts.clone());
            if let Some(store) = &self.rotate_store {
                *store.0.lock().unwrap() = Some(account(2));
            }
            Ok(CloudflareBatchResult {
                deletes: deleted,
                posts,
            })
        }
    }

    fn fake_record(
        post: &edgion_center_adapter_cloudflare::CloudflareBatchRecord,
        index: usize,
    ) -> CloudflareRecord {
        let value = match post.kind.as_str() {
            "A" => edgion_center_core::DnsRecordSetValue::A {
                address: post.content.as_deref().unwrap().parse().unwrap(),
            },
            _ => panic!("test only supports A records"),
        };
        CloudflareRecord {
            id: format!("{index:032x}"),
            name: post.name.clone(),
            ttl: post.ttl,
            value,
            proxied: post.proxied,
            proxiable: true,
            flatten_cname: None,
            ipv4_only: false,
            ipv6_only: false,
            private_routing: false,
            comment: post.comment.clone(),
            tags: post.tags.clone(),
            modified_on: Some("record-revision".into()),
        }
    }

    struct FakeFactory(Arc<dyn CloudflareApi>);

    impl ApiFactory for FakeFactory {
        fn build(
            &self,
            _: CloudflareApiToken,
        ) -> Result<Arc<dyn CloudflareApi>, NormalizedProviderError> {
            Ok(self.0.clone())
        }
    }

    fn service(
        store: Arc<Store>,
        resolver: Arc<MountedCredentialResolver>,
        api: Arc<dyn CloudflareApi>,
        per_account_concurrency: usize,
    ) -> CloudflareDnsWriteService {
        CloudflareDnsWriteService {
            account_store: store,
            mounted_resolver: resolver,
            timeout: Duration::from_secs(5),
            global: Arc::new(Semaphore::new(4)),
            per_account_concurrency,
            accounts: Mutex::new(HashMap::new()),
            api_factory: Arc::new(FakeFactory(api)),
        }
    }

    fn request(name: &str) -> CloudflareZoneCreateRequest {
        CloudflareZoneCreateRequest {
            name: AbsoluteDnsName::new(name).unwrap(),
        }
    }

    fn delete_request(name: &str, revision: &str) -> CloudflareZoneDeleteRequest {
        CloudflareZoneDeleteRequest {
            expected_revision: DnsRecordRevision::new(revision).unwrap(),
            confirm_name: AbsoluteDnsName::new(name).unwrap(),
        }
    }

    fn user_record() -> CloudflareRecord {
        CloudflareRecord {
            id: "11111111111111111111111111111111".into(),
            name: "www.example.com".into(),
            ttl: 300,
            value: edgion_center_core::DnsRecordSetValue::A {
                address: "192.0.2.1".parse().unwrap(),
            },
            proxied: Some(false),
            proxiable: true,
            flatten_cname: None,
            ipv4_only: false,
            ipv6_only: false,
            private_routing: false,
            comment: None,
            tags: BTreeSet::new(),
            modified_on: Some("record-revision".into()),
        }
    }

    fn record_key(owner: &str) -> CloudflareRecordSetKey {
        CloudflareRecordSetKey {
            owner: DnsOwnerName::new(owner).unwrap(),
            record_type: edgion_center_app::api::cloudflare_dns::CloudflareRecordType::A,
        }
    }

    fn record_put(guard: CloudflareRecordMutationGuardDto) -> CloudflareRecordSetPutRequest {
        CloudflareRecordSetPutRequest {
            guard,
            ttl: edgion_center_app::api::cloudflare_dns::CloudflareRecordTtlDto::Automatic,
            values: vec![
                edgion_center_app::api::cloudflare_dns::CloudflareRecordValueDto::A {
                    address: "192.0.2.10".parse().unwrap(),
                },
            ],
            proxy: Some(CloudflareProxyOptions::DnsOnly),
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: None,
            tags: Vec::new(),
        }
    }

    fn remote_record_put(
        guard: CloudflareRecordMutationGuardDto,
        addresses: [&str; 2],
    ) -> CloudflareRecordSetPutRequest {
        CloudflareRecordSetPutRequest {
            guard,
            ttl: edgion_center_app::api::cloudflare_dns::CloudflareRecordTtlDto::Automatic,
            values: addresses
                .into_iter()
                .map(|address| {
                    edgion_center_app::api::cloudflare_dns::CloudflareRecordValueDto::A {
                        address: address.parse().unwrap(),
                    }
                })
                .collect(),
            proxy: Some(CloudflareProxyOptions::DnsOnly),
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: None,
            tags: vec!["owner:center".into()],
        }
    }

    fn soa_record_key(owner: &str) -> CloudflareRecordSetKey {
        CloudflareRecordSetKey {
            owner: DnsOwnerName::new(owner).unwrap(),
            record_type: edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Soa,
        }
    }

    fn soa_record_put() -> CloudflareRecordSetPutRequest {
        CloudflareRecordSetPutRequest {
            guard: CloudflareRecordMutationGuardDto::MustNotExist,
            ttl: edgion_center_app::api::cloudflare_dns::CloudflareRecordTtlDto::Seconds(300),
            values: vec![
                edgion_center_app::api::cloudflare_dns::CloudflareRecordValueDto::Soa {
                    primary_name_server: AbsoluteDnsName::new("ns1.example.net").unwrap(),
                    responsible_mailbox: AbsoluteDnsName::new("hostmaster.example.com").unwrap(),
                    serial: 1,
                    refresh: 3_600,
                    retry: 600,
                    expire: 86_400,
                    minimum: 300,
                },
            ],
            proxy: None,
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn config_is_default_off_strict_and_bounded() {
        assert!(
            compose_dns_write_admin(&CloudflareDnsWriteConfig::default(), None, None)
                .unwrap()
                .is_none()
        );
        let config: CloudflareDnsWriteConfig = serde_yaml::from_str(
            "enabled: true\noperation_timeout_secs: 30\nglobal_concurrency: 4\nper_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(config.enabled);
        assert!(serde_yaml::from_str::<CloudflareDnsWriteConfig>("unknown: true\n").is_err());
        for invalid in [
            CloudflareDnsWriteConfig {
                operation_timeout_secs: 0,
                ..config.clone()
            },
            CloudflareDnsWriteConfig {
                global_concurrency: MAX_GLOBAL_CONCURRENCY + 1,
                ..config.clone()
            },
            CloudflareDnsWriteConfig {
                per_account_concurrency: MAX_ACCOUNT_CONCURRENCY + 1,
                ..config.clone()
            },
            CloudflareDnsWriteConfig {
                global_concurrency: 1,
                per_account_concurrency: 2,
                ..config
            },
        ] {
            assert!(compose_dns_write_admin(&invalid, None, None).is_err());
        }
    }

    #[tokio::test]
    async fn observe_only_account_is_rejected_before_provider_dispatch() {
        let (_directory, resolver) = mounted().await;
        let mut observe_only = account(1);
        observe_only.metadata.management_policy = ManagementPolicy::ObserveOnly;
        let store = Arc::new(Store(Mutex::new(Some(observe_only))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            service
                .create_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &request("example.com"),
                )
                .await,
            Err(CloudflareDnsAdminError::InvalidRequest)
        );
        assert_eq!(api.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn mismatched_store_identity_is_rejected_before_provider_dispatch() {
        let (_directory, resolver) = mounted().await;
        let mut mismatched = account(1);
        mismatched.metadata.id = CloudResourceId::new("cf-other").unwrap();
        let store = Arc::new(Store(Mutex::new(Some(mismatched))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            service
                .create_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &request("example.com"),
                )
                .await,
            Err(CloudflareDnsAdminError::InvalidRequest)
        );
        assert_eq!(api.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn create_zone_succeeds_and_authority_race_is_unknown() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let success_service = service(store, resolver, api.clone(), 1);
        let zone = success_service
            .create_zone(
                &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                &request("example.com"),
            )
            .await
            .unwrap();
        assert_eq!(zone.name.as_str(), "example.com");
        assert_eq!(api.calls.load(Ordering::SeqCst), 1);

        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, Some(store.clone())));
        let raced_service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            raced_service
                .create_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &request("example.net"),
                )
                .await,
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
        assert_eq!(api.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn zone_delete_succeeds_only_after_all_fresh_guards() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);

        service
            .delete_zone(
                &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                &DnsZoneId::new(ZONE_ID).unwrap(),
                &delete_request("example.com", "revision"),
            )
            .await
            .unwrap();

        assert_eq!(api.zone_reads.load(Ordering::SeqCst), 1);
        assert_eq!(api.record_reads.load(Ordering::SeqCst), 1);
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn zone_delete_name_and_revision_conflicts_never_dispatch() {
        for request in [
            delete_request("other.example", "revision"),
            delete_request("example.com", "stale-revision"),
        ] {
            let (_directory, resolver) = mounted().await;
            let store = Arc::new(Store(Mutex::new(Some(account(1)))));
            let api = Arc::new(FakeApi::new(Duration::ZERO, None));
            let service = service(store, resolver, api.clone(), 1);
            assert_eq!(
                service
                    .delete_zone(
                        &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                        &DnsZoneId::new(ZONE_ID).unwrap(),
                        &request,
                    )
                    .await,
                Err(CloudflareDnsAdminError::Conflict)
            );
            assert_eq!(api.delete_calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn zone_delete_authority_rotation_during_preflight_never_dispatches() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let mut rotating = FakeApi::new(Duration::ZERO, None);
        rotating.rotate_store_during_preflight = Some(store.clone());
        let api = Arc::new(rotating);
        let service = service(store, resolver, api.clone(), 1);

        assert_eq!(
            service
                .delete_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &delete_request("example.com", "revision"),
                )
                .await,
            Err(CloudflareDnsAdminError::Unavailable)
        );
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn zone_delete_rejects_nonempty_or_dnssec_enabled_state_without_dispatch() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        api.records.lock().unwrap().push(user_record());
        let nonempty_service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            nonempty_service
                .delete_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &delete_request("example.com", "revision"),
                )
                .await,
            Err(CloudflareDnsAdminError::Conflict)
        );
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 0);

        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        *api.dnssec.lock().unwrap() = Some(CloudflareDnssec {
            status: CloudflareDnssecStatus::Active,
            ds: None,
            modified_on: Some("dnssec-revision".into()),
        });
        let dnssec_service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            dnssec_service
                .delete_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &delete_request("example.com", "revision"),
                )
                .await,
            Err(CloudflareDnsAdminError::Conflict)
        );
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn zone_delete_not_found_never_dispatches() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        api.zone_present.store(false, Ordering::SeqCst);
        let service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            service
                .delete_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &delete_request("example.com", "revision"),
                )
                .await,
            Err(CloudflareDnsAdminError::NotFound)
        );
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn zone_delete_post_dispatch_ambiguity_is_unknown() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let mut delayed = FakeApi::new(Duration::ZERO, None);
        delayed.delete_delay = Duration::from_millis(50);
        let api = Arc::new(delayed);
        let mut timeout_service = service(store, resolver, api.clone(), 1);
        timeout_service.timeout = Duration::from_millis(10);
        assert_eq!(
            timeout_service
                .delete_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &delete_request("example.com", "revision"),
                )
                .await,
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 1);

        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, Some(store.clone())));
        let rotated_service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            rotated_service
                .delete_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &delete_request("example.com", "revision"),
                )
                .await,
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 1);

        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        *api.delete_ack_id.lock().unwrap() = "ffffffffffffffffffffffffffffffff".into();
        let mismatched_ack_service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            mismatched_ack_service
                .delete_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &delete_request("example.com", "revision"),
                )
                .await,
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn zone_delete_shares_the_existing_per_account_limit() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let mut delayed = FakeApi::new(Duration::ZERO, None);
        delayed.delete_delay = Duration::from_millis(20);
        let api = Arc::new(delayed);
        let service = service(store, resolver, api.clone(), 1);
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let request = delete_request("example.com", "revision");
        let (first, second) = tokio::join!(
            service.delete_zone(&account_id, &zone_id, &request),
            service.delete_zone(&account_id, &zone_id, &request),
        );
        first.unwrap();
        second.unwrap();
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 2);
        assert_eq!(api.peak.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unknown_provider_outcome_is_preserved() {
        let error = NormalizedProviderError::new(
            ProviderErrorCategory::UnknownOutcome,
            "provider_unknown",
            "provider details",
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            map_provider_error(error),
            CloudflareDnsAdminError::UnknownOutcome
        );
    }

    #[test]
    fn post_dispatch_mapping_failure_is_unknown_outcome() {
        let observed = ObservedCloudflareZone {
            zone: DnsZoneRef {
                provider_account_id: CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                provider: CloudProvider::Cloudflare,
                zone_id: DnsZoneId::new(ZONE_ID).unwrap(),
                apex: AbsoluteDnsName::new("example.com").unwrap(),
                visibility: ZoneVisibility::Public,
            },
            kind: CloudflareZoneKind::Internal,
            status: CloudflareZoneStatus::Active,
            name_servers: BTreeSet::from([AbsoluteDnsName::new("ns1.example.net").unwrap()]),
            revision: Some(DnsRecordRevision::new("revision").unwrap()),
        };
        assert_eq!(
            map_post_dispatch_zone(observed),
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
    }

    #[tokio::test]
    async fn timeout_classification_tracks_provider_dispatch() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let mut pre_dispatch_service = service(store, resolver, api.clone(), 1);
        pre_dispatch_service.timeout = Duration::from_millis(10);
        let held_global = pre_dispatch_service
            .global
            .clone()
            .acquire_many_owned(4)
            .await
            .unwrap();
        assert_eq!(
            pre_dispatch_service
                .create_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &request("before.example"),
                )
                .await,
            Err(CloudflareDnsAdminError::Unavailable)
        );
        assert_eq!(api.calls.load(Ordering::SeqCst), 0);
        drop(held_global);

        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::from_millis(50), None));
        let mut post_dispatch_service = service(store, resolver, api.clone(), 1);
        post_dispatch_service.timeout = Duration::from_millis(10);
        assert_eq!(
            post_dispatch_service
                .create_zone(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &request("after.example"),
                )
                .await,
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
        assert_eq!(api.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn per_account_limit_serializes_provider_dispatch() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::from_millis(20), None));
        let service = service(store, resolver, api.clone(), 1);
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let first = request("one.example");
        let second = request("two.example");
        let (first, second) = tokio::join!(
            service.create_zone(&account_id, &first),
            service.create_zone(&account_id, &second)
        );
        first.unwrap();
        second.unwrap();
        assert_eq!(api.calls.load(Ordering::SeqCst), 2);
        assert_eq!(api.peak.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rrset_put_and_delete_succeed_with_one_batch_each() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let key = record_key("www.example.com");
        let created = service
            .put_record_set(
                &account_id,
                &zone_id,
                &key,
                &record_put(CloudflareRecordMutationGuardDto::MustNotExist),
            )
            .await
            .unwrap();
        assert_eq!(created.owner, key.owner);
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 1);

        service
            .delete_record_set(
                &account_id,
                &zone_id,
                &key,
                &CloudflareRecordSetDeleteRequest {
                    expected_revision: created.revision,
                },
            )
            .await
            .unwrap();
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 2);
        assert!(api.records.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn remote_rrset_create_and_replace_use_one_batch_and_one_marker() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let key = record_key("remote.example.com");
        let alias = "r".repeat(43);

        let created = service
            .put_remote_record_set(
                &account_id,
                &zone_id,
                &key,
                &remote_record_put(
                    CloudflareRecordMutationGuardDto::MustNotExist,
                    ["192.0.2.10", "192.0.2.11"],
                ),
                &alias,
            )
            .await
            .unwrap();
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            created.control,
            CloudflareRecordControlDto::Remote {
                caller_alias: alias.clone()
            }
        );
        assert_eq!(created.tags, vec!["owner:center"]);
        let expected_marker = format!("edgion-center-remote:{alias}");
        let records = api.records.lock().unwrap().clone();
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|record| record.tags.contains(&expected_marker)));

        let replaced = service
            .put_remote_record_set(
                &account_id,
                &zone_id,
                &key,
                &remote_record_put(
                    CloudflareRecordMutationGuardDto::MatchRevision {
                        revision: created.revision,
                    },
                    ["192.0.2.20", "192.0.2.21"],
                ),
                &alias,
            )
            .await
            .unwrap();
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 2);
        assert!(matches!(
            replaced.control,
            CloudflareRecordControlDto::Remote { caller_alias } if caller_alias == alias
        ));
        let records = api.records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|record| record.tags.contains(&expected_marker)));
    }

    #[tokio::test]
    async fn remote_rrset_stale_guard_and_scope_mismatch_do_not_dispatch() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        api.records.lock().unwrap().push(user_record());
        let service = service(store, resolver, api.clone(), 1);
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let alias = "s".repeat(43);

        assert_eq!(
            service
                .put_remote_record_set(
                    &account_id,
                    &zone_id,
                    &record_key("www.example.com"),
                    &remote_record_put(
                        CloudflareRecordMutationGuardDto::MatchRevision {
                            revision: DnsRecordRevision::new("stale").unwrap(),
                        },
                        ["192.0.2.20", "192.0.2.21"],
                    ),
                    &alias,
                )
                .await,
            Err(CloudflareDnsAdminError::Conflict)
        );
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 0);

        assert_eq!(
            service
                .put_remote_record_set(
                    &CloudResourceId::new("cf-other").unwrap(),
                    &zone_id,
                    &record_key("other.example.com"),
                    &remote_record_put(
                        CloudflareRecordMutationGuardDto::MustNotExist,
                        ["192.0.2.20", "192.0.2.21"],
                    ),
                    &alias,
                )
                .await,
            Err(CloudflareDnsAdminError::InvalidRequest)
        );
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn remote_rrset_marker_mismatch_after_dispatch_is_unknown() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        api.corrupt_remote_marker.store(true, Ordering::SeqCst);
        let service = service(store, resolver, api.clone(), 1);

        assert_eq!(
            service
                .put_remote_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &record_key("mismatch.example.com"),
                    &remote_record_put(
                        CloudflareRecordMutationGuardDto::MustNotExist,
                        ["192.0.2.30", "192.0.2.31"],
                    ),
                    &"e".repeat(43),
                )
                .await,
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ordinary_put_with_exact_guard_clears_remote_marker() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let key = record_key("manual.example.com");
        let remote = service
            .put_remote_record_set(
                &account_id,
                &zone_id,
                &key,
                &remote_record_put(
                    CloudflareRecordMutationGuardDto::MustNotExist,
                    ["192.0.2.40", "192.0.2.41"],
                ),
                &"c".repeat(43),
            )
            .await
            .unwrap();

        let manual = service
            .put_record_set(
                &account_id,
                &zone_id,
                &key,
                &remote_record_put(
                    CloudflareRecordMutationGuardDto::MatchRevision {
                        revision: remote.revision,
                    },
                    ["192.0.2.50", "192.0.2.51"],
                ),
            )
            .await
            .unwrap();
        assert_eq!(manual.control, CloudflareRecordControlDto::Manual);
        assert!(api.records.lock().unwrap().iter().all(|record| record
            .tags
            .iter()
            .all(|tag| !tag.starts_with("edgion-center-"))));
    }

    #[tokio::test]
    async fn remote_rrset_preserves_authority_and_concurrency_guards() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, Some(store.clone())));
        let authority_service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            authority_service
                .put_remote_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &record_key("authority.example.com"),
                    &remote_record_put(
                        CloudflareRecordMutationGuardDto::MustNotExist,
                        ["192.0.2.60", "192.0.2.61"],
                    ),
                    &"a".repeat(43),
                )
                .await,
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 1);

        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let mut delayed = FakeApi::new(Duration::ZERO, None);
        delayed.batch_delay = Duration::from_millis(20);
        let api = Arc::new(delayed);
        let service = service(store, resolver, api.clone(), 1);
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let first_key = record_key("remote-one.example.com");
        let second_key = record_key("remote-two.example.com");
        let first = remote_record_put(
            CloudflareRecordMutationGuardDto::MustNotExist,
            ["192.0.2.70", "192.0.2.71"],
        );
        let second = remote_record_put(
            CloudflareRecordMutationGuardDto::MustNotExist,
            ["192.0.2.80", "192.0.2.81"],
        );
        let alias = "q".repeat(43);
        let (first, second) = tokio::join!(
            service.put_remote_record_set(&account_id, &zone_id, &first_key, &first, &alias),
            service.put_remote_record_set(&account_id, &zone_id, &second_key, &second, &alias),
        );
        first.unwrap();
        second.unwrap();
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 2);
        assert_eq!(api.peak.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rrset_guard_conflict_performs_no_mutation() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let key = record_key("www.example.com");
        service
            .put_record_set(
                &account_id,
                &zone_id,
                &key,
                &record_put(CloudflareRecordMutationGuardDto::MustNotExist),
            )
            .await
            .unwrap();
        let before = api.batch_calls.load(Ordering::SeqCst);
        assert_eq!(
            service
                .put_record_set(
                    &account_id,
                    &zone_id,
                    &key,
                    &record_put(CloudflareRecordMutationGuardDto::MustNotExist),
                )
                .await,
            Err(CloudflareDnsAdminError::Conflict)
        );
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), before);
    }

    #[tokio::test]
    async fn rrset_post_dispatch_timeout_and_authority_change_are_unknown() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let mut delayed = FakeApi::new(Duration::ZERO, None);
        delayed.batch_delay = Duration::from_millis(50);
        let api = Arc::new(delayed);
        let mut timed_service = service(store, resolver, api.clone(), 1);
        timed_service.timeout = Duration::from_millis(10);
        assert_eq!(
            timed_service
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &record_key("timeout.example.com"),
                    &record_put(CloudflareRecordMutationGuardDto::MustNotExist),
                )
                .await,
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 1);

        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, Some(store.clone())));
        let service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            service
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &record_key("race.example.com"),
                    &record_put(CloudflareRecordMutationGuardDto::MustNotExist),
                )
                .await,
            Err(CloudflareDnsAdminError::UnknownOutcome)
        );
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn apex_delegation_is_rejected_without_mutation() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);
        let request = CloudflareRecordSetPutRequest {
            guard: CloudflareRecordMutationGuardDto::MustNotExist,
            ttl: edgion_center_app::api::cloudflare_dns::CloudflareRecordTtlDto::Seconds(300),
            values: vec![
                edgion_center_app::api::cloudflare_dns::CloudflareRecordValueDto::Ns {
                    target: AbsoluteDnsName::new("ns1.example.net").unwrap(),
                },
            ],
            proxy: None,
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: None,
            tags: Vec::new(),
        };
        assert_eq!(
            service
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &CloudflareRecordSetKey {
                        owner: DnsOwnerName::new("example.com").unwrap(),
                        record_type:
                            edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Ns,
                    },
                    &request,
                )
                .await,
            Err(CloudflareDnsAdminError::InvalidRequest)
        );
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn non_apex_soa_put_is_rejected_without_mutation() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            service
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &soa_record_key("sub.example.com"),
                    &soa_record_put(),
                )
                .await,
            Err(CloudflareDnsAdminError::InvalidRequest)
        );
        assert_eq!(api.calls.load(Ordering::SeqCst), 0);
        assert_eq!(api.zone_reads.load(Ordering::SeqCst), 0);
        assert_eq!(api.record_reads.load(Ordering::SeqCst), 0);
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn non_apex_soa_delete_is_rejected_without_mutation() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let api = Arc::new(FakeApi::new(Duration::ZERO, None));
        let service = service(store, resolver, api.clone(), 1);
        assert_eq!(
            service
                .delete_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &soa_record_key("sub.example.com"),
                    &CloudflareRecordSetDeleteRequest {
                        expected_revision: DnsRecordRevision::new("expected-revision").unwrap(),
                    },
                )
                .await,
            Err(CloudflareDnsAdminError::InvalidRequest)
        );
        assert_eq!(api.calls.load(Ordering::SeqCst), 0);
        assert_eq!(api.zone_reads.load(Ordering::SeqCst), 0);
        assert_eq!(api.record_reads.load(Ordering::SeqCst), 0);
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn rrset_writes_share_the_existing_per_account_limit() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let mut delayed = FakeApi::new(Duration::ZERO, None);
        delayed.batch_delay = Duration::from_millis(20);
        let api = Arc::new(delayed);
        let service = service(store, resolver, api.clone(), 1);
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let first_key = record_key("one.example.com");
        let second_key = record_key("two.example.com");
        let first_request = record_put(CloudflareRecordMutationGuardDto::MustNotExist);
        let second_request = record_put(CloudflareRecordMutationGuardDto::MustNotExist);
        let (first, second) = tokio::join!(
            service.put_record_set(&account_id, &zone_id, &first_key, &first_request),
            service.put_record_set(&account_id, &zone_id, &second_key, &second_request),
        );
        first.unwrap();
        second.unwrap();
        assert_eq!(api.batch_calls.load(Ordering::SeqCst), 2);
        assert_eq!(api.peak.load(Ordering::SeqCst), 1);
    }
}
