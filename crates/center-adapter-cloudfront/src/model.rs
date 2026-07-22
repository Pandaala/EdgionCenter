use edgion_center_core::{
    CloudResourceId, DomainName, NormalizedProviderError, ProviderErrorCategory,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroize;

use crate::{
    AwsPartition, CloudFrontApiResult, CloudFrontDistributionDetail, CloudFrontDistributionSummary,
};

type HmacSha256 = Hmac<Sha256>;

pub struct CloudFrontFingerprintKey([u8; 32]);
impl CloudFrontFingerprintKey {
    pub fn new(value: [u8; 32]) -> CloudFrontApiResult<Self> {
        if value.iter().all(|byte| *byte == 0) {
            return Err(validation("weak_cloudfront_fingerprint_key"));
        }
        Ok(Self(value))
    }
    pub(crate) fn mac_etag_revision(
        &self,
        partition: AwsPartition,
        account: &str,
        distribution: &str,
        etag: &str,
    ) -> CloudFrontApiResult<CloudFrontEtagRevisionMac> {
        let mut mac = HmacSha256::new_from_slice(&self.0)
            .map_err(|_| validation("invalid_cloudfront_fingerprint_key"))?;
        for value in [partition.arn_partition(), account, distribution, etag] {
            mac.update(&(value.len() as u64).to_be_bytes());
            mac.update(value.as_bytes());
        }
        Ok(CloudFrontEtagRevisionMac(
            mac.finalize()
                .into_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        ))
    }
}
impl Drop for CloudFrontFingerprintKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CloudFrontEtagRevisionMac(String);
impl CloudFrontEtagRevisionMac {
    pub(crate) fn validate(&self) -> CloudFrontApiResult<()> {
        if self.0.len() != 64
            || !self
                .0
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(validation("invalid_cloudfront_config_fingerprint"));
        }
        Ok(())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontObservationAuthority {
    provider_account_id: CloudResourceId,
    aws_account_id: String,
    partition: AwsPartition,
    account_generation: u64,
    credential_revision: String,
    observation_token: String,
    observed_at_unix_ms: i64,
    valid_until_unix_ms: i64,
}
impl CloudFrontObservationAuthority {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        provider_account_id: CloudResourceId,
        aws_account_id: String,
        partition: AwsPartition,
        account_generation: u64,
        credential_revision: String,
        observation_token: String,
        observed_at_unix_ms: i64,
        valid_until_unix_ms: i64,
    ) -> CloudFrontApiResult<Self> {
        provider_account_id
            .validate()
            .map_err(|_| validation("invalid_cloudfront_provider_account_id"))?;
        if !is_aws_account_id(&aws_account_id)
            || account_generation == 0
            || credential_revision.is_empty()
            || credential_revision.len() > 512
            || observation_token.is_empty()
            || observation_token.len() > 512
            || observed_at_unix_ms <= 0
            || valid_until_unix_ms <= observed_at_unix_ms
        {
            return Err(validation("invalid_cloudfront_observation_authority"));
        }
        Ok(Self {
            provider_account_id,
            aws_account_id,
            partition,
            account_generation,
            credential_revision,
            observation_token,
            observed_at_unix_ms,
            valid_until_unix_ms,
        })
    }
    pub(crate) fn validate(&self) -> CloudFrontApiResult<()> {
        Self::new(
            self.provider_account_id.clone(),
            self.aws_account_id.clone(),
            self.partition,
            self.account_generation,
            self.credential_revision.clone(),
            self.observation_token.clone(),
            self.observed_at_unix_ms,
            self.valid_until_unix_ms,
        )
        .map(|_| ())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontDetailIssueKind {
    Missing,
    Inaccessible,
    Unavailable,
    Malformed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudFrontDetailIssue {
    pub kind: CloudFrontDetailIssueKind,
    pub code: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum CloudFrontDetailObservation {
    Complete(Box<CloudFrontDistributionDetail>),
    Partial(CloudFrontDetailIssue),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CloudFrontOwnershipHint {
    Present { center_resource_id: CloudResourceId },
    Absent,
    Unknown { code: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudFrontInventoryEntry {
    pub summary: CloudFrontDistributionSummary,
    pub detail: CloudFrontDetailObservation,
    pub ownership_hint: CloudFrontOwnershipHint,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudFrontInventory {
    pub authority: CloudFrontObservationAuthority,
    pub entries: Vec<CloudFrontInventoryEntry>,
}

pub(crate) fn validate_summary(
    summary: &CloudFrontDistributionSummary,
    partition: AwsPartition,
    account: &str,
) -> CloudFrontApiResult<()> {
    if summary.id.is_empty()
        || summary.id.len() > 128
        || !summary
            .id
            .bytes()
            .all(|value| value.is_ascii_uppercase() || value.is_ascii_digit())
    {
        return Err(validation("invalid_cloudfront_distribution_id"));
    }
    if summary.arn
        != format!(
            "arn:{}:cloudfront::{account}:distribution/{}",
            partition.arn_partition(),
            summary.id
        )
    {
        return Err(validation("cloudfront_distribution_arn_scope_mismatch"));
    }
    DomainName::new(summary.domain_name.clone())
        .map_err(|_| validation("invalid_cloudfront_distribution_domain"))?;
    if summary.status.is_empty()
        || summary.status.len() > 128
        || summary.status.chars().any(char::is_control)
        || summary.last_modified_unix_seconds <= 0
    {
        return Err(validation("invalid_cloudfront_distribution_summary"));
    }
    Ok(())
}
pub(crate) fn validate_detail(
    detail: &CloudFrontDistributionDetail,
    partition: AwsPartition,
    account: &str,
) -> CloudFrontApiResult<()> {
    validate_summary(&detail.summary, partition, account)?;
    if detail.etag.is_empty()
        || detail.etag.len() > 512
        || detail.etag.chars().any(char::is_control)
    {
        return Err(validation("invalid_cloudfront_distribution_etag"));
    };
    detail.etag_revision_mac.validate()
}
pub(crate) fn detail_issue(
    kind: CloudFrontDetailIssueKind,
    code: impl Into<String>,
) -> CloudFrontDetailObservation {
    CloudFrontDetailObservation::Partial(CloudFrontDetailIssue {
        kind,
        code: code.into(),
    })
}
pub(crate) fn classify_detail_error(error: &NormalizedProviderError) -> CloudFrontDetailIssueKind {
    match error.category() {
        ProviderErrorCategory::NotFound => CloudFrontDetailIssueKind::Missing,
        ProviderErrorCategory::Authentication | ProviderErrorCategory::Authorization => {
            CloudFrontDetailIssueKind::Inaccessible
        }
        ProviderErrorCategory::Transient | ProviderErrorCategory::Throttled => {
            CloudFrontDetailIssueKind::Unavailable
        }
        _ => CloudFrontDetailIssueKind::Malformed,
    }
}
pub(crate) fn is_aws_account_id(value: &str) -> bool {
    value.len() == 12 && value.bytes().all(|byte| byte.is_ascii_digit())
}
pub(crate) fn validation(code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::Validation,
        code,
        "CloudFront adapter validation failed",
        None,
        None,
    )
    .expect("static normalized provider error")
}
