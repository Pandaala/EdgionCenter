use std::{collections::BTreeSet, fmt, net::IpAddr, str::FromStr, time::Duration};

use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_cloudfront::{
    config::{
        interceptors::BeforeDeserializationInterceptorContextMut, retry::RetryConfig,
        timeout::TimeoutConfig, ConfigBag, Intercept, RuntimeComponents,
    },
    error::ProvideErrorMetadata,
    types::{CacheBehavior, DefaultCacheBehavior, DistributionConfig, DistributionSummary, Origin},
};
use aws_smithy_types::body::SdkBody;
use edgion_center_core::{
    CertificateName, DomainName, NormalizedProviderError, ProviderErrorCategory,
};
use http_body_util::Limited;

use crate::{
    AcmCertificateKeyAlgorithm, AcmCertificateObservation, AcmCertificateStatus,
    AcmCertificateType, AwsPartition, CloudFrontApi, CloudFrontApiResult,
    CloudFrontCacheBehaviorProjection, CloudFrontCustomErrorResponseProjection,
    CloudFrontDistributionConfigProjection, CloudFrontDistributionDetail,
    CloudFrontDistributionPage, CloudFrontDistributionSummary, CloudFrontDomainConflict,
    CloudFrontDomainConflictPage, CloudFrontDomainConflictResourceType, CloudFrontFingerprintKey,
    CloudFrontGeoRestrictionProjection, CloudFrontInvalidationDetail, CloudFrontInvalidationPage,
    CloudFrontInvalidationStatus, CloudFrontInvalidationSummary, CloudFrontLoggingProjection,
    CloudFrontOriginGroupProjection, CloudFrontOriginKind, CloudFrontOriginProjection,
    CloudFrontPolicyKind, CloudFrontPolicyPage, CloudFrontPolicyScope, CloudFrontPolicySummary,
    CloudFrontTags, CloudFrontViewerCertificateProjection,
};

const READ_MAX_ATTEMPTS: u32 = 3;
const THROTTLE_RETRY_AFTER_MS: u64 = 1_000;
const OPERATION_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_POLICY_PAGE_ITEMS: u16 = 100;
const MAX_INVALIDATION_PAGE_ITEMS: u16 = 100;
const MAX_INVALIDATION_PATHS: usize = 3_000;
const MAX_DOMAIN_CONFLICT_PAGE_ITEMS: u16 = 100;

#[derive(Debug, Clone, Copy)]
struct ResponseBodyLimit {
    max_bytes: usize,
}

impl Intercept for ResponseBodyLimit {
    fn name(&self) -> &'static str {
        "CloudFrontResponseBodyLimit"
    }

    fn modify_before_deserialization(
        &self,
        context: &mut BeforeDeserializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let body = context.response_mut().take_body();
        let max_bytes = self.max_bytes;
        *context.response_mut().body_mut() = body.map_preserve_contents(move |body| {
            SdkBody::from_body_1_x(Limited::new(body, max_bytes))
        });
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsAssumeRoleSpec {
    role_arn: String,
    session_name: String,
}

impl AwsAssumeRoleSpec {
    pub fn new(
        role_arn: impl Into<String>,
        session_name: impl Into<String>,
    ) -> CloudFrontApiResult<Self> {
        let value = Self {
            role_arn: role_arn.into(),
            session_name: session_name.into(),
        };
        validate_role_arn(&value.role_arn)?;
        validate_role_session_name(&value.session_name)?;
        Ok(value)
    }

    pub fn role_arn(&self) -> &str {
        &self.role_arn
    }

    pub fn session_name(&self) -> &str {
        &self.session_name
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsCloudFrontSdkConfigFactory;

struct RedactedCredentialsProvider<P>(P);

impl<P> fmt::Debug for RedactedCredentialsProvider<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RedactedCredentialsProvider")
    }
}

impl<P> aws_credential_types::provider::ProvideCredentials for RedactedCredentialsProvider<P>
where
    P: aws_credential_types::provider::ProvideCredentials + Send + Sync,
{
    fn provide_credentials<'a>(
        &'a self,
    ) -> aws_credential_types::provider::future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        self.0.provide_credentials()
    }
}

impl AwsCloudFrontSdkConfigFactory {
    pub async fn ambient() -> CloudFrontApiResult<SdkConfig> {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        if config.endpoint_url().is_some() {
            return Err(validation("ambient_aws_endpoint_override_forbidden"));
        }
        Ok(config)
    }

    pub async fn assume_role(
        base_config: &SdkConfig,
        spec: &AwsAssumeRoleSpec,
        external_id: Option<String>,
    ) -> CloudFrontApiResult<SdkConfig> {
        validate_endpoint(base_config.endpoint_url())?;
        if external_id
            .as_deref()
            .is_some_and(|value| !is_valid_external_id(value))
        {
            return Err(validation("invalid_aws_external_id"));
        }
        let mut builder = aws_config::sts::AssumeRoleProvider::builder(spec.role_arn.clone())
            .session_name(spec.session_name.clone())
            .configure(base_config);
        if let Some(external_id) = external_id {
            builder = builder.external_id(external_id);
        }
        let provider = builder.build().await;
        let mut config = base_config
            .to_builder()
            .identity_cache(aws_config::identity::IdentityCache::lazy().build())
            .credentials_provider(aws_sdk_cloudfront::config::SharedCredentialsProvider::new(
                RedactedCredentialsProvider(provider),
            ));
        config.set_endpoint_url(None);
        Ok(config.build())
    }
}

/// Loopback-only endpoint overrides for hermetic transport tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AwsCloudFrontApiOptions {
    pub acm_endpoint_url: Option<String>,
    pub cloudfront_endpoint_url: Option<String>,
    pub sts_endpoint_url: Option<String>,
}

/// Credential-owning, read-only CloudFront SDK transport.
pub struct AwsCloudFrontApi {
    account_id: String,
    partition: AwsPartition,
    credential_revision: String,
    read_client: aws_sdk_cloudfront::Client,
    acm_read_client: aws_sdk_acm::Client,
    fingerprint_key: CloudFrontFingerprintKey,
}

impl AwsCloudFrontApi {
    pub async fn new(
        sdk_config: &SdkConfig,
        fingerprint_key: CloudFrontFingerprintKey,
        credential_revision: impl Into<String>,
    ) -> CloudFrontApiResult<Self> {
        Self::with_options(
            sdk_config,
            fingerprint_key,
            credential_revision,
            AwsCloudFrontApiOptions::default(),
        )
        .await
    }

    pub async fn with_options(
        sdk_config: &SdkConfig,
        fingerprint_key: CloudFrontFingerprintKey,
        credential_revision: impl Into<String>,
        options: AwsCloudFrontApiOptions,
    ) -> CloudFrontApiResult<Self> {
        let credential_revision = credential_revision.into();
        validate_non_secret_revision(&credential_revision)?;
        if sdk_config.endpoint_url().is_some() {
            return Err(validation("inherited_aws_endpoint_override_forbidden"));
        }
        validate_endpoint(options.acm_endpoint_url.as_deref())?;
        validate_endpoint(options.cloudfront_endpoint_url.as_deref())?;
        validate_endpoint(options.sts_endpoint_url.as_deref())?;
        let timeout = TimeoutConfig::builder()
            .operation_attempt_timeout(OPERATION_ATTEMPT_TIMEOUT)
            .operation_timeout(OPERATION_TIMEOUT)
            .build();
        let mut sts_config = aws_sdk_sts::config::Builder::from(sdk_config)
            .retry_config(
                aws_sdk_sts::config::retry::RetryConfig::standard()
                    .with_max_attempts(READ_MAX_ATTEMPTS),
            )
            .timeout_config(timeout.clone())
            .interceptor(ResponseBodyLimit {
                max_bytes: MAX_RESPONSE_BODY_BYTES,
            });
        if let Some(endpoint) = options.sts_endpoint_url {
            sts_config = sts_config.endpoint_url(endpoint);
        }
        let identity = aws_sdk_sts::Client::from_conf(sts_config.build())
            .get_caller_identity()
            .send()
            .await
            .map_err(|error| map_read_error(error.as_service_error().and_then(|e| e.code())))?;
        let account_id = identity
            .account()
            .filter(|value| is_aws_account_id(value))
            .ok_or_else(|| validation("invalid_sts_account_id"))?
            .to_string();
        let partition = partition_from_identity_arn(
            identity
                .arn()
                .ok_or_else(|| validation("missing_sts_identity_arn"))?,
            &account_id,
        )?;

        let mut read_config = aws_sdk_cloudfront::config::Builder::from(sdk_config)
            .retry_config(RetryConfig::standard().with_max_attempts(READ_MAX_ATTEMPTS))
            .timeout_config(timeout.clone())
            .interceptor(ResponseBodyLimit {
                max_bytes: MAX_RESPONSE_BODY_BYTES,
            });
        if let Some(endpoint) = options.cloudfront_endpoint_url {
            read_config = read_config.endpoint_url(endpoint);
        }
        let mut acm_config = aws_sdk_acm::config::Builder::from(sdk_config)
            .region(aws_sdk_acm::config::Region::new("us-east-1"))
            .retry_config(
                aws_sdk_acm::config::retry::RetryConfig::standard()
                    .with_max_attempts(READ_MAX_ATTEMPTS),
            )
            .timeout_config(timeout)
            .interceptor(ResponseBodyLimit {
                max_bytes: MAX_RESPONSE_BODY_BYTES,
            });
        if let Some(endpoint) = options.acm_endpoint_url {
            acm_config = acm_config.endpoint_url(endpoint);
        }
        Ok(Self {
            account_id,
            partition,
            credential_revision,
            read_client: aws_sdk_cloudfront::Client::from_conf(read_config.build()),
            acm_read_client: aws_sdk_acm::Client::from_conf(acm_config.build()),
            fingerprint_key,
        })
    }
}

