use std::{net::IpAddr, str::FromStr};

use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_cloudfront::{
    config::{retry::RetryConfig, timeout::TimeoutConfig},
    error::ProvideErrorMetadata,
};
use edgion_center_core::{NormalizedProviderError, ProviderErrorCategory};

use crate::{
    is_aws_account_id, validation, AwsPartition, CloudFrontApi, CloudFrontApiResult,
    CloudFrontCreateDistribution, CloudFrontDeleteGuard, CloudFrontDispatchTracker,
    CloudFrontDistributionDetail, CloudFrontDistributionPage, CloudFrontDistributionSummary,
    CloudFrontFingerprintKey, CloudFrontHttpsOrigin, CloudFrontLifecycleResult,
    CloudFrontOriginEndpointUpdate, CloudFrontTags, CloudFrontWebAclUpdate,
};

const FIXED_ORIGIN_ID: &str = "edgion-api-origin";
const CACHING_DISABLED_POLICY_ID: &str = "4135ea2d-6df8-44a3-9df3-4b5a84be39ad";
const ALL_VIEWER_EXCEPT_HOST_HEADER_POLICY_ID: &str = "b689b0a8-53d0-40ab-baf2-68738e2966ac";
const READ_MAX_ATTEMPTS: u32 = 3;
const MUTATION_MAX_ATTEMPTS: u32 = 1;
const OPERATION_ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const OPERATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const DEFAULT_THROTTLE_RETRY_AFTER_MS: u64 = 1_000;

/// Loopback-only endpoint overrides are retained solely for hermetic inventory tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AwsCloudFrontApiOptions {
    pub cloudfront_endpoint_url: Option<String>,
    pub sts_endpoint_url: Option<String>,
}

/// Loads the standard ambient AWS credential chain for CloudFront composition.
///
/// Provider-specific production composition deliberately uses this factory instead of borrowing
/// another AWS adapter's transport setup. Endpoint overrides remain a hermetic test-only seam.
#[derive(Debug, Clone, Copy, Default)]
pub struct AwsCloudFrontSdkConfigFactory;

impl AwsCloudFrontSdkConfigFactory {
    pub async fn ambient() -> CloudFrontApiResult<SdkConfig> {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        if config.endpoint_url().is_some() {
            return Err(validation("inherited_aws_endpoint_override_forbidden"));
        }
        Ok(config)
    }
}

/// Credential-owning AWS transport for inventory and the retained fixed lifecycle only.
pub struct AwsCloudFrontApi {
    account_id: String,
    partition: AwsPartition,
    credential_revision: String,
    read_client: aws_sdk_cloudfront::Client,
    mutation_client: aws_sdk_cloudfront::Client,
    fingerprint_key: CloudFrontFingerprintKey,
    dispatch_tracker: Option<CloudFrontDispatchTracker>,
}

/// Short-lived raw provider configuration evidence for a later fixed-scope writer.
/// It is never serializable or returned through inventory DTOs.
#[allow(dead_code)]
pub(crate) struct CloudFrontSensitiveSdkConfigSnapshot {
    pub(crate) config: aws_sdk_cloudfront::types::DistributionConfig,
    pub(crate) etag: String,
    pub(crate) wire: crate::wire_fidelity::CloudFrontSensitiveWireBytes,
}

impl AwsCloudFrontApi {
    pub async fn new(
        config: &SdkConfig,
        fingerprint_key: CloudFrontFingerprintKey,
        revision: impl Into<String>,
    ) -> CloudFrontApiResult<Self> {
        Self::with_options(
            config,
            fingerprint_key,
            revision,
            AwsCloudFrontApiOptions::default(),
        )
        .await
    }
    pub async fn with_options(
        config: &SdkConfig,
        fingerprint_key: CloudFrontFingerprintKey,
        revision: impl Into<String>,
        options: AwsCloudFrontApiOptions,
    ) -> CloudFrontApiResult<Self> {
        let credential_revision = revision.into();
        if credential_revision.is_empty()
            || credential_revision.len() > 512
            || credential_revision.trim() != credential_revision
            || credential_revision.chars().any(char::is_control)
        {
            return Err(validation("invalid_cloudfront_credential_revision"));
        }
        if config.endpoint_url().is_some() {
            return Err(validation("inherited_aws_endpoint_override_forbidden"));
        }
        validate_endpoint(options.cloudfront_endpoint_url.as_deref())?;
        validate_endpoint(options.sts_endpoint_url.as_deref())?;
        let timeout = TimeoutConfig::builder()
            .operation_attempt_timeout(OPERATION_ATTEMPT_TIMEOUT)
            .operation_timeout(OPERATION_TIMEOUT)
            .build();
        let mut sts = aws_sdk_sts::config::Builder::from(config)
            .retry_config(
                aws_sdk_sts::config::retry::RetryConfig::standard()
                    .with_max_attempts(READ_MAX_ATTEMPTS),
            )
            .timeout_config(timeout.clone());
        if let Some(endpoint) = options.sts_endpoint_url {
            sts = sts.endpoint_url(endpoint);
        }
        let identity = aws_sdk_sts::Client::from_conf(sts.build())
            .get_caller_identity()
            .send()
            .await
            .map_err(|error| map_error(error.as_service_error().and_then(|value| value.code())))?;
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
        let mut cloudfront = aws_sdk_cloudfront::config::Builder::from(config)
            .retry_config(RetryConfig::standard().with_max_attempts(READ_MAX_ATTEMPTS))
            .timeout_config(timeout.clone());
        let mut mutation_cloudfront = aws_sdk_cloudfront::config::Builder::from(config)
            .retry_config(RetryConfig::standard().with_max_attempts(MUTATION_MAX_ATTEMPTS))
            .timeout_config(timeout);
        if let Some(endpoint) = options.cloudfront_endpoint_url {
            cloudfront = cloudfront.endpoint_url(endpoint.clone());
            mutation_cloudfront = mutation_cloudfront.endpoint_url(endpoint);
        }
        Ok(Self {
            account_id,
            partition,
            credential_revision,
            read_client: aws_sdk_cloudfront::Client::from_conf(cloudfront.build()),
            mutation_client: aws_sdk_cloudfront::Client::from_conf(mutation_cloudfront.build()),
            fingerprint_key,
            dispatch_tracker: None,
        })
    }

