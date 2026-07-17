//! Provider-neutral managed-zone lifecycle contract.
//!
//! Parent-zone and registrar changes are deliberately outside this port. The
//! contract reports NS and DS work as external actions; it never implies that
//! those actions were performed.

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    AbsoluteDnsName, CloudProvider, CloudResourceId, DeletionPolicy, DnsZoneRef, IdempotencyKey,
    ManagementPolicy, NormalizedProviderError, ZoneVisibility,
};
use crate::{CoreError, CoreResult};

pub type ZoneLifecycleProviderResult<T> = Result<T, NormalizedProviderError>;

macro_rules! opaque_id {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> CoreResult<Self> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 1024
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

opaque_id!(ZoneLifecycleRevision, "zone lifecycle revision");
opaque_id!(ZoneLifecycleMutationId, "zone lifecycle mutation ID");

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneOrigin {
    #[default]
    Imported,
    CenterCreated,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnssecDesiredState {
    #[default]
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnssecDsRecord {
    pub key_tag: u16,
    pub algorithm: u8,
    pub digest_type: u8,
    /// Upper-case hexadecimal without whitespace.
    pub digest: String,
}

impl DnssecDsRecord {
    pub fn validate(&self) -> CoreResult<()> {
        let expected_digest_length = match self.digest_type {
            1 => 40,
            2 => 64,
            4 => 96,
            _ => {
                return Err(CoreError::Conflict(
                    "DNSSEC DS digest type is unsupported".into(),
                ));
            }
        };
        if self.digest.len() != expected_digest_length
            || !self.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.digest.bytes().any(|byte| byte.is_ascii_lowercase())
        {
            return Err(CoreError::Conflict(
                "DNSSEC DS digest must be upper-case hexadecimal".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnssecProviderState {
    Unsupported,
    Disabled,
    Enabling,
    AwaitingDs,
    Active,
    Disabling,
    AwaitingDsRemoval,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnssecExternalAction {
    PublishDs { records: Vec<DnssecDsRecord> },
    RemoveDs { key_tags: BTreeSet<u16> },
    WaitForProviderActivation,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnssecObservation {
    pub state: DnssecProviderState,
    #[serde(default)]
    pub ds_records: Vec<DnssecDsRecord>,
    pub external_action: DnssecExternalAction,
    /// Sanitized provider state useful for diagnosis; never a raw response.
    pub provider_detail: Option<String>,
}

/// Reinterprets provider signing state against Center intent without claiming
/// that a registrar or parent-zone mutation occurred.
pub fn dnssec_transition_for_intent(
    desired: DnssecDesiredState,
    observed: &DnssecObservation,
) -> DnssecObservation {
    if desired == DnssecDesiredState::Disabled
        && !matches!(
            observed.state,
            DnssecProviderState::Disabled | DnssecProviderState::Unsupported
        )
    {
        let key_tags: BTreeSet<_> = observed
            .ds_records
            .iter()
            .map(|record| record.key_tag)
            .collect();
        let mut transition = observed.clone();
        transition.state = DnssecProviderState::AwaitingDsRemoval;
        transition.external_action = if key_tags.is_empty() {
            DnssecExternalAction::WaitForProviderActivation
        } else {
            DnssecExternalAction::RemoveDs { key_tags }
        };
        transition
    } else {
        observed.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationState {
    NotApplicable,
    NotChecked,
    Absent,
    Partial,
    Delegated,
    Mismatch,
    CheckFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationObservation {
    pub state: DelegationState,
    #[serde(default)]
    pub expected_nameservers: BTreeSet<AbsoluteDnsName>,
    #[serde(default)]
    pub parent_nameservers: BTreeSet<AbsoluteDnsName>,
    pub checked_at: Option<String>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoritativeDnsVerification {
    NotChecked,
    Verified { checked_at: String },
    Failed { checked_at: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneReadiness {
    AwaitingAuthoritativeVerification,
    Ready,
    VerificationFailed,
}

pub fn evaluate_zone_readiness(verification: &AuthoritativeDnsVerification) -> ZoneReadiness {
    match verification {
        AuthoritativeDnsVerification::NotChecked => {
            ZoneReadiness::AwaitingAuthoritativeVerification
        }
        AuthoritativeDnsVerification::Verified { .. } => ZoneReadiness::Ready,
        AuthoritativeDnsVerification::Failed { .. } => ZoneReadiness::VerificationFailed,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneLifecycleObservation {
    pub zone: DnsZoneRef,
    pub revision: ZoneLifecycleRevision,
    #[serde(default)]
    pub authoritative_nameservers: BTreeSet<AbsoluteDnsName>,
    pub delegation: DelegationObservation,
    pub authoritative_verification: AuthoritativeDnsVerification,
    pub readiness: ZoneReadiness,
    pub dnssec: DnssecObservation,
    /// Records other than provider-required apex NS and SOA records.
    pub non_default_record_count: u64,
}

/// Evidence produced independently from provider control-plane status. CLD-14
/// may supply a network resolver implementation; tests and other compositions
/// can inject an implementation without coupling it to a cloud adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneAuthorityEvidence {
    pub zone: DnsZoneRef,
    pub observed_revision: ZoneLifecycleRevision,
    pub delegation: DelegationObservation,
    pub authoritative_verification: AuthoritativeDnsVerification,
}

#[async_trait]
pub trait ZoneAuthorityVerifier: Send + Sync {
    async fn verify(
        &self,
        zone: &DnsZoneRef,
        authoritative_nameservers: &BTreeSet<AbsoluteDnsName>,
    ) -> ZoneLifecycleProviderResult<ZoneAuthorityEvidence>;
}

pub fn apply_zone_authority_evidence(
    observation: &mut ZoneLifecycleObservation,
    evidence: ZoneAuthorityEvidence,
) -> CoreResult<()> {
    if evidence.delegation.expected_nameservers != observation.authoritative_nameservers {
        return Err(CoreError::Conflict(
            "authority evidence nameservers do not match the observed zone".into(),
        ));
    }
    if evidence.zone != observation.zone || evidence.observed_revision != observation.revision {
        return Err(CoreError::Conflict(
            "authority evidence does not match the observed zone revision".into(),
        ));
    }
    observation.delegation = evidence.delegation;
    observation.authoritative_verification = evidence.authoritative_verification;
    observation.readiness = evaluate_zone_readiness(&observation.authoritative_verification);
    observation.validate()
}

impl ZoneLifecycleObservation {
    pub fn validate(&self) -> CoreResult<()> {
        self.zone.validate()?;
        if self.readiness != evaluate_zone_readiness(&self.authoritative_verification) {
            return Err(CoreError::Conflict(
                "zone readiness lacks matching authoritative verification".into(),
            ));
        }
        match &self.authoritative_verification {
            AuthoritativeDnsVerification::Verified { checked_at }
            | AuthoritativeDnsVerification::Failed { checked_at, .. }
                if checked_at.is_empty()
                    || checked_at.len() > 128
                    || checked_at.chars().any(char::is_control) =>
            {
                return Err(CoreError::Conflict(
                    "authoritative verification timestamp is invalid".into(),
                ));
            }
            _ => {}
        }
        match self.zone.visibility {
            ZoneVisibility::Public => {
                if self.authoritative_nameservers.is_empty()
                    || self.delegation.state == DelegationState::NotApplicable
                    || self.delegation.expected_nameservers != self.authoritative_nameservers
                {
                    return Err(CoreError::Conflict(
                        "public zone lifecycle has invalid delegation evidence".into(),
                    ));
                }
                if self.readiness == ZoneReadiness::Ready
                    && (self.delegation.state != DelegationState::Delegated
                        || self.delegation.parent_nameservers != self.authoritative_nameservers)
                {
                    return Err(CoreError::Conflict(
                        "public zone cannot be ready without exact parent delegation".into(),
                    ));
                }
            }
            ZoneVisibility::Private => {
                if self.delegation.state != DelegationState::NotApplicable
                    || !self.delegation.parent_nameservers.is_empty()
                {
                    return Err(CoreError::Conflict(
                        "private zone cannot claim parent delegation".into(),
                    ));
                }
            }
        }
        for record in &self.dnssec.ds_records {
            record.validate()?;
        }
        match &self.dnssec.external_action {
            DnssecExternalAction::PublishDs { records } => {
                if records.is_empty() {
                    return Err(CoreError::Conflict(
                        "DS publication action requires records".into(),
                    ));
                }
                for record in records {
                    record.validate()?;
                }
            }
            DnssecExternalAction::RemoveDs { key_tags } if key_tags.is_empty() => {
                return Err(CoreError::Conflict(
                    "DS removal action requires key tags".into(),
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneCreationRequest {
    pub provider_account_id: CloudResourceId,
    pub provider: CloudProvider,
    pub apex: AbsoluteDnsName,
    pub visibility: ZoneVisibility,
    pub idempotency_key: IdempotencyKey,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneDeletionBlocker {
    ImportedZone,
    ObserveOnly,
    RetainPolicy,
    NonEmptyZone,
    DelegatedZone,
    DnssecEnabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneDeletionPlan {
    pub zone: DnsZoneRef,
    pub observed_revision: ZoneLifecycleRevision,
    pub origin: ZoneOrigin,
    pub management_policy: ManagementPolicy,
    pub deletion_policy: DeletionPolicy,
    pub non_default_record_count: u64,
    pub delegation_state: DelegationState,
    pub dnssec_state: DnssecProviderState,
    #[serde(default)]
    pub blockers: BTreeSet<ZoneDeletionBlocker>,
}

impl ZoneDeletionPlan {
    pub fn from_observation(
        observation: &ZoneLifecycleObservation,
        origin: ZoneOrigin,
        management_policy: ManagementPolicy,
        deletion_policy: DeletionPolicy,
    ) -> Self {
        let mut blockers = BTreeSet::new();
        if origin == ZoneOrigin::Imported {
            blockers.insert(ZoneDeletionBlocker::ImportedZone);
        }
        if management_policy != ManagementPolicy::Managed {
            blockers.insert(ZoneDeletionBlocker::ObserveOnly);
        }
        if deletion_policy != DeletionPolicy::DeleteExternal {
            blockers.insert(ZoneDeletionBlocker::RetainPolicy);
        }
        if observation.non_default_record_count != 0 {
            blockers.insert(ZoneDeletionBlocker::NonEmptyZone);
        }
        // A failed or missing parent check is not evidence that deletion is
        // safe. Public zones must be observed absent from the parent first.
        if !matches!(
            observation.delegation.state,
            DelegationState::Absent | DelegationState::NotApplicable
        ) {
            blockers.insert(ZoneDeletionBlocker::DelegatedZone);
        }
        if !matches!(
            observation.dnssec.state,
            DnssecProviderState::Disabled | DnssecProviderState::Unsupported
        ) {
            blockers.insert(ZoneDeletionBlocker::DnssecEnabled);
        }
        Self {
            zone: observation.zone.clone(),
            observed_revision: observation.revision.clone(),
            origin,
            management_policy,
            deletion_policy,
            non_default_record_count: observation.non_default_record_count,
            delegation_state: observation.delegation.state.clone(),
            dnssec_state: observation.dnssec.state.clone(),
            blockers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneDeletionAcknowledgement {
    NonEmptyZone,
    DelegatedZone,
    DnssecEnabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneDeletionApproval {
    pub approved_revision: ZoneLifecycleRevision,
    pub approved_zone: DnsZoneRef,
    pub approved_by: String,
    pub approved_at: String,
    #[serde(default)]
    pub acknowledgements: BTreeSet<ZoneDeletionAcknowledgement>,
}

/// In-process deletion capability. Its fields are private and it is not
/// deserializable, so provider adapters can only receive a request produced by
/// [`authorize_zone_deletion`]. Persist the plan and approval, then authorize
/// again after a fresh observation; never persist or accept this capability at
/// an untrusted API boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneDeletionRequest {
    zone: DnsZoneRef,
    revision: ZoneLifecycleRevision,
    approval: ZoneDeletionApproval,
}

impl ZoneDeletionRequest {
    pub fn zone(&self) -> &DnsZoneRef {
        &self.zone
    }

    pub fn revision(&self) -> &ZoneLifecycleRevision {
        &self.revision
    }

    pub fn approval(&self) -> &ZoneDeletionApproval {
        &self.approval
    }
}

pub fn authorize_zone_deletion(
    plan: &ZoneDeletionPlan,
    approval: ZoneDeletionApproval,
) -> CoreResult<ZoneDeletionRequest> {
    if approval.approved_revision != plan.observed_revision {
        return Err(CoreError::Conflict(
            "zone deletion approval is stale".into(),
        ));
    }
    if approval.approved_zone != plan.zone {
        return Err(CoreError::Conflict(
            "zone deletion approval is bound to another zone".into(),
        ));
    }
    if approval.approved_by.trim().is_empty() || approval.approved_at.trim().is_empty() {
        return Err(CoreError::Conflict(
            "zone deletion approval is incomplete".into(),
        ));
    }
    let mut expected = BTreeSet::new();
    if plan.origin == ZoneOrigin::Imported {
        expected.insert(ZoneDeletionBlocker::ImportedZone);
    }
    if plan.management_policy != ManagementPolicy::Managed {
        expected.insert(ZoneDeletionBlocker::ObserveOnly);
    }
    if plan.deletion_policy != DeletionPolicy::DeleteExternal {
        expected.insert(ZoneDeletionBlocker::RetainPolicy);
    }
    if plan.non_default_record_count != 0 {
        expected.insert(ZoneDeletionBlocker::NonEmptyZone);
    }
    if !matches!(
        plan.delegation_state,
        DelegationState::Absent | DelegationState::NotApplicable
    ) {
        expected.insert(ZoneDeletionBlocker::DelegatedZone);
    }
    if !matches!(
        plan.dnssec_state,
        DnssecProviderState::Disabled | DnssecProviderState::Unsupported
    ) {
        expected.insert(ZoneDeletionBlocker::DnssecEnabled);
    }
    if plan.blockers != expected {
        return Err(CoreError::Conflict(
            "zone deletion plan blockers do not match its observation".into(),
        ));
    }
    // Acknowledgement cannot make an unsafe DNS state safe. The operator must
    // remove records, detach delegation, and finish DNSSEC shutdown, then
    // produce and approve a fresh plan.
    if !plan.blockers.is_empty() {
        return Err(CoreError::Conflict(
            "zone deletion preconditions are not satisfied".into(),
        ));
    }
    Ok(ZoneDeletionRequest {
        zone: plan.zone.clone(),
        revision: plan.observed_revision.clone(),
        approval,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneLifecycleMutationState {
    Pending,
    Succeeded,
    Failed,
    UnknownOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneLifecycleMutationReceipt {
    pub mutation_id: ZoneLifecycleMutationId,
    pub state: ZoneLifecycleMutationState,
}

#[async_trait]
pub trait ZoneLifecycleProvider: Send + Sync {
    async fn create_zone(
        &self,
        request: &ZoneCreationRequest,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt>;
    async fn observe_zone(
        &self,
        zone: &DnsZoneRef,
    ) -> ZoneLifecycleProviderResult<Option<ZoneLifecycleObservation>>;
    async fn set_dnssec(
        &self,
        zone: &DnsZoneRef,
        desired: DnssecDesiredState,
        expected_revision: &ZoneLifecycleRevision,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt>;
    async fn delete_zone(
        &self,
        request: &ZoneDeletionRequest,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt>;
    async fn observe_mutation(
        &self,
        mutation_id: &ZoneLifecycleMutationId,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt>;
}

#[cfg(test)]
mod tests {
    use super::super::DnsZoneId;
    use super::*;

    #[test]
    fn provider_acceptance_does_not_make_zone_ready() {
        assert_eq!(
            evaluate_zone_readiness(&AuthoritativeDnsVerification::NotChecked),
            ZoneReadiness::AwaitingAuthoritativeVerification
        );
    }

    #[test]
    fn disable_intent_requires_external_ds_removal_before_provider_shutdown() {
        let observed = DnssecObservation {
            state: DnssecProviderState::AwaitingDs,
            ds_records: vec![DnssecDsRecord {
                key_tag: 42,
                algorithm: 13,
                digest_type: 2,
                digest: "AA".repeat(32),
            }],
            external_action: DnssecExternalAction::PublishDs {
                records: vec![DnssecDsRecord {
                    key_tag: 42,
                    algorithm: 13,
                    digest_type: 2,
                    digest: "AA".repeat(32),
                }],
            },
            provider_detail: None,
        };
        let transition = dnssec_transition_for_intent(DnssecDesiredState::Disabled, &observed);
        assert_eq!(transition.state, DnssecProviderState::AwaitingDsRemoval);
        assert!(
            matches!(transition.external_action, DnssecExternalAction::RemoveDs { ref key_tags } if key_tags.contains(&42))
        );
    }

    #[test]
    fn imported_and_default_retain_zones_cannot_be_deleted() {
        let blockers = [
            ZoneDeletionBlocker::ImportedZone,
            ZoneDeletionBlocker::ObserveOnly,
            ZoneDeletionBlocker::RetainPolicy,
        ]
        .into_iter()
        .collect();
        let plan = ZoneDeletionPlan {
            zone: fixture_zone(),
            observed_revision: ZoneLifecycleRevision::new("r1").unwrap(),
            origin: ZoneOrigin::Imported,
            management_policy: ManagementPolicy::ObserveOnly,
            deletion_policy: DeletionPolicy::Retain,
            non_default_record_count: 0,
            delegation_state: DelegationState::Absent,
            dnssec_state: DnssecProviderState::Disabled,
            blockers,
        };
        assert!(authorize_zone_deletion(&plan, approval("r1", [])).is_err());
    }

    #[test]
    fn dangerous_deletion_cannot_be_overridden_by_acknowledgement() {
        let blockers = [
            ZoneDeletionBlocker::NonEmptyZone,
            ZoneDeletionBlocker::DelegatedZone,
            ZoneDeletionBlocker::DnssecEnabled,
        ]
        .into_iter()
        .collect();
        let plan = ZoneDeletionPlan {
            zone: fixture_zone(),
            observed_revision: ZoneLifecycleRevision::new("r1").unwrap(),
            origin: ZoneOrigin::CenterCreated,
            management_policy: ManagementPolicy::Managed,
            deletion_policy: DeletionPolicy::DeleteExternal,
            non_default_record_count: 2,
            delegation_state: DelegationState::Delegated,
            dnssec_state: DnssecProviderState::Active,
            blockers,
        };
        let all = [
            ZoneDeletionAcknowledgement::NonEmptyZone,
            ZoneDeletionAcknowledgement::DelegatedZone,
            ZoneDeletionAcknowledgement::DnssecEnabled,
        ];
        assert!(authorize_zone_deletion(&plan, approval("r1", all.clone())).is_err());
        assert!(authorize_zone_deletion(&plan, approval("stale", all)).is_err());
    }

    #[test]
    fn safe_current_plan_still_requires_exact_approval() {
        let plan = ZoneDeletionPlan {
            zone: fixture_zone(),
            observed_revision: ZoneLifecycleRevision::new("r2").unwrap(),
            origin: ZoneOrigin::CenterCreated,
            management_policy: ManagementPolicy::Managed,
            deletion_policy: DeletionPolicy::DeleteExternal,
            non_default_record_count: 0,
            delegation_state: DelegationState::Absent,
            dnssec_state: DnssecProviderState::Disabled,
            blockers: BTreeSet::new(),
        };
        assert!(authorize_zone_deletion(&plan, approval("r2", [])).is_ok());
        assert!(authorize_zone_deletion(&plan, approval("r1", [])).is_err());
    }

    #[test]
    fn independent_authority_evidence_is_required_for_readiness() {
        let nameservers: BTreeSet<_> = [AbsoluteDnsName::new("ns1.example.net").unwrap()]
            .into_iter()
            .collect();
        let mut observation = ZoneLifecycleObservation {
            zone: fixture_zone(),
            revision: ZoneLifecycleRevision::new("r3").unwrap(),
            authoritative_nameservers: nameservers.clone(),
            delegation: DelegationObservation {
                state: DelegationState::NotChecked,
                expected_nameservers: nameservers.clone(),
                parent_nameservers: BTreeSet::new(),
                checked_at: None,
                failure: None,
            },
            authoritative_verification: AuthoritativeDnsVerification::NotChecked,
            readiness: ZoneReadiness::AwaitingAuthoritativeVerification,
            dnssec: DnssecObservation {
                state: DnssecProviderState::Disabled,
                ds_records: Vec::new(),
                external_action: DnssecExternalAction::None,
                provider_detail: None,
            },
            non_default_record_count: 0,
        };
        let evidence_zone = observation.zone.clone();
        let evidence_revision = observation.revision.clone();
        apply_zone_authority_evidence(
            &mut observation,
            ZoneAuthorityEvidence {
                zone: evidence_zone,
                observed_revision: evidence_revision,
                delegation: DelegationObservation {
                    state: DelegationState::Delegated,
                    expected_nameservers: nameservers.clone(),
                    parent_nameservers: nameservers,
                    checked_at: Some("2026-07-17T00:00:00Z".into()),
                    failure: None,
                },
                authoritative_verification: AuthoritativeDnsVerification::Verified {
                    checked_at: "2026-07-17T00:00:01Z".into(),
                },
            },
        )
        .unwrap();
        assert_eq!(observation.readiness, ZoneReadiness::Ready);
    }

    fn fixture_zone() -> DnsZoneRef {
        DnsZoneRef {
            provider_account_id: CloudResourceId::new("account").unwrap(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new("zone").unwrap(),
            apex: AbsoluteDnsName::new("example.com").unwrap(),
            visibility: ZoneVisibility::Public,
        }
    }

    fn approval<const N: usize>(
        revision: &str,
        acknowledgements: [ZoneDeletionAcknowledgement; N],
    ) -> ZoneDeletionApproval {
        ZoneDeletionApproval {
            approved_revision: ZoneLifecycleRevision::new(revision).unwrap(),
            approved_zone: fixture_zone(),
            approved_by: "operator".into(),
            approved_at: "2026-07-17T00:00:00Z".into(),
            acknowledgements: acknowledgements.into_iter().collect(),
        }
    }
}
