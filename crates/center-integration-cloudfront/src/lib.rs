//! CloudFront-specific Admin API composition.
//!
//! The service accepts only Ambient AWS ProviderAccounts and obtains a per-account local
//! fingerprint key. It never exposes an AWS SDK client or raw DistributionConfig to the HTTP app.

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use edgion_center_adapter_aws_waf::{AwsWafApi, AwsWafScope, AwsWafSdkApi, AwsWafWebAclId};
use edgion_center_adapter_cloudfront::{
    AwsCloudFrontApi, AwsCloudFrontSdkConfigFactory, CloudFrontApi, CloudFrontCreateDistribution,
    CloudFrontDispatchTracker, CloudFrontFingerprintKey, CloudFrontHttpsOrigin,
    CloudFrontInventoryAdapter, CloudFrontOriginEndpointUpdate, CloudFrontWebAclUpdate,
};
use edgion_center_adapter_credential_files::{
    CredentialPurpose, MountedCredentialResolver, ResolveCredentialRequest,
};
use edgion_center_app::api::cloudfront::{
    CloudFrontAdminError, CloudFrontAdminService, CloudFrontCreateRequest, CloudFrontDeleteRequest,
    CloudFrontDistributionDto, CloudFrontOriginUpdateRequest, CloudFrontWebAclDetachRequest,
    CloudFrontWebAclRequest, SharedCloudFrontAdminService,
};
use edgion_center_core::{
    CloudProvider, CloudResourceId, CoreError, CoreResult, CredentialRef, CredentialSource,
    ManagementPolicy, ProviderAccount, ProviderAccountScope, ProviderAccountStore,
    ProviderErrorCategory,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 180;
const DEFAULT_GLOBAL_CONCURRENCY: usize = 4;
const MAX_GLOBAL_CONCURRENCY: usize = 16;
const DEFAULT_ACCOUNT_CONCURRENCY: usize = 1;
const MAX_ACCOUNT_CONCURRENCY: usize = 2;
const MAX_TRACKED_ACCOUNTS: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloudFrontAdminConfig {
    pub read_enabled: bool,
    pub write_enabled: bool,
    pub fingerprint_key_ref: Option<String>,
    pub operation_timeout_secs: u64,
    pub global_concurrency: usize,
    pub per_account_concurrency: usize,
}
impl Default for CloudFrontAdminConfig {
    fn default() -> Self {
        Self {
            read_enabled: false,
            write_enabled: false,
            fingerprint_key_ref: None,
            operation_timeout_secs: DEFAULT_TIMEOUT_SECS,
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            per_account_concurrency: DEFAULT_ACCOUNT_CONCURRENCY,
        }
    }
}

pub fn compose_cloudfront_admin(
    config: &CloudFrontAdminConfig,
    account_store: Option<Arc<dyn ProviderAccountStore>>,
    mounted_resolver: Option<Arc<MountedCredentialResolver>>,
) -> CoreResult<Option<SharedCloudFrontAdminService>> {
    if !config.read_enabled && !config.write_enabled {
        return Ok(None);
    }
    if !(1..=MAX_TIMEOUT_SECS).contains(&config.operation_timeout_secs)
        || !(1..=MAX_GLOBAL_CONCURRENCY).contains(&config.global_concurrency)
        || !(1..=MAX_ACCOUNT_CONCURRENCY).contains(&config.per_account_concurrency)
        || config.per_account_concurrency > config.global_concurrency
    {
        return Err(CoreError::Conflict(
            "invalid CloudFront Admin configuration".into(),
        ));
    }
    let key_ref = config.fingerprint_key_ref.clone().ok_or_else(|| {
        CoreError::Conflict("CloudFront Admin requires fingerprint_key_ref".into())
    })?;
    let key_ref = CredentialRef::new(key_ref)
        .map_err(|_| CoreError::Conflict("invalid CloudFront fingerprint_key_ref".into()))?;
    Ok(Some(Arc::new(CloudFrontService {
        account_store: account_store.ok_or_else(|| {
            CoreError::Conflict("CloudFront Admin requires a provider account store".into())
        })?,
        mounted_resolver: mounted_resolver.ok_or_else(|| {
            CoreError::Conflict("CloudFront Admin requires mounted credentials".into())
        })?,
        key_ref,
        read_enabled: config.read_enabled,
        write_enabled: config.write_enabled,
        timeout: Duration::from_secs(config.operation_timeout_secs),
        global: Arc::new(Semaphore::new(config.global_concurrency)),
        per_account_concurrency: config.per_account_concurrency,
        accounts: Mutex::new(HashMap::new()),
    })))
}

struct Authority {
    account: ProviderAccount,
    key_revision: String,
}
struct CloudFrontService {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    key_ref: CredentialRef,
    read_enabled: bool,
    write_enabled: bool,
    timeout: Duration,
    global: Arc<Semaphore>,
    per_account_concurrency: usize,
    accounts: Mutex<HashMap<CloudResourceId, Weak<Semaphore>>>,
}

impl CloudFrontService {
    async fn admission(
        &self,
        id: &CloudResourceId,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit), CloudFrontAdminError> {
        let account = {
            let mut values = self
                .accounts
                .lock()
                .map_err(|_| CloudFrontAdminError::Unavailable)?;
            if let Some(value) = values.get(id).and_then(Weak::upgrade) {
                value
            } else {
                values.retain(|_, value| value.strong_count() > 0);
                if values.len() >= MAX_TRACKED_ACCOUNTS {
                    return Err(CloudFrontAdminError::Unavailable);
                }
                let value = Arc::new(Semaphore::new(self.per_account_concurrency));
                values.insert(id.clone(), Arc::downgrade(&value));
                value
            }
        };
        let account = account
            .acquire_owned()
            .await
            .map_err(|_| CloudFrontAdminError::Unavailable)?;
        let global = self
            .global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| CloudFrontAdminError::Unavailable)?;
        Ok((account, global))
    }
    async fn prepare(
        &self,
        id: &CloudResourceId,
    ) -> Result<(AwsCloudFrontApi, Authority), CloudFrontAdminError> {
        let account = self
            .account_store
            .get(id)
            .await
            .map_err(|_| CloudFrontAdminError::Unavailable)?
            .ok_or(CloudFrontAdminError::NotFound)?;
        validate_account(id, &account)?;
        let resolved = self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: id,
                provider: &CloudProvider::Aws,
                purpose: CredentialPurpose::CloudFrontFingerprintHmac,
                credential_ref: &self.key_ref,
            })
            .await
            .map_err(|_| CloudFrontAdminError::Unavailable)?;
        let revision = resolved.revision().as_str().to_string();
        let key = resolved
            .with_bytes(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .ok_or(CloudFrontAdminError::Unavailable)?;
        let sdk = AwsCloudFrontSdkConfigFactory::ambient()
            .await
            .map_err(|_| CloudFrontAdminError::Unavailable)?;
        let api = AwsCloudFrontApi::new(
            &sdk,
            CloudFrontFingerprintKey::new(key).map_err(|_| CloudFrontAdminError::Unavailable)?,
            revision.clone(),
        )
        .await
        .map_err(|_| CloudFrontAdminError::Unavailable)?;
        let Some(ProviderAccountScope::Aws { account_id }) = account.spec.scope.as_ref() else {
            return Err(CloudFrontAdminError::InvalidRequest);
        };
        if api.verified_account_id() != account_id {
            return Err(CloudFrontAdminError::InvalidRequest);
        }
        Ok((
            api,
            Authority {
                account,
                key_revision: revision,
            },
        ))
    }
    async fn current(&self, authority: &Authority) -> bool {
        let Ok(Some(account)) = self.account_store.get(&authority.account.metadata.id).await else {
            return false;
        };
        if account != authority.account {
            return false;
        }
        let Ok(resolved) = self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: &account.metadata.id,
                provider: &CloudProvider::Aws,
                purpose: CredentialPurpose::CloudFrontFingerprintHmac,
                credential_ref: &self.key_ref,
            })
            .await
        else {
            return false;
        };
        resolved.revision().as_str() == authority.key_revision
            && matches!(self.account_store.get(&account.metadata.id).await, Ok(Some(final_account)) if final_account == account)
    }
    async fn resolve_cloudfront_web_acl(
        &self,
        authority: &Authority,
        requested_id: String,
        cloudfront: &AwsCloudFrontApi,
    ) -> Result<String, CloudFrontAdminError> {
        let id =
            AwsWafWebAclId::new(requested_id).map_err(|_| CloudFrontAdminError::InvalidRequest)?;
        let Some(ProviderAccountScope::Aws { account_id }) = authority.account.spec.scope.as_ref()
        else {
            return Err(CloudFrontAdminError::InvalidRequest);
        };
        let sdk = AwsCloudFrontSdkConfigFactory::ambient()
            .await
            .map_err(|_| CloudFrontAdminError::Unavailable)?;
        let waf = AwsWafSdkApi::new(&sdk)
            .await
            .map_err(|_| CloudFrontAdminError::Unavailable)?;
        if waf.verified_account_id() != account_id
            || waf.verified_account_id() != cloudfront.verified_account_id()
        {
            return Err(CloudFrontAdminError::InvalidRequest);
        }
        let acl = waf
            .get_web_acl(&AwsWafScope::Cloudfront, &id)
            .await
            .map_err(map_error)?
            .ok_or(CloudFrontAdminError::NotFound)?;
        if acl.id != id || acl.scope != AwsWafScope::Cloudfront {
            return Err(CloudFrontAdminError::InvalidRequest);
        }
        let expected_arn_prefix = format!(
            "arn:{}:wafv2:",
            cloudfront.verified_partition().arn_partition()
        );
        if !acl.arn.starts_with(&expected_arn_prefix) {
            return Err(CloudFrontAdminError::InvalidRequest);
        }
        Ok(acl.arn)
    }
    async fn within<T>(
        &self,
        account_id: &CloudResourceId,
        write: bool,
        operation: impl std::future::Future<Output = Result<T, CloudFrontAdminError>>,
    ) -> Result<T, CloudFrontAdminError> {
        if (write && !self.write_enabled) || (!write && !self.read_enabled) {
            return Err(CloudFrontAdminError::Unavailable);
        }
        tokio::time::timeout(self.timeout, async {
            let _admission = self.admission(account_id).await?;
            operation.await
        })
        .await
        .unwrap_or(Err(CloudFrontAdminError::Unavailable))
    }

    async fn within_write<T, F, Fut>(
        &self,
        account_id: &CloudResourceId,
        operation: F,
    ) -> Result<T, CloudFrontAdminError>
    where
        F: FnOnce(CloudFrontDispatchTracker) -> Fut,
        Fut: Future<Output = Result<T, CloudFrontAdminError>>,
    {
        if !self.write_enabled {
            return Err(CloudFrontAdminError::Unavailable);
        }
        let tracker = CloudFrontDispatchTracker::default();
        await_write_with_timeout(self.timeout, tracker.clone(), async {
            let _admission = self.admission(account_id).await?;
            operation(tracker.clone()).await
        })
        .await
    }
}

