//! Bounded zone-scoped Cloudflare WAF Rulesets adapter.
//!
//! The Cloudflare Rulesets API has no conditional-write header.  The adapter
//! therefore requires the caller to supply the ID and version from a fresh
//! entry-point observation, verifies that observation immediately before a
//! mutation, and uses only single-rule endpoints.  It never sends a PUT for
//! an entry-point ruleset, because that endpoint replaces every rule and could
//! remove rules not owned by Center.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Debug, Formatter},
    sync::Arc,
};

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    CloudProvider, CloudResourceId, NormalizedProviderError, ProviderAccountScope,
    ProviderAccountSpec, ProviderErrorCategory,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{CloudflareApi, CloudflareApiResult, CloudflareZone};

const OWNER_REF_PREFIX: &str = "edgion-center-waf:v1:";
const OWNER_REF_SIGNATURE_BYTES: usize = 12;
const OWNER_REF_SIGNATURE_LEN: usize = 16;
const OWNER_REF_DOMAIN: &[u8] = b"edgion-center/cloudflare-waf-owner-ref/v1";
const MAX_EXPRESSION_BYTES: usize = 4_096;
const MAX_DESCRIPTION_BYTES: usize = 500;
const MAX_REFERENCE_BYTES: usize = 128;
const MAX_MANAGED_OVERRIDES: usize = 100;
const MAX_RATE_LIMIT_CHARACTERISTICS: usize = 2;
const MAX_RULES_PER_ENTRYPOINT: usize = 1_000;

/// The three zone WAF phases intentionally supported by Center.
///
/// The provider has many additional Rulesets phases.  They are deliberately
/// not representable here so this adapter cannot become a general Rulesets
/// Engine client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareWafPhase {
    Managed,
    Custom,
    RateLimit,
}

impl CloudflareWafPhase {
    pub(crate) fn provider_name(self) -> &'static str {
        match self {
            Self::Managed => "http_request_firewall_managed",
            Self::Custom => "http_request_firewall_custom",
            Self::RateLimit => "http_ratelimit",
        }
    }
}

/// Allowed terminal and preview actions for Center-owned custom WAF rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareWafAction {
    Block,
    Challenge,
    ManagedChallenge,
    Log,
}

impl CloudflareWafAction {
    fn provider_name(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Challenge => "challenge",
            Self::ManagedChallenge => "managed_challenge",
            Self::Log => "log",
        }
    }

    fn weakens_protection(self) -> bool {
        matches!(self, Self::Log)
    }
}

/// A bounded provider expression.  It is accepted only as an expression, not
/// as an arbitrary Cloudflare action-parameters object, and is redacted from
/// `Debug` output so it cannot accidentally enter audit or diagnostic logs.
#[derive(Clone, PartialEq, Eq)]
pub struct CloudflareWafExpression(String);

impl CloudflareWafExpression {
    pub fn new(value: impl Into<String>) -> CloudflareApiResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_EXPRESSION_BYTES
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            return Err(validation("invalid_cloudflare_waf_expression"));
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for CloudflareWafExpression {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CloudflareWafExpression([REDACTED])")
    }
}

impl Serialize for CloudflareWafExpression {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CloudflareWafExpression {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?)
            .map_err(|_| serde::de::Error::custom("invalid Cloudflare WAF expression"))
    }
}

/// Explicit confirmation required for a skip, disable, or preview/log rule.
/// The confirmation is a bounded audit correlation value; a later Admin API
/// layer supplies the actor and durable audit record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CloudflareWafSecurityWeakeningIntent(String);

impl CloudflareWafSecurityWeakeningIntent {
    pub fn new(value: impl Into<String>) -> CloudflareApiResult<Self> {
        let value = value.into();
        if !valid_text(&value, MAX_REFERENCE_BYTES) {
            return Err(validation("invalid_cloudflare_waf_weakening_intent"));
        }
        Ok(Self(value))
    }

    fn validate(&self) -> CloudflareApiResult<()> {
        if valid_text(&self.0, MAX_REFERENCE_BYTES) {
            Ok(())
        } else {
            Err(validation("invalid_cloudflare_waf_weakening_intent"))
        }
    }
}

/// Stable Center caller reference, encoded as a Cloudflare `ref` with the
/// adapter-owned prefix.  Only rules carrying this exact prefix are mutable or
/// deletable through this adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CloudflareWafRuleRef(String);

