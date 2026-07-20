use std::collections::BTreeSet;

use edgion_center_core::{
    CloudResourceId, DomainName, NormalizedProviderError, ProviderErrorCategory,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroize;

use crate::{
    AwsPartition, CloudFrontApiResult, CloudFrontDistributionConfigProjection,
    CloudFrontDistributionDetail, CloudFrontDistributionSummary, CloudFrontOriginKind,
};

const MAX_CREDENTIAL_REVISION_LEN: usize = 512;
const MAX_OBSERVATION_TOKEN_LEN: usize = 512;
const MAX_ETAG_LEN: usize = 512;
const MAX_PROVIDER_IDENTIFIER_LEN: usize = 512;
const MAX_CONFIG_BYTES: usize = 2 * 1024 * 1024;
const MAX_CONFIG_ITEMS: usize = 10_000;
const MAX_FEATURE_CODE_LEN: usize = 128;
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
        aws_account_id: &str,
        distribution_id: &str,
        etag: &str,
    ) -> CloudFrontApiResult<CloudFrontEtagRevisionMac> {
        let mut mac = HmacSha256::new_from_slice(&self.0)
            .map_err(|_| validation("invalid_cloudfront_fingerprint_key"))?;
        for component in [
            partition.arn_partition(),
            aws_account_id,
            distribution_id,
            etag,
        ] {
            mac.update(&(component.len() as u64).to_be_bytes());
            mac.update(component.as_bytes());
        }
        let digest = mac.finalize().into_bytes();
        Ok(CloudFrontEtagRevisionMac(
            digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        ))
    }

    #[allow(clippy::too_many_arguments)] // Every logical and provider scope fence is MAC-bound.
    pub(crate) fn mac_enablement_intent(
        &self,
        provider_account_id: &CloudResourceId,
        account_generation: u64,
        credential_revision: &str,
        partition: AwsPartition,
        aws_account_id: &str,
        distribution_id: &str,
        etag: &str,
        desired_enabled: bool,
    ) -> CloudFrontApiResult<CloudFrontMutationIntentMac> {
        let mut mac = HmacSha256::new_from_slice(&self.0)
            .map_err(|_| validation("invalid_cloudfront_fingerprint_key"))?;
        for component in [
            "cloudfront_enablement_intent_v1",
            provider_account_id.as_str(),
            credential_revision,
            partition.arn_partition(),
            aws_account_id,
            distribution_id,
            etag,
            if desired_enabled {
                "enabled"
            } else {
                "disabled"
            },
        ] {
            mac.update(&(component.len() as u64).to_be_bytes());
            mac.update(component.as_bytes());
        }
        mac.update(&account_generation.to_be_bytes());
        let digest = mac.finalize().into_bytes();
        Ok(CloudFrontMutationIntentMac(
            digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        ))
    }

    #[allow(clippy::too_many_arguments)] // The exact wire revision binds every execution fence.
    pub(crate) fn mac_desired_wire_revision(
        &self,
        provider_account_id: &CloudResourceId,
        account_generation: u64,
        credential_revision: &str,
        partition: AwsPartition,
        aws_account_id: &str,
        distribution_id: &str,
        etag: &str,
        desired_wire: &[u8],
    ) -> CloudFrontApiResult<CloudFrontDesiredWireRevisionMac> {
        let mut mac = HmacSha256::new_from_slice(&self.0)
            .map_err(|_| validation("invalid_cloudfront_fingerprint_key"))?;
        for component in [
            "cloudfront_desired_wire_revision_v1",
            aws_sdk_cloudfront::meta::PKG_VERSION,
            "cloudfront_api_2020-05-31",
            "ordered_xml_comparator_v1",
            provider_account_id.as_str(),
            credential_revision,
            partition.arn_partition(),
            aws_account_id,
            distribution_id,
            etag,
        ] {
            mac.update(&(component.len() as u64).to_be_bytes());
            mac.update(component.as_bytes());
        }
        mac.update(&account_generation.to_be_bytes());
        mac.update(&(desired_wire.len() as u64).to_be_bytes());
        mac.update(desired_wire);
        let digest = mac.finalize().into_bytes();
        Ok(CloudFrontDesiredWireRevisionMac(
            digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        ))
    }

    #[allow(clippy::too_many_arguments)] // Every immutable plan dimension is MAC-bound.
    pub(crate) fn mac_enablement_plan_revision(
        &self,
        provider_account_id: &CloudResourceId,
        account_generation: u64,
        credential_revision: &str,
        partition: AwsPartition,
        aws_account_id: &str,
        distribution_id: &str,
        distribution_arn: &str,
        valid_until_unix_ms: i64,
        action: &str,
        risk: &str,
        intent_revision: &CloudFrontMutationIntentMac,
        desired_wire_revision: &CloudFrontDesiredWireRevisionMac,
        write_set: &BTreeSet<String>,
        blockers: &BTreeSet<String>,
    ) -> CloudFrontApiResult<CloudFrontEnablementPlanRevisionMac> {
        let mut mac = HmacSha256::new_from_slice(&self.0)
            .map_err(|_| validation("invalid_cloudfront_fingerprint_key"))?;
        for component in [
            "cloudfront_enablement_plan_v1",
            provider_account_id.as_str(),
            credential_revision,
            partition.arn_partition(),
            aws_account_id,
            distribution_id,
            distribution_arn,
            action,
            risk,
            intent_revision.as_str(),
            desired_wire_revision.as_str(),
        ] {
            mac.update(&(component.len() as u64).to_be_bytes());
            mac.update(component.as_bytes());
        }
        mac.update(&account_generation.to_be_bytes());
        mac.update(&valid_until_unix_ms.to_be_bytes());
        for values in [write_set, blockers] {
            mac.update(&(values.len() as u64).to_be_bytes());
            for value in values {
                mac.update(&(value.len() as u64).to_be_bytes());
                mac.update(value.as_bytes());
            }
        }
        let digest = mac.finalize().into_bytes();
        Ok(CloudFrontEnablementPlanRevisionMac(
            digest.iter().map(|byte| format!("{byte:02x}")).collect(),
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

/// Scope-bound identifier for a mutation intent. It is not mutation authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CloudFrontMutationIntentMac(String);

impl CloudFrontMutationIntentMac {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn test_value(value: char) -> Self {
        Self(value.to_string().repeat(64))
    }
}

/// Keyed revision of the exact desired XML produced by the pinned SDK after wire admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CloudFrontDesiredWireRevisionMac(String);

impl CloudFrontDesiredWireRevisionMac {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn test_value(value: char) -> Self {
        Self(value.to_string().repeat(64))
    }
}

/// Versioned keyed revision of one complete, immutable enablement preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CloudFrontEnablementPlanRevisionMac(String);

impl CloudFrontEnablementPlanRevisionMac {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn test_value(value: char) -> Self {
        Self(value.to_string().repeat(64))
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
    #[allow(clippy::too_many_arguments)] // Every authority dimension is explicit and validated here.
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
        if !is_aws_account_id(&aws_account_id) {
            return Err(validation("invalid_cloudfront_aws_account_id"));
        }
        if account_generation == 0 {
            return Err(validation("invalid_cloudfront_account_generation"));
        }
        validate_bounded_text(
            &credential_revision,
            MAX_CREDENTIAL_REVISION_LEN,
            "invalid_cloudfront_credential_revision",
        )?;
        validate_bounded_text(
            &observation_token,
            MAX_OBSERVATION_TOKEN_LEN,
            "invalid_cloudfront_observation_token",
        )?;
        if observed_at_unix_ms <= 0 || valid_until_unix_ms <= observed_at_unix_ms {
            return Err(validation("invalid_cloudfront_observation_freshness"));
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

    pub fn validate(&self) -> CloudFrontApiResult<()> {
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

    pub fn provider_account_id(&self) -> &CloudResourceId {
        &self.provider_account_id
    }

    pub fn aws_account_id(&self) -> &str {
        &self.aws_account_id
    }

    pub fn partition(&self) -> AwsPartition {
        self.partition
    }

    pub fn account_generation(&self) -> u64 {
        self.account_generation
    }

    pub fn credential_revision(&self) -> &str {
        &self.credential_revision
    }

    pub fn observation_token(&self) -> &str {
        &self.observation_token
    }

    pub fn observed_at_unix_ms(&self) -> i64 {
        self.observed_at_unix_ms
    }

    pub fn valid_until_unix_ms(&self) -> i64 {
        self.valid_until_unix_ms
    }

    pub fn is_fresh_at(&self, now_unix_ms: i64) -> bool {
        now_unix_ms >= self.observed_at_unix_ms && now_unix_ms < self.valid_until_unix_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontMutationEligibility {
    Eligible,
    Ineligible { reasons: BTreeSet<String> },
}

impl CloudFrontMutationEligibility {
    pub fn is_eligible(&self) -> bool {
        matches!(self, Self::Eligible)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedCloudFrontDistributionDetail {
    pub detail: CloudFrontDistributionDetail,
    pub mutation_eligibility: CloudFrontMutationEligibility,
    /// List and detail observations remain separate even when their mutable fields changed.
    pub changed_since_summary: bool,
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
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDetailIssue {
    pub kind: CloudFrontDetailIssueKind,
    /// Stable sanitized adapter/provider error code; raw provider text is never retained.
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum CloudFrontDetailObservation {
    Complete(Box<ObservedCloudFrontDistributionDetail>),
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
#[serde(rename_all = "camelCase")]
pub struct CloudFrontInventoryEntry {
    pub summary: CloudFrontDistributionSummary,
    pub detail: CloudFrontDetailObservation,
    /// This is an inventory hint only and never grants mutation authority.
    pub ownership_hint: CloudFrontOwnershipHint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontInventory {
    pub authority: CloudFrontObservationAuthority,
    pub entries: Vec<CloudFrontInventoryEntry>,
}

pub(crate) fn validate_summary(
    summary: &CloudFrontDistributionSummary,
    partition: AwsPartition,
    account_id: &str,
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
    let expected_arn = format!(
        "arn:{}:cloudfront::{account_id}:distribution/{}",
        partition.arn_partition(),
        summary.id
    );
    if summary.arn != expected_arn {
        return Err(validation("cloudfront_distribution_arn_scope_mismatch"));
    }
    DomainName::new(summary.domain_name.clone())
        .map_err(|_| validation("invalid_cloudfront_distribution_domain"))?;
    validate_bounded_text(
        &summary.status,
        128,
        "invalid_cloudfront_distribution_status",
    )?;
    if summary.last_modified_unix_seconds <= 0 {
        return Err(validation("invalid_cloudfront_last_modified_time"));
    }
    Ok(())
}

pub(crate) fn validate_detail(
    detail: &CloudFrontDistributionDetail,
    partition: AwsPartition,
    account_id: &str,
) -> CloudFrontApiResult<()> {
    validate_summary(&detail.summary, partition, account_id)?;
    validate_bounded_text(
        &detail.etag,
        MAX_ETAG_LEN,
        "invalid_cloudfront_distribution_etag",
    )?;
    detail.etag_revision_mac.validate()?;
    if detail.config.enabled != detail.summary.enabled {
        return Err(validation("cloudfront_distribution_enabled_mismatch"));
    }
    validate_config(&detail.config)
}

fn validate_config(config: &CloudFrontDistributionConfigProjection) -> CloudFrontApiResult<()> {
    let encoded = serde_json::to_vec(config)
        .map_err(|_| validation("cloudfront_config_serialization_failed"))?;
    if encoded.len() > MAX_CONFIG_BYTES {
        return Err(validation("cloudfront_config_size_limit"));
    }
    validate_bounded_text(
        &config.caller_reference,
        256,
        "invalid_cloudfront_caller_reference",
    )?;
    validate_bounded_optional_text(&config.comment, 2048, "invalid_cloudfront_comment")?;
    validate_bounded_optional_text(
        &config.default_root_object,
        1024,
        "invalid_cloudfront_default_root_object",
    )?;
    for alias in &config.aliases {
        DomainName::new(alias.clone()).map_err(|_| validation("invalid_cloudfront_alias"))?;
    }
    if config.origins.is_empty()
        || config.origins.len() > MAX_CONFIG_ITEMS
        || config.origin_groups.len() > MAX_CONFIG_ITEMS
        || config.ordered_cache_behaviors.len() > MAX_CONFIG_ITEMS
        || config.custom_error_responses.len() > MAX_CONFIG_ITEMS
    {
        return Err(validation("invalid_cloudfront_config_collection_size"));
    }

    let mut origin_ids = BTreeSet::new();
    for origin in &config.origins {
        validate_bounded_text(
            &origin.id,
            MAX_PROVIDER_IDENTIFIER_LEN,
            "invalid_cloudfront_origin_id",
        )?;
        if !origin_ids.insert(origin.id.clone()) {
            return Err(validation("duplicate_cloudfront_origin_id"));
        }
        DomainName::new(origin.domain_name.clone())
            .map_err(|_| validation("invalid_cloudfront_origin_domain"))?;
        if !(1..=3).contains(&origin.connection_attempts)
            || !(1..=10).contains(&origin.connection_timeout_seconds)
        {
            return Err(validation("invalid_cloudfront_origin_connection_policy"));
        }
        validate_feature_codes(&origin.unsupported_features)?;
        if origin.custom_header_count > 30 {
            return Err(validation("invalid_cloudfront_origin_header_count"));
        }
    }

    let mut all_target_ids = origin_ids.clone();
    let mut origin_group_ids = BTreeSet::new();
    for group in &config.origin_groups {
        validate_bounded_text(
            &group.id,
            MAX_PROVIDER_IDENTIFIER_LEN,
            "invalid_cloudfront_origin_group_id",
        )?;
        if !all_target_ids.insert(group.id.clone()) {
            return Err(validation("duplicate_cloudfront_origin_target_id"));
        }
        origin_group_ids.insert(group.id.clone());
        if group.primary_origin_id == group.secondary_origin_id
            || !origin_ids.contains(&group.primary_origin_id)
            || !origin_ids.contains(&group.secondary_origin_id)
            || group.failover_status_codes.is_empty()
            || group
                .failover_status_codes
                .iter()
                .any(|code| !matches!(code, 400 | 403 | 404 | 416 | 429 | 500 | 502 | 503 | 504))
        {
            return Err(validation("invalid_cloudfront_origin_group"));
        }
        validate_feature_codes(&group.unsupported_features)?;
    }

    validate_behavior(&config.default_cache_behavior, &all_target_ids, false)?;
    let mut patterns = BTreeSet::new();
    for behavior in &config.ordered_cache_behaviors {
        validate_behavior(behavior, &all_target_ids, true)?;
        if !patterns.insert(behavior.path_pattern.clone()) {
            return Err(validation("duplicate_cloudfront_cache_behavior_path"));
        }
    }
    validate_feature_codes(&config.unsupported_features)?;
    Ok(())
}

fn validate_behavior(
    behavior: &crate::CloudFrontCacheBehaviorProjection,
    target_ids: &BTreeSet<String>,
    ordered: bool,
) -> CloudFrontApiResult<()> {
    let read_methods = BTreeSet::from(["GET".to_string(), "HEAD".to_string()]);
    let read_options_methods =
        BTreeSet::from(["GET".to_string(), "HEAD".to_string(), "OPTIONS".to_string()]);
    let all_methods = BTreeSet::from([
        "GET".to_string(),
        "HEAD".to_string(),
        "OPTIONS".to_string(),
        "POST".to_string(),
        "PUT".to_string(),
        "PATCH".to_string(),
        "DELETE".to_string(),
    ]);
    if behavior.path_pattern.is_some() != ordered
        || !target_ids.contains(&behavior.target_origin_id)
        || !matches!(
            behavior.viewer_protocol_policy.as_str(),
            "allow-all" | "https-only" | "redirect-to-https"
        )
        || !matches!(
            &behavior.allowed_methods,
            methods if methods == &read_methods
                || methods == &read_options_methods
                || methods == &all_methods
        )
        || !matches!(
            &behavior.cached_methods,
            methods if methods == &read_methods || methods == &read_options_methods
        )
        || !behavior.cached_methods.is_subset(&behavior.allowed_methods)
    {
        return Err(validation("invalid_cloudfront_cache_behavior"));
    }
    if let Some(pattern) = behavior.path_pattern.as_deref() {
        validate_cache_behavior_path(pattern)?;
    }
    if let Some(policy_id) = behavior.cache_policy_id.as_deref() {
        validate_bounded_text(
            policy_id,
            MAX_PROVIDER_IDENTIFIER_LEN,
            "invalid_cloudfront_cache_policy_id",
        )?;
    } else if !behavior
        .unsupported_features
        .contains("legacy_forwarded_values")
    {
        // The initial managed shape requires a cache policy. A provider-observed legacy
        // ForwardedValues behavior remains inventory-safe, but its feature marker keeps the
        // distribution mutation-ineligible instead of silently flattening the legacy shape.
        return Err(validation("missing_cloudfront_cache_policy_id"));
    }
    validate_feature_codes(&behavior.unsupported_features)
}

fn validate_cache_behavior_path(pattern: &str) -> CloudFrontApiResult<()> {
    validate_bounded_text(pattern, 255, "invalid_cloudfront_cache_behavior_path")?;
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err(validation("invalid_cloudfront_cache_behavior_path"));
            }
            index += 3;
            continue;
        }
        if !(byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'_' | b'-'
                    | b'.'
                    | b'$'
                    | b'/'
                    | b'~'
                    | b'"'
                    | b'\''
                    | b'@'
                    | b':'
                    | b'+'
                    | b'&'
                    | b'*'
                    | b'?'
            ))
        {
            return Err(validation("invalid_cloudfront_cache_behavior_path"));
        }
        index += 1;
    }
    Ok(())
}

pub(crate) fn mutation_eligibility(
    detail: &CloudFrontDistributionDetail,
) -> CloudFrontMutationEligibility {
    let config = &detail.config;
    let mut reasons = config.unsupported_features.clone();
    // The AWS SDK XML decoder is not a strict proof that future provider fields were retained.
    // CLD-28F must replace this guard with a complete schema-preservation proof before mutation.
    reasons.insert("wire_schema_not_lossless".to_string());
    if !matches!(detail.summary.status.as_str(), "Deployed" | "InProgress") {
        reasons.insert("unknown_distribution_status".to_string());
    }
    if config.staging {
        reasons.insert("staging_distribution".to_string());
    }
    if !config.continuous_deployment_policy_id.is_empty() {
        reasons.insert("continuous_deployment".to_string());
    }
    for origin in &config.origins {
        reasons.extend(origin.unsupported_features.iter().cloned());
        if origin.kind != CloudFrontOriginKind::Custom {
            reasons.insert(
                match origin.kind {
                    CloudFrontOriginKind::S3 => "s3_origin",
                    CloudFrontOriginKind::Vpc => "vpc_origin",
                    CloudFrontOriginKind::Unknown => "unknown_origin",
                    CloudFrontOriginKind::Custom => unreachable!(),
                }
                .to_string(),
            );
        }
        if origin.custom_header_count != 0 {
            reasons.insert("custom_origin_headers_redacted".to_string());
        }
    }
    for group in &config.origin_groups {
        reasons.extend(group.unsupported_features.iter().cloned());
    }
    let origin_group_ids = config
        .origin_groups
        .iter()
        .map(|group| group.id.as_str())
        .collect::<BTreeSet<_>>();
    for behavior in
        std::iter::once(&config.default_cache_behavior).chain(config.ordered_cache_behaviors.iter())
    {
        reasons.extend(behavior.unsupported_features.iter().cloned());
        if behavior.field_level_encryption_id.is_some() {
            reasons.insert("field_level_encryption".to_string());
        }
        if behavior.realtime_log_config_arn.is_some() {
            reasons.insert("realtime_log_config".to_string());
        }
        if origin_group_ids.contains(behavior.target_origin_id.as_str()) {
            if behavior.allowed_methods.contains("POST") {
                reasons.insert("origin_group_write_methods".to_string());
            }
            if behavior.allowed_methods.contains("OPTIONS")
                && !behavior.cached_methods.contains("OPTIONS")
            {
                reasons.insert("origin_group_uncached_options".to_string());
            }
        }
    }
    if reasons.is_empty() {
        CloudFrontMutationEligibility::Eligible
    } else {
        CloudFrontMutationEligibility::Ineligible { reasons }
    }
}

pub(crate) fn detail_issue(
    kind: CloudFrontDetailIssueKind,
    code: impl Into<String>,
) -> CloudFrontDetailObservation {
    let code = code.into();
    CloudFrontDetailObservation::Partial(CloudFrontDetailIssue { kind, code })
}

pub(crate) fn classify_detail_error(error: &NormalizedProviderError) -> CloudFrontDetailIssueKind {
    match error.category() {
        ProviderErrorCategory::Authentication | ProviderErrorCategory::Authorization => {
            CloudFrontDetailIssueKind::Inaccessible
        }
        ProviderErrorCategory::NotFound => CloudFrontDetailIssueKind::Missing,
        ProviderErrorCategory::Validation => CloudFrontDetailIssueKind::Malformed,
        ProviderErrorCategory::Quota
        | ProviderErrorCategory::Conflict
        | ProviderErrorCategory::Transient
        | ProviderErrorCategory::Throttled
        | ProviderErrorCategory::UnknownOutcome => CloudFrontDetailIssueKind::Unavailable,
    }
}

pub(crate) fn is_aws_account_id(value: &str) -> bool {
    value.len() == 12 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_feature_codes(codes: &BTreeSet<String>) -> CloudFrontApiResult<()> {
    for code in codes {
        if code.is_empty()
            || code.len() > MAX_FEATURE_CODE_LEN
            || !code.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'-' | b'.' | b':')
            })
        {
            return Err(validation("invalid_cloudfront_unsupported_feature_code"));
        }
    }
    Ok(())
}

fn validate_bounded_text(
    value: &str,
    max_len: usize,
    error_code: &'static str,
) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(validation(error_code));
    }
    Ok(())
}

fn validate_bounded_optional_text(
    value: &str,
    max_len: usize,
    error_code: &'static str,
) -> CloudFrontApiResult<()> {
    if value.len() > max_len || value.chars().any(char::is_control) {
        return Err(validation(error_code));
    }
    Ok(())
}

pub(crate) fn validation(code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::Validation,
        code,
        "CloudFront provider response failed validation",
        None,
        None,
    )
    .expect("static normalized CloudFront validation error")
}
