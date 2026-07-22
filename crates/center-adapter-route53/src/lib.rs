//! Amazon Route 53 DNS adapter.
//!
//! The crate is independent of SQL, Kubernetes, federation, Admin API, and
//! Edgion resources. A composition root injects a credential-owning API client
//! whose AWS account identity was verified with STS before construction.

mod api;
mod aws_sdk;
mod model;

pub use api::{
    Route53AliasTargetData, Route53Api, Route53ApiResult, Route53ChangeAction, Route53ChangeBatch,
    Route53ChangeInfo, Route53CreateHostedZoneRequest, Route53CreateHostedZoneResult,
    Route53DnssecInfo, Route53GeoLocationData, Route53HostedZone, Route53HostedZonePage,
    Route53KeySigningKey, Route53RecordChange, Route53RecordCursor, Route53RecordPage,
    Route53RecordSet,
};
pub use aws_sdk::{
    AwsAssumeRoleSpec, AwsRoute53Api, AwsRoute53ApiOptions, AwsRoute53SdkConfigFactory,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    evaluate_zone_readiness, validate_dns_changes, AbsoluteDnsName, AuthoritativeDnsVerification,
    CloudProvider, CloudResourceId, DelegationObservation, DelegationState, DnsBatchAtomicity,
    DnsChangeId, DnsChangeReceipt, DnsChangeState, DnsGuardStrength, DnsPage, DnsPageRequest,
    DnsPageToken, DnsPropagationState, DnsProvider, DnsProviderResult, DnsRecordChange,
    DnsRecordObjectId, DnsRecordSetKey, DnsZoneId, DnsZoneRef, DnssecDesiredState, DnssecDsRecord,
    DnssecExternalAction, DnssecObservation, DnssecProviderState, NormalizedProviderError,
    ObservedDnsRecordSet, ObservedDnsZone, ProviderAccountScope, ProviderAccountSpec,
    ProviderDnsRecordSet, ProviderDnsRecordType, ProviderErrorCategory, Route53GeoLocation,
    Route53RoutingPolicy, ZoneCreationRequest, ZoneDeletionRequest, ZoneLifecycleMutationId,
    ZoneLifecycleMutationReceipt, ZoneLifecycleMutationState, ZoneLifecycleObservation,
    ZoneLifecycleProvider, ZoneLifecycleProviderResult, ZoneLifecycleRevision, ZoneVisibility,
};
use hmac::{Hmac, Mac};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

const HOSTED_ZONE_PAGE_SIZE: u16 = 100;
const RECORD_PAGE_SIZE: u16 = 300;
const MAX_PROVIDER_PAGES: usize = 10_000;
const MAX_INVENTORY_ZONES: usize = 10_000;
const MAX_INVENTORY_RECORDS: usize = 100_000;
const MAX_PROVIDER_CHANGE_ACTIONS: usize = 1_000;
const MAX_PROVIDER_RECORD_ELEMENTS: usize = 1_000;
const MAX_PROVIDER_RECORD_VALUE_BYTES: usize = 32_000;
type HmacSha256 = Hmac<Sha256>;

pub type Result<T> = std::result::Result<T, NormalizedProviderError>;

/// Stable composition-provided key for authenticated local inventory cursors.
pub struct Route53CursorKey([u8; 32]);

impl Route53CursorKey {
    pub fn new(value: [u8; 32]) -> Result<Self> {
        if value.iter().all(|byte| *byte == 0) {
            return Err(validation("weak_route53_cursor_key"));
        }
        Ok(Self(value))
    }
}

impl Drop for Route53CursorKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Stable composition-provided key for authenticated RRset mutation receipts.
pub struct Route53MutationReceiptKey([u8; 32]);

impl Route53MutationReceiptKey {
    pub fn new(value: [u8; 32]) -> Result<Self> {
        if value.iter().all(|byte| *byte == 0) {
            return Err(validation("weak_route53_mutation_receipt_key"));
        }
        Ok(Self(value))
    }
}

impl Drop for Route53MutationReceiptKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Closed-purpose, no-I/O verifier for Route 53 RRset mutation receipts.
///
/// The verifier owns its key, is intentionally neither cloneable nor serializable, and never
/// exposes the authenticated provider change identity.
pub struct Route53MutationReceiptVerifier {
    center_account_id: CloudResourceId,
    aws_account_id: String,
    key: Route53MutationReceiptKey,
}

impl Route53MutationReceiptVerifier {
    pub fn new(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        key: Route53MutationReceiptKey,
    ) -> Result<Self> {
        center_account_id
            .validate()
            .map_err(|_| validation("invalid_center_account_id"))?;
        account
            .validate()
            .map_err(|_| validation("invalid_provider_account"))?;
        if account.provider != CloudProvider::Aws {
            return Err(validation("route53_provider_required"));
        }
        let ProviderAccountScope::Aws { account_id } = account
            .scope
            .as_ref()
            .ok_or_else(|| validation("route53_account_scope_required"))?
        else {
            return Err(validation("route53_account_scope_mismatch"));
        };
        Ok(Self {
            center_account_id,
            aws_account_id: account_id.clone(),
            key,
        })
    }

    pub fn validate_scope(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        change_id: &DnsChangeId,
    ) -> DnsProviderResult<()> {
        self.decode_scope(account_id, zone_id, change_id)
            .map(|_| ())
    }

    fn decode_scope(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        change_id: &DnsChangeId,
    ) -> DnsProviderResult<ChangeToken> {
        account_id
            .validate()
            .map_err(|_| not_found("route53_change_not_found"))?;
        zone_id
            .validate()
            .map_err(|_| not_found("route53_change_not_found"))?;
        change_id
            .validate()
            .map_err(|_| not_found("route53_change_not_found"))?;
        let token: ChangeToken = verify_token(
            change_id.as_str(),
            &self.key.0,
            TokenDomain::MutationReceipt,
        )
        .map_err(|_| not_found("route53_change_not_found"))?;
        if token.version != 1
            || account_id != &self.center_account_id
            || normalize_zone_id(zone_id.as_str()).ok().as_deref() != Some(zone_id.as_str())
            || token.center_scope != scope_hash(account_id.as_str())
            || token.external_scope != scope_hash(&self.aws_account_id)
            || token.zone_scope != scope_hash(zone_id.as_str())
            || token.guard != DnsGuardStrength::Atomic
            || normalize_change_id(&token.provider_change_id)
                .ok()
                .as_deref()
                != Some(token.provider_change_id.as_str())
        {
            return Err(not_found("route53_change_not_found"));
        }
        Ok(token)
    }

    fn matches(&self, center_account_id: &CloudResourceId, account: &ProviderAccountSpec) -> bool {
        account.provider == CloudProvider::Aws
            && &self.center_account_id == center_account_id
            && matches!(
                account.scope.as_ref(),
                Some(ProviderAccountScope::Aws { account_id }) if account_id == &self.aws_account_id
            )
    }
}

/// Stable composition-provided key for authenticated hosted-zone lifecycle tokens.
pub struct Route53LifecycleTokenKey([u8; 32]);

impl Route53LifecycleTokenKey {
    pub fn new(value: [u8; 32]) -> Result<Self> {
        if value.iter().all(|byte| *byte == 0) {
            return Err(validation("weak_route53_lifecycle_token_key"));
        }
        Ok(Self(value))
    }
}

impl Drop for Route53LifecycleTokenKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub struct Route53DnsAdapter {
    center_account_id: CloudResourceId,
    aws_account_id: String,
    api: Arc<dyn Route53Api>,
    cursor_key: Route53CursorKey,
    mutation_receipt_verifier: Option<Route53MutationReceiptVerifier>,
    lifecycle_token_key: Option<Route53LifecycleTokenKey>,
}

