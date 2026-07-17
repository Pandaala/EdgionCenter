//! Credential-owning Cloudflare API v4 HTTP client.

use std::{
    collections::BTreeSet,
    fmt::{Debug, Formatter},
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_core::{
    AbsoluteDnsName, CaaTag, DnsCharacterString, DnsRecordSetValue, DnsTxtValue,
    DnssecDesiredState, NormalizedProviderError, ProviderErrorCategory,
};
use reqwest::{
    header::{self, HeaderValue},
    Client, StatusCode, Url,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    CloudflareApi, CloudflareApiResult, CloudflareBatchRequest, CloudflareBatchResult,
    CloudflareCreateZoneRequest, CloudflareDeleteZoneAck, CloudflareDnssec, CloudflareDnssecDs,
    CloudflareDnssecStatus, CloudflarePage, CloudflareRecord, CloudflareZone, CloudflareZoneKind,
    CloudflareZoneStatus,
};

const DEFAULT_API_BASE: &str = "https://api.cloudflare.com/client/v4/";
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_MUTATION_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_THROTTLE_DELAY_MS: u64 = 1_000;
const MAX_RETRY_DELAY_MS: u64 = 60 * 60 * 1_000;

/// API token material owned by this adapter crate and zeroized on drop.
pub struct CloudflareApiToken(Zeroizing<String>);

impl CloudflareApiToken {
    pub fn new(value: impl Into<String>) -> CloudflareApiResult<Self> {
        let mut value = value.into();
        if value.is_empty()
            || value.len() > 4096
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            value.zeroize();
            return Err(error(
                ProviderErrorCategory::Validation,
                "invalid_cloudflare_api_token",
                None,
                None,
            ));
        }
        Ok(Self(Zeroizing::new(value)))
    }

    fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for CloudflareApiToken {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CloudflareApiToken([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudflareTokenStatus {
    Active,
    Disabled,
    Expired,
    Unknown,
}

/// Minimal Cloudflare v4 HTTP client. It owns credential material and never
/// includes provider response bodies, headers, or tokens in returned errors.
pub struct CloudflareHttpApi {
    client: Client,
    base_url: Url,
    token: CloudflareApiToken,
}

impl Debug for CloudflareHttpApi {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CloudflareHttpApi")
            .field("base_url", &self.base_url)
            .field("token", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl CloudflareHttpApi {
    pub fn new(token: CloudflareApiToken) -> CloudflareApiResult<Self> {
        Self::with_base_url(token, DEFAULT_API_BASE)
    }

    /// Custom endpoints are intended for private Cloudflare-compatible API
    /// gateways and tests. Plain HTTP is accepted only for loopback hosts.
    pub fn with_base_url(
        token: CloudflareApiToken,
        base_url: impl AsRef<str>,
    ) -> CloudflareApiResult<Self> {
        let mut base_url = Url::parse(base_url.as_ref()).map_err(|_| {
            error(
                ProviderErrorCategory::Validation,
                "invalid_cloudflare_api_base_url",
                None,
                None,
            )
        })?;
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }
        if base_url.cannot_be_a_base()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || base_url.username() != ""
            || base_url.password().is_some()
            || !matches!(base_url.scheme(), "https" | "http")
            || (base_url.scheme() == "http" && !is_loopback(&base_url))
        {
            return Err(error(
                ProviderErrorCategory::Validation,
                "invalid_cloudflare_api_base_url",
                None,
                None,
            ));
        }
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| {
                error(
                    ProviderErrorCategory::Validation,
                    "cloudflare_http_client_build_failed",
                    None,
                    None,
                )
            })?;
        Ok(Self {
            client,
            base_url,
            token,
        })
    }

    /// Verifies only token state. This does not prove DNS Read or DNS Write
    /// authorization, which must be checked by an account-scoped DNS request.
    pub async fn verify_user_token(&self) -> CloudflareApiResult<CloudflareTokenStatus> {
        let envelope: Envelope<TokenVerification> = self.get("user/tokens/verify", &[]).await?;
        let result = require_result(envelope)?;
        Ok(match result.status.as_str() {
            "active" => CloudflareTokenStatus::Active,
            "disabled" => CloudflareTokenStatus::Disabled,
            "expired" => CloudflareTokenStatus::Expired,
            _ => CloudflareTokenStatus::Unknown,
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> CloudflareApiResult<Envelope<T>> {
        let url = self.base_url.join(path).map_err(|_| {
            error(
                ProviderErrorCategory::Validation,
                "invalid_cloudflare_request_path",
                None,
                None,
            )
        })?;
        let response = self
            .client
            .get(url)
            .header(header::AUTHORIZATION, self.authorization_header()?)
            .query(query)
            .send()
            .await
            .map_err(map_transport_error)?;
        decode_response(response, self.token.expose(), RequestKind::Read).await
    }

    async fn post_batch<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &CloudflareBatchRequest,
    ) -> CloudflareApiResult<Envelope<T>> {
        let encoded = serde_json::to_vec(body).map_err(|_| {
            error(
                ProviderErrorCategory::Validation,
                "cloudflare_batch_encoding_failed",
                None,
                None,
            )
        })?;
        if encoded.len() > MAX_MUTATION_REQUEST_BYTES {
            return Err(error(
                ProviderErrorCategory::Validation,
                "cloudflare_batch_request_too_large",
                None,
                None,
            ));
        }
        let url = self.base_url.join(path).map_err(|_| {
            error(
                ProviderErrorCategory::Validation,
                "invalid_cloudflare_request_path",
                None,
                None,
            )
        })?;
        let response = self
            .client
            .post(url)
            .header(header::AUTHORIZATION, self.authorization_header()?)
            .header(header::CONTENT_TYPE, "application/json")
            .body(encoded)
            .send()
            .await
            .map_err(map_write_transport_error)?;
        decode_response(response, self.token.expose(), RequestKind::Mutation).await
    }

    async fn mutate<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> CloudflareApiResult<Envelope<T>> {
        let encoded = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| malformed("cloudflare_mutation_encoding_failed"))?;
        if encoded
            .as_ref()
            .is_some_and(|value| value.len() > MAX_MUTATION_REQUEST_BYTES)
        {
            return Err(malformed("cloudflare_mutation_request_too_large"));
        }
        let url = self
            .base_url
            .join(path)
            .map_err(|_| malformed("invalid_cloudflare_request_path"))?;
        let mut request = self
            .client
            .request(method, url)
            .header(header::AUTHORIZATION, self.authorization_header()?);
        if let Some(encoded) = encoded {
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(encoded);
        }
        let response = request.send().await.map_err(map_write_transport_error)?;
        decode_response(response, self.token.expose(), RequestKind::Mutation).await
    }

    fn authorization_header(&self) -> CloudflareApiResult<HeaderValue> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(7 + self.token.expose().len()));
        bytes.extend_from_slice(b"Bearer ");
        bytes.extend_from_slice(self.token.expose().as_bytes());
        let mut value = HeaderValue::from_bytes(&bytes).map_err(|_| {
            error(
                ProviderErrorCategory::Validation,
                "invalid_cloudflare_authorization_header",
                None,
                None,
            )
        })?;
        value.set_sensitive(true);
        Ok(value)
    }
}

#[async_trait]
impl CloudflareApi for CloudflareHttpApi {
    async fn create_zone(
        &self,
        request: &CloudflareCreateZoneRequest,
    ) -> CloudflareApiResult<CloudflareZone> {
        validate_identifier(&request.account_id, "invalid_cloudflare_account_id")?;
        AbsoluteDnsName::new(&request.name)
            .map_err(|_| malformed("invalid_cloudflare_zone_name"))?;
        let body = serde_json::json!({
            "account": { "id": request.account_id },
            "name": request.name,
            "type": zone_kind_name(request.kind),
        });
        let envelope: Envelope<RawZone> = self
            .mutate(reqwest::Method::POST, "zones", Some(&body))
            .await?;
        map_zone(require_lifecycle_mutation_result(envelope)?)
            .map_err(|_| unknown("cloudflare_create_zone_result_invalid", None))
    }

    async fn get_zone(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareZone>> {
        validate_identifier(zone_id, "invalid_cloudflare_zone_id")?;
        let envelope: Envelope<RawZone> = match self.get(&format!("zones/{zone_id}"), &[]).await {
            Ok(envelope) => envelope,
            Err(error) if error.category() == ProviderErrorCategory::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(Some(map_zone(require_result(envelope)?)?))
    }

    async fn delete_zone(&self, zone_id: &str) -> CloudflareApiResult<CloudflareDeleteZoneAck> {
        validate_identifier(zone_id, "invalid_cloudflare_zone_id")?;
        let envelope: Envelope<RawDeleteZoneAck> = self
            .mutate(reqwest::Method::DELETE, &format!("zones/{zone_id}"), None)
            .await?;
        let result = require_lifecycle_mutation_result(envelope)?;
        if result.id.is_empty() {
            return Err(unknown("cloudflare_delete_zone_result_invalid", None));
        }
        Ok(CloudflareDeleteZoneAck { id: result.id })
    }

    async fn get_dnssec(&self, zone_id: &str) -> CloudflareApiResult<Option<CloudflareDnssec>> {
        validate_identifier(zone_id, "invalid_cloudflare_zone_id")?;
        let envelope: Envelope<RawDnssec> = match self
            .get(&format!("zones/{zone_id}/dnssec"), &[])
            .await
        {
            Ok(envelope) => envelope,
            Err(error) if error.category() == ProviderErrorCategory::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(Some(map_dnssec(require_result(envelope)?)?))
    }

    async fn patch_dnssec(
        &self,
        zone_id: &str,
        desired: DnssecDesiredState,
    ) -> CloudflareApiResult<CloudflareDnssec> {
        validate_identifier(zone_id, "invalid_cloudflare_zone_id")?;
        let status = match desired {
            DnssecDesiredState::Enabled => "active",
            DnssecDesiredState::Disabled => "disabled",
        };
        let envelope: Envelope<RawDnssec> = self
            .mutate(
                reqwest::Method::PATCH,
                &format!("zones/{zone_id}/dnssec"),
                Some(&serde_json::json!({ "status": status })),
            )
            .await?;
        map_dnssec(require_lifecycle_mutation_result(envelope)?)
            .map_err(|_| unknown("cloudflare_dnssec_patch_result_invalid", None))
    }

    async fn list_zones(
        &self,
        account_id: &str,
        page: u32,
        per_page: u32,
    ) -> CloudflareApiResult<CloudflarePage<CloudflareZone>> {
        validate_identifier(account_id, "invalid_cloudflare_account_id")?;
        validate_page(page, per_page, 50)?;
        let envelope: Envelope<Vec<RawZone>> = self
            .get(
                "zones",
                &[
                    ("account.id", account_id.to_string()),
                    ("type", "full,partial,secondary,internal".to_string()),
                    ("page", page.to_string()),
                    ("per_page", per_page.to_string()),
                ],
            )
            .await?;
        let info = validate_result_info(&envelope, page, per_page)?;
        let items = require_result(envelope)?
            .into_iter()
            .map(map_zone)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CloudflarePage {
            items,
            page: info.page,
            total_pages: info.total_pages,
        })
    }

    async fn list_records(
        &self,
        zone_id: &str,
        page: u32,
        per_page: u32,
    ) -> CloudflareApiResult<CloudflarePage<CloudflareRecord>> {
        validate_identifier(zone_id, "invalid_cloudflare_zone_id")?;
        validate_page(page, per_page, 5_000_000)?;
        let envelope: Envelope<Vec<RawRecord>> = self
            .get(
                &format!("zones/{zone_id}/dns_records"),
                &[
                    ("page", page.to_string()),
                    ("per_page", per_page.to_string()),
                ],
            )
            .await?;
        let info = validate_result_info(&envelope, page, per_page)?;
        let items = require_result(envelope)?
            .into_iter()
            .map(map_record)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CloudflarePage {
            items,
            page: info.page,
            total_pages: info.total_pages,
        })
    }

    async fn batch_records(
        &self,
        zone_id: &str,
        request: &CloudflareBatchRequest,
    ) -> CloudflareApiResult<CloudflareBatchResult> {
        validate_identifier(zone_id, "invalid_cloudflare_zone_id")?;
        let envelope: Envelope<RawBatchResult> = self
            .post_batch(&format!("zones/{zone_id}/dns_records/batch"), request)
            .await?;
        let result = require_mutation_result(envelope)?;
        if !result.patches.is_empty() || !result.puts.is_empty() {
            return Err(unknown("cloudflare_batch_unexpected_result_section", None));
        }
        let deletes = result
            .deletes
            .into_iter()
            .map(map_record)
            .collect::<Result<_, _>>()
            .map_err(|_| unknown("cloudflare_batch_delete_result_invalid", None))?;
        let posts = result
            .posts
            .into_iter()
            .map(map_record)
            .collect::<Result<_, _>>()
            .map_err(|_| unknown("cloudflare_batch_post_result_invalid", None))?;
        Ok(CloudflareBatchResult { deletes, posts })
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    success: bool,
    result: Option<T>,
    #[serde(default)]
    errors: Vec<ApiMessage>,
    result_info: Option<ResultInfo>,
    #[serde(skip)]
    request_id: Option<String>,
}

#[derive(Deserialize)]
struct ApiMessage {
    code: u64,
}

#[derive(Clone, Copy, Deserialize)]
struct ResultInfo {
    page: u32,
    per_page: u32,
    count: usize,
    total_pages: u32,
}

#[derive(Deserialize)]
struct TokenVerification {
    status: String,
}

#[derive(Deserialize)]
struct RawZone {
    id: String,
    account: RawAccount,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    status: String,
    #[serde(default)]
    name_servers: Vec<String>,
    modified_on: Option<String>,
}

#[derive(Deserialize)]
struct RawDeleteZoneAck {
    id: String,
}

#[derive(Deserialize)]
struct RawDnssec {
    status: String,
    key_tag: Option<u16>,
    algorithm: Option<u8>,
    digest_type: Option<u8>,
    digest: Option<String>,
    modified_on: Option<String>,
}

#[derive(Deserialize)]
struct RawAccount {
    id: String,
}

#[derive(Deserialize)]
struct RawRecord {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    ttl: u32,
    content: Option<String>,
    data: Option<Value>,
    priority: Option<u16>,
    proxied: Option<bool>,
    proxiable: bool,
    settings: Option<RawSettings>,
    #[serde(default)]
    private_routing: bool,
    comment: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    modified_on: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSettings {
    #[serde(default)]
    ipv4_only: bool,
    #[serde(default)]
    ipv6_only: bool,
    flatten_cname: Option<bool>,
}

#[derive(Deserialize)]
struct RawBatchResult {
    #[serde(default)]
    deletes: Vec<RawRecord>,
    #[serde(default)]
    patches: Vec<RawRecord>,
    #[serde(default)]
    puts: Vec<RawRecord>,
    #[serde(default)]
    posts: Vec<RawRecord>,
}

fn map_zone(zone: RawZone) -> CloudflareApiResult<CloudflareZone> {
    let kind = match zone.kind.as_str() {
        "full" => CloudflareZoneKind::Full,
        "partial" => CloudflareZoneKind::Partial,
        "secondary" => CloudflareZoneKind::Secondary,
        "internal" => CloudflareZoneKind::Internal,
        _ => return Err(malformed("unsupported_cloudflare_zone_type")),
    };
    let status = match zone.status.as_str() {
        "initializing" => CloudflareZoneStatus::Initializing,
        "pending" => CloudflareZoneStatus::Pending,
        "active" => CloudflareZoneStatus::Active,
        "moved" => CloudflareZoneStatus::Moved,
        _ => return Err(malformed("unsupported_cloudflare_zone_status")),
    };
    let name_servers = zone
        .name_servers
        .into_iter()
        .map(|value| {
            AbsoluteDnsName::new(value).map_err(|_| malformed("invalid_cloudflare_zone_nameserver"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(CloudflareZone {
        id: zone.id,
        account_id: zone.account.id,
        name: zone.name,
        kind,
        status,
        name_servers,
        modified_on: zone.modified_on,
    })
}

fn zone_kind_name(kind: CloudflareZoneKind) -> &'static str {
    match kind {
        CloudflareZoneKind::Full => "full",
        CloudflareZoneKind::Partial => "partial",
        CloudflareZoneKind::Secondary => "secondary",
        CloudflareZoneKind::Internal => "internal",
    }
}

fn map_dnssec(value: RawDnssec) -> CloudflareApiResult<CloudflareDnssec> {
    let status = match value.status.as_str() {
        "active" => CloudflareDnssecStatus::Active,
        "pending" => CloudflareDnssecStatus::Pending,
        "disabled" => CloudflareDnssecStatus::Disabled,
        "pending-disabled" => CloudflareDnssecStatus::PendingDisabled,
        "error" => CloudflareDnssecStatus::Error,
        _ => return Err(malformed("unsupported_cloudflare_dnssec_status")),
    };
    let ds = match (
        value.key_tag,
        value.algorithm,
        value.digest_type,
        value.digest,
    ) {
        (Some(key_tag), Some(algorithm), Some(digest_type), Some(digest)) => {
            Some(CloudflareDnssecDs {
                key_tag,
                algorithm,
                digest_type,
                digest,
            })
        }
        (None, None, None, None) => None,
        _ => return Err(malformed("incomplete_cloudflare_dnssec_ds")),
    };
    Ok(CloudflareDnssec {
        status,
        ds,
        modified_on: value.modified_on,
    })
}

fn map_record(record: RawRecord) -> CloudflareApiResult<CloudflareRecord> {
    let content = record.content.as_deref();
    let value = match record.kind.as_str() {
        "A" => DnsRecordSetValue::A {
            address: required_content(content)?
                .parse::<Ipv4Addr>()
                .map_err(|_| malformed("invalid_cloudflare_a_content"))?,
        },
        "AAAA" => DnsRecordSetValue::Aaaa {
            address: required_content(content)?
                .parse::<Ipv6Addr>()
                .map_err(|_| malformed("invalid_cloudflare_aaaa_content"))?,
        },
        "CNAME" => DnsRecordSetValue::Cname {
            target: absolute_name(
                required_content(content)?,
                "invalid_cloudflare_cname_content",
            )?,
        },
        "NS" => DnsRecordSetValue::Ns {
            target: absolute_name(required_content(content)?, "invalid_cloudflare_ns_content")?,
        },
        "TXT" => DnsRecordSetValue::Txt {
            value: txt_value(required_content(content)?)?,
        },
        "MX" => DnsRecordSetValue::Mx {
            preference: record
                .priority
                .or_else(|| data_u16(record.data.as_ref(), "priority"))
                .ok_or_else(|| malformed("missing_cloudflare_mx_priority"))?,
            exchange: absolute_name(required_content(content)?, "invalid_cloudflare_mx_content")?,
        },
        "SRV" => DnsRecordSetValue::Srv {
            priority: required_data_u16(
                record.data.as_ref(),
                "priority",
                "missing_cloudflare_srv_priority",
            )?,
            weight: required_data_u16(
                record.data.as_ref(),
                "weight",
                "missing_cloudflare_srv_weight",
            )?,
            port: required_data_u16(record.data.as_ref(), "port", "missing_cloudflare_srv_port")?,
            target: absolute_name(
                required_data_str(
                    record.data.as_ref(),
                    "target",
                    "missing_cloudflare_srv_target",
                )?,
                "invalid_cloudflare_srv_target",
            )?,
        },
        "CAA" => DnsRecordSetValue::Caa {
            flags: required_data_u8(
                record.data.as_ref(),
                "flags",
                "missing_cloudflare_caa_flags",
            )?,
            tag: CaaTag::new(required_data_str(
                record.data.as_ref(),
                "tag",
                "missing_cloudflare_caa_tag",
            )?)
            .map_err(|_| malformed("invalid_cloudflare_caa_tag"))?,
            value: DnsCharacterString::new(
                required_data_str(
                    record.data.as_ref(),
                    "value",
                    "missing_cloudflare_caa_value",
                )?
                .as_bytes()
                .to_vec(),
            )
            .map_err(|_| malformed("invalid_cloudflare_caa_value"))?,
        },
        _ => return Err(malformed("unsupported_cloudflare_record_type")),
    };
    let settings = record.settings.unwrap_or_default();
    let mut tags = BTreeSet::new();
    for tag in record.tags {
        if !tags.insert(tag) {
            return Err(malformed("duplicate_cloudflare_record_tag"));
        }
    }
    Ok(CloudflareRecord {
        id: record.id,
        name: record.name,
        ttl: record.ttl,
        value,
        proxied: record.proxied,
        proxiable: record.proxiable,
        flatten_cname: settings.flatten_cname,
        ipv4_only: settings.ipv4_only,
        ipv6_only: settings.ipv6_only,
        private_routing: record.private_routing,
        comment: record.comment,
        tags,
        modified_on: record.modified_on,
    })
}

fn required_content(value: Option<&str>) -> CloudflareApiResult<&str> {
    value.ok_or_else(|| malformed("missing_cloudflare_record_content"))
}

fn absolute_name(value: &str, code: &str) -> CloudflareApiResult<AbsoluteDnsName> {
    AbsoluteDnsName::new(value).map_err(|_| malformed(code))
}

pub(crate) fn txt_value(value: &str) -> CloudflareApiResult<DnsTxtValue> {
    let bytes = value.as_bytes();
    let mut offset = 0;
    let mut segments = Vec::new();
    while offset < bytes.len() {
        while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        if offset == bytes.len() {
            break;
        }
        if bytes[offset] != b'"' {
            return Err(malformed("invalid_cloudflare_txt_content"));
        }
        offset += 1;
        let mut segment = Vec::new();
        let mut closed = false;
        while offset < bytes.len() {
            match bytes[offset] {
                b'"' => {
                    offset += 1;
                    closed = true;
                    break;
                }
                b'\\' => {
                    offset += 1;
                    let Some(escaped) = bytes.get(offset).copied() else {
                        return Err(malformed("invalid_cloudflare_txt_content"));
                    };
                    if offset + 2 < bytes.len()
                        && bytes[offset..offset + 3].iter().all(u8::is_ascii_digit)
                    {
                        let numeric = u16::from(bytes[offset] - b'0') * 100
                            + u16::from(bytes[offset + 1] - b'0') * 10
                            + u16::from(bytes[offset + 2] - b'0');
                        segment.push(
                            u8::try_from(numeric)
                                .map_err(|_| malformed("invalid_cloudflare_txt_numeric_escape"))?,
                        );
                        offset += 3;
                    } else {
                        segment.push(escaped);
                        offset += 1;
                    }
                }
                byte if byte.is_ascii_control() => {
                    return Err(malformed("invalid_cloudflare_txt_content"));
                }
                byte => {
                    segment.push(byte);
                    offset += 1;
                }
            }
            if segment.len() > 255 {
                return Err(malformed("invalid_cloudflare_txt_segment_length"));
            }
        }
        if !closed {
            return Err(malformed("invalid_cloudflare_txt_content"));
        }
        if bytes
            .get(offset)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            return Err(malformed("invalid_cloudflare_txt_content"));
        }
        segments.push(
            DnsCharacterString::new(segment)
                .map_err(|_| malformed("invalid_cloudflare_txt_segment"))?,
        );
    }
    DnsTxtValue::new(segments).map_err(|_| malformed("invalid_cloudflare_txt_content"))
}

fn required_data_str<'a>(
    data: Option<&'a Value>,
    field: &str,
    code: &str,
) -> CloudflareApiResult<&'a str> {
    data.and_then(|value| value.get(field))
        .and_then(Value::as_str)
        .ok_or_else(|| malformed(code))
}

fn data_u16(data: Option<&Value>, field: &str) -> Option<u16> {
    data.and_then(|value| value.get(field))
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
}

fn required_data_u16(data: Option<&Value>, field: &str, code: &str) -> CloudflareApiResult<u16> {
    data_u16(data, field).ok_or_else(|| malformed(code))
}

fn required_data_u8(data: Option<&Value>, field: &str, code: &str) -> CloudflareApiResult<u8> {
    data.and_then(|value| value.get(field))
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| malformed(code))
}

fn validate_identifier(value: &str, code: &str) -> CloudflareApiResult<()> {
    if value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(error(ProviderErrorCategory::Validation, code, None, None))
    }
}

fn validate_page(page: u32, per_page: u32, maximum: u32) -> CloudflareApiResult<()> {
    if page == 0 || per_page == 0 || per_page > maximum {
        Err(error(
            ProviderErrorCategory::Validation,
            "invalid_cloudflare_page_request",
            None,
            None,
        ))
    } else {
        Ok(())
    }
}

fn require_result<T>(envelope: Envelope<T>) -> CloudflareApiResult<T> {
    if !envelope.success {
        return Err(envelope_error(&envelope.errors, envelope.request_id));
    }
    envelope
        .result
        .ok_or_else(|| malformed("missing_cloudflare_result"))
}

fn require_mutation_result<T>(envelope: Envelope<T>) -> CloudflareApiResult<T> {
    if !envelope.success {
        return Err(envelope_error(&envelope.errors, envelope.request_id));
    }
    envelope
        .result
        .ok_or_else(|| unknown("missing_cloudflare_batch_result", envelope.request_id))
}

fn require_lifecycle_mutation_result<T>(envelope: Envelope<T>) -> CloudflareApiResult<T> {
    if !envelope.success {
        return Err(envelope_error(&envelope.errors, envelope.request_id));
    }
    envelope
        .result
        .ok_or_else(|| unknown("missing_cloudflare_lifecycle_result", envelope.request_id))
}

fn validate_result_info<T>(
    envelope: &Envelope<Vec<T>>,
    requested_page: u32,
    requested_per_page: u32,
) -> CloudflareApiResult<ResultInfo> {
    let info = envelope
        .result_info
        .ok_or_else(|| malformed("missing_cloudflare_result_info"))?;
    let result_len = envelope.result.as_ref().map(Vec::len);
    if info.page != requested_page
        || info.per_page != requested_per_page
        || result_len != Some(info.count)
    {
        return Err(malformed("invalid_cloudflare_result_info"));
    }
    Ok(info)
}

async fn decode_response<T: DeserializeOwned>(
    mut response: reqwest::Response,
    credential: &str,
    request_kind: RequestKind,
) -> CloudflareApiResult<Envelope<T>> {
    let status = response.status();
    let request_id = sanitized_header(response.headers(), "cf-ray", credential);
    let retry_after_ms = retry_after_ms(response.headers());
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return if status.is_success() {
            Err(response_decode_error(
                request_kind,
                "cloudflare_response_too_large",
                request_id,
            ))
        } else {
            Err(http_status_error(
                status,
                retry_after_ms,
                request_id,
                None,
                request_kind,
            ))
        };
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| map_response_transport_error(error, request_kind))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return if status.is_success() {
                Err(response_decode_error(
                    request_kind,
                    "cloudflare_response_too_large",
                    request_id,
                ))
            } else {
                Err(http_status_error(
                    status,
                    retry_after_ms,
                    request_id,
                    None,
                    request_kind,
                ))
            };
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let parsed = serde_json::from_slice::<Envelope<Value>>(&bytes).ok();
        let provider_code = parsed
            .as_ref()
            .and_then(|body| body.errors.first())
            .map(|item| item.code);
        return Err(http_status_error(
            status,
            retry_after_ms,
            request_id,
            provider_code,
            request_kind,
        ));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        response_decode_error(request_kind, "invalid_cloudflare_json", request_id.clone())
    })?;
    let mut envelope: Envelope<T> = serde_json::from_value(value).map_err(|_| {
        response_decode_error(
            request_kind,
            "invalid_cloudflare_response",
            request_id.clone(),
        )
    })?;
    envelope.request_id = request_id;
    Ok(envelope)
}

fn http_status_error(
    status: StatusCode,
    retry_after_ms: Option<u64>,
    request_id: Option<String>,
    provider_code: Option<u64>,
    request_kind: RequestKind,
) -> NormalizedProviderError {
    let code = provider_code
        .map(|value| format!("cloudflare_api_{value}"))
        .unwrap_or_else(|| format!("cloudflare_http_{}", status.as_u16()));
    let category = if request_kind == RequestKind::Mutation
        && (status == StatusCode::REQUEST_TIMEOUT || status.is_server_error())
    {
        ProviderErrorCategory::UnknownOutcome
    } else {
        status_category(status)
    };
    let retry = match category {
        ProviderErrorCategory::Throttled => {
            Some(retry_after_ms.unwrap_or(DEFAULT_THROTTLE_DELAY_MS))
        }
        ProviderErrorCategory::Transient | ProviderErrorCategory::Quota => retry_after_ms,
        _ => None,
    };
    error(category, &code, retry, request_id)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestKind {
    Read,
    Mutation,
}

fn status_category(status: StatusCode) -> ProviderErrorCategory {
    match status.as_u16() {
        401 => ProviderErrorCategory::Authentication,
        403 => ProviderErrorCategory::Authorization,
        404 => ProviderErrorCategory::NotFound,
        408 => ProviderErrorCategory::Transient,
        409 => ProviderErrorCategory::Conflict,
        429 => ProviderErrorCategory::Throttled,
        500..=599 => ProviderErrorCategory::Transient,
        _ => ProviderErrorCategory::Validation,
    }
}

fn retry_after_ms(headers: &header::HeaderMap) -> Option<u64> {
    headers
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|seconds| seconds.checked_mul(1_000))
        .map(|delay| delay.min(MAX_RETRY_DELAY_MS))
}

fn sanitized_header(headers: &header::HeaderMap, name: &str, credential: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 512
                && value.trim() == *value
                && !value.chars().any(char::is_control)
                && !value.contains(credential)
        })
        .map(ToOwned::to_owned)
}

