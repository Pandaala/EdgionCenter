//! Observation-bound CloudFront public custom-origin and failover fragments.

use std::{collections::BTreeSet, net::IpAddr};

use edgion_center_core::{
    CloudResourceId, CloudResourceKind, CredentialRef, DomainName, OriginAddress, OriginDrainState,
    OriginEndpoint, OriginEndpointName, OriginFailoverMode, OriginPool, OriginProtocol,
    OriginTlsMode,
};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::{
    model::{validate_detail, validation},
    AwsPartition, CloudFrontApiResult, CloudFrontDetailObservation, CloudFrontEtagRevisionMac,
    CloudFrontObservationAuthority, CloudFrontPlanningInventory,
};

const MAX_ORIGINS: usize = 100;
const MAX_ORIGIN_GROUPS: usize = 10;
const MAX_PUBLIC_HEADERS: usize = 30;
const MAX_HEADER_NAME_LEN: usize = 256;
const MAX_HEADER_VALUE_LEN: usize = 1_783;
const MAX_HEADER_TOTAL_LEN: usize = 10_240;
const MAX_ORIGIN_PATH_LEN: usize = 255;
const MAX_REVISION_LEN: usize = 512;
const MAX_APPROVAL_FRESHNESS_MS: i64 = 5 * 60 * 1_000;
const PUBLIC_ADDRESS_POLICY_REVISION: &str = "center-public-address-policy-v1";
const SUPPORTED_FAILOVER_CODES: [u16; 9] = [400, 403, 404, 416, 429, 500, 502, 503, 504];

