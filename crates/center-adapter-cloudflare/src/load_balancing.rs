//! Cloudflare Load Balancing models, API seam, and fail-closed planning.
//!
//! This module deliberately does not reconcile resources. It translates a
//! provider-neutral origin pool into explicit Cloudflare objects, checks a
//! fresh entitlement/quota observation, and produces an expand/verify/contract
//! plan which a durable operation executor can apply later.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use edgion_center_core::{
    HealthCheckMethod, HealthCheckSourceScope, NormalizedProviderError, OriginAddress,
    OriginDrainState, OriginFailoverMode, OriginHealthObservation, OriginPoolSpec, OriginProtocol,
    OriginTlsMode, ProviderErrorCategory,
};
use reqwest::Method;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{CloudflareApiResult, CloudflareHttpApi};

const MAX_CLOUDFLARE_NAME: usize = 64;
const MAX_SESSION_AFFINITY_TTL: u32 = 604_800;
const MAX_DRAIN_SECONDS: u32 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CloudflareLoadBalancingId(String);

impl CloudflareLoadBalancingId {
    pub fn new(value: impl Into<String>) -> CloudflareApiResult<Self> {
        let value = value.into();
        if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(validation("invalid_cloudflare_load_balancing_id"));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for CloudflareLoadBalancingId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for CloudflareLoadBalancingId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CloudflareLoadBalancingId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(|_| serde::de::Error::custom("invalid Cloudflare resource ID"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareMonitorType {
    Http,
    Https,
    Tcp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareMonitor {
    pub id: CloudflareLoadBalancingId,
    #[serde(rename = "type")]
    pub monitor_type: CloudflareMonitorType,
    pub description: String,
    pub method: Option<String>,
    pub path: Option<String>,
    pub port: u16,
    pub header: Option<BTreeMap<String, Vec<String>>>,
    pub expected_codes: Option<String>,
    pub expected_body: Option<String>,
    pub interval: u32,
    pub timeout: u32,
    pub retries: u16,
    pub consecutive_up: u16,
    pub consecutive_down: u16,
    pub allow_insecure: bool,
    pub follow_redirects: bool,
    pub created_on: Option<String>,
    pub modified_on: Option<String>,
    #[serde(flatten)]
    pub unsupported_fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareMonitorRequest {
    #[serde(rename = "type")]
    pub monitor_type: CloudflareMonitorType,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<BTreeMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_codes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_body: Option<String>,
    pub interval: u32,
    pub timeout: u32,
    pub retries: u16,
    pub consecutive_up: u16,
    pub consecutive_down: u16,
    pub allow_insecure: bool,
    pub follow_redirects: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CloudflareHealthRegion {
    #[serde(rename = "WNAM")]
    WesternNorthAmerica,
    #[serde(rename = "ENAM")]
    EasternNorthAmerica,
    #[serde(rename = "WEU")]
    WesternEurope,
    #[serde(rename = "EEU")]
    EasternEurope,
    #[serde(rename = "NSAM")]
    NorthernSouthAmerica,
    #[serde(rename = "SSAM")]
    SouthernSouthAmerica,
    #[serde(rename = "OC")]
    Oceania,
    #[serde(rename = "ME")]
    MiddleEast,
    #[serde(rename = "NAF")]
    NorthernAfrica,
    #[serde(rename = "SAF")]
    SouthernAfrica,
    #[serde(rename = "SAS")]
    SouthernAsia,
    #[serde(rename = "SEAS")]
    SouthEastAsia,
    #[serde(rename = "NEAS")]
    NorthEastAsia,
    #[serde(rename = "ALL_REGIONS")]
    AllRegions,
}

/// Cloudflare represents origin weights in hundredths between zero and one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CloudflareOriginWeight(u8);

impl CloudflareOriginWeight {
    pub fn from_basis_points(value: u8) -> Self {
        Self(value)
    }

    pub fn basis_points(self) -> u8 {
        self.0
    }
}

impl Serialize for CloudflareOriginWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(f64::from(self.0) / 100.0)
    }
}

impl<'de> Deserialize<'de> for CloudflareOriginWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(serde::de::Error::custom("invalid Cloudflare origin weight"));
        }
        let scaled = value * 100.0;
        if (scaled.round() - scaled).abs() > f64::EPSILON {
            return Err(serde::de::Error::custom(
                "Cloudflare origin weight is not a multiple of 0.01",
            ));
        }
        Ok(Self(scaled.round() as u8))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflarePoolOrigin {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub enabled: bool,
    pub weight: CloudflareOriginWeight,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedCloudflarePoolOrigin {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub enabled: bool,
    pub weight: CloudflareOriginWeight,
    pub header: Option<BTreeMap<String, Vec<String>>>,
    #[serde(flatten)]
    pub unsupported_fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareOriginSteeringPolicy {
    Random,
    Hash,
    LeastOutstandingRequests,
    LeastConnections,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareOriginSteering {
    pub policy: CloudflareOriginSteeringPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflarePool {
    pub id: CloudflareLoadBalancingId,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub minimum_origins: u16,
    pub monitor: Option<CloudflareLoadBalancingId>,
    #[serde(default)]
    pub check_regions: BTreeSet<CloudflareHealthRegion>,
    pub origin_steering: CloudflareOriginSteering,
    pub origins: Vec<ObservedCloudflarePoolOrigin>,
    pub created_on: Option<String>,
    pub modified_on: Option<String>,
    #[serde(flatten)]
    pub unsupported_fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflarePoolRequest {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub minimum_origins: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor: Option<CloudflareLoadBalancingId>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty", default)]
    pub check_regions: BTreeSet<CloudflareHealthRegion>,
    pub origin_steering: CloudflareOriginSteering,
    pub origins: Vec<CloudflarePoolOrigin>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareSteeringPolicy {
    Off,
    Random,
    Geo,
    DynamicLatency,
    Proximity,
    LeastOutstandingRequests,
    LeastConnections,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareSessionAffinity {
    None,
    Cookie,
    IpCookie,
    Header,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareZeroDowntimeFailover {
    None,
    Temporary,
    Sticky,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareSessionAffinityAttributes {
    #[serde(default)]
    pub headers: BTreeSet<String>,
    pub require_all_headers: bool,
    pub drain_duration: u32,
    pub zero_downtime_failover: CloudflareZeroDowntimeFailover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareRandomSteering {
    /// Pool IDs mapped to Cloudflare's 0.1 pool-weight units.
    pub pool_weights: BTreeMap<CloudflareLoadBalancingId, CloudflarePoolWeight>,
}

/// Cloudflare random-steering pool weights use tenths, unlike origin weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CloudflarePoolWeight(u8);

impl CloudflarePoolWeight {
    pub fn new(tenths: u8) -> CloudflareApiResult<Self> {
        if tenths > 10 {
            return Err(validation("invalid_cloudflare_pool_weight"));
        }
        Ok(Self(tenths))
    }
}

impl Serialize for CloudflarePoolWeight {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(f64::from(self.0) / 10.0)
    }
}

impl<'de> Deserialize<'de> for CloudflarePoolWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        let scaled = value * 10.0;
        if !value.is_finite()
            || !(0.0..=1.0).contains(&value)
            || (scaled.round() - scaled).abs() > f64::EPSILON
        {
            return Err(serde::de::Error::custom("invalid Cloudflare pool weight"));
        }
        Ok(Self(scaled.round() as u8))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareLoadBalancer {
    pub id: CloudflareLoadBalancingId,
    pub name: String,
    pub enabled: bool,
    pub proxied: bool,
    pub steering_policy: CloudflareSteeringPolicy,
    pub default_pools: Vec<CloudflareLoadBalancingId>,
    pub fallback_pool: CloudflareLoadBalancingId,
    pub session_affinity: CloudflareSessionAffinity,
    pub session_affinity_ttl: u32,
    pub session_affinity_attributes: CloudflareSessionAffinityAttributes,
    pub random_steering: Option<CloudflareRandomSteering>,
    pub created_on: Option<String>,
    pub modified_on: Option<String>,
    #[serde(flatten)]
    pub unsupported_fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareLoadBalancerRequest {
    pub name: String,
    pub enabled: bool,
    pub proxied: bool,
    pub steering_policy: CloudflareSteeringPolicy,
    pub default_pools: Vec<CloudflareLoadBalancingId>,
    pub fallback_pool: CloudflareLoadBalancingId,
    pub session_affinity: CloudflareSessionAffinity,
    pub session_affinity_ttl: u32,
    pub session_affinity_attributes: CloudflareSessionAffinityAttributes,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub random_steering: Option<CloudflareRandomSteering>,
}

impl CloudflareLoadBalancerRequest {
    pub fn validate(&self) -> CloudflareApiResult<()> {
        if self.name.is_empty()
            || self.name.len() > 253
            || self.default_pools.is_empty()
            || self.session_affinity_ttl > MAX_SESSION_AFFINITY_TTL
            || self.session_affinity_attributes.drain_duration > MAX_DRAIN_SECONDS
        {
            return Err(validation("invalid_cloudflare_load_balancer"));
        }
        if self.session_affinity == CloudflareSessionAffinity::Header
            && self.session_affinity_attributes.headers.is_empty()
        {
            return Err(validation("header_affinity_requires_headers"));
        }
        let ttl_valid = match self.session_affinity {
            CloudflareSessionAffinity::None => self.session_affinity_ttl == 0,
            CloudflareSessionAffinity::Cookie | CloudflareSessionAffinity::IpCookie => {
                (1_800..=MAX_SESSION_AFFINITY_TTL).contains(&self.session_affinity_ttl)
            }
            CloudflareSessionAffinity::Header => (30..=3_600).contains(&self.session_affinity_ttl),
        };
        if !ttl_valid {
            return Err(validation("cloudflare_session_affinity_ttl_invalid"));
        }
        if self.session_affinity != CloudflareSessionAffinity::Header
            && !self.session_affinity_attributes.headers.is_empty()
        {
            return Err(validation("affinity_headers_require_header_mode"));
        }
        if self.session_affinity_attributes.drain_duration != 0
            && (!self.proxied || self.session_affinity == CloudflareSessionAffinity::None)
        {
            return Err(validation(
                "cloudflare_endpoint_drain_configuration_invalid",
            ));
        }
        if !self.proxied && self.session_affinity != CloudflareSessionAffinity::None {
            return Err(validation("dns_only_affinity_unsupported"));
        }
        if self.session_affinity == CloudflareSessionAffinity::Header
            && self.session_affinity_attributes.zero_downtime_failover
                == CloudflareZeroDowntimeFailover::Sticky
        {
            return Err(validation(
                "sticky_failover_with_header_affinity_unsupported",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareReferenceType {
    Referral,
    Referrer,
    #[serde(rename = "*")]
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareReferencedResourceType {
    LoadBalancer,
    Monitor,
    Pool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareResourceReference {
    pub reference_type: CloudflareReferenceType,
    pub resource_id: CloudflareLoadBalancingId,
    pub resource_name: String,
    pub resource_type: CloudflareReferencedResourceType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareDeleteResult {
    pub id: CloudflareLoadBalancingId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareCanonicalRevision(String);

impl CloudflareCanonicalRevision {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Best-effort provider revision plus an exact Center ownership-record fence.
/// Cloudflare does not expose a conditional ETag for these mutations, so the
/// adapter re-observes immediately before its single PATCH/DELETE request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareOwnershipClaim {
    pub scope: CloudflareMutationScope,
    pub resource_kind: CloudflareReferencedResourceType,
    pub resource_id: CloudflareLoadBalancingId,
    pub expected_provider_revision: CloudflareCanonicalRevision,
    pub center_ownership_revision: String,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareMutationScope {
    Account(String),
    Zone(String),
}

impl CloudflareOwnershipClaim {
    fn validate(
        &self,
        expected_scope: &CloudflareMutationScope,
        expected_kind: CloudflareReferencedResourceType,
        expected_id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<()> {
        let now_unix_ms = current_unix_ms()?;
        if self.center_ownership_revision.is_empty()
            || self.center_ownership_revision.len() > 256
            || self.center_ownership_revision.chars().any(char::is_control)
            || self.scope != *expected_scope
            || self.resource_kind != expected_kind
            || self.resource_id != *expected_id
            || now_unix_ms < self.observed_at_unix_ms
            || now_unix_ms >= self.valid_until_unix_ms
        {
            return Err(validation("invalid_cloudflare_ownership_fence"));
        }
        Ok(())
    }
}

#[async_trait]
pub trait CloudflareOwnershipVerifier: Send + Sync {
    /// Verifies the claim against the composition-owned adoption/ownership
    /// registry. Success must mean the exact ownership revision is current.
    async fn verify(&self, claim: &CloudflareOwnershipClaim) -> CloudflareApiResult<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedCloudflareMutation {
    claim: CloudflareOwnershipClaim,
}

pub async fn authorize_cloudflare_mutation<V: CloudflareOwnershipVerifier>(
    verifier: &V,
    claim: CloudflareOwnershipClaim,
) -> CloudflareApiResult<AuthorizedCloudflareMutation> {
    claim.validate(&claim.scope, claim.resource_kind, &claim.resource_id)?;
    verifier.verify(&claim).await?;
    // Verification may cross a freshness boundary.
    claim.validate(&claim.scope, claim.resource_kind, &claim.resource_id)?;
    Ok(AuthorizedCloudflareMutation { claim })
}

impl AuthorizedCloudflareMutation {
    fn validate(
        &self,
        expected_scope: &CloudflareMutationScope,
        expected_kind: CloudflareReferencedResourceType,
        expected_id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<()> {
        self.claim
            .validate(expected_scope, expected_kind, expected_id)
    }
}

fn current_unix_ms() -> CloudflareApiResult<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| validation("cloudflare_system_time_invalid"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| validation("cloudflare_system_time_invalid"))
}

pub fn cloudflare_canonical_revision<T: Serialize>(
    value: &T,
) -> CloudflareApiResult<CloudflareCanonicalRevision> {
    let encoded =
        serde_json::to_vec(value).map_err(|_| validation("cloudflare_revision_encoding_failed"))?;
    let digest = Sha256::digest(encoded);
    Ok(CloudflareCanonicalRevision(
        digest.iter().map(|byte| format!("{byte:02x}")).collect(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflarePoolHealth {
    pub pool_id: CloudflareLoadBalancingId,
    #[serde(default)]
    pub pop_health: BTreeMap<String, CloudflarePopHealth>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflarePopHealth {
    pub healthy: bool,
    #[serde(default)]
    pub origins: Vec<BTreeMap<String, CloudflareOriginHealth>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudflareOriginHealth {
    pub healthy: bool,
    pub failure_reason: Option<String>,
    pub response_code: Option<u16>,
    pub rtt: Option<String>,
}

/// Provider and Center health are intentionally carried side by side. Neither
/// source overrides, fills, or upgrades the other source's observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndependentOriginHealth {
    pub provider: Option<CloudflareOriginHealth>,
    pub center: Option<OriginHealthObservation>,
}

#[async_trait]
pub trait CloudflareLoadBalancingApi: Send + Sync {
    async fn create_monitor(
        &self,
        account_id: &str,
        request: &CloudflareMonitorRequest,
    ) -> CloudflareApiResult<CloudflareMonitor>;
    async fn get_monitor(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<CloudflareMonitor>;
    async fn update_monitor(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
        request: &CloudflareMonitorRequest,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareMonitor>;
    async fn monitor_references(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<Vec<CloudflareResourceReference>>;
    async fn delete_monitor(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareDeleteResult>;

    async fn create_pool(
        &self,
        account_id: &str,
        request: &CloudflarePoolRequest,
    ) -> CloudflareApiResult<CloudflarePool>;
    async fn get_pool(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<CloudflarePool>;
    async fn update_pool(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
        request: &CloudflarePoolRequest,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflarePool>;
    async fn pool_references(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<Vec<CloudflareResourceReference>>;
    async fn pool_health(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<CloudflarePoolHealth>;
    async fn delete_pool(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareDeleteResult>;

    async fn create_load_balancer(
        &self,
        zone_id: &str,
        request: &CloudflareLoadBalancerRequest,
    ) -> CloudflareApiResult<CloudflareLoadBalancer>;
    async fn get_load_balancer(
        &self,
        zone_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<CloudflareLoadBalancer>;
    async fn update_load_balancer(
        &self,
        zone_id: &str,
        id: &CloudflareLoadBalancingId,
        request: &CloudflareLoadBalancerRequest,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareLoadBalancer>;
    async fn delete_load_balancer(
        &self,
        zone_id: &str,
        id: &CloudflareLoadBalancingId,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareDeleteResult>;
}

#[async_trait]
impl CloudflareLoadBalancingApi for CloudflareHttpApi {
    async fn create_monitor(
        &self,
        account_id: &str,
        request: &CloudflareMonitorRequest,
    ) -> CloudflareApiResult<CloudflareMonitor> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        let observed =
            mutate_json(self, Method::POST, &monitor_path(account_id, None), request).await?;
        validate_monitor_result(&observed, request, None)?;
        Ok(observed)
    }

    async fn get_monitor(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<CloudflareMonitor> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        self.read_result(&monitor_path(account_id, Some(id)), &[])
            .await
    }

    async fn update_monitor(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
        request: &CloudflareMonitorRequest,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareMonitor> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        authorization.validate(
            &CloudflareMutationScope::Account(account_id.to_string()),
            CloudflareReferencedResourceType::Monitor,
            id,
        )?;
        let current = self.get_monitor(account_id, id).await?;
        validate_observation(&current.unsupported_fields, &current, authorization)?;
        let observed = mutate_json(
            self,
            Method::PATCH,
            &monitor_path(account_id, Some(id)),
            request,
        )
        .await?;
        validate_monitor_result(&observed, request, Some(id))?;
        Ok(observed)
    }

    async fn monitor_references(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<Vec<CloudflareResourceReference>> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        self.read_result(
            &format!("{}/references", monitor_path(account_id, Some(id))),
            &[],
        )
        .await
    }

    async fn delete_monitor(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareDeleteResult> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        authorization.validate(
            &CloudflareMutationScope::Account(account_id.to_string()),
            CloudflareReferencedResourceType::Monitor,
            id,
        )?;
        let current = self.get_monitor(account_id, id).await?;
        validate_observation(&current.unsupported_fields, &current, authorization)?;
        let result = self
            .mutation_result(Method::DELETE, &monitor_path(account_id, Some(id)), None)
            .await?;
        ensure_deleted_id(result, id, "cloudflare_monitor_delete_response_mismatch")
    }

    async fn create_pool(
        &self,
        account_id: &str,
        request: &CloudflarePoolRequest,
    ) -> CloudflareApiResult<CloudflarePool> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        let observed =
            mutate_json(self, Method::POST, &pool_path(account_id, None), request).await?;
        validate_pool_result(&observed, request, None)?;
        Ok(observed)
    }

    async fn get_pool(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<CloudflarePool> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        self.read_result(&pool_path(account_id, Some(id)), &[])
            .await
    }

    async fn update_pool(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
        request: &CloudflarePoolRequest,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflarePool> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        authorization.validate(
            &CloudflareMutationScope::Account(account_id.to_string()),
            CloudflareReferencedResourceType::Pool,
            id,
        )?;
        let current = self.get_pool(account_id, id).await?;
        validate_pool_observation(&current, authorization)?;
        let observed = mutate_json(
            self,
            Method::PATCH,
            &pool_path(account_id, Some(id)),
            request,
        )
        .await?;
        validate_pool_result(&observed, request, Some(id))?;
        Ok(observed)
    }

    async fn pool_references(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<Vec<CloudflareResourceReference>> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        self.read_result(
            &format!("{}/references", pool_path(account_id, Some(id))),
            &[],
        )
        .await
    }

    async fn pool_health(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<CloudflarePoolHealth> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        self.read_result(&format!("{}/health", pool_path(account_id, Some(id))), &[])
            .await
    }

    async fn delete_pool(
        &self,
        account_id: &str,
        id: &CloudflareLoadBalancingId,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareDeleteResult> {
        validate_provider_id(account_id, "invalid_cloudflare_account_id")?;
        authorization.validate(
            &CloudflareMutationScope::Account(account_id.to_string()),
            CloudflareReferencedResourceType::Pool,
            id,
        )?;
        let current = self.get_pool(account_id, id).await?;
        validate_pool_observation(&current, authorization)?;
        let result = self
            .mutation_result(Method::DELETE, &pool_path(account_id, Some(id)), None)
            .await?;
        ensure_deleted_id(result, id, "cloudflare_pool_delete_response_mismatch")
    }

    async fn create_load_balancer(
        &self,
        zone_id: &str,
        request: &CloudflareLoadBalancerRequest,
    ) -> CloudflareApiResult<CloudflareLoadBalancer> {
        validate_provider_id(zone_id, "invalid_cloudflare_zone_id")?;
        request.validate()?;
        let observed = mutate_json(
            self,
            Method::POST,
            &load_balancer_path(zone_id, None),
            request,
        )
        .await?;
        validate_load_balancer_result(&observed, request, None)?;
        Ok(observed)
    }

    async fn get_load_balancer(
        &self,
        zone_id: &str,
        id: &CloudflareLoadBalancingId,
    ) -> CloudflareApiResult<CloudflareLoadBalancer> {
        validate_provider_id(zone_id, "invalid_cloudflare_zone_id")?;
        self.read_result(&load_balancer_path(zone_id, Some(id)), &[])
            .await
    }

    async fn update_load_balancer(
        &self,
        zone_id: &str,
        id: &CloudflareLoadBalancingId,
        request: &CloudflareLoadBalancerRequest,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareLoadBalancer> {
        validate_provider_id(zone_id, "invalid_cloudflare_zone_id")?;
        request.validate()?;
        authorization.validate(
            &CloudflareMutationScope::Zone(zone_id.to_string()),
            CloudflareReferencedResourceType::LoadBalancer,
            id,
        )?;
        let current = self.get_load_balancer(zone_id, id).await?;
        validate_observation(&current.unsupported_fields, &current, authorization)?;
        let observed = mutate_json(
            self,
            Method::PATCH,
            &load_balancer_path(zone_id, Some(id)),
            request,
        )
        .await?;
        validate_load_balancer_result(&observed, request, Some(id))?;
        Ok(observed)
    }

    async fn delete_load_balancer(
        &self,
        zone_id: &str,
        id: &CloudflareLoadBalancingId,
        authorization: &AuthorizedCloudflareMutation,
    ) -> CloudflareApiResult<CloudflareDeleteResult> {
        validate_provider_id(zone_id, "invalid_cloudflare_zone_id")?;
        authorization.validate(
            &CloudflareMutationScope::Zone(zone_id.to_string()),
            CloudflareReferencedResourceType::LoadBalancer,
            id,
        )?;
        let current = self.get_load_balancer(zone_id, id).await?;
        validate_observation(&current.unsupported_fields, &current, authorization)?;
        let result = self
            .mutation_result(Method::DELETE, &load_balancer_path(zone_id, Some(id)), None)
            .await?;
        ensure_deleted_id(
            result,
            id,
            "cloudflare_load_balancer_delete_response_mismatch",
        )
    }
}

async fn mutate_json<T, B>(
    api: &CloudflareHttpApi,
    method: Method,
    path: &str,
    body: &B,
) -> CloudflareApiResult<T>
where
    T: for<'de> Deserialize<'de>,
    B: Serialize,
{
    let body = serde_json::to_value(body)
        .map_err(|_| validation("cloudflare_load_balancing_encoding_failed"))?;
    api.mutation_result(method, path, Some(&body)).await
}

fn validate_observation<T: Serialize>(
    unsupported_fields: &BTreeMap<String, Value>,
    observed: &T,
    authorization: &AuthorizedCloudflareMutation,
) -> CloudflareApiResult<()> {
    if !unsupported_fields.is_empty() {
        return Err(conflict("cloudflare_resource_has_unsupported_fields"));
    }
    if cloudflare_canonical_revision(observed)? != authorization.claim.expected_provider_revision {
        return Err(conflict("cloudflare_resource_revision_changed"));
    }
    Ok(())
}

fn validate_pool_observation(
    observed: &CloudflarePool,
    authorization: &AuthorizedCloudflareMutation,
) -> CloudflareApiResult<()> {
    if observed
        .origins
        .iter()
        .any(|origin| !origin.unsupported_fields.is_empty())
    {
        return Err(conflict("cloudflare_resource_has_unsupported_fields"));
    }
    validate_observation(&observed.unsupported_fields, observed, authorization)
}

fn ensure_deleted_id(
    result: CloudflareDeleteResult,
    expected: &CloudflareLoadBalancingId,
    code: &'static str,
) -> CloudflareApiResult<CloudflareDeleteResult> {
    if result.id != *expected {
        return Err(unknown_outcome(code));
    }
    Ok(result)
}

fn validate_monitor_result(
    observed: &CloudflareMonitor,
    request: &CloudflareMonitorRequest,
    expected_id: Option<&CloudflareLoadBalancingId>,
) -> CloudflareApiResult<()> {
    if !observed.unsupported_fields.is_empty()
        || expected_id.is_some_and(|id| observed.id != *id)
        || observed.monitor_type != request.monitor_type
        || observed.description != request.description
        || observed.method != request.method
        || observed.path != request.path
        || observed.port != request.port
        || observed.header != request.header
        || observed.expected_codes != request.expected_codes
        || observed.expected_body != request.expected_body
        || observed.interval != request.interval
        || observed.timeout != request.timeout
        || observed.retries != request.retries
        || observed.consecutive_up != request.consecutive_up
        || observed.consecutive_down != request.consecutive_down
        || observed.allow_insecure != request.allow_insecure
        || observed.follow_redirects != request.follow_redirects
    {
        return Err(unknown_outcome("cloudflare_monitor_response_mismatch"));
    }
    Ok(())
}

fn validate_pool_result(
    observed: &CloudflarePool,
    request: &CloudflarePoolRequest,
    expected_id: Option<&CloudflareLoadBalancingId>,
) -> CloudflareApiResult<()> {
    if !observed.unsupported_fields.is_empty()
        || expected_id.is_some_and(|id| observed.id != *id)
        || observed.name != request.name
        || observed.description != request.description
        || observed.enabled != request.enabled
        || observed.minimum_origins != request.minimum_origins
        || observed.monitor != request.monitor
        || observed.check_regions != request.check_regions
        || observed.origin_steering != request.origin_steering
        || observed.origins.len() != request.origins.len()
        || observed
            .origins
            .iter()
            .zip(&request.origins)
            .any(|(observed, request)| {
                !observed.unsupported_fields.is_empty()
                    || observed.name != request.name
                    || observed.address != request.address
                    || observed.port != request.port
                    || observed.enabled != request.enabled
                    || observed.weight != request.weight
                    || observed.header != request.header
            })
    {
        return Err(unknown_outcome("cloudflare_pool_response_mismatch"));
    }
    Ok(())
}

fn validate_load_balancer_result(
    observed: &CloudflareLoadBalancer,
    request: &CloudflareLoadBalancerRequest,
    expected_id: Option<&CloudflareLoadBalancingId>,
) -> CloudflareApiResult<()> {
    if !observed.unsupported_fields.is_empty()
        || expected_id.is_some_and(|id| observed.id != *id)
        || observed.name != request.name
        || observed.enabled != request.enabled
        || observed.proxied != request.proxied
        || observed.steering_policy != request.steering_policy
        || observed.default_pools != request.default_pools
        || observed.fallback_pool != request.fallback_pool
        || observed.session_affinity != request.session_affinity
        || observed.session_affinity_ttl != request.session_affinity_ttl
        || observed.session_affinity_attributes != request.session_affinity_attributes
        || observed.random_steering != request.random_steering
    {
        return Err(unknown_outcome(
            "cloudflare_load_balancer_response_mismatch",
        ));
    }
    Ok(())
}

fn monitor_path(account_id: &str, id: Option<&CloudflareLoadBalancingId>) -> String {
    let base = format!("accounts/{account_id}/load_balancers/monitors");
    id.map_or(base.clone(), |id| format!("{base}/{id}"))
}

fn pool_path(account_id: &str, id: Option<&CloudflareLoadBalancingId>) -> String {
    let base = format!("accounts/{account_id}/load_balancers/pools");
    id.map_or(base.clone(), |id| format!("{base}/{id}"))
}

fn load_balancer_path(zone_id: &str, id: Option<&CloudflareLoadBalancingId>) -> String {
    let base = format!("zones/{zone_id}/load_balancers");
    id.map_or(base.clone(), |id| format!("{base}/{id}"))
}

pub async fn delete_monitor_if_unreferenced<A: CloudflareLoadBalancingApi>(
    api: &A,
    account_id: &str,
    id: &CloudflareLoadBalancingId,
    center: &CenterReferenceEvidence,
    provider: &ProviderReferenceEvidence,
    authorization: &AuthorizedCloudflareMutation,
) -> CloudflareApiResult<CloudflareDeleteResult> {
    ensure_unreferenced(center, provider, authorization)?;
    if api.monitor_references(account_id, id).await? != provider.references {
        return Err(conflict("cloudflare_monitor_reference_revision_changed"));
    }
    api.delete_monitor(account_id, id, authorization).await
}

pub async fn delete_pool_if_unreferenced<A: CloudflareLoadBalancingApi>(
    api: &A,
    account_id: &str,
    id: &CloudflareLoadBalancingId,
    center: &CenterReferenceEvidence,
    provider: &ProviderReferenceEvidence,
    authorization: &AuthorizedCloudflareMutation,
) -> CloudflareApiResult<CloudflareDeleteResult> {
    ensure_unreferenced(center, provider, authorization)?;
    if api.pool_references(account_id, id).await? != provider.references {
        return Err(conflict("cloudflare_pool_reference_revision_changed"));
    }
    api.delete_pool(account_id, id, authorization).await
}

/// Requires both Center's typed reverse-reference index and Cloudflare's
/// provider-side reference graph to be empty. The subsequent provider delete
/// must still preserve a provider conflict because a new reference can race
/// this preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CenterReferenceEvidence {
    pub scope: CloudflareMutationScope,
    pub resource_kind: CloudflareReferencedResourceType,
    pub resource_id: CloudflareLoadBalancingId,
    pub center_ownership_revision: String,
    pub reverse_index_revision: String,
    pub reference_count: u32,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderReferenceEvidence {
    pub scope: CloudflareMutationScope,
    pub resource_kind: CloudflareReferencedResourceType,
    pub resource_id: CloudflareLoadBalancingId,
    pub provider_resource_revision: CloudflareCanonicalRevision,
    pub references: Vec<CloudflareResourceReference>,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
}

pub fn ensure_unreferenced(
    center: &CenterReferenceEvidence,
    provider: &ProviderReferenceEvidence,
    authorization: &AuthorizedCloudflareMutation,
) -> CloudflareApiResult<()> {
    let now = current_unix_ms()?;
    let claim = &authorization.claim;
    if center.scope != claim.scope
        || provider.scope != claim.scope
        || center.resource_kind != claim.resource_kind
        || provider.resource_kind != claim.resource_kind
        || center.resource_id != claim.resource_id
        || provider.resource_id != claim.resource_id
        || center.center_ownership_revision != claim.center_ownership_revision
        || provider.provider_resource_revision != claim.expected_provider_revision
        || center.reverse_index_revision.is_empty()
        || now < center.observed_at_unix_ms
        || now >= center.valid_until_unix_ms
        || now < provider.observed_at_unix_ms
        || now >= provider.valid_until_unix_ms
        || center.reference_count != 0
        || !provider.references.is_empty()
    {
        return Err(conflict("cloudflare_resource_still_or_unknown_referenced"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityEvidence {
    Allowed,
    Denied,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareLoadBalancingEntitlements {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub catalog_revision: String,
    pub catalog_observed_at_unix_ms: i64,
    pub catalog_valid_until_unix_ms: i64,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
    pub monitors_and_pools_write: CapabilityEvidence,
    pub load_balancers_write: CapabilityEvidence,
    pub max_monitors: Option<u32>,
    pub max_pools: Option<u32>,
    pub max_load_balancers: Option<u32>,
    pub allowed_health_regions: Option<BTreeSet<CloudflareHealthRegion>>,
    pub allowed_steering: Option<BTreeSet<CloudflareSteeringPolicy>>,
    pub allowed_session_affinity: Option<BTreeSet<CloudflareSessionAffinity>>,
    pub endpoint_drain: CapabilityEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareLoadBalancingUsage {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
    pub monitors: u32,
    pub pools: u32,
    pub load_balancers: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareLoadBalancingDemand {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub additional_monitors: u32,
    pub additional_pools: u32,
    pub additional_load_balancers: u32,
    pub health_regions: BTreeSet<CloudflareHealthRegion>,
    pub steering: CloudflareSteeringPolicy,
    pub session_affinity: CloudflareSessionAffinity,
    pub requires_endpoint_drain: bool,
}

pub fn preflight_load_balancing(
    entitlements: &CloudflareLoadBalancingEntitlements,
    usage: &CloudflareLoadBalancingUsage,
    demand: &CloudflareLoadBalancingDemand,
    now_unix_ms: i64,
) -> CloudflareApiResult<()> {
    if now_unix_ms < entitlements.observed_at_unix_ms
        || now_unix_ms >= entitlements.valid_until_unix_ms
    {
        return Err(validation("cloudflare_load_balancing_entitlements_stale"));
    }
    if now_unix_ms < usage.observed_at_unix_ms || now_unix_ms >= usage.valid_until_unix_ms {
        return Err(validation("cloudflare_load_balancing_usage_stale"));
    }
    if now_unix_ms < entitlements.catalog_observed_at_unix_ms
        || now_unix_ms >= entitlements.catalog_valid_until_unix_ms
    {
        return Err(validation("cloudflare_load_balancing_catalog_stale"));
    }
    if entitlements.account_id != demand.account_id
        || entitlements.zone_id != demand.zone_id
        || entitlements.credential_revision != demand.credential_revision
        || usage.account_id != demand.account_id
        || usage.zone_id != demand.zone_id
        || usage.credential_revision != demand.credential_revision
        || entitlements.catalog_revision.is_empty()
    {
        return Err(validation("cloudflare_load_balancing_scope_mismatch"));
    }
    require_allowed(
        entitlements.monitors_and_pools_write,
        demand.additional_monitors != 0 || demand.additional_pools != 0,
        "cloudflare_monitors_pools_write_not_proven",
    )?;
    require_allowed(
        entitlements.load_balancers_write,
        demand.additional_load_balancers != 0,
        "cloudflare_load_balancers_write_not_proven",
    )?;
    quota(
        usage.monitors,
        demand.additional_monitors,
        entitlements.max_monitors,
        "cloudflare_monitor_quota_not_proven",
    )?;
    quota(
        usage.pools,
        demand.additional_pools,
        entitlements.max_pools,
        "cloudflare_pool_quota_not_proven",
    )?;
    quota(
        usage.load_balancers,
        demand.additional_load_balancers,
        entitlements.max_load_balancers,
        "cloudflare_load_balancer_quota_not_proven",
    )?;
    if !demand.health_regions.is_empty()
        && !entitlements
            .allowed_health_regions
            .as_ref()
            .is_some_and(|allowed| demand.health_regions.is_subset(allowed))
    {
        return Err(validation("cloudflare_health_regions_not_entitled"));
    }
    if !entitlements
        .allowed_steering
        .as_ref()
        .is_some_and(|allowed| allowed.contains(&demand.steering))
    {
        return Err(validation("cloudflare_steering_not_entitled"));
    }
    if demand.session_affinity != CloudflareSessionAffinity::None
        && !entitlements
            .allowed_session_affinity
            .as_ref()
            .is_some_and(|allowed| allowed.contains(&demand.session_affinity))
    {
        return Err(validation("cloudflare_session_affinity_not_entitled"));
    }
    require_allowed(
        entitlements.endpoint_drain,
        demand.requires_endpoint_drain,
        "cloudflare_endpoint_drain_not_entitled",
    )
}

fn require_allowed(
    evidence: CapabilityEvidence,
    required: bool,
    code: &'static str,
) -> CloudflareApiResult<()> {
    if required && evidence != CapabilityEvidence::Allowed {
        Err(validation(code))
    } else {
        Ok(())
    }
}

fn quota(
    current: u32,
    additional: u32,
    limit: Option<u32>,
    code: &'static str,
) -> CloudflareApiResult<()> {
    if additional == 0 {
        return Ok(());
    }
    if limit.is_none_or(|limit| {
        current
            .checked_add(additional)
            .is_none_or(|total| total > limit)
    }) {
        return Err(validation(code));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflarePoolTier {
    pub priority: u16,
    pub request: CloudflarePoolRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareOriginPoolMappingOptions {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub origin_pool_revision: String,
    pub now_unix_ms: i64,
    pub pool_name_prefix: String,
    pub monitor_id: Option<CloudflareLoadBalancingId>,
    pub check_regions: BTreeSet<CloudflareHealthRegion>,
    pub origin_steering: CloudflareOriginSteeringPolicy,
    pub endpoint_drain_supported: bool,
    pub proxied: bool,
    pub session_affinity: CloudflareSessionAffinity,
    pub drain_duration_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareOriginProtocolSource {
    ZoneSslRoutingMode { settings_revision: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareOriginTlsVerification {
    FullStrict { settings_revision: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareHttpsOnlyMode {
    AlwaysUseHttps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareHttpsOnlyEvidence {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub mode: CloudflareHttpsOnlyMode,
    /// Revision of the observed Always Use HTTPS zone setting.
    pub settings_revision: String,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareOriginTransportProof {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub origin_pool_revision: String,
    pub protocol_source: CloudflareOriginProtocolSource,
    pub tls_verification: CloudflareOriginTlsVerification,
    pub https_only: CloudflareHttpsOnlyEvidence,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
}

/// Maps each priority tier to one Cloudflare pool. The first slice accepts
/// only HTTPS origins backed by fresh Full Strict zone-setting evidence;
/// independent SNI remains unsupported.
pub fn map_origin_pool(
    spec: &OriginPoolSpec,
    options: &CloudflareOriginPoolMappingOptions,
    transport: &CloudflareOriginTransportProof,
) -> CloudflareApiResult<Vec<CloudflarePoolTier>> {
    spec.validate()
        .map_err(|_| validation("invalid_origin_pool_spec"))?;
    validate_cloudflare_name(&options.pool_name_prefix)?;
    validate_provider_id(&options.account_id, "invalid_cloudflare_account_id")?;
    validate_provider_id(&options.zone_id, "invalid_cloudflare_zone_id")?;
    if options.credential_revision.is_empty()
        || options.origin_pool_revision.is_empty()
        || options.account_id != transport.account_id
        || options.zone_id != transport.zone_id
        || options.credential_revision != transport.credential_revision
        || options.origin_pool_revision != transport.origin_pool_revision
        || options.now_unix_ms < transport.observed_at_unix_ms
        || options.now_unix_ms >= transport.valid_until_unix_ms
        || transport.https_only.account_id != options.account_id
        || transport.https_only.zone_id != options.zone_id
        || transport.https_only.credential_revision != options.credential_revision
        || transport.https_only.mode != CloudflareHttpsOnlyMode::AlwaysUseHttps
        || transport.https_only.settings_revision.is_empty()
        || options.now_unix_ms < transport.https_only.observed_at_unix_ms
        || options.now_unix_ms >= transport.https_only.valid_until_unix_ms
    {
        return Err(validation("cloudflare_origin_transport_proof_mismatch"));
    }
    let protocols = spec
        .endpoints
        .iter()
        .map(|endpoint| endpoint.protocol)
        .collect::<BTreeSet<_>>();
    if protocols != BTreeSet::from([OriginProtocol::Https]) {
        return Err(validation("cloudflare_origin_transport_protocol_mismatch"));
    }
    let CloudflareOriginProtocolSource::ZoneSslRoutingMode {
        settings_revision: protocol_revision,
    } = &transport.protocol_source;
    let CloudflareOriginTlsVerification::FullStrict {
        settings_revision: tls_revision,
    } = &transport.tls_verification;
    if protocol_revision.is_empty() || tls_revision.is_empty() || protocol_revision != tls_revision
    {
        return Err(validation(
            "cloudflare_full_strict_zone_evidence_not_proven",
        ));
    }
    if spec.failover_mode == OriginFailoverMode::AllHealthy
        && spec
            .endpoints
            .iter()
            .map(|endpoint| endpoint.priority)
            .collect::<BTreeSet<_>>()
            .len()
            > 1
    {
        return Err(validation("all_healthy_with_priority_tiers_is_ambiguous"));
    }

    let mut by_priority = BTreeMap::<u16, Vec<_>>::new();
    for endpoint in &spec.endpoints {
        if !matches!(
            endpoint.protocol,
            OriginProtocol::Http | OriginProtocol::Https
        ) || endpoint.tls_mode != OriginTlsMode::Verify
        {
            return Err(validation("cloudflare_lb_origin_protocol_unsupported"));
        }
        if endpoint.server_name.is_some()
            && endpoint.server_name.as_ref() != endpoint.host_header.as_ref()
        {
            return Err(validation("cloudflare_lb_independent_sni_unsupported"));
        }
        if endpoint.headers.secret_ref.is_some()
            || endpoint
                .headers
                .literal
                .keys()
                .any(|name| !name.eq_ignore_ascii_case("host"))
        {
            return Err(validation("cloudflare_lb_origin_headers_unsupported"));
        }
        if endpoint.drain == OriginDrainState::Draining
            && (!options.endpoint_drain_supported
                || !options.proxied
                || options.session_affinity == CloudflareSessionAffinity::None
                || options.drain_duration_seconds == 0
                || options.drain_duration_seconds > MAX_DRAIN_SECONDS)
        {
            return Err(validation(
                "cloudflare_endpoint_drain_configuration_invalid",
            ));
        }
        by_priority
            .entry(endpoint.priority)
            .or_default()
            .push(endpoint);
    }

    let mut tiers = Vec::with_capacity(by_priority.len());
    for (priority, endpoints) in by_priority {
        let weights = exact_cloudflare_weights(
            &endpoints
                .iter()
                .map(|endpoint| endpoint.weight)
                .collect::<Vec<_>>(),
        )?;
        let active = endpoints
            .iter()
            .filter(|endpoint| endpoint.drain == OriginDrainState::Active)
            .count();
        if usize::from(spec.minimum_healthy) > active {
            return Err(validation("cloudflare_tier_below_minimum_origins"));
        }
        let name = format!("{}-p{priority}", options.pool_name_prefix);
        validate_cloudflare_name(&name)?;
        let origins = endpoints
            .iter()
            .zip(weights)
            .map(|(endpoint, weight)| CloudflarePoolOrigin {
                name: endpoint.name.as_str().to_string(),
                address: match &endpoint.address {
                    OriginAddress::Hostname(value) => value.as_str().to_string(),
                    OriginAddress::Ip(value) => value.to_string(),
                },
                port: endpoint.port,
                enabled: endpoint.drain == OriginDrainState::Active,
                weight,
                header: endpoint.host_header.as_ref().map(|host| {
                    BTreeMap::from([("Host".to_string(), vec![host.as_str().to_string()])])
                }),
            })
            .collect();
        tiers.push(CloudflarePoolTier {
            priority,
            request: CloudflarePoolRequest {
                name,
                description: "Managed by EdgionCenter".to_string(),
                enabled: true,
                minimum_origins: spec.minimum_healthy,
                monitor: options.monitor_id.clone(),
                check_regions: options.check_regions.clone(),
                origin_steering: CloudflareOriginSteering {
                    policy: options.origin_steering,
                },
                origins,
            },
        });
    }
    Ok(tiers)
}

pub fn map_health_check(
    spec: &OriginPoolSpec,
    description: impl Into<String>,
) -> CloudflareApiResult<Option<CloudflareMonitorRequest>> {
    let Some(check) = &spec.health_check else {
        return Ok(None);
    };
    check
        .validate()
        .map_err(|_| validation("invalid_origin_health_check"))?;
    if matches!(check.sources, HealthCheckSourceScope::Regions(_)) {
        return Err(validation("health_regions_must_map_to_cloudflare_catalog"));
    }
    if check.headers.secret_ref.is_some() {
        return Err(validation("cloudflare_monitor_secret_headers_unsupported"));
    }
    let monitor_type = match check.protocol {
        OriginProtocol::Http => CloudflareMonitorType::Http,
        OriginProtocol::Https => CloudflareMonitorType::Https,
        OriginProtocol::Tcp => CloudflareMonitorType::Tcp,
        OriginProtocol::Tls => return Err(validation("cloudflare_tls_monitor_unsupported")),
    };
    let statuses = if check.expected.statuses.is_empty() {
        None
    } else {
        Some(format_expected_codes(&check.expected.statuses))
    };
    Ok(Some(CloudflareMonitorRequest {
        monitor_type,
        description: description.into(),
        method: check.method.map(|method| match method {
            HealthCheckMethod::Get => "GET".to_string(),
            HealthCheckMethod::Head => "HEAD".to_string(),
        }),
        path: check.path.clone(),
        port: check.port,
        header: (!check.headers.literal.is_empty()).then(|| {
            check
                .headers
                .literal
                .iter()
                .map(|(name, value)| (name.clone(), vec![value.clone()]))
                .collect()
        }),
        expected_codes: statuses,
        expected_body: check.expected.body_contains.clone(),
        interval: check.interval_seconds,
        timeout: check.timeout_seconds,
        retries: 0,
        consecutive_up: check.healthy_threshold,
        consecutive_down: check.unhealthy_threshold,
        allow_insecure: false,
        follow_redirects: false,
    }))
}

fn format_expected_codes(statuses: &BTreeSet<u16>) -> String {
    statuses
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn exact_cloudflare_weights(weights: &[u16]) -> CloudflareApiResult<Vec<CloudflareOriginWeight>> {
    let positive = weights
        .iter()
        .copied()
        .filter(|value| *value != 0)
        .collect::<Vec<_>>();
    if positive.is_empty() {
        return Err(validation("cloudflare_origin_weights_all_zero"));
    }
    let divisor = positive.iter().copied().reduce(gcd).unwrap_or(1);
    let maximum = positive
        .iter()
        .map(|value| value / divisor)
        .max()
        .unwrap_or(0);
    if maximum > 100 {
        return Err(validation(
            "cloudflare_origin_weights_not_exactly_representable",
        ));
    }
    let scale = 100 / maximum;
    Ok(weights
        .iter()
        .map(|weight| CloudflareOriginWeight::from_basis_points(((weight / divisor) * scale) as u8))
        .collect())
}

fn gcd(mut left: u16, mut right: u16) -> u16 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CloudflareRolloutSymbol(String);

impl CloudflareRolloutSymbol {
    pub fn new(value: impl Into<String>) -> CloudflareApiResult<Self> {
        let value = value.into();
        validate_cloudflare_name(&value)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareRolloutRef {
    Existing(CloudflareLoadBalancingId),
    Created(CloudflareRolloutSymbol),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareSymbolicPoolRequest {
    pub symbol: CloudflareRolloutSymbol,
    pub request: CloudflarePoolRequest,
    pub monitor: Option<CloudflareRolloutRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareSymbolicLoadBalancerRequest {
    pub name: String,
    pub enabled: bool,
    pub proxied: bool,
    pub steering_policy: CloudflareSteeringPolicy,
    pub default_pools: Vec<CloudflareRolloutRef>,
    pub fallback_pool: CloudflareRolloutRef,
    pub session_affinity: CloudflareSessionAffinity,
    pub session_affinity_ttl: u32,
    pub session_affinity_attributes: CloudflareSessionAffinityAttributes,
}

impl CloudflareSymbolicLoadBalancerRequest {
    fn validate(&self) -> CloudflareApiResult<()> {
        if self.name.is_empty()
            || self.name.len() > 253
            || self.default_pools.is_empty()
            || self.session_affinity_attributes.drain_duration > MAX_DRAIN_SECONDS
        {
            return Err(validation("invalid_symbolic_cloudflare_load_balancer"));
        }
        let ttl_valid = match self.session_affinity {
            CloudflareSessionAffinity::None => self.session_affinity_ttl == 0,
            CloudflareSessionAffinity::Cookie | CloudflareSessionAffinity::IpCookie => {
                (1_800..=MAX_SESSION_AFFINITY_TTL).contains(&self.session_affinity_ttl)
            }
            CloudflareSessionAffinity::Header => {
                (30..=3_600).contains(&self.session_affinity_ttl)
                    && !self.session_affinity_attributes.headers.is_empty()
            }
        };
        if !ttl_valid
            || (!self.proxied && self.session_affinity != CloudflareSessionAffinity::None)
            || (self.session_affinity == CloudflareSessionAffinity::None
                && self.session_affinity_attributes.drain_duration != 0)
        {
            return Err(validation("invalid_symbolic_cloudflare_affinity"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareContractTarget {
    pub id: CloudflareLoadBalancingId,
    pub authorization: AuthorizedCloudflareMutation,
    pub retirement: CloudflareRetirementMode,
    pub reference_evidence: CloudflareReferenceEvidenceRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareReferenceEvidenceRequirement {
    FreshCenterAndProvider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareRetirementMode {
    Drain(CloudflareDrainProof),
    ImmediateDisable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareDrainProof {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub load_balancer_id: CloudflareLoadBalancingId,
    pub load_balancer_revision: CloudflareCanonicalRevision,
    pub proxied: bool,
    pub session_affinity: CloudflareSessionAffinity,
    pub drain_duration_seconds: u32,
    pub entitlement: CapabilityEvidence,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareRolloutContext {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub load_balancer_id: CloudflareLoadBalancingId,
    pub load_balancer_revision: CloudflareCanonicalRevision,
    pub now_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareLoadBalancingRolloutStep {
    CreateMonitor {
        symbol: CloudflareRolloutSymbol,
        request: CloudflareMonitorRequest,
    },
    CreatePool(CloudflareSymbolicPoolRequest),
    VerifyProviderHealthy {
        pool: CloudflareRolloutRef,
        minimum_origins: u16,
    },
    AttachOldAndNew(CloudflareSymbolicLoadBalancerRequest),
    VerifyLoadBalancerAttachment(Vec<CloudflareRolloutRef>),
    DrainAndDisableRetiredPool(CloudflareLoadBalancingId),
    ImmediatelyDisableRetiredPool(CloudflareLoadBalancingId),
    DetachRetired(CloudflareSymbolicLoadBalancerRequest),
    ContractPool(CloudflareContractTarget),
    ContractMonitor(CloudflareContractTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareLoadBalancingRolloutPlan {
    pub steps: Vec<CloudflareLoadBalancingRolloutStep>,
}

pub fn plan_expand_verify_contract(
    context: &CloudflareRolloutContext,
    new_monitor: Option<(CloudflareRolloutSymbol, CloudflareMonitorRequest)>,
    new_pools: Vec<CloudflareSymbolicPoolRequest>,
    final_load_balancer: CloudflareSymbolicLoadBalancerRequest,
    current_pools: Vec<CloudflareLoadBalancingId>,
    retired_pools: Vec<CloudflareContractTarget>,
    retired_monitors: Vec<CloudflareContractTarget>,
) -> CloudflareApiResult<CloudflareLoadBalancingRolloutPlan> {
    validate_provider_id(&context.account_id, "invalid_cloudflare_account_id")?;
    validate_provider_id(&context.zone_id, "invalid_cloudflare_zone_id")?;
    if context.credential_revision.is_empty() {
        return Err(validation("invalid_cloudflare_rollout_credential_revision"));
    }
    final_load_balancer.validate()?;
    let created = new_pools
        .iter()
        .map(|pool| pool.symbol.clone())
        .collect::<BTreeSet<_>>();
    let monitor_symbol = new_monitor.as_ref().map(|(symbol, _)| symbol);
    let valid_ref = |reference: &CloudflareRolloutRef| match reference {
        CloudflareRolloutRef::Existing(_) => true,
        CloudflareRolloutRef::Created(symbol) => created.contains(symbol),
    };
    if new_pools.is_empty()
        || created.len() != new_pools.len()
        || final_load_balancer.default_pools.is_empty()
        || !final_load_balancer.default_pools.iter().all(valid_ref)
        || !valid_ref(&final_load_balancer.fallback_pool)
        || new_pools.iter().any(|pool| {
            pool.request.monitor.is_some()
                || matches!(&pool.monitor, Some(CloudflareRolloutRef::Created(symbol)) if Some(symbol) != monitor_symbol)
        })
        || retired_pools.iter().any(|target| {
            target
                .authorization
                .validate(
                    &CloudflareMutationScope::Account(context.account_id.clone()),
                    CloudflareReferencedResourceType::Pool,
                    &target.id,
                )
                .is_err()
        })
        || retired_monitors.iter().any(|target| {
            target
                .authorization
                .validate(
                    &CloudflareMutationScope::Account(context.account_id.clone()),
                    CloudflareReferencedResourceType::Monitor,
                    &target.id,
                )
                .is_err()
        })
        || retired_pools.iter().any(|target| match &target.retirement {
            CloudflareRetirementMode::ImmediateDisable => false,
            CloudflareRetirementMode::Drain(proof) => {
                proof.account_id != context.account_id
                    || proof.zone_id != context.zone_id
                    || proof.credential_revision != context.credential_revision
                    || proof.load_balancer_id != context.load_balancer_id
                    || proof.load_balancer_revision != context.load_balancer_revision
                    || !proof.proxied
                    || !final_load_balancer.proxied
                    || proof.session_affinity == CloudflareSessionAffinity::None
                    || proof.drain_duration_seconds == 0
                    || proof.drain_duration_seconds > MAX_DRAIN_SECONDS
                    || proof.entitlement != CapabilityEvidence::Allowed
                    || context.now_unix_ms < proof.observed_at_unix_ms
                    || context.now_unix_ms >= proof.valid_until_unix_ms
                    || final_load_balancer.session_affinity != proof.session_affinity
                    || final_load_balancer.session_affinity_attributes.drain_duration
                        != proof.drain_duration_seconds
            }
        })
    {
        return Err(validation("unsafe_cloudflare_load_balancing_rollout"));
    }
    let mut steps = Vec::new();
    if let Some((symbol, request)) = new_monitor {
        steps.push(CloudflareLoadBalancingRolloutStep::CreateMonitor { symbol, request });
    }
    for pool in new_pools {
        let minimum_origins = pool.request.minimum_origins;
        let reference = CloudflareRolloutRef::Created(pool.symbol.clone());
        steps.push(CloudflareLoadBalancingRolloutStep::CreatePool(pool));
        steps.push(CloudflareLoadBalancingRolloutStep::VerifyProviderHealthy {
            pool: reference,
            minimum_origins,
        });
    }
    let mut attach = final_load_balancer.clone();
    for id in current_pools {
        let reference = CloudflareRolloutRef::Existing(id);
        if !attach.default_pools.contains(&reference) {
            attach.default_pools.push(reference);
        }
    }
    steps.push(CloudflareLoadBalancingRolloutStep::AttachOldAndNew(attach));
    steps.push(
        CloudflareLoadBalancingRolloutStep::VerifyLoadBalancerAttachment(
            final_load_balancer.default_pools.clone(),
        ),
    );
    steps.extend(retired_pools.iter().map(|target| match target.retirement {
        CloudflareRetirementMode::Drain(_) => {
            CloudflareLoadBalancingRolloutStep::DrainAndDisableRetiredPool(target.id.clone())
        }
        CloudflareRetirementMode::ImmediateDisable => {
            CloudflareLoadBalancingRolloutStep::ImmediatelyDisableRetiredPool(target.id.clone())
        }
    }));
    steps.push(CloudflareLoadBalancingRolloutStep::DetachRetired(
        final_load_balancer,
    ));
    steps.extend(
        retired_pools
            .into_iter()
            .map(CloudflareLoadBalancingRolloutStep::ContractPool),
    );
    steps.extend(
        retired_monitors
            .into_iter()
            .map(CloudflareLoadBalancingRolloutStep::ContractMonitor),
    );
    Ok(CloudflareLoadBalancingRolloutPlan { steps })
}

fn validate_provider_id(value: &str, code: &'static str) -> CloudflareApiResult<()> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(validation(code));
    }
    Ok(())
}

fn validate_cloudflare_name(value: &str) -> CloudflareApiResult<()> {
    if value.is_empty()
        || value.len() > MAX_CLOUDFLARE_NAME
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(validation("invalid_cloudflare_pool_name"));
    }
    Ok(())
}

fn validation(code: &'static str) -> NormalizedProviderError {
    error(ProviderErrorCategory::Validation, code)
}

fn conflict(code: &'static str) -> NormalizedProviderError {
    error(ProviderErrorCategory::Conflict, code)
}

fn unknown_outcome(code: &'static str) -> NormalizedProviderError {
    error(ProviderErrorCategory::UnknownOutcome, code)
}

fn error(category: ProviderErrorCategory, code: &'static str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        code,
        "Cloudflare load balancing request rejected",
        None,
        None,
    )
    .expect("static provider error is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_center_core::{
        DomainName, HealthCheckExpectedResponse, HealthCheckSpec, OriginEndpoint,
        OriginEndpointName, OriginRequestHeaders,
    };

    fn id(hex: char) -> CloudflareLoadBalancingId {
        CloudflareLoadBalancingId::new(hex.to_string().repeat(32)).unwrap()
    }

    struct FakeOwnershipVerifier {
        allow: bool,
    }

    #[async_trait]
    impl CloudflareOwnershipVerifier for FakeOwnershipVerifier {
        async fn verify(&self, _claim: &CloudflareOwnershipClaim) -> CloudflareApiResult<()> {
            if self.allow {
                Ok(())
            } else {
                Err(conflict("fake_ownership_denied"))
            }
        }
    }

    fn ownership_claim(
        scope: CloudflareMutationScope,
        kind: CloudflareReferencedResourceType,
        resource_id: CloudflareLoadBalancingId,
        revision: CloudflareCanonicalRevision,
    ) -> CloudflareOwnershipClaim {
        let now = current_unix_ms().unwrap();
        CloudflareOwnershipClaim {
            scope,
            resource_kind: kind,
            resource_id,
            expected_provider_revision: revision,
            center_ownership_revision: "ownership-1".to_string(),
            observed_at_unix_ms: now - 1_000,
            valid_until_unix_ms: now + 60_000,
        }
    }

    async fn authorize(claim: CloudflareOwnershipClaim) -> AuthorizedCloudflareMutation {
        authorize_cloudflare_mutation(&FakeOwnershipVerifier { allow: true }, claim)
            .await
            .unwrap()
    }

    fn endpoint(name: &str, weight: u16, priority: u16) -> OriginEndpoint {
        OriginEndpoint {
            name: OriginEndpointName::new(name).unwrap(),
            address: OriginAddress::Hostname(
                DomainName::new(format!("{name}.example.com")).unwrap(),
            ),
            port: 443,
            protocol: OriginProtocol::Https,
            host_header: Some(DomainName::new("origin.example.com").unwrap()),
            server_name: None,
            tls_mode: OriginTlsMode::Verify,
            weight,
            priority,
            drain: OriginDrainState::Active,
            headers: OriginRequestHeaders::default(),
        }
    }

    fn pool() -> OriginPoolSpec {
        OriginPoolSpec {
            provider_account_ref: None,
            endpoints: vec![endpoint("primary-a", 10, 0), endpoint("primary-b", 20, 0)],
            health_check: None,
            failover_mode: OriginFailoverMode::PriorityTiers,
            minimum_healthy: 1,
        }
    }

    fn options() -> CloudflareOriginPoolMappingOptions {
        CloudflareOriginPoolMappingOptions {
            account_id: "a".repeat(32),
            zone_id: "b".repeat(32),
            credential_revision: "credential-1".to_string(),
            origin_pool_revision: "pool-revision-1".to_string(),
            now_unix_ms: 1_500,
            pool_name_prefix: "edgion-main".to_string(),
            monitor_id: Some(id('a')),
            check_regions: BTreeSet::from([CloudflareHealthRegion::WesternEurope]),
            origin_steering: CloudflareOriginSteeringPolicy::Random,
            endpoint_drain_supported: false,
            proxied: true,
            session_affinity: CloudflareSessionAffinity::None,
            drain_duration_seconds: 0,
        }
    }

    fn transport() -> CloudflareOriginTransportProof {
        CloudflareOriginTransportProof {
            account_id: "a".repeat(32),
            zone_id: "b".repeat(32),
            credential_revision: "credential-1".to_string(),
            origin_pool_revision: "pool-revision-1".to_string(),
            protocol_source: CloudflareOriginProtocolSource::ZoneSslRoutingMode {
                settings_revision: "zone-settings-1".to_string(),
            },
            tls_verification: CloudflareOriginTlsVerification::FullStrict {
                settings_revision: "zone-settings-1".to_string(),
            },
            https_only: CloudflareHttpsOnlyEvidence {
                account_id: "a".repeat(32),
                zone_id: "b".repeat(32),
                credential_revision: "credential-1".to_string(),
                mode: CloudflareHttpsOnlyMode::AlwaysUseHttps,
                settings_revision: "always-use-https-1".to_string(),
                observed_at_unix_ms: 1_000,
                valid_until_unix_ms: 2_000,
            },
            observed_at_unix_ms: 1_000,
            valid_until_unix_ms: 2_000,
        }
    }

    #[test]
    fn priority_tiers_and_exact_weights_map_without_rounding() {
        let mut desired = pool();
        desired.endpoints.push(endpoint("backup", 1, 1));
        let mapped = map_origin_pool(&desired, &options(), &transport()).unwrap();
        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].priority, 0);
        assert_eq!(mapped[0].request.origins[0].weight.basis_points(), 50);
        assert_eq!(mapped[0].request.origins[1].weight.basis_points(), 100);
        assert_eq!(mapped[1].priority, 1);
    }

    #[test]
    fn unrepresentable_weights_fail_closed() {
        let mut desired = pool();
        desired.endpoints[0].weight = 1;
        desired.endpoints[1].weight = 101;
        let error = map_origin_pool(&desired, &options(), &transport()).unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Validation);
        assert_eq!(
            error.code(),
            "cloudflare_origin_weights_not_exactly_representable"
        );
    }

    #[test]
    fn transport_proof_rejects_mixed_protocols_scope_and_staleness() {
        let mut desired = pool();
        desired.endpoints[1].protocol = OriginProtocol::Http;
        desired.endpoints[1].port = 80;
        assert_eq!(
            map_origin_pool(&desired, &options(), &transport())
                .unwrap_err()
                .code(),
            "cloudflare_origin_transport_protocol_mismatch"
        );
        desired.endpoints[1].protocol = OriginProtocol::Https;
        desired.endpoints[1].port = 443;
        for endpoint in &mut desired.endpoints {
            endpoint.protocol = OriginProtocol::Http;
            endpoint.port = 80;
        }
        assert_eq!(
            map_origin_pool(&desired, &options(), &transport())
                .unwrap_err()
                .code(),
            "cloudflare_origin_transport_protocol_mismatch"
        );
        for endpoint in &mut desired.endpoints {
            endpoint.protocol = OriginProtocol::Https;
            endpoint.port = 443;
        }
        let mut mismatched_full_strict = transport();
        mismatched_full_strict.tls_verification = CloudflareOriginTlsVerification::FullStrict {
            settings_revision: "zone-settings-2".to_string(),
        };
        assert_eq!(
            map_origin_pool(&desired, &options(), &mismatched_full_strict)
                .unwrap_err()
                .code(),
            "cloudflare_full_strict_zone_evidence_not_proven"
        );
        let mut wrong_scope = transport();
        wrong_scope.zone_id = "c".repeat(32);
        assert_eq!(
            map_origin_pool(&desired, &options(), &wrong_scope)
                .unwrap_err()
                .code(),
            "cloudflare_origin_transport_proof_mismatch"
        );
        let mut wrong_https_scope = transport();
        wrong_https_scope.https_only.zone_id = "c".repeat(32);
        assert_eq!(
            map_origin_pool(&desired, &options(), &wrong_https_scope)
                .unwrap_err()
                .code(),
            "cloudflare_origin_transport_proof_mismatch"
        );
        let mut stale_https = transport();
        stale_https.https_only.valid_until_unix_ms = options().now_unix_ms;
        assert_eq!(
            map_origin_pool(&desired, &options(), &stale_https)
                .unwrap_err()
                .code(),
            "cloudflare_origin_transport_proof_mismatch"
        );
        let mut stale = transport();
        stale.valid_until_unix_ms = options().now_unix_ms;
        assert_eq!(
            map_origin_pool(&desired, &options(), &stale)
                .unwrap_err()
                .code(),
            "cloudflare_origin_transport_proof_mismatch"
        );
    }

    #[test]
    fn port_maps_and_only_independent_sni_or_unproven_drain_fail_closed() {
        let mut desired = pool();
        desired.endpoints[0].port = 8443;
        let mapped = map_origin_pool(&desired, &options(), &transport()).unwrap();
        assert_eq!(mapped[0].request.origins[0].port, 8443);
        desired.endpoints[0].server_name = desired.endpoints[0].host_header.clone();
        map_origin_pool(&desired, &options(), &transport()).unwrap();
        desired.endpoints[0].server_name = Some(DomainName::new("tls.example.com").unwrap());
        assert_eq!(
            map_origin_pool(&desired, &options(), &transport())
                .unwrap_err()
                .code(),
            "cloudflare_lb_independent_sni_unsupported"
        );
        desired.endpoints[0].server_name = desired.endpoints[0].host_header.clone();
        desired.endpoints[0].drain = OriginDrainState::Draining;
        assert_eq!(
            map_origin_pool(&desired, &options(), &transport())
                .unwrap_err()
                .code(),
            "cloudflare_endpoint_drain_configuration_invalid"
        );
        let mut supported = options();
        supported.endpoint_drain_supported = true;
        supported.proxied = true;
        supported.session_affinity = CloudflareSessionAffinity::Cookie;
        supported.drain_duration_seconds = 300;
        let mapped = map_origin_pool(&desired, &supported, &transport()).unwrap();
        assert!(!mapped[0].request.origins[0].enabled);
    }

    #[test]
    fn monitor_preserves_health_check_port() {
        let mut desired = pool();
        desired.health_check = Some(HealthCheckSpec {
            protocol: OriginProtocol::Https,
            port: 8443,
            method: Some(HealthCheckMethod::Get),
            path: Some("/ready".to_string()),
            headers: OriginRequestHeaders::default(),
            interval_seconds: 60,
            timeout_seconds: 5,
            healthy_threshold: 2,
            unhealthy_threshold: 3,
            expected: HealthCheckExpectedResponse {
                statuses: BTreeSet::from([200]),
                body_contains: None,
            },
            sources: HealthCheckSourceScope::ProviderDefault,
        });
        assert_eq!(
            map_health_check(&desired, "owner:team-a")
                .unwrap()
                .unwrap()
                .port,
            8443
        );
    }

    #[tokio::test]
    async fn verifier_seals_revision_and_unsupported_observation_fences() {
        let observed = serde_json::json!({"id": "resource", "enabled": true});
        let revision = cloudflare_canonical_revision(&observed).unwrap();
        let claim = ownership_claim(
            CloudflareMutationScope::Account("a".repeat(32)),
            CloudflareReferencedResourceType::Pool,
            id('a'),
            revision,
        );
        claim
            .validate(
                &CloudflareMutationScope::Account("a".repeat(32)),
                CloudflareReferencedResourceType::Pool,
                &id('a'),
            )
            .unwrap();
        assert!(claim
            .validate(
                &CloudflareMutationScope::Account("a".repeat(32)),
                CloudflareReferencedResourceType::Monitor,
                &id('a'),
            )
            .is_err());
        assert!(authorize_cloudflare_mutation(
            &FakeOwnershipVerifier { allow: false },
            claim.clone(),
        )
        .await
        .is_err());
        let authorization = authorize(claim).await;
        validate_observation(&BTreeMap::new(), &observed, &authorization).unwrap();
        assert_eq!(
            validate_observation(
                &BTreeMap::from([("load_shedding".to_string(), serde_json::json!({}))]),
                &observed,
                &authorization,
            )
            .unwrap_err()
            .code(),
            "cloudflare_resource_has_unsupported_fields"
        );
        let changed = serde_json::json!({"id": "resource", "enabled": false});
        assert_eq!(
            validate_observation(&BTreeMap::new(), &changed, &authorization)
                .unwrap_err()
                .code(),
            "cloudflare_resource_revision_changed"
        );
    }

    #[test]
    fn random_pool_weights_use_tenths_not_origin_hundredths() {
        assert_eq!(
            serde_json::to_string(&CloudflarePoolWeight::new(3).unwrap()).unwrap(),
            "0.3"
        );
        assert!(serde_json::from_str::<CloudflarePoolWeight>("0.35").is_err());
        assert!(CloudflarePoolWeight::new(11).is_err());
    }

    #[test]
    fn quota_entitlement_and_region_checks_are_fail_closed() {
        let entitlements = CloudflareLoadBalancingEntitlements {
            account_id: "a".repeat(32),
            zone_id: "b".repeat(32),
            credential_revision: "credential-1".to_string(),
            catalog_revision: "catalog-1".to_string(),
            catalog_observed_at_unix_ms: 1_000,
            catalog_valid_until_unix_ms: 2_000,
            observed_at_unix_ms: 1_000,
            valid_until_unix_ms: 2_000,
            monitors_and_pools_write: CapabilityEvidence::Allowed,
            load_balancers_write: CapabilityEvidence::Allowed,
            max_monitors: Some(2),
            max_pools: Some(3),
            max_load_balancers: Some(1),
            allowed_health_regions: Some(BTreeSet::from([CloudflareHealthRegion::WesternEurope])),
            allowed_steering: Some(BTreeSet::from([CloudflareSteeringPolicy::Off])),
            allowed_session_affinity: Some(BTreeSet::new()),
            endpoint_drain: CapabilityEvidence::Denied,
        };
        let demand = CloudflareLoadBalancingDemand {
            account_id: "a".repeat(32),
            zone_id: "b".repeat(32),
            credential_revision: "credential-1".to_string(),
            additional_monitors: 1,
            additional_pools: 2,
            additional_load_balancers: 1,
            health_regions: BTreeSet::from([CloudflareHealthRegion::WesternEurope]),
            steering: CloudflareSteeringPolicy::Off,
            session_affinity: CloudflareSessionAffinity::None,
            requires_endpoint_drain: false,
        };
        preflight_load_balancing(
            &entitlements,
            &CloudflareLoadBalancingUsage {
                account_id: "a".repeat(32),
                zone_id: "b".repeat(32),
                credential_revision: "credential-1".to_string(),
                observed_at_unix_ms: 1_000,
                valid_until_unix_ms: 2_000,
                monitors: 1,
                pools: 1,
                load_balancers: 0,
            },
            &demand,
            1_500,
        )
        .unwrap();
        let error = preflight_load_balancing(
            &entitlements,
            &CloudflareLoadBalancingUsage {
                account_id: "a".repeat(32),
                zone_id: "b".repeat(32),
                credential_revision: "credential-1".to_string(),
                observed_at_unix_ms: 1_000,
                valid_until_unix_ms: 2_000,
                monitors: 2,
                pools: 1,
                load_balancers: 0,
            },
            &demand,
            1_500,
        )
        .unwrap_err();
        assert_eq!(error.code(), "cloudflare_monitor_quota_not_proven");
        assert_eq!(
            preflight_load_balancing(
                &entitlements,
                &CloudflareLoadBalancingUsage {
                    account_id: "a".repeat(32),
                    zone_id: "b".repeat(32),
                    credential_revision: "credential-1".to_string(),
                    observed_at_unix_ms: 1_000,
                    valid_until_unix_ms: 3_000,
                    monitors: 0,
                    pools: 0,
                    load_balancers: 0,
                },
                &demand,
                2_000,
            )
            .unwrap_err()
            .code(),
            "cloudflare_load_balancing_entitlements_stale"
        );
    }

    #[tokio::test]
    async fn deletion_requires_verified_ownership_and_typed_reference_evidence() {
        let now = current_unix_ms().unwrap();
        let resource_id = id('c');
        let revision = CloudflareCanonicalRevision("provider-revision".to_string());
        let claim = ownership_claim(
            CloudflareMutationScope::Account("a".repeat(32)),
            CloudflareReferencedResourceType::Pool,
            resource_id.clone(),
            revision.clone(),
        );
        let authorization = authorize(claim.clone()).await;
        let reference = CloudflareResourceReference {
            reference_type: CloudflareReferenceType::Referrer,
            resource_id: id('a'),
            resource_name: "app.example.com".to_string(),
            resource_type: CloudflareReferencedResourceType::LoadBalancer,
        };
        let center = CenterReferenceEvidence {
            scope: claim.scope.clone(),
            resource_kind: claim.resource_kind,
            resource_id: resource_id.clone(),
            center_ownership_revision: claim.center_ownership_revision.clone(),
            reverse_index_revision: "index-1".to_string(),
            reference_count: 0,
            observed_at_unix_ms: now - 1_000,
            valid_until_unix_ms: now + 60_000,
        };
        let provider = ProviderReferenceEvidence {
            scope: claim.scope.clone(),
            resource_kind: claim.resource_kind,
            resource_id,
            provider_resource_revision: revision,
            references: Vec::new(),
            observed_at_unix_ms: now - 1_000,
            valid_until_unix_ms: now + 60_000,
        };
        let mut referenced_center = center.clone();
        referenced_center.reference_count = 1;
        assert_eq!(
            ensure_unreferenced(&referenced_center, &provider, &authorization)
                .unwrap_err()
                .code(),
            "cloudflare_resource_still_or_unknown_referenced"
        );
        let mut referenced_provider = provider.clone();
        referenced_provider.references.push(reference);
        assert_eq!(
            ensure_unreferenced(&center, &referenced_provider, &authorization)
                .unwrap_err()
                .code(),
            "cloudflare_resource_still_or_unknown_referenced"
        );
        ensure_unreferenced(&center, &provider, &authorization).unwrap();
    }

    #[test]
    fn load_balancer_rejects_dns_only_affinity_and_allows_distinct_fallback() {
        let pool_a = id('a');
        let mut request = CloudflareLoadBalancerRequest {
            name: "app.example.com".to_string(),
            enabled: true,
            proxied: false,
            steering_policy: CloudflareSteeringPolicy::Off,
            default_pools: vec![pool_a.clone()],
            fallback_pool: pool_a,
            session_affinity: CloudflareSessionAffinity::Cookie,
            session_affinity_ttl: 3_600,
            session_affinity_attributes: CloudflareSessionAffinityAttributes {
                headers: BTreeSet::new(),
                require_all_headers: false,
                drain_duration: 0,
                zero_downtime_failover: CloudflareZeroDowntimeFailover::Temporary,
            },
            random_steering: None,
        };
        assert_eq!(
            request.validate().unwrap_err().code(),
            "dns_only_affinity_unsupported"
        );
        request.proxied = true;
        request.fallback_pool = id('b');
        request.validate().unwrap();
        request.session_affinity_ttl = 1_799;
        assert_eq!(
            request.validate().unwrap_err().code(),
            "cloudflare_session_affinity_ttl_invalid"
        );
        request.session_affinity = CloudflareSessionAffinity::None;
        request.session_affinity_ttl = 1;
        assert_eq!(
            request.validate().unwrap_err().code(),
            "cloudflare_session_affinity_ttl_invalid"
        );
        request.session_affinity = CloudflareSessionAffinity::Header;
        request.session_affinity_ttl = 30;
        request.session_affinity_attributes.headers = BTreeSet::from(["X-Session".to_string()]);
        request.validate().unwrap();
    }

    #[tokio::test]
    async fn rollout_expands_verifies_switches_then_contracts() {
        let monitor_id = id('b');
        let mut desired_pool = map_origin_pool(&pool(), &options(), &transport())
            .unwrap()
            .remove(0)
            .request;
        desired_pool.monitor = None;
        let pool_symbol = CloudflareRolloutSymbol::new("new-pool").unwrap();
        let pool_reference = CloudflareRolloutRef::Created(pool_symbol.clone());
        let load_balancer = CloudflareSymbolicLoadBalancerRequest {
            name: "app.example.com".to_string(),
            enabled: true,
            proxied: true,
            steering_policy: CloudflareSteeringPolicy::Off,
            default_pools: vec![pool_reference.clone()],
            fallback_pool: pool_reference,
            session_affinity: CloudflareSessionAffinity::Cookie,
            session_affinity_ttl: 1_800,
            session_affinity_attributes: CloudflareSessionAffinityAttributes {
                headers: BTreeSet::new(),
                require_all_headers: false,
                drain_duration: 300,
                zero_downtime_failover: CloudflareZeroDowntimeFailover::None,
            },
        };
        let now = current_unix_ms().unwrap();
        let lb_revision = CloudflareCanonicalRevision("lb-revision".to_string());
        let context = CloudflareRolloutContext {
            account_id: "a".repeat(32),
            zone_id: "b".repeat(32),
            credential_revision: "credential-1".to_string(),
            load_balancer_id: id('d'),
            load_balancer_revision: lb_revision,
            now_unix_ms: now,
        };
        let retired_pool_id = id('e');
        let retired_pool_authorization = authorize(ownership_claim(
            CloudflareMutationScope::Account("a".repeat(32)),
            CloudflareReferencedResourceType::Pool,
            retired_pool_id.clone(),
            CloudflareCanonicalRevision("pool-revision".to_string()),
        ))
        .await;
        let retired_pool = CloudflareContractTarget {
            id: retired_pool_id.clone(),
            authorization: retired_pool_authorization,
            retirement: CloudflareRetirementMode::Drain(CloudflareDrainProof {
                account_id: context.account_id.clone(),
                zone_id: context.zone_id.clone(),
                credential_revision: context.credential_revision.clone(),
                load_balancer_id: context.load_balancer_id.clone(),
                load_balancer_revision: context.load_balancer_revision.clone(),
                proxied: true,
                session_affinity: CloudflareSessionAffinity::Cookie,
                drain_duration_seconds: 300,
                entitlement: CapabilityEvidence::Allowed,
                observed_at_unix_ms: now - 1_000,
                valid_until_unix_ms: now + 60_000,
            }),
            reference_evidence: CloudflareReferenceEvidenceRequirement::FreshCenterAndProvider,
        };
        let monitor_authorization = authorize(ownership_claim(
            CloudflareMutationScope::Account("a".repeat(32)),
            CloudflareReferencedResourceType::Monitor,
            monitor_id.clone(),
            CloudflareCanonicalRevision("revision".to_string()),
        ))
        .await;
        let contract = CloudflareContractTarget {
            id: monitor_id.clone(),
            authorization: monitor_authorization,
            retirement: CloudflareRetirementMode::ImmediateDisable,
            reference_evidence: CloudflareReferenceEvidenceRequirement::FreshCenterAndProvider,
        };
        let plan = plan_expand_verify_contract(
            &context,
            None,
            vec![CloudflareSymbolicPoolRequest {
                symbol: pool_symbol,
                request: desired_pool,
                monitor: None,
            }],
            load_balancer,
            vec![retired_pool_id],
            vec![retired_pool],
            vec![contract],
        )
        .unwrap();
        assert!(matches!(
            plan.steps[0],
            CloudflareLoadBalancingRolloutStep::CreatePool(_)
        ));
        assert!(matches!(
            plan.steps[1],
            CloudflareLoadBalancingRolloutStep::VerifyProviderHealthy { .. }
        ));
        assert!(matches!(
            plan.steps[2],
            CloudflareLoadBalancingRolloutStep::AttachOldAndNew(_)
        ));
        assert!(matches!(
            plan.steps[4],
            CloudflareLoadBalancingRolloutStep::DrainAndDisableRetiredPool(_)
        ));
        assert!(matches!(
            plan.steps[7],
            CloudflareLoadBalancingRolloutStep::ContractMonitor(_)
        ));
    }

    #[test]
    fn provider_and_center_health_remain_independent() {
        let health = IndependentOriginHealth {
            provider: Some(CloudflareOriginHealth {
                healthy: false,
                failure_reason: Some("timeout".to_string()),
                response_code: None,
                rtt: None,
            }),
            center: None,
        };
        assert!(!health.provider.unwrap().healthy);
        assert!(health.center.is_none());
    }

    /// Compile-only API harness. This is deliberately not a live acceptance
    /// test: disposable-account create/failover/recovery/cleanup remains
    /// pending until a safety-gated executor exists in the integration suite.
    #[test]
    fn compile_only_load_balancing_api_harness() {
        fn assert_api<T: CloudflareLoadBalancingApi>() {}
        assert_api::<CloudflareHttpApi>();
    }
}
