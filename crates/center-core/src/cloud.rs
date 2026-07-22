//! Provider-neutral cloud infrastructure intent and status.
//!
//! These types deliberately do not reference Edgion Controllers, Gateway API
//! resources, provider SDKs, persistence frameworks, or arbitrary vendor JSON.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult};

mod capabilities;
#[cfg(feature = "test-support")]
mod capability_store_conformance;
mod credentials;
mod direct_call;
mod dns;
#[cfg(feature = "test-support")]
mod dns_provider_conformance;
mod dns_verification;
#[cfg(feature = "test-support")]
mod provider_account_store_conformance;
mod provider_accounts;
mod provider_errors;
mod status;
mod zone_lifecycle;

pub use capabilities::{
    is_retired_capability_snapshot_json, validate_write, CapabilityAction, CapabilityDecision,
    CapabilityDecisionOutcome, CapabilityDimension, CapabilityDimensionObservation,
    CapabilityDiscoveryFence, CapabilityDiscoveryIssue, CapabilityDiscoveryReport,
    CapabilityDiscoveryRequest, CapabilityDiscoveryState, CapabilityEvaluationContext,
    CapabilityEvidence, CapabilityGateBlocker, CapabilityGateReason, CapabilityIssueScope,
    CapabilityIssueSeverity, CapabilityObservation, CapabilityReason, CapabilityRequirement,
    CapabilityScope, CapabilitySnapshotKey, CapabilitySnapshotStore, CapabilityStoreWrite,
    DiscoveryToken, DnsCapability, ProviderCapability, ProviderCapabilityDiscoverer,
    ProviderCapabilitySnapshot, ProviderRegion, SanitizedCapabilityCode,
    SanitizedCapabilityMessage, TriState, WafCapability,
};
pub use credentials::{
    CredentialInspection, CredentialInspector, CredentialIssue, CredentialIssueKind,
    CredentialSource, CredentialState, ProviderIdentity,
};
pub use direct_call::{IdempotencyKey, OperationError, OperationErrorKind};
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
    WafReady,
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

/// Validates the persisted ProviderAccount envelope without routing it through
/// a retired, unified desired-resource aggregate.
pub(crate) fn validate_provider_account(account: &ProviderAccount) -> CoreResult<()> {
    let metadata = &account.metadata;
    if metadata.display_name.trim().is_empty() || metadata.generation == 0 {
        return Err(CoreError::Conflict(format!(
            "provider account {} has invalid metadata",
            metadata.id
        )));
    }
    account.status.validate(metadata.generation)?;
    account.spec.validate()
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

    fn provider_account() -> ProviderAccount {
        ProviderAccount {
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
        }
    }

    #[test]
    fn domain_names_are_canonical() {
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
    fn provider_account_is_validated_without_a_resource_aggregate() {
        assert!(validate_provider_account(&provider_account()).is_ok());
        let mut invalid = provider_account();
        invalid.metadata.generation = 0;
        assert!(matches!(
            validate_provider_account(&invalid),
            Err(CoreError::Conflict(_))
        ));
    }

    #[test]
    fn serialized_provider_account_contains_only_a_credential_reference() {
        let json = serde_json::to_value(provider_account()).unwrap();
        assert_eq!(
            json["spec"]["credentialSource"]["credentialRef"],
            "secret/cloudflare-main"
        );
        assert!(json.to_string().find("token").is_none());
        assert!(json.to_string().find("secretAccessKey").is_none());
    }
}