impl Route53DnsAdapter {
    pub fn new(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn Route53Api>,
        cursor_key: Route53CursorKey,
    ) -> Result<Self> {
        Self::new_inner(center_account_id, account, api, cursor_key, None, None)
    }

    /// Backward-compatible explicit name for the inventory-only constructor.
    pub fn new_read_only(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn Route53Api>,
        cursor_key: Route53CursorKey,
    ) -> Result<Self> {
        Self::new(center_account_id, account, api, cursor_key)
    }

    /// Construct a write-capable adapter with three distinct closed-purpose keys.
    pub fn new_with_write_keys(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn Route53Api>,
        cursor_key: Route53CursorKey,
        mutation_receipt_key: Route53MutationReceiptKey,
        lifecycle_token_key: Route53LifecycleTokenKey,
    ) -> Result<Self> {
        if cursor_key.0 == mutation_receipt_key.0
            || cursor_key.0 == lifecycle_token_key.0
            || mutation_receipt_key.0 == lifecycle_token_key.0
        {
            return Err(validation("route53_signing_key_reuse"));
        }
        let mutation_receipt_verifier = Route53MutationReceiptVerifier::new(
            center_account_id.clone(),
            account,
            mutation_receipt_key,
        )?;
        Self::new_inner(
            center_account_id,
            account,
            api,
            cursor_key,
            Some(mutation_receipt_verifier),
            Some(lifecycle_token_key),
        )
    }

    /// Construct an adapter that can mutate RRsets but has no hosted-zone lifecycle authority.
    pub fn new_with_record_write_key(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn Route53Api>,
        cursor_key: Route53CursorKey,
        mutation_receipt_key: Route53MutationReceiptKey,
    ) -> Result<Self> {
        if cursor_key.0 == mutation_receipt_key.0 {
            return Err(validation("route53_signing_key_reuse"));
        }
        let mutation_receipt_verifier = Route53MutationReceiptVerifier::new(
            center_account_id.clone(),
            account,
            mutation_receipt_key,
        )?;
        Self::new_with_record_write_verifier(
            center_account_id,
            account,
            api,
            cursor_key,
            mutation_receipt_verifier,
        )
    }

    /// Construct an RRset writer from a receipt verifier that may have authenticated a request
    /// before the provider API client was created.
    pub fn new_with_record_write_verifier(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn Route53Api>,
        cursor_key: Route53CursorKey,
        mutation_receipt_verifier: Route53MutationReceiptVerifier,
    ) -> Result<Self> {
        if cursor_key.0 == mutation_receipt_verifier.key.0 {
            return Err(validation("route53_signing_key_reuse"));
        }
        if !mutation_receipt_verifier.matches(&center_account_id, account) {
            return Err(validation("route53_mutation_receipt_scope_mismatch"));
        }
        Self::new_inner(
            center_account_id,
            account,
            api,
            cursor_key,
            Some(mutation_receipt_verifier),
            None,
        )
    }

    fn new_inner(
        center_account_id: CloudResourceId,
        account: &ProviderAccountSpec,
        api: Arc<dyn Route53Api>,
        cursor_key: Route53CursorKey,
        mutation_receipt_verifier: Option<Route53MutationReceiptVerifier>,
        lifecycle_token_key: Option<Route53LifecycleTokenKey>,
    ) -> Result<Self> {
        center_account_id
            .validate()
            .map_err(|_| validation("invalid_center_account_id"))?;
        account
            .validate()
            .map_err(|_| validation("invalid_provider_account"))?;
        if account.provider != CloudProvider::Aws {
            return Err(validation("route53_provider_required"));
        }
        let ProviderAccountScope::Aws { account_id } = account
            .scope
            .as_ref()
            .ok_or_else(|| validation("route53_account_scope_required"))?
        else {
            return Err(validation("route53_account_scope_mismatch"));
        };
        if api.verified_account_id() != account_id {
            return Err(validation("route53_verified_account_mismatch"));
        }
        Ok(Self {
            center_account_id,
            aws_account_id: account_id.clone(),
            api,
            cursor_key,
            mutation_receipt_verifier,
            lifecycle_token_key,
        })
    }

    fn mutation_receipt_verifier(&self) -> Result<&Route53MutationReceiptVerifier> {
        self.mutation_receipt_verifier
            .as_ref()
            .ok_or_else(|| validation("route53_mutation_authority_unavailable"))
    }

    fn lifecycle_token_key(&self) -> Result<&Route53LifecycleTokenKey> {
        self.lifecycle_token_key
            .as_ref()
            .ok_or_else(|| validation("route53_mutation_authority_unavailable"))
    }

    async fn all_zones(&self) -> DnsProviderResult<Vec<ObservedDnsZone>> {
        let mut zones = Vec::new();
        let mut marker = None::<String>;
        let mut seen_markers = BTreeSet::new();
        let mut seen_ids = BTreeSet::new();
        for _ in 0..MAX_PROVIDER_PAGES {
            let page = self
                .api
                .list_hosted_zones(marker.as_deref(), HOSTED_ZONE_PAGE_SIZE)
                .await?;
            validate_zone_page(&page)?;
            for zone in page.items {
                if zone.private_zone {
                    continue;
                }
                let mapped = self.map_zone(zone)?;
                if !seen_ids.insert(mapped.zone.zone_id.clone()) {
                    return Err(validation("duplicate_route53_zone_id"));
                }
                zones.push(mapped);
                if zones.len() > MAX_INVENTORY_ZONES {
                    return Err(validation("route53_zone_inventory_limit"));
                }
            }
            let Some(next) = page.next_marker else {
                zones.sort_by(|left, right| left.zone.zone_id.cmp(&right.zone.zone_id));
                return Ok(zones);
            };
            if marker.as_deref() == Some(next.as_str()) || !seen_markers.insert(next.clone()) {
                return Err(validation("route53_zone_pagination_loop"));
            }
            marker = Some(next);
        }
        Err(validation("route53_zone_pagination_limit"))
    }

    async fn all_records_with_raw(
        &self,
        zone: &DnsZoneRef,
    ) -> DnsProviderResult<Vec<(ObservedDnsRecordSet, Route53RecordSet)>> {
        self.validate_zone_ref(zone)?;
        let provider_zone = self
            .api
            .get_hosted_zone(zone.zone_id.as_str())
            .await?
            .ok_or_else(|| not_found("route53_zone_not_found"))?;
        if self.map_zone(provider_zone)?.zone != *zone {
            return Err(validation("route53_zone_identity_mismatch"));
        }

        let mut records = Vec::new();
        let mut cursor = None::<Route53RecordCursor>;
        let mut seen_cursors = BTreeSet::new();
        let mut seen_keys = BTreeSet::new();
        for _ in 0..MAX_PROVIDER_PAGES {
            let page = self
                .api
                .list_record_sets(zone.zone_id.as_str(), cursor.as_ref(), RECORD_PAGE_SIZE)
                .await?;
            validate_record_page(&page)?;
            for record in page.items {
                let (record_set, revision) = model::map_record_set(zone, record.clone())?;
                if !seen_keys.insert(record_set.key.clone()) {
                    return Err(validation("duplicate_route53_record_identity"));
                }
                records.push((
                    ObservedDnsRecordSet {
                        zone: zone.clone(),
                        record_set,
                        provider_object_ids: BTreeSet::<DnsRecordObjectId>::new(),
                        revision,
                    },
                    record,
                ));
                if records.len() > MAX_INVENTORY_RECORDS {
                    return Err(validation("route53_record_inventory_limit"));
                }
            }
            let Some(next) = page.next else {
                records.sort_by(|left, right| left.0.record_set.key.cmp(&right.0.record_set.key));
                return Ok(records);
            };
            if cursor.as_ref() == Some(&next) || !seen_cursors.insert(next.clone()) {
                return Err(validation("route53_record_pagination_loop"));
            }
            cursor = Some(next);
        }
        Err(validation("route53_record_pagination_limit"))
    }

