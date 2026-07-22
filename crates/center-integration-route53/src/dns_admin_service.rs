use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_adapter_credential_files::{
    CredentialPurpose, MountedCredentialResolver, ResolveCredentialRequest,
};
use edgion_center_adapter_route53::{
    AwsRoute53Api, AwsRoute53SdkConfigFactory, Route53Api, Route53CursorKey, Route53DnsAdapter,
};
use edgion_center_app::api::route53_dns::{
    Route53DnsAdminError, Route53DnsAdminService, Route53RecordPageDto, Route53RecordPageRequest,
    Route53RecordSetDto, Route53RecordSetKey, Route53ZoneDto, Route53ZonePageDto,
    Route53ZonePageRequest, SharedRoute53DnsAdminService, MAX_ROUTE53_PAGE_LIMIT,
};
use edgion_center_core::{
    CloudProvider, CloudResourceId, CoreError, CoreResult, CredentialRef, CredentialSource,
    DnsPageRequest, DnsProvider, DnsZoneId, NormalizedProviderError, ObservedDnsZone,
    ProviderAccount, ProviderAccountScope, ProviderAccountStore, ProviderErrorCategory,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 180;
const DEFAULT_GLOBAL_CONCURRENCY: usize = 8;
const MAX_GLOBAL_CONCURRENCY: usize = 32;
const DEFAULT_ACCOUNT_CONCURRENCY: usize = 2;
const MAX_ACCOUNT_CONCURRENCY: usize = 4;
const MAX_TRACKED_ACCOUNTS: usize = 1_024;

/// Strict, independently default-off switch for Route 53 DNS inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Route53DnsReadConfig {
    pub enabled: bool,
    pub cursor_key_ref: Option<String>,
    pub operation_timeout_secs: u64,
    pub global_concurrency: usize,
    pub per_account_concurrency: usize,
}

pub fn compose_dns_admin(
    config: &Route53DnsReadConfig,
    account_store: Option<Arc<dyn ProviderAccountStore>>,
    mounted_resolver: Option<Arc<MountedCredentialResolver>>,
) -> CoreResult<Option<SharedRoute53DnsAdminService>> {
    let Some(cursor_key_ref) = validated_config(config)? else {
        return Ok(None);
    };
    let account_store = account_store.ok_or_else(|| {
        CoreError::Conflict("Route 53 DNS read requires a provider account store".into())
    })?;
    let mounted_resolver = mounted_resolver.ok_or_else(|| {
        CoreError::Conflict("Route 53 DNS read requires mounted credentials".into())
    })?;
    Ok(Some(Arc::new(Route53DnsReadService {
        account_store,
        mounted_resolver,
        cursor_key_ref,
        timeout: Duration::from_secs(config.operation_timeout_secs),
        global: Arc::new(Semaphore::new(config.global_concurrency)),
        per_account_concurrency: config.per_account_concurrency,
        accounts: Mutex::new(HashMap::new()),
        api_factory: Arc::new(ProductionApiFactory),
    })))
}

impl Default for Route53DnsReadConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cursor_key_ref: None,
            operation_timeout_secs: DEFAULT_TIMEOUT_SECS,
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            per_account_concurrency: DEFAULT_ACCOUNT_CONCURRENCY,
        }
    }
}

#[async_trait]
pub(crate) trait ApiFactory: Send + Sync {
    async fn build(
        &self,
    ) -> Result<Arc<dyn Route53Api>, edgion_center_core::NormalizedProviderError>;
}

pub(crate) struct ProductionApiFactory;

#[async_trait]
impl ApiFactory for ProductionApiFactory {
    async fn build(
        &self,
    ) -> Result<Arc<dyn Route53Api>, edgion_center_core::NormalizedProviderError> {
        let config = AwsRoute53SdkConfigFactory::ambient().await?;
        AwsRoute53Api::new(&config)
            .await
            .map(|api| Arc::new(api) as Arc<dyn Route53Api>)
    }
}

struct Route53DnsReadService {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    cursor_key_ref: CredentialRef,
    timeout: Duration,
    global: Arc<Semaphore>,
    per_account_concurrency: usize,
    accounts: Mutex<HashMap<CloudResourceId, Weak<Semaphore>>>,
    api_factory: Arc<dyn ApiFactory>,
}

struct RequestAuthority {
    account: ProviderAccount,
    cursor_revision: String,
}