impl CloudflareWafRuleRef {
    pub fn new(value: impl Into<String>) -> CloudflareApiResult<Self> {
        let value = value.into();
        if !valid_text(
            &value,
            MAX_REFERENCE_BYTES - OWNER_REF_PREFIX.len() - OWNER_REF_SIGNATURE_LEN - 1,
        ) || value.starts_with(OWNER_REF_PREFIX)
        {
            return Err(validation("invalid_cloudflare_waf_rule_ref"));
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> CloudflareApiResult<()> {
        if valid_text(
            &self.0,
            MAX_REFERENCE_BYTES - OWNER_REF_PREFIX.len() - OWNER_REF_SIGNATURE_LEN - 1,
        ) && !self.0.starts_with(OWNER_REF_PREFIX)
        {
            Ok(())
        } else {
            Err(validation("invalid_cloudflare_waf_rule_ref"))
        }
    }
}

/// Active and optional verification-only keys for non-forgeable Cloudflare
/// rule ownership bindings. The key is deployment-owned and distinct from API
/// tokens and DNS cursor keys.
#[derive(Clone, PartialEq, Eq)]
pub struct CloudflareWafOwnershipKeyRing {
    active: Zeroizing<[u8; 32]>,
    fallback: Option<Zeroizing<[u8; 32]>>,
}

impl Debug for CloudflareWafOwnershipKeyRing {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CloudflareWafOwnershipKeyRing")
            .field("active", &"[REDACTED]")
            .field("fallback", &self.fallback.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl CloudflareWafOwnershipKeyRing {
    pub fn new(active: [u8; 32], fallback: Option<[u8; 32]>) -> CloudflareApiResult<Self> {
        if active.iter().all(|byte| *byte == 0) || fallback == Some(active) {
            return Err(validation("invalid_cloudflare_waf_ownership_key_ring"));
        }
        if fallback.is_some_and(|key| key.iter().all(|byte| *byte == 0)) {
            return Err(validation("invalid_cloudflare_waf_ownership_key_ring"));
        }
        Ok(Self {
            active: Zeroizing::new(active),
            fallback: fallback.map(Zeroizing::new),
        })
    }

    fn sign(
        &self,
        account_id: &CloudResourceId,
        zone_id: &str,
        phase: CloudflareWafPhase,
        reference: &CloudflareWafRuleRef,
    ) -> String {
        let signature =
            ownership_signature(&self.active, account_id, zone_id, phase, reference.as_str());
        format!("{OWNER_REF_PREFIX}{}.{}", reference.as_str(), signature)
    }

    fn verify(
        &self,
        account_id: &CloudResourceId,
        zone_id: &str,
        phase: CloudflareWafPhase,
        provider_reference: &str,
    ) -> Option<String> {
        let value = provider_reference.strip_prefix(OWNER_REF_PREFIX)?;
        let (reference, signature) = value.rsplit_once('.')?;
        let reference = CloudflareWafRuleRef::new(reference.to_owned()).ok()?;
        let active =
            ownership_signature(&self.active, account_id, zone_id, phase, reference.as_str());
        let fallback = self
            .fallback
            .as_ref()
            .map(|key| ownership_signature(key, account_id, zone_id, phase, reference.as_str()));
        if constant_time_eq(signature.as_bytes(), active.as_bytes())
            || fallback
                .is_some_and(|value| constant_time_eq(signature.as_bytes(), value.as_bytes()))
        {
            Some(reference.0)
        } else {
            None
        }
    }
}

fn ownership_signature(
    key: &[u8; 32],
    account_id: &CloudResourceId,
    zone_id: &str,
    phase: CloudflareWafPhase,
    reference: &str,
) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("fixed key size");
    mac.update(OWNER_REF_DOMAIN);
    for value in [
        account_id.as_str(),
        zone_id,
        phase.provider_name(),
        reference,
    ] {
        mac.update(&(value.len() as u32).to_be_bytes());
        mac.update(value.as_bytes());
    }
    let bytes = mac.finalize().into_bytes();
    URL_SAFE_NO_PAD.encode(&bytes[..OWNER_REF_SIGNATURE_BYTES])
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

/// A current entry-point identifier and version.  A caller obtains this from
/// [`CloudflareZoneWafAdapter::inventory`] and must provide it for every
/// update, delete, and reorder operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafRulesetRevision {
    pub id: String,
    pub version: String,
}

impl CloudflareWafRulesetRevision {
    fn validate(&self) -> CloudflareApiResult<()> {
        if !valid_cloudflare_id(&self.id) || !valid_version(&self.version) {
            return Err(validation("invalid_cloudflare_waf_ruleset_revision"));
        }
        Ok(())
    }
}

/// Position for a newly created or reordered Center-owned rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloudflareWafRulePosition {
    First,
    Before { rule_id: String },
    After { rule_id: String },
    Index { index: u16 },
}

impl CloudflareWafRulePosition {
    fn validate(&self) -> CloudflareApiResult<()> {
        match self {
            Self::First => Ok(()),
            Self::Before { rule_id } | Self::After { rule_id } => {
                if valid_cloudflare_id(rule_id) {
                    Ok(())
                } else {
                    Err(validation("invalid_cloudflare_waf_position_rule_id"))
                }
            }
            Self::Index { index } if *index > 0 => Ok(()),
            Self::Index { .. } => Err(validation("invalid_cloudflare_waf_position_index")),
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        match self {
            Self::First => json!({ "before": "" }),
            Self::Before { rule_id } => json!({ "before": rule_id }),
            Self::After { rule_id } => json!({ "after": rule_id }),
            Self::Index { index } => json!({ "index": index }),
        }
    }
}

/// A rule override for a Cloudflare managed ruleset.  The adapter intentionally
/// accepts only an action and enabled state, never arbitrary override JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafManagedRuleOverride {
    pub managed_rule_id: String,
    pub action: Option<CloudflareWafAction>,
    pub enabled: Option<bool>,
}

impl CloudflareWafManagedRuleOverride {
    fn validate(&self) -> CloudflareApiResult<()> {
        if !valid_cloudflare_id(&self.managed_rule_id)
            || (self.action.is_none() && self.enabled.is_none())
        {
            return Err(validation("invalid_cloudflare_waf_managed_override"));
        }
        Ok(())
    }

    fn weakens_protection(&self) -> bool {
        self.enabled == Some(false)
            || self
                .action
                .is_some_and(CloudflareWafAction::weakens_protection)
    }

    fn to_json(&self) -> Value {
        let mut value = serde_json::Map::new();
        value.insert(
            "id".to_string(),
            Value::String(self.managed_rule_id.clone()),
        );
        if let Some(action) = self.action {
            value.insert(
                "action".to_string(),
                Value::String(action.provider_name().to_string()),
            );
        }
        if let Some(enabled) = self.enabled {
            value.insert("enabled".to_string(), Value::Bool(enabled));
        }
        Value::Object(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafManagedRuleSpec {
    pub reference: CloudflareWafRuleRef,
    pub description: String,
    pub expression: CloudflareWafExpression,
    pub managed_ruleset_id: String,
    #[serde(default)]
    pub overrides: Vec<CloudflareWafManagedRuleOverride>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub position: Option<CloudflareWafRulePosition>,
    pub weakening_intent: Option<CloudflareWafSecurityWeakeningIntent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafCustomRuleSpec {
    pub reference: CloudflareWafRuleRef,
    pub description: String,
    pub expression: CloudflareWafExpression,
    pub action: CloudflareWafAction,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub position: Option<CloudflareWafRulePosition>,
    pub weakening_intent: Option<CloudflareWafSecurityWeakeningIntent>,
}

/// The bounded set of rate-limit counters accepted in the first slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareWafRateLimitCharacteristic {
    IpSource,
    Colo,
}

impl CloudflareWafRateLimitCharacteristic {
    fn provider_name(self) -> &'static str {
        match self {
            Self::IpSource => "ip.src",
            Self::Colo => "cf.colo.id",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafRateLimitRuleSpec {
    pub reference: CloudflareWafRuleRef,
    pub description: String,
    pub expression: CloudflareWafExpression,
    pub action: CloudflareWafAction,
    pub characteristics: BTreeSet<CloudflareWafRateLimitCharacteristic>,
    pub period_secs: u32,
    pub requests_per_period: u32,
    pub mitigation_timeout_secs: u32,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub position: Option<CloudflareWafRulePosition>,
    pub weakening_intent: Option<CloudflareWafSecurityWeakeningIntent>,
}

/// A bounded WAF rule definition.  Its phase is derived from the variant;
/// callers cannot send a free-form phase or action parameters payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloudflareWafRuleSpec {
    Managed(CloudflareWafManagedRuleSpec),
    Custom(CloudflareWafCustomRuleSpec),
    RateLimit(CloudflareWafRateLimitRuleSpec),
}

impl CloudflareWafRuleSpec {
    fn phase(&self) -> CloudflareWafPhase {
        match self {
            Self::Managed(_) => CloudflareWafPhase::Managed,
            Self::Custom(_) => CloudflareWafPhase::Custom,
            Self::RateLimit(_) => CloudflareWafPhase::RateLimit,
        }
    }

    fn validate(&self) -> CloudflareApiResult<()> {
        match self {
            Self::Managed(value) => {
                value.reference.validate()?;
                validate_description(&value.description)?;
                if !valid_cloudflare_id(&value.managed_ruleset_id)
                    || value.overrides.len() > MAX_MANAGED_OVERRIDES
                {
                    return Err(validation("invalid_cloudflare_waf_managed_rule"));
                }
                for override_value in &value.overrides {
                    override_value.validate()?;
                }
                if !value.enabled
                    || value
                        .overrides
                        .iter()
                        .any(CloudflareWafManagedRuleOverride::weakens_protection)
                {
                    require_weakening_intent(value.weakening_intent.as_ref())?;
                }
                validate_position(value.position.as_ref())
            }
            Self::Custom(value) => {
                value.reference.validate()?;
                validate_description(&value.description)?;
                if !value.enabled || value.action.weakens_protection() {
                    require_weakening_intent(value.weakening_intent.as_ref())?;
                }
                validate_position(value.position.as_ref())
            }
            Self::RateLimit(value) => {
                value.reference.validate()?;
                validate_description(&value.description)?;
                if value.characteristics.is_empty()
                    || value.characteristics.len() > MAX_RATE_LIMIT_CHARACTERISTICS
                    || !valid_rate_limit_period(value.period_secs)
                    || value.requests_per_period == 0
                    || !valid_mitigation_timeout(value.mitigation_timeout_secs)
                {
                    return Err(validation("invalid_cloudflare_waf_rate_limit"));
                }
                if !value.enabled || value.action.weakens_protection() {
                    require_weakening_intent(value.weakening_intent.as_ref())?;
                }
                validate_position(value.position.as_ref())
            }
        }
    }

    pub(crate) fn to_payload(
        &self,
        include_position: bool,
        provider_reference: String,
    ) -> CloudflareWafRulePayload {
        let (reference, description, expression, action, enabled, position, parameters, rate_limit) =
            match self {
                Self::Managed(value) => (
                    provider_reference,
                    value.description.clone(),
                    value.expression.as_str().to_string(),
                    "execute".to_string(),
                    value.enabled,
                    value.position.clone(),
                    Some(json!({
                        "id": value.managed_ruleset_id,
                        "overrides": { "rules": value.overrides.iter().map(CloudflareWafManagedRuleOverride::to_json).collect::<Vec<_>>() },
                    })),
                    None,
                ),
                Self::Custom(value) => (
                    provider_reference,
                    value.description.clone(),
                    value.expression.as_str().to_string(),
                    value.action.provider_name().to_string(),
                    value.enabled,
                    value.position.clone(),
                    None,
                    None,
                ),
                Self::RateLimit(value) => (
                    provider_reference,
                    value.description.clone(),
                    value.expression.as_str().to_string(),
                    value.action.provider_name().to_string(),
                    value.enabled,
                    value.position.clone(),
                    None,
                    Some(json!({
                        "characteristics": value.characteristics.iter().map(|item| item.provider_name()).collect::<Vec<_>>(),
                        "period": value.period_secs,
                        "requests_per_period": value.requests_per_period,
                        "mitigation_timeout": value.mitigation_timeout_secs,
                    })),
                ),
            };
        CloudflareWafRulePayload {
            reference,
            description,
            expression,
            action,
            enabled,
            position: include_position.then_some(position).flatten(),
            action_parameters: parameters,
            rate_limit,
        }
    }

    fn reference(&self) -> &CloudflareWafRuleRef {
        match self {
            Self::Managed(value) => &value.reference,
            Self::Custom(value) => &value.reference,
            Self::RateLimit(value) => &value.reference,
        }
    }
}

/// Explicitly limited exception shape for zone WAF managed rules.  It can skip
/// complete managed rulesets but cannot skip arbitrary Cloudflare products or
/// phases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafManagedExceptionSpec {
    pub reference: CloudflareWafRuleRef,
    pub description: String,
    pub expression: CloudflareWafExpression,
    pub managed_ruleset_ids: BTreeSet<String>,
    pub position: CloudflareWafRulePosition,
    pub weakening_intent: CloudflareWafSecurityWeakeningIntent,
}

impl CloudflareWafManagedExceptionSpec {
    fn validate(&self) -> CloudflareApiResult<()> {
        self.reference.validate()?;
        self.weakening_intent.validate()?;
        validate_description(&self.description)?;
        if self.managed_ruleset_ids.is_empty()
            || self.managed_ruleset_ids.len() > MAX_MANAGED_OVERRIDES
            || self
                .managed_ruleset_ids
                .iter()
                .any(|id| !valid_cloudflare_id(id))
        {
            return Err(validation("invalid_cloudflare_waf_managed_exception"));
        }
        self.position.validate()
    }

    fn to_payload(&self, provider_reference: String) -> CloudflareWafRulePayload {
        CloudflareWafRulePayload {
            reference: provider_reference,
            description: self.description.clone(),
            expression: self.expression.as_str().to_string(),
            action: "skip".to_string(),
            enabled: true,
            position: Some(self.position.clone()),
            action_parameters: Some(json!({ "rulesets": self.managed_ruleset_ids })),
            rate_limit: None,
        }
    }
}

/// Read-only, sanitized provider rule returned from a phase entry-point.
/// `action` is intentionally opaque so an unowned provider action cannot make
/// inventory fail or be interpreted as a Center-supported operation.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafProviderRule {
    pub id: String,
    #[serde(deserialize_with = "deserialize_version")]
    pub version: String,
    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
    pub action: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    expression: Option<String>,
    #[serde(default)]
    action_parameters: Option<Value>,
    #[serde(default, rename = "ratelimit")]
    rate_limit: Option<Value>,
}

impl Debug for CloudflareWafProviderRule {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CloudflareWafProviderRule")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("reference", &self.reference)
            .field("action", &self.action)
            .field("enabled", &self.enabled)
            .field("description", &"[REDACTED]")
            .field("expression", &"[REDACTED]")
            .field("action_parameters", &"[REDACTED]")
            .field("rate_limit", &"[REDACTED]")
            .finish()
    }
}

/// Raw, bounded Rulesets API entry-point response used only by the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafRuleset {
    pub id: String,
    #[serde(deserialize_with = "deserialize_version")]
    pub version: String,
    #[serde(deserialize_with = "deserialize_phase")]
    pub phase: CloudflareWafPhase,
    #[serde(default)]
    pub rules: Vec<CloudflareWafProviderRule>,
}

/// Private payload emitted only by the adapter after typed validation.
#[derive(Clone)]
pub struct CloudflareWafRulePayload {
    reference: String,
    description: String,
    expression: String,
    action: String,
    enabled: bool,
    position: Option<CloudflareWafRulePosition>,
    action_parameters: Option<Value>,
    rate_limit: Option<Value>,
}

impl CloudflareWafRulePayload {
    pub(crate) fn to_json(&self) -> Value {
        let mut value = serde_json::Map::new();
        value.insert("ref".to_string(), Value::String(self.reference.clone()));
        value.insert(
            "description".to_string(),
            Value::String(self.description.clone()),
        );
        value.insert(
            "expression".to_string(),
            Value::String(self.expression.clone()),
        );
        value.insert("action".to_string(), Value::String(self.action.clone()));
        value.insert("enabled".to_string(), Value::Bool(self.enabled));
        if let Some(parameters) = &self.action_parameters {
            value.insert("action_parameters".to_string(), parameters.clone());
        }
        if let Some(rate_limit) = &self.rate_limit {
            value.insert("ratelimit".to_string(), rate_limit.clone());
        }
        if let Some(position) = &self.position {
            value.insert("position".to_string(), position.to_json());
        }
        Value::Object(value)
    }
}

impl Debug for CloudflareWafRulePayload {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CloudflareWafRulePayload")
            .field("reference", &self.reference)
            .field("description", &self.description)
            .field("expression", &"[REDACTED]")
            .field("action", &self.action)
            .field("enabled", &self.enabled)
            .field("position", &self.position)
            .field("action_parameters", &"[REDACTED]")
            .field("rate_limit", &self.rate_limit)
            .finish()
    }
}

/// Narrow Rulesets API seam.  Implementations must make exactly one provider
/// mutation per method and classify ambiguous write results as `UnknownOutcome`.
#[async_trait]
pub trait CloudflareWafApi: Send + Sync {
    async fn get_zone_waf_entrypoint(
        &self,
        zone_id: &str,
        phase: CloudflareWafPhase,
    ) -> CloudflareApiResult<Option<CloudflareWafRuleset>>;

