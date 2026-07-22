//! Platform-neutral domain types and capability ports for EdgionCenter.
//!
//! This crate must remain independent of HTTP/gRPC frameworks and platform
//! adapters such as SQLx and Kube.

mod admin;
mod audit;
mod authz;
mod capabilities;
mod cloud;
mod controller;
mod coordination;
mod error;

pub use admin::{CreateRole, CreateUser, RoleAdmin, RoleRecord, UpdateUser, UserAdmin, UserRecord};
pub use audit::{AuditEvent, AuditFilter, AuditPage, AuditReader, AuditWriter, Page};
pub use authz::{
    Action, ActionOperation, AllowAllAuthorizer, Authorizer, AuthzMode, Decision, Principal,
};
pub use capabilities::{CenterCapabilities, CenterMode};
pub use cloud::{
    apply_dns_verification_evidence, authorize_zone_deletion, dnssec_transition_for_intent,
    evaluate_zone_readiness, is_retired_capability_snapshot_json, provider_account_from_desired,
    validate_dns_changes, validate_stored_provider_account, validate_write, AbsoluteDnsName,
    AuthoritativeDnsVerification, BoundedCloudEventHistory, CaaTag, CapabilityAction,
    CapabilityDecision, CapabilityDecisionOutcome, CapabilityDimension,
    CapabilityDimensionObservation, CapabilityDiscoveryFence, CapabilityDiscoveryIssue,
    CapabilityDiscoveryReport, CapabilityDiscoveryRequest, CapabilityDiscoveryState,
    CapabilityEvaluationContext, CapabilityEvidence, CapabilityGateBlocker, CapabilityGateReason,
    CapabilityIssueScope, CapabilityIssueSeverity, CapabilityObservation, CapabilityReason,
    CapabilityRequirement, CapabilityScope, CapabilitySnapshotKey, CapabilitySnapshotStore,
    CapabilityStoreWrite, CloudCondition, CloudConditionStatus, CloudConditionType,
    CloudCorrelationId, CloudEvent, CloudProvider, CloudResourceId, CloudResourceKind,
    CloudResourceMetadata, CloudResourceRef, CloudResourceStatus, CloudflareCnameFlattening,
    CloudflareProxyOptions, CredentialInspection, CredentialInspector, CredentialIssue,
    CredentialIssueKind, CredentialRef, CredentialSource, CredentialState, DelegationObservation,
    DelegationState, DeletionPolicy, DiscoveryToken, DnsBatchAtomicity, DnsCapability, DnsChangeId,
    DnsChangeReceipt, DnsChangeState, DnsCharacterString, DnsGuardStrength, DnsMutationGuard,
    DnsName, DnsOwnerName, DnsPage, DnsPageRequest, DnsPageToken, DnsPropagationState,
    DnsPropagationVerifier, DnsProvider, DnsProviderResult, DnsQueryOutcome, DnsRecordChange,
    DnsRecordExtension, DnsRecordObjectId, DnsRecordRevision, DnsRecordSet, DnsRecordSetKey,
    DnsRecordSetSpec, DnsRecordSetValue, DnsRecordType, DnsRoutingIdentity, DnsRrsetExpectation,
    DnsTtl, DnsTxtValue, DnsVerificationBinding, DnsVerificationBudgetUse, DnsVerificationError,
    DnsVerificationErrorKind, DnsVerificationEvidence, DnsVerificationPolicy,
    DnsVerificationRequest, DnsVerificationRequestId, DnsVerificationResult, DnsVerificationScope,
    DnsZoneId, DnsZoneRef, DnssecDesiredState, DnssecDsRecord, DnssecEvidenceSource,
    DnssecExternalAction, DnssecObservation, DnssecProviderState, DnssecValidationState,
    DnssecVerificationEvidence, DnssecVerificationExpectation, DomainName, GoogleDnsGeoPolicy,
    GoogleDnsGeoPolicyItem, GoogleDnsHealthCheckRef, GoogleDnsHealthCheckTargets,
    GoogleDnsInternalLoadBalancerTarget, GoogleDnsIpProtocol, GoogleDnsLoadBalancerType,
    GoogleDnsPolicyItemData, GoogleDnsRoutingPolicy, GoogleDnsRoutingPolicyKind,
    GoogleDnsTrickleTraffic, GoogleDnsWeight, GoogleDnsWrrPolicyItem, IdempotencyKey, ManagedZone,
    ManagedZoneSpec, ManagementPolicy, NameserverCheck, NormalizedProviderError,
    ObservedDnsRecordSet, ObservedDnsZone, OperationError, OperationErrorKind, ProviderAccount,
    ProviderAccountCreateResult, ProviderAccountDesired, ProviderAccountPage,
    ProviderAccountPageRequest, ProviderAccountReplaceResult, ProviderAccountScope,
    ProviderAccountSpec, ProviderAccountStore, ProviderCapability, ProviderCapabilityDiscoverer,
    ProviderCapabilitySnapshot, ProviderDnsRecordSet, ProviderDnsRecordType, ProviderErrorCategory,
    ProviderIdentity, ProviderRegion, ProviderResourceRef, RecursiveResolverCheck,
    ResolverProfileId, ResolverProfileRef, ResolverProfileRevision, Route53AliasTarget,
    Route53FailoverRole, Route53GeoLocation, Route53HealthCheckId, Route53RoutingPolicy,
    SanitizedCapabilityCode, SanitizedCapabilityMessage, SanitizedDnsFailureCode, TriState,
    WafCapability, ZoneAuthorityEvidence, ZoneCreationRequest, ZoneDeletionAcknowledgement,
    ZoneDeletionApproval, ZoneDeletionBlocker, ZoneDeletionPlan, ZoneDeletionRequest,
    ZoneLifecycleMutationId, ZoneLifecycleMutationReceipt, ZoneLifecycleMutationState,
    ZoneLifecycleObservation, ZoneLifecycleProvider, ZoneLifecycleProviderResult,
    ZoneLifecycleRevision, ZoneOrigin, ZoneReadiness, ZoneVisibility,
};
pub use controller::{
    ControllerDirectory, ControllerId, ControllerOwnerLocator, ControllerOwnerRoute,
    ControllerPhase, ControllerRecord, ControllerRegistration, ControllerRuntimeObservation,
    EvictionOutcome, EvictionResult, EvictionTarget, OfflineOutcome, OwnershipFence, SessionId,
};
pub use coordination::{CoordinationRole, Coordinator, Leadership, ReleaseOutcome, RenewalOutcome};
pub use error::{CoreError, CoreResult};

#[cfg(feature = "test-support")]
pub use cloud::test_support as cloud_test_support;