    async fn all_records(&self, zone: &DnsZoneRef) -> DnsProviderResult<Vec<ObservedDnsRecordSet>> {
        Ok(self
            .all_records_with_raw(zone)
            .await?
            .into_iter()
            .map(|(observed, _)| observed)
            .collect())
    }

    /// Observe one complete, bounded Route 53 RRset snapshot.
    ///
    /// This provider-specific operation performs exactly one pass over the provider's
    /// pagination and retains the same identity, duplicate, loop, and 100,000-record
    /// validation enforced by the regular inventory implementation. It intentionally
    /// does not expose the raw Route 53 representation.
    pub async fn observe_all_record_sets(
        &self,
        zone: &DnsZoneRef,
    ) -> DnsProviderResult<Vec<ObservedDnsRecordSet>> {
        self.all_records(zone).await
    }

    /// Observe one public hosted zone directly by its provider identity.
    ///
    /// This avoids rebuilding the account's complete zone inventory when a caller
    /// already owns the exact Center account and Route 53 hosted-zone identifiers.
    pub async fn observe_zone_by_id(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> DnsProviderResult<Option<ObservedDnsZone>> {
        account_id
            .validate()
            .map_err(|_| validation("invalid_account"))?;
        zone_id
            .validate()
            .map_err(|_| validation("invalid_route53_zone_id"))?;
        if account_id != &self.center_account_id
            || normalize_zone_id(zone_id.as_str())? != zone_id.as_str()
        {
            return Err(validation("route53_zone_scope_mismatch"));
        }
        let Some(raw_zone) = self.api.get_hosted_zone(zone_id.as_str()).await? else {
            return Ok(None);
        };
        let observed = self.map_zone(raw_zone)?;
        if observed.zone.provider_account_id != *account_id || observed.zone.zone_id != *zone_id {
            return Err(validation("route53_zone_identity_mismatch"));
        }
        Ok(Some(observed))
    }

    /// Authenticate a mutation receipt's Center-account and hosted-zone scope without I/O.
    ///
    /// The provider change identity remains private to the adapter and is only consumed by
    /// [`DnsProvider::observe_change`] after the caller has obtained the authoritative zone.
    pub fn validate_change_receipt_scope(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        change_id: &DnsChangeId,
    ) -> DnsProviderResult<()> {
        self.mutation_receipt_verifier()?
            .validate_scope(account_id, zone_id, change_id)
    }

    fn map_zone(&self, zone: Route53HostedZone) -> DnsProviderResult<ObservedDnsZone> {
        if zone.private_zone {
            return Err(validation("route53_private_zone_unsupported"));
        }
        if zone.has_linked_service {
            return Err(validation("route53_linked_service_zone_unsupported"));
        }
        if zone.has_unsupported_features {
            return Err(validation("route53_zone_features_unsupported"));
        }
        Ok(ObservedDnsZone {
            zone: DnsZoneRef {
                provider_account_id: self.center_account_id.clone(),
                provider: CloudProvider::Aws,
                zone_id: DnsZoneId::new(normalize_zone_id(&zone.id)?)
                    .map_err(|_| validation("invalid_route53_zone_id"))?,
                apex: AbsoluteDnsName::new(model::decode_domain_presentation(&zone.name)?)
                    .map_err(|_| validation("invalid_route53_zone_name"))?,
                visibility: ZoneVisibility::Public,
            },
            revision: None,
        })
    }

    fn validate_zone_ref(&self, zone: &DnsZoneRef) -> DnsProviderResult<()> {
        zone.validate().map_err(|_| validation("invalid_zone"))?;
        if zone.provider != CloudProvider::Aws
            || zone.provider_account_id != self.center_account_id
            || zone.visibility != ZoneVisibility::Public
            || normalize_zone_id(zone.zone_id.as_str())? != zone.zone_id.as_str()
        {
            return Err(validation("route53_zone_scope_mismatch"));
        }
        Ok(())
    }

    fn scope(&self, method: CursorMethod) -> CursorScope {
        CursorScope {
            center_account_id: self.center_account_id.as_str().to_string(),
            aws_account_id: self.aws_account_id.clone(),
            method,
        }
    }

    fn build_change_receipt(
        &self,
        zone: &DnsZoneRef,
        provider_change_id: &str,
        request_digest: String,
        submitted_at_unix_seconds: i64,
        status: &str,
    ) -> DnsProviderResult<DnsChangeReceipt> {
        let token = ChangeToken {
            version: 1,
            center_scope: scope_hash(self.center_account_id.as_str()),
            external_scope: scope_hash(&self.aws_account_id),
            zone_scope: scope_hash(zone.zone_id.as_str()),
            provider_change_id: provider_change_id.to_string(),
            request_digest,
            submitted_at_unix_seconds,
            guard: DnsGuardStrength::Atomic,
        };
        let id = DnsChangeId::new(sign_token(
            &token,
            &self.mutation_receipt_verifier()?.key.0,
            TokenDomain::MutationReceipt,
        )?)
        .map_err(|_| unknown_outcome("route53_receipt_encoding_failed"))?;
        change_receipt(id, status, DnsGuardStrength::Atomic)
            .map_err(|_| unknown_outcome("invalid_route53_change_status"))
    }

    fn decode_change_receipt(
        &self,
        zone: &DnsZoneRef,
        change_id: &DnsChangeId,
    ) -> DnsProviderResult<ChangeToken> {
        self.validate_zone_ref(zone)?;
        self.mutation_receipt_verifier()?.decode_scope(
            &zone.provider_account_id,
            &zone.zone_id,
            change_id,
        )
    }

    fn validate_lifecycle_zone_ref(&self, zone: &DnsZoneRef) -> Result<()> {
        zone.validate().map_err(|_| validation("invalid_zone"))?;
        if zone.provider != CloudProvider::Aws
            || zone.provider_account_id != self.center_account_id
            || normalize_zone_id(zone.zone_id.as_str())? != zone.zone_id.as_str()
        {
            return Err(validation("route53_zone_scope_mismatch"));
        }
        Ok(())
    }

    async fn observe_lifecycle_inner(
        &self,
        zone: &DnsZoneRef,
    ) -> ZoneLifecycleProviderResult<Option<ZoneLifecycleObservation>> {
        self.validate_lifecycle_zone_ref(zone)?;
        let Some(raw_zone) = self.api.get_hosted_zone(zone.zone_id.as_str()).await? else {
            return Ok(None);
        };
        let normalized_id = normalize_zone_id(&raw_zone.id)?;
        let apex = AbsoluteDnsName::new(model::decode_domain_presentation(&raw_zone.name)?)
            .map_err(|_| validation("invalid_route53_zone_name"))?;
        let visibility = if raw_zone.private_zone {
            ZoneVisibility::Private
        } else {
            ZoneVisibility::Public
        };
        if normalized_id != zone.zone_id.as_str()
            || apex != zone.apex
            || visibility != zone.visibility
        {
            return Err(validation("route53_zone_identity_mismatch"));
        }
        if raw_zone.has_linked_service || raw_zone.has_unsupported_features {
            return Err(validation("route53_zone_lifecycle_unsupported"));
        }
        let nameservers = raw_zone
            .name_servers
            .iter()
            .map(|value| {
                AbsoluteDnsName::new(model::decode_domain_presentation(value)?)
                    .map_err(|_| validation("invalid_route53_nameserver"))
            })
            .collect::<Result<BTreeSet<_>>>()?;
        if !raw_zone.private_zone && nameservers.is_empty() {
            return Err(validation("missing_route53_delegation_set"));
        }
        if raw_zone.private_zone && !nameservers.is_empty() {
            return Err(validation("invalid_route53_private_delegation_set"));
        }
        let dnssec_raw = if raw_zone.private_zone {
            None
        } else {
            Some(self.api.get_dnssec(zone.zone_id.as_str()).await?)
        };
        let dnssec = match dnssec_raw.as_ref() {
            None => DnssecObservation {
                state: DnssecProviderState::Unsupported,
                ds_records: Vec::new(),
                external_action: DnssecExternalAction::None,
                provider_detail: Some("private_zone".to_string()),
            },
            Some(value) => map_dnssec_observation(value)?,
        };
        let non_default_record_count = raw_zone
            .resource_record_set_count
            .checked_sub(2)
            .ok_or_else(|| validation("invalid_route53_record_set_count"))?;
        let revision = lifecycle_revision(&raw_zone, dnssec_raw.as_ref())?;
        let delegation = if raw_zone.private_zone {
            DelegationObservation {
                state: DelegationState::NotApplicable,
                expected_nameservers: BTreeSet::new(),
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
            .map_err(|_| validation("invalid_route53_lifecycle_observation"))?;
        Ok(Some(observation))
    }

    fn lifecycle_receipt(
        &self,
        zone_id: Option<&str>,
        operation: LifecycleOperation,
        change: &Route53ChangeInfo,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        let provider_change_id = normalize_change_id(&change.id)
            .map_err(|_| unknown_outcome("invalid_route53_lifecycle_change"))?;
        if change.submitted_at_unix_seconds < 0 {
            return Err(unknown_outcome("invalid_route53_lifecycle_change"));
        }
        let token = LifecycleMutationToken {
            version: 1,
            center_scope: scope_hash(self.center_account_id.as_str()),
            account_scope: scope_hash(&self.aws_account_id),
            zone_scope: zone_id.map(scope_hash),
            provider_change_id: Some(provider_change_id),
            submitted_at_unix_seconds: change.submitted_at_unix_seconds,
            operation,
        };
        let mutation_id = ZoneLifecycleMutationId::new(sign_token(
            &token,
            &self.lifecycle_token_key()?.0,
            TokenDomain::Lifecycle,
        )?)
        .map_err(|_| unknown_outcome("route53_lifecycle_receipt_encoding_failed"))?;
        lifecycle_change_receipt(mutation_id, &change.status)
    }

    fn recovered_create_receipt(
        &self,
        zone_id: &str,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        let token = LifecycleMutationToken {
            version: 1,
            center_scope: scope_hash(self.center_account_id.as_str()),
            account_scope: scope_hash(&self.aws_account_id),
            zone_scope: Some(scope_hash(zone_id)),
            provider_change_id: None,
            submitted_at_unix_seconds: 0,
            operation: LifecycleOperation::Create,
        };
        let mutation_id = ZoneLifecycleMutationId::new(sign_token(
            &token,
            &self.lifecycle_token_key()?.0,
            TokenDomain::Lifecycle,
        )?)
        .map_err(|_| unknown_outcome("route53_lifecycle_receipt_encoding_failed"))?;
        Ok(ZoneLifecycleMutationReceipt {
            mutation_id,
            state: ZoneLifecycleMutationState::Succeeded,
        })
    }

    async fn find_zone_by_caller_reference(
        &self,
        caller_reference: &str,
    ) -> Result<Option<Route53HostedZone>> {
        let mut marker = None::<String>;
        let mut seen = BTreeSet::new();
        for _ in 0..MAX_PROVIDER_PAGES {
            let page = self
                .api
                .list_hosted_zones(marker.as_deref(), HOSTED_ZONE_PAGE_SIZE)
                .await?;
            validate_zone_page(&page)?;
            if let Some(zone) = page
                .items
                .into_iter()
                .find(|zone| zone.caller_reference == caller_reference)
            {
                return Ok(Some(zone));
            }
            let Some(next) = page.next_marker else {
                return Ok(None);
            };
            if !seen.insert(next.clone()) {
                return Err(validation("route53_zone_pagination_loop"));
            }
            marker = Some(next);
        }
        Err(validation("route53_zone_pagination_limit"))
    }
}

#[async_trait]
impl DnsProvider for Route53DnsAdapter {
    async fn get_zone(&self, zone: &DnsZoneRef) -> DnsProviderResult<Option<ObservedDnsZone>> {
        self.validate_zone_ref(zone)?;
        let Some(provider_zone) = self.api.get_hosted_zone(zone.zone_id.as_str()).await? else {
            return Ok(None);
        };
        let observed = self.map_zone(provider_zone)?;
        if observed.zone != *zone {
            return Err(validation("route53_zone_identity_mismatch"));
        }
        Ok(Some(observed))
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
            return Err(validation("route53_account_scope_mismatch"));
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
        self.validate_zone_ref(zone)?;
        page.validate().map_err(|_| validation("invalid_page"))?;
        paginate(
            self.all_records(zone).await?,
            page,
            self.scope(CursorMethod::Records {
                zone_id: zone.zone_id.as_str().to_string(),
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
        self.mutation_receipt_verifier()?;
        self.validate_zone_ref(zone)?;
        validate_dns_changes(zone, changes).map_err(|_| validation("invalid_dns_changes"))?;
        let current = self.all_records_with_raw(zone).await?;
        let changes = plan_change_batch(zone, changes, current)?;
        let request_bytes = serde_json::to_vec(&changes)
            .map_err(|_| validation("route53_change_encoding_failed"))?;
        let request_digest = URL_SAFE_NO_PAD.encode(Sha256::digest(request_bytes));
        let request = Route53ChangeBatch {
            comment: change_comment(&request_digest),
            changes,
        };
        let result = self
            .api
            .change_record_sets(zone.zone_id.as_str(), &request)
            .await?;
        let provider_change_id = normalize_change_id(&result.id)
            .map_err(|_| unknown_outcome("invalid_route53_change_response"))?;
        if result.submitted_at_unix_seconds < 0
            || result.comment.as_deref() != Some(request.comment.as_str())
        {
            return Err(unknown_outcome("invalid_route53_change_response"));
        }
        let receipt = self.build_change_receipt(
            zone,
            &provider_change_id,
            request_digest,
            result.submitted_at_unix_seconds,
            &result.status,
        )?;
        receipt
            .validate_against_request(minimum_guard)
            .map_err(|_| unknown_outcome("invalid_route53_change_receipt"))?;
        Ok(receipt)
    }

    async fn observe_change(
        &self,
        zone: &DnsZoneRef,
        change_id: &DnsChangeId,
    ) -> DnsProviderResult<DnsChangeReceipt> {
        self.mutation_receipt_verifier()?;
        self.validate_zone_ref(zone)?;
        let token = self.decode_change_receipt(zone, change_id)?;
        let result = self
            .api
            .get_change(&token.provider_change_id)
            .await?
            .ok_or_else(|| not_found("route53_change_not_found"))?;
        let returned_id = normalize_change_id(&result.id)
            .map_err(|_| validation("invalid_route53_change_response"))?;
        if returned_id != token.provider_change_id {
            return Err(validation("route53_change_identity_mismatch"));
        }
        if result.submitted_at_unix_seconds != token.submitted_at_unix_seconds
            || result.comment.as_deref() != Some(change_comment(&token.request_digest).as_str())
        {
            return Err(validation("route53_change_metadata_mismatch"));
        }
        change_receipt(change_id.clone(), &result.status, token.guard)
    }
}

#[async_trait]
impl ZoneLifecycleProvider for Route53DnsAdapter {
    async fn create_zone(
        &self,
        request: &ZoneCreationRequest,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        self.lifecycle_token_key()?;
        if request.provider != CloudProvider::Aws
            || request.provider_account_id != self.center_account_id
        {
            return Err(validation("route53_zone_creation_scope_mismatch"));
        }
        if request.visibility != ZoneVisibility::Public {
            return Err(validation("route53_private_zone_creation_requires_vpc"));
        }
        let caller_reference = stable_caller_reference(
            self.center_account_id.as_str(),
            &self.aws_account_id,
            request.idempotency_key.as_str(),
        );
        if let Some(found) = self
            .find_zone_by_caller_reference(&caller_reference)
            .await?
        {
            let zone_id = normalize_zone_id(&found.id)?;
            let found = self
                .api
                .get_hosted_zone(&zone_id)
                .await?
                .ok_or_else(|| unknown_outcome("route53_recovered_zone_disappeared"))?;
            validate_created_zone(&found, request, &caller_reference)?;
            return self.recovered_create_receipt(&zone_id);
        }
        let created = self
            .api
            .create_hosted_zone(&Route53CreateHostedZoneRequest {
                name: request.apex.as_str().to_string(),
                caller_reference: caller_reference.clone(),
            })
            .await?;
        validate_created_zone(&created.hosted_zone, request, &caller_reference)
            .map_err(|_| unknown_outcome("route53_create_zone_result_mismatch"))?;
        let zone_id = normalize_zone_id(&created.hosted_zone.id)
            .map_err(|_| unknown_outcome("route53_create_zone_result_mismatch"))?;
        self.lifecycle_receipt(Some(&zone_id), LifecycleOperation::Create, &created.change)
    }

    async fn observe_zone(
        &self,
        zone: &DnsZoneRef,
    ) -> ZoneLifecycleProviderResult<Option<ZoneLifecycleObservation>> {
        self.observe_lifecycle_inner(zone).await
    }

    async fn set_dnssec(
        &self,
        zone: &DnsZoneRef,
        desired: DnssecDesiredState,
        expected_revision: &ZoneLifecycleRevision,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        self.lifecycle_token_key()?;
        let observed = self
            .observe_lifecycle_inner(zone)
            .await?
            .ok_or_else(|| not_found("route53_zone_not_found"))?;
        if &observed.revision != expected_revision {
            return Err(conflict("route53_zone_lifecycle_revision_conflict"));
        }
        if zone.visibility != ZoneVisibility::Public {
            return Err(conflict("route53_private_dnssec_unsupported"));
        }
        let (operation, change) = match desired {
            DnssecDesiredState::Enabled => {
                if matches!(
                    observed.dnssec.state,
                    DnssecProviderState::Enabling
                        | DnssecProviderState::AwaitingDs
                        | DnssecProviderState::Active
                ) {
                    return Err(conflict("route53_dnssec_already_enabled"));
                }
                let raw = self.api.get_dnssec(zone.zone_id.as_str()).await?;
                if !raw
                    .key_signing_keys
                    .iter()
                    .any(|key| key.status == "ACTIVE" && key.ds_record.is_some())
                {
                    // Creating and activating a KSK needs KMS input absent from this provider port.
                    return Err(conflict("route53_dnssec_active_ksk_required"));
                }
                (
                    LifecycleOperation::EnableDnssec,
                    self.api
                        .enable_hosted_zone_dnssec(zone.zone_id.as_str())
                        .await?,
                )
            }
            DnssecDesiredState::Disabled => {
                if observed.dnssec.state == DnssecProviderState::Disabled {
                    return Err(conflict("route53_dnssec_already_disabled"));
                }
                // Route 53 rejects a disable while a parent DS exists. This port has no
                // independently verified parent-DS-removal evidence, so it must not dispatch.
                return Err(conflict(
                    "route53_dnssec_parent_ds_removal_verification_required",
                ));
            }
        };
        self.lifecycle_receipt(Some(zone.zone_id.as_str()), operation, &change)
    }

    async fn delete_zone(
        &self,
        request: &ZoneDeletionRequest,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        self.lifecycle_token_key()?;
        if &request.approval().approved_revision != request.revision()
            || &request.approval().approved_zone != request.zone()
            || request.approval().approved_by.trim().is_empty()
            || request.approval().approved_at.trim().is_empty()
            || !request.approval().acknowledgements.is_empty()
        {
            return Err(validation("invalid_route53_zone_deletion_approval"));
        }
        let observed = self
            .observe_lifecycle_inner(request.zone())
            .await?
            .ok_or_else(|| not_found("route53_zone_not_found"))?;
        if &observed.revision != request.revision() {
            return Err(conflict("route53_zone_lifecycle_revision_conflict"));
        }
        if observed.non_default_record_count != 0
            || !matches!(
                observed.dnssec.state,
                DnssecProviderState::Disabled | DnssecProviderState::Unsupported
            )
        {
            return Err(conflict("route53_zone_deletion_precondition_failed"));
        }
        let change = self
            .api
            .delete_hosted_zone(request.zone().zone_id.as_str())
            .await?;
        self.lifecycle_receipt(
            Some(request.zone().zone_id.as_str()),
            LifecycleOperation::Delete,
            &change,
        )
    }

    async fn observe_mutation(
        &self,
        mutation_id: &ZoneLifecycleMutationId,
    ) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
        let token: LifecycleMutationToken = verify_token(
            mutation_id.as_str(),
            &self.lifecycle_token_key()?.0,
            TokenDomain::Lifecycle,
        )
        .map_err(|_| not_found("route53_lifecycle_mutation_not_found"))?;
        if token.version != 1
            || token.center_scope != scope_hash(self.center_account_id.as_str())
            || token.account_scope != scope_hash(&self.aws_account_id)
            || token.zone_scope.as_deref().is_none_or(str::is_empty)
        {
            return Err(not_found("route53_lifecycle_mutation_not_found"));
        }
        let Some(provider_change_id) = token.provider_change_id.as_deref() else {
            if token.operation != LifecycleOperation::Create {
                return Err(not_found("route53_lifecycle_mutation_not_found"));
            }
            return Ok(ZoneLifecycleMutationReceipt {
                mutation_id: mutation_id.clone(),
                state: ZoneLifecycleMutationState::Succeeded,
            });
        };
        if normalize_change_id(provider_change_id).ok().as_deref() != Some(provider_change_id) {
            return Err(not_found("route53_lifecycle_mutation_not_found"));
        }
        let change = self
            .api
            .get_change(provider_change_id)
            .await?
            .ok_or_else(|| not_found("route53_lifecycle_mutation_not_found"))?;
        if normalize_change_id(&change.id).ok().as_deref() != Some(provider_change_id)
            || change.submitted_at_unix_seconds != token.submitted_at_unix_seconds
        {
            return Err(validation("route53_lifecycle_change_metadata_mismatch"));
        }
        lifecycle_change_receipt(mutation_id.clone(), &change.status)
    }
}

fn plan_change_batch(
    zone: &DnsZoneRef,
    changes: &[DnsRecordChange],
    current: Vec<(ObservedDnsRecordSet, Route53RecordSet)>,
) -> DnsProviderResult<Vec<Route53RecordChange>> {
    let current = current
        .into_iter()
        .map(|(observed, raw)| (observed.record_set.key.clone(), (observed, raw)))
        .collect::<BTreeMap<_, _>>();
    let mut resultant = current
        .iter()
        .map(|(key, (observed, _))| (key.clone(), observed.record_set.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut affected_owners = BTreeSet::new();
    let mut affected_groups = BTreeSet::new();
    let mut request = Vec::new();
    for change in changes {
        let key = change_key(change);
        affected_owners.insert(key.owner.clone());
        affected_groups.insert((key.owner.clone(), key.record_type));
        match change {
            DnsRecordChange::Create { record_set, .. } => {
                if current.contains_key(&record_set.key) {
                    return Err(conflict("route53_create_guard_conflict"));
                }
                request.push(Route53RecordChange {
                    action: Route53ChangeAction::Create,
                    record_set: model::render_record_set(zone, record_set)?,
                });
                resultant.insert(record_set.key.clone(), record_set.clone());
            }
            DnsRecordChange::Replace {
                previous, desired, ..
            } => {
                let (observed, raw) = current
                    .get(&desired.key)
                    .ok_or_else(|| conflict("route53_replace_guard_conflict"))?;
                if !same_route53_observation(observed, previous) {
                    return Err(conflict("route53_replace_guard_conflict"));
                }
                validate_replace_shape(&previous.record_set, desired)?;
                if previous.record_set == *desired {
                    return Err(validation("route53_noop_replace"));
                }
                request.push(Route53RecordChange {
                    action: Route53ChangeAction::Delete,
                    record_set: raw.clone(),
                });
                request.push(Route53RecordChange {
                    action: Route53ChangeAction::Create,
                    record_set: model::render_record_set(zone, desired)?,
                });
                resultant.insert(desired.key.clone(), desired.clone());
            }
            DnsRecordChange::Delete { previous, .. } => {
                let (observed, raw) = current
                    .get(&previous.record_set.key)
                    .ok_or_else(|| conflict("route53_delete_guard_conflict"))?;
                if !same_route53_observation(observed, previous) {
                    return Err(conflict("route53_delete_guard_conflict"));
                }
                request.push(Route53RecordChange {
                    action: Route53ChangeAction::Delete,
                    record_set: raw.clone(),
                });
                resultant.remove(&previous.record_set.key);
            }
        }
    }
    validate_resultant_inventory(&resultant, &affected_owners, &affected_groups)?;
    if request.is_empty() || request.len() > MAX_PROVIDER_CHANGE_ACTIONS {
        return Err(validation("route53_change_action_limit"));
    }
    validate_change_request(&request)?;
    Ok(request)
}

fn validate_replace_shape(
    previous: &ProviderDnsRecordSet,
    desired: &ProviderDnsRecordSet,
) -> DnsProviderResult<()> {
    let previous_alias = route53_alias_and_health(previous);
    let desired_alias = route53_alias_and_health(desired);
    if resultant_routing_family(previous) != resultant_routing_family(desired)
        || previous_alias.0 != desired_alias.0
        || previous_alias.1 != desired_alias.1
    {
        return Err(conflict("route53_replace_shape_conflict"));
    }
    Ok(())
}

fn route53_alias_and_health(
    record: &ProviderDnsRecordSet,
) -> (bool, Option<&edgion_center_core::Route53HealthCheckId>) {
    match record.extension.as_ref() {
        Some(edgion_center_core::DnsRecordExtension::Route53 {
            alias_target,
            health_check_id,
            ..
        }) => (alias_target.is_some(), health_check_id.as_ref()),
        _ => (false, None),
    }
}

fn change_key(change: &DnsRecordChange) -> &DnsRecordSetKey {
    match change {
        DnsRecordChange::Create { record_set, .. } => &record_set.key,
        DnsRecordChange::Replace { desired, .. } => &desired.key,
        DnsRecordChange::Delete { previous, .. } => &previous.record_set.key,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultantRoutingFamily {
    Simple,
    Weighted,
    Failover,
    Latency,
    Geolocation,
    Multivalue,
}

fn validate_resultant_inventory(
    resultant: &BTreeMap<DnsRecordSetKey, ProviderDnsRecordSet>,
    affected_owners: &BTreeSet<edgion_center_core::DnsOwnerName>,
    affected_groups: &BTreeSet<(edgion_center_core::DnsOwnerName, ProviderDnsRecordType)>,
) -> DnsProviderResult<()> {
    for owner in affected_owners {
        let mut has_cname = false;
        let mut has_other = false;
        for record in resultant
            .values()
            .filter(|record| &record.key.owner == owner)
        {
            if record.key.record_type == ProviderDnsRecordType::Cname {
                has_cname = true;
            } else {
                has_other = true;
            }
        }
        if has_cname && has_other {
            return Err(conflict("route53_resultant_cname_conflict"));
        }
    }

    for (owner, record_type) in affected_groups {
        let members = resultant
            .values()
            .filter(|record| &record.key.owner == owner && record.key.record_type == *record_type)
            .collect::<Vec<_>>();
        let Some(first) = members.first() else {
            continue;
        };
        let family = resultant_routing_family(first);
        let mut ttl = None;
        let mut selectors = BTreeSet::new();
        for member in &members {
            if resultant_routing_family(member) != family {
                return Err(conflict("route53_resultant_routing_family_conflict"));
            }
            if let edgion_center_core::DnsTtl::Seconds(value) = member.ttl {
                if ttl.replace(value).is_some_and(|existing| existing != value) {
                    return Err(conflict("route53_resultant_ttl_conflict"));
                }
            }
            if let Some(selector) = resultant_selector(member) {
                if !selectors.insert(selector) {
                    return Err(conflict("route53_resultant_selector_conflict"));
                }
            }
        }
        if family == ResultantRoutingFamily::Weighted && members.len() > 100 {
            return Err(conflict("route53_resultant_weighted_limit"));
        }
    }
    Ok(())
}

fn resultant_routing_family(record: &ProviderDnsRecordSet) -> ResultantRoutingFamily {
    let Some(edgion_center_core::DnsRecordExtension::Route53 {
        routing_policy: Some(policy),
        ..
    }) = record.extension.as_ref()
    else {
        return ResultantRoutingFamily::Simple;
    };
    match policy {
        Route53RoutingPolicy::Weighted { .. } => ResultantRoutingFamily::Weighted,
        Route53RoutingPolicy::Failover { .. } => ResultantRoutingFamily::Failover,
        Route53RoutingPolicy::Latency { .. } => ResultantRoutingFamily::Latency,
        Route53RoutingPolicy::Geolocation { .. } => ResultantRoutingFamily::Geolocation,
        Route53RoutingPolicy::Multivalue => ResultantRoutingFamily::Multivalue,
    }
}

fn resultant_selector(record: &ProviderDnsRecordSet) -> Option<String> {
    let Some(edgion_center_core::DnsRecordExtension::Route53 {
        routing_policy: Some(policy),
        ..
    }) = record.extension.as_ref()
    else {
        return None;
    };
    match policy {
        Route53RoutingPolicy::Failover { role } => Some(format!("failover:{role:?}")),
        Route53RoutingPolicy::Latency { region } => Some(format!("latency:{region}")),
        Route53RoutingPolicy::Geolocation { location } => Some(match location {
            Route53GeoLocation::Default => "geo:default".to_string(),
            Route53GeoLocation::Continent { code } => format!("geo:continent:{code}"),
            Route53GeoLocation::Country { code } => format!("geo:country:{code}"),
            Route53GeoLocation::UsSubdivision { code } => format!("geo:us:{code}"),
        }),
        Route53RoutingPolicy::Weighted { .. } | Route53RoutingPolicy::Multivalue => None,
    }
}

fn validate_change_request(request: &[Route53RecordChange]) -> DnsProviderResult<()> {
    let mut elements = 0usize;
    for change in request {
        let record = &change.record_set;
        if record
            .resource_records
            .iter()
            .any(|value| value.len() > MAX_PROVIDER_RECORD_VALUE_BYTES)
        {
            return Err(validation("route53_record_value_size_limit"));
        }
        let cost = if record.alias_target.is_some() {
            1
        } else {
            record.resource_records.len()
        };
        elements = elements
            .checked_add(cost)
            .ok_or_else(|| validation("route53_change_element_limit"))?;
    }
    if elements == 0 || elements > MAX_PROVIDER_RECORD_ELEMENTS {
        return Err(validation("route53_change_element_limit"));
    }
    Ok(())
}

fn same_route53_observation(
    current: &ObservedDnsRecordSet,
    requested: &ObservedDnsRecordSet,
) -> bool {
    current.zone == requested.zone
        && current.record_set == requested.record_set
        && current.revision == requested.revision
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
    aws_account_id: String,
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
    #[serde(rename = "v")]
    version: u8,
    #[serde(rename = "c")]
    center_scope: String,
    #[serde(rename = "a")]
    external_scope: String,
    #[serde(rename = "z")]
    zone_scope: String,
    #[serde(rename = "p")]
    provider_change_id: String,
    #[serde(rename = "r")]
    request_digest: String,
    #[serde(rename = "s")]
    submitted_at_unix_seconds: i64,
    #[serde(rename = "g")]
    guard: DnsGuardStrength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LifecycleOperation {
    Create,
    Delete,
    EnableDnssec,
}

#[derive(Serialize, Deserialize)]
struct LifecycleMutationToken {
    version: u8,
    center_scope: String,
    account_scope: String,
    zone_scope: Option<String>,
    provider_change_id: Option<String>,
    submitted_at_unix_seconds: i64,
    operation: LifecycleOperation,
}

#[derive(Clone, Copy)]
enum TokenDomain {
    InventoryCursor,
    MutationReceipt,
    Lifecycle,
}

impl TokenDomain {
    fn separator(self) -> &'static [u8] {
        match self {
            Self::InventoryCursor => b"edgion-center/route53/inventory-cursor/v1\0",
            Self::MutationReceipt => b"edgion-center/route53/mutation-receipt/v1\0",
            Self::Lifecycle => b"edgion-center/route53/lifecycle-token/v1\0",
        }
    }
}

fn paginate<T: Clone + Serialize>(
    items: Vec<T>,
    page: &DnsPageRequest,
    scope: CursorScope,
    key: &Route53CursorKey,
) -> DnsProviderResult<DnsPage<T>> {
    let inventory_digest = canonical_inventory_digest(&items)?;
    let offset = match page.token.as_ref() {
        Some(token) => {
            let decoded: CursorToken =
                verify_token(token.as_str(), &key.0, TokenDomain::InventoryCursor)
                    .map_err(|_| validation("invalid_route53_page_token"))?;
            if decoded.version != 1 || decoded.scope != scope || decoded.offset >= items.len() {
                return Err(validation("invalid_route53_page_token"));
            }
            if decoded.inventory_digest != inventory_digest {
                return Err(validation("route53_inventory_changed"));
            }
            decoded.offset
        }
        None => 0,
    };
    let end = offset
        .saturating_add(usize::from(page.limit))
        .min(items.len());
    let next = if end < items.len() {
        Some(
            DnsPageToken::new(sign_token(
                &CursorToken {
                    version: 1,
                    scope,
                    offset: end,
                    inventory_digest,
                },
                &key.0,
                TokenDomain::InventoryCursor,
            )?)
            .map_err(|_| validation("route53_page_token_encoding_failed"))?,
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
        .map_err(|_| validation("invalid_route53_page"))?;
    Ok(result)
}

fn canonical_inventory_digest<T: Serialize>(items: &[T]) -> DnsProviderResult<String> {
    let canonical =
        serde_json::to_vec(items).map_err(|_| validation("route53_inventory_encoding_failed"))?;
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(canonical)))
}

fn stable_caller_reference(center_scope: &str, account_scope: &str, key: &str) -> String {
    let digest = Sha256::new()
        .chain_update(center_scope.as_bytes())
        .chain_update([0])
        .chain_update(account_scope.as_bytes())
        .chain_update([0])
        .chain_update(key.as_bytes())
        .finalize();
    format!("edgion-{}", URL_SAFE_NO_PAD.encode(digest))
}

fn validate_created_zone(
    zone: &Route53HostedZone,
    request: &ZoneCreationRequest,
    caller_reference: &str,
) -> Result<()> {
    let apex = AbsoluteDnsName::new(model::decode_domain_presentation(&zone.name)?)
        .map_err(|_| validation("invalid_route53_zone_name"))?;
    normalize_zone_id(&zone.id)?;
    if zone.private_zone
        || zone.has_linked_service
        || zone.has_unsupported_features
        || zone.caller_reference != caller_reference
        || apex != request.apex
        || zone.name_servers.is_empty()
    {
        return Err(validation("route53_create_zone_result_mismatch"));
    }
    Ok(())
}

fn lifecycle_revision(
    zone: &Route53HostedZone,
    dnssec: Option<&Route53DnssecInfo>,
) -> Result<ZoneLifecycleRevision> {
    let mut zone = zone.clone();
    zone.name_servers.sort();
    let mut dnssec = dnssec.cloned();
    if let Some(value) = dnssec.as_mut() {
        value.key_signing_keys.sort_by(|left, right| {
            (&left.status, &left.ds_record).cmp(&(&right.status, &right.ds_record))
        });
    }
    let bytes = serde_json::to_vec(&(zone, dnssec))
        .map_err(|_| validation("route53_lifecycle_revision_encoding_failed"))?;
    ZoneLifecycleRevision::new(URL_SAFE_NO_PAD.encode(Sha256::digest(bytes)))
        .map_err(|_| validation("invalid_route53_lifecycle_revision"))
}

fn parse_ds_record(value: &str) -> Result<DnssecDsRecord> {
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    if parts.len() != 4 {
        return Err(validation("invalid_route53_dnssec_ds_record"));
    }
    let record = DnssecDsRecord {
        key_tag: parts[0]
            .parse()
            .map_err(|_| validation("invalid_route53_dnssec_ds_record"))?,
        algorithm: parts[1]
            .parse()
            .map_err(|_| validation("invalid_route53_dnssec_ds_record"))?,
        digest_type: parts[2]
            .parse()
            .map_err(|_| validation("invalid_route53_dnssec_ds_record"))?,
        digest: parts[3].to_ascii_uppercase(),
    };
    record
        .validate()
        .map_err(|_| validation("invalid_route53_dnssec_ds_record"))?;
    Ok(record)
}

fn map_dnssec_observation(value: &Route53DnssecInfo) -> Result<DnssecObservation> {
    let mut failed = false;
    let mut records = Vec::new();
    let mut active_records = Vec::new();
    for key in &value.key_signing_keys {
        match key.status.as_str() {
            "ACTIVE" | "INACTIVE" | "DELETING" => {}
            "ACTION_NEEDED" | "INTERNAL_FAILURE" => failed = true,
            _ => return Err(validation("unsupported_route53_dnssec_ksk_status")),
        }
        if let Some(ds) = key.ds_record.as_deref() {
            let record = parse_ds_record(ds)?;
            if key.status == "ACTIVE" {
                active_records.push(record.clone());
            }
            records.push(record);
        }
    }
    records.sort_by_key(|record| record.key_tag);
    records.dedup();
    active_records.sort_by_key(|record| record.key_tag);
    active_records.dedup();
    if failed {
        return Ok(DnssecObservation {
            state: DnssecProviderState::Failed,
            ds_records: records,
            external_action: DnssecExternalAction::None,
            provider_detail: Some("ksk_action_required".to_string()),
        });
    }
    let (state, external_action) = match value.serve_signature.as_str() {
        "NOT_SIGNING" if records.is_empty() => {
            (DnssecProviderState::Disabled, DnssecExternalAction::None)
        }
        "NOT_SIGNING" => (
            DnssecProviderState::AwaitingDsRemoval,
            DnssecExternalAction::RemoveDs {
                key_tags: records.iter().map(|record| record.key_tag).collect(),
            },
        ),
        "SIGNING" if active_records.is_empty() => (
            DnssecProviderState::Enabling,
            DnssecExternalAction::WaitForProviderActivation,
        ),
        "SIGNING" => (
            DnssecProviderState::AwaitingDs,
            DnssecExternalAction::PublishDs {
                records: active_records,
            },
        ),
        "DELETING" if records.is_empty() => (
            DnssecProviderState::Disabling,
            DnssecExternalAction::WaitForProviderActivation,
        ),
        "DELETING" => (
            DnssecProviderState::AwaitingDsRemoval,
            DnssecExternalAction::RemoveDs {
                key_tags: records.iter().map(|record| record.key_tag).collect(),
            },
        ),
        _ => return Err(validation("unsupported_route53_dnssec_signing_status")),
    };
    Ok(DnssecObservation {
        state,
        ds_records: records,
        external_action,
        provider_detail: Some(value.serve_signature.to_ascii_lowercase()),
    })
}

fn lifecycle_change_receipt(
    mutation_id: ZoneLifecycleMutationId,
    status: &str,
) -> ZoneLifecycleProviderResult<ZoneLifecycleMutationReceipt> {
    let state = match status {
        "PENDING" => ZoneLifecycleMutationState::Pending,
        "INSYNC" => ZoneLifecycleMutationState::Succeeded,
        _ => return Err(validation("unsupported_route53_change_status")),
    };
    Ok(ZoneLifecycleMutationReceipt { mutation_id, state })
}

fn scope_hash(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

fn change_comment(request_digest: &str) -> String {
    format!("edgion:{request_digest}")
}

fn normalize_change_id(value: &str) -> Result<String> {
    let normalized = value.strip_prefix("/change/").unwrap_or(value);
    if normalized.len() < 2
        || normalized.len() > 64
        || !normalized.starts_with('C')
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || normalized.contains('/')
    {
        return Err(validation("invalid_route53_change_id"));
    }
    Ok(normalized.to_string())
}

fn change_receipt(
    id: DnsChangeId,
    status: &str,
    guard_strength: DnsGuardStrength,
) -> DnsProviderResult<DnsChangeReceipt> {
    let (state, propagation) = match status {
        "PENDING" => (DnsChangeState::Pending, DnsPropagationState::Pending),
        "INSYNC" => (
            DnsChangeState::ProviderCommitted,
            DnsPropagationState::ProviderReportedApplied,
        ),
        _ => return Err(validation("unsupported_route53_change_status")),
    };
    let receipt = DnsChangeReceipt {
        id,
        state,
        submission_atomicity: DnsBatchAtomicity::AllOrNothing,
        propagation,
        guard_strength,
    };
    receipt
        .validate()
        .map_err(|_| validation("invalid_route53_change_receipt"))?;
    Ok(receipt)
}

fn sign_token<T: Serialize>(
    value: &T,
    key: &[u8; 32],
    domain: TokenDomain,
) -> DnsProviderResult<String> {
    let encoded =
        serde_json::to_vec(value).map_err(|_| validation("route53_token_encoding_failed"))?;
    let signature = HmacSha256::new_from_slice(key)
        .expect("fixed HMAC key")
        .chain_update(domain.separator())
        .chain_update(&encoded)
        .finalize()
        .into_bytes();
    let mut authenticated = encoded;
    authenticated.extend_from_slice(&signature);
    Ok(URL_SAFE_NO_PAD.encode(authenticated))
}

fn verify_token<T: DeserializeOwned>(
    value: &str,
    key: &[u8; 32],
    domain: TokenDomain,
) -> Result<T> {
    let authenticated = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| validation("invalid_route53_token"))?;
    if authenticated.len() <= 32 {
        return Err(validation("invalid_route53_token"));
    }
    let (encoded, signature) = authenticated.split_at(authenticated.len() - 32);
    HmacSha256::new_from_slice(key)
        .expect("fixed HMAC key")
        .chain_update(domain.separator())
        .chain_update(encoded)
        .verify_slice(signature)
        .map_err(|_| validation("invalid_route53_token"))?;
    serde_json::from_slice(encoded).map_err(|_| validation("invalid_route53_token"))
}

fn validate_zone_page(page: &Route53HostedZonePage) -> DnsProviderResult<()> {
    if page.items.len() > usize::from(HOSTED_ZONE_PAGE_SIZE)
        || page.is_truncated != page.next_marker.is_some()
        || (page.is_truncated && page.items.is_empty())
        || page.next_marker.as_ref().is_some_and(|marker| {
            marker.is_empty() || marker.len() > 64 || marker.chars().any(char::is_control)
        })
    {
        return Err(validation("invalid_route53_zone_page"));
    }
    Ok(())
}

fn validate_record_page(page: &Route53RecordPage) -> DnsProviderResult<()> {
    if page.items.len() > usize::from(RECORD_PAGE_SIZE)
        || page.is_truncated != page.next.is_some()
        || (page.is_truncated && page.items.is_empty())
        || page.next.as_ref().is_some_and(|next| {
            next.name.is_empty()
                || next.name.len() > 1024
                || model::decode_domain_presentation(&next.name).is_err()
                || next.record_type.is_empty()
                || !is_route53_record_type(&next.record_type)
                || next.name.chars().any(char::is_control)
                || next.record_type.chars().any(char::is_control)
                || next.set_identifier.as_ref().is_some_and(|identifier| {
                    identifier.is_empty()
                        || identifier.len() > 128
                        || identifier.chars().any(char::is_control)
                })
        })
    {
        return Err(validation("invalid_route53_record_page"));
    }
    Ok(())
}

fn is_route53_record_type(value: &str) -> bool {
    matches!(
        value,
        "SOA"
            | "A"
            | "TXT"
            | "NS"
            | "CNAME"
            | "MX"
            | "NAPTR"
            | "PTR"
            | "SRV"
            | "SPF"
            | "AAAA"
            | "CAA"
            | "DS"
            | "TLSA"
            | "SSHFP"
            | "SVCB"
            | "HTTPS"
    )
}

pub(crate) fn normalize_zone_id(value: &str) -> Result<String> {
    let normalized = value.strip_prefix("/hostedzone/").unwrap_or(value);
    if normalized.is_empty()
        || normalized.len() > 32
        || !normalized.starts_with('Z')
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || value.starts_with("/hostedzone//")
        || normalized.contains('/')
    {
        return Err(validation("invalid_route53_zone_id"));
    }
    Ok(normalized.to_string())
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
        "Route 53 DNS adapter rejected the request",
        None,
        None,
    )
    .expect("static normalized provider error")
}

#[cfg(test)]
mod tests;
