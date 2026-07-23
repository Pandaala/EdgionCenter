//! AWS Route 53-specific DNS Admin API contract.
//!
//! AWS SDK clients, credentials, provider comments, and raw provider responses stay behind the
//! injected service. The public DTO preserves the complete validated Route 53 record model while
//! keeping this module independent from the provider SDK.

use std::{
    collections::BTreeSet,
    net::{Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use async_trait::async_trait;
use axum::{
    extract::{rejection::JsonRejection, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    AbsoluteDnsName, CaaTag, CloudProvider, CloudResourceId, CoreError, CoreResult, DnsChangeId,
    DnsChangeReceipt, DnsChangeState, DnsCharacterString, DnsOwnerName, DnsPageToken,
    DnsPropagationState, DnsRecordExtension, DnsRecordRevision, DnsRecordSetKey, DnsRecordSetValue,
    DnsRoutingIdentity, DnsTtl, DnsTxtValue, DnsZoneId, DnsZoneRef, IdempotencyKey,
    ProviderDnsRecordSet, ProviderDnsRecordType, Route53AliasTarget, Route53FailoverRole,
    Route53GeoLocation, Route53HealthCheckId, Route53RoutingPolicy, ZoneCreationRequest,
    ZoneLifecycleMutationReceipt, ZoneLifecycleMutationState, ZoneLifecycleObservation,
    ZoneLifecycleRevision, ZoneVisibility,
};
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::ApiResponse;

const DEFAULT_ROUTE53_PAGE_LIMIT: u16 = 50;
pub const MAX_ROUTE53_PAGE_LIMIT: u16 = 300;
pub const MAX_ROUTE53_CHANGE_BATCH: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route53DnsAdminError {
    InvalidRequest,
    NotFound,
    Conflict,
    UnknownOutcome,
    RestartRequired,
    InvalidProviderObservation,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route53ZonePageRequest {
    pub limit: u16,
    pub cursor: Option<DnsPageToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route53RecordPageRequest {
    pub limit: u16,
    pub cursor: Option<DnsPageToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53ZoneDto {
    pub provider_account_id: CloudResourceId,
    pub zone_id: DnsZoneId,
    pub apex: AbsoluteDnsName,
    pub visibility: ZoneVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53ZonePageDto {
    pub items: Vec<Route53ZoneDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<DnsPageToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Route53RecordType {
    #[serde(rename = "A")]
    A,
    #[serde(rename = "AAAA")]
    Aaaa,
    #[serde(rename = "CNAME")]
    Cname,
    #[serde(rename = "TXT")]
    Txt,
    #[serde(rename = "MX")]
    Mx,
    #[serde(rename = "SRV")]
    Srv,
    #[serde(rename = "CAA")]
    Caa,
    #[serde(rename = "NS")]
    Ns,
    #[serde(rename = "SOA")]
    Soa,
}

impl Route53RecordType {
    pub const fn core(self) -> ProviderDnsRecordType {
        match self {
            Self::A => ProviderDnsRecordType::A,
            Self::Aaaa => ProviderDnsRecordType::Aaaa,
            Self::Cname => ProviderDnsRecordType::Cname,
            Self::Txt => ProviderDnsRecordType::Txt,
            Self::Mx => ProviderDnsRecordType::Mx,
            Self::Srv => ProviderDnsRecordType::Srv,
            Self::Caa => ProviderDnsRecordType::Caa,
            Self::Ns => ProviderDnsRecordType::Ns,
            Self::Soa => ProviderDnsRecordType::Soa,
        }
    }

    pub const fn from_core(value: ProviderDnsRecordType) -> Option<Self> {
        match value {
            ProviderDnsRecordType::A => Some(Self::A),
            ProviderDnsRecordType::Aaaa => Some(Self::Aaaa),
            ProviderDnsRecordType::Cname => Some(Self::Cname),
            ProviderDnsRecordType::Txt => Some(Self::Txt),
            ProviderDnsRecordType::Mx => Some(Self::Mx),
            ProviderDnsRecordType::Srv => Some(Self::Srv),
            ProviderDnsRecordType::Caa => Some(Self::Caa),
            ProviderDnsRecordType::Ns => Some(Self::Ns),
            ProviderDnsRecordType::Soa => Some(Self::Soa),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route53RecordSetKey {
    pub owner: DnsOwnerName,
    pub record_type: Route53RecordType,
    pub set_identifier: Option<String>,
}

impl Route53RecordSetKey {
    pub fn core(&self) -> DnsRecordSetKey {
        DnsRecordSetKey {
            owner: self.owner.clone(),
            record_type: self.record_type.core(),
            routing: match &self.set_identifier {
                Some(set_identifier) => DnsRoutingIdentity::Route53 {
                    set_identifier: set_identifier.clone(),
                },
                None => DnsRoutingIdentity::Simple,
            },
        }
    }
}

/// Route 53 does not expose an RRset tag that Center can reserve. Cross-mode audit is best-effort
/// and not revision-queryable, so reads must not claim authoritative remote ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route53RecordControlDto {
    ExternalOrManual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53RecordSetDto {
    pub provider_account_id: CloudResourceId,
    pub zone_id: DnsZoneId,
    pub zone_apex: AbsoluteDnsName,
    pub zone_visibility: ZoneVisibility,
    /// Complete provider-safe Route 53 record model. Its typed Route 53 extension contains Alias,
    /// routing policy, and health-check identity without exposing raw AWS transport fields.
    pub record_set: ProviderDnsRecordSet,
    pub control: Route53RecordControlDto,
    pub revision: DnsRecordRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53RecordPageDto {
    pub zone: Route53ZoneDto,
    pub items: Vec<Route53RecordSetDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<DnsPageToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Route53RecordTtlDto {
    Inherited {},
    Seconds { seconds: u32 },
}

impl Route53RecordTtlDto {
    fn core(self) -> DnsTtl {
        match self {
            Self::Inherited {} => DnsTtl::Inherited,
            Self::Seconds { seconds } => DnsTtl::Seconds(seconds),
        }
    }
}

/// Lossless DNS character-string projection. Route 53 values are not assumed to be UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53OctetsDto {
    pub base64: String,
}

impl Route53OctetsDto {
    fn decode(&self) -> CoreResult<Vec<u8>> {
        let decoded = URL_SAFE_NO_PAD.decode(&self.base64).map_err(|_| {
            CoreError::Conflict("Route 53 DNS octets are not valid base64url".to_string())
        })?;
        if URL_SAFE_NO_PAD.encode(&decoded) != self.base64 {
            return Err(CoreError::Conflict(
                "Route 53 DNS octets are not canonical base64url".to_string(),
            ));
        }
        Ok(decoded)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "UPPERCASE",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Route53RecordValueDto {
    A {
        address: Ipv4Addr,
    },
    Aaaa {
        address: Ipv6Addr,
    },
    Cname {
        target: AbsoluteDnsName,
    },
    Txt {
        segments: Vec<Route53OctetsDto>,
    },
    Mx {
        preference: u16,
        exchange: AbsoluteDnsName,
    },
    Srv {
        priority: u16,
        weight: u16,
        port: u16,
        target: AbsoluteDnsName,
    },
    Caa {
        flags: u8,
        tag: CaaTag,
        value: Route53OctetsDto,
    },
    Ns {
        target: AbsoluteDnsName,
    },
    Soa {
        primary_name_server: AbsoluteDnsName,
        responsible_mailbox: AbsoluteDnsName,
        serial: u32,
        refresh: u32,
        retry: u32,
        expire: u32,
        minimum: u32,
    },
}

impl Route53RecordValueDto {
    fn core(&self) -> CoreResult<DnsRecordSetValue> {
        Ok(match self {
            Self::A { address } => DnsRecordSetValue::A { address: *address },
            Self::Aaaa { address } => DnsRecordSetValue::Aaaa { address: *address },
            Self::Cname { target } => DnsRecordSetValue::Cname {
                target: target.clone(),
            },
            Self::Txt { segments } => DnsRecordSetValue::Txt {
                value: DnsTxtValue::new(
                    segments
                        .iter()
                        .map(|value| value.decode().and_then(DnsCharacterString::new))
                        .collect::<CoreResult<_>>()?,
                )?,
            },
            Self::Mx {
                preference,
                exchange,
            } => DnsRecordSetValue::Mx {
                preference: *preference,
                exchange: exchange.clone(),
            },
            Self::Srv {
                priority,
                weight,
                port,
                target,
            } => DnsRecordSetValue::Srv {
                priority: *priority,
                weight: *weight,
                port: *port,
                target: target.clone(),
            },
            Self::Caa { flags, tag, value } => DnsRecordSetValue::Caa {
                flags: *flags,
                tag: tag.clone(),
                value: DnsCharacterString::new(value.decode()?)?,
            },
            Self::Ns { target } => DnsRecordSetValue::Ns {
                target: target.clone(),
            },
            Self::Soa {
                primary_name_server,
                responsible_mailbox,
                serial,
                refresh,
                retry,
                expire,
                minimum,
            } => DnsRecordSetValue::Soa {
                primary_name_server: primary_name_server.clone(),
                responsible_mailbox: responsible_mailbox.clone(),
                serial: *serial,
                refresh: *refresh,
                retry: *retry,
                expire: *expire,
                minimum: *minimum,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53AliasTargetDto {
    pub target_zone_id: DnsZoneId,
    pub target: AbsoluteDnsName,
    pub evaluate_target_health: bool,
}

impl Route53AliasTargetDto {
    fn core(&self) -> Route53AliasTarget {
        Route53AliasTarget {
            target_zone_id: self.target_zone_id.clone(),
            target: self.target.clone(),
            evaluate_target_health: self.evaluate_target_health,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Route53GeoLocationDto {
    Default {},
    Continent { code: String },
    Country { code: String },
    UsSubdivision { code: String },
}

impl Route53GeoLocationDto {
    fn core(&self) -> Route53GeoLocation {
        match self {
            Self::Default {} => Route53GeoLocation::Default,
            Self::Continent { code } => Route53GeoLocation::Continent { code: code.clone() },
            Self::Country { code } => Route53GeoLocation::Country { code: code.clone() },
            Self::UsSubdivision { code } => {
                Route53GeoLocation::UsSubdivision { code: code.clone() }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Route53RoutingPolicyDto {
    Weighted { weight: u8 },
    Failover { role: Route53FailoverRole },
    Latency { region: String },
    Geolocation { location: Route53GeoLocationDto },
    Multivalue {},
}

impl Route53RoutingPolicyDto {
    fn core(&self) -> Route53RoutingPolicy {
        match self {
            Self::Weighted { weight } => Route53RoutingPolicy::Weighted { weight: *weight },
            Self::Failover { role } => Route53RoutingPolicy::Failover { role: *role },
            Self::Latency { region } => Route53RoutingPolicy::Latency {
                region: region.clone(),
            },
            Self::Geolocation { location } => Route53RoutingPolicy::Geolocation {
                location: location.core(),
            },
            Self::Multivalue {} => Route53RoutingPolicy::Multivalue,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Route53RecordMutationGuardDto {
    MustNotExist {},
    MatchRevision { revision: DnsRecordRevision },
}

impl Route53RecordMutationGuardDto {
    pub fn expected_revision(&self) -> Option<&DnsRecordRevision> {
        match self {
            Self::MustNotExist {} => None,
            Self::MatchRevision { revision } => Some(revision),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53RecordSetDesiredDto {
    pub ttl: Route53RecordTtlDto,
    pub values: Vec<Route53RecordValueDto>,
    pub alias_target: Option<Route53AliasTargetDto>,
    pub routing_policy: Option<Route53RoutingPolicyDto>,
    pub health_check_id: Option<Route53HealthCheckId>,
}

impl Route53RecordSetDesiredDto {
    pub fn record_set(&self, key: &Route53RecordSetKey) -> CoreResult<ProviderDnsRecordSet> {
        if matches!(key.record_type, Route53RecordType::Soa) {
            return Err(CoreError::Conflict(
                "SOA changes require the hosted-zone lifecycle contract".to_string(),
            ));
        }
        if self.values.len() > 1_000 {
            return Err(CoreError::Conflict(
                "Route 53 RRset value count is invalid".to_string(),
            ));
        }
        let values = self
            .values
            .iter()
            .map(Route53RecordValueDto::core)
            .collect::<CoreResult<BTreeSet<_>>>()?;
        if values.len() != self.values.len()
            || values
                .iter()
                .any(|value| value.record_type() != key.record_type.core())
        {
            return Err(CoreError::Conflict(
                "Route 53 RRset values do not match the path identity".to_string(),
            ));
        }
        let alias_target = self.alias_target.as_ref().map(Route53AliasTargetDto::core);
        if alias_target.is_some()
            && (!values.is_empty() || !matches!(self.ttl, Route53RecordTtlDto::Inherited {}))
        {
            return Err(CoreError::Conflict(
                "Route 53 Alias RRsets require inherited TTL and no record values".to_string(),
            ));
        }
        if alias_target.is_none() && values.is_empty() {
            return Err(CoreError::Conflict(
                "Route 53 non-Alias RRsets require at least one record value".to_string(),
            ));
        }
        let routing_policy = self
            .routing_policy
            .as_ref()
            .map(Route53RoutingPolicyDto::core);
        let extension =
            if alias_target.is_some() || routing_policy.is_some() || self.health_check_id.is_some()
            {
                Some(DnsRecordExtension::Route53 {
                    alias_target,
                    routing_policy,
                    health_check_id: self.health_check_id.clone(),
                })
            } else {
                None
            };
        Ok(ProviderDnsRecordSet {
            key: key.core(),
            ttl: self.ttl.core(),
            values,
            extension,
        })
    }
}

/// Desired RRset state and optimistic guard. All record identity remains path-owned.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53RecordSetPutRequest {
    pub guard: Route53RecordMutationGuardDto,
    pub desired: Route53RecordSetDesiredDto,
}

impl Route53RecordSetPutRequest {
    pub fn record_set(&self, key: &Route53RecordSetKey) -> CoreResult<ProviderDnsRecordSet> {
        self.desired.record_set(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53RecordSetDeleteRequest {
    pub expected_revision: DnsRecordRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53BatchRecordKeyDto {
    pub owner: DnsOwnerName,
    pub record_type: Route53RecordType,
    pub set_identifier: Option<String>,
}

impl Route53BatchRecordKeyDto {
    pub fn key(&self) -> Route53RecordSetKey {
        Route53RecordSetKey {
            owner: self.owner.clone(),
            record_type: self.record_type,
            set_identifier: self.set_identifier.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Route53RecordBatchChangeDto {
    Create {
        key: Route53BatchRecordKeyDto,
        desired: Route53RecordSetDesiredDto,
    },
    Replace {
        key: Route53BatchRecordKeyDto,
        expected_revision: DnsRecordRevision,
        desired: Route53RecordSetDesiredDto,
    },
    Delete {
        key: Route53BatchRecordKeyDto,
        expected_revision: DnsRecordRevision,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53RecordChangeBatchRequest {
    pub changes: Vec<Route53RecordBatchChangeDto>,
}

impl Route53RecordChangeBatchRequest {
    pub fn validate_shape(&self) -> CoreResult<()> {
        if self.changes.is_empty() || self.changes.len() > MAX_ROUTE53_CHANGE_BATCH {
            return Err(CoreError::Conflict(
                "Route 53 change batch size is invalid".to_string(),
            ));
        }
        let mut keys = BTreeSet::new();
        for change in &self.changes {
            let key = match change {
                Route53RecordBatchChangeDto::Create { key, desired } => {
                    let key = key.key();
                    key.core().validate()?;
                    desired.record_set(&key)?;
                    key
                }
                Route53RecordBatchChangeDto::Replace {
                    key,
                    expected_revision,
                    desired,
                } => {
                    expected_revision.validate()?;
                    let key = key.key();
                    key.core().validate()?;
                    desired.record_set(&key)?;
                    key
                }
                Route53RecordBatchChangeDto::Delete {
                    key,
                    expected_revision,
                } => {
                    expected_revision.validate()?;
                    let key = key.key();
                    key.core().validate()?;
                    key
                }
            };
            if !keys.insert(key.core()) {
                return Err(CoreError::Conflict(
                    "Route 53 change batch contains duplicate identities".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Route53ProviderApplicationDto {
    Pending,
    InSync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route53AuthoritativeConvergenceDto {
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53ChangeReceiptDto {
    pub receipt: DnsChangeId,
    pub provider_application: Route53ProviderApplicationDto,
    pub authoritative_convergence: Route53AuthoritativeConvergenceDto,
}

impl Route53ChangeReceiptDto {
    pub fn from_core(receipt: DnsChangeReceipt) -> CoreResult<Self> {
        receipt.validate()?;
        let provider_application = match (receipt.state, receipt.propagation) {
            (
                DnsChangeState::Pending,
                DnsPropagationState::Pending | DnsPropagationState::Unknown,
            ) => Route53ProviderApplicationDto::Pending,
            (DnsChangeState::ProviderCommitted, DnsPropagationState::ProviderReportedApplied) => {
                Route53ProviderApplicationDto::InSync
            }
            _ => {
                return Err(CoreError::Conflict(
                    "Route 53 change receipt state is invalid".to_string(),
                ))
            }
        };
        Ok(Self {
            receipt: receipt.id,
            provider_application,
            authoritative_convergence: Route53AuthoritativeConvergenceDto::NotChecked,
        })
    }
}

#[async_trait]
pub trait Route53DnsAdminService: Send + Sync {
    async fn list_zones(
        &self,
        account_id: &CloudResourceId,
        page: &Route53ZonePageRequest,
    ) -> Result<Route53ZonePageDto, Route53DnsAdminError>;

    async fn get_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> Result<Route53ZoneDto, Route53DnsAdminError>;

    async fn list_record_sets(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        page: &Route53RecordPageRequest,
    ) -> Result<Route53RecordPageDto, Route53DnsAdminError>;

    async fn get_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &Route53RecordSetKey,
    ) -> Result<Route53RecordSetDto, Route53DnsAdminError>;
}

pub type SharedRoute53DnsAdminService = Arc<dyn Route53DnsAdminService>;

#[async_trait]
pub trait Route53DnsWriteAdminService: Send + Sync {
    async fn put_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &Route53RecordSetKey,
        request: &Route53RecordSetPutRequest,
    ) -> Result<Route53ChangeReceiptDto, Route53DnsAdminError>;

    async fn delete_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &Route53RecordSetKey,
        request: &Route53RecordSetDeleteRequest,
    ) -> Result<Route53ChangeReceiptDto, Route53DnsAdminError>;

    async fn apply_change_batch(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        request: &Route53RecordChangeBatchRequest,
    ) -> Result<Route53ChangeReceiptDto, Route53DnsAdminError>;

    async fn get_change(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        receipt: &DnsChangeId,
    ) -> Result<Route53ChangeReceiptDto, Route53DnsAdminError>;
}

pub type SharedRoute53DnsWriteAdminService = Arc<dyn Route53DnsWriteAdminService>;

/// Provider-specific public hosted-zone create request. The idempotency key is explicit so a
/// caller can safely recover a lost response without letting Center invent a new AWS zone.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53ZoneCreateRequest {
    pub apex: AbsoluteDnsName,
    pub idempotency_key: IdempotencyKey,
}

/// Exact fresh-observation guard for the narrow hosted-zone delete surface.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53ZoneDeleteRequest {
    pub apex: AbsoluteDnsName,
    pub revision: ZoneLifecycleRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53ZoneLifecycleMutationDto {
    pub mutation_id: edgion_center_core::ZoneLifecycleMutationId,
    pub provider_application: Route53ZoneLifecycleApplicationDto,
    pub authoritative_convergence: Route53AuthoritativeConvergenceDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route53ZoneLifecycleApplicationDto {
    Pending,
    Accepted,
}

impl Route53ZoneLifecycleMutationDto {
    pub fn from_core(value: ZoneLifecycleMutationReceipt) -> CoreResult<Self> {
        let provider_application = match value.state {
            ZoneLifecycleMutationState::Pending => Route53ZoneLifecycleApplicationDto::Pending,
            ZoneLifecycleMutationState::Succeeded => Route53ZoneLifecycleApplicationDto::Accepted,
            ZoneLifecycleMutationState::Failed | ZoneLifecycleMutationState::UnknownOutcome => {
                return Err(CoreError::Conflict(
                    "invalid Route 53 lifecycle receipt".into(),
                ));
            }
        };
        Ok(Self {
            mutation_id: value.mutation_id,
            provider_application,
            authoritative_convergence: Route53AuthoritativeConvergenceDto::NotChecked,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53ZoneLifecycleObservationDto {
    pub zone: Route53ZoneDto,
    pub revision: ZoneLifecycleRevision,
    pub authoritative_nameservers: Vec<AbsoluteDnsName>,
    pub delegation: edgion_center_core::DelegationObservation,
    pub readiness: edgion_center_core::ZoneReadiness,
    pub dnssec: edgion_center_core::DnssecObservation,
    pub non_default_record_count: u64,
}

impl Route53ZoneLifecycleObservationDto {
    pub fn from_core(value: ZoneLifecycleObservation) -> CoreResult<Self> {
        value.validate()?;
        if value.zone.provider != CloudProvider::Aws
            || value.zone.visibility != ZoneVisibility::Public
        {
            return Err(CoreError::Conflict(
                "invalid Route 53 lifecycle observation".into(),
            ));
        }
        Ok(Self {
            zone: Route53ZoneDto {
                provider_account_id: value.zone.provider_account_id,
                zone_id: value.zone.zone_id,
                apex: value.zone.apex,
                visibility: value.zone.visibility,
            },
            revision: value.revision,
            authoritative_nameservers: value.authoritative_nameservers.into_iter().collect(),
            delegation: value.delegation,
            readiness: value.readiness,
            dnssec: value.dnssec,
            non_default_record_count: value.non_default_record_count,
        })
    }
}

#[async_trait]
pub trait Route53ZoneLifecycleAdminService: Send + Sync {
    async fn create_zone(
        &self,
        account_id: &CloudResourceId,
        request: &ZoneCreationRequest,
    ) -> Result<Route53ZoneLifecycleMutationDto, Route53DnsAdminError>;

    async fn observe_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        apex: &AbsoluteDnsName,
    ) -> Result<Route53ZoneLifecycleObservationDto, Route53DnsAdminError>;

    async fn delete_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        request: &Route53ZoneDeleteRequest,
    ) -> Result<Route53ZoneLifecycleMutationDto, Route53DnsAdminError>;
}

pub type SharedRoute53ZoneLifecycleAdminService = Arc<dyn Route53ZoneLifecycleAdminService>;

impl Route53ZoneDto {
    fn validate(&self) -> CoreResult<()> {
        self.provider_account_id.validate()?;
        validate_route53_zone_id(&self.zone_id)?;
        if self.visibility != ZoneVisibility::Public {
            return Err(CoreError::Conflict(
                "Route 53 Admin API exposes only public zones".to_string(),
            ));
        }
        self.zone_ref().validate()
    }

    fn zone_ref(&self) -> DnsZoneRef {
        DnsZoneRef {
            provider_account_id: self.provider_account_id.clone(),
            provider: CloudProvider::Aws,
            zone_id: self.zone_id.clone(),
            apex: self.apex.clone(),
            visibility: self.visibility,
        }
    }
}

impl Route53ZonePageDto {
    fn validate(&self, account_id: &CloudResourceId, requested_limit: u16) -> CoreResult<()> {
        if !(1..=MAX_ROUTE53_PAGE_LIMIT).contains(&requested_limit)
            || self.items.len() > requested_limit as usize
        {
            return Err(CoreError::Conflict(
                "Route 53 zone page exceeds the requested limit".to_string(),
            ));
        }
        if let Some(cursor) = &self.next_cursor {
            cursor.validate()?;
        }
        let mut identities = std::collections::BTreeSet::new();
        for zone in &self.items {
            zone.validate()?;
            if &zone.provider_account_id != account_id || !identities.insert(zone.zone_id.clone()) {
                return Err(CoreError::Conflict(
                    "Route 53 zone page scope is inconsistent".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl Route53RecordSetDto {
    fn validate(&self) -> CoreResult<()> {
        self.provider_account_id.validate()?;
        validate_route53_zone_id(&self.zone_id)?;
        self.revision.validate()?;
        let zone = Route53ZoneDto {
            provider_account_id: self.provider_account_id.clone(),
            zone_id: self.zone_id.clone(),
            apex: self.zone_apex.clone(),
            visibility: self.zone_visibility,
        };
        zone.validate()?;
        self.record_set.validate(&zone.zone_ref())
    }

    fn validate_against_zone(&self, zone: &Route53ZoneDto) -> CoreResult<()> {
        self.validate()?;
        if self.provider_account_id != zone.provider_account_id
            || self.zone_id != zone.zone_id
            || self.zone_apex != zone.apex
            || self.zone_visibility != ZoneVisibility::Public
        {
            return Err(CoreError::Conflict(
                "Route 53 RRset scope is inconsistent".to_string(),
            ));
        }
        Ok(())
    }
}

impl Route53RecordPageDto {
    fn validate(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        requested_limit: u16,
    ) -> CoreResult<()> {
        self.zone.validate()?;
        if &self.zone.provider_account_id != account_id
            || &self.zone.zone_id != zone_id
            || !(1..=MAX_ROUTE53_PAGE_LIMIT).contains(&requested_limit)
            || self.items.len() > requested_limit as usize
        {
            return Err(CoreError::Conflict(
                "Route 53 RRset page scope is inconsistent".to_string(),
            ));
        }
        if let Some(cursor) = &self.next_cursor {
            cursor.validate()?;
        }
        let mut identities = std::collections::BTreeSet::new();
        for record in &self.items {
            record.validate_against_zone(&self.zone)?;
            if !identities.insert(record.record_set.key.clone()) {
                return Err(CoreError::Conflict(
                    "Route 53 RRset page contains duplicate identities".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route53PageQuery {
    limit: Option<String>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53RecordDetailQuery {
    owner: Option<String>,
    set_identifier: Option<String>,
}

fn parse_page(query: Route53PageQuery) -> Result<Route53ZonePageRequest, &'static str> {
    let limit = match query.limit {
        Some(value) => value
            .parse::<u16>()
            .ok()
            .filter(|limit| (1..=MAX_ROUTE53_PAGE_LIMIT).contains(limit))
            .ok_or("invalid_page")?,
        None => DEFAULT_ROUTE53_PAGE_LIMIT,
    };
    let cursor = query
        .cursor
        .map(DnsPageToken::new)
        .transpose()
        .map_err(|_| "invalid_page")?;
    Ok(Route53ZonePageRequest { limit, cursor })
}

fn parse_record_page(query: Route53PageQuery) -> Result<Route53RecordPageRequest, &'static str> {
    let page = parse_page(query)?;
    Ok(Route53RecordPageRequest {
        limit: page.limit,
        cursor: page.cursor,
    })
}

fn parse_zone_id(value: String) -> Result<DnsZoneId, &'static str> {
    let value = DnsZoneId::new(value).map_err(|_| "invalid_zone_id")?;
    validate_route53_zone_id(&value).map_err(|_| "invalid_zone_id")?;
    Ok(value)
}

fn parse_change_receipt(value: String) -> Result<DnsChangeId, &'static str> {
    if value.len() > 4096 {
        return Err("invalid_change_receipt");
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(&value)
        .map_err(|_| "invalid_change_receipt")?;
    if decoded.len() <= 32 || URL_SAFE_NO_PAD.encode(decoded) != value {
        return Err("invalid_change_receipt");
    }
    DnsChangeId::new(value).map_err(|_| "invalid_change_receipt")
}

fn validate_route53_zone_id(value: &DnsZoneId) -> CoreResult<()> {
    let value = value.as_str();
    if value.is_empty()
        || value.len() > 32
        || !value.starts_with('Z')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return Err(CoreError::Conflict(
            "Route 53 hosted zone ID is invalid".to_string(),
        ));
    }
    Ok(())
}

fn parse_record_identity(
    account_id: String,
    zone_id: String,
    record_type: String,
    query: Route53RecordDetailQuery,
) -> Result<(CloudResourceId, DnsZoneId, Route53RecordSetKey), &'static str> {
    let account_id = CloudResourceId::new(account_id).map_err(|_| "invalid_account_id")?;
    let zone_id = parse_zone_id(zone_id)?;
    let record_type = match record_type.as_str() {
        "A" => Route53RecordType::A,
        "AAAA" => Route53RecordType::Aaaa,
        "CNAME" => Route53RecordType::Cname,
        "TXT" => Route53RecordType::Txt,
        "MX" => Route53RecordType::Mx,
        "SRV" => Route53RecordType::Srv,
        "CAA" => Route53RecordType::Caa,
        "NS" => Route53RecordType::Ns,
        "SOA" => Route53RecordType::Soa,
        _ => return Err("invalid_record_type"),
    };
    let owner = query
        .owner
        .and_then(|owner| DnsOwnerName::new(owner).ok())
        .ok_or("invalid_record_owner")?;
    let key = Route53RecordSetKey {
        owner,
        record_type,
        set_identifier: query.set_identifier,
    };
    key.core()
        .validate()
        .map_err(|_| "invalid_record_identity")?;
    Ok((account_id, zone_id, key))
}

fn error_response(status: StatusCode, code: &'static str) -> Response {
    (status, Json(ApiResponse::<()>::err_body(code.to_string()))).into_response()
}

fn json_rejection_response(rejection: JsonRejection) -> Response {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        error_response(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large")
    } else {
        error_response(StatusCode::BAD_REQUEST, "invalid_request")
    }
}

fn map_service_error(error: Route53DnsAdminError) -> Response {
    let (status, code) = match error {
        Route53DnsAdminError::InvalidRequest => (StatusCode::BAD_REQUEST, "invalid_request"),
        Route53DnsAdminError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        Route53DnsAdminError::Conflict => (StatusCode::CONFLICT, "conflict"),
        Route53DnsAdminError::UnknownOutcome => {
            (StatusCode::SERVICE_UNAVAILABLE, "unknown_outcome")
        }
        Route53DnsAdminError::RestartRequired => {
            (StatusCode::CONFLICT, "pagination_restart_required")
        }
        Route53DnsAdminError::InvalidProviderObservation => (
            StatusCode::SERVICE_UNAVAILABLE,
            "invalid_provider_observation",
        ),
        Route53DnsAdminError::Unavailable => {
            (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
        }
    };
    error_response(status, code)
}

pub async fn list_zones(
    State(state): State<ApiState>,
    Path(account_id): Path<String>,
    Query(query): Query<Route53PageQuery>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let page = match parse_page(query) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let Some(service) = state.route53_dns_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.list_zones(&account_id, &page).await {
        Ok(result) if result.validate(&account_id, page.limit).is_ok() => {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response"),
        Err(error) => map_service_error(error),
    }
}

pub async fn get_zone(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let zone_id = match parse_zone_id(zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let Some(service) = state.route53_dns_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.get_zone(&account_id, &zone_id).await {
        Ok(result)
            if result.validate().is_ok()
                && result.provider_account_id == account_id
                && result.zone_id == zone_id =>
        {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response"),
        Err(error) => map_service_error(error),
    }
}

pub async fn list_record_sets(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    Query(query): Query<Route53PageQuery>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let zone_id = match parse_zone_id(zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let page = match parse_record_page(query) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let Some(service) = state.route53_dns_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.list_record_sets(&account_id, &zone_id, &page).await {
        Ok(result) if result.validate(&account_id, &zone_id, page.limit).is_ok() => {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response"),
        Err(error) => map_service_error(error),
    }
}

pub async fn get_record_set(
    State(state): State<ApiState>,
    Path((account_id, zone_id, record_type)): Path<(String, String, String)>,
    Query(query): Query<Route53RecordDetailQuery>,
) -> Response {
    let (account_id, zone_id, key) =
        match parse_record_identity(account_id, zone_id, record_type, query) {
            Ok(value) => value,
            Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
        };
    let Some(service) = state.route53_dns_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.get_record_set(&account_id, &zone_id, &key).await {
        Ok(result)
            if result.validate().is_ok()
                && result.provider_account_id == account_id
                && result.zone_id == zone_id
                && result.record_set.key == key.core() =>
        {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response"),
        Err(error) => map_service_error(error),
    }
}

pub async fn put_record_set(
    State(state): State<ApiState>,
    Path((account_id, zone_id, record_type)): Path<(String, String, String)>,
    Query(query): Query<Route53RecordDetailQuery>,
    body: Result<Json<Route53RecordSetPutRequest>, JsonRejection>,
) -> Response {
    let (account_id, zone_id, key) =
        match parse_record_identity(account_id, zone_id, record_type, query) {
            Ok(value) => value,
            Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
        };
    let Json(request) = match body {
        Ok(value) => value,
        Err(rejection) => return json_rejection_response(rejection),
    };
    if request.record_set(&key).is_err()
        || request
            .guard
            .expected_revision()
            .is_some_and(|revision| revision.validate().is_err())
    {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(service) = state.route53_dns_write_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service
        .put_record_set(&account_id, &zone_id, &key, &request)
        .await
    {
        Ok(result) if result.receipt.validate().is_ok() => {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => map_service_error(Route53DnsAdminError::UnknownOutcome),
        Err(error) => map_service_error(error),
    }
}

pub async fn delete_record_set(
    State(state): State<ApiState>,
    Path((account_id, zone_id, record_type)): Path<(String, String, String)>,
    Query(query): Query<Route53RecordDetailQuery>,
    body: Result<Json<Route53RecordSetDeleteRequest>, JsonRejection>,
) -> Response {
    let (account_id, zone_id, key) =
        match parse_record_identity(account_id, zone_id, record_type, query) {
            Ok(value) => value,
            Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
        };
    let Json(request) = match body {
        Ok(value) => value,
        Err(rejection) => return json_rejection_response(rejection),
    };
    if request.expected_revision.validate().is_err() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(service) = state.route53_dns_write_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service
        .delete_record_set(&account_id, &zone_id, &key, &request)
        .await
    {
        Ok(result) if result.receipt.validate().is_ok() => {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => map_service_error(Route53DnsAdminError::UnknownOutcome),
        Err(error) => map_service_error(error),
    }
}

pub async fn apply_change_batch(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    body: Result<Json<Route53RecordChangeBatchRequest>, JsonRejection>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let zone_id = match parse_zone_id(zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let Json(request) = match body {
        Ok(value) => value,
        Err(rejection) => return json_rejection_response(rejection),
    };
    if request.validate_shape().is_err() {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(service) = state.route53_dns_write_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service
        .apply_change_batch(&account_id, &zone_id, &request)
        .await
    {
        Ok(result) if result.receipt.validate().is_ok() => {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => map_service_error(Route53DnsAdminError::UnknownOutcome),
        Err(error) => map_service_error(error),
    }
}

pub async fn get_change(
    State(state): State<ApiState>,
    Path((account_id, zone_id, receipt)): Path<(String, String, String)>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let zone_id = match parse_zone_id(zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let receipt = match parse_change_receipt(receipt) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_change_receipt"),
    };
    let Some(service) = state.route53_dns_write_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.get_change(&account_id, &zone_id, &receipt).await {
        Ok(result) if result.receipt == receipt && result.receipt.validate().is_ok() => {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response"),
        Err(error) => map_service_error(error),
    }
}

pub async fn create_zone(
    State(state): State<ApiState>,
    Path(account_id): Path<String>,
    body: Result<Json<Route53ZoneCreateRequest>, JsonRejection>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let Json(request) = match body {
        Ok(value) => value,
        Err(rejection) => return json_rejection_response(rejection),
    };
    let creation = ZoneCreationRequest {
        provider_account_id: account_id.clone(),
        provider: CloudProvider::Aws,
        apex: request.apex,
        visibility: ZoneVisibility::Public,
        idempotency_key: request.idempotency_key,
    };
    let Some(service) = state.route53_zone_lifecycle_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.create_zone(&account_id, &creation).await {
        Ok(result) => Json(ApiResponse::ok_body(result)).into_response(),
        Err(error) => map_service_error(error),
    }
}

pub async fn observe_zone_lifecycle(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    Query(query): Query<Route53ZoneLifecycleQuery>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let zone_id = match parse_zone_id(zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let apex = match query
        .apex
        .and_then(|value| AbsoluteDnsName::new(value).ok())
    {
        Some(value) => value,
        None => return error_response(StatusCode::BAD_REQUEST, "invalid_zone_apex"),
    };
    let Some(service) = state.route53_zone_lifecycle_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.observe_zone(&account_id, &zone_id, &apex).await {
        Ok(result)
            if result.zone.provider_account_id == account_id
                && result.zone.zone_id == zone_id
                && result.zone.apex == apex
                && result.zone.validate().is_ok() =>
        {
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Ok(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response"),
        Err(error) => map_service_error(error),
    }
}

pub async fn delete_zone(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    body: Result<Json<Route53ZoneDeleteRequest>, JsonRejection>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let zone_id = match parse_zone_id(zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let Json(request) = match body {
        Ok(value) => value,
        Err(rejection) => return json_rejection_response(rejection),
    };
    let Some(service) = state.route53_zone_lifecycle_admin.as_deref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.delete_zone(&account_id, &zone_id, &request).await {
        Ok(result) => Json(ApiResponse::ok_body(result)).into_response(),
        Err(error) => map_service_error(error),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Route53ZoneLifecycleQuery {
    apex: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_and_record_identity_are_strict_and_bounded() {
        assert_eq!(parse_page(Route53PageQuery::default()).unwrap().limit, 50);
        assert!(parse_page(Route53PageQuery {
            limit: Some("301".into()),
            cursor: None,
        })
        .is_err());
        assert!(parse_zone_id("/hostedzone/Z1234567890".into()).is_err());
        assert!(parse_zone_id("z1234567890".into()).is_err());
        assert!(parse_zone_id("Z1234567890".into()).is_ok());

        let (_, _, simple) = parse_record_identity(
            "aws-main".into(),
            "Z1234567890".into(),
            "A".into(),
            Route53RecordDetailQuery {
                owner: Some("www.example.com".into()),
                set_identifier: None,
            },
        )
        .unwrap();
        assert_eq!(simple.core().routing, DnsRoutingIdentity::Simple);

        let (_, _, routed) = parse_record_identity(
            "aws-main".into(),
            "Z1234567890".into(),
            "AAAA".into(),
            Route53RecordDetailQuery {
                owner: Some("www.example.com".into()),
                set_identifier: Some("primary".into()),
            },
        )
        .unwrap();
        assert_eq!(
            routed.core().routing,
            DnsRoutingIdentity::Route53 {
                set_identifier: "primary".into()
            }
        );
    }

    #[test]
    fn wire_contract_exposes_only_honest_control_state() {
        assert_eq!(
            serde_json::to_value(Route53RecordControlDto::ExternalOrManual).unwrap(),
            serde_json::json!("external_or_manual")
        );
        assert_eq!(
            serde_json::to_value(Route53RecordType::Aaaa).unwrap(),
            serde_json::json!("AAAA")
        );
    }

    #[test]
    fn simple_a_and_txt_write_bodies_build_portable_record_sets() {
        let a_key = Route53RecordSetKey {
            owner: DnsOwnerName::new("www.example.com").unwrap(),
            record_type: Route53RecordType::A,
            set_identifier: None,
        };
        let a: Route53RecordSetPutRequest = serde_json::from_value(serde_json::json!({
            "guard": { "type": "must_not_exist" },
            "desired": {
                "ttl": { "type": "seconds", "seconds": 60 },
                "values": [{ "type": "A", "address": "192.0.2.1" }],
                "aliasTarget": null,
                "routingPolicy": null,
                "healthCheckId": null
            }
        }))
        .unwrap();
        let a = a.record_set(&a_key).unwrap();
        assert_eq!(a.extension, None);
        assert_eq!(a.ttl, DnsTtl::Seconds(60));

        let txt_key = Route53RecordSetKey {
            owner: DnsOwnerName::new("_check.example.com").unwrap(),
            record_type: Route53RecordType::Txt,
            set_identifier: None,
        };
        let txt: Route53RecordSetPutRequest = serde_json::from_value(serde_json::json!({
            "guard": { "type": "must_not_exist" },
            "desired": {
                "ttl": { "type": "seconds", "seconds": 300 },
                "values": [{ "type": "TXT", "segments": [{ "base64": "aGk" }] }],
                "aliasTarget": null,
                "routingPolicy": null,
                "healthCheckId": null
            }
        }))
        .unwrap();
        let txt = txt.record_set(&txt_key).unwrap();
        assert_eq!(txt.extension, None);
        assert_eq!(txt.values.len(), 1);
    }

    #[test]
    fn alias_write_body_requires_the_native_route53_alias_shape() {
        let key = Route53RecordSetKey {
            owner: DnsOwnerName::new("api.example.com").unwrap(),
            record_type: Route53RecordType::A,
            set_identifier: None,
        };
        let alias: Route53RecordSetPutRequest = serde_json::from_value(serde_json::json!({
            "guard": { "type": "must_not_exist" },
            "desired": {
                "ttl": { "type": "inherited" },
                "values": [],
                "aliasTarget": {
                    "targetZoneId": "Z0123456789ABCDEF",
                    "target": "dualstack.example.elb.amazonaws.com.",
                    "evaluateTargetHealth": false
                },
                "routingPolicy": { "type": "weighted", "weight": 10 },
                "healthCheckId": "12345678-1234-1234-1234-123456789012"
            }
        }))
        .unwrap();
        let record = alias.record_set(&key).unwrap();
        assert!(record.values.is_empty());
        assert_eq!(record.ttl, DnsTtl::Inherited);

        let invalid: Route53RecordSetPutRequest = serde_json::from_value(serde_json::json!({
            "guard": { "type": "must_not_exist" },
            "desired": {
                "ttl": { "type": "seconds", "seconds": 60 },
                "values": [{ "type": "A", "address": "192.0.2.1" }],
                "aliasTarget": {
                    "targetZoneId": "Z0123456789ABCDEF",
                    "target": "dualstack.example.elb.amazonaws.com.",
                    "evaluateTargetHealth": false
                },
                "routingPolicy": null,
                "healthCheckId": null
            }
        }))
        .unwrap();
        assert!(invalid.record_set(&key).is_err());
    }

    #[test]
    fn write_body_rejects_path_identity_and_nested_unknown_fields() {
        let base = serde_json::json!({
            "guard": { "type": "must_not_exist" },
            "desired": {
                "ttl": { "type": "seconds", "seconds": 60 },
                "values": [{ "type": "A", "address": "192.0.2.1" }],
                "aliasTarget": null,
                "routingPolicy": null,
                "healthCheckId": null
            },
            "owner": "www.example.com"
        });
        assert!(serde_json::from_value::<Route53RecordSetPutRequest>(base).is_err());
        let nested = serde_json::json!({
            "guard": { "type": "must_not_exist", "revision": "forbidden" },
            "desired": {
                "ttl": { "type": "seconds", "seconds": 60 },
                "values": [{ "type": "A", "address": "192.0.2.1" }],
                "aliasTarget": null,
                "routingPolicy": null,
                "healthCheckId": null
            }
        });
        assert!(serde_json::from_value::<Route53RecordSetPutRequest>(nested).is_err());
    }

    #[test]
    fn opaque_receipts_and_unit_request_variants_are_canonical_and_strict() {
        let canonical = URL_SAFE_NO_PAD.encode([7_u8; 33]);
        assert!(parse_change_receipt(canonical).is_ok());
        for invalid in [
            "C123".to_string(),
            "/change/C123".to_string(),
            format!("{}=", URL_SAFE_NO_PAD.encode([7_u8; 33])),
            URL_SAFE_NO_PAD.encode([7_u8; 32]),
        ] {
            assert!(parse_change_receipt(invalid).is_err());
        }

        assert!(
            serde_json::from_value::<Route53RecordMutationGuardDto>(serde_json::json!({
                "type": "must_not_exist", "revision": "forbidden"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<Route53RecordTtlDto>(serde_json::json!({
                "type": "inherited", "seconds": 1
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<Route53GeoLocationDto>(serde_json::json!({
                "type": "default", "code": "EU"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<Route53RoutingPolicyDto>(serde_json::json!({
                "type": "multivalue", "weight": 1
            }))
            .is_err()
        );
    }
}
