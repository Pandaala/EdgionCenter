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
    sync::Arc,
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
const MAX_BATCH_OPERATIONS: usize = 10_000;
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Account-bound Cloudflare DNS adapter.
pub struct CloudflareDnsAdapter {
    center_account_id: CloudResourceId,
    cloudflare_account_id: String,
    api: Arc<dyn CloudflareApi>,
    cursor_key: CloudflareCursorKey,
}

impl CloudflareDnsAdapter {
    pub fn new(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn CloudflareApi>,
        cursor_key: CloudflareCursorKey,
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
        Ok(Self {
            center_account_id,
            cloudflare_account_id: account_id.clone(),
            api,
            cursor_key,
        })
    }

    /// Validates an opaque record-page cursor for the exact account and zone without provider I/O.
    pub fn validate_record_inventory_cursor(
        &self,
        zone_id: &DnsZoneId,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<()> {
        page.validate().map_err(|_| validation("invalid_page"))?;
        if !valid_cloudflare_identifier(zone_id.as_str()) {
            return Err(validation("invalid_cloudflare_zone_id"));
        }
        decode_cursor_offset(page, &self.record_cursor_scope(zone_id), &self.cursor_key).map(drop)
    }

    fn record_cursor_scope(&self, zone_id: &DnsZoneId) -> CursorScope {
        CursorScope::Records {
            center_scope: cursor_scope_tag(
                &self.cursor_key,
                b"center-account",
                self.center_account_id.as_str(),
            ),
            external_scope: cursor_scope_tag(
                &self.cursor_key,
                b"native-account",
                &self.cloudflare_account_id,
            ),
            zone_scope: cursor_scope_tag(&self.cursor_key, b"zone", zone_id.as_str()),
        }
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
        if zone.account_id != self.cloudflare_account_id {
            return Err(validation("cloudflare_zone_account_mismatch"));
        }
        let visibility = match zone.kind {
            CloudflareZoneKind::Full
            | CloudflareZoneKind::Partial
            | CloudflareZoneKind::Secondary => ZoneVisibility::Public,
            CloudflareZoneKind::Internal => ZoneVisibility::Private,
        };
        Ok(ObservedDnsZone {
            zone: DnsZoneRef {
                provider_account_id: self.center_account_id.clone(),
                provider: CloudProvider::Cloudflare,
                zone_id: DnsZoneId::new(zone.id).map_err(|_| validation("invalid_zone_id"))?,
                apex: AbsoluteDnsName::new(zone.name)
                    .map_err(|_| validation("invalid_zone_name"))?,
                visibility,
            },
            revision: zone
                .modified_on
                .map(DnsRecordRevision::new)
                .transpose()
                .map_err(|_| validation("invalid_zone_revision"))?,
        })
    }

    fn map_zone_inventory(
        &self,
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
        let observed = self.map_zone(zone)?;
        Ok(ObservedCloudflareZone {
            zone: observed.zone,
            kind,
            status,
            name_servers,
            revision: observed.revision,
        })
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
        let token = LifecycleMutationToken {
            version: 1,
            center_scope: scope_hash(self.center_account_id.as_str()),
            external_scope: scope_hash(&self.cloudflare_account_id),
            mutation,
        };
        let mutation_id = ZoneLifecycleMutationId::new(sign_token(&token, &self.cursor_key)?)
            .map_err(|_| validation("cloudflare_lifecycle_receipt_encoding_failed"))?;
        Ok(ZoneLifecycleMutationReceipt { mutation_id, state })
    }

    fn build_receipt(
        &self,
        zone: &DnsZoneRef,
        request: &CloudflareBatchRequest,
    ) -> DnsProviderResult<DnsChangeReceipt> {
        let request_bytes = serde_json::to_vec(request)
            .map_err(|_| validation("cloudflare_batch_encoding_failed"))?;
        let token = ChangeToken {
            version: 1,
            center_scope: scope_hash(self.center_account_id.as_str()),
            external_scope: scope_hash(&self.cloudflare_account_id),
            zone_scope: scope_hash(zone.zone_id.as_str()),
            request_scope: URL_SAFE_NO_PAD.encode(Sha256::digest(request_bytes)),
            guard: DnsGuardStrength::BestEffort,
        };
        let id = DnsChangeId::new(sign_token(&token, &self.cursor_key)?)
            .map_err(|_| validation("cloudflare_receipt_encoding_failed"))?;
        Ok(committed_receipt(id))
    }

    fn observe_receipt(
        &self,
        zone: &DnsZoneRef,
        change_id: &DnsChangeId,
    ) -> DnsProviderResult<DnsChangeReceipt> {
        let token: ChangeToken = verify_token(change_id.as_str(), &self.cursor_key)
            .map_err(|_| not_found("cloudflare_change_not_found"))?;
        if token.version != 1
            || token.center_scope != scope_hash(self.center_account_id.as_str())
            || token.external_scope != scope_hash(&self.cloudflare_account_id)
            || token.zone_scope != scope_hash(zone.zone_id.as_str())
            || token.guard != DnsGuardStrength::BestEffort
        {
            return Err(not_found("cloudflare_change_not_found"));
        }
        Ok(committed_receipt(change_id.clone()))
    }
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
        let scope = CursorScope::Zones {
            center_scope: cursor_scope_tag(
                &self.cursor_key,
                b"center-account",
                self.center_account_id.as_str(),
            ),
            external_scope: cursor_scope_tag(
                &self.cursor_key,
                b"native-account",
                &self.cloudflare_account_id,
            ),
        };
        let offset = decode_cursor_offset(page, &scope, &self.cursor_key)?;
        paginate(
            self.all_zones().await?,
            page,
            scope,
            &self.cursor_key,
            offset,
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
        let scope = self.record_cursor_scope(&zone.zone_id);
        let offset = decode_cursor_offset(page, &scope, &self.cursor_key)?;
        paginate(
            self.all_records(zone).await?,
            page,
            scope,
            &self.cursor_key,
            offset,
        )
    }

    async fn apply_record_changes(
        &self,
        zone: &DnsZoneRef,
        changes: &[DnsRecordChange],
        minimum_guard: DnsGuardStrength,
    ) -> DnsProviderResult<DnsChangeReceipt> {
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
        let scope = CursorScope::Zones {
            center_scope: cursor_scope_tag(
                &self.cursor_key,
                b"center-account",
                self.center_account_id.as_str(),
            ),
            external_scope: cursor_scope_tag(
                &self.cursor_key,
                b"native-account",
                &self.cloudflare_account_id,
            ),
        };
        let offset = decode_cursor_offset(page, &scope, &self.cursor_key)?;
        paginate(
            self.all_zone_inventory().await?,
            page,
            scope,
            &self.cursor_key,
            offset,
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
        if request.provider != CloudProvider::Cloudflare
            || request.provider_account_id != self.center_account_id
        {
            return Err(validation("cloudflare_zone_creation_scope_mismatch"));
        }
        if request.visibility != ZoneVisibility::Public {
            return Err(validation("cloudflare_private_zone_creation_unsupported"));
        }
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
        let zone = DnsZoneRef {
            provider_account_id: self.center_account_id.clone(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(created.id)
                .map_err(|_| unknown_outcome("cloudflare_create_zone_id_invalid"))?,
            apex: request.apex.clone(),
            visibility: ZoneVisibility::Public,
        };
        self.lifecycle_receipt(
            LifecycleMutation::Create {
                zone,
                request_scope: scope_hash(request.idempotency_key.as_str()),
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
            return self.lifecycle_receipt(
                LifecycleMutation::Dnssec {
                    zone: zone.clone(),
                    desired,
                },
                ZoneLifecycleMutationState::Succeeded,
            );
        }
        let result = self
            .api
            .patch_dnssec(zone.zone_id.as_str(), desired)
            .await?;
        map_dnssec_observation(Some(&result))?;
        self.lifecycle_receipt(
            LifecycleMutation::Dnssec {
                zone: zone.clone(),
                desired,
            },
            ZoneLifecycleMutationState::Pending,
        )
    }

    async fn delete_zone(
        &self,
        request: &ZoneDeletionRequest,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        if request.approval().approved_revision != *request.revision()
            || request.approval().approved_zone != *request.zone()
            || request.approval().approved_by.trim().is_empty()
            || request.approval().approved_at.trim().is_empty()
        {
            return Err(validation("invalid_cloudflare_zone_deletion_approval"));
        }
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
        self.lifecycle_receipt(
            LifecycleMutation::Delete {
                zone: request.zone().clone(),
            },
            ZoneLifecycleMutationState::Pending,
        )
    }

    async fn observe_mutation(
        &self,
        mutation_id: &ZoneLifecycleMutationId,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        let token: LifecycleMutationToken = verify_token(mutation_id.as_str(), &self.cursor_key)
            .map_err(|_| not_found("cloudflare_lifecycle_mutation_not_found"))?;
        if token.version != 1
            || token.center_scope != scope_hash(self.center_account_id.as_str())
            || token.external_scope != scope_hash(&self.cloudflare_account_id)
        {
            return Err(not_found("cloudflare_lifecycle_mutation_not_found"));
        }
        let state = match &token.mutation {
            LifecycleMutation::Create { zone, .. } => {
                match self.api.get_zone(zone.zone_id.as_str()).await? {
                    Some(value) => {
                        if self.map_zone(value)?.zone == *zone {
                            ZoneLifecycleMutationState::Succeeded
                        } else {
                            ZoneLifecycleMutationState::UnknownOutcome
                        }
                    }
                    None => ZoneLifecycleMutationState::Pending,
                }
            }
            LifecycleMutation::Delete { zone } => {
                if self.api.get_zone(zone.zone_id.as_str()).await?.is_none() {
                    ZoneLifecycleMutationState::Succeeded
                } else {
                    ZoneLifecycleMutationState::Pending
                }
            }
            LifecycleMutation::Dnssec { zone, desired } => {
                let value = self.api.get_dnssec(zone.zone_id.as_str()).await?;
                dnssec_mutation_state(value.as_ref(), *desired)
            }
        };
        Ok(ZoneLifecycleMutationReceipt {
            mutation_id: mutation_id.clone(),
            state,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct LifecycleMutationToken {
    version: u8,
    center_scope: String,
    external_scope: String,
    mutation: LifecycleMutation,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LifecycleMutation {
    Create {
        zone: DnsZoneRef,
        request_scope: String,
    },
    Delete {
        zone: DnsZoneRef,
    },
    Dnssec {
        zone: DnsZoneRef,
        desired: DnssecDesiredState,
    },
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

fn sign_token<T: Serialize>(value: &T, key: &CloudflareCursorKey) -> DnsProviderResult<String> {
    let encoded =
        serde_json::to_vec(value).map_err(|_| validation("cloudflare_token_encoding_failed"))?;
    let signature = HmacSha256::new_from_slice(key.0.as_ref())
        .expect("fixed HMAC key")
        .chain_update(&encoded)
        .finalize()
        .into_bytes();
    let mut authenticated = encoded;
    authenticated.extend_from_slice(&signature);
    Ok(URL_SAFE_NO_PAD.encode(authenticated))
}

fn verify_token<T: for<'de> Deserialize<'de>>(
    value: &str,
    key: &CloudflareCursorKey,
) -> DnsProviderResult<T> {
    let authenticated = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| validation("invalid_cloudflare_token"))?;
    if authenticated.len() < 32 {
        return Err(validation("invalid_cloudflare_token"));
    }
    let (encoded, signature) = authenticated.split_at(authenticated.len() - 32);
    HmacSha256::new_from_slice(key.0.as_ref())
        .expect("fixed HMAC key")
        .chain_update(encoded)
        .verify_slice(signature)
        .map_err(|_| validation("invalid_cloudflare_token_signature"))?;
    serde_json::from_slice(encoded).map_err(|_| validation("invalid_cloudflare_token"))
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
    Zones {
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

#[derive(Serialize, Deserialize)]
struct Cursor {
    version: u8,
    scope: CursorScope,
    offset: usize,
}

fn decode_cursor_offset(
    request: &DnsPageRequest,
    scope: &CursorScope,
    key: &CloudflareCursorKey,
) -> DnsProviderResult<usize> {
    let offset = match request.token.as_ref() {
        None => 0,
        Some(token) => {
            let authenticated = URL_SAFE_NO_PAD
                .decode(token.as_str())
                .map_err(|_| validation("invalid_cloudflare_cursor"))?;
            if authenticated.len() < 32 {
                return Err(validation("invalid_cloudflare_cursor"));
            }
            let (bytes, signature) = authenticated.split_at(authenticated.len() - 32);
            HmacSha256::new_from_slice(key.0.as_ref())
                .expect("fixed HMAC key")
                .chain_update(bytes)
                .verify_slice(signature)
                .map_err(|_| validation("invalid_cloudflare_cursor_signature"))?;
            let cursor: Cursor = serde_json::from_slice(bytes)
                .map_err(|_| validation("invalid_cloudflare_cursor"))?;
            if cursor.version != 2 || &cursor.scope != scope {
                return Err(validation("cloudflare_cursor_scope_mismatch"));
            }
            cursor.offset
        }
    };
    Ok(offset)
}

fn paginate<T>(
    items: Vec<T>,
    request: &DnsPageRequest,
    scope: CursorScope,
    key: &CloudflareCursorKey,
    offset: usize,
) -> DnsProviderResult<DnsPage<T>> {
    if offset > items.len() {
        return Err(validation("invalid_cloudflare_cursor_offset"));
    }
    let end = (offset + usize::from(request.limit)).min(items.len());
    let next = if end < items.len() {
        let encoded = serde_json::to_vec(&Cursor {
            version: 2,
            scope,
            offset: end,
        })
        .map_err(|_| validation("cloudflare_cursor_encoding_failed"))?;
        let signature = HmacSha256::new_from_slice(key.0.as_ref())
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
    use std::{net::Ipv4Addr, sync::Mutex};

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

    struct FakeApi {
        zones: Mutex<Vec<CloudflareZone>>,
        get_zone_override: Mutex<Option<CloudflareZone>>,
        get_zone_calls: Mutex<u64>,
        list_zone_calls: Mutex<u64>,
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
            self.zones.lock().unwrap().retain(|zone| zone.id != zone_id);
            Ok(CloudflareDeleteZoneAck {
                id: zone_id.to_string(),
            })
        }

        async fn get_dnssec(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareDnssec>> {
            Ok(self.dnssec.lock().unwrap().get(zone_id).cloned())
        }

        async fn patch_dnssec(
            &self,
            zone_id: &str,
            desired: DnssecDesiredState,
        ) -> CloudflareApiResult<CloudflareDnssec> {
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
            Ok(CloudflareBatchResult {
                deletes: deleted,
                posts: posted,
            })
        }
    }

    #[tokio::test]
    async fn scoped_adapter_passes_the_complete_shared_dns_contract() {
        let center_account = CloudResourceId::new("cloudflare-main").unwrap();
        let account = account();
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account,
            Arc::new(fake_api()),
            CloudflareCursorKey::new([7; 32]).unwrap(),
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
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            CloudflareCursorKey::new([9; 32]).unwrap(),
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
        assert!(!zone_payload.contains(&scope_hash(center_account.as_str())));
        assert!(!zone_payload.contains(&scope_hash(native_account)));
        let zone_cursor: Cursor = serde_json::from_str(zone_payload).unwrap();
        assert_eq!(zone_cursor.version, 2);
        assert!(matches!(zone_cursor.scope, CursorScope::Zones { .. }));

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
        assert_eq!(record_cursor.version, 2);
        assert!(matches!(record_cursor.scope, CursorScope::Records { .. }));
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
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(api),
            CloudflareCursorKey::new([12; 32]).unwrap(),
        )
        .unwrap();
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
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(api),
            CloudflareCursorKey::new([14; 32]).unwrap(),
        )
        .unwrap();
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
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            CloudflareCursorKey::new([13; 32]).unwrap(),
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
        let adapter = CloudflareDnsAdapter::new(
            center_account.clone(),
            &account(),
            Arc::new(fake_api()),
            CloudflareCursorKey::new([15; 32]).unwrap(),
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
            zones: Mutex::new(zones),
            get_zone_override: Mutex::new(None),
            get_zone_calls: Mutex::new(0),
            list_zone_calls: Mutex::new(0),
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
