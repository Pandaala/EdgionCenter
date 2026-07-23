use std::{
    collections::BTreeSet,
    fmt::{Debug, Formatter},
    net::IpAddr,
    str::FromStr,
};

use edgion_center_core::{NormalizedProviderError, ProviderErrorCategory};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_RULES: usize = 1_500;
pub(crate) const MAX_WEB_ACLS: usize = 10_000;

pub(crate) type Result<T> = std::result::Result<T, NormalizedProviderError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafScope {
    Cloudfront,
    Regional { region: String },
}

impl AwsWafScope {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Cloudfront => Ok(()),
            Self::Regional { region } if valid_region(region) => Ok(()),
            Self::Regional { .. } => Err(validation("invalid_aws_waf_regional_scope")),
        }
    }

    pub fn is_cloudfront(&self) -> bool {
        matches!(self, Self::Cloudfront)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AwsWafWebAclId(String);

impl AwsWafWebAclId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if valid_identifier(&value, 128) {
            Ok(Self(value))
        } else {
            Err(validation("invalid_aws_waf_web_acl_id"))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for AwsWafWebAclId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("AwsWafWebAclId")
            .field(&self.0)
            .finish()
    }
}

/// WAF lock tokens are concurrency authority, not an audit/display value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AwsWafLockToken(String);