    async fn create_zone_waf_entrypoint(
        &self,
        zone_id: &str,
        phase: CloudflareWafPhase,
        rule: &CloudflareWafRulePayload,
    ) -> CloudflareApiResult<CloudflareWafRuleset>;

    async fn create_zone_waf_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule: &CloudflareWafRulePayload,
    ) -> CloudflareApiResult<CloudflareWafRuleset>;

    async fn update_zone_waf_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
        rule: &CloudflareWafRulePayload,
    ) -> CloudflareApiResult<CloudflareWafRuleset>;

    async fn reorder_zone_waf_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
        position: &CloudflareWafRulePosition,
    ) -> CloudflareApiResult<CloudflareWafRuleset>;

    async fn delete_zone_waf_rule(
        &self,
        zone_id: &str,
        ruleset_id: &str,
        rule_id: &str,
    ) -> CloudflareApiResult<CloudflareWafRuleset>;
}

/// Combined credential-owning Zone and Rulesets API seam.
pub trait CloudflareZoneWafApi: CloudflareApi + CloudflareWafApi {}
impl<T> CloudflareZoneWafApi for T where T: CloudflareApi + CloudflareWafApi + ?Sized {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafRuleInventoryItem {
    pub id: String,
    pub version: String,
    pub reference: Option<String>,
    /// The verified caller reference without the provider-only ownership
    /// binding. It is absent for unowned, forged, or stale-scope references.
    pub center_reference: Option<String>,
    pub action: String,
    pub enabled: bool,
    pub center_owned: bool,
    pub position: usize,
    /// Present only when a Center-owned provider rule can be parsed as one of
    /// the bounded supported definitions.  Unowned and unsupported rules stay
    /// opaque and cannot be updated or deleted through this adapter.
    pub definition: Option<CloudflareWafOwnedRuleDefinition>,
}

/// A high-trust read representation of a fully recognized Center-owned rule.
/// Expression `Debug` output is redacted; Admin audit DTOs must not serialize
/// this value into audit events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloudflareWafOwnedRuleDefinition {
    Managed {
        description: String,
        expression: CloudflareWafExpression,
        managed_ruleset_id: String,
        overrides: Vec<CloudflareWafManagedRuleOverride>,
        enabled: bool,
    },
    ManagedException {
        description: String,
        expression: CloudflareWafExpression,
        managed_ruleset_ids: BTreeSet<String>,
        enabled: bool,
    },
    Custom {
        description: String,
        expression: CloudflareWafExpression,
        action: CloudflareWafAction,
        enabled: bool,
    },
    RateLimit {
        description: String,
        expression: CloudflareWafExpression,
        action: CloudflareWafAction,
        characteristics: BTreeSet<CloudflareWafRateLimitCharacteristic>,
        period_secs: u32,
        requests_per_period: u32,
        mitigation_timeout_secs: u32,
        enabled: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareWafPhaseAvailability {
    Available,
    EntryPointAbsent,
    PermissionDenied,
    QuotaLimited,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafPhaseInventory {
    pub phase: CloudflareWafPhase,
    pub availability: CloudflareWafPhaseAvailability,
    pub revision: Option<CloudflareWafRulesetRevision>,
    pub rules: Vec<CloudflareWafRuleInventoryItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareZoneWafInventory {
    pub zone_id: String,
    pub phases: Vec<CloudflareWafPhaseInventory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafMutationReceipt {
    pub phase: CloudflareWafPhase,
    pub revision: CloudflareWafRulesetRevision,
    pub rule_id: String,
    pub security_weakening_confirmed: bool,
}

/// Account-bound Cloudflare Zone WAF adapter.
pub struct CloudflareZoneWafAdapter {
    center_account_id: CloudResourceId,
    cloudflare_account_id: String,
    ownership: CloudflareWafOwnershipKeyRing,
    api: Arc<dyn CloudflareZoneWafApi>,
}

impl CloudflareZoneWafAdapter {
    pub fn new(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        ownership: CloudflareWafOwnershipKeyRing,
        api: Arc<dyn CloudflareZoneWafApi>,
    ) -> CloudflareApiResult<Self> {
        center_account_id
            .validate()
            .map_err(|_| validation("invalid_provider_account_id"))?;
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
        if !valid_cloudflare_id(account_id) {
            return Err(validation("invalid_cloudflare_account_id"));
        }
        Ok(Self {
            center_account_id,
            cloudflare_account_id: account_id.clone(),
            ownership,
            api,
        })
    }

    /// Observes one freshly fetched zone and all supported WAF entry-points.
    pub async fn inventory(
        &self,
        zone_id: &str,
    ) -> CloudflareApiResult<CloudflareZoneWafInventory> {
        self.observe_zone(zone_id).await?;
        let mut phases = Vec::new();
        for phase in [
            CloudflareWafPhase::Managed,
            CloudflareWafPhase::Custom,
            CloudflareWafPhase::RateLimit,
        ] {
            let entrypoint = self.api.get_zone_waf_entrypoint(zone_id, phase).await;
            phases.push(entrypoint_inventory(
                phase,
                entrypoint,
                &self.ownership,
                &self.center_account_id,
                zone_id,
            )?);
        }
        Ok(CloudflareZoneWafInventory {
            zone_id: zone_id.to_string(),
            phases,
        })
    }

    /// Creates one Center-owned custom, managed, or rate-limit rule.  An
    /// entry-point is created only after a fresh 404 observation; otherwise a
    /// single-rule POST preserves every existing provider rule.
    pub async fn create_rule(
        &self,
        zone_id: &str,
        expected: Option<&CloudflareWafRulesetRevision>,
        spec: &CloudflareWafRuleSpec,
    ) -> CloudflareApiResult<CloudflareWafMutationReceipt> {
        spec.validate()?;
        self.observe_zone(zone_id).await?;
        let phase = spec.phase();
        let entrypoint = self.api.get_zone_waf_entrypoint(zone_id, phase).await?;
        let payload = spec.to_payload(
            true,
            self.ownership
                .sign(&self.center_account_id, zone_id, phase, spec.reference()),
        );
        let result = match entrypoint {
            Some(entrypoint) => {
                let expected =
                    expected.ok_or_else(|| conflict("cloudflare_waf_revision_required"))?;
                ensure_revision(&entrypoint, phase, expected)?;
                self.ensure_reference_available(zone_id, phase, &entrypoint, spec.reference())?;
                self.api
                    .create_zone_waf_rule(zone_id, &entrypoint.id, &payload)
                    .await?
            }
            None => {
                if expected.is_some() {
                    return Err(conflict("cloudflare_waf_entrypoint_missing"));
                }
                self.api
                    .create_zone_waf_entrypoint(zone_id, phase, &payload)
                    .await?
            }
        };
        receipt_from_result(phase, &result, &payload.reference, weakening_for_spec(spec))
    }

    /// Replaces the bounded definition of one Center-owned rule.  The
    /// provider receives a PATCH for that rule only; unowned rules remain
    /// untouched and retain their relative order.
    pub async fn update_rule(
        &self,
        zone_id: &str,
        expected: &CloudflareWafRulesetRevision,
        rule_id: &str,
        spec: &CloudflareWafRuleSpec,
    ) -> CloudflareApiResult<CloudflareWafMutationReceipt> {
        spec.validate()?;
        self.observe_zone(zone_id).await?;
        let phase = spec.phase();
        let entrypoint = self.require_entrypoint(zone_id, phase, expected).await?;
        let payload = spec.to_payload(
            false,
            self.ownership
                .sign(&self.center_account_id, zone_id, phase, spec.reference()),
        );
        let existing = self.ensure_center_owned_rule(zone_id, phase, &entrypoint, rule_id)?;
        let existing_reference = existing.reference.as_deref().and_then(|reference| {
            self.ownership
                .verify(&self.center_account_id, zone_id, phase, reference)
        });
        if existing_reference.as_deref() != Some(spec.reference().as_str())
            || !definition_matches_spec(spec, parse_owned_rule_definition(phase, existing))
        {
            return Err(conflict("cloudflare_waf_rule_reference_mismatch"));
        }
        let result = self
            .api
            .update_zone_waf_rule(zone_id, &entrypoint.id, rule_id, &payload)
            .await?;
        receipt_from_result(phase, &result, &payload.reference, weakening_for_spec(spec))
    }

    /// Adds a managed-rules exception before a specified deployment rule.  The
    /// dedicated exception type always proves a security-weakening intent.
    pub async fn create_managed_exception(
        &self,
        zone_id: &str,
        expected: &CloudflareWafRulesetRevision,
        spec: &CloudflareWafManagedExceptionSpec,
    ) -> CloudflareApiResult<CloudflareWafMutationReceipt> {
        spec.validate()?;
        self.observe_zone(zone_id).await?;
        let phase = CloudflareWafPhase::Managed;
        let entrypoint = self.require_entrypoint(zone_id, phase, expected).await?;
        self.ensure_reference_available(zone_id, phase, &entrypoint, &spec.reference)?;
        let payload = spec.to_payload(self.ownership.sign(
            &self.center_account_id,
            zone_id,
            phase,
            &spec.reference,
        ));
        let result = self
            .api
            .create_zone_waf_rule(zone_id, &entrypoint.id, &payload)
            .await?;
        receipt_from_result(phase, &result, &payload.reference, true)
    }

    pub async fn reorder_rule(
        &self,
        zone_id: &str,
        phase: CloudflareWafPhase,
        expected: &CloudflareWafRulesetRevision,
        rule_id: &str,
        position: &CloudflareWafRulePosition,
    ) -> CloudflareApiResult<CloudflareWafMutationReceipt> {
        position.validate()?;
        self.observe_zone(zone_id).await?;
        let entrypoint = self.require_entrypoint(zone_id, phase, expected).await?;
        self.ensure_center_owned_rule(zone_id, phase, &entrypoint, rule_id)?;
        let result = self
            .api
            .reorder_zone_waf_rule(zone_id, &entrypoint.id, rule_id, position)
            .await?;
        receipt_from_existing_rule(phase, &result, rule_id, false)
    }

    pub async fn delete_rule(
        &self,
        zone_id: &str,
        phase: CloudflareWafPhase,
        expected: &CloudflareWafRulesetRevision,
        rule_id: &str,
        weakening_intent: CloudflareWafSecurityWeakeningIntent,
    ) -> CloudflareApiResult<CloudflareWafMutationReceipt> {
        weakening_intent.validate()?;
        self.observe_zone(zone_id).await?;
        let entrypoint = self.require_entrypoint(zone_id, phase, expected).await?;
        self.ensure_center_owned_rule(zone_id, phase, &entrypoint, rule_id)?;
        let result = self
            .api
            .delete_zone_waf_rule(zone_id, &entrypoint.id, rule_id)
            .await?;
        receipt_from_removed_rule(phase, &result, rule_id, weakening_intent)
    }

    async fn require_entrypoint(
        &self,
        zone_id: &str,
        phase: CloudflareWafPhase,
        expected: &CloudflareWafRulesetRevision,
    ) -> CloudflareApiResult<CloudflareWafRuleset> {
        let entrypoint = self
            .api
            .get_zone_waf_entrypoint(zone_id, phase)
            .await?
            .ok_or_else(|| not_found("cloudflare_waf_entrypoint_not_found"))?;
        ensure_revision(&entrypoint, phase, expected)?;
        Ok(entrypoint)
    }

    fn ensure_center_owned_rule<'a>(
        &self,
        zone_id: &str,
        phase: CloudflareWafPhase,
        entrypoint: &'a CloudflareWafRuleset,
        rule_id: &str,
    ) -> CloudflareApiResult<&'a CloudflareWafProviderRule> {
        let rule = find_rule(entrypoint, rule_id)?;
        let Some(reference) = rule.reference.as_deref() else {
            return Err(conflict("cloudflare_waf_rule_not_center_owned"));
        };
        let Some(verified_reference) =
            self.ownership
                .verify(&self.center_account_id, zone_id, phase, reference)
        else {
            return Err(conflict("cloudflare_waf_rule_not_center_owned"));
        };
        let verified_count = entrypoint
            .rules
            .iter()
            .filter_map(|candidate| candidate.reference.as_deref())
            .filter_map(|candidate| {
                self.ownership
                    .verify(&self.center_account_id, zone_id, phase, candidate)
            })
            .filter(|candidate| candidate == &verified_reference)
            .count();
        if verified_count != 1 || parse_owned_rule_definition(phase, rule).is_none() {
            return Err(conflict("cloudflare_waf_rule_not_center_owned"));
        }
        Ok(rule)
    }

    fn ensure_reference_available(
        &self,
        zone_id: &str,
        phase: CloudflareWafPhase,
        entrypoint: &CloudflareWafRuleset,
        reference: &CloudflareWafRuleRef,
    ) -> CloudflareApiResult<()> {
        if entrypoint
            .rules
            .iter()
            .filter_map(|rule| rule.reference.as_deref())
            .filter_map(|value| {
                self.ownership
                    .verify(&self.center_account_id, zone_id, phase, value)
            })
            .any(|value| value == reference.as_str())
        {
            return Err(conflict("cloudflare_waf_rule_reference_conflict"));
        }
        Ok(())
    }

    async fn observe_zone(&self, zone_id: &str) -> CloudflareApiResult<CloudflareZone> {
        if !valid_cloudflare_id(zone_id) {
            return Err(validation("invalid_cloudflare_zone_id"));
        }
        let zone = self
            .api
            .get_zone(zone_id)
            .await?
            .ok_or_else(|| not_found("cloudflare_zone_not_found"))?;
        if zone.id != zone_id || zone.account_id != self.cloudflare_account_id {
            return Err(unknown_outcome("cloudflare_waf_zone_scope_mismatch"));
        }
        let _ = &self.center_account_id;
        Ok(zone)
    }
}

fn entrypoint_inventory(
    phase: CloudflareWafPhase,
    entrypoint: CloudflareApiResult<Option<CloudflareWafRuleset>>,
    ownership: &CloudflareWafOwnershipKeyRing,
    center_account_id: &CloudResourceId,
    zone_id: &str,
) -> CloudflareApiResult<CloudflareWafPhaseInventory> {
    let entrypoint = match entrypoint {
        Ok(Some(entrypoint)) => entrypoint,
        Ok(None) => {
            return Ok(CloudflareWafPhaseInventory {
                phase,
                availability: CloudflareWafPhaseAvailability::EntryPointAbsent,
                revision: None,
                rules: Vec::new(),
            });
        }
        Err(error) => {
            let availability = match error.category() {
                ProviderErrorCategory::Authorization | ProviderErrorCategory::Authentication => {
                    CloudflareWafPhaseAvailability::PermissionDenied
                }
                ProviderErrorCategory::Quota | ProviderErrorCategory::Throttled => {
                    CloudflareWafPhaseAvailability::QuotaLimited
                }
                _ => CloudflareWafPhaseAvailability::Unavailable,
            };
            return Ok(CloudflareWafPhaseInventory {
                phase,
                availability,
                revision: None,
                rules: Vec::new(),
            });
        }
    };
    validate_entrypoint(&entrypoint, phase)?;
    let mut verified_references = BTreeMap::<String, usize>::new();
    for rule in &entrypoint.rules {
        if let Some(reference) = rule
            .reference
            .as_deref()
            .and_then(|reference| ownership.verify(center_account_id, zone_id, phase, reference))
        {
            *verified_references.entry(reference).or_default() += 1;
        }
    }
    let rules = entrypoint
        .rules
        .iter()
        .enumerate()
        .map(|(position, rule)| {
            let center_reference = rule
                .reference
                .as_deref()
                .and_then(|reference| {
                    ownership.verify(center_account_id, zone_id, phase, reference)
                })
                .filter(|reference| verified_references.get(reference) == Some(&1));
            let definition = center_reference
                .as_ref()
                .and_then(|_| parse_owned_rule_definition(phase, rule));
            let center_owned = center_reference.is_some();
            CloudflareWafRuleInventoryItem {
                id: rule.id.clone(),
                version: rule.version.clone(),
                reference: rule.reference.clone(),
                center_reference,
                action: rule.action.clone(),
                enabled: rule.enabled,
                center_owned,
                position,
                definition,
            }
        })
        .collect();
    Ok(CloudflareWafPhaseInventory {
        phase,
        availability: CloudflareWafPhaseAvailability::Available,
        revision: Some(CloudflareWafRulesetRevision {
            id: entrypoint.id,
            version: entrypoint.version,
        }),
        rules,
    })
}

fn ensure_revision(
    entrypoint: &CloudflareWafRuleset,
    phase: CloudflareWafPhase,
    expected: &CloudflareWafRulesetRevision,
) -> CloudflareApiResult<()> {
    expected.validate()?;
    validate_entrypoint(entrypoint, phase)?;
    if entrypoint.id != expected.id || entrypoint.version != expected.version {
        return Err(conflict("cloudflare_waf_ruleset_version_conflict"));
    }
    Ok(())
}

fn parse_owned_rule_definition(
    phase: CloudflareWafPhase,
    rule: &CloudflareWafProviderRule,
) -> Option<CloudflareWafOwnedRuleDefinition> {
    if !rule
        .reference
        .as_deref()
        .is_some_and(|value| value.starts_with(OWNER_REF_PREFIX))
    {
        return None;
    }
    let description = rule.description.as_deref()?;
    if !valid_text(description, MAX_DESCRIPTION_BYTES) {
        return None;
    }
    let expression = CloudflareWafExpression::new(rule.expression.clone()?).ok()?;
    match phase {
        CloudflareWafPhase::Managed if rule.action == "execute" => {
            let parameters = rule.action_parameters.as_ref()?.as_object()?;
            let managed_ruleset_id = parameters.get("id")?.as_str()?;
            if !valid_cloudflare_id(managed_ruleset_id) {
                return None;
            }
            let overrides = parameters
                .get("overrides")
                .and_then(Value::as_object)
                .and_then(|value| value.get("rules"))
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .map(parse_managed_override)
                        .collect::<Option<Vec<_>>>()
                })
                .unwrap_or(Some(Vec::new()))?;
            if overrides.len() > MAX_MANAGED_OVERRIDES {
                return None;
            }
            Some(CloudflareWafOwnedRuleDefinition::Managed {
                description: description.to_string(),
                expression,
                managed_ruleset_id: managed_ruleset_id.to_string(),
                overrides,
                enabled: rule.enabled,
            })
        }
        CloudflareWafPhase::Managed if rule.action == "skip" => {
            let rulesets = rule
                .action_parameters
                .as_ref()?
                .get("rulesets")?
                .as_array()?
                .iter()
                .map(Value::as_str)
                .collect::<Option<BTreeSet<_>>>()?;
            if rulesets.is_empty()
                || rulesets.len() > MAX_MANAGED_OVERRIDES
                || rulesets.iter().any(|value| !valid_cloudflare_id(value))
            {
                return None;
            }
            Some(CloudflareWafOwnedRuleDefinition::ManagedException {
                description: description.to_string(),
                expression,
                managed_ruleset_ids: rulesets.into_iter().map(ToOwned::to_owned).collect(),
                enabled: rule.enabled,
            })
        }
        CloudflareWafPhase::Custom => Some(CloudflareWafOwnedRuleDefinition::Custom {
            description: description.to_string(),
            expression,
            action: parse_waf_action(&rule.action)?,
            enabled: rule.enabled,
        }),
        CloudflareWafPhase::RateLimit => {
            let values = rule.rate_limit.as_ref()?.as_object()?;
            let characteristics = values
                .get("characteristics")?
                .as_array()?
                .iter()
                .map(Value::as_str)
                .map(|value| match value? {
                    "ip.src" => Some(CloudflareWafRateLimitCharacteristic::IpSource),
                    "cf.colo.id" => Some(CloudflareWafRateLimitCharacteristic::Colo),
                    _ => None,
                })
                .collect::<Option<BTreeSet<_>>>()?;
            let period_secs = u32::try_from(values.get("period")?.as_u64()?).ok()?;
            let requests_per_period =
                u32::try_from(values.get("requests_per_period")?.as_u64()?).ok()?;
            let mitigation_timeout_secs =
                u32::try_from(values.get("mitigation_timeout")?.as_u64()?).ok()?;
            if characteristics.is_empty()
                || characteristics.len() > MAX_RATE_LIMIT_CHARACTERISTICS
                || !valid_rate_limit_period(period_secs)
                || requests_per_period == 0
                || !valid_mitigation_timeout(mitigation_timeout_secs)
            {
                return None;
            }
            Some(CloudflareWafOwnedRuleDefinition::RateLimit {
                description: description.to_string(),
                expression,
                action: parse_waf_action(&rule.action)?,
                characteristics,
                period_secs,
                requests_per_period,
                mitigation_timeout_secs,
                enabled: rule.enabled,
            })
        }
        CloudflareWafPhase::Managed => None,
    }
}

fn definition_matches_spec(
    spec: &CloudflareWafRuleSpec,
    definition: Option<CloudflareWafOwnedRuleDefinition>,
) -> bool {
    matches!(
        (spec, definition),
        (
            CloudflareWafRuleSpec::Managed(_),
            Some(CloudflareWafOwnedRuleDefinition::Managed { .. })
        ) | (
            CloudflareWafRuleSpec::Custom(_),
            Some(CloudflareWafOwnedRuleDefinition::Custom { .. })
        ) | (
            CloudflareWafRuleSpec::RateLimit(_),
            Some(CloudflareWafOwnedRuleDefinition::RateLimit { .. })
        )
    )
}

fn parse_managed_override(value: &Value) -> Option<CloudflareWafManagedRuleOverride> {
    let value = value.as_object()?;
    let managed_rule_id = value.get("id")?.as_str()?.to_string();
    let action = match value.get("action") {
        Some(value) => Some(parse_waf_action(value.as_str()?)?),
        None => None,
    };
    let enabled = match value.get("enabled") {
        Some(value) => Some(value.as_bool()?),
        None => None,
    };
    let override_value = CloudflareWafManagedRuleOverride {
        managed_rule_id,
        action,
        enabled,
    };
    override_value.validate().ok()?;
    Some(override_value)
}

fn parse_waf_action(value: &str) -> Option<CloudflareWafAction> {
    match value {
        "block" => Some(CloudflareWafAction::Block),
        "challenge" => Some(CloudflareWafAction::Challenge),
        "managed_challenge" => Some(CloudflareWafAction::ManagedChallenge),
        "log" => Some(CloudflareWafAction::Log),
        _ => None,
    }
}

fn find_rule<'a>(
    entrypoint: &'a CloudflareWafRuleset,
    rule_id: &str,
) -> CloudflareApiResult<&'a CloudflareWafProviderRule> {
    if !valid_cloudflare_id(rule_id) {
        return Err(validation("invalid_cloudflare_waf_rule_id"));
    }
    let Some(rule) = entrypoint.rules.iter().find(|rule| rule.id == rule_id) else {
        return Err(not_found("cloudflare_waf_rule_not_found"));
    };
    Ok(rule)
}

