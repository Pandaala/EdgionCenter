//! Provider-neutral origin routing and active-health contracts.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::net::IpAddr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{CloudResourceId, CredentialRef, DomainName, OriginPoolSpec};
use crate::{CoreError, CoreResult};

const MAX_ENDPOINTS: usize = 1_000;
const MAX_HEADER_COUNT: usize = 64;
const MAX_EXPECTED_BODY_BYTES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OriginEndpointName(String);

impl OriginEndpointName {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value.trim() == value
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if !valid {
            return Err(CoreError::InvalidIdentifier {
                kind: "origin endpoint",
                value,
            });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> CoreResult<()> {
        Self::new(self.0.clone()).map(|_| ())
    }
}

impl Display for OriginEndpointName {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginProtocol {
    Http,
    Https,
    Tcp,
    Tls,
}

impl OriginProtocol {
    pub fn uses_http(self) -> bool {
        matches!(self, Self::Http | Self::Https)
    }

    pub fn uses_tls(self) -> bool {
        matches!(self, Self::Https | Self::Tls)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum OriginAddress {
    Hostname(DomainName),
    Ip(IpAddr),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginDrainState {
    #[default]
    Active,
    Draining,
    Disabled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginTlsMode {
    #[default]
    Verify,
    Insecure,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRequestHeaders {
    /// Non-secret literal headers. Authorization, Cookie, and Proxy-Authorization
    /// are rejected and must be supplied through `secret_ref`.
    #[serde(default)]
    pub literal: BTreeMap<String, String>,
    pub secret_ref: Option<CredentialRef>,
}

impl OriginRequestHeaders {
    pub fn validate(&self) -> CoreResult<()> {
        if self.literal.len() > MAX_HEADER_COUNT {
            return Err(CoreError::Conflict("too many origin headers".to_string()));
        }
        let mut canonical_names = BTreeSet::new();
        for (name, value) in &self.literal {
            let lower = name.to_ascii_lowercase();
            let valid_name = !name.is_empty()
                && name.len() <= 128
                && name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(
                            byte,
                            b'!' | b'#'
                                ..=b'\''
                                    | b'*'
                                    | b'+'
                                    | b'-'
                                    | b'.'
                                    | b'^'
                                    | b'_'
                                    | b'`'
                                    | b'|'
                                    | b'~'
                        )
                });
            if !valid_name
                || !canonical_names.insert(lower.clone())
                || matches!(
                    lower.as_str(),
                    "authorization" | "cookie" | "proxy-authorization"
                )
                || value.len() > 4_096
                || value.chars().any(|character| {
                    (character.is_ascii_control() && character != '\t') || character == '\u{7f}'
                })
            {
                return Err(CoreError::Conflict(
                    "invalid or secret origin header".to_string(),
                ));
            }
        }
        if let Some(reference) = &self.secret_ref {
            reference.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginEndpoint {
    pub name: OriginEndpointName,
    pub address: OriginAddress,
    pub port: u16,
    pub protocol: OriginProtocol,
    pub host_header: Option<DomainName>,
    pub server_name: Option<DomainName>,
    #[serde(default)]
    pub tls_mode: OriginTlsMode,
    pub weight: u16,
    pub priority: u16,
    #[serde(default)]
    pub drain: OriginDrainState,
    #[serde(default)]
    pub headers: OriginRequestHeaders,
}

impl OriginEndpoint {
    pub fn validate(&self) -> CoreResult<()> {
        self.name.validate()?;
        if let OriginAddress::Hostname(name) = &self.address {
            validate_domain_name(name)?;
        }
        if let Some(name) = &self.host_header {
            validate_domain_name(name)?;
        }
        if let Some(name) = &self.server_name {
            validate_domain_name(name)?;
        }
        if self.port == 0 || self.weight > 1_000 {
            return Err(CoreError::Conflict(
                "invalid origin port or weight".to_string(),
            ));
        }
        if !self.protocol.uses_http()
            && (self.host_header.is_some()
                || !self.headers.literal.is_empty()
                || self.headers.secret_ref.is_some())
        {
            return Err(CoreError::Conflict(
                "Host and request headers require an HTTP origin".to_string(),
            ));
        }
        if !self.protocol.uses_tls() && self.server_name.is_some() {
            return Err(CoreError::Conflict("SNI requires a TLS origin".to_string()));
        }
        if !self.protocol.uses_tls() && self.tls_mode != OriginTlsMode::Verify {
            return Err(CoreError::Conflict(
                "TLS verification mode requires a TLS origin".to_string(),
            ));
        }
        if self.drain == OriginDrainState::Active && self.weight == 0 {
            return Err(CoreError::Conflict(
                "an active origin must have positive weight".to_string(),
            ));
        }
        self.headers.validate()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OriginEndpointWire {
    name: OriginEndpointName,
    address: OriginAddress,
    port: u16,
    protocol: OriginProtocol,
    host_header: Option<DomainName>,
    server_name: Option<DomainName>,
    #[serde(default)]
    tls_mode: OriginTlsMode,
    weight: u16,
    priority: u16,
    drain: Option<OriginDrainState>,
    /// CLD-01 serialized this boolean before drain acquired explicit states.
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    headers: OriginRequestHeaders,
}

impl<'de> Deserialize<'de> for OriginEndpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = OriginEndpointWire::deserialize(deserializer)?;
        let legacy_drain = wire.enabled.map(|enabled| {
            if enabled {
                OriginDrainState::Active
            } else {
                OriginDrainState::Disabled
            }
        });
        if wire
            .drain
            .zip(legacy_drain)
            .is_some_and(|(drain, legacy)| drain != legacy)
        {
            return Err(serde::de::Error::custom(
                "origin drain conflicts with legacy enabled state",
            ));
        }
        Ok(Self {
            name: wire.name,
            address: wire.address,
            port: wire.port,
            protocol: wire.protocol,
            host_header: wire.host_header,
            server_name: wire.server_name,
            tls_mode: wire.tls_mode,
            weight: wire.weight,
            priority: wire.priority,
            drain: wire.drain.or(legacy_drain).unwrap_or_default(),
            headers: wire.headers,
        })
    }
}

fn validate_domain_name(name: &DomainName) -> CoreResult<()> {
    let canonical = DomainName::new(name.as_str())?;
    if canonical != *name {
        return Err(CoreError::Conflict(
            "origin domain name is not canonical".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HealthCheckMethod {
    Get,
    Head,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckExpectedResponse {
    #[serde(default)]
    pub statuses: BTreeSet<u16>,
    pub body_contains: Option<String>,
}

impl HealthCheckExpectedResponse {
    fn validate(&self, http: bool) -> CoreResult<()> {
        if !http && (!self.statuses.is_empty() || self.body_contains.is_some()) {
            return Err(CoreError::Conflict(
                "expected HTTP response requires an HTTP health check".to_string(),
            ));
        }
        if self
            .statuses
            .iter()
            .any(|status| !(100..=599).contains(status))
            || self
                .body_contains
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > MAX_EXPECTED_BODY_BYTES)
        {
            return Err(CoreError::Conflict(
                "invalid health-check response expectation".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HealthCheckSourceRegion(String);

impl HealthCheckSourceRegion {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(CoreError::InvalidIdentifier {
                kind: "health-check source region",
                value,
            });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> CoreResult<()> {
        Self::new(self.0.clone()).map(|_| ())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "regions", rename_all = "snake_case")]
pub enum HealthCheckSourceScope {
    #[default]
    ProviderDefault,
    Regions(BTreeSet<HealthCheckSourceRegion>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckSpec {
    pub protocol: OriginProtocol,
    pub port: u16,
    pub method: Option<HealthCheckMethod>,
    pub path: Option<String>,
    #[serde(default)]
    pub headers: OriginRequestHeaders,
    pub interval_seconds: u32,
    pub timeout_seconds: u32,
    pub healthy_threshold: u16,
    pub unhealthy_threshold: u16,
    pub expected: HealthCheckExpectedResponse,
    #[serde(default)]
    pub sources: HealthCheckSourceScope,
}

impl HealthCheckSpec {
    pub fn validate(&self) -> CoreResult<()> {
        let http = self.protocol.uses_http();
        if self.port == 0
            || self.interval_seconds == 0
            || self.timeout_seconds == 0
            || self.timeout_seconds >= self.interval_seconds
            || self.healthy_threshold == 0
            || self.unhealthy_threshold == 0
        {
            return Err(CoreError::Conflict(
                "invalid health-check timing".to_string(),
            ));
        }
        if http != self.method.is_some() || http != self.path.is_some() {
            return Err(CoreError::Conflict(
                "HTTP health checks require method and absolute path".to_string(),
            ));
        }
        if self.path.as_ref().is_some_and(|path| {
            !path.starts_with('/') || path.len() > 2_048 || path.chars().any(char::is_control)
        }) {
            return Err(CoreError::Conflict("invalid health-check path".to_string()));
        }
        if matches!(&self.sources, HealthCheckSourceScope::Regions(regions) if regions.is_empty()) {
            return Err(CoreError::Conflict(
                "explicit health-check source regions cannot be empty".to_string(),
            ));
        }
        if let HealthCheckSourceScope::Regions(regions) = &self.sources {
            for region in regions {
                region.validate()?;
            }
        }
        if !http && (!self.headers.literal.is_empty() || self.headers.secret_ref.is_some()) {
            return Err(CoreError::Conflict(
                "request headers require an HTTP health check".to_string(),
            ));
        }
        self.headers.validate()?;
        self.expected.validate(http)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginFailoverMode {
    /// All healthy origins in the first available priority tier may receive traffic.
    #[default]
    PriorityTiers,
    /// Every healthy active origin may receive traffic regardless of priority.
    AllHealthy,
}

impl OriginPoolSpec {
    pub fn validate(&self) -> CoreResult<()> {
        if self.endpoints.is_empty()
            || self.endpoints.len() > MAX_ENDPOINTS
            || self.minimum_healthy == 0
        {
            return Err(CoreError::Conflict(
                "origin pool requires bounded endpoints and positive minimum health".to_string(),
            ));
        }
        let mut names = BTreeSet::new();
        for endpoint in &self.endpoints {
            endpoint.validate()?;
            if !names.insert(endpoint.name.clone()) {
                return Err(CoreError::Conflict(format!(
                    "duplicate origin endpoint {}",
                    endpoint.name
                )));
            }
        }
        let selectable = self
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.drain == OriginDrainState::Active)
            .count();
        if usize::from(self.minimum_healthy) > selectable {
            return Err(CoreError::Conflict(
                "minimum healthy origins exceeds active origins".to_string(),
            ));
        }
        if let Some(check) = &self.health_check {
            check.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginHealthState {
    Unknown,
    Healthy,
    Unhealthy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginHealthSource {
    CenterActive,
    Provider,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginHealthObservation {
    pub endpoint: OriginEndpointName,
    pub source: OriginHealthSource,
    pub state: OriginHealthState,
    pub consecutive_successes: u16,
    pub consecutive_failures: u16,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
    pub last_transition_unix_ms: i64,
    pub reason_code: String,
}

impl OriginHealthObservation {
    pub fn is_fresh_at(&self, now_unix_ms: i64) -> bool {
        self.observed_at_unix_ms <= now_unix_ms && now_unix_ms < self.valid_until_unix_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginProbeSample {
    pub endpoint: OriginEndpointName,
    pub source: OriginHealthSource,
    pub succeeded: bool,
    pub observed_at_unix_ms: i64,
    pub reason_code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OriginHealthTransitionPolicy {
    pub valid_for_ms: u64,
    pub healthy_threshold: u16,
    pub unhealthy_threshold: u16,
}

pub fn evaluate_origin_probe(
    previous: Option<&OriginHealthObservation>,
    sample: OriginProbeSample,
    policy: OriginHealthTransitionPolicy,
) -> CoreResult<OriginHealthObservation> {
    let OriginProbeSample {
        endpoint,
        source,
        succeeded,
        observed_at_unix_ms,
        reason_code,
    } = sample;
    let OriginHealthTransitionPolicy {
        valid_for_ms,
        healthy_threshold,
        unhealthy_threshold,
    } = policy;
    if valid_for_ms == 0 || healthy_threshold == 0 || unhealthy_threshold == 0 {
        return Err(CoreError::Conflict(
            "invalid health observation policy".to_string(),
        ));
    }
    if previous.is_some_and(|value| {
        value.endpoint != endpoint
            || value.source != source
            || value.observed_at_unix_ms >= observed_at_unix_ms
    }) {
        return Err(CoreError::Conflict(
            "health observation does not continue the same monotonic stream".to_string(),
        ));
    }
    let fresh_previous = previous.filter(|value| value.is_fresh_at(observed_at_unix_ms));
    let (successes, failures) = if succeeded {
        (
            fresh_previous.map_or(1, |value| value.consecutive_successes.saturating_add(1)),
            0,
        )
    } else {
        (
            0,
            fresh_previous.map_or(1, |value| value.consecutive_failures.saturating_add(1)),
        )
    };
    let previous_state = fresh_previous.map_or(OriginHealthState::Unknown, |value| value.state);
    let state = if succeeded && successes >= healthy_threshold {
        OriginHealthState::Healthy
    } else if !succeeded && failures >= unhealthy_threshold {
        OriginHealthState::Unhealthy
    } else {
        previous_state
    };
    let valid_for_ms = i64::try_from(valid_for_ms)
        .map_err(|_| CoreError::Conflict("health freshness window is too large".to_string()))?;
    let valid_until_unix_ms = observed_at_unix_ms
        .checked_add(valid_for_ms)
        .ok_or_else(|| CoreError::Conflict("health freshness window overflows".to_string()))?;
    if !reason_code
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_uppercase())
        || reason_code.len() > 128
        || !reason_code.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(CoreError::Conflict(
            "invalid health reason code".to_string(),
        ));
    }
    Ok(OriginHealthObservation {
        endpoint,
        source,
        state,
        consecutive_successes: successes,
        consecutive_failures: failures,
        observed_at_unix_ms,
        valid_until_unix_ms,
        last_transition_unix_ms: fresh_previous
            .filter(|value| value.state == state)
            .map_or(observed_at_unix_ms, |value| value.last_transition_unix_ms),
        reason_code,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginSelection<'a> {
    pub priority: Option<u16>,
    pub endpoints: Vec<&'a OriginEndpoint>,
}

pub fn select_origin_tier<'a>(
    spec: &'a OriginPoolSpec,
    health: &BTreeMap<(OriginEndpointName, OriginHealthSource), OriginHealthObservation>,
    source: OriginHealthSource,
    now_unix_ms: i64,
) -> OriginSelection<'a> {
    let healthy = |endpoint: &&OriginEndpoint| {
        endpoint.drain == OriginDrainState::Active
            && health
                .get(&(endpoint.name.clone(), source))
                .is_some_and(|observation| {
                    observation.endpoint == endpoint.name
                        && observation.source == source
                        && observation.state == OriginHealthState::Healthy
                        && observation.is_fresh_at(now_unix_ms)
                })
    };
    match spec.failover_mode {
        OriginFailoverMode::AllHealthy => {
            let endpoints: Vec<_> = spec.endpoints.iter().filter(healthy).collect();
            OriginSelection {
                priority: None,
                endpoints: if endpoints.len() >= usize::from(spec.minimum_healthy) {
                    endpoints
                } else {
                    Vec::new()
                },
            }
        }
        OriginFailoverMode::PriorityTiers => {
            let priorities = spec
                .endpoints
                .iter()
                .filter(healthy)
                .map(|endpoint| endpoint.priority)
                .collect::<BTreeSet<_>>();
            let priority = priorities.into_iter().find(|priority| {
                spec.endpoints
                    .iter()
                    .filter(healthy)
                    .filter(|endpoint| endpoint.priority == *priority)
                    .count()
                    >= usize::from(spec.minimum_healthy)
            });
            OriginSelection {
                priority,
                endpoints: spec
                    .endpoints
                    .iter()
                    .filter(healthy)
                    .filter(|endpoint| Some(endpoint.priority) == priority)
                    .collect(),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginPoolCapabilities {
    pub protocols: BTreeSet<OriginProtocol>,
    pub independent_host_and_sni: bool,
    pub weighted_origins: bool,
    pub priority_failover: bool,
    pub graceful_drain: bool,
    pub selectable_health_regions: bool,
}

impl OriginPoolCapabilities {
    pub fn validate_spec(&self, spec: &OriginPoolSpec) -> CoreResult<()> {
        spec.validate()?;
        if spec
            .endpoints
            .iter()
            .any(|endpoint| !self.protocols.contains(&endpoint.protocol))
        {
            return Err(CoreError::Unsupported("origin protocol"));
        }
        if !self.independent_host_and_sni
            && spec.endpoints.iter().any(|endpoint| {
                endpoint.host_header.as_ref() != endpoint.server_name.as_ref()
                    && endpoint.server_name.is_some()
            })
        {
            return Err(CoreError::Unsupported("independent origin Host and SNI"));
        }
        if !self.weighted_origins && spec.endpoints.iter().any(|endpoint| endpoint.weight != 1) {
            return Err(CoreError::Unsupported("weighted origins"));
        }
        if !self.priority_failover
            && (spec.failover_mode == OriginFailoverMode::PriorityTiers
                && spec
                    .endpoints
                    .iter()
                    .map(|endpoint| endpoint.priority)
                    .collect::<BTreeSet<_>>()
                    .len()
                    > 1)
        {
            return Err(CoreError::Unsupported("origin priority failover"));
        }
        if !self.graceful_drain
            && spec
                .endpoints
                .iter()
                .any(|endpoint| endpoint.drain == OriginDrainState::Draining)
        {
            return Err(CoreError::Unsupported("graceful origin drain"));
        }
        if !self.selectable_health_regions
            && spec
                .health_check
                .as_ref()
                .is_some_and(|check| matches!(check.sources, HealthCheckSourceScope::Regions(_)))
        {
            return Err(CoreError::Unsupported("selectable health-check regions"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginHealthRequest {
    pub pool_id: CloudResourceId,
    pub endpoint: OriginEndpointName,
    pub desired_generation: u64,
}

#[async_trait]
pub trait OriginHealthObserver: Send + Sync {
    async fn observe(
        &self,
        request: &OriginHealthRequest,
    ) -> Result<Vec<OriginHealthObservation>, super::NormalizedProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(
        endpoint: OriginEndpointName,
        source: OriginHealthSource,
        succeeded: bool,
        observed_at_unix_ms: i64,
        reason_code: &str,
    ) -> OriginProbeSample {
        OriginProbeSample {
            endpoint,
            source,
            succeeded,
            observed_at_unix_ms,
            reason_code: reason_code.to_string(),
        }
    }

    fn policy(
        valid_for_ms: u64,
        healthy_threshold: u16,
        unhealthy_threshold: u16,
    ) -> OriginHealthTransitionPolicy {
        OriginHealthTransitionPolicy {
            valid_for_ms,
            healthy_threshold,
            unhealthy_threshold,
        }
    }

    fn endpoint(name: &str, priority: u16) -> OriginEndpoint {
        OriginEndpoint {
            name: OriginEndpointName::new(name).unwrap(),
            address: OriginAddress::Hostname(
                DomainName::new(format!("{name}.example.com")).unwrap(),
            ),
            port: 443,
            protocol: OriginProtocol::Https,
            host_header: Some(DomainName::new("app.example.com").unwrap()),
            server_name: Some(DomainName::new("tls.example.com").unwrap()),
            tls_mode: OriginTlsMode::Verify,
            weight: 100,
            priority,
            drain: OriginDrainState::Active,
            headers: OriginRequestHeaders {
                literal: BTreeMap::from([("X-Health".to_string(), "ready".to_string())]),
                secret_ref: Some(CredentialRef::new("secret/origin-header").unwrap()),
            },
        }
    }

    #[test]
    fn host_sni_and_destination_are_independent_and_secret_values_are_absent() {
        let value = endpoint("primary", 0);
        assert_ne!(value.host_header, value.server_name);
        let json = serde_json::to_string(&value).unwrap();
        assert!(json.contains("secret/origin-header"));
        assert!(!json.contains("secret-value"));
        value.validate().unwrap();
    }

    #[test]
    fn hysteresis_and_freshness_are_fail_closed() {
        let name = OriginEndpointName::new("primary").unwrap();
        let first = evaluate_origin_probe(
            None,
            sample(
                name.clone(),
                OriginHealthSource::CenterActive,
                true,
                1_000,
                "ProbePassed",
            ),
            policy(2_000, 2, 2),
        )
        .unwrap();
        assert_eq!(first.state, OriginHealthState::Unknown);
        let healthy = evaluate_origin_probe(
            Some(&first),
            sample(
                name.clone(),
                OriginHealthSource::CenterActive,
                true,
                2_000,
                "ProbePassed",
            ),
            policy(2_000, 2, 2),
        )
        .unwrap();
        assert_eq!(healthy.state, OriginHealthState::Healthy);
        assert!(healthy.is_fresh_at(3_999));
        assert!(!healthy.is_fresh_at(4_000));
        let one_failure = evaluate_origin_probe(
            Some(&healthy),
            sample(
                name,
                OriginHealthSource::CenterActive,
                false,
                3_000,
                "ProbeFailed",
            ),
            policy(2_000, 2, 2),
        )
        .unwrap();
        assert_eq!(one_failure.state, OriginHealthState::Healthy);
    }

    #[test]
    fn active_active_and_active_passive_selection_are_explicit() {
        let mut spec = OriginPoolSpec {
            provider_account_ref: None,
            endpoints: vec![
                endpoint("primary-a", 0),
                endpoint("primary-b", 0),
                endpoint("backup", 1),
            ],
            health_check: None,
            failover_mode: OriginFailoverMode::PriorityTiers,
            minimum_healthy: 1,
        };
        spec.validate().unwrap();
        let observations = spec
            .endpoints
            .iter()
            .map(|endpoint| {
                (
                    (endpoint.name.clone(), OriginHealthSource::Provider),
                    evaluate_origin_probe(
                        None,
                        sample(
                            endpoint.name.clone(),
                            OriginHealthSource::Provider,
                            true,
                            1_000,
                            "ProviderHealthy",
                        ),
                        policy(10_000, 1, 1),
                    )
                    .unwrap(),
                )
            })
            .collect();
        let selected =
            select_origin_tier(&spec, &observations, OriginHealthSource::Provider, 2_000);
        assert_eq!(selected.priority, Some(0));
        assert_eq!(selected.endpoints.len(), 2);

        spec.failover_mode = OriginFailoverMode::AllHealthy;
        let selected =
            select_origin_tier(&spec, &observations, OriginHealthSource::Provider, 2_000);
        assert_eq!(selected.priority, None);
        assert_eq!(selected.endpoints.len(), 3);
    }

    #[test]
    fn selection_requires_minimum_health_in_one_eligible_tier() {
        let spec = OriginPoolSpec {
            provider_account_ref: None,
            endpoints: vec![
                endpoint("primary-a", 0),
                endpoint("primary-b", 0),
                endpoint("backup-a", 1),
                endpoint("backup-b", 1),
            ],
            health_check: None,
            failover_mode: OriginFailoverMode::PriorityTiers,
            minimum_healthy: 2,
        };
        let observations = spec
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.name.as_str() != "primary-b")
            .map(|endpoint| {
                let observation = evaluate_origin_probe(
                    None,
                    sample(
                        endpoint.name.clone(),
                        OriginHealthSource::Provider,
                        true,
                        1_000,
                        "ProviderHealthy",
                    ),
                    policy(10_000, 1, 1),
                )
                .unwrap();
                (
                    (endpoint.name.clone(), OriginHealthSource::Provider),
                    observation,
                )
            })
            .collect();

        let selected =
            select_origin_tier(&spec, &observations, OriginHealthSource::Provider, 2_000);
        assert_eq!(selected.priority, Some(1));
        assert_eq!(selected.endpoints.len(), 2);
    }

    #[test]
    fn health_sources_and_expired_hysteresis_are_isolated() {
        let name = OriginEndpointName::new("primary").unwrap();
        let expired = evaluate_origin_probe(
            None,
            sample(
                name.clone(),
                OriginHealthSource::CenterActive,
                true,
                1_000,
                "ProbePassed",
            ),
            policy(1_000, 1, 2),
        )
        .unwrap();
        let reset = evaluate_origin_probe(
            Some(&expired),
            sample(
                name.clone(),
                OriginHealthSource::CenterActive,
                false,
                2_000,
                "ProbeFailed",
            ),
            policy(1_000, 1, 2),
        )
        .unwrap();
        assert_eq!(reset.state, OriginHealthState::Unknown);
        assert_eq!(reset.consecutive_failures, 1);

        let spec = OriginPoolSpec {
            provider_account_ref: None,
            endpoints: vec![endpoint("primary", 0)],
            health_check: None,
            failover_mode: OriginFailoverMode::PriorityTiers,
            minimum_healthy: 1,
        };
        let observations = BTreeMap::from([((name, OriginHealthSource::CenterActive), expired)]);
        let provider_selection =
            select_origin_tier(&spec, &observations, OriginHealthSource::Provider, 1_500);
        assert!(provider_selection.endpoints.is_empty());
    }

    #[test]
    fn legacy_disabled_endpoint_migrates_without_becoming_active() {
        let mut value = serde_json::to_value(endpoint("legacy", 0)).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("drain");
        object.insert("enabled".to_string(), serde_json::Value::Bool(false));
        let migrated: OriginEndpoint = serde_json::from_value(value).unwrap();
        assert_eq!(migrated.drain, OriginDrainState::Disabled);
        assert!(!serde_json::to_string(&migrated)
            .unwrap()
            .contains("enabled"));
    }

    #[test]
    fn headers_reject_case_duplicates_and_control_bytes() {
        let duplicate = OriginRequestHeaders {
            literal: BTreeMap::from([
                ("X-Test".to_string(), "one".to_string()),
                ("x-test".to_string(), "two".to_string()),
            ]),
            secret_ref: None,
        };
        assert!(duplicate.validate().is_err());

        let control = OriginRequestHeaders {
            literal: BTreeMap::from([("X-Test".to_string(), "one\0two".to_string())]),
            secret_ref: None,
        };
        assert!(control.validate().is_err());
    }
}