#[async_trait]
impl CloudFrontApi for AwsCloudFrontApi {
    fn verified_account_id(&self) -> &str {
        &self.account_id
    }

    fn verified_partition(&self) -> AwsPartition {
        self.partition
    }

    fn credential_revision(&self) -> &str {
        &self.credential_revision
    }

    async fn list_distributions(
        &self,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontDistributionPage> {
        if max_items == 0 {
            return Err(validation("invalid_cloudfront_page_size"));
        }
        let output = self
            .read_client
            .list_distributions()
            .set_marker(marker.map(ToString::to_string))
            .max_items(i32::from(max_items))
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        let list = output
            .distribution_list()
            .ok_or_else(|| validation("missing_cloudfront_distribution_list"))?;
        validate_quantity(list.quantity(), list.items().len(), "distribution_list")?;
        if list.marker() != marker.unwrap_or_default()
            || list.max_items() <= 0
            || list.items().len() > usize::from(max_items)
        {
            return Err(validation("cloudfront_distribution_page_scope_mismatch"));
        }
        let next_marker = list.next_marker().map(ToString::to_string);
        if list.is_truncated()
            && next_marker
                .as_deref()
                .is_none_or(|next| next.is_empty() || marker == Some(next))
        {
            return Err(validation("invalid_cloudfront_next_marker"));
        }
        let items = list
            .items()
            .iter()
            .map(|summary| map_summary(summary, self.partition, &self.account_id))
            .collect::<CloudFrontApiResult<Vec<_>>>()?;
        Ok(CloudFrontDistributionPage {
            items,
            is_truncated: list.is_truncated(),
            next_marker,
        })
    }

    async fn get_distribution(
        &self,
        distribution_id: &str,
    ) -> CloudFrontApiResult<Option<CloudFrontDistributionDetail>> {
        validate_distribution_id(distribution_id)?;
        let output = match self
            .read_client
            .get_distribution()
            .id(distribution_id)
            .send()
            .await
        {
            Ok(output) => output,
            Err(error) if service_code(&error) == Some("NoSuchDistribution") => return Ok(None),
            Err(error) => return Err(map_read_error(service_code(&error))),
        };
        let distribution = output
            .distribution()
            .ok_or_else(|| validation("missing_cloudfront_distribution"))?;
        if distribution.id() != distribution_id {
            return Err(validation("cloudfront_distribution_id_mismatch"));
        }
        let detail_etag = require_etag(output.e_tag())?;

        // The config endpoint is the authoritative whole-replacement snapshot. Requiring the
        // same ETag prevents combining metadata and configuration from different revisions.
        let config_output = match self
            .read_client
            .get_distribution_config()
            .id(distribution_id)
            .send()
            .await
        {
            Ok(output) => output,
            Err(error) if service_code(&error) == Some("NoSuchDistribution") => return Ok(None),
            Err(error) => return Err(map_read_error(service_code(&error))),
        };
        let config_etag = require_etag(config_output.e_tag())?;
        if detail_etag != config_etag {
            return Err(provider_error(
                ProviderErrorCategory::Conflict,
                "cloudfront_observation_revision_changed",
                None,
            ));
        }
        let config = config_output
            .distribution_config()
            .ok_or_else(|| validation("missing_cloudfront_distribution_config"))?;
        let summary = map_distribution_summary(
            distribution,
            config.enabled(),
            self.partition,
            &self.account_id,
        )?;
        Ok(Some(CloudFrontDistributionDetail {
            summary,
            etag: detail_etag.to_string(),
            etag_revision_mac: self.fingerprint_key.mac_etag_revision(
                self.partition,
                &self.account_id,
                distribution_id,
                detail_etag,
            )?,
            config: map_config(config)?,
        }))
    }

    async fn describe_acm_certificate(
        &self,
        certificate_arn: &str,
    ) -> CloudFrontApiResult<Option<AcmCertificateObservation>> {
        validate_acm_certificate_arn(certificate_arn, self.partition, &self.account_id)?;
        let output = match self
            .acm_read_client
            .describe_certificate()
            .certificate_arn(certificate_arn)
            .send()
            .await
        {
            Ok(output) => output,
            Err(error) if acm_service_code(&error) == Some("ResourceNotFoundException") => {
                return Ok(None);
            }
            Err(error) => return Err(map_acm_read_error(acm_service_code(&error))),
        };
        let detail = output
            .certificate()
            .ok_or_else(|| validation("missing_acm_certificate"))?;
        map_acm_certificate(detail, certificate_arn, self.partition, &self.account_id).map(Some)
    }

    async fn list_domain_conflicts(
        &self,
        domain: &str,
        validation_distribution_id: &str,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontDomainConflictPage> {
        validate_exact_domain(domain)?;
        validate_distribution_id(validation_distribution_id)?;
        if max_items == 0 || max_items > MAX_DOMAIN_CONFLICT_PAGE_ITEMS {
            return Err(validation("invalid_cloudfront_domain_conflict_page_size"));
        }
        if let Some(marker) = marker {
            validate_domain_conflict_text(
                marker,
                1_024,
                "invalid_cloudfront_domain_conflict_marker",
            )?;
        }
        let validation_resource = aws_sdk_cloudfront::types::DistributionResourceId::builder()
            .distribution_id(validation_distribution_id)
            .build();
        let output = self
            .read_client
            .list_domain_conflicts()
            .domain(domain)
            .domain_control_validation_resource(validation_resource)
            .set_marker(marker.map(ToString::to_string))
            .max_items(i32::from(max_items))
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        if output.domain_conflicts().len() > usize::from(max_items) {
            return Err(validation("cloudfront_domain_conflict_page_scope_mismatch"));
        }
        let next_marker = output.next_marker().map(ToString::to_string);
        if next_marker
            .as_deref()
            .is_some_and(|next| next.is_empty() || marker == Some(next))
        {
            return Err(validation("invalid_cloudfront_domain_conflict_next_marker"));
        }
        let items = output
            .domain_conflicts()
            .iter()
            .map(|item| map_domain_conflict(item, domain))
            .collect::<CloudFrontApiResult<Vec<_>>>()?;
        if items.iter().cloned().collect::<BTreeSet<_>>().len() != items.len() {
            return Err(validation("duplicate_cloudfront_domain_conflict"));
        }
        Ok(CloudFrontDomainConflictPage {
            queried_domain: domain.to_string(),
            validation_distribution_id: validation_distribution_id.to_string(),
            items,
            next_marker,
        })
    }

    async fn list_policies(
        &self,
        kind: CloudFrontPolicyKind,
        scope: CloudFrontPolicyScope,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontPolicyPage> {
        if max_items == 0 || max_items > MAX_POLICY_PAGE_ITEMS {
            return Err(validation("invalid_cloudfront_policy_page_size"));
        }
        if let Some(marker) = marker {
            validate_policy_marker(marker)?;
        }
        match kind {
            CloudFrontPolicyKind::Cache => self.list_cache_policies(scope, marker, max_items).await,
            CloudFrontPolicyKind::OriginRequest => {
                self.list_origin_request_policies(scope, marker, max_items)
                    .await
            }
            CloudFrontPolicyKind::ResponseHeaders => {
                self.list_response_headers_policies(scope, marker, max_items)
                    .await
            }
        }
    }

    async fn list_invalidations(
        &self,
        distribution_id: &str,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontInvalidationPage> {
        validate_distribution_id(distribution_id)?;
        if max_items == 0 || max_items > MAX_INVALIDATION_PAGE_ITEMS {
            return Err(validation("invalid_cloudfront_invalidation_page_size"));
        }
        if let Some(marker) = marker {
            validate_invalidation_text(marker, 1024, "invalid_cloudfront_invalidation_marker")?;
        }
        let output = self
            .read_client
            .list_invalidations()
            .distribution_id(distribution_id)
            .set_marker(marker.map(ToString::to_string))
            .max_items(i32::from(max_items))
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        let list = output
            .invalidation_list()
            .ok_or_else(|| validation("missing_cloudfront_invalidation_list"))?;
        validate_invalidation_page(list, marker, max_items)?;
        let mut seen_ids = BTreeSet::new();
        let items = list
            .items()
            .iter()
            .map(|summary| {
                if !seen_ids.insert(summary.id()) {
                    return Err(validation("duplicate_cloudfront_invalidation_id"));
                }
                map_invalidation_summary(
                    distribution_id,
                    summary.id(),
                    summary.status(),
                    summary.create_time().secs(),
                )
            })
            .collect::<CloudFrontApiResult<Vec<_>>>()?;
        Ok(CloudFrontInvalidationPage {
            distribution_id: distribution_id.to_string(),
            items,
            is_truncated: list.is_truncated(),
            next_marker: list.next_marker().map(ToString::to_string),
        })
    }

    async fn get_invalidation(
        &self,
        distribution_id: &str,
        invalidation_id: &str,
    ) -> CloudFrontApiResult<Option<CloudFrontInvalidationDetail>> {
        validate_distribution_id(distribution_id)?;
        validate_invalidation_text(invalidation_id, 1024, "invalid_cloudfront_invalidation_id")?;
        let output = match self
            .read_client
            .get_invalidation()
            .distribution_id(distribution_id)
            .id(invalidation_id)
            .send()
            .await
        {
            Ok(output) => output,
            Err(error) if service_code(&error) == Some("NoSuchInvalidation") => return Ok(None),
            Err(error) => return Err(map_read_error(service_code(&error))),
        };
        let invalidation = output
            .invalidation()
            .ok_or_else(|| validation("missing_cloudfront_invalidation"))?;
        if invalidation.id() != invalidation_id {
            return Err(validation("cloudfront_invalidation_id_mismatch"));
        }
        let summary = map_invalidation_summary(
            distribution_id,
            invalidation.id(),
            invalidation.status(),
            invalidation.create_time().secs(),
        )?;
        let batch = invalidation
            .invalidation_batch()
            .ok_or_else(|| validation("missing_cloudfront_invalidation_batch"))?;
        validate_invalidation_text(
            batch.caller_reference(),
            4096,
            "invalid_cloudfront_invalidation_caller_reference",
        )?;
        let paths = batch
            .paths()
            .ok_or_else(|| validation("missing_cloudfront_invalidation_paths"))?;
        validate_quantity(paths.quantity(), paths.items().len(), "invalidation_paths")?;
        if paths.items().is_empty() || paths.items().len() > MAX_INVALIDATION_PATHS {
            return Err(validation("invalid_cloudfront_invalidation_path_count"));
        }
        for path in paths.items() {
            validate_provider_invalidation_item(path)?;
        }
        Ok(Some(CloudFrontInvalidationDetail {
            distribution_id: summary.distribution_id,
            id: summary.id,
            status: summary.status,
            created_at_unix_seconds: summary.created_at_unix_seconds,
            caller_reference: batch.caller_reference().to_string(),
            paths: paths.items().to_vec(),
        }))
    }

    async fn list_tags_for_resource(&self, arn: &str) -> CloudFrontApiResult<CloudFrontTags> {
        validate_distribution_arn(arn, self.partition, &self.account_id, None)?;
        let output = self
            .read_client
            .list_tags_for_resource()
            .resource(arn)
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        let mut keys = BTreeSet::new();
        let mut center_resource_id = None;
        for tag in output.tags().map(|tags| tags.items()).unwrap_or_default() {
            if !keys.insert(tag.key().to_string()) {
                return Err(validation("duplicate_cloudfront_tag_key"));
            }
            if tag.key() == "edgion.center/resource-id" {
                center_resource_id = Some(tag.value().unwrap_or_default().to_string());
            }
        }
        Ok(CloudFrontTags {
            keys,
            center_resource_id,
        })
    }
}

fn validate_invalidation_page(
    list: &aws_sdk_cloudfront::types::InvalidationList,
    requested_marker: Option<&str>,
    requested_max_items: u16,
) -> CloudFrontApiResult<()> {
    validate_quantity(list.quantity(), list.items().len(), "invalidation_list")?;
    if list.marker() != requested_marker.unwrap_or_default()
        || list.max_items() != i32::from(requested_max_items)
        || list.items().len() > usize::from(requested_max_items)
    {
        return Err(validation("cloudfront_invalidation_page_scope_mismatch"));
    }
    match (list.is_truncated(), list.next_marker()) {
        (true, Some(next)) => {
            validate_invalidation_text(next, 1024, "invalid_cloudfront_invalidation_next_marker")?;
            if requested_marker == Some(next)
                || list.items().last().is_none_or(|item| item.id() != next)
            {
                return Err(validation("invalid_cloudfront_invalidation_next_marker"));
            }
        }
        (false, None) => {}
        _ => return Err(validation("invalid_cloudfront_invalidation_next_marker")),
    }
    Ok(())
}

fn map_invalidation_summary(
    distribution_id: &str,
    invalidation_id: &str,
    status: &str,
    created_at_unix_seconds: i64,
) -> CloudFrontApiResult<CloudFrontInvalidationSummary> {
    validate_distribution_id(distribution_id)?;
    validate_invalidation_text(invalidation_id, 1024, "invalid_cloudfront_invalidation_id")?;
    if created_at_unix_seconds <= 0 {
        return Err(validation("invalid_cloudfront_invalidation_create_time"));
    }
    let status = match status {
        "InProgress" => CloudFrontInvalidationStatus::InProgress,
        "Completed" => CloudFrontInvalidationStatus::Completed,
        _ => return Err(validation("unknown_cloudfront_invalidation_status")),
    };
    Ok(CloudFrontInvalidationSummary {
        distribution_id: distribution_id.to_string(),
        id: invalidation_id.to_string(),
        status,
        created_at_unix_seconds,
    })
}

fn validate_invalidation_text(
    value: &str,
    max_len: usize,
    code: &'static str,
) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(validation(code))
    } else {
        Ok(())
    }
}

fn validate_provider_invalidation_item(value: &str) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > 8192
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(validation("invalid_cloudfront_invalidation_item"))
    } else {
        Ok(())
    }
}