fn validate_entrypoint(
    entrypoint: &CloudflareWafRuleset,
    phase: CloudflareWafPhase,
) -> CloudflareApiResult<()> {
    if entrypoint.phase != phase
        || !valid_cloudflare_id(&entrypoint.id)
        || !valid_version(&entrypoint.version)
        || entrypoint.rules.len() > MAX_RULES_PER_ENTRYPOINT
        || entrypoint
            .rules
            .iter()
            .map(|rule| &rule.id)
            .collect::<BTreeSet<_>>()
            .len()
            != entrypoint.rules.len()
        || entrypoint.rules.iter().any(|rule| {
            !valid_cloudflare_id(&rule.id)
                || !valid_version(&rule.version)
                || !valid_text(&rule.action, 128)
                || rule
                    .reference
                    .as_deref()
                    .is_some_and(|reference| !valid_text(reference, MAX_REFERENCE_BYTES))
        })
    {
        return Err(unknown_outcome("cloudflare_waf_entrypoint_result_invalid"));
    }
    Ok(())
}

fn receipt_from_result(
    phase: CloudflareWafPhase,
    result: &CloudflareWafRuleset,
    reference: &str,
    security_weakening_confirmed: bool,
) -> CloudflareApiResult<CloudflareWafMutationReceipt> {
    validate_entrypoint(result, phase)?;
    let mut matches = result
        .rules
        .iter()
        .filter(|rule| rule.reference.as_deref() == Some(reference));
    let rule = matches
        .next()
        .filter(|_| matches.next().is_none())
        .ok_or_else(|| unknown_outcome("cloudflare_waf_mutation_result_missing_rule"))?;
    Ok(CloudflareWafMutationReceipt {
        phase,
        revision: CloudflareWafRulesetRevision {
            id: result.id.clone(),
            version: result.version.clone(),
        },
        rule_id: rule.id.clone(),
        security_weakening_confirmed,
    })
}

