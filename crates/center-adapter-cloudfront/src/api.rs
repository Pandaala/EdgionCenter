use std::collections::BTreeSet;

use async_trait::async_trait;
use edgion_center_core::{NormalizedProviderError, ProviderErrorCategory};
use serde::{Deserialize, Serialize};

use crate::CloudFrontEtagRevisionMac;

pub type CloudFrontApiResult<T> = Result<T, NormalizedProviderError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsPartition {
    Aws,
    AwsChina,
    AwsUsGov,
}

impl AwsPartition {
    pub fn arn_partition(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::AwsChina => "aws-cn",
            Self::AwsUsGov => "aws-us-gov",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDistributionSummary {
    pub id: String,
    pub arn: String,
    pub domain_name: String,
    pub status: String,
    pub enabled: bool,
    pub last_modified_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFrontDistributionPage {
    pub items: Vec<CloudFrontDistributionSummary>,
    pub is_truncated: bool,
    pub next_marker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontPolicyKind {
    Cache,
    OriginRequest,
    ResponseHeaders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontPolicyScope {
    AwsManaged,
    AccountCustom,
}

/// A policy revision observed through a scope-filtered list followed by an exact get.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontPolicySummary {
    pub id: String,
    pub name: String,
    pub kind: CloudFrontPolicyKind,
    pub scope: CloudFrontPolicyScope,
    pub etag: String,
    pub last_modified_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFrontPolicyPage {
    pub items: Vec<CloudFrontPolicySummary>,
    pub next_marker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontInvalidationStatus {
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontInvalidationSummary {
    pub distribution_id: String,
    pub id: String,
    pub status: CloudFrontInvalidationStatus,
    pub created_at_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFrontInvalidationPage {
    pub distribution_id: String,
    pub items: Vec<CloudFrontInvalidationSummary>,
    pub is_truncated: bool,
    pub next_marker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontInvalidationDetail {
    pub distribution_id: String,
    pub id: String,
    pub status: CloudFrontInvalidationStatus,
    pub created_at_unix_seconds: i64,
    pub caller_reference: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontOriginKind {
    Custom,
    S3,
    Vpc,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontOriginProjection {
    pub id: String,
    pub domain_name: String,
    pub origin_path: String,
    pub kind: CloudFrontOriginKind,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub protocol_policy: Option<String>,
    #[serde(default)]
    pub tls_protocols: BTreeSet<String>,
    pub connection_attempts: u8,
    pub connection_timeout_seconds: u8,
    pub response_timeout_seconds: Option<u16>,
    pub keepalive_timeout_seconds: Option<u16>,
    /// Names and values are both redacted because either can be an origin credential.
    pub custom_header_count: u8,
    #[serde(default)]
    pub unsupported_features: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontOriginGroupProjection {
    pub id: String,
    pub primary_origin_id: String,
    pub secondary_origin_id: String,
    #[serde(default)]
    pub failover_status_codes: BTreeSet<u16>,
    #[serde(default)]
    pub unsupported_features: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontCacheBehaviorProjection {
    pub path_pattern: Option<String>,
    pub target_origin_id: String,
    pub viewer_protocol_policy: String,
    #[serde(default)]
    pub allowed_methods: BTreeSet<String>,
    #[serde(default)]
    pub cached_methods: BTreeSet<String>,
    pub compress: bool,
    pub cache_policy_id: Option<String>,
    pub origin_request_policy_id: Option<String>,
    pub response_headers_policy_id: Option<String>,
    pub field_level_encryption_id: Option<String>,
    pub realtime_log_config_arn: Option<String>,
    #[serde(default)]
    pub unsupported_features: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontCustomErrorResponseProjection {
    pub error_code: u16,
    pub response_page_path: Option<String>,
    pub response_code: Option<u16>,
    pub minimum_ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontLoggingProjection {
    pub enabled: bool,
    pub include_cookies: bool,
    pub bucket: String,
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontViewerCertificateProjection {
    pub cloudfront_default_certificate: bool,
    pub certificate_arn: Option<String>,
    pub certificate_source: Option<String>,
    pub ssl_support_method: Option<String>,
    pub minimum_protocol_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontGeoRestrictionProjection {
    pub restriction_type: String,
    #[serde(default)]
    pub locations: BTreeSet<String>,
}

/// Sanitized provider configuration. Secret custom-header values never cross the API seam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDistributionConfigProjection {
    pub caller_reference: String,
    #[serde(default)]
    pub aliases: BTreeSet<String>,
    pub default_root_object: String,
    #[serde(default)]
    pub origins: Vec<CloudFrontOriginProjection>,
    #[serde(default)]
    pub origin_groups: Vec<CloudFrontOriginGroupProjection>,
    pub default_cache_behavior: CloudFrontCacheBehaviorProjection,
    #[serde(default)]
    pub ordered_cache_behaviors: Vec<CloudFrontCacheBehaviorProjection>,
    #[serde(default)]
    pub custom_error_responses: Vec<CloudFrontCustomErrorResponseProjection>,
    pub comment: String,
    pub logging: CloudFrontLoggingProjection,
    pub price_class: String,
    pub enabled: bool,
    pub viewer_certificate: CloudFrontViewerCertificateProjection,
    pub geo_restriction: CloudFrontGeoRestrictionProjection,
    pub web_acl_id: String,
    pub http_version: String,
    pub ipv6_enabled: bool,
    pub staging: bool,
    pub continuous_deployment_policy_id: String,
    /// Stable feature codes identifying fields not safe for later whole-config replacement.
    #[serde(default)]
    pub unsupported_features: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDistributionDetail {
    pub summary: CloudFrontDistributionSummary,
    pub etag: String,
    /// Keyed, scope-bound MAC of the opaque provider ETag revision; not a content hash.
    pub etag_revision_mac: CloudFrontEtagRevisionMac,
    pub config: CloudFrontDistributionConfigProjection,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudFrontTags {
    /// Tag keys are retained for inventory diagnostics; arbitrary values are omitted.
    pub keys: BTreeSet<String>,
    /// Value of the reserved `edgion.center/resource-id` tag only.
    pub center_resource_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcmCertificateStatus {
    PendingValidation,
    Issued,
    Inactive,
    Expired,
    ValidationTimedOut,
    Revoked,
    Failed,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcmCertificateType {
    Imported,
    AmazonIssued,
    Private,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcmCertificateKeyAlgorithm {
    Rsa1024,
    Rsa2048,
    Rsa3072,
    Rsa4096,
    EcPrime256v1,
    Unsupported(String),
}

/// Exact, read-only observation of an existing ACM viewer certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcmCertificateObservation {
    pub arn: String,
    pub account_id: String,
    pub partition: AwsPartition,
    pub region: String,
    pub domain_name: String,
    #[serde(default)]
    pub subject_alternative_names: BTreeSet<String>,
    pub status: AcmCertificateStatus,
    pub certificate_type: AcmCertificateType,
    pub key_algorithm: AcmCertificateKeyAlgorithm,
    pub managed_by: Option<String>,
    pub not_before_unix_seconds: Option<i64>,
    pub not_after_unix_seconds: Option<i64>,
    #[serde(default)]
    pub in_use_by: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CloudFrontDomainConflictResourceType {
    Distribution,
    DistributionTenant,
    Unknown(String),
}

/// Sanitized item returned by CloudFront's read-only domain-conflict lookup.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CloudFrontDomainConflict {
    pub domain: String,
    pub resource_type: CloudFrontDomainConflictResourceType,
    pub resource_id: String,
    pub account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFrontDomainConflictPage {
    pub queried_domain: String,
    pub validation_distribution_id: String,
    pub items: Vec<CloudFrontDomainConflict>,
    pub next_marker: Option<String>,
}

/// Read-only CloudFront transport seam. Mutation methods intentionally do not exist.
#[async_trait]
pub trait CloudFrontApi: Send + Sync {
    fn verified_account_id(&self) -> &str;

    fn verified_partition(&self) -> AwsPartition;

    /// Revision of the resolved credential authority that constructed this transport.
    fn credential_revision(&self) -> &str;

    async fn list_distributions(
        &self,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontDistributionPage>;

    async fn get_distribution(
        &self,
        distribution_id: &str,
    ) -> CloudFrontApiResult<Option<CloudFrontDistributionDetail>>;

    async fn describe_acm_certificate(
        &self,
        _certificate_arn: &str,
    ) -> CloudFrontApiResult<Option<AcmCertificateObservation>> {
        Err(acm_read_not_implemented())
    }

    async fn list_domain_conflicts(
        &self,
        _domain: &str,
        _validation_distribution_id: &str,
        _marker: Option<&str>,
        _max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontDomainConflictPage> {
        Err(domain_conflict_read_not_implemented())
    }

    /// Lists one policy kind and scope. Every item is revision-verified with an exact read.
    async fn list_policies(
        &self,
        kind: CloudFrontPolicyKind,
        scope: CloudFrontPolicyScope,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontPolicyPage>;

    async fn list_invalidations(
        &self,
        _distribution_id: &str,
        _marker: Option<&str>,
        _max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontInvalidationPage> {
        Err(invalidation_read_not_implemented())
    }

    async fn get_invalidation(
        &self,
        _distribution_id: &str,
        _invalidation_id: &str,
    ) -> CloudFrontApiResult<Option<CloudFrontInvalidationDetail>> {
        Err(invalidation_read_not_implemented())
    }

    async fn list_tags_for_resource(&self, arn: &str) -> CloudFrontApiResult<CloudFrontTags>;
}

fn acm_read_not_implemented() -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::Validation,
        "acm_certificate_read_not_implemented",
        "ACM certificate observation is not implemented by this transport",
        None,
        None,
    )
    .expect("static normalized provider error")
}

fn domain_conflict_read_not_implemented() -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::Validation,
        "cloudfront_domain_conflict_read_not_implemented",
        "CloudFront domain-conflict observation is not implemented by this transport",
        None,
        None,
    )
    .expect("static normalized provider error")
}

fn invalidation_read_not_implemented() -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::Validation,
        "cloudfront_invalidation_read_not_implemented",
        "CloudFront invalidation observation is not implemented by this transport",
        None,
        None,
    )
    .expect("static normalized provider error")
}
