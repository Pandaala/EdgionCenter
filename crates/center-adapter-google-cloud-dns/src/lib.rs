//! Google Cloud DNS v1 adapter with an SDK-free, credential-owning transport seam.

mod api;
mod http;
mod model;
pub use api::*;
pub use http::GoogleCloudDnsHttpApi;

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    evaluate_zone_readiness, validate_dns_changes, AbsoluteDnsName, AuthoritativeDnsVerification,
    CloudProvider, CloudResourceId, DelegationObservation, DelegationState, DnsBatchAtomicity,
    DnsCapability, DnsChangeId, DnsChangeReceipt, DnsChangeState, DnsGuardStrength, DnsPage,
    DnsPageRequest, DnsPageToken, DnsPropagationState, DnsProvider, DnsProviderResult,
    DnsRecordChange, DnsRecordObjectId, DnsRecordSetKey, DnsZoneId, DnsZoneRef, DnssecDesiredState,
    DnssecDsRecord, DnssecExternalAction, DnssecObservation, DnssecProviderState,
    NormalizedProviderError, ObservedDnsRecordSet, ObservedDnsZone, ProviderAccountScope,
    ProviderAccountSpec, ProviderErrorCategory, ZoneCreationRequest, ZoneDeletionRequest,
    ZoneLifecycleMutationId, ZoneLifecycleMutationReceipt, ZoneLifecycleMutationState,
    ZoneLifecycleObservation, ZoneLifecycleProvider, ZoneLifecycleProviderResult,
    ZoneLifecycleRevision, ZoneVisibility,
};
use hmac::{Hmac, Mac};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use zeroize::Zeroize;

const PROVIDER_PAGE_SIZE: u16 = 1000;
const MAX_PROVIDER_PAGES: usize = 10_000;
const MAX_ZONES: usize = 10_000;
const MAX_RECORDS: usize = 100_000;
const MAX_CHANGE_RRSETS: usize = 1000;
type HmacSha256 = Hmac<Sha256>;
pub type Result<T> = std::result::Result<T, NormalizedProviderError>;

pub struct GoogleCloudDnsCursorKey([u8; 32]);
impl GoogleCloudDnsCursorKey {
    pub fn new(value: [u8; 32]) -> Result<Self> {
        if value.iter().all(|v| *v == 0) {
            return Err(validation("weak_google_dns_cursor_key"));
        }
        Ok(Self(value))
    }
}
impl Drop for GoogleCloudDnsCursorKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub struct GoogleCloudDnsAdapter {
    center_account_id: CloudResourceId,
    project_id: String,
    api: Arc<dyn GoogleCloudDnsApi>,
    cursor_key: GoogleCloudDnsCursorKey,
}

/// Action-scoped DNS features, separated because Cloud DNS public and private
/// managed zones do not have identical provider behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleCloudDnsCapabilityProfile {
    pub public_zones: BTreeSet<DnsCapability>,
    pub private_zones: BTreeSet<DnsCapability>,
}

impl GoogleCloudDnsAdapter {
    pub fn capability_profile() -> GoogleCloudDnsCapabilityProfile {
        let common = BTreeSet::from([
            DnsCapability::RecordSets,
            DnsCapability::WeightedRouting,
            DnsCapability::GeolocationRouting,
            DnsCapability::FailoverRouting,
            DnsCapability::AtomicChanges,
        ]);
        let mut public_zones = common.clone();
        public_zones.insert(DnsCapability::PublicZones);
        public_zones.insert(DnsCapability::ApexAlias);
        let mut private_zones = common;
        private_zones.insert(DnsCapability::PrivateZones);
        GoogleCloudDnsCapabilityProfile {
            public_zones,
            private_zones,
        }
    }

    pub fn new(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn GoogleCloudDnsApi>,
        cursor_key: GoogleCloudDnsCursorKey,
    ) -> Result<Self> {
        center_account_id
            .validate()
            .map_err(|_| validation("invalid_center_account_id"))?;
        account
            .validate()
            .map_err(|_| validation("invalid_provider_account"))?;
        if account.provider != CloudProvider::GoogleCloud {
            return Err(validation("google_cloud_provider_required"));
        }
        let ProviderAccountScope::GoogleCloud { project_id } = account
            .scope
            .as_ref()
            .ok_or_else(|| validation("google_project_scope_required"))?
        else {
            return Err(validation("google_project_scope_mismatch"));
        };
        if api.verified_project_id() != project_id {
            return Err(validation("google_verified_project_mismatch"));
        }
        Ok(Self {
            center_account_id,
            project_id: project_id.clone(),
            api,
            cursor_key,
        })
    }