fn receipt_from_existing_rule(
    phase: CloudflareWafPhase,
    result: &CloudflareWafRuleset,
    rule_id: &str,
    security_weakening_confirmed: bool,
) -> CloudflareApiResult<CloudflareWafMutationReceipt> {
    validate_entrypoint(result, phase)?;
    if !result.rules.iter().any(|rule| rule.id == rule_id) {
        return Err(unknown_outcome(
            "cloudflare_waf_mutation_result_missing_rule",
        ));
    }
    Ok(CloudflareWafMutationReceipt {
        phase,
        revision: CloudflareWafRulesetRevision {
            id: result.id.clone(),
            version: result.version.clone(),
        },
        rule_id: rule_id.to_string(),
        security_weakening_confirmed,
    })
}

fn receipt_from_removed_rule(
    phase: CloudflareWafPhase,
    result: &CloudflareWafRuleset,
    rule_id: &str,
    _intent: CloudflareWafSecurityWeakeningIntent,
) -> CloudflareApiResult<CloudflareWafMutationReceipt> {
    validate_entrypoint(result, phase)?;
    if result.rules.iter().any(|rule| rule.id == rule_id) {
        return Err(unknown_outcome(
            "cloudflare_waf_delete_result_retained_rule",
        ));
    }
    Ok(CloudflareWafMutationReceipt {
        phase,
        revision: CloudflareWafRulesetRevision {
            id: result.id.clone(),
            version: result.version.clone(),
        },
        rule_id: rule_id.to_string(),
        security_weakening_confirmed: true,
    })
}