    /// Attaches request-local dispatch evidence without exposing raw provider requests.
    pub fn with_dispatch_tracker(mut self, tracker: CloudFrontDispatchTracker) -> Self {
        self.dispatch_tracker = Some(tracker);
        self
    }

    fn mark_dispatched(&self) {
        if let Some(tracker) = &self.dispatch_tracker {
            tracker.mark_dispatched();
        }
    }
    #[allow(dead_code)]
    pub(crate) async fn read_sensitive_sdk_config_snapshot(
        &self,
        distribution_id: &str,
    ) -> CloudFrontApiResult<Option<CloudFrontSensitiveSdkConfigSnapshot>> {
        validate_distribution_id(distribution_id)?;
        let (capture, handle) = crate::wire_fidelity::cloudfront_response_capture();
        let output = match self
            .read_client
            .get_distribution_config()
            .id(distribution_id)
            .customize()
            .interceptor(capture)
            .send()
            .await
        {
            Ok(value) => value,
            Err(error) if service_code(&error) == Some("NoSuchDistribution") => return Ok(None),
            Err(error) => return Err(map_error(service_code(&error))),
        };
        let etag = output
            .e_tag()
            .filter(|value| !value.is_empty() && value.len() <= 512)
            .ok_or_else(|| validation("missing_cloudfront_etag"))?
            .to_string();
        let config = output
            .distribution_config()
            .ok_or_else(|| validation("missing_cloudfront_distribution_config"))?
            .clone();
        Ok(Some(CloudFrontSensitiveSdkConfigSnapshot {
            config,
            etag,
            wire: handle.take()?,
        }))
    }
    #[allow(dead_code)]
    pub(crate) async fn assert_sdk_config_round_trip(
        &self,
        distribution_id: &str,
        snapshot: &CloudFrontSensitiveSdkConfigSnapshot,
    ) -> CloudFrontApiResult<()> {
        crate::wire_fidelity::assert_sdk_config_round_trip(
            &self.read_client,
            distribution_id,
            &snapshot.etag,
            &snapshot.wire,
            &snapshot.config,
        )
        .await
    }

    /// Creates exactly one enabled, HTTPS-only API distribution. This is intentionally not a
    /// generic CloudFront configuration API.
    pub async fn create_minimal_distribution(
        &self,
        request: CloudFrontCreateDistribution,
    ) -> CloudFrontApiResult<CloudFrontLifecycleResult> {
        validate_caller_reference(&request.caller_reference)?;
        validate_https_origin(&request.origin)?;
        let config = minimal_distribution_config(&request)?;
        let request = self
            .mutation_client
            .create_distribution()
            .distribution_config(config);
        self.mark_dispatched();
        let output = request
            .send()
            .await
            .map_err(|error| map_mutation_error(service_code(&error)))?;
        self.lifecycle_result(
            output.distribution(),
            output.e_tag(),
            "cloudfront_create_result_incomplete",
        )
        .map_err(|_| unknown_mutation_outcome())
    }

    pub async fn update_origin_endpoint(
        &self,
        distribution_id: &str,
        update: CloudFrontOriginEndpointUpdate,
    ) -> CloudFrontApiResult<CloudFrontLifecycleResult> {
        validate_distribution_id(distribution_id)?;
        validate_https_origin(&CloudFrontHttpsOrigin {
            domain_name: update.domain_name.clone(),
            https_port: update.https_port,
        })?;
        self.update_from_fresh_config(distribution_id, |config| {
            let origin = single_supported_origin_mut(config)?;
            origin.domain_name = update.domain_name.clone();
            let custom = origin
                .custom_origin_config
                .as_mut()
                .ok_or_else(|| validation("cloudfront_supported_custom_origin_required"))?;
            custom.https_port = i32::from(update.https_port);
            Ok(vec![":DomainName", ":HttpsPort"])
        })
        .await
    }