    fn validate_zone_ref(&self, zone: &DnsZoneRef) -> Result<()> {
        zone.validate().map_err(|_| validation("invalid_zone"))?;
        if zone.provider != CloudProvider::GoogleCloud
            || zone.provider_account_id != self.center_account_id
        {
            return Err(validation("google_zone_scope_mismatch"));
        }
        Ok(())
    }

    fn map_zone(&self, raw: GoogleManagedZone) -> Result<ObservedDnsZone> {
        if raw.kind != GoogleZoneKind::Authoritative {
            return Err(validation("google_complex_zone_unsupported"));
        }
        let visibility = match raw.visibility {
            GoogleZoneVisibility::Public => ZoneVisibility::Public,
            GoogleZoneVisibility::Private => ZoneVisibility::Private,
        };
        if raw.id.is_empty()
            || raw.id.len() > 20
            || !raw.id.bytes().all(|v| v.is_ascii_digit())
            || raw.name.is_empty()
        {
            return Err(validation("invalid_google_zone_identity"));
        }
        Ok(ObservedDnsZone {
            zone: DnsZoneRef {
                provider_account_id: self.center_account_id.clone(),
                provider: CloudProvider::GoogleCloud,
                zone_id: DnsZoneId::new(raw.id)
                    .map_err(|_| validation("invalid_google_zone_id"))?,
                apex: AbsoluteDnsName::new(raw.dns_name)
                    .map_err(|_| validation("invalid_google_zone_name"))?,
                visibility,
            },
            revision: None,
        })
    }

    async fn all_zones(&self) -> Result<Vec<ObservedDnsZone>> {
        let mut token = None::<String>;
        let mut seen = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut out = Vec::new();
        for _ in 0..MAX_PROVIDER_PAGES {
            let page = self
                .api
                .list_managed_zones(token.as_deref(), PROVIDER_PAGE_SIZE)
                .await?;
            validate_page(&page.next_page_token, page.items.len())?;
            for raw in page.items {
                let mapped = self.map_zone(raw)?;
                if !ids.insert(mapped.zone.zone_id.clone()) {
                    return Err(validation("duplicate_google_zone_id"));
                }
                out.push(mapped);
                if out.len() > MAX_ZONES {
                    return Err(validation("google_zone_inventory_limit"));
                }
            }
            let Some(next) = page.next_page_token else {
                out.sort_by(|a, b| a.zone.zone_id.cmp(&b.zone.zone_id));
                return Ok(out);
            };
            if token.as_deref() == Some(&next) || !seen.insert(next.clone()) {
                return Err(validation("google_zone_pagination_loop"));
            }
            token = Some(next);
        }
        Err(validation("google_zone_pagination_limit"))
    }

    async fn checked_zone(&self, zone: &DnsZoneRef) -> Result<GoogleManagedZone> {
        self.validate_zone_ref(zone)?;
        let raw = self
            .api
            .get_managed_zone(zone.zone_id.as_str())
            .await?
            .ok_or_else(|| not_found("google_zone_not_found"))?;
        let mapped = self.map_zone(raw.clone())?;
        if mapped.zone != *zone {
            return Err(validation("google_zone_identity_mismatch"));
        }
        Ok(raw)
    }

    async fn all_records_with_raw(
        &self,
        zone: &DnsZoneRef,
    ) -> Result<Vec<(ObservedDnsRecordSet, GoogleResourceRecordSet)>> {
        let zone_metadata = self.checked_zone(zone).await?;
        let mut token = None::<String>;
        let mut seen = BTreeSet::new();
        let mut keys = BTreeSet::new();
        let mut out = Vec::new();
        for _ in 0..MAX_PROVIDER_PAGES {
            let page = self
                .api
                .list_record_sets(zone.zone_id.as_str(), token.as_deref(), PROVIDER_PAGE_SIZE)
                .await?;
            validate_page(&page.next_page_token, page.items.len())?;
            for raw in page.items {
                if zone_metadata.dnssec_state != GoogleDnsSecState::Off {
                    validate_dnssec_record(&raw)?;
                }
                let (record_set, revision) = model::map_record_set(zone, raw.clone())?;
                if !keys.insert(record_set.key.clone()) {
                    return Err(validation("duplicate_google_record_identity"));
                }
                out.push((
                    ObservedDnsRecordSet {
                        zone: zone.clone(),
                        record_set,
                        provider_object_ids: BTreeSet::<DnsRecordObjectId>::new(),
                        revision,
                    },
                    raw,
                ));
                if out.len() > MAX_RECORDS {
                    return Err(validation("google_record_inventory_limit"));
                }
            }
            let Some(next) = page.next_page_token else {
                out.sort_by(|a, b| a.0.record_set.key.cmp(&b.0.record_set.key));
                return Ok(out);
            };
            if token.as_deref() == Some(&next) || !seen.insert(next.clone()) {
                return Err(validation("google_record_pagination_loop"));
            }
            token = Some(next);
        }
        Err(validation("google_record_pagination_limit"))
    }

