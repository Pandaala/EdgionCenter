//! Cloudflare provider adapters.
//!
//! This crate is independent of SQL, Kubernetes, federation, Admin API, and
//! Edgion resources. Composition roots inject a credential-owning API client.

mod http;
pub mod load_balancing;
pub mod origin_rules;

pub use http::{
    CloudflareApiToken, CloudflareCredentialProbe, CloudflareHttpApi, CloudflareTokenStatus,
    CloudflareTokenVerification,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    evaluate_zone_readiness, validate_dns_changes, AbsoluteDnsName, AuthoritativeDnsVerification,
    CloudProvider, CloudResourceId, CloudflareCnameFlattening, CloudflareProxyOptions,
    DelegationObservation, DelegationState, DnsBatchAtomicity, DnsChangeId, DnsChangeReceipt,
    DnsChangeState, DnsGuardStrength, DnsOwnerName, DnsPage, DnsPageRequest, DnsPageToken,
    DnsPropagationState, DnsProvider, DnsProviderResult, DnsRecordChange, DnsRecordExtension,
    DnsRecordObjectId, DnsRecordRevision, DnsRecordSetKey, DnsRecordSetValue, DnsRoutingIdentity,
    DnsTtl, DnsZoneId, DnsZoneRef, DnssecDesiredState, DnssecDsRecord, DnssecExternalAction,
    DnssecObservation, DnssecProviderState, NormalizedProviderError, ObservedDnsRecordSet,
    ObservedDnsZone, ProviderAccountScope, ProviderAccountSpec, ProviderDnsRecordSet,
    ProviderDnsRecordType, ProviderErrorCategory, ZoneCreationRequest, ZoneDeletionRequest,
    ZoneLifecycleMutationId, ZoneLifecycleMutationReceipt, ZoneLifecycleMutationState,
    ZoneLifecycleObservation, ZoneLifecycleProvider, ZoneLifecycleProviderResult,
    ZoneLifecycleRevision, ZoneVisibility,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const MAX_ZONE_PROVIDER_PAGES: u32 = 200;
const MAX_RECORD_PROVIDER_PAGES: u32 = 20;
const PROVIDER_PAGE_SIZE: u32 = 5_000;
const MAX_INVENTORY_ZONES: usize = 10_000;
const MAX_INVENTORY_RECORDS: usize = 100_000;
const MAX_INVENTORY_TAG_BYTES: usize = 64 * 1024 * 1024;
const MAX_BATCH_OPERATIONS: usize = 10_000;
const DEFAULT_CURSOR_TTL_SECS: u64 = 900;
const MAX_CURSOR_TTL_SECS: u64 = 3_600;
const DEFAULT_CURSOR_CLOCK_SKEW_SECS: u64 = 30;
const MAX_CURSOR_CLOCK_SKEW_SECS: u64 = 300;
type HmacSha256 = Hmac<Sha256>;

pub type CloudflareApiResult<T> = Result<T, NormalizedProviderError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareZoneKind {
    Full,
    Partial,
    Secondary,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareZoneStatus {
    Initializing,
    Pending,
    Active,
    Moved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareZone {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub kind: CloudflareZoneKind,
    pub status: CloudflareZoneStatus,
    #[serde(default)]
    pub name_servers: BTreeSet<AbsoluteDnsName>,
    pub modified_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareCreateZoneRequest {
    pub account_id: String,
    pub name: String,
    pub kind: CloudflareZoneKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareDeleteZoneAck {
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CloudflareDnssecStatus {
    Active,
    Pending,
    Disabled,
    PendingDisabled,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareDnssecDs {
    pub key_tag: u16,
    pub algorithm: u8,
    pub digest_type: u8,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareDnssec {
    pub status: CloudflareDnssecStatus,
    pub ds: Option<CloudflareDnssecDs>,
    pub modified_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareRecord {
    pub id: String,
    pub name: String,
    pub ttl: u32,
    pub value: DnsRecordSetValue,
    pub proxied: Option<bool>,
    pub proxiable: bool,
    pub flatten_cname: Option<bool>,
    #[serde(default)]
    pub ipv4_only: bool,
    #[serde(default)]
    pub ipv6_only: bool,
    #[serde(default)]
    pub private_routing: bool,
    pub comment: Option<String>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    pub modified_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflarePage<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub total_pages: u32,
}

/// Provider-specific, sanitized Cloudflare zone inventory observation.
///
/// The provider-native account ID, credentials, raw response metadata, and
/// provider error details are intentionally absent. The Center account ID in
/// `zone` is the only account identity exposed to consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservedCloudflareZone {
    pub zone: DnsZoneRef,
    pub kind: CloudflareZoneKind,
    pub status: CloudflareZoneStatus,
    pub name_servers: BTreeSet<AbsoluteDnsName>,
    pub revision: Option<DnsRecordRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareBatchRequest {
    pub deletes: Vec<CloudflareBatchDelete>,
    pub patches: Vec<serde_json::Value>,
    pub puts: Vec<serde_json::Value>,
    pub posts: Vec<CloudflareBatchRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct CloudflareBatchDelete {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloudflareBatchRecord {
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub ttl: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<CloudflareBatchSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub tags: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloudflareBatchSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flatten_cname: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareBatchResult {
    pub deletes: Vec<CloudflareRecord>,
    pub posts: Vec<CloudflareRecord>,
}

#[async_trait]
pub trait CloudflareApi: Send + Sync {
    async fn create_zone(
        &self,
        request: &CloudflareCreateZoneRequest,
    ) -> CloudflareApiResult<CloudflareZone>;

    async fn get_zone(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareZone>>;

    async fn delete_zone(&self, zone_id: &str) -> CloudflareApiResult<CloudflareDeleteZoneAck>;

    async fn get_dnssec(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareDnssec>>;

    async fn patch_dnssec(
        &self,
        zone_id: &str,
        desired: DnssecDesiredState,
    ) -> CloudflareApiResult<CloudflareDnssec>;

    async fn list_zones(
        &self,
        account_id: &str,
        page: u32,
        per_page: u32,
    ) -> CloudflareApiResult<CloudflarePage<CloudflareZone>>;

    /// Returns every record kind from the provider response or fails when a
    /// kind cannot be represented. Implementations must never silently filter
    /// unsupported record types.
    async fn list_records(
        &self,
        zone_id: &str,
        page: u32,
        per_page: u32,
    ) -> CloudflareApiResult<CloudflarePage<CloudflareRecord>>;

    async fn batch_records(
        &self,
        zone_id: &str,
        request: &CloudflareBatchRequest,
    ) -> CloudflareApiResult<CloudflareBatchResult>;
}

/// Account-bound, read-only Cloudflare zone inventory.
///
/// Unlike the portable [`DnsProvider`] contract, this seam retains the
/// Cloudflare zone kind, status, and authoritative nameservers required by a
/// provider-specific management surface. Implementations never return the
/// provider-native account ID.
#[async_trait]
pub trait CloudflareZoneInventory: Send + Sync {
    async fn list_zone_inventory(
        &self,
        provider_account_id: &CloudResourceId,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<DnsPage<ObservedCloudflareZone>>;

    async fn get_zone_by_id(
        &self,
        provider_account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> DnsProviderResult<Option<ObservedCloudflareZone>>;
}

/// Minimal synchronous Cloudflare DNS writer.
///
/// This surface intentionally has no cursor or mutation-token authority. A successful call
/// returns the provider's validated resource directly; an ambiguous provider response remains an
/// `UnknownOutcome` and must be reconciled by a subsequent read before any retry.
pub struct CloudflareDnsSyncWriter {
    center_account_id: CloudResourceId,
    cloudflare_account_id: String,
    api: Arc<dyn CloudflareApi>,
    instance: Arc<()>,
}

/// Result of one synchronous RRset mutation.
///
/// Create and replace return the authoritative post-mutation observation. Delete succeeds only
/// after a post-mutation observation confirms that the RRset is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudflareDnsSyncRecordOutcome {
    Present(Box<ObservedDnsRecordSet>),
    Deleted,
}

/// Opaque, single-use proof that one exact Zone passed the synchronous deletion preflight.
///
/// Fields are private so callers cannot bypass the fresh apex, revision, record, and DNSSEC
/// checks. Consuming the guard prevents accidental repeated dispatch through the same proof.
#[derive(Debug)]
pub struct CloudflareDnsSyncZoneDeleteGuard {
    zone_id: DnsZoneId,
    writer_instance: Arc<()>,
}

impl CloudflareDnsSyncWriter {
    pub fn new(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn CloudflareApi>,
    ) -> Result<Self, NormalizedProviderError> {
        center_account_id
            .validate()
            .map_err(|_| validation("invalid_provider_account_id"))?;
        account
            .validate()
            .map_err(|_| validation("invalid_provider_account"))?;
        if account.provider != CloudProvider::Cloudflare {
            return Err(validation("cloudflare_provider_required"));
        }
        let ProviderAccountScope::Cloudflare { account_id } = account
            .scope
            .as_ref()
            .ok_or_else(|| validation("cloudflare_account_scope_required"))?
        else {
            return Err(validation("cloudflare_account_scope_mismatch"));
        };
        if !valid_cloudflare_identifier(account_id) {
            return Err(validation("invalid_cloudflare_account_id"));
        }
        Ok(Self {
            center_account_id,
            cloudflare_account_id: account_id.clone(),
            api,
            instance: Arc::new(()),
        })
    }

    /// Creates one public, full Cloudflare zone with exactly one provider request and no retry.
    pub async fn create_zone(
        &self,
        apex: &AbsoluteDnsName,
    ) -> DnsProviderResult<ObservedCloudflareZone> {
        let request = CloudflareCreateZoneRequest {
            account_id: self.cloudflare_account_id.clone(),
            name: apex.as_str().to_owned(),
            kind: CloudflareZoneKind::Full,
        };
        let created = self.api.create_zone(&request).await?;
        if created.account_id != self.cloudflare_account_id
            || created.name != apex.as_str()
            || created.kind != CloudflareZoneKind::Full
            || created.name_servers.is_empty()
            || !valid_cloudflare_identifier(&created.id)
        {
            return Err(unknown_outcome("cloudflare_create_zone_result_mismatch"));
        }
        let observed = map_zone_inventory_for_account(
            &self.center_account_id,
            &self.cloudflare_account_id,
            created,
        )
        .map_err(|_| unknown_outcome("cloudflare_create_zone_result_mismatch"))?;
        if observed.zone.apex != *apex || observed.zone.visibility != ZoneVisibility::Public {
            return Err(unknown_outcome("cloudflare_create_zone_result_mismatch"));
        }
        Ok(observed)
    }

    /// Observes one exact zone without cursor authority or mutation side effects.
    pub async fn observe_zone(
        &self,
        zone_id: &DnsZoneId,
    ) -> DnsProviderResult<Option<ObservedCloudflareZone>> {
        zone_id
            .validate()
            .map_err(|_| validation("invalid_cloudflare_zone_id"))?;
        if !valid_cloudflare_identifier(zone_id.as_str()) {
            return Err(validation("invalid_cloudflare_zone_id"));
        }
        self.api
            .get_zone(zone_id.as_str())
            .await?
            .map(|zone| {
                map_zone_inventory_for_account(
                    &self.center_account_id,
                    &self.cloudflare_account_id,
                    zone,
                )
            })
            .transpose()
    }

    /// Validates one exact empty, DNSSEC-disabled zone without mutation.
    ///
    /// The apex and provider revision are checked against a fresh zone observation. Records and
    /// DNSSEC are observed before an opaque guard is returned. The caller must recheck its local
    /// account and credential authority before consuming the guard in [`Self::delete_zone`].
    pub async fn preflight_zone_delete(
        &self,
        zone_id: &DnsZoneId,
        expected_apex: &AbsoluteDnsName,
        expected_revision: &DnsRecordRevision,
    ) -> DnsProviderResult<CloudflareDnsSyncZoneDeleteGuard> {
        zone_id
            .validate()
            .map_err(|_| validation("invalid_cloudflare_zone_id"))?;
        expected_revision
            .validate()
            .map_err(|_| validation("invalid_cloudflare_zone_revision"))?;
        if !valid_cloudflare_identifier(zone_id.as_str()) {
            return Err(validation("invalid_cloudflare_zone_id"));
        }
        let provider_zone = self
            .api
            .get_zone(zone_id.as_str())
            .await?
            .ok_or_else(|| not_found("cloudflare_zone_not_found"))?;
        let observed = map_zone_inventory_for_account(
            &self.center_account_id,
            &self.cloudflare_account_id,
            provider_zone,
        )?;
        if observed.zone.apex != *expected_apex {
            return Err(conflict("cloudflare_zone_name_confirmation_mismatch"));
        }
        if observed.zone.visibility != ZoneVisibility::Public {
            return Err(validation("cloudflare_zone_scope_mismatch"));
        }
        if observed.revision.as_ref() != Some(expected_revision) {
            return Err(conflict("cloudflare_zone_revision_conflict"));
        }

        let mut records = Vec::new();
        let mut expected_total_pages = None;
        for page in 1..=MAX_RECORD_PROVIDER_PAGES {
            let result = self
                .api
                .list_records(zone_id.as_str(), page, PROVIDER_PAGE_SIZE)
                .await?;
            validate_provider_page(page, PROVIDER_PAGE_SIZE, &result)?;
            if result.total_pages > MAX_RECORD_PROVIDER_PAGES {
                return Err(validation("cloudflare_record_pagination_limit"));
            }
            validate_stable_total_pages(&mut expected_total_pages, result.total_pages)?;
            records.extend(result.items);
            if records.len() > MAX_INVENTORY_RECORDS {
                return Err(validation("cloudflare_record_inventory_limit"));
            }
            if page >= result.total_pages {
                break;
            }
        }
        if records.iter().any(|record| {
            !(record.name == expected_apex.as_str()
                && matches!(
                    record.value,
                    DnsRecordSetValue::Soa { .. } | DnsRecordSetValue::Ns { .. }
                ))
        }) {
            return Err(conflict("cloudflare_zone_not_empty"));
        }

        let dnssec = self.api.get_dnssec(zone_id.as_str()).await?;
        let dnssec = map_dnssec_observation(dnssec.as_ref())?;
        if !matches!(
            dnssec.state,
            DnssecProviderState::Disabled | DnssecProviderState::Unsupported
        ) {
            return Err(conflict("cloudflare_zone_dnssec_not_disabled"));
        }

        Ok(CloudflareDnsSyncZoneDeleteGuard {
            zone_id: zone_id.clone(),
            writer_instance: self.instance.clone(),
        })
    }

    /// Consumes one validated preflight guard and dispatches exactly one provider delete.
    pub async fn delete_zone(
        &self,
        guard: CloudflareDnsSyncZoneDeleteGuard,
    ) -> DnsProviderResult<()> {
        if !Arc::ptr_eq(&self.instance, &guard.writer_instance) {
            return Err(validation("cloudflare_zone_delete_guard_scope_mismatch"));
        }
        let ack = self.api.delete_zone(guard.zone_id.as_str()).await?;
        if ack.id != guard.zone_id.as_str() {
            return Err(unknown_outcome("cloudflare_delete_zone_result_mismatch"));
        }
        Ok(())
    }

    /// Observes one exact RRset using a bounded provider scan.
    pub async fn observe_record_set(
        &self,
        zone: &DnsZoneRef,
        key: &DnsRecordSetKey,
    ) -> DnsProviderResult<Option<ObservedDnsRecordSet>> {
        key.validate()
            .map_err(|_| validation("invalid_record_key"))?;
        Ok(self
            .all_records(zone)
            .await?
            .into_iter()
            .find(|record| &record.record_set.key == key))
    }

    /// Applies exactly one guarded RRset change with one Cloudflare batch request and no retry.
    pub async fn apply_record_change(
        &self,
        zone: &DnsZoneRef,
        change: &DnsRecordChange,
    ) -> DnsProviderResult<CloudflareDnsSyncRecordOutcome> {
        self.validate_zone_ref(zone)?;
        validate_dns_changes(zone, std::slice::from_ref(change))
            .map_err(|_| validation("invalid_dns_changes"))?;
        let current = self.all_records(zone).await?;
        let request = plan_batch(std::slice::from_ref(change), current)?;
        let result = self
            .api
            .batch_records(zone.zone_id.as_str(), &request)
            .await?;
        validate_batch_result(zone, std::slice::from_ref(change), &request, result)?;

        let (key, expected) = match change {
            DnsRecordChange::Create { record_set, .. } => (&record_set.key, Some(record_set)),
            DnsRecordChange::Replace { desired, .. } => (&desired.key, Some(desired)),
            DnsRecordChange::Delete { previous, .. } => (&previous.record_set.key, None),
        };
        let observed = self
            .observe_record_set(zone, key)
            .await
            .map_err(|_| unknown_outcome("cloudflare_record_post_observation_failed"))?;
        match (expected, observed) {
            (Some(expected), Some(observed)) if &observed.record_set == expected => {
                Ok(CloudflareDnsSyncRecordOutcome::Present(Box::new(observed)))
            }
            (None, None) => Ok(CloudflareDnsSyncRecordOutcome::Deleted),
            _ => Err(unknown_outcome(
                "cloudflare_record_post_observation_mismatch",
            )),
        }
    }

    async fn all_records(&self, zone: &DnsZoneRef) -> DnsProviderResult<Vec<ObservedDnsRecordSet>> {
        self.validate_zone_ref(zone)?;
        let provider_zone = self
            .api
            .get_zone(zone.zone_id.as_str())
            .await?
            .ok_or_else(|| not_found("cloudflare_zone_not_found"))?;
        let observed_zone = map_zone_for_account(
            &self.center_account_id,
            &self.cloudflare_account_id,
            provider_zone,
        )?;
        if observed_zone.zone != *zone {
            return Err(validation("cloudflare_zone_identity_mismatch"));
        }
        let mut records = Vec::new();
        let mut expected_total_pages = None;
        for page in 1..=MAX_RECORD_PROVIDER_PAGES {
            let result = self
                .api
                .list_records(zone.zone_id.as_str(), page, PROVIDER_PAGE_SIZE)
                .await?;
            validate_provider_page(page, PROVIDER_PAGE_SIZE, &result)?;
            if result.total_pages > MAX_RECORD_PROVIDER_PAGES {
                return Err(validation("cloudflare_record_pagination_limit"));
            }
            validate_stable_total_pages(&mut expected_total_pages, result.total_pages)?;
            records.extend(result.items);
            if records.len() > MAX_INVENTORY_RECORDS {
                return Err(validation("cloudflare_record_inventory_limit"));
            }
            if page >= result.total_pages {
                return aggregate_records(zone, records);
            }
        }
        Err(validation("cloudflare_record_pagination_limit"))
    }

    fn validate_zone_ref(&self, zone: &DnsZoneRef) -> DnsProviderResult<()> {
        zone.validate().map_err(|_| validation("invalid_zone"))?;
        if zone.provider != CloudProvider::Cloudflare
            || zone.provider_account_id != self.center_account_id
            || zone.visibility != ZoneVisibility::Public
            || !valid_cloudflare_identifier(zone.zone_id.as_str())
        {
            return Err(validation("cloudflare_zone_scope_mismatch"));
        }
        Ok(())
    }
}

/// Stable composition-provided key for opaque cursor authentication.
///
/// All replicas serving one account must receive the same key. The type is
/// intentionally neither printable nor serializable.
pub struct CloudflareCursorKey(Zeroizing<[u8; 32]>);

impl CloudflareCursorKey {
    pub fn new(value: [u8; 32]) -> Result<Self, NormalizedProviderError> {
        if value.iter().all(|byte| *byte == 0) {
            return Err(validation("weak_cloudflare_cursor_key"));
        }
        Ok(Self(Zeroizing::new(value)))
    }

    pub fn from_zeroizing(value: Zeroizing<[u8; 32]>) -> Result<Self, NormalizedProviderError> {
        if value.iter().all(|byte| *byte == 0) {
            return Err(validation("weak_cloudflare_cursor_key"));
        }
        Ok(Self(value))
    }
}

trait CloudflareCursorClock: Send + Sync {
    fn unix_seconds(&self) -> DnsProviderResult<u64>;
}

struct SystemCloudflareCursorClock;

impl CloudflareCursorClock for SystemCloudflareCursorClock {
    fn unix_seconds(&self) -> DnsProviderResult<u64> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|_| validation("cloudflare_cursor_clock_unavailable"))
    }
}

/// Active signing key and optional verification-only fallback for DNS cursors.
///
/// Mutation and change receipt tokens deliberately continue to use only the
/// active key; the fallback is confined to pagination cursor verification.
pub struct CloudflareCursorKeyRing {
    active: CloudflareCursorKey,
    fallback: Option<CloudflareCursorKey>,
    ttl_secs: u64,
    clock_skew_secs: u64,
    clock: Arc<dyn CloudflareCursorClock>,
}

impl std::fmt::Debug for CloudflareCursorKeyRing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CloudflareCursorKeyRing")
            .field("active", &"[REDACTED]")
            .field("fallback", &self.fallback.as_ref().map(|_| "[REDACTED]"))
            .field("ttl_secs", &self.ttl_secs)
            .field("clock_skew_secs", &self.clock_skew_secs)
            .finish()
    }
}

impl CloudflareCursorKeyRing {
    pub fn new(
        active: CloudflareCursorKey,
        fallback: Option<CloudflareCursorKey>,
        ttl: Duration,
        clock_skew: Duration,
    ) -> Result<Self, NormalizedProviderError> {
        Self::with_clock(
            active,
            fallback,
            ttl,
            clock_skew,
            Arc::new(SystemCloudflareCursorClock),
        )
    }

    fn with_clock(
        active: CloudflareCursorKey,
        fallback: Option<CloudflareCursorKey>,
        ttl: Duration,
        clock_skew: Duration,
        clock: Arc<dyn CloudflareCursorClock>,
    ) -> Result<Self, NormalizedProviderError> {
        let ttl_secs = ttl.as_secs();
        let clock_skew_secs = clock_skew.as_secs();
        if ttl.subsec_nanos() != 0
            || clock_skew.subsec_nanos() != 0
            || ttl_secs == 0
            || ttl_secs > MAX_CURSOR_TTL_SECS
            || clock_skew_secs > MAX_CURSOR_CLOCK_SKEW_SECS
            || clock_skew_secs >= ttl_secs
        {
            return Err(validation("invalid_cloudflare_cursor_lifetime"));
        }
        if fallback
            .as_ref()
            .is_some_and(|fallback| fallback.0.as_ref() == active.0.as_ref())
        {
            return Err(validation("duplicate_cloudflare_cursor_key"));
        }
        Ok(Self {
            active,
            fallback,
            ttl_secs,
            clock_skew_secs,
            clock,
        })
    }

    fn now(&self) -> DnsProviderResult<u64> {
        self.clock.unix_seconds()
    }
}

impl From<CloudflareCursorKey> for CloudflareCursorKeyRing {
    fn from(active: CloudflareCursorKey) -> Self {
        Self::new(
            active,
            None,
            Duration::from_secs(DEFAULT_CURSOR_TTL_SECS),
            Duration::from_secs(DEFAULT_CURSOR_CLOCK_SKEW_SECS),
        )
        .expect("static cursor lifetime is valid")
    }
}

/// Independent HMAC key for Cloudflare mutation observation tokens.
///
/// This type is intentionally not interchangeable with pagination cursor keys.
pub struct CloudflareMutationTokenKey(Zeroizing<[u8; 32]>);

impl CloudflareMutationTokenKey {
    pub fn new(value: [u8; 32]) -> Result<Self, NormalizedProviderError> {
        if value.iter().all(|byte| *byte == 0) {
            return Err(validation("weak_cloudflare_mutation_token_key"));
        }
        Ok(Self(Zeroizing::new(value)))
    }

    pub fn from_zeroizing(value: Zeroizing<[u8; 32]>) -> Result<Self, NormalizedProviderError> {
        if value.iter().all(|byte| *byte == 0) {
            return Err(validation("weak_cloudflare_mutation_token_key"));
        }
        Ok(Self(value))
    }
}

/// Active signer and optional verification-only fallback for mutation tokens.
pub struct CloudflareMutationTokenKeyRing {
    active: CloudflareMutationTokenKey,
    fallback: Option<CloudflareMutationTokenKey>,
}

impl CloudflareMutationTokenKeyRing {
    pub fn new(
        active: CloudflareMutationTokenKey,
        fallback: Option<CloudflareMutationTokenKey>,
    ) -> Result<Self, NormalizedProviderError> {
        if fallback
            .as_ref()
            .is_some_and(|fallback| fallback.0.as_ref() == active.0.as_ref())
        {
            return Err(validation("duplicate_cloudflare_mutation_token_key"));
        }
        Ok(Self { active, fallback })
    }
}

impl std::fmt::Debug for CloudflareMutationTokenKeyRing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CloudflareMutationTokenKeyRing")
            .field("active", &"[REDACTED]")
            .field("fallback", &self.fallback.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// Opaque proof that a record cursor was checked before authoritative provider I/O.
pub struct ValidatedCloudflareRecordCursor {
    adapter_instance: Arc<()>,
    center_account_id: CloudResourceId,
    cloudflare_account_id: String,
    zone_id: DnsZoneId,
    request: DnsPageRequest,
    cursor: Option<VerifiedCursor>,
    now: u64,
}

/// Account-bound Cloudflare DNS adapter.
pub struct CloudflareDnsAdapter {
    adapter_instance: Arc<()>,
    center_account_id: CloudResourceId,
    cloudflare_account_id: String,
    api: Arc<dyn CloudflareApi>,
    cursor_keys: CloudflareCursorKeyRing,
    mutation_token_keys: Option<CloudflareMutationTokenKeyRing>,
}

impl CloudflareDnsAdapter {
    pub fn new(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn CloudflareApi>,
        cursor_keys: impl Into<CloudflareCursorKeyRing>,
    ) -> Result<Self, NormalizedProviderError> {
        Self::build(center_account_id, account, api, cursor_keys.into(), None)
    }

    fn build(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn CloudflareApi>,
        cursor_keys: CloudflareCursorKeyRing,
        mutation_token_keys: Option<CloudflareMutationTokenKeyRing>,
    ) -> Result<Self, NormalizedProviderError> {
        account
            .validate()
            .map_err(|_| validation("invalid_provider_account"))?;
        let ProviderAccountScope::Cloudflare { account_id } = account
            .scope
            .as_ref()
            .ok_or_else(|| validation("cloudflare_account_scope_required"))?
        else {
            return Err(validation("cloudflare_account_scope_mismatch"));
        };
        if account.provider != CloudProvider::Cloudflare {
            return Err(validation("cloudflare_provider_required"));
        }
        if let Some(mutation_keys) = mutation_token_keys.as_ref() {
            let cursor_materials =
                std::iter::once(&cursor_keys.active).chain(cursor_keys.fallback.as_ref());
            let mutation_materials =
                std::iter::once(&mutation_keys.active).chain(mutation_keys.fallback.as_ref());
            if cursor_materials.clone().any(|cursor| {
                mutation_materials
                    .clone()
                    .any(|mutation| cursor.0.as_ref() == mutation.0.as_ref())
            }) {
                return Err(validation("cloudflare_token_key_material_reused"));
            }
        }
        Ok(Self {
            adapter_instance: Arc::new(()),
            center_account_id,
            cloudflare_account_id: account_id.clone(),
            api,
            cursor_keys,
            mutation_token_keys,
        })
    }

    pub fn new_with_cursor_key_ring(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn CloudflareApi>,
        cursor_keys: CloudflareCursorKeyRing,
    ) -> Result<Self, NormalizedProviderError> {
        Self::new(center_account_id, account, api, cursor_keys)
    }

    /// Constructs a write-capable adapter with independent cursor and mutation authorities.
    ///
    /// H5a keeps this adapter API available for hermetic provider work only. Production write
    /// composition remains blocked until H5b supplies durable operation fences and recovery
    /// evidence around every provider mutation.
    pub fn new_with_cursor_and_mutation_key_rings(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn CloudflareApi>,
        cursor_keys: CloudflareCursorKeyRing,
        mutation_token_keys: CloudflareMutationTokenKeyRing,
    ) -> Result<Self, NormalizedProviderError> {
        Self::build(
            center_account_id,
            account,
            api,
            cursor_keys,
            Some(mutation_token_keys),
        )
    }

    fn mutation_token_keys(&self) -> DnsProviderResult<&CloudflareMutationTokenKeyRing> {
        self.mutation_token_keys
            .as_ref()
            .ok_or_else(|| validation("cloudflare_mutation_token_authority_required"))
    }

    /// Validates an opaque record-page cursor for the exact account and zone without provider I/O.
    pub fn validate_record_inventory_cursor(
        &self,
        zone_id: &DnsZoneId,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<ValidatedCloudflareRecordCursor> {
        page.validate().map_err(|_| validation("invalid_page"))?;
        if !valid_cloudflare_identifier(zone_id.as_str()) {
            return Err(validation("invalid_cloudflare_zone_id"));
        }
        let now = self.cursor_keys.now()?;
        let cursor = decode_cursor(page, &self.cursor_keys, now, |key| {
            self.record_cursor_scope(zone_id, key)
        })?;
        Ok(ValidatedCloudflareRecordCursor {
            adapter_instance: self.adapter_instance.clone(),
            center_account_id: self.center_account_id.clone(),
            cloudflare_account_id: self.cloudflare_account_id.clone(),
            zone_id: zone_id.clone(),
            request: page.clone(),
            cursor,
            now,
        })
    }

    fn record_cursor_scope(&self, zone_id: &DnsZoneId, key: &CloudflareCursorKey) -> CursorScope {
        CursorScope::Records {
            center_scope: cursor_scope_tag(key, b"center-account", self.center_account_id.as_str()),
            external_scope: cursor_scope_tag(key, b"native-account", &self.cloudflare_account_id),
            zone_scope: cursor_scope_tag(key, b"zone", zone_id.as_str()),
        }
    }

    /// Lists record sets using a cursor proof captured before any provider read.
    pub async fn list_record_sets_with_validated_cursor(
        &self,
        zone: &DnsZoneRef,
        page: &DnsPageRequest,
        validated: ValidatedCloudflareRecordCursor,
    ) -> DnsProviderResult<DnsPage<ObservedDnsRecordSet>> {
        page.validate().map_err(|_| validation("invalid_page"))?;
        self.validate_zone_ref(zone)?;
        if !Arc::ptr_eq(&validated.adapter_instance, &self.adapter_instance)
            || validated.center_account_id != self.center_account_id
            || validated.cloudflare_account_id != self.cloudflare_account_id
            || validated.zone_id != zone.zone_id
            || validated.request != *page
        {
            return Err(validation("cloudflare_cursor_scope_mismatch"));
        }
        paginate(
            self.all_records(zone).await?,
            page,
            |key| self.record_cursor_scope(&zone.zone_id, key),
            &self.cursor_keys,
            validated.cursor,
            validated.now,
        )
    }

    async fn all_zones(&self) -> DnsProviderResult<Vec<ObservedDnsZone>> {
        let mut zones = Vec::new();
        let mut expected_total_pages = None;
        for page in 1..=MAX_ZONE_PROVIDER_PAGES {
            let result = self
                .api
                .list_zones(&self.cloudflare_account_id, page, 50)
                .await?;
            validate_provider_page(page, 50, &result)?;
            if result.total_pages > MAX_ZONE_PROVIDER_PAGES {
                return Err(validation("cloudflare_zone_pagination_limit"));
            }
            validate_stable_total_pages(&mut expected_total_pages, result.total_pages)?;
            for zone in result.items {
                zones.push(self.map_zone(zone)?);
            }
            if zones.len() > MAX_INVENTORY_ZONES {
                return Err(validation("cloudflare_zone_inventory_limit"));
            }
            if page >= result.total_pages {
                zones.sort_by(|left, right| left.zone.zone_id.cmp(&right.zone.zone_id));
                if zones
                    .windows(2)
                    .any(|pair| pair[0].zone.zone_id == pair[1].zone.zone_id)
                {
                    return Err(validation("duplicate_cloudflare_zone_id"));
                }
                return Ok(zones);
            }
        }
        Err(validation("cloudflare_zone_pagination_limit"))
    }

    async fn all_zone_inventory(&self) -> DnsProviderResult<Vec<ObservedCloudflareZone>> {
        let mut zones = Vec::new();
        let mut expected_total_pages = None;
        for page in 1..=MAX_ZONE_PROVIDER_PAGES {
            let result = self
                .api
                .list_zones(&self.cloudflare_account_id, page, 50)
                .await?;
            validate_provider_page(page, 50, &result)?;
            if result.total_pages > MAX_ZONE_PROVIDER_PAGES {
                return Err(validation("cloudflare_zone_pagination_limit"));
            }
            validate_stable_total_pages(&mut expected_total_pages, result.total_pages)?;
            for zone in result.items {
                zones.push(self.map_zone_inventory(zone)?);
            }
            if zones.len() > MAX_INVENTORY_ZONES {
                return Err(validation("cloudflare_zone_inventory_limit"));
            }
            if page >= result.total_pages {
                zones.sort_by(|left, right| left.zone.zone_id.cmp(&right.zone.zone_id));
                if zones
                    .windows(2)
                    .any(|pair| pair[0].zone.zone_id == pair[1].zone.zone_id)
                {
                    return Err(validation("duplicate_cloudflare_zone_id"));
                }
                return Ok(zones);
            }
        }
        Err(validation("cloudflare_zone_pagination_limit"))
    }

    async fn all_records(&self, zone: &DnsZoneRef) -> DnsProviderResult<Vec<ObservedDnsRecordSet>> {
        aggregate_records(zone, self.all_cloudflare_records(zone).await?)
    }

    async fn all_cloudflare_records(
        &self,
        zone: &DnsZoneRef,
    ) -> DnsProviderResult<Vec<CloudflareRecord>> {
        self.validate_zone_ref(zone)?;
        if !valid_cloudflare_identifier(zone.zone_id.as_str()) {
            return Err(validation("invalid_cloudflare_zone_id"));
        }
        let provider_zone = self
            .api
            .get_zone(zone.zone_id.as_str())
            .await?
            .ok_or_else(|| not_found("cloudflare_zone_not_found"))?;
        if self.map_zone(provider_zone)?.zone != *zone {
            return Err(validation("cloudflare_zone_identity_mismatch"));
        }
        let mut records = Vec::new();
        let mut expected_total_pages = None;
        for page in 1..=MAX_RECORD_PROVIDER_PAGES {
            let result = self
                .api
                .list_records(zone.zone_id.as_str(), page, PROVIDER_PAGE_SIZE)
                .await?;
            validate_provider_page(page, PROVIDER_PAGE_SIZE, &result)?;
            if result.total_pages > MAX_RECORD_PROVIDER_PAGES {
                return Err(validation("cloudflare_record_pagination_limit"));
            }
            validate_stable_total_pages(&mut expected_total_pages, result.total_pages)?;
            records.extend(result.items);
            if records.len() > MAX_INVENTORY_RECORDS {
                return Err(validation("cloudflare_record_inventory_limit"));
            }
            if page >= result.total_pages {
                return Ok(records);
            }
        }
        Err(validation("cloudflare_record_pagination_limit"))
    }

    fn map_zone(&self, zone: CloudflareZone) -> DnsProviderResult<ObservedDnsZone> {
        map_zone_for_account(&self.center_account_id, &self.cloudflare_account_id, zone)
    }

    fn map_zone_inventory(
        &self,
        zone: CloudflareZone,
    ) -> DnsProviderResult<ObservedCloudflareZone> {
        map_zone_inventory_for_account(&self.center_account_id, &self.cloudflare_account_id, zone)
    }

    fn validate_zone_ref(&self, zone: &DnsZoneRef) -> DnsProviderResult<()> {
        zone.validate().map_err(|_| validation("invalid_zone"))?;
        if zone.provider != CloudProvider::Cloudflare
            || zone.provider_account_id != self.center_account_id
        {
            return Err(validation("cloudflare_zone_scope_mismatch"));
        }
        Ok(())
    }

    async fn lifecycle_observation(
        &self,
        zone: &DnsZoneRef,
    ) -> ZoneLifecycleProviderResult<Option<ZoneLifecycleObservation>> {
        self.validate_zone_ref(zone)?;
        let Some(provider_zone) = self.api.get_zone(zone.zone_id.as_str()).await? else {
            return Ok(None);
        };
        let observed_zone = self.map_zone(provider_zone.clone())?;
        if observed_zone.zone != *zone {
            return Err(validation("cloudflare_zone_identity_mismatch"));
        }
        let records = self.all_cloudflare_records(zone).await?;
        let non_default_record_count = records
            .iter()
            .filter(|record| {
                !(matches!(record.value, DnsRecordSetValue::Soa { .. })
                    || (record.name == zone.apex.as_str()
                        && matches!(record.value, DnsRecordSetValue::Ns { .. })))
            })
            .count() as u64;
        let dnssec = self.api.get_dnssec(zone.zone_id.as_str()).await?;
        let dnssec_observation = map_dnssec_observation(dnssec.as_ref())?;
        let revision = lifecycle_revision(&provider_zone, dnssec.as_ref(), &records)?;
        let delegation_state = if zone.visibility == ZoneVisibility::Private {
            DelegationState::NotApplicable
        } else {
            DelegationState::NotChecked
        };
        let delegation = DelegationObservation {
            state: delegation_state,
            expected_nameservers: provider_zone.name_servers.clone(),
            parent_nameservers: BTreeSet::new(),
            checked_at: None,
            failure: None,
        };
        let authoritative_verification = AuthoritativeDnsVerification::NotChecked;
        let observation = ZoneLifecycleObservation {
            zone: zone.clone(),
            revision,
            authoritative_nameservers: provider_zone.name_servers,
            delegation,
            readiness: evaluate_zone_readiness(&authoritative_verification),
            authoritative_verification,
            dnssec: dnssec_observation,
            non_default_record_count,
        };
        observation
            .validate()
            .map_err(|_| validation("invalid_cloudflare_lifecycle_observation"))?;
        Ok(Some(observation))
    }

    fn lifecycle_receipt(
        &self,
        mutation: LifecycleMutation,
        state: ZoneLifecycleMutationState,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        let keys = self.mutation_token_keys()?;
        let token = LifecycleMutationToken {
            version: 2,
            center_scope: mutation_scope_tag(
                &keys.active,
                b"center-account",
                self.center_account_id.as_str().as_bytes(),
            ),
            external_scope: mutation_scope_tag(
                &keys.active,
                b"native-account",
                self.cloudflare_account_id.as_bytes(),
            ),
            resource_scope: lifecycle_resource_scope(&keys.active, &mutation)?,
            mutation,
        };
        let mutation_id = ZoneLifecycleMutationId::new(sign_mutation_token(
            &token,
            &keys.active,
            MutationTokenDomain::ZoneLifecycle,
        )?)
        .map_err(|_| validation("cloudflare_lifecycle_receipt_encoding_failed"))?;
        Ok(ZoneLifecycleMutationReceipt { mutation_id, state })
    }

    fn preflight_lifecycle_receipt(
        &self,
        mutation: &LifecycleMutation,
    ) -> ZoneLifecycleProviderResult<()> {
        let keys = self.mutation_token_keys()?;
        let token = LifecycleMutationToken {
            version: 2,
            center_scope: mutation_scope_tag(
                &keys.active,
                b"center-account",
                self.center_account_id.as_str().as_bytes(),
            ),
            external_scope: mutation_scope_tag(
                &keys.active,
                b"native-account",
                self.cloudflare_account_id.as_bytes(),
            ),
            resource_scope: lifecycle_resource_scope(&keys.active, mutation)?,
            mutation: mutation.clone(),
        };
        ZoneLifecycleMutationId::new(sign_mutation_token(
            &token,
            &keys.active,
            MutationTokenDomain::ZoneLifecycle,
        )?)
        .map(|_| ())
        .map_err(|_| validation("cloudflare_lifecycle_receipt_encoding_failed"))
    }

    fn build_receipt(
        &self,
        zone: &DnsZoneRef,
        request: &CloudflareBatchRequest,
    ) -> DnsProviderResult<DnsChangeReceipt> {
        let keys = self.mutation_token_keys()?;
        let request_bytes = serde_json::to_vec(request)
            .map_err(|_| validation("cloudflare_batch_encoding_failed"))?;
        let token = ChangeToken {
            version: 2,
            center_scope: mutation_scope_tag(
                &keys.active,
                b"center-account",
                self.center_account_id.as_str().as_bytes(),
            ),
            external_scope: mutation_scope_tag(
                &keys.active,
                b"native-account",
                self.cloudflare_account_id.as_bytes(),
            ),
            zone_scope: mutation_scope_tag(&keys.active, b"zone", zone.zone_id.as_str().as_bytes()),
            request_scope: mutation_scope_tag(&keys.active, b"request", &request_bytes),
            guard: DnsGuardStrength::BestEffort,
        };
        let id = DnsChangeId::new(sign_mutation_token(
            &token,
            &keys.active,
            MutationTokenDomain::DnsChange,
        )?)
        .map_err(|_| validation("cloudflare_receipt_encoding_failed"))?;
        Ok(committed_receipt(id))
    }

    fn observe_receipt(
        &self,
        zone: &DnsZoneRef,
        change_id: &DnsChangeId,
    ) -> DnsProviderResult<DnsChangeReceipt> {
        let keys = self.mutation_token_keys()?;
        let verified: VerifiedMutationToken<ChangeToken> =
            verify_mutation_token(change_id.as_str(), keys, MutationTokenDomain::DnsChange)
                .map_err(|_| not_found("cloudflare_change_not_found"))?;
        let verification_key = keys.verification_key(verified.key)?;
        let mut token = verified.value;
        if token.version != 2
            || token.center_scope
                != mutation_scope_tag(
                    verification_key,
                    b"center-account",
                    self.center_account_id.as_str().as_bytes(),
                )
            || token.external_scope
                != mutation_scope_tag(
                    verification_key,
                    b"native-account",
                    self.cloudflare_account_id.as_bytes(),
                )
            || token.zone_scope
                != mutation_scope_tag(verification_key, b"zone", zone.zone_id.as_str().as_bytes())
            || token.guard != DnsGuardStrength::BestEffort
        {
            return Err(not_found("cloudflare_change_not_found"));
        }
        token.center_scope = mutation_scope_tag(
            &keys.active,
            b"center-account",
            self.center_account_id.as_str().as_bytes(),
        );
        token.external_scope = mutation_scope_tag(
            &keys.active,
            b"native-account",
            self.cloudflare_account_id.as_bytes(),
        );
        token.zone_scope =
            mutation_scope_tag(&keys.active, b"zone", zone.zone_id.as_str().as_bytes());
        let resealed = DnsChangeId::new(sign_mutation_token(
            &token,
            &keys.active,
            MutationTokenDomain::DnsChange,
        )?)
        .map_err(|_| validation("cloudflare_receipt_encoding_failed"))?;
        Ok(committed_receipt(resealed))
    }
}

fn map_zone_for_account(
    center_account_id: &CloudResourceId,
    cloudflare_account_id: &str,
    zone: CloudflareZone,
) -> DnsProviderResult<ObservedDnsZone> {
    if zone.account_id != cloudflare_account_id {
        return Err(validation("cloudflare_zone_account_mismatch"));
    }
    let visibility = match zone.kind {
        CloudflareZoneKind::Full | CloudflareZoneKind::Partial | CloudflareZoneKind::Secondary => {
            ZoneVisibility::Public
        }
        CloudflareZoneKind::Internal => ZoneVisibility::Private,
    };
    Ok(ObservedDnsZone {
        zone: DnsZoneRef {
            provider_account_id: center_account_id.clone(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(zone.id).map_err(|_| validation("invalid_zone_id"))?,
            apex: AbsoluteDnsName::new(zone.name).map_err(|_| validation("invalid_zone_name"))?,
            visibility,
        },
        revision: zone
            .modified_on
            .map(DnsRecordRevision::new)
            .transpose()
            .map_err(|_| validation("invalid_zone_revision"))?,
    })
}

fn map_zone_inventory_for_account(
    center_account_id: &CloudResourceId,
    cloudflare_account_id: &str,
    zone: CloudflareZone,
) -> DnsProviderResult<ObservedCloudflareZone> {
    if !valid_cloudflare_identifier(&zone.id) {
        return Err(validation("invalid_cloudflare_zone_id"));
    }
    if !valid_cloudflare_identifier(&zone.account_id) {
        return Err(validation("invalid_cloudflare_zone_account_id"));
    }
    let kind = zone.kind;
    let status = zone.status;
    let name_servers = zone.name_servers.clone();
    let observed = map_zone_for_account(center_account_id, cloudflare_account_id, zone)?;
    Ok(ObservedCloudflareZone {
        zone: observed.zone,
        kind,
        status,
        name_servers,
        revision: observed.revision,
    })
}

#[async_trait]
impl DnsProvider for CloudflareDnsAdapter {
    async fn get_zone(&self, zone: &DnsZoneRef) -> DnsProviderResult<Option<ObservedDnsZone>> {
        self.validate_zone_ref(zone)?;
        let Some(observed) = self.api.get_zone(zone.zone_id.as_str()).await? else {
            return Ok(None);
        };
        let observed = self.map_zone(observed)?;
        if observed.zone != *zone {
            return Err(validation("cloudflare_zone_identity_mismatch"));
        }
        Ok(Some(observed))
    }

    async fn list_zones(
        &self,
        provider_account_id: &CloudResourceId,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<DnsPage<ObservedDnsZone>> {
        page.validate().map_err(|_| validation("invalid_page"))?;
        if provider_account_id != &self.center_account_id {
            return Err(validation("cloudflare_account_scope_mismatch"));
        }
        let now = self.cursor_keys.now()?;
        let scope = |key: &CloudflareCursorKey| CursorScope::DnsZones {
            center_scope: cursor_scope_tag(key, b"center-account", self.center_account_id.as_str()),
            external_scope: cursor_scope_tag(key, b"native-account", &self.cloudflare_account_id),
        };
        let cursor = decode_cursor(page, &self.cursor_keys, now, scope)?;
        paginate(
            self.all_zones().await?,
            page,
            scope,
            &self.cursor_keys,
            cursor,
            now,
        )
    }

    async fn get_record_set(
        &self,
        zone: &DnsZoneRef,
        key: &DnsRecordSetKey,
    ) -> DnsProviderResult<Option<ObservedDnsRecordSet>> {
        key.validate()
            .map_err(|_| validation("invalid_record_key"))?;
        Ok(self
            .all_records(zone)
            .await?
            .into_iter()
            .find(|record| &record.record_set.key == key))
    }

    async fn list_record_sets(
        &self,
        zone: &DnsZoneRef,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<DnsPage<ObservedDnsRecordSet>> {
        page.validate().map_err(|_| validation("invalid_page"))?;
        self.validate_zone_ref(zone)?;
        let validated = self.validate_record_inventory_cursor(&zone.zone_id, page)?;
        self.list_record_sets_with_validated_cursor(zone, page, validated)
            .await
    }

    async fn apply_record_changes(
        &self,
        zone: &DnsZoneRef,
        changes: &[DnsRecordChange],
        minimum_guard: DnsGuardStrength,
    ) -> DnsProviderResult<DnsChangeReceipt> {
        self.mutation_token_keys()?;
        self.validate_zone_ref(zone)?;
        validate_dns_changes(zone, changes).map_err(|_| validation("invalid_dns_changes"))?;
        if minimum_guard > DnsGuardStrength::BestEffort {
            return Err(conflict("cloudflare_atomic_guard_unsupported"));
        }
        let current = self.all_records(zone).await?;
        let request = plan_batch(changes, current)?;
        let receipt = self.build_receipt(zone, &request)?;
        let result = self
            .api
            .batch_records(zone.zone_id.as_str(), &request)
            .await?;
        validate_batch_result(zone, changes, &request, result)?;
        Ok(receipt)
    }

    async fn observe_change(
        &self,
        zone: &DnsZoneRef,
        change_id: &DnsChangeId,
    ) -> DnsProviderResult<DnsChangeReceipt> {
        self.mutation_token_keys()?;
        self.validate_zone_ref(zone)?;
        self.observe_receipt(zone, change_id)
    }
}

#[async_trait]
impl CloudflareZoneInventory for CloudflareDnsAdapter {
    async fn list_zone_inventory(
        &self,
        provider_account_id: &CloudResourceId,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<DnsPage<ObservedCloudflareZone>> {
        page.validate().map_err(|_| validation("invalid_page"))?;
        if provider_account_id != &self.center_account_id {
            return Err(validation("cloudflare_account_scope_mismatch"));
        }
        let now = self.cursor_keys.now()?;
        let scope = |key: &CloudflareCursorKey| CursorScope::CloudflareZones {
            center_scope: cursor_scope_tag(key, b"center-account", self.center_account_id.as_str()),
            external_scope: cursor_scope_tag(key, b"native-account", &self.cloudflare_account_id),
        };
        let cursor = decode_cursor(page, &self.cursor_keys, now, scope)?;
        paginate(
            self.all_zone_inventory().await?,
            page,
            scope,
            &self.cursor_keys,
            cursor,
            now,
        )
    }

    async fn get_zone_by_id(
        &self,
        provider_account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> DnsProviderResult<Option<ObservedCloudflareZone>> {
        provider_account_id
            .validate()
            .map_err(|_| validation("invalid_provider_account_id"))?;
        zone_id
            .validate()
            .map_err(|_| validation("invalid_cloudflare_zone_id"))?;
        if provider_account_id != &self.center_account_id {
            return Err(validation("cloudflare_account_scope_mismatch"));
        }
        if !valid_cloudflare_identifier(zone_id.as_str()) {
            return Err(validation("invalid_cloudflare_zone_id"));
        }
        let Some(zone) = self.api.get_zone(zone_id.as_str()).await? else {
            return Ok(None);
        };
        let observed = self.map_zone_inventory(zone)?;
        if &observed.zone.zone_id != zone_id {
            return Err(validation("cloudflare_zone_identity_mismatch"));
        }
        Ok(Some(observed))
    }
}

#[async_trait]
impl ZoneLifecycleProvider for CloudflareDnsAdapter {
    async fn create_zone(
        &self,
        request: &ZoneCreationRequest,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        let keys = self.mutation_token_keys()?;
        let request_scope = mutation_scope_tag(
            &keys.active,
            b"create-idempotency",
            request.idempotency_key.as_str().as_bytes(),
        );
        if request.provider != CloudProvider::Cloudflare
            || request.provider_account_id != self.center_account_id
        {
            return Err(validation("cloudflare_zone_creation_scope_mismatch"));
        }
        if request.visibility != ZoneVisibility::Public {
            return Err(validation("cloudflare_private_zone_creation_unsupported"));
        }
        self.preflight_lifecycle_receipt(&LifecycleMutation::Create {
            zone_id: "0".repeat(32),
            apex: request.apex.clone(),
            request_scope: request_scope.clone(),
        })?;
        let provider_request = CloudflareCreateZoneRequest {
            account_id: self.cloudflare_account_id.clone(),
            name: request.apex.as_str().to_string(),
            kind: CloudflareZoneKind::Full,
        };
        let created = self.api.create_zone(&provider_request).await?;
        if created.account_id != self.cloudflare_account_id
            || created.name != request.apex.as_str()
            || created.kind != CloudflareZoneKind::Full
            || created.name_servers.is_empty()
        {
            return Err(unknown_outcome("cloudflare_create_zone_result_mismatch"));
        }
        if !valid_cloudflare_identifier(&created.id) {
            return Err(unknown_outcome("cloudflare_create_zone_id_invalid"));
        }
        self.lifecycle_receipt(
            LifecycleMutation::Create {
                zone_id: created.id,
                apex: request.apex.clone(),
                request_scope,
            },
            ZoneLifecycleMutationState::Pending,
        )
    }

    async fn observe_zone(
        &self,
        zone: &DnsZoneRef,
    ) -> ZoneLifecycleProviderResult<Option<ZoneLifecycleObservation>> {
        self.lifecycle_observation(zone).await
    }

    async fn set_dnssec(
        &self,
        zone: &DnsZoneRef,
        desired: DnssecDesiredState,
        expected_revision: &ZoneLifecycleRevision,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        self.mutation_token_keys()?;
        let mutation = LifecycleMutation::Dnssec {
            zone_id: zone.zone_id.as_str().to_owned(),
            desired,
        };
        self.preflight_lifecycle_receipt(&mutation)?;
        let observed = self
            .lifecycle_observation(zone)
            .await?
            .ok_or_else(|| not_found("cloudflare_zone_not_found"))?;
        if &observed.revision != expected_revision {
            return Err(conflict("cloudflare_zone_lifecycle_revision_conflict"));
        }
        if desired == DnssecDesiredState::Disabled {
            if observed.dnssec.state != DnssecProviderState::Disabled {
                return Err(conflict(
                    "cloudflare_parent_ds_removal_verification_required",
                ));
            }
            return self.lifecycle_receipt(mutation, ZoneLifecycleMutationState::Succeeded);
        }
        let result = self
            .api
            .patch_dnssec(zone.zone_id.as_str(), desired)
            .await?;
        map_dnssec_observation(Some(&result))?;
        self.lifecycle_receipt(mutation, ZoneLifecycleMutationState::Pending)
    }

    async fn delete_zone(
        &self,
        request: &ZoneDeletionRequest,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        self.mutation_token_keys()?;
        if request.approval().approved_revision != *request.revision()
            || request.approval().approved_zone != *request.zone()
            || request.approval().approved_by.trim().is_empty()
            || request.approval().approved_at.trim().is_empty()
        {
            return Err(validation("invalid_cloudflare_zone_deletion_approval"));
        }
        let mutation = LifecycleMutation::Delete {
            zone_id: request.zone().zone_id.as_str().to_owned(),
        };
        self.preflight_lifecycle_receipt(&mutation)?;
        let observed = self
            .lifecycle_observation(request.zone())
            .await?
            .ok_or_else(|| not_found("cloudflare_zone_not_found"))?;
        if &observed.revision != request.revision() {
            return Err(conflict("cloudflare_zone_lifecycle_revision_conflict"));
        }
        let ack = self
            .api
            .delete_zone(request.zone().zone_id.as_str())
            .await?;
        if ack.id != request.zone().zone_id.as_str() {
            return Err(unknown_outcome("cloudflare_delete_zone_result_mismatch"));
        }
        self.lifecycle_receipt(mutation, ZoneLifecycleMutationState::Pending)
    }

    async fn observe_mutation(
        &self,
        mutation_id: &ZoneLifecycleMutationId,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        let keys = self.mutation_token_keys()?;
        let verified: VerifiedMutationToken<LifecycleMutationToken> = verify_mutation_token(
            mutation_id.as_str(),
            keys,
            MutationTokenDomain::ZoneLifecycle,
        )
        .map_err(|_| not_found("cloudflare_lifecycle_mutation_not_found"))?;
        let verification_key = keys.verification_key(verified.key)?;
        let mut token = verified.value;
        if token.version != 2
            || token.center_scope
                != mutation_scope_tag(
                    verification_key,
                    b"center-account",
                    self.center_account_id.as_str().as_bytes(),
                )
            || token.external_scope
                != mutation_scope_tag(
                    verification_key,
                    b"native-account",
                    self.cloudflare_account_id.as_bytes(),
                )
            || token.resource_scope != lifecycle_resource_scope(verification_key, &token.mutation)?
        {
            return Err(not_found("cloudflare_lifecycle_mutation_not_found"));
        }
        let state = match &token.mutation {
            LifecycleMutation::Create { zone_id, apex, .. } => {
                let expected_zone = DnsZoneRef {
                    provider_account_id: self.center_account_id.clone(),
                    provider: CloudProvider::Cloudflare,
                    zone_id: DnsZoneId::new(zone_id.clone())
                        .map_err(|_| not_found("cloudflare_lifecycle_mutation_not_found"))?,
                    apex: apex.clone(),
                    visibility: ZoneVisibility::Public,
                };
                match self.api.get_zone(zone_id).await? {
                    Some(value) => {
                        if self.map_zone(value)?.zone == expected_zone {
                            ZoneLifecycleMutationState::Succeeded
                        } else {
                            ZoneLifecycleMutationState::UnknownOutcome
                        }
                    }
                    None => ZoneLifecycleMutationState::Pending,
                }
            }
            LifecycleMutation::Delete { zone_id } => {
                if self.api.get_zone(zone_id).await?.is_none() {
                    ZoneLifecycleMutationState::Succeeded
                } else {
                    ZoneLifecycleMutationState::Pending
                }
            }
            LifecycleMutation::Dnssec { zone_id, desired } => {
                let value = self.api.get_dnssec(zone_id).await?;
                dnssec_mutation_state(value.as_ref(), *desired)
            }
        };
        token.center_scope = mutation_scope_tag(
            &keys.active,
            b"center-account",
            self.center_account_id.as_str().as_bytes(),
        );
        token.external_scope = mutation_scope_tag(
            &keys.active,
            b"native-account",
            self.cloudflare_account_id.as_bytes(),
        );
        token.resource_scope = lifecycle_resource_scope(&keys.active, &token.mutation)?;
        let resealed = ZoneLifecycleMutationId::new(sign_mutation_token(
            &token,
            &keys.active,
            MutationTokenDomain::ZoneLifecycle,
        )?)
        .map_err(|_| validation("cloudflare_lifecycle_receipt_encoding_failed"))?;
        Ok(ZoneLifecycleMutationReceipt {
            mutation_id: resealed,
            state,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct LifecycleMutationToken {
    version: u8,
    center_scope: String,
    external_scope: String,
    resource_scope: String,
    mutation: LifecycleMutation,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LifecycleMutation {
    Create {
        zone_id: String,
        apex: AbsoluteDnsName,
        request_scope: String,
    },
    Delete {
        zone_id: String,
    },
    Dnssec {
        zone_id: String,
        desired: DnssecDesiredState,
    },
}

fn lifecycle_resource_scope(
    key: &CloudflareMutationTokenKey,
    mutation: &LifecycleMutation,
) -> DnsProviderResult<String> {
    let encoded = match mutation {
        LifecycleMutation::Create { zone_id, apex, .. } => {
            if !valid_cloudflare_identifier(zone_id) {
                return Err(validation("invalid_cloudflare_lifecycle_locator"));
            }
            format!("create\0{zone_id}\0{}", apex.as_str())
        }
        LifecycleMutation::Delete { zone_id } => {
            if !valid_cloudflare_identifier(zone_id) {
                return Err(validation("invalid_cloudflare_lifecycle_locator"));
            }
            format!("delete\0{zone_id}")
        }
        LifecycleMutation::Dnssec { zone_id, desired } => {
            if !valid_cloudflare_identifier(zone_id) {
                return Err(validation("invalid_cloudflare_lifecycle_locator"));
            }
            let desired = match desired {
                DnssecDesiredState::Disabled => "disabled",
                DnssecDesiredState::Enabled => "enabled",
            };
            format!("dnssec\0{zone_id}\0{desired}")
        }
    };
    Ok(mutation_scope_tag(
        key,
        b"lifecycle-resource",
        encoded.as_bytes(),
    ))
}

fn lifecycle_revision(
    zone: &CloudflareZone,
    dnssec: Option<&CloudflareDnssec>,
    records: &[CloudflareRecord],
) -> ZoneLifecycleProviderResult<ZoneLifecycleRevision> {
    let mut records = records.to_vec();
    records.sort_by(|left, right| left.id.cmp(&right.id));
    let encoded = serde_json::to_vec(&(zone, dnssec, records))
        .map_err(|_| validation("cloudflare_lifecycle_revision_encoding_failed"))?;
    ZoneLifecycleRevision::new(format!("sha256:{:x}", Sha256::digest(encoded)))
        .map_err(|_| validation("invalid_cloudflare_lifecycle_revision"))
}

fn map_dnssec_observation(
    value: Option<&CloudflareDnssec>,
) -> ZoneLifecycleProviderResult<DnssecObservation> {
    let Some(value) = value else {
        return Ok(DnssecObservation {
            state: DnssecProviderState::Unsupported,
            ds_records: Vec::new(),
            external_action: DnssecExternalAction::None,
            provider_detail: Some("dnssec_resource_absent".to_string()),
        });
    };
    let ds_records = value
        .ds
        .as_ref()
        .map(|record| {
            let record = DnssecDsRecord {
                key_tag: record.key_tag,
                algorithm: record.algorithm,
                digest_type: record.digest_type,
                digest: record.digest.to_ascii_uppercase(),
            };
            record
                .validate()
                .map_err(|_| validation("invalid_cloudflare_dnssec_ds"))?;
            Ok(record)
        })
        .transpose()?
        .into_iter()
        .collect::<Vec<_>>();
    let key_tags = ds_records
        .iter()
        .map(|record| record.key_tag)
        .collect::<BTreeSet<_>>();
    let (state, external_action, detail) = match value.status {
        CloudflareDnssecStatus::Active => (
            DnssecProviderState::Active,
            if ds_records.is_empty() {
                DnssecExternalAction::WaitForProviderActivation
            } else {
                DnssecExternalAction::PublishDs {
                    records: ds_records.clone(),
                }
            },
            "active",
        ),
        CloudflareDnssecStatus::Pending => (
            DnssecProviderState::Enabling,
            if ds_records.is_empty() {
                DnssecExternalAction::WaitForProviderActivation
            } else {
                DnssecExternalAction::PublishDs {
                    records: ds_records.clone(),
                }
            },
            "pending",
        ),
        CloudflareDnssecStatus::Disabled if key_tags.is_empty() => (
            DnssecProviderState::Disabled,
            DnssecExternalAction::None,
            "disabled",
        ),
        CloudflareDnssecStatus::Disabled => (
            DnssecProviderState::AwaitingDsRemoval,
            DnssecExternalAction::RemoveDs { key_tags },
            "disabled-awaiting-ds-removal",
        ),
        CloudflareDnssecStatus::PendingDisabled => (
            DnssecProviderState::AwaitingDsRemoval,
            if key_tags.is_empty() {
                DnssecExternalAction::WaitForProviderActivation
            } else {
                DnssecExternalAction::RemoveDs { key_tags }
            },
            "pending-disabled",
        ),
        CloudflareDnssecStatus::Error => (
            DnssecProviderState::Failed,
            DnssecExternalAction::None,
            "error",
        ),
    };
    Ok(DnssecObservation {
        state,
        ds_records,
        external_action,
        provider_detail: Some(detail.to_string()),
    })
}

fn dnssec_mutation_state(
    value: Option<&CloudflareDnssec>,
    desired: DnssecDesiredState,
) -> ZoneLifecycleMutationState {
    match (value.map(|value| value.status), desired) {
        (Some(CloudflareDnssecStatus::Active), DnssecDesiredState::Enabled)
        | (Some(CloudflareDnssecStatus::Disabled), DnssecDesiredState::Disabled) => {
            ZoneLifecycleMutationState::Succeeded
        }
        (Some(CloudflareDnssecStatus::Error), _) => ZoneLifecycleMutationState::Failed,
        (Some(CloudflareDnssecStatus::Pending), DnssecDesiredState::Enabled)
        | (Some(CloudflareDnssecStatus::PendingDisabled), DnssecDesiredState::Disabled) => {
            ZoneLifecycleMutationState::Pending
        }
        _ => ZoneLifecycleMutationState::UnknownOutcome,
    }
}

fn plan_batch(
    changes: &[DnsRecordChange],
    current: Vec<ObservedDnsRecordSet>,
) -> DnsProviderResult<CloudflareBatchRequest> {
    let current = current
        .into_iter()
        .map(|record| (record.record_set.key.clone(), record))
        .collect::<BTreeMap<_, _>>();
    let mut delete_ids = BTreeSet::new();
    let mut posts = Vec::new();
    for change in changes {
        match change {
            DnsRecordChange::Create { record_set, .. } => {
                if current.contains_key(&record_set.key) {
                    return Err(conflict("cloudflare_create_guard_conflict"));
                }
                posts.extend(render_record_set(record_set)?);
            }
            DnsRecordChange::Replace {
                previous, desired, ..
            } => {
                let observed = current
                    .get(&desired.key)
                    .ok_or_else(|| conflict("cloudflare_replace_guard_conflict"))?;
                if observed != previous {
                    return Err(conflict("cloudflare_replace_guard_conflict"));
                }
                collect_delete_ids(observed, &mut delete_ids)?;
                posts.extend(render_record_set(desired)?);
            }
            DnsRecordChange::Delete { previous, .. } => {
                let observed = current
                    .get(&previous.record_set.key)
                    .ok_or_else(|| conflict("cloudflare_delete_guard_conflict"))?;
                if observed != previous {
                    return Err(conflict("cloudflare_delete_guard_conflict"));
                }
                collect_delete_ids(observed, &mut delete_ids)?;
            }
        }
    }
    if delete_ids.len().saturating_add(posts.len()) > MAX_BATCH_OPERATIONS {
        return Err(validation("cloudflare_batch_operation_limit"));
    }
    let request = CloudflareBatchRequest {
        deletes: delete_ids
            .into_iter()
            .map(|id| CloudflareBatchDelete { id })
            .collect(),
        patches: Vec::new(),
        puts: Vec::new(),
        posts,
    };
    if request.deletes.is_empty() && request.posts.is_empty() {
        return Err(validation("empty_cloudflare_batch"));
    }
    Ok(request)
}

fn collect_delete_ids(
    record: &ObservedDnsRecordSet,
    ids: &mut BTreeSet<String>,
) -> DnsProviderResult<()> {
    if record.provider_object_ids.is_empty() {
        return Err(validation("missing_cloudflare_record_object_ids"));
    }
    for id in &record.provider_object_ids {
        if !valid_cloudflare_identifier(id.as_str()) || !ids.insert(id.as_str().to_string()) {
            return Err(validation("invalid_cloudflare_record_object_id"));
        }
    }
    Ok(())
}

fn render_record_set(
    record_set: &ProviderDnsRecordSet,
) -> DnsProviderResult<Vec<CloudflareBatchRecord>> {
    let (proxied, settings, comment, tags) = match &record_set.extension {
        Some(DnsRecordExtension::Cloudflare {
            proxy,
            cname_flattening,
            comment,
            tags,
        }) => {
            let proxied = proxy.map(|value| value == CloudflareProxyOptions::Proxied);
            let flatten_cname = match cname_flattening {
                CloudflareCnameFlattening::ProviderDefault => None,
                CloudflareCnameFlattening::Flatten => Some(true),
                CloudflareCnameFlattening::DoNotFlatten => Some(false),
            };
            (
                proxied,
                flatten_cname.map(|flatten_cname| CloudflareBatchSettings {
                    flatten_cname: Some(flatten_cname),
                }),
                comment.clone(),
                tags.clone(),
            )
        }
        None => (None, None, None, BTreeSet::new()),
        Some(_) => return Err(validation("invalid_cloudflare_record_extension")),
    };
    let ttl = match record_set.ttl {
        DnsTtl::Automatic => 1,
        DnsTtl::Seconds(value) => value,
        DnsTtl::Inherited => return Err(validation("invalid_cloudflare_record_ttl")),
    };
    record_set
        .values
        .iter()
        .map(|value| {
            let rendered = render_value(value)?;
            Ok(CloudflareBatchRecord {
                kind: rendered.kind.to_string(),
                name: record_set.key.owner.as_str().to_string(),
                ttl,
                content: rendered.content,
                data: rendered.data,
                priority: rendered.priority,
                proxied,
                settings: settings.clone(),
                comment: comment.clone(),
                tags: tags.clone(),
            })
        })
        .collect()
}

struct RenderedValue {
    kind: &'static str,
    content: Option<String>,
    data: Option<serde_json::Value>,
    priority: Option<u16>,
}

fn rendered_content(kind: &'static str, content: String) -> RenderedValue {
    RenderedValue {
        kind,
        content: Some(content),
        data: None,
        priority: None,
    }
}

fn render_value(value: &DnsRecordSetValue) -> DnsProviderResult<RenderedValue> {
    Ok(match value {
        DnsRecordSetValue::A { address } => rendered_content("A", address.to_string()),
        DnsRecordSetValue::Aaaa { address } => rendered_content("AAAA", address.to_string()),
        DnsRecordSetValue::Cname { target } => {
            rendered_content("CNAME", target.as_str().to_string())
        }
        DnsRecordSetValue::Txt { value } => rendered_content("TXT", render_txt(value)),
        DnsRecordSetValue::Mx {
            preference,
            exchange,
        } => RenderedValue {
            kind: "MX",
            content: Some(exchange.as_str().to_string()),
            data: Some(serde_json::json!({
                "priority": preference,
                "target": exchange.as_str(),
            })),
            priority: Some(*preference),
        },
        DnsRecordSetValue::Srv {
            priority,
            weight,
            port,
            target,
        } => RenderedValue {
            kind: "SRV",
            content: None,
            data: Some(serde_json::json!({
                "priority": priority,
                "weight": weight,
                "port": port,
                "target": target.as_str(),
            })),
            priority: None,
        },
        DnsRecordSetValue::Caa { flags, tag, value } => {
            let value = std::str::from_utf8(value.as_bytes())
                .map_err(|_| validation("cloudflare_caa_value_not_utf8"))?;
            RenderedValue {
                kind: "CAA",
                content: None,
                data: Some(serde_json::json!({
                    "flags": flags,
                    "tag": tag.as_str(),
                    "value": value,
                })),
                priority: None,
            }
        }
        DnsRecordSetValue::Ns { target } => rendered_content("NS", target.as_str().to_string()),
        DnsRecordSetValue::Soa { .. } => {
            return Err(validation("cloudflare_soa_mutation_unsupported"));
        }
    })
}

fn render_txt(value: &edgion_center_core::DnsTxtValue) -> String {
    value
        .segments()
        .iter()
        .map(|segment| {
            let mut rendered = String::from("\"");
            for byte in segment.as_bytes() {
                match *byte {
                    b'"' | b'\\' => {
                        rendered.push('\\');
                        rendered.push(char::from(*byte));
                    }
                    0x20..=0x7e => rendered.push(char::from(*byte)),
                    value => rendered.push_str(&format!("\\{value:03}")),
                }
            }
            rendered.push('"');
            rendered
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_batch_result(
    zone: &DnsZoneRef,
    changes: &[DnsRecordChange],
    request: &CloudflareBatchRequest,
    result: CloudflareBatchResult,
) -> DnsProviderResult<()> {
    let expected_deleted = request
        .deletes
        .iter()
        .map(|item| item.id.clone())
        .collect::<BTreeSet<_>>();
    let actual_deleted = result
        .deletes
        .iter()
        .map(|item| item.id.clone())
        .collect::<BTreeSet<_>>();
    if result.deletes.len() != expected_deleted.len() || actual_deleted != expected_deleted {
        return Err(unknown_outcome("cloudflare_batch_delete_result_mismatch"));
    }
    if result.posts.len() != request.posts.len()
        || result
            .posts
            .iter()
            .any(|record| !valid_cloudflare_identifier(&record.id))
    {
        return Err(unknown_outcome("cloudflare_batch_post_result_mismatch"));
    }
    let observed = aggregate_records(zone, result.posts)
        .map_err(|_| unknown_outcome("cloudflare_batch_post_result_invalid"))?;
    let actual = observed
        .into_iter()
        .map(|record| (record.record_set.key.clone(), record.record_set))
        .collect::<BTreeMap<_, _>>();
    let expected = changes
        .iter()
        .filter_map(|change| match change {
            DnsRecordChange::Create { record_set, .. }
            | DnsRecordChange::Replace {
                desired: record_set,
                ..
            } => Some((record_set.key.clone(), record_set.clone())),
            DnsRecordChange::Delete { .. } => None,
        })
        .collect::<BTreeMap<_, _>>();
    if actual != expected {
        return Err(unknown_outcome("cloudflare_batch_post_result_mismatch"));
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct ChangeToken {
    #[serde(rename = "v")]
    version: u8,
    #[serde(rename = "c")]
    center_scope: String,
    #[serde(rename = "a")]
    external_scope: String,
    #[serde(rename = "z")]
    zone_scope: String,
    #[serde(rename = "r")]
    request_scope: String,
    #[serde(rename = "g")]
    guard: DnsGuardStrength,
}

#[cfg(test)]
fn scope_hash(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

fn committed_receipt(id: DnsChangeId) -> DnsChangeReceipt {
    DnsChangeReceipt {
        id,
        state: DnsChangeState::ProviderCommitted,
        submission_atomicity: DnsBatchAtomicity::AllOrNothing,
        propagation: DnsPropagationState::ProviderReportedApplied,
        guard_strength: DnsGuardStrength::BestEffort,
    }
}

#[derive(Clone, Copy)]
enum MutationTokenDomain {
    DnsChange,
    ZoneLifecycle,
}

impl MutationTokenDomain {
    fn separator(self) -> &'static [u8] {
        match self {
            Self::DnsChange => b"edgion-cloudflare-dns-change-token-v2\0",
            Self::ZoneLifecycle => b"edgion-cloudflare-zone-lifecycle-token-v2\0",
        }
    }

    fn maximum_encoded_len(self) -> usize {
        match self {
            Self::DnsChange => 512,
            Self::ZoneLifecycle => 1_024,
        }
    }

    fn maximum_decoded_len(self) -> usize {
        self.maximum_encoded_len().saturating_mul(3) / 4
    }
}

#[derive(Clone, Copy)]
enum MutationVerificationKey {
    Active,
    Fallback,
}

struct VerifiedMutationToken<T> {
    value: T,
    key: MutationVerificationKey,
}

impl CloudflareMutationTokenKeyRing {
    fn verification_key(
        &self,
        key: MutationVerificationKey,
    ) -> DnsProviderResult<&CloudflareMutationTokenKey> {
        match key {
            MutationVerificationKey::Active => Ok(&self.active),
            MutationVerificationKey::Fallback => self
                .fallback
                .as_ref()
                .ok_or_else(|| validation("invalid_cloudflare_token_signature")),
        }
    }
}

fn mutation_scope_tag(key: &CloudflareMutationTokenKey, domain: &[u8], value: &[u8]) -> String {
    let tag = HmacSha256::new_from_slice(key.0.as_ref())
        .expect("fixed HMAC key")
        .chain_update(b"edgion-cloudflare-mutation-scope-v2\0")
        .chain_update(domain)
        .chain_update(b"\0")
        .chain_update(value)
        .finalize()
        .into_bytes();
    URL_SAFE_NO_PAD.encode(tag)
}

fn sign_mutation_token<T: Serialize>(
    value: &T,
    key: &CloudflareMutationTokenKey,
    domain: MutationTokenDomain,
) -> DnsProviderResult<String> {
    let encoded =
        serde_json::to_vec(value).map_err(|_| validation("cloudflare_token_encoding_failed"))?;
    let signature = HmacSha256::new_from_slice(key.0.as_ref())
        .expect("fixed HMAC key")
        .chain_update(domain.separator())
        .chain_update(&encoded)
        .finalize()
        .into_bytes();
    let mut authenticated = encoded;
    authenticated.extend_from_slice(&signature);
    let encoded = URL_SAFE_NO_PAD.encode(authenticated);
    if encoded.len() > domain.maximum_encoded_len() {
        return Err(validation("cloudflare_token_encoding_failed"));
    }
    Ok(encoded)
}

fn verify_mutation_token<T: for<'de> Deserialize<'de>>(
    value: &str,
    keys: &CloudflareMutationTokenKeyRing,
    domain: MutationTokenDomain,
) -> DnsProviderResult<VerifiedMutationToken<T>> {
    if value.len() > domain.maximum_encoded_len() {
        return Err(validation("invalid_cloudflare_token"));
    }
    let authenticated = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| validation("invalid_cloudflare_token"))?;
    if authenticated.len() < 32 || authenticated.len() > domain.maximum_decoded_len() {
        return Err(validation("invalid_cloudflare_token"));
    }
    let (encoded, signature) = authenticated.split_at(authenticated.len() - 32);
    let verified_key = if verify_mutation_token_signature(encoded, signature, &keys.active, domain)
    {
        MutationVerificationKey::Active
    } else if keys.fallback.as_ref().is_some_and(|fallback| {
        verify_mutation_token_signature(encoded, signature, fallback, domain)
    }) {
        MutationVerificationKey::Fallback
    } else {
        return Err(validation("invalid_cloudflare_token_signature"));
    };
    let value =
        serde_json::from_slice(encoded).map_err(|_| validation("invalid_cloudflare_token"))?;
    Ok(VerifiedMutationToken {
        value,
        key: verified_key,
    })
}

fn verify_mutation_token_signature(
    encoded: &[u8],
    signature: &[u8],
    key: &CloudflareMutationTokenKey,
    domain: MutationTokenDomain,
) -> bool {
    HmacSha256::new_from_slice(key.0.as_ref())
        .expect("fixed HMAC key")
        .chain_update(domain.separator())
        .chain_update(encoded)
        .verify_slice(signature)
        .is_ok()
}

fn aggregate_records(
    zone: &DnsZoneRef,
    records: Vec<CloudflareRecord>,
) -> DnsProviderResult<Vec<ObservedDnsRecordSet>> {
    let mut groups = BTreeMap::<DnsRecordSetKey, Vec<CloudflareRecord>>::new();
    let mut object_ids = BTreeSet::new();
    for record in records {
        if !valid_cloudflare_identifier(&record.id) {
            return Err(validation("invalid_cloudflare_record_object_id"));
        }
        if !object_ids.insert(record.id.clone()) {
            return Err(validation("duplicate_cloudflare_record_id"));
        }
        if record.ipv4_only || record.ipv6_only || record.private_routing {
            return Err(validation("unsupported_cloudflare_record_settings"));
        }
        let key = DnsRecordSetKey {
            owner: DnsOwnerName::new(&record.name)
                .map_err(|_| validation("invalid_record_name"))?,
            record_type: record.value.record_type(),
            routing: DnsRoutingIdentity::Simple,
        };
        if !key.owner.is_within(&zone.apex) {
            return Err(validation("cloudflare_record_outside_zone"));
        }
        groups.entry(key).or_default().push(record);
    }
    groups
        .into_iter()
        .map(|(key, mut members)| {
            members.sort_by(|left, right| left.id.cmp(&right.id));
            let first = members.first().expect("non-empty DNS record group");
            let signature = member_signature(first, key.record_type)?;
            if members
                .iter()
                .skip(1)
                .any(|member| member_signature(member, key.record_type).as_ref() != Ok(&signature))
            {
                return Err(validation("heterogeneous_cloudflare_rrset"));
            }
            let values = members
                .iter()
                .map(|member| member.value.clone())
                .collect::<BTreeSet<_>>();
            if values.len() != members.len() {
                return Err(validation("duplicate_cloudflare_record_value"));
            }
            let extension = cloudflare_extension(first, key.record_type)?;
            let record_set = ProviderDnsRecordSet {
                key,
                ttl: if first.ttl == 1 {
                    DnsTtl::Automatic
                } else {
                    DnsTtl::Seconds(first.ttl)
                },
                values,
                extension,
            };
            record_set
                .validate(zone)
                .map_err(|_| validation("invalid_cloudflare_rrset"))?;
            let encoded = serde_json::to_vec(&members)
                .map_err(|_| validation("cloudflare_revision_encoding_failed"))?;
            let revision = format!("sha256:{:x}", Sha256::digest(encoded));
            Ok(ObservedDnsRecordSet {
                zone: zone.clone(),
                provider_object_ids: members
                    .iter()
                    .map(|member| {
                        DnsRecordObjectId::new(&member.id)
                            .map_err(|_| validation("invalid_record_object_id"))
                    })
                    .collect::<Result<_, _>>()?,
                record_set,
                revision: DnsRecordRevision::new(revision)
                    .map_err(|_| validation("invalid_record_revision"))?,
            })
        })
        .collect()
}

#[derive(PartialEq, Eq)]
struct MemberSignature {
    ttl: u32,
    proxied: Option<bool>,
    proxiable: Option<bool>,
    flatten_cname: Option<bool>,
    comment: Option<String>,
    tags: BTreeSet<String>,
}

fn member_signature(
    record: &CloudflareRecord,
    record_type: ProviderDnsRecordType,
) -> DnsProviderResult<MemberSignature> {
    let proxy_fields = matches!(
        record_type,
        ProviderDnsRecordType::A | ProviderDnsRecordType::Aaaa | ProviderDnsRecordType::Cname
    );
    Ok(MemberSignature {
        ttl: record.ttl,
        proxied: proxy_fields.then_some(record.proxied).flatten(),
        proxiable: proxy_fields.then_some(record.proxiable),
        flatten_cname: (record_type == ProviderDnsRecordType::Cname)
            .then_some(record.flatten_cname)
            .flatten(),
        comment: record.comment.clone(),
        tags: record.tags.clone(),
    })
}

fn cloudflare_extension(
    record: &CloudflareRecord,
    record_type: ProviderDnsRecordType,
) -> DnsProviderResult<Option<DnsRecordExtension>> {
    if record_type != ProviderDnsRecordType::Cname && record.flatten_cname.is_some() {
        return Err(validation("invalid_cloudflare_cname_flattening"));
    }
    let proxiable = matches!(
        record_type,
        ProviderDnsRecordType::A | ProviderDnsRecordType::Aaaa | ProviderDnsRecordType::Cname
    );
    let proxy = match (proxiable, record.proxied) {
        (true, Some(true)) => Some(CloudflareProxyOptions::Proxied),
        (true, Some(false)) => Some(CloudflareProxyOptions::DnsOnly),
        (true, None) => return Err(validation("missing_cloudflare_proxy_state")),
        (false, None | Some(false)) => None,
        (false, Some(true)) => return Err(validation("invalid_cloudflare_proxy_state")),
    };
    if proxy == Some(CloudflareProxyOptions::Proxied) && !record.proxiable {
        return Err(validation("cloudflare_record_not_proxiable"));
    }
    let cname_flattening = match (record_type, record.flatten_cname) {
        (ProviderDnsRecordType::Cname, Some(true)) => CloudflareCnameFlattening::Flatten,
        (ProviderDnsRecordType::Cname, Some(false)) => CloudflareCnameFlattening::DoNotFlatten,
        _ => CloudflareCnameFlattening::ProviderDefault,
    };
    if !proxiable
        && record.ttl != 1
        && record.comment.is_none()
        && record.tags.is_empty()
        && cname_flattening == CloudflareCnameFlattening::ProviderDefault
    {
        return Ok(None);
    }
    Ok(Some(DnsRecordExtension::Cloudflare {
        proxy,
        cname_flattening,
        comment: record.comment.clone(),
        tags: record.tags.clone(),
    }))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum CursorScope {
    DnsZones {
        center_scope: String,
        external_scope: String,
    },
    CloudflareZones {
        center_scope: String,
        external_scope: String,
    },
    Records {
        center_scope: String,
        external_scope: String,
        zone_scope: String,
    },
}

fn cursor_scope_tag(key: &CloudflareCursorKey, domain: &[u8], value: &str) -> String {
    let tag = HmacSha256::new_from_slice(key.0.as_ref())
        .expect("fixed HMAC key")
        .chain_update(b"edgion-cloudflare-cursor-scope-v2\0")
        .chain_update(domain)
        .chain_update(b"\0")
        .chain_update(value.as_bytes())
        .finalize()
        .into_bytes();
    URL_SAFE_NO_PAD.encode(tag)
}

#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    version: u8,
    scope: CursorScope,
    offset: u64,
    limit: u16,
    inventory_tag: String,
    #[serde(default)]
    issued_at: u64,
    #[serde(default)]
    expires_at: u64,
}

#[derive(Debug, Clone, Copy)]
enum CursorVerificationKey {
    Active,
    Fallback,
}

struct VerifiedCursor {
    cursor: Cursor,
    key: CursorVerificationKey,
}

fn decode_cursor<F>(
    request: &DnsPageRequest,
    keys: &CloudflareCursorKeyRing,
    now: u64,
    scope: F,
) -> DnsProviderResult<Option<VerifiedCursor>>
where
    F: Fn(&CloudflareCursorKey) -> CursorScope,
{
    let cursor = match request.token.as_ref() {
        None => None,
        Some(token) => {
            let authenticated = URL_SAFE_NO_PAD
                .decode(token.as_str())
                .map_err(|_| validation("invalid_cloudflare_cursor"))?;
            if authenticated.len() < 32 {
                return Err(validation("invalid_cloudflare_cursor"));
            }
            let (bytes, signature) = authenticated.split_at(authenticated.len() - 32);
            let (key, verification_key) = if verify_cursor_signature(bytes, signature, &keys.active)
            {
                (&keys.active, CursorVerificationKey::Active)
            } else if let Some(fallback) = keys
                .fallback
                .as_ref()
                .filter(|fallback| verify_cursor_signature(bytes, signature, fallback))
            {
                (fallback, CursorVerificationKey::Fallback)
            } else {
                return Err(validation("invalid_cloudflare_cursor_signature"));
            };
            let cursor: Cursor = serde_json::from_slice(bytes)
                .map_err(|_| validation("invalid_cloudflare_cursor"))?;
            if cursor.version != 4 {
                return Err(validation("unsupported_cloudflare_cursor_version"));
            }
            if cursor.scope != scope(key) {
                return Err(validation("cloudflare_cursor_scope_mismatch"));
            }
            if cursor.limit != request.limit {
                return Err(validation("cloudflare_cursor_page_size_mismatch"));
            }
            if cursor.offset == 0 || cursor.inventory_tag.is_empty() {
                return Err(validation("invalid_cloudflare_cursor_offset"));
            }
            validate_cursor_time(&cursor, keys, now)?;
            Some(VerifiedCursor {
                cursor,
                key: verification_key,
            })
        }
    };
    Ok(cursor)
}

fn verify_cursor_signature(bytes: &[u8], signature: &[u8], key: &CloudflareCursorKey) -> bool {
    HmacSha256::new_from_slice(key.0.as_ref())
        .expect("fixed HMAC key")
        .chain_update(bytes)
        .verify_slice(signature)
        .is_ok()
}

fn validate_cursor_time(
    cursor: &Cursor,
    keys: &CloudflareCursorKeyRing,
    now: u64,
) -> DnsProviderResult<()> {
    let lifetime = cursor
        .expires_at
        .checked_sub(cursor.issued_at)
        .ok_or_else(|| validation("invalid_cloudflare_cursor_time"))?;
    if lifetime == 0 || lifetime > keys.ttl_secs {
        return Err(validation("invalid_cloudflare_cursor_time"));
    }
    let latest_issued_at = now
        .checked_add(keys.clock_skew_secs)
        .ok_or_else(|| validation("invalid_cloudflare_cursor_time"))?;
    if cursor.issued_at > latest_issued_at {
        return Err(validation("cloudflare_cursor_not_yet_valid"));
    }
    let latest_valid_at = cursor
        .expires_at
        .checked_add(keys.clock_skew_secs)
        .ok_or_else(|| validation("invalid_cloudflare_cursor_time"))?;
    if now > latest_valid_at {
        return Err(validation("cloudflare_cursor_expired"));
    }
    Ok(())
}

fn paginate<T: Serialize, F>(
    items: Vec<T>,
    request: &DnsPageRequest,
    scope: F,
    keys: &CloudflareCursorKeyRing,
    cursor: Option<VerifiedCursor>,
    now: u64,
) -> DnsProviderResult<DnsPage<T>>
where
    F: Fn(&CloudflareCursorKey) -> CursorScope,
{
    let verified_with_fallback = matches!(
        cursor.as_ref().map(|cursor| cursor.key),
        Some(CursorVerificationKey::Fallback)
    );
    let verification_key = match cursor.as_ref().map(|cursor| cursor.key) {
        Some(CursorVerificationKey::Fallback) => keys
            .fallback
            .as_ref()
            .ok_or_else(|| validation("cloudflare_cursor_scope_mismatch"))?,
        Some(CursorVerificationKey::Active) | None => &keys.active,
    };
    let verified_inventory_tag = inventory_tag(&items, verification_key)?;
    let offset = match cursor {
        Some(verified) => {
            if verified.cursor.inventory_tag != verified_inventory_tag {
                return Err(validation("cloudflare_inventory_changed"));
            }
            usize::try_from(verified.cursor.offset)
                .map_err(|_| validation("invalid_cloudflare_cursor_offset"))?
        }
        None => 0,
    };
    if offset > items.len() {
        return Err(validation("invalid_cloudflare_cursor_offset"));
    }
    let end = offset
        .checked_add(usize::from(request.limit))
        .ok_or_else(|| validation("invalid_cloudflare_cursor_offset"))?
        .min(items.len());
    let next = if end < items.len() {
        let expires_at = now
            .checked_add(keys.ttl_secs)
            .ok_or_else(|| validation("cloudflare_cursor_encoding_failed"))?;
        let active_inventory_tag = if verified_with_fallback {
            inventory_tag(&items, &keys.active)?
        } else {
            verified_inventory_tag
        };
        let encoded = serde_json::to_vec(&Cursor {
            version: 4,
            scope: scope(&keys.active),
            offset: u64::try_from(end)
                .map_err(|_| validation("cloudflare_cursor_encoding_failed"))?,
            limit: request.limit,
            inventory_tag: active_inventory_tag,
            issued_at: now,
            expires_at,
        })
        .map_err(|_| validation("cloudflare_cursor_encoding_failed"))?;
        let signature = HmacSha256::new_from_slice(keys.active.0.as_ref())
            .expect("fixed HMAC key")
            .chain_update(&encoded)
            .finalize()
            .into_bytes();
        let mut authenticated = encoded;
        authenticated.extend_from_slice(&signature);
        Some(
            DnsPageToken::new(URL_SAFE_NO_PAD.encode(authenticated))
                .map_err(|_| validation("cloudflare_cursor_encoding_failed"))?,
        )
    } else {
        None
    };
    Ok(DnsPage {
        items: items.into_iter().skip(offset).take(end - offset).collect(),
        next,
    })
}

fn inventory_tag<T: Serialize>(
    items: &[T],
    key: &CloudflareCursorKey,
) -> DnsProviderResult<String> {
    inventory_tag_with_limit(items, key, MAX_INVENTORY_TAG_BYTES)
}

fn inventory_tag_with_limit<T: Serialize>(
    items: &[T],
    key: &CloudflareCursorKey,
    limit: usize,
) -> DnsProviderResult<String> {
    let mac = HmacSha256::new_from_slice(key.0.as_ref())
        .expect("fixed HMAC key")
        .chain_update(b"edgion-cloudflare-inventory-v3\0");
    let mut writer = InventoryTagWriter {
        mac,
        bytes: 0,
        limit,
        exceeded: false,
    };
    if serde_json::to_writer(&mut writer, items).is_err() {
        return Err(validation(if writer.exceeded {
            "cloudflare_inventory_serialization_limit"
        } else {
            "cloudflare_inventory_encoding_failed"
        }));
    }
    let tag = writer.mac.finalize().into_bytes();
    Ok(URL_SAFE_NO_PAD.encode(tag))
}

struct InventoryTagWriter {
    mac: HmacSha256,
    bytes: usize,
    limit: usize,
    exceeded: bool,
}

impl Write for InventoryTagWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let Some(total) = self.bytes.checked_add(bytes.len()) else {
            self.exceeded = true;
            return Err(std::io::Error::other("inventory tag byte limit exceeded"));
        };
        if total > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::other("inventory tag byte limit exceeded"));
        }
        self.mac.update(bytes);
        self.bytes = total;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn validate_provider_page<T>(
    requested: u32,
    per_page: u32,
    page: &CloudflarePage<T>,
) -> DnsProviderResult<()> {
    if page.page != requested
        || (page.total_pages == 0 && (requested != 1 || !page.items.is_empty()))
        || (page.total_pages != 0 && page.total_pages < page.page)
        || page.items.len() > per_page as usize
    {
        return Err(validation("invalid_cloudflare_provider_page"));
    }
    Ok(())
}

fn validate_stable_total_pages(expected: &mut Option<u32>, observed: u32) -> DnsProviderResult<()> {
    match expected {
        None => *expected = Some(observed),
        Some(value) if *value == observed => {}
        Some(_) => return Err(validation("cloudflare_total_pages_changed")),
    }
    Ok(())
}

fn validation(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Validation, code)
}

fn conflict(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Conflict, code)
}

fn not_found(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::NotFound, code)
}

fn unknown_outcome(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::UnknownOutcome, code)
}

fn valid_cloudflare_identifier(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn provider_error(category: ProviderErrorCategory, code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        code,
        "Cloudflare DNS adapter rejected the request",
        None,
        None,
    )
    .expect("static normalized provider error")
}

#[cfg(test)]
mod tests {
    use std::{
        net::Ipv4Addr,
        sync::{
            atomic::{AtomicU64, Ordering},
            Mutex,
        },
    };

    use edgion_center_core::{
        cloud_test_support::{assert_dns_provider_conformance, DnsAdapterConformanceFixture},
        CredentialSource, DnsCharacterString, DnsTxtValue, IdempotencyKey, ZoneReadiness,
    };
    use wiremock::{
        matchers::{body_json, header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    use super::*;

    const ZONE_A_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ZONE_B_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const ZONE_OTHER_ID: &str = "cccccccccccccccccccccccccccccccc";
    const RECORD_A_ID: &str = "11111111111111111111111111111111";
    const RECORD_B_ID: &str = "22222222222222222222222222222222";
    const RECORD_C_ID: &str = "33333333333333333333333333333333";

    struct FixedCursorClock(AtomicU64);

    impl FixedCursorClock {
        fn new(now: u64) -> Self {
            Self(AtomicU64::new(now))
        }

        fn set(&self, now: u64) {
            self.0.store(now, Ordering::SeqCst);
        }
    }

    impl CloudflareCursorClock for FixedCursorClock {
        fn unix_seconds(&self) -> DnsProviderResult<u64> {
            Ok(self.0.load(Ordering::SeqCst))
        }
    }

    fn cursor_ring(
        active: [u8; 32],
        fallback: Option<[u8; 32]>,
        clock: Arc<FixedCursorClock>,
    ) -> CloudflareCursorKeyRing {
        CloudflareCursorKeyRing::with_clock(
            CloudflareCursorKey::new(active).unwrap(),
            fallback.map(|key| CloudflareCursorKey::new(key).unwrap()),
            Duration::from_secs(900),
            Duration::from_secs(30),
            clock,
        )
        .unwrap()
    }

    struct FakeApi {
        total_calls: AtomicU64,
        zones: Mutex<Vec<CloudflareZone>>,
        create_zone_override: Mutex<Option<CloudflareZone>>,
        get_zone_override: Mutex<Option<CloudflareZone>>,
        get_zone_calls: Mutex<u64>,
        list_zone_calls: Mutex<u64>,
        list_record_calls: Mutex<u64>,
        delete_zone_calls: Mutex<u64>,
        delete_zone_ack_override: Mutex<Option<String>>,
        batch_record_calls: Mutex<u64>,
        batch_result_override: Mutex<Option<CloudflareBatchResult>>,
        zone_total_pages: u32,
        records: Mutex<BTreeMap<String, Vec<CloudflareRecord>>>,
        dnssec: Mutex<BTreeMap<String, CloudflareDnssec>>,
        sequence: Mutex<u64>,
    }

    #[async_trait]
    impl CloudflareApi for FakeApi {
        async fn create_zone(
            &self,
            request: &CloudflareCreateZoneRequest,
        ) -> CloudflareApiResult<CloudflareZone> {
            self.total_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(zone) = self.create_zone_override.lock().unwrap().clone() {
                return Ok(zone);
            }
            let mut sequence = self.sequence.lock().unwrap();
            *sequence += 1;
            let zone = CloudflareZone {
                id: format!("{:032x}", *sequence),
                account_id: request.account_id.clone(),
                name: request.name.clone(),
                kind: request.kind,
                status: CloudflareZoneStatus::Pending,
                name_servers: [
                    AbsoluteDnsName::new("ada.ns.cloudflare.com").unwrap(),
                    AbsoluteDnsName::new("bob.ns.cloudflare.com").unwrap(),
                ]
                .into_iter()
                .collect(),
                modified_on: Some(format!("zone-revision-{}", *sequence)),
            };
            drop(sequence);
            self.zones.lock().unwrap().push(zone.clone());
            Ok(zone)
        }

        async fn get_zone(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareZone>> {
            self.total_calls.fetch_add(1, Ordering::SeqCst);
            *self.get_zone_calls.lock().unwrap() += 1;
            if let Some(zone) = self.get_zone_override.lock().unwrap().clone() {
                return Ok(Some(zone));
            }
            Ok(self
                .zones
                .lock()
                .unwrap()
                .iter()
                .find(|zone| zone.id == zone_id)
                .cloned())
        }

        async fn delete_zone(&self, zone_id: &str) -> CloudflareApiResult<CloudflareDeleteZoneAck> {
            self.total_calls.fetch_add(1, Ordering::SeqCst);
            *self.delete_zone_calls.lock().unwrap() += 1;
            self.zones.lock().unwrap().retain(|zone| zone.id != zone_id);
            Ok(CloudflareDeleteZoneAck {
                id: self
                    .delete_zone_ack_override
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| zone_id.to_string()),
            })
        }

        async fn get_dnssec(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareDnssec>> {
            self.total_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.dnssec.lock().unwrap().get(zone_id).cloned())
        }

        async fn patch_dnssec(
            &self,
            zone_id: &str,
            desired: DnssecDesiredState,
        ) -> CloudflareApiResult<CloudflareDnssec> {
            self.total_calls.fetch_add(1, Ordering::SeqCst);
            let value = CloudflareDnssec {
                status: match desired {
                    DnssecDesiredState::Enabled => CloudflareDnssecStatus::Active,
                    DnssecDesiredState::Disabled => CloudflareDnssecStatus::Disabled,
                },
                ds: (desired == DnssecDesiredState::Enabled).then_some(CloudflareDnssecDs {
                    key_tag: 2371,
                    algorithm: 13,
                    digest_type: 2,
                    digest: "AA".repeat(32),
                }),
                modified_on: Some("dnssec-revision".to_string()),
            };
            self.dnssec
                .lock()
                .unwrap()
                .insert(zone_id.to_string(), value.clone());
            Ok(value)
        }

        async fn list_zones(
            &self,
            account_id: &str,
            page: u32,
            _per_page: u32,
        ) -> CloudflareApiResult<CloudflarePage<CloudflareZone>> {
            self.total_calls.fetch_add(1, Ordering::SeqCst);
            *self.list_zone_calls.lock().unwrap() += 1;
            Ok(CloudflarePage {
                items: self
                    .zones
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|zone| zone.account_id == account_id)
                    .cloned()
                    .collect(),
                page,
                total_pages: self.zone_total_pages,
            })
        }

        async fn list_records(
            &self,
            zone_id: &str,
            page: u32,
            _per_page: u32,
        ) -> CloudflareApiResult<CloudflarePage<CloudflareRecord>> {
            self.total_calls.fetch_add(1, Ordering::SeqCst);
            *self.list_record_calls.lock().unwrap() += 1;
            Ok(CloudflarePage {
                items: self
                    .records
                    .lock()
                    .unwrap()
                    .get(zone_id)
                    .cloned()
                    .unwrap_or_default(),
                page,
                total_pages: 1,
            })
        }

        async fn batch_records(
            &self,
            zone_id: &str,
            request: &CloudflareBatchRequest,
        ) -> CloudflareApiResult<CloudflareBatchResult> {
            self.total_calls.fetch_add(1, Ordering::SeqCst);
            *self.batch_record_calls.lock().unwrap() += 1;
            let mut records = self.records.lock().unwrap();
            let mut candidate = records.get(zone_id).cloned().unwrap_or_default();
            let mut deleted = Vec::new();
            for deletion in &request.deletes {
                let index = candidate
                    .iter()
                    .position(|record| record.id == deletion.id)
                    .ok_or_else(|| conflict("fake_cloudflare_delete_missing"))?;
                deleted.push(candidate.remove(index));
            }
            let posted_keys = request
                .posts
                .iter()
                .map(|post| (post.name.clone(), post.kind.clone()))
                .collect::<BTreeSet<_>>();
            if candidate.iter().any(|record| {
                posted_keys.contains(&(
                    record.name.clone(),
                    record_type_name(record.value.record_type()).to_string(),
                ))
            }) {
                return Err(conflict("fake_cloudflare_create_exists"));
            }
            let mut posted = Vec::new();
            for post in &request.posts {
                let mut sequence = self.sequence.lock().unwrap();
                *sequence += 1;
                let id = format!("{:032x}", *sequence);
                drop(sequence);
                let record = fake_record_from_post(id, post)?;
                candidate.push(record.clone());
                posted.push(record);
            }
            records.insert(zone_id.to_string(), candidate);
            let result = CloudflareBatchResult {
                deletes: deleted,
                posts: posted,
            };
            Ok(self
                .batch_result_override
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(result))
        }
    }

    #[tokio::test]
    async fn scoped_adapter_passes_the_complete_shared_dns_contract() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let account = account();
        let adapter = write_adapter(
            center_account.clone(),
            &account,
            Arc::new(fake_api()),
            [7; 32],
        )
        .unwrap();
        let zones = adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 10,
                    token: None,
                },
            )
            .await
            .unwrap()
            .items;
        let primary = zones
            .iter()
            .find(|zone| zone.zone.apex.as_str() == "example.test")
            .unwrap()
            .clone();
        let secondary = zones
            .iter()
            .find(|zone| zone.zone.apex.as_str() == "example.net")
            .unwrap()
            .clone();
        let records = adapter
            .list_record_sets(
                &primary.zone,
                &DnsPageRequest {
                    limit: 10,
                    token: None,
                },
            )
            .await
            .unwrap()
            .items;
        let create_record = txt_record("create.example.test", "first");
        let replacement_record = txt_record("create.example.test", "replacement");
        let fixture = DnsAdapterConformanceFixture {
            provider: CloudProvider::Cloudflare,
            provider_account_id: center_account,
            other_account_id: CloudResourceId::new("cloudflare-other").unwrap(),
            primary_zone: primary,
            secondary_zone: secondary,
            primary_records: records,
            create_record,
            replacement_record,
            maximum_guard: DnsGuardStrength::BestEffort,
        };
        assert_dns_provider_conformance(&adapter, &fixture).await;
    }

    #[tokio::test]
    async fn lifecycle_observation_and_dnssec_receipts_are_conservative() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let adapter = write_adapter(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            [9; 32],
        )
        .unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account.clone(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let observation = ZoneLifecycleProvider::observe_zone(&adapter, &zone)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observation.delegation.state, DelegationState::NotChecked);
        assert_eq!(
            observation.authoritative_verification,
            AuthoritativeDnsVerification::NotChecked
        );
        assert_eq!(
            observation.readiness,
            ZoneReadiness::AwaitingAuthoritativeVerification
        );
        assert_eq!(observation.non_default_record_count, 3);
        assert_eq!(observation.dnssec.state, DnssecProviderState::Unsupported);

        let receipt = adapter
            .set_dnssec(&zone, DnssecDesiredState::Enabled, &observation.revision)
            .await
            .unwrap();
        assert_eq!(receipt.state, ZoneLifecycleMutationState::Pending);
        assert_eq!(
            adapter
                .observe_mutation(&receipt.mutation_id)
                .await
                .unwrap()
                .state,
            ZoneLifecycleMutationState::Succeeded
        );
        let enabled = ZoneLifecycleProvider::observe_zone(&adapter, &zone)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(enabled.dnssec.state, DnssecProviderState::Active);
        assert!(matches!(
            enabled.dnssec.external_action,
            DnssecExternalAction::PublishDs { .. }
        ));
        let disable_error = adapter
            .set_dnssec(&zone, DnssecDesiredState::Disabled, &enabled.revision)
            .await
            .unwrap_err();
        assert_eq!(disable_error.category(), ProviderErrorCategory::Conflict);
        assert_eq!(
            disable_error.code(),
            "cloudflare_parent_ds_removal_verification_required"
        );

        let created = adapter
            .create_zone(&ZoneCreationRequest {
                provider_account_id: center_account,
                provider: CloudProvider::Cloudflare,
                apex: AbsoluteDnsName::new("created.example").unwrap(),
                visibility: ZoneVisibility::Public,
                idempotency_key: IdempotencyKey::new("create-zone-1").unwrap(),
            })
            .await
            .unwrap();
        assert_eq!(created.state, ZoneLifecycleMutationState::Pending);
        assert_eq!(
            adapter
                .observe_mutation(&created.mutation_id)
                .await
                .unwrap()
                .state,
            ZoneLifecycleMutationState::Succeeded
        );
    }

    #[tokio::test]
    async fn sync_writer_creates_public_full_zone_with_one_provider_call() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let writer =
            CloudflareDnsSyncWriter::new(center_account.clone(), &account(), api.clone()).unwrap();
        let apex = AbsoluteDnsName::new("created.example").unwrap();

        let observed = writer.create_zone(&apex).await.unwrap();

        assert_eq!(observed.zone.provider_account_id, center_account);
        assert_eq!(observed.zone.provider, CloudProvider::Cloudflare);
        assert_eq!(observed.zone.apex, apex);
        assert_eq!(observed.zone.visibility, ZoneVisibility::Public);
        assert_eq!(observed.kind, CloudflareZoneKind::Full);
        assert_eq!(observed.status, CloudflareZoneStatus::Pending);
        assert_eq!(observed.name_servers, fake_nameservers());
        assert_eq!(api.total_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn sync_writer_rejects_invalid_accounts_without_provider_io() {
        let api = Arc::new(fake_api());
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let wrong_provider = ProviderAccountSpec {
            provider: CloudProvider::Aws,
            scope: Some(ProviderAccountScope::Aws {
                account_id: "123456789012".to_string(),
            }),
            credential_source: CredentialSource::Ambient,
        };
        assert!(
            CloudflareDnsSyncWriter::new(center_account.clone(), &wrong_provider, api.clone())
                .is_err()
        );

        let mut invalid_native_account = account();
        invalid_native_account.scope = Some(ProviderAccountScope::Cloudflare {
            account_id: "not-a-cloudflare-account".to_string(),
        });
        assert!(
            CloudflareDnsSyncWriter::new(center_account, &invalid_native_account, api.clone())
                .is_err()
        );
        assert_eq!(api.total_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn sync_writer_treats_provider_response_mismatches_as_unknown_outcomes() {
        let apex = AbsoluteDnsName::new("created.example").unwrap();
        let valid = CloudflareZone {
            id: "dddddddddddddddddddddddddddddddd".to_string(),
            account_id: "0123456789abcdef0123456789abcdef".to_string(),
            name: apex.as_str().to_string(),
            kind: CloudflareZoneKind::Full,
            status: CloudflareZoneStatus::Pending,
            name_servers: fake_nameservers(),
            modified_on: Some("created-revision".to_string()),
        };
        let mut mismatches = Vec::new();
        let mut account_mismatch = valid.clone();
        account_mismatch.account_id = "fedcba9876543210fedcba9876543210".to_string();
        mismatches.push(account_mismatch);
        let mut name_mismatch = valid.clone();
        name_mismatch.name = "other.example".to_string();
        mismatches.push(name_mismatch);
        let mut kind_mismatch = valid.clone();
        kind_mismatch.kind = CloudflareZoneKind::Partial;
        mismatches.push(kind_mismatch);
        let mut id_mismatch = valid.clone();
        id_mismatch.id = "invalid-id".to_string();
        mismatches.push(id_mismatch);
        let mut nameservers_mismatch = valid;
        nameservers_mismatch.name_servers.clear();
        mismatches.push(nameservers_mismatch);

        for mismatch in mismatches {
            let api = Arc::new(fake_api());
            *api.create_zone_override.lock().unwrap() = Some(mismatch);
            let writer = CloudflareDnsSyncWriter::new(
                CloudResourceId::new("cloudflare-main").unwrap(),
                &account(),
                api.clone(),
            )
            .unwrap();

            let error = writer.create_zone(&apex).await.unwrap_err();

            assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
            assert_eq!(error.code(), "cloudflare_create_zone_result_mismatch");
            assert_eq!(api.total_calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn sync_writer_observes_exact_zone_and_rrset_without_mutation() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let writer =
            CloudflareDnsSyncWriter::new(center_account.clone(), &account(), api.clone()).unwrap();

        let zone = writer
            .observe_zone(&DnsZoneId::new(ZONE_A_ID).unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(zone.zone.provider_account_id, center_account);
        assert_eq!(zone.zone.apex.as_str(), "example.test");
        let key = txt_record("txt.example.test", "ignored").key;
        let record = writer
            .observe_record_set(&zone.zone, &key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            record.record_set.values,
            BTreeSet::from([txt_value("seed")])
        );
        assert_eq!(*api.batch_record_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn sync_writer_deletes_one_exact_safe_zone_with_one_mutation() {
        let api = Arc::new(fake_api());
        let writer = CloudflareDnsSyncWriter::new(
            CloudResourceId::new("cloudflare-main").unwrap(),
            &account(),
            api.clone(),
        )
        .unwrap();
        let zone_id = DnsZoneId::new(ZONE_B_ID).unwrap();
        let observed = writer.observe_zone(&zone_id).await.unwrap().unwrap();
        let revision = observed.revision.unwrap();

        let guard = writer
            .preflight_zone_delete(
                &zone_id,
                &AbsoluteDnsName::new("example.net").unwrap(),
                &revision,
            )
            .await
            .unwrap();
        writer.delete_zone(guard).await.unwrap();

        assert_eq!(*api.delete_zone_calls.lock().unwrap(), 1);
        assert!(!api
            .zones
            .lock()
            .unwrap()
            .iter()
            .any(|zone| zone.id == ZONE_B_ID));
    }

    #[tokio::test]
    async fn sync_writer_zone_delete_guard_cannot_cross_writer_instances() {
        let api = Arc::new(fake_api());
        let first = CloudflareDnsSyncWriter::new(
            CloudResourceId::new("cloudflare-main").unwrap(),
            &account(),
            api.clone(),
        )
        .unwrap();
        let second = CloudflareDnsSyncWriter::new(
            CloudResourceId::new("cloudflare-main").unwrap(),
            &account(),
            api.clone(),
        )
        .unwrap();
        let guard = first
            .preflight_zone_delete(
                &DnsZoneId::new(ZONE_B_ID).unwrap(),
                &AbsoluteDnsName::new("example.net").unwrap(),
                &DnsRecordRevision::new("zone-b-revision").unwrap(),
            )
            .await
            .unwrap();

        let error = second.delete_zone(guard).await.unwrap_err();

        assert_eq!(error.category(), ProviderErrorCategory::Validation);
        assert_eq!(error.code(), "cloudflare_zone_delete_guard_scope_mismatch");
        assert_eq!(*api.delete_zone_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn sync_writer_zone_delete_guards_fail_before_mutation() {
        let cases = [
            ("wrong.example", "zone-b-revision"),
            ("example.net", "stale-zone-revision"),
        ];
        for (apex, revision) in cases {
            let api = Arc::new(fake_api());
            let writer = CloudflareDnsSyncWriter::new(
                CloudResourceId::new("cloudflare-main").unwrap(),
                &account(),
                api.clone(),
            )
            .unwrap();

            let error = writer
                .preflight_zone_delete(
                    &DnsZoneId::new(ZONE_B_ID).unwrap(),
                    &AbsoluteDnsName::new(apex).unwrap(),
                    &DnsRecordRevision::new(revision).unwrap(),
                )
                .await
                .unwrap_err();

            assert_eq!(error.category(), ProviderErrorCategory::Conflict);
            assert_eq!(*api.delete_zone_calls.lock().unwrap(), 0);
        }

        let api = Arc::new(fake_api());
        let writer = CloudflareDnsSyncWriter::new(
            CloudResourceId::new("cloudflare-main").unwrap(),
            &account(),
            api.clone(),
        )
        .unwrap();
        let error = writer
            .preflight_zone_delete(
                &DnsZoneId::new(ZONE_A_ID).unwrap(),
                &AbsoluteDnsName::new("example.test").unwrap(),
                &DnsRecordRevision::new("zone-a-revision").unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Conflict);
        assert_eq!(error.code(), "cloudflare_zone_not_empty");
        assert_eq!(*api.delete_zone_calls.lock().unwrap(), 0);

        let api = Arc::new(fake_api());
        api.records.lock().unwrap().insert(
            ZONE_B_ID.to_string(),
            vec![CloudflareRecord {
                id: RECORD_A_ID.to_string(),
                name: "unexpected.example.net".to_string(),
                ttl: 300,
                value: DnsRecordSetValue::Soa {
                    primary_name_server: AbsoluteDnsName::new("ns.example.net").unwrap(),
                    responsible_mailbox: AbsoluteDnsName::new("hostmaster.example.net").unwrap(),
                    serial: 1,
                    refresh: 7_200,
                    retry: 900,
                    expire: 1_209_600,
                    minimum: 86_400,
                },
                proxied: None,
                proxiable: false,
                flatten_cname: None,
                ipv4_only: false,
                ipv6_only: false,
                private_routing: false,
                comment: None,
                tags: BTreeSet::new(),
                modified_on: Some("unexpected-soa-revision".to_string()),
            }],
        );
        let writer = CloudflareDnsSyncWriter::new(
            CloudResourceId::new("cloudflare-main").unwrap(),
            &account(),
            api.clone(),
        )
        .unwrap();
        let error = writer
            .preflight_zone_delete(
                &DnsZoneId::new(ZONE_B_ID).unwrap(),
                &AbsoluteDnsName::new("example.net").unwrap(),
                &DnsRecordRevision::new("zone-b-revision").unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "cloudflare_zone_not_empty");
        assert_eq!(*api.delete_zone_calls.lock().unwrap(), 0);

        let api = Arc::new(fake_api());
        api.dnssec.lock().unwrap().insert(
            ZONE_B_ID.to_string(),
            CloudflareDnssec {
                status: CloudflareDnssecStatus::Pending,
                ds: None,
                modified_on: Some("dnssec-pending".to_string()),
            },
        );
        let writer = CloudflareDnsSyncWriter::new(
            CloudResourceId::new("cloudflare-main").unwrap(),
            &account(),
            api.clone(),
        )
        .unwrap();
        let error = writer
            .preflight_zone_delete(
                &DnsZoneId::new(ZONE_B_ID).unwrap(),
                &AbsoluteDnsName::new("example.net").unwrap(),
                &DnsRecordRevision::new("zone-b-revision").unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Conflict);
        assert_eq!(error.code(), "cloudflare_zone_dnssec_not_disabled");
        assert_eq!(*api.delete_zone_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn sync_writer_zone_delete_ack_mismatch_is_unknown_without_retry() {
        let api = Arc::new(fake_api());
        *api.delete_zone_ack_override.lock().unwrap() = Some(ZONE_A_ID.to_string());
        let writer = CloudflareDnsSyncWriter::new(
            CloudResourceId::new("cloudflare-main").unwrap(),
            &account(),
            api.clone(),
        )
        .unwrap();

        let guard = writer
            .preflight_zone_delete(
                &DnsZoneId::new(ZONE_B_ID).unwrap(),
                &AbsoluteDnsName::new("example.net").unwrap(),
                &DnsRecordRevision::new("zone-b-revision").unwrap(),
            )
            .await
            .unwrap();
        let error = writer.delete_zone(guard).await.unwrap_err();

        assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
        assert_eq!(error.code(), "cloudflare_delete_zone_result_mismatch");
        assert_eq!(*api.delete_zone_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn sync_writer_dispatches_one_batch_and_returns_fresh_create_replace_delete_results() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let writer = CloudflareDnsSyncWriter::new(center_account, &account(), api.clone()).unwrap();
        let zone = writer
            .observe_zone(&DnsZoneId::new(ZONE_A_ID).unwrap())
            .await
            .unwrap()
            .unwrap()
            .zone;
        let desired = txt_record("new.example.test", "created");

        let created = writer
            .apply_record_change(
                &zone,
                &DnsRecordChange::Create {
                    record_set: desired.clone(),
                    guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                },
            )
            .await
            .unwrap();
        let CloudflareDnsSyncRecordOutcome::Present(created) = created else {
            panic!("create must return a present RRset")
        };
        assert_eq!(created.record_set, desired);
        assert_eq!(*api.batch_record_calls.lock().unwrap(), 1);

        let replacement = txt_record("new.example.test", "replaced");
        let replaced = writer
            .apply_record_change(
                &zone,
                &DnsRecordChange::Replace {
                    previous: created.as_ref().clone(),
                    desired: replacement.clone(),
                    guard: edgion_center_core::DnsMutationGuard::MatchObserved {
                        revision: created.revision.clone(),
                    },
                },
            )
            .await
            .unwrap();
        let CloudflareDnsSyncRecordOutcome::Present(replaced) = replaced else {
            panic!("replace must return a present RRset")
        };
        assert_eq!(replaced.record_set, replacement);
        assert_eq!(*api.batch_record_calls.lock().unwrap(), 2);

        let deleted = writer
            .apply_record_change(
                &zone,
                &DnsRecordChange::Delete {
                    previous: replaced.as_ref().clone(),
                    guard: edgion_center_core::DnsMutationGuard::MatchObserved {
                        revision: replaced.revision,
                    },
                },
            )
            .await
            .unwrap();
        assert_eq!(deleted, CloudflareDnsSyncRecordOutcome::Deleted);
        assert_eq!(*api.batch_record_calls.lock().unwrap(), 3);
    }

    #[tokio::test]
    async fn sync_writer_guard_conflicts_never_dispatch_a_mutation() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let writer = CloudflareDnsSyncWriter::new(center_account, &account(), api.clone()).unwrap();
        let zone = writer
            .observe_zone(&DnsZoneId::new(ZONE_A_ID).unwrap())
            .await
            .unwrap()
            .unwrap()
            .zone;
        let existing = writer
            .observe_record_set(&zone, &txt_record("txt.example.test", "ignored").key)
            .await
            .unwrap()
            .unwrap();

        let create_error = writer
            .apply_record_change(
                &zone,
                &DnsRecordChange::Create {
                    record_set: txt_record("txt.example.test", "duplicate"),
                    guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(create_error.category(), ProviderErrorCategory::Conflict);

        let stale_revision = DnsRecordRevision::new("sha256:stale").unwrap();
        let mut stale = existing;
        stale.revision = stale_revision.clone();
        let replace_error = writer
            .apply_record_change(
                &zone,
                &DnsRecordChange::Replace {
                    previous: stale,
                    desired: txt_record("txt.example.test", "replacement"),
                    guard: edgion_center_core::DnsMutationGuard::MatchObserved {
                        revision: stale_revision,
                    },
                },
            )
            .await
            .unwrap_err();
        assert_eq!(replace_error.category(), ProviderErrorCategory::Conflict);
        assert_eq!(*api.batch_record_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn sync_writer_maps_batch_response_mismatch_to_unknown_outcome() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        *api.batch_result_override.lock().unwrap() = Some(CloudflareBatchResult {
            deletes: Vec::new(),
            posts: Vec::new(),
        });
        let writer = CloudflareDnsSyncWriter::new(center_account, &account(), api.clone()).unwrap();
        let zone = writer
            .observe_zone(&DnsZoneId::new(ZONE_A_ID).unwrap())
            .await
            .unwrap()
            .unwrap()
            .zone;

        let error = writer
            .apply_record_change(
                &zone,
                &DnsRecordChange::Create {
                    record_set: txt_record("new.example.test", "value"),
                    guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                },
            )
            .await
            .unwrap_err();

        assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
        assert_eq!(*api.batch_record_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn sync_writer_rejects_zone_scope_and_reserved_record_types_before_mutation() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let writer = CloudflareDnsSyncWriter::new(center_account, &account(), api.clone()).unwrap();
        let mut zone = writer
            .observe_zone(&DnsZoneId::new(ZONE_A_ID).unwrap())
            .await
            .unwrap()
            .unwrap()
            .zone;
        zone.visibility = ZoneVisibility::Private;
        let error = writer
            .apply_record_change(
                &zone,
                &DnsRecordChange::Create {
                    record_set: txt_record("new.example.test", "value"),
                    guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Validation);

        zone.visibility = ZoneVisibility::Public;
        let reserved = ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("example.test").unwrap(),
                record_type: ProviderDnsRecordType::Ns,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: BTreeSet::from([DnsRecordSetValue::Ns {
                target: AbsoluteDnsName::new("ns.example.net").unwrap(),
            }]),
            extension: None,
        };
        let error = writer
            .apply_record_change(
                &zone,
                &DnsRecordChange::Create {
                    record_set: reserved,
                    guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Validation);
        assert_eq!(*api.batch_record_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn modified_cursor_fails_authentication() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            CloudflareCursorKey::new([9; 32]).unwrap(),
        )
        .unwrap();
        let page = adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(page.next.unwrap().as_str()).unwrap();
        bytes[0] ^= 1;
        let tampered = DnsPageToken::new(URL_SAFE_NO_PAD.encode(bytes)).unwrap();
        assert!(adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(tampered),
                },
            )
            .await
            .is_err());
        assert_eq!(*api.list_zone_calls.lock().unwrap(), 1);
    }

    #[test]
    fn cursor_key_ring_rejects_unsafe_lifetimes_and_duplicate_material() {
        let key = || CloudflareCursorKey::new([31; 32]).unwrap();
        assert_eq!(
            CloudflareCursorKeyRing::new(
                key(),
                Some(key()),
                Duration::from_secs(900),
                Duration::from_secs(30),
            )
            .unwrap_err()
            .code(),
            "duplicate_cloudflare_cursor_key"
        );
        for (ttl, skew) in [(0, 0), (3_601, 30), (300, 300), (900, 301)] {
            assert_eq!(
                CloudflareCursorKeyRing::new(
                    key(),
                    None,
                    Duration::from_secs(ttl),
                    Duration::from_secs(skew),
                )
                .unwrap_err()
                .code(),
                "invalid_cloudflare_cursor_lifetime"
            );
        }
        assert!(CloudflareCursorKeyRing::new(
            key(),
            None,
            Duration::from_millis(900_001),
            Duration::from_secs(30),
        )
        .is_err());
    }

    #[tokio::test]
    async fn promoted_key_accepts_fallback_cursor_and_reissues_with_active() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let mut third = api.zones.lock().unwrap()[0].clone();
        third.id = "dddddddddddddddddddddddddddddddd".to_string();
        third.name = "third.example".to_string();
        api.zones.lock().unwrap().push(third);
        let old_clock = Arc::new(FixedCursorClock::new(1_000));
        let old = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            cursor_ring([41; 32], None, old_clock),
        )
        .unwrap();
        let first = old
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        let old_cursor = first.next.unwrap();

        let promoted_clock = Arc::new(FixedCursorClock::new(1_010));
        let promoted = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            cursor_ring([42; 32], Some([41; 32]), promoted_clock.clone()),
        )
        .unwrap();
        let second = promoted
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(old_cursor.clone()),
                },
            )
            .await
            .unwrap();
        let active_cursor = second.next.unwrap();

        let active_only = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            cursor_ring([42; 32], None, promoted_clock),
        )
        .unwrap();
        assert!(active_only
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(active_cursor),
                },
            )
            .await
            .is_ok());

        let calls_before_rejection = *api.list_zone_calls.lock().unwrap();
        let error = active_only
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(old_cursor),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "invalid_cloudflare_cursor_signature");
        assert_eq!(*api.list_zone_calls.lock().unwrap(), calls_before_rejection);
    }

    #[tokio::test]
    async fn mutation_rotation_reseals_with_active_and_domains_do_not_cross() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account.clone(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let old = CloudflareDnsAdapter::new_with_cursor_and_mutation_key_rings(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            cursor_ring([45; 32], None, Arc::new(FixedCursorClock::new(1_000))),
            CloudflareMutationTokenKeyRing::new(
                CloudflareMutationTokenKey::new([55; 32]).unwrap(),
                None,
            )
            .unwrap(),
        )
        .unwrap();
        let change = old
            .build_receipt(
                &zone,
                &CloudflareBatchRequest {
                    deletes: Vec::new(),
                    patches: Vec::new(),
                    puts: Vec::new(),
                    posts: Vec::new(),
                },
            )
            .unwrap();
        let mutation = old
            .lifecycle_receipt(
                LifecycleMutation::Delete {
                    zone_id: zone.zone_id.as_str().to_owned(),
                },
                ZoneLifecycleMutationState::Pending,
            )
            .unwrap();

        let api = Arc::new(fake_api());
        let promoted = CloudflareDnsAdapter::new_with_cursor_and_mutation_key_rings(
            center_account.clone(),
            &account(),
            api.clone(),
            cursor_ring(
                [46; 32],
                Some([45; 32]),
                Arc::new(FixedCursorClock::new(1_010)),
            ),
            CloudflareMutationTokenKeyRing::new(
                CloudflareMutationTokenKey::new([56; 32]).unwrap(),
                Some(CloudflareMutationTokenKey::new([55; 32]).unwrap()),
            )
            .unwrap(),
        )
        .unwrap();
        let resealed_change = promoted.observe_receipt(&zone, &change.id).unwrap();
        assert_ne!(resealed_change.id, change.id);
        let resealed_mutation = promoted
            .observe_mutation(&mutation.mutation_id)
            .await
            .unwrap();
        assert_ne!(resealed_mutation.mutation_id, mutation.mutation_id);

        let active_only = CloudflareDnsAdapter::new_with_cursor_and_mutation_key_rings(
            center_account,
            &account(),
            api.clone(),
            cursor_ring([46; 32], None, Arc::new(FixedCursorClock::new(1_010))),
            CloudflareMutationTokenKeyRing::new(
                CloudflareMutationTokenKey::new([56; 32]).unwrap(),
                None,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(active_only
            .observe_receipt(&zone, &resealed_change.id)
            .is_ok());
        assert!(active_only
            .observe_mutation(&resealed_mutation.mutation_id)
            .await
            .is_ok());

        let calls = *api.get_zone_calls.lock().unwrap();
        let cross_lifecycle = ZoneLifecycleMutationId::new(change.id.as_str()).unwrap();
        assert_eq!(
            promoted
                .observe_mutation(&cross_lifecycle)
                .await
                .unwrap_err()
                .code(),
            "cloudflare_lifecycle_mutation_not_found"
        );
        let cross_change = DnsChangeId::new(mutation.mutation_id.as_str()).unwrap();
        assert_eq!(
            promoted
                .observe_receipt(&zone, &cross_change)
                .unwrap_err()
                .code(),
            "cloudflare_change_not_found"
        );
        assert_eq!(*api.get_zone_calls.lock().unwrap(), calls);
    }

    #[test]
    fn mutation_authority_rejects_duplicate_and_cursor_reused_material() {
        assert_eq!(
            CloudflareMutationTokenKeyRing::new(
                CloudflareMutationTokenKey::new([60; 32]).unwrap(),
                Some(CloudflareMutationTokenKey::new([60; 32]).unwrap()),
            )
            .unwrap_err()
            .code(),
            "duplicate_cloudflare_mutation_token_key"
        );
        assert!(CloudflareMutationTokenKey::new([0; 32]).is_err());
        let error = CloudflareDnsAdapter::new_with_cursor_and_mutation_key_rings(
            CloudResourceId::new("cloudflare-main").unwrap(),
            &account(),
            Arc::new(fake_api()),
            CloudflareCursorKey::new([61; 32]).unwrap().into(),
            CloudflareMutationTokenKeyRing::new(
                CloudflareMutationTokenKey::new([62; 32]).unwrap(),
                Some(CloudflareMutationTokenKey::new([61; 32]).unwrap()),
            )
            .unwrap(),
        )
        .err()
        .unwrap();
        assert_eq!(error.code(), "cloudflare_token_key_material_reused");
        let debug = format!(
            "{:?}",
            CloudflareMutationTokenKeyRing::new(
                CloudflareMutationTokenKey::new([63; 32]).unwrap(),
                None,
            )
            .unwrap()
        );
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("63"));
    }

    #[test]
    fn compact_lifecycle_tokens_fit_maximum_boundaries_and_preflight() {
        let center_account = CloudResourceId::new("x".repeat(512)).unwrap();
        let adapter = write_adapter(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            [66; 32],
        )
        .unwrap();
        let apex = AbsoluteDnsName::new(format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61),
        ))
        .unwrap();
        let mutations = [
            LifecycleMutation::Create {
                zone_id: ZONE_A_ID.to_string(),
                apex,
                request_scope: mutation_scope_tag(
                    &adapter.mutation_token_keys().unwrap().active,
                    b"create-idempotency",
                    b"maximum-idempotency-scope",
                ),
            },
            LifecycleMutation::Delete {
                zone_id: ZONE_A_ID.to_string(),
            },
            LifecycleMutation::Dnssec {
                zone_id: ZONE_A_ID.to_string(),
                desired: DnssecDesiredState::Enabled,
            },
        ];
        for mutation in mutations {
            adapter.preflight_lifecycle_receipt(&mutation).unwrap();
            let receipt = adapter
                .lifecycle_receipt(mutation, ZoneLifecycleMutationState::Pending)
                .unwrap();
            assert!(receipt.mutation_id.as_str().len() <= 1_024);
            let authenticated = URL_SAFE_NO_PAD
                .decode(receipt.mutation_id.as_str())
                .unwrap();
            let payload = std::str::from_utf8(&authenticated[..authenticated.len() - 32]).unwrap();
            assert!(!payload.contains(center_account.as_str()));
            assert!(!payload.contains("0123456789abcdef0123456789abcdef"));
            assert!(!payload.contains("provider_account_id"));
        }
    }

    #[tokio::test]
    async fn read_only_adapter_rejects_every_write_and_observe_before_provider_io() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            CloudflareCursorKey::new([64; 32]).unwrap(),
        )
        .unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account.clone(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let revision = ZoneLifecycleRevision::new("revision-1").unwrap();
        let plan = edgion_center_core::ZoneDeletionPlan {
            zone: zone.clone(),
            observed_revision: revision.clone(),
            origin: edgion_center_core::ZoneOrigin::CenterCreated,
            management_policy: edgion_center_core::ManagementPolicy::Managed,
            deletion_policy: edgion_center_core::DeletionPolicy::DeleteExternal,
            non_default_record_count: 0,
            delegation_state: DelegationState::NotApplicable,
            dnssec_state: DnssecProviderState::Unsupported,
            blockers: BTreeSet::new(),
        };
        let deletion = edgion_center_core::authorize_zone_deletion(
            &plan,
            edgion_center_core::ZoneDeletionApproval {
                approved_revision: revision.clone(),
                approved_zone: zone.clone(),
                approved_by: "operator".to_string(),
                approved_at: "2026-07-20T00:00:00Z".to_string(),
                acknowledgements: BTreeSet::new(),
            },
        )
        .unwrap();
        let expected_code = "cloudflare_mutation_token_authority_required";
        assert_eq!(
            adapter
                .apply_record_changes(
                    &zone,
                    &[DnsRecordChange::Create {
                        record_set: txt_record("new.example.test", "value"),
                        guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                    }],
                    DnsGuardStrength::BestEffort,
                )
                .await
                .unwrap_err()
                .code(),
            expected_code
        );
        assert_eq!(
            adapter
                .create_zone(&ZoneCreationRequest {
                    provider_account_id: center_account,
                    provider: CloudProvider::Cloudflare,
                    apex: AbsoluteDnsName::new("created.example").unwrap(),
                    visibility: ZoneVisibility::Public,
                    idempotency_key: IdempotencyKey::new("create-zone-read-only").unwrap(),
                })
                .await
                .unwrap_err()
                .code(),
            expected_code
        );
        assert_eq!(
            adapter
                .set_dnssec(&zone, DnssecDesiredState::Enabled, &revision)
                .await
                .unwrap_err()
                .code(),
            expected_code
        );
        assert_eq!(
            adapter.delete_zone(&deletion).await.unwrap_err().code(),
            expected_code
        );
        assert_eq!(
            adapter
                .observe_change(&zone, &DnsChangeId::new("opaque-change").unwrap())
                .await
                .unwrap_err()
                .code(),
            expected_code
        );
        assert_eq!(
            adapter
                .observe_mutation(
                    &ZoneLifecycleMutationId::new("opaque-lifecycle-mutation").unwrap(),
                )
                .await
                .unwrap_err()
                .code(),
            expected_code
        );
        assert_eq!(*api.get_zone_calls.lock().unwrap(), 0);
        assert_eq!(*api.list_zone_calls.lock().unwrap(), 0);
        assert_eq!(api.total_calls.load(Ordering::SeqCst), 0);
        assert_eq!(api.zones.lock().unwrap().len(), 3);
        assert!(api.dnssec.lock().unwrap().is_empty());
        assert_eq!(api.records.lock().unwrap()[ZONE_A_ID].len(), 3);
    }

    #[test]
    fn legacy_and_oversized_mutation_tokens_fail_closed() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let adapter = write_adapter(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            [65; 32],
        )
        .unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let legacy_body = serde_json::to_vec(&serde_json::json!({
            "v": 1,
            "c": scope_hash("cloudflare-main"),
            "a": scope_hash("0123456789abcdef0123456789abcdef"),
            "z": scope_hash(ZONE_A_ID),
            "r": "legacy",
            "g": "best_effort",
        }))
        .unwrap();
        let cursor_key = CloudflareCursorKey::new([65; 32]).unwrap();
        let signature = HmacSha256::new_from_slice(cursor_key.0.as_ref())
            .unwrap()
            .chain_update(&legacy_body)
            .finalize()
            .into_bytes();
        let mut legacy = legacy_body;
        legacy.extend_from_slice(&signature);
        let legacy = DnsChangeId::new(URL_SAFE_NO_PAD.encode(legacy)).unwrap();
        assert_eq!(
            adapter.observe_receipt(&zone, &legacy).unwrap_err().code(),
            "cloudflare_change_not_found"
        );
        let oversized = "a".repeat(513);
        let keys = adapter.mutation_token_keys().unwrap();
        assert_eq!(
            verify_mutation_token::<ChangeToken>(&oversized, keys, MutationTokenDomain::DnsChange,)
                .err()
                .unwrap()
                .code(),
            "invalid_cloudflare_token"
        );
    }

    #[tokio::test]
    async fn expired_future_and_v3_cursors_fail_before_provider_io() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let clock = Arc::new(FixedCursorClock::new(1_000));
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            cursor_ring([43; 32], None, clock.clone()),
        )
        .unwrap();
        let first = adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        let cursor = first.next.unwrap();
        let provider_calls = *api.list_zone_calls.lock().unwrap();

        clock.set(1_931);
        let expired = adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(cursor.clone()),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(expired.code(), "cloudflare_cursor_expired");
        assert_eq!(*api.list_zone_calls.lock().unwrap(), provider_calls);

        clock.set(969);
        let future = adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(cursor),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(future.code(), "cloudflare_cursor_not_yet_valid");
        assert_eq!(*api.list_zone_calls.lock().unwrap(), provider_calls);

        let key = CloudflareCursorKey::new([43; 32]).unwrap();
        let legacy = serde_json::to_vec(&serde_json::json!({
            "version": 3,
            "scope": CursorScope::DnsZones {
                center_scope: cursor_scope_tag(&key, b"center-account", center_account.as_str()),
                external_scope: cursor_scope_tag(
                    &key,
                    b"native-account",
                    "0123456789abcdef0123456789abcdef",
                ),
            },
            "offset": 1,
            "limit": 1,
            "inventory_tag": "legacy",
        }))
        .unwrap();
        let signature = HmacSha256::new_from_slice(key.0.as_ref())
            .unwrap()
            .chain_update(&legacy)
            .finalize()
            .into_bytes();
        let mut authenticated = legacy;
        authenticated.extend_from_slice(&signature);
        let legacy = DnsPageToken::new(URL_SAFE_NO_PAD.encode(authenticated)).unwrap();
        clock.set(1_000);
        let unsupported = adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(legacy),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(unsupported.code(), "unsupported_cloudflare_cursor_version");
        assert_eq!(*api.list_zone_calls.lock().unwrap(), provider_calls);
    }

    #[tokio::test]
    async fn validated_record_cursor_uses_one_captured_time() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let clock = Arc::new(FixedCursorClock::new(1_000));
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api,
            cursor_ring([44; 32], None, clock.clone()),
        )
        .unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let first = adapter
            .list_record_sets(
                &zone,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        let request = DnsPageRequest {
            limit: 1,
            token: first.next,
        };
        clock.set(1_930);
        let validated = adapter
            .validate_record_inventory_cursor(&zone.zone_id, &request)
            .unwrap();
        clock.set(10_000);
        let second = adapter
            .list_record_sets_with_validated_cursor(&zone, &request, validated)
            .await
            .unwrap();
        assert_eq!(second.items.len(), 1);
        let next: Cursor = {
            let authenticated = URL_SAFE_NO_PAD
                .decode(second.next.unwrap().as_str())
                .unwrap();
            serde_json::from_slice(&authenticated[..authenticated.len() - 32]).unwrap()
        };
        assert_eq!(next.issued_at, 1_930);
        assert_eq!(next.expires_at, 2_830);
    }

    #[tokio::test]
    async fn validated_record_cursor_rejects_cross_adapter_proofs_before_provider_io() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account.clone(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let clock = Arc::new(FixedCursorClock::new(1_000));
        let issuer = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            cursor_ring([51; 32], None, clock.clone()),
        )
        .unwrap();
        let first = issuer
            .list_record_sets(
                &zone,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        let request = DnsPageRequest {
            limit: 1,
            token: first.next,
        };
        let verifier = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            cursor_ring([52; 32], Some([51; 32]), clock.clone()),
        )
        .unwrap();

        for (active, fallback) in [
            ([52; 32], None),
            ([52; 32], Some([53; 32])),
            ([54; 32], Some([51; 32])),
        ] {
            let validated = verifier
                .validate_record_inventory_cursor(&zone.zone_id, &request)
                .unwrap();
            let target_api = Arc::new(fake_api());
            let target = CloudflareDnsAdapter::new(
                center_account.clone(),
                &account(),
                target_api.clone(),
                cursor_ring(active, fallback, clock.clone()),
            )
            .unwrap();
            let error = target
                .list_record_sets_with_validated_cursor(&zone, &request, validated)
                .await
                .unwrap_err();
            assert_eq!(error.code(), "cloudflare_cursor_scope_mismatch");
            assert_eq!(*target_api.list_record_calls.lock().unwrap(), 0);
        }
    }

    #[test]
    fn cursor_time_accepts_exact_skew_boundaries_and_rejects_invalid_arithmetic() {
        let keys = cursor_ring([55; 32], None, Arc::new(FixedCursorClock::new(1_000)));
        let cursor = |issued_at, expires_at| Cursor {
            version: 4,
            scope: CursorScope::DnsZones {
                center_scope: "center".to_string(),
                external_scope: "external".to_string(),
            },
            offset: 1,
            limit: 1,
            inventory_tag: "inventory".to_string(),
            issued_at,
            expires_at,
        };

        assert!(validate_cursor_time(&cursor(1_030, 1_930), &keys, 1_000).is_ok());
        assert!(validate_cursor_time(&cursor(1_000, 1_900), &keys, 1_930).is_ok());

        for (value, now) in [
            (cursor(1_001, 1_000), 1_000),
            (cursor(1_000, 1_901), 1_000),
            (cursor(u64::MAX - 1, u64::MAX), u64::MAX - 30),
            (cursor(1, 2), u64::MAX - 29),
        ] {
            assert_eq!(
                validate_cursor_time(&value, &keys, now).unwrap_err().code(),
                "invalid_cloudflare_cursor_time"
            );
        }
    }

    #[tokio::test]
    async fn excessive_zone_page_count_fails_after_one_provider_call() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let mut fake = fake_api();
        fake.zone_total_pages = MAX_ZONE_PROVIDER_PAGES + 1;
        let api = Arc::new(fake);
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            CloudflareCursorKey::new([9; 32]).unwrap(),
        )
        .unwrap();
        let error = adapter
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "cloudflare_zone_pagination_limit");
        assert_eq!(*api.list_zone_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn inventory_cursors_bind_keyed_scopes_without_disclosing_identifiers() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let native_account = "0123456789abcdef0123456789abcdef";
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            CloudflareCursorKey::new([8; 32]).unwrap(),
        )
        .unwrap();

        let zone_page = adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        let zone_token = zone_page.next.unwrap();
        let zone_bytes = URL_SAFE_NO_PAD.decode(zone_token.as_str()).unwrap();
        let zone_payload = std::str::from_utf8(&zone_bytes[..zone_bytes.len() - 32]).unwrap();
        assert!(!zone_payload.contains(center_account.as_str()));
        assert!(!zone_payload.contains(native_account));
        assert!(!zone_payload.contains(ZONE_A_ID));
        assert!(!zone_payload.contains(ZONE_B_ID));
        assert!(!zone_payload.contains(&scope_hash(center_account.as_str())));
        assert!(!zone_payload.contains(&scope_hash(native_account)));
        let zone_cursor: Cursor = serde_json::from_str(zone_payload).unwrap();
        assert_eq!(zone_cursor.version, 4);
        assert_eq!(zone_cursor.limit, 1);
        assert!(!zone_cursor.inventory_tag.is_empty());
        assert_eq!(zone_cursor.expires_at - zone_cursor.issued_at, 900);
        assert!(matches!(zone_cursor.scope, CursorScope::DnsZones { .. }));

        let zone = zone_page.items.into_iter().next().unwrap().zone;
        let record_page = adapter
            .list_record_sets(
                &zone,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        let record_token = record_page.next.unwrap();
        let record_bytes = URL_SAFE_NO_PAD.decode(record_token.as_str()).unwrap();
        let record_payload = std::str::from_utf8(&record_bytes[..record_bytes.len() - 32]).unwrap();
        assert!(!record_payload.contains(center_account.as_str()));
        assert!(!record_payload.contains(native_account));
        assert!(!record_payload.contains(zone.zone_id.as_str()));
        assert!(!record_payload.contains(&scope_hash(center_account.as_str())));
        assert!(!record_payload.contains(&scope_hash(native_account)));
        assert!(!record_payload.contains(&scope_hash(zone.zone_id.as_str())));
        let record_cursor: Cursor = serde_json::from_str(record_payload).unwrap();
        assert_eq!(record_cursor.version, 4);
        assert_eq!(record_cursor.limit, 1);
        assert!(!record_cursor.inventory_tag.is_empty());
        assert_eq!(record_cursor.expires_at - record_cursor.issued_at, 900);
        assert!(matches!(record_cursor.scope, CursorScope::Records { .. }));
    }

    #[tokio::test]
    async fn unchanged_inventory_continuation_returns_the_next_stable_slice() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            CloudflareCursorKey::new([18; 32]).unwrap(),
        )
        .unwrap();

        let first = adapter
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        let second = adapter
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: first.next,
                },
            )
            .await
            .unwrap();

        assert_eq!(first.items.len(), 1);
        assert_eq!(second.items.len(), 1);
        assert_ne!(first.items[0].zone.zone_id, second.items[0].zone.zone_id);
        assert!(second.next.is_none());
        assert_eq!(*api.list_zone_calls.lock().unwrap(), 2);
    }

    #[test]
    fn inventory_tag_streams_with_a_strict_serialized_byte_limit() {
        let key = CloudflareCursorKey::new([22; 32]).unwrap();
        let error = inventory_tag_with_limit(&["bounded-value"], &key, 2).unwrap_err();
        assert_eq!(error.code(), "cloudflare_inventory_serialization_limit");

        let tag = inventory_tag_with_limit(&["bounded-value"], &key, 1_024).unwrap();
        assert_eq!(URL_SAFE_NO_PAD.decode(tag).unwrap().len(), 32);
    }

    #[tokio::test]
    async fn zone_inventory_insert_delete_update_and_same_count_replace_require_restart() {
        async fn assert_changed(mut mutate: impl FnMut(&mut Vec<CloudflareZone>)) {
            let center_account = CloudResourceId::new("cloudflare-main").unwrap();
            let api = Arc::new(fake_api());
            let adapter = CloudflareDnsAdapter::new(
                center_account.clone(),
                &account(),
                api.clone(),
                CloudflareCursorKey::new([19; 32]).unwrap(),
            )
            .unwrap();
            let first = adapter
                .list_zone_inventory(
                    &center_account,
                    &DnsPageRequest {
                        limit: 1,
                        token: None,
                    },
                )
                .await
                .unwrap();
            mutate(&mut api.zones.lock().unwrap());

            let error = adapter
                .list_zone_inventory(
                    &center_account,
                    &DnsPageRequest {
                        limit: 1,
                        token: first.next,
                    },
                )
                .await
                .unwrap_err();
            assert_eq!(error.code(), "cloudflare_inventory_changed");
        }

        assert_changed(|zones| {
            let mut inserted = zones[0].clone();
            inserted.id = "dddddddddddddddddddddddddddddddd".to_string();
            inserted.name = "inserted.example".to_string();
            zones.push(inserted);
        })
        .await;
        assert_changed(|zones| {
            zones.remove(0);
        })
        .await;
        assert_changed(|zones| zones[0].modified_on = Some("updated-revision".to_string())).await;
        assert_changed(|zones| {
            zones.remove(0);
            let mut replacement = zones[0].clone();
            replacement.id = "dddddddddddddddddddddddddddddddd".to_string();
            replacement.name = "replacement.example".to_string();
            zones.push(replacement);
        })
        .await;
    }

    #[tokio::test]
    async fn record_inventory_change_requires_restart() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            CloudflareCursorKey::new([20; 32]).unwrap(),
        )
        .unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let first = adapter
            .list_record_sets(
                &zone,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        api.records.lock().unwrap().get_mut(ZONE_A_ID).unwrap()[0].modified_on =
            Some("updated-record-revision".to_string());

        let error = adapter
            .list_record_sets(
                &zone,
                &DnsPageRequest {
                    limit: 1,
                    token: first.next,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "cloudflare_inventory_changed");
    }

    #[tokio::test]
    async fn page_size_and_cross_method_mismatches_reject_before_provider_io() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            CloudflareCursorKey::new([21; 32]).unwrap(),
        )
        .unwrap();
        let first = adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: None,
                },
            )
            .await
            .unwrap();
        let cursor = first.next.unwrap();
        assert_eq!(*api.list_zone_calls.lock().unwrap(), 1);

        let page_size_error = adapter
            .list_zones(
                &center_account,
                &DnsPageRequest {
                    limit: 2,
                    token: Some(cursor.clone()),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(
            page_size_error.code(),
            "cloudflare_cursor_page_size_mismatch"
        );
        assert_eq!(*api.list_zone_calls.lock().unwrap(), 1);

        let method_error = adapter
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(cursor),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(method_error.code(), "cloudflare_cursor_scope_mismatch");
        assert_eq!(*api.list_zone_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn provider_specific_zone_inventory_retains_cloudflare_fields() {
        const ZONE_ID: &str = "abcdef0123456789abcdef0123456789";
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = fake_api();
        {
            let mut zones = api.zones.lock().unwrap();
            zones.truncate(1);
            zones[0].id = ZONE_ID.to_string();
        }
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(api),
            CloudflareCursorKey::new([6; 32]).unwrap(),
        )
        .unwrap();

        let page = adapter
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 10,
                    token: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        let observed = &page.items[0];
        assert_eq!(observed.zone.provider_account_id, center_account);
        assert_eq!(observed.zone.zone_id.as_str(), ZONE_ID);
        assert_eq!(observed.kind, CloudflareZoneKind::Full);
        assert_eq!(observed.status, CloudflareZoneStatus::Active);
        assert_eq!(observed.name_servers, fake_nameservers());
        assert_eq!(
            observed.revision.as_ref().unwrap().as_str(),
            "zone-a-revision"
        );

        let detail = adapter
            .get_zone_by_id(&center_account, &DnsZoneId::new(ZONE_ID).unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(detail, *observed);
    }

    #[tokio::test]
    async fn provider_specific_zone_inventory_rejects_malformed_provider_scope() {
        const ZONE_ID: &str = "abcdef0123456789abcdef0123456789";
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();

        let malformed_zone_api = fake_api();
        malformed_zone_api.zones.lock().unwrap()[0].id = "zone-a".to_string();
        let malformed_zone_adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(malformed_zone_api),
            CloudflareCursorKey::new([4; 32]).unwrap(),
        )
        .unwrap();
        let malformed_zone = malformed_zone_adapter
            .list_zone_inventory(
                &center_account,
                &DnsPageRequest {
                    limit: 10,
                    token: None,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(malformed_zone.code(), "invalid_cloudflare_zone_id");

        let api = fake_api();
        {
            let mut zones = api.zones.lock().unwrap();
            zones.truncate(1);
            zones[0].id = ZONE_ID.to_string();
            zones[0].account_id = "not-a-cloudflare-account-id".to_string();
        }
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(api),
            CloudflareCursorKey::new([5; 32]).unwrap(),
        )
        .unwrap();

        let error = adapter
            .get_zone_by_id(&center_account, &DnsZoneId::new(ZONE_ID).unwrap())
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Validation);
        assert_eq!(error.code(), "invalid_cloudflare_zone_account_id");

        let invalid_id = adapter
            .get_zone_by_id(&center_account, &DnsZoneId::new("zone-a").unwrap())
            .await
            .unwrap_err();
        assert_eq!(invalid_id.code(), "invalid_cloudflare_zone_id");

        let mismatched_api = fake_api();
        *mismatched_api.get_zone_override.lock().unwrap() = Some(CloudflareZone {
            id: "11111111111111111111111111111111".to_string(),
            account_id: "0123456789abcdef0123456789abcdef".to_string(),
            name: "example.test".to_string(),
            kind: CloudflareZoneKind::Full,
            status: CloudflareZoneStatus::Active,
            name_servers: fake_nameservers(),
            modified_on: None,
        });
        let mismatched_adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(mismatched_api),
            CloudflareCursorKey::new([3; 32]).unwrap(),
        )
        .unwrap();
        let mismatch = mismatched_adapter
            .get_zone_by_id(&center_account, &DnsZoneId::new(ZONE_ID).unwrap())
            .await
            .unwrap_err();
        assert_eq!(mismatch.code(), "cloudflare_zone_identity_mismatch");
    }

    #[tokio::test]
    async fn record_inventory_revalidates_provider_zone_account() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            CloudflareCursorKey::new([3; 32]).unwrap(),
        )
        .unwrap();
        let forged = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_OTHER_ID).unwrap(),
            apex: AbsoluteDnsName::new("other.example").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        assert!(adapter
            .list_record_sets(
                &forged,
                &DnsPageRequest {
                    limit: 10,
                    token: None,
                },
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn record_inventory_rejects_invalid_zone_id_before_provider_call() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let api = Arc::new(fake_api());
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            api.clone(),
            CloudflareCursorKey::new([3; 32]).unwrap(),
        )
        .unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new("zone-a").unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };

        let error = adapter
            .list_record_sets(
                &zone,
                &DnsPageRequest {
                    limit: 10,
                    token: None,
                },
            )
            .await
            .unwrap_err();

        assert_eq!(error.code(), "invalid_cloudflare_zone_id");
        assert_eq!(*api.get_zone_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn http_pages_are_fully_scanned_before_rrset_aggregation() {
        const ZONE_ID: &str = "abcdef0123456789abcdef0123456789";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/client/v4/zones/{ZONE_ID}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": {
                    "id": ZONE_ID,
                    "account": { "id": "0123456789abcdef0123456789abcdef" },
                    "name": "example.test",
                    "type": "full",
                    "status": "active",
                    "name_servers": ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"],
                    "modified_on": "2026-07-17T00:00:00Z"
                }
            })))
            .mount(&server)
            .await;
        for (page, id, address) in [
            (1, "11111111111111111111111111111111", "192.0.2.1"),
            (2, "22222222222222222222222222222222", "192.0.2.2"),
        ] {
            Mock::given(method("GET"))
                .and(path(format!("/client/v4/zones/{ZONE_ID}/dns_records")))
                .and(query_param("page", page.to_string()))
                .and(query_param("per_page", PROVIDER_PAGE_SIZE.to_string()))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "success": true,
                    "errors": [],
                    "result": [{
                        "id": id, "name": "a.example.test", "type": "A", "ttl": 300,
                        "content": address, "proxied": false, "proxiable": true,
                        "settings": {}, "modified_on": "2026-07-17T00:00:00Z"
                    }],
                    "result_info": {
                        "page": page, "per_page": PROVIDER_PAGE_SIZE,
                        "count": 1, "total_pages": 2
                    }
                })))
                .mount(&server)
                .await;
        }
        let api = CloudflareHttpApi::with_base_url(
            CloudflareApiToken::new("secret-token").unwrap(),
            format!("{}/client/v4/", server.uri()),
        )
        .unwrap();
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(api),
            CloudflareCursorKey::new([11; 32]).unwrap(),
        )
        .unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let records = adapter
            .list_record_sets(
                &zone,
                &DnsPageRequest {
                    limit: 10,
                    token: None,
                },
            )
            .await
            .unwrap()
            .items;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_set.values.len(), 2);
        assert_eq!(records[0].provider_object_ids.len(), 2);
    }

    #[tokio::test]
    async fn atomic_guard_is_rejected_before_any_http_call() {
        let server = MockServer::start().await;
        let api = CloudflareHttpApi::with_base_url(
            CloudflareApiToken::new("secret-token").unwrap(),
            format!("{}/client/v4/", server.uri()),
        )
        .unwrap();
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let adapter =
            write_adapter(center_account.clone(), &account(), Arc::new(api), [12; 32]).unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new("abcdef0123456789abcdef0123456789").unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let error = adapter
            .apply_record_changes(
                &zone,
                &[DnsRecordChange::Create {
                    record_set: txt_record("new.example.test", "value"),
                    guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                }],
                DnsGuardStrength::Atomic,
            )
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Conflict);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn http_adapter_applies_a_best_effort_create_end_to_end() {
        const ACCOUNT_ID: &str = "0123456789abcdef0123456789abcdef";
        const ZONE_ID: &str = "abcdef0123456789abcdef0123456789";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/client/v4/zones/{ZONE_ID}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": {
                    "id": ZONE_ID,
                    "account": { "id": ACCOUNT_ID },
                    "name": "example.test",
                    "type": "full",
                    "status": "active",
                    "name_servers": ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"],
                    "modified_on": "2026-07-17T00:00:00Z"
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/client/v4/zones/{ZONE_ID}/dns_records")))
            .and(query_param("page", "1"))
            .and(query_param("per_page", PROVIDER_PAGE_SIZE.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": [],
                "result_info": {
                    "page": 1, "per_page": PROVIDER_PAGE_SIZE,
                    "count": 0, "total_pages": 1
                }
            })))
            .mount(&server)
            .await;
        let desired = txt_record("new.example.test", "value");
        let expected_batch = CloudflareBatchRequest {
            deletes: Vec::new(),
            patches: Vec::new(),
            puts: Vec::new(),
            posts: render_record_set(&desired).unwrap(),
        };
        Mock::given(method("POST"))
            .and(path(format!(
                "/client/v4/zones/{ZONE_ID}/dns_records/batch"
            )))
            .and(header("authorization", "Bearer secret-token"))
            .and(body_json(serde_json::to_value(&expected_batch).unwrap()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": {
                    "posts": [{
                        "id": "44444444444444444444444444444444",
                        "name": "new.example.test", "type": "TXT", "ttl": 300,
                        "content": "\"value\"", "proxied": false, "proxiable": false,
                        "settings": {}, "tags": [],
                        "modified_on": "2026-07-17T00:00:00Z"
                    }]
                }
            })))
            .mount(&server)
            .await;
        let api = CloudflareHttpApi::with_base_url(
            CloudflareApiToken::new("secret-token").unwrap(),
            format!("{}/client/v4/", server.uri()),
        )
        .unwrap();
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let adapter =
            write_adapter(center_account.clone(), &account(), Arc::new(api), [14; 32]).unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let receipt = adapter
            .apply_record_changes(
                &zone,
                &[DnsRecordChange::Create {
                    record_set: desired,
                    guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                }],
                DnsGuardStrength::BestEffort,
            )
            .await
            .unwrap();
        assert_eq!(receipt.state, DnsChangeState::ProviderCommitted);
        assert_eq!(
            adapter.observe_change(&zone, &receipt.id).await.unwrap(),
            receipt
        );
    }

    #[test]
    fn signed_receipts_are_zone_bound_and_tamper_evident() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let adapter = write_adapter(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            [13; 32],
        )
        .unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let request = CloudflareBatchRequest {
            deletes: Vec::new(),
            patches: Vec::new(),
            puts: Vec::new(),
            posts: render_record_set(&txt_record("new.example.test", "value")).unwrap(),
        };
        let receipt = adapter.build_receipt(&zone, &request).unwrap();
        assert_eq!(
            adapter.observe_receipt(&zone, &receipt.id).unwrap(),
            receipt
        );
        let mut bytes = URL_SAFE_NO_PAD.decode(receipt.id.as_str()).unwrap();
        bytes[0] ^= 1;
        let tampered = DnsChangeId::new(URL_SAFE_NO_PAD.encode(bytes)).unwrap();
        assert!(adapter.observe_receipt(&zone, &tampered).is_err());
        let mut other_zone = zone;
        other_zone.zone_id = DnsZoneId::new(ZONE_B_ID).unwrap();
        assert!(adapter.observe_receipt(&other_zone, &receipt.id).is_err());
    }

    #[test]
    fn signed_receipt_fits_with_a_maximum_length_center_account_id() {
        let center_account = CloudResourceId::new("x".repeat(512)).unwrap();
        let adapter = write_adapter(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            [15; 32],
        )
        .unwrap();
        let zone = DnsZoneRef {
            provider_account_id: center_account,
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let request = CloudflareBatchRequest {
            deletes: Vec::new(),
            patches: Vec::new(),
            puts: Vec::new(),
            posts: render_record_set(&txt_record("new.example.test", "value")).unwrap(),
        };
        let receipt = adapter.build_receipt(&zone, &request).unwrap();
        assert!(receipt.id.as_str().len() <= 512);
        assert_eq!(
            adapter.observe_receipt(&zone, &receipt.id).unwrap(),
            receipt
        );
    }

    #[test]
    fn heterogeneous_member_metadata_fails_closed() {
        let zone = DnsZoneRef {
            provider_account_id: CloudResourceId::new("cloudflare-main").unwrap(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let mut first = a_record(RECORD_A_ID, "192.0.2.1");
        let mut second = a_record(RECORD_B_ID, "192.0.2.2");
        first.comment = Some("one".to_string());
        second.comment = Some("two".to_string());
        assert!(aggregate_records(&zone, vec![first, second]).is_err());
    }

    #[test]
    fn record_inventory_rejects_non_cloudflare_object_ids() {
        let zone = DnsZoneRef {
            provider_account_id: CloudResourceId::new("cloudflare-main").unwrap(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        for id in [
            "record-a",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "gggggggggggggggggggggggggggggggg",
        ] {
            let error = aggregate_records(&zone, vec![a_record(id, "192.0.2.1")]).unwrap_err();
            assert_eq!(error.code(), "invalid_cloudflare_record_object_id");
        }
    }

    #[test]
    fn record_inventory_rejects_flattening_on_non_cname_records() {
        let zone = DnsZoneRef {
            provider_account_id: CloudResourceId::new("cloudflare-main").unwrap(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        for flatten_cname in [false, true] {
            let mut record = a_record(RECORD_A_ID, "192.0.2.1");
            record.flatten_cname = Some(flatten_cname);
            let error = aggregate_records(&zone, vec![record]).unwrap_err();
            assert_eq!(error.code(), "invalid_cloudflare_cname_flattening");
        }
    }

    #[test]
    fn txt_batch_rendering_round_trips_binary_segments() {
        let value = edgion_center_core::DnsTxtValue::new(vec![
            DnsCharacterString::new(b"quote:\" slash:\\".to_vec()).unwrap(),
            DnsCharacterString::new(vec![0, 31, 255]).unwrap(),
            DnsCharacterString::new(Vec::new()).unwrap(),
        ])
        .unwrap();
        let rendered = render_txt(&value);
        assert_eq!(http::txt_value(&rendered).unwrap(), value);
    }

    #[test]
    fn incomplete_batch_success_is_an_unknown_outcome() {
        let zone = DnsZoneRef {
            provider_account_id: CloudResourceId::new("cloudflare-main").unwrap(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_A_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility: ZoneVisibility::Public,
        };
        let desired = txt_record("new.example.test", "value");
        let changes = vec![DnsRecordChange::Create {
            record_set: desired.clone(),
            guard: edgion_center_core::DnsMutationGuard::MustNotExist,
        }];
        let request = CloudflareBatchRequest {
            deletes: Vec::new(),
            patches: Vec::new(),
            puts: Vec::new(),
            posts: render_record_set(&desired).unwrap(),
        };
        let error = validate_batch_result(
            &zone,
            &changes,
            &request,
            CloudflareBatchResult {
                deletes: Vec::new(),
                posts: Vec::new(),
            },
        )
        .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
    }

    fn account() -> ProviderAccountSpec {
        ProviderAccountSpec {
            provider: CloudProvider::Cloudflare,
            scope: Some(ProviderAccountScope::Cloudflare {
                account_id: "0123456789abcdef0123456789abcdef".to_string(),
            }),
            credential_source: CredentialSource::Ambient,
        }
    }

    fn write_adapter(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn CloudflareApi>,
        cursor_key: [u8; 32],
    ) -> Result<CloudflareDnsAdapter, NormalizedProviderError> {
        CloudflareDnsAdapter::new_with_cursor_and_mutation_key_rings(
            center_account_id,
            account,
            api,
            CloudflareCursorKey::new(cursor_key)?.into(),
            CloudflareMutationTokenKeyRing::new(CloudflareMutationTokenKey::new([250; 32])?, None)?,
        )
    }

    fn fake_api() -> FakeApi {
        let zones = vec![
            CloudflareZone {
                id: ZONE_A_ID.to_string(),
                account_id: "0123456789abcdef0123456789abcdef".to_string(),
                name: "example.test".to_string(),
                kind: CloudflareZoneKind::Full,
                status: CloudflareZoneStatus::Active,
                name_servers: fake_nameservers(),
                modified_on: Some("zone-a-revision".to_string()),
            },
            CloudflareZone {
                id: ZONE_B_ID.to_string(),
                account_id: "0123456789abcdef0123456789abcdef".to_string(),
                name: "example.net".to_string(),
                kind: CloudflareZoneKind::Full,
                status: CloudflareZoneStatus::Active,
                name_servers: fake_nameservers(),
                modified_on: Some("zone-b-revision".to_string()),
            },
            CloudflareZone {
                id: ZONE_OTHER_ID.to_string(),
                account_id: "fedcba9876543210fedcba9876543210".to_string(),
                name: "other.example".to_string(),
                kind: CloudflareZoneKind::Full,
                status: CloudflareZoneStatus::Active,
                name_servers: fake_nameservers(),
                modified_on: Some("zone-other-revision".to_string()),
            },
        ];
        let records = BTreeMap::from([(
            ZONE_A_ID.to_string(),
            vec![
                a_record(RECORD_A_ID, "192.0.2.1"),
                CloudflareRecord {
                    id: RECORD_B_ID.to_string(),
                    name: "txt.example.test".to_string(),
                    ttl: 300,
                    value: txt_value("seed"),
                    proxied: None,
                    proxiable: false,
                    flatten_cname: None,
                    ipv4_only: false,
                    ipv6_only: false,
                    private_routing: false,
                    comment: None,
                    tags: BTreeSet::new(),
                    modified_on: Some("revision-b".to_string()),
                },
                CloudflareRecord {
                    id: RECORD_C_ID.to_string(),
                    name: "ns.example.test".to_string(),
                    ttl: 300,
                    value: DnsRecordSetValue::Ns {
                        target: AbsoluteDnsName::new("ns.example.net").unwrap(),
                    },
                    proxied: None,
                    proxiable: false,
                    flatten_cname: None,
                    ipv4_only: false,
                    ipv6_only: false,
                    private_routing: false,
                    comment: None,
                    tags: BTreeSet::new(),
                    modified_on: Some("revision-c".to_string()),
                },
            ],
        )]);
        FakeApi {
            total_calls: AtomicU64::new(0),
            zones: Mutex::new(zones),
            create_zone_override: Mutex::new(None),
            get_zone_override: Mutex::new(None),
            get_zone_calls: Mutex::new(0),
            list_zone_calls: Mutex::new(0),
            list_record_calls: Mutex::new(0),
            delete_zone_calls: Mutex::new(0),
            delete_zone_ack_override: Mutex::new(None),
            batch_record_calls: Mutex::new(0),
            batch_result_override: Mutex::new(None),
            zone_total_pages: 1,
            records: Mutex::new(records),
            dnssec: Mutex::new(BTreeMap::new()),
            sequence: Mutex::new(1000),
        }
    }

    fn fake_nameservers() -> BTreeSet<AbsoluteDnsName> {
        [
            AbsoluteDnsName::new("ada.ns.cloudflare.com").unwrap(),
            AbsoluteDnsName::new("bob.ns.cloudflare.com").unwrap(),
        ]
        .into_iter()
        .collect()
    }

    fn record_type_name(record_type: ProviderDnsRecordType) -> &'static str {
        match record_type {
            ProviderDnsRecordType::A => "A",
            ProviderDnsRecordType::Aaaa => "AAAA",
            ProviderDnsRecordType::Cname => "CNAME",
            ProviderDnsRecordType::Txt => "TXT",
            ProviderDnsRecordType::Mx => "MX",
            ProviderDnsRecordType::Srv => "SRV",
            ProviderDnsRecordType::Caa => "CAA",
            ProviderDnsRecordType::Ns => "NS",
            ProviderDnsRecordType::Soa => "SOA",
            ProviderDnsRecordType::GoogleAlias => "ALIAS",
        }
    }

    fn fake_record_from_post(
        id: String,
        post: &CloudflareBatchRecord,
    ) -> CloudflareApiResult<CloudflareRecord> {
        let value = match post.kind.as_str() {
            "TXT" => DnsRecordSetValue::Txt {
                value: http::txt_value(
                    post.content
                        .as_deref()
                        .ok_or_else(|| validation("fake_cloudflare_txt_missing"))?,
                )?,
            },
            _ => return Err(validation("fake_cloudflare_post_type_unsupported")),
        };
        Ok(CloudflareRecord {
            id,
            name: post.name.clone(),
            ttl: post.ttl,
            value,
            proxied: post.proxied,
            proxiable: false,
            flatten_cname: post
                .settings
                .as_ref()
                .and_then(|settings| settings.flatten_cname),
            ipv4_only: false,
            ipv6_only: false,
            private_routing: false,
            comment: post.comment.clone(),
            tags: post.tags.clone(),
            modified_on: Some("fake-batch-revision".to_string()),
        })
    }

    fn a_record(id: &str, address: &str) -> CloudflareRecord {
        CloudflareRecord {
            id: id.to_string(),
            name: "a.example.test".to_string(),
            ttl: 300,
            value: DnsRecordSetValue::A {
                address: address.parse::<Ipv4Addr>().unwrap(),
            },
            proxied: Some(false),
            proxiable: true,
            flatten_cname: None,
            ipv4_only: false,
            ipv6_only: false,
            private_routing: false,
            comment: None,
            tags: BTreeSet::new(),
            modified_on: Some(format!("{id}-revision")),
        }
    }

    fn txt_value(value: &str) -> DnsRecordSetValue {
        DnsRecordSetValue::Txt {
            value: DnsTxtValue::new(vec![
                DnsCharacterString::new(value.as_bytes().to_vec()).unwrap()
            ])
            .unwrap(),
        }
    }

    fn txt_record(owner: &str, value: &str) -> ProviderDnsRecordSet {
        ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new(owner).unwrap(),
                record_type: ProviderDnsRecordType::Txt,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: BTreeSet::from([txt_value(value)]),
            extension: None,
        }
    }
}