fn envelope_error(errors: &[ApiMessage], request_id: Option<String>) -> NormalizedProviderError {
    let provider_code = errors.first().map(|item| item.code);
    let code = provider_code
        .map(|value| format!("cloudflare_api_{value}"))
        .unwrap_or_else(|| "cloudflare_api_failure".to_string());
    let category = match provider_code {
        Some(9103 | 9109 | 10000) => ProviderErrorCategory::Authentication,
        _ => ProviderErrorCategory::Validation,
    };
    error(category, &code, None, request_id)
}

fn map_transport_error(error_value: reqwest::Error) -> NormalizedProviderError {
    let code = if error_value.is_timeout() {
        "cloudflare_http_timeout"
    } else if error_value.is_connect() {
        "cloudflare_http_connect_failed"
    } else if error_value.is_builder() {
        "cloudflare_http_request_invalid"
    } else {
        "cloudflare_http_transport_failed"
    };
    let category = if error_value.is_builder() {
        ProviderErrorCategory::Validation
    } else {
        ProviderErrorCategory::Transient
    };
    error(category, code, None, None)
}

fn map_write_transport_error(error_value: reqwest::Error) -> NormalizedProviderError {
    if error_value.is_builder() {
        error(
            ProviderErrorCategory::Validation,
            "cloudflare_http_request_invalid",
            None,
            None,
        )
    } else {
        unknown("cloudflare_batch_transport_unknown", None)
    }
}

