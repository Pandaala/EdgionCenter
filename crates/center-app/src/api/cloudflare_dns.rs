//! Cloudflare-specific, read-only DNS zone Admin API.
//!
//! The HTTP layer depends only on this sanitized service port. Cloudflare HTTP clients, SDK
//! response types, credentials, and raw provider failures must remain behind the composition
//! boundary.

use std::{
    collections::BTreeSet,
    net::{Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use async_trait::async_trait;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    AbsoluteDnsName, CaaTag, CloudProvider, CloudResourceId, CloudflareCnameFlattening,
    CloudflareProxyOptions, CoreError, CoreResult, DnsCharacterString, DnsOwnerName, DnsPageToken,
    DnsRecordExtension, DnsRecordObjectId, DnsRecordRevision, DnsRecordSetKey, DnsRecordSetValue,
    DnsRoutingIdentity, DnsTtl, DnsTxtValue, DnsZoneId, DnsZoneRef, ProviderDnsRecordSet,
    ProviderDnsRecordType, ZoneVisibility,
};
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::ApiResponse;

const DEFAULT_PAGE_LIMIT: u16 = 50;
const MAX_PAGE_LIMIT: u16 = 100;
const MAX_NAMESERVERS: usize = 20;
const CLOUDFLARE_ZONE_ID_LEN: usize = 32;
const CLOUDFLARE_RECORD_ID_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareZoneKind {
    Full,
    Partial,
    Secondary,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareZoneStatus {
    Initializing,
    Pending,
    Active,
    Moved,
}

/// Explicitly sanitized Cloudflare zone projection. It contains no account-native identifier,
/// credentials, raw provider metadata, headers, or response bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareZoneDto {
    pub provider_account_id: CloudResourceId,
    pub zone_id: DnsZoneId,
    pub name: AbsoluteDnsName,
    pub kind: CloudflareZoneKind,
    pub status: CloudflareZoneStatus,
    pub visibility: ZoneVisibility,
    pub nameservers: Vec<AbsoluteDnsName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<DnsRecordRevision>,
}

impl CloudflareZoneDto {
    fn validate(&self) -> CoreResult<()> {
        self.provider_account_id.validate()?;
        validate_cloudflare_zone_id(&self.zone_id)?;
        if let Some(revision) = &self.revision {
            revision.validate()?;
        }
        let visibility_matches_kind = matches!(
            (self.kind, self.visibility),
            (CloudflareZoneKind::Internal, ZoneVisibility::Private)
                | (
                    CloudflareZoneKind::Full
                        | CloudflareZoneKind::Partial
                        | CloudflareZoneKind::Secondary,
                    ZoneVisibility::Public
                )
        );
        if !visibility_matches_kind {
            return Err(CoreError::Conflict(
                "Cloudflare zone kind and visibility are inconsistent".to_string(),
            ));
        }
        if self.nameservers.len() > MAX_NAMESERVERS {
            return Err(CoreError::Conflict(
                "Cloudflare zone nameserver projection exceeds its bound".to_string(),
            ));
        }
        let unique: BTreeSet<_> = self
            .nameservers
            .iter()
            .map(AbsoluteDnsName::as_str)
            .collect();
        if unique.len() != self.nameservers.len() {
            return Err(CoreError::Conflict(
                "Cloudflare zone nameserver projection contains duplicates".to_string(),
            ));
        }
        Ok(())
    }
}

fn validate_cloudflare_zone_id(zone_id: &DnsZoneId) -> CoreResult<()> {
    zone_id.validate()?;
    let value = zone_id.as_str();
    if value.len() != CLOUDFLARE_ZONE_ID_LEN
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(CoreError::InvalidIdentifier {
            kind: "Cloudflare zone ID",
            value: value.to_string(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareZonePageRequest {
    pub limit: u16,
    pub cursor: Option<DnsPageToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareZonePageDto {
    pub items: Vec<CloudflareZoneDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<DnsPageToken>,
}

impl CloudflareZonePageDto {
    fn validate(&self, account_id: &CloudResourceId, requested_limit: u16) -> CoreResult<()> {
        if self.items.len() > requested_limit as usize {
            return Err(CoreError::Conflict(
                "Cloudflare zone page exceeds the requested limit".to_string(),
            ));
        }
        if let Some(cursor) = &self.next_cursor {
            cursor.validate()?;
        }
        let mut zone_ids = BTreeSet::new();
        for zone in &self.items {
            zone.validate()?;
            if &zone.provider_account_id != account_id || !zone_ids.insert(zone.zone_id.as_str()) {
                return Err(CoreError::Conflict(
                    "Cloudflare zone page scope is inconsistent".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CloudflareRecordType {
    A,
    Aaaa,
    Cname,
    Txt,
    Mx,
    Srv,
    Caa,
    Ns,
    Soa,
}

impl CloudflareRecordType {
    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "A" => Some(Self::A),
            "AAAA" => Some(Self::Aaaa),
            "CNAME" => Some(Self::Cname),
            "TXT" => Some(Self::Txt),
            "MX" => Some(Self::Mx),
            "SRV" => Some(Self::Srv),
            "CAA" => Some(Self::Caa),
            "NS" => Some(Self::Ns),
            "SOA" => Some(Self::Soa),
            _ => None,
        }
    }

    fn core(self) -> ProviderDnsRecordType {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "seconds", rename_all = "snake_case")]
pub enum CloudflareRecordTtlDto {
    Automatic,
    Seconds(u32),
}

impl CloudflareRecordTtlDto {
    fn core(self) -> DnsTtl {
        match self {
            Self::Automatic => DnsTtl::Automatic,
            Self::Seconds(seconds) => DnsTtl::Seconds(seconds),
        }
    }
}

/// Lossless DNS character-string projection. TXT and CAA data are octets, not necessarily UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareOctetsDto {
    pub base64: String,
}

impl CloudflareOctetsDto {
    fn decode(&self) -> CoreResult<Vec<u8>> {
        let decoded = URL_SAFE_NO_PAD.decode(&self.base64).map_err(|_| {
            CoreError::Conflict("Cloudflare DNS octets are not valid base64url".to_string())
        })?;
        if URL_SAFE_NO_PAD.encode(&decoded) != self.base64 {
            return Err(CoreError::Conflict(
                "Cloudflare DNS octets are not canonical base64url".to_string(),
            ));
        }
        Ok(decoded)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(
    tag = "type",
    rename_all = "UPPERCASE",
    rename_all_fields = "camelCase"
)]
pub enum CloudflareRecordValueDto {
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
        segments: Vec<CloudflareOctetsDto>,
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
        value: CloudflareOctetsDto,
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

impl CloudflareRecordValueDto {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareRecordSetKey {
    pub owner: DnsOwnerName,
    pub record_type: CloudflareRecordType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareRecordPageRequest {
    pub limit: u16,
    pub cursor: Option<DnsPageToken>,
}

/// Explicit Cloudflare RRset projection. Provider transport objects and account-native IDs are
/// intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareRecordSetDto {
    pub provider_account_id: CloudResourceId,
    pub zone_id: DnsZoneId,
    pub zone_apex: AbsoluteDnsName,
    pub zone_visibility: ZoneVisibility,
    pub owner: DnsOwnerName,
    pub record_type: CloudflareRecordType,
    pub ttl: CloudflareRecordTtlDto,
    pub values: Vec<CloudflareRecordValueDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<CloudflareProxyOptions>,
    pub cname_flattening: CloudflareCnameFlattening,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub tags: Vec<String>,
    pub provider_object_ids: Vec<DnsRecordObjectId>,
    pub revision: DnsRecordRevision,
}

impl CloudflareRecordSetDto {
    fn validate(&self) -> CoreResult<()> {
        self.provider_account_id.validate()?;
        validate_cloudflare_zone_id(&self.zone_id)?;
        self.revision.validate()?;
        let values = self
            .values
            .iter()
            .map(CloudflareRecordValueDto::core)
            .collect::<CoreResult<BTreeSet<_>>>()?;
        if values.len() != self.values.len() {
            return Err(CoreError::Conflict(
                "Cloudflare RRset contains duplicate values".to_string(),
            ));
        }
        if let CloudflareRecordTtlDto::Seconds(seconds) = self.ttl {
            if !(30..=86_400).contains(&seconds) {
                return Err(CoreError::Conflict(
                    "Cloudflare RRset TTL is outside the provider range".to_string(),
                ));
            }
        }
        let tags = self.tags.iter().cloned().collect::<BTreeSet<_>>();
        if tags.len() != self.tags.len() {
            return Err(CoreError::Conflict(
                "Cloudflare RRset contains duplicate tags".to_string(),
            ));
        }
        let object_ids = self
            .provider_object_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if object_ids.len() != self.provider_object_ids.len() || object_ids.is_empty() {
            return Err(CoreError::Conflict(
                "Cloudflare RRset object IDs are invalid".to_string(),
            ));
        }
        for object_id in &object_ids {
            object_id.validate()?;
            let value = object_id.as_str();
            if value.len() != CLOUDFLARE_RECORD_ID_LEN
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(CoreError::Conflict(
                    "Cloudflare RRset contains an invalid provider object ID".to_string(),
                ));
            }
        }
        let zone = DnsZoneRef {
            provider_account_id: self.provider_account_id.clone(),
            provider: CloudProvider::Cloudflare,
            zone_id: self.zone_id.clone(),
            apex: self.zone_apex.clone(),
            visibility: self.zone_visibility,
        };
        let proxy_capable = matches!(
            self.record_type,
            CloudflareRecordType::A | CloudflareRecordType::Aaaa | CloudflareRecordType::Cname
        );
        let has_metadata = self.comment.is_some() || !tags.is_empty();
        let extension = if proxy_capable
            || has_metadata
            || self.cname_flattening != CloudflareCnameFlattening::ProviderDefault
        {
            Some(DnsRecordExtension::Cloudflare {
                proxy: self.proxy,
                cname_flattening: self.cname_flattening,
                comment: self.comment.clone(),
                tags,
            })
        } else {
            if self.proxy.is_some() {
                return Err(CoreError::Conflict(
                    "Cloudflare proxy state is invalid for this RRset".to_string(),
                ));
            }
            None
        };
        ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: self.owner.clone(),
                record_type: self.record_type.core(),
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: self.ttl.core(),
            values,
            extension,
        }
        .validate(&zone)
    }

    fn matches_scope(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: Option<&CloudflareRecordSetKey>,
    ) -> bool {
        &self.provider_account_id == account_id
            && &self.zone_id == zone_id
            && key.is_none_or(|key| self.owner == key.owner && self.record_type == key.record_type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareRecordPageDto {
    pub items: Vec<CloudflareRecordSetDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<DnsPageToken>,
}

impl CloudflareRecordPageDto {
    fn validate(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        zone: &CloudflareZoneDto,
        requested_limit: u16,
    ) -> CoreResult<()> {
        if !(1..=MAX_PAGE_LIMIT).contains(&requested_limit)
            || self.items.len() > requested_limit as usize
        {
            return Err(CoreError::Conflict(
                "Cloudflare RRset page exceeds the requested limit".to_string(),
            ));
        }
        if let Some(cursor) = &self.next_cursor {
            cursor.validate()?;
        }
        let mut keys = BTreeSet::new();
        for record in &self.items {
            record.validate()?;
            if !record.matches_scope(account_id, zone_id, None)
                || record.zone_apex != zone.name
                || record.zone_visibility != zone.visibility
                || !keys.insert((record.owner.as_str(), record.record_type))
            {
                return Err(CoreError::Conflict(
                    "Cloudflare RRset page scope is inconsistent".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[async_trait]
pub trait CloudflareDnsAdminService: Send + Sync {
    async fn list_zones(
        &self,
        account_id: &CloudResourceId,
        page: &CloudflareZonePageRequest,
    ) -> Result<CloudflareZonePageDto, CloudflareDnsAdminError>;

    async fn get_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
    ) -> Result<CloudflareZoneDto, CloudflareDnsAdminError>;

    async fn list_record_sets(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        page: &CloudflareRecordPageRequest,
    ) -> Result<CloudflareRecordPageDto, CloudflareDnsAdminError>;

    async fn get_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
    ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError>;
}

/// Sanitized service failures. Provider payloads and diagnostic text are deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudflareDnsAdminError {
    InvalidRequest,
    NotFound,
    Unavailable,
    InvalidProviderObservation,
}

pub type SharedCloudflareDnsAdminService = Arc<dyn CloudflareDnsAdminService>;

#[derive(Debug, Default, Deserialize)]
pub struct CloudflareZoneListQuery {
    limit: Option<String>,
    cursor: Option<String>,
}

fn parse_page(query: CloudflareZoneListQuery) -> Result<CloudflareZonePageRequest, &'static str> {
    let limit = match query.limit {
        Some(value) => value
            .parse::<u16>()
            .ok()
            .filter(|limit| (1..=MAX_PAGE_LIMIT).contains(limit))
            .ok_or("invalid_page")?,
        None => DEFAULT_PAGE_LIMIT,
    };
    let cursor = query
        .cursor
        .map(DnsPageToken::new)
        .transpose()
        .map_err(|_| "invalid_page")?;
    Ok(CloudflareZonePageRequest { limit, cursor })
}

fn parse_record_page(
    query: CloudflareZoneListQuery,
) -> Result<CloudflareRecordPageRequest, &'static str> {
    let page = parse_page(query)?;
    Ok(CloudflareRecordPageRequest {
        limit: page.limit,
        cursor: page.cursor,
    })
}

#[derive(Debug, Deserialize)]
pub struct CloudflareRecordDetailQuery {
    owner: Option<String>,
}

fn parse_zone_id(value: String) -> Result<DnsZoneId, &'static str> {
    match DnsZoneId::new(value) {
        Ok(value) if validate_cloudflare_zone_id(&value).is_ok() => Ok(value),
        Err(_) | Ok(_) => Err("invalid_zone_id"),
    }
}

fn error_response(status: StatusCode, code: &'static str) -> Response {
    (status, Json(ApiResponse::<()>::err_body(code.to_string()))).into_response()
}

fn map_service_error(error: CloudflareDnsAdminError) -> Response {
    let (status, code) = match error {
        CloudflareDnsAdminError::InvalidRequest => (StatusCode::BAD_REQUEST, "invalid_request"),
        CloudflareDnsAdminError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        CloudflareDnsAdminError::Unavailable => {
            (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
        }
        CloudflareDnsAdminError::InvalidProviderObservation => (
            StatusCode::SERVICE_UNAVAILABLE,
            "invalid_provider_observation",
        ),
    };
    error_response(status, code)
}

enum AuthoritativeZoneError {
    Service(CloudflareDnsAdminError),
    InvalidResponse,
}

async fn load_authoritative_zone(
    service: &dyn CloudflareDnsAdminService,
    account_id: &CloudResourceId,
    zone_id: &DnsZoneId,
) -> Result<CloudflareZoneDto, AuthoritativeZoneError> {
    let zone = service
        .get_zone(account_id, zone_id)
        .await
        .map_err(AuthoritativeZoneError::Service)?;
    if zone.validate().is_err()
        || &zone.provider_account_id != account_id
        || &zone.zone_id != zone_id
    {
        return Err(AuthoritativeZoneError::InvalidResponse);
    }
    Ok(zone)
}

fn map_authoritative_zone_error(error: AuthoritativeZoneError) -> Response {
    match error {
        AuthoritativeZoneError::Service(error) => map_service_error(error),
        AuthoritativeZoneError::InvalidResponse => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response")
        }
    }
}

pub async fn list_zones(
    State(state): State<ApiState>,
    Path(account_id): Path<String>,
    Query(query): Query<CloudflareZoneListQuery>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let page = match parse_page(query) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let service = match state.cloudflare_dns_admin.as_deref() {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    match service.list_zones(&account_id, &page).await {
        Ok(result) => {
            if result.validate(&account_id, page.limit).is_err() {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response");
            }
            Json(ApiResponse::ok_body(result)).into_response()
        }
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
    let service = match state.cloudflare_dns_admin.as_deref() {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    match service.get_zone(&account_id, &zone_id).await {
        Ok(result) => {
            if result.validate().is_err()
                || result.provider_account_id != account_id
                || result.zone_id != zone_id
            {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response");
            }
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Err(error) => map_service_error(error),
    }
}

pub async fn list_record_sets(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    Query(query): Query<CloudflareZoneListQuery>,
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
    let service = match state.cloudflare_dns_admin.as_deref() {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    let zone = match load_authoritative_zone(service, &account_id, &zone_id).await {
        Ok(value) => value,
        Err(error) => return map_authoritative_zone_error(error),
    };
    match service.list_record_sets(&account_id, &zone_id, &page).await {
        Ok(result) => {
            if result
                .validate(&account_id, &zone_id, &zone, page.limit)
                .is_err()
            {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response");
            }
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Err(error) => map_service_error(error),
    }
}

pub async fn get_record_set(
    State(state): State<ApiState>,
    Path((account_id, zone_id, record_type)): Path<(String, String, String)>,
    Query(query): Query<CloudflareRecordDetailQuery>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let zone_id = match parse_zone_id(zone_id) {
        Ok(value) => value,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let record_type = match CloudflareRecordType::parse(&record_type) {
        Some(value) => value,
        None => return error_response(StatusCode::BAD_REQUEST, "invalid_record_type"),
    };
    let owner = match query.owner.and_then(|owner| DnsOwnerName::new(owner).ok()) {
        Some(value) => value,
        None => return error_response(StatusCode::BAD_REQUEST, "invalid_record_owner"),
    };
    let key = CloudflareRecordSetKey { owner, record_type };
    let service = match state.cloudflare_dns_admin.as_deref() {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    let zone = match load_authoritative_zone(service, &account_id, &zone_id).await {
        Ok(value) => value,
        Err(error) => return map_authoritative_zone_error(error),
    };
    match service.get_record_set(&account_id, &zone_id, &key).await {
        Ok(result) => {
            if result.validate().is_err()
                || !result.matches_scope(&account_id, &zone_id, Some(&key))
                || result.zone_apex != zone.name
                || result.zone_visibility != zone.visibility
            {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response");
            }
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Err(error) => map_service_error(error),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use axum::{body::Body, http::Request};
    use edgion_center_core::{AuthzMode, CenterCapabilities, CenterMode};
    use tower::ServiceExt;

    use super::*;
    use crate::{
        aggregator::ResourceAggregator,
        commander::Commander,
        fed_sync::registry::ControllerRegistry,
        metadata_store::CenterMetaDataStore,
        proxy::ProxyForwarder,
        watch_cache::{CenterSyncClient, CenterWatchCacheRegistry},
    };

    const ZONE_ID: &str = "0123456789abcdef0123456789abcdef";

    struct FakeService {
        list_error: Mutex<Option<CloudflareDnsAdminError>>,
        get_error: Mutex<Option<CloudflareDnsAdminError>>,
        last_page: Mutex<Option<CloudflareZonePageRequest>>,
        record_error: Mutex<Option<CloudflareDnsAdminError>>,
        record_response: Mutex<Option<CloudflareRecordSetDto>>,
        last_record_page: Mutex<Option<CloudflareRecordPageRequest>>,
        last_record_key: Mutex<Option<CloudflareRecordSetKey>>,
    }

    impl Default for FakeService {
        fn default() -> Self {
            Self {
                list_error: Mutex::new(None),
                get_error: Mutex::new(None),
                last_page: Mutex::new(None),
                record_error: Mutex::new(None),
                record_response: Mutex::new(None),
                last_record_page: Mutex::new(None),
                last_record_key: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl CloudflareDnsAdminService for FakeService {
        async fn list_zones(
            &self,
            account_id: &CloudResourceId,
            page: &CloudflareZonePageRequest,
        ) -> Result<CloudflareZonePageDto, CloudflareDnsAdminError> {
            *self.last_page.lock().unwrap() = Some(page.clone());
            if let Some(error) = self.list_error.lock().unwrap().take() {
                return Err(error);
            }
            Ok(CloudflareZonePageDto {
                items: vec![zone(account_id.as_str(), ZONE_ID)],
                next_cursor: Some(DnsPageToken::new("cursor-2").unwrap()),
            })
        }

        async fn get_zone(
            &self,
            account_id: &CloudResourceId,
            zone_id: &DnsZoneId,
        ) -> Result<CloudflareZoneDto, CloudflareDnsAdminError> {
            if let Some(error) = self.get_error.lock().unwrap().take() {
                return Err(error);
            }
            Ok(zone(account_id.as_str(), zone_id.as_str()))
        }

        async fn list_record_sets(
            &self,
            account_id: &CloudResourceId,
            zone_id: &DnsZoneId,
            page: &CloudflareRecordPageRequest,
        ) -> Result<CloudflareRecordPageDto, CloudflareDnsAdminError> {
            *self.last_record_page.lock().unwrap() = Some(page.clone());
            if let Some(error) = self.record_error.lock().unwrap().take() {
                return Err(error);
            }
            let item = self
                .record_response
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| record(account_id.as_str(), zone_id.as_str()));
            Ok(CloudflareRecordPageDto {
                items: vec![item],
                next_cursor: Some(DnsPageToken::new("record-cursor-2").unwrap()),
            })
        }

        async fn get_record_set(
            &self,
            account_id: &CloudResourceId,
            zone_id: &DnsZoneId,
            key: &CloudflareRecordSetKey,
        ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError> {
            *self.last_record_key.lock().unwrap() = Some(key.clone());
            if let Some(error) = self.record_error.lock().unwrap().take() {
                return Err(error);
            }
            Ok(self
                .record_response
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| record(account_id.as_str(), zone_id.as_str())))
        }
    }

    fn zone(account_id: &str, zone_id: &str) -> CloudflareZoneDto {
        CloudflareZoneDto {
            provider_account_id: CloudResourceId::new(account_id).unwrap(),
            zone_id: DnsZoneId::new(zone_id).unwrap(),
            name: AbsoluteDnsName::new("example.com").unwrap(),
            kind: CloudflareZoneKind::Full,
            status: CloudflareZoneStatus::Active,
            visibility: ZoneVisibility::Public,
            nameservers: vec![AbsoluteDnsName::new("ns1.example.net").unwrap()],
            revision: Some(DnsRecordRevision::new("revision-1").unwrap()),
        }
    }

    fn record(account_id: &str, zone_id: &str) -> CloudflareRecordSetDto {
        CloudflareRecordSetDto {
            provider_account_id: CloudResourceId::new(account_id).unwrap(),
            zone_id: DnsZoneId::new(zone_id).unwrap(),
            zone_apex: AbsoluteDnsName::new("example.com").unwrap(),
            zone_visibility: ZoneVisibility::Public,
            owner: DnsOwnerName::new("www.example.com").unwrap(),
            record_type: CloudflareRecordType::A,
            ttl: CloudflareRecordTtlDto::Automatic,
            values: vec![CloudflareRecordValueDto::A {
                address: "192.0.2.1".parse().unwrap(),
            }],
            proxy: Some(CloudflareProxyOptions::Proxied),
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: Some("managed by Center".to_string()),
            tags: vec!["owner:edge".to_string()],
            provider_object_ids: vec![
                DnsRecordObjectId::new("abcdef0123456789abcdef0123456789").unwrap()
            ],
            revision: DnsRecordRevision::new("sha256:record-revision").unwrap(),
        }
    }

    fn state(service: Option<SharedCloudflareDnsAdminService>, capability: bool) -> ApiState {
        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let commander = Arc::new(Commander::new(
            registry.clone(),
            Arc::new(parking_lot::Mutex::new(HashMap::new())),
            5,
        ));
        let proxy = Arc::new(ProxyForwarder::new(
            registry.clone(),
            Arc::new(parking_lot::Mutex::new(HashMap::new())),
            5,
        ));
        let mut capabilities = CenterCapabilities::for_mode(CenterMode::Standalone);
        capabilities.cloudflare_dns_read = capability;
        ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            commander,
            proxy,
            controller_directory: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: None,
            role_admin: None,
            audit_reader: None,
            cloudflare_dns_admin: service,
            provider_account_store: None,
            capability_snapshot_store: None,
            credential_inspection_service: None,
            metadata_store,
            sync_client,
            registry,
            platform_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            authz_mode: AuthzMode::AllowAll,
            platform_mode: CenterMode::Standalone,
            capabilities,
        }
    }

    async fn request(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, body)
    }

    #[tokio::test]
    async fn list_and_get_return_sanitized_zone_views() {
        let service = Arc::new(FakeService::default());
        let app = super::super::router(state(Some(service), true));
        let (status, body) = request(
            app.clone(),
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["items"][0]["providerAccountId"], "account-1");
        assert_eq!(body["data"]["items"][0]["kind"], "full");
        assert_eq!(body["data"]["items"][0]["status"], "active");
        assert!(body.to_string().find("token").is_none());

        let (status, body) = request(
            app,
            &format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["zoneId"], ZONE_ID);
        assert_eq!(body["data"]["visibility"], "public");
    }

    #[tokio::test]
    async fn pagination_is_bounded_and_forwarded() {
        let service = Arc::new(FakeService::default());
        let app = super::super::router(state(Some(service.clone()), true));
        let (status, body) = request(
            app.clone(),
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones?limit=10&cursor=cursor-1",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["nextCursor"], "cursor-2");
        let page = service.last_page.lock().unwrap().clone().unwrap();
        assert_eq!(page.limit, 10);
        assert_eq!(page.cursor.unwrap().as_str(), "cursor-1");

        let (status, body) = request(
            app,
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones?limit=0",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_page");
    }

    #[tokio::test]
    async fn invalid_identifiers_fail_with_stable_codes() {
        let app = super::super::router(state(Some(Arc::new(FakeService::default())), true));
        let (status, body) = request(
            app.clone(),
            "/api/v1/center/cloudflare/dns/accounts/%20/zones",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_account_id");

        let (status, body) = request(
            app,
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_zone_id");
    }

    #[tokio::test]
    async fn service_errors_are_classified_without_leaking_messages() {
        for (error, expected_status, expected_code) in [
            (
                CloudflareDnsAdminError::NotFound,
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                CloudflareDnsAdminError::InvalidRequest,
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (
                CloudflareDnsAdminError::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
            ),
            (
                CloudflareDnsAdminError::InvalidProviderObservation,
                StatusCode::SERVICE_UNAVAILABLE,
                "invalid_provider_observation",
            ),
        ] {
            let service = Arc::new(FakeService::default());
            *service.get_error.lock().unwrap() = Some(error);
            let app = super::super::router(state(Some(service), true));
            let (status, body) = request(
                app,
                &format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}"),
            )
            .await;
            assert_eq!(status, expected_status);
            assert_eq!(body["error"], expected_code);
        }
    }

    #[test]
    fn zone_projection_validates_cloudflare_identity_and_optional_revision() {
        let mut value = zone("account-1", ZONE_ID);
        value.revision = None;
        assert!(value.validate().is_ok());
        assert!(serde_json::to_value(&value)
            .unwrap()
            .get("revision")
            .is_none());

        value.zone_id = DnsZoneId::new("zone-1").unwrap();
        assert!(value.validate().is_err());

        value.zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        value.kind = CloudflareZoneKind::Internal;
        assert!(value.validate().is_err());
        value.visibility = ZoneVisibility::Private;
        assert!(value.validate().is_ok());
    }

    #[tokio::test]
    async fn routes_require_both_capability_and_service() {
        let path = "/api/v1/center/cloudflare/dns/accounts/account-1/zones";
        for state in [
            state(None, false),
            state(None, true),
            state(Some(Arc::new(FakeService::default())), false),
        ] {
            let (status, _) = request(super::super::router(state), path).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
        let (status, _) = request(
            super::super::router(state(Some(Arc::new(FakeService::default())), true)),
            path,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn server_info_reports_cloudflare_dns_read_capability() {
        let app = super::super::router(state(Some(Arc::new(FakeService::default())), true));
        let (status, body) = request(app, "/api/v1/server-info").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["capabilities"]["cloudflareDnsRead"], true);

        let app = super::super::router(state(None, true));
        let (status, body) = request(app, "/api/v1/server-info").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["capabilities"]["cloudflareDnsRead"], false);
    }

    #[tokio::test]
    async fn record_list_and_detail_return_explicit_cloudflare_views() {
        let service = Arc::new(FakeService::default());
        let app = super::super::router(state(Some(service.clone()), true));
        let base =
            format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets");
        let (status, body) = request(
            app.clone(),
            &format!("{base}?limit=10&cursor=record-cursor-1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["items"][0]["recordType"], "A");
        assert_eq!(body["data"]["items"][0]["ttl"]["type"], "automatic");
        assert_eq!(body["data"]["items"][0]["proxy"], "proxied");
        assert_eq!(
            body["data"]["items"][0]["providerObjectIds"][0],
            "abcdef0123456789abcdef0123456789"
        );
        assert_eq!(body["data"]["nextCursor"], "record-cursor-2");
        let page = service.last_record_page.lock().unwrap().clone().unwrap();
        assert_eq!(page.limit, 10);
        assert_eq!(page.cursor.unwrap().as_str(), "record-cursor-1");

        let (status, body) = request(app, &format!("{base}/a?owner=www.example.com")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["owner"], "www.example.com");
        let key = service.last_record_key.lock().unwrap().clone().unwrap();
        assert_eq!(key.record_type, CloudflareRecordType::A);
        assert_eq!(key.owner.as_str(), "www.example.com");
        let serialized = body.to_string();
        for forbidden in ["apiToken", "externalAccountId", "modifiedOn", "rawError"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn record_projection_preserves_binary_txt_and_caa_octets() {
        let binary = CloudflareOctetsDto {
            base64: URL_SAFE_NO_PAD.encode([0, 0xff, b'\n']),
        };
        let txt = CloudflareRecordValueDto::Txt {
            segments: vec![binary.clone()],
        };
        assert!(matches!(txt.core().unwrap(), DnsRecordSetValue::Txt { .. }));
        let caa = CloudflareRecordValueDto::Caa {
            flags: 0,
            tag: CaaTag::new("issue").unwrap(),
            value: binary,
        };
        assert!(matches!(caa.core().unwrap(), DnsRecordSetValue::Caa { .. }));
        let non_canonical = CloudflareOctetsDto {
            base64: "AA==".to_string(),
        };
        assert!(non_canonical.decode().is_err());
    }

    #[test]
    fn record_projection_rejects_invalid_provider_semantics() {
        let mut value = record("account-1", ZONE_ID);
        value.ttl = CloudflareRecordTtlDto::Seconds(1);
        value.proxy = Some(CloudflareProxyOptions::DnsOnly);
        assert!(value.validate().is_err());

        value.ttl = CloudflareRecordTtlDto::Seconds(300);
        value.provider_object_ids = vec![DnsRecordObjectId::new("record-1").unwrap()];
        assert!(value.validate().is_err());

        value.provider_object_ids =
            vec![DnsRecordObjectId::new("abcdef0123456789abcdef0123456789").unwrap()];
        value.owner = DnsOwnerName::new("outside.example.net").unwrap();
        assert!(value.validate().is_err());
    }

    #[tokio::test]
    async fn record_requests_reject_invalid_identity_and_page() {
        let app = super::super::router(state(Some(Arc::new(FakeService::default())), true));
        let base =
            format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets");
        for (path, code) in [
            (format!("{base}?limit=101"), "invalid_page"),
            (
                format!("{base}/HTTPS?owner=www.example.com"),
                "invalid_record_type",
            ),
            (format!("{base}/A"), "invalid_record_owner"),
            (
                format!("{base}/A?owner=bad..example.com"),
                "invalid_record_owner",
            ),
        ] {
            let (status, body) = request(app.clone(), &path).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"], code);
        }
    }

    #[tokio::test]
    async fn invalid_record_service_observations_fail_closed() {
        let service = Arc::new(FakeService::default());
        let mut invalid = record("another-account", ZONE_ID);
        invalid.proxy = None;
        *service.record_response.lock().unwrap() = Some(invalid);
        let app = super::super::router(state(Some(service), true));
        let path =
            format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets");
        let (status, body) = request(app, &path).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "invalid_service_response");
    }

    #[tokio::test]
    async fn record_list_requires_the_authoritative_zone_apex() {
        let service = Arc::new(FakeService::default());
        let mut wrong_zone = record("account-1", ZONE_ID);
        wrong_zone.zone_apex = AbsoluteDnsName::new("sub.example.com").unwrap();
        wrong_zone.owner = DnsOwnerName::new("www.sub.example.com").unwrap();
        assert!(wrong_zone.validate().is_ok());
        *service.record_response.lock().unwrap() = Some(wrong_zone);
        let path =
            format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets");
        let (status, body) = request(super::super::router(state(Some(service), true)), &path).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "invalid_service_response");
    }

    #[tokio::test]
    async fn record_detail_requires_the_authoritative_zone_visibility() {
        let service = Arc::new(FakeService::default());
        let mut wrong_zone = record("account-1", ZONE_ID);
        wrong_zone.zone_visibility = ZoneVisibility::Private;
        assert!(wrong_zone.validate().is_ok());
        *service.record_response.lock().unwrap() = Some(wrong_zone);
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A?owner=www.example.com"
        );
        let (status, body) = request(super::super::router(state(Some(service), true)), &path).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "invalid_service_response");
    }

    #[tokio::test]
    async fn record_service_errors_are_redacted_and_routes_are_gated() {
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A?owner=www.example.com"
        );
        let service = Arc::new(FakeService::default());
        *service.record_error.lock().unwrap() = Some(CloudflareDnsAdminError::Unavailable);
        let (status, body) = request(super::super::router(state(Some(service), true)), &path).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "service_unavailable");
        assert!(!body.to_string().contains("provider"));

        for disabled in [
            state(None, true),
            state(Some(Arc::new(FakeService::default())), false),
        ] {
            let (status, _) = request(super::super::router(disabled), &path).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
    }
}
