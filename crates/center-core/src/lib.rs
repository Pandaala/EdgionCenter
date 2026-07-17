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
    apply_zone_authority_evidence, authorize_zone_deletion, dnssec_transition_for_intent,
    evaluate_zone_readiness, validate_dns_changes, validate_write, AbsoluteDnsName,
    AuthoritativeDnsVerification, BoundedCloudEventHistory, CaaTag, CacheCapability,
    CapabilityAction, CapabilityDecision, CapabilityDecisionOutcome, CapabilityDimension,
    CapabilityDimensionObservation, CapabilityDiscoveryFence, CapabilityDiscoveryIssue,
    CapabilityDiscoveryReport, CapabilityDiscoveryRequest, CapabilityDiscoveryState,
    CapabilityEvaluationContext, CapabilityEvidence, CapabilityGateBlocker, CapabilityGateReason,
    CapabilityIssueScope, CapabilityIssueSeverity, CapabilityObservation, CapabilityReason,
    CapabilityRequirement, CapabilityScope, CapabilitySnapshotKey, CapabilitySnapshotStore,
    CapabilityStoreWrite, CertificateBinding, CertificateBindingSpec, CertificateCapability,
    CertificateManagement, CertificateName, CertificatePurpose, ClaimedOperation, CloudCondition,
    CloudConditionStatus, CloudConditionType, CloudCorrelationId, CloudEvent, CloudOperation,
    CloudOperationAction, CloudOperationPhase, CloudOperationStep, CloudOperationStepPhase,
    CloudOperationStepPurpose, CloudProvider, CloudResource, CloudResourceId, CloudResourceKind,
    CloudResourceMetadata, CloudResourceRef, CloudResourceSet, CloudResourceStatus,
    CloudflareCnameFlattening, CloudflareProxyOptions, CredentialInspection, CredentialInspector,
    CredentialIssue, CredentialIssueKind, CredentialRef, CredentialSource, CredentialState,
    DelegationObservation, DelegationState, DeletionPolicy, DiscoveryToken, DispatchPolicy,
    DispatchedStep, DnsBatchAtomicity, DnsCapability, DnsChangeId, DnsChangeReceipt,
    DnsChangeState, DnsCharacterString, DnsGuardStrength, DnsMutationGuard, DnsName, DnsOwnerName,
    DnsPage, DnsPageRequest, DnsPageToken, DnsPropagationState, DnsProvider, DnsProviderResult,
    DnsRecordChange, DnsRecordExtension, DnsRecordObjectId, DnsRecordRevision, DnsRecordSet,
    DnsRecordSetKey, DnsRecordSetSpec, DnsRecordSetValue, DnsRecordType, DnsRoutingIdentity,
    DnsTtl, DnsTxtValue, DnsZoneId, DnsZoneRef, DnssecDesiredState, DnssecDsRecord,
    DnssecExternalAction, DnssecObservation, DnssecProviderState, DomainBinding, DomainBindingSpec,
    DomainName, EdgeApplication, EdgeApplicationSpec, EdgeCapability, EnqueueOperationResult,
    GoogleDnsGeoPolicy, GoogleDnsGeoPolicyItem, GoogleDnsHealthCheckRef,
    GoogleDnsHealthCheckTargets, GoogleDnsInternalLoadBalancerTarget, GoogleDnsIpProtocol,
    GoogleDnsLoadBalancerType, GoogleDnsPolicyItemData, GoogleDnsRoutingPolicy,
    GoogleDnsRoutingPolicyKind, GoogleDnsTrickleTraffic, GoogleDnsWeight, GoogleDnsWrrPolicyItem,
    HealthCheckCapability, HealthCheckSpec, IdempotencyKey, LeaseUpdate, ManagedZone,
    ManagedZoneSpec, ManagementPolicy, NewCloudOperation, NewCloudOperationStep,
    NormalizedProviderError, ObservedDnsRecordSet, ObservedDnsZone, OperationError,
    OperationErrorKind, OperationId, OperationLease, OperationStore, OriginAddress, OriginEndpoint,
    OriginPool, OriginPoolSpec, OriginProtocol, ProviderAccount, ProviderAccountScope,
    ProviderAccountSpec, ProviderCapability, ProviderCapabilityDiscoverer,
    ProviderCapabilitySnapshot, ProviderDnsRecordSet, ProviderDnsRecordType, ProviderErrorCategory,
    ProviderIdentity, ProviderRegion, ProviderResourceRef, Route53AliasTarget, Route53FailoverRole,
    Route53GeoLocation, Route53HealthCheckId, Route53RoutingPolicy, SanitizedCapabilityCode,
    SanitizedCapabilityMessage, StepCompletion, TriState, UnknownOutcomeResolution, WafCapability,
    ZoneAuthorityEvidence, ZoneAuthorityVerifier, ZoneCreationRequest, ZoneDeletionAcknowledgement,
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
