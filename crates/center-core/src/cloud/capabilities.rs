//! Provider-neutral capability discovery and fail-closed evaluation.
//!
//! Capability identifiers and evaluation dimensions are closed enums. Provider
//! adapters may attach bounded diagnostics, but they cannot add string-keyed
//! capabilities or opaque vendor payloads to the core contract.

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    CloudProvider, CloudResourceId, CloudResourceKind, CoreError, CoreResult, ProviderAccountSpec,
    ProviderResourceRef,
};

pub const CAPABILITY_CONTRACT_VERSION: u16 = 1;

const MAX_REGION_LEN: usize = 128;
const MAX_REVISION_LEN: usize = 512;
const MAX_DISCOVERY_TOKEN_LEN: usize = 512;
const MAX_EXTERNAL_ID_LEN: usize = 1024;
const MAX_DIAGNOSTIC_CODE_LEN: usize = 128;
const MAX_DIAGNOSTIC_MESSAGE_LEN: usize = 2048;
const MAX_CAPABILITIES_PER_REPORT: usize = 256;
const MAX_ISSUES_PER_REPORT: usize = 128;
const MAX_SNAPSHOT_SERIALIZED_BYTES: usize = 512 * 1024;

macro_rules! sanitized_diagnostic {
    ($name:ident, $kind:literal, $max_len:expr) => {
        /// Structurally sanitized diagnostic text. This enforces bounds,
        /// trimming, and control-character rules; it cannot identify every
        /// possible credential or secret supplied by an adapter.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> CoreResult<Self> {
                let value = value.into();
                validate_bounded_text(&value, $kind, $max_len)?;
                Ok(Self(value))
            }

            pub fn validate(&self) -> CoreResult<()> {
                Self::new(self.0.clone()).map(|_| ())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

sanitized_diagnostic!(
    SanitizedCapabilityCode,
    "capability diagnostic code",
    MAX_DIAGNOSTIC_CODE_LEN
);
sanitized_diagnostic!(
    SanitizedCapabilityMessage,
    "capability diagnostic message",
    MAX_DIAGNOSTIC_MESSAGE_LEN
);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderRegion(String);

impl ProviderRegion {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        validate_bounded_text(&value, "provider region", MAX_REGION_LEN)?;
        Ok(Self(value))
    }

    pub fn validate(&self) -> CoreResult<()> {
        Self::new(self.0.clone()).map(|_| ())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ProviderRegion {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsCapability {
    PublicZones,
    PrivateZones,
    RecordSets,
    Dnssec,
    ApexAlias,
    ProxiedRecords,
    WeightedRouting,
    GeolocationRouting,
    FailoverRouting,
    AtomicChanges,
}

/// Provider-neutral WAF capabilities. Provider-specific rule expressions and
/// protected-target identifiers remain outside the core contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WafCapability {
    ManagedRules,
    CustomRules,
    RateLimiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "family", content = "name", rename_all = "snake_case")]
pub enum ProviderCapability {
    Dns(DnsCapability),
    Waf(WafCapability),
}

/// Identifies persisted snapshots that advertise retired capability families.
/// Stores treat these payloads as unavailable during upgrade rather than
/// re-exposing their retired variants through the current API.
pub fn is_retired_capability_snapshot_json(value: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(value) else {
        return false;
    };
    contains_retired_capability_family(&value)
}

fn contains_retired_capability_family(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(values) => {
            matches!(
                values.get("family").and_then(serde_json::Value::as_str),
                Some("certificate" | "edge" | "health_check" | "cache")
            ) || values.values().any(contains_retired_capability_family)
        }
        serde_json::Value::Array(values) => values.iter().any(contains_retired_capability_family),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAction {
    Observe,
    Create,
    Update,
    Delete,
    Execute,
}

impl CapabilityAction {
    const ALL: [Self; 5] = [
        Self::Observe,
        Self::Create,
        Self::Update,
        Self::Delete,
        Self::Execute,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRequirement {
    pub capability: ProviderCapability,
    pub action: CapabilityAction,
}

/// Scope at which an entitlement was observed. Account observations must not
/// be reused for region- or resource-scoped provider features.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CapabilityScope {
    Account,
    Region {
        region: ProviderRegion,
    },
    Resource {
        resource_kind: CloudResourceKind,
        resource: ProviderResourceRef,
    },
}

impl CapabilityScope {
    fn validate_for_account(&self, account_id: &CloudResourceId) -> CoreResult<()> {
        match self {
            Self::Account => Ok(()),
            Self::Region { region } => region.validate(),
            Self::Resource {
                resource_kind: _,
                resource,
            } => {
                resource.provider_account_id.validate()?;
                validate_bounded_text(
                    &resource.external_id,
                    "provider external resource",
                    MAX_EXTERNAL_ID_LEN,
                )?;
                if &resource.provider_account_id != account_id {
                    return Err(CoreError::Conflict(
                        "capability resource scope belongs to a different provider account"
                            .to_string(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Every mutation gate checks all dimensions. This prevents an adapter's
/// implementation support from being mistaken for provider availability,
/// account entitlement, caller access, location support, or remaining quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDimension {
    AdapterSupport,
    ProviderSupport,
    Entitlement,
    Access,
    Location,
    Quota,
}

impl CapabilityDimension {
    const ACTION_INDEPENDENT: [Self; 4] = [
        Self::AdapterSupport,
        Self::ProviderSupport,
        Self::Entitlement,
        Self::Location,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriState {
    Affirmative,
    Negative,
    Unknown,
    NotApplicable,
}

/// Stable reason taxonomy. Provider-specific response text belongs only in a
/// bounded, sanitized diagnostic and never changes the semantic reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityReason {
    AdapterNotImplemented,
    ProviderUnsupported,
    NotEntitled,
    AuthenticationFailed,
    DiscoveryPermissionDenied,
    BusinessPermissionDenied,
    LocationUnavailable,
    QuotaExhausted,
    NotDiscovered,
    ProbeFailed,
    ProviderUnavailable,
    InvalidProviderResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityEvidence {
    AdapterContract,
    ProviderMetadata,
    ProviderProbe,
    PermissionProbe,
    QuotaProbe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDimensionObservation {
    pub dimension: CapabilityDimension,
    /// Access and quota observations are action-specific. Other dimensions
    /// must leave this empty.
    pub action: Option<CapabilityAction>,
    pub state: TriState,
    pub reason: Option<CapabilityReason>,
    pub evidence: CapabilityEvidence,
    pub code: Option<SanitizedCapabilityCode>,
    pub message: Option<SanitizedCapabilityMessage>,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
}

impl CapabilityDimensionObservation {
    pub fn validate(&self) -> CoreResult<()> {
        if self.observed_at_unix_ms <= 0 || self.valid_until_unix_ms <= self.observed_at_unix_ms {
            return Err(CoreError::Conflict(
                "capability observation freshness window is invalid".to_string(),
            ));
        }
        if matches!(self.state, TriState::Affirmative | TriState::NotApplicable)
            && self.reason.is_some()
        {
            return Err(CoreError::Conflict(
                "affirmative capability observation must not contain a failure reason".to_string(),
            ));
        }
        if matches!(self.state, TriState::Negative | TriState::Unknown) && self.reason.is_none() {
            return Err(CoreError::Conflict(
                "non-affirmative capability observation requires a stable reason".to_string(),
            ));
        }
        if !reason_matches_dimension(self.dimension, self.state, self.reason) {
            return Err(CoreError::Conflict(
                "capability reason does not match its dimension and state".to_string(),
            ));
        }
        let action_is_valid = match self.dimension {
            CapabilityDimension::Access | CapabilityDimension::Quota => self.action.is_some(),
            _ => self.action.is_none(),
        };
        if !action_is_valid
            || (self.state == TriState::NotApplicable
                && self.dimension != CapabilityDimension::Quota)
        {
            return Err(CoreError::Conflict(
                "capability action or applicability is invalid for its dimension".to_string(),
            ));
        }
        if !evidence_matches_dimension(self.dimension, self.evidence) {
            return Err(CoreError::Conflict(
                "capability evidence is invalid for its dimension".to_string(),
            ));
        }
        self.code
            .as_ref()
            .map_or(Ok(()), |value| value.validate())?;
        self.message
            .as_ref()
            .map_or(Ok(()), |value| value.validate())
    }

    pub fn is_fresh_at(&self, now_unix_ms: i64) -> bool {
        now_unix_ms >= self.observed_at_unix_ms && now_unix_ms < self.valid_until_unix_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityObservation {
    pub capability: ProviderCapability,
    pub dimensions: Vec<CapabilityDimensionObservation>,
}

impl CapabilityObservation {
    pub fn validate(&self) -> CoreResult<()> {
        let maximum_dimensions =
            CapabilityDimension::ACTION_INDEPENDENT.len() + CapabilityAction::ALL.len() * 2;
        if self.dimensions.len() > maximum_dimensions {
            return Err(CoreError::Conflict(
                "capability observation has too many dimensions".to_string(),
            ));
        }
        let mut dimensions = BTreeSet::new();
        for observation in &self.dimensions {
            observation.validate()?;
            if !dimensions.insert((observation.dimension, observation.action)) {
                return Err(CoreError::Conflict(
                    "capability observation contains duplicate dimensions".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDiscoveryState {
    Complete,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityIssueSeverity {
    Warning,
    Blocking,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CapabilityIssueScope {
    Account,
    Requirement {
        requirement: CapabilityRequirement,
    },
    Dimension {
        requirement: CapabilityRequirement,
        dimension: CapabilityDimension,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDiscoveryIssue {
    pub severity: CapabilityIssueSeverity,
    pub scope: CapabilityIssueScope,
    pub reason: CapabilityReason,
    pub code: SanitizedCapabilityCode,
    pub message: SanitizedCapabilityMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiscoveryToken(String);

impl DiscoveryToken {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        validate_bounded_text(
            &value,
            "capability discovery token",
            MAX_DISCOVERY_TOKEN_LEN,
        )?;
        Ok(Self(value))
    }

    pub fn validate(&self) -> CoreResult<()> {
        Self::new(self.0.clone()).map(|_| ())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDiscoveryFence {
    pub provider_account_generation: u64,
    pub credential_revision: Option<String>,
    /// Monotonically increasing epoch allocated by the discovery orchestrator.
    pub discovery_epoch: u64,
    /// Opaque ownership token disambiguating the claimant of an epoch.
    pub discovery_token: DiscoveryToken,
}

impl CapabilityDiscoveryFence {
    pub fn validate(&self) -> CoreResult<()> {
        if self.provider_account_generation == 0
            || self.provider_account_generation > i64::MAX as u64
            || self.discovery_epoch == 0
            || self.discovery_epoch > i64::MAX as u64
        {
            return Err(CoreError::Conflict(
                "capability discovery fence is invalid".to_string(),
            ));
        }
        validate_optional_bounded_text(
            self.credential_revision.as_deref(),
            "credential revision",
            MAX_REVISION_LEN,
        )?;
        self.discovery_token.validate()
    }
}

impl CapabilityDiscoveryIssue {
    pub fn validate(&self) -> CoreResult<()> {
        self.code.validate()?;
        self.message.validate()?;
        let required_scope = match self.reason {
            CapabilityReason::AuthenticationFailed => {
                self.severity == CapabilityIssueSeverity::Blocking
                    && self.scope == CapabilityIssueScope::Account
            }
            CapabilityReason::BusinessPermissionDenied => {
                self.severity == CapabilityIssueSeverity::Blocking
                    && matches!(
                        self.scope,
                        CapabilityIssueScope::Requirement { .. }
                            | CapabilityIssueScope::Dimension { .. }
                    )
            }
            _ => true,
        };
        if !required_scope {
            return Err(CoreError::Conflict(
                "capability issue severity or scope is invalid for its reason".to_string(),
            ));
        }
        if let CapabilityIssueScope::Dimension { dimension, .. } = self.scope {
            if !issue_reason_matches_dimension(self.reason, dimension) {
                return Err(CoreError::Conflict(
                    "capability issue reason does not match its dimension".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDiscoveryRequest {
    pub provider_account_id: CloudResourceId,
    pub fence: CapabilityDiscoveryFence,
    pub account: ProviderAccountSpec,
    pub scope: CapabilityScope,
}

impl CapabilityDiscoveryRequest {
    pub fn validate(&self) -> CoreResult<()> {
        self.provider_account_id.validate()?;
        self.fence.validate()?;
        self.account.validate()?;
        self.scope.validate_for_account(&self.provider_account_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDiscoveryReport {
    pub state: CapabilityDiscoveryState,
    #[serde(default)]
    pub observations: Vec<CapabilityObservation>,
    #[serde(default)]
    pub issues: Vec<CapabilityDiscoveryIssue>,
}

impl CapabilityDiscoveryReport {
    pub fn validate(&self) -> CoreResult<()> {
        validate_observations_and_issues(&self.observations, &self.issues)?;
        match self.state {
            CapabilityDiscoveryState::Complete if !self.issues.is_empty() => Err(
                CoreError::Conflict("complete capability discovery cannot contain issues".into()),
            ),
            CapabilityDiscoveryState::Failed if self.issues.is_empty() => Err(CoreError::Conflict(
                "failed capability discovery requires an issue".into(),
            )),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilitySnapshot {
    /// Internal persistence/coordination version. Public API DTOs must not
    /// expose the fence token or opaque credential revision nested here.
    pub contract_version: u16,
    pub provider_account_id: CloudResourceId,
    pub provider: CloudProvider,
    pub fence: CapabilityDiscoveryFence,
    pub scope: CapabilityScope,
    pub state: CapabilityDiscoveryState,
    pub discovered_at_unix_ms: i64,
    #[serde(default)]
    pub observations: Vec<CapabilityObservation>,
    #[serde(default)]
    pub issues: Vec<CapabilityDiscoveryIssue>,
}

impl ProviderCapabilitySnapshot {
    pub fn from_report(
        request: &CapabilityDiscoveryRequest,
        discovered_at_unix_ms: i64,
        report: CapabilityDiscoveryReport,
    ) -> CoreResult<Self> {
        request.validate()?;
        report.validate()?;
        let snapshot = Self {
            contract_version: CAPABILITY_CONTRACT_VERSION,
            provider_account_id: request.provider_account_id.clone(),
            provider: request.account.provider.clone(),
            fence: request.fence.clone(),
            scope: request.scope.clone(),
            state: report.state,
            discovered_at_unix_ms,
            observations: report.observations,
            issues: report.issues,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> CoreResult<()> {
        if self.contract_version != CAPABILITY_CONTRACT_VERSION || self.discovered_at_unix_ms <= 0 {
            return Err(CoreError::Conflict(
                "provider capability snapshot metadata is invalid".to_string(),
            ));
        }
        self.provider_account_id.validate()?;
        self.fence.validate()?;
        self.scope.validate_for_account(&self.provider_account_id)?;
        validate_observations_and_issues(&self.observations, &self.issues)?;
        match self.state {
            CapabilityDiscoveryState::Complete if !self.issues.is_empty() => Err(
                CoreError::Conflict("complete capability snapshot cannot contain issues".into()),
            ),
            CapabilityDiscoveryState::Failed if self.issues.is_empty() => Err(CoreError::Conflict(
                "failed capability snapshot requires an issue".into(),
            )),
            _ => Ok(()),
        }?;
        let serialized_size = serde_json::to_vec(self)
            .map_err(|_| CoreError::Conflict("capability snapshot cannot be serialized".into()))?
            .len();
        if serialized_size > MAX_SNAPSHOT_SERIALIZED_BYTES {
            return Err(CoreError::Conflict(
                "provider capability snapshot exceeds its serialized size limit".into(),
            ));
        }
        Ok(())
    }

    /// Evaluates mutation requirements without I/O. Only a fresh affirmative
    /// observation for every dimension produces `Allowed`.
    pub fn evaluate(
        &self,
        required: impl IntoIterator<Item = CapabilityRequirement>,
        context: &CapabilityEvaluationContext<'_>,
    ) -> Vec<CapabilityDecision> {
        let required: BTreeSet<_> = required.into_iter().collect();
        required
            .into_iter()
            .map(|requirement| self.evaluate_one(requirement, context))
            .collect()
    }

    fn evaluate_one(
        &self,
        requirement: CapabilityRequirement,
        context: &CapabilityEvaluationContext<'_>,
    ) -> CapabilityDecision {
        let snapshot_reason = if self.validate().is_err() {
            Some(CapabilityGateReason::SnapshotInvalid)
        } else if &self.provider_account_id != context.provider_account_id {
            Some(CapabilityGateReason::ProviderAccountMismatch)
        } else if self.provider != context.provider {
            Some(CapabilityGateReason::ProviderMismatch)
        } else if self.fence.provider_account_generation != context.provider_account_generation {
            Some(CapabilityGateReason::AccountGenerationMismatch)
        } else if self.fence.credential_revision.as_deref() != context.credential_revision {
            Some(CapabilityGateReason::CredentialRevisionMismatch)
        } else if &self.scope != context.scope {
            Some(CapabilityGateReason::ScopeMismatch)
        } else if self.state == CapabilityDiscoveryState::Failed {
            Some(CapabilityGateReason::DiscoveryFailed)
        } else {
            None
        };
        if let Some(reason) = snapshot_reason {
            return CapabilityDecision::indeterminate(requirement, reason, None);
        }

        if let Some(issue) = self.issues.iter().find(|issue| {
            issue.severity == CapabilityIssueSeverity::Blocking
                && match &issue.scope {
                    CapabilityIssueScope::Account => true,
                    CapabilityIssueScope::Requirement {
                        requirement: affected,
                    } => affected == &requirement,
                    CapabilityIssueScope::Dimension {
                        requirement: affected,
                        ..
                    } => affected == &requirement,
                }
        }) {
            let dimension = match issue.scope {
                CapabilityIssueScope::Dimension { dimension, .. } => Some(dimension),
                _ => None,
            };
            return CapabilityDecision {
                requirement,
                outcome: CapabilityDecisionOutcome::Indeterminate,
                blockers: vec![CapabilityGateBlocker {
                    dimension,
                    reason: CapabilityGateReason::BlockingDiscoveryIssue,
                    diagnostic_reason: Some(issue.reason),
                    code: Some(issue.code.clone()),
                    message: Some(issue.message.clone()),
                }],
            };
        }

        let Some(observation) = self
            .observations
            .iter()
            .find(|observation| observation.capability == requirement.capability)
        else {
            return CapabilityDecision::indeterminate(
                requirement,
                CapabilityGateReason::CapabilityNotReported,
                None,
            );
        };

        let mut blockers = Vec::new();
        let required_dimensions = CapabilityDimension::ACTION_INDEPENDENT
            .into_iter()
            .map(|dimension| (dimension, None))
            .chain([
                (CapabilityDimension::Access, Some(requirement.action)),
                (CapabilityDimension::Quota, Some(requirement.action)),
            ]);
        for (dimension, action) in required_dimensions {
            let Some(value) = observation
                .dimensions
                .iter()
                .find(|value| value.dimension == dimension && value.action == action)
            else {
                blockers.push(CapabilityGateBlocker {
                    dimension: Some(dimension),
                    reason: CapabilityGateReason::DimensionNotReported,
                    diagnostic_reason: None,
                    code: None,
                    message: None,
                });
                continue;
            };
            if !value.is_fresh_at(context.now_unix_ms) {
                blockers.push(CapabilityGateBlocker {
                    dimension: Some(dimension),
                    reason: CapabilityGateReason::ObservationStale,
                    diagnostic_reason: value.reason,
                    code: value.code.clone(),
                    message: value.message.clone(),
                });
                continue;
            }
            let passes = value.state == TriState::Affirmative
                || (dimension == CapabilityDimension::Quota
                    && value.state == TriState::NotApplicable);
            if !passes {
                blockers.push(CapabilityGateBlocker {
                    dimension: Some(dimension),
                    reason: if value.state == TriState::Negative {
                        CapabilityGateReason::DimensionNegative
                    } else {
                        CapabilityGateReason::DimensionUnknown
                    },
                    diagnostic_reason: value.reason,
                    code: value.code.clone(),
                    message: value.message.clone(),
                });
            }
        }

        let outcome = if blockers.is_empty() {
            CapabilityDecisionOutcome::Allowed
        } else if blockers
            .iter()
            .any(|blocker| blocker.reason == CapabilityGateReason::DimensionNegative)
        {
            CapabilityDecisionOutcome::Denied
        } else {
            CapabilityDecisionOutcome::Indeterminate
        };
        CapabilityDecision {
            requirement,
            outcome,
            blockers,
        }
    }
}

pub struct CapabilityEvaluationContext<'a> {
    pub now_unix_ms: i64,
    pub provider_account_id: &'a CloudResourceId,
    pub provider: CloudProvider,
    pub provider_account_generation: u64,
    pub credential_revision: Option<&'a str>,
    pub scope: &'a CapabilityScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDecisionOutcome {
    Allowed,
    Denied,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityGateReason {
    SnapshotInvalid,
    ProviderAccountMismatch,
    ProviderMismatch,
    AccountGenerationMismatch,
    CredentialRevisionMismatch,
    ScopeMismatch,
    DiscoveryFailed,
    BlockingDiscoveryIssue,
    CapabilityNotReported,
    DimensionNotReported,
    ObservationStale,
    DimensionNegative,
    DimensionUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityGateBlocker {
    pub dimension: Option<CapabilityDimension>,
    pub reason: CapabilityGateReason,
    pub diagnostic_reason: Option<CapabilityReason>,
    pub code: Option<SanitizedCapabilityCode>,
    pub message: Option<SanitizedCapabilityMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDecision {
    pub requirement: CapabilityRequirement,
    pub outcome: CapabilityDecisionOutcome,
    #[serde(default)]
    pub blockers: Vec<CapabilityGateBlocker>,
}

impl CapabilityDecision {
    fn indeterminate(
        requirement: CapabilityRequirement,
        reason: CapabilityGateReason,
        dimension: Option<CapabilityDimension>,
    ) -> Self {
        Self {
            requirement,
            outcome: CapabilityDecisionOutcome::Indeterminate,
            blockers: vec![CapabilityGateBlocker {
                dimension,
                reason,
                diagnostic_reason: None,
                code: None,
                message: None,
            }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySnapshotKey {
    pub provider_account_id: CloudResourceId,
    pub scope: CapabilityScope,
}

impl CapabilitySnapshotKey {
    pub fn validate(&self) -> CoreResult<()> {
        self.provider_account_id.validate()?;
        self.scope.validate_for_account(&self.provider_account_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityStoreWrite {
    Stored,
    FenceLost,
}

#[async_trait::async_trait]
pub trait ProviderCapabilityDiscoverer: Send + Sync {
    /// Provider/auth/permission failures should be normalized into a failed or
    /// partial report. `CoreError` is reserved for invalid contracts and
    /// adapter-internal failures whose semantics cannot be normalized safely.
    async fn discover(
        &self,
        request: &CapabilityDiscoveryRequest,
    ) -> CoreResult<CapabilityDiscoveryReport>;
}

#[async_trait::async_trait]
pub trait CapabilitySnapshotStore: Send + Sync {
    /// Returns the last committed snapshot for this key. A new discovery may
    /// advance authority while a still-fresh committed snapshot remains readable;
    /// consumers must evaluate its generation, credential revision, scope, and
    /// freshness before use. Malformed persisted state returns an error and must
    /// not affect other keys.
    async fn get(
        &self,
        key: &CapabilitySnapshotKey,
    ) -> CoreResult<Option<ProviderCapabilitySnapshot>>;

    /// Atomically allocates the next monotonic epoch and an opaque ownership
    /// token. Epochs increase per key across account generations and credential
    /// revisions; opaque revisions are compared for exact equality, never order.
    async fn begin_discovery(
        &self,
        key: &CapabilitySnapshotKey,
        provider_account_generation: u64,
        credential_revision: Option<&str>,
    ) -> CoreResult<CapabilityDiscoveryFence>;

    /// Stores the result only when the current authoritative fence matches
    /// exactly. Stores compare generation, credential revision, monotonic
    /// epoch, and opaque token for equality; they never sort opaque revisions
    /// or use wall-clock timestamps to infer ownership. Implementations must
    /// call `validate_write` before their atomic compare-and-set. The first
    /// committed payload under a fence is immutable: repeating the identical
    /// payload is idempotently `Stored`, while a different payload is a conflict.
    async fn put_if_current(
        &self,
        key: &CapabilitySnapshotKey,
        expected_fence: &CapabilityDiscoveryFence,
        snapshot: &ProviderCapabilitySnapshot,
    ) -> CoreResult<CapabilityStoreWrite>;

    /// Removes only snapshots that still belong to the exact stale account
    /// generation and credential revision. Invalidation atomically removes or
    /// fences the matching coordination record, so an in-flight stale writer
    /// cannot restore it afterward. A delayed rotation event must not invalidate
    /// snapshots discovered under newer authority.
    async fn invalidate_account_revision(
        &self,
        account_id: &CloudResourceId,
        stale_provider_account_generation: u64,
        stale_credential_revision: Option<&str>,
    ) -> CoreResult<()>;
}

pub fn validate_write(
    key: &CapabilitySnapshotKey,
    expected_fence: &CapabilityDiscoveryFence,
    snapshot: &ProviderCapabilitySnapshot,
) -> CoreResult<()> {
    key.validate()?;
    expected_fence.validate()?;
    snapshot.validate()?;
    if key.provider_account_id != snapshot.provider_account_id
        || key.scope != snapshot.scope
        || expected_fence != &snapshot.fence
    {
        return Err(CoreError::Conflict(
            "capability snapshot write does not match its key and authority fence".to_string(),
        ));
    }
    Ok(())
}

fn validate_observations_and_issues(
    observations: &[CapabilityObservation],
    issues: &[CapabilityDiscoveryIssue],
) -> CoreResult<()> {
    if observations.len() > MAX_CAPABILITIES_PER_REPORT || issues.len() > MAX_ISSUES_PER_REPORT {
        return Err(CoreError::Conflict(
            "capability discovery result exceeds its size limit".to_string(),
        ));
    }
    let mut capabilities = BTreeSet::new();
    for observation in observations {
        observation.validate()?;
        if !capabilities.insert(observation.capability) {
            return Err(CoreError::Conflict(
                "capability discovery result contains duplicate capabilities".to_string(),
            ));
        }
    }
    for issue in issues {
        issue.validate()?;
    }
    Ok(())
}

fn validate_bounded_text(value: &str, kind: &'static str, max_len: usize) -> CoreResult<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(CoreError::Conflict(format!("{kind} is invalid")));
    }
    Ok(())
}

fn reason_matches_dimension(
    dimension: CapabilityDimension,
    state: TriState,
    reason: Option<CapabilityReason>,
) -> bool {
    match state {
        TriState::Affirmative | TriState::NotApplicable => reason.is_none(),
        TriState::Negative => matches!(
            (dimension, reason),
            (
                CapabilityDimension::AdapterSupport,
                Some(CapabilityReason::AdapterNotImplemented)
            ) | (
                CapabilityDimension::ProviderSupport,
                Some(CapabilityReason::ProviderUnsupported)
            ) | (
                CapabilityDimension::Entitlement,
                Some(CapabilityReason::NotEntitled)
            ) | (
                CapabilityDimension::Access,
                Some(CapabilityReason::BusinessPermissionDenied)
            ) | (
                CapabilityDimension::Location,
                Some(CapabilityReason::LocationUnavailable)
            ) | (
                CapabilityDimension::Quota,
                Some(CapabilityReason::QuotaExhausted)
            )
        ),
        TriState::Unknown => matches!(
            reason,
            Some(
                CapabilityReason::NotDiscovered
                    | CapabilityReason::ProbeFailed
                    | CapabilityReason::ProviderUnavailable
                    | CapabilityReason::InvalidProviderResponse
                    | CapabilityReason::AuthenticationFailed
                    | CapabilityReason::DiscoveryPermissionDenied
            )
        ),
    }
}

fn evidence_matches_dimension(
    dimension: CapabilityDimension,
    evidence: CapabilityEvidence,
) -> bool {
    match dimension {
        CapabilityDimension::AdapterSupport => evidence == CapabilityEvidence::AdapterContract,
        CapabilityDimension::ProviderSupport => matches!(
            evidence,
            CapabilityEvidence::AdapterContract
                | CapabilityEvidence::ProviderMetadata
                | CapabilityEvidence::ProviderProbe
        ),
        CapabilityDimension::Entitlement | CapabilityDimension::Location => matches!(
            evidence,
            CapabilityEvidence::ProviderMetadata | CapabilityEvidence::ProviderProbe
        ),
        CapabilityDimension::Access => evidence == CapabilityEvidence::PermissionProbe,
        CapabilityDimension::Quota => matches!(
            evidence,
            CapabilityEvidence::QuotaProbe | CapabilityEvidence::ProviderMetadata
        ),
    }
}

fn issue_reason_matches_dimension(
    reason: CapabilityReason,
    dimension: CapabilityDimension,
) -> bool {
    match reason {
        CapabilityReason::AdapterNotImplemented => dimension == CapabilityDimension::AdapterSupport,
        CapabilityReason::ProviderUnsupported => dimension == CapabilityDimension::ProviderSupport,
        CapabilityReason::NotEntitled => dimension == CapabilityDimension::Entitlement,
        CapabilityReason::BusinessPermissionDenied
        | CapabilityReason::DiscoveryPermissionDenied => dimension == CapabilityDimension::Access,
        CapabilityReason::LocationUnavailable => dimension == CapabilityDimension::Location,
        CapabilityReason::QuotaExhausted => dimension == CapabilityDimension::Quota,
        CapabilityReason::AuthenticationFailed => false,
        CapabilityReason::NotDiscovered
        | CapabilityReason::ProbeFailed
        | CapabilityReason::ProviderUnavailable
        | CapabilityReason::InvalidProviderResponse => true,
    }
}

fn validate_optional_bounded_text(
    value: Option<&str>,
    kind: &'static str,
    max_len: usize,
) -> CoreResult<()> {
    match value {
        Some(value) => validate_bounded_text(value, kind, max_len),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CredentialRef, CredentialSource};

    const OBSERVED_AT: i64 = 1_000;
    const VALID_UNTIL: i64 = 2_000;

    fn capability() -> ProviderCapability {
        ProviderCapability::Dns(DnsCapability::RecordSets)
    }

    fn requirement() -> CapabilityRequirement {
        CapabilityRequirement {
            capability: capability(),
            action: CapabilityAction::Create,
        }
    }

    fn request() -> CapabilityDiscoveryRequest {
        CapabilityDiscoveryRequest {
            provider_account_id: CloudResourceId::new("cloudflare-main").unwrap(),
            fence: CapabilityDiscoveryFence {
                provider_account_generation: 4,
                credential_revision: Some("secret-rv-9".to_string()),
                discovery_epoch: 7,
                discovery_token: DiscoveryToken::new("discovery-7-owner-a").unwrap(),
            },
            account: ProviderAccountSpec {
                provider: CloudProvider::Cloudflare,
                scope: None,
                credential_source: CredentialSource::StaticSecret {
                    credential_ref: CredentialRef::new("cloud/cloudflare-main").unwrap(),
                },
            },
            scope: CapabilityScope::Account,
        }
    }

    fn dimension_observation(
        dimension: CapabilityDimension,
        state: TriState,
        reason: Option<CapabilityReason>,
    ) -> CapabilityDimensionObservation {
        CapabilityDimensionObservation {
            dimension,
            action: matches!(
                dimension,
                CapabilityDimension::Access | CapabilityDimension::Quota
            )
            .then_some(CapabilityAction::Create),
            state,
            reason,
            evidence: match dimension {
                CapabilityDimension::AdapterSupport => CapabilityEvidence::AdapterContract,
                CapabilityDimension::Access => CapabilityEvidence::PermissionProbe,
                CapabilityDimension::Quota => CapabilityEvidence::QuotaProbe,
                _ => CapabilityEvidence::ProviderProbe,
            },
            code: None,
            message: None,
            observed_at_unix_ms: OBSERVED_AT,
            valid_until_unix_ms: VALID_UNTIL,
        }
    }

    fn affirmative_observation() -> CapabilityObservation {
        let mut dimensions: Vec<_> = CapabilityDimension::ACTION_INDEPENDENT
            .into_iter()
            .map(|dimension| dimension_observation(dimension, TriState::Affirmative, None))
            .collect();
        dimensions.extend([
            dimension_observation(CapabilityDimension::Access, TriState::Affirmative, None),
            dimension_observation(CapabilityDimension::Quota, TriState::Affirmative, None),
        ]);
        CapabilityObservation {
            capability: capability(),
            dimensions,
        }
    }

    fn snapshot(observation: CapabilityObservation) -> ProviderCapabilitySnapshot {
        ProviderCapabilitySnapshot::from_report(
            &request(),
            OBSERVED_AT,
            CapabilityDiscoveryReport {
                state: CapabilityDiscoveryState::Complete,
                observations: vec![observation],
                issues: Vec::new(),
            },
        )
        .unwrap()
    }

    fn evaluation_context(
        snapshot: &ProviderCapabilitySnapshot,
    ) -> CapabilityEvaluationContext<'_> {
        CapabilityEvaluationContext {
            now_unix_ms: 1_500,
            provider_account_id: &snapshot.provider_account_id,
            provider: CloudProvider::Cloudflare,
            provider_account_generation: 4,
            credential_revision: Some("secret-rv-9"),
            scope: &snapshot.scope,
        }
    }

    #[test]
    fn closed_capability_family_has_stable_tagged_serialization() {
        let value = serde_json::to_value(ProviderCapability::Dns(DnsCapability::Dnssec)).unwrap();
        assert_eq!(value["family"], "dns");
        assert_eq!(value["name"], "dnssec");
        assert!(value.as_object().unwrap().get("extensions").is_none());
    }

    #[test]
    fn all_fresh_affirmative_dimensions_allow_mutation() {
        let snapshot = snapshot(affirmative_observation());
        let decisions = snapshot.evaluate([requirement()], &evaluation_context(&snapshot));
        assert_eq!(decisions[0].outcome, CapabilityDecisionOutcome::Allowed);
        assert!(decisions[0].blockers.is_empty());
    }

    #[test]
    fn missing_unknown_and_stale_dimensions_fail_closed() {
        let mut missing = affirmative_observation();
        missing
            .dimensions
            .retain(|value| value.dimension != CapabilityDimension::Quota);
        let missing = snapshot(missing);
        assert_eq!(
            missing.evaluate([requirement()], &evaluation_context(&missing))[0].outcome,
            CapabilityDecisionOutcome::Indeterminate
        );

        let mut unknown = affirmative_observation();
        let access = unknown
            .dimensions
            .iter_mut()
            .find(|value| value.dimension == CapabilityDimension::Access)
            .unwrap();
        access.state = TriState::Unknown;
        access.reason = Some(CapabilityReason::ProbeFailed);
        let unknown = snapshot(unknown);
        assert_eq!(
            unknown.evaluate([requirement()], &evaluation_context(&unknown))[0].outcome,
            CapabilityDecisionOutcome::Indeterminate
        );

        let stale = snapshot(affirmative_observation());
        let mut context = evaluation_context(&stale);
        context.now_unix_ms = VALID_UNTIL;
        let decision = &stale.evaluate([requirement()], &context)[0];
        assert_eq!(decision.outcome, CapabilityDecisionOutcome::Indeterminate);
        assert!(decision
            .blockers
            .iter()
            .all(|blocker| blocker.reason == CapabilityGateReason::ObservationStale));
    }

    #[test]
    fn permission_denial_is_not_provider_unsupported() {
        let mut observation = affirmative_observation();
        let access = observation
            .dimensions
            .iter_mut()
            .find(|value| value.dimension == CapabilityDimension::Access)
            .unwrap();
        access.state = TriState::Negative;
        access.reason = Some(CapabilityReason::BusinessPermissionDenied);
        access.code = Some(SanitizedCapabilityCode::new("dns_write_denied").unwrap());
        let snapshot = snapshot(observation);
        let decision = &snapshot.evaluate([requirement()], &evaluation_context(&snapshot))[0];
        assert_eq!(decision.outcome, CapabilityDecisionOutcome::Denied);
        let blocker = decision
            .blockers
            .iter()
            .find(|blocker| blocker.dimension == Some(CapabilityDimension::Access))
            .unwrap();
        assert_eq!(
            blocker.diagnostic_reason,
            Some(CapabilityReason::BusinessPermissionDenied)
        );
        assert_ne!(
            blocker.diagnostic_reason,
            Some(CapabilityReason::ProviderUnsupported)
        );
    }

    #[test]
    fn account_or_credential_changes_invalidate_affirmative_snapshot() {
        let snapshot = snapshot(affirmative_observation());
        let mut generation = evaluation_context(&snapshot);
        generation.provider_account_generation += 1;
        assert_eq!(
            snapshot.evaluate([requirement()], &generation)[0].blockers[0].reason,
            CapabilityGateReason::AccountGenerationMismatch
        );

        let mut credential = evaluation_context(&snapshot);
        credential.credential_revision = Some("secret-rv-10");
        assert_eq!(
            snapshot.evaluate([requirement()], &credential)[0].blockers[0].reason,
            CapabilityGateReason::CredentialRevisionMismatch
        );
    }

    #[test]
    fn failed_discovery_never_grants_previous_affirmative_observations() {
        let failed = ProviderCapabilitySnapshot::from_report(
            &request(),
            OBSERVED_AT,
            CapabilityDiscoveryReport {
                state: CapabilityDiscoveryState::Failed,
                observations: vec![affirmative_observation()],
                issues: vec![CapabilityDiscoveryIssue {
                    severity: CapabilityIssueSeverity::Blocking,
                    scope: CapabilityIssueScope::Account,
                    reason: CapabilityReason::ProviderUnavailable,
                    code: SanitizedCapabilityCode::new("provider_unavailable").unwrap(),
                    message: SanitizedCapabilityMessage::new(
                        "provider discovery endpoint is unavailable",
                    )
                    .unwrap(),
                }],
            },
        )
        .unwrap();
        assert_eq!(
            failed.evaluate([requirement()], &evaluation_context(&failed))[0].outcome,
            CapabilityDecisionOutcome::Indeterminate
        );
    }

    #[test]
    fn request_and_report_reject_cross_account_scope_and_duplicates() {
        let mut request = request();
        request.scope = CapabilityScope::Resource {
            resource_kind: CloudResourceKind::ManagedZone,
            resource: ProviderResourceRef {
                provider_account_id: CloudResourceId::new("another-account").unwrap(),
                external_id: "zone-1".to_string(),
            },
        };
        assert!(request.validate().is_err());

        let report = CapabilityDiscoveryReport {
            state: CapabilityDiscoveryState::Complete,
            observations: vec![affirmative_observation(), affirmative_observation()],
            issues: Vec::new(),
        };
        assert!(report.validate().is_err());
    }

    #[test]
    fn diagnostics_are_bounded_and_permission_reason_is_access_only() {
        let invalid = dimension_observation(
            CapabilityDimension::ProviderSupport,
            TriState::Negative,
            Some(CapabilityReason::BusinessPermissionDenied),
        );
        assert!(invalid.validate().is_err());

        assert!(
            SanitizedCapabilityMessage::new("x".repeat(MAX_DIAGNOSTIC_MESSAGE_LEN + 1)).is_err()
        );
    }

    #[test]
    fn account_identity_is_checked_for_account_and_region_scopes() {
        let account = snapshot(affirmative_observation());
        let another = CloudResourceId::new("another-account").unwrap();
        let mut context = evaluation_context(&account);
        context.provider_account_id = &another;
        assert_eq!(
            account.evaluate([requirement()], &context)[0].blockers[0].reason,
            CapabilityGateReason::ProviderAccountMismatch
        );

        let mut region_request = request();
        region_request.scope = CapabilityScope::Region {
            region: ProviderRegion::new("us-east-1").unwrap(),
        };
        let region = ProviderCapabilitySnapshot::from_report(
            &region_request,
            OBSERVED_AT,
            CapabilityDiscoveryReport {
                state: CapabilityDiscoveryState::Complete,
                observations: vec![affirmative_observation()],
                issues: Vec::new(),
            },
        )
        .unwrap();
        let mut context = evaluation_context(&region);
        context.provider_account_id = &another;
        assert_eq!(
            region.evaluate([requirement()], &context)[0].blockers[0].reason,
            CapabilityGateReason::ProviderAccountMismatch
        );
    }

    #[test]
    fn access_and_quota_are_evaluated_for_the_requested_action() {
        let mut observation = affirmative_observation();
        let mut update_access = dimension_observation(
            CapabilityDimension::Access,
            TriState::Negative,
            Some(CapabilityReason::BusinessPermissionDenied),
        );
        update_access.action = Some(CapabilityAction::Update);
        let mut update_quota =
            dimension_observation(CapabilityDimension::Quota, TriState::NotApplicable, None);
        update_quota.action = Some(CapabilityAction::Update);
        observation.dimensions.extend([update_access, update_quota]);
        let snapshot = snapshot(observation);

        assert_eq!(
            snapshot.evaluate([requirement()], &evaluation_context(&snapshot))[0].outcome,
            CapabilityDecisionOutcome::Allowed
        );
        let update = CapabilityRequirement {
            capability: capability(),
            action: CapabilityAction::Update,
        };
        assert_eq!(
            snapshot.evaluate([update], &evaluation_context(&snapshot))[0].outcome,
            CapabilityDecisionOutcome::Denied
        );

        let invalid = CapabilityDimensionObservation {
            dimension: CapabilityDimension::Access,
            action: Some(CapabilityAction::Observe),
            state: TriState::NotApplicable,
            reason: None,
            evidence: CapabilityEvidence::PermissionProbe,
            code: None,
            message: None,
            observed_at_unix_ms: OBSERVED_AT,
            valid_until_unix_ms: VALID_UNTIL,
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn dimension_evidence_allowlist_rejects_unsafe_affirmative_claims() {
        let mut access =
            dimension_observation(CapabilityDimension::Access, TriState::Affirmative, None);
        access.evidence = CapabilityEvidence::AdapterContract;
        assert!(access.validate().is_err());

        let mut quota =
            dimension_observation(CapabilityDimension::Quota, TriState::Affirmative, None);
        quota.evidence = CapabilityEvidence::ProviderProbe;
        assert!(quota.validate().is_err());
        quota.evidence = CapabilityEvidence::ProviderMetadata;
        assert!(quota.validate().is_ok());
    }

    #[test]
    fn partial_account_wide_auth_issue_blocks_fresh_affirmative_data() {
        let partial = ProviderCapabilitySnapshot::from_report(
            &request(),
            OBSERVED_AT,
            CapabilityDiscoveryReport {
                state: CapabilityDiscoveryState::Partial,
                observations: vec![affirmative_observation()],
                issues: vec![CapabilityDiscoveryIssue {
                    severity: CapabilityIssueSeverity::Blocking,
                    scope: CapabilityIssueScope::Account,
                    reason: CapabilityReason::AuthenticationFailed,
                    code: SanitizedCapabilityCode::new("authentication_failed").unwrap(),
                    message: SanitizedCapabilityMessage::new(
                        "provider authentication failed during discovery",
                    )
                    .unwrap(),
                }],
            },
        )
        .unwrap();
        let decision = &partial.evaluate([requirement()], &evaluation_context(&partial))[0];
        assert_eq!(decision.outcome, CapabilityDecisionOutcome::Indeterminate);
        assert_eq!(
            decision.blockers[0].reason,
            CapabilityGateReason::BlockingDiscoveryIssue
        );
    }

    #[test]
    fn resource_kind_participates_in_scope_identity_and_diagnostics_do_not_echo_input() {
        let resource = ProviderResourceRef {
            provider_account_id: CloudResourceId::new("cloudflare-main").unwrap(),
            external_id: "shared-external-id".to_string(),
        };
        let zone = CapabilityScope::Resource {
            resource_kind: CloudResourceKind::ManagedZone,
            resource: resource.clone(),
        };
        let record_set = CapabilityScope::Resource {
            resource_kind: CloudResourceKind::DnsRecordSet,
            resource,
        };
        assert_ne!(zone, record_set);

        let error = SanitizedCapabilityCode::new(" leaked-value\n")
            .unwrap_err()
            .to_string();
        assert!(!error.contains("leaked-value"));
    }

    #[test]
    fn security_issues_cannot_bypass_blocking_scope_rules_with_warning_severity() {
        let code = SanitizedCapabilityCode::new("security_issue").unwrap();
        let message = SanitizedCapabilityMessage::new("security discovery issue").unwrap();
        let authentication_warning = CapabilityDiscoveryIssue {
            severity: CapabilityIssueSeverity::Warning,
            scope: CapabilityIssueScope::Account,
            reason: CapabilityReason::AuthenticationFailed,
            code: code.clone(),
            message: message.clone(),
        };
        assert!(authentication_warning.validate().is_err());

        let permission_warning = CapabilityDiscoveryIssue {
            severity: CapabilityIssueSeverity::Warning,
            scope: CapabilityIssueScope::Requirement {
                requirement: requirement(),
            },
            reason: CapabilityReason::BusinessPermissionDenied,
            code: code.clone(),
            message: message.clone(),
        };
        assert!(permission_warning.validate().is_err());

        let wrong_dimension = CapabilityDiscoveryIssue {
            severity: CapabilityIssueSeverity::Blocking,
            scope: CapabilityIssueScope::Dimension {
                requirement: requirement(),
                dimension: CapabilityDimension::ProviderSupport,
            },
            reason: CapabilityReason::BusinessPermissionDenied,
            code,
            message,
        };
        assert!(wrong_dimension.validate().is_err());
    }

    #[test]
    fn snapshot_write_helper_requires_exact_key_and_authority_fence() {
        let snapshot = snapshot(affirmative_observation());
        let key = CapabilitySnapshotKey {
            provider_account_id: snapshot.provider_account_id.clone(),
            scope: snapshot.scope.clone(),
        };
        assert!(validate_write(&key, &snapshot.fence, &snapshot).is_ok());

        let mut stale_fence = snapshot.fence.clone();
        stale_fence.discovery_epoch -= 1;
        assert!(validate_write(&key, &stale_fence, &snapshot).is_err());

        let wrong_key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("wrong-account").unwrap(),
            scope: CapabilityScope::Account,
        };
        assert!(validate_write(&wrong_key, &snapshot.fence, &snapshot).is_err());
    }

    #[test]
    fn retained_capability_snapshot_fits_the_shared_persistence_budget() {
        let capabilities = [
            ProviderCapability::Dns(DnsCapability::PublicZones),
            ProviderCapability::Dns(DnsCapability::PrivateZones),
            ProviderCapability::Dns(DnsCapability::RecordSets),
            ProviderCapability::Dns(DnsCapability::Dnssec),
            ProviderCapability::Dns(DnsCapability::ApexAlias),
            ProviderCapability::Dns(DnsCapability::ProxiedRecords),
            ProviderCapability::Dns(DnsCapability::WeightedRouting),
            ProviderCapability::Dns(DnsCapability::GeolocationRouting),
            ProviderCapability::Dns(DnsCapability::FailoverRouting),
            ProviderCapability::Dns(DnsCapability::AtomicChanges),
            ProviderCapability::Waf(WafCapability::ManagedRules),
            ProviderCapability::Waf(WafCapability::CustomRules),
            ProviderCapability::Waf(WafCapability::RateLimiting),
        ];
        let message = SanitizedCapabilityMessage::new("x".repeat(MAX_DIAGNOSTIC_MESSAGE_LEN))
            .expect("maximum diagnostic message");
        let observations = capabilities
            .into_iter()
            .map(|capability| {
                let dimensions = CapabilityDimension::ACTION_INDEPENDENT
                    .into_iter()
                    .map(|dimension| (dimension, None))
                    .chain(CapabilityAction::ALL.into_iter().flat_map(|action| {
                        [
                            (CapabilityDimension::Access, Some(action)),
                            (CapabilityDimension::Quota, Some(action)),
                        ]
                    }))
                    .map(|(dimension, action)| CapabilityDimensionObservation {
                        dimension,
                        action,
                        state: TriState::Affirmative,
                        reason: None,
                        evidence: match dimension {
                            CapabilityDimension::AdapterSupport => {
                                CapabilityEvidence::AdapterContract
                            }
                            CapabilityDimension::Access => CapabilityEvidence::PermissionProbe,
                            CapabilityDimension::Quota => CapabilityEvidence::QuotaProbe,
                            _ => CapabilityEvidence::ProviderProbe,
                        },
                        code: None,
                        message: Some(message.clone()),
                        observed_at_unix_ms: OBSERVED_AT,
                        valid_until_unix_ms: VALID_UNTIL,
                    })
                    .collect();
                CapabilityObservation {
                    capability,
                    dimensions,
                }
            })
            .collect();
        let result = ProviderCapabilitySnapshot::from_report(
            &request(),
            OBSERVED_AT,
            CapabilityDiscoveryReport {
                state: CapabilityDiscoveryState::Complete,
                observations,
                issues: Vec::new(),
            },
        );
        assert!(result.is_ok());
    }

    #[test]
    fn retired_capability_snapshot_detection_is_narrow() {
        assert!(is_retired_capability_snapshot_json(
            r#"{"observations":[{"capability":{"family":"edge","name":"http_proxy"}}]}"#
        ));
        assert!(is_retired_capability_snapshot_json(
            r#"{"capability":{"family":"certificate","name":"managed"}}"#
        ));
        assert!(!is_retired_capability_snapshot_json(
            r#"{"observations":[{"capability":{"family":"dns","name":"record_sets"}},{"capability":{"family":"waf","name":"managed_rules"}}]}"#
        ));
        assert!(!is_retired_capability_snapshot_json("not-json"));
    }
}