impl Route53DnsReadService {
    async fn prepare(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<(Route53DnsAdapter, RequestAuthority), Route53DnsAdminError> {
        let account = self
            .account_store
            .get(account_id)
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?
            .ok_or(Route53DnsAdminError::NotFound)?;
        validate_account(account_id, &account)?;
        let (cursor_key, cursor_revision) = self.resolve_cursor_key(&account).await?;
        let api = self
            .api_factory
            .build()
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        let adapter = Route53DnsAdapter::new_read_only(
            account.metadata.id.clone(),
            &account.spec,
            api,
            cursor_key,
        )
        .map_err(|_| Route53DnsAdminError::Unavailable)?;
        Ok((
            adapter,
            RequestAuthority {
                account,
                cursor_revision,
            },
        ))
    }

    async fn resolve_cursor_key(
        &self,
        account: &ProviderAccount,
    ) -> Result<(Route53CursorKey, String), Route53DnsAdminError> {
        let resolved = self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: &account.metadata.id,
                provider: &CloudProvider::Aws,
                purpose: CredentialPurpose::Route53DnsCursorHmac,
                credential_ref: &self.cursor_key_ref,
            })
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        let revision = resolved.revision().as_str().to_owned();
        let key = resolved
            .with_bytes(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .ok_or(Route53DnsAdminError::Unavailable)?;
        let key = Route53CursorKey::new(key).map_err(|_| Route53DnsAdminError::Unavailable)?;
        Ok((key, revision))
    }

    async fn authority_is_current(&self, authority: &RequestAuthority) -> bool {
        let Ok(Some(current)) = self.account_store.get(&authority.account.metadata.id).await else {
            return false;
        };
        if current != authority.account {
            return false;
        }
        let Ok((_, revision)) = self.resolve_cursor_key(&current).await else {
            return false;
        };
        if revision != authority.cursor_revision {
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
        let account = self.account_semaphore(account_id)?;
        let account = account
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
}

#[async_trait]
impl Route53DnsAdminService for Route53DnsReadService {
    async fn list_zones(
        &self,
        account_id: &CloudResourceId,
        page: &Route53ZonePageRequest,
    ) -> Result<Route53ZonePageDto, Route53DnsAdminError> {
        validate_page(page.limit, page.cursor.as_ref())?;
        let result = tokio::time::timeout(self.timeout, async {
            let (_account_permit, _global_permit) = self.admission(account_id).await?;
            let (adapter, authority) = self.prepare(account_id).await?;
            if !self.authority_is_current(&authority).await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            let page = adapter
                .list_zones(
                    account_id,
                    &DnsPageRequest {
                        limit: page.limit,
                        token: page.cursor.clone(),
                    },
                )
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            let items = page
                .items
                .into_iter()
                .map(crate::dns_admin::map_zone)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Route53ZonePageDto {
                items,
                next_cursor: page.next,
            })
        })
        .await;
        result.unwrap_or(Err(Route53DnsAdminError::Unavailable))
    }

    async fn get_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> Result<Route53ZoneDto, Route53DnsAdminError> {
        let result = tokio::time::timeout(self.timeout, async {
            let (_account_permit, _global_permit) = self.admission(account_id).await?;
            let (adapter, authority) = self.prepare(account_id).await?;
            if !self.authority_is_current(&authority).await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            let observed = observe_exact_zone(&adapter, account_id, zone_id).await?;
            if !self.authority_is_current(&authority).await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            crate::dns_admin::map_zone(observed)
        })
        .await;
        result.unwrap_or(Err(Route53DnsAdminError::Unavailable))
    }

    async fn list_record_sets(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        page: &Route53RecordPageRequest,
    ) -> Result<Route53RecordPageDto, Route53DnsAdminError> {
        validate_page(page.limit, page.cursor.as_ref())?;
        let result = tokio::time::timeout(self.timeout, async {
            let (_account_permit, _global_permit) = self.admission(account_id).await?;
            let (adapter, authority) = self.prepare(account_id).await?;
            if !self.authority_is_current(&authority).await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            let zone = observe_exact_zone(&adapter, account_id, zone_id).await?;
            let page = adapter
                .list_record_sets(
                    &zone.zone,
                    &DnsPageRequest {
                        limit: page.limit,
                        token: page.cursor.clone(),
                    },
                )
                .await
                .map_err(map_provider_error)?;
            if !self.authority_is_current(&authority).await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            let zone = crate::dns_admin::map_zone(zone)?;
            let items = page
                .items
                .into_iter()
                .map(crate::dns_admin::map_record)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Route53RecordPageDto {
                zone,
                items,
                next_cursor: page.next,
            })
        })
        .await;
        result.unwrap_or(Err(Route53DnsAdminError::Unavailable))
    }

    async fn get_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &Route53RecordSetKey,
    ) -> Result<Route53RecordSetDto, Route53DnsAdminError> {
        let core_key = key.core();
        core_key
            .validate()
            .map_err(|_| Route53DnsAdminError::InvalidRequest)?;
        let result = tokio::time::timeout(self.timeout, async {
            let (_account_permit, _global_permit) = self.admission(account_id).await?;
            let (adapter, authority) = self.prepare(account_id).await?;
            if !self.authority_is_current(&authority).await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            let zone = observe_exact_zone(&adapter, account_id, zone_id).await?;
            let observed = adapter
                .get_record_set(&zone.zone, &core_key)
                .await
                .map_err(map_provider_error)?
                .ok_or(Route53DnsAdminError::NotFound)?;
            if !self.authority_is_current(&authority).await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            crate::dns_admin::map_record(observed)
        })
        .await;
        result.unwrap_or(Err(Route53DnsAdminError::Unavailable))
    }
}