impl AwsCloudFrontApi {
    async fn list_cache_policies(
        &self,
        scope: CloudFrontPolicyScope,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontPolicyPage> {
        let output = self
            .read_client
            .list_cache_policies()
            .r#type(cache_policy_type(scope))
            .set_marker(marker.map(ToString::to_string))
            .max_items(i32::from(max_items))
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        let list = output
            .cache_policy_list()
            .ok_or_else(|| validation("missing_cloudfront_cache_policy_list"))?;
        validate_policy_page(
            list.quantity(),
            list.items().len(),
            list.max_items(),
            max_items,
            marker,
            list.next_marker(),
        )?;
        let mut items = Vec::with_capacity(list.items().len());
        let mut seen_ids = BTreeSet::new();
        for summary in list.items() {
            if policy_scope_from_cache_type(summary.r#type())? != scope {
                return Err(validation("cloudfront_policy_scope_mismatch"));
            }
            let listed = summary
                .cache_policy()
                .ok_or_else(|| validation("missing_cloudfront_cache_policy"))?;
            validate_policy_id(listed.id())?;
            if !seen_ids.insert(listed.id()) {
                return Err(validation("duplicate_cloudfront_policy_id"));
            }
            let output = self
                .read_client
                .get_cache_policy()
                .id(listed.id())
                .send()
                .await
                .map_err(|error| map_policy_observation_error(service_code(&error)))?;
            let exact = output
                .cache_policy()
                .ok_or_else(|| validation("missing_cloudfront_cache_policy"))?;
            if exact != listed {
                return Err(policy_observation_changed());
            }
            items.push(map_policy_summary(
                exact.id(),
                exact
                    .cache_policy_config()
                    .ok_or_else(|| validation("missing_cloudfront_cache_policy_config"))?
                    .name(),
                CloudFrontPolicyKind::Cache,
                scope,
                require_etag(output.e_tag())?,
                exact.last_modified_time().secs(),
            )?);
        }
        Ok(CloudFrontPolicyPage {
            items,
            next_marker: list.next_marker().map(ToString::to_string),
        })
    }

    async fn list_origin_request_policies(
        &self,
        scope: CloudFrontPolicyScope,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontPolicyPage> {
        let output = self
            .read_client
            .list_origin_request_policies()
            .r#type(origin_request_policy_type(scope))
            .set_marker(marker.map(ToString::to_string))
            .max_items(i32::from(max_items))
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        let list = output
            .origin_request_policy_list()
            .ok_or_else(|| validation("missing_cloudfront_origin_request_policy_list"))?;
        validate_policy_page(
            list.quantity(),
            list.items().len(),
            list.max_items(),
            max_items,
            marker,
            list.next_marker(),
        )?;
        let mut items = Vec::with_capacity(list.items().len());
        let mut seen_ids = BTreeSet::new();
        for summary in list.items() {
            if policy_scope_from_origin_request_type(summary.r#type())? != scope {
                return Err(validation("cloudfront_policy_scope_mismatch"));
            }
            let listed = summary
                .origin_request_policy()
                .ok_or_else(|| validation("missing_cloudfront_origin_request_policy"))?;
            validate_policy_id(listed.id())?;
            if !seen_ids.insert(listed.id()) {
                return Err(validation("duplicate_cloudfront_policy_id"));
            }
            let output = self
                .read_client
                .get_origin_request_policy()
                .id(listed.id())
                .send()
                .await
                .map_err(|error| map_policy_observation_error(service_code(&error)))?;
            let exact = output
                .origin_request_policy()
                .ok_or_else(|| validation("missing_cloudfront_origin_request_policy"))?;
            if exact != listed {
                return Err(policy_observation_changed());
            }
            items.push(map_policy_summary(
                exact.id(),
                exact
                    .origin_request_policy_config()
                    .ok_or_else(|| validation("missing_cloudfront_origin_request_policy_config"))?
                    .name(),
                CloudFrontPolicyKind::OriginRequest,
                scope,
                require_etag(output.e_tag())?,
                exact.last_modified_time().secs(),
            )?);
        }
        Ok(CloudFrontPolicyPage {
            items,
            next_marker: list.next_marker().map(ToString::to_string),
        })
    }

    async fn list_response_headers_policies(
        &self,
        scope: CloudFrontPolicyScope,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontPolicyPage> {
        let output = self
            .read_client
            .list_response_headers_policies()
            .r#type(response_headers_policy_type(scope))
            .set_marker(marker.map(ToString::to_string))
            .max_items(i32::from(max_items))
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        let list = output
            .response_headers_policy_list()
            .ok_or_else(|| validation("missing_cloudfront_response_headers_policy_list"))?;
        validate_policy_page(
            list.quantity(),
            list.items().len(),
            list.max_items(),
            max_items,
            marker,
            list.next_marker(),
        )?;
        let mut items = Vec::with_capacity(list.items().len());
        let mut seen_ids = BTreeSet::new();
        for summary in list.items() {
            if policy_scope_from_response_headers_type(summary.r#type())? != scope {
                return Err(validation("cloudfront_policy_scope_mismatch"));
            }
            let listed = summary
                .response_headers_policy()
                .ok_or_else(|| validation("missing_cloudfront_response_headers_policy"))?;
            validate_policy_id(listed.id())?;
            if !seen_ids.insert(listed.id()) {
                return Err(validation("duplicate_cloudfront_policy_id"));
            }
            let output = self
                .read_client
                .get_response_headers_policy()
                .id(listed.id())
                .send()
                .await
                .map_err(|error| map_policy_observation_error(service_code(&error)))?;
            let exact = output
                .response_headers_policy()
                .ok_or_else(|| validation("missing_cloudfront_response_headers_policy"))?;
            if exact != listed {
                return Err(policy_observation_changed());
            }
            items.push(map_policy_summary(
                exact.id(),
                exact
                    .response_headers_policy_config()
                    .ok_or_else(|| validation("missing_cloudfront_response_headers_policy_config"))?
                    .name(),
                CloudFrontPolicyKind::ResponseHeaders,
                scope,
                require_etag(output.e_tag())?,
                exact.last_modified_time().secs(),
            )?);
        }
        Ok(CloudFrontPolicyPage {
            items,
            next_marker: list.next_marker().map(ToString::to_string),
        })
    }
}

fn cache_policy_type(scope: CloudFrontPolicyScope) -> aws_sdk_cloudfront::types::CachePolicyType {
    match scope {
        CloudFrontPolicyScope::AwsManaged => aws_sdk_cloudfront::types::CachePolicyType::Managed,
        CloudFrontPolicyScope::AccountCustom => aws_sdk_cloudfront::types::CachePolicyType::Custom,
    }
}

fn origin_request_policy_type(
    scope: CloudFrontPolicyScope,
) -> aws_sdk_cloudfront::types::OriginRequestPolicyType {
    match scope {
        CloudFrontPolicyScope::AwsManaged => {
            aws_sdk_cloudfront::types::OriginRequestPolicyType::Managed
        }
        CloudFrontPolicyScope::AccountCustom => {
            aws_sdk_cloudfront::types::OriginRequestPolicyType::Custom
        }
    }
}

fn response_headers_policy_type(
    scope: CloudFrontPolicyScope,
) -> aws_sdk_cloudfront::types::ResponseHeadersPolicyType {
    match scope {
        CloudFrontPolicyScope::AwsManaged => {
            aws_sdk_cloudfront::types::ResponseHeadersPolicyType::Managed
        }
        CloudFrontPolicyScope::AccountCustom => {
            aws_sdk_cloudfront::types::ResponseHeadersPolicyType::Custom
        }
    }
}

fn policy_scope_from_cache_type(
    value: &aws_sdk_cloudfront::types::CachePolicyType,
) -> CloudFrontApiResult<CloudFrontPolicyScope> {
    match value {
        aws_sdk_cloudfront::types::CachePolicyType::Managed => {
            Ok(CloudFrontPolicyScope::AwsManaged)
        }
        aws_sdk_cloudfront::types::CachePolicyType::Custom => {
            Ok(CloudFrontPolicyScope::AccountCustom)
        }
        _ => Err(validation("unknown_cloudfront_policy_scope")),
    }
}

fn policy_scope_from_origin_request_type(
    value: &aws_sdk_cloudfront::types::OriginRequestPolicyType,
) -> CloudFrontApiResult<CloudFrontPolicyScope> {
    match value {
        aws_sdk_cloudfront::types::OriginRequestPolicyType::Managed => {
            Ok(CloudFrontPolicyScope::AwsManaged)
        }
        aws_sdk_cloudfront::types::OriginRequestPolicyType::Custom => {
            Ok(CloudFrontPolicyScope::AccountCustom)
        }
        _ => Err(validation("unknown_cloudfront_policy_scope")),
    }
}

fn policy_scope_from_response_headers_type(
    value: &aws_sdk_cloudfront::types::ResponseHeadersPolicyType,
) -> CloudFrontApiResult<CloudFrontPolicyScope> {
    match value {
        aws_sdk_cloudfront::types::ResponseHeadersPolicyType::Managed => {
            Ok(CloudFrontPolicyScope::AwsManaged)
        }
        aws_sdk_cloudfront::types::ResponseHeadersPolicyType::Custom => {
            Ok(CloudFrontPolicyScope::AccountCustom)
        }
        _ => Err(validation("unknown_cloudfront_policy_scope")),
    }
}

fn validate_policy_page(
    quantity: i32,
    item_count: usize,
    provider_max_items: i32,
    requested_max_items: u16,
    marker: Option<&str>,
    next_marker: Option<&str>,
) -> CloudFrontApiResult<()> {
    validate_quantity(quantity, item_count, "policy_list")?;
    if provider_max_items != i32::from(requested_max_items)
        || item_count > usize::try_from(provider_max_items).unwrap_or_default()
        || item_count > usize::from(requested_max_items)
    {
        return Err(validation("cloudfront_policy_page_scope_mismatch"));
    }
    if next_marker.is_some_and(|next| validate_policy_marker(next).is_err() || marker == Some(next))
    {
        return Err(validation("invalid_cloudfront_policy_next_marker"));
    }
    Ok(())
}

fn map_policy_summary(
    id: &str,
    name: &str,
    kind: CloudFrontPolicyKind,
    scope: CloudFrontPolicyScope,
    etag: &str,
    last_modified_unix_seconds: i64,
) -> CloudFrontApiResult<CloudFrontPolicySummary> {
    validate_policy_text(id, 128, "invalid_cloudfront_policy_id")?;
    validate_policy_text(name, 128, "invalid_cloudfront_policy_name")?;
    validate_policy_text(etag, 256, "invalid_cloudfront_policy_etag")?;
    if last_modified_unix_seconds <= 0 {
        return Err(validation("invalid_cloudfront_policy_last_modified"));
    }
    Ok(CloudFrontPolicySummary {
        id: id.to_string(),
        name: name.to_string(),
        kind,
        scope,
        etag: etag.to_string(),
        last_modified_unix_seconds,
    })
}

fn validate_policy_text(
    value: &str,
    max_len: usize,
    code: &'static str,
) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(|character| character.is_control())
    {
        return Err(validation(code));
    }
    Ok(())
}

fn validate_policy_id(value: &str) -> CloudFrontApiResult<()> {
    validate_policy_text(value, 128, "invalid_cloudfront_policy_id")?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(validation("invalid_cloudfront_policy_id"));
    }
    Ok(())
}

fn validate_policy_marker(value: &str) -> CloudFrontApiResult<()> {
    validate_policy_text(value, 1024, "invalid_cloudfront_policy_marker")
}

fn validate_exact_domain(value: &str) -> CloudFrontApiResult<()> {
    let domain = DomainName::new(value.to_string())
        .map_err(|_| validation("invalid_cloudfront_domain_conflict_query"))?;
    if domain.as_str() != value || !value.is_ascii() {
        return Err(validation("invalid_cloudfront_domain_conflict_query"));
    }
    Ok(())
}

fn validate_domain_conflict_text(
    value: &str,
    max_len: usize,
    code: &'static str,
) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(validation(code));
    }
    Ok(())
}

