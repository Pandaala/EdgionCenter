use std::{
    collections::HashMap,
    future::Future,
    str,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_adapter_cloudflare::{
    CloudflareApi, CloudflareApiToken, CloudflareCursorKey, CloudflareCursorKeyRing,
    CloudflareDnsAdapter, CloudflareHttpApi, CloudflareZoneInventory, ObservedCloudflareZone,
};
use edgion_center_adapter_credential_files::{
    CredentialPurpose, MountedCredentialResolver, ResolveCredentialRequest, ResolvedCredential,
};
use edgion_center_app::api::cloudflare_dns::{
    CloudflareDnsAdminError, CloudflareDnsAdminService, CloudflareRecordPageDto,
    CloudflareRecordPageRequest, CloudflareRecordPageResult, CloudflareRecordSetDto,
    CloudflareRecordSetKey, CloudflareZoneDto, CloudflareZonePageDto, CloudflareZonePageRequest,
    SharedCloudflareDnsAdminService,
};
use edgion_center_core::{
    CloudProvider, CloudResourceId, CoreError, CoreResult, CredentialRef, CredentialSource,
    DnsPageRequest, DnsProvider, DnsRecordSetKey, DnsRoutingIdentity, DnsZoneId,
    NormalizedProviderError, ProviderAccount, ProviderAccountScope, ProviderAccountStore,
    ProviderDnsRecordType, ProviderErrorCategory,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

use crate::dns_admin::{map_record, map_zone};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 120;
const DEFAULT_GLOBAL_CONCURRENCY: usize = 16;
const MAX_GLOBAL_CONCURRENCY: usize = 256;
const DEFAULT_ACCOUNT_CONCURRENCY: usize = 2;
const MAX_ACCOUNT_CONCURRENCY: usize = 32;
const MAX_TRACKED_ACCOUNTS: usize = 1_024;
const DEFAULT_CURSOR_MAX_LIFETIME_SECS: u64 = 900;
const MAX_CURSOR_MAX_LIFETIME_SECS: u64 = 3_600;
const DEFAULT_CURSOR_CLOCK_SKEW_SECS: u64 = 30;
const MAX_CURSOR_CLOCK_SKEW_SECS: u64 = 300;

/// Strict, default-off production switch for read-only Cloudflare DNS inventory.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloudflareDnsReadConfig {
    pub enabled: bool,
    /// Active signing key. The name is retained for configuration compatibility.
    pub cursor_key_ref: Option<String>,
    /// Optional verification-only key used during a bounded rotation overlap.
    pub cursor_fallback_key_ref: Option<String>,
    pub cursor_max_lifetime_secs: u64,
    pub cursor_clock_skew_secs: u64,
    pub operation_timeout_secs: u64,
    pub global_concurrency: usize,
    pub per_account_concurrency: usize,
}

impl Default for CloudflareDnsReadConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cursor_key_ref: None,
            cursor_fallback_key_ref: None,
            cursor_max_lifetime_secs: DEFAULT_CURSOR_MAX_LIFETIME_SECS,
            cursor_clock_skew_secs: DEFAULT_CURSOR_CLOCK_SKEW_SECS,
            operation_timeout_secs: DEFAULT_TIMEOUT_SECS,
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            per_account_concurrency: DEFAULT_ACCOUNT_CONCURRENCY,
        }
    }
}

impl std::fmt::Debug for CloudflareDnsReadConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CloudflareDnsReadConfig")
            .field("enabled", &self.enabled)
            .field(
                "cursor_key_ref",
                &self.cursor_key_ref.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "cursor_fallback_key_ref",
                &self.cursor_fallback_key_ref.as_ref().map(|_| "[REDACTED]"),
            )
            .field("cursor_max_lifetime_secs", &self.cursor_max_lifetime_secs)
            .field("cursor_clock_skew_secs", &self.cursor_clock_skew_secs)
            .field("operation_timeout_secs", &self.operation_timeout_secs)
            .field("global_concurrency", &self.global_concurrency)
            .field("per_account_concurrency", &self.per_account_concurrency)
            .finish()
    }
}