    pub async fn set_distribution_enabled(
        &self,
        distribution_id: &str,
        enabled: bool,
    ) -> CloudFrontApiResult<CloudFrontLifecycleResult> {
        validate_distribution_id(distribution_id)?;
        self.update_from_fresh_config(distribution_id, |config| {
            assert_supported_minimal_update_shape(config)?;
            config.enabled = enabled;
            Ok(vec![":Enabled"])
        })
        .await
    }

    /// Attaches, replaces, or explicitly detaches only the root `WebACLId`. The target must pass
    /// the same fixed-shape eligibility check as the CLD-28F lifecycle. A current identical value
    /// is a read-only no-op and never dispatches `UpdateDistribution`.
    pub async fn set_distribution_web_acl(
        &self,
        distribution_id: &str,
        update: CloudFrontWebAclUpdate,
    ) -> CloudFrontApiResult<CloudFrontLifecycleResult> {
        validate_distribution_id(distribution_id)?;
        validate_web_acl_id(update.web_acl_id.as_deref())?;
        let snapshot = self
            .read_sensitive_sdk_config_snapshot(distribution_id)
            .await?
            .ok_or_else(|| validation("cloudfront_distribution_not_found"))?;
        self.assert_sdk_config_round_trip(distribution_id, &snapshot)
            .await?;
        assert_supported_minimal_update_shape(&snapshot.config)?;
        if normalized_web_acl_id(&snapshot.config) == update.web_acl_id.as_deref() {
            let detail = self
                .get_distribution(distribution_id)
                .await?
                .ok_or_else(|| validation("cloudfront_distribution_not_found"))?;
            return Ok(CloudFrontLifecycleResult {
                deployed: detail.summary.status == "Deployed",
                distribution: detail,
            });
        }
        let mut desired = snapshot.config.clone();
        desired.web_acl_id = update.web_acl_id;
        crate::wire_fidelity::assert_sdk_config_write_set(
            &self.read_client,
            distribution_id,
            &snapshot.etag,
            &snapshot.config,
            &desired,
            &[":WebACLId"],
        )
        .await?;
        let request = self
            .mutation_client
            .update_distribution()
            .id(distribution_id)
            .if_match(snapshot.etag)
            .distribution_config(desired);
        self.mark_dispatched();
        let output = request
            .send()
            .await
            .map_err(|error| map_mutation_error(service_code(&error)))?;
        self.lifecycle_result(
            output.distribution(),
            output.e_tag(),
            "cloudfront_web_acl_update_result_incomplete",
        )
        .map_err(|_| unknown_mutation_outcome())
    }

    /// Deletes only a separately disabled and deployed fixed-shape distribution with no aliases
    /// or Web ACL association. These are the only reference-bearing fields exposed by the
    /// retained product surface; future reference types must be checked here before delete.
    pub async fn delete_disabled_distribution(
        &self,
        distribution_id: &str,
        guard: &CloudFrontDeleteGuard,
    ) -> CloudFrontApiResult<()> {
        validate_distribution_id(distribution_id)?;
        if guard.confirmation != distribution_id {
            return Err(validation("cloudfront_delete_guard_rejected"));
        }
        let snapshot = self
            .read_sensitive_sdk_config_snapshot(distribution_id)
            .await?
            .ok_or_else(|| validation("cloudfront_distribution_not_found"))?;
        self.assert_sdk_config_round_trip(distribution_id, &snapshot)
            .await?;
        assert_supported_minimal_update_shape(&snapshot.config)?;
        if has_web_acl_association(&snapshot.config) {
            return Err(validation("cloudfront_delete_requires_no_web_acl"));
        }
        if snapshot.config.enabled {
            return Err(validation(
                "cloudfront_delete_requires_disabled_distribution",
            ));
        }
        let observed = self
            .read_client
            .get_distribution()
            .id(distribution_id)
            .send()
            .await
            .map_err(|error| map_error(service_code(&error)))?;
        if observed
            .distribution()
            .is_none_or(|distribution| distribution.status() != "Deployed")
        {
            return Err(validation("cloudfront_delete_requires_disabled_deployment"));
        }
        let request = self
            .mutation_client
            .delete_distribution()
            .id(distribution_id)
            .if_match(snapshot.etag);
        self.mark_dispatched();
        request
            .send()
            .await
            .map_err(|error| map_mutation_error(service_code(&error)))?;
        Ok(())
    }

