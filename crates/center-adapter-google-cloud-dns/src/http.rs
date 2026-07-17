//! Credential-owning Cloud DNS v1 REST transport.

use std::{fmt, net::IpAddr, time::Duration};

use async_trait::async_trait;
use edgion_center_core::{NormalizedProviderError, ProviderErrorCategory};
use google_cloud_auth::credentials::{CacheableResource, Credentials};
use http::Extensions;
use reqwest::{header, Client, Response, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use tokio::time::sleep;
use zeroize::Zeroizing;

use crate::{
    unknown_outcome, GoogleChange, GoogleChangeRequest, GoogleCloudDnsApi, GoogleCloudDnsApiResult,
    GoogleDnsKey, GoogleDnsKeyDigest, GoogleDnsSecState, GoogleManagedZone,
    GoogleManagedZoneCreate, GoogleManagedZonePage, GoogleRecordSetPage, GoogleResourceRecordSet,
    GoogleZoneKind, GoogleZoneVisibility,
};

const DEFAULT_ENDPOINT: &str = "https://dns.googleapis.com/dns/v1/";
const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const READ_ATTEMPTS: usize = 3;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;

enum AuthSource {
    Adc(Credentials),
    TestToken(Zeroizing<String>),
}

/// Cloud DNS v1 transport using Application Default Credentials. ADC supports
/// attached service accounts, Workload Identity Federation, and service-account files.
pub struct GoogleCloudDnsHttpApi {
    project_id: String,
    endpoint: Url,
    client: Client,
    auth: AuthSource,
}

impl fmt::Debug for GoogleCloudDnsHttpApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleCloudDnsHttpApi")
            .field("project_id", &self.project_id)
            .field("endpoint", &self.endpoint)
            .field("credentials", &"[REDACTED]")
            .finish()
    }
}

impl GoogleCloudDnsHttpApi {
    pub fn ambient(project_id: impl Into<String>) -> GoogleCloudDnsApiResult<Self> {
        let credentials = google_cloud_auth::credentials::Builder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build()
            .map_err(|_| validation("google_adc_build_failed"))?;
        Self::with_credentials(project_id, credentials)
    }

    pub fn with_credentials(
        project_id: impl Into<String>,
        credentials: Credentials,
    ) -> GoogleCloudDnsApiResult<Self> {
        Self::build(
            project_id.into(),
            DEFAULT_ENDPOINT,
            AuthSource::Adc(credentials),
        )
    }

