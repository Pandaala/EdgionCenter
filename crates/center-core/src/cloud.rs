//! Provider-neutral cloud infrastructure intent and status.
//!
//! These types deliberately do not reference Edgion Controllers, Gateway API
//! resources, provider SDKs, persistence frameworks, or arbitrary vendor JSON.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult};

mod capabilities;
#[cfg(feature = "test-support")]
mod capability_store_conformance;
mod credentials;
mod dns;
#[cfg(feature = "test-support")]
mod dns_provider_conformance;
mod dns_verification;
mod operations;
mod origin;
#[cfg(feature = "test-support")]
mod provider_account_store_conformance;
mod provider_accounts;
mod provider_errors;
mod status;
mod zone_lifecycle;

pub use capabilities::{
    validate_write, CacheCapability, CapabilityAction, CapabilityDecision,
    CapabilityDecisionOutcome, CapabilityDimension, CapabilityDimensionObservation,
    CapabilityDiscoveryFence, CapabilityDiscoveryIssue, CapabilityDiscoveryReport,
    CapabilityDiscoveryRequest, CapabilityDiscoveryState, CapabilityEvaluationContext,
    CapabilityEvidence, CapabilityGateBlocker, CapabilityGateReason, CapabilityIssueScope,
    CapabilityIssueSeverity, CapabilityObservation, CapabilityReason, CapabilityRequirement,
    CapabilityScope, CapabilitySnapshotKey, CapabilitySnapshotStore, CapabilityStoreWrite,
    CertificateCapability, DiscoveryToken, DnsCapability, EdgeCapability, HealthCheckCapability,
    ProviderCapability, ProviderCapabilityDiscoverer, ProviderCapabilitySnapshot, ProviderRegion,
    SanitizedCapabilityCode, SanitizedCapabilityMessage, TriState, WafCapability,
};
pub use credentials::{
    CredentialInspection, CredentialInspector, CredentialIssue, CredentialIssueKind,
    CredentialSource, CredentialState, ProviderIdentity,
};
pub use dns::{
    validate_dns_changes, AbsoluteDnsName, CaaTag, CloudflareCnameFlattening,
    CloudflareProxyOptions, DnsBatchAtomicity, DnsChangeId, DnsChangeReceipt, DnsChangeState,
    DnsCharacterString, DnsGuardStrength, DnsMutationGuard, DnsOwnerName, DnsPage, DnsPageRequest,
    DnsPageToken, DnsPropagationState, DnsProvider, DnsProviderResult, DnsRecordChange,
    DnsRecordExtension, DnsRecordObjectId, DnsRecordRevision, DnsRecordSetKey, DnsRecordSetValue,
    DnsRoutingIdentity, DnsTtl, DnsTxtValue, DnsZoneId, DnsZoneRef, GoogleDnsGeoPolicy,
    GoogleDnsGeoPolicyItem, GoogleDnsHealthCheckRef, GoogleDnsHealthCheckTargets,
    GoogleDnsInternalLoadBalancerTarget, GoogleDnsIpProtocol, GoogleDnsLoadBalancerType,
    GoogleDnsPolicyItemData, GoogleDnsRoutingPolicy, GoogleDnsRoutingPolicyKind,
    GoogleDnsTrickleTraffic, GoogleDnsWeight, GoogleDnsWrrPolicyItem, ObservedDnsRecordSet,
    ObservedDnsZone, ProviderDnsRecordSet, ProviderDnsRecordType, Route53AliasTarget,
    Route53FailoverRole, Route53GeoLocation, Route53HealthCheckId, Route53RoutingPolicy,
};
pub use dns_verification::{
    apply_dns_verification_evidence, DnsPropagationVerifier, DnsQueryOutcome, DnsRrsetExpectation,
    DnsVerificationBinding, DnsVerificationBudgetUse, DnsVerificationError,
    DnsVerificationErrorKind, DnsVerificationEvidence, DnsVerificationPolicy,
    DnsVerificationRequest, DnsVerificationRequestId, DnsVerificationResult, DnsVerificationScope,
    DnssecEvidenceSource, DnssecValidationState, DnssecVerificationEvidence,
    DnssecVerificationExpectation, NameserverCheck, RecursiveResolverCheck, ResolverProfileId,
    ResolverProfileRef, ResolverProfileRevision, SanitizedDnsFailureCode,
};
pub use operations::{
    ClaimedOperation, CloudOperation, CloudOperationAction, CloudOperationPhase,
    CloudOperationStep, CloudOperationStepPhase, CloudOperationStepPurpose, DispatchPolicy,
    DispatchedStep, EnqueueOperationResult, IdempotencyKey, LeaseUpdate, NewCloudOperation,
    NewCloudOperationStep, OperationError, OperationErrorKind, OperationId, OperationLease,
    OperationStore, StepCompletion, UnknownOutcomeResolution,
};
pub use origin::{
    evaluate_origin_probe, select_origin_tier, HealthCheckExpectedResponse, HealthCheckMethod,
    HealthCheckSourceRegion, HealthCheckSourceScope, HealthCheckSpec, OriginAddress,
    OriginDrainState, OriginEndpoint, OriginEndpointName, OriginFailoverMode,
    OriginHealthObservation, OriginHealthObserver, OriginHealthRequest, OriginHealthSource,
    OriginHealthState, OriginHealthTransitionPolicy, OriginPoolCapabilities, OriginProbeSample,
    OriginProtocol, OriginRequestHeaders, OriginSelection, OriginTlsMode,
};
pub use provider_accounts::{
    provider_account_from_desired, validate_stored_provider_account, ProviderAccountCreateResult,
    ProviderAccountDesired, ProviderAccountPage, ProviderAccountPageRequest,
    ProviderAccountReplaceResult, ProviderAccountStore,
};
pub use provider_errors::{NormalizedProviderError, ProviderErrorCategory};
pub use status::{BoundedCloudEventHistory, CloudCorrelationId, CloudEvent};
pub use zone_lifecycle::{
    authorize_zone_deletion, dnssec_transition_for_intent, evaluate_zone_readiness,
    AuthoritativeDnsVerification, DelegationObservation, DelegationState, DnssecDesiredState,
    DnssecDsRecord, DnssecExternalAction, DnssecObservation, DnssecProviderState,
    ZoneAuthorityEvidence, ZoneCreationRequest, ZoneDeletionAcknowledgement, ZoneDeletionApproval,
    ZoneDeletionBlocker, ZoneDeletionPlan, ZoneDeletionRequest, ZoneLifecycleMutationId,
    ZoneLifecycleMutationReceipt, ZoneLifecycleMutationState, ZoneLifecycleObservation,
    ZoneLifecycleProvider, ZoneLifecycleProviderResult, ZoneLifecycleRevision, ZoneOrigin,
    ZoneReadiness,
};