    async fn update_from_fresh_config<F>(
        &self,
        distribution_id: &str,
        overlay: F,
    ) -> CloudFrontApiResult<CloudFrontLifecycleResult>
    where
        F: FnOnce(
            &mut aws_sdk_cloudfront::types::DistributionConfig,
        ) -> CloudFrontApiResult<Vec<&'static str>>,
    {
        let snapshot = self
            .read_sensitive_sdk_config_snapshot(distribution_id)
            .await?
            .ok_or_else(|| validation("cloudfront_distribution_not_found"))?;
        self.assert_sdk_config_round_trip(distribution_id, &snapshot)
            .await?;
        let mut desired = snapshot.config.clone();
        let write_set = overlay(&mut desired)?;
        crate::wire_fidelity::assert_sdk_config_write_set(
            &self.read_client,
            distribution_id,
            &snapshot.etag,
            &snapshot.config,
            &desired,
            &write_set,
        )
        .await?;
        let request = self
            .mutation_client
            .update_distribution()
            .id(distribution_id)
            .if_match(snapshot.etag)
            .distribution_config(desired);
        self.mark_dispatched();
        let output = request
            .send()
            .await
            .map_err(|error| map_mutation_error(service_code(&error)))?;
        self.lifecycle_result(
            output.distribution(),
            output.e_tag(),
            "cloudfront_update_result_incomplete",
        )
        .map_err(|_| unknown_mutation_outcome())
    }