fn map_domain_conflict(
    item: &aws_sdk_cloudfront::types::DomainConflict,
    queried_domain: &str,
) -> CloudFrontApiResult<CloudFrontDomainConflict> {
    let query = DomainName::new(queried_domain.to_string())
        .map_err(|_| validation("invalid_cloudfront_domain_conflict_query"))?;
    let conflict_name = CertificateName::new(item.domain().to_string())
        .map_err(|_| validation("invalid_cloudfront_domain_conflict_domain"))?;
    let overlaps = if conflict_name.wildcard {
        query
            .as_str()
            .strip_suffix(&format!(".{}", conflict_name.domain.as_str()))
            .is_some_and(|prefix| !prefix.is_empty() && !prefix.contains('.'))
    } else {
        conflict_name.domain == query
    };
    if !overlaps {
        return Err(validation("unrelated_cloudfront_domain_conflict"));
    }
    validate_domain_conflict_text(
        item.resource_id(),
        512,
        "invalid_cloudfront_domain_conflict_resource_id",
    )?;
    validate_domain_conflict_text(
        item.account_id(),
        128,
        "invalid_cloudfront_domain_conflict_account_id",
    )?;
    let resource_type = match item.resource_type().as_str() {
        "distribution" => CloudFrontDomainConflictResourceType::Distribution,
        "distribution-tenant" => CloudFrontDomainConflictResourceType::DistributionTenant,
        other => CloudFrontDomainConflictResourceType::Unknown(other.to_string()),
    };
    Ok(CloudFrontDomainConflict {
        domain: item.domain().to_string(),
        resource_type,
        resource_id: item.resource_id().to_string(),
        account_id: item.account_id().to_string(),
    })
}