#[cfg(feature = "test-support")]
pub mod test_support {
    pub use super::capability_store_conformance::*;
    pub use super::dns_provider_conformance::*;
    pub use super::provider_account_store_conformance::*;
}

macro_rules! identifier {
    ($name:ident, $kind:literal, $max_len:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> CoreResult<Self> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > $max_len
                    || value.trim() != value
                    || value.chars().any(char::is_control)
                {
                    return Err(CoreError::InvalidIdentifier { kind: $kind, value });
                }
                Ok(Self(value))
            }

            pub fn validate(&self) -> CoreResult<()> {
                Self::new(self.0.clone()).map(|_| ())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

identifier!(CloudResourceId, "cloud resource", 512);
identifier!(CredentialRef, "credential reference", usize::MAX);

/// Canonical DNS name without a trailing dot.
///
/// Unicode is retained rather than silently converted. Provider adapters must
/// apply their required IDNA/Punycode representation at the API boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DomainName(String);

impl DomainName {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        normalize_dns_name(value.into(), "domain name", false).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Canonical DNS owner name. Unlike a public hostname, DNS owner labels may
/// contain underscores (for example `_acme-challenge.example.com`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DnsName(String);

impl DnsName {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        normalize_dns_name(value.into(), "DNS name", true).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for DnsName {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn normalize_dns_name(
    original: String,
    kind: &'static str,
    allow_underscore: bool,
) -> CoreResult<String> {
    let value = original.strip_suffix('.').unwrap_or(&original);
    let invalid = value.is_empty()
        || value.len() > 253
        || value.starts_with("*.")
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || label.chars().any(|character| {
                    !(character.is_alphanumeric()
                        || character == '-'
                        || (allow_underscore && character == '_'))
                })
        });
    if invalid {
        return Err(CoreError::InvalidIdentifier {
            kind,
            value: original,
        });
    }
    Ok(value.to_lowercase())
}

impl Display for DomainName {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudResourceKind {
    ProviderAccount,
    ManagedZone,
    DnsRecordSet,
    DomainBinding,
    CertificateBinding,
    EdgeApplication,
    OriginPool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudResourceRef {
    pub kind: CloudResourceKind,
    pub id: CloudResourceId,
}

impl CloudResourceRef {
    pub fn new(kind: CloudResourceKind, id: CloudResourceId) -> Self {
        Self { kind, id }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudProvider {
    Cloudflare,
    Aws,
    GoogleCloud,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ManagementPolicy {
    Managed,
    #[default]
    ObserveOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeletionPolicy {
    #[default]
    Retain,
    DeleteExternal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudResourceMetadata {
    pub id: CloudResourceId,
    pub display_name: String,
    pub owner: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub generation: u64,
    #[serde(default)]
    pub management_policy: ManagementPolicy,
    #[serde(default)]
    pub deletion_policy: DeletionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderResourceRef {
    pub provider_account_id: CloudResourceId,
    pub external_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudConditionStatus {
    True,
    False,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudConditionType {
    Accepted,
    CredentialsValid,
    DnsReady,
    CertificateReady,
    OriginHealthy,
    Programmed,
    DriftDetected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudCondition {
    pub condition_type: CloudConditionType,
    pub status: CloudConditionStatus,
    pub reason: String,
    pub message: String,
    pub observed_generation: u64,
    pub last_transition_unix_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudResourceStatus {
    pub observed_generation: Option<u64>,
    pub provider_resource: Option<ProviderResourceRef>,
    #[serde(default)]
    pub conditions: Vec<CloudCondition>,
}

macro_rules! resource {
    ($name:ident, $spec:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            pub metadata: CloudResourceMetadata,
            pub spec: $spec,
            #[serde(default)]
            pub status: CloudResourceStatus,
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccountSpec {
    pub provider: CloudProvider,
    /// Provider-native account boundary. Older observe-only resources may omit
    /// it, but concrete mutation adapters require the matching typed scope.
    #[serde(default)]
    pub scope: Option<ProviderAccountScope>,
    pub credential_source: CredentialSource,
}

impl ProviderAccountSpec {
    pub fn validate(&self) -> CoreResult<()> {
        self.credential_source.validate()?;
        if let Some(scope) = self.scope.as_ref() {
            scope.validate_for(&self.provider)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ProviderAccountScope {
    Cloudflare { account_id: String },
    Aws { account_id: String },
    GoogleCloud { project_id: String },
}

impl ProviderAccountScope {
    pub fn validate_for(&self, provider: &CloudProvider) -> CoreResult<()> {
        let (matches, valid, kind) = match self {
            Self::Cloudflare { account_id } => (
                provider == &CloudProvider::Cloudflare,
                account_id.len() == 32
                    && account_id
                        .bytes()
                        .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value)),
                "Cloudflare account ID",
            ),
            Self::Aws { account_id } => (
                provider == &CloudProvider::Aws,
                account_id.len() == 12 && account_id.chars().all(|value| value.is_ascii_digit()),
                "AWS account ID",
            ),
            Self::GoogleCloud { project_id } => (
                provider == &CloudProvider::GoogleCloud,
                (6..=30).contains(&project_id.len())
                    && project_id.starts_with(|value: char| value.is_ascii_lowercase())
                    && project_id.ends_with(|value: char| value.is_ascii_alphanumeric())
                    && project_id.chars().all(|value| {
                        value.is_ascii_lowercase() || value.is_ascii_digit() || value == '-'
                    }),
                "Google Cloud project ID",
            ),
        };
        if !matches || !valid {
            return Err(CoreError::Conflict(format!("{kind} is invalid")));
        }
        Ok(())
    }
}

resource!(ProviderAccount, ProviderAccountSpec);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneVisibility {
    Public,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedZoneSpec {
    pub provider_account_ref: CloudResourceRef,
    pub name: DomainName,
    pub visibility: ZoneVisibility,
    /// Existing serialized resources conservatively deserialize as imported.
    #[serde(default)]
    pub origin: ZoneOrigin,
    #[serde(default)]
    pub dnssec: DnssecDesiredState,
}

resource!(ManagedZone, ManagedZoneSpec);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DnsRecordType {
    A,
    Aaaa,
    Cname,
    Txt,
    Mx,
    Srv,
    Caa,
    Ns,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRecordSetSpec {
    pub zone_ref: CloudResourceRef,
    pub name: DnsName,
    pub record_type: DnsRecordType,
    #[serde(default)]
    pub values: Vec<String>,
    pub ttl_seconds: Option<u32>,
}

resource!(DnsRecordSet, DnsRecordSetSpec);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainBindingSpec {
    pub hostname: DomainName,
    pub zone_ref: CloudResourceRef,
    pub certificate_ref: Option<CloudResourceRef>,
    pub edge_application_ref: Option<CloudResourceRef>,
    pub origin_pool_ref: Option<CloudResourceRef>,
}

resource!(DomainBinding, DomainBindingSpec);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateName {
    pub domain: DomainName,
    pub wildcard: bool,
}

impl CertificateName {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        let (wildcard, domain) = match value.strip_prefix("*.") {
            Some(domain) => (true, domain.to_string()),
            None => (false, value),
        };
        Ok(Self {
            domain: DomainName::new(domain)?,
            wildcard,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CertificatePurpose {
    PublicEdge,
    OriginOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CertificateManagement {
    ProviderManaged,
    Acme,
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateBindingSpec {
    pub provider_account_ref: Option<CloudResourceRef>,
    #[serde(default)]
    pub names: Vec<CertificateName>,
    pub purpose: CertificatePurpose,
    pub management: CertificateManagement,
    pub deployment_target_ref: Option<CloudResourceRef>,
}

resource!(CertificateBinding, CertificateBindingSpec);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeApplicationSpec {
    pub provider_account_ref: CloudResourceRef,
    #[serde(default)]
    pub domains: Vec<DomainName>,
    pub origin_pool_ref: CloudResourceRef,
}

resource!(EdgeApplication, EdgeApplicationSpec);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginPoolSpec {
    pub provider_account_ref: Option<CloudResourceRef>,
    #[serde(default)]
    pub endpoints: Vec<OriginEndpoint>,
    pub health_check: Option<HealthCheckSpec>,
    #[serde(default)]
    pub failover_mode: OriginFailoverMode,
    #[serde(default = "default_minimum_healthy")]
    pub minimum_healthy: u16,
}

const fn default_minimum_healthy() -> u16 {
    1
}

resource!(OriginPool, OriginPoolSpec);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "resource", rename_all = "snake_case")]
pub enum CloudResource {
    ProviderAccount(ProviderAccount),
    ManagedZone(ManagedZone),
    DnsRecordSet(DnsRecordSet),
    DomainBinding(DomainBinding),
    CertificateBinding(CertificateBinding),
    EdgeApplication(EdgeApplication),
    OriginPool(OriginPool),
}

impl CloudResource {
    pub fn kind(&self) -> CloudResourceKind {
        match self {
            Self::ProviderAccount(_) => CloudResourceKind::ProviderAccount,
            Self::ManagedZone(_) => CloudResourceKind::ManagedZone,
            Self::DnsRecordSet(_) => CloudResourceKind::DnsRecordSet,
            Self::DomainBinding(_) => CloudResourceKind::DomainBinding,
            Self::CertificateBinding(_) => CloudResourceKind::CertificateBinding,
            Self::EdgeApplication(_) => CloudResourceKind::EdgeApplication,
            Self::OriginPool(_) => CloudResourceKind::OriginPool,
        }
    }

    pub fn metadata(&self) -> &CloudResourceMetadata {
        match self {
            Self::ProviderAccount(resource) => &resource.metadata,
            Self::ManagedZone(resource) => &resource.metadata,
            Self::DnsRecordSet(resource) => &resource.metadata,
            Self::DomainBinding(resource) => &resource.metadata,
            Self::CertificateBinding(resource) => &resource.metadata,
            Self::EdgeApplication(resource) => &resource.metadata,
            Self::OriginPool(resource) => &resource.metadata,
        }
    }

    pub fn status(&self) -> &CloudResourceStatus {
        match self {
            Self::ProviderAccount(resource) => &resource.status,
            Self::ManagedZone(resource) => &resource.status,
            Self::DnsRecordSet(resource) => &resource.status,
            Self::DomainBinding(resource) => &resource.status,
            Self::CertificateBinding(resource) => &resource.status,
            Self::EdgeApplication(resource) => &resource.status,
            Self::OriginPool(resource) => &resource.status,
        }
    }

    pub fn validate(&self) -> CoreResult<()> {
        let metadata = self.metadata();
        if metadata.display_name.trim().is_empty() || metadata.generation == 0 {
            return Err(CoreError::Conflict(format!(
                "cloud resource {} has invalid metadata",
                metadata.id
            )));
        }
        self.status().validate(metadata.generation)?;

        match self {
            Self::ProviderAccount(resource) => resource.spec.validate(),
            Self::ManagedZone(resource) => require_kind(
                &resource.spec.provider_account_ref,
                CloudResourceKind::ProviderAccount,
                &metadata.id,
            ),
            Self::DnsRecordSet(resource) => {
                require_kind(
                    &resource.spec.zone_ref,
                    CloudResourceKind::ManagedZone,
                    &metadata.id,
                )?;
                if resource.spec.values.is_empty() || resource.spec.ttl_seconds == Some(0) {
                    return Err(CoreError::Conflict(format!(
                        "DNS record set {} must have values and a positive TTL",
                        metadata.id
                    )));
                }
                Ok(())
            }
            Self::DomainBinding(resource) => {
                require_kind(
                    &resource.spec.zone_ref,
                    CloudResourceKind::ManagedZone,
                    &metadata.id,
                )?;
                require_optional_kind(
                    resource.spec.certificate_ref.as_ref(),
                    CloudResourceKind::CertificateBinding,
                    &metadata.id,
                )?;
                require_optional_kind(
                    resource.spec.edge_application_ref.as_ref(),
                    CloudResourceKind::EdgeApplication,
                    &metadata.id,
                )?;
                require_optional_kind(
                    resource.spec.origin_pool_ref.as_ref(),
                    CloudResourceKind::OriginPool,
                    &metadata.id,
                )
            }
            Self::CertificateBinding(resource) => {
                require_optional_kind(
                    resource.spec.provider_account_ref.as_ref(),
                    CloudResourceKind::ProviderAccount,
                    &metadata.id,
                )?;
                require_optional_kind(
                    resource.spec.deployment_target_ref.as_ref(),
                    CloudResourceKind::EdgeApplication,
                    &metadata.id,
                )?;
                if resource.spec.names.is_empty() {
                    return Err(CoreError::Conflict(format!(
                        "certificate binding {} must contain at least one name",
                        metadata.id
                    )));
                }
                Ok(())
            }
            Self::EdgeApplication(resource) => {
                require_kind(
                    &resource.spec.provider_account_ref,
                    CloudResourceKind::ProviderAccount,
                    &metadata.id,
                )?;
                require_kind(
                    &resource.spec.origin_pool_ref,
                    CloudResourceKind::OriginPool,
                    &metadata.id,
                )
            }
            Self::OriginPool(resource) => {
                require_optional_kind(
                    resource.spec.provider_account_ref.as_ref(),
                    CloudResourceKind::ProviderAccount,
                    &metadata.id,
                )?;
                resource.spec.validate().map_err(|error| {
                    CoreError::Conflict(format!("origin pool {} is invalid: {error}", metadata.id))
                })
            }
        }
    }

    fn references(&self) -> Vec<&CloudResourceRef> {
        match self {
            Self::ProviderAccount(_) => Vec::new(),
            Self::ManagedZone(resource) => vec![&resource.spec.provider_account_ref],
            Self::DnsRecordSet(resource) => vec![&resource.spec.zone_ref],
            Self::DomainBinding(resource) => {
                let mut references = vec![&resource.spec.zone_ref];
                references.extend(resource.spec.certificate_ref.iter());
                references.extend(resource.spec.edge_application_ref.iter());
                references.extend(resource.spec.origin_pool_ref.iter());
                references
            }
            Self::CertificateBinding(resource) => {
                let mut references = Vec::new();
                references.extend(resource.spec.provider_account_ref.iter());
                references.extend(resource.spec.deployment_target_ref.iter());
                references
            }
            Self::EdgeApplication(resource) => vec![
                &resource.spec.provider_account_ref,
                &resource.spec.origin_pool_ref,
            ],
            Self::OriginPool(resource) => resource.spec.provider_account_ref.iter().collect(),
        }
    }
}

fn require_optional_kind(
    reference: Option<&CloudResourceRef>,
    expected: CloudResourceKind,
    source: &CloudResourceId,
) -> CoreResult<()> {
    match reference {
        Some(reference) => require_kind(reference, expected, source),
        None => Ok(()),
    }
}

fn require_kind(
    reference: &CloudResourceRef,
    expected: CloudResourceKind,
    source: &CloudResourceId,
) -> CoreResult<()> {
    if reference.kind != expected {
        return Err(CoreError::Conflict(format!(
            "cloud resource {source} requires a {expected:?} reference, got {:?}",
            reference.kind
        )));
    }
    Ok(())
}

/// A validated in-memory resource graph used by planners and contract tests.
#[derive(Debug, Clone, Default)]
pub struct CloudResourceSet {
    resources: BTreeMap<CloudResourceId, CloudResource>,
}

impl CloudResourceSet {
    pub fn new(resources: impl IntoIterator<Item = CloudResource>) -> CoreResult<Self> {
        let mut set = Self::default();
        for resource in resources {
            resource.validate()?;
            let id = resource.metadata().id.clone();
            if set.resources.insert(id.clone(), resource).is_some() {
                return Err(CoreError::Conflict(format!(
                    "duplicate cloud resource id {id}"
                )));
            }
        }
        set.validate_references()?;
        Ok(set)
    }

    pub fn get(&self, id: &CloudResourceId) -> Option<&CloudResource> {
        self.resources.get(id)
    }

    pub fn len(&self) -> usize {
        self.resources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    pub fn reverse_references(&self, id: &CloudResourceId) -> BTreeSet<CloudResourceId> {
        self.resources
            .values()
            .filter(|resource| {
                resource
                    .references()
                    .iter()
                    .any(|reference| &reference.id == id)
            })
            .map(|resource| resource.metadata().id.clone())
            .collect()
    }

    pub fn validate_references(&self) -> CoreResult<()> {
        for resource in self.resources.values() {
            for reference in resource.references() {
                let target = self.resources.get(&reference.id).ok_or_else(|| {
                    CoreError::NotFound(format!(
                        "cloud resource {} references missing {}",
                        resource.metadata().id,
                        reference.id
                    ))
                })?;
                let actual = target.kind();
                if actual != reference.kind {
                    return Err(CoreError::Conflict(format!(
                        "cloud resource {} expects {:?} reference {}, found {:?}",
                        resource.metadata().id,
                        reference.kind,
                        reference.id,
                        actual
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(id: &str) -> CloudResourceMetadata {
        CloudResourceMetadata {
            id: CloudResourceId::new(id).unwrap(),
            display_name: id.to_string(),
            owner: Some("platform".to_string()),
            labels: BTreeMap::new(),
            generation: 1,
            management_policy: ManagementPolicy::Managed,
            deletion_policy: DeletionPolicy::Retain,
        }
    }

    fn reference(kind: CloudResourceKind, id: &str) -> CloudResourceRef {
        CloudResourceRef::new(kind, CloudResourceId::new(id).unwrap())
    }

    fn provider_account() -> CloudResource {
        CloudResource::ProviderAccount(ProviderAccount {
            metadata: metadata("provider-main"),
            spec: ProviderAccountSpec {
                provider: CloudProvider::Cloudflare,
                scope: Some(ProviderAccountScope::Cloudflare {
                    account_id: "023e105f4ecef8ad9ca31a8372d0c353".to_string(),
                }),
                credential_source: CredentialSource::StaticSecret {
                    credential_ref: CredentialRef::new("secret/cloudflare-main").unwrap(),
                },
            },
            status: CloudResourceStatus::default(),
        })
    }

    fn zone() -> CloudResource {
        CloudResource::ManagedZone(ManagedZone {
            metadata: metadata("zone-example"),
            spec: ManagedZoneSpec {
                provider_account_ref: reference(
                    CloudResourceKind::ProviderAccount,
                    "provider-main",
                ),
                name: DomainName::new("Example.COM.").unwrap(),
                visibility: ZoneVisibility::Public,
                origin: ZoneOrigin::Imported,
                dnssec: DnssecDesiredState::Disabled,
            },
            status: CloudResourceStatus::default(),
        })
    }

    #[test]
    fn domain_names_are_canonical_and_wildcards_are_explicit() {
        assert_eq!(
            DomainName::new("API.Example.COM.").unwrap().as_str(),
            "api.example.com"
        );
        assert!(DomainName::new("*.example.com").is_err());
        assert!(DomainName::new("broken..example.com").is_err());
        assert!(DomainName::new("https://example.com").is_err());
        assert_eq!(
            DnsName::new("_acme-challenge.Example.com.")
                .unwrap()
                .as_str(),
            "_acme-challenge.example.com"
        );

        let wildcard = CertificateName::new("*.Example.com").unwrap();
        assert!(wildcard.wildcard);
        assert_eq!(wildcard.domain.as_str(), "example.com");
    }

    #[test]
    fn lifecycle_defaults_are_conservative() {
        assert_eq!(ManagementPolicy::default(), ManagementPolicy::ObserveOnly);
        assert_eq!(DeletionPolicy::default(), DeletionPolicy::Retain);

        let json = serde_json::json!({
            "id": "zone-example",
            "displayName": "Example",
            "owner": null,
            "generation": 1
        });
        let decoded: CloudResourceMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.management_policy, ManagementPolicy::ObserveOnly);
        assert_eq!(decoded.deletion_policy, DeletionPolicy::Retain);
    }

    #[test]
    fn legacy_zone_intent_defaults_to_imported_and_dnssec_disabled() {
        let spec: ManagedZoneSpec = serde_json::from_value(serde_json::json!({
            "providerAccountRef": {
                "kind": "provider_account",
                "id": "provider-main"
            },
            "name": "example.com",
            "visibility": "public"
        }))
        .unwrap();
        assert_eq!(spec.origin, ZoneOrigin::Imported);
        assert_eq!(spec.dnssec, DnssecDesiredState::Disabled);
    }

    #[test]
    fn resource_graph_validates_reference_kind_and_presence() {
        let valid = CloudResourceSet::new([provider_account(), zone()]).unwrap();
        assert_eq!(valid.len(), 2);
        assert_eq!(
            valid.reverse_references(&CloudResourceId::new("provider-main").unwrap()),
            BTreeSet::from([CloudResourceId::new("zone-example").unwrap()])
        );

        let missing = CloudResourceSet::new([zone()]).unwrap_err();
        assert!(matches!(missing, CoreError::NotFound(_)));

        let mut wrong = match zone() {
            CloudResource::ManagedZone(zone) => zone,
            _ => unreachable!(),
        };
        wrong.spec.provider_account_ref.kind = CloudResourceKind::OriginPool;
        let error = CloudResourceSet::new([provider_account(), CloudResource::ManagedZone(wrong)])
            .unwrap_err();
        assert!(matches!(error, CoreError::Conflict(_)));
    }

    #[test]
    fn semantic_reference_kinds_are_enforced_before_graph_lookup() {
        let mut wrong = match zone() {
            CloudResource::ManagedZone(zone) => zone,
            _ => unreachable!(),
        };
        wrong.spec.provider_account_ref = reference(CloudResourceKind::OriginPool, "origin-main");
        let error = CloudResourceSet::new([CloudResource::ManagedZone(wrong)]).unwrap_err();
        assert!(matches!(error, CoreError::Conflict(_)));
    }

    #[test]
    fn duplicate_resource_ids_are_rejected() {
        let error = CloudResourceSet::new([provider_account(), provider_account()]).unwrap_err();
        assert!(matches!(error, CoreError::Conflict(_)));
    }

    #[test]
    fn serialized_provider_account_contains_only_a_credential_reference() {
        let json = serde_json::to_value(provider_account()).unwrap();
        assert_eq!(
            json["resource"]["spec"]["credentialSource"]["credentialRef"],
            "secret/cloudflare-main"
        );
        assert!(json.to_string().find("token").is_none());
        assert!(json.to_string().find("secretAccessKey").is_none());
    }
}