    fn scope(&self, method: CursorMethod) -> CursorScope {
        CursorScope {
            center_account_id: self.center_account_id.as_str().into(),
            project_id: self.project_id.clone(),
            method,
        }
    }
    fn receipt(
        &self,
        zone: &DnsZoneRef,
        change: &GoogleChange,
        digest: String,
    ) -> Result<DnsChangeReceipt> {
        if change.id.is_empty() || change.id.len() > 512 || change.start_time.is_empty() {
            return Err(unknown_outcome("invalid_google_change_response"));
        }
        let token = ChangeToken {
            version: 1,
            center_scope: scope_hash(self.center_account_id.as_str()),
            project_scope: scope_hash(&self.project_id),
            zone_scope: scope_hash(zone.zone_id.as_str()),
            provider_change_id: change.id.clone(),
            request_digest: digest,
            guard: DnsGuardStrength::Atomic,
        };
        let id = DnsChangeId::new(sign_token(&token, &self.cursor_key)?)
            .map_err(|_| unknown_outcome("google_receipt_encoding_failed"))?;
        change_receipt(id, &change.status, DnsGuardStrength::Atomic)
    }
    fn decode_receipt(&self, zone: &DnsZoneRef, id: &DnsChangeId) -> Result<ChangeToken> {
        id.validate()
            .map_err(|_| not_found("google_change_not_found"))?;
        let t: ChangeToken = verify_token(id.as_str(), &self.cursor_key)
            .map_err(|_| not_found("google_change_not_found"))?;
        if t.version != 1
            || t.center_scope != scope_hash(self.center_account_id.as_str())
            || t.project_scope != scope_hash(&self.project_id)
            || t.zone_scope != scope_hash(zone.zone_id.as_str())
            || t.guard != DnsGuardStrength::Atomic
        {
            return Err(not_found("google_change_not_found"));
        }
        Ok(t)
    }
}

