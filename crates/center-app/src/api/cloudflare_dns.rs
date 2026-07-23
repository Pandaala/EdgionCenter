//! Cloudflare-specific DNS zone inventory and bounded synchronous write Admin API.
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
    extract::{rejection::JsonRejection, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
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
use sha2::{Digest, Sha256};

use super::ApiState;
use crate::common::{
    api::ApiResponse,
    unified_auth::{AuthProvider, UnifiedAuthClaims},
};

const DEFAULT_PAGE_LIMIT: u16 = 50;
const MAX_PAGE_LIMIT: u16 = 100;
const MAX_NAMESERVERS: usize = 20;
const CLOUDFLARE_ZONE_ID_LEN: usize = 32;
const CLOUDFLARE_RECORD_ID_LEN: usize = 32;
const RESERVED_TAG_PREFIX: &str = "edgion-center-";
const REMOTE_CONTROL_TAG_NAME: &str = "edgion-center-remote";
const REMOTE_CALLER_ALIAS_LEN: usize = 43;
const REMOTE_CALLER_ALIAS_DOMAIN: &[u8] = b"edgion-center/cloudflare-dns/remote-caller-alias/v1";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "seconds",
    rename_all = "snake_case",
    deny_unknown_fields
)]
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "UPPERCASE",
    rename_all_fields = "camelCase",
    deny_unknown_fields
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