fn weakening_for_spec(spec: &CloudflareWafRuleSpec) -> bool {
    match spec {
        CloudflareWafRuleSpec::Managed(value) => {
            !value.enabled
                || value
                    .overrides
                    .iter()
                    .any(CloudflareWafManagedRuleOverride::weakens_protection)
        }
        CloudflareWafRuleSpec::Custom(value) => !value.enabled || value.action.weakens_protection(),
        CloudflareWafRuleSpec::RateLimit(value) => {
            !value.enabled || value.action.weakens_protection()
        }
    }
}

fn validate_description(value: &str) -> CloudflareApiResult<()> {
    if valid_text(value, MAX_DESCRIPTION_BYTES) {
        Ok(())
    } else {
        Err(validation("invalid_cloudflare_waf_description"))
    }
}

fn validate_position(value: Option<&CloudflareWafRulePosition>) -> CloudflareApiResult<()> {
    value.map(CloudflareWafRulePosition::validate).transpose()?;
    Ok(())
}

fn require_weakening_intent(
    value: Option<&CloudflareWafSecurityWeakeningIntent>,
) -> CloudflareApiResult<()> {
    match value {
        Some(value) => value.validate(),
        None => Err(validation(
            "cloudflare_waf_security_weakening_intent_required",
        )),
    }
}