#[async_trait]
impl DnsProvider for GoogleCloudDnsAdapter {
    async fn get_zone(&self, zone: &DnsZoneRef) -> DnsProviderResult<Option<ObservedDnsZone>> {
        self.validate_zone_ref(zone)?;
        let Some(raw) = self.api.get_managed_zone(zone.zone_id.as_str()).await? else {
            return Ok(None);
        };
        let mapped = self.map_zone(raw)?;
        if mapped.zone != *zone {
            return Err(validation("google_zone_identity_mismatch"));
        }
        Ok(Some(mapped))
    }
    async fn list_zones(
        &self,
        account: &CloudResourceId,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<DnsPage<ObservedDnsZone>> {
        account
            .validate()
            .map_err(|_| validation("invalid_account"))?;
        page.validate().map_err(|_| validation("invalid_page"))?;
        if account != &self.center_account_id {
            return Err(validation("google_account_scope_mismatch"));
        }
        paginate(
            self.all_zones().await?,
            page,
            self.scope(CursorMethod::Zones),
            &self.cursor_key,
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
            .all_records_with_raw(zone)
            .await?
            .into_iter()
            .map(|v| v.0)
            .find(|v| &v.record_set.key == key))
    }
    async fn list_record_sets(
        &self,
        zone: &DnsZoneRef,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<DnsPage<ObservedDnsRecordSet>> {
        self.validate_zone_ref(zone)?;
        page.validate().map_err(|_| validation("invalid_page"))?;
        let values = self
            .all_records_with_raw(zone)
            .await?
            .into_iter()
            .map(|v| v.0)
            .collect();
        paginate(
            values,
            page,
            self.scope(CursorMethod::Records {
                zone_id: zone.zone_id.as_str().into(),
            }),
            &self.cursor_key,
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
        let zone_metadata = self.checked_zone(zone).await?;
        if zone_metadata.dnssec_state != GoogleDnsSecState::Off
            && changes.iter().any(|change| {
                let desired = match change {
                    DnsRecordChange::Create { record_set, .. }
                    | DnsRecordChange::Replace {
                        desired: record_set,
                        ..
                    } => Some(record_set),
                    DnsRecordChange::Delete { .. } => None,
                };
                desired.is_some_and(|record| {
                    matches!(
                        record.extension,
                        Some(edgion_center_core::DnsRecordExtension::GoogleAlias { .. })
                    )
                })
            })
        {
            return Err(validation("google_alias_dnssec_unsupported"));
        }
        let current = self.all_records_with_raw(zone).await?;
        let request = plan_changes(zone, changes, current)?;
        if zone_metadata.dnssec_state != GoogleDnsSecState::Off {
            for addition in &request.additions {
                validate_dnssec_record(addition)?;
            }
        }
        let digest = semantic_change_digest(&request)?;
        let response = self
            .api
            .create_change(zone.zone_id.as_str(), &request)
            .await?;
        let echoed = GoogleChangeRequest {
            additions: response.additions.clone(),
            deletions: response.deletions.clone(),
        };
        if semantic_change_digest(&echoed)? != digest {
            return Err(unknown_outcome("google_change_echo_mismatch"));
        }
        let receipt = self.receipt(zone, &response, digest)?;
        receipt
            .validate_against_request(minimum_guard)
            .map_err(|_| unknown_outcome("invalid_google_change_receipt"))?;
        Ok(receipt)
    }
    async fn observe_change(
        &self,
        zone: &DnsZoneRef,
        id: &DnsChangeId,
    ) -> DnsProviderResult<DnsChangeReceipt> {
        self.validate_zone_ref(zone)?;
        let token = self.decode_receipt(zone, id)?;
        let change = self
            .api
            .get_change(zone.zone_id.as_str(), &token.provider_change_id)
            .await?
            .ok_or_else(|| not_found("google_change_not_found"))?;
        if change.id != token.provider_change_id {
            return Err(validation("google_change_identity_mismatch"));
        }
        let observed_request = GoogleChangeRequest {
            additions: change.additions.clone(),
            deletions: change.deletions.clone(),
        };
        if semantic_change_digest(&observed_request)? != token.request_digest {
            return Err(validation("google_change_payload_mismatch"));
        }
        change_receipt(id.clone(), &change.status, token.guard)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LifecycleOperation {
    Create,
    Delete,
    EnableDnssec,
}

#[derive(Serialize, Deserialize)]
struct LifecycleToken {
    version: u8,
    center_scope: String,
    project_scope: String,
    zone_scope: String,
    operation: LifecycleOperation,
}

#[async_trait]
impl ZoneLifecycleProvider for GoogleCloudDnsAdapter {
    async fn create_zone(
        &self,
        request: &ZoneCreationRequest,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        if request.provider != CloudProvider::GoogleCloud
            || request.provider_account_id != self.center_account_id
        {
            return Err(validation("google_zone_create_scope_mismatch"));
        }
        if request.visibility != ZoneVisibility::Public {
            return Err(validation("google_private_zone_network_required"));
        }
        let digest =
            URL_SAFE_NO_PAD.encode(Sha256::digest(request.idempotency_key.as_str().as_bytes()));
        let name = format!("center-{}", digest[..24].to_ascii_lowercase());
        if let Some(existing) = self.api.get_managed_zone(&name).await? {
            let mapped = self.map_zone(existing)?;
            if mapped.zone.apex != request.apex || mapped.zone.visibility != request.visibility {
                return Err(conflict("google_zone_create_idempotency_conflict"));
            }
            return self.lifecycle_receipt(
                mapped.zone.zone_id.as_str(),
                LifecycleOperation::Create,
                ZoneLifecycleMutationState::Succeeded,
            );
        }
        let raw = self
            .api
            .create_managed_zone(&GoogleManagedZoneCreate {
                name,
                dns_name: request.apex.as_str().to_string(),
                visibility: GoogleZoneVisibility::Public,
                dnssec_state: GoogleDnsSecState::Off,
            })
            .await?;
        let mapped = self.map_zone(raw)?;
        if mapped.zone.apex != request.apex || mapped.zone.visibility != request.visibility {
            return Err(unknown_outcome("google_zone_create_identity_mismatch"));
        }
        self.lifecycle_receipt(
            mapped.zone.zone_id.as_str(),
            LifecycleOperation::Create,
            ZoneLifecycleMutationState::Succeeded,
        )
    }

    async fn observe_zone(
        &self,
        zone: &DnsZoneRef,
    ) -> ZoneLifecycleProviderResult<Option<ZoneLifecycleObservation>> {
        self.observe_lifecycle(zone).await
    }

    async fn set_dnssec(
        &self,
        zone: &DnsZoneRef,
        desired: DnssecDesiredState,
        expected_revision: &ZoneLifecycleRevision,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        let current = self
            .observe_lifecycle(zone)
            .await?
            .ok_or_else(|| not_found("google_zone_not_found"))?;
        if &current.revision != expected_revision {
            return Err(conflict("google_zone_lifecycle_revision_conflict"));
        }
        if zone.visibility != ZoneVisibility::Public {
            return Err(validation("google_private_dnssec_unsupported"));
        }
        match desired {
            DnssecDesiredState::Enabled => {
                if !matches!(current.dnssec.state, DnssecProviderState::Disabled) {
                    return self.lifecycle_receipt(
                        zone.zone_id.as_str(),
                        LifecycleOperation::EnableDnssec,
                        ZoneLifecycleMutationState::Succeeded,
                    );
                }
                let updated = self
                    .api
                    .set_managed_zone_dnssec(zone.zone_id.as_str(), GoogleDnsSecState::On)
                    .await?;
                if updated.id != zone.zone_id.as_str()
                    || updated.dnssec_state == GoogleDnsSecState::Off
                {
                    return Err(unknown_outcome("google_dnssec_enable_response_mismatch"));
                }
                self.lifecycle_receipt(
                    zone.zone_id.as_str(),
                    LifecycleOperation::EnableDnssec,
                    ZoneLifecycleMutationState::Succeeded,
                )
            }
            DnssecDesiredState::Disabled => {
                if matches!(current.dnssec.state, DnssecProviderState::Disabled) {
                    return self.lifecycle_receipt(
                        zone.zone_id.as_str(),
                        LifecycleOperation::EnableDnssec,
                        ZoneLifecycleMutationState::Succeeded,
                    );
                }
                Err(conflict("google_parent_ds_removal_verification_required"))
            }
        }
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
            return Err(validation("google_zone_delete_approval_invalid"));
        }
        let current = self
            .observe_lifecycle(request.zone())
            .await?
            .ok_or_else(|| not_found("google_zone_not_found"))?;
        if &current.revision != request.revision() {
            return Err(conflict("google_zone_lifecycle_revision_conflict"));
        }
        if current.non_default_record_count != 0
            || !matches!(
                current.dnssec.state,
                DnssecProviderState::Disabled | DnssecProviderState::Unsupported
            )
        {
            return Err(conflict("google_zone_delete_precondition_failed"));
        }
        self.api
            .delete_managed_zone(request.zone().zone_id.as_str())
            .await?;
        self.lifecycle_receipt(
            request.zone().zone_id.as_str(),
            LifecycleOperation::Delete,
            ZoneLifecycleMutationState::Succeeded,
        )
    }

    async fn observe_mutation(
        &self,
        mutation_id: &ZoneLifecycleMutationId,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        let token: LifecycleToken = verify_token(mutation_id.as_str(), &self.cursor_key)
            .map_err(|_| not_found("google_lifecycle_mutation_not_found"))?;
        if token.version != 1
            || token.center_scope != scope_hash(self.center_account_id.as_str())
            || token.project_scope != scope_hash(&self.project_id)
        {
            return Err(not_found("google_lifecycle_mutation_not_found"));
        }
        Ok(ZoneLifecycleMutationReceipt {
            mutation_id: mutation_id.clone(),
            state: ZoneLifecycleMutationState::Succeeded,
        })
    }
}

impl GoogleCloudDnsAdapter {
    fn lifecycle_receipt(
        &self,
        zone_id: &str,
        operation: LifecycleOperation,
        state: ZoneLifecycleMutationState,
    ) -> Result<ZoneLifecycleMutationReceipt> {
        let token = LifecycleToken {
            version: 1,
            center_scope: scope_hash(self.center_account_id.as_str()),
            project_scope: scope_hash(&self.project_id),
            zone_scope: scope_hash(zone_id),
            operation,
        };
        let mutation_id = ZoneLifecycleMutationId::new(sign_token(&token, &self.cursor_key)?)
            .map_err(|_| unknown_outcome("google_lifecycle_receipt_encoding_failed"))?;
        Ok(ZoneLifecycleMutationReceipt { mutation_id, state })
    }

    async fn observe_lifecycle(
        &self,
        zone: &DnsZoneRef,
    ) -> Result<Option<ZoneLifecycleObservation>> {
        self.validate_zone_ref(zone)?;
        let Some(raw) = self.api.get_managed_zone(zone.zone_id.as_str()).await? else {
            return Ok(None);
        };
        let raw_for_revision = raw.clone();
        let mapped = self.map_zone(raw.clone())?;
        if mapped.zone != *zone {
            return Err(validation("google_zone_identity_mismatch"));
        }
        let nameservers = raw
            .name_servers
            .iter()
            .map(|value| {
                AbsoluteDnsName::new(value).map_err(|_| validation("invalid_google_nameserver"))
            })
            .collect::<Result<BTreeSet<_>>>()?;
        if zone.visibility == ZoneVisibility::Public && nameservers.is_empty() {
            return Err(validation("missing_google_nameservers"));
        }
        let records = self.all_records_with_raw(zone).await?;
        let non_default_record_count = records
            .iter()
            .filter(|(_, raw)| {
                !(raw
                    .name
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case(zone.apex.as_str())
                    && matches!(raw.record_type.as_str(), "NS" | "SOA"))
            })
            .count() as u64;
        let dnssec = match raw.dnssec_state {
            GoogleDnsSecState::Off => DnssecObservation {
                state: DnssecProviderState::Disabled,
                ds_records: Vec::new(),
                external_action: DnssecExternalAction::None,
                provider_detail: Some("off".into()),
            },
            GoogleDnsSecState::On | GoogleDnsSecState::Transfer => {
                let keys = self.api.list_dns_keys(zone.zone_id.as_str()).await?;
                let mut ds_records = Vec::new();
                for key in keys.into_iter().filter(|key| key.key_type == "keySigning") {
                    for digest in key.digests {
                        let digest_type = match digest.digest_type.as_str() {
                            "sha1" => 1,
                            "sha256" => 2,
                            "sha384" => 4,
                            _ => return Err(validation("unknown_google_dns_key_digest")),
                        };
                        let record = DnssecDsRecord {
                            key_tag: key.key_tag,
                            algorithm: key.algorithm,
                            digest_type,
                            digest: digest.digest.to_ascii_uppercase(),
                        };
                        record
                            .validate()
                            .map_err(|_| validation("invalid_google_ds_record"))?;
                        ds_records.push(record);
                    }
                }
                if ds_records.is_empty() {
                    return Err(validation("missing_google_key_signing_ds"));
                }
                DnssecObservation {
                    state: DnssecProviderState::AwaitingDs,
                    external_action: DnssecExternalAction::PublishDs {
                        records: ds_records.clone(),
                    },
                    ds_records,
                    provider_detail: Some(
                        match raw.dnssec_state {
                            GoogleDnsSecState::On => "on",
                            GoogleDnsSecState::Transfer => "transfer",
                            GoogleDnsSecState::Off => unreachable!(),
                        }
                        .into(),
                    ),
                }
            }
        };
        let revision_bytes = serde_json::to_vec(&(raw_for_revision, &records, &dnssec))
            .map_err(|_| validation("google_lifecycle_revision_encoding_failed"))?;
        let revision =
            ZoneLifecycleRevision::new(URL_SAFE_NO_PAD.encode(Sha256::digest(revision_bytes)))
                .map_err(|_| validation("invalid_google_lifecycle_revision"))?;
        let delegation = if zone.visibility == ZoneVisibility::Private {
            DelegationObservation {
                state: DelegationState::NotApplicable,
                expected_nameservers: nameservers.clone(),
                parent_nameservers: BTreeSet::new(),
                checked_at: None,
                failure: None,
            }
        } else {
            DelegationObservation {
                state: DelegationState::NotChecked,
                expected_nameservers: nameservers.clone(),
                parent_nameservers: BTreeSet::new(),
                checked_at: None,
                failure: None,
            }
        };
        let authoritative_verification = AuthoritativeDnsVerification::NotChecked;
        let observation = ZoneLifecycleObservation {
            zone: zone.clone(),
            revision,
            authoritative_nameservers: nameservers,
            delegation,
            readiness: evaluate_zone_readiness(&authoritative_verification),
            authoritative_verification,
            dnssec,
            non_default_record_count,
        };
        observation
            .validate()
            .map_err(|_| validation("invalid_google_lifecycle_observation"))?;
        Ok(Some(observation))
    }
}

fn semantic_change_digest(request: &GoogleChangeRequest) -> Result<String> {
    let mut request = request.clone();
    for record in request
        .additions
        .iter_mut()
        .chain(request.deletions.iter_mut())
    {
        strip_output_only_record_fields(record);
    }
    let bytes =
        serde_json::to_vec(&request).map_err(|_| validation("google_change_encoding_failed"))?;
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(bytes)))
}

fn strip_output_only_record_fields(record: &mut GoogleResourceRecordSet) {
    record.signature_rrdatas.clear();
    record.extra.remove("kind");
    let Some(policy) = record.routing_policy.as_mut() else {
        return;
    };
    policy.extra.remove("kind");
    fn targets(value: &mut GoogleHealthCheckTargets) {
        value.extra.remove("kind");
        for target in &mut value.internal_load_balancers {
            target.extra.remove("kind");
        }
    }
    fn geo(value: &mut GoogleGeoPolicy) {
        value.extra.remove("kind");
        for item in &mut value.items {
            item.signature_rrdatas.clear();
            item.extra.remove("kind");
            if let Some(value) = item.health_checked_targets.as_mut() {
                targets(value);
            }
        }
    }
    match &mut policy.routing_data {
        GoogleRoutingData::Geo { geo: value } => geo(value),
        GoogleRoutingData::Wrr { wrr } => {
            wrr.extra.remove("kind");
            for item in &mut wrr.items {
                item.signature_rrdatas.clear();
                item.extra.remove("kind");
                if let Some(value) = item.health_checked_targets.as_mut() {
                    targets(value);
                }
            }
        }
        GoogleRoutingData::PrimaryBackup { primary_backup } => {
            primary_backup.extra.remove("kind");
            targets(&mut primary_backup.primary_targets);
            geo(&mut primary_backup.backup_geo_targets);
        }
    }
}

fn validate_dnssec_record(record: &GoogleResourceRecordSet) -> Result<()> {
    if record.record_type == "ALIAS" {
        return Err(validation("google_alias_dnssec_unsupported"));
    }
    let Some(policy) = record.routing_policy.as_ref() else {
        return Ok(());
    };
    fn target_count(targets: &GoogleHealthCheckTargets) -> usize {
        targets.external_endpoints.len() + targets.internal_load_balancers.len()
    }
    fn geo(policy: &GoogleGeoPolicy) -> Result<()> {
        for item in &policy.items {
            if item
                .health_checked_targets
                .as_ref()
                .is_some_and(|targets| target_count(targets) > 1)
            {
                return Err(validation("google_dnssec_health_target_limit"));
            }
        }
        Ok(())
    }
    match &policy.routing_data {
        GoogleRoutingData::Geo { geo: policy } => geo(policy),
        GoogleRoutingData::Wrr { wrr } => {
            for item in &wrr.items {
                if let Some(targets) = item.health_checked_targets.as_ref() {
                    if !item.rrdatas.is_empty() || target_count(targets) > 1 {
                        return Err(validation("google_dnssec_wrr_target_conflict"));
                    }
                }
            }
            Ok(())
        }
        GoogleRoutingData::PrimaryBackup { primary_backup } => {
            if target_count(&primary_backup.primary_targets) > 1 {
                return Err(validation("google_dnssec_health_target_limit"));
            }
            geo(&primary_backup.backup_geo_targets)
        }
    }
}

fn plan_changes(
    zone: &DnsZoneRef,
    changes: &[DnsRecordChange],
    current: Vec<(ObservedDnsRecordSet, GoogleResourceRecordSet)>,
) -> Result<GoogleChangeRequest> {
    let current = current
        .into_iter()
        .map(|(o, r)| (o.record_set.key.clone(), (o, r)))
        .collect::<BTreeMap<_, _>>();
    let mut additions = Vec::new();
    let mut deletions = Vec::new();
    for change in changes {
        match change {
            DnsRecordChange::Create { record_set, .. } => {
                if current.contains_key(&record_set.key) {
                    return Err(conflict("google_create_guard_conflict"));
                }
                additions.push(model::render_record_set(zone, record_set)?);
            }
            DnsRecordChange::Replace {
                previous, desired, ..
            } => {
                let (observed, raw) = current
                    .get(&desired.key)
                    .ok_or_else(|| conflict("google_replace_guard_conflict"))?;
                if !same_observation(observed, previous) {
                    return Err(conflict("google_replace_guard_conflict"));
                }
                if previous.record_set == *desired {
                    return Err(validation("google_noop_replace"));
                }
                deletions.push(raw.clone());
                additions.push(model::render_record_set(zone, desired)?);
            }
            DnsRecordChange::Delete { previous, .. } => {
                let (observed, raw) = current
                    .get(&previous.record_set.key)
                    .ok_or_else(|| conflict("google_delete_guard_conflict"))?;
                if !same_observation(observed, previous) {
                    return Err(conflict("google_delete_guard_conflict"));
                }
                deletions.push(raw.clone());
            }
        }
    }
    if additions.len() > MAX_CHANGE_RRSETS
        || deletions.len() > MAX_CHANGE_RRSETS
        || additions.is_empty() && deletions.is_empty()
    {
        return Err(validation("google_change_rrset_limit"));
    }
    Ok(GoogleChangeRequest {
        additions,
        deletions,
    })
}
fn same_observation(a: &ObservedDnsRecordSet, b: &ObservedDnsRecordSet) -> bool {
    a.zone == b.zone && a.record_set == b.record_set && a.revision == b.revision
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CursorMethod {
    Zones,
    Records { zone_id: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorScope {
    center_account_id: String,
    project_id: String,
    method: CursorMethod,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorToken {
    version: u8,
    scope: CursorScope,
    offset: usize,
    inventory_digest: String,
}
#[derive(Serialize, Deserialize)]
struct ChangeToken {
    version: u8,
    center_scope: String,
    project_scope: String,
    zone_scope: String,
    provider_change_id: String,
    request_digest: String,
    guard: DnsGuardStrength,
}

fn paginate<T: Clone + Serialize>(
    items: Vec<T>,
    page: &DnsPageRequest,
    scope: CursorScope,
    key: &GoogleCloudDnsCursorKey,
) -> Result<DnsPage<T>> {
    let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(
        serde_json::to_vec(&items).map_err(|_| validation("google_inventory_encoding_failed"))?,
    ));
    let offset = match &page.token {
        Some(value) => {
            let t: CursorToken = verify_token(value.as_str(), key)
                .map_err(|_| validation("invalid_google_page_token"))?;
            if t.version != 1
                || t.scope != scope
                || t.inventory_digest != digest
                || t.offset >= items.len()
            {
                return Err(validation("invalid_google_page_token"));
            }
            t.offset
        }
        None => 0,
    };
    let end = offset.saturating_add(page.limit.into()).min(items.len());
    let next = if end < items.len() {
        Some(
            DnsPageToken::new(sign_token(
                &CursorToken {
                    version: 1,
                    scope,
                    offset: end,
                    inventory_digest: digest,
                },
                key,
            )?)
            .map_err(|_| validation("google_page_token_encoding_failed"))?,
        )
    } else {
        None
    };
    let result = DnsPage {
        items: items[offset..end].to_vec(),
        next,
    };
    result
        .validate(page.limit, |_| Ok(()))
        .map_err(|_| validation("invalid_google_page"))?;
    Ok(result)
}
fn sign_token<T: Serialize>(value: &T, key: &GoogleCloudDnsCursorKey) -> Result<String> {
    let encoded =
        serde_json::to_vec(value).map_err(|_| validation("google_token_encoding_failed"))?;
    let sig = HmacSha256::new_from_slice(&key.0)
        .expect("fixed HMAC key")
        .chain_update(&encoded)
        .finalize()
        .into_bytes();
    let mut out = encoded;
    out.extend_from_slice(&sig);
    Ok(URL_SAFE_NO_PAD.encode(out))
}
fn verify_token<T: DeserializeOwned>(value: &str, key: &GoogleCloudDnsCursorKey) -> Result<T> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| validation("invalid_google_token"))?;
    if bytes.len() <= 32 {
        return Err(validation("invalid_google_token"));
    }
    let (encoded, sig) = bytes.split_at(bytes.len() - 32);
    HmacSha256::new_from_slice(&key.0)
        .expect("fixed HMAC key")
        .chain_update(encoded)
        .verify_slice(sig)
        .map_err(|_| validation("invalid_google_token"))?;
    serde_json::from_slice(encoded).map_err(|_| validation("invalid_google_token"))
}
fn validate_page(token: &Option<String>, len: usize) -> Result<()> {
    if len > PROVIDER_PAGE_SIZE.into()
        || token
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 4096 || v.chars().any(char::is_control))
    {
        return Err(validation("invalid_google_provider_page"));
    }
    Ok(())
}
fn scope_hash(v: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(v.as_bytes()))
}
fn change_receipt(
    id: DnsChangeId,
    status: &str,
    guard: DnsGuardStrength,
) -> Result<DnsChangeReceipt> {
    let (state, propagation) = match status {
        "pending" => (DnsChangeState::Pending, DnsPropagationState::Pending),
        "done" => (
            DnsChangeState::ProviderCommitted,
            DnsPropagationState::ProviderReportedApplied,
        ),
        _ => return Err(validation("unsupported_google_change_status")),
    };
    let r = DnsChangeReceipt {
        id,
        state,
        submission_atomicity: DnsBatchAtomicity::AllOrNothing,
        propagation,
        guard_strength: guard,
    };
    r.validate()
        .map_err(|_| validation("invalid_google_change_receipt"))?;
    Ok(r)
}
pub(crate) fn validation(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Validation, code)
}
fn not_found(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::NotFound, code)
}
fn conflict(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Conflict, code)
}
fn unknown_outcome(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::UnknownOutcome, code)
}
fn provider_error(category: ProviderErrorCategory, code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        code,
        "Google Cloud DNS adapter rejected the request",
        None,
        None,
    )
    .expect("static normalized provider error")
}

#[cfg(test)]
mod tests;