    fn lifecycle_result(
        &self,
        distribution: Option<&aws_sdk_cloudfront::types::Distribution>,
        etag: Option<&str>,
        missing_code: &str,
    ) -> CloudFrontApiResult<CloudFrontLifecycleResult> {
        let distribution = distribution.ok_or_else(|| validation(missing_code))?;
        let etag = etag
            .filter(|value| !value.is_empty() && value.len() <= 512)
            .ok_or_else(|| validation(missing_code))?;
        let config = distribution
            .distribution_config()
            .ok_or_else(|| validation(missing_code))?;
        let summary = map_distribution(
            distribution,
            config.enabled(),
            self.partition,
            &self.account_id,
        )?;
        Ok(CloudFrontLifecycleResult {
            deployed: summary.status == "Deployed",
            distribution: CloudFrontDistributionDetail {
                etag: etag.to_string(),
                etag_revision_mac: self.fingerprint_key.mac_etag_revision(
                    self.partition,
                    &self.account_id,
                    &summary.id,
                    etag,
                )?,
                summary,
                web_acl_id: project_web_acl_id(config),
                supported_origin: project_supported_origin(config),
            },
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
            .map_err(|error| map_error(service_code(&error)))?;
        let list = output
            .distribution_list()
            .ok_or_else(|| validation("missing_cloudfront_distribution_list"))?;
        if list.is_truncated() != list.next_marker().is_some()
            || list.items().len() > usize::from(max_items)
        {
            return Err(validation("inconsistent_cloudfront_distribution_page"));
        }
        let items = list
            .items()
            .iter()
            .map(|value| map_summary(value, self.partition, &self.account_id))
            .collect::<CloudFrontApiResult<Vec<_>>>()?;
        Ok(CloudFrontDistributionPage {
            items,
            is_truncated: list.is_truncated(),
            next_marker: list.next_marker().map(ToString::to_string),
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
            Ok(value) => value,
            Err(error) if service_code(&error) == Some("NoSuchDistribution") => return Ok(None),
            Err(error) => return Err(map_error(service_code(&error))),
        };
        let distribution = output
            .distribution()
            .ok_or_else(|| validation("missing_cloudfront_distribution"))?;
        let etag = output
            .e_tag()
            .filter(|value| !value.is_empty() && value.len() <= 512)
            .ok_or_else(|| validation("missing_cloudfront_etag"))?;
        let config = distribution
            .distribution_config()
            .ok_or_else(|| validation("missing_cloudfront_distribution_config"))?;
        let summary = map_distribution(
            distribution,
            config.enabled(),
            self.partition,
            &self.account_id,
        )?;
        Ok(Some(CloudFrontDistributionDetail {
            summary,
            etag: etag.to_string(),
            etag_revision_mac: self.fingerprint_key.mac_etag_revision(
                self.partition,
                &self.account_id,
                distribution_id,
                etag,
            )?,
            web_acl_id: project_web_acl_id(config),
            supported_origin: project_supported_origin(config),
        }))
    }
    async fn list_tags_for_resource(&self, arn: &str) -> CloudFrontApiResult<CloudFrontTags> {
        let output = self
            .read_client
            .list_tags_for_resource()
            .resource(arn)
            .send()
            .await
            .map_err(|error| map_error(service_code(&error)))?;
        let mut tags = CloudFrontTags::default();
        for tag in output.tags().map(|value| value.items()).unwrap_or_default() {
            let key = tag.key();
            if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
                return Err(validation("invalid_cloudfront_tag_key"));
            };
            if key == "edgion.center/resource-id" {
                tags.center_resource_id = tag.value().map(ToString::to_string);
            };
            tags.keys.insert(key.to_string());
        }
        Ok(tags)
    }
}

fn map_summary(
    value: &aws_sdk_cloudfront::types::DistributionSummary,
    partition: AwsPartition,
    account: &str,
) -> CloudFrontApiResult<CloudFrontDistributionSummary> {
    map_values(
        value.id(),
        value.arn(),
        value.domain_name(),
        value.status(),
        value.enabled(),
        value.last_modified_time().secs(),
        (partition, account),
    )
}
fn map_distribution(
    value: &aws_sdk_cloudfront::types::Distribution,
    enabled: bool,
    partition: AwsPartition,
    account: &str,
) -> CloudFrontApiResult<CloudFrontDistributionSummary> {
    map_values(
        value.id(),
        value.arn(),
        value.domain_name(),
        value.status(),
        enabled,
        value.last_modified_time().secs(),
        (partition, account),
    )
}
fn map_values(
    id: &str,
    arn: &str,
    domain: &str,
    status: &str,
    enabled: bool,
    modified: i64,
    scope: (AwsPartition, &str),
) -> CloudFrontApiResult<CloudFrontDistributionSummary> {
    let (partition, account) = scope;
    validate_distribution_id(id)?;
    if arn
        != format!(
            "arn:{}:cloudfront::{account}:distribution/{id}",
            partition.arn_partition()
        )
        || domain.is_empty()
        || status.is_empty()
        || modified <= 0
    {
        return Err(validation("invalid_cloudfront_distribution_summary"));
    }
    Ok(CloudFrontDistributionSummary {
        id: id.to_string(),
        arn: arn.to_string(),
        domain_name: domain.to_string(),
        status: status.to_string(),
        enabled,
        last_modified_unix_seconds: modified,
    })
}

fn minimal_distribution_config(
    request: &CloudFrontCreateDistribution,
) -> CloudFrontApiResult<aws_sdk_cloudfront::types::DistributionConfig> {
    use aws_sdk_cloudfront::types::{
        AllowedMethods, CachedMethods, DefaultCacheBehavior, GeoRestriction, GeoRestrictionType,
        Method, Origin, OriginProtocolPolicy, OriginSslProtocols, Origins, Restrictions,
        SslProtocol, ViewerCertificate, ViewerProtocolPolicy,
    };

    let ssl_protocols = OriginSslProtocols::builder()
        .quantity(1)
        .items(SslProtocol::TlSv12)
        .build()
        .map_err(|_| validation("cloudfront_minimal_ssl_protocols_build_failed"))?;
    let custom_origin = aws_sdk_cloudfront::types::CustomOriginConfig::builder()
        .http_port(80)
        .https_port(i32::from(request.origin.https_port))
        .origin_protocol_policy(OriginProtocolPolicy::HttpsOnly)
        .origin_ssl_protocols(ssl_protocols)
        .origin_read_timeout(30)
        .origin_keepalive_timeout(5)
        .build()
        .map_err(|_| validation("cloudfront_minimal_custom_origin_build_failed"))?;
    let origin = Origin::builder()
        .id(FIXED_ORIGIN_ID)
        .domain_name(&request.origin.domain_name)
        .custom_origin_config(custom_origin)
        .connection_attempts(2)
        .connection_timeout(5)
        .build()
        .map_err(|_| validation("cloudfront_minimal_origin_build_failed"))?;
    let cached_methods = CachedMethods::builder()
        .quantity(2)
        .items(Method::Get)
        .items(Method::Head)
        .build()
        .map_err(|_| validation("cloudfront_minimal_cached_methods_build_failed"))?;
    let methods = AllowedMethods::builder()
        .quantity(7)
        .items(Method::Get)
        .items(Method::Head)
        .items(Method::Options)
        .items(Method::Put)
        .items(Method::Patch)
        .items(Method::Post)
        .items(Method::Delete)
        .cached_methods(cached_methods)
        .build()
        .map_err(|_| validation("cloudfront_minimal_allowed_methods_build_failed"))?;
    let behavior = DefaultCacheBehavior::builder()
        .target_origin_id(FIXED_ORIGIN_ID)
        .viewer_protocol_policy(ViewerProtocolPolicy::RedirectToHttps)
        .allowed_methods(methods)
        .cache_policy_id(CACHING_DISABLED_POLICY_ID)
        .origin_request_policy_id(ALL_VIEWER_EXCEPT_HOST_HEADER_POLICY_ID)
        .compress(false)
        .build()
        .map_err(|_| validation("cloudfront_minimal_behavior_build_failed"))?;
    let geo_restriction = GeoRestriction::builder()
        .restriction_type(GeoRestrictionType::None)
        .quantity(0)
        .build()
        .map_err(|_| validation("cloudfront_minimal_geo_restriction_build_failed"))?;
    aws_sdk_cloudfront::types::DistributionConfig::builder()
        .caller_reference(&request.caller_reference)
        .origins(
            Origins::builder()
                .quantity(1)
                .items(origin)
                .build()
                .map_err(|_| validation("cloudfront_minimal_origins_build_failed"))?,
        )
        .default_cache_behavior(behavior)
        .comment("Edgion Center managed API distribution")
        .enabled(true)
        .viewer_certificate(
            ViewerCertificate::builder()
                .cloud_front_default_certificate(true)
                .build(),
        )
        .restrictions(
            Restrictions::builder()
                .geo_restriction(geo_restriction)
                .build(),
        )
        .build()
        .map_err(|_| validation("cloudfront_minimal_distribution_build_failed"))
}

fn validate_caller_reference(value: &str) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(validation("invalid_cloudfront_caller_reference"))
    } else {
        Ok(())
    }
}