fn map_response_transport_error(
    error_value: reqwest::Error,
    request_kind: RequestKind,
) -> NormalizedProviderError {
    match request_kind {
        RequestKind::Read => map_transport_error(error_value),
        RequestKind::Mutation => unknown("cloudflare_batch_response_unknown", None),
    }
}

fn response_decode_error(
    request_kind: RequestKind,
    code: &str,
    request_id: Option<String>,
) -> NormalizedProviderError {
    match request_kind {
        RequestKind::Read => transient(code, request_id),
        RequestKind::Mutation => unknown(code, request_id),
    }
}

fn malformed(code: &str) -> NormalizedProviderError {
    malformed_with_request(code, None)
}

fn malformed_with_request(code: &str, request_id: Option<String>) -> NormalizedProviderError {
    error(ProviderErrorCategory::Validation, code, None, request_id)
}

fn transient(code: &str, request_id: Option<String>) -> NormalizedProviderError {
    error(ProviderErrorCategory::Transient, code, None, request_id)
}

fn unknown(code: &str, request_id: Option<String>) -> NormalizedProviderError {
    error(
        ProviderErrorCategory::UnknownOutcome,
        code,
        None,
        request_id,
    )
}

fn error(
    category: ProviderErrorCategory,
    code: &str,
    retry_after_ms: Option<u64>,
    request_id: Option<String>,
) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        code,
        "Cloudflare API request failed",
        retry_after_ms,
        request_id,
    )
    .expect("static Cloudflare provider error")
}

fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

#[cfg(test)]
mod tests {
    use wiremock::{
        matchers::{body_json, header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    use super::*;

    const ACCOUNT_ID: &str = "0123456789abcdef0123456789abcdef";
    const ZONE_ID: &str = "abcdef0123456789abcdef0123456789";

    async fn api(server: &MockServer) -> CloudflareHttpApi {
        CloudflareHttpApi::with_base_url(
            CloudflareApiToken::new("super-secret-token").unwrap(),
            format!("{}/client/v4/", server.uri()),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn lists_zones_with_bearer_auth_and_account_scope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/client/v4/zones"))
            .and(header("authorization", "Bearer super-secret-token"))
            .and(query_param("account.id", ACCOUNT_ID))
            .and(query_param(
                "type",
                "full,partial,secondary,internal",
            ))
            .and(query_param("page", "2"))
            .and(query_param("per_page", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "messages": [],
                "result": [{
                    "id": ZONE_ID,
                    "account": { "id": ACCOUNT_ID },
                    "name": "example.test",
                    "type": "full",
                    "status": "active",
                    "name_servers": ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"],
                    "modified_on": "2026-07-17T00:00:00Z"
                }],
                "result_info": { "page": 2, "per_page": 50, "total_pages": 3, "count": 1, "total_count": 101 }
            })))
            .mount(&server)
            .await;
        let page = api(&server)
            .await
            .list_zones(ACCOUNT_ID, 2, 50)
            .await
            .unwrap();
        assert_eq!(page.page, 2);
        assert_eq!(page.total_pages, 3);
        assert_eq!(page.items[0].account_id, ACCOUNT_ID);
    }

    #[tokio::test]
    async fn decodes_supported_record_shapes_and_rejects_unknown_types() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/client/v4/zones/{ZONE_ID}/dns_records")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": [{
                    "id": "11111111111111111111111111111111",
                    "name": "_service._tcp.example.test",
                    "type": "SRV",
                    "ttl": 300,
                    "content": "0 5 443 target.example.test",
                    "data": { "priority": 0, "weight": 5, "port": 443, "target": "target.example.test" },
                    "proxied": false,
                    "proxiable": false,
                    "settings": {},
                    "tags": ["owner:platform"],
                    "modified_on": "2026-07-17T00:00:00Z"
                }],
                "result_info": { "page": 1, "per_page": 5000, "count": 1, "total_pages": 1 }
            })))
            .mount(&server)
            .await;
        let records = api(&server)
            .await
            .list_records(ZONE_ID, 1, 5000)
            .await
            .unwrap();
        assert!(matches!(
            records.items[0].value,
            DnsRecordSetValue::Srv { port: 443, .. }
        ));

        let unknown = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": [{
                    "id": "22222222222222222222222222222222", "name": "example.test",
                    "type": "HTTPS", "ttl": 300, "content": "1 . alpn=h2", "proxiable": false
                }],
                "result_info": { "page": 1, "per_page": 5000, "count": 1, "total_pages": 1 }
            })))
            .mount(&unknown)
            .await;
        let error = api(&unknown)
            .await
            .list_records(ZONE_ID, 1, 5000)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "unsupported_cloudflare_record_type");
    }

    #[tokio::test]
    async fn sanitizes_provider_errors_and_preserves_retry_metadata() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "7")
                    .insert_header("cf-ray", "safe-request-id")
                    .set_body_json(serde_json::json!({
                        "success": false,
                        "errors": [{ "code": 1015, "message": "secret provider detail" }],
                        "result": null
                    })),
            )
            .mount(&server)
            .await;
        let error = api(&server).await.get_zone(ZONE_ID).await.unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Throttled);
        assert_eq!(error.code(), "cloudflare_api_1015");
        assert_eq!(error.retry_after_ms(), Some(7000));
        assert_eq!(error.provider_request_id(), Some("safe-request-id"));
        assert!(!format!("{error:?}").contains("secret provider detail"));
        assert!(!format!("{:?}", api(&server).await).contains("super-secret-token"));
    }

    #[tokio::test]
    async fn two_hundred_failure_envelope_is_not_accepted() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": false,
                "errors": [{ "code": 9109, "message": "Invalid access token" }],
                "result": null
            })))
            .mount(&server)
            .await;
        let error = api(&server).await.get_zone(ZONE_ID).await.unwrap_err();
        assert_eq!(error.code(), "cloudflare_api_9109");
        assert_eq!(error.category(), ProviderErrorCategory::Authentication);
    }

    #[tokio::test]
    async fn not_found_zone_maps_to_absence() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not json"))
            .mount(&server)
            .await;
        assert!(api(&server)
            .await
            .get_zone(ZONE_ID)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn unknown_record_settings_fail_closed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": [{
                    "id": "33333333333333333333333333333333",
                    "name": "example.test", "type": "A", "ttl": 300,
                    "content": "192.0.2.1", "proxied": false, "proxiable": true,
                    "settings": { "new_provider_behavior": true }
                }],
                "result_info": { "page": 1, "per_page": 5000, "count": 1, "total_pages": 1 }
            })))
            .mount(&server)
            .await;
        let error = api(&server)
            .await
            .list_records(ZONE_ID, 1, 5000)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "invalid_cloudflare_response");
    }

    #[tokio::test]
    async fn untrusted_request_id_cannot_echo_the_credential() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(500)
                    .insert_header("cf-ray", "prefix-super-secret-token-suffix")
                    .set_body_string("super-secret-token in body"),
            )
            .mount(&server)
            .await;
        let error = api(&server).await.get_zone(ZONE_ID).await.unwrap_err();
        let serialized = serde_json::to_string(&error).unwrap();
        assert!(!serialized.contains("super-secret-token"));
        assert_eq!(error.provider_request_id(), None);
    }

    #[tokio::test]
    async fn retry_after_is_bounded() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "999999999999")
                    .set_body_string("throttled"),
            )
            .mount(&server)
            .await;
        let error = api(&server).await.get_zone(ZONE_ID).await.unwrap_err();
        assert_eq!(error.retry_after_ms(), Some(MAX_RETRY_DELAY_MS));
    }

    #[tokio::test]
    async fn invalid_path_identifier_is_rejected_before_network_io() {
        let server = MockServer::start().await;
        let error = api(&server)
            .await
            .get_zone("../dns_records?token=exfiltrate")
            .await
            .unwrap_err();
        assert_eq!(error.code(), "invalid_cloudflare_zone_id");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn redirects_are_not_followed_with_authorization() {
        let destination = MockServer::start().await;
        let origin = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/stolen", destination.uri())),
            )
            .mount(&origin)
            .await;
        let error = api(&origin).await.get_zone(ZONE_ID).await.unwrap_err();
        assert_eq!(error.code(), "cloudflare_http_302");
        assert!(destination.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn verifies_user_token_state_without_claiming_dns_permissions() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/client/v4/user/tokens/verify"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": { "id": "token-id", "status": "active" }
            })))
            .mount(&server)
            .await;
        assert_eq!(
            api(&server).await.verify_user_token().await.unwrap(),
            CloudflareTokenStatus::Active
        );
    }

    #[tokio::test]
    async fn zone_lifecycle_and_dnssec_use_typed_v4_paths() {
        let server = MockServer::start().await;
        let zone_result = serde_json::json!({
            "id": ZONE_ID,
            "account": { "id": ACCOUNT_ID },
            "name": "example.test",
            "type": "full",
            "status": "pending",
            "name_servers": ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"],
            "modified_on": "2026-07-17T00:00:00Z"
        });
        Mock::given(method("POST"))
            .and(path("/client/v4/zones"))
            .and(header("authorization", "Bearer super-secret-token"))
            .and(body_json(serde_json::json!({
                "account": { "id": ACCOUNT_ID },
                "name": "example.test",
                "type": "full"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true, "errors": [], "result": zone_result
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/client/v4/zones/{ZONE_ID}/dnssec")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": {
                    "status": "active", "key_tag": 2371, "algorithm": 13,
                    "digest_type": 2, "digest": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "modified_on": "dnssec-r1"
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path(format!("/client/v4/zones/{ZONE_ID}/dnssec")))
            .and(body_json(serde_json::json!({ "status": "disabled" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true, "errors": [],
                "result": { "status": "pending-disabled", "key_tag": 2371,
                    "algorithm": 13, "digest_type": 2, "digest": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(format!("/client/v4/zones/{ZONE_ID}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true, "errors": [], "result": { "id": ZONE_ID }
            })))
            .mount(&server)
            .await;

        let api = api(&server).await;
        let created = api
            .create_zone(&CloudflareCreateZoneRequest {
                account_id: ACCOUNT_ID.to_string(),
                name: "example.test".to_string(),
                kind: CloudflareZoneKind::Full,
            })
            .await
            .unwrap();
        assert_eq!(created.status, CloudflareZoneStatus::Pending);
        assert_eq!(created.name_servers.len(), 2);
        let observed = api.get_dnssec(ZONE_ID).await.unwrap().unwrap();
        assert_eq!(observed.status, CloudflareDnssecStatus::Active);
        assert_eq!(observed.ds.unwrap().key_tag, 2371);
        let disabling = api
            .patch_dnssec(ZONE_ID, DnssecDesiredState::Disabled)
            .await
            .unwrap();
        assert_eq!(disabling.status, CloudflareDnssecStatus::PendingDisabled);
        assert_eq!(api.delete_zone(ZONE_ID).await.unwrap().id, ZONE_ID);
    }

    #[tokio::test]
    async fn lifecycle_mutation_missing_result_is_unknown_outcome() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true, "errors": [], "result": null
            })))
            .mount(&server)
            .await;
        let error = api(&server).await.delete_zone(ZONE_ID).await.unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
    }

    #[tokio::test]
    async fn batch_posts_exact_body_and_decodes_complete_result() {
        let server = MockServer::start().await;
        let request = batch_request();
        Mock::given(method("POST"))
            .and(path(format!(
                "/client/v4/zones/{ZONE_ID}/dns_records/batch"
            )))
            .and(header("authorization", "Bearer super-secret-token"))
            .and(body_json(serde_json::to_value(&request).unwrap()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "result": {
                    "deletes": [], "patches": [], "puts": [],
                    "posts": [{
                        "id": "44444444444444444444444444444444",
                        "name": "txt.example.test", "type": "TXT", "ttl": 300,
                        "content": "\"hello\"", "proxied": false, "proxiable": false,
                        "settings": {}, "tags": [], "modified_on": "2026-07-17T00:00:00Z"
                    }]
                }
            })))
            .mount(&server)
            .await;
        let result = api(&server)
            .await
            .batch_records(ZONE_ID, &request)
            .await
            .unwrap();
        assert_eq!(result.deletes.len(), 0);
        assert_eq!(result.posts.len(), 1);
    }

    #[tokio::test]
    async fn mutation_server_error_and_malformed_success_are_unknown_outcomes() {
        for response in [
            ResponseTemplate::new(503).set_body_string("unavailable"),
            ResponseTemplate::new(200).set_body_string("truncated-json"),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true, "errors": [], "result": null
            })),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(response)
                .mount(&server)
                .await;
            let error = api(&server)
                .await
                .batch_records(ZONE_ID, &batch_request())
                .await
                .unwrap_err();
            assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
            assert_eq!(error.retry_after_ms(), None);
        }
    }

    #[tokio::test]
    async fn explicit_batch_rejection_is_not_an_unknown_outcome() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": false,
                "errors": [{ "code": 1004, "message": "DNS validation failed" }],
                "result": null
            })))
            .mount(&server)
            .await;
        let error = api(&server)
            .await
            .batch_records(ZONE_ID, &batch_request())
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Validation);
    }

    fn batch_request() -> CloudflareBatchRequest {
        CloudflareBatchRequest {
            deletes: Vec::new(),
            patches: Vec::new(),
            puts: Vec::new(),
            posts: vec![crate::CloudflareBatchRecord {
                kind: "TXT".to_string(),
                name: "txt.example.test".to_string(),
                ttl: 300,
                content: Some("\"hello\"".to_string()),
                data: None,
                priority: None,
                proxied: None,
                settings: None,
                comment: None,
                tags: BTreeSet::new(),
            }],
        }
    }

    #[test]
    fn token_and_endpoint_validation_fail_closed() {
        assert!(CloudflareApiToken::new(" token ").is_err());
        let token = CloudflareApiToken::new("secret").unwrap();
        assert!(CloudflareHttpApi::with_base_url(token, "http://example.com/client/v4/").is_err());
    }

    #[test]
    fn parses_cloudflare_txt_character_strings_and_escapes() {
        let parsed = txt_value(r#""hello" "world" "a\032b" "quote:\" slash:\\" """#).unwrap();
        let segments = parsed.segments();
        assert_eq!(segments.len(), 5);
        assert_eq!(segments[0].as_bytes(), b"hello");
        assert_eq!(segments[1].as_bytes(), b"world");
        assert_eq!(segments[2].as_bytes(), b"a b");
        assert_eq!(segments[3].as_bytes(), b"quote:\" slash:\\");
        assert_eq!(segments[4].as_bytes(), b"");
    }

    #[test]
    fn malformed_cloudflare_txt_content_fails_closed() {
        for invalid in [
            "unquoted",
            r#""unterminated"#,
            r#""bad"trailing"#,
            r#""bad\999""#,
            "",
        ] {
            assert!(txt_value(invalid).is_err(), "accepted {invalid:?}");
        }
        let too_long = format!("\"{}\"", "x".repeat(256));
        assert!(txt_value(&too_long).is_err());
    }
}
