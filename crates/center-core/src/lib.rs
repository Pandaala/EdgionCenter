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
mod global_resource_plan;
mod global_resource_store;
mod global_resources;

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
    DnssecVerificationEvidence, DnssecVerificationExpectation, DomainName, IdempotencyKey,
    ManagedZone, ManagedZoneSpec, ManagementPolicy, NameserverCheck, NormalizedProviderError,
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
pub use global_resource_plan::{
    normalized_global_resource_desired, normalized_global_resource_observed, plan_global_resource,
    resolve_global_resource_targets, validate_global_resource_ownership, GlobalResourceOwnership,
    GlobalResourcePlan, GlobalResourcePlanBinding, GlobalResourcePlanReason,
    GlobalResourcePlanState, GlobalResourceResolvedTarget, GlobalResourceTargetObservation,
    GlobalResourceTargetPlan, GlobalResourceTargetResolutionState, GLOBAL_RESOURCE_ID_ANNOTATION,
    GLOBAL_RESOURCE_MANAGED_BY_LABEL, GLOBAL_RESOURCE_MANAGED_BY_VALUE,
    GLOBAL_RESOURCE_PLAN_SCHEMA_VERSION, GLOBAL_RESOURCE_REVISION_ANNOTATION,
    MAX_GLOBAL_RESOURCE_CHANGED_PATHS, MAX_GLOBAL_RESOURCE_CHANGED_PATH_BYTES,
};
pub use global_resource_store::{
    global_resource_for_create, global_resource_for_replace,
    validate_global_resource_expected_generation, GlobalResource, GlobalResourceAdoptionPolicy,
    GlobalResourceCreateResult, GlobalResourceDesired, GlobalResourceId, GlobalResourcePage,
    GlobalResourcePageRequest, GlobalResourcePrunePolicy, GlobalResourceReplaceResult,
    GlobalResourceRevision, GlobalResourceStore, GlobalResourceSyncMode, GlobalResourceSyncPolicy,
    GlobalResourceTargetSelector, MAX_GLOBAL_RESOURCE_PAGE_SIZE,
    MAX_GLOBAL_RESOURCE_TEMPLATE_BYTES,
};
pub use global_resources::{
    EdgionConfigDataType, GlobalResourceCatalogEntry, GlobalResourceInventoryKind,
    GlobalResourcesConfig, DEFAULT_PLATFORM_NAMESPACES, GLOBAL_RESOURCE_CATALOG_VERSION,
    GLOBAL_RESOURCE_KINDS, MAX_PLATFORM_NAMESPACES,
};

#[cfg(feature = "test-support")]
pub use cloud::test_support as cloud_test_support;