    /// Loopback-only seam for hermetic wire tests. It cannot redirect production
    /// credentials and is intentionally not configurable through environment variables.
    #[doc(hidden)]
    pub fn with_loopback_test_token(
        project_id: impl Into<String>,
        endpoint: impl AsRef<str>,
        token: impl Into<String>,
    ) -> GoogleCloudDnsApiResult<Self> {
        let token = token.into();
        if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
            return Err(validation("invalid_google_test_token"));
        }
        Self::build(
            project_id.into(),
            endpoint.as_ref(),
            AuthSource::TestToken(Zeroizing::new(token)),
        )
    }

    fn build(
        project_id: String,
        endpoint: &str,
        auth: AuthSource,
    ) -> GoogleCloudDnsApiResult<Self> {
        validate_project_id(&project_id)?;
        let mut endpoint =
            Url::parse(endpoint).map_err(|_| validation("invalid_google_dns_endpoint"))?;
        if !endpoint.path().ends_with('/') {
            endpoint.set_path(&format!("{}/", endpoint.path()));
        }
        let is_default = endpoint.as_str() == DEFAULT_ENDPOINT;
        if endpoint.cannot_be_a_base()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.username() != ""
            || endpoint.password().is_some()
            || (!is_default && !is_loopback(&endpoint))
            || (is_default && endpoint.scheme() != "https")
        {
            return Err(validation("invalid_google_dns_endpoint"));
        }
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| validation("google_http_client_build_failed"))?;
        Ok(Self {
            project_id,
            endpoint,
            client,
            auth,
        })
    }

    fn url(&self, segments: &[&str]) -> GoogleCloudDnsApiResult<Url> {
        let mut url = self.endpoint.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| validation("invalid_google_request_path"))?;
            path.pop_if_empty();
            for segment in segments {
                if segment.is_empty() || segment.chars().any(char::is_control) {
                    return Err(validation("invalid_google_request_path"));
                }
                path.push(segment);
            }
        }
        Ok(url)
    }

    async fn authorize(
        &self,
        request: reqwest::RequestBuilder,
    ) -> GoogleCloudDnsApiResult<reqwest::RequestBuilder> {
        match &self.auth {
            AuthSource::Adc(credentials) => {
                let headers = credentials
                    .headers(Extensions::new())
                    .await
                    .map_err(|_| authentication("google_adc_token_failed"))?;
                let CacheableResource::New { data, .. } = headers else {
                    return Err(authentication("google_adc_headers_unavailable"));
                };
                Ok(request.headers(data))
            }
            AuthSource::TestToken(token) => Ok(request.bearer_auth(token.as_str())),
        }
    }

    async fn read<T: DeserializeOwned>(
        &self,
        url: Url,
        query: &[(&str, String)],
    ) -> GoogleCloudDnsApiResult<T> {
        let mut last = None;
        for attempt in 0..READ_ATTEMPTS {
            let request = self
                .authorize(self.client.get(url.clone()).query(query))
                .await?;
            match request.send().await {
                Ok(response) => match decode::<T>(response, RequestKind::Read).await {
                    Ok(value) => return Ok(value),
                    Err(error) if retryable(&error) && attempt + 1 < READ_ATTEMPTS => {
                        last = Some(error)
                    }
                    Err(error) => return Err(error),
                },
                Err(error) if attempt + 1 < READ_ATTEMPTS && !error.is_builder() => {
                    last = Some(transient("google_read_transport_failed", None));
                }
                Err(error) => return Err(map_transport(error, RequestKind::Read)),
            }
            sleep(Duration::from_millis(50 * (attempt as u64 + 1))).await;
        }
        Err(last.unwrap_or_else(|| transient("google_read_retry_exhausted", None)))
    }

    async fn mutation<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        url: Url,
        body: &B,
    ) -> GoogleCloudDnsApiResult<T> {
        let encoded =
            serde_json::to_vec(body).map_err(|_| validation("google_change_encoding_failed"))?;
        if encoded.len() > MAX_REQUEST_BYTES {
            return Err(validation("google_change_request_too_large"));
        }
        let request = self
            .authorize(
                self.client
                    .post(url)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(encoded),
            )
            .await?;
        let response = request
            .send()
            .await
            .map_err(|error| map_transport(error, RequestKind::Mutation))?;
        decode(response, RequestKind::Mutation).await
    }
}

#[async_trait]
impl GoogleCloudDnsApi for GoogleCloudDnsHttpApi {
    fn verified_project_id(&self) -> &str {
        &self.project_id
    }