pub fn compose_dns_admin(
    config: &CloudflareDnsReadConfig,
    account_store: Option<Arc<dyn ProviderAccountStore>>,
    mounted_resolver: Option<Arc<MountedCredentialResolver>>,
) -> CoreResult<Option<SharedCloudflareDnsAdminService>> {
    if !config.enabled {
        return Ok(None);
    }
    if !(1..=MAX_TIMEOUT_SECS).contains(&config.operation_timeout_secs)
        || !(1..=MAX_GLOBAL_CONCURRENCY).contains(&config.global_concurrency)
        || !(1..=MAX_ACCOUNT_CONCURRENCY).contains(&config.per_account_concurrency)
        || config.per_account_concurrency > config.global_concurrency
        || !(1..=MAX_CURSOR_MAX_LIFETIME_SECS).contains(&config.cursor_max_lifetime_secs)
        || config.cursor_clock_skew_secs > MAX_CURSOR_CLOCK_SKEW_SECS
        || config.cursor_clock_skew_secs >= config.cursor_max_lifetime_secs
    {
        return Err(CoreError::Conflict(
            "Cloudflare DNS read limits are invalid".into(),
        ));
    }
    let cursor_key_ref = CredentialRef::new(
        config
            .cursor_key_ref
            .clone()
            .ok_or_else(|| CoreError::Conflict("Cloudflare DNS cursor key is required".into()))?,
    )?;
    let cursor_fallback_key_ref = config
        .cursor_fallback_key_ref
        .clone()
        .map(CredentialRef::new)
        .transpose()?;
    if cursor_fallback_key_ref.as_ref() == Some(&cursor_key_ref) {
        return Err(CoreError::Conflict(
            "Cloudflare DNS cursor key references must be distinct".into(),
        ));
    }
    let account_store = account_store.ok_or_else(|| {
        CoreError::Conflict("Cloudflare DNS read requires a provider account store".into())
    })?;
    let mounted_resolver = mounted_resolver.ok_or_else(|| {
        CoreError::Conflict("Cloudflare DNS read requires mounted credentials".into())
    })?;
    Ok(Some(Arc::new(CloudflareDnsReadService {
        account_store,
        mounted_resolver,
        cursor_key_ref,
        cursor_fallback_key_ref,
        cursor_max_lifetime: Duration::from_secs(config.cursor_max_lifetime_secs),
        cursor_clock_skew: Duration::from_secs(config.cursor_clock_skew_secs),
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

struct CloudflareDnsReadService {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    cursor_key_ref: CredentialRef,
    cursor_fallback_key_ref: Option<CredentialRef>,
    cursor_max_lifetime: Duration,
    cursor_clock_skew: Duration,
    timeout: Duration,
    global: Arc<Semaphore>,
    per_account_concurrency: usize,
    accounts: Mutex<HashMap<CloudResourceId, Weak<Semaphore>>>,
    api_factory: Arc<dyn ApiFactory>,
}

struct RequestAuthority {
    account: ProviderAccount,
    token_revision: String,
    cursor_revision: String,
    cursor_fallback_revision: Option<String>,
}

struct ResolvedMaterial {
    token: CloudflareApiToken,
    token_revision: String,
    cursor_key: Zeroizing<[u8; 32]>,
    cursor_revision: String,
    cursor_fallback_key: Option<Zeroizing<[u8; 32]>>,
    cursor_fallback_revision: Option<String>,
}

impl CloudflareDnsReadService {
    async fn run<T, F, Fut>(
        &self,
        account_id: &CloudResourceId,
        operation: F,
    ) -> Result<T, CloudflareDnsAdminError>
    where
        F: FnOnce(Arc<CloudflareDnsAdapter>) -> Fut,
        Fut: Future<Output = Result<T, NormalizedProviderError>>,
    {
        tokio::time::timeout(self.timeout, async {
            let account = self.load_account(account_id).await?;
            let account_limit = self.account_semaphore(account_id)?;
            let _account_permit = acquire(account_limit).await?;
            let _global_permit = acquire(self.global.clone()).await?;
            let (adapter, authority) = self.prepare(account).await?;
            self.recheck(&authority).await?;
            let result = operation(adapter).await.map_err(map_provider_error)?;
            self.recheck(&authority).await?;
            Ok(result)
        })
        .await
        .map_err(|_| CloudflareDnsAdminError::Unavailable)?
    }

    async fn load_account(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<ProviderAccount, CloudflareDnsAdminError> {
        let account = self
            .account_store
            .get(account_id)
            .await
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?
            .ok_or(CloudflareDnsAdminError::NotFound)?;
        validate_account(
            &account,
            &self.cursor_key_ref,
            self.cursor_fallback_key_ref.as_ref(),
        )?;
        Ok(account)
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

    async fn prepare(
        &self,
        account: ProviderAccount,
    ) -> Result<(Arc<CloudflareDnsAdapter>, RequestAuthority), CloudflareDnsAdminError> {
        let material = self.resolve_material(&account).await?;
        let api = self
            .api_factory
            .build(material.token)
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let cursor_key = CloudflareCursorKey::from_zeroizing(material.cursor_key)
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let cursor_fallback_key = material
            .cursor_fallback_key
            .map(CloudflareCursorKey::from_zeroizing)
            .transpose()
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let cursor_key_ring = CloudflareCursorKeyRing::new(
            cursor_key,
            cursor_fallback_key,
            self.cursor_max_lifetime,
            self.cursor_clock_skew,
        )
        .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let adapter = CloudflareDnsAdapter::new_with_cursor_key_ring(
            account.metadata.id.clone(),
            &account.spec,
            api,
            cursor_key_ring,
        )
        .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        Ok((
            Arc::new(adapter),
            RequestAuthority {
                account,
                token_revision: material.token_revision,
                cursor_revision: material.cursor_revision,
                cursor_fallback_revision: material.cursor_fallback_revision,
            },
        ))
    }

    async fn resolve_material(
        &self,
        account: &ProviderAccount,
    ) -> Result<ResolvedMaterial, CloudflareDnsAdminError> {
        let CredentialSource::StaticSecret { credential_ref } = &account.spec.credential_source
        else {
            return Err(CloudflareDnsAdminError::InvalidRequest);
        };
        let token = self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: &account.metadata.id,
                provider: &CloudProvider::Cloudflare,
                purpose: CredentialPurpose::CloudflareApiToken,
                credential_ref,
            })
            .await
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let cursor = self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: &account.metadata.id,
                provider: &CloudProvider::Cloudflare,
                purpose: CredentialPurpose::CloudflareDnsCursorHmac,
                credential_ref: &self.cursor_key_ref,
            })
            .await
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let cursor_fallback = match self.cursor_fallback_key_ref.as_ref() {
            Some(credential_ref) => Some(
                self.mounted_resolver
                    .resolve(ResolveCredentialRequest {
                        provider_account_id: &account.metadata.id,
                        provider: &CloudProvider::Cloudflare,
                        purpose: CredentialPurpose::CloudflareDnsCursorHmac,
                        credential_ref,
                    })
                    .await
                    .map_err(|_| CloudflareDnsAdminError::Unavailable)?,
            ),
            None => None,
        };
        if same_material(&token, &cursor)
            || cursor_fallback.as_ref().is_some_and(|fallback| {
                same_material(&token, fallback) || same_material(&cursor, fallback)
            })
        {
            return Err(CloudflareDnsAdminError::Unavailable);
        }
        let token_revision = token.revision().as_str().to_owned();
        let cursor_revision = cursor.revision().as_str().to_owned();
        let cursor_fallback_revision = cursor_fallback
            .as_ref()
            .map(|credential| credential.revision().as_str().to_owned());
        let token = token
            .with_bytes(|bytes| str::from_utf8(bytes).map(str::to_owned))
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let token =
            CloudflareApiToken::new(token).map_err(|_| CloudflareDnsAdminError::Unavailable)?;
        let cursor_key = cursor
            .with_bytes(copy_cursor_key)
            .ok_or(CloudflareDnsAdminError::Unavailable)?;
        let cursor_fallback_key = match cursor_fallback.as_ref() {
            Some(credential) => Some(
                credential
                    .with_bytes(copy_cursor_key)
                    .ok_or(CloudflareDnsAdminError::Unavailable)?,
            ),
            None => None,
        };
        Ok(ResolvedMaterial {
            token,
            token_revision,
            cursor_key,
            cursor_revision,
            cursor_fallback_key,
            cursor_fallback_revision,
        })
    }

    async fn recheck(&self, authority: &RequestAuthority) -> Result<(), CloudflareDnsAdminError> {
        let current = self
            .account_store
            .get(&authority.account.metadata.id)
            .await
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?
            .ok_or(CloudflareDnsAdminError::Unavailable)?;
        if current != authority.account {
            return Err(CloudflareDnsAdminError::Unavailable);
        }
        let material = self.resolve_material(&current).await?;
        if material.token_revision != authority.token_revision
            || material.cursor_revision != authority.cursor_revision
            || material.cursor_fallback_revision != authority.cursor_fallback_revision
        {
            return Err(CloudflareDnsAdminError::Unavailable);
        }
        let final_account = self
            .account_store
            .get(&authority.account.metadata.id)
            .await
            .map_err(|_| CloudflareDnsAdminError::Unavailable)?
            .ok_or(CloudflareDnsAdminError::Unavailable)?;
        if final_account != authority.account {
            return Err(CloudflareDnsAdminError::Unavailable);
        }
        Ok(())
    }
}

fn validate_account(
    account: &ProviderAccount,
    cursor_key_ref: &CredentialRef,
    cursor_fallback_key_ref: Option<&CredentialRef>,
) -> Result<(), CloudflareDnsAdminError> {
    if account.spec.provider != CloudProvider::Cloudflare
        || !matches!(
            account.spec.scope.as_ref(),
            Some(ProviderAccountScope::Cloudflare { .. })
        )
    {
        return Err(CloudflareDnsAdminError::InvalidRequest);
    }
    let CredentialSource::StaticSecret { credential_ref } = &account.spec.credential_source else {
        return Err(CloudflareDnsAdminError::InvalidRequest);
    };
    if credential_ref == cursor_key_ref || cursor_fallback_key_ref == Some(credential_ref) {
        return Err(CloudflareDnsAdminError::InvalidRequest);
    }
    Ok(())
}

async fn acquire(
    semaphore: Arc<Semaphore>,
) -> Result<OwnedSemaphorePermit, CloudflareDnsAdminError> {
    semaphore
        .acquire_owned()
        .await
        .map_err(|_| CloudflareDnsAdminError::Unavailable)
}

fn same_material(left: &ResolvedCredential, right: &ResolvedCredential) -> bool {
    left.with_bytes(|left| right.with_bytes(|right| left == right))
}

fn copy_cursor_key(bytes: &[u8]) -> Option<Zeroizing<[u8; 32]>> {
    if bytes.len() != 32 || bytes.iter().all(|byte| *byte == 0) {
        return None;
    }
    let mut key = Zeroizing::new([0_u8; 32]);
    key.copy_from_slice(bytes);
    Some(key)
}

fn map_provider_error(error: NormalizedProviderError) -> CloudflareDnsAdminError {
    match error.category() {
        ProviderErrorCategory::NotFound => CloudflareDnsAdminError::NotFound,
        ProviderErrorCategory::Validation
            if matches!(
                error.code(),
                "cloudflare_inventory_changed" | "cloudflare_total_pages_changed"
            ) =>
        {
            CloudflareDnsAdminError::RestartRequired
        }
        ProviderErrorCategory::Validation
            if matches!(
                error.code(),
                "invalid_cloudflare_cursor"
                    | "invalid_cloudflare_cursor_signature"
                    | "unsupported_cloudflare_cursor_version"
                    | "cloudflare_cursor_scope_mismatch"
                    | "cloudflare_cursor_page_size_mismatch"
                    | "invalid_cloudflare_cursor_offset"
                    | "invalid_cloudflare_cursor_time"
                    | "cloudflare_cursor_not_yet_valid"
                    | "cloudflare_cursor_expired"
                    | "invalid_page"
            ) =>
        {
            CloudflareDnsAdminError::InvalidRequest
        }
        ProviderErrorCategory::Validation
            if error.code() == "cloudflare_cursor_clock_unavailable" =>
        {
            CloudflareDnsAdminError::Unavailable
        }
        ProviderErrorCategory::Validation => CloudflareDnsAdminError::InvalidProviderObservation,
        ProviderErrorCategory::Authentication
        | ProviderErrorCategory::Authorization
        | ProviderErrorCategory::Quota
        | ProviderErrorCategory::Conflict
        | ProviderErrorCategory::Transient
        | ProviderErrorCategory::Throttled
        | ProviderErrorCategory::UnknownOutcome => CloudflareDnsAdminError::Unavailable,
    }
}

fn core_record_key(key: &CloudflareRecordSetKey) -> DnsRecordSetKey {
    let record_type = match key.record_type {
        edgion_center_app::api::cloudflare_dns::CloudflareRecordType::A => ProviderDnsRecordType::A,
        edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Aaaa => {
            ProviderDnsRecordType::Aaaa
        }
        edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Cname => {
            ProviderDnsRecordType::Cname
        }
        edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Txt => {
            ProviderDnsRecordType::Txt
        }
        edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Mx => {
            ProviderDnsRecordType::Mx
        }
        edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Srv => {
            ProviderDnsRecordType::Srv
        }
        edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Caa => {
            ProviderDnsRecordType::Caa
        }
        edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Ns => {
            ProviderDnsRecordType::Ns
        }
        edgion_center_app::api::cloudflare_dns::CloudflareRecordType::Soa => {
            ProviderDnsRecordType::Soa
        }
    };
    DnsRecordSetKey {
        owner: key.owner.clone(),
        record_type,
        routing: DnsRoutingIdentity::Simple,
    }
}

async fn authoritative_zone(
    adapter: &CloudflareDnsAdapter,
    account_id: &CloudResourceId,
    zone_id: &DnsZoneId,
) -> Result<ObservedCloudflareZone, NormalizedProviderError> {
    adapter
        .get_zone_by_id(account_id, zone_id)
        .await?
        .ok_or_else(|| {
            NormalizedProviderError::new(
                ProviderErrorCategory::NotFound,
                "cloudflare_zone_not_found",
                "Cloudflare DNS zone was not found",
                None,
                None,
            )
            .expect("static provider error")
        })
}

#[async_trait]
impl CloudflareDnsAdminService for CloudflareDnsReadService {
    async fn list_zones(
        &self,
        account_id: &CloudResourceId,
        page: &CloudflareZonePageRequest,
    ) -> Result<CloudflareZonePageDto, CloudflareDnsAdminError> {
        let request = DnsPageRequest {
            limit: page.limit,
            token: page.cursor.clone(),
        };
        self.run(account_id, move |adapter| async move {
            let page = adapter.list_zone_inventory(account_id, &request).await?;
            Ok(CloudflareZonePageDto {
                items: page
                    .items
                    .into_iter()
                    .map(|zone| map_zone(zone).map_err(admin_mapping_error))
                    .collect::<Result<_, _>>()?,
                next_cursor: page.next,
            })
        })
        .await
    }

    async fn get_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> Result<CloudflareZoneDto, CloudflareDnsAdminError> {
        self.run(account_id, move |adapter| async move {
            let zone = adapter
                .get_zone_by_id(account_id, zone_id)
                .await?
                .ok_or_else(|| {
                    NormalizedProviderError::new(
                        ProviderErrorCategory::NotFound,
                        "cloudflare_zone_not_found",
                        "Cloudflare DNS zone was not found",
                        None,
                        None,
                    )
                    .expect("static provider error")
                })?;
            map_zone(zone).map_err(admin_mapping_error)
        })
        .await
    }

    async fn list_record_sets(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        page: &CloudflareRecordPageRequest,
    ) -> Result<CloudflareRecordPageResult, CloudflareDnsAdminError> {
        let request = DnsPageRequest {
            limit: page.limit,
            token: page.cursor.clone(),
        };
        self.run(account_id, move |adapter| async move {
            let validated = adapter.validate_record_inventory_cursor(zone_id, &request)?;
            let observed_zone = authoritative_zone(&adapter, account_id, zone_id).await?;
            let page = adapter
                .list_record_sets_with_validated_cursor(&observed_zone.zone, &request, validated)
                .await?;
            Ok(CloudflareRecordPageResult {
                zone: map_zone(observed_zone).map_err(admin_mapping_error)?,
                page: CloudflareRecordPageDto {
                    items: page
                        .items
                        .into_iter()
                        .map(|record| map_record(record).map_err(admin_mapping_error))
                        .collect::<Result<_, _>>()?,
                    next_cursor: page.next,
                },
            })
        })
        .await
    }

    async fn get_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
    ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError> {
        let key = core_record_key(key);
        self.run(account_id, move |adapter| async move {
            let zone = authoritative_zone(&adapter, account_id, zone_id)
                .await?
                .zone;
            let record = adapter.get_record_set(&zone, &key).await?.ok_or_else(|| {
                NormalizedProviderError::new(
                    ProviderErrorCategory::NotFound,
                    "cloudflare_record_set_not_found",
                    "Cloudflare DNS record set was not found",
                    None,
                    None,
                )
                .expect("static provider error")
            })?;
            map_record(record).map_err(admin_mapping_error)
        })
        .await
    }
}

fn admin_mapping_error(error: CloudflareDnsAdminError) -> NormalizedProviderError {
    let code = match error {
        CloudflareDnsAdminError::InvalidRequest => "cloudflare_admin_mapping_request_invalid",
        CloudflareDnsAdminError::NotFound => "cloudflare_admin_mapping_not_found",
        CloudflareDnsAdminError::Conflict => "cloudflare_admin_mapping_conflict",
        CloudflareDnsAdminError::UnknownOutcome => "cloudflare_admin_mapping_unknown_outcome",
        CloudflareDnsAdminError::Unavailable => "cloudflare_admin_mapping_unavailable",
        CloudflareDnsAdminError::RestartRequired => "cloudflare_admin_mapping_restart_required",
        CloudflareDnsAdminError::InvalidProviderObservation => {
            "cloudflare_admin_mapping_observation_invalid"
        }
    };
    NormalizedProviderError::new(
        ProviderErrorCategory::Validation,
        code,
        "Cloudflare DNS observation could not be mapped",
        None,
        None,
    )
    .expect("static provider error")
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use edgion_center_adapter_cloudflare::{
        CloudflareApiResult, CloudflareBatchRequest, CloudflareBatchResult,
        CloudflareCreateZoneRequest, CloudflareDeleteZoneAck, CloudflareDnssec, CloudflarePage,
        CloudflareRecord, CloudflareZone, CloudflareZoneKind, CloudflareZoneStatus,
    };
    use edgion_center_adapter_credential_files::{
        MountedCredentialBinding, MountedCredentialConfig,
    };
    use edgion_center_core::{
        provider_account_from_desired, DeletionPolicy, DnssecDesiredState, ManagementPolicy,
        ProviderAccountCreateResult, ProviderAccountDesired, ProviderAccountPage,
        ProviderAccountPageRequest, ProviderAccountReplaceResult, ProviderAccountSpec,
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
            _account_id: &CloudResourceId,
            _desired: &ProviderAccountDesired,
        ) -> CoreResult<ProviderAccountCreateResult> {
            unreachable!()
        }

        async fn get(&self, _account_id: &CloudResourceId) -> CoreResult<Option<ProviderAccount>> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn list(
            &self,
            _page: &ProviderAccountPageRequest,
        ) -> CoreResult<ProviderAccountPage> {
            unreachable!()
        }

        async fn replace_if_generation(
            &self,
            _account_id: &CloudResourceId,
            _expected_generation: u64,
            _desired: &ProviderAccountDesired,
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
                management_policy: ManagementPolicy::ObserveOnly,
                deletion_policy: DeletionPolicy::Retain,
                spec: ProviderAccountSpec {
                    provider: CloudProvider::Cloudflare,
                    scope: Some(ProviderAccountScope::Cloudflare {
                        account_id: NATIVE_ACCOUNT.into(),
                    }),
                    credential_source: CredentialSource::StaticSecret {
                        credential_ref: CredentialRef::new("cloudflare/token").unwrap(),
                    },
                },
            },
        )
        .unwrap()
    }

    async fn mounted(
        token: &[u8],
        cursor: &[u8],
    ) -> (tempfile::TempDir, Arc<MountedCredentialResolver>) {
        mounted_with_fallback(token, cursor, None).await
    }

    async fn mounted_with_fallback(
        token: &[u8],
        cursor: &[u8],
        cursor_fallback: Option<&[u8]>,
    ) -> (tempfile::TempDir, Arc<MountedCredentialResolver>) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("revision.key"), [9_u8; 32]).unwrap();
        std::fs::write(directory.path().join("token"), token).unwrap();
        std::fs::write(directory.path().join("cursor"), cursor).unwrap();
        if let Some(cursor_fallback) = cursor_fallback {
            std::fs::write(directory.path().join("cursor-fallback"), cursor_fallback).unwrap();
        }
        let mut bindings = vec![
            MountedCredentialBinding {
                credential_ref: "cloudflare/token".into(),
                provider_account_id: CENTER_ACCOUNT.into(),
                provider: CloudProvider::Cloudflare,
                purpose: CredentialPurpose::CloudflareApiToken,
                file: "token".into(),
            },
            MountedCredentialBinding {
                credential_ref: "cloudflare/cursor".into(),
                provider_account_id: CENTER_ACCOUNT.into(),
                provider: CloudProvider::Cloudflare,
                purpose: CredentialPurpose::CloudflareDnsCursorHmac,
                file: "cursor".into(),
            },
        ];
        if cursor_fallback.is_some() {
            bindings.push(MountedCredentialBinding {
                credential_ref: "cloudflare/cursor-fallback".into(),
                provider_account_id: CENTER_ACCOUNT.into(),
                provider: CloudProvider::Cloudflare,
                purpose: CredentialPurpose::CloudflareDnsCursorHmac,
                file: "cursor-fallback".into(),
            });
        }
        let resolver = MountedCredentialResolver::from_config(&MountedCredentialConfig {
            enabled: true,
            root_directory: Some(directory.path().to_string_lossy().into_owned()),
            revision_key_file: Some("revision.key".into()),
            bindings,
        })
        .await
        .unwrap()
        .unwrap();
        (directory, Arc::new(resolver))
    }

    struct FakeApi {
        calls: Arc<AtomicUsize>,
        rotate_store: Option<Arc<Store>>,
        rotate_cursor_file: Option<PathBuf>,
        delay: Duration,
    }

    #[async_trait]
    impl CloudflareApi for FakeApi {
        async fn create_zone(
            &self,
            _: &CloudflareCreateZoneRequest,
        ) -> CloudflareApiResult<CloudflareZone> {
            unreachable!()
        }
        async fn get_zone(&self, _: &str) -> CloudflareApiResult<Option<CloudflareZone>> {
            unreachable!()
        }
        async fn delete_zone(&self, _: &str) -> CloudflareApiResult<CloudflareDeleteZoneAck> {
            unreachable!()
        }
        async fn get_dnssec(&self, _: &str) -> CloudflareApiResult<Option<CloudflareDnssec>> {
            unreachable!()
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
            account_id: &str,
            page: u32,
            _: u32,
        ) -> CloudflareApiResult<CloudflarePage<CloudflareZone>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            if let Some(store) = &self.rotate_store {
                *store.0.lock().unwrap() = Some(account(2));
            }
            if let Some(path) = &self.rotate_cursor_file {
                std::fs::write(path, [10_u8; 32]).unwrap();
            }
            Ok(CloudflarePage {
                items: vec![CloudflareZone {
                    id: ZONE_ID.into(),
                    account_id: account_id.into(),
                    name: "example.com".into(),
                    kind: CloudflareZoneKind::Full,
                    status: CloudflareZoneStatus::Active,
                    name_servers: BTreeSet::new(),
                    modified_on: Some("revision".into()),
                }],
                page,
                total_pages: 1,
            })
        }
        async fn list_records(
            &self,
            _: &str,
            _: u32,
            _: u32,
        ) -> CloudflareApiResult<CloudflarePage<CloudflareRecord>> {
            unreachable!()
        }
        async fn batch_records(
            &self,
            _: &str,
            _: &CloudflareBatchRequest,
        ) -> CloudflareApiResult<CloudflareBatchResult> {
            unreachable!()
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
    ) -> CloudflareDnsReadService {
        CloudflareDnsReadService {
            account_store: store,
            mounted_resolver: resolver,
            cursor_key_ref: CredentialRef::new("cloudflare/cursor").unwrap(),
            cursor_fallback_key_ref: None,
            cursor_max_lifetime: Duration::from_secs(DEFAULT_CURSOR_MAX_LIFETIME_SECS),
            cursor_clock_skew: Duration::from_secs(DEFAULT_CURSOR_CLOCK_SKEW_SECS),
            timeout: Duration::from_secs(5),
            global: Arc::new(Semaphore::new(2)),
            per_account_concurrency: 1,
            accounts: Mutex::new(HashMap::new()),
            api_factory: Arc::new(FakeFactory(api)),
        }
    }

    #[test]
    fn config_is_strict_bounded_and_redacted() {
        let config: CloudflareDnsReadConfig = serde_yaml::from_str(
            "enabled: true\ncursor_key_ref: cloudflare/cursor\ncursor_fallback_key_ref: cloudflare/cursor-fallback\ncursor_max_lifetime_secs: 900\ncursor_clock_skew_secs: 30\noperation_timeout_secs: 10\nglobal_concurrency: 4\nper_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(config.enabled);
        let debug = format!("{config:?}");
        assert!(!debug.contains("cloudflare/cursor"));
        assert!(!debug.contains("cloudflare/cursor-fallback"));
        assert!(serde_yaml::from_str::<CloudflareDnsReadConfig>("unknown: true\n").is_err());
        let mut invalid = config.clone();
        invalid.per_account_concurrency = 5;
        assert!(compose_dns_admin(&invalid, None, None).is_err());
        let mut invalid = config.clone();
        invalid.cursor_fallback_key_ref = invalid.cursor_key_ref.clone();
        assert!(compose_dns_admin(&invalid, None, None).is_err());
        let mut invalid = config.clone();
        invalid.cursor_max_lifetime_secs = MAX_CURSOR_MAX_LIFETIME_SECS + 1;
        assert!(compose_dns_admin(&invalid, None, None).is_err());
        let mut invalid = config;
        invalid.cursor_clock_skew_secs = invalid.cursor_max_lifetime_secs;
        assert!(compose_dns_admin(&invalid, None, None).is_err());
        assert!(
            compose_dns_admin(&CloudflareDnsReadConfig::default(), None, None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn changed_inventory_requires_a_sanitized_pagination_restart() {
        for code in [
            "cloudflare_inventory_changed",
            "cloudflare_total_pages_changed",
        ] {
            let error = NormalizedProviderError::new(
                ProviderErrorCategory::Validation,
                code,
                "provider details must not cross the Admin boundary",
                None,
                None,
            )
            .unwrap();
            assert_eq!(
                map_provider_error(error),
                CloudflareDnsAdminError::RestartRequired
            );
        }
    }

    #[test]
    fn cursor_lifetime_failures_are_sanitized_without_provider_classification() {
        for code in [
            "unsupported_cloudflare_cursor_version",
            "invalid_cloudflare_cursor_time",
            "cloudflare_cursor_not_yet_valid",
            "cloudflare_cursor_expired",
        ] {
            let error = NormalizedProviderError::new(
                ProviderErrorCategory::Validation,
                code,
                "cursor details must not cross the Admin boundary",
                None,
                None,
            )
            .unwrap();
            assert_eq!(
                map_provider_error(error),
                CloudflareDnsAdminError::InvalidRequest
            );
        }
    }

    #[tokio::test]
    async fn list_zones_uses_exact_authority_and_maps_inventory() {
        let (_directory, resolver) = mounted(b"token", &[7_u8; 32]).await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let calls = Arc::new(AtomicUsize::new(0));
        let service = service(
            store,
            resolver,
            Arc::new(FakeApi {
                calls: calls.clone(),
                rotate_store: None,
                rotate_cursor_file: None,
                delay: Duration::ZERO,
            }),
        );
        let page = service
            .list_zones(
                &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                &CloudflareZonePageRequest {
                    limit: 10,
                    cursor: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items[0].zone_id.as_str(), ZONE_ID);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn identical_token_and_cursor_key_fail_before_provider_io() {
        let same = [7_u8; 32];
        let (_directory, resolver) = mounted(&same, &same).await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let calls = Arc::new(AtomicUsize::new(0));
        let service = service(
            store,
            resolver,
            Arc::new(FakeApi {
                calls: calls.clone(),
                rotate_store: None,
                rotate_cursor_file: None,
                delay: Duration::ZERO,
            }),
        );
        assert_eq!(
            service
                .list_zones(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &CloudflareZonePageRequest {
                        limit: 10,
                        cursor: None
                    },
                )
                .await,
            Err(CloudflareDnsAdminError::Unavailable)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fallback_key_is_optional_and_must_not_reuse_material() {
        let same = [8_u8; 32];
        let (_directory, resolver) = mounted_with_fallback(b"token", &same, Some(&same)).await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut reused_material_service = service(
            store,
            resolver,
            Arc::new(FakeApi {
                calls: calls.clone(),
                rotate_store: None,
                rotate_cursor_file: None,
                delay: Duration::ZERO,
            }),
        );
        reused_material_service.cursor_fallback_key_ref =
            Some(CredentialRef::new("cloudflare/cursor-fallback").unwrap());
        assert_eq!(
            reused_material_service
                .list_zones(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &CloudflareZonePageRequest {
                        limit: 10,
                        cursor: None,
                    },
                )
                .await,
            Err(CloudflareDnsAdminError::Unavailable)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let (_directory, resolver) =
            mounted_with_fallback(b"token", &[7_u8; 32], Some(&[8_u8; 32])).await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let mut distinct_material_service = service(
            store,
            resolver,
            Arc::new(FakeApi {
                calls: calls.clone(),
                rotate_store: None,
                rotate_cursor_file: None,
                delay: Duration::ZERO,
            }),
        );
        distinct_material_service.cursor_fallback_key_ref =
            Some(CredentialRef::new("cloudflare/cursor-fallback").unwrap());
        distinct_material_service
            .list_zones(
                &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                &CloudflareZonePageRequest {
                    limit: 10,
                    cursor: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn post_call_generation_change_discards_provider_result() {
        let (_directory, resolver) = mounted(b"token", &[7_u8; 32]).await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let calls = Arc::new(AtomicUsize::new(0));
        let service = service(
            store.clone(),
            resolver,
            Arc::new(FakeApi {
                calls: calls.clone(),
                rotate_store: Some(store),
                rotate_cursor_file: None,
                delay: Duration::ZERO,
            }),
        );
        assert_eq!(
            service
                .list_zones(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &CloudflareZonePageRequest {
                        limit: 10,
                        cursor: None
                    },
                )
                .await,
            Err(CloudflareDnsAdminError::Unavailable)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn post_call_fallback_revision_change_discards_provider_result() {
        let (directory, resolver) =
            mounted_with_fallback(b"token", &[7_u8; 32], Some(&[8_u8; 32])).await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut service = service(
            store,
            resolver,
            Arc::new(FakeApi {
                calls: calls.clone(),
                rotate_store: None,
                rotate_cursor_file: Some(directory.path().join("cursor-fallback")),
                delay: Duration::ZERO,
            }),
        );
        service.cursor_fallback_key_ref =
            Some(CredentialRef::new("cloudflare/cursor-fallback").unwrap());
        assert_eq!(
            service
                .list_zones(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &CloudflareZonePageRequest {
                        limit: 10,
                        cursor: None,
                    },
                )
                .await,
            Err(CloudflareDnsAdminError::Unavailable)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn operation_deadline_bounds_provider_work() {
        let (_directory, resolver) = mounted(b"token", &[7_u8; 32]).await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut service = service(
            store,
            resolver,
            Arc::new(FakeApi {
                calls: calls.clone(),
                rotate_store: None,
                rotate_cursor_file: None,
                delay: Duration::from_millis(50),
            }),
        );
        service.timeout = Duration::from_millis(10);
        assert_eq!(
            service
                .list_zones(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &CloudflareZonePageRequest {
                        limit: 10,
                        cursor: None,
                    },
                )
                .await,
            Err(CloudflareDnsAdminError::Unavailable)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn invalid_record_cursor_fails_before_authoritative_provider_read() {
        let (_directory, resolver) = mounted(b"token", &[7_u8; 32]).await;
        let store = Arc::new(Store(Mutex::new(Some(account(1)))));
        let calls = Arc::new(AtomicUsize::new(0));
        let service = service(
            store,
            resolver,
            Arc::new(FakeApi {
                calls: calls.clone(),
                rotate_store: None,
                rotate_cursor_file: None,
                delay: Duration::ZERO,
            }),
        );
        assert_eq!(
            service
                .list_record_sets(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &CloudflareRecordPageRequest {
                        limit: 10,
                        cursor: Some(edgion_center_core::DnsPageToken::new("invalid").unwrap()),
                    },
                )
                .await,
            Err(CloudflareDnsAdminError::InvalidRequest)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