macro_rules! target_id {
    ($name:ident, $code:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> CloudFrontApiResult<Self> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 128
                    || value.trim() != value
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    return Err(validation($code));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

target_id!(CloudFrontOriginId, "invalid_cloudfront_origin_id");
target_id!(
    CloudFrontOriginGroupId,
    "invalid_cloudfront_origin_group_id"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontOriginProtocolPolicy {
    HttpOnly,
    HttpsOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum CloudFrontOriginSslProtocol {
    #[serde(rename = "TLSv1.2")]
    Tls12,
}

/// Exact CLD-28A observation fence. It carries no mutation authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDistributionObservationBinding {
    authority: CloudFrontObservationAuthority,
    distribution_id: String,
    distribution_arn: String,
    etag: String,
    etag_revision_mac: CloudFrontEtagRevisionMac,
}

impl CloudFrontDistributionObservationBinding {
    pub(crate) fn from_inventory(
        planning_inventory: &CloudFrontPlanningInventory,
        distribution_id: &str,
        now_unix_ms: i64,
    ) -> CloudFrontApiResult<Self> {
        let inventory = planning_inventory.inventory();
        inventory.authority.validate()?;
        if !inventory.authority.is_fresh_at(now_unix_ms) {
            return Err(validation("stale_cloudfront_distribution_observation"));
        }
        let entry = inventory
            .entries
            .iter()
            .find(|entry| entry.summary.id == distribution_id)
            .ok_or_else(|| validation("cloudfront_distribution_observation_missing"))?;
        let CloudFrontDetailObservation::Complete(observed) = &entry.detail else {
            return Err(validation("cloudfront_distribution_observation_incomplete"));
        };
        validate_detail(
            &observed.detail,
            inventory.authority.partition(),
            inventory.authority.aws_account_id(),
        )?;
        if observed.detail.summary.id != entry.summary.id
            || observed.detail.summary.arn != entry.summary.arn
            || inventory
                .entries
                .iter()
                .filter(|candidate| candidate.summary.id == distribution_id)
                .count()
                != 1
        {
            return Err(validation("cloudfront_distribution_observation_mismatch"));
        }
        Ok(Self {
            authority: inventory.authority.clone(),
            distribution_id: observed.detail.summary.id.clone(),
            distribution_arn: observed.detail.summary.arn.clone(),
            etag: observed.detail.etag.clone(),
            etag_revision_mac: observed.detail.etag_revision_mac.clone(),
        })
    }

    pub fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.authority.validate()?;
        self.etag_revision_mac.validate()?;
        let expected_arn = format!(
            "arn:{}:cloudfront::{}:distribution/{}",
            self.authority.partition().arn_partition(),
            self.authority.aws_account_id(),
            self.distribution_id
        );
        if !self.authority.is_fresh_at(now_unix_ms)
            || self.distribution_id.is_empty()
            || self.distribution_arn != expected_arn
            || self.etag.is_empty()
        {
            return Err(validation("invalid_cloudfront_distribution_binding"));
        }
        Ok(())
    }

    pub fn provider_account_id(&self) -> &CloudResourceId {
        self.authority.provider_account_id()
    }

    pub fn aws_account_id(&self) -> &str {
        self.authority.aws_account_id()
    }

    pub fn partition(&self) -> AwsPartition {
        self.authority.partition()
    }

    pub fn account_generation(&self) -> u64 {
        self.authority.account_generation()
    }

    pub fn distribution_id(&self) -> &str {
        &self.distribution_id
    }

    pub fn distribution_arn(&self) -> &str {
        &self.distribution_arn
    }

    pub fn etag(&self) -> &str {
        &self.etag
    }

    pub(crate) fn authority(&self) -> &CloudFrontObservationAuthority {
        &self.authority
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Sealed until the trusted composition resolver is wired to the Admin API.
pub(crate) enum CloudFrontOriginEndpointClassification {
    PublicCustom,
    PublicAwsCustom,
    S3Website,
    VpcOrigin,
    Unknown,
}

/// Complete output from a composition-owned resolver profile.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // Sealed evidence has no public constructor in the fragment-only phase.
pub(crate) struct CloudFrontOriginResolution {
    pub hostname: DomainName,
    pub cname_chain: Vec<DomainName>,
    pub resolved_addresses: BTreeSet<IpAddr>,
    pub resolver_profile_id: CloudResourceId,
    pub resolver_profile_revision: String,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
}

/// Trusted composition policy that is the only minter of origin approvals.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Constructed only by the future trusted composition root and unit tests today.
pub(crate) struct CloudFrontPublicOriginPolicy {
    authority_id: CloudResourceId,
    authority_revision: String,
    resolver_profile_id: CloudResourceId,
    resolver_profile_revision: String,
    approved_suffixes: BTreeSet<DomainName>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CloudFrontPublicOriginApprovalRequest {
    pub distribution_id: String,
    pub hostname: DomainName,
    pub protocol: OriginProtocol,
    pub classification: CloudFrontOriginEndpointClassification,
    pub resolution: CloudFrontOriginResolution,
    pub now_unix_ms: i64,
}

#[allow(dead_code)]
impl CloudFrontPublicOriginPolicy {
    pub(crate) fn new(
        authority_id: CloudResourceId,
        authority_revision: impl Into<String>,
        resolver_profile_id: CloudResourceId,
        resolver_profile_revision: impl Into<String>,
        approved_suffixes: BTreeSet<DomainName>,
    ) -> CloudFrontApiResult<Self> {
        authority_id
            .validate()
            .map_err(|_| validation("invalid_cloudfront_origin_approval_authority"))?;
        resolver_profile_id
            .validate()
            .map_err(|_| validation("invalid_cloudfront_origin_resolver_profile"))?;
        let authority_revision = authority_revision.into();
        let resolver_profile_revision = resolver_profile_revision.into();
        validate_revision(
            &authority_revision,
            "invalid_cloudfront_origin_approval_revision",
        )?;
        validate_revision(
            &resolver_profile_revision,
            "invalid_cloudfront_origin_resolver_revision",
        )?;
        if approved_suffixes.is_empty()
            || approved_suffixes
                .iter()
                .any(|suffix| is_special_use_origin_name(suffix.as_str()))
        {
            return Err(validation("invalid_cloudfront_approved_origin_suffix"));
        }
        Ok(Self {
            authority_id,
            authority_revision,
            resolver_profile_id,
            resolver_profile_revision,
            approved_suffixes,
        })
    }

    pub(crate) fn approve(
        &self,
        inventory: &CloudFrontPlanningInventory,
        request: CloudFrontPublicOriginApprovalRequest,
    ) -> CloudFrontApiResult<CloudFrontPublicOriginApproval> {
        let CloudFrontPublicOriginApprovalRequest {
            distribution_id,
            hostname,
            protocol,
            classification,
            resolution,
            now_unix_ms,
        } = request;
        let binding = CloudFrontDistributionObservationBinding::from_inventory(
            inventory,
            &distribution_id,
            now_unix_ms,
        )?;
        if !matches!(
            classification,
            CloudFrontOriginEndpointClassification::PublicCustom
                | CloudFrontOriginEndpointClassification::PublicAwsCustom
        ) || resolution.hostname != hostname
            || resolution.resolver_profile_id != self.resolver_profile_id
            || resolution.resolver_profile_revision != self.resolver_profile_revision
            || !self
                .approved_suffixes
                .iter()
                .any(|suffix| hostname_is_within(&hostname, suffix))
            || resolution.cname_chain.len() > 16
            || resolution
                .cname_chain
                .iter()
                .any(|name| is_special_use_origin_name(name.as_str()))
        {
            return Err(validation("cloudfront_public_origin_policy_denied"));
        }
        let approval = CloudFrontPublicOriginApproval {
            binding,
            hostname,
            protocol,
            resolved_addresses: resolution.resolved_addresses,
            approval_authority: self.authority_id.clone(),
            approval_revision: self.authority_revision.clone(),
            resolver_profile_id: self.resolver_profile_id.clone(),
            resolver_profile_revision: self.resolver_profile_revision.clone(),
            public_address_policy_revision: PUBLIC_ADDRESS_POLICY_REVISION.to_string(),
            cname_chain: resolution.cname_chain,
            observed_at_unix_ms: resolution.observed_at_unix_ms,
            valid_until_unix_ms: resolution.valid_until_unix_ms,
        };
        approval.validate_at(now_unix_ms)?;
        Ok(approval)
    }
}

/// Resolver/registry output. Its fields are private and it has no wire constructor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontPublicOriginApproval {
    binding: CloudFrontDistributionObservationBinding,
    hostname: DomainName,
    protocol: OriginProtocol,
    resolved_addresses: BTreeSet<IpAddr>,
    approval_authority: CloudResourceId,
    approval_revision: String,
    resolver_profile_id: CloudResourceId,
    resolver_profile_revision: String,
    public_address_policy_revision: String,
    cname_chain: Vec<DomainName>,
    observed_at_unix_ms: i64,
    valid_until_unix_ms: i64,
}

impl CloudFrontPublicOriginApproval {
    pub(crate) fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.binding.validate_at(now_unix_ms)?;
        self.approval_authority
            .validate()
            .map_err(|_| validation("invalid_cloudfront_origin_approval_authority"))?;
        validate_revision(
            &self.approval_revision,
            "invalid_cloudfront_origin_approval_revision",
        )?;
        self.resolver_profile_id
            .validate()
            .map_err(|_| validation("invalid_cloudfront_origin_resolver_profile"))?;
        validate_revision(
            &self.resolver_profile_revision,
            "invalid_cloudfront_origin_resolver_revision",
        )?;
        if is_special_use_origin_name(self.hostname.as_str())
            || !matches!(self.protocol, OriginProtocol::Http | OriginProtocol::Https)
            || self.resolved_addresses.is_empty()
            || self.public_address_policy_revision != PUBLIC_ADDRESS_POLICY_REVISION
            || self
                .resolved_addresses
                .iter()
                .any(|address| !is_public_unicast(*address))
            || self.observed_at_unix_ms <= 0
            || self
                .valid_until_unix_ms
                .saturating_sub(self.observed_at_unix_ms)
                > MAX_APPROVAL_FRESHNESS_MS
            || now_unix_ms < self.observed_at_unix_ms
            || now_unix_ms >= self.valid_until_unix_ms
        {
            return Err(validation("invalid_cloudfront_public_origin_approval"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontSecretHeaderBinding {
    credential_ref: CredentialRef,
    secret_revision: String,
}

impl CloudFrontSecretHeaderBinding {
    pub fn new(
        credential_ref: CredentialRef,
        secret_revision: impl Into<String>,
    ) -> CloudFrontApiResult<Self> {
        credential_ref
            .validate()
            .map_err(|_| validation("invalid_cloudfront_origin_secret_reference"))?;
        if credential_ref.as_str().len() > MAX_REVISION_LEN {
            return Err(validation("invalid_cloudfront_origin_secret_reference"));
        }
        let secret_revision = secret_revision.into();
        validate_revision(
            &secret_revision,
            "invalid_cloudfront_origin_secret_revision",
        )?;
        Ok(Self {
            credential_ref,
            secret_revision,
        })
    }

    pub fn credential_ref(&self) -> &CredentialRef {
        &self.credential_ref
    }
}

struct ResolvedCloudFrontOriginSecretHeaderValue {
    name: Zeroizing<String>,
    value: Zeroizing<String>,
}

/// Execution-window secret bundle. It is deliberately non-Debug, non-Clone, and non-Serialize.
pub struct ResolvedCloudFrontOriginSecretHeaders {
    credential_ref: CredentialRef,
    secret_revision: String,
    headers: Vec<ResolvedCloudFrontOriginSecretHeaderValue>,
}

impl ResolvedCloudFrontOriginSecretHeaders {
    pub fn new(
        credential_ref: CredentialRef,
        secret_revision: impl Into<String>,
        headers: Vec<(String, String)>,
    ) -> CloudFrontApiResult<Self> {
        let headers = headers
            .into_iter()
            .map(|(name, value)| ResolvedCloudFrontOriginSecretHeaderValue {
                name: Zeroizing::new(name),
                value: Zeroizing::new(value),
            })
            .collect::<Vec<_>>();
        if headers.is_empty() || headers.len() > MAX_PUBLIC_HEADERS {
            return Err(validation("invalid_cloudfront_origin_secret_header_count"));
        }
        let binding = CloudFrontSecretHeaderBinding::new(credential_ref.clone(), secret_revision)?;
        Ok(Self {
            credential_ref,
            secret_revision: binding.secret_revision,
            headers,
        })
    }
}

pub fn validate_resolved_origin_secret_headers(
    fragment: &CloudFrontCustomOriginFragment,
    resolved: &ResolvedCloudFrontOriginSecretHeaders,
) -> CloudFrontApiResult<()> {
    let Some(expected) = fragment.secret_headers.as_ref() else {
        return Err(validation("unexpected_cloudfront_origin_secret_header"));
    };
    if expected.credential_ref != resolved.credential_ref
        || expected.secret_revision != resolved.secret_revision
    {
        return Err(validation("cloudfront_origin_secret_revision_mismatch"));
    }
    if fragment.public_headers.len() + resolved.headers.len() > MAX_PUBLIC_HEADERS {
        return Err(validation("cloudfront_origin_header_limit"));
    }
    let mut names = fragment
        .public_headers
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut total = fragment
        .public_headers
        .iter()
        .fold(0usize, |size, (name, value)| {
            size.saturating_add(name.len()).saturating_add(value.len())
        });
    for header in &resolved.headers {
        let name = header.name.as_str();
        let value = header.value.as_str();
        let lower = name.to_ascii_lowercase();
        total = total.saturating_add(name.len()).saturating_add(value.len());
        if !names.insert(lower.clone())
            || is_forbidden_origin_header(&lower)
            || !is_valid_header_pair(name, value)
            || total > MAX_HEADER_TOTAL_LEN
        {
            return Err(validation(
                "invalid_cloudfront_resolved_origin_secret_header",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontCustomOriginIntent {
    pub pool_id: CloudResourceId,
    pub pool_generation: u64,
    pub endpoint_name: OriginEndpointName,
    pub origin_id: CloudFrontOriginId,
    pub origin_path: String,
    pub protocol_policy: CloudFrontOriginProtocolPolicy,
    pub http_port: u16,
    pub https_port: u16,
    pub ssl_protocols: BTreeSet<CloudFrontOriginSslProtocol>,
    pub connection_attempts: u8,
    pub connection_timeout_seconds: u8,
    pub response_timeout_seconds: u16,
    pub keepalive_timeout_seconds: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudFrontOriginOperation {
    Create,
    Update,
}

#[derive(Debug, Clone)]
pub struct CloudFrontCustomOriginFragmentRequest {
    pub intent: CloudFrontCustomOriginIntent,
    pub secret_headers: Option<CloudFrontSecretHeaderBinding>,
    pub distribution_id: String,
    pub operation: CloudFrontOriginOperation,
    pub now_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontCustomOriginFragment {
    binding: CloudFrontDistributionObservationBinding,
    pool_id: CloudResourceId,
    pool_generation: u64,
    endpoint_name: OriginEndpointName,
    origin_id: CloudFrontOriginId,
    hostname: DomainName,
    origin_path: String,
    protocol_policy: CloudFrontOriginProtocolPolicy,
    http_port: u16,
    https_port: u16,
    ssl_protocols: BTreeSet<CloudFrontOriginSslProtocol>,
    connection_attempts: u8,
    connection_timeout_seconds: u8,
    response_timeout_seconds: u16,
    keepalive_timeout_seconds: u16,
    public_headers: std::collections::BTreeMap<String, String>,
    secret_headers: Option<CloudFrontSecretHeaderBinding>,
    public_origin_approval: CloudFrontPublicOriginApproval,
}

impl CloudFrontCustomOriginFragment {
    pub fn binding(&self) -> &CloudFrontDistributionObservationBinding {
        &self.binding
    }

    pub fn origin_id(&self) -> &CloudFrontOriginId {
        &self.origin_id
    }

    pub fn hostname(&self) -> &DomainName {
        &self.hostname
    }

    pub(crate) fn approval_valid_until_unix_ms(&self) -> i64 {
        self.public_origin_approval.valid_until_unix_ms
    }

    pub fn provider_configuration(&self) -> CloudFrontCustomOriginConfiguration<'_> {
        CloudFrontCustomOriginConfiguration {
            origin_id: &self.origin_id,
            hostname: &self.hostname,
            origin_path: &self.origin_path,
            protocol_policy: self.protocol_policy,
            http_port: self.http_port,
            https_port: self.https_port,
            ssl_protocols: &self.ssl_protocols,
            connection_attempts: self.connection_attempts,
            connection_timeout_seconds: self.connection_timeout_seconds,
            response_timeout_seconds: self.response_timeout_seconds,
            keepalive_timeout_seconds: self.keepalive_timeout_seconds,
            public_headers: &self.public_headers,
            secret_headers: self.secret_headers.as_ref(),
        }
    }

    pub(crate) fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.binding.validate_at(now_unix_ms)?;
        self.public_origin_approval.validate_at(now_unix_ms)?;
        if self.public_origin_approval.binding != self.binding
            || self.public_origin_approval.hostname != self.hostname
            || self.pool_generation == 0
        {
            return Err(validation("invalid_cloudfront_custom_origin_fragment"));
        }
        Ok(())
    }
}

/// Read-only validated values for CLD-28F full-configuration composition.
pub struct CloudFrontCustomOriginConfiguration<'a> {
    pub origin_id: &'a CloudFrontOriginId,
    pub hostname: &'a DomainName,
    pub origin_path: &'a str,
    pub protocol_policy: CloudFrontOriginProtocolPolicy,
    pub http_port: u16,
    pub https_port: u16,
    pub ssl_protocols: &'a BTreeSet<CloudFrontOriginSslProtocol>,
    pub connection_attempts: u8,
    pub connection_timeout_seconds: u8,
    pub response_timeout_seconds: u16,
    pub keepalive_timeout_seconds: u16,
    pub public_headers: &'a std::collections::BTreeMap<String, String>,
    pub secret_headers: Option<&'a CloudFrontSecretHeaderBinding>,
}

pub fn build_custom_origin_fragment(
    request: CloudFrontCustomOriginFragmentRequest,
    pool: &OriginPool,
    approval: &CloudFrontPublicOriginApproval,
    inventory: &CloudFrontPlanningInventory,
) -> CloudFrontApiResult<CloudFrontCustomOriginFragment> {
    let CloudFrontCustomOriginFragmentRequest {
        intent,
        secret_headers,
        distribution_id,
        operation,
        now_unix_ms,
    } = request;
    let binding = &approval.binding;
    let observed_binding = CloudFrontDistributionObservationBinding::from_inventory(
        inventory,
        &distribution_id,
        now_unix_ms,
    )?;
    if observed_binding != *binding {
        return Err(validation("cloudfront_origin_binding_mismatch"));
    }
    approval.validate_at(now_unix_ms)?;
    let entry = inventory
        .inventory()
        .entries
        .iter()
        .find(|entry| entry.summary.id == distribution_id)
        .ok_or_else(|| validation("cloudfront_distribution_observation_missing"))?;
    let CloudFrontDetailObservation::Complete(observed) = &entry.detail else {
        return Err(validation("cloudfront_distribution_observation_incomplete"));
    };
    let origin_exists = observed
        .detail
        .config
        .origins
        .iter()
        .any(|origin| origin.id == intent.origin_id.as_str());
    let cross_kind_collision = observed
        .detail
        .config
        .origin_groups
        .iter()
        .any(|group| group.id == intent.origin_id.as_str());
    let invalid_operation = match operation {
        CloudFrontOriginOperation::Create => {
            origin_exists || observed.detail.config.origins.len() >= MAX_ORIGINS
        }
        CloudFrontOriginOperation::Update => !origin_exists,
    };
    if invalid_operation || cross_kind_collision {
        return Err(validation("invalid_cloudfront_origin_operation"));
    }
    pool.spec
        .validate()
        .map_err(|_| validation("invalid_cloudfront_origin_pool"))?;
    if pool.spec.health_check.is_some()
        || pool.spec.minimum_healthy != 1
        || pool.spec.failover_mode != OriginFailoverMode::PriorityTiers
    {
        return Err(validation("unsupported_cloudfront_origin_pool_semantics"));
    }
    if pool.metadata.id != intent.pool_id
        || pool.metadata.generation != intent.pool_generation
        || intent.pool_generation == 0
        || approval.binding != *binding
    {
        return Err(validation("cloudfront_origin_binding_mismatch"));
    }
    if pool
        .spec
        .provider_account_ref
        .as_ref()
        .is_some_and(|reference| {
            reference.kind != CloudResourceKind::ProviderAccount
                || reference.id != *binding.provider_account_id()
        })
    {
        return Err(validation("cloudfront_origin_provider_account_mismatch"));
    }
    if pool.spec.endpoints.len() > MAX_ORIGINS {
        return Err(validation("cloudfront_origin_count_limit"));
    }
    let endpoint = pool
        .spec
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == intent.endpoint_name)
        .ok_or_else(|| validation("cloudfront_origin_endpoint_missing"))?;
    let OriginAddress::Hostname(hostname) = &endpoint.address else {
        return Err(validation("cloudfront_public_hostname_origin_required"));
    };
    validate_port(intent.http_port)?;
    validate_port(intent.https_port)?;
    validate_origin_path(&intent.origin_path)?;
    validate_portable_endpoint(endpoint, hostname, &intent)?;
    validate_public_headers(endpoint)?;
    if endpoint.headers.literal.len() + usize::from(secret_headers.is_some()) > MAX_PUBLIC_HEADERS {
        return Err(validation("cloudfront_origin_header_limit"));
    }
    if approval.hostname != *hostname
        || approval.protocol != endpoint.protocol
        || endpoint.headers.secret_ref.as_ref()
            != secret_headers.as_ref().map(|v| &v.credential_ref)
    {
        return Err(validation("cloudfront_origin_approval_mismatch"));
    }
    if secret_headers.is_some()
        && (intent.protocol_policy != CloudFrontOriginProtocolPolicy::HttpsOnly
            || endpoint.tls_mode != OriginTlsMode::Verify)
    {
        return Err(validation(
            "cloudfront_secret_origin_requires_verified_https",
        ));
    }
    Ok(CloudFrontCustomOriginFragment {
        binding: binding.clone(),
        pool_id: intent.pool_id,
        pool_generation: intent.pool_generation,
        endpoint_name: intent.endpoint_name,
        origin_id: intent.origin_id,
        hostname: hostname.clone(),
        origin_path: intent.origin_path,
        protocol_policy: intent.protocol_policy,
        http_port: intent.http_port,
        https_port: intent.https_port,
        ssl_protocols: intent.ssl_protocols,
        connection_attempts: intent.connection_attempts,
        connection_timeout_seconds: intent.connection_timeout_seconds,
        response_timeout_seconds: intent.response_timeout_seconds,
        keepalive_timeout_seconds: intent.keepalive_timeout_seconds,
        public_headers: endpoint.headers.literal.clone(),
        secret_headers,
        public_origin_approval: approval.clone(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontOriginGroupIntent {
    pub group_id: CloudFrontOriginGroupId,
    pub primary_origin_id: CloudFrontOriginId,
    pub secondary_origin_id: CloudFrontOriginId,
    pub failover_status_codes: BTreeSet<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontOriginGroupFragment {
    binding: CloudFrontDistributionObservationBinding,
    group_id: CloudFrontOriginGroupId,
    primary_origin_id: CloudFrontOriginId,
    secondary_origin_id: CloudFrontOriginId,
    failover_status_codes: BTreeSet<u16>,
    member_approval_valid_until_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudFrontOriginGroupOperation {
    Create,
    Update,
}

impl CloudFrontOriginGroupFragment {
    pub fn binding(&self) -> &CloudFrontDistributionObservationBinding {
        &self.binding
    }

    pub fn group_id(&self) -> &CloudFrontOriginGroupId {
        &self.group_id
    }

    pub fn primary_origin_id(&self) -> &CloudFrontOriginId {
        &self.primary_origin_id
    }

    pub fn secondary_origin_id(&self) -> &CloudFrontOriginId {
        &self.secondary_origin_id
    }

    pub fn failover_status_codes(&self) -> &BTreeSet<u16> {
        &self.failover_status_codes
    }

    pub(crate) fn approval_valid_until_unix_ms(&self) -> i64 {
        self.member_approval_valid_until_unix_ms
    }

    pub(crate) fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.binding.validate_at(now_unix_ms)?;
        if now_unix_ms >= self.member_approval_valid_until_unix_ms {
            return Err(validation("stale_cloudfront_origin_group_fragment"));
        }
        Ok(())
    }
}

pub fn build_origin_group_fragment(
    intent: CloudFrontOriginGroupIntent,
    origins: &[CloudFrontCustomOriginFragment],
    inventory: &CloudFrontPlanningInventory,
    distribution_id: &str,
    operation: CloudFrontOriginGroupOperation,
    now_unix_ms: i64,
) -> CloudFrontApiResult<CloudFrontOriginGroupFragment> {
    let binding = CloudFrontDistributionObservationBinding::from_inventory(
        inventory,
        distribution_id,
        now_unix_ms,
    )?;
    binding.validate_at(now_unix_ms)?;
    let entry = inventory
        .inventory()
        .entries
        .iter()
        .find(|entry| entry.summary.id == distribution_id)
        .ok_or_else(|| validation("cloudfront_distribution_observation_missing"))?;
    let CloudFrontDetailObservation::Complete(observed) = &entry.detail else {
        return Err(validation("cloudfront_distribution_observation_incomplete"));
    };
    let observed_groups = &observed.detail.config.origin_groups;
    let observed_origins = &observed.detail.config.origins;
    let group_exists = observed_groups
        .iter()
        .any(|group| group.id == intent.group_id.as_str());
    let invalid_operation = match operation {
        CloudFrontOriginGroupOperation::Create => {
            group_exists || observed_groups.len() >= MAX_ORIGIN_GROUPS
        }
        CloudFrontOriginGroupOperation::Update => !group_exists,
    };
    if invalid_operation
        || observed_origins
            .iter()
            .any(|origin| origin.id == intent.group_id.as_str())
        || observed_groups.len() > MAX_ORIGIN_GROUPS
        || origins.len() > MAX_ORIGINS
        || origins.is_empty()
        || intent.primary_origin_id == intent.secondary_origin_id
        || intent.group_id.as_str() == intent.primary_origin_id.as_str()
        || intent.group_id.as_str() == intent.secondary_origin_id.as_str()
        || intent.failover_status_codes.is_empty()
        || intent.failover_status_codes.len() > SUPPORTED_FAILOVER_CODES.len()
        || intent
            .failover_status_codes
            .iter()
            .any(|code| !SUPPORTED_FAILOVER_CODES.contains(code))
    {
        return Err(validation("invalid_cloudfront_origin_group_intent"));
    }
    let mut ids = BTreeSet::new();
    let source_pool = origins
        .first()
        .map(|origin| (&origin.pool_id, origin.pool_generation));
    for origin in origins {
        origin.validate_at(now_unix_ms)?;
        if origin.binding != binding
            || source_pool != Some((&origin.pool_id, origin.pool_generation))
            || intent.group_id.as_str() == origin.origin_id.as_str()
            || observed_groups
                .iter()
                .any(|group| group.id == origin.origin_id.as_str())
            || !ids.insert(origin.origin_id.as_str())
        {
            return Err(validation("cloudfront_origin_group_binding_mismatch"));
        }
    }
    if !ids.contains(intent.primary_origin_id.as_str())
        || !ids.contains(intent.secondary_origin_id.as_str())
    {
        return Err(validation("cloudfront_origin_group_member_missing"));
    }
    let primary = origins
        .iter()
        .find(|origin| origin.origin_id == intent.primary_origin_id)
        .ok_or_else(|| validation("cloudfront_origin_group_member_missing"))?;
    let secondary = origins
        .iter()
        .find(|origin| origin.origin_id == intent.secondary_origin_id)
        .ok_or_else(|| validation("cloudfront_origin_group_member_missing"))?;
    if primary.endpoint_name == secondary.endpoint_name || primary.hostname == secondary.hostname {
        return Err(validation("cloudfront_origin_group_duplicate_endpoint"));
    }
    Ok(CloudFrontOriginGroupFragment {
        binding,
        group_id: intent.group_id,
        primary_origin_id: intent.primary_origin_id,
        secondary_origin_id: intent.secondary_origin_id,
        failover_status_codes: intent.failover_status_codes,
        member_approval_valid_until_unix_ms: primary
            .public_origin_approval
            .valid_until_unix_ms
            .min(secondary.public_origin_approval.valid_until_unix_ms),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum CloudFrontOriginTargetRef {
    Origin(CloudFrontOriginId),
    OriginGroup(CloudFrontOriginGroupId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CloudFrontViewerMethod {
    Get,
    Head,
    Options,
    Post,
    Put,
    Patch,
    Delete,
}

pub fn validate_target_methods(
    target: &CloudFrontOriginTargetRef,
    allowed: &BTreeSet<CloudFrontViewerMethod>,
    cached: &BTreeSet<CloudFrontViewerMethod>,
) -> CloudFrontApiResult<()> {
    let read = BTreeSet::from([CloudFrontViewerMethod::Get, CloudFrontViewerMethod::Head]);
    let read_options = BTreeSet::from([
        CloudFrontViewerMethod::Get,
        CloudFrontViewerMethod::Head,
        CloudFrontViewerMethod::Options,
    ]);
    let all = BTreeSet::from([
        CloudFrontViewerMethod::Get,
        CloudFrontViewerMethod::Head,
        CloudFrontViewerMethod::Options,
        CloudFrontViewerMethod::Post,
        CloudFrontViewerMethod::Put,
        CloudFrontViewerMethod::Patch,
        CloudFrontViewerMethod::Delete,
    ]);
    if (allowed != &read && allowed != &read_options && allowed != &all)
        || !allowed.contains(&CloudFrontViewerMethod::Get)
        || !allowed.contains(&CloudFrontViewerMethod::Head)
        || !cached.is_subset(allowed)
        || !cached.contains(&CloudFrontViewerMethod::Get)
        || !cached.contains(&CloudFrontViewerMethod::Head)
        || cached.iter().any(|method| {
            !matches!(
                method,
                CloudFrontViewerMethod::Get
                    | CloudFrontViewerMethod::Head
                    | CloudFrontViewerMethod::Options
            )
        })
    {
        return Err(validation("invalid_cloudfront_behavior_methods"));
    }
    if matches!(target, CloudFrontOriginTargetRef::OriginGroup(_))
        && (allowed.iter().any(|method| {
            matches!(
                method,
                CloudFrontViewerMethod::Post
                    | CloudFrontViewerMethod::Put
                    | CloudFrontViewerMethod::Patch
                    | CloudFrontViewerMethod::Delete
            )
        }) || (allowed.contains(&CloudFrontViewerMethod::Options)
            && !cached.contains(&CloudFrontViewerMethod::Options)))
    {
        return Err(validation("cloudfront_origin_group_method_policy"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloudFrontTargetReference {
    OriginGroupPrimary { group_id: String },
    OriginGroupSecondary { group_id: String },
    DefaultCacheBehavior,
    OrderedCacheBehavior { path_pattern: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontReferenceCompleteness {
    SanitizedProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontTargetReferenceEvidence {
    binding: CloudFrontDistributionObservationBinding,
    target: CloudFrontOriginTargetRef,
    references: BTreeSet<CloudFrontTargetReference>,
    completeness: CloudFrontReferenceCompleteness,
    /// Always false for the CLD-28A sanitized projection.
    can_authorize_removal: bool,
}

impl CloudFrontTargetReferenceEvidence {
    pub fn binding(&self) -> &CloudFrontDistributionObservationBinding {
        &self.binding
    }

    pub fn target(&self) -> &CloudFrontOriginTargetRef {
        &self.target
    }

    pub fn references(&self) -> &BTreeSet<CloudFrontTargetReference> {
        &self.references
    }

    pub fn completeness(&self) -> CloudFrontReferenceCompleteness {
        self.completeness
    }

    pub fn can_authorize_removal(&self) -> bool {
        self.can_authorize_removal
    }
}

pub fn collect_target_references(
    inventory: &CloudFrontPlanningInventory,
    distribution_id: &str,
    target: CloudFrontOriginTargetRef,
    now_unix_ms: i64,
) -> CloudFrontApiResult<CloudFrontTargetReferenceEvidence> {
    let binding = CloudFrontDistributionObservationBinding::from_inventory(
        inventory,
        distribution_id,
        now_unix_ms,
    )?;
    let entry = inventory
        .inventory()
        .entries
        .iter()
        .find(|entry| entry.summary.id == distribution_id)
        .ok_or_else(|| validation("cloudfront_distribution_observation_missing"))?;
    let CloudFrontDetailObservation::Complete(observed) = &entry.detail else {
        return Err(validation("cloudfront_distribution_observation_incomplete"));
    };
    let target_id = match &target {
        CloudFrontOriginTargetRef::Origin(id) => id.as_str(),
        CloudFrontOriginTargetRef::OriginGroup(id) => id.as_str(),
    };
    let target_exists = match &target {
        CloudFrontOriginTargetRef::Origin(_) => observed
            .detail
            .config
            .origins
            .iter()
            .any(|origin| origin.id == target_id),
        CloudFrontOriginTargetRef::OriginGroup(_) => observed
            .detail
            .config
            .origin_groups
            .iter()
            .any(|group| group.id == target_id),
    };
    if !target_exists {
        return Err(validation("cloudfront_origin_target_missing"));
    }
    let mut references = BTreeSet::new();
    if matches!(target, CloudFrontOriginTargetRef::Origin(_)) {
        for group in &observed.detail.config.origin_groups {
            if group.primary_origin_id == target_id {
                references.insert(CloudFrontTargetReference::OriginGroupPrimary {
                    group_id: group.id.clone(),
                });
            }
            if group.secondary_origin_id == target_id {
                references.insert(CloudFrontTargetReference::OriginGroupSecondary {
                    group_id: group.id.clone(),
                });
            }
        }
    }
    if observed
        .detail
        .config
        .default_cache_behavior
        .target_origin_id
        == target_id
    {
        references.insert(CloudFrontTargetReference::DefaultCacheBehavior);
    }
    for behavior in &observed.detail.config.ordered_cache_behaviors {
        if behavior.target_origin_id == target_id {
            references.insert(CloudFrontTargetReference::OrderedCacheBehavior {
                path_pattern: behavior.path_pattern.clone().unwrap_or_default(),
            });
        }
    }
    Ok(CloudFrontTargetReferenceEvidence {
        binding,
        target,
        references,
        completeness: CloudFrontReferenceCompleteness::SanitizedProjection,
        can_authorize_removal: false,
    })
}

fn validate_port(value: u16) -> CloudFrontApiResult<()> {
    if matches!(value, 80 | 443) || value >= 1_024 {
        Ok(())
    } else {
        Err(validation("invalid_cloudfront_origin_port"))
    }
}

fn validate_origin_path(path: &str) -> CloudFrontApiResult<()> {
    if path.len() > MAX_ORIGIN_PATH_LEN
        || path.chars().any(char::is_control)
        || (!path.is_empty()
            && (!path.starts_with('/')
                || path.ends_with('/')
                || !path.is_ascii()
                || !has_valid_percent_encoding(path)
                || path
                    .bytes()
                    .any(|byte| byte == b' ' || matches!(byte, b'?' | b'#'))))
    {
        Err(validation("invalid_cloudfront_origin_path"))
    } else {
        Ok(())
    }
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

fn is_special_use_origin_name(value: &str) -> bool {
    !value.contains('.')
        || value == "localhost"
        || value.ends_with(".localhost")
        || value.ends_with(".local")
        || value.ends_with(".internal")
        || value.ends_with(".invalid")
        || value.ends_with(".test")
        || value.ends_with(".example")
}

#[allow(dead_code)]
fn hostname_is_within(hostname: &DomainName, suffix: &DomainName) -> bool {
    hostname.as_str() == suffix.as_str()
        || hostname
            .as_str()
            .strip_suffix(suffix.as_str())
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn validate_portable_endpoint(
    endpoint: &OriginEndpoint,
    hostname: &DomainName,
    intent: &CloudFrontCustomOriginIntent,
) -> CloudFrontApiResult<()> {
    let expected = match intent.protocol_policy {
        CloudFrontOriginProtocolPolicy::HttpOnly => (OriginProtocol::Http, intent.http_port),
        CloudFrontOriginProtocolPolicy::HttpsOnly => (OriginProtocol::Https, intent.https_port),
    };
    if endpoint.protocol != expected.0
        || endpoint.port != expected.1
        || endpoint.tls_mode != OriginTlsMode::Verify
        || endpoint.weight != 1
        || endpoint.priority != 0
        || endpoint.drain != OriginDrainState::Active
        || endpoint
            .host_header
            .as_ref()
            .is_some_and(|value| value != hostname)
        || endpoint
            .server_name
            .as_ref()
            .is_some_and(|value| value != hostname)
        || !(1..=3).contains(&intent.connection_attempts)
        || !(1..=10).contains(&intent.connection_timeout_seconds)
        || !(1..=120).contains(&intent.response_timeout_seconds)
        || !(1..=300).contains(&intent.keepalive_timeout_seconds)
        || (intent.protocol_policy == CloudFrontOriginProtocolPolicy::HttpsOnly
            && intent.ssl_protocols != BTreeSet::from([CloudFrontOriginSslProtocol::Tls12]))
        || (intent.protocol_policy == CloudFrontOriginProtocolPolicy::HttpOnly
            && !intent.ssl_protocols.is_empty())
    {
        return Err(validation("unsupported_cloudfront_portable_origin"));
    }
    Ok(())
}

fn validate_public_headers(endpoint: &OriginEndpoint) -> CloudFrontApiResult<()> {
    if endpoint.headers.literal.len() > MAX_PUBLIC_HEADERS {
        return Err(validation("cloudfront_origin_header_limit"));
    }
    let mut names = BTreeSet::new();
    let mut total = 0usize;
    for (name, value) in &endpoint.headers.literal {
        let lower = name.to_ascii_lowercase();
        total = total.saturating_add(name.len()).saturating_add(value.len());
        if !is_valid_header_pair(name, value)
            || total > MAX_HEADER_TOTAL_LEN
            || !names.insert(lower.clone())
            || is_forbidden_origin_header(&lower)
        {
            return Err(validation("invalid_cloudfront_origin_header"));
        }
    }
    Ok(())
}

fn is_valid_header_pair(name: &str, value: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_HEADER_NAME_LEN
        && value.len() <= MAX_HEADER_VALUE_LEN
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        ..=b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
                )
        })
        && !value.chars().any(char::is_control)
}

fn is_forbidden_origin_header(name: &str) -> bool {
    name.starts_with("x-amz-")
        || name.starts_with("x-edge-")
        || matches!(
            name,
            "cache-control"
                | "connection"
                | "content-length"
                | "cookie"
                | "host"
                | "if-match"
                | "if-modified-since"
                | "if-none-match"
                | "if-range"
                | "if-unmodified-since"
                | "max-forwards"
                | "pragma"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "proxy-connection"
                | "range"
                | "request-range"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
                | "via"
                | "x-real-ip"
        )
}

fn validate_revision(value: &str, code: &'static str) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > MAX_REVISION_LEN
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(validation(code))
    } else {
        Ok(())
    }
}

fn is_public_unicast(value: IpAddr) -> bool {
    match value {
        IpAddr::V4(address) => {
            let octets = address.octets();
            !address.is_private()
                && !address.is_loopback()
                && !address.is_link_local()
                && !address.is_multicast()
                && !address.is_unspecified()
                && !address.is_broadcast()
                && !address.is_documentation()
                && octets[0] != 0
                && octets[0] < 224
                && !(octets[0] == 100 && (64..=127).contains(&octets[1]))
                && !(octets[0] == 198 && matches!(octets[1], 18 | 19))
                && !(octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                && !(octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            (segments[0] & 0xe000) == 0x2000
                && !(address.is_unspecified()
                    || address.is_loopback()
                    || address.is_multicast()
                    || address.is_unique_local()
                    || address.is_unicast_link_local()
                    || (segments[0] == 0x2001 && segments[1] <= 0x01ff)
                    || (segments[0] == 0x2001 && segments[1] == 0x0db8)
                    || segments[0] == 0x2002
                    || (segments[0] & 0xfff0) == 0x3ff0)
                && address.to_ipv4_mapped().is_none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{adapter, detail, summary, FakeApi, ACCOUNT_ID};
    use crate::{
        CloudFrontDistributionPage, CloudFrontOriginGroupProjection, CloudFrontOriginProjection,
        CloudFrontTags,
    };
    use edgion_center_core::{
        CloudResourceMetadata, CloudResourceStatus, DeletionPolicy, ManagementPolicy,
        OriginPoolSpec,
    };
    use std::{collections::BTreeMap, sync::Arc};

    fn inventory() -> CloudFrontPlanningInventory {
        let summary = summary();
        let api = Arc::new(FakeApi {
            account_id: ACCOUNT_ID.to_string(),
            partition: AwsPartition::Aws,
            pages: vec![CloudFrontDistributionPage {
                items: vec![summary.clone()],
                is_truncated: false,
                next_marker: None,
            }],
            detail: Some(detail(summary)),
            tags: CloudFrontTags::default(),
        });
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            adapter(api)
                .planning_inventory("origin-observation", 1_000, 2_000)
                .await
                .unwrap()
        })
    }

    fn pool(protocol: OriginProtocol, secret: bool) -> OriginPool {
        let hostname = DomainName::new("origin.example.com").unwrap();
        OriginPool {
            metadata: CloudResourceMetadata {
                id: CloudResourceId::new("pool-main").unwrap(),
                display_name: "Pool".to_string(),
                owner: None,
                labels: BTreeMap::new(),
                generation: 3,
                management_policy: ManagementPolicy::ObserveOnly,
                deletion_policy: DeletionPolicy::Retain,
            },
            spec: OriginPoolSpec {
                provider_account_ref: None,
                endpoints: vec![OriginEndpoint {
                    name: OriginEndpointName::new("origin-a").unwrap(),
                    address: OriginAddress::Hostname(hostname),
                    port: if protocol == OriginProtocol::Https {
                        443
                    } else {
                        80
                    },
                    protocol,
                    host_header: None,
                    server_name: None,
                    tls_mode: OriginTlsMode::Verify,
                    weight: 1,
                    priority: 0,
                    drain: OriginDrainState::Active,
                    headers: edgion_center_core::OriginRequestHeaders {
                        literal: BTreeMap::from([("X-Public".to_string(), "value".to_string())]),
                        secret_ref: secret.then(|| CredentialRef::new("secret/origin").unwrap()),
                    },
                }],
                health_check: None,
                failover_mode: OriginFailoverMode::PriorityTiers,
                minimum_healthy: 1,
            },
            status: CloudResourceStatus::default(),
        }
    }

    fn intent(protocol: OriginProtocol) -> CloudFrontCustomOriginIntent {
        CloudFrontCustomOriginIntent {
            pool_id: CloudResourceId::new("pool-main").unwrap(),
            pool_generation: 3,
            endpoint_name: OriginEndpointName::new("origin-a").unwrap(),
            origin_id: CloudFrontOriginId::new("origin-a").unwrap(),
            origin_path: "/api".to_string(),
            protocol_policy: if protocol == OriginProtocol::Https {
                CloudFrontOriginProtocolPolicy::HttpsOnly
            } else {
                CloudFrontOriginProtocolPolicy::HttpOnly
            },
            http_port: 80,
            https_port: 443,
            ssl_protocols: if protocol == OriginProtocol::Https {
                BTreeSet::from([CloudFrontOriginSslProtocol::Tls12])
            } else {
                BTreeSet::new()
            },
            connection_attempts: 3,
            connection_timeout_seconds: 10,
            response_timeout_seconds: 30,
            keepalive_timeout_seconds: 5,
        }
    }

    fn policy() -> CloudFrontPublicOriginPolicy {
        CloudFrontPublicOriginPolicy::new(
            CloudResourceId::new("approved-origins").unwrap(),
            "approval-1",
            CloudResourceId::new("public-resolver").unwrap(),
            "resolver-1",
            BTreeSet::from([DomainName::new("example.com").unwrap()]),
        )
        .unwrap()
    }

    fn resolution(
        hostname: &str,
        address: &str,
        valid_until_unix_ms: i64,
    ) -> CloudFrontOriginResolution {
        CloudFrontOriginResolution {
            hostname: DomainName::new(hostname).unwrap(),
            cname_chain: vec![],
            resolved_addresses: BTreeSet::from([address.parse().unwrap()]),
            resolver_profile_id: CloudResourceId::new("public-resolver").unwrap(),
            resolver_profile_revision: "resolver-1".to_string(),
            observed_at_unix_ms: 1_000,
            valid_until_unix_ms,
        }
    }

    fn approval(
        inventory: &CloudFrontPlanningInventory,
        protocol: OriginProtocol,
    ) -> CloudFrontPublicOriginApproval {
        approval_for(inventory, "origin.example.com", protocol)
    }

    fn approval_for(
        inventory: &CloudFrontPlanningInventory,
        hostname: &str,
        protocol: OriginProtocol,
    ) -> CloudFrontPublicOriginApproval {
        policy()
            .approve(
                inventory,
                approval_request(
                    hostname,
                    protocol,
                    CloudFrontOriginEndpointClassification::PublicCustom,
                    "8.8.8.8",
                    2_000,
                ),
            )
            .unwrap()
    }

    fn approval_request(
        hostname: &str,
        protocol: OriginProtocol,
        classification: CloudFrontOriginEndpointClassification,
        address: &str,
        valid_until_unix_ms: i64,
    ) -> CloudFrontPublicOriginApprovalRequest {
        CloudFrontPublicOriginApprovalRequest {
            distribution_id: "E123EXAMPLE".to_string(),
            hostname: DomainName::new(hostname).unwrap(),
            protocol,
            classification,
            resolution: resolution(hostname, address, valid_until_unix_ms),
            now_unix_ms: 1_500,
        }
    }

    fn fragment_request(
        intent: CloudFrontCustomOriginIntent,
        secret_headers: Option<CloudFrontSecretHeaderBinding>,
    ) -> CloudFrontCustomOriginFragmentRequest {
        CloudFrontCustomOriginFragmentRequest {
            intent,
            secret_headers,
            distribution_id: "E123EXAMPLE".to_string(),
            operation: CloudFrontOriginOperation::Create,
            now_unix_ms: 1_500,
        }
    }

    fn second_fragment(inventory: &CloudFrontPlanningInventory) -> CloudFrontCustomOriginFragment {
        let mut second_pool = pool(OriginProtocol::Https, false);
        second_pool.spec.endpoints[0].name = OriginEndpointName::new("origin-b").unwrap();
        second_pool.spec.endpoints[0].address =
            OriginAddress::Hostname(DomainName::new("origin-b.example.com").unwrap());
        let mut second_intent = intent(OriginProtocol::Https);
        second_intent.endpoint_name = OriginEndpointName::new("origin-b").unwrap();
        second_intent.origin_id = CloudFrontOriginId::new("origin-b").unwrap();
        build_custom_origin_fragment(
            fragment_request(second_intent, None),
            &second_pool,
            &approval_for(inventory, "origin-b.example.com", OriginProtocol::Https),
            inventory,
        )
        .unwrap()
    }

    #[test]
    fn builds_only_observation_bound_public_custom_origin_fragments() {
        let inventory = inventory();
        let approval = approval(&inventory, OriginProtocol::Https);
        let fragment = build_custom_origin_fragment(
            fragment_request(intent(OriginProtocol::Https), None),
            &pool(OriginProtocol::Https, false),
            &approval,
            &inventory,
        )
        .unwrap();
        assert_eq!(fragment.hostname().as_str(), "origin.example.com");
        assert!(!serde_json::to_string(&fragment)
            .unwrap()
            .contains("dispatch"));
    }

    #[test]
    fn secret_fragment_contains_only_reference_and_requires_https() {
        let inventory = inventory();
        let secret = CloudFrontSecretHeaderBinding::new(
            CredentialRef::new("secret/origin").unwrap(),
            "secret-revision-1",
        )
        .unwrap();
        let fragment = build_custom_origin_fragment(
            fragment_request(intent(OriginProtocol::Https), Some(secret)),
            &pool(OriginProtocol::Https, true),
            &approval(&inventory, OriginProtocol::Https),
            &inventory,
        )
        .unwrap();
        let encoded = serde_json::to_string(&fragment).unwrap();
        assert!(encoded.contains("secret/origin"));
        assert!(!encoded.contains("header-name-canary"));
        assert!(!encoded.contains("header-value-canary"));

        let error = build_custom_origin_fragment(
            fragment_request(
                intent(OriginProtocol::Http),
                Some(
                    CloudFrontSecretHeaderBinding::new(
                        CredentialRef::new("secret/origin").unwrap(),
                        "secret-revision-1",
                    )
                    .unwrap(),
                ),
            ),
            &pool(OriginProtocol::Http, true),
            &approval(&inventory, OriginProtocol::Http),
            &inventory,
        )
        .unwrap_err();
        assert_eq!(
            error.code(),
            "cloudfront_secret_origin_requires_verified_https"
        );
    }

    #[test]
    fn resolved_secret_headers_are_revision_bound_and_never_enter_fragments() {
        let inventory = inventory();
        let fragment = build_custom_origin_fragment(
            fragment_request(
                intent(OriginProtocol::Https),
                Some(
                    CloudFrontSecretHeaderBinding::new(
                        CredentialRef::new("secret/origin").unwrap(),
                        "secret-revision-1",
                    )
                    .unwrap(),
                ),
            ),
            &pool(OriginProtocol::Https, true),
            &approval(&inventory, OriginProtocol::Https),
            &inventory,
        )
        .unwrap();
        let resolved = ResolvedCloudFrontOriginSecretHeaders::new(
            CredentialRef::new("secret/origin").unwrap(),
            "secret-revision-1",
            vec![(
                "X-Origin-Auth-Canary".to_string(),
                "header-value-canary".to_string(),
            )],
        )
        .unwrap();
        validate_resolved_origin_secret_headers(&fragment, &resolved).unwrap();
        let encoded = serde_json::to_string(&fragment).unwrap();
        assert!(!encoded.contains("X-Origin-Auth-Canary"));
        assert!(!encoded.contains("header-value-canary"));

        let stale = ResolvedCloudFrontOriginSecretHeaders::new(
            CredentialRef::new("secret/origin").unwrap(),
            "secret-revision-2",
            vec![(
                "X-Origin-Auth-Canary".to_string(),
                "header-value-canary".to_string(),
            )],
        )
        .unwrap();
        assert_eq!(
            validate_resolved_origin_secret_headers(&fragment, &stale)
                .unwrap_err()
                .code(),
            "cloudfront_origin_secret_revision_mismatch"
        );
        for headers in [
            vec![("x-public".to_string(), "duplicate".to_string())],
            vec![("Host".to_string(), "forbidden".to_string())],
        ] {
            let invalid = ResolvedCloudFrontOriginSecretHeaders::new(
                CredentialRef::new("secret/origin").unwrap(),
                "secret-revision-1",
                headers,
            )
            .unwrap();
            assert_eq!(
                validate_resolved_origin_secret_headers(&fragment, &invalid)
                    .unwrap_err()
                    .code(),
                "invalid_cloudfront_resolved_origin_secret_header"
            );
        }
    }

    #[test]
    fn group_and_method_contracts_are_primary_secondary_not_load_balancing() {
        let inventory = inventory();
        let first = build_custom_origin_fragment(
            fragment_request(intent(OriginProtocol::Https), None),
            &pool(OriginProtocol::Https, false),
            &approval(&inventory, OriginProtocol::Https),
            &inventory,
        )
        .unwrap();
        let second = second_fragment(&inventory);
        let group = build_origin_group_fragment(
            CloudFrontOriginGroupIntent {
                group_id: CloudFrontOriginGroupId::new("group-main").unwrap(),
                primary_origin_id: CloudFrontOriginId::new("origin-a").unwrap(),
                secondary_origin_id: CloudFrontOriginId::new("origin-b").unwrap(),
                failover_status_codes: BTreeSet::from(SUPPORTED_FAILOVER_CODES),
            },
            &[first, second],
            &inventory,
            "E123EXAMPLE",
            CloudFrontOriginGroupOperation::Create,
            1_500,
        )
        .unwrap();
        let target = CloudFrontOriginTargetRef::OriginGroup(group.group_id().clone());
        let reads = BTreeSet::from([
            CloudFrontViewerMethod::Get,
            CloudFrontViewerMethod::Head,
            CloudFrontViewerMethod::Options,
        ]);
        validate_target_methods(&target, &reads, &reads).unwrap();
        let writes = BTreeSet::from([
            CloudFrontViewerMethod::Get,
            CloudFrontViewerMethod::Head,
            CloudFrontViewerMethod::Options,
            CloudFrontViewerMethod::Post,
            CloudFrontViewerMethod::Put,
            CloudFrontViewerMethod::Patch,
            CloudFrontViewerMethod::Delete,
        ]);
        assert_eq!(
            validate_target_methods(
                &target,
                &writes,
                &BTreeSet::from([
                    CloudFrontViewerMethod::Get,
                    CloudFrontViewerMethod::Head,
                    CloudFrontViewerMethod::Options,
                ])
            )
            .unwrap_err()
            .code(),
            "cloudfront_origin_group_method_policy"
        );
    }

    #[test]
    fn sanitized_reference_evidence_never_authorizes_removal() {
        let inventory = inventory();
        let evidence = collect_target_references(
            &inventory,
            "E123EXAMPLE",
            CloudFrontOriginTargetRef::Origin(CloudFrontOriginId::new("origin-1").unwrap()),
            1_500,
        )
        .unwrap();
        assert!(!evidence.can_authorize_removal());
        assert!(evidence
            .references()
            .contains(&CloudFrontTargetReference::DefaultCacheBehavior));
    }

    #[test]
    fn private_and_documentation_addresses_are_not_public_approval() {
        for address in [
            "10.0.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "203.0.113.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
        ] {
            assert!(!is_public_unicast(address.parse().unwrap()), "{address}");
        }
        assert!(is_public_unicast("8.8.8.8".parse().unwrap()));
        assert!(is_public_unicast("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn approval_rejects_special_names_private_answers_and_unbounded_freshness() {
        let inventory = inventory();
        for (hostname, address) in [
            ("origin.localhost", "8.8.8.8"),
            ("origin.example.com", "10.0.0.1"),
            ("origin.example.com", "203.0.113.10"),
        ] {
            assert!(policy()
                .approve(
                    &inventory,
                    approval_request(
                        hostname,
                        OriginProtocol::Https,
                        CloudFrontOriginEndpointClassification::PublicCustom,
                        address,
                        2_000,
                    ),
                )
                .is_err());
        }
        assert!(policy()
            .approve(
                &inventory,
                approval_request(
                    "origin.example.com",
                    OriginProtocol::Https,
                    CloudFrontOriginEndpointClassification::PublicCustom,
                    "8.8.8.8",
                    1_000 + MAX_APPROVAL_FRESHNESS_MS + 1,
                ),
            )
            .is_err());

        for classification in [
            CloudFrontOriginEndpointClassification::S3Website,
            CloudFrontOriginEndpointClassification::VpcOrigin,
            CloudFrontOriginEndpointClassification::Unknown,
        ] {
            assert!(policy()
                .approve(
                    &inventory,
                    approval_request(
                        "origin.example.com",
                        OriginProtocol::Https,
                        classification,
                        "8.8.8.8",
                        2_000,
                    ),
                )
                .is_err());
        }
        assert!(policy()
            .approve(
                &inventory,
                approval_request(
                    "unapproved.net",
                    OriginProtocol::Https,
                    CloudFrontOriginEndpointClassification::PublicCustom,
                    "8.8.8.8",
                    2_000,
                ),
            )
            .is_err());

        let aws_public = policy()
            .approve(
                &inventory,
                approval_request(
                    "public-alb.example.com",
                    OriginProtocol::Https,
                    CloudFrontOriginEndpointClassification::PublicAwsCustom,
                    "8.8.8.8",
                    2_000,
                ),
            )
            .unwrap();
        assert_eq!(aws_public.hostname.as_str(), "public-alb.example.com");
    }

    #[test]
    fn secret_bundle_reserves_a_header_slot_during_fragment_planning() {
        let inventory = inventory();
        let secret = || {
            CloudFrontSecretHeaderBinding::new(
                CredentialRef::new("secret/origin").unwrap(),
                "secret-revision-1",
            )
            .unwrap()
        };
        let mut thirty = pool(OriginProtocol::Https, true);
        thirty.spec.endpoints[0].headers.literal = (0..30)
            .map(|index| (format!("X-Public-{index}"), "v".to_string()))
            .collect();
        assert_eq!(
            build_custom_origin_fragment(
                fragment_request(intent(OriginProtocol::Https), Some(secret())),
                &thirty,
                &approval(&inventory, OriginProtocol::Https),
                &inventory,
            )
            .unwrap_err()
            .code(),
            "cloudfront_origin_header_limit"
        );

        thirty.spec.endpoints[0]
            .headers
            .literal
            .remove("X-Public-29");
        let fragment = build_custom_origin_fragment(
            fragment_request(intent(OriginProtocol::Https), Some(secret())),
            &thirty,
            &approval(&inventory, OriginProtocol::Https),
            &inventory,
        )
        .unwrap();
        let resolved = ResolvedCloudFrontOriginSecretHeaders::new(
            CredentialRef::new("secret/origin").unwrap(),
            "secret-revision-1",
            vec![("X-Secret".to_string(), "value".to_string())],
        )
        .unwrap();
        validate_resolved_origin_secret_headers(&fragment, &resolved).unwrap();
    }

    #[test]
    fn origin_group_create_quota_comes_from_the_observed_distribution() {
        let mut inventory = inventory();
        let CloudFrontDetailObservation::Complete(observed) =
            &mut inventory.inventory_mut().entries[0].detail
        else {
            unreachable!();
        };
        let mut second: CloudFrontOriginProjection = observed.detail.config.origins[0].clone();
        second.id = "origin-2".to_string();
        observed.detail.config.origins.push(second);
        observed.detail.config.origin_groups = (0..MAX_ORIGIN_GROUPS)
            .map(|index| CloudFrontOriginGroupProjection {
                id: format!("existing-{index}"),
                primary_origin_id: "origin-1".to_string(),
                secondary_origin_id: "origin-2".to_string(),
                failover_status_codes: BTreeSet::from([503]),
                unsupported_features: BTreeSet::new(),
            })
            .collect();
        let first = build_custom_origin_fragment(
            fragment_request(intent(OriginProtocol::Https), None),
            &pool(OriginProtocol::Https, false),
            &approval(&inventory, OriginProtocol::Https),
            &inventory,
        )
        .unwrap();
        let second = second_fragment(&inventory);
        build_origin_group_fragment(
            CloudFrontOriginGroupIntent {
                group_id: CloudFrontOriginGroupId::new("existing-0").unwrap(),
                primary_origin_id: CloudFrontOriginId::new("origin-a").unwrap(),
                secondary_origin_id: CloudFrontOriginId::new("origin-b").unwrap(),
                failover_status_codes: BTreeSet::from([503]),
            },
            &[first.clone(), second.clone()],
            &inventory,
            "E123EXAMPLE",
            CloudFrontOriginGroupOperation::Update,
            1_500,
        )
        .unwrap();
        let error = build_origin_group_fragment(
            CloudFrontOriginGroupIntent {
                group_id: CloudFrontOriginGroupId::new("group-main").unwrap(),
                primary_origin_id: CloudFrontOriginId::new("origin-a").unwrap(),
                secondary_origin_id: CloudFrontOriginId::new("origin-b").unwrap(),
                failover_status_codes: BTreeSet::from(SUPPORTED_FAILOVER_CODES),
            },
            &[first, second],
            &inventory,
            "E123EXAMPLE",
            CloudFrontOriginGroupOperation::Create,
            1_500,
        )
        .unwrap_err();
        assert_eq!(error.code(), "invalid_cloudfront_origin_group_intent");
    }

    #[test]
    fn cloudfront_specific_mapping_rejects_ip_weight_drain_and_forbidden_headers() {
        let inventory = inventory();
        let public_approval = approval(&inventory, OriginProtocol::Https);

        let mut invalid_pool = pool(OriginProtocol::Https, false);
        invalid_pool.spec.endpoints[0].weight = 2;
        assert!(build_custom_origin_fragment(
            fragment_request(intent(OriginProtocol::Https), None),
            &invalid_pool,
            &public_approval,
            &inventory,
        )
        .is_err());

        let mut invalid_pool = pool(OriginProtocol::Https, false);
        invalid_pool.spec.endpoints[0].drain = OriginDrainState::Draining;
        assert!(build_custom_origin_fragment(
            fragment_request(intent(OriginProtocol::Https), None),
            &invalid_pool,
            &public_approval,
            &inventory,
        )
        .is_err());

        let mut invalid_pool = pool(OriginProtocol::Https, false);
        invalid_pool.spec.endpoints[0].headers.literal =
            BTreeMap::from([("X-Amz-Credential".to_string(), "not-public".to_string())]);
        assert_eq!(
            build_custom_origin_fragment(
                fragment_request(intent(OriginProtocol::Https), None),
                &invalid_pool,
                &public_approval,
                &inventory,
            )
            .unwrap_err()
            .code(),
            "invalid_cloudfront_origin_header"
        );

        let mut invalid_pool = pool(OriginProtocol::Https, false);
        invalid_pool.spec.endpoints[0].address = OriginAddress::Ip("8.8.8.8".parse().unwrap());
        assert_eq!(
            build_custom_origin_fragment(
                fragment_request(intent(OriginProtocol::Https), None),
                &invalid_pool,
                &public_approval,
                &inventory,
            )
            .unwrap_err()
            .code(),
            "cloudfront_public_hostname_origin_required"
        );
    }

    #[test]
    fn origin_group_options_must_be_cached_and_invalid_members_fail_closed() {
        let inventory = inventory();
        let first = build_custom_origin_fragment(
            fragment_request(intent(OriginProtocol::Https), None),
            &pool(OriginProtocol::Https, false),
            &approval(&inventory, OriginProtocol::Https),
            &inventory,
        )
        .unwrap();
        let bad_group = CloudFrontOriginGroupIntent {
            group_id: CloudFrontOriginGroupId::new("group-main").unwrap(),
            primary_origin_id: CloudFrontOriginId::new("origin-a").unwrap(),
            secondary_origin_id: CloudFrontOriginId::new("missing").unwrap(),
            failover_status_codes: BTreeSet::from([503]),
        };
        assert_eq!(
            build_origin_group_fragment(
                bad_group,
                &[first],
                &inventory,
                "E123EXAMPLE",
                CloudFrontOriginGroupOperation::Create,
                1_500,
            )
            .unwrap_err()
            .code(),
            "cloudfront_origin_group_member_missing"
        );

        let allowed = BTreeSet::from([
            CloudFrontViewerMethod::Get,
            CloudFrontViewerMethod::Head,
            CloudFrontViewerMethod::Options,
        ]);
        let cached = BTreeSet::from([CloudFrontViewerMethod::Get, CloudFrontViewerMethod::Head]);
        assert_eq!(
            validate_target_methods(
                &CloudFrontOriginTargetRef::OriginGroup(
                    CloudFrontOriginGroupId::new("group-main").unwrap()
                ),
                &allowed,
                &cached,
            )
            .unwrap_err()
            .code(),
            "cloudfront_origin_group_method_policy"
        );
    }

    #[test]
    fn origin_paths_require_safe_percent_encoding() {
        for path in [
            "relative",
            "/trailing/",
            "/space here",
            "/bad%2",
            "/query?x=1",
        ] {
            assert!(validate_origin_path(path).is_err(), "{path}");
        }
        for path in ["", "/api", "/encoded%20path"] {
            validate_origin_path(path).unwrap();
        }
    }

    #[test]
    fn observed_target_namespace_and_origin_operations_fail_closed() {
        let inventory = inventory();
        let mut colliding_intent = intent(OriginProtocol::Https);
        colliding_intent.origin_id = CloudFrontOriginId::new("origin-1").unwrap();
        assert_eq!(
            build_custom_origin_fragment(
                fragment_request(colliding_intent, None),
                &pool(OriginProtocol::Https, false),
                &approval(&inventory, OriginProtocol::Https),
                &inventory,
            )
            .unwrap_err()
            .code(),
            "invalid_cloudfront_origin_operation"
        );

        let mut missing_update = fragment_request(intent(OriginProtocol::Https), None);
        missing_update.operation = CloudFrontOriginOperation::Update;
        assert_eq!(
            build_custom_origin_fragment(
                missing_update,
                &pool(OriginProtocol::Https, false),
                &approval(&inventory, OriginProtocol::Https),
                &inventory,
            )
            .unwrap_err()
            .code(),
            "invalid_cloudfront_origin_operation"
        );

        let first = build_custom_origin_fragment(
            fragment_request(intent(OriginProtocol::Https), None),
            &pool(OriginProtocol::Https, false),
            &approval(&inventory, OriginProtocol::Https),
            &inventory,
        )
        .unwrap();
        let second = second_fragment(&inventory);
        let mut stale_first = first.clone();
        stale_first.public_origin_approval.valid_until_unix_ms = 1_400;
        assert_eq!(
            build_origin_group_fragment(
                CloudFrontOriginGroupIntent {
                    group_id: CloudFrontOriginGroupId::new("group-stale").unwrap(),
                    primary_origin_id: CloudFrontOriginId::new("origin-a").unwrap(),
                    secondary_origin_id: CloudFrontOriginId::new("origin-b").unwrap(),
                    failover_status_codes: BTreeSet::from([503]),
                },
                &[stale_first, second.clone()],
                &inventory,
                "E123EXAMPLE",
                CloudFrontOriginGroupOperation::Create,
                1_500,
            )
            .unwrap_err()
            .code(),
            "invalid_cloudfront_public_origin_approval"
        );
        assert_eq!(
            build_origin_group_fragment(
                CloudFrontOriginGroupIntent {
                    group_id: CloudFrontOriginGroupId::new("origin-1").unwrap(),
                    primary_origin_id: CloudFrontOriginId::new("origin-a").unwrap(),
                    secondary_origin_id: CloudFrontOriginId::new("origin-b").unwrap(),
                    failover_status_codes: BTreeSet::from([503]),
                },
                &[first, second],
                &inventory,
                "E123EXAMPLE",
                CloudFrontOriginGroupOperation::Create,
                1_500,
            )
            .unwrap_err()
            .code(),
            "invalid_cloudfront_origin_group_intent"
        );
    }

    #[test]
    fn methods_use_only_provider_shapes_and_options_cache_parity() {
        let target =
            CloudFrontOriginTargetRef::Origin(CloudFrontOriginId::new("origin-a").unwrap());
        let read = BTreeSet::from([CloudFrontViewerMethod::Get, CloudFrontViewerMethod::Head]);
        validate_target_methods(&target, &read, &read).unwrap();
        let read_options = BTreeSet::from([
            CloudFrontViewerMethod::Get,
            CloudFrontViewerMethod::Head,
            CloudFrontViewerMethod::Options,
        ]);
        validate_target_methods(&target, &read_options, &read_options).unwrap();
        validate_target_methods(&target, &read_options, &read).unwrap();
        let invalid = BTreeSet::from([
            CloudFrontViewerMethod::Get,
            CloudFrontViewerMethod::Head,
            CloudFrontViewerMethod::Post,
        ]);
        assert_eq!(
            validate_target_methods(&target, &invalid, &read)
                .unwrap_err()
                .code(),
            "invalid_cloudfront_behavior_methods"
        );
    }
}