fn map_policy_observation_error(code: Option<&str>) -> NormalizedProviderError {
    if matches!(
        code,
        Some("NoSuchCachePolicy" | "NoSuchOriginRequestPolicy" | "NoSuchResponseHeadersPolicy")
    ) {
        policy_observation_changed()
    } else {
        map_read_error(code)
    }
}

fn policy_observation_changed() -> NormalizedProviderError {
    provider_error(
        ProviderErrorCategory::Conflict,
        "cloudfront_policy_observation_changed",
        None,
    )
}

fn map_summary(
    value: &DistributionSummary,
    partition: AwsPartition,
    account_id: &str,
) -> CloudFrontApiResult<CloudFrontDistributionSummary> {
    validate_distribution_arn(value.arn(), partition, account_id, Some(value.id()))?;
    non_empty(value.id(), "missing_cloudfront_distribution_id")?;
    non_empty(value.domain_name(), "missing_cloudfront_domain_name")?;
    non_empty(value.status(), "missing_cloudfront_status")?;
    Ok(CloudFrontDistributionSummary {
        id: value.id().to_string(),
        arn: value.arn().to_string(),
        domain_name: value.domain_name().to_string(),
        status: value.status().to_string(),
        enabled: value.enabled(),
        last_modified_unix_seconds: value.last_modified_time().secs(),
    })
}

fn map_distribution_summary(
    value: &aws_sdk_cloudfront::types::Distribution,
    enabled: bool,
    partition: AwsPartition,
    account_id: &str,
) -> CloudFrontApiResult<CloudFrontDistributionSummary> {
    validate_distribution_arn(value.arn(), partition, account_id, Some(value.id()))?;
    non_empty(value.domain_name(), "missing_cloudfront_domain_name")?;
    non_empty(value.status(), "missing_cloudfront_status")?;
    Ok(CloudFrontDistributionSummary {
        id: value.id().to_string(),
        arn: value.arn().to_string(),
        domain_name: value.domain_name().to_string(),
        status: value.status().to_string(),
        enabled,
        last_modified_unix_seconds: value.last_modified_time().secs(),
    })
}