impl CloudflareRecordSetKey {
    pub fn core(&self) -> DnsRecordSetKey {
        DnsRecordSetKey {
            owner: self.owner.clone(),
            record_type: self.record_type.core(),
            routing: DnsRoutingIdentity::Simple,
        }
    }
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
    pub control: CloudflareRecordControlDto,
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
        if tags
            .iter()
            .any(|tag| is_reserved_cloudflare_record_tag(tag))
            || matches!(
                &self.control,
                CloudflareRecordControlDto::Remote { caller_alias }
                    if !valid_remote_caller_alias(caller_alias)
            )
        {
            return Err(CoreError::Conflict(
                "Cloudflare RRset control projection is invalid".to_string(),
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

/// Sanitized provenance projected from Cloudflare record tags. It is informational only and is
/// never authorization or ownership evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CloudflareRecordControlDto {
    Manual,
    Remote { caller_alias: String },
    InvalidRemoteMarker,
}

/// Remove Center-reserved tags from a provider observation and project the bounded remote-control
/// marker. Malformed or conflicting reserved tags are never echoed to the caller.
pub fn split_cloudflare_record_tags(
    raw: BTreeSet<String>,
) -> CoreResult<(Vec<String>, CloudflareRecordControlDto)> {
    let mut tags = Vec::with_capacity(raw.len());
    let mut remote_alias = None;
    let mut invalid_marker = false;
    for tag in raw {
        let Some((name, value)) = tag.split_once(':') else {
            if is_reserved_cloudflare_record_tag(&tag) {
                invalid_marker = true;
            } else {
                tags.push(tag);
            }
            continue;
        };
        if !name.to_ascii_lowercase().starts_with(RESERVED_TAG_PREFIX) {
            tags.push(tag);
            continue;
        }
        if name != REMOTE_CONTROL_TAG_NAME
            || !valid_remote_caller_alias(value)
            || remote_alias.replace(value.to_string()).is_some()
        {
            invalid_marker = true;
        }
    }
    let control = if invalid_marker {
        CloudflareRecordControlDto::InvalidRemoteMarker
    } else if let Some(caller_alias) = remote_alias {
        CloudflareRecordControlDto::Remote { caller_alias }
    } else {
        CloudflareRecordControlDto::Manual
    };
    Ok((tags, control))
}

fn is_reserved_cloudflare_record_tag(tag: &str) -> bool {
    tag.split_once(':')
        .map_or(tag, |(name, _)| name)
        .to_ascii_lowercase()
        .starts_with(RESERVED_TAG_PREFIX)
}

fn valid_remote_caller_alias(value: &str) -> bool {
    value.len() == REMOTE_CALLER_ALIAS_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
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

/// Internal service envelope used to validate an RRset page against the exact authoritative zone
/// observed in the same service operation. Only `page` is serialized by the HTTP handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudflareRecordPageResult {
    pub zone: CloudflareZoneDto,
    pub page: CloudflareRecordPageDto,
}

impl CloudflareRecordPageResult {
    fn validate(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        requested_limit: u16,
    ) -> CoreResult<()> {
        self.zone.validate()?;
        if &self.zone.provider_account_id != account_id || &self.zone.zone_id != zone_id {
            return Err(CoreError::Conflict(
                "Cloudflare authoritative zone scope is inconsistent".to_string(),
            ));
        }
        self.page
            .validate(account_id, zone_id, &self.zone, requested_limit)
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
    ) -> Result<CloudflareRecordPageResult, CloudflareDnsAdminError>;

    async fn get_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
    ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareZoneCreateRequest {
    pub name: AbsoluteDnsName,
}

/// Exact provider observation and human-readable apex confirmation required before deleting a
/// Cloudflare Zone. Account and Zone identities remain path-owned and are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareZoneDeleteRequest {
    pub expected_revision: DnsRecordRevision,
    pub confirm_name: AbsoluteDnsName,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CloudflareRecordMutationGuardDto {
    MustNotExist,
    MatchRevision { revision: DnsRecordRevision },
}

impl CloudflareRecordMutationGuardDto {
    pub fn expected_revision(&self) -> Option<&DnsRecordRevision> {
        match self {
            Self::MustNotExist => None,
            Self::MatchRevision { revision } => Some(revision),
        }
    }
}

/// Desired Cloudflare RRset state. Account, zone, owner, type, provider object IDs, and the
/// resulting revision are path- or provider-owned identity and are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareRecordSetPutRequest {
    pub guard: CloudflareRecordMutationGuardDto,
    pub ttl: CloudflareRecordTtlDto,
    pub values: Vec<CloudflareRecordValueDto>,
    pub proxy: Option<CloudflareProxyOptions>,
    #[serde(default)]
    pub cname_flattening: CloudflareCnameFlattening,
    pub comment: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl CloudflareRecordSetPutRequest {
    pub fn record_set(&self, key: &CloudflareRecordSetKey) -> CoreResult<ProviderDnsRecordSet> {
        self.record_set_with_remote_alias(key, None)
    }

    pub fn remote_record_set(
        &self,
        key: &CloudflareRecordSetKey,
        caller_alias: &str,
    ) -> CoreResult<ProviderDnsRecordSet> {
        if !valid_remote_caller_alias(caller_alias) {
            return Err(CoreError::Conflict(
                "Cloudflare remote caller alias is invalid".to_string(),
            ));
        }
        self.record_set_with_remote_alias(key, Some(caller_alias))
    }

    fn record_set_with_remote_alias(
        &self,
        key: &CloudflareRecordSetKey,
        remote_caller_alias: Option<&str>,
    ) -> CoreResult<ProviderDnsRecordSet> {
        if key.record_type == CloudflareRecordType::Soa {
            return Err(CoreError::Conflict(
                "SOA changes require the zone lifecycle contract".to_string(),
            ));
        }
        if self.values.is_empty() || self.values.len() > 100 {
            return Err(CoreError::Conflict(
                "Cloudflare RRset value count is invalid".to_string(),
            ));
        }
        let values = self
            .values
            .iter()
            .map(CloudflareRecordValueDto::core)
            .collect::<CoreResult<BTreeSet<_>>>()?;
        if values.len() != self.values.len()
            || values
                .iter()
                .any(|value| value.record_type() != key.record_type.core())
        {
            return Err(CoreError::Conflict(
                "Cloudflare RRset values do not match the path identity".to_string(),
            ));
        }
        if matches!(key.record_type, CloudflareRecordType::Cname) && values.len() != 1 {
            return Err(CoreError::Conflict(
                "Cloudflare CNAME RRsets require exactly one value".to_string(),
            ));
        }
        if let CloudflareRecordTtlDto::Seconds(seconds) = self.ttl {
            if !(30..=86_400).contains(&seconds) {
                return Err(CoreError::Conflict(
                    "Cloudflare RRset TTL is outside the provider range".to_string(),
                ));
            }
        }
        let proxy_capable = matches!(
            key.record_type,
            CloudflareRecordType::A | CloudflareRecordType::Aaaa | CloudflareRecordType::Cname
        );
        if proxy_capable != self.proxy.is_some() {
            return Err(CoreError::Conflict(
                "Cloudflare proxy state does not match the record type".to_string(),
            ));
        }
        if self.proxy == Some(CloudflareProxyOptions::Proxied)
            && self.ttl != CloudflareRecordTtlDto::Automatic
        {
            return Err(CoreError::Conflict(
                "proxied Cloudflare records require automatic TTL".to_string(),
            ));
        }
        if key.record_type != CloudflareRecordType::Cname
            && self.cname_flattening != CloudflareCnameFlattening::ProviderDefault
        {
            return Err(CoreError::Conflict(
                "Cloudflare CNAME flattening applies only to CNAME records".to_string(),
            ));
        }
        if self.proxy == Some(CloudflareProxyOptions::Proxied)
            && self.cname_flattening != CloudflareCnameFlattening::ProviderDefault
        {
            return Err(CoreError::Conflict(
                "proxied Cloudflare CNAME records cannot override flattening".to_string(),
            ));
        }
        if self
            .comment
            .as_ref()
            .is_some_and(|value| value.chars().count() > 500 || value.chars().any(char::is_control))
        {
            return Err(CoreError::Conflict(
                "Cloudflare DNS record comment is invalid".to_string(),
            ));
        }
        let tags = self.tags.iter().cloned().collect::<BTreeSet<_>>();
        let max_user_tags = if remote_caller_alias.is_some() {
            19
        } else {
            20
        };
        if tags.len() != self.tags.len() || tags.len() > max_user_tags {
            return Err(CoreError::Conflict(
                "Cloudflare DNS record tags are invalid".to_string(),
            ));
        }
        let mut tag_names = BTreeSet::new();
        for tag in &tags {
            if tag.is_empty() || tag.chars().count() > 133 || tag.chars().any(char::is_control) {
                return Err(CoreError::Conflict(
                    "Cloudflare DNS record tags are invalid".to_string(),
                ));
            }
            let Some((name, value)) = tag.split_once(':') else {
                return Err(CoreError::Conflict(
                    "Cloudflare DNS record tags are invalid".to_string(),
                ));
            };
            if name.is_empty()
                || name.chars().count() > 32
                || value.chars().count() > 100
                || !name.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                })
                || !tag_names.insert(name.to_ascii_lowercase())
                || is_reserved_cloudflare_record_tag(tag)
            {
                return Err(CoreError::Conflict(
                    "Cloudflare DNS record tags are invalid".to_string(),
                ));
            }
        }
        let mut tags = tags;
        if let Some(caller_alias) = remote_caller_alias {
            tags.insert(format!("{REMOTE_CONTROL_TAG_NAME}:{caller_alias}"));
        }
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

/// Derive a stable, opaque Cloudflare remote-caller alias from validated authentication identity.
/// Raw provider, issuer, and subject values never cross the Cloudflare tag boundary.
pub fn derive_cloudflare_remote_caller_alias(claims: &UnifiedAuthClaims) -> CoreResult<String> {
    let (provider, issuer) = match claims.provider {
        AuthProvider::Oidc => (
            "oidc",
            claims.iss.as_deref().ok_or_else(|| {
                CoreError::Conflict("validated authentication issuer is unavailable".to_string())
            })?,
        ),
        AuthProvider::Local => ("local", "edgion-center-local"),
    };
    let subject = claims.sub.as_deref().ok_or_else(|| {
        CoreError::Conflict("validated authentication subject is unavailable".to_string())
    })?;
    if issuer.is_empty()
        || issuer.len() > 2_048
        || issuer.chars().any(char::is_control)
        || subject.is_empty()
        || subject.len() > 255
        || subject.chars().any(char::is_control)
    {
        return Err(CoreError::Conflict(
            "validated authentication identity is invalid".to_string(),
        ));
    }
    let mut digest = Sha256::new();
    digest.update(REMOTE_CALLER_ALIAS_DOMAIN);
    for value in [provider.as_bytes(), issuer.as_bytes(), subject.as_bytes()] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value);
    }
    Ok(URL_SAFE_NO_PAD.encode(digest.finalize()))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareRecordSetDeleteRequest {
    pub expected_revision: DnsRecordRevision,
}

#[async_trait]
pub trait CloudflareDnsWriteAdminService: Send + Sync {
    async fn create_zone(
        &self,
        account_id: &CloudResourceId,
        request: &CloudflareZoneCreateRequest,
    ) -> Result<CloudflareZoneDto, CloudflareDnsAdminError>;

    async fn delete_zone(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        request: &CloudflareZoneDeleteRequest,
    ) -> Result<(), CloudflareDnsAdminError>;

    async fn put_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
        request: &CloudflareRecordSetPutRequest,
    ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError>;

    async fn put_remote_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
        request: &CloudflareRecordSetPutRequest,
        caller_alias: &str,
    ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError>;

    async fn delete_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &CloudflareRecordSetKey,
        request: &CloudflareRecordSetDeleteRequest,
    ) -> Result<(), CloudflareDnsAdminError>;
}

/// Sanitized service failures. Provider payloads and diagnostic text are deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudflareDnsAdminError {
    InvalidRequest,
    NotFound,
    Conflict,
    UnknownOutcome,
    Unavailable,
    RestartRequired,
    InvalidProviderObservation,
}

pub type SharedCloudflareDnsAdminService = Arc<dyn CloudflareDnsAdminService>;
pub type SharedCloudflareDnsWriteAdminService = Arc<dyn CloudflareDnsWriteAdminService>;

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
#[serde(deny_unknown_fields)]
pub struct CloudflareRecordDetailQuery {
    owner: Option<String>,
}

fn parse_zone_id(value: String) -> Result<DnsZoneId, &'static str> {
    match DnsZoneId::new(value) {
        Ok(value) if validate_cloudflare_zone_id(&value).is_ok() => Ok(value),
        Err(_) | Ok(_) => Err("invalid_zone_id"),
    }
}

