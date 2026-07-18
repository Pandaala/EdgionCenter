//! Cloudflare Origin Rules adapter.
//!
//! The adapter intentionally manages individual rules in the
//! `http_request_origin` entrypoint. It never replaces a complete ruleset and
//! never treats a Center database row as provider ownership proof.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use async_trait::async_trait;
use edgion_center_core::{
    AbsoluteDnsName, CloudProvider, CloudResourceId, NormalizedProviderError, ProviderAccountScope,
    ProviderAccountSpec, ProviderErrorCategory,
};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{CloudflareApiResult, CloudflareHttpApi};

const ORIGIN_PHASE: &str = "http_request_origin";
const OWNERSHIP_PREFIX: &str = "edgion-center-origin-rule:";
const MAX_RULES: usize = 300;
const MAX_DESCRIPTION_BYTES: usize = 500;
const MAX_PATH_BYTES: usize = 4_096;

/// A Center-controlled rule key. It is rendered into Cloudflare's stable
/// `ref` field and must not contain provider-generated identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OriginRuleKey(String);

impl OriginRuleKey {
    pub fn new(value: impl Into<String>) -> CloudflareApiResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 96
            || !value.starts_with(|value: char| value.is_ascii_lowercase())
            || !value
                .bytes()
                .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == b'-')
        {
            return Err(validation("invalid_origin_rule_key"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn provider_ref(&self) -> String {
        format!("{OWNERSHIP_PREFIX}{}", self.0)
    }
}

/// Restricted, structurally rendered rule match. Arbitrary Rules language is
/// deliberately excluded so a caller cannot smuggle an expression through a
/// string field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRuleMatch {
    pub hostname: AbsoluteDnsName,
    #[serde(default)]
    pub path: Option<OriginPathMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OriginPathMatch {
    Exact(String),
    Prefix(String),
}

impl OriginRuleMatch {
    pub fn validate(&self) -> CloudflareApiResult<()> {
        if let Some(path) = self.path.as_ref() {
            let value = path.value();
            if value.is_empty()
                || !value.starts_with('/')
                || value.len() > MAX_PATH_BYTES
                || value.chars().any(char::is_control)
            {
                return Err(validation("invalid_origin_rule_path"));
            }
        }
        Ok(())
    }

    pub fn expression(&self) -> CloudflareApiResult<String> {
        self.validate()?;
        let hostname = rules_literal(self.hostname.as_str())?;
        let host = format!("http.host eq {hostname}");
        Ok(match self.path.as_ref() {
            None => format!("({host})"),
            Some(OriginPathMatch::Exact(path)) => format!(
                "({host} and http.request.uri.path eq {})",
                rules_literal(path)?
            ),
            Some(OriginPathMatch::Prefix(path)) => format!(
                "({host} and starts_with(http.request.uri.path, {}))",
                rules_literal(path)?
            ),
        })
    }

    pub fn matches(&self, request: &OriginRuleTestRequest) -> bool {
        if self.hostname.as_str() != request.hostname.as_str() {
            return false;
        }
        match self.path.as_ref() {
            None => true,
            Some(OriginPathMatch::Exact(path)) => request.path == *path,
            Some(OriginPathMatch::Prefix(path)) => request.path.starts_with(path),
        }
    }
}

impl OriginPathMatch {
    fn value(&self) -> &str {
        match self {
            Self::Exact(value) | Self::Prefix(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRuleTestRequest {
    pub hostname: AbsoluteDnsName,
    pub path: String,
}

impl OriginRuleTestRequest {
    pub fn validate(&self) -> CloudflareApiResult<()> {
        if self.path.is_empty()
            || !self.path.starts_with('/')
            || self.path.len() > MAX_PATH_BYTES
            || self.path.chars().any(char::is_control)
        {
            return Err(validation("invalid_origin_rule_test_path"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OriginDnsTarget {
    Hostname(AbsoluteDnsName),
}

impl OriginDnsTarget {
    fn render(&self) -> String {
        match self {
            Self::Hostname(value) => value.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRuleOverrides {
    #[serde(default)]
    pub dns_target: Option<OriginDnsTarget>,
    #[serde(default)]
    pub host_header: Option<AbsoluteDnsName>,
    #[serde(default)]
    pub sni: Option<AbsoluteDnsName>,
    #[serde(default)]
    pub port: Option<u16>,
}

impl OriginRuleOverrides {
    pub fn validate(&self) -> CloudflareApiResult<()> {
        if self.dns_target.is_none()
            && self.host_header.is_none()
            && self.sni.is_none()
            && self.port.is_none()
        {
            return Err(validation("empty_origin_rule_overrides"));
        }
        if self.port == Some(0) {
            return Err(validation("invalid_origin_rule_port"));
        }
        if self.dns_target.as_ref().is_some_and(|target| match target {
            OriginDnsTarget::Hostname(value) => value.as_str().parse::<std::net::IpAddr>().is_ok(),
        }) {
            return Err(validation("cloudflare_origin_host_must_be_hostname"));
        }
        Ok(())
    }

    fn effective_sni(&self, request_hostname: &AbsoluteDnsName) -> AbsoluteDnsName {
        self.sni
            .clone()
            .or_else(|| self.host_header.clone())
            .unwrap_or_else(|| request_hostname.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesiredOriginRule {
    pub key: OriginRuleKey,
    /// Lower values execute first. Cloudflare applies the last matching
    /// non-terminating override for each field.
    pub priority: u32,
    pub match_rule: OriginRuleMatch,
    pub overrides: OriginRuleOverrides,
    pub description: String,
    pub enabled: bool,
    /// Earlier Center-owned rules this rule is deliberately allowed to
    /// override when both match. The reference makes last-match-wins behavior
    /// explicit instead of an accidental consequence of ordering.
    #[serde(default)]
    pub explicitly_overrides_keys: BTreeSet<OriginRuleKey>,
}

impl DesiredOriginRule {
    pub fn validate(&self) -> CloudflareApiResult<()> {
        self.match_rule.validate()?;
        self.overrides.validate()?;
        if self.priority == 0
            || self.description.len() > MAX_DESCRIPTION_BYTES
            || self.description.chars().any(char::is_control)
        {
            return Err(validation("invalid_origin_rule_metadata"));
        }
        if self.explicitly_overrides_keys.contains(&self.key) {
            return Err(validation("origin_rule_cannot_override_itself"));
        }
        Ok(())
    }
}

/// Independent evidence supplied by origin/certificate observation. Because a
/// Host override also changes SNI unless an explicit SNI override is present,
/// this evidence is mandatory for either field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginTlsCompatibility {
    pub account_id: String,
    pub endpoint_id: String,
    pub endpoint_revision: String,
    pub effective_sni: AbsoluteDnsName,
    /// Exact DNS names or a one-label wildcard (`*.example.com`).
    pub certificate_names: BTreeSet<String>,
    pub observed_at_unix_ms: u64,
    pub valid_until_unix_ms: u64,
}

impl OriginTlsCompatibility {
    fn validate_for(
        &self,
        account_id: &str,
        endpoint: &CloudflareOriginEndpointEvidence,
        rule: &DesiredOriginRule,
        now_unix_ms: u64,
    ) -> CloudflareApiResult<()> {
        if self.account_id != account_id
            || self.endpoint_id != endpoint.endpoint_id
            || self.endpoint_revision != endpoint.endpoint_revision
            || self.endpoint_revision.is_empty()
            || self.observed_at_unix_ms > now_unix_ms
            || now_unix_ms >= self.valid_until_unix_ms
        {
            return Err(conflict("origin_tls_evidence_scope_or_freshness_mismatch"));
        }
        for name in &self.certificate_names {
            validate_certificate_name(name)?;
        }
        let expected = rule.overrides.effective_sni(&rule.match_rule.hostname);
        if self.effective_sni != expected
            || !self
                .certificate_names
                .iter()
                .any(|name| certificate_name_covers(name, expected.as_str()))
        {
            return Err(conflict("origin_tls_certificate_incompatible"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareOriginEndpointEvidence {
    pub account_id: String,
    pub endpoint_id: String,
    pub endpoint_revision: String,
    pub hostname: AbsoluteDnsName,
    pub proxied: bool,
    pub observed_at_unix_ms: u64,
    pub valid_until_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRulesCapabilityEvidence {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub observed_at_unix_ms: u64,
    pub valid_until_unix_ms: u64,
    pub max_rules: usize,
    pub current_rules: usize,
    pub host_override: bool,
    pub sni_override: bool,
    pub dns_override: bool,
    pub port_override: bool,
    pub trace_execute: bool,
    pub proxied_origin_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginRuleOwnership {
    Owned { key: OriginRuleKey },
    Unowned,
}

/// Persisted adoption/create evidence required in addition to the provider
/// `ref` marker. A copied marker alone never proves ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRuleOwnershipProof {
    pub zone_id: String,
    pub ruleset_id: String,
    pub rule_id: String,
    pub rule_revision: String,
    pub key: OriginRuleKey,
    pub match_rule: OriginRuleMatch,
    pub priority: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedOriginRule {
    pub provider_id: String,
    pub provider_version: String,
    pub provider_ref: Option<String>,
    pub ownership: OriginRuleOwnership,
    pub typed_match: Option<OriginRuleMatch>,
    pub owned_priority: Option<u32>,
    pub expression: String,
    pub overrides: OriginRuleOverrides,
    pub description: String,
    pub enabled: bool,
    pub position: usize,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRulesObservation {
    pub ruleset_id: String,
    pub ruleset_version: String,
    pub ruleset_revision: String,
    pub rules: Vec<ObservedOriginRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRuleFence {
    pub ruleset_id: String,
    pub ruleset_version: String,
    pub ruleset_revision: String,
    pub rule_id: String,
    pub rule_version: String,
    pub rule_revision: String,
    pub ownership_key: OriginRuleKey,
    pub match_rule: OriginRuleMatch,
    pub priority: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplicitOriginConflictOverride {
    /// Exact ruleset revision the operator reviewed.
    pub ruleset_revision: String,
    /// Exact provider IDs of overlapping unowned rules the operator accepts.
    pub accepted_unowned_rule_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRuleMutationPlan {
    pub desired: DesiredOriginRule,
    pub ruleset_id: String,
    pub ruleset_revision: String,
    #[serde(default)]
    pub fence: Option<OriginRuleFence>,
    #[serde(default)]
    pub acknowledged_unowned_conflicts: BTreeSet<String>,
    pub ownership_proofs: Vec<OriginRuleOwnershipProof>,
    pub placement: OriginRulePlacementPlan,
    pub authority: OriginMutationAuthority,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OriginRulePlacementPlan {
    Preserve,
    First,
    BeforeOwned {
        rule_id: String,
        rule_revision: String,
    },
    AfterOwned {
        rule_id: String,
        rule_revision: String,
    },
    AppendAfterAcknowledgedUnowned {
        rule_id: String,
        rule_revision: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginMutationAuthority {
    pub account_id: String,
    pub zone_id: String,
    pub credential_revision: String,
    pub endpoint_revisions: BTreeMap<String, String>,
    pub valid_until_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRuleDeleteSafety {
    pub ruleset_revision: String,
    pub test_request: OriginRuleTestRequest,
    pub effect: OriginRuleDeleteEffect,
    pub traffic_change_ack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum OriginRuleDeleteEffect {
    ExplicitOverrides {
        matching_chain: Vec<OriginRuleKey>,
        effective_overrides: OriginRuleOverrides,
    },
    ProviderDefault {
        provider_default_revision: String,
        effect: ProviderDefaultEffect,
    },
    MayUseProviderDefault {
        matching_chain: Vec<OriginRuleKey>,
        explicit_overrides: Option<OriginRuleOverrides>,
        provider_default_revision: String,
        provider_default_effect: ProviderDefaultEffect,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "overrides", rename_all = "snake_case")]
pub enum ProviderDefaultEffect {
    NoOverride,
    Overrides(OriginRuleOverrides),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDefaultOriginEvidence {
    pub ruleset_revision: String,
    pub test_request: OriginRuleTestRequest,
    pub provider_default_revision: String,
    pub effect: ProviderDefaultEffect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginRuleDeletePlan {
    pub fence: OriginRuleFence,
    pub safety: OriginRuleDeleteSafety,
    pub ownership_proofs: Vec<OriginRuleOwnershipProof>,
    pub authority: OriginMutationAuthority,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalOriginRulePreview {
    pub matching_owned_rules: Vec<OriginRuleKey>,
    pub final_overrides: Option<OriginRuleOverrides>,
    /// Always true because arbitrary unowned Cloudflare expressions are not
    /// evaluated by this restricted local preview.
    pub incomplete: bool,
}

/// Locally previews only Center-owned desired rules. It does not claim to
/// emulate arbitrary Cloudflare expressions; use provider Trace when allowed.
pub fn preview_owned_origin_rules(
    rules: &[DesiredOriginRule],
    request: &OriginRuleTestRequest,
) -> CloudflareApiResult<LocalOriginRulePreview> {
    request.validate()?;
    validate_desired_origin_rules(rules)?;
    let mut ordered = rules.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|rule| rule.priority);
    let mut matching = Vec::new();
    let mut final_overrides: Option<OriginRuleOverrides> = None;
    for rule in ordered {
        if rule.enabled && rule.match_rule.matches(request) {
            matching.push(rule.key.clone());
            let result = final_overrides.get_or_insert(OriginRuleOverrides {
                dns_target: None,
                host_header: None,
                sni: None,
                port: None,
            });
            if rule.overrides.dns_target.is_some() {
                result.dns_target = rule.overrides.dns_target.clone();
            }
            if rule.overrides.host_header.is_some() {
                result.host_header = rule.overrides.host_header.clone();
            }
            if rule.overrides.sni.is_some() {
                result.sni = rule.overrides.sni.clone();
            }
            if rule.overrides.port.is_some() {
                result.port = rule.overrides.port;
            }
        }
    }
    Ok(LocalOriginRulePreview {
        matching_owned_rules: matching,
        final_overrides,
        incomplete: true,
    })
}

fn derive_delete_safety(
    observation: &OriginRulesObservation,
    deleting_key: &OriginRuleKey,
    test_request: OriginRuleTestRequest,
    provider_default: Option<&ProviderDefaultOriginEvidence>,
    traffic_change_ack: bool,
) -> CloudflareApiResult<OriginRuleDeleteSafety> {
    test_request.validate()?;
    if !traffic_change_ack {
        return Err(conflict("origin_rule_delete_traffic_change_ack_required"));
    }
    if observation
        .rules
        .iter()
        .any(|rule| rule.ownership == OriginRuleOwnership::Unowned)
    {
        return Err(conflict("origin_rule_delete_opaque_unowned_rule"));
    }
    let deleting = observation
        .rules
        .iter()
        .find(|rule| matches!(&rule.ownership, OriginRuleOwnership::Owned { key } if key == deleting_key))
        .ok_or_else(|| conflict("owned_origin_rule_not_found"))?;
    let deleting_match = deleting
        .typed_match
        .as_ref()
        .ok_or_else(|| conflict("owned_origin_rule_match_proof_missing"))?
        .clone();
    if !deleting_match.matches(&test_request) {
        return Err(conflict("origin_rule_delete_test_request_does_not_match"));
    }
    let mut remaining = observation
        .rules
        .iter()
        .filter(|rule| rule.provider_id != deleting.provider_id && rule.enabled)
        .collect::<Vec<_>>();
    remaining.sort_by_key(|rule| rule.owned_priority.unwrap_or(u32::MAX));
    let mut chain = Vec::new();
    let mut effective: Option<OriginRuleOverrides> = None;
    let mut domain_fields = BTreeSet::new();
    for rule in &remaining {
        let OriginRuleOwnership::Owned { key } = &rule.ownership else {
            continue;
        };
        if rule
            .typed_match
            .as_ref()
            .ok_or_else(|| conflict("owned_origin_rule_match_proof_missing"))?
            .matches(&test_request)
        {
            chain.push(key.clone());
            merge_overrides(
                effective.get_or_insert_with(empty_overrides),
                &rule.overrides,
            );
        }
        if match_covers(
            rule.typed_match
                .as_ref()
                .ok_or_else(|| conflict("owned_origin_rule_match_proof_missing"))?,
            &deleting_match,
        ) {
            domain_fields.extend(override_fields(&rule.overrides));
        }
    }
    let fully_covered = override_fields(&deleting.overrides).is_subset(&domain_fields);
    let effect = if fully_covered {
        let effective_overrides =
            effective.ok_or_else(|| conflict("origin_rule_delete_domain_coverage_inconsistent"))?;
        effective_overrides.validate()?;
        OriginRuleDeleteEffect::ExplicitOverrides {
            matching_chain: chain,
            effective_overrides,
        }
    } else {
        let evidence = provider_default
            .ok_or_else(|| conflict("origin_rule_provider_default_evidence_required"))?;
        if evidence.ruleset_revision != observation.ruleset_revision
            || evidence.test_request != test_request
            || evidence.provider_default_revision.is_empty()
        {
            return Err(conflict("origin_rule_provider_default_evidence_mismatch"));
        }
        validate_provider_default_effect(&evidence.effect)?;
        if let Some(explicit_overrides) = effective {
            OriginRuleDeleteEffect::MayUseProviderDefault {
                matching_chain: chain,
                explicit_overrides: Some(explicit_overrides),
                provider_default_revision: evidence.provider_default_revision.clone(),
                provider_default_effect: evidence.effect.clone(),
            }
        } else {
            OriginRuleDeleteEffect::ProviderDefault {
                provider_default_revision: evidence.provider_default_revision.clone(),
                effect: evidence.effect.clone(),
            }
        }
    };
    Ok(OriginRuleDeleteSafety {
        ruleset_revision: observation.ruleset_revision.clone(),
        test_request,
        effect,
        traffic_change_ack,
    })
}

fn empty_overrides() -> OriginRuleOverrides {
    OriginRuleOverrides {
        dns_target: None,
        host_header: None,
        sni: None,
        port: None,
    }
}

fn validate_provider_default_effect(effect: &ProviderDefaultEffect) -> CloudflareApiResult<()> {
    if let ProviderDefaultEffect::Overrides(overrides) = effect {
        overrides.validate()?;
    }
    Ok(())
}

fn match_covers(covering: &OriginRuleMatch, target: &OriginRuleMatch) -> bool {
    if covering.hostname != target.hostname {
        return false;
    }
    match (covering.path.as_ref(), target.path.as_ref()) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(OriginPathMatch::Exact(left)), Some(OriginPathMatch::Exact(right))) => left == right,
        (Some(OriginPathMatch::Exact(_)), Some(OriginPathMatch::Prefix(_))) => false,
        (Some(OriginPathMatch::Prefix(prefix)), Some(OriginPathMatch::Exact(exact))) => {
            exact.starts_with(prefix)
        }
        (Some(OriginPathMatch::Prefix(cover)), Some(OriginPathMatch::Prefix(target))) => {
            target.starts_with(cover)
        }
    }
}

fn merge_overrides(target: &mut OriginRuleOverrides, value: &OriginRuleOverrides) {
    if value.dns_target.is_some() {
        target.dns_target = value.dns_target.clone();
    }
    if value.host_header.is_some() {
        target.host_header = value.host_header.clone();
    }
    if value.sni.is_some() {
        target.sni = value.sni.clone();
    }
    if value.port.is_some() {
        target.port = value.port;
    }
}

fn parse_origin_trace(
    result: CloudflareTraceResult,
) -> CloudflareApiResult<CloudflareOriginTraceOutcome> {
    let mut outcome = CloudflareOriginTraceOutcome {
        matched_route_steps: Vec::new(),
        effective_overrides: None,
        incomplete: result.status_code.is_none(),
    };
    fn visit(
        items: &[CloudflareTraceItem],
        outcome: &mut CloudflareOriginTraceOutcome,
    ) -> CloudflareApiResult<()> {
        for item in items {
            if item.matched == Some(true) {
                if item.action.as_deref() == Some("route") {
                    let Some(parameters) = item.action_parameters.clone() else {
                        outcome.incomplete = true;
                        continue;
                    };
                    let parameters: CloudflareRouteParameters = serde_json::from_value(parameters)
                        .map_err(|_| validation("cloudflare_trace_route_parameters_invalid"))?;
                    let overrides = map_parameters(&parameters)?;
                    merge_overrides(
                        outcome
                            .effective_overrides
                            .get_or_insert_with(empty_overrides),
                        &overrides,
                    );
                    outcome.matched_route_steps.push(
                        item.step_name
                            .clone()
                            .unwrap_or_else(|| "unknown-route-step".to_string()),
                    );
                } else if item.action.is_some() {
                    outcome.incomplete = true;
                }
            }
            visit(&item.trace, outcome)?;
        }
        Ok(())
    }
    visit(&result.trace, &mut outcome)?;
    Ok(outcome)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudflareRouteOrigin {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudflareRouteSni {
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudflareRouteParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<CloudflareRouteOrigin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sni: Option<CloudflareRouteSni>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudflareOriginRule {
    pub id: String,
    pub version: String,
    pub action: String,
    pub action_parameters: CloudflareRouteParameters,
    pub expression: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(rename = "ref", default)]
    pub rule_ref: Option<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudflareOriginRuleset {
    pub id: String,
    pub version: String,
    pub phase: String,
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub last_updated: Option<String>,
    #[serde(default)]
    pub rules: Vec<CloudflareOriginRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum CloudflareRulePosition {
    Before { before: String },
    After { after: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloudflareOriginRuleWrite {
    pub action: &'static str,
    pub action_parameters: CloudflareRouteParameters,
    pub expression: String,
    pub description: String,
    pub enabled: bool,
    #[serde(rename = "ref")]
    pub rule_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<CloudflareRulePosition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloudflareCreateOriginRulesetRequest {
    pub name: &'static str,
    pub description: &'static str,
    pub kind: &'static str,
    pub phase: &'static str,
    pub rules: Vec<CloudflareOriginRuleWrite>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloudflareTraceRequest {
    url: String,
    method: String,
    protocol: &'static str,
    skip_response: bool,
    #[serde(skip)]
    account_id: String,
    #[serde(skip)]
    endpoint_id: String,
    #[serde(skip)]
    endpoint_revision: String,
    #[serde(skip)]
    valid_until_unix_ms: u64,
}

impl CloudflareTraceRequest {
    pub fn new(
        endpoint: &CloudflareOriginEndpointEvidence,
        path: &str,
        method: impl Into<String>,
        now_unix_ms: u64,
    ) -> CloudflareApiResult<Self> {
        validate_cf_id(&endpoint.account_id, "invalid_cloudflare_account_id")?;
        if endpoint.endpoint_id.is_empty()
            || endpoint.endpoint_revision.is_empty()
            || endpoint.observed_at_unix_ms > now_unix_ms
            || now_unix_ms >= endpoint.valid_until_unix_ms
        {
            return Err(validation("invalid_cloudflare_trace_endpoint_evidence"));
        }
        OriginRuleTestRequest {
            hostname: endpoint.hostname.clone(),
            path: path.to_string(),
        }
        .validate()?;
        let method = method.into().to_ascii_uppercase();
        if !matches!(method.as_str(), "GET" | "HEAD") {
            return Err(validation("invalid_cloudflare_trace_method"));
        }
        Ok(Self {
            url: format!("https://{}{path}", endpoint.hostname.as_str()),
            method,
            protocol: "HTTP/1.1",
            skip_response: true,
            account_id: endpoint.account_id.clone(),
            endpoint_id: endpoint.endpoint_id.clone(),
            endpoint_revision: endpoint.endpoint_revision.clone(),
            valid_until_unix_ms: endpoint.valid_until_unix_ms,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn hostname(&self) -> CloudflareApiResult<AbsoluteDnsName> {
        let url =
            Url::parse(&self.url).map_err(|_| validation("invalid_cloudflare_trace_request"))?;
        AbsoluteDnsName::new(
            url.host_str()
                .ok_or_else(|| validation("invalid_cloudflare_trace_request"))?,
        )
        .map_err(|_| validation("invalid_cloudflare_trace_request"))
    }

    fn validate(&self, account_id: &str, now_unix_ms: Option<u64>) -> CloudflareApiResult<()> {
        let url =
            Url::parse(&self.url).map_err(|_| validation("invalid_cloudflare_trace_request"))?;
        if self.account_id != account_id
            || self.endpoint_id.is_empty()
            || self.endpoint_revision.is_empty()
            || url.scheme() != "https"
            || url.username() != ""
            || url.password().is_some()
            || url.fragment().is_some()
            || !matches!(self.method.as_str(), "GET" | "HEAD")
            || self.protocol != "HTTP/1.1"
            || !self.skip_response
            || now_unix_ms.is_some_and(|now| now >= self.valid_until_unix_ms)
        {
            return Err(validation("invalid_cloudflare_trace_request"));
        }
        self.hostname()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CloudflareTraceResult {
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub trace: Vec<CloudflareTraceItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CloudflareTraceItem {
    #[serde(rename = "type", default)]
    pub item_type: Option<String>,
    #[serde(default)]
    pub step_name: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub expression: Option<String>,
    #[serde(default)]
    pub matched: Option<bool>,
    #[serde(default)]
    pub action_parameters: Option<serde_json::Value>,
    #[serde(default)]
    pub trace: Vec<CloudflareTraceItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareOriginTraceOutcome {
    pub matched_route_steps: Vec<String>,
    pub effective_overrides: Option<OriginRuleOverrides>,
    pub incomplete: bool,
}

#[async_trait]
pub trait CloudflareOriginRulesApi: Send + Sync {
    async fn create_origin_ruleset(
        &self,
        zone_id: &str,
        request: &CloudflareCreateOriginRulesetRequest,
    ) -> CloudflareApiResult<CloudflareOriginRuleset>;

    async fn get_origin_ruleset(
        &self,
        zone_id: &str,
    ) -> CloudflareApiResult<Option<CloudflareOriginRuleset>>;

    async fn create_origin_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        request: &CloudflareOriginRuleWrite,
    ) -> CloudflareApiResult<CloudflareOriginRuleset>;

    async fn patch_origin_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
        request: &CloudflareOriginRuleWrite,
    ) -> CloudflareApiResult<CloudflareOriginRuleset>;

    async fn delete_origin_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
    ) -> CloudflareApiResult<CloudflareOriginRuleset>;

    async fn trace_request(
        &self,
        account_id: &str,
        request: &CloudflareTraceRequest,
    ) -> CloudflareApiResult<CloudflareTraceResult>;
}

#[async_trait]
impl CloudflareOriginRulesApi for CloudflareHttpApi {
    async fn create_origin_ruleset(
        &self,
        zone_id: &str,
        request: &CloudflareCreateOriginRulesetRequest,
    ) -> CloudflareApiResult<CloudflareOriginRuleset> {
        validate_cf_id(zone_id, "invalid_cloudflare_zone_id")?;
        let body = serde_json::to_value(request)
            .map_err(|_| validation("cloudflare_origin_ruleset_encoding_failed"))?;
        self.mutation_result(
            Method::POST,
            &format!("zones/{zone_id}/rulesets"),
            Some(&body),
        )
        .await
    }

    async fn get_origin_ruleset(
        &self,
        zone_id: &str,
    ) -> CloudflareApiResult<Option<CloudflareOriginRuleset>> {
        validate_cf_id(zone_id, "invalid_cloudflare_zone_id")?;
        let path = format!("zones/{zone_id}/rulesets/phases/{ORIGIN_PHASE}/entrypoint");
        match self.read_result(&path, &[]).await {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.category() == ProviderErrorCategory::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn create_origin_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        request: &CloudflareOriginRuleWrite,
    ) -> CloudflareApiResult<CloudflareOriginRuleset> {
        validate_cf_id(zone_id, "invalid_cloudflare_zone_id")?;
        validate_cf_id(ruleset_id, "invalid_cloudflare_ruleset_id")?;
        let body = serde_json::to_value(request)
            .map_err(|_| validation("cloudflare_origin_rule_encoding_failed"))?;
        self.mutation_result(
            Method::POST,
            &format!("zones/{zone_id}/rulesets/{ruleset_id}/rules"),
            Some(&body),
        )
        .await
    }

    async fn patch_origin_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
        request: &CloudflareOriginRuleWrite,
    ) -> CloudflareApiResult<CloudflareOriginRuleset> {
        validate_cf_id(zone_id, "invalid_cloudflare_zone_id")?;
        validate_cf_id(ruleset_id, "invalid_cloudflare_ruleset_id")?;
        validate_cf_id(rule_id, "invalid_cloudflare_rule_id")?;
        let body = serde_json::to_value(request)
            .map_err(|_| validation("cloudflare_origin_rule_encoding_failed"))?;
        self.mutation_result(
            Method::PATCH,
            &format!("zones/{zone_id}/rulesets/{ruleset_id}/rules/{rule_id}"),
            Some(&body),
        )
        .await
    }

    async fn delete_origin_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
    ) -> CloudflareApiResult<CloudflareOriginRuleset> {
        validate_cf_id(zone_id, "invalid_cloudflare_zone_id")?;
        validate_cf_id(ruleset_id, "invalid_cloudflare_ruleset_id")?;
        validate_cf_id(rule_id, "invalid_cloudflare_rule_id")?;
        self.mutation_result(
            Method::DELETE,
            &format!("zones/{zone_id}/rulesets/{ruleset_id}/rules/{rule_id}"),
            None,
        )
        .await
    }

    async fn trace_request(
        &self,
        account_id: &str,
        request: &CloudflareTraceRequest,
    ) -> CloudflareApiResult<CloudflareTraceResult> {
        validate_cf_id(account_id, "invalid_cloudflare_account_id")?;
        request.validate(account_id, None)?;
        let body = serde_json::to_value(request)
            .map_err(|_| validation("cloudflare_trace_encoding_failed"))?;
        self.execute_result(
            &format!("accounts/{account_id}/request-tracer/trace"),
            &body,
        )
        .await
    }
}

/// Account-bound high-level adapter. Planning re-observes complete phase state
/// and mutation methods re-observe the exact fence immediately before the one
/// non-retried provider write.
pub struct CloudflareOriginRulesAdapter {
    center_account_id: CloudResourceId,
    cloudflare_account_id: String,
    api: Arc<dyn CloudflareOriginRulesApi>,
}

impl CloudflareOriginRulesAdapter {
    pub fn new(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn CloudflareOriginRulesApi>,
    ) -> CloudflareApiResult<Self> {
        account
            .validate()
            .map_err(|_| validation("invalid_provider_account"))?;
        if account.provider != CloudProvider::Cloudflare {
            return Err(validation("cloudflare_provider_required"));
        }
        let ProviderAccountScope::Cloudflare { account_id } = account
            .scope
            .as_ref()
            .ok_or_else(|| validation("cloudflare_account_scope_required"))?
        else {
            return Err(validation("cloudflare_account_scope_mismatch"));
        };
        validate_cf_id(account_id, "invalid_cloudflare_account_id")?;
        Ok(Self {
            center_account_id,
            cloudflare_account_id: account_id.clone(),
            api,
        })
    }

    pub async fn observe(
        &self,
        provider_account_id: &CloudResourceId,
        zone_id: &str,
        ownership_proofs: &[OriginRuleOwnershipProof],
    ) -> CloudflareApiResult<Option<OriginRulesObservation>> {
        self.validate_request_scope(provider_account_id, zone_id)?;
        self.api
            .get_origin_ruleset(zone_id)
            .await?
            .map_or(Ok(None), |value| {
                map_ruleset(zone_id, value, ownership_proofs).map(Some)
            })
    }

    /// Creates the phase entrypoint only when it is absent. A conflict or
    /// ambiguous POST outcome is resolved by observation; the POST is never
    /// replayed blindly.
    pub async fn ensure_origin_ruleset(
        &self,
        provider_account_id: &CloudResourceId,
        zone_id: &str,
        ownership_proofs: &[OriginRuleOwnershipProof],
        capability: &OriginRulesCapabilityEvidence,
        now_unix_ms: u64,
    ) -> CloudflareApiResult<OriginRulesObservation> {
        self.validate_request_scope(provider_account_id, zone_id)?;
        validate_capability_authority(
            &self.cloudflare_account_id,
            zone_id,
            capability,
            now_unix_ms,
        )?;
        if capability.current_rules != 0 {
            return Err(conflict("origin_rules_capability_usage_mismatch"));
        }
        if let Some(existing) = self.api.get_origin_ruleset(zone_id).await? {
            return map_ruleset(zone_id, existing, ownership_proofs);
        }
        let request = CloudflareCreateOriginRulesetRequest {
            name: "Edgion Center Origin Rules",
            description: "Origin Rules phase entrypoint managed by Edgion Center",
            kind: "zone",
            phase: ORIGIN_PHASE,
            rules: Vec::new(),
        };
        match self.api.create_origin_ruleset(zone_id, &request).await {
            Ok(created) => map_ruleset(zone_id, created, ownership_proofs)
                .map_err(|_| unknown("cloudflare_origin_ruleset_create_result_invalid")),
            Err(error)
                if matches!(
                    error.category(),
                    ProviderErrorCategory::Conflict | ProviderErrorCategory::UnknownOutcome
                ) =>
            {
                match self.api.get_origin_ruleset(zone_id).await? {
                    Some(observed) => map_ruleset(zone_id, observed, ownership_proofs),
                    None => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn plan_create(
        &self,
        zone_id: &str,
        observation: &OriginRulesObservation,
        desired: DesiredOriginRule,
        capability: &OriginRulesCapabilityEvidence,
        endpoints: &[CloudflareOriginEndpointEvidence],
        tls: Option<&OriginTlsCompatibility>,
        explicit_override: Option<&ExplicitOriginConflictOverride>,
        now_unix_ms: u64,
    ) -> CloudflareApiResult<OriginRuleMutationPlan> {
        desired.validate()?;
        self.validate_plan_evidence(
            zone_id,
            observation,
            &desired,
            capability,
            endpoints,
            tls,
            now_unix_ms,
            true,
        )?;
        if observation
            .rules
            .iter()
            .any(|rule| rule.provider_ref.as_deref() == Some(desired.key.provider_ref().as_str()))
        {
            return Err(conflict("origin_rule_key_already_exists"));
        }
        validate_owned_conflicts(observation, &desired, None)?;
        let placement = plan_priority_placement(observation, desired.priority, None)?;
        let mut conflicts = overlapping_unowned(observation, &desired)?;
        if matches!(
            placement,
            OriginRulePlacementPlan::AppendAfterAcknowledgedUnowned { .. }
        ) {
            conflicts = unowned_rule_ids(observation);
        }
        validate_explicit_override(observation, &conflicts, explicit_override)?;
        let authority = build_mutation_authority(
            &self.cloudflare_account_id,
            zone_id,
            capability,
            endpoints,
            tls,
        )?;
        Ok(OriginRuleMutationPlan {
            desired,
            ruleset_id: observation.ruleset_id.clone(),
            ruleset_revision: observation.ruleset_revision.clone(),
            fence: None,
            acknowledged_unowned_conflicts: conflicts,
            ownership_proofs: ownership_proofs_from_observation(zone_id, observation)?,
            placement,
            authority,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn plan_update(
        &self,
        zone_id: &str,
        observation: &OriginRulesObservation,
        desired: DesiredOriginRule,
        capability: &OriginRulesCapabilityEvidence,
        endpoints: &[CloudflareOriginEndpointEvidence],
        tls: Option<&OriginTlsCompatibility>,
        explicit_override: Option<&ExplicitOriginConflictOverride>,
        now_unix_ms: u64,
    ) -> CloudflareApiResult<OriginRuleMutationPlan> {
        desired.validate()?;
        self.validate_plan_evidence(
            zone_id,
            observation,
            &desired,
            capability,
            endpoints,
            tls,
            now_unix_ms,
            false,
        )?;
        let observed = observation
            .rules
            .iter()
            .find(|rule| {
                matches!(&rule.ownership, OriginRuleOwnership::Owned { key } if key == &desired.key)
            })
            .ok_or_else(|| conflict("owned_origin_rule_not_found"))?;
        validate_owned_conflicts(observation, &desired, Some(&observed.provider_id))?;
        let placement =
            plan_priority_placement(observation, desired.priority, Some(&observed.provider_id))?;
        let mut conflicts = overlapping_unowned(observation, &desired)?;
        if matches!(
            placement,
            OriginRulePlacementPlan::AppendAfterAcknowledgedUnowned { .. }
        ) {
            conflicts = unowned_rule_ids(observation);
        }
        validate_explicit_override(observation, &conflicts, explicit_override)?;
        let authority = build_mutation_authority(
            &self.cloudflare_account_id,
            zone_id,
            capability,
            endpoints,
            tls,
        )?;
        Ok(OriginRuleMutationPlan {
            desired: desired.clone(),
            ruleset_id: observation.ruleset_id.clone(),
            ruleset_revision: observation.ruleset_revision.clone(),
            fence: Some(rule_fence(observation, observed, desired.key)?),
            acknowledged_unowned_conflicts: conflicts,
            ownership_proofs: ownership_proofs_from_observation(zone_id, observation)?,
            placement,
            authority,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn plan_delete(
        &self,
        zone_id: &str,
        observation: &OriginRulesObservation,
        key: &OriginRuleKey,
        test_request: OriginRuleTestRequest,
        provider_default: Option<&ProviderDefaultOriginEvidence>,
        traffic_change_ack: bool,
        capability: &OriginRulesCapabilityEvidence,
        endpoints: &[CloudflareOriginEndpointEvidence],
        now_unix_ms: u64,
    ) -> CloudflareApiResult<OriginRuleDeletePlan> {
        let observed = observation
            .rules
            .iter()
            .find(|rule| matches!(&rule.ownership, OriginRuleOwnership::Owned { key: value } if value == key))
            .ok_or_else(|| conflict("owned_origin_rule_not_found"))?;
        let safety = derive_delete_safety(
            observation,
            key,
            test_request,
            provider_default,
            traffic_change_ack,
        )?;
        validate_capability_authority(
            &self.cloudflare_account_id,
            zone_id,
            capability,
            now_unix_ms,
        )?;
        if capability.current_rules != observation.rules.len() {
            return Err(conflict("origin_rules_capability_usage_mismatch"));
        }
        Ok(OriginRuleDeletePlan {
            fence: rule_fence(observation, observed, key.clone())?,
            safety,
            ownership_proofs: ownership_proofs_from_observation(zone_id, observation)?,
            authority: build_mutation_authority(
                &self.cloudflare_account_id,
                zone_id,
                capability,
                endpoints,
                None,
            )?,
        })
    }

    pub async fn apply_create(
        &self,
        provider_account_id: &CloudResourceId,
        zone_id: &str,
        plan: &OriginRuleMutationPlan,
        now_unix_ms: u64,
    ) -> CloudflareApiResult<OriginRulesObservation> {
        self.validate_request_scope(provider_account_id, zone_id)?;
        validate_mutation_authority(
            &plan.authority,
            &self.cloudflare_account_id,
            zone_id,
            now_unix_ms,
        )?;
        if plan.fence.is_some() {
            return Err(validation("create_origin_rule_has_update_fence"));
        }
        let current = self
            .require_ruleset_revision(zone_id, &plan.ruleset_revision, &plan.ownership_proofs)
            .await?;
        if current.rules.iter().any(|rule| {
            rule.provider_ref.as_deref() == Some(plan.desired.key.provider_ref().as_str())
        }) {
            return Err(conflict("origin_rule_key_already_exists"));
        }
        let write = render_write(
            &plan.desired,
            validate_planned_placement(&current, &plan.placement, plan.desired.priority, None)?,
        )?;
        let result = self
            .api
            .create_origin_rule(zone_id, &plan.ruleset_id, &write)
            .await?;
        let observed = map_owned_mutation_result(
            zone_id,
            result,
            &plan.desired.key,
            &plan.desired.match_rule,
            plan.desired.priority,
            &plan.ownership_proofs,
            None,
        )?;
        if observed.ruleset_id != plan.ruleset_id {
            return Err(unknown("cloudflare_origin_rule_create_ruleset_mismatch"));
        }
        ensure_desired_postcondition(&observed, &plan.desired)?;
        ensure_unowned_relative_order(&current, &observed)?;
        Ok(observed)
    }

    pub async fn apply_update(
        &self,
        provider_account_id: &CloudResourceId,
        zone_id: &str,
        plan: &OriginRuleMutationPlan,
        now_unix_ms: u64,
    ) -> CloudflareApiResult<OriginRulesObservation> {
        self.validate_request_scope(provider_account_id, zone_id)?;
        validate_mutation_authority(
            &plan.authority,
            &self.cloudflare_account_id,
            zone_id,
            now_unix_ms,
        )?;
        let fence = plan
            .fence
            .as_ref()
            .ok_or_else(|| validation("update_origin_rule_fence_required"))?;
        let current = self
            .require_fence(zone_id, fence, &plan.ownership_proofs)
            .await?;
        // Omitting `position` preserves the existing rule position. The
        // adapter never uses an update as an implicit reorder operation.
        let write = render_write(
            &plan.desired,
            validate_planned_placement(
                &current,
                &plan.placement,
                plan.desired.priority,
                Some(&fence.rule_id),
            )?,
        )?;
        let result = self
            .api
            .patch_origin_rule(zone_id, &fence.ruleset_id, &fence.rule_id, &write)
            .await?;
        let observed = map_owned_mutation_result(
            zone_id,
            result,
            &plan.desired.key,
            &plan.desired.match_rule,
            plan.desired.priority,
            &plan.ownership_proofs,
            Some(&fence.rule_id),
        )?;
        if observed.ruleset_id != fence.ruleset_id {
            return Err(unknown("cloudflare_origin_rule_update_ruleset_mismatch"));
        }
        ensure_desired_postcondition(&observed, &plan.desired)?;
        ensure_unowned_relative_order(&current, &observed)?;
        Ok(observed)
    }

    pub async fn apply_delete(
        &self,
        provider_account_id: &CloudResourceId,
        zone_id: &str,
        plan: &OriginRuleDeletePlan,
        now_unix_ms: u64,
    ) -> CloudflareApiResult<OriginRulesObservation> {
        self.validate_request_scope(provider_account_id, zone_id)?;
        validate_mutation_authority(
            &plan.authority,
            &self.cloudflare_account_id,
            zone_id,
            now_unix_ms,
        )?;
        let current = self
            .require_fence(zone_id, &plan.fence, &plan.ownership_proofs)
            .await?;
        let provider_default = match &plan.safety.effect {
            OriginRuleDeleteEffect::ProviderDefault {
                provider_default_revision,
                effect,
            } => Some(ProviderDefaultOriginEvidence {
                ruleset_revision: plan.safety.ruleset_revision.clone(),
                test_request: plan.safety.test_request.clone(),
                provider_default_revision: provider_default_revision.clone(),
                effect: effect.clone(),
            }),
            OriginRuleDeleteEffect::MayUseProviderDefault {
                provider_default_revision,
                provider_default_effect,
                ..
            } => Some(ProviderDefaultOriginEvidence {
                ruleset_revision: plan.safety.ruleset_revision.clone(),
                test_request: plan.safety.test_request.clone(),
                provider_default_revision: provider_default_revision.clone(),
                effect: provider_default_effect.clone(),
            }),
            OriginRuleDeleteEffect::ExplicitOverrides { .. } => None,
        };
        let recomputed = derive_delete_safety(
            &current,
            &plan.fence.ownership_key,
            plan.safety.test_request.clone(),
            provider_default.as_ref(),
            plan.safety.traffic_change_ack,
        )?;
        if recomputed != plan.safety {
            return Err(conflict("origin_rule_delete_effect_changed"));
        }
        let result = self
            .api
            .delete_origin_rule(zone_id, &plan.fence.ruleset_id, &plan.fence.rule_id)
            .await?;
        let remaining_proofs = ownership_proofs_from_observation(zone_id, &current)?
            .into_iter()
            .filter(|proof| proof.rule_id != plan.fence.rule_id)
            .collect::<Vec<_>>();
        let observed = map_ruleset(zone_id, result, &remaining_proofs)
            .map_err(|_| unknown("cloudflare_origin_rule_delete_result_invalid"))?;
        if observed.ruleset_id != plan.fence.ruleset_id
            || observed
                .rules
                .iter()
                .any(|rule| rule.provider_id == plan.fence.rule_id)
        {
            return Err(unknown("cloudflare_origin_rule_delete_result_mismatch"));
        }
        ensure_unowned_relative_order(&current, &observed)?;
        Ok(observed)
    }

    pub async fn trace(
        &self,
        provider_account_id: &CloudResourceId,
        zone_id: &str,
        request: &CloudflareTraceRequest,
        capability: &OriginRulesCapabilityEvidence,
        now_unix_ms: u64,
    ) -> CloudflareApiResult<CloudflareOriginTraceOutcome> {
        if provider_account_id != &self.center_account_id {
            return Err(validation("cloudflare_account_scope_mismatch"));
        }
        if capability.account_id != self.cloudflare_account_id
            || capability.zone_id != zone_id
            || !capability.trace_execute
            || capability.credential_revision.is_empty()
            || capability.observed_at_unix_ms > now_unix_ms
            || now_unix_ms >= capability.valid_until_unix_ms
        {
            return Err(conflict("cloudflare_trace_execute_capability_required"));
        }
        request.validate(&self.cloudflare_account_id, Some(now_unix_ms))?;
        let raw = self
            .api
            .trace_request(&self.cloudflare_account_id, request)
            .await?;
        parse_origin_trace(raw)
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_plan_evidence(
        &self,
        zone_id: &str,
        observation: &OriginRulesObservation,
        desired: &DesiredOriginRule,
        capability: &OriginRulesCapabilityEvidence,
        endpoints: &[CloudflareOriginEndpointEvidence],
        tls: Option<&OriginTlsCompatibility>,
        now_unix_ms: u64,
        creating: bool,
    ) -> CloudflareApiResult<()> {
        validate_cf_id(zone_id, "invalid_cloudflare_zone_id")?;
        if capability.account_id != self.cloudflare_account_id
            || capability.zone_id != zone_id
            || capability.credential_revision.is_empty()
            || capability.observed_at_unix_ms > now_unix_ms
            || now_unix_ms >= capability.valid_until_unix_ms
            || capability.current_rules != observation.rules.len()
            || capability.max_rules == 0
            || capability.max_rules > MAX_RULES
            || (creating && capability.current_rules >= capability.max_rules)
        {
            return Err(conflict(
                "origin_rules_capability_scope_freshness_or_quota_mismatch",
            ));
        }
        if (desired.overrides.host_header.is_some() && !capability.host_override)
            || (desired.overrides.sni.is_some() && !capability.sni_override)
            || (desired.overrides.dns_target.is_some() && !capability.dns_override)
            || (desired.overrides.port.is_some() && !capability.port_override)
        {
            return Err(conflict("origin_rules_capability_not_entitled"));
        }
        let endpoint_for = |hostname: &AbsoluteDnsName| {
            endpoints.iter().find(|evidence| {
                evidence.account_id == self.cloudflare_account_id
                    && evidence.hostname == *hostname
                    && !evidence.endpoint_id.is_empty()
                    && !evidence.endpoint_revision.is_empty()
                    && evidence.observed_at_unix_ms <= now_unix_ms
                    && now_unix_ms < evidence.valid_until_unix_ms
            })
        };
        if let Some(OriginDnsTarget::Hostname(hostname)) = desired.overrides.dns_target.as_ref() {
            let endpoint = endpoint_for(hostname)
                .ok_or_else(|| conflict("origin_dns_target_same_account_evidence_required"))?;
            if capability.proxied_origin_required && !endpoint.proxied {
                return Err(conflict("origin_dns_target_must_be_proxied"));
            }
        }
        if desired.overrides.host_header.is_some() || desired.overrides.sni.is_some() {
            let effective_sni = desired
                .overrides
                .effective_sni(&desired.match_rule.hostname);
            let endpoint = endpoint_for(&effective_sni)
                .ok_or_else(|| conflict("origin_sni_same_account_evidence_required"))?;
            tls.ok_or_else(|| conflict("origin_tls_compatibility_required"))?
                .validate_for(&self.cloudflare_account_id, endpoint, desired, now_unix_ms)?;
        }
        Ok(())
    }

    async fn require_ruleset_revision(
        &self,
        zone_id: &str,
        revision: &str,
        ownership_proofs: &[OriginRuleOwnershipProof],
    ) -> CloudflareApiResult<OriginRulesObservation> {
        let current = self
            .api
            .get_origin_ruleset(zone_id)
            .await?
            .ok_or_else(|| conflict("cloudflare_origin_ruleset_missing"))?;
        let current = map_ruleset(zone_id, current, ownership_proofs)?;
        if current.ruleset_revision != revision {
            return Err(conflict("cloudflare_origin_ruleset_revision_changed"));
        }
        Ok(current)
    }

    async fn require_fence(
        &self,
        zone_id: &str,
        fence: &OriginRuleFence,
        ownership_proofs: &[OriginRuleOwnershipProof],
    ) -> CloudflareApiResult<OriginRulesObservation> {
        let raw = self
            .api
            .get_origin_ruleset(zone_id)
            .await?
            .ok_or_else(|| conflict("cloudflare_origin_ruleset_missing"))?;
        let current = map_ruleset(zone_id, raw, ownership_proofs)?;
        if current.ruleset_revision != fence.ruleset_revision {
            return Err(conflict("cloudflare_origin_ruleset_revision_changed"));
        }
        if current.ruleset_id != fence.ruleset_id {
            return Err(conflict("cloudflare_origin_ruleset_identity_changed"));
        }
        let rule = current
            .rules
            .iter()
            .find(|rule| rule.provider_id == fence.rule_id)
            .ok_or_else(|| conflict("cloudflare_origin_rule_disappeared"))?;
        if rule.provider_version != fence.rule_version
            || rule.revision != fence.rule_revision
            || rule.ownership
                != (OriginRuleOwnership::Owned {
                    key: fence.ownership_key.clone(),
                })
        {
            return Err(conflict("cloudflare_origin_rule_fence_changed"));
        }
        Ok(current)
    }

    fn validate_request_scope(
        &self,
        provider_account_id: &CloudResourceId,
        zone_id: &str,
    ) -> CloudflareApiResult<()> {
        if provider_account_id != &self.center_account_id {
            return Err(validation("cloudflare_account_scope_mismatch"));
        }
        validate_cf_id(zone_id, "invalid_cloudflare_zone_id")
    }
}

fn map_ruleset(
    zone_id: &str,
    value: CloudflareOriginRuleset,
    ownership_proofs: &[OriginRuleOwnershipProof],
) -> CloudflareApiResult<OriginRulesObservation> {
    validate_cf_id(&value.id, "invalid_cloudflare_ruleset_id")?;
    if value.phase != ORIGIN_PHASE || !matches!(value.kind.as_str(), "zone" | "root") {
        return Err(validation("invalid_cloudflare_origin_ruleset"));
    }
    if value.version.is_empty() || value.rules.len() > MAX_RULES {
        return Err(validation("invalid_cloudflare_origin_ruleset_version"));
    }
    let mut seen_ids = BTreeSet::new();
    let mut seen_owned_keys = BTreeSet::new();
    let mut seen_owned_priorities = BTreeSet::new();
    let mut rules = Vec::with_capacity(value.rules.len());
    for (position, rule) in value.rules.iter().enumerate() {
        validate_cf_id(&rule.id, "invalid_cloudflare_rule_id")?;
        if !seen_ids.insert(rule.id.clone())
            || rule.version.is_empty()
            || rule.action != "route"
            || rule.description.len() > MAX_DESCRIPTION_BYTES
        {
            return Err(validation("invalid_cloudflare_origin_rule"));
        }
        let overrides = map_parameters(&rule.action_parameters)?;
        let revision = hash_json(rule, "cloudflare_origin_rule_revision_failed")?;
        let matching_proofs = ownership_proofs
            .iter()
            .filter(|proof| {
                proof.zone_id == zone_id
                    && proof.ruleset_id == value.id
                    && proof.rule_id == rule.id
                    && proof.rule_revision == revision
            })
            .collect::<Vec<_>>();
        if matching_proofs.len() > 1 {
            return Err(conflict("ambiguous_origin_rule_ownership_proof"));
        }
        let typed_match = if let Some(proof) = matching_proofs.first() {
            proof.match_rule.validate()?;
            if proof.priority == 0
                || rule.rule_ref.as_deref() != Some(proof.key.provider_ref().as_str())
                || proof.match_rule.expression()? != rule.expression
            {
                return Err(conflict("origin_rule_ownership_proof_mismatch"));
            }
            Some(proof.match_rule.clone())
        } else {
            None
        };
        let ownership = matching_proofs
            .first()
            .map_or(OriginRuleOwnership::Unowned, |proof| {
                OriginRuleOwnership::Owned {
                    key: proof.key.clone(),
                }
            });
        if let OriginRuleOwnership::Owned { key } = &ownership {
            let priority = matching_proofs[0].priority;
            if !seen_owned_keys.insert(key.clone()) || !seen_owned_priorities.insert(priority) {
                return Err(conflict("duplicate_owned_origin_rule_key"));
            }
        }
        let owned_priority = matching_proofs.first().map(|proof| proof.priority);
        rules.push(ObservedOriginRule {
            provider_id: rule.id.clone(),
            provider_version: rule.version.clone(),
            provider_ref: rule.rule_ref.clone(),
            ownership,
            typed_match,
            owned_priority,
            expression: rule.expression.clone(),
            overrides,
            description: rule.description.clone(),
            enabled: rule.enabled,
            position,
            revision,
        });
    }
    let ruleset_revision = hash_json(&value, "cloudflare_origin_ruleset_revision_failed")?;
    Ok(OriginRulesObservation {
        ruleset_id: value.id,
        ruleset_version: value.version,
        ruleset_revision,
        rules,
    })
}

fn map_owned_mutation_result(
    zone_id: &str,
    raw: CloudflareOriginRuleset,
    key: &OriginRuleKey,
    match_rule: &OriginRuleMatch,
    priority: u32,
    prior_proofs: &[OriginRuleOwnershipProof],
    expected_rule_id: Option<&str>,
) -> CloudflareApiResult<OriginRulesObservation> {
    let unowned = map_ruleset(zone_id, raw.clone(), &[])
        .map_err(|_| unknown("cloudflare_origin_rule_mutation_result_invalid"))?;
    let matches = unowned
        .rules
        .iter()
        .filter(|rule| {
            rule.provider_ref.as_deref() == Some(key.provider_ref().as_str())
                && expected_rule_id.is_none_or(|id| rule.provider_id == id)
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(unknown("cloudflare_origin_rule_mutation_result_mismatch"));
    }
    let proof = OriginRuleOwnershipProof {
        zone_id: zone_id.to_string(),
        ruleset_id: unowned.ruleset_id.clone(),
        rule_id: matches[0].provider_id.clone(),
        rule_revision: matches[0].revision.clone(),
        key: key.clone(),
        match_rule: match_rule.clone(),
        priority,
    };
    let mut proofs = prior_proofs
        .iter()
        .filter(|existing| existing.rule_id != proof.rule_id)
        .cloned()
        .collect::<Vec<_>>();
    proofs.push(proof);
    map_ruleset(zone_id, raw, &proofs)
        .map_err(|_| unknown("cloudflare_origin_rule_mutation_result_invalid"))
}

fn ensure_unowned_relative_order(
    before: &OriginRulesObservation,
    after: &OriginRulesObservation,
) -> CloudflareApiResult<()> {
    let expected = before
        .rules
        .iter()
        .filter(|rule| rule.ownership == OriginRuleOwnership::Unowned)
        .map(|rule| (rule.provider_id.as_str(), rule.revision.as_str()))
        .collect::<Vec<_>>();
    let expected_set = expected.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>();
    let actual = after
        .rules
        .iter()
        .filter(|rule| expected_set.contains(rule.provider_id.as_str()))
        .map(|rule| (rule.provider_id.as_str(), rule.revision.as_str()))
        .collect::<Vec<_>>();
    if actual != expected {
        return Err(unknown("cloudflare_unowned_origin_rule_order_changed"));
    }
    Ok(())
}

fn ensure_desired_postcondition(
    observation: &OriginRulesObservation,
    desired: &DesiredOriginRule,
) -> CloudflareApiResult<()> {
    let matches = observation
        .rules
        .iter()
        .filter(|rule| {
            rule.ownership
                == OriginRuleOwnership::Owned {
                    key: desired.key.clone(),
                }
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(unknown("cloudflare_origin_rule_postcondition_mismatch"));
    }
    let rule = matches[0];
    if rule.expression != desired.match_rule.expression()?
        || rule.overrides != desired.overrides
        || rule.description != desired.description
        || rule.enabled != desired.enabled
        || rule.owned_priority != Some(desired.priority)
    {
        return Err(unknown("cloudflare_origin_rule_postcondition_mismatch"));
    }
    priority_position(observation, desired.priority, Some(&rule.provider_id))
        .map_err(|_| unknown("cloudflare_origin_rule_priority_postcondition_mismatch"))?;
    Ok(())
}

fn map_parameters(value: &CloudflareRouteParameters) -> CloudflareApiResult<OriginRuleOverrides> {
    let dns_target = value
        .origin
        .as_ref()
        .and_then(|origin| origin.host.as_deref())
        .map(|host| {
            AbsoluteDnsName::new(host)
                .map(OriginDnsTarget::Hostname)
                .map_err(|_| validation("invalid_cloudflare_origin_host"))
        })
        .transpose()?;
    let result = OriginRuleOverrides {
        dns_target,
        host_header: value
            .host_header
            .as_deref()
            .map(AbsoluteDnsName::new)
            .transpose()
            .map_err(|_| validation("invalid_cloudflare_origin_host_header"))?,
        sni: value
            .sni
            .as_ref()
            .map(|sni| AbsoluteDnsName::new(&sni.value))
            .transpose()
            .map_err(|_| validation("invalid_cloudflare_origin_sni"))?,
        port: value.origin.as_ref().and_then(|origin| origin.port),
    };
    result.validate()?;
    Ok(result)
}

fn render_write(
    desired: &DesiredOriginRule,
    position: Option<CloudflareRulePosition>,
) -> CloudflareApiResult<CloudflareOriginRuleWrite> {
    desired.validate()?;
    let origin = if desired.overrides.dns_target.is_some() || desired.overrides.port.is_some() {
        Some(CloudflareRouteOrigin {
            host: desired
                .overrides
                .dns_target
                .as_ref()
                .map(OriginDnsTarget::render),
            port: desired.overrides.port,
        })
    } else {
        None
    };
    Ok(CloudflareOriginRuleWrite {
        action: "route",
        action_parameters: CloudflareRouteParameters {
            host_header: desired
                .overrides
                .host_header
                .as_ref()
                .map(|value| value.as_str().to_string()),
            origin,
            sni: desired
                .overrides
                .sni
                .as_ref()
                .map(|value| CloudflareRouteSni {
                    value: value.as_str().to_string(),
                }),
        },
        expression: desired.match_rule.expression()?,
        description: desired.description.clone(),
        enabled: desired.enabled,
        rule_ref: desired.key.provider_ref(),
        position,
    })
}

pub fn validate_desired_origin_rules(rules: &[DesiredOriginRule]) -> CloudflareApiResult<()> {
    if rules.len() > MAX_RULES {
        return Err(validation("cloudflare_origin_rule_quota_exceeded"));
    }
    let mut keys = BTreeSet::new();
    let mut priorities = BTreeSet::new();
    for rule in rules {
        rule.validate()?;
        if !keys.insert(rule.key.clone()) || !priorities.insert(rule.priority) {
            return Err(conflict("conflicting_owned_origin_rules"));
        }
    }
    let mut ordered = rules.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|rule| rule.priority);
    for (index, earlier) in ordered.iter().enumerate() {
        for later in &ordered[index + 1..] {
            if matches_overlap(&earlier.match_rule, &later.match_rule)
                && conflicting_override_fields(&earlier.overrides, &later.overrides)
                && !later.explicitly_overrides_keys.contains(&earlier.key)
            {
                return Err(conflict("conflicting_owned_origin_rules"));
            }
        }
    }
    Ok(())
}

fn validate_owned_conflicts(
    observation: &OriginRulesObservation,
    desired: &DesiredOriginRule,
    skip_provider_id: Option<&str>,
) -> CloudflareApiResult<()> {
    for observed in &observation.rules {
        if skip_provider_id == Some(observed.provider_id.as_str()) {
            continue;
        }
        let OriginRuleOwnership::Owned { key } = &observed.ownership else {
            continue;
        };
        let typed_match = observed
            .typed_match
            .as_ref()
            .ok_or_else(|| conflict("owned_origin_rule_match_proof_missing"))?;
        if matches_overlap(typed_match, &desired.match_rule)
            && conflicting_override_fields(&observed.overrides, &desired.overrides)
            && !desired.explicitly_overrides_keys.contains(key)
        {
            return Err(conflict("conflicting_owned_origin_rules"));
        }
    }
    Ok(())
}

fn conflicting_override_fields(left: &OriginRuleOverrides, right: &OriginRuleOverrides) -> bool {
    (left.dns_target.is_some() && right.dns_target.is_some() && left.dns_target != right.dns_target)
        || (left.host_header.is_some()
            && right.host_header.is_some()
            && left.host_header != right.host_header)
        || (left.sni.is_some() && right.sni.is_some() && left.sni != right.sni)
        || (left.port.is_some() && right.port.is_some() && left.port != right.port)
}

fn overlapping_unowned(
    observation: &OriginRulesObservation,
    desired: &DesiredOriginRule,
) -> CloudflareApiResult<BTreeSet<String>> {
    let fields = override_fields(&desired.overrides);
    Ok(observation
        .rules
        .iter()
        .filter(|rule| rule.ownership == OriginRuleOwnership::Unowned)
        // Unowned expressions are opaque. Unless a separate typed proof can
        // establish disjointness, every overlapping override field is a
        // potential conflict.
        .filter(|rule| !fields.is_disjoint(&override_fields(&rule.overrides)))
        .map(|rule| rule.provider_id.clone())
        .collect())
}

fn validate_explicit_override(
    observation: &OriginRulesObservation,
    conflicts: &BTreeSet<String>,
    explicit: Option<&ExplicitOriginConflictOverride>,
) -> CloudflareApiResult<()> {
    if conflicts.is_empty() {
        return Ok(());
    }
    let explicit = explicit.ok_or_else(|| conflict("unowned_origin_rule_conflict"))?;
    if explicit.ruleset_revision != observation.ruleset_revision
        || conflicts != &explicit.accepted_unowned_rule_ids
    {
        return Err(conflict("unowned_origin_rule_conflict_not_acknowledged"));
    }
    Ok(())
}

fn rule_fence(
    observation: &OriginRulesObservation,
    rule: &ObservedOriginRule,
    key: OriginRuleKey,
) -> CloudflareApiResult<OriginRuleFence> {
    let match_rule = rule
        .typed_match
        .clone()
        .ok_or_else(|| conflict("owned_origin_rule_match_proof_missing"))?;
    Ok(OriginRuleFence {
        ruleset_id: observation.ruleset_id.clone(),
        ruleset_version: observation.ruleset_version.clone(),
        ruleset_revision: observation.ruleset_revision.clone(),
        rule_id: rule.provider_id.clone(),
        rule_version: rule.provider_version.clone(),
        rule_revision: rule.revision.clone(),
        ownership_key: key,
        match_rule,
        priority: rule
            .owned_priority
            .ok_or_else(|| conflict("owned_origin_rule_priority_proof_missing"))?,
    })
}

fn priority_position(
    observation: &OriginRulesObservation,
    desired_priority: u32,
    current_rule_id: Option<&str>,
) -> CloudflareApiResult<Option<CloudflareRulePosition>> {
    let owned = observation
        .rules
        .iter()
        .filter_map(|rule| rule.owned_priority.map(|priority| (priority, rule)))
        .collect::<Vec<_>>();
    if owned.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err(conflict("owned_origin_rule_provider_order_drift"));
    }
    if owned.iter().any(|(priority, rule)| {
        *priority == desired_priority && Some(rule.provider_id.as_str()) != current_rule_id
    }) {
        return Err(conflict("duplicate_owned_origin_rule_priority"));
    }
    if let Some(current_id) = current_rule_id {
        let current = owned
            .iter()
            .find(|(_, rule)| rule.provider_id == current_id)
            .ok_or_else(|| conflict("owned_origin_rule_priority_anchor_missing"))?;
        if current.0 == desired_priority {
            return Ok(None);
        }
    }
    let anchors = owned
        .into_iter()
        .filter(|(_, rule)| Some(rule.provider_id.as_str()) != current_rule_id)
        .collect::<Vec<_>>();
    if anchors.is_empty() {
        if observation
            .rules
            .iter()
            .any(|rule| Some(rule.provider_id.as_str()) != current_rule_id)
        {
            return Err(conflict("owned_origin_rule_priority_anchor_missing"));
        }
        return Ok(None);
    }
    if let Some((_, next)) = anchors
        .iter()
        .find(|(priority, _)| *priority > desired_priority)
    {
        return Ok(Some(CloudflareRulePosition::Before {
            before: next.provider_id.clone(),
        }));
    }
    Ok(Some(CloudflareRulePosition::After {
        after: anchors
            .last()
            .expect("non-empty owned anchors")
            .1
            .provider_id
            .clone(),
    }))
}

fn plan_priority_placement(
    observation: &OriginRulesObservation,
    desired_priority: u32,
    current_rule_id: Option<&str>,
) -> CloudflareApiResult<OriginRulePlacementPlan> {
    let owned = observation
        .rules
        .iter()
        .filter_map(|rule| rule.owned_priority.map(|priority| (priority, rule)))
        .collect::<Vec<_>>();
    if owned.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err(conflict("owned_origin_rule_provider_order_drift"));
    }
    if owned.iter().any(|(priority, rule)| {
        *priority == desired_priority && Some(rule.provider_id.as_str()) != current_rule_id
    }) {
        return Err(conflict("duplicate_owned_origin_rule_priority"));
    }
    if let Some(current_id) = current_rule_id {
        let current = owned
            .iter()
            .find(|(_, rule)| rule.provider_id == current_id)
            .ok_or_else(|| conflict("owned_origin_rule_priority_anchor_missing"))?;
        if current.0 == desired_priority {
            return Ok(OriginRulePlacementPlan::Preserve);
        }
    }
    let anchors = owned
        .iter()
        .filter(|(_, rule)| Some(rule.provider_id.as_str()) != current_rule_id)
        .collect::<Vec<_>>();
    if let Some((_, next)) = anchors
        .iter()
        .find(|(priority, _)| *priority > desired_priority)
    {
        return Ok(OriginRulePlacementPlan::BeforeOwned {
            rule_id: next.provider_id.clone(),
            rule_revision: next.revision.clone(),
        });
    }
    if let Some((_, previous)) = anchors.last() {
        return Ok(OriginRulePlacementPlan::AfterOwned {
            rule_id: previous.provider_id.clone(),
            rule_revision: previous.revision.clone(),
        });
    }
    let remaining = observation
        .rules
        .iter()
        .filter(|rule| Some(rule.provider_id.as_str()) != current_rule_id)
        .collect::<Vec<_>>();
    if let Some(last) = remaining.last() {
        Ok(OriginRulePlacementPlan::AppendAfterAcknowledgedUnowned {
            rule_id: last.provider_id.clone(),
            rule_revision: last.revision.clone(),
        })
    } else {
        Ok(OriginRulePlacementPlan::First)
    }
}

fn validate_planned_placement(
    observation: &OriginRulesObservation,
    planned: &OriginRulePlacementPlan,
    desired_priority: u32,
    current_rule_id: Option<&str>,
) -> CloudflareApiResult<Option<CloudflareRulePosition>> {
    let current = plan_priority_placement(observation, desired_priority, current_rule_id)?;
    if &current != planned {
        return Err(conflict("origin_rule_placement_anchor_changed"));
    }
    Ok(match current {
        OriginRulePlacementPlan::Preserve | OriginRulePlacementPlan::First => None,
        OriginRulePlacementPlan::BeforeOwned { rule_id, .. } => {
            Some(CloudflareRulePosition::Before { before: rule_id })
        }
        OriginRulePlacementPlan::AfterOwned { rule_id, .. }
        | OriginRulePlacementPlan::AppendAfterAcknowledgedUnowned { rule_id, .. } => {
            Some(CloudflareRulePosition::After { after: rule_id })
        }
    })
}

fn unowned_rule_ids(observation: &OriginRulesObservation) -> BTreeSet<String> {
    observation
        .rules
        .iter()
        .filter(|rule| rule.ownership == OriginRuleOwnership::Unowned)
        .map(|rule| rule.provider_id.clone())
        .collect()
}

fn validate_capability_authority(
    account_id: &str,
    zone_id: &str,
    capability: &OriginRulesCapabilityEvidence,
    now_unix_ms: u64,
) -> CloudflareApiResult<()> {
    if capability.account_id != account_id
        || capability.zone_id != zone_id
        || capability.credential_revision.is_empty()
        || capability.observed_at_unix_ms > now_unix_ms
        || now_unix_ms >= capability.valid_until_unix_ms
    {
        return Err(conflict("origin_rules_capability_authority_mismatch"));
    }
    Ok(())
}

fn build_mutation_authority(
    account_id: &str,
    zone_id: &str,
    capability: &OriginRulesCapabilityEvidence,
    endpoints: &[CloudflareOriginEndpointEvidence],
    tls: Option<&OriginTlsCompatibility>,
) -> CloudflareApiResult<OriginMutationAuthority> {
    let mut valid_until = capability.valid_until_unix_ms;
    let mut endpoint_revisions = BTreeMap::new();
    for endpoint in endpoints {
        if endpoint.account_id != account_id
            || endpoint.endpoint_id.is_empty()
            || endpoint.endpoint_revision.is_empty()
            || endpoint.valid_until_unix_ms == 0
            || endpoint_revisions
                .insert(
                    endpoint.endpoint_id.clone(),
                    endpoint.endpoint_revision.clone(),
                )
                .is_some()
        {
            return Err(conflict("origin_endpoint_authority_mismatch"));
        }
        valid_until = valid_until.min(endpoint.valid_until_unix_ms);
    }
    if let Some(tls) = tls {
        valid_until = valid_until.min(tls.valid_until_unix_ms);
    }
    Ok(OriginMutationAuthority {
        account_id: account_id.to_string(),
        zone_id: zone_id.to_string(),
        credential_revision: capability.credential_revision.clone(),
        endpoint_revisions,
        valid_until_unix_ms: valid_until,
    })
}

fn validate_mutation_authority(
    authority: &OriginMutationAuthority,
    account_id: &str,
    zone_id: &str,
    now_unix_ms: u64,
) -> CloudflareApiResult<()> {
    if authority.account_id != account_id
        || authority.zone_id != zone_id
        || authority.credential_revision.is_empty()
        || now_unix_ms >= authority.valid_until_unix_ms
    {
        return Err(conflict("origin_mutation_authority_expired_or_mismatched"));
    }
    Ok(())
}

fn ownership_proofs_from_observation(
    zone_id: &str,
    observation: &OriginRulesObservation,
) -> CloudflareApiResult<Vec<OriginRuleOwnershipProof>> {
    observation
        .rules
        .iter()
        .filter_map(|rule| {
            let OriginRuleOwnership::Owned { key } = &rule.ownership else {
                return None;
            };
            Some((|| {
                Ok(OriginRuleOwnershipProof {
                    zone_id: zone_id.to_string(),
                    ruleset_id: observation.ruleset_id.clone(),
                    rule_id: rule.provider_id.clone(),
                    rule_revision: rule.revision.clone(),
                    key: key.clone(),
                    match_rule: rule
                        .typed_match
                        .clone()
                        .ok_or_else(|| conflict("owned_origin_rule_match_proof_missing"))?,
                    priority: rule
                        .owned_priority
                        .ok_or_else(|| conflict("owned_origin_rule_priority_proof_missing"))?,
                })
            })())
        })
        .collect()
}

fn override_fields(value: &OriginRuleOverrides) -> BTreeSet<&'static str> {
    let mut result = BTreeSet::new();
    if value.dns_target.is_some() {
        result.insert("dns_target");
    }
    if value.host_header.is_some() {
        result.insert("host_header");
    }
    if value.sni.is_some() {
        result.insert("sni");
    }
    if value.port.is_some() {
        result.insert("port");
    }
    result
}

fn matches_overlap(left: &OriginRuleMatch, right: &OriginRuleMatch) -> bool {
    if left.hostname != right.hostname {
        return false;
    }
    match (left.path.as_ref(), right.path.as_ref()) {
        (None, _) | (_, None) => true,
        (Some(OriginPathMatch::Exact(left)), Some(OriginPathMatch::Exact(right))) => left == right,
        (Some(OriginPathMatch::Exact(exact)), Some(OriginPathMatch::Prefix(prefix)))
        | (Some(OriginPathMatch::Prefix(prefix)), Some(OriginPathMatch::Exact(exact))) => {
            exact.starts_with(prefix)
        }
        (Some(OriginPathMatch::Prefix(left)), Some(OriginPathMatch::Prefix(right))) => {
            left.starts_with(right) || right.starts_with(left)
        }
    }
}

fn certificate_name_covers(certificate_name: &str, hostname: &str) -> bool {
    if certificate_name == hostname {
        return true;
    }
    let Some(suffix) = certificate_name.strip_prefix("*.") else {
        return false;
    };
    hostname
        .strip_suffix(suffix)
        .is_some_and(|prefix| prefix.ends_with('.') && !prefix[..prefix.len() - 1].contains('.'))
}

fn validate_certificate_name(value: &str) -> CloudflareApiResult<()> {
    if let Some(suffix) = value.strip_prefix("*.") {
        AbsoluteDnsName::new(suffix).map_err(|_| validation("invalid_origin_certificate_name"))?;
    } else {
        AbsoluteDnsName::new(value).map_err(|_| validation("invalid_origin_certificate_name"))?;
    }
    Ok(())
}

fn rules_literal(value: &str) -> CloudflareApiResult<String> {
    serde_json::to_string(value).map_err(|_| validation("origin_rule_expression_render_failed"))
}

fn hash_json<T: Serialize>(value: &T, code: &str) -> CloudflareApiResult<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| validation(code))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn validate_cf_id(value: &str, code: &str) -> CloudflareApiResult<()> {
    if value.len() == 32
        && value
            .bytes()
            .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
    {
        Ok(())
    } else {
        Err(validation(code))
    }
}

fn default_true() -> bool {
    true
}

fn validation(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Validation, code)
}

fn conflict(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Conflict, code)
}

fn unknown(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::UnknownOutcome, code)
}

fn provider_error(category: ProviderErrorCategory, code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        code,
        "Cloudflare Origin Rules adapter rejected the request",
        None,
        None,
    )
    .expect("static normalized provider error")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> AbsoluteDnsName {
        AbsoluteDnsName::new(value).unwrap()
    }

    fn desired(key: &str, priority: u32, path: OriginPathMatch) -> DesiredOriginRule {
        DesiredOriginRule {
            key: OriginRuleKey::new(key).unwrap(),
            priority,
            match_rule: OriginRuleMatch {
                hostname: name("www.example.com"),
                path: Some(path),
            },
            overrides: OriginRuleOverrides {
                dns_target: Some(OriginDnsTarget::Hostname(name("origin.example.net"))),
                host_header: None,
                sni: None,
                port: Some(8443),
            },
            description: "route to application origin".to_string(),
            enabled: true,
            explicitly_overrides_keys: BTreeSet::new(),
        }
    }

    #[test]
    fn restricted_match_renders_without_expression_injection() {
        let value = desired(
            "app",
            10,
            OriginPathMatch::Prefix("/api/\" and true".to_string()),
        );
        assert_eq!(
            value.match_rule.expression().unwrap(),
            "(http.host eq \"www.example.com\" and starts_with(http.request.uri.path, \"/api/\\\" and true\"))"
        );
    }

    #[test]
    fn local_preview_applies_last_matching_field_override() {
        let first = desired("first", 10, OriginPathMatch::Prefix("/api".to_string()));
        let mut second = desired("second", 20, OriginPathMatch::Exact("/api/v1".to_string()));
        second.overrides = OriginRuleOverrides {
            dns_target: None,
            host_header: None,
            sni: None,
            port: Some(9443),
        };
        second
            .explicitly_overrides_keys
            .insert(OriginRuleKey::new("first").unwrap());
        let preview = preview_owned_origin_rules(
            &[second, first],
            &OriginRuleTestRequest {
                hostname: name("www.example.com"),
                path: "/api/v1".to_string(),
            },
        )
        .unwrap();
        assert!(preview.incomplete);
        assert_eq!(
            preview.matching_owned_rules,
            vec![
                OriginRuleKey::new("first").unwrap(),
                OriginRuleKey::new("second").unwrap()
            ]
        );
        let route = preview.final_overrides.unwrap();
        assert_eq!(route.port, Some(9443));
        assert_eq!(
            route.dns_target,
            Some(OriginDnsTarget::Hostname(name("origin.example.net")))
        );
    }

    #[test]
    fn tls_check_accounts_for_implicit_host_header_sni() {
        let mut rule = desired("app", 10, OriginPathMatch::Prefix("/".to_string()));
        rule.overrides.host_header = Some(name("backend.example.net"));
        let endpoint = CloudflareOriginEndpointEvidence {
            account_id: "d".repeat(32),
            endpoint_id: "endpoint-1".to_string(),
            endpoint_revision: "endpoint:1".to_string(),
            hostname: name("backend.example.net"),
            proxied: true,
            observed_at_unix_ms: 10,
            valid_until_unix_ms: 30,
        };
        let incompatible = OriginTlsCompatibility {
            account_id: "d".repeat(32),
            endpoint_id: "endpoint-1".to_string(),
            endpoint_revision: "endpoint:1".to_string(),
            effective_sni: name("backend.example.net"),
            certificate_names: BTreeSet::from(["other.example.net".to_string()]),
            observed_at_unix_ms: 10,
            valid_until_unix_ms: 30,
        };
        assert_eq!(
            incompatible
                .validate_for(&"d".repeat(32), &endpoint, &rule, 20)
                .unwrap_err()
                .category(),
            ProviderErrorCategory::Conflict
        );
        let compatible = OriginTlsCompatibility {
            account_id: "d".repeat(32),
            endpoint_id: "endpoint-1".to_string(),
            endpoint_revision: "endpoint:1".to_string(),
            effective_sni: name("backend.example.net"),
            certificate_names: BTreeSet::from(["*.example.net".to_string()]),
            observed_at_unix_ms: 10,
            valid_until_unix_ms: 30,
        };
        compatible
            .validate_for(&"d".repeat(32), &endpoint, &rule, 20)
            .unwrap();
    }

    #[test]
    fn mapping_requires_provider_ownership_marker_and_hashes_full_rule() {
        let ruleset = CloudflareOriginRuleset {
            id: "a".repeat(32),
            version: "7".to_string(),
            phase: ORIGIN_PHASE.to_string(),
            kind: "zone".to_string(),
            name: "origin".to_string(),
            description: "origin rules".to_string(),
            last_updated: Some("2026-07-17T00:00:00Z".to_string()),
            rules: vec![CloudflareOriginRule {
                id: "b".repeat(32),
                version: "3".to_string(),
                action: "route".to_string(),
                action_parameters: CloudflareRouteParameters {
                    host_header: None,
                    origin: Some(CloudflareRouteOrigin {
                        host: Some("origin.example.net".to_string()),
                        port: Some(8443),
                    }),
                    sni: None,
                },
                expression: "(http.host eq \"www.example.com\")".to_string(),
                description: "owned".to_string(),
                enabled: true,
                rule_ref: Some("edgion-center-origin-rule:app".to_string()),
                last_updated: Some("2026-07-17T00:00:00Z".to_string()),
            }],
        };
        let unowned = map_ruleset("c".repeat(32).as_str(), ruleset.clone(), &[]).unwrap();
        assert_eq!(unowned.rules[0].ownership, OriginRuleOwnership::Unowned);
        let proof = OriginRuleOwnershipProof {
            zone_id: "c".repeat(32),
            ruleset_id: "a".repeat(32),
            rule_id: "b".repeat(32),
            rule_revision: unowned.rules[0].revision.clone(),
            key: OriginRuleKey::new("app").unwrap(),
            match_rule: OriginRuleMatch {
                hostname: name("www.example.com"),
                path: None,
            },
            priority: 10,
        };
        let observed = map_ruleset(&proof.zone_id, ruleset, std::slice::from_ref(&proof)).unwrap();
        assert!(observed.ruleset_revision.starts_with("sha256:"));
        assert_eq!(
            observed.rules[0].ownership,
            OriginRuleOwnership::Owned {
                key: OriginRuleKey::new("app").unwrap()
            }
        );
    }

    #[test]
    fn unowned_exact_conflict_requires_revision_bound_acknowledgement() {
        let rule = desired("app", 10, OriginPathMatch::Prefix("/api".to_string()));
        let observation = OriginRulesObservation {
            ruleset_id: "a".repeat(32),
            ruleset_version: "1".to_string(),
            ruleset_revision: "sha256:one".to_string(),
            rules: vec![ObservedOriginRule {
                provider_id: "b".repeat(32),
                provider_version: "1".to_string(),
                provider_ref: None,
                ownership: OriginRuleOwnership::Unowned,
                typed_match: None,
                owned_priority: None,
                expression: "(ip.src in {192.0.2.0/24})".to_string(),
                overrides: rule.overrides.clone(),
                description: "user rule".to_string(),
                enabled: true,
                position: 0,
                revision: "sha256:rule".to_string(),
            }],
        };
        let conflicts = overlapping_unowned(&observation, &rule).unwrap();
        assert_eq!(conflicts, BTreeSet::from(["b".repeat(32)]));
        assert!(validate_explicit_override(&observation, &conflicts, None).is_err());
        validate_explicit_override(
            &observation,
            &conflicts,
            Some(&ExplicitOriginConflictOverride {
                ruleset_revision: "sha256:one".to_string(),
                accepted_unowned_rule_ids: conflicts.clone(),
            }),
        )
        .unwrap();
        assert!(validate_explicit_override(
            &observation,
            &BTreeSet::from(["b".repeat(32)]),
            Some(&ExplicitOriginConflictOverride {
                ruleset_revision: "sha256:one".to_string(),
                accepted_unowned_rule_ids: BTreeSet::from(["b".repeat(32), "c".repeat(32)]),
            }),
        )
        .is_err());
    }

    #[test]
    fn trace_request_is_bounded_and_account_endpoint_has_no_zone_guessing() {
        let endpoint = CloudflareOriginEndpointEvidence {
            account_id: "d".repeat(32),
            endpoint_id: "trace-endpoint".to_string(),
            endpoint_revision: "endpoint:1".to_string(),
            hostname: name("www.example.com"),
            proxied: true,
            observed_at_unix_ms: 10,
            valid_until_unix_ms: 30,
        };
        let request = CloudflareTraceRequest::new(&endpoint, "/api?q=1", "get", 20).unwrap();
        assert_eq!(request.url(), "https://www.example.com/api?q=1");
        assert_eq!(request.method(), "GET");
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "url": "https://www.example.com/api?q=1",
                "method": "GET",
                "protocol": "HTTP/1.1",
                "skip_response": true
            })
        );
        assert!(CloudflareTraceRequest::new(&endpoint, "/", "CONNECT", 20).is_err());
    }

    #[test]
    fn cloudflare_dns_override_rejects_ip_even_if_dns_parser_accepts_it() {
        let target = AbsoluteDnsName::new("192.0.2.10").unwrap();
        let overrides = OriginRuleOverrides {
            dns_target: Some(OriginDnsTarget::Hostname(target)),
            host_header: None,
            sni: None,
            port: None,
        };
        assert_eq!(
            overrides.validate().unwrap_err().category(),
            ProviderErrorCategory::Validation
        );
    }

    #[test]
    fn owned_last_match_wins_requires_explicit_predecessor_key() {
        let first = desired("first", 10, OriginPathMatch::Prefix("/api".to_string()));
        let mut second = desired("second", 20, OriginPathMatch::Exact("/api/v1".to_string()));
        second.overrides.port = Some(9443);
        assert_eq!(
            validate_desired_origin_rules(&[first.clone(), second.clone()])
                .unwrap_err()
                .category(),
            ProviderErrorCategory::Conflict
        );
        second.explicitly_overrides_keys.insert(first.key.clone());
        validate_desired_origin_rules(&[first, second]).unwrap();
    }

    #[test]
    fn provider_rule_unknown_fields_fail_closed() {
        let value = serde_json::json!({
            "id": "b".repeat(32),
            "version": "1",
            "action": "route",
            "action_parameters": {"origin": {"host": "origin.example.net"}},
            "expression": "(http.host eq \"www.example.com\")",
            "mystery": true
        });
        assert!(serde_json::from_value::<CloudflareOriginRule>(value).is_err());
    }

    #[test]
    fn priority_requires_owned_anchor_when_unowned_rules_exist() {
        let observation = OriginRulesObservation {
            ruleset_id: "a".repeat(32),
            ruleset_version: "1".to_string(),
            ruleset_revision: "sha256:one".to_string(),
            rules: vec![ObservedOriginRule {
                provider_id: "b".repeat(32),
                provider_version: "1".to_string(),
                provider_ref: None,
                ownership: OriginRuleOwnership::Unowned,
                typed_match: None,
                owned_priority: None,
                expression: "true".to_string(),
                overrides: OriginRuleOverrides {
                    dns_target: None,
                    host_header: None,
                    sni: None,
                    port: Some(8080),
                },
                description: "user".to_string(),
                enabled: true,
                position: 0,
                revision: "sha256:user".to_string(),
            }],
        };
        assert_eq!(
            priority_position(&observation, 10, None)
                .unwrap_err()
                .category(),
            ProviderErrorCategory::Conflict
        );
    }
}