    async fn get_managed_zone(
        &self,
        zone: &str,
    ) -> GoogleCloudDnsApiResult<Option<GoogleManagedZone>> {
        let url = self.url(&["projects", &self.project_id, "managedZones", zone])?;
        match self.read::<RawManagedZone>(url, &[]).await {
            Ok(value) => Ok(Some(map_zone(value)?)),
            Err(error) if error.category() == ProviderErrorCategory::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn list_managed_zones(
        &self,
        page_token: Option<&str>,
        max_results: u16,
    ) -> GoogleCloudDnsApiResult<GoogleManagedZonePage> {
        let url = self.url(&["projects", &self.project_id, "managedZones"])?;
        let mut query = vec![("maxResults", max_results.to_string())];
        if let Some(token) = page_token {
            query.push(("pageToken", token.to_string()));
        }
        let response: RawZonePage = self.read(url, &query).await?;
        Ok(GoogleManagedZonePage {
            items: response
                .managed_zones
                .into_iter()
                .map(map_zone)
                .collect::<GoogleCloudDnsApiResult<_>>()?,
            next_page_token: response.next_page_token,
        })
    }

    async fn list_record_sets(
        &self,
        zone: &str,
        page_token: Option<&str>,
        max_results: u16,
    ) -> GoogleCloudDnsApiResult<GoogleRecordSetPage> {
        let url = self.url(&["projects", &self.project_id, "managedZones", zone, "rrsets"])?;
        let mut query = vec![("maxResults", max_results.to_string())];
        if let Some(token) = page_token {
            query.push(("pageToken", token.to_string()));
        }
        let response: RawRecordPage = self.read(url, &query).await?;
        Ok(GoogleRecordSetPage {
            items: response.rrsets,
            next_page_token: response.next_page_token,
        })
    }

    async fn create_change(
        &self,
        zone: &str,
        request: &GoogleChangeRequest,
    ) -> GoogleCloudDnsApiResult<GoogleChange> {
        let url = self.url(&[
            "projects",
            &self.project_id,
            "managedZones",
            zone,
            "changes",
        ])?;
        self.mutation(url, request).await
    }

    async fn get_change(
        &self,
        zone: &str,
        change_id: &str,
    ) -> GoogleCloudDnsApiResult<Option<GoogleChange>> {
        let url = self.url(&[
            "projects",
            &self.project_id,
            "managedZones",
            zone,
            "changes",
            change_id,
        ])?;
        match self.read(url, &[]).await {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.category() == ProviderErrorCategory::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn create_managed_zone(
        &self,
        request: &GoogleManagedZoneCreate,
    ) -> GoogleCloudDnsApiResult<GoogleManagedZone> {
        let url = self.url(&["projects", &self.project_id, "managedZones"])?;
        let body = serde_json::json!({
            "name": request.name,
            "dnsName": format!("{}.", request.dns_name.trim_end_matches('.')),
            "visibility": match request.visibility { GoogleZoneVisibility::Public => "public", GoogleZoneVisibility::Private => "private" },
            "description": "Managed by EdgionCenter",
            "dnssecConfig": { "state": match request.dnssec_state { GoogleDnsSecState::Off => "off", GoogleDnsSecState::On => "on", GoogleDnsSecState::Transfer => "transfer" } }
        });
        self.mutation::<RawManagedZone, _>(url, &body)
            .await
            .and_then(map_zone)
    }

    async fn delete_managed_zone(&self, zone: &str) -> GoogleCloudDnsApiResult<()> {
        let url = self.url(&["projects", &self.project_id, "managedZones", zone])?;
        let request = self.authorize(self.client.delete(url)).await?;
        let response = request
            .send()
            .await
            .map_err(|error| map_transport(error, RequestKind::Mutation))?;
        let status = response.status();
        if status.is_success() {
            // Consume the bounded body so connections remain reusable. Google
            // normally returns an empty response for managed-zone deletion.
            if response
                .content_length()
                .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
            {
                return Err(unknown_outcome("google_delete_response_too_large"));
            }
            response
                .bytes()
                .await
                .map_err(|_| unknown_outcome("google_delete_response_read_failed"))?;
            return Ok(());
        }
        decode::<Value>(response, RequestKind::Mutation)
            .await
            .map(|_| ())
    }

    async fn set_managed_zone_dnssec(
        &self,
        zone: &str,
        state: GoogleDnsSecState,
    ) -> GoogleCloudDnsApiResult<GoogleManagedZone> {
        let mut url = self.url(&["projects", &self.project_id, "managedZones", zone])?;
        url.query_pairs_mut()
            .append_pair("updateMask", "dnssecConfig.state");
        let body = serde_json::json!({ "dnssecConfig": { "state": match state { GoogleDnsSecState::Off => "off", GoogleDnsSecState::On => "on", GoogleDnsSecState::Transfer => "transfer" } } });
        let request = self.authorize(self.client.patch(url).json(&body)).await?;
        let response = request
            .send()
            .await
            .map_err(|error| map_transport(error, RequestKind::Mutation))?;
        decode::<RawManagedZone>(response, RequestKind::Mutation)
            .await
            .and_then(map_zone)
    }

    async fn list_dns_keys(&self, zone: &str) -> GoogleCloudDnsApiResult<Vec<GoogleDnsKey>> {
        let url = self.url(&[
            "projects",
            &self.project_id,
            "managedZones",
            zone,
            "dnsKeys",
        ])?;
        let raw: RawDnsKeyPage = self
            .read(url, &[("digestType", "sha256".to_string())])
            .await?;
        if raw.next_page_token.is_some() {
            return Err(validation("google_dns_keys_pagination_unsupported"));
        }
        raw.dns_keys.into_iter().map(map_dns_key).collect()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawZonePage {
    #[serde(default)]
    managed_zones: Vec<RawManagedZone>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRecordPage {
    #[serde(default)]
    rrsets: Vec<GoogleResourceRecordSet>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManagedZone {
    id: String,
    name: String,
    dns_name: String,
    visibility: String,
    dnssec_config: Option<RawDnsSec>,
    forwarding_config: Option<Value>,
    peering_config: Option<Value>,
    reverse_lookup_config: Option<Value>,
    service_directory_config: Option<Value>,
    #[serde(default)]
    name_servers: Vec<String>,
}

#[derive(Deserialize)]
struct RawDnsSec {
    state: Option<String>,
}

fn map_zone(value: RawManagedZone) -> GoogleCloudDnsApiResult<GoogleManagedZone> {
    let visibility = match value.visibility.as_str() {
        "public" => GoogleZoneVisibility::Public,
        "private" => GoogleZoneVisibility::Private,
        _ => return Err(validation("invalid_google_zone_visibility")),
    };
    let kinds = [
        value.forwarding_config.is_some(),
        value.peering_config.is_some(),
        value.reverse_lookup_config.is_some(),
        value.service_directory_config.is_some(),
    ];
    if kinds.iter().filter(|value| **value).count() > 1 {
        return Err(validation("invalid_google_zone_kind"));
    }
    let kind = if kinds[0] {
        GoogleZoneKind::Forwarding
    } else if kinds[1] {
        GoogleZoneKind::Peering
    } else if kinds[2] {
        GoogleZoneKind::ReverseLookup
    } else if kinds[3] {
        GoogleZoneKind::ServiceDirectory
    } else {
        GoogleZoneKind::Authoritative
    };
    let dnssec_state = match value
        .dnssec_config
        .and_then(|value| value.state)
        .as_deref()
        .unwrap_or("off")
    {
        "off" => GoogleDnsSecState::Off,
        "on" => GoogleDnsSecState::On,
        "transfer" => GoogleDnsSecState::Transfer,
        _ => return Err(validation("invalid_google_dnssec_state")),
    };
    Ok(GoogleManagedZone {
        id: value.id,
        name: value.name,
        dns_name: value.dns_name,
        visibility,
        kind,
        dnssec_state,
        name_servers: value.name_servers,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDnsKeyPage {
    #[serde(default)]
    dns_keys: Vec<RawDnsKey>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDnsKey {
    key_tag: u16,
    algorithm: String,
    #[serde(rename = "type")]
    key_type: String,
    #[serde(default)]
    digests: Vec<GoogleDnsKeyDigest>,
}

fn map_dns_key(value: RawDnsKey) -> GoogleCloudDnsApiResult<GoogleDnsKey> {
    let algorithm = match value.algorithm.as_str() {
        "rsasha256" => 8,
        "rsasha512" => 10,
        "ecdsap256sha256" => 13,
        "ecdsap384sha384" => 14,
        _ => return Err(validation("unknown_google_dns_key_algorithm")),
    };
    Ok(GoogleDnsKey {
        key_tag: value.key_tag,
        algorithm,
        key_type: value.key_type,
        digests: value.digests,
    })
}

#[derive(Clone, Copy)]
enum RequestKind {
    Read,
    Mutation,
}

async fn decode<T: DeserializeOwned>(
    response: Response,
    kind: RequestKind,
) -> GoogleCloudDnsApiResult<T> {
    let status = response.status();
    let request_id = request_id(response.headers());
    let retry_after = retry_after_ms(response.headers());
    if response
        .content_length()
        .is_some_and(|value| value > MAX_RESPONSE_BYTES as u64)
    {
        return Err(response_error(
            kind,
            "google_response_too_large",
            request_id,
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| response_error(kind, "google_response_read_failed", request_id.clone()))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(response_error(
            kind,
            "google_response_too_large",
            request_id,
        ));
    }
    if !status.is_success() {
        let envelope = serde_json::from_slice::<GoogleErrorEnvelope>(&bytes).ok();
        return Err(map_status(
            status,
            envelope.as_ref(),
            retry_after,
            request_id,
            kind,
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| response_error(kind, "google_response_invalid", request_id))
}

#[derive(Deserialize)]
struct GoogleErrorEnvelope {
    error: GoogleErrorBody,
}
#[derive(Deserialize)]
struct GoogleErrorBody {
    #[serde(default)]
    errors: Vec<GoogleErrorReason>,
}
#[derive(Deserialize)]
struct GoogleErrorReason {
    reason: String,
}

fn map_status(
    status: StatusCode,
    body: Option<&GoogleErrorEnvelope>,
    retry_after: Option<u64>,
    request_id: Option<String>,
    kind: RequestKind,
) -> NormalizedProviderError {
    let reason = body
        .and_then(|value| value.error.errors.first())
        .map(|value| value.reason.as_str());
    let (category, code, delay) =
        if matches!(reason, Some("quotaExceeded")) || status == StatusCode::PAYLOAD_TOO_LARGE {
            (
                ProviderErrorCategory::Quota,
                "google_dns_quota_exceeded",
                retry_after,
            )
        } else if matches!(reason, Some("rateLimitExceeded" | "userRateLimitExceeded"))
            || status == StatusCode::TOO_MANY_REQUESTS
        {
            (
                ProviderErrorCategory::Throttled,
                "google_dns_throttled",
                Some(retry_after.unwrap_or(1_000)),
            )
        } else if status == StatusCode::UNAUTHORIZED {
            (
                ProviderErrorCategory::Authentication,
                "google_dns_unauthenticated",
                None,
            )
        } else if status == StatusCode::FORBIDDEN {
            (
                ProviderErrorCategory::Authorization,
                "google_dns_forbidden",
                None,
            )
        } else if status == StatusCode::NOT_FOUND || reason == Some("notFound") {
            (
                ProviderErrorCategory::NotFound,
                "google_dns_not_found",
                None,
            )
        } else if matches!(
            status,
            StatusCode::CONFLICT | StatusCode::PRECONDITION_FAILED
        ) || matches!(reason, Some("preconditionFailed" | "alreadyExists"))
        {
            (ProviderErrorCategory::Conflict, "google_dns_conflict", None)
        } else if matches!(
            reason,
            Some(
                "required"
                    | "invalidValue"
                    | "invalidFieldValue"
                    | "invalidZoneApex"
                    | "invalidRecordCount"
                    | "cnameResourceRecordSetConflict"
                    | "wildcardNotAllowed"
                    | "recordTypeDisallowedAtZoneApex"
            )
        ) || status == StatusCode::BAD_REQUEST
        {
            (
                ProviderErrorCategory::Validation,
                "google_dns_invalid_request",
                None,
            )
        } else if matches!(kind, RequestKind::Mutation)
            && (status == StatusCode::REQUEST_TIMEOUT || status.is_server_error())
        {
            (
                ProviderErrorCategory::UnknownOutcome,
                "google_dns_mutation_unknown",
                None,
            )
        } else {
            (
                ProviderErrorCategory::Transient,
                "google_dns_transient",
                retry_after,
            )
        };
    error(category, code, delay, request_id)
}

fn map_transport(value: reqwest::Error, kind: RequestKind) -> NormalizedProviderError {
    if value.is_builder() {
        return validation("google_http_request_invalid");
    }
    match kind {
        RequestKind::Read => transient("google_read_transport_failed", None),
        RequestKind::Mutation => error(
            ProviderErrorCategory::UnknownOutcome,
            "google_mutation_transport_unknown",
            None,
            None,
        ),
    }
}

fn response_error(
    kind: RequestKind,
    code: &str,
    request_id: Option<String>,
) -> NormalizedProviderError {
    match kind {
        RequestKind::Read => transient(code, request_id),
        RequestKind::Mutation => error(
            ProviderErrorCategory::UnknownOutcome,
            code,
            None,
            request_id,
        ),
    }
}

fn request_id(headers: &header::HeaderMap) -> Option<String> {
    ["x-goog-request-id", "x-guploader-uploadid"]
        .iter()
        .find_map(|name| headers.get(*name)?.to_str().ok())
        .filter(|value| {
            !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
        })
        .map(str::to_string)
}
fn retry_after_ms(headers: &header::HeaderMap) -> Option<u64> {
    headers
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()?
        .checked_mul(1_000)
}
fn retryable(error: &NormalizedProviderError) -> bool {
    matches!(
        error.category(),
        ProviderErrorCategory::Transient | ProviderErrorCategory::Throttled
    )
}
fn is_loopback(url: &Url) -> bool {
    match url.host_str() {
        Some("localhost") => true,
        Some(host) => host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()),
        None => false,
    }
}
fn validate_project_id(value: &str) -> GoogleCloudDnsApiResult<()> {
    if (6..=30).contains(&value.len())
        && value.starts_with(|c: char| c.is_ascii_lowercase())
        && value.ends_with(|c: char| c.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        Ok(())
    } else {
        Err(validation("invalid_google_project_id"))
    }
}
fn validation(code: &str) -> NormalizedProviderError {
    error(ProviderErrorCategory::Validation, code, None, None)
}
fn authentication(code: &str) -> NormalizedProviderError {
    error(ProviderErrorCategory::Authentication, code, None, None)
}
fn transient(code: &str, request_id: Option<String>) -> NormalizedProviderError {
    error(ProviderErrorCategory::Transient, code, None, request_id)
}
fn error(
    category: ProviderErrorCategory,
    code: &str,
    retry_after: Option<u64>,
    request_id: Option<String>,
) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        code,
        "Google Cloud DNS API request failed",
        retry_after,
        request_id,
    )
    .expect("static Google provider error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GoogleCloudDnsApi;
    use serde_json::json;
    use wiremock::{
        matchers::{body_json, header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    const PROJECT: &str = "edgion-dns-test";

    async fn api(server: &MockServer) -> GoogleCloudDnsHttpApi {
        GoogleCloudDnsHttpApi::with_loopback_test_token(
            PROJECT,
            format!("{}/dns/v1/", server.uri()),
            "test-secret-token",
        )
        .unwrap()
    }

    #[tokio::test]
    async fn lists_zones_with_project_scope_adc_header_and_nested_zone_metadata() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/dns/v1/projects/{PROJECT}/managedZones")))
            .and(query_param("maxResults", "25"))
            .and(header("authorization", "Bearer test-secret-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "managedZones": [{
                    "id": "1234567890123456789",
                    "name": "private-zone",
                    "dnsName": "private.example.test.",
                    "visibility": "private",
                    "dnssecConfig": {"state": "off"},
                    "forwardingConfig": {"targetNameServers": []},
                    "kind": "dns#managedZone"
                }],
                "nextPageToken": "next-zone-page"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let page = api(&server)
            .await
            .list_managed_zones(None, 25)
            .await
            .unwrap();
        assert_eq!(page.next_page_token.as_deref(), Some("next-zone-page"));
        assert_eq!(page.items[0].kind, GoogleZoneKind::Forwarding);
        assert_eq!(page.items[0].visibility, GoogleZoneVisibility::Private);
    }

    #[tokio::test]
    async fn mutation_uses_official_routing_union_shape_and_never_retries() {
        let server = MockServer::start().await;
        let rrset = GoogleResourceRecordSet {
            name: "www.example.test.".into(),
            record_type: "A".into(),
            ttl: 30,
            rrdatas: Vec::new(),
            signature_rrdatas: Vec::new(),
            routing_policy: Some(crate::GoogleRoutingPolicy {
                health_check: None,
                routing_data: crate::GoogleRoutingData::Wrr {
                    wrr: crate::GoogleWrrPolicy {
                        items: vec![crate::GoogleWrrPolicyItem {
                            weight: "1.0".parse().unwrap(),
                            rrdatas: vec!["192.0.2.1".into()],
                            signature_rrdatas: Vec::new(),
                            health_checked_targets: None,
                            extra: Default::default(),
                        }],
                        extra: Default::default(),
                    },
                },
                extra: Default::default(),
            }),
            extra: Default::default(),
        };
        let request = GoogleChangeRequest {
            additions: vec![rrset],
            deletions: Vec::new(),
        };
        let expected = json!({
            "additions": [{
                "name": "www.example.test.", "type": "A", "ttl": 30,
                "rrdatas": [], "signatureRrdatas": [],
                "routingPolicy": {"wrr": {"items": [{
                    "weight": 1.0, "rrdatas": ["192.0.2.1"], "signatureRrdatas": []
                }]}}
            }],
            "deletions": []
        });
        Mock::given(method("POST"))
            .and(path(format!(
                "/dns/v1/projects/{PROJECT}/managedZones/123/changes"
            )))
            .and(body_json(expected))
            .respond_with(
                ResponseTemplate::new(503).set_body_json(json!({"error": {"errors": []}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = api(&server)
            .await
            .create_change("123", &request)
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
    }

    #[tokio::test]
    async fn reads_retry_transient_responses_with_a_bound() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/dns/v1/projects/{PROJECT}/managedZones/123")))
            .respond_with(
                ResponseTemplate::new(503).set_body_json(json!({"error": {"errors": []}})),
            )
            .expect(READ_ATTEMPTS as u64)
            .mount(&server)
            .await;

        let error = api(&server)
            .await
            .get_managed_zone("123")
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Transient);
    }

    #[test]
    fn endpoint_and_project_validation_fail_closed() {
        assert!(GoogleCloudDnsHttpApi::with_loopback_test_token(
            "bad",
            "https://evil.example/dns/v1/",
            "secret"
        )
        .is_err());
        assert!(GoogleCloudDnsHttpApi::with_loopback_test_token(
            PROJECT,
            "http://evil.example/dns/v1/",
            "secret"
        )
        .is_err());
    }
}