fn parse_record_identity(
    account_id: String,
    zone_id: String,
    record_type: String,
    owner: Option<String>,
) -> Result<(CloudResourceId, DnsZoneId, CloudflareRecordSetKey), &'static str> {
    let account_id = CloudResourceId::new(account_id).map_err(|_| "invalid_account_id")?;
    let zone_id = parse_zone_id(zone_id)?;
    let record_type = CloudflareRecordType::parse(&record_type).ok_or("invalid_record_type")?;
    let owner = owner
        .and_then(|owner| DnsOwnerName::new(owner).ok())
        .ok_or("invalid_record_owner")?;
    Ok((
        account_id,
        zone_id,
        CloudflareRecordSetKey { owner, record_type },
    ))
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

fn map_service_error(error: CloudflareDnsAdminError) -> Response {
    let (status, code) = match error {
        CloudflareDnsAdminError::InvalidRequest => (StatusCode::BAD_REQUEST, "invalid_request"),
        CloudflareDnsAdminError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        CloudflareDnsAdminError::Conflict => (StatusCode::CONFLICT, "conflict"),
        CloudflareDnsAdminError::UnknownOutcome => {
            (StatusCode::SERVICE_UNAVAILABLE, "unknown_outcome")
        }
        CloudflareDnsAdminError::Unavailable => {
            (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
        }
        CloudflareDnsAdminError::RestartRequired => {
            (StatusCode::CONFLICT, "pagination_restart_required")
        }
        CloudflareDnsAdminError::InvalidProviderObservation => (
            StatusCode::SERVICE_UNAVAILABLE,
            "invalid_provider_observation",
        ),
    };
    error_response(status, code)
}

pub async fn create_zone(
    State(state): State<ApiState>,
    Path(account_id): Path<String>,
    body: Result<Json<CloudflareZoneCreateRequest>, JsonRejection>,
) -> Response {
    let account_id = match CloudResourceId::new(account_id) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_account_id"),
    };
    let Json(request) = match body {
        Ok(value) => value,
        Err(rejection) => return json_rejection_response(rejection),
    };
    let service = match state.cloudflare_dns_write_admin.as_deref() {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    match service.create_zone(&account_id, &request).await {
        Ok(result) => {
            if result.validate().is_err()
                || result.provider_account_id != account_id
                || result.name != request.name
                || result.kind != CloudflareZoneKind::Full
                || result.visibility != ZoneVisibility::Public
            {
                return map_service_error(CloudflareDnsAdminError::UnknownOutcome);
            }
            (StatusCode::CREATED, Json(ApiResponse::ok_body(result))).into_response()
        }
        Err(error) => map_service_error(error),
    }
}

pub async fn delete_zone(
    State(state): State<ApiState>,
    Path((account_id, zone_id)): Path<(String, String)>,
    body: Result<Json<CloudflareZoneDeleteRequest>, JsonRejection>,
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
        Ok(value) if value.expected_revision.validate().is_ok() => value,
        Ok(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_request"),
        Err(rejection) => return json_rejection_response(rejection),
    };
    let service = match state.cloudflare_dns_write_admin.as_deref() {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    match service.delete_zone(&account_id, &zone_id, &request).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_service_error(error),
    }
}

pub async fn put_record_set(
    State(state): State<ApiState>,
    Path((account_id, zone_id, record_type)): Path<(String, String, String)>,
    Query(query): Query<CloudflareRecordDetailQuery>,
    body: Result<Json<CloudflareRecordSetPutRequest>, JsonRejection>,
) -> Response {
    let (account_id, zone_id, key) =
        match parse_record_identity(account_id, zone_id, record_type, query.owner) {
            Ok(value) => value,
            Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
        };
    let Json(request) = match body {
        Ok(value) => value,
        Err(rejection) => return json_rejection_response(rejection),
    };
    let desired = match request.record_set(&key) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    if request
        .guard
        .expected_revision()
        .is_some_and(|revision| revision.validate().is_err())
    {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let service = match state.cloudflare_dns_write_admin.as_deref() {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    match service
        .put_record_set(&account_id, &zone_id, &key, &request)
        .await
    {
        Ok(result) => {
            let result_values = result
                .values
                .iter()
                .map(CloudflareRecordValueDto::core)
                .collect::<CoreResult<BTreeSet<_>>>();
            if result.validate().is_err()
                || !result.matches_scope(&account_id, &zone_id, Some(&key))
                || result.ttl.core() != desired.ttl
                || result_values.as_ref().ok() != Some(&desired.values)
                || result.proxy != request.proxy
                || result.cname_flattening != request.cname_flattening
                || result.comment != request.comment
                || result.tags.iter().cloned().collect::<BTreeSet<_>>()
                    != request.tags.iter().cloned().collect::<BTreeSet<_>>()
                || result.control != CloudflareRecordControlDto::Manual
            {
                return map_service_error(CloudflareDnsAdminError::UnknownOutcome);
            }
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Err(error) => map_service_error(error),
    }
}

pub async fn put_remote_record_set(
    State(state): State<ApiState>,
    claims: Option<Extension<UnifiedAuthClaims>>,
    Path((account_id, zone_id, record_type)): Path<(String, String, String)>,
    Query(query): Query<CloudflareRecordDetailQuery>,
    body: Result<Json<CloudflareRecordSetPutRequest>, JsonRejection>,
) -> Response {
    let Extension(claims) = match claims {
        Some(value) => value,
        None => return error_response(StatusCode::UNAUTHORIZED, "authentication_required"),
    };
    let caller_alias = match derive_cloudflare_remote_caller_alias(&claims) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::UNAUTHORIZED, "invalid_identity"),
    };
    let (account_id, zone_id, key) =
        match parse_record_identity(account_id, zone_id, record_type, query.owner) {
            Ok(value) => value,
            Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
        };
    let Json(request) = match body {
        Ok(value) => value,
        Err(rejection) => return json_rejection_response(rejection),
    };
    let desired = match request.remote_record_set(&key, &caller_alias) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    if request
        .guard
        .expected_revision()
        .is_some_and(|revision| revision.validate().is_err())
    {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let service = match state.cloudflare_dns_write_admin.as_deref() {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    match service
        .put_remote_record_set(&account_id, &zone_id, &key, &request, &caller_alias)
        .await
    {
        Ok(result) => {
            let result_values = result
                .values
                .iter()
                .map(CloudflareRecordValueDto::core)
                .collect::<CoreResult<BTreeSet<_>>>();
            let control_matches = matches!(
                &result.control,
                CloudflareRecordControlDto::Remote { caller_alias: result_alias }
                    if result_alias == &caller_alias
            );
            if result.validate().is_err()
                || !result.matches_scope(&account_id, &zone_id, Some(&key))
                || result.ttl.core() != desired.ttl
                || result_values.as_ref().ok() != Some(&desired.values)
                || result.proxy != request.proxy
                || result.cname_flattening != request.cname_flattening
                || result.comment != request.comment
                || result.tags.iter().cloned().collect::<BTreeSet<_>>()
                    != request.tags.iter().cloned().collect::<BTreeSet<_>>()
                || !control_matches
            {
                return map_service_error(CloudflareDnsAdminError::UnknownOutcome);
            }
            Json(ApiResponse::ok_body(result)).into_response()
        }
        Err(error) => map_service_error(error),
    }
}

pub async fn delete_record_set(
    State(state): State<ApiState>,
    Path((account_id, zone_id, record_type)): Path<(String, String, String)>,
    Query(query): Query<CloudflareRecordDetailQuery>,
    body: Result<Json<CloudflareRecordSetDeleteRequest>, JsonRejection>,
) -> Response {
    let (account_id, zone_id, key) =
        match parse_record_identity(account_id, zone_id, record_type, query.owner) {
            Ok(value) => value,
            Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
        };
    if key.record_type == CloudflareRecordType::Soa {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Json(request) = match body {
        Ok(value) if value.expected_revision.validate().is_ok() => value,
        Ok(_) => return error_response(StatusCode::BAD_REQUEST, "invalid_request"),
        Err(rejection) => return json_rejection_response(rejection),
    };
    let service = match state.cloudflare_dns_write_admin.as_deref() {
        Some(value) => value,
        None => return error_response(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
    };
    match service
        .delete_record_set(&account_id, &zone_id, &key, &request)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => map_service_error(error),
    }
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
    match service.list_record_sets(&account_id, &zone_id, &page).await {
        Ok(result) => {
            if result.validate(&account_id, &zone_id, page.limit).is_err() {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "invalid_service_response");
            }
            Json(ApiResponse::ok_body(result.page)).into_response()
        }
        Err(error) => map_service_error(error),
    }
}

pub async fn get_record_set(
    State(state): State<ApiState>,
    Path((account_id, zone_id, record_type)): Path<(String, String, String)>,
    Query(query): Query<CloudflareRecordDetailQuery>,
) -> Response {
    let (account_id, zone_id, key) =
        match parse_record_identity(account_id, zone_id, record_type, query.owner) {
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
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
    };

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
        get_zone_calls: AtomicUsize,
        list_record_calls: AtomicUsize,
    }

    struct FakeWriteService {
        error: Mutex<Option<CloudflareDnsAdminError>>,
        response: Mutex<Option<CloudflareZoneDto>>,
        record_response: Mutex<Option<CloudflareRecordSetDto>>,
        last_account_id: Mutex<Option<CloudResourceId>>,
        last_request: Mutex<Option<CloudflareZoneCreateRequest>>,
        last_zone_id: Mutex<Option<DnsZoneId>>,
        last_zone_delete_request: Mutex<Option<CloudflareZoneDeleteRequest>>,
        last_record_key: Mutex<Option<CloudflareRecordSetKey>>,
        last_put_request: Mutex<Option<CloudflareRecordSetPutRequest>>,
        last_remote_alias: Mutex<Option<String>>,
        last_delete_request: Mutex<Option<CloudflareRecordSetDeleteRequest>>,
        calls: AtomicUsize,
        record_calls: AtomicUsize,
    }

    impl Default for FakeWriteService {
        fn default() -> Self {
            Self {
                error: Mutex::new(None),
                response: Mutex::new(None),
                record_response: Mutex::new(None),
                last_account_id: Mutex::new(None),
                last_request: Mutex::new(None),
                last_zone_id: Mutex::new(None),
                last_zone_delete_request: Mutex::new(None),
                last_record_key: Mutex::new(None),
                last_put_request: Mutex::new(None),
                last_remote_alias: Mutex::new(None),
                last_delete_request: Mutex::new(None),
                calls: AtomicUsize::new(0),
                record_calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl CloudflareDnsWriteAdminService for FakeWriteService {
        async fn create_zone(
            &self,
            account_id: &CloudResourceId,
            request: &CloudflareZoneCreateRequest,
        ) -> Result<CloudflareZoneDto, CloudflareDnsAdminError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_account_id.lock().unwrap() = Some(account_id.clone());
            *self.last_request.lock().unwrap() = Some(request.clone());
            if let Some(error) = self.error.lock().unwrap().take() {
                return Err(error);
            }
            Ok(self
                .response
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| zone(account_id.as_str(), ZONE_ID)))
        }

        async fn put_record_set(
            &self,
            account_id: &CloudResourceId,
            zone_id: &DnsZoneId,
            key: &CloudflareRecordSetKey,
            request: &CloudflareRecordSetPutRequest,
        ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError> {
            self.record_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_record_key.lock().unwrap() = Some(key.clone());
            *self.last_put_request.lock().unwrap() = Some(request.clone());
            if let Some(error) = self.error.lock().unwrap().take() {
                return Err(error);
            }
            Ok(self
                .record_response
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| record(account_id.as_str(), zone_id.as_str())))
        }

        async fn put_remote_record_set(
            &self,
            account_id: &CloudResourceId,
            zone_id: &DnsZoneId,
            key: &CloudflareRecordSetKey,
            request: &CloudflareRecordSetPutRequest,
            caller_alias: &str,
        ) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError> {
            self.record_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_record_key.lock().unwrap() = Some(key.clone());
            *self.last_put_request.lock().unwrap() = Some(request.clone());
            *self.last_remote_alias.lock().unwrap() = Some(caller_alias.to_string());
            if let Some(error) = self.error.lock().unwrap().take() {
                return Err(error);
            }
            let response = if let Some(response) = self.record_response.lock().unwrap().clone() {
                response
            } else {
                let mut response = record(account_id.as_str(), zone_id.as_str());
                response.control = CloudflareRecordControlDto::Remote {
                    caller_alias: caller_alias.to_string(),
                };
                response
            };
            Ok(response)
        }

        async fn delete_zone(
            &self,
            account_id: &CloudResourceId,
            zone_id: &DnsZoneId,
            request: &CloudflareZoneDeleteRequest,
        ) -> Result<(), CloudflareDnsAdminError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_account_id.lock().unwrap() = Some(account_id.clone());
            *self.last_zone_id.lock().unwrap() = Some(zone_id.clone());
            *self.last_zone_delete_request.lock().unwrap() = Some(request.clone());
            if let Some(error) = self.error.lock().unwrap().take() {
                return Err(error);
            }
            Ok(())
        }

        async fn delete_record_set(
            &self,
            _account_id: &CloudResourceId,
            _zone_id: &DnsZoneId,
            key: &CloudflareRecordSetKey,
            request: &CloudflareRecordSetDeleteRequest,
        ) -> Result<(), CloudflareDnsAdminError> {
            self.record_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_record_key.lock().unwrap() = Some(key.clone());
            *self.last_delete_request.lock().unwrap() = Some(request.clone());
            if let Some(error) = self.error.lock().unwrap().take() {
                return Err(error);
            }
            Ok(())
        }
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
                get_zone_calls: AtomicUsize::new(0),
                list_record_calls: AtomicUsize::new(0),
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
            self.get_zone_calls.fetch_add(1, Ordering::SeqCst);
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
        ) -> Result<CloudflareRecordPageResult, CloudflareDnsAdminError> {
            self.list_record_calls.fetch_add(1, Ordering::SeqCst);
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
            Ok(CloudflareRecordPageResult {
                zone: zone(account_id.as_str(), zone_id.as_str()),
                page: CloudflareRecordPageDto {
                    items: vec![item],
                    next_cursor: Some(DnsPageToken::new("record-cursor-2").unwrap()),
                },
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
            control: CloudflareRecordControlDto::Manual,
            provider_object_ids: vec![
                DnsRecordObjectId::new("abcdef0123456789abcdef0123456789").unwrap()
            ],
            revision: DnsRecordRevision::new("sha256:record-revision").unwrap(),
        }
    }

    fn txt_record(account_id: &str, zone_id: &str) -> CloudflareRecordSetDto {
        CloudflareRecordSetDto {
            provider_account_id: CloudResourceId::new(account_id).unwrap(),
            zone_id: DnsZoneId::new(zone_id).unwrap(),
            zone_apex: AbsoluteDnsName::new("example.com").unwrap(),
            zone_visibility: ZoneVisibility::Public,
            owner: DnsOwnerName::new("txt.example.com").unwrap(),
            record_type: CloudflareRecordType::Txt,
            ttl: CloudflareRecordTtlDto::Seconds(300),
            values: vec![CloudflareRecordValueDto::Txt {
                segments: vec![CloudflareOctetsDto {
                    base64: "aGVsbG8".to_string(),
                }],
            }],
            proxy: None,
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: None,
            tags: Vec::new(),
            control: CloudflareRecordControlDto::Manual,
            provider_object_ids: vec![
                DnsRecordObjectId::new("abcdef0123456789abcdef0123456789").unwrap()
            ],
            revision: DnsRecordRevision::new("sha256:txt-record-revision").unwrap(),
        }
    }

    fn state(service: Option<SharedCloudflareDnsAdminService>, capability: bool) -> ApiState {
        state_with_write(service, capability, None, false)
    }

    fn state_with_write(
        service: Option<SharedCloudflareDnsAdminService>,
        capability: bool,
        write_service: Option<SharedCloudflareDnsWriteAdminService>,
        write_capability: bool,
    ) -> ApiState {
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
        capabilities.cloudflare_dns_write = write_capability;
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
            cloudflare_dns_write_admin: write_service,
            cloudflare_waf_admin: None,
            route53_dns_admin: None,
            route53_dns_write_admin: None,
            route53_zone_lifecycle_admin: None,
            cloudfront_admin: None,
            aws_waf_admin: None,
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
        json_request(app, axum::http::Method::GET, uri, Body::empty()).await
    }

    async fn json_request(
        app: axum::Router,
        method: axum::http::Method,
        uri: &str,
        body: Body,
    ) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(body)
                    .unwrap(),
            )
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

    async fn json_request_with_claims(
        app: axum::Router,
        method: axum::http::Method,
        uri: &str,
        body: Body,
        claims: UnifiedAuthClaims,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body)
            .unwrap();
        request.extensions_mut().insert(claims);
        let response = app.oneshot(request).await.unwrap();
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

    fn oidc_claims(issuer: Option<&str>, subject: Option<&str>) -> UnifiedAuthClaims {
        UnifiedAuthClaims {
            provider: AuthProvider::Oidc,
            sub: subject.map(str::to_string),
            iss: issuer.map(str::to_string),
            groups: Vec::new(),
            claims: serde_json::json!({}),
        }
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
                CloudflareDnsAdminError::RestartRequired,
                StatusCode::CONFLICT,
                "pagination_restart_required",
            ),
            (
                CloudflareDnsAdminError::InvalidProviderObservation,
                StatusCode::SERVICE_UNAVAILABLE,
                "invalid_provider_observation",
            ),
            (
                CloudflareDnsAdminError::Conflict,
                StatusCode::CONFLICT,
                "conflict",
            ),
            (
                CloudflareDnsAdminError::UnknownOutcome,
                StatusCode::SERVICE_UNAVAILABLE,
                "unknown_outcome",
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
    async fn create_zone_is_strict_synchronous_and_returns_created() {
        let service = Arc::new(FakeWriteService::default());
        let app = super::super::router(state_with_write(None, false, Some(service.clone()), true));
        let path = "/api/v1/center/cloudflare/dns/accounts/account-1/zones";
        let (status, body) = json_request(
            app,
            axum::http::Method::POST,
            path,
            Body::from(r#"{"name":"example.com"}"#),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["data"]["providerAccountId"], "account-1");
        assert_eq!(body["data"]["name"], "example.com");
        assert_eq!(service.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            service
                .last_account_id
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .as_str(),
            "account-1"
        );
        assert_eq!(
            service
                .last_request
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .name
                .as_str(),
            "example.com"
        );
    }

    #[tokio::test]
    async fn create_zone_rejects_unknown_fields_and_invalid_names_before_calling_service() {
        for body in [
            r#"{"name":"example.com","account":"secret"}"#,
            r#"{"name":"bad..example.com"}"#,
            r#"{"name":"example.com","nameServers":[]}"#,
        ] {
            let service = Arc::new(FakeWriteService::default());
            let app =
                super::super::router(state_with_write(None, false, Some(service.clone()), true));
            let (status, response) = json_request(
                app,
                axum::http::Method::POST,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones",
                Body::from(body),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(response["error"], "invalid_request");
            assert_eq!(service.calls.load(Ordering::SeqCst), 0);
            assert!(!response.to_string().contains("secret"));
        }
    }

    #[tokio::test]
    async fn create_zone_errors_are_stable_redacted_and_scope_is_validated() {
        let path = "/api/v1/center/cloudflare/dns/accounts/account-1/zones";
        for (error, expected_status, expected_code) in [
            (
                CloudflareDnsAdminError::Conflict,
                StatusCode::CONFLICT,
                "conflict",
            ),
            (
                CloudflareDnsAdminError::UnknownOutcome,
                StatusCode::SERVICE_UNAVAILABLE,
                "unknown_outcome",
            ),
        ] {
            let service = Arc::new(FakeWriteService::default());
            *service.error.lock().unwrap() = Some(error);
            let app = super::super::router(state_with_write(None, false, Some(service), true));
            let (status, body) = json_request(
                app,
                axum::http::Method::POST,
                path,
                Body::from(r#"{"name":"example.com"}"#),
            )
            .await;
            assert_eq!(status, expected_status);
            assert_eq!(body["error"], expected_code);
            assert_eq!(body.as_object().unwrap().len(), 2);
        }

        for invalid in [
            zone("another-account", ZONE_ID),
            {
                let mut value = zone("account-1", ZONE_ID);
                value.name = AbsoluteDnsName::new("another.example").unwrap();
                value
            },
            {
                let mut value = zone("account-1", ZONE_ID);
                value.kind = CloudflareZoneKind::Internal;
                value.visibility = ZoneVisibility::Private;
                value
            },
        ] {
            let service = Arc::new(FakeWriteService::default());
            *service.response.lock().unwrap() = Some(invalid);
            let app = super::super::router(state_with_write(None, false, Some(service), true));
            let (status, body) = json_request(
                app,
                axum::http::Method::POST,
                path,
                Body::from(r#"{"name":"example.com"}"#),
            )
            .await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body["error"], "unknown_outcome");
        }
    }

    #[tokio::test]
    async fn delete_zone_is_strict_synchronous_path_scoped_and_returns_no_content() {
        let service = Arc::new(FakeWriteService::default());
        let app = super::super::router(state_with_write(None, false, Some(service.clone()), true));
        let path = format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}");
        let (status, body) = json_request(
            app,
            axum::http::Method::DELETE,
            &path,
            Body::from(r#"{"expectedRevision":"revision-1","confirmName":"example.com"}"#),
        )
        .await;

        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(body, serde_json::Value::Null);
        assert_eq!(service.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            service
                .last_account_id
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .as_str(),
            "account-1"
        );
        assert_eq!(
            service
                .last_zone_id
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .as_str(),
            ZONE_ID
        );
        let request = service
            .last_zone_delete_request
            .lock()
            .unwrap()
            .clone()
            .unwrap();
        assert_eq!(request.expected_revision.as_str(), "revision-1");
        assert_eq!(request.confirm_name.as_str(), "example.com");
    }

    #[tokio::test]
    async fn delete_zone_rejects_missing_unknown_body_identity_and_invalid_path_before_service() {
        let cases = [
            (
                format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}"),
                r#"{"confirmName":"example.com"}"#,
                "invalid_request",
            ),
            (
                format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}"),
                r#"{"expectedRevision":"revision-1"}"#,
                "invalid_request",
            ),
            (
                format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}"),
                r#"{"expectedRevision":"revision-1","confirmName":"example.com","zoneId":"0123456789abcdef0123456789abcdef"}"#,
                "invalid_request",
            ),
            (
                format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}"),
                r#"{"expectedRevision":" ","confirmName":"example.com"}"#,
                "invalid_request",
            ),
            (
                format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}"),
                r#"{"expectedRevision":"revision-1","confirmName":"bad..example.com"}"#,
                "invalid_request",
            ),
            (
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/not-a-zone-id".to_string(),
                r#"{"expectedRevision":"revision-1","confirmName":"example.com"}"#,
                "invalid_zone_id",
            ),
            (
                format!("/api/v1/center/cloudflare/dns/accounts/%20/zones/{ZONE_ID}"),
                r#"{"expectedRevision":"revision-1","confirmName":"example.com"}"#,
                "invalid_account_id",
            ),
        ];
        for (path, body, expected_error) in cases {
            let service = Arc::new(FakeWriteService::default());
            let app =
                super::super::router(state_with_write(None, false, Some(service.clone()), true));
            let (status, response) =
                json_request(app, axum::http::Method::DELETE, &path, Body::from(body)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(response["error"], expected_error);
            assert_eq!(service.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn delete_zone_maps_errors_without_provider_diagnostics() {
        let path = format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}");
        for (error, expected_status, expected_code) in [
            (
                CloudflareDnsAdminError::Conflict,
                StatusCode::CONFLICT,
                "conflict",
            ),
            (
                CloudflareDnsAdminError::UnknownOutcome,
                StatusCode::SERVICE_UNAVAILABLE,
                "unknown_outcome",
            ),
        ] {
            let service = Arc::new(FakeWriteService::default());
            *service.error.lock().unwrap() = Some(error);
            let app = super::super::router(state_with_write(None, false, Some(service), true));
            let (status, body) = json_request(
                app,
                axum::http::Method::DELETE,
                &path,
                Body::from(r#"{"expectedRevision":"revision-1","confirmName":"example.com"}"#),
            )
            .await;
            assert_eq!(status, expected_status);
            assert_eq!(body["error"], expected_code);
            assert_eq!(body.as_object().unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn delete_zone_route_requires_write_service_and_capability() {
        let path = format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}");
        for state in [
            state_with_write(None, false, None, false),
            state_with_write(None, false, None, true),
            state_with_write(
                None,
                false,
                Some(Arc::new(FakeWriteService::default())),
                false,
            ),
        ] {
            let (status, _) = json_request(
                super::super::router(state),
                axum::http::Method::DELETE,
                &path,
                Body::from(r#"{"expectedRevision":"revision-1","confirmName":"example.com"}"#),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn delete_zone_request_body_is_bounded() {
        let service = Arc::new(FakeWriteService::default());
        let app = super::super::router(state_with_write(None, false, Some(service.clone()), true));
        let path = format!("/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}");
        let body = format!(
            r#"{{"expectedRevision":"revision-1","confirmName":"example.com","padding":"{}"}}"#,
            "x".repeat(70 * 1024)
        );
        let (status, response) =
            json_request(app, axum::http::Method::DELETE, &path, Body::from(body)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(response["error"], "request_too_large");
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn create_zone_route_and_advertised_capability_require_service_and_flag() {
        let path = "/api/v1/center/cloudflare/dns/accounts/account-1/zones";
        for state in [
            state_with_write(None, false, None, false),
            state_with_write(None, false, None, true),
            state_with_write(
                None,
                false,
                Some(Arc::new(FakeWriteService::default())),
                false,
            ),
        ] {
            let app = super::super::router(state);
            let (status, _) = json_request(
                app.clone(),
                axum::http::Method::POST,
                path,
                Body::from(r#"{"name":"example.com"}"#),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            let (status, info) = request(app, "/api/v1/server-info").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(info["data"]["capabilities"]["cloudflareDnsWrite"], false);
        }

        let app = super::super::router(state_with_write(
            None,
            false,
            Some(Arc::new(FakeWriteService::default())),
            true,
        ));
        let (status, info) = request(app, "/api/v1/server-info").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(info["data"]["capabilities"]["cloudflareDnsWrite"], true);
    }

    #[tokio::test]
    async fn read_and_write_services_share_the_zone_collection_path() {
        let app = super::super::router(state_with_write(
            Some(Arc::new(FakeService::default())),
            true,
            Some(Arc::new(FakeWriteService::default())),
            true,
        ));
        let path = "/api/v1/center/cloudflare/dns/accounts/account-1/zones";

        let (status, _) = request(app.clone(), path).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = json_request(
            app,
            axum::http::Method::POST,
            path,
            Body::from(r#"{"name":"example.com"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    const PUT_A_BODY: &str = r#"{
        "guard":{"type":"must_not_exist"},
        "ttl":{"type":"automatic"},
        "values":[{"type":"A","address":"192.0.2.1"}],
        "proxy":"proxied",
        "cnameFlattening":"provider_default",
        "comment":"managed by Center",
        "tags":["owner:edge"]
    }"#;

    #[test]
    fn remote_caller_alias_is_stable_and_identity_separated() {
        let oidc = oidc_claims(Some("https://issuer.example"), Some("service-a"));
        let first = derive_cloudflare_remote_caller_alias(&oidc).unwrap();
        assert_eq!(first, derive_cloudflare_remote_caller_alias(&oidc).unwrap());
        assert_eq!(first.len(), REMOTE_CALLER_ALIAS_LEN);
        assert_ne!(
            first,
            derive_cloudflare_remote_caller_alias(&oidc_claims(
                Some("https://other-issuer.example"),
                Some("service-a")
            ))
            .unwrap()
        );
        assert_ne!(
            first,
            derive_cloudflare_remote_caller_alias(&oidc_claims(
                Some("https://issuer.example"),
                Some("service-b")
            ))
            .unwrap()
        );
        let local = UnifiedAuthClaims {
            provider: AuthProvider::Local,
            sub: Some("service-a".to_string()),
            iss: None,
            groups: Vec::new(),
            claims: serde_json::json!({}),
        };
        assert_ne!(
            first,
            derive_cloudflare_remote_caller_alias(&local).unwrap()
        );
        let local_alias = derive_cloudflare_remote_caller_alias(&local).unwrap();
        assert_eq!(
            local_alias,
            derive_cloudflare_remote_caller_alias(&local).unwrap()
        );
        let mut other_local = local.clone();
        other_local.sub = Some("service-b".to_string());
        assert_ne!(
            local_alias,
            derive_cloudflare_remote_caller_alias(&other_local).unwrap()
        );
        assert!(
            derive_cloudflare_remote_caller_alias(&oidc_claims(None, Some("service-a"))).is_err()
        );
        assert!(derive_cloudflare_remote_caller_alias(&oidc_claims(
            Some("https://issuer.example"),
            None
        ))
        .is_err());
    }

    #[test]
    fn record_tags_project_remote_control_without_echoing_invalid_markers() {
        let alias = derive_cloudflare_remote_caller_alias(&oidc_claims(
            Some("https://issuer.example"),
            Some("service-a"),
        ))
        .unwrap();
        let (tags, control) = split_cloudflare_record_tags(BTreeSet::from([
            "owner:edge".to_string(),
            format!("{REMOTE_CONTROL_TAG_NAME}:{alias}"),
        ]))
        .unwrap();
        assert_eq!(tags, vec!["owner:edge"]);
        assert_eq!(
            control,
            CloudflareRecordControlDto::Remote {
                caller_alias: alias
            }
        );

        let (tags, control) = split_cloudflare_record_tags(BTreeSet::from([
            "owner:edge".to_string(),
            "EDGION-CENTER-REMOTE:not-an-alias".to_string(),
            "edgion-center-private:must-not-leak".to_string(),
        ]))
        .unwrap();
        assert_eq!(tags, vec!["owner:edge"]);
        assert_eq!(control, CloudflareRecordControlDto::InvalidRemoteMarker);
    }

    #[tokio::test]
    async fn remote_record_put_requires_claims_and_forwards_only_derived_alias() {
        let service = Arc::new(FakeWriteService::default());
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A/remote-control?owner=www.example.com"
        );
        let (status, body) = json_request(
            super::super::router(state_with_write(None, false, Some(service.clone()), true)),
            axum::http::Method::PUT,
            &path,
            Body::from(PUT_A_BODY),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "authentication_required");
        assert_eq!(service.record_calls.load(Ordering::SeqCst), 0);

        let claims = oidc_claims(Some("https://issuer.example"), Some("service-a"));
        let expected_alias = derive_cloudflare_remote_caller_alias(&claims).unwrap();
        let (status, body) = json_request_with_claims(
            super::super::router(state_with_write(None, false, Some(service.clone()), true)),
            axum::http::Method::PUT,
            &path,
            Body::from(PUT_A_BODY),
            claims,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["control"]["type"], "remote");
        assert_eq!(body["data"]["control"]["callerAlias"], expected_alias);
        assert_eq!(
            service.last_remote_alias.lock().unwrap().as_deref(),
            Some(expected_alias.as_str())
        );
        assert!(!body.to_string().contains("issuer.example"));
        assert!(!body.to_string().contains("service-a"));
    }

    #[tokio::test]
    async fn remote_record_put_rejects_invalid_identity_body_control_and_reserved_tags() {
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A/remote-control?owner=www.example.com"
        );
        let cases = [
            (oidc_claims(None, Some("service-a")), PUT_A_BODY.to_string()),
            (
                oidc_claims(Some("https://issuer.example"), None),
                PUT_A_BODY.to_string(),
            ),
            (
                UnifiedAuthClaims {
                    provider: AuthProvider::Local,
                    sub: None,
                    iss: None,
                    groups: Vec::new(),
                    claims: serde_json::json!({}),
                },
                PUT_A_BODY.to_string(),
            ),
            (
                oidc_claims(Some("https://issuer.example"), Some("service-a")),
                PUT_A_BODY.replace(
                    "\"tags\":[\"owner:edge\"]",
                    "\"tags\":[\"edgion-center-remote:fake\"]",
                ),
            ),
            (
                oidc_claims(Some("https://issuer.example"), Some("service-a")),
                PUT_A_BODY.replace(
                    "\"tags\":[\"owner:edge\"]",
                    "\"tags\":[\"owner:edge\"],\"callerAlias\":\"fake\"",
                ),
            ),
            (
                oidc_claims(Some("https://issuer.example"), Some("service-a")),
                PUT_A_BODY.replace(
                    "\"tags\":[\"owner:edge\"]",
                    "\"tags\":[\"owner:edge\"],\"control\":{\"type\":\"manual\"}",
                ),
            ),
        ];
        for (claims, body) in cases {
            let service = Arc::new(FakeWriteService::default());
            let (status, _) = json_request_with_claims(
                super::super::router(state_with_write(None, false, Some(service.clone()), true)),
                axum::http::Method::PUT,
                &path,
                Body::from(body),
                claims,
            )
            .await;
            assert!(matches!(
                status,
                StatusCode::UNAUTHORIZED | StatusCode::BAD_REQUEST
            ));
            assert_eq!(service.record_calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn remote_record_put_fails_unknown_when_control_is_not_exact() {
        let service = Arc::new(FakeWriteService::default());
        *service.record_response.lock().unwrap() = Some(record("account-1", ZONE_ID));
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A/remote-control?owner=www.example.com"
        );
        let (status, body) = json_request_with_claims(
            super::super::router(state_with_write(None, false, Some(service), true)),
            axum::http::Method::PUT,
            &path,
            Body::from(PUT_A_BODY),
            oidc_claims(Some("https://issuer.example"), Some("service-a")),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "unknown_outcome");
    }

    #[tokio::test]
    async fn remote_record_put_uses_the_bounded_write_body_limit() {
        let service = Arc::new(FakeWriteService::default());
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A/remote-control?owner=www.example.com"
        );
        let mut body = PUT_A_BODY.as_bytes().to_vec();
        body.resize(64 * 1024 + 1, b' ');
        let (status, _) = json_request_with_claims(
            super::super::router(state_with_write(None, false, Some(service.clone()), true)),
            axum::http::Method::PUT,
            &path,
            Body::from(body),
            oidc_claims(Some("https://issuer.example"), Some("service-a")),
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(service.record_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn record_put_and_delete_are_synchronous_and_path_scoped() {
        let service = Arc::new(FakeWriteService::default());
        let app = super::super::router(state_with_write(None, false, Some(service.clone()), true));
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A?owner=www.example.com"
        );

        let (status, body) = json_request(
            app.clone(),
            axum::http::Method::PUT,
            &path,
            Body::from(PUT_A_BODY),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["owner"], "www.example.com");
        assert_eq!(body["data"]["recordType"], "A");
        assert_eq!(service.record_calls.load(Ordering::SeqCst), 1);
        assert!(matches!(
            service
                .last_put_request
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .guard,
            CloudflareRecordMutationGuardDto::MustNotExist
        ));

        let (status, body) = json_request(
            app,
            axum::http::Method::DELETE,
            &path,
            Body::from(r#"{"expectedRevision":"sha256:record-revision"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(body, serde_json::Value::Null);
        assert_eq!(service.record_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            service
                .last_delete_request
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .expected_revision
                .as_str(),
            "sha256:record-revision"
        );
    }

    #[tokio::test]
    async fn record_put_normalizes_unproxied_txt_without_metadata() {
        let service = Arc::new(FakeWriteService::default());
        *service.record_response.lock().unwrap() = Some(txt_record("account-1", ZONE_ID));
        let app = super::super::router(state_with_write(None, false, Some(service.clone()), true));
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/TXT?owner=txt.example.com"
        );
        let body = r#"{
            "guard":{"type":"must_not_exist"},
            "ttl":{"type":"seconds","seconds":300},
            "values":[{"type":"TXT","segments":[{"base64":"aGVsbG8"}]}],
            "proxy":null
        }"#;

        let (status, response) =
            json_request(app, axum::http::Method::PUT, &path, Body::from(body)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["data"]["recordType"], "TXT");
        assert_eq!(response["data"]["ttl"]["seconds"], 300);

        let request = service.last_put_request.lock().unwrap().clone().unwrap();
        let desired = request
            .record_set(&CloudflareRecordSetKey {
                owner: DnsOwnerName::new("txt.example.com").unwrap(),
                record_type: CloudflareRecordType::Txt,
            })
            .unwrap();
        assert_eq!(desired.extension, None);
    }

    #[tokio::test]
    async fn record_write_rejects_body_identity_and_invalid_semantics_before_service() {
        let cases = [
            PUT_A_BODY.replace(
                "\"tags\":[\"owner:edge\"]",
                "\"tags\":[\"owner:edge\"],\"owner\":\"other.example.com\"",
            ),
            PUT_A_BODY.replace("\"type\":\"A\"", "\"type\":\"AAAA\""),
            PUT_A_BODY.replace("\"proxy\":\"proxied\",", ""),
            PUT_A_BODY.replace(
                "\"cnameFlattening\":\"provider_default\"",
                "\"cnameFlattening\":\"flatten\"",
            ),
            PUT_A_BODY.replace(
                "\"ttl\":{\"type\":\"automatic\"}",
                "\"ttl\":{\"type\":\"automatic\",\"seconds\":300}",
            ),
            PUT_A_BODY.replace("owner:edge", "EDGION-CENTER-REMOTE:not-authorized"),
        ];
        for body in cases {
            let service = Arc::new(FakeWriteService::default());
            let app =
                super::super::router(state_with_write(None, false, Some(service.clone()), true));
            let path = format!(
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A?owner=www.example.com"
            );
            let (status, response) =
                json_request(app, axum::http::Method::PUT, &path, Body::from(body)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(response["error"], "invalid_request");
            assert_eq!(service.record_calls.load(Ordering::SeqCst), 0);
        }

        let service = Arc::new(FakeWriteService::default());
        let app = super::super::router(state_with_write(None, false, Some(service.clone()), true));
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A?owner=www.example.com"
        );
        let (status, response) = json_request(
            app,
            axum::http::Method::DELETE,
            &path,
            Body::from(r#"{"expectedRevision":"sha256:record-revision","providerObjectIds":[]}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(response["error"], "invalid_request");
        assert_eq!(service.record_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn record_mutation_query_rejects_unknown_fields() {
        let service = Arc::new(FakeWriteService::default());
        let app = super::super::router(state_with_write(None, false, Some(service.clone()), true));
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A?owner=www.example.com&unexpected=value"
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::PUT)
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(PUT_A_BODY))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(service.record_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn record_write_rejects_control_plane_types_and_maps_unknown_outcome() {
        let service = Arc::new(FakeWriteService::default());
        let app = super::super::router(state_with_write(None, false, Some(service.clone()), true));
        let soa_path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/SOA?owner=example.com"
        );
        let (status, _) = json_request(
            app,
            axum::http::Method::DELETE,
            &soa_path,
            Body::from(r#"{"expectedRevision":"sha256:record-revision"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(service.record_calls.load(Ordering::SeqCst), 0);

        *service.error.lock().unwrap() = Some(CloudflareDnsAdminError::UnknownOutcome);
        let app = super::super::router(state_with_write(None, false, Some(service), true));
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A?owner=www.example.com"
        );
        let (status, body) =
            json_request(app, axum::http::Method::PUT, &path, Body::from(PUT_A_BODY)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "unknown_outcome");
    }

    #[tokio::test]
    async fn record_replace_guard_is_forwarded_and_invalid_success_is_unknown() {
        let replace_body = PUT_A_BODY.replace(
            r#"{"type":"must_not_exist"}"#,
            r#"{"type":"match_revision","revision":"sha256:record-revision"}"#,
        );
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A?owner=www.example.com"
        );
        let service = Arc::new(FakeWriteService::default());
        let app = super::super::router(state_with_write(None, false, Some(service.clone()), true));
        let (status, _) = json_request(
            app,
            axum::http::Method::PUT,
            &path,
            Body::from(replace_body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            service
                .last_put_request
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .guard
                .expected_revision()
                .unwrap()
                .as_str(),
            "sha256:record-revision"
        );

        let mut invalid = record("another-account", ZONE_ID);
        invalid.proxy = Some(CloudflareProxyOptions::Proxied);
        *service.record_response.lock().unwrap() = Some(invalid);
        let app = super::super::router(state_with_write(None, false, Some(service), true));
        let (status, body) = json_request(
            app,
            axum::http::Method::PUT,
            &path,
            Body::from(replace_body),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "unknown_outcome");
    }

    #[tokio::test]
    async fn record_write_routes_require_service_and_capability() {
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets/A?owner=www.example.com"
        );
        for state in [
            state_with_write(None, false, None, false),
            state_with_write(None, false, None, true),
            state_with_write(
                None,
                false,
                Some(Arc::new(FakeWriteService::default())),
                false,
            ),
        ] {
            let (status, _) = json_request(
                super::super::router(state),
                axum::http::Method::PUT,
                &path,
                Body::from(PUT_A_BODY),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
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
        assert_eq!(service.list_record_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service.get_zone_calls.load(Ordering::SeqCst), 0);

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

    #[tokio::test]
    async fn record_list_uses_one_list_call_without_a_get_zone_preflight() {
        let service = Arc::new(FakeService::default());
        let path = format!(
            "/api/v1/center/cloudflare/dns/accounts/account-1/zones/{ZONE_ID}/record-sets?limit=10&cursor=record-cursor-1"
        );

        let (status, body) = request(
            super::super::router(state(Some(service.clone()), true)),
            &path,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["nextCursor"], "record-cursor-2");
        assert_eq!(service.list_record_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service.get_zone_calls.load(Ordering::SeqCst), 0);
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