#[allow(deprecated)] // Deprecated fields must still be observed to fail closed for whole-config replacement.
fn map_config(
    config: &DistributionConfig,
) -> CloudFrontApiResult<CloudFrontDistributionConfigProjection> {
    let origins = config
        .origins()
        .ok_or_else(|| validation("missing_cloudfront_origins"))?;
    validate_quantity(origins.quantity(), origins.items().len(), "origins")?;
    if let Some(aliases) = config.aliases() {
        validate_quantity(aliases.quantity(), aliases.items().len(), "aliases")?;
    }
    let origin_groups = config.origin_groups();
    if let Some(groups) = origin_groups {
        validate_quantity(groups.quantity(), groups.items().len(), "origin_groups")?;
    }
    let cache_behaviors = config.cache_behaviors();
    if let Some(behaviors) = cache_behaviors {
        validate_quantity(
            behaviors.quantity(),
            behaviors.items().len(),
            "cache_behaviors",
        )?;
    }
    let errors = config.custom_error_responses();
    if let Some(errors) = errors {
        validate_quantity(
            errors.quantity(),
            errors.items().len(),
            "custom_error_responses",
        )?;
    }
    let logging = config
        .logging()
        .ok_or_else(|| validation("missing_cloudfront_logging"))?;
    let certificate = config
        .viewer_certificate()
        .ok_or_else(|| validation("missing_cloudfront_viewer_certificate"))?;
    let geo = config
        .restrictions()
        .and_then(|value| value.geo_restriction())
        .ok_or_else(|| validation("missing_cloudfront_geo_restriction"))?;
    validate_quantity(geo.quantity(), geo.items().len(), "geo_restriction")?;

    let mut unsupported = BTreeSet::new();
    for (present, code) in [
        (config.anycast_ip_list_id().is_some(), "anycast_ip_list"),
        (config.tenant_config().is_some(), "tenant_config"),
        (config.viewer_mtls_config().is_some(), "viewer_mtls"),
        (
            config.connection_function_association().is_some(),
            "connection_function",
        ),
        (config.cache_tag_config().is_some(), "cache_tag_config"),
        (
            !config.web_acl_id().unwrap_or_default().is_empty(),
            "web_acl",
        ),
        (config.staging().unwrap_or(false), "staging_distribution"),
        (
            !config
                .continuous_deployment_policy_id()
                .unwrap_or_default()
                .is_empty(),
            "continuous_deployment",
        ),
    ] {
        if present {
            unsupported.insert(code.to_string());
        }
    }
    if config
        .connection_mode()
        .is_some_and(|mode| mode.as_str() != "direct")
    {
        unsupported.insert("non_direct_connection_mode".to_string());
    }

    Ok(CloudFrontDistributionConfigProjection {
        caller_reference: config.caller_reference().to_string(),
        aliases: config
            .aliases()
            .map(|value| value.items().iter().cloned().collect())
            .unwrap_or_default(),
        default_root_object: config.default_root_object().unwrap_or_default().to_string(),
        origins: origins
            .items()
            .iter()
            .map(map_origin)
            .collect::<CloudFrontApiResult<_>>()?,
        origin_groups: origin_groups
            .map(|groups| groups.items().iter().map(map_origin_group).collect())
            .transpose()?
            .unwrap_or_default(),
        default_cache_behavior: map_default_behavior(
            config
                .default_cache_behavior()
                .ok_or_else(|| validation("missing_cloudfront_default_behavior"))?,
        )?,
        ordered_cache_behaviors: cache_behaviors
            .map(|behaviors| behaviors.items().iter().map(map_behavior).collect())
            .transpose()?
            .unwrap_or_default(),
        custom_error_responses: errors
            .map(|values| values.items().iter().map(map_custom_error).collect())
            .transpose()?
            .unwrap_or_default(),
        comment: config.comment().to_string(),
        logging: CloudFrontLoggingProjection {
            enabled: logging.enabled(),
            include_cookies: logging.include_cookies(),
            bucket: logging.bucket().to_string(),
            prefix: logging.prefix().to_string(),
        },
        price_class: config
            .price_class()
            .map(|v| v.as_str())
            .unwrap_or("None")
            .to_string(),
        enabled: config.enabled(),
        viewer_certificate: CloudFrontViewerCertificateProjection {
            cloudfront_default_certificate: certificate
                .cloud_front_default_certificate()
                .unwrap_or(false),
            certificate_arn: certificate
                .acm_certificate_arn()
                .or_else(|| certificate.iam_certificate_id())
                .map(ToString::to_string),
            certificate_source: certificate
                .certificate_source()
                .map(|v| v.as_str().to_string()),
            ssl_support_method: certificate
                .ssl_support_method()
                .map(|v| v.as_str().to_string()),
            minimum_protocol_version: certificate
                .minimum_protocol_version()
                .map(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        },
        geo_restriction: CloudFrontGeoRestrictionProjection {
            restriction_type: geo.restriction_type().as_str().to_string(),
            locations: geo.items().iter().cloned().collect(),
        },
        web_acl_id: config.web_acl_id().unwrap_or_default().to_string(),
        http_version: config
            .http_version()
            .map(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        ipv6_enabled: config.is_ipv6_enabled().unwrap_or(false),
        staging: config.staging().unwrap_or(false),
        continuous_deployment_policy_id: config
            .continuous_deployment_policy_id()
            .unwrap_or_default()
            .to_string(),
        unsupported_features: unsupported,
    })
}

fn map_origin(value: &Origin) -> CloudFrontApiResult<CloudFrontOriginProjection> {
    let kind_count = [
        value.custom_origin_config.is_some(),
        value.s3_origin_config.is_some(),
        value.vpc_origin_config.is_some(),
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    let kind = match (
        value.custom_origin_config.is_some(),
        value.s3_origin_config.is_some(),
        value.vpc_origin_config.is_some(),
    ) {
        (true, false, false) => CloudFrontOriginKind::Custom,
        (false, true, false) => CloudFrontOriginKind::S3,
        (false, false, true) => CloudFrontOriginKind::Vpc,
        _ => CloudFrontOriginKind::Unknown,
    };
    let mut unsupported = BTreeSet::new();
    if kind_count != 1 {
        unsupported.insert("ambiguous_origin_kind".to_string());
    }
    let headers = value
        .custom_headers()
        .map(|h| h.items())
        .unwrap_or_default();
    if let Some(custom_headers) = value.custom_headers() {
        validate_quantity(
            custom_headers.quantity(),
            custom_headers.items().len(),
            "origin_custom_headers",
        )?;
    }
    if !headers.is_empty() {
        unsupported.insert("custom_origin_headers".to_string());
    }
    if value.origin_shield().is_some() {
        unsupported.insert("origin_shield".to_string());
    }
    if value
        .origin_access_control_id()
        .is_some_and(|v| !v.is_empty())
    {
        unsupported.insert("origin_access_control".to_string());
    }
    let custom = value.custom_origin_config();
    if let Some(protocols) = custom.and_then(|value| value.origin_ssl_protocols()) {
        validate_quantity(
            protocols.quantity(),
            protocols.items().len(),
            "origin_ssl_protocols",
        )?;
    }
    if custom.is_some_and(|v| v.ip_address_type().is_some()) {
        unsupported.insert("origin_ip_address_type".to_string());
    }
    if custom.is_some_and(|v| v.origin_mtls_config().is_some()) {
        unsupported.insert("origin_mtls".to_string());
    }
    let read_timeout = custom
        .and_then(|v| v.origin_read_timeout())
        .or_else(|| {
            value
                .s3_origin_config()
                .and_then(|v| v.origin_read_timeout())
        })
        .or_else(|| {
            value
                .vpc_origin_config()
                .and_then(|v| v.origin_read_timeout())
        });
    Ok(CloudFrontOriginProjection {
        id: value.id().to_string(),
        domain_name: value.domain_name().to_string(),
        origin_path: value.origin_path().unwrap_or_default().to_string(),
        kind,
        http_port: custom
            .map(|v| checked_u16(v.http_port(), "invalid_cloudfront_http_port"))
            .transpose()?,
        https_port: custom
            .map(|v| checked_u16(v.https_port(), "invalid_cloudfront_https_port"))
            .transpose()?,
        protocol_policy: custom.map(|v| v.origin_protocol_policy().as_str().to_string()),
        tls_protocols: custom
            .and_then(|v| v.origin_ssl_protocols())
            .map(|v| v.items().iter().map(|p| p.as_str().to_string()).collect())
            .unwrap_or_default(),
        connection_attempts: checked_u8(
            value.connection_attempts().unwrap_or(3),
            "invalid_cloudfront_connection_attempts",
        )?,
        connection_timeout_seconds: checked_u8(
            value.connection_timeout().unwrap_or(10),
            "invalid_cloudfront_connection_timeout",
        )?,
        response_timeout_seconds: read_timeout
            .map(|v| checked_u16(v, "invalid_cloudfront_response_timeout"))
            .transpose()?,
        keepalive_timeout_seconds: custom
            .and_then(|v| v.origin_keepalive_timeout())
            .or_else(|| {
                value
                    .vpc_origin_config()
                    .and_then(|v| v.origin_keepalive_timeout())
            })
            .map(|v| checked_u16(v, "invalid_cloudfront_keepalive_timeout"))
            .transpose()?,
        custom_header_count: checked_u8(
            headers.len() as i32,
            "invalid_cloudfront_origin_header_count",
        )?,
        unsupported_features: unsupported,
    })
}

fn map_origin_group(
    value: &aws_sdk_cloudfront::types::OriginGroup,
) -> CloudFrontApiResult<CloudFrontOriginGroupProjection> {
    let members = value
        .members()
        .ok_or_else(|| validation("missing_cloudfront_origin_group_members"))?;
    validate_quantity(
        members.quantity(),
        members.items().len(),
        "origin_group_members",
    )?;
    if members.items().len() != 2 {
        return Err(validation("invalid_cloudfront_origin_group_member_count"));
    }
    let codes = value
        .failover_criteria()
        .and_then(|v| v.status_codes())
        .ok_or_else(|| validation("missing_cloudfront_failover_codes"))?;
    validate_quantity(
        codes.quantity(),
        codes.items().len(),
        "failover_status_codes",
    )?;
    let mut unsupported = BTreeSet::new();
    if value.selection_criteria().is_some() {
        unsupported.insert("origin_group_selection_criteria".to_string());
    }
    Ok(CloudFrontOriginGroupProjection {
        id: value.id().to_string(),
        primary_origin_id: members.items()[0].origin_id().to_string(),
        secondary_origin_id: members.items()[1].origin_id().to_string(),
        failover_status_codes: codes
            .items()
            .iter()
            .map(|v| checked_u16(*v, "invalid_cloudfront_failover_code"))
            .collect::<CloudFrontApiResult<_>>()?,
        unsupported_features: unsupported,
    })
}

#[allow(clippy::too_many_arguments)] // Flattens the two AWS behavior shapes into one redacted DTO.
fn behavior_projection(
    path_pattern: Option<String>,
    target: &str,
    viewer_policy: &str,
    allowed: Option<&aws_sdk_cloudfront::types::AllowedMethods>,
    compress: Option<bool>,
    trusted_signers: bool,
    trusted_keys: bool,
    smooth: Option<bool>,
    lambda: bool,
    functions: bool,
    field: Option<&str>,
    realtime: Option<&str>,
    cache_policy: Option<&str>,
    origin_policy: Option<&str>,
    response_policy: Option<&str>,
    grpc: bool,
    forwarded: bool,
    legacy_ttls: bool,
) -> CloudFrontApiResult<CloudFrontCacheBehaviorProjection> {
    let mut unsupported = BTreeSet::new();
    for (present, code) in [
        (trusted_signers, "trusted_signers"),
        (trusted_keys, "trusted_key_groups"),
        (smooth.unwrap_or(false), "smooth_streaming"),
        (lambda, "lambda_associations"),
        (functions, "function_associations"),
        (grpc, "grpc"),
        (forwarded, "legacy_forwarded_values"),
        (legacy_ttls, "legacy_ttl_fields"),
    ] {
        if present {
            unsupported.insert(code.to_string());
        }
    }
    if let Some(methods) = allowed {
        validate_quantity(methods.quantity(), methods.items().len(), "allowed_methods")?;
        if let Some(cached) = methods.cached_methods() {
            validate_quantity(cached.quantity(), cached.items().len(), "cached_methods")?;
        }
    }
    Ok(CloudFrontCacheBehaviorProjection {
        path_pattern,
        target_origin_id: target.to_string(),
        viewer_protocol_policy: viewer_policy.to_string(),
        allowed_methods: allowed
            .map(|v| v.items().iter().map(|m| m.as_str().to_string()).collect())
            .unwrap_or_default(),
        cached_methods: allowed
            .and_then(|v| v.cached_methods())
            .map(|v| v.items().iter().map(|m| m.as_str().to_string()).collect())
            .unwrap_or_default(),
        compress: compress.unwrap_or(false),
        cache_policy_id: cache_policy.map(ToString::to_string),
        origin_request_policy_id: origin_policy.map(ToString::to_string),
        response_headers_policy_id: response_policy.map(ToString::to_string),
        field_level_encryption_id: field.map(ToString::to_string),
        realtime_log_config_arn: realtime.map(ToString::to_string),
        unsupported_features: unsupported,
    })
}

#[allow(deprecated)] // Legacy CloudFront settings remain part of the provider revision.
fn map_default_behavior(
    v: &DefaultCacheBehavior,
) -> CloudFrontApiResult<CloudFrontCacheBehaviorProjection> {
    if let Some(values) = v.trusted_signers() {
        validate_quantity(values.quantity(), values.items().len(), "trusted_signers")?;
    }
    if let Some(values) = v.trusted_key_groups() {
        validate_quantity(
            values.quantity(),
            values.items().len(),
            "trusted_key_groups",
        )?;
    }
    if let Some(values) = v.lambda_function_associations() {
        validate_quantity(
            values.quantity(),
            values.items().len(),
            "lambda_function_associations",
        )?;
    }
    if let Some(values) = v.function_associations() {
        validate_quantity(
            values.quantity(),
            values.items().len(),
            "function_associations",
        )?;
    }
    behavior_projection(
        None,
        v.target_origin_id(),
        v.viewer_protocol_policy().as_str(),
        v.allowed_methods(),
        v.compress(),
        v.trusted_signers()
            .is_some_and(|values| !values.items().is_empty()),
        v.trusted_key_groups()
            .is_some_and(|values| !values.items().is_empty()),
        v.smooth_streaming(),
        v.lambda_function_associations()
            .is_some_and(|values| !values.items().is_empty()),
        v.function_associations()
            .is_some_and(|values| !values.items().is_empty()),
        v.field_level_encryption_id(),
        v.realtime_log_config_arn(),
        v.cache_policy_id(),
        v.origin_request_policy_id(),
        v.response_headers_policy_id(),
        v.grpc_config().is_some(),
        v.forwarded_values().is_some(),
        v.min_ttl().is_some() || v.default_ttl().is_some() || v.max_ttl().is_some(),
    )
}
#[allow(deprecated)] // Legacy CloudFront settings remain part of the provider revision.
fn map_behavior(v: &CacheBehavior) -> CloudFrontApiResult<CloudFrontCacheBehaviorProjection> {
    if let Some(values) = v.trusted_signers() {
        validate_quantity(values.quantity(), values.items().len(), "trusted_signers")?;
    }
    if let Some(values) = v.trusted_key_groups() {
        validate_quantity(
            values.quantity(),
            values.items().len(),
            "trusted_key_groups",
        )?;
    }
    if let Some(values) = v.lambda_function_associations() {
        validate_quantity(
            values.quantity(),
            values.items().len(),
            "lambda_function_associations",
        )?;
    }
    if let Some(values) = v.function_associations() {
        validate_quantity(
            values.quantity(),
            values.items().len(),
            "function_associations",
        )?;
    }
    behavior_projection(
        Some(v.path_pattern().to_string()),
        v.target_origin_id(),
        v.viewer_protocol_policy().as_str(),
        v.allowed_methods(),
        v.compress(),
        v.trusted_signers()
            .is_some_and(|values| !values.items().is_empty()),
        v.trusted_key_groups()
            .is_some_and(|values| !values.items().is_empty()),
        v.smooth_streaming(),
        v.lambda_function_associations()
            .is_some_and(|values| !values.items().is_empty()),
        v.function_associations()
            .is_some_and(|values| !values.items().is_empty()),
        v.field_level_encryption_id(),
        v.realtime_log_config_arn(),
        v.cache_policy_id(),
        v.origin_request_policy_id(),
        v.response_headers_policy_id(),
        v.grpc_config().is_some(),
        v.forwarded_values().is_some(),
        v.min_ttl().is_some() || v.default_ttl().is_some() || v.max_ttl().is_some(),
    )
}

fn map_custom_error(
    v: &aws_sdk_cloudfront::types::CustomErrorResponse,
) -> CloudFrontApiResult<CloudFrontCustomErrorResponseProjection> {
    Ok(CloudFrontCustomErrorResponseProjection {
        error_code: checked_u16(v.error_code(), "invalid_cloudfront_error_code")?,
        response_page_path: v.response_page_path().map(ToString::to_string),
        response_code: v
            .response_code()
            .map(|s| {
                s.parse()
                    .map_err(|_| validation("invalid_cloudfront_response_code"))
            })
            .transpose()?,
        minimum_ttl_seconds: u64::try_from(v.error_caching_min_ttl().unwrap_or(0))
            .map_err(|_| validation("invalid_cloudfront_error_ttl"))?,
    })
}

fn require_etag(value: Option<&str>) -> CloudFrontApiResult<&str> {
    let value = value.ok_or_else(|| validation("missing_cloudfront_etag"))?;
    validate_policy_text(value, 256, "invalid_cloudfront_policy_etag")?;
    Ok(value)
}
fn checked_u16(value: i32, code: &str) -> CloudFrontApiResult<u16> {
    u16::try_from(value).map_err(|_| validation(code))
}
fn checked_u8(value: i32, code: &str) -> CloudFrontApiResult<u8> {
    u8::try_from(value).map_err(|_| validation(code))
}
fn validate_quantity(quantity: i32, len: usize, kind: &str) -> CloudFrontApiResult<()> {
    if usize::try_from(quantity).ok() == Some(len) {
        Ok(())
    } else {
        Err(validation(&format!("invalid_cloudfront_{kind}_quantity")))
    }
}
fn non_empty(value: &str, code: &str) -> CloudFrontApiResult<()> {
    if value.is_empty() {
        Err(validation(code))
    } else {
        Ok(())
    }
}
fn validate_distribution_id(value: &str) -> CloudFrontApiResult<()> {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Ok(())
    } else {
        Err(validation("invalid_cloudfront_distribution_id"))
    }
}

fn validate_distribution_arn(
    arn: &str,
    partition: AwsPartition,
    account: &str,
    id: Option<&str>,
) -> CloudFrontApiResult<()> {
    let expected_prefix = format!(
        "arn:{}:cloudfront::{}:distribution/",
        partition.arn_partition(),
        account
    );
    let actual = arn
        .strip_prefix(&expected_prefix)
        .ok_or_else(|| validation("cloudfront_distribution_arn_scope_mismatch"))?;
    validate_distribution_id(actual)?;
    if id.is_some_and(|id| id != actual) {
        return Err(validation("cloudfront_distribution_arn_id_mismatch"));
    }
    Ok(())
}

fn validate_acm_certificate_arn(
    arn: &str,
    partition: AwsPartition,
    account: &str,
) -> CloudFrontApiResult<()> {
    let expected_prefix = format!(
        "arn:{}:acm:us-east-1:{}:certificate/",
        partition.arn_partition(),
        account
    );
    let certificate_id = arn
        .strip_prefix(&expected_prefix)
        .ok_or_else(|| validation("acm_certificate_arn_scope_mismatch"))?;
    if certificate_id.is_empty()
        || certificate_id.len() > 512
        || !certificate_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(validation("invalid_acm_certificate_arn"));
    }
    Ok(())
}

fn map_acm_certificate(
    detail: &aws_sdk_acm::types::CertificateDetail,
    expected_arn: &str,
    partition: AwsPartition,
    account_id: &str,
) -> CloudFrontApiResult<AcmCertificateObservation> {
    let arn = detail
        .certificate_arn()
        .ok_or_else(|| validation("missing_acm_certificate_arn"))?;
    validate_acm_certificate_arn(arn, partition, account_id)?;
    if arn != expected_arn {
        return Err(validation("acm_certificate_arn_mismatch"));
    }
    let domain_name = detail
        .domain_name()
        .ok_or_else(|| validation("missing_acm_certificate_domain_name"))?;
    validate_acm_text(domain_name, 253, "invalid_acm_certificate_domain_name")?;

    let mut subject_alternative_names = BTreeSet::new();
    for name in detail.subject_alternative_names() {
        validate_acm_text(name, 253, "invalid_acm_certificate_san")?;
        if !subject_alternative_names.insert(name.clone()) {
            return Err(validation("duplicate_acm_certificate_san"));
        }
    }
    let mut in_use_by = BTreeSet::new();
    for resource in detail.in_use_by() {
        validate_acm_text(resource, 2_048, "invalid_acm_certificate_in_use_by")?;
        if !in_use_by.insert(resource.clone()) {
            return Err(validation("duplicate_acm_certificate_in_use_by"));
        }
    }

    let raw_status = detail
        .status()
        .ok_or_else(|| validation("missing_acm_certificate_status"))?
        .as_str();
    validate_acm_text(raw_status, 128, "invalid_acm_certificate_status")?;
    let status = match raw_status {
        "PENDING_VALIDATION" => AcmCertificateStatus::PendingValidation,
        "ISSUED" => AcmCertificateStatus::Issued,
        "INACTIVE" => AcmCertificateStatus::Inactive,
        "EXPIRED" => AcmCertificateStatus::Expired,
        "VALIDATION_TIMED_OUT" => AcmCertificateStatus::ValidationTimedOut,
        "REVOKED" => AcmCertificateStatus::Revoked,
        "FAILED" => AcmCertificateStatus::Failed,
        other => AcmCertificateStatus::Unknown(other.to_string()),
    };
    let raw_type = detail
        .r#type()
        .ok_or_else(|| validation("missing_acm_certificate_type"))?
        .as_str();
    validate_acm_text(raw_type, 128, "invalid_acm_certificate_type")?;
    let certificate_type = match raw_type {
        "IMPORTED" => AcmCertificateType::Imported,
        "AMAZON_ISSUED" => AcmCertificateType::AmazonIssued,
        "PRIVATE" => AcmCertificateType::Private,
        other => AcmCertificateType::Unknown(other.to_string()),
    };
    let raw_key_algorithm = detail
        .key_algorithm()
        .ok_or_else(|| validation("missing_acm_certificate_key_algorithm"))?
        .as_str();
    validate_acm_text(
        raw_key_algorithm,
        128,
        "invalid_acm_certificate_key_algorithm",
    )?;
    let key_algorithm = match raw_key_algorithm {
        "RSA_1024" => AcmCertificateKeyAlgorithm::Rsa1024,
        "RSA_2048" => AcmCertificateKeyAlgorithm::Rsa2048,
        "RSA_3072" => AcmCertificateKeyAlgorithm::Rsa3072,
        "RSA_4096" => AcmCertificateKeyAlgorithm::Rsa4096,
        "EC_prime256v1" => AcmCertificateKeyAlgorithm::EcPrime256v1,
        other => AcmCertificateKeyAlgorithm::Unsupported(other.to_string()),
    };
    let managed_by = detail
        .managed_by()
        .map(|value| value.as_str())
        .map(|value| {
            validate_acm_text(value, 128, "invalid_acm_certificate_managed_by")?;
            Ok(value.to_string())
        })
        .transpose()?;

    Ok(AcmCertificateObservation {
        arn: arn.to_string(),
        account_id: account_id.to_string(),
        partition,
        region: "us-east-1".to_string(),
        domain_name: domain_name.to_string(),
        subject_alternative_names,
        status,
        certificate_type,
        key_algorithm,
        managed_by,
        not_before_unix_seconds: detail.not_before().map(|value| value.secs()),
        not_after_unix_seconds: detail.not_after().map(|value| value.secs()),
        in_use_by,
    })
}

fn validate_acm_text(value: &str, max_len: usize, code: &str) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(validation(code))
    } else {
        Ok(())
    }
}