fn validate_https_origin(value: &CloudFrontHttpsOrigin) -> CloudFrontApiResult<()> {
    if value.https_port == 0 {
        return Err(validation("invalid_cloudfront_origin_https_port"));
    }
    edgion_center_core::DomainName::new(value.domain_name.clone())
        .map_err(|_| validation("invalid_cloudfront_origin_domain"))?;
    Ok(())
}

/// Existing distributions remain readable even when their legacy configuration cannot be
/// represented by the fixed lifecycle. Only a fully-supported single HTTPS origin is projected.
fn project_supported_origin(
    config: &aws_sdk_cloudfront::types::DistributionConfig,
) -> Option<CloudFrontHttpsOrigin> {
    assert_supported_minimal_update_shape(config).ok()?;
    let origin = config.origins()?.items().first()?;
    let result = CloudFrontHttpsOrigin {
        domain_name: origin.domain_name().to_string(),
        https_port: u16::try_from(origin.custom_origin_config()?.https_port()).ok()?,
    };
    validate_https_origin(&result).ok()?;
    Some(result)
}

/// Web ACL IDs are opaque provider values. Do not surface malformed values from an untrusted
/// provider response, but retain the fact that an association exists for lifecycle checks.
fn project_web_acl_id(config: &aws_sdk_cloudfront::types::DistributionConfig) -> Option<String> {
    let value = config.web_acl_id()?.trim();
    if value.is_empty()
        || value.len() > 2048
        || value.chars().any(char::is_control)
        || value != config.web_acl_id()?
    {
        None
    } else {
        Some(value.to_string())
    }
}

fn has_web_acl_association(config: &aws_sdk_cloudfront::types::DistributionConfig) -> bool {
    config
        .web_acl_id()
        .is_some_and(|value| !value.trim().is_empty())
}

fn normalized_web_acl_id(config: &aws_sdk_cloudfront::types::DistributionConfig) -> Option<&str> {
    config.web_acl_id().filter(|value| !value.trim().is_empty())
}

fn validate_web_acl_id(value: Option<&str>) -> CloudFrontApiResult<()> {
    if value.is_some_and(|value| {
        value.is_empty()
            || value.len() > 2_048
            || value.trim() != value
            || value.chars().any(char::is_control)
    }) {
        Err(validation("invalid_cloudfront_web_acl_id"))
    } else {
        Ok(())
    }
}

fn assert_supported_minimal_update_shape(
    config: &aws_sdk_cloudfront::types::DistributionConfig,
) -> CloudFrontApiResult<()> {
    let origins = config
        .origins()
        .ok_or_else(|| validation("cloudfront_origins_missing"))?;
    if origins.quantity() != 1 || origins.items().len() != 1 || config.origin_groups().is_some() {
        return Err(validation("cloudfront_single_origin_shape_required"));
    }
    if config
        .cache_behaviors()
        .is_some_and(|behaviors| behaviors.quantity() != 0 || !behaviors.items().is_empty())
        || config
            .aliases()
            .is_some_and(|aliases| aliases.quantity() != 0 || !aliases.items().is_empty())
    {
        return Err(validation("cloudfront_fixed_behavior_shape_required"));
    }
    let origin = origins.items().first().expect("one origin was checked");
    if origin.id() != FIXED_ORIGIN_ID
        || origin.custom_headers().is_some()
        || origin.s3_origin_config().is_some()
        || origin.vpc_origin_config().is_some()
    {
        return Err(validation("cloudfront_supported_origin_shape_required"));
    }
    let custom = origin
        .custom_origin_config()
        .ok_or_else(|| validation("cloudfront_supported_custom_origin_required"))?;
    if custom.origin_protocol_policy()
        != &aws_sdk_cloudfront::types::OriginProtocolPolicy::HttpsOnly
    {
        return Err(validation("cloudfront_https_origin_required"));
    }
    let behavior = config
        .default_cache_behavior()
        .ok_or_else(|| validation("cloudfront_default_behavior_missing"))?;
    if behavior.target_origin_id() != FIXED_ORIGIN_ID
        || behavior.cache_policy_id() != Some(CACHING_DISABLED_POLICY_ID)
        || behavior.origin_request_policy_id() != Some(ALL_VIEWER_EXCEPT_HOST_HEADER_POLICY_ID)
        || behavior.viewer_protocol_policy()
            != &aws_sdk_cloudfront::types::ViewerProtocolPolicy::RedirectToHttps
    {
        return Err(validation("cloudfront_fixed_behavior_shape_required"));
    }
    Ok(())
}

