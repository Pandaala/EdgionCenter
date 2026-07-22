//! Cloudflare-specific Zone WAF Admin API.
//!
//! This module deliberately exposes only bounded, phase-specific WAF operations. It has no
//! Cloudflare SDK dependency and never serializes credentials or raw provider objects. Bounded
//! expressions are returned only to the high-trust WAF read route and never derive `Debug`.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use edgion_center_core::{CloudResourceId, DnsZoneId};
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::ApiResponse;

const CLOUDFLARE_ZONE_ID_LEN: usize = 32;
const MAX_RULES_PER_RULESET: usize = 1_000;
const MAX_EXPRESSION_LEN: usize = 4_096;
const SECURITY_WEAKEN_CONFIRMATION: &str = "WEAKEN_CLOUDFLARE_WAF";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareWafPhase {
    Managed,
    Custom,
    RateLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareWafAction {
    Block,
    Challenge,
    ManagedChallenge,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareWafOwnership {
    CenterOwned,
    ObserveOnly,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareManagedRuleOverrideDto {
    pub managed_rule_id: String,
    pub action: Option<CloudflareWafAction>,
    pub enabled: Option<bool>,
}

impl CloudflareManagedRuleOverrideDto {
    fn valid(&self) -> bool {
        valid_opaque_id(&self.managed_rule_id)
            && (self.action.is_some() || self.enabled.is_some())
            && self.action != Some(CloudflareWafAction::Log)
            && self.enabled != Some(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CloudflareWafRulePositionDto {
    First,
    Before { rule_id: String },
    After { rule_id: String },
    Index { index: u16 },
}

impl CloudflareWafRulePositionDto {
    fn valid(&self) -> bool {
        match self {
            Self::First => true,
            Self::Before { rule_id } | Self::After { rule_id } => valid_opaque_id(rule_id),
            Self::Index { index } => *index > 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareRateLimitCharacteristicDto {
    IpSource,
    Colo,
}

/// Sanitized rule inventory. Expressions cross the boundary only for verified Center-owned,
/// fully parsed definitions; opaque provider expressions and arbitrary action parameters do not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafRuleDto {
    pub rule_id: String,
    pub version: String,
    /// Opaque provider action for inventory only. It is never accepted by a mutation request.
    pub action: String,
    pub enabled: bool,
    pub ownership: CloudflareWafOwnership,
    pub position: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<CloudflareWafRuleDefinitionDto>,
}

/// Recognized Center-owned definitions are available to high-trust WAF editors. Unknown and
/// unowned provider rules remain opaque through [`CloudflareWafRuleDto`].
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloudflareWafRuleDefinitionDto {
    Managed {
        reference: String,
        description: String,
        expression: String,
        managed_ruleset_id: String,
        overrides: Vec<CloudflareManagedRuleOverrideView>,
    },
    ManagedException {
        reference: String,
        description: String,
        expression: String,
        managed_ruleset_ids: Vec<String>,
        position: CloudflareWafRulePositionDto,
    },
    Custom {
        reference: String,
        description: String,
        expression: String,
        action: CloudflareWafAction,
    },
    RateLimit {
        reference: String,
        description: String,
        expression: String,
        action: CloudflareWafAction,
        characteristics: std::collections::BTreeSet<CloudflareRateLimitCharacteristicDto>,
        period_secs: u32,
        requests_per_period: u32,
        mitigation_timeout_secs: u32,
    },
}

impl std::fmt::Debug for CloudflareWafRuleDefinitionDto {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CloudflareWafRuleDefinitionDto([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareManagedRuleOverrideView {
    pub managed_rule_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<CloudflareWafAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

impl CloudflareWafRuleDefinitionDto {
    fn valid(&self) -> bool {
        match self {
            Self::Managed {
                reference,
                description,
                expression,
                managed_ruleset_id,
                overrides,
            } => {
                valid_rule_reference(reference)
                    && valid_description(description)
                    && valid_expression(expression)
                    && valid_opaque_id(managed_ruleset_id)
                    && overrides.len() <= MAX_RULES_PER_RULESET
                    && overrides
                        .iter()
                        .all(|item| valid_opaque_id(&item.managed_rule_id))
            }
            Self::ManagedException {
                reference,
                description,
                expression,
                managed_ruleset_ids,
                position,
            } => {
                valid_rule_reference(reference)
                    && valid_description(description)
                    && valid_expression(expression)
                    && !managed_ruleset_ids.is_empty()
                    && managed_ruleset_ids.len() <= MAX_RULES_PER_RULESET
                    && managed_ruleset_ids.iter().all(|id| valid_opaque_id(id))
                    && position.valid()
            }
            Self::Custom {
                reference,
                description,
                expression,
                action: _,
            } => {
                valid_rule_reference(reference)
                    && valid_description(description)
                    && valid_expression(expression)
            }
            Self::RateLimit {
                reference,
                description,
                expression,
                action: _,
                characteristics,
                period_secs,
                requests_per_period,
                mitigation_timeout_secs,
            } => {
                valid_rule_reference(reference)
                    && valid_description(description)
                    && valid_expression(expression)
                    && !characteristics.is_empty()
                    && characteristics.len() <= 2
                    && valid_rate_limit_period(*period_secs)
                    && (1..=1_000_000).contains(requests_per_period)
                    && valid_mitigation_timeout(*mitigation_timeout_secs)
            }
        }
    }

    fn matches_phase(&self, phase: CloudflareWafPhase) -> bool {
        matches!(
            (phase, self),
            (
                CloudflareWafPhase::Managed,
                Self::Managed { .. } | Self::ManagedException { .. }
            ) | (CloudflareWafPhase::Custom, Self::Custom { .. })
                | (CloudflareWafPhase::RateLimit, Self::RateLimit { .. })
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafRulesetDto {
    pub provider_account_id: CloudResourceId,
    pub zone_id: DnsZoneId,
    pub phase: CloudflareWafPhase,
    pub availability: CloudflareWafPhaseAvailability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ruleset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub rules: Vec<CloudflareWafRuleDto>,
}

impl CloudflareWafRulesetDto {
    fn validate(&self, account_id: &CloudResourceId, zone_id: &DnsZoneId) -> bool {
        self.provider_account_id == *account_id
            && self.zone_id == *zone_id
            && self
                .ruleset_id
                .as_ref()
                .is_none_or(|id| valid_opaque_id(id))
            && self
                .version
                .as_ref()
                .is_none_or(|version| valid_version(version))
            && self.ruleset_id.is_some() == self.version.is_some()
            && match self.availability {
                CloudflareWafPhaseAvailability::Available => self.ruleset_id.is_some(),
                CloudflareWafPhaseAvailability::EntryPointAbsent
                | CloudflareWafPhaseAvailability::PermissionDenied
                | CloudflareWafPhaseAvailability::QuotaLimited
                | CloudflareWafPhaseAvailability::Unavailable => {
                    self.ruleset_id.is_none() && self.rules.is_empty()
                }
            }
            && self.rules.len() <= MAX_RULES_PER_RULESET
            && self.rules.iter().enumerate().all(|(index, rule)| {
                valid_opaque_id(&rule.rule_id)
                    && valid_version(&rule.version)
                    && valid_opaque_action(&rule.action)
                    && rule.position == index
                    && rule.definition.as_ref().is_none_or(|definition| {
                        definition.valid() && definition.matches_phase(self.phase)
                    })
            })
            && self.rules.iter().all(|rule| {
                rule.definition.is_none() || rule.ownership == CloudflareWafOwnership::CenterOwned
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafInventoryDto {
    pub rulesets: Vec<CloudflareWafRulesetDto>,
}

impl CloudflareWafInventoryDto {
    fn validate(&self, account_id: &CloudResourceId, zone_id: &DnsZoneId) -> bool {
        self.rulesets.len() == 3
            && self
                .rulesets
                .iter()
                .all(|ruleset| ruleset.validate(account_id, zone_id))
            && self
                .rulesets
                .iter()
                .map(|ruleset| ruleset.phase)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == 3
    }

    fn phase(&self, phase: CloudflareWafPhase) -> Option<CloudflareWafRulesetDto> {
        self.rulesets
            .iter()
            .find(|ruleset| ruleset.phase == phase)
            .cloned()
    }
}

impl Ord for CloudflareWafPhase {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

impl PartialOrd for CloudflareWafPhase {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareWafVersionGuardDto {
    pub ruleset_id: String,
    pub ruleset_version: String,
}

impl CloudflareWafVersionGuardDto {
    fn valid(&self) -> bool {
        valid_opaque_id(&self.ruleset_id) && valid_version(&self.ruleset_version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareWafRulesetVersionGuardDto {
    pub ruleset_id: Option<String>,
    pub ruleset_version: Option<String>,
}

impl CloudflareWafRulesetVersionGuardDto {
    fn valid(&self) -> bool {
        self.ruleset_id.is_some() == self.ruleset_version.is_some()
            && self
                .ruleset_id
                .as_ref()
                .is_none_or(|id| valid_opaque_id(id))
            && self
                .ruleset_version
                .as_ref()
                .is_none_or(|version| valid_version(version))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareManagedRuleUpdateRequest {
    pub guard: CloudflareWafVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub managed_ruleset_id: String,
    #[serde(default)]
    pub overrides: Vec<CloudflareManagedRuleOverrideDto>,
}

impl CloudflareManagedRuleUpdateRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && valid_opaque_id(&self.managed_ruleset_id)
            && self.overrides.len() <= MAX_RULES_PER_RULESET
            && self
                .overrides
                .iter()
                .all(CloudflareManagedRuleOverrideDto::valid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareManagedRuleCreateRequest {
    pub guard: CloudflareWafRulesetVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub managed_ruleset_id: String,
    #[serde(default)]
    pub overrides: Vec<CloudflareManagedRuleOverrideDto>,
    pub position: Option<CloudflareWafRulePositionDto>,
}

impl CloudflareManagedRuleCreateRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && valid_opaque_id(&self.managed_ruleset_id)
            && self.overrides.len() <= MAX_RULES_PER_RULESET
            && self
                .overrides
                .iter()
                .all(CloudflareManagedRuleOverrideDto::valid)
            && self
                .position
                .as_ref()
                .is_none_or(CloudflareWafRulePositionDto::valid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareManagedRuleExceptionRequest {
    pub guard: CloudflareWafVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub managed_ruleset_ids: Vec<String>,
    pub position: CloudflareWafRulePositionDto,
    pub confirmation: String,
}

impl CloudflareManagedRuleExceptionRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && !self.managed_ruleset_ids.is_empty()
            && self.managed_ruleset_ids.len() <= MAX_RULES_PER_RULESET
            && self
                .managed_ruleset_ids
                .iter()
                .all(|id| valid_opaque_id(id))
            && self.position.valid()
            && self.confirmation == SECURITY_WEAKEN_CONFIRMATION
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareManagedSecurityWeakenRequest {
    pub guard: CloudflareWafVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub managed_ruleset_id: String,
    #[serde(default)]
    pub overrides: Vec<CloudflareManagedRuleOverrideDto>,
    pub enabled: bool,
    pub confirmation: String,
}

impl CloudflareManagedSecurityWeakenRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && valid_opaque_id(&self.managed_ruleset_id)
            && self.overrides.len() <= MAX_RULES_PER_RULESET
            && self.overrides.iter().all(|item| {
                valid_opaque_id(&item.managed_rule_id)
                    && (item.action.is_some() || item.enabled.is_some())
            })
            && (!self.enabled
                || self.overrides.iter().any(|item| {
                    item.enabled == Some(false) || item.action == Some(CloudflareWafAction::Log)
                }))
            && self.confirmation == SECURITY_WEAKEN_CONFIRMATION
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareCustomRuleCreateRequest {
    pub guard: CloudflareWafRulesetVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub action: CloudflareWafAction,
    pub position: Option<CloudflareWafRulePositionDto>,
}

impl CloudflareCustomRuleCreateRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && self.action != CloudflareWafAction::Log
            && self
                .position
                .as_ref()
                .is_none_or(CloudflareWafRulePositionDto::valid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareCustomRuleUpdateRequest {
    pub guard: CloudflareWafVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub action: CloudflareWafAction,
}

impl CloudflareCustomRuleUpdateRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && self.action != CloudflareWafAction::Log
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareRuleDeleteRequest {
    pub guard: CloudflareWafVersionGuardDto,
    pub confirmation: String,
}

impl CloudflareRuleDeleteRequest {
    fn valid(&self) -> bool {
        self.guard.valid() && self.confirmation == SECURITY_WEAKEN_CONFIRMATION
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareRuleOrderRequest {
    pub guard: CloudflareWafVersionGuardDto,
    pub position: CloudflareWafRulePositionDto,
}

impl CloudflareRuleOrderRequest {
    fn valid(&self) -> bool {
        self.guard.valid() && self.position.valid()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareRateLimitPeriod {
    Seconds10,
    Seconds60,
    Seconds120,
    Seconds300,
    Seconds600,
    Seconds3600,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareRateLimitRuleCreateRequest {
    pub guard: CloudflareWafRulesetVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub action: CloudflareWafAction,
    pub characteristics: std::collections::BTreeSet<CloudflareRateLimitCharacteristicDto>,
    pub requests_per_period: u32,
    pub period_secs: u32,
    pub mitigation_timeout_secs: u32,
    pub position: Option<CloudflareWafRulePositionDto>,
}

impl CloudflareRateLimitRuleCreateRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && self.action != CloudflareWafAction::Log
            && !self.characteristics.is_empty()
            && self.characteristics.len() <= 2
            && (1..=1_000_000).contains(&self.requests_per_period)
            && valid_rate_limit_period(self.period_secs)
            && valid_mitigation_timeout(self.mitigation_timeout_secs)
            && self
                .position
                .as_ref()
                .is_none_or(CloudflareWafRulePositionDto::valid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareRateLimitRuleUpdateRequest {
    pub guard: CloudflareWafVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub action: CloudflareWafAction,
    pub characteristics: std::collections::BTreeSet<CloudflareRateLimitCharacteristicDto>,
    pub requests_per_period: u32,
    pub period_secs: u32,
    pub mitigation_timeout_secs: u32,
}

impl CloudflareRateLimitRuleUpdateRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && self.action != CloudflareWafAction::Log
            && !self.characteristics.is_empty()
            && self.characteristics.len() <= 2
            && (1..=1_000_000).contains(&self.requests_per_period)
            && valid_rate_limit_period(self.period_secs)
            && valid_mitigation_timeout(self.mitigation_timeout_secs)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CloudflareSecurityWeakenChange {
    Disable,
    SetLog,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareCustomSecurityWeakenRequest {
    pub guard: CloudflareWafVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub action: CloudflareWafAction,
    pub enabled: bool,
    pub confirmation: String,
}

impl CloudflareCustomSecurityWeakenRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && (!self.enabled || self.action == CloudflareWafAction::Log)
            && self.confirmation == SECURITY_WEAKEN_CONFIRMATION
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareRateLimitSecurityWeakenRequest {
    pub guard: CloudflareWafVersionGuardDto,
    pub reference: String,
    pub description: String,
    pub expression: String,
    pub action: CloudflareWafAction,
    pub characteristics: std::collections::BTreeSet<CloudflareRateLimitCharacteristicDto>,
    pub requests_per_period: u32,
    pub period_secs: u32,
    pub mitigation_timeout_secs: u32,
    pub enabled: bool,
    pub confirmation: String,
}

impl CloudflareRateLimitSecurityWeakenRequest {
    fn valid(&self) -> bool {
        self.guard.valid()
            && valid_rule_reference(&self.reference)
            && valid_description(&self.description)
            && valid_expression(&self.expression)
            && (!self.enabled || self.action == CloudflareWafAction::Log)
            && !self.characteristics.is_empty()
            && self.characteristics.len() <= 2
            && (1..=1_000_000).contains(&self.requests_per_period)
            && valid_rate_limit_period(self.period_secs)
            && valid_mitigation_timeout(self.mitigation_timeout_secs)
            && self.confirmation == SECURITY_WEAKEN_CONFIRMATION
    }
}

/// All mutation kinds are closed and phase-specific. The service receives a complete path-bound
/// identity, so a provider adapter cannot accidentally use body-controlled account or Zone scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareWafMutation {
    CreateManagedRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        request: CloudflareManagedRuleCreateRequest,
    },
    UpdateManagedRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareManagedRuleUpdateRequest,
    },
    WeakenManagedRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareManagedSecurityWeakenRequest,
    },
    DeleteManagedRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareRuleDeleteRequest,
    },
    OrderManagedRules {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareRuleOrderRequest,
    },
    SetManagedRuleException {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        request: CloudflareManagedRuleExceptionRequest,
    },
    CreateCustomRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        request: CloudflareCustomRuleCreateRequest,
    },
    UpdateCustomRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareCustomRuleUpdateRequest,
    },
    DeleteCustomRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareRuleDeleteRequest,
    },
    OrderCustomRules {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareRuleOrderRequest,
    },
    WeakenCustomRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareCustomSecurityWeakenRequest,
    },
    CreateRateLimitRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        request: CloudflareRateLimitRuleCreateRequest,
    },
    UpdateRateLimitRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareRateLimitRuleUpdateRequest,
    },
    DeleteRateLimitRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareRuleDeleteRequest,
    },
    OrderRateLimitRules {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareRuleOrderRequest,
    },
    WeakenRateLimitRule {
        account_id: CloudResourceId,
        zone_id: DnsZoneId,
        rule_id: String,
        request: CloudflareRateLimitSecurityWeakenRequest,
    },
}

impl CloudflareWafMutation {
    fn phase(&self) -> CloudflareWafPhase {
        match self {
            Self::CreateManagedRule { .. }
            | Self::UpdateManagedRule { .. }
            | Self::WeakenManagedRule { .. }
            | Self::DeleteManagedRule { .. }
            | Self::OrderManagedRules { .. }
            | Self::SetManagedRuleException { .. } => CloudflareWafPhase::Managed,
            Self::CreateCustomRule { .. }
            | Self::UpdateCustomRule { .. }
            | Self::DeleteCustomRule { .. }
            | Self::OrderCustomRules { .. }
            | Self::WeakenCustomRule { .. } => CloudflareWafPhase::Custom,
            Self::CreateRateLimitRule { .. }
            | Self::UpdateRateLimitRule { .. }
            | Self::DeleteRateLimitRule { .. }
            | Self::OrderRateLimitRules { .. }
            | Self::WeakenRateLimitRule { .. } => CloudflareWafPhase::RateLimit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareWafMutationResult {
    pub provider_account_id: CloudResourceId,
    pub zone_id: DnsZoneId,
    pub phase: CloudflareWafPhase,
    pub ruleset_id: String,
    pub ruleset_version: String,
    pub rule_id: String,
    pub security_weakening_confirmed: bool,
}

impl CloudflareWafMutationResult {
    fn validate(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        expected_phase: CloudflareWafPhase,
    ) -> bool {
        self.provider_account_id == *account_id
            && self.zone_id == *zone_id
            && self.phase == expected_phase
            && valid_opaque_id(&self.ruleset_id)
            && valid_version(&self.ruleset_version)
            && valid_opaque_id(&self.rule_id)
    }
}

#[async_trait]
pub trait CloudflareWafAdminService: Send + Sync {
    async fn read_inventory(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> Result<CloudflareWafInventoryDto, CloudflareWafAdminError>;

    /// A provider implementation must fresh-read the exact Zone and ruleset before dispatching
    /// one mutation, preserve unowned rules, and return `UnknownOutcome` after ambiguous I/O.
    async fn mutate(
        &self,
        mutation: CloudflareWafMutation,
    ) -> Result<CloudflareWafMutationResult, CloudflareWafAdminError>;
}

/// Sanitized service failures. Provider diagnostic text must never reach HTTP responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudflareWafAdminError {
    InvalidRequest,
    NotFound,
    Conflict,
    EntitlementDenied,
    UnknownOutcome,
    Unavailable,
    InvalidProviderObservation,
}

pub type SharedCloudflareWafAdminService = Arc<dyn CloudflareWafAdminService>;

fn valid_opaque_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_version(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_opaque_action(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn valid_rule_reference(value: &str) -> bool {
    valid_opaque_id(value) && value.len() <= 90 && !value.starts_with("edgion-center-waf:")
}

fn valid_description(value: &str) -> bool {
    !value.is_empty() && value.len() <= 500 && !value.chars().any(char::is_control)
}

fn valid_rate_limit_period(value: u32) -> bool {
    matches!(value, 10 | 60 | 120 | 300 | 600 | 3_600)
}

fn valid_mitigation_timeout(value: u32) -> bool {
    matches!(value, 10 | 30 | 60 | 120 | 300 | 600 | 1_800 | 3_600)
}

fn valid_expression(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_EXPRESSION_LEN && !value.chars().any(char::is_control)
}

fn parse_scope(
    account_id: String,
    zone_id: String,
) -> Result<(CloudResourceId, DnsZoneId), &'static str> {
    let account_id = CloudResourceId::new(account_id).map_err(|_| "invalid_account_id")?;
    let zone_id = DnsZoneId::new(zone_id).map_err(|_| "invalid_zone_id")?;
    let value = zone_id.as_str();
    if value.len() != CLOUDFLARE_ZONE_ID_LEN
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("invalid_zone_id");
    }
    Ok((account_id, zone_id))
}

fn parse_rule_scope(
    account_id: String,
    zone_id: String,
    rule_id: String,
) -> Result<(CloudResourceId, DnsZoneId, String), &'static str> {
    let (account_id, zone_id) = parse_scope(account_id, zone_id)?;
    if !valid_opaque_id(&rule_id) {
        return Err("invalid_rule_id");
    }
    Ok((account_id, zone_id, rule_id))
}

fn error_response(status: StatusCode, code: &'static str) -> Response {
    (status, Json(ApiResponse::<()>::err_body(code.to_string()))).into_response()
}

fn json_rejection_response(rejection: JsonRejection) -> Response {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        error_response(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large")
    } else {
        error_response(StatusCode::BAD_REQUEST, "invalid_request")
    }
}

fn map_service_error(error: CloudflareWafAdminError) -> Response {
    let (status, code) = match error {
        CloudflareWafAdminError::InvalidRequest => (StatusCode::BAD_REQUEST, "invalid_request"),
        CloudflareWafAdminError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        CloudflareWafAdminError::Conflict => (StatusCode::CONFLICT, "conflict"),
        CloudflareWafAdminError::EntitlementDenied => (StatusCode::FORBIDDEN, "entitlement_denied"),
        CloudflareWafAdminError::UnknownOutcome => {
            (StatusCode::SERVICE_UNAVAILABLE, "unknown_outcome")
        }
        CloudflareWafAdminError::Unavailable => {
            (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
        }
        CloudflareWafAdminError::InvalidProviderObservation => (
            StatusCode::SERVICE_UNAVAILABLE,
            "invalid_provider_observation",
        ),
    };
    error_response(status, code)
}

fn service(state: &ApiState) -> Option<&dyn CloudflareWafAdminService> {
    state.cloudflare_waf_admin.as_deref()
}

async fn inventory_response(
    state: ApiState,
    account_id: CloudResourceId,
    zone_id: DnsZoneId,
    phase: Option<CloudflareWafPhase>,
) -> Response {
    let service = match service(&state) {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    match service.read_inventory(&account_id, &zone_id).await {
        Ok(inventory) if inventory.validate(&account_id, &zone_id) => match phase {
            Some(phase) => match inventory.phase(phase) {
                Some(ruleset) => Json(ApiResponse::ok_body(ruleset)).into_response(),
                None => error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response"),
            },
            None => Json(ApiResponse::ok_body(inventory)).into_response(),
        },
        Ok(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response"),
        Err(error) => map_service_error(error),
    }
}

async fn mutation_response(
    state: ApiState,
    account_id: CloudResourceId,
    zone_id: DnsZoneId,
    mutation: CloudflareWafMutation,
) -> Response {
    let service = match service(&state) {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    let expected_phase = mutation.phase();
    match service.mutate(mutation).await {
        Ok(result) if result.validate(&account_id, &zone_id, expected_phase) => {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response"),
        Err(error) => map_service_error(error),
    }
}

pub async fn list_rulesets(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
) -> Response {
    match parse_scope(account_id, zone_id) {
        Ok((account_id, zone_id)) => inventory_response(state, account_id, zone_id, None).await,
        Err(code) => error_response(StatusCode::BAD_REQUEST, code),
    }
}

pub async fn list_managed_rules(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
) -> Response {
    match parse_scope(account_id, zone_id) {
        Ok((account_id, zone_id)) => {
            inventory_response(
                state,
                account_id,
                zone_id,
                Some(CloudflareWafPhase::Managed),
            )
            .await
        }
        Err(code) => error_response(StatusCode::BAD_REQUEST, code),
    }
}

pub async fn list_custom_rules(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
) -> Response {
    match parse_scope(account_id, zone_id) {
        Ok((account_id, zone_id)) => {
            inventory_response(state, account_id, zone_id, Some(CloudflareWafPhase::Custom)).await
        }
        Err(code) => error_response(StatusCode::BAD_REQUEST, code),
    }
}

pub async fn list_rate_limit_rules(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
) -> Response {
    match parse_scope(account_id, zone_id) {
        Ok((account_id, zone_id)) => {
            inventory_response(
                state,
                account_id,
                zone_id,
                Some(CloudflareWafPhase::RateLimit),
            )
            .await
        }
        Err(code) => error_response(StatusCode::BAD_REQUEST, code),
    }
}

pub async fn update_managed_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareManagedRuleUpdateRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::UpdateManagedRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn create_managed_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    body: Result<Json<CloudflareManagedRuleCreateRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id) = match parse_scope(account_id, zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::CreateManagedRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn set_managed_rule_exception(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    body: Result<Json<CloudflareManagedRuleExceptionRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id) = match parse_scope(account_id, zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_security_weaken_confirmation",
        );
    }
    let mutation = CloudflareWafMutation::SetManagedRuleException {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn weaken_managed_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareManagedSecurityWeakenRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_security_weaken_confirmation",
        );
    }
    let mutation = CloudflareWafMutation::WeakenManagedRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn delete_managed_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareRuleDeleteRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_security_weaken_confirmation",
        );
    }
    let mutation = CloudflareWafMutation::DeleteManagedRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn order_managed_rules(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareRuleOrderRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::OrderManagedRules {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn create_custom_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    body: Result<Json<CloudflareCustomRuleCreateRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id) = match parse_scope(account_id, zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::CreateCustomRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn update_custom_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareCustomRuleUpdateRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::UpdateCustomRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn delete_custom_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareRuleDeleteRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::DeleteCustomRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn order_custom_rules(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareRuleOrderRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::OrderCustomRules {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn weaken_custom_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareCustomSecurityWeakenRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_security_weaken_confirmation",
        );
    }
    let mutation = CloudflareWafMutation::WeakenCustomRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn create_rate_limit_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    body: Result<Json<CloudflareRateLimitRuleCreateRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id) = match parse_scope(account_id, zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::CreateRateLimitRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn update_rate_limit_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareRateLimitRuleUpdateRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::UpdateRateLimitRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn delete_rate_limit_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareRuleDeleteRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::DeleteRateLimitRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn order_rate_limit_rules(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareRuleOrderRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mutation = CloudflareWafMutation::OrderRateLimitRules {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

pub async fn weaken_rate_limit_rule(
    State(state): State<ApiState>,
    Path((account_id, zone_id, rule_id)): Path<(String, String, String)>,
    body: Result<Json<CloudflareRateLimitSecurityWeakenRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(value) => value,
        Err(error) => return json_rejection_response(error),
    };
    let (account_id, zone_id, rule_id) = match parse_rule_scope(account_id, zone_id, rule_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    if !request.valid() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_security_weaken_confirmation",
        );
    }
    let mutation = CloudflareWafMutation::WeakenRateLimitRule {
        account_id: account_id.clone(),
        zone_id: zone_id.clone(),
        rule_id,
        request,
    };
    mutation_response(state, account_id, zone_id, mutation).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_dtos_reject_unknown_or_unbounded_values() {
        let unknown = serde_json::from_str::<CloudflareCustomRuleCreateRequest>(
            r#"{"guard":{"rulesetVersion":"1"},"expression":"http.request.uri.path eq \"/\"","action":"block","enabled":true,"extra":true}"#,
        );
        assert!(unknown.is_err());
        let invalid_action = serde_json::from_str::<CloudflareManagedRuleUpdateRequest>(
            r#"{"guard":{"rulesetVersion":"1","ruleVersion":"1"},"action":"execute","enabled":true}"#,
        );
        assert!(invalid_action.is_err());
        assert!(!valid_expression(&"x".repeat(MAX_EXPRESSION_LEN + 1)));
        assert!(!valid_rule_reference(&"r".repeat(91)));

        let first_managed_deployment: CloudflareManagedRuleCreateRequest = serde_json::from_str(
            r#"{"guard":{"rulesetId":null,"rulesetVersion":null},"reference":"managed-1","description":"managed deployment","expression":"http.request.uri.path eq \"/\"","managedRulesetId":"ruleset-1","overrides":[]}"#,
        )
        .unwrap();
        assert!(first_managed_deployment.valid());

        let weakening_on_the_ordinary_write_route: CloudflareCustomRuleCreateRequest =
            serde_json::from_str(
                r#"{"guard":{"rulesetId":null,"rulesetVersion":null},"reference":"custom-1","description":"custom rule","expression":"http.request.uri.path eq \"/\"","action":"log"}"#,
            )
            .unwrap();
        assert!(!weakening_on_the_ordinary_write_route.valid());
    }

    #[test]
    fn weakening_requires_exact_confirmation() {
        let request = CloudflareCustomSecurityWeakenRequest {
            guard: CloudflareWafVersionGuardDto {
                ruleset_id: "entrypoint".to_string(),
                ruleset_version: "v1".to_string(),
            },
            reference: "rule-ref".to_string(),
            description: "test rule".to_string(),
            expression: "http.request.uri.path eq \"/\"".to_string(),
            action: CloudflareWafAction::Log,
            enabled: true,
            confirmation: "yes".to_string(),
        };
        assert!(!request.valid());
    }

    #[test]
    fn orders_are_bounded_and_distinct() {
        let request = CloudflareRuleOrderRequest {
            guard: CloudflareWafVersionGuardDto {
                ruleset_id: "entrypoint".to_string(),
                ruleset_version: "v1".to_string(),
            },
            position: CloudflareWafRulePositionDto::Index { index: 0 },
        };
        assert!(!request.valid());
    }

    #[test]
    fn inventory_validation_fails_closed_on_phase_or_availability_mismatch() {
        let account_id = CloudResourceId::new("cf-main").unwrap();
        let zone_id = DnsZoneId::new("zone-1").unwrap();
        let custom_definition = CloudflareWafRuleDefinitionDto::Custom {
            reference: "custom-1".to_string(),
            description: "custom rule".to_string(),
            expression: "http.request.uri.path eq \"/\"".to_string(),
            action: CloudflareWafAction::Block,
        };
        let mut ruleset = CloudflareWafRulesetDto {
            provider_account_id: account_id.clone(),
            zone_id: zone_id.clone(),
            phase: CloudflareWafPhase::Managed,
            availability: CloudflareWafPhaseAvailability::Available,
            ruleset_id: Some("ruleset-1".to_string()),
            version: Some("v1".to_string()),
            rules: vec![CloudflareWafRuleDto {
                rule_id: "rule-1".to_string(),
                version: "v1".to_string(),
                action: "block".to_string(),
                enabled: true,
                ownership: CloudflareWafOwnership::CenterOwned,
                position: 0,
                definition: Some(custom_definition),
            }],
        };
        assert!(!ruleset.validate(&account_id, &zone_id));

        ruleset.rules.clear();
        ruleset.availability = CloudflareWafPhaseAvailability::PermissionDenied;
        assert!(!ruleset.validate(&account_id, &zone_id));

        ruleset.ruleset_id = None;
        ruleset.version = None;
        assert!(ruleset.validate(&account_id, &zone_id));
    }
}