fn valid_rate_limit_period(value: u32) -> bool {
    matches!(value, 10 | 60 | 120 | 300 | 600 | 3600)
}

fn valid_mitigation_timeout(value: u32) -> bool {
    matches!(value, 0 | 10 | 60 | 120 | 300 | 600 | 3600 | 86400)
}

fn valid_cloudflare_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_version(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_text(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn enabled_by_default() -> bool {
    true
}

fn deserialize_phase<'de, D>(deserializer: D) -> Result<CloudflareWafPhase, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    match value.as_str() {
        "http_request_firewall_managed" => Ok(CloudflareWafPhase::Managed),
        "http_request_firewall_custom" => Ok(CloudflareWafPhase::Custom),
        "http_ratelimit" => Ok(CloudflareWafPhase::RateLimit),
        _ => Err(serde::de::Error::custom("unsupported Cloudflare WAF phase")),
    }
}

fn deserialize_version<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::String(value) => Ok(value),
        Value::Number(value) if value.is_u64() => Ok(value.to_string()),
        _ => Err(serde::de::Error::custom(
            "invalid Cloudflare ruleset version",
        )),
    }
}

fn provider_error(category: ProviderErrorCategory, code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        code,
        "Cloudflare WAF adapter rejected the request",
        None,
        None,
    )
    .expect("static normalized provider error")
}

fn validation(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Validation, code)
}

fn conflict(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Conflict, code)
}

fn not_found(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::NotFound, code)
}