impl AwsWafLockToken {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if valid_non_secret_token(&value, 1_024) {
            Ok(Self(value))
        } else {
            Err(validation("invalid_aws_waf_lock_token"))
        }
    }
    /// Opaque optimistic-concurrency value. It may be returned to the Admin
    /// client for a subsequent exact compare, but must remain redacted in
    /// logs and Debug output.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for AwsWafLockToken {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AwsWafLockToken([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafAction {
    Allow,
    Block,
    Count,
    Challenge,
    Captcha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafDefaultAction {
    Allow,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafVisibilityConfig {
    pub cloudwatch_metrics_enabled: bool,
    pub sampled_requests_enabled: bool,
    pub metric_name: String,
}

impl AwsWafVisibilityConfig {
    pub fn validate(&self) -> Result<()> {
        if valid_identifier(&self.metric_name, 128) {
            Ok(())
        } else {
            Err(validation("invalid_aws_waf_metric_name"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafManagedRuleGroup {
    pub vendor_name: String,
    pub name: String,
    pub version: Option<String>,
    pub capacity: u32,
    #[serde(default)]
    pub excluded_rules: BTreeSet<String>,
    #[serde(default)]
    pub rule_action_overrides: Vec<AwsWafManagedRuleOverride>,
}

impl AwsWafManagedRuleGroup {
    pub fn validate(&self) -> Result<()> {
        if !valid_identifier(&self.vendor_name, 128)
            || !valid_identifier(&self.name, 128)
            || self
                .version
                .as_deref()
                .is_some_and(|value| !valid_identifier(value, 128))
            || self.capacity == 0
            || self.capacity > 5_000
            || self.excluded_rules.len() > 100
            || self
                .excluded_rules
                .iter()
                .any(|value| !valid_identifier(value, 128))
            || self.rule_action_overrides.len() > 100
        {
            return Err(validation("invalid_aws_waf_managed_rule_group"));
        }
        for override_value in &self.rule_action_overrides {
            override_value.validate()?;
        }
        Ok(())
    }
    pub(crate) fn key(&self) -> String {
        format!("{}/{}", self.vendor_name, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafManagedRuleOverride {
    pub name: String,
    pub action: AwsWafAction,
}
impl AwsWafManagedRuleOverride {
    fn validate(&self) -> Result<()> {
        if valid_identifier(&self.name, 128) {
            Ok(())
        } else {
            Err(validation("invalid_aws_waf_managed_rule_override"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafIpSetReference {
    pub arn: String,
}
impl AwsWafIpSetReference {
    fn validate(&self, scope: &AwsWafScope, account: &str) -> Result<()> {
        validate_waf_arn(&self.arn, scope, account, "ipset")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafIpAddressVersion {
    Ipv4,
    Ipv6,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AwsWafIpSetId(String);
impl AwsWafIpSetId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if valid_identifier(&value, 128) {
            Ok(Self(value))
        } else {
            Err(validation("invalid_aws_waf_ip_set_id"))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Debug for AwsWafIpSetId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AwsWafIpSetId").field(&self.0).finish()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafIpSet {
    pub id: AwsWafIpSetId,
    pub name: String,
    pub arn: String,
    pub scope: AwsWafScope,
    pub address_version: AwsWafIpAddressVersion,
    pub addresses: BTreeSet<String>,
    pub lock_token: AwsWafLockToken,
}
impl AwsWafIpSet {
    pub fn revision(&self) -> AwsWafIpSetRevision {
        AwsWafIpSetRevision {
            id: self.id.clone(),
            scope: self.scope.clone(),
            lock_token: self.lock_token.clone(),
        }
    }

    pub fn validate(&self, account: &str) -> Result<()> {
        self.scope.validate()?;
        if !valid_identifier(&self.name, 128)
            || self.addresses.len() > 10_000
            || self
                .addresses
                .iter()
                .any(|address| !valid_ip_set_address(address, self.address_version))
        {
            return Err(validation("invalid_aws_waf_ip_set"));
        }
        validate_waf_arn(&self.arn, &self.scope, account, "ipset")
    }
}

fn valid_ip_set_address(value: &str, version: AwsWafIpAddressVersion) -> bool {
    let Some((address, prefix)) = value.split_once('/') else {
        return false;
    };
    if address.is_empty() || prefix.is_empty() || prefix.starts_with('+') || prefix.len() > 3 {
        return false;
    }
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    match (IpAddr::from_str(address), version) {
        (Ok(IpAddr::V4(_)), AwsWafIpAddressVersion::Ipv4) => prefix <= 32,
        (Ok(IpAddr::V6(_)), AwsWafIpAddressVersion::Ipv6) => prefix <= 128,
        _ => false,
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafIpSetRevision {
    pub id: AwsWafIpSetId,
    pub scope: AwsWafScope,
    pub lock_token: AwsWafLockToken,
}
pub struct AwsWafIpSetPage {
    pub items: Vec<AwsWafIpSet>,
    pub next_marker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafRateAggregateKey {
    Ip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AwsWafStatement {
    ManagedRuleGroup(AwsWafManagedRuleGroup),
    IpSetReference(AwsWafIpSetReference),
    RateBased {
        limit: u32,
        aggregate_key: AwsWafRateAggregateKey,
        scope_down_ip_set: Option<AwsWafIpSetReference>,
    },
}

impl AwsWafStatement {
    fn validate(&self, scope: &AwsWafScope, account: &str) -> Result<()> {
        match self {
            Self::ManagedRuleGroup(value) => value.validate(),
            Self::IpSetReference(value) => value.validate(scope, account),
            Self::RateBased {
                limit,
                scope_down_ip_set,
                ..
            } => {
                if !(100..=20_000_000).contains(limit) {
                    return Err(validation("invalid_aws_waf_rate_limit"));
                }
                if let Some(ip_set) = scope_down_ip_set {
                    ip_set.validate(scope, account)?;
                }
                Ok(())
            }
        }
    }
    pub(crate) fn capacity(&self) -> u32 {
        match self {
            Self::ManagedRuleGroup(value) => value.capacity,
            Self::IpSetReference(_) => 1,
            Self::RateBased {
                scope_down_ip_set, ..
            } => 2 + u32::from(scope_down_ip_set.is_some()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AwsWafRuleOwner {
    Center {
        reference: String,
    },
    #[default]
    External,
}

/// The only two AWS WAF rule-group override actions. This is kept separate
/// from a normal rule action because managed rule groups cannot use `Action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafManagedRuleOverrideAction {
    None,
    Count,
}

impl AwsWafRuleOwner {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Center { reference } if valid_identifier(reference, 128) => Ok(()),
            Self::External => Ok(()),
            _ => Err(validation("invalid_aws_waf_rule_owner")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafRule {
    pub name: String,
    pub priority: u32,
    pub action: AwsWafAction,
    pub statement: AwsWafStatement,
    pub visibility: AwsWafVisibilityConfig,
    /// Provider ownership is observed or constructed by a trusted server-side
    /// mapper. It is never accepted from an Admin request body.
    #[serde(skip_deserializing, default)]
    pub owner: AwsWafRuleOwner,
    /// Observed only for managed-rule-group statements. It is server-owned so
    /// untrusted request DTOs cannot claim a provider ownership mode.
    #[serde(skip_deserializing, default)]
    pub managed_override_action: Option<AwsWafManagedRuleOverrideAction>,
}

impl AwsWafRule {
    fn validate(
        &self,
        scope: &AwsWafScope,
        account: &str,
        capability: &AwsWafCapabilityEvidence,
    ) -> Result<()> {
        if !valid_identifier(&self.name, 128) {
            return Err(validation("invalid_aws_waf_rule_name"));
        }
        self.visibility.validate()?;
        self.owner.validate()?;
        self.statement.validate(scope, account)?;
        match (&self.statement, self.managed_override_action) {
            (AwsWafStatement::ManagedRuleGroup(_), Some(_)) => {}
            (AwsWafStatement::ManagedRuleGroup(_), None) => {
                return Err(validation("aws_waf_managed_rule_override_missing"));
            }
            (_, None) => {}
            (_, Some(_)) => return Err(validation("invalid_aws_waf_rule_override_action")),
        }
        if matches!(self.action, AwsWafAction::Challenge) && !capability.challenge_allowed {
            return Err(authorization("aws_waf_challenge_not_entitled"));
        }
        if matches!(self.action, AwsWafAction::Captcha) && !capability.captcha_allowed {
            return Err(authorization("aws_waf_captcha_not_entitled"));
        }
        if let AwsWafStatement::ManagedRuleGroup(group) = &self.statement {
            if !capability.managed_rule_groups.contains(&group.key()) {
                return Err(authorization("aws_waf_managed_rule_group_not_entitled"));
            }
        }
        Ok(())
    }
    pub(crate) fn center_reference(&self) -> Option<&str> {
        match &self.owner {
            AwsWafRuleOwner::Center { reference } => Some(reference),
            AwsWafRuleOwner::External => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafCapabilityEvidence {
    #[serde(default)]
    pub managed_rule_groups: BTreeSet<String>,
    pub challenge_allowed: bool,
    pub captcha_allowed: bool,
    pub maximum_wcu: u32,
}

impl AwsWafCapabilityEvidence {
    pub fn validate(&self) -> Result<()> {
        if self.maximum_wcu > 0 && self.maximum_wcu <= 5_000 {
            Ok(())
        } else {
            Err(validation("invalid_aws_waf_capacity_evidence"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafWebAcl {
    pub id: AwsWafWebAclId,
    pub name: String,
    pub arn: String,
    pub scope: AwsWafScope,
    pub default_action: AwsWafDefaultAction,
    pub visibility: AwsWafVisibilityConfig,
    pub rules: Vec<AwsWafRule>,
    pub capacity: u32,
    pub lock_token: AwsWafLockToken,
}

impl AwsWafWebAcl {
    pub fn validate(&self, account: &str, capability: &AwsWafCapabilityEvidence) -> Result<()> {
        self.scope.validate()?;
        if !valid_identifier(&self.name, 128) || self.rules.len() > MAX_RULES {
            return Err(validation("invalid_aws_waf_web_acl"));
        }
        self.visibility.validate()?;
        validate_waf_arn(&self.arn, &self.scope, account, "webacl")?;
        let mut priorities = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut references = BTreeSet::new();
        let mut computed_capacity = 0_u32;
        for rule in &self.rules {
            rule.validate(&self.scope, account, capability)?;
            if !priorities.insert(rule.priority) {
                return Err(conflict("aws_waf_rule_priority_collision"));
            }
            if !names.insert(rule.name.clone()) {
                return Err(conflict("aws_waf_rule_name_collision"));
            }
            if let Some(reference) = rule.center_reference() {
                if !references.insert(reference.to_string()) {
                    return Err(conflict("aws_waf_center_rule_reference_collision"));
                }
            }
            computed_capacity = computed_capacity
                .checked_add(rule.statement.capacity())
                .ok_or_else(|| quota("aws_waf_capacity_overflow"))?;
        }
        // AWS only exposes an authoritative aggregate WCU for an ACL. It does
        // not expose a lossless per-managed-group WCU split, so equality can
        // only be checked locally when no managed group is present. Writes
        // still call provider CheckCapacity before dispatch.
        let has_managed_group = self
            .rules
            .iter()
            .any(|rule| matches!(rule.statement, AwsWafStatement::ManagedRuleGroup(_)));
        if !has_managed_group && self.capacity != computed_capacity {
            return Err(conflict("aws_waf_capacity_mismatch"));
        }
        let capacity_to_check = if has_managed_group {
            // The provider returned this aggregate WCU. The integration calls
            // CheckCapacity for every proposed write; never substitute the
            // synthetic per-group placeholder for that provider result.
            self.capacity
        } else {
            computed_capacity
        };
        if capacity_to_check > capability.maximum_wcu {
            return Err(quota("aws_waf_capacity_exceeded"));
        }
        Ok(())
    }
    pub fn revision(&self) -> AwsWafWebAclRevision {
        AwsWafWebAclRevision {
            id: self.id.clone(),
            scope: self.scope.clone(),
            lock_token: self.lock_token.clone(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafWebAclRevision {
    pub id: AwsWafWebAclId,
    pub scope: AwsWafScope,
    pub lock_token: AwsWafLockToken,
}
impl Debug for AwsWafWebAclRevision {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsWafWebAclRevision")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("lock_token", &"[REDACTED]")
            .finish()
    }
}
impl AwsWafWebAclRevision {
    pub fn validate(&self) -> Result<()> {
        self.scope.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsWafWebAclPage {
    pub items: Vec<AwsWafWebAcl>,
    pub next_marker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafCreateWebAclRequest {
    pub name: String,
    pub scope: AwsWafScope,
    pub default_action: AwsWafDefaultAction,
    pub visibility: AwsWafVisibilityConfig,
    #[serde(default)]
    pub rules: Vec<AwsWafRule>,
    /// Entitlement and capacity are trusted provider observations. An Admin
    /// caller cannot provide its own evidence.
    #[serde(skip_deserializing, default)]
    pub capability: AwsWafCapabilityEvidence,
}

impl AwsWafCreateWebAclRequest {
    pub fn to_unbound_acl(&self) -> Result<AwsWafWebAcl> {
        self.scope.validate()?;
        self.capability.validate()?;
        if !valid_identifier(&self.name, 128) || self.rules.len() > MAX_RULES {
            return Err(validation("invalid_aws_waf_create_request"));
        }
        self.visibility.validate()?;
        Ok(AwsWafWebAcl {
            id: AwsWafWebAclId::new("unbound")?,
            name: self.name.clone(),
            arn: "unbound".to_string(),
            scope: self.scope.clone(),
            default_action: self.default_action,
            visibility: self.visibility.clone(),
            capacity: capacity(&self.rules)?,
            lock_token: AwsWafLockToken::new("unbound")?,
            rules: self.rules.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafCenterRulesUpdate {
    pub revision: AwsWafWebAclRevision,
    pub default_action: Option<AwsWafDefaultAction>,
    #[serde(default)]
    pub rules: Vec<AwsWafRule>,
    /// Entitlement and capacity are trusted provider observations. An Admin
    /// caller cannot provide its own evidence.
    #[serde(skip_deserializing, default)]
    pub capability: AwsWafCapabilityEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafAssociationTarget {
    pub resource_arn: String,
    pub resource_kind: AwsWafRegionalResourceKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafRegionalResourceKind {
    ApplicationLoadBalancer,
    ApiGatewayStage,
    AppSyncApi,
    CognitoUserPool,
}
impl AwsWafAssociationTarget {
    pub fn validate(&self, scope: &AwsWafScope, account: &str) -> Result<()> {
        let AwsWafScope::Regional { region } = scope else {
            return Err(validation("aws_waf_regional_association_scope_required"));
        };
        let expected_service = match self.resource_kind {
            AwsWafRegionalResourceKind::ApplicationLoadBalancer => "elasticloadbalancing",
            AwsWafRegionalResourceKind::ApiGatewayStage => "apigateway",
            AwsWafRegionalResourceKind::AppSyncApi => "appsync",
            AwsWafRegionalResourceKind::CognitoUserPool => "cognito-idp",
        };
        let parts: Vec<_> = self.resource_arn.split(':').collect();
        if parts.len() != 6
            || parts[0] != "arn"
            || !matches!(parts[1], "aws" | "aws-cn" | "aws-us-gov")
            || parts[2] != expected_service
            || parts[3] != region
            || parts[4] != account
            || !valid_regional_resource_shape(self.resource_kind, parts[5])
        {
            return Err(validation("invalid_aws_waf_regional_association_target"));
        }
        Ok(())
    }
}

fn valid_regional_resource_shape(kind: AwsWafRegionalResourceKind, resource: &str) -> bool {
    match kind {
        AwsWafRegionalResourceKind::ApplicationLoadBalancer => {
            resource.starts_with("loadbalancer/app/") && resource.len() > "loadbalancer/app/".len()
        }
        AwsWafRegionalResourceKind::ApiGatewayStage => {
            (resource.starts_with("/restapis/") || resource.starts_with("/apis/"))
                && resource.contains("/stages/")
        }
        AwsWafRegionalResourceKind::AppSyncApi => {
            resource.starts_with("apis/") && resource.len() > "apis/".len()
        }
        AwsWafRegionalResourceKind::CognitoUserPool => {
            resource.starts_with("userpool/") && resource.len() > "userpool/".len()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AwsWafAssociation {
    pub target: AwsWafAssociationTarget,
    pub web_acl_id: AwsWafWebAclId,
}

pub(crate) fn capacity(rules: &[AwsWafRule]) -> Result<u32> {
    rules.iter().try_fold(0_u32, |total, rule| {
        total
            .checked_add(rule.statement.capacity())
            .ok_or_else(|| quota("aws_waf_capacity_overflow"))
    })
}

pub(crate) fn validate_waf_arn(
    arn: &str,
    scope: &AwsWafScope,
    account: &str,
    expected_kind: &str,
) -> Result<()> {
    let parts: Vec<_> = arn.split(':').collect();
    if parts.len() != 6
        || parts[0] != "arn"
        || !matches!(parts[1], "aws" | "aws-cn" | "aws-us-gov")
        || parts[2] != "wafv2"
        || parts[4] != account
    {
        return Err(validation("invalid_aws_waf_resource_arn"));
    }
    let (expected_region, expected_scope) = match scope {
        AwsWafScope::Cloudfront => ("us-east-1", "global"),
        AwsWafScope::Regional { region } => (region.as_str(), "regional"),
    };
    if parts[3] != expected_region
        || !parts[5].starts_with(&format!("{expected_scope}/{expected_kind}/"))
    {
        return Err(validation("aws_waf_resource_scope_mismatch"));
    }
    Ok(())
}

fn valid_region(value: &str) -> bool {
    value.len() >= 3
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value.contains('-')
}
fn valid_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
fn valid_non_secret_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
pub(crate) fn provider_error(
    category: ProviderErrorCategory,
    code: &str,
) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        code,
        "Sanitized AWS WAF adapter error",
        None,
        None,
    )
    .expect("fixed provider error")
}
pub(crate) fn validation(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Validation, code)
}
pub(crate) fn authorization(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Authorization, code)
}
pub(crate) fn conflict(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Conflict, code)
}
pub(crate) fn quota(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Quota, code)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsWafManagedRuleGroupCatalogEntry {
    pub vendor_name: String,
    pub name: String,
    pub versions: BTreeSet<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsWafCapacityObservation {
    pub scope: AwsWafScope,
    pub required_wcu: u32,
    pub account_wcu_ceiling: u32,
}
impl AwsWafCapacityObservation {
    pub fn authorize(&self) -> Result<()> {
        self.scope.validate()?;
        if self.account_wcu_ceiling > 0 && self.required_wcu <= self.account_wcu_ceiling {
            Ok(())
        } else {
            Err(quota("aws_waf_check_capacity_exceeded"))
        }
    }
}
