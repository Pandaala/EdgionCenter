//! Provider-neutral DNS propagation and authoritative verification contract.
//!
//! Verification is deliberately separate from the DNS provider control-plane
//! port. Provider acceptance is an input fence, never evidence that DNS data
//! has been published.

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    AbsoluteDnsName, DelegationObservation, DelegationState, DnsChangeId, DnsRecordRevision,
    DnsRecordSetKey, DnsRecordSetValue, DnsZoneRef, DnssecDsRecord, ProviderDnsRecordType,
    ZoneAuthorityEvidence, ZoneLifecycleObservation, ZoneLifecycleRevision,
};
use crate::{CoreError, CoreResult};

const MAX_NAMESERVERS: usize = 16;
const MAX_RESOLVER_PROFILES: usize = 8;
const MAX_EXPECTED_VALUES: usize = 128;
const MAX_TOTAL_TIMEOUT_MS: u64 = 15 * 60 * 1_000;
const MAX_QUERY_TIMEOUT_MS: u64 = 10_000;
const MAX_ATTEMPTS: u8 = 10;
const MAX_QUERIES: u16 = 1_024;
const MAX_EVIDENCE_AGE_MS: u64 = 15 * 60 * 1_000;

macro_rules! opaque_id {
    ($name:ident, $kind:literal, $max:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> CoreResult<Self> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > $max
                    || value.trim() != value
                    || value.chars().any(char::is_control)
                {
                    return Err(CoreError::InvalidIdentifier { kind: $kind, value });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = CoreError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

opaque_id!(DnsVerificationRequestId, "DNS verification request ID", 512);
opaque_id!(ResolverProfileId, "DNS resolver profile ID", 128);
opaque_id!(
    ResolverProfileRevision,
    "DNS resolver profile revision",
    512
);
opaque_id!(SanitizedDnsFailureCode, "sanitized DNS failure code", 64);

/// Immutable reference to a deployment-configured resolver profile. Evidence
/// from an older profile configuration cannot satisfy a newer request.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolverProfileRef {
    pub id: ResolverProfileId,
    pub revision: ResolverProfileRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsVerificationBinding {
    pub zone: DnsZoneRef,
    pub zone_revision: ZoneLifecycleRevision,
    #[serde(default)]
    pub authoritative_nameservers: BTreeSet<AbsoluteDnsName>,
    pub record_revision: Option<DnsRecordRevision>,
    pub provider_change_id: Option<DnsChangeId>,
}

impl DnsVerificationBinding {
    pub fn validate(&self) -> CoreResult<()> {
        self.zone.validate()?;
        if self.authoritative_nameservers.len() > MAX_NAMESERVERS {
            return Err(CoreError::Conflict(
                "DNS verification has too many authoritative nameservers".into(),
            ));
        }
        if let Some(revision) = self.record_revision.as_ref() {
            revision.validate()?;
        }
        if let Some(change_id) = self.provider_change_id.as_ref() {
            change_id.validate()?;
        }
        if self.record_revision.is_none() && self.provider_change_id.is_none() {
            return Err(CoreError::Conflict(
                "DNS verification requires a record revision or provider change fence".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DnsRrsetExpectation {
    Present {
        key: DnsRecordSetKey,
        values: BTreeSet<DnsRecordSetValue>,
    },
    Absent {
        key: DnsRecordSetKey,
    },
}

impl DnsRrsetExpectation {
    pub fn key(&self) -> &DnsRecordSetKey {
        match self {
            Self::Present { key, .. } | Self::Absent { key } => key,
        }
    }

    pub fn validate(&self, zone: &DnsZoneRef) -> CoreResult<()> {
        let key = self.key();
        key.validate()?;
        if !key.owner.is_within(&zone.apex)
            || !matches!(key.routing, super::DnsRoutingIdentity::Simple)
            || !matches!(
                key.record_type,
                ProviderDnsRecordType::A
                    | ProviderDnsRecordType::Aaaa
                    | ProviderDnsRecordType::Cname
                    | ProviderDnsRecordType::Txt
            )
        {
            return Err(CoreError::Conflict(
                "DNS verification supports only simple in-zone A, AAAA, CNAME, and TXT RRsets"
                    .into(),
            ));
        }
        if let Self::Present { values, .. } = self {
            if values.is_empty()
                || values.len() > MAX_EXPECTED_VALUES
                || values
                    .iter()
                    .any(|value| value.record_type() != key.record_type)
            {
                return Err(CoreError::Conflict(
                    "DNS verification expected RRset values are invalid".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DnsVerificationScope {
    Public {
        authority_profile: ResolverProfileRef,
        #[serde(default)]
        recursive_profiles: BTreeSet<ResolverProfileRef>,
    },
    Private {
        resolver_profile: ResolverProfileRef,
    },
    SplitHorizon {
        view: ResolverProfileRef,
    },
    DelegatedValidation {
        child_apex: AbsoluteDnsName,
        #[serde(default)]
        child_nameservers: BTreeSet<AbsoluteDnsName>,
        authority_profile: ResolverProfileRef,
        #[serde(default)]
        recursive_profiles: BTreeSet<ResolverProfileRef>,
    },
}

impl DnsVerificationScope {
    fn recursive_profiles(&self) -> usize {
        match self {
            Self::Public {
                recursive_profiles, ..
            }
            | Self::DelegatedValidation {
                recursive_profiles, ..
            } => recursive_profiles.len(),
            Self::Private { .. } | Self::SplitHorizon { .. } => 1,
        }
    }

    fn validates_authoritative_servers(&self) -> bool {
        matches!(self, Self::Public { .. } | Self::DelegatedValidation { .. })
    }

    fn authority_profile(&self) -> Option<&ResolverProfileRef> {
        match self {
            Self::Public {
                authority_profile, ..
            }
            | Self::DelegatedValidation {
                authority_profile, ..
            } => Some(authority_profile),
            Self::Private { .. } | Self::SplitHorizon { .. } => None,
        }
    }

    fn authoritative_nameservers<'a>(
        &'a self,
        binding: &'a DnsVerificationBinding,
    ) -> &'a BTreeSet<AbsoluteDnsName> {
        match self {
            Self::DelegatedValidation {
                child_nameservers, ..
            } => child_nameservers,
            _ => &binding.authoritative_nameservers,
        }
    }

    fn validate(&self, zone: &DnsZoneRef) -> CoreResult<()> {
        if self.recursive_profiles() > MAX_RESOLVER_PROFILES {
            return Err(CoreError::Conflict(
                "DNS verification has too many resolver profiles".into(),
            ));
        }
        match self {
            Self::Public {
                recursive_profiles, ..
            } if zone.visibility != super::ZoneVisibility::Public
                || recursive_profiles.is_empty() =>
            {
                Err(CoreError::Conflict(
                    "public DNS verification requires a public zone and resolver profile".into(),
                ))
            }
            Self::Private { .. } if zone.visibility != super::ZoneVisibility::Private => Err(
                CoreError::Conflict("private DNS verification requires a private zone".into()),
            ),
            Self::SplitHorizon { .. } if zone.visibility != super::ZoneVisibility::Private => {
                Err(CoreError::Conflict(
                    "split-horizon lifecycle verification requires a private zone view".into(),
                ))
            }
            Self::DelegatedValidation { child_apex, .. }
                if child_apex == &zone.apex
                    || !child_apex
                        .as_str()
                        .ends_with(&format!(".{}", zone.apex.as_str())) =>
            {
                Err(CoreError::Conflict(
                    "delegated validation apex must be below the managed zone".into(),
                ))
            }
            Self::DelegatedValidation {
                child_nameservers, ..
            } if child_nameservers.is_empty() || child_nameservers.len() > MAX_NAMESERVERS => {
                Err(CoreError::Conflict(
                    "delegated validation requires a bounded child nameserver set".into(),
                ))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DnssecVerificationExpectation {
    NotRequested,
    Unsigned { require_parent_ds_absent: bool },
    Signed { expected_ds: Vec<DnssecDsRecord> },
}

impl DnssecVerificationExpectation {
    fn validate(&self) -> CoreResult<()> {
        if let Self::Signed { expected_ds } = self {
            if expected_ds.is_empty() || expected_ds.len() > 16 {
                return Err(CoreError::Conflict(
                    "DNSSEC verification requires a bounded non-empty DS set".into(),
                ));
            }
            for record in expected_ds {
                record.validate()?;
            }
            let unique: BTreeSet<_> = expected_ds
                .iter()
                .map(|record| {
                    (
                        record.key_tag,
                        record.algorithm,
                        record.digest_type,
                        record.digest.as_str(),
                    )
                })
                .collect();
            if unique.len() != expected_ds.len() {
                return Err(CoreError::Conflict(
                    "DNSSEC verification contains duplicate DS records".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsVerificationPolicy {
    pub total_timeout_ms: u64,
    pub per_query_timeout_ms: u64,
    pub max_attempts: u8,
    pub max_queries: u16,
    pub retry_initial_ms: u64,
    pub retry_max_ms: u64,
    pub evidence_max_age_ms: u64,
}

impl DnsVerificationPolicy {
    pub fn validate(&self, minimum_queries_per_attempt: usize) -> CoreResult<()> {
        if self.total_timeout_ms == 0
            || self.total_timeout_ms > MAX_TOTAL_TIMEOUT_MS
            || self.per_query_timeout_ms < 100
            || self.per_query_timeout_ms > MAX_QUERY_TIMEOUT_MS
            || self.per_query_timeout_ms > self.total_timeout_ms
            || self.max_attempts == 0
            || self.max_attempts > MAX_ATTEMPTS
            || self.max_queries == 0
            || self.max_queries > MAX_QUERIES
            || self.retry_initial_ms == 0
            || self.retry_initial_ms > self.retry_max_ms
            || self.retry_max_ms > self.total_timeout_ms
            || self.evidence_max_age_ms == 0
            || self.evidence_max_age_ms > MAX_EVIDENCE_AGE_MS
            || minimum_queries_per_attempt.saturating_mul(self.max_attempts as usize)
                > self.max_queries as usize
        {
            return Err(CoreError::Conflict(
                "DNS verification policy or query budget is invalid".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsVerificationRequest {
    pub request_id: DnsVerificationRequestId,
    pub binding: DnsVerificationBinding,
    pub scope: DnsVerificationScope,
    pub expectation: DnsRrsetExpectation,
    pub dnssec: DnssecVerificationExpectation,
    pub policy: DnsVerificationPolicy,
}

impl DnsVerificationRequest {
    pub fn validate(&self) -> CoreResult<()> {
        self.binding.validate()?;
        self.scope.validate(&self.binding.zone)?;
        self.expectation.validate(&self.binding.zone)?;
        if let DnsVerificationScope::DelegatedValidation { child_apex, .. } = &self.scope {
            let child_owner = super::DnsOwnerName::new(child_apex.as_str())?;
            if self.expectation.key().owner != child_owner
                && !self.expectation.key().owner.is_within(child_apex)
            {
                return Err(CoreError::Conflict(
                    "delegated validation expectation must be inside the delegated child".into(),
                ));
            }
        }
        self.dnssec.validate()?;
        if !self.scope.validates_authoritative_servers()
            && !matches!(self.dnssec, DnssecVerificationExpectation::NotRequested)
        {
            return Err(CoreError::Conflict(
                "private and split-horizon DNSSEC verification is not supported".into(),
            ));
        }
        if self.scope.validates_authoritative_servers()
            && self.binding.authoritative_nameservers.is_empty()
        {
            return Err(CoreError::Conflict(
                "authoritative DNS verification requires nameservers".into(),
            ));
        }
        if !self.scope.validates_authoritative_servers()
            && !self.binding.authoritative_nameservers.is_empty()
        {
            return Err(CoreError::Conflict(
                "private DNS verification must use its configured resolver profile".into(),
            ));
        }
        let dnssec_queries = usize::from(!matches!(
            self.dnssec,
            DnssecVerificationExpectation::NotRequested
        ));
        let minimum_queries = self.scope.authoritative_nameservers(&self.binding).len()
            + self.scope.recursive_profiles()
            + dnssec_queries;
        self.policy.validate(minimum_queries.max(1))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DnsQueryOutcome {
    Match {
        #[serde(default)]
        values: BTreeSet<DnsRecordSetValue>,
        ttl_seconds: Option<u32>,
        authoritative: bool,
    },
    Mismatch {
        #[serde(default)]
        values: BTreeSet<DnsRecordSetValue>,
    },
    NoData {
        authoritative: bool,
        soa_present: bool,
    },
    NxDomain {
        authoritative: bool,
        soa_present: bool,
    },
    Timeout,
    ServFail,
    Refused,
    TransportFailure {
        code: SanitizedDnsFailureCode,
    },
    UnsafeAddress,
    BudgetExhausted,
}

impl DnsQueryOutcome {
    fn satisfies(&self, expectation: &DnsRrsetExpectation, authoritative: bool) -> bool {
        match (expectation, self) {
            (
                DnsRrsetExpectation::Present {
                    values: expected, ..
                },
                Self::Match {
                    values: observed,
                    authoritative: answer_authoritative,
                    ..
                },
            ) => expected == observed && (!authoritative || *answer_authoritative),
            (
                DnsRrsetExpectation::Absent { .. },
                Self::NoData {
                    authoritative: answer_authoritative,
                    soa_present: true,
                }
                | Self::NxDomain {
                    authoritative: answer_authoritative,
                    soa_present: true,
                },
            ) => !authoritative || *answer_authoritative,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameserverCheck {
    pub nameserver: AbsoluteDnsName,
    pub attempts: u8,
    pub outcome: DnsQueryOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecursiveResolverCheck {
    pub profile: ResolverProfileRef,
    pub attempts: u8,
    pub outcome: DnsQueryOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnssecValidationState {
    NotRequested,
    SecureLocalChain,
    AuthenticatedByTrustedResolver,
    Insecure,
    Bogus,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnssecEvidenceSource {
    NotChecked,
    DirectParentAuthoritative,
    TrustedRecursiveAuthenticated,
    /// Parent DS and target RRset were validated locally from the root trust
    /// anchors through one explicitly configured recursive transport.
    LocallyValidatedRecursive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnssecVerificationEvidence {
    #[serde(default)]
    pub observed_parent_ds: Vec<DnssecDsRecord>,
    pub parent_source: DnssecEvidenceSource,
    pub parent_soa_present_for_absence: bool,
    pub validation: DnssecValidationState,
    pub validator_profile: Option<ResolverProfileRef>,
}

impl DnssecVerificationEvidence {
    fn satisfies(&self, expectation: &DnssecVerificationExpectation) -> bool {
        match expectation {
            DnssecVerificationExpectation::NotRequested => {
                self.validation == DnssecValidationState::NotRequested
            }
            DnssecVerificationExpectation::Unsigned {
                require_parent_ds_absent: false,
            } => matches!(
                self.validation,
                DnssecValidationState::SecureLocalChain
                    | DnssecValidationState::AuthenticatedByTrustedResolver
                    | DnssecValidationState::Insecure
            ),
            DnssecVerificationExpectation::Unsigned {
                require_parent_ds_absent: true,
            } => {
                self.observed_parent_ds.is_empty()
                    && matches!(
                        self.parent_source,
                        DnssecEvidenceSource::DirectParentAuthoritative
                            | DnssecEvidenceSource::TrustedRecursiveAuthenticated
                            | DnssecEvidenceSource::LocallyValidatedRecursive
                    )
                    && self.parent_soa_present_for_absence
                    && self.validation != DnssecValidationState::Bogus
            }
            DnssecVerificationExpectation::Signed { expected_ds } => {
                ds_sets_equal(expected_ds, &self.observed_parent_ds)
                    && matches!(
                        (self.parent_source, self.validation),
                        (
                            DnssecEvidenceSource::LocallyValidatedRecursive,
                            DnssecValidationState::SecureLocalChain
                        ) | (
                            DnssecEvidenceSource::TrustedRecursiveAuthenticated,
                            DnssecValidationState::AuthenticatedByTrustedResolver
                        )
                    )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsVerificationBudgetUse {
    pub queries: u16,
    pub attempts: u16,
    pub elapsed_ms: u64,
    pub exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsVerificationEvidence {
    pub request_id: DnsVerificationRequestId,
    pub binding: DnsVerificationBinding,
    pub expectation: DnsRrsetExpectation,
    pub started_at_unix_ms: i64,
    pub completed_at_unix_ms: i64,
    /// Profile used to resolve and contact authoritative and parent servers.
    /// It is absent for private and split-horizon profile-only checks.
    pub authority_profile: Option<ResolverProfileRef>,
    #[serde(default)]
    pub authoritative: Vec<NameserverCheck>,
    #[serde(default)]
    pub recursive: Vec<RecursiveResolverCheck>,
    pub delegation: DelegationObservation,
    pub dnssec: DnssecVerificationEvidence,
    pub budget: DnsVerificationBudgetUse,
}

impl DnsVerificationEvidence {
    pub fn validate_against(
        &self,
        request: &DnsVerificationRequest,
        now_unix_ms: i64,
    ) -> CoreResult<()> {
        request.validate()?;
        if self.request_id != request.request_id
            || self.binding != request.binding
            || self.expectation != request.expectation
        {
            return Err(CoreError::Conflict(
                "DNS verification evidence does not match its request binding".into(),
            ));
        }
        if self.authority_profile.as_ref() != request.scope.authority_profile() {
            return Err(CoreError::Conflict(
                "DNS verification evidence has the wrong authority resolver profile".into(),
            ));
        }
        match &request.dnssec {
            DnssecVerificationExpectation::NotRequested
                if self.dnssec.validator_profile.is_some() =>
            {
                return Err(CoreError::Conflict(
                    "unrequested DNSSEC evidence cannot name a validator profile".into(),
                ));
            }
            DnssecVerificationExpectation::NotRequested => {}
            _ if self.dnssec.validator_profile.as_ref() != request.scope.authority_profile() => {
                return Err(CoreError::Conflict(
                    "DNSSEC evidence is not bound to the authority resolver profile".into(),
                ));
            }
            _ => {}
        }
        if self.started_at_unix_ms <= 0
            || self.completed_at_unix_ms < self.started_at_unix_ms
            || now_unix_ms < self.completed_at_unix_ms
            || (now_unix_ms - self.completed_at_unix_ms) as u64 > request.policy.evidence_max_age_ms
            || self.budget.elapsed_ms > request.policy.total_timeout_ms
            || self.budget.queries > request.policy.max_queries
        {
            return Err(CoreError::Conflict(
                "DNS verification evidence is stale or exceeds its budget".into(),
            ));
        }
        validate_check_attempts(
            self.authoritative.iter().map(|check| check.attempts),
            request.policy.max_attempts,
        )?;
        validate_check_attempts(
            self.recursive.iter().map(|check| check.attempts),
            request.policy.max_attempts,
        )?;
        for outcome in self
            .authoritative
            .iter()
            .map(|check| &check.outcome)
            .chain(self.recursive.iter().map(|check| &check.outcome))
        {
            validate_outcome(outcome, request.expectation.key().record_type)?;
        }
        let actual_nameservers: BTreeSet<_> = self
            .authoritative
            .iter()
            .map(|check| check.nameserver.clone())
            .collect();
        if actual_nameservers.len() != self.authoritative.len()
            || actual_nameservers != *request.scope.authoritative_nameservers(&request.binding)
        {
            return Err(CoreError::Conflict(
                "DNS verification evidence has incomplete nameserver coverage".into(),
            ));
        }
        let expected_profiles = scope_profiles(&request.scope);
        let actual_profiles: BTreeSet<_> = self
            .recursive
            .iter()
            .map(|check| check.profile.clone())
            .collect();
        if actual_profiles.len() != self.recursive.len() || actual_profiles != expected_profiles {
            return Err(CoreError::Conflict(
                "DNS verification evidence has incomplete resolver-profile coverage".into(),
            ));
        }
        let reported_attempts: u16 = self
            .authoritative
            .iter()
            .map(|check| u16::from(check.attempts))
            .chain(self.recursive.iter().map(|check| u16::from(check.attempts)))
            .sum();
        if self.budget.attempts < reported_attempts
            || self.budget.attempts > self.budget.queries
            || self.budget.queries < self.authoritative.len() as u16 + self.recursive.len() as u16
        {
            return Err(CoreError::Conflict(
                "DNS verification budget use does not cover reported checks".into(),
            ));
        }
        for record in &self.dnssec.observed_parent_ds {
            record.validate()?;
        }
        Ok(())
    }

    pub fn authoritative_published(&self, request: &DnsVerificationRequest) -> bool {
        !self.authoritative.is_empty()
            && self
                .authoritative
                .iter()
                .all(|check| check.outcome.satisfies(&request.expectation, true))
    }

    pub fn recursive_visible(&self, request: &DnsVerificationRequest) -> bool {
        !self.recursive.is_empty()
            && self
                .recursive
                .iter()
                .all(|check| check.outcome.satisfies(&request.expectation, false))
    }

    pub fn to_zone_authority_evidence(
        &self,
        request: &DnsVerificationRequest,
        now_unix_ms: i64,
    ) -> CoreResult<ZoneAuthorityEvidence> {
        self.validate_against(request, now_unix_ms)?;
        if matches!(
            request.scope,
            DnsVerificationScope::DelegatedValidation { .. }
        ) {
            return Err(CoreError::Conflict(
                "delegated child verification cannot update parent zone readiness".into(),
            ));
        }
        let published = match request.scope {
            DnsVerificationScope::Public { .. } => {
                self.authoritative_published(request) && self.recursive_visible(request)
            }
            DnsVerificationScope::Private { .. } | DnsVerificationScope::SplitHorizon { .. } => {
                self.recursive_visible(request)
            }
            DnsVerificationScope::DelegatedValidation { .. } => unreachable!(),
        };
        let delegation_valid = if request.scope.validates_authoritative_servers() {
            self.delegation.expected_nameservers == request.binding.authoritative_nameservers
                && self.delegation.state == DelegationState::Delegated
                && self.delegation.parent_nameservers == request.binding.authoritative_nameservers
        } else {
            self.delegation.state == DelegationState::NotApplicable
                && self.delegation.expected_nameservers.is_empty()
                && self.delegation.parent_nameservers.is_empty()
        };
        if !published
            || !self.dnssec.satisfies(&request.dnssec)
            || !delegation_valid
            || self.budget.exhausted
        {
            return Err(CoreError::Conflict(
                "DNS verification evidence does not prove zone authority readiness".into(),
            ));
        }
        Ok(ZoneAuthorityEvidence {
            zone: request.binding.zone.clone(),
            observed_revision: request.binding.zone_revision.clone(),
            delegation: self.delegation.clone(),
            authoritative_verification: super::AuthoritativeDnsVerification::Verified {
                checked_at: self.completed_at_unix_ms.to_string(),
            },
        })
    }
}

/// Applies verification through the only readiness path that checks the full
/// request binding, freshness, delegation, every authoritative nameserver,
/// and DNSSEC expectation.
pub fn apply_dns_verification_evidence(
    observation: &mut ZoneLifecycleObservation,
    request: &DnsVerificationRequest,
    evidence: &DnsVerificationEvidence,
    now_unix_ms: i64,
) -> CoreResult<()> {
    if matches!(
        request.scope,
        DnsVerificationScope::DelegatedValidation { .. }
    ) {
        return Err(CoreError::Conflict(
            "delegated child verification cannot update parent zone readiness".into(),
        ));
    }
    if observation.zone != request.binding.zone
        || observation.revision != request.binding.zone_revision
        || observation.authoritative_nameservers != request.binding.authoritative_nameservers
    {
        return Err(CoreError::Conflict(
            "DNS verification request does not match the current zone observation".into(),
        ));
    }
    let projection = evidence.to_zone_authority_evidence(request, now_unix_ms)?;
    let mut next = observation.clone();
    next.delegation = projection.delegation;
    next.authoritative_verification = projection.authoritative_verification;
    next.readiness = super::evaluate_zone_readiness(&next.authoritative_verification);
    next.validate()?;
    *observation = next;
    Ok(())
}

fn validate_check_attempts(attempts: impl Iterator<Item = u8>, maximum: u8) -> CoreResult<()> {
    if attempts
        .into_iter()
        .any(|attempts| attempts == 0 || attempts > maximum)
    {
        return Err(CoreError::Conflict(
            "DNS verification evidence has an invalid attempt count".into(),
        ));
    }
    Ok(())
}

fn validate_outcome(
    outcome: &DnsQueryOutcome,
    record_type: ProviderDnsRecordType,
) -> CoreResult<()> {
    let values = match outcome {
        DnsQueryOutcome::Match { values, .. } | DnsQueryOutcome::Mismatch { values } => values,
        _ => return Ok(()),
    };
    if values.len() > MAX_EXPECTED_VALUES
        || values
            .iter()
            .any(|value| value.record_type() != record_type)
    {
        return Err(CoreError::Conflict(
            "DNS verification response values are invalid".into(),
        ));
    }
    Ok(())
}

fn scope_profiles(scope: &DnsVerificationScope) -> BTreeSet<ResolverProfileRef> {
    match scope {
        DnsVerificationScope::Public {
            recursive_profiles, ..
        }
        | DnsVerificationScope::DelegatedValidation {
            recursive_profiles, ..
        } => recursive_profiles.clone(),
        DnsVerificationScope::Private { resolver_profile } => {
            BTreeSet::from([resolver_profile.clone()])
        }
        DnsVerificationScope::SplitHorizon { view } => BTreeSet::from([view.clone()]),
    }
}

fn ds_sets_equal(left: &[DnssecDsRecord], right: &[DnssecDsRecord]) -> bool {
    let tuples = |records: &[DnssecDsRecord]| {
        records
            .iter()
            .map(|record| {
                (
                    record.key_tag,
                    record.algorithm,
                    record.digest_type,
                    record.digest.clone(),
                )
            })
            .collect::<BTreeSet<_>>()
    };
    tuples(left) == tuples(right) && left.len() == right.len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsVerificationErrorKind {
    InvalidRequest,
    UnknownResolverProfile,
    Cancelled,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsVerificationError {
    pub kind: DnsVerificationErrorKind,
    pub code: SanitizedDnsFailureCode,
}

pub type DnsVerificationResult<T> = Result<T, DnsVerificationError>;

#[async_trait]
pub trait DnsPropagationVerifier: Send + Sync {
    async fn verify(
        &self,
        request: &DnsVerificationRequest,
    ) -> DnsVerificationResult<DnsVerificationEvidence>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::{
        CloudProvider, CloudResourceId, DnsOwnerName, DnsRoutingIdentity, DnsZoneId, ZoneVisibility,
    };
    use std::net::Ipv4Addr;

    fn profile(id: &str) -> ResolverProfileRef {
        ResolverProfileRef {
            id: ResolverProfileId::new(id).unwrap(),
            revision: ResolverProfileRevision::new(format!("{id}-r1")).unwrap(),
        }
    }

    fn request() -> DnsVerificationRequest {
        let zone = DnsZoneRef {
            provider_account_id: CloudResourceId::new("account").unwrap(),
            provider: CloudProvider::Aws,
            zone_id: DnsZoneId::new("zone").unwrap(),
            apex: AbsoluteDnsName::new("example.com").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        DnsVerificationRequest {
            request_id: DnsVerificationRequestId::new("verify-1").unwrap(),
            binding: DnsVerificationBinding {
                zone,
                zone_revision: ZoneLifecycleRevision::new("zone-r1").unwrap(),
                authoritative_nameservers: BTreeSet::from([
                    AbsoluteDnsName::new("ns1.example.net").unwrap(),
                    AbsoluteDnsName::new("ns2.example.net").unwrap(),
                ]),
                record_revision: Some(DnsRecordRevision::new("record-r1").unwrap()),
                provider_change_id: Some(DnsChangeId::new("change-1").unwrap()),
            },
            scope: DnsVerificationScope::Public {
                authority_profile: profile("authority"),
                recursive_profiles: BTreeSet::from([profile("public-a")]),
            },
            expectation: DnsRrsetExpectation::Present {
                key: DnsRecordSetKey {
                    owner: DnsOwnerName::new("www.example.com").unwrap(),
                    record_type: ProviderDnsRecordType::A,
                    routing: DnsRoutingIdentity::Simple,
                },
                values: BTreeSet::from([DnsRecordSetValue::A {
                    address: Ipv4Addr::new(192, 0, 2, 10),
                }]),
            },
            dnssec: DnssecVerificationExpectation::NotRequested,
            policy: DnsVerificationPolicy {
                total_timeout_ms: 30_000,
                per_query_timeout_ms: 2_000,
                max_attempts: 3,
                max_queries: 32,
                retry_initial_ms: 100,
                retry_max_ms: 1_000,
                evidence_max_age_ms: 60_000,
            },
        }
    }

    fn matching_evidence(request: &DnsVerificationRequest) -> DnsVerificationEvidence {
        let match_outcome = DnsQueryOutcome::Match {
            values: match &request.expectation {
                DnsRrsetExpectation::Present { values, .. } => values.clone(),
                DnsRrsetExpectation::Absent { .. } => unreachable!(),
            },
            ttl_seconds: Some(60),
            authoritative: true,
        };
        DnsVerificationEvidence {
            request_id: request.request_id.clone(),
            binding: request.binding.clone(),
            expectation: request.expectation.clone(),
            started_at_unix_ms: 1_000,
            completed_at_unix_ms: 2_000,
            authority_profile: request.scope.authority_profile().cloned(),
            authoritative: request
                .scope
                .authoritative_nameservers(&request.binding)
                .iter()
                .cloned()
                .map(|nameserver| NameserverCheck {
                    nameserver,
                    attempts: 1,
                    outcome: match_outcome.clone(),
                })
                .collect(),
            recursive: scope_profiles(&request.scope)
                .into_iter()
                .map(|profile| RecursiveResolverCheck {
                    profile,
                    attempts: 1,
                    outcome: match_outcome.clone(),
                })
                .collect(),
            delegation: DelegationObservation {
                state: DelegationState::Delegated,
                expected_nameservers: request
                    .scope
                    .authoritative_nameservers(&request.binding)
                    .clone(),
                parent_nameservers: request
                    .scope
                    .authoritative_nameservers(&request.binding)
                    .clone(),
                checked_at: Some("2000".into()),
                failure: None,
            },
            dnssec: DnssecVerificationEvidence {
                observed_parent_ds: vec![],
                parent_source: DnssecEvidenceSource::NotChecked,
                parent_soa_present_for_absence: false,
                validation: DnssecValidationState::NotRequested,
                validator_profile: None,
            },
            budget: DnsVerificationBudgetUse {
                queries: 3,
                attempts: 3,
                elapsed_ms: 1_000,
                exhausted: false,
            },
        }
    }

    #[test]
    fn exact_fresh_evidence_proves_authority() {
        let request = request();
        let evidence = matching_evidence(&request);
        assert!(evidence.validate_against(&request, 2_100).is_ok());
        assert!(evidence.authoritative_published(&request));
        assert!(evidence.recursive_visible(&request));
        let zone_evidence = evidence
            .to_zone_authority_evidence(&request, 2_100)
            .unwrap();
        assert_eq!(
            zone_evidence.observed_revision,
            request.binding.zone_revision
        );
    }

    #[test]
    fn stale_or_rebound_evidence_fails_closed() {
        let request = request();
        let mut evidence = matching_evidence(&request);
        assert!(evidence.validate_against(&request, 100_000).is_err());
        evidence.binding.zone_revision = ZoneLifecycleRevision::new("zone-r2").unwrap();
        assert!(evidence.validate_against(&request, 2_100).is_err());
    }

    #[test]
    fn resolver_profile_revision_is_part_of_the_evidence_fence() {
        let request = request();
        let mut evidence = matching_evidence(&request);
        evidence.authority_profile.as_mut().unwrap().revision =
            ResolverProfileRevision::new("authority-r2").unwrap();
        assert!(evidence.validate_against(&request, 2_100).is_err());

        let mut evidence = matching_evidence(&request);
        evidence.recursive[0].profile.revision =
            ResolverProfileRevision::new("public-a-r2").unwrap();
        assert!(evidence.validate_against(&request, 2_100).is_err());

        let mut evidence = matching_evidence(&request);
        evidence.dnssec.validator_profile = request.scope.authority_profile().cloned();
        assert!(evidence.validate_against(&request, 2_100).is_err());
    }

    #[test]
    fn one_nameserver_mismatch_prevents_publication() {
        let request = request();
        let mut evidence = matching_evidence(&request);
        evidence.authoritative[0].outcome = DnsQueryOutcome::Timeout;
        assert!(!evidence.authoritative_published(&request));
        assert!(evidence
            .to_zone_authority_evidence(&request, 2_100)
            .is_err());
    }

    #[test]
    fn recursive_mismatch_prevents_public_zone_readiness() {
        let request = request();
        let mut evidence = matching_evidence(&request);
        evidence.recursive[0].outcome = DnsQueryOutcome::Mismatch {
            values: BTreeSet::from([DnsRecordSetValue::A {
                address: Ipv4Addr::new(192, 0, 2, 99),
            }]),
        };
        assert!(evidence.authoritative_published(&request));
        assert!(!evidence.recursive_visible(&request));
        assert!(evidence
            .to_zone_authority_evidence(&request, 2_100)
            .is_err());
    }

    #[test]
    fn absence_requires_authoritative_negative_proof_with_soa() {
        let mut request = request();
        let mut evidence = matching_evidence(&request);
        request.expectation = DnsRrsetExpectation::Absent {
            key: request.expectation.key().clone(),
        };
        evidence.expectation = request.expectation.clone();
        evidence.binding = request.binding.clone();
        for check in &mut evidence.authoritative {
            check.outcome = DnsQueryOutcome::NoData {
                authoritative: true,
                soa_present: true,
            };
        }
        assert!(evidence.authoritative_published(&request));
        evidence.authoritative[0].outcome = DnsQueryOutcome::NoData {
            authoritative: true,
            soa_present: false,
        };
        assert!(!evidence.authoritative_published(&request));
    }

    #[test]
    fn private_scope_rejects_direct_nameserver_targets() {
        let mut request = request();
        request.binding.zone.visibility = ZoneVisibility::Private;
        request.scope = DnsVerificationScope::Private {
            resolver_profile: profile("corp"),
        };
        assert!(request.validate().is_err());
        request.binding.authoritative_nameservers.clear();
        assert!(request.validate().is_ok());
        request.dnssec = DnssecVerificationExpectation::Unsigned {
            require_parent_ds_absent: true,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn private_zone_readiness_uses_only_its_bound_resolver_profile() {
        let public_request = request();
        let mut request = request();
        request.binding.zone.visibility = ZoneVisibility::Private;
        request.binding.authoritative_nameservers.clear();
        request.scope = DnsVerificationScope::Private {
            resolver_profile: profile("corp"),
        };
        let values = match &request.expectation {
            DnsRrsetExpectation::Present { values, .. } => values.clone(),
            DnsRrsetExpectation::Absent { .. } => unreachable!(),
        };
        let mut evidence = matching_evidence(&public_request);
        evidence.binding = request.binding.clone();
        evidence.authority_profile = None;
        evidence.authoritative.clear();
        evidence.recursive = vec![RecursiveResolverCheck {
            profile: profile("corp"),
            attempts: 1,
            outcome: DnsQueryOutcome::Match {
                values,
                ttl_seconds: Some(60),
                authoritative: false,
            },
        }];
        evidence.delegation = DelegationObservation {
            state: DelegationState::NotApplicable,
            expected_nameservers: BTreeSet::new(),
            parent_nameservers: BTreeSet::new(),
            checked_at: Some("2000".into()),
            failure: None,
        };
        assert!(evidence.to_zone_authority_evidence(&request, 2_100).is_ok());
    }

    #[test]
    fn budget_must_cover_worst_case_attempts() {
        let mut request = request();
        request.policy.max_queries = 8;
        assert!(request.validate().is_err());
    }

    #[test]
    fn delegated_child_uses_child_nameservers_but_cannot_ready_parent_zone() {
        let mut request = request();
        let child_nameservers = BTreeSet::from([
            AbsoluteDnsName::new("ns1.validation.example.net").unwrap(),
            AbsoluteDnsName::new("ns2.validation.example.net").unwrap(),
        ]);
        request.scope = DnsVerificationScope::DelegatedValidation {
            child_apex: AbsoluteDnsName::new("validation.example.com").unwrap(),
            child_nameservers: child_nameservers.clone(),
            authority_profile: profile("authority"),
            recursive_profiles: BTreeSet::from([profile("public-a")]),
        };
        request.expectation = DnsRrsetExpectation::Present {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("token.validation.example.com").unwrap(),
                record_type: ProviderDnsRecordType::A,
                routing: DnsRoutingIdentity::Simple,
            },
            values: BTreeSet::from([DnsRecordSetValue::A {
                address: Ipv4Addr::new(192, 0, 2, 20),
            }]),
        };
        assert!(request.validate().is_ok());
        let evidence = matching_evidence(&request);
        let covered: BTreeSet<_> = evidence
            .authoritative
            .iter()
            .map(|check| check.nameserver.clone())
            .collect();
        assert_eq!(covered, child_nameservers);
        assert!(evidence.validate_against(&request, 2_100).is_ok());
        assert!(evidence
            .to_zone_authority_evidence(&request, 2_100)
            .is_err());

        let mut observation = crate::cloud::ZoneLifecycleObservation {
            zone: request.binding.zone.clone(),
            revision: request.binding.zone_revision.clone(),
            authoritative_nameservers: request.binding.authoritative_nameservers.clone(),
            delegation: DelegationObservation {
                state: DelegationState::Absent,
                expected_nameservers: request.binding.authoritative_nameservers.clone(),
                parent_nameservers: BTreeSet::new(),
                checked_at: None,
                failure: None,
            },
            authoritative_verification: crate::cloud::AuthoritativeDnsVerification::NotChecked,
            readiness: crate::cloud::ZoneReadiness::AwaitingAuthoritativeVerification,
            dnssec: crate::cloud::DnssecObservation {
                state: crate::cloud::DnssecProviderState::Disabled,
                ds_records: Vec::new(),
                external_action: crate::cloud::DnssecExternalAction::None,
                provider_detail: None,
            },
            non_default_record_count: 0,
        };
        assert!(
            apply_dns_verification_evidence(&mut observation, &request, &evidence, 2_100).is_err()
        );
        assert_eq!(
            observation.readiness,
            crate::cloud::ZoneReadiness::AwaitingAuthoritativeVerification
        );
    }

    #[test]
    fn delegated_child_requires_explicit_nameservers() {
        let mut request = request();
        request.scope = DnsVerificationScope::DelegatedValidation {
            child_apex: AbsoluteDnsName::new("validation.example.com").unwrap(),
            child_nameservers: BTreeSet::new(),
            authority_profile: profile("authority"),
            recursive_profiles: BTreeSet::from([profile("public-a")]),
        };
        request.expectation = DnsRrsetExpectation::Absent {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("token.validation.example.com").unwrap(),
                record_type: ProviderDnsRecordType::Txt,
                routing: DnsRoutingIdentity::Simple,
            },
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn signed_dnssec_requires_exact_ds_and_local_validation() {
        let mut request = request();
        let ds = DnssecDsRecord {
            key_tag: 42,
            algorithm: 13,
            digest_type: 2,
            digest: "AA".repeat(32),
        };
        request.dnssec = DnssecVerificationExpectation::Signed {
            expected_ds: vec![ds.clone()],
        };
        let mut evidence = matching_evidence(&request);
        evidence.dnssec = DnssecVerificationEvidence {
            observed_parent_ds: vec![ds],
            parent_source: DnssecEvidenceSource::LocallyValidatedRecursive,
            parent_soa_present_for_absence: false,
            validation: DnssecValidationState::SecureLocalChain,
            validator_profile: request.scope.authority_profile().cloned(),
        };
        assert!(evidence.to_zone_authority_evidence(&request, 2_100).is_ok());
        evidence.dnssec.validator_profile.as_mut().unwrap().revision =
            ResolverProfileRevision::new("authority-r2").unwrap();
        assert!(evidence
            .to_zone_authority_evidence(&request, 2_100)
            .is_err());
        evidence.dnssec.validator_profile = request.scope.authority_profile().cloned();
        evidence.dnssec.parent_source = DnssecEvidenceSource::DirectParentAuthoritative;
        assert!(evidence
            .to_zone_authority_evidence(&request, 2_100)
            .is_err());
        evidence.dnssec.parent_source = DnssecEvidenceSource::LocallyValidatedRecursive;
        evidence.dnssec.validation = DnssecValidationState::Indeterminate;
        assert!(evidence
            .to_zone_authority_evidence(&request, 2_100)
            .is_err());
    }

    #[test]
    fn unsigned_dnssec_never_accepts_indeterminate_validation() {
        let mut request = request();
        request.dnssec = DnssecVerificationExpectation::Unsigned {
            require_parent_ds_absent: false,
        };
        let mut evidence = matching_evidence(&request);
        evidence.dnssec.validation = DnssecValidationState::Indeterminate;
        evidence.dnssec.validator_profile = request.scope.authority_profile().cloned();
        assert!(evidence
            .to_zone_authority_evidence(&request, 2_100)
            .is_err());
        evidence.dnssec.validation = DnssecValidationState::Insecure;
        assert!(evidence.to_zone_authority_evidence(&request, 2_100).is_ok());
    }
}