fn single_supported_origin_mut(
    config: &mut aws_sdk_cloudfront::types::DistributionConfig,
) -> CloudFrontApiResult<&mut aws_sdk_cloudfront::types::Origin> {
    assert_supported_minimal_update_shape(config)?;
    Ok(config
        .origins
        .as_mut()
        .expect("validated origins")
        .items
        .first_mut()
        .expect("validated one origin"))
}
fn validate_distribution_id(value: &str) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        Err(validation("invalid_cloudfront_distribution_id"))
    } else {
        Ok(())
    }
}
fn partition_from_identity_arn(arn: &str, account: &str) -> CloudFrontApiResult<AwsPartition> {
    if !arn.contains(&format!(":{account}:")) {
        return Err(validation("sts_identity_account_mismatch"));
    };
    match arn.split(':').nth(1) {
        Some("aws") => Ok(AwsPartition::Aws),
        Some("aws-cn") => Ok(AwsPartition::AwsChina),
        Some("aws-us-gov") => Ok(AwsPartition::AwsUsGov),
        _ => Err(validation("unsupported_aws_partition")),
    }
}
fn validate_endpoint(endpoint: Option<&str>) -> CloudFrontApiResult<()> {
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    let parsed = url::Url::parse(endpoint).map_err(|_| validation("invalid_aws_endpoint_url"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| validation("invalid_aws_endpoint_url"))?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !matches!(parsed.scheme(), "http" | "https")
        || !(host.eq_ignore_ascii_case("localhost")
            || IpAddr::from_str(host.trim_start_matches('[').trim_end_matches(']'))
                .is_ok_and(|address| address.is_loopback()))
    {
        return Err(validation("untrusted_aws_endpoint_url"));
    };
    Ok(())
}
fn service_code<E, R>(error: &aws_sdk_cloudfront::error::SdkError<E, R>) -> Option<&str>
where
    E: ProvideErrorMetadata,
{
    error
        .as_service_error()
        .and_then(ProvideErrorMetadata::code)
}
fn map_error(code: Option<&str>) -> NormalizedProviderError {
    let category = match code {
        Some("AccessDenied" | "AccessDeniedException") => ProviderErrorCategory::Authorization,
        Some("Throttling" | "ThrottlingException") => ProviderErrorCategory::Throttled,
        Some("NoSuchDistribution" | "NoSuchResource") => ProviderErrorCategory::NotFound,
        Some("InvalidArgument" | "ValidationError") => ProviderErrorCategory::Validation,
        _ => ProviderErrorCategory::Transient,
    };
    NormalizedProviderError::new(
        category,
        "cloudfront_service_error",
        "AWS provider request failed",
        (category == ProviderErrorCategory::Throttled).then_some(DEFAULT_THROTTLE_RETRY_AFTER_MS),
        None,
    )
    .expect("static normalized provider error")
}

fn map_mutation_error(code: Option<&str>) -> NormalizedProviderError {
    match code {
        Some("PreconditionFailed") => NormalizedProviderError::new(
            ProviderErrorCategory::Conflict,
            "cloudfront_etag_conflict",
            "CloudFront configuration changed; read fresh state before retrying",
            None,
            None,
        )
        .expect("static normalized provider error"),
        Some("NoSuchDistribution") => NormalizedProviderError::new(
            ProviderErrorCategory::NotFound,
            "cloudfront_distribution_not_found",
            "CloudFront distribution does not exist",
            None,
            None,
        )
        .expect("static normalized provider error"),
        Some("DistributionNotDisabled" | "InvalidIfMatchVersion") => NormalizedProviderError::new(
            ProviderErrorCategory::Conflict,
            "cloudfront_mutation_conflict",
            "CloudFront lifecycle guard rejected the mutation",
            None,
            None,
        )
        .expect("static normalized provider error"),
        Some(
            "InvalidArgument" | "ValidationError" | "IllegalUpdate" | "InvalidDefaultRootObject",
        ) => map_error(code),
        Some("AccessDenied" | "AccessDeniedException" | "Throttling" | "ThrottlingException") => {
            map_error(code)
        }
        _ => unknown_mutation_outcome(),
    }
}

fn unknown_mutation_outcome() -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::UnknownOutcome,
        "cloudfront_mutation_unknown_outcome",
        "CloudFront mutation outcome is unknown; observe provider state before retrying",
        None,
        None,
    )
    .expect("static normalized provider error")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CloudFrontCreateDistribution {
        CloudFrontCreateDistribution {
            caller_reference: "cld-28f-unit-test".to_string(),
            origin: CloudFrontHttpsOrigin {
                domain_name: "api.example.com".to_string(),
                https_port: 8443,
            },
        }
    }

    #[test]
    fn minimal_create_shape_is_fixed_and_https_only() {
        let config = minimal_distribution_config(&request()).expect("fixed config");
        assert!(config.enabled);
        assert!(config.origin_groups.is_none());
        assert_eq!(
            config
                .cache_behaviors
                .as_ref()
                .map(|value| value.quantity()),
            None
        );
        let origin = config
            .origins
            .as_ref()
            .expect("origins")
            .items
            .first()
            .expect("one origin");
        assert_eq!(origin.id(), FIXED_ORIGIN_ID);
        assert_eq!(origin.domain_name(), "api.example.com");
        let custom = origin.custom_origin_config().expect("custom origin");
        assert_eq!(custom.https_port(), 8443);
        assert_eq!(
            custom.origin_protocol_policy(),
            &aws_sdk_cloudfront::types::OriginProtocolPolicy::HttpsOnly
        );
        let behavior = config.default_cache_behavior().expect("behavior");
        assert_eq!(behavior.cache_policy_id(), Some(CACHING_DISABLED_POLICY_ID));
        assert_eq!(
            behavior.origin_request_policy_id(),
            Some(ALL_VIEWER_EXCEPT_HOST_HEADER_POLICY_ID)
        );
    }

    #[test]
    fn throttling_is_normalized_without_panicking_and_mutations_are_single_attempt() {
        let error = map_error(Some("ThrottlingException"));
        assert_eq!(error.category(), ProviderErrorCategory::Throttled);
        assert_eq!(
            error.retry_after_ms(),
            Some(DEFAULT_THROTTLE_RETRY_AFTER_MS)
        );
        let mutation = map_mutation_error(Some("Throttling"));
        assert_eq!(mutation.category(), ProviderErrorCategory::Throttled);
        assert_eq!(MUTATION_MAX_ATTEMPTS, 1);
    }

    #[test]
    fn endpoint_overlay_changes_only_the_single_supported_origin() {
        let mut config = minimal_distribution_config(&request()).expect("fixed config");
        let origin = single_supported_origin_mut(&mut config).expect("supported origin");
        origin.domain_name = "api-next.example.com".to_string();
        origin
            .custom_origin_config
            .as_mut()
            .expect("custom origin")
            .https_port = 9443;
        assert_supported_minimal_update_shape(&config).expect("fixed shape remains supported");
        assert_eq!(
            config.origins().expect("origins").items()[0].domain_name(),
            "api-next.example.com"
        );
    }

    #[test]
    fn detail_projects_only_the_fixed_https_origin_and_safe_web_acl() {
        let mut config = minimal_distribution_config(&request()).expect("fixed config");
        config.web_acl_id =
            Some("arn:aws:wafv2:us-east-1:123456789012:global/webacl/api/abc".into());
        assert_eq!(
            project_supported_origin(&config),
            Some(CloudFrontHttpsOrigin {
                domain_name: "api.example.com".to_string(),
                https_port: 8443,
            })
        );
        assert_eq!(
            project_web_acl_id(&config).as_deref(),
            Some("arn:aws:wafv2:us-east-1:123456789012:global/webacl/api/abc")
        );
        assert!(has_web_acl_association(&config));

        config
            .origins
            .as_mut()
            .expect("origins")
            .items
            .first_mut()
            .expect("origin")
            .custom_origin_config
            .as_mut()
            .expect("custom origin")
            .https_port = 0;
        assert_eq!(project_supported_origin(&config), None);
    }

    #[test]
    fn delete_reference_guard_detects_even_unprojectable_web_acl() {
        let mut config = minimal_distribution_config(&request()).expect("fixed config");
        config.web_acl_id = Some("  \u{0007}  ".into());
        assert_eq!(project_web_acl_id(&config), None);
        assert!(has_web_acl_association(&config));
    }

    #[test]
    fn web_acl_overlay_changes_only_the_root_association() {
        let current = minimal_distribution_config(&request()).expect("fixed config");
        let mut desired = current.clone();
        desired.web_acl_id =
            Some("arn:aws:wafv2:us-east-1:123456789012:global/webacl/api/abc".into());
        assert_supported_minimal_update_shape(&desired).expect("web ACL preserves fixed shape");
        assert_eq!(normalized_web_acl_id(&current), None);
        assert_eq!(
            normalized_web_acl_id(&desired),
            Some("arn:aws:wafv2:us-east-1:123456789012:global/webacl/api/abc")
        );
        assert!(validate_web_acl_id(normalized_web_acl_id(&desired)).is_ok());
        assert!(validate_web_acl_id(Some(" ")).is_err());

        // Attach, replace, detach, then repeat detach as the observable no-op state.
        desired.web_acl_id =
            Some("arn:aws:wafv2:us-east-1:123456789012:global/webacl/api/next".into());
        assert_eq!(
            normalized_web_acl_id(&desired),
            Some("arn:aws:wafv2:us-east-1:123456789012:global/webacl/api/next")
        );
        desired.web_acl_id = None;
        assert_eq!(normalized_web_acl_id(&desired), None);
        assert_eq!(normalized_web_acl_id(&desired), None);
    }

    #[test]
    fn ambiguous_mutation_failure_is_unknown_outcome() {
        assert_eq!(
            map_mutation_error(Some("InternalError")).category(),
            ProviderErrorCategory::UnknownOutcome
        );
        assert_eq!(
            map_mutation_error(Some("PreconditionFailed")).category(),
            ProviderErrorCategory::Conflict
        );
    }
}