fn validate_page(
    limit: u16,
    cursor: Option<&edgion_center_core::DnsPageToken>,
) -> Result<(), Route53DnsAdminError> {
    if limit == 0 || limit > MAX_ROUTE53_PAGE_LIMIT {
        return Err(Route53DnsAdminError::InvalidRequest);
    }
    if cursor.is_some_and(|value| value.validate().is_err()) {
        return Err(Route53DnsAdminError::InvalidRequest);
    }
    Ok(())
}

pub(crate) async fn observe_exact_zone(
    adapter: &Route53DnsAdapter,
    account_id: &CloudResourceId,
    zone_id: &DnsZoneId,
) -> Result<ObservedDnsZone, Route53DnsAdminError> {
    adapter
        .observe_zone_by_id(account_id, zone_id)
        .await
        .map_err(map_provider_error)?
        .ok_or(Route53DnsAdminError::NotFound)
}

pub(crate) fn map_provider_error(error: NormalizedProviderError) -> Route53DnsAdminError {
    if error.code() == "route53_inventory_changed" {
        return Route53DnsAdminError::RestartRequired;
    }
    match error.category() {
        ProviderErrorCategory::NotFound => Route53DnsAdminError::NotFound,
        ProviderErrorCategory::Validation
            if matches!(error.code(), "invalid_route53_page_token" | "invalid_page") =>
        {
            Route53DnsAdminError::InvalidRequest
        }
        ProviderErrorCategory::Validation => Route53DnsAdminError::InvalidProviderObservation,
        _ => Route53DnsAdminError::Unavailable,
    }
}

fn validated_config(config: &Route53DnsReadConfig) -> CoreResult<Option<CredentialRef>> {
    if !config.enabled {
        return Ok(None);
    }
    if !(1..=MAX_TIMEOUT_SECS).contains(&config.operation_timeout_secs)
        || !(1..=MAX_GLOBAL_CONCURRENCY).contains(&config.global_concurrency)
        || !(1..=MAX_ACCOUNT_CONCURRENCY).contains(&config.per_account_concurrency)
        || config.per_account_concurrency > config.global_concurrency
    {
        return Err(CoreError::Conflict(
            "Route 53 DNS read limits are invalid".into(),
        ));
    }
    let key = config
        .cursor_key_ref
        .as_ref()
        .ok_or_else(|| CoreError::Conflict("Route 53 DNS cursor key is required".into()))?;
    CredentialRef::new(key.clone()).map(Some)
}

fn validate_account(
    requested_account_id: &CloudResourceId,
    account: &ProviderAccount,
) -> Result<(), Route53DnsAdminError> {
    if &account.metadata.id != requested_account_id
        || account.spec.provider != CloudProvider::Aws
        || !matches!(
            account.spec.scope.as_ref(),
            Some(ProviderAccountScope::Aws { .. })
        )
        || account.spec.credential_source != CredentialSource::Ambient
    {
        return Err(Route53DnsAdminError::InvalidRequest);
    }
    Ok(())
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn read_config_is_default_off_strict_and_bounded() {
        assert_eq!(
            validated_config(&Route53DnsReadConfig::default()).unwrap(),
            None
        );
        assert!(serde_yaml::from_str::<Route53DnsReadConfig>("unknown: true\n").is_err());
        let config: Route53DnsReadConfig = serde_yaml::from_str(
            "enabled: true\ncursor_key_ref: aws/route53-cursor\noperation_timeout_secs: 60\nglobal_concurrency: 8\nper_account_concurrency: 2\n",
        )
        .unwrap();
        assert_eq!(
            validated_config(&config).unwrap().unwrap().as_str(),
            "aws/route53-cursor"
        );
        for invalid in [
            Route53DnsReadConfig {
                cursor_key_ref: None,
                ..config.clone()
            },
            Route53DnsReadConfig {
                operation_timeout_secs: 0,
                ..config.clone()
            },
            Route53DnsReadConfig {
                global_concurrency: 0,
                ..config.clone()
            },
            Route53DnsReadConfig {
                per_account_concurrency: MAX_ACCOUNT_CONCURRENCY + 1,
                ..config.clone()
            },
            Route53DnsReadConfig {
                global_concurrency: 1,
                per_account_concurrency: 2,
                ..config
            },
        ] {
            assert!(validated_config(&invalid).is_err());
        }
    }
}

#[cfg(test)]
#[path = "dns_admin_service_tests.rs"]
mod tests;
