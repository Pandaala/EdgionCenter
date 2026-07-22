//! AWS Route 53-specific DNS Admin API contract.
//!
//! AWS SDK clients, credentials, provider comments, and raw provider responses stay behind the
//! injected service. The public DTO preserves the complete validated Route 53 record model while
//! keeping this module independent from the provider SDK.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use edgion_center_core::{
    AbsoluteDnsName, CloudProvider, CloudResourceId, CoreError, CoreResult, DnsOwnerName,
    DnsPageToken, DnsRecordRevision, DnsRecordSetKey, DnsRoutingIdentity, DnsZoneId, DnsZoneRef,
    ProviderDnsRecordSet, ProviderDnsRecordType, ZoneVisibility,
};
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::ApiResponse;

const DEFAULT_ROUTE53_PAGE_LIMIT: u16 = 50;
pub const MAX_ROUTE53_PAGE_LIMIT: u16 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route53DnsAdminError {
    InvalidRequest,
    NotFound,
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
            ProviderDnsRecordType::GoogleAlias => None,
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

fn map_service_error(error: Route53DnsAdminError) -> Response {
    let (status, code) = match error {
        Route53DnsAdminError::InvalidRequest => (StatusCode::BAD_REQUEST, "invalid_request"),
        Route53DnsAdminError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
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
        assert!(Route53RecordType::from_core(ProviderDnsRecordType::GoogleAlias).is_none());
    }
}
