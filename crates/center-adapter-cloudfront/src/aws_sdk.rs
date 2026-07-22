use std::{net::IpAddr, str::FromStr};

use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_cloudfront::error::ProvideErrorMetadata;
use edgion_center_core::{NormalizedProviderError, ProviderErrorCategory};

use crate::{
    is_aws_account_id, validation, AwsPartition, CloudFrontApi, CloudFrontApiResult,
    CloudFrontDistributionDetail, CloudFrontDistributionPage, CloudFrontDistributionSummary,
    CloudFrontFingerprintKey, CloudFrontTags,
};

/// Loopback-only endpoint overrides are retained solely for hermetic inventory tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AwsCloudFrontApiOptions {
    pub cloudfront_endpoint_url: Option<String>,
    pub sts_endpoint_url: Option<String>,
}

/// Credential-owning, read-only AWS transport. It has no distribution writer or planner.
pub struct AwsCloudFrontApi {
    account_id: String,
    partition: AwsPartition,
    credential_revision: String,
    read_client: aws_sdk_cloudfront::Client,
    fingerprint_key: CloudFrontFingerprintKey,
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
        let mut sts = aws_sdk_sts::config::Builder::from(config);
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
        let mut cloudfront = aws_sdk_cloudfront::config::Builder::from(config);
        if let Some(endpoint) = options.cloudfront_endpoint_url {
            cloudfront = cloudfront.endpoint_url(endpoint);
        }
        Ok(Self {
            account_id,
            partition,
            credential_revision,
            read_client: aws_sdk_cloudfront::Client::from_conf(cloudfront.build()),
            fingerprint_key,
        })
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
        None,
        None,
    )
    .expect("static normalized provider error")
}