fn unknown_outcome(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::UnknownOutcome, code)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    use edgion_center_core::{AbsoluteDnsName, CredentialSource, DnssecDesiredState};

    use super::*;
    use crate::{
        CloudflareBatchRequest, CloudflareBatchResult, CloudflareCreateZoneRequest,
        CloudflareDeleteZoneAck, CloudflareDnssec, CloudflarePage, CloudflareRecord,
        CloudflareZoneKind, CloudflareZoneStatus,
    };

    const ACCOUNT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ZONE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const RULESET: &str = "cccccccccccccccccccccccccccccccc";
    const OWNED_RULE: &str = "dddddddddddddddddddddddddddddddd";
    const UNOWNED_RULE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const OWNERSHIP_KEY: [u8; 32] = [7; 32];

    struct FakeApi {
        entrypoint: Mutex<CloudflareApiResult<Option<CloudflareWafRuleset>>>,
        mutation_result: Mutex<CloudflareApiResult<CloudflareWafRuleset>>,
        dispatched: AtomicUsize,
        payloads: Mutex<Vec<Value>>,
    }

    impl FakeApi {
        fn new(
            entrypoint: CloudflareApiResult<Option<CloudflareWafRuleset>>,
            mutation_result: CloudflareApiResult<CloudflareWafRuleset>,
        ) -> Self {
            Self {
                entrypoint: Mutex::new(entrypoint),
                mutation_result: Mutex::new(mutation_result),
                dispatched: AtomicUsize::new(0),
                payloads: Mutex::new(Vec::new()),
            }
        }

        fn zone() -> CloudflareZone {
            CloudflareZone {
                id: ZONE.to_string(),
                account_id: ACCOUNT.to_string(),
                name: "example.com".to_string(),
                kind: CloudflareZoneKind::Full,
                status: CloudflareZoneStatus::Active,
                name_servers: [AbsoluteDnsName::new("ada.ns.cloudflare.com").unwrap()]
                    .into_iter()
                    .collect(),
                modified_on: Some("2026-07-22T00:00:00Z".to_string()),
            }
        }

        fn unavailable() -> NormalizedProviderError {
            provider_error(ProviderErrorCategory::Authorization, "cloudflare_api_10000")
        }
    }

    #[async_trait]
    impl CloudflareApi for FakeApi {
        async fn create_zone(
            &self,
            _: &CloudflareCreateZoneRequest,
        ) -> CloudflareApiResult<CloudflareZone> {
            Err(Self::unavailable())
        }
        async fn get_zone(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareZone>> {
            assert_eq!(zone_id, ZONE);
            Ok(Some(Self::zone()))
        }
        async fn delete_zone(&self, _: &str) -> CloudflareApiResult<CloudflareDeleteZoneAck> {
            Err(Self::unavailable())
        }
        async fn get_dnssec(&self, _: &str) -> CloudflareApiResult<Option<CloudflareDnssec>> {
            Err(Self::unavailable())
        }
        async fn patch_dnssec(
            &self,
            _: &str,
            _: DnssecDesiredState,
        ) -> CloudflareApiResult<CloudflareDnssec> {
            Err(Self::unavailable())
        }
        async fn list_zones(
            &self,
            _: &str,
            _: u32,
            _: u32,
        ) -> CloudflareApiResult<CloudflarePage<CloudflareZone>> {
            Err(Self::unavailable())
        }
        async fn list_records(
            &self,
            _: &str,
            _: u32,
            _: u32,
        ) -> CloudflareApiResult<CloudflarePage<CloudflareRecord>> {
            Err(Self::unavailable())
        }
        async fn batch_records(
            &self,
            _: &str,
            _: &CloudflareBatchRequest,
        ) -> CloudflareApiResult<CloudflareBatchResult> {
            Err(Self::unavailable())
        }
    }

    #[async_trait]
    impl CloudflareWafApi for FakeApi {
        async fn get_zone_waf_entrypoint(
            &self,
            _: &str,
            phase: CloudflareWafPhase,
        ) -> CloudflareApiResult<Option<CloudflareWafRuleset>> {
            if phase == CloudflareWafPhase::Custom {
                self.entrypoint.lock().unwrap().clone()
            } else {
                Ok(None)
            }
        }
        async fn create_zone_waf_entrypoint(
            &self,
            _: &str,
            _: CloudflareWafPhase,
            rule: &CloudflareWafRulePayload,
        ) -> CloudflareApiResult<CloudflareWafRuleset> {
            self.dispatched.fetch_add(1, Ordering::SeqCst);
            self.payloads.lock().unwrap().push(rule.to_json());
            self.mutation_result.lock().unwrap().clone()
        }
        async fn create_zone_waf_rule(
            &self,
            _: &str,
            _: &str,
            rule: &CloudflareWafRulePayload,
        ) -> CloudflareApiResult<CloudflareWafRuleset> {
            self.dispatched.fetch_add(1, Ordering::SeqCst);
            self.payloads.lock().unwrap().push(rule.to_json());
            self.mutation_result.lock().unwrap().clone()
        }
        async fn update_zone_waf_rule(
            &self,
            _: &str,
            _: &str,
            _: &str,
            rule: &CloudflareWafRulePayload,
        ) -> CloudflareApiResult<CloudflareWafRuleset> {
            self.dispatched.fetch_add(1, Ordering::SeqCst);
            self.payloads.lock().unwrap().push(rule.to_json());
            self.mutation_result.lock().unwrap().clone()
        }
        async fn reorder_zone_waf_rule(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &CloudflareWafRulePosition,
        ) -> CloudflareApiResult<CloudflareWafRuleset> {
            self.dispatched.fetch_add(1, Ordering::SeqCst);
            self.mutation_result.lock().unwrap().clone()
        }
        async fn delete_zone_waf_rule(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> CloudflareApiResult<CloudflareWafRuleset> {
            self.dispatched.fetch_add(1, Ordering::SeqCst);
            self.mutation_result.lock().unwrap().clone()
        }
    }

    fn ruleset(version: &str, rules: Vec<CloudflareWafProviderRule>) -> CloudflareWafRuleset {
        CloudflareWafRuleset {
            id: RULESET.to_string(),
            version: version.to_string(),
            phase: CloudflareWafPhase::Custom,
            rules,
        }
    }

    fn provider_rule(id: &str, reference: Option<&str>, action: &str) -> CloudflareWafProviderRule {
        CloudflareWafProviderRule {
            id: id.to_string(),
            version: "1".to_string(),
            reference: reference.map(ToOwned::to_owned),
            action: action.to_string(),
            enabled: true,
            description: Some("rule".to_string()),
            expression: Some("http.host eq \"example.com\"".to_string()),
            action_parameters: None,
            rate_limit: None,
        }
    }

    fn adapter(api: Arc<FakeApi>) -> CloudflareZoneWafAdapter {
        let account = ProviderAccountSpec {
            provider: CloudProvider::Cloudflare,
            scope: Some(ProviderAccountScope::Cloudflare {
                account_id: ACCOUNT.to_string(),
            }),
            credential_source: CredentialSource::Ambient,
        };
        CloudflareZoneWafAdapter::new(
            CloudResourceId::new("cf-main").unwrap(),
            &account,
            CloudflareWafOwnershipKeyRing::new(OWNERSHIP_KEY, None).unwrap(),
            api,
        )
        .unwrap()
    }

    fn signed_reference(phase: CloudflareWafPhase, reference: &str) -> String {
        CloudflareWafOwnershipKeyRing::new(OWNERSHIP_KEY, None)
            .unwrap()
            .sign(
                &CloudResourceId::new("cf-main").unwrap(),
                ZONE,
                phase,
                &CloudflareWafRuleRef::new(reference).unwrap(),
            )
    }

    fn custom_spec(position: Option<CloudflareWafRulePosition>) -> CloudflareWafRuleSpec {
        CloudflareWafRuleSpec::Custom(CloudflareWafCustomRuleSpec {
            reference: CloudflareWafRuleRef::new("api-block").unwrap(),
            description: "Block API abuse".to_string(),
            expression: CloudflareWafExpression::new("http.request.uri.path starts_with \"/api\"")
                .unwrap(),
            action: CloudflareWafAction::Block,
            enabled: true,
            position,
            weakening_intent: None,
        })
    }

    #[tokio::test]
    async fn inventory_keeps_unowned_opaque_and_surfaces_permission_and_quota_evidence() {
        let opaque = provider_rule(UNOWNED_RULE, None, "future_provider_action");
        let owned = provider_rule(
            OWNED_RULE,
            Some(&signed_reference(CloudflareWafPhase::Custom, "known")),
            "block",
        );
        let owned_opaque = provider_rule(
            ZONE,
            Some(&signed_reference(CloudflareWafPhase::Custom, "legacy")),
            "future_action",
        );
        let api = Arc::new(FakeApi::new(
            Ok(Some(ruleset("1", vec![opaque, owned, owned_opaque]))),
            Ok(ruleset("2", vec![])),
        ));
        let inventory = adapter(api).inventory(ZONE).await.unwrap();
        let custom = &inventory.phases[1];
        assert_eq!(
            custom.availability,
            CloudflareWafPhaseAvailability::Available
        );
        assert_eq!(custom.rules[0].position, 0);
        assert!(!custom.rules[0].center_owned);
        assert!(custom.rules[0].definition.is_none());
        assert!(matches!(
            custom.rules[1].definition,
            Some(CloudflareWafOwnedRuleDefinition::Custom { .. })
        ));
        assert!(custom.rules[2].center_owned);
        assert!(custom.rules[2].definition.is_none());

        let denied = Arc::new(FakeApi::new(
            Err(FakeApi::unavailable()),
            Ok(ruleset("2", vec![])),
        ));
        let inventory = adapter(denied).inventory(ZONE).await.unwrap();
        assert_eq!(
            inventory.phases[1].availability,
            CloudflareWafPhaseAvailability::PermissionDenied
        );

        let quota = provider_error(ProviderErrorCategory::Quota, "cloudflare_plan_limit");
        let quota_api = Arc::new(FakeApi::new(Err(quota), Ok(ruleset("2", vec![]))));
        let inventory = adapter(quota_api).inventory(ZONE).await.unwrap();
        assert_eq!(
            inventory.phases[1].availability,
            CloudflareWafPhaseAvailability::QuotaLimited
        );
    }

    #[tokio::test]
    async fn duplicate_or_forged_owner_references_are_never_mutable() {
        let signed = signed_reference(CloudflareWafPhase::Custom, "api-block");
        let entrypoint = ruleset(
            "1",
            vec![
                provider_rule(OWNED_RULE, Some(&signed), "block"),
                provider_rule(ZONE, Some(&signed), "block"),
            ],
        );
        let api = Arc::new(FakeApi::new(Ok(Some(entrypoint)), Ok(ruleset("2", vec![]))));
        let waf_adapter = adapter(api.clone());
        let inventory = waf_adapter.inventory(ZONE).await.unwrap();
        assert!(inventory.phases[1]
            .rules
            .iter()
            .all(|rule| !rule.center_owned && rule.definition.is_none()));
        let result = waf_adapter
            .update_rule(
                ZONE,
                &CloudflareWafRulesetRevision {
                    id: RULESET.to_string(),
                    version: "1".to_string(),
                },
                OWNED_RULE,
                &custom_spec(None),
            )
            .await;
        assert_eq!(
            result.unwrap_err().category(),
            ProviderErrorCategory::Conflict
        );
        assert_eq!(api.dispatched.load(Ordering::SeqCst), 0);

        let forged = provider_rule(
            OWNED_RULE,
            Some("edgion-center-waf:v1:api-block.invalid"),
            "block",
        );
        let api = Arc::new(FakeApi::new(
            Ok(Some(ruleset("1", vec![forged]))),
            Ok(ruleset("2", vec![])),
        ));
        let inventory = adapter(api).inventory(ZONE).await.unwrap();
        assert!(!inventory.phases[1].rules[0].center_owned);
    }

    #[tokio::test]
    async fn stale_revision_fails_before_dispatch_and_owned_create_keeps_position_bounded() {
        let existing = ruleset("2", vec![provider_rule(UNOWNED_RULE, None, "block")]);
        let result = ruleset(
            "3",
            vec![provider_rule(
                OWNED_RULE,
                Some(&signed_reference(CloudflareWafPhase::Custom, "api-block")),
                "block",
            )],
        );
        let api = Arc::new(FakeApi::new(Ok(Some(existing)), Ok(result)));
        let stale = CloudflareWafRulesetRevision {
            id: RULESET.to_string(),
            version: "1".to_string(),
        };
        let error = adapter(api.clone())
            .create_rule(
                ZONE,
                Some(&stale),
                &custom_spec(Some(CloudflareWafRulePosition::Before {
                    rule_id: UNOWNED_RULE.to_string(),
                })),
            )
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Conflict);
        assert_eq!(api.dispatched.load(Ordering::SeqCst), 0);

        let current = CloudflareWafRulesetRevision {
            id: RULESET.to_string(),
            version: "2".to_string(),
        };
        let receipt = adapter(api.clone())
            .create_rule(
                ZONE,
                Some(&current),
                &custom_spec(Some(CloudflareWafRulePosition::Before {
                    rule_id: UNOWNED_RULE.to_string(),
                })),
            )
            .await
            .unwrap();
        assert_eq!(receipt.rule_id, OWNED_RULE);
        assert_eq!(api.dispatched.load(Ordering::SeqCst), 1);
        assert_eq!(
            api.payloads.lock().unwrap()[0]["position"]["before"],
            UNOWNED_RULE
        );
    }

    #[tokio::test]
    async fn ambiguous_mutation_is_not_retried_and_debug_redacts_expression() {
        let current = ruleset("1", vec![]);
        let api = Arc::new(FakeApi::new(
            Ok(Some(current)),
            Err(unknown_outcome("cloudflare_waf_write_ambiguous")),
        ));
        let revision = CloudflareWafRulesetRevision {
            id: RULESET.to_string(),
            version: "1".to_string(),
        };
        let spec = custom_spec(None);
        let error = adapter(api.clone())
            .create_rule(ZONE, Some(&revision), &spec)
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
        assert_eq!(api.dispatched.load(Ordering::SeqCst), 1);
        let debug = format!("{spec:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("http.request.uri.path"));
    }
}