async fn await_write_with_timeout<T>(
    timeout: Duration,
    tracker: CloudFrontDispatchTracker,
    operation: impl Future<Output = Result<T, CloudFrontAdminError>>,
) -> Result<T, CloudFrontAdminError> {
    match tokio::time::timeout(timeout, operation).await {
        Ok(value) => value,
        Err(_) if tracker.was_dispatched() => Err(CloudFrontAdminError::UnknownOutcome),
        Err(_) => Err(CloudFrontAdminError::Unavailable),
    }
}

fn post_authority_error(tracker: &CloudFrontDispatchTracker) -> CloudFrontAdminError {
    if tracker.was_dispatched() {
        CloudFrontAdminError::UnknownOutcome
    } else {
        CloudFrontAdminError::Unavailable
    }
}

#[async_trait]
impl CloudFrontAdminService for CloudFrontService {
    async fn list(
        &self,
        id: &CloudResourceId,
    ) -> Result<Vec<CloudFrontDistributionDto>, CloudFrontAdminError> {
        self.within(id, false, async {
            let (api, authority) = self.prepare(id).await?;
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(i64::MAX as u128) as i64;
            let inventory = CloudFrontInventoryAdapter::new(
                id.clone(),
                authority.account.metadata.generation,
                &authority.account.spec,
                Arc::new(api),
            )
            .map_err(|_| CloudFrontAdminError::Unavailable)?
            .inventory("cloudfront-admin".to_string(), now, now + 60_000)
            .await
            .map_err(map_error)?;
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            Ok(inventory
                .entries
                .into_iter()
                .filter_map(|entry| match entry.detail {
                    edgion_center_adapter_cloudfront::CloudFrontDetailObservation::Complete(
                        value,
                    ) => Some(dto(*value)),
                    _ => None,
                })
                .collect())
        })
        .await
    }
    async fn get(
        &self,
        id: &CloudResourceId,
        distribution: &str,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError> {
        self.within(id, false, async {
            let (api, authority) = self.prepare(id).await?;
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            let value = api
                .get_distribution(distribution)
                .await
                .map_err(map_error)?
                .ok_or(CloudFrontAdminError::NotFound)?;
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            Ok(dto(value))
        })
        .await
    }
    async fn create(
        &self,
        id: &CloudResourceId,
        request: CloudFrontCreateRequest,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError> {
        self.within_write(id, |tracker| async move {
            let (api, authority) = self.prepare(id).await?;
            let api = api.with_dispatch_tracker(tracker.clone());
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            let result = api
                .create_minimal_distribution(CloudFrontCreateDistribution {
                    caller_reference: request.caller_reference,
                    origin: CloudFrontHttpsOrigin {
                        domain_name: request.origin_domain_name,
                        https_port: request.origin_https_port,
                    },
                })
                .await
                .map_err(map_error)?;
            if !self.current(&authority).await {
                return Err(post_authority_error(&tracker));
            }
            Ok(dto(result.distribution))
        })
        .await
    }
    async fn update_origin(
        &self,
        id: &CloudResourceId,
        distribution: &str,
        request: CloudFrontOriginUpdateRequest,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError> {
        self.within_write(id, |tracker| async move {
            let (api, authority) = self.prepare(id).await?;
            let api = api.with_dispatch_tracker(tracker.clone());
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            let result = api
                .update_origin_endpoint(
                    distribution,
                    CloudFrontOriginEndpointUpdate {
                        domain_name: request.origin_domain_name,
                        https_port: request.origin_https_port,
                    },
                )
                .await
                .map_err(map_error)?;
            if !self.current(&authority).await {
                return Err(post_authority_error(&tracker));
            }
            Ok(dto(result.distribution))
        })
        .await
    }
    async fn set_enabled(
        &self,
        id: &CloudResourceId,
        distribution: &str,
        enabled: bool,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError> {
        self.within_write(id, |tracker| async move {
            let (api, authority) = self.prepare(id).await?;
            let api = api.with_dispatch_tracker(tracker.clone());
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            let result = api
                .set_distribution_enabled(distribution, enabled)
                .await
                .map_err(map_error)?;
            if !self.current(&authority).await {
                return Err(post_authority_error(&tracker));
            }
            Ok(dto(result.distribution))
        })
        .await
    }
    async fn delete(
        &self,
        id: &CloudResourceId,
        distribution: &str,
        request: CloudFrontDeleteRequest,
    ) -> Result<(), CloudFrontAdminError> {
        self.within_write(id, |tracker| async move {
            let (api, authority) = self.prepare(id).await?;
            let api = api.with_dispatch_tracker(tracker.clone());
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            api.delete_disabled_distribution(
                distribution,
                &edgion_center_adapter_cloudfront::CloudFrontDeleteGuard {
                    confirmation: request.confirmation,
                },
            )
            .await
            .map_err(map_error)?;
            if !self.current(&authority).await {
                return Err(post_authority_error(&tracker));
            }
            Ok(())
        })
        .await
    }
    async fn set_web_acl(
        &self,
        id: &CloudResourceId,
        distribution: &str,
        request: CloudFrontWebAclRequest,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError> {
        self.within_write(id, |tracker| async move {
            let (api, authority) = self.prepare(id).await?;
            let api = api.with_dispatch_tracker(tracker.clone());
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            let web_acl_id = self
                .resolve_cloudfront_web_acl(&authority, request.web_acl_id, &api)
                .await?;
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            let result = api
                .set_distribution_web_acl(
                    distribution,
                    CloudFrontWebAclUpdate {
                        web_acl_id: Some(web_acl_id),
                    },
                )
                .await
                .map_err(map_error)?;
            if !self.current(&authority).await {
                return Err(post_authority_error(&tracker));
            }
            Ok(dto(result.distribution))
        })
        .await
    }
    async fn detach_web_acl(
        &self,
        id: &CloudResourceId,
        distribution: &str,
        request: CloudFrontWebAclDetachRequest,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError> {
        if request.confirmation != distribution {
            return Err(CloudFrontAdminError::InvalidRequest);
        }
        self.within_write(id, |tracker| async move {
            let (api, authority) = self.prepare(id).await?;
            let api = api.with_dispatch_tracker(tracker.clone());
            if !self.current(&authority).await {
                return Err(CloudFrontAdminError::Unavailable);
            }
            let result = api
                .set_distribution_web_acl(distribution, CloudFrontWebAclUpdate { web_acl_id: None })
                .await
                .map_err(map_error)?;
            if !self.current(&authority).await {
                return Err(post_authority_error(&tracker));
            }
            Ok(dto(result.distribution))
        })
        .await
    }
}

fn validate_account(
    id: &CloudResourceId,
    account: &ProviderAccount,
) -> Result<(), CloudFrontAdminError> {
    if account.metadata.id != *id
        || account.spec.provider != CloudProvider::Aws
        || account.metadata.management_policy != ManagementPolicy::Managed
        || !matches!(account.spec.credential_source, CredentialSource::Ambient)
        || !matches!(account.spec.scope, Some(ProviderAccountScope::Aws { .. }))
    {
        Err(CloudFrontAdminError::InvalidRequest)
    } else {
        Ok(())
    }
}
fn dto(
    value: edgion_center_adapter_cloudfront::CloudFrontDistributionDetail,
) -> CloudFrontDistributionDto {
    let summary = value.summary;
    CloudFrontDistributionDto {
        id: summary.id,
        arn: summary.arn,
        domain_name: summary.domain_name,
        status: summary.status.clone(),
        enabled: summary.enabled,
        etag: value.etag,
        deployed: summary.status == "Deployed",
        web_acl_id: value.web_acl_id,
        supported_origin: value.supported_origin.map(|origin| {
            edgion_center_app::api::cloudfront::CloudFrontHttpsOriginDto {
                domain_name: origin.domain_name,
                https_port: origin.https_port,
            }
        }),
    }
}
fn map_error(value: edgion_center_core::NormalizedProviderError) -> CloudFrontAdminError {
    edgion_center_app::common::observe::cloud_metrics::record_provider_error(
        "aws",
        value.category(),
    );
    match value.category() {
        ProviderErrorCategory::NotFound => CloudFrontAdminError::NotFound,
        ProviderErrorCategory::Conflict => CloudFrontAdminError::Conflict,
        ProviderErrorCategory::UnknownOutcome => CloudFrontAdminError::UnknownOutcome,
        ProviderErrorCategory::Validation
        | ProviderErrorCategory::Authorization
        | ProviderErrorCategory::Authentication => CloudFrontAdminError::InvalidRequest,
        _ => CloudFrontAdminError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_closed_and_needs_no_dependencies() {
        let config = CloudFrontAdminConfig::default();
        assert!(!config.read_enabled && !config.write_enabled);
        assert!(compose_cloudfront_admin(&config, None, None)
            .expect("disabled config")
            .is_none());
    }

    #[test]
    fn enabled_configuration_is_strict() {
        let config: CloudFrontAdminConfig = serde_yaml::from_str(
            "read_enabled: true\nfingerprint_key_ref: aws/cloudfront-fingerprint\noperation_timeout_secs: 60\nglobal_concurrency: 4\nper_account_concurrency: 1\n",
        )
        .expect("strict known fields");
        assert!(config.read_enabled);
        assert!(serde_yaml::from_str::<CloudFrontAdminConfig>(
            "read_enabled: true\nendpoint_url: https://example.invalid\n"
        )
        .is_err());
        let mut invalid = config;
        invalid.global_concurrency = 1;
        invalid.per_account_concurrency = 2;
        assert!(compose_cloudfront_admin(&invalid, None, None).is_err());
    }

    #[tokio::test]
    async fn write_timeout_before_dispatch_is_unavailable() {
        let tracker = CloudFrontDispatchTracker::default();
        let result = await_write_with_timeout(
            Duration::from_millis(1),
            tracker,
            std::future::pending::<Result<(), CloudFrontAdminError>>(),
        )
        .await;
        assert_eq!(result, Err(CloudFrontAdminError::Unavailable));
    }

    #[tokio::test]
    async fn write_timeout_after_dispatch_is_unknown_outcome() {
        let tracker = CloudFrontDispatchTracker::default();
        let dispatched = tracker.clone();
        let result = await_write_with_timeout(Duration::from_millis(1), tracker, async move {
            dispatched.mark_dispatched();
            std::future::pending::<Result<(), CloudFrontAdminError>>().await
        })
        .await;
        assert_eq!(result, Err(CloudFrontAdminError::UnknownOutcome));
    }

    #[test]
    fn post_authority_drift_is_unknown_only_after_dispatch() {
        let tracker = CloudFrontDispatchTracker::default();
        assert_eq!(
            post_authority_error(&tracker),
            CloudFrontAdminError::Unavailable
        );
        tracker.mark_dispatched();
        assert_eq!(
            post_authority_error(&tracker),
            CloudFrontAdminError::UnknownOutcome
        );
    }
}