fn partition_from_identity_arn(arn: &str, account: &str) -> CloudFrontApiResult<AwsPartition> {
    if !arn.contains(&format!(":{account}:")) {
        return Err(validation("sts_identity_account_mismatch"));
    }
    match arn.split(':').nth(1) {
        Some("aws") => Ok(AwsPartition::Aws),
        Some("aws-cn") => Ok(AwsPartition::AwsChina),
        Some("aws-us-gov") => Ok(AwsPartition::AwsUsGov),
        _ => Err(validation("unsupported_aws_partition")),
    }
}
fn is_aws_account_id(value: &str) -> bool {
    value.len() == 12 && value.bytes().all(|b| b.is_ascii_digit())
}
fn validate_non_secret_revision(value: &str) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > 512
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(validation("invalid_cloudfront_credential_revision"))
    } else {
        Ok(())
    }
}
fn validate_role_arn(value: &str) -> CloudFrontApiResult<()> {
    let mut p = value.splitn(6, ':');
    let valid = p.next() == Some("arn")
        && matches!(p.next(), Some("aws" | "aws-cn" | "aws-us-gov"))
        && p.next() == Some("iam")
        && p.next() == Some("")
        && p.next().is_some_and(is_aws_account_id)
        && p.next()
            .is_some_and(|v| v.starts_with("role/") && v.len() > 5 && is_role_path(&v[5..]));
    if valid {
        Ok(())
    } else {
        Err(validation("invalid_aws_role_arn"))
    }
}
fn is_role_path(value: &str) -> bool {
    value.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(b, b'/' | b'_' | b'+' | b'=' | b',' | b'.' | b'@' | b'-')
    })
}
fn validate_role_session_name(value: &str) -> CloudFrontApiResult<()> {
    if (2..=64).contains(&value.len())
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'=' | b',' | b'.' | b'@' | b'-')
        })
    {
        Ok(())
    } else {
        Err(validation("invalid_aws_role_session_name"))
    }
}
fn is_valid_external_id(value: &str) -> bool {
    (2..=1_224).contains(&value.len())
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'_' | b'+' | b'=' | b',' | b'.' | b'@' | b':' | b'/' | b'-'
                )
        })
}
fn validate_endpoint(endpoint: Option<&str>) -> CloudFrontApiResult<()> {
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    let parsed = url::Url::parse(endpoint).map_err(|_| validation("invalid_aws_endpoint_url"))?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(validation("invalid_aws_endpoint_url"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| validation("invalid_aws_endpoint_url"))?;
    match parsed.scheme() {
        "http" | "https" if is_loopback_host(host) => Ok(()),
        _ => Err(validation("untrusted_aws_endpoint_url")),
    }
}
fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || IpAddr::from_str(host.trim_start_matches('[').trim_end_matches(']'))
            .is_ok_and(|a| a.is_loopback())
}
fn service_code<E, R>(error: &aws_sdk_cloudfront::error::SdkError<E, R>) -> Option<&str>
where
    E: ProvideErrorMetadata,
{
    error
        .as_service_error()
        .and_then(ProvideErrorMetadata::code)
}
fn acm_service_code<E, R>(error: &aws_sdk_acm::error::SdkError<E, R>) -> Option<&str>
where
    E: ProvideErrorMetadata,
{
    error
        .as_service_error()
        .and_then(ProvideErrorMetadata::code)
}
fn map_acm_read_error(code: Option<&str>) -> NormalizedProviderError {
    match code {
        Some(code) => map_service_error(code),
        None => provider_error(
            ProviderErrorCategory::Transient,
            "acm_transport_error",
            None,
        ),
    }
}
fn map_read_error(code: Option<&str>) -> NormalizedProviderError {
    match code {
        Some(code) => map_service_error(code),
        None => provider_error(
            ProviderErrorCategory::Transient,
            "cloudfront_transport_error",
            None,
        ),
    }
}
fn map_service_error(code: &str) -> NormalizedProviderError {
    let (category, retry) = match code {
        "InvalidClientTokenId"
        | "SignatureDoesNotMatch"
        | "ExpiredToken"
        | "ExpiredTokenException"
        | "UnrecognizedClientException" => (ProviderErrorCategory::Authentication, None),
        "AccessDenied" | "AccessDeniedException" | "NotAuthorized" => {
            (ProviderErrorCategory::Authorization, None)
        }
        "Throttling" | "ThrottlingException" => (
            ProviderErrorCategory::Throttled,
            Some(THROTTLE_RETRY_AFTER_MS),
        ),
        "TooManyDistributions" | "LimitsExceeded" => (ProviderErrorCategory::Quota, None),
        "NoSuchDistribution"
        | "EntityNotFound"
        | "NoSuchResource"
        | "NoSuchCachePolicy"
        | "NoSuchOriginRequestPolicy"
        | "NoSuchResponseHeadersPolicy"
        | "NoSuchInvalidation" => (ProviderErrorCategory::NotFound, None),
        "InvalidArgument" | "ValidationError" => (ProviderErrorCategory::Validation, None),
        "InternalFailure" | "ServiceUnavailable" | "RequestTimeoutException" => {
            (ProviderErrorCategory::Transient, None)
        }
        _ => (ProviderErrorCategory::Transient, None),
    };
    provider_error(category, "cloudfront_service_error", retry)
}
fn validation(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Validation, code, None)
}
fn provider_error(
    category: ProviderErrorCategory,
    code: &str,
    retry: Option<u64>,
) -> NormalizedProviderError {
    NormalizedProviderError::new(category, code, "AWS provider request failed", retry, None)
        .expect("static normalized provider error")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_override_is_loopback_only() {
        assert!(validate_endpoint(Some("http://127.0.0.1:8080")).is_ok());
        assert!(validate_endpoint(Some("https://cloudfront.example.test")).is_err());
    }
    #[test]
    fn assume_role_inputs_fail_closed() {
        assert!(AwsAssumeRoleSpec::new(
            "arn:aws:iam::123456789012:role/edge/read",
            "edgion-cloudfront"
        )
        .is_ok());
        assert!(
            AwsAssumeRoleSpec::new("arn:aws:s3::123456789012:role/no", "edgion-cloudfront")
                .is_err()
        );
    }

    #[test]
    fn policy_not_found_is_classified_but_exact_get_drift_remains_conflict() {
        for code in [
            "NoSuchCachePolicy",
            "NoSuchOriginRequestPolicy",
            "NoSuchResponseHeadersPolicy",
        ] {
            assert_eq!(
                map_service_error(code).category(),
                ProviderErrorCategory::NotFound
            );
            assert_eq!(
                map_policy_observation_error(Some(code)).category(),
                ProviderErrorCategory::Conflict
            );
        }
    }

    #[test]
    fn domain_conflict_missing_validation_resource_is_not_transient() {
        assert_eq!(
            map_service_error("EntityNotFound").category(),
            ProviderErrorCategory::NotFound
        );
    }
}
