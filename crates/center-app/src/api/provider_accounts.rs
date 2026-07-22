//! Provider-neutral, secret-free ProviderAccount Admin API.

use std::collections::BTreeMap;

use axum::{
    extract::{rejection::JsonRejection, rejection::QueryRejection, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    CloudProvider, CloudResourceId, CoreError, CredentialRef, CredentialSource, DeletionPolicy,
    ManagementPolicy, ProviderAccount, ProviderAccountCreateResult, ProviderAccountDesired,
    ProviderAccountPageRequest, ProviderAccountReplaceResult, ProviderAccountScope,
    ProviderAccountSpec,
};
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::{ApiResponse, ListResponse};

const DEFAULT_PAGE_SIZE: u16 = 50;
const CURSOR_PREFIX: &str = "v1.";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListProviderAccountsQuery {
    #[serde(default = "default_page_size")]
    pub limit: u16,
    pub cursor: Option<String>,
}

fn default_page_size() -> u16 {
    DEFAULT_PAGE_SIZE
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateProviderAccountRequest {
    pub account_id: String,
    pub desired: ProviderAccountDesiredDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceProviderAccountRequest {
    pub desired: ProviderAccountDesiredDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAccountDesiredDto {
    pub display_name: String,
    pub owner: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub management_policy: ManagementPolicyDto,
    pub provider: CloudProviderDto,
    pub scope: ProviderAccountScopeDto,
    pub credential_source: CredentialSourceDto,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementPolicyDto {
    Managed,
    #[default]
    ObserveOnly,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionPolicyDto {
    #[default]
    Retain,
    DeleteExternal,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudProviderDto {
    Cloudflare,
    Aws,
    GoogleCloud,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    tag = "provider",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProviderAccountScopeDto {
    Cloudflare { account_id: String },
    Aws { account_id: String },
    GoogleCloud { project_id: String },
}

/// Credential selection contains aliases and identity metadata only. There is
/// intentionally no variant capable of carrying a token, key, or secret value.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CredentialSourceDto {
    StaticSecret {
        credential_ref: String,
    },
    Ambient,
    Federated {
        subject_token_ref: Option<String>,
        target_principal: String,
        audience: Option<String>,
    },
    AssumeIdentity {
        base_credential_ref: Option<String>,
        target_principal: String,
        external_id_ref: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccountDto {
    pub account_id: String,
    pub display_name: String,
    pub owner: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub generation: u64,
    pub management_policy: ManagementPolicyDto,
    pub deletion_policy: DeletionPolicyDto,
    pub provider: CloudProviderDto,
    pub scope: Option<ProviderAccountScopeDto>,
    pub credential_source: CredentialSourceDto,
}

impl ProviderAccountDesiredDto {
    fn into_core(self) -> Result<ProviderAccountDesired, CoreError> {
        let desired = ProviderAccountDesired {
            display_name: self.display_name,
            owner: self.owner,
            labels: self.labels,
            management_policy: self.management_policy.into(),
            deletion_policy: DeletionPolicy::Retain,
            spec: ProviderAccountSpec {
                provider: self.provider.into(),
                scope: Some(self.scope.into()),
                credential_source: self.credential_source.try_into()?,
            },
        };
        desired.validate()?;
        Ok(desired)
    }
}

impl From<ManagementPolicyDto> for ManagementPolicy {
    fn from(value: ManagementPolicyDto) -> Self {
        match value {
            ManagementPolicyDto::Managed => Self::Managed,
            ManagementPolicyDto::ObserveOnly => Self::ObserveOnly,
        }
    }
}

impl From<ManagementPolicy> for ManagementPolicyDto {
    fn from(value: ManagementPolicy) -> Self {
        match value {
            ManagementPolicy::Managed => Self::Managed,
            ManagementPolicy::ObserveOnly => Self::ObserveOnly,
        }
    }
}

impl From<DeletionPolicyDto> for DeletionPolicy {
    fn from(value: DeletionPolicyDto) -> Self {
        match value {
            DeletionPolicyDto::Retain => Self::Retain,
            DeletionPolicyDto::DeleteExternal => Self::DeleteExternal,
        }
    }
}

impl From<DeletionPolicy> for DeletionPolicyDto {
    fn from(value: DeletionPolicy) -> Self {
        match value {
            DeletionPolicy::Retain => Self::Retain,
            DeletionPolicy::DeleteExternal => Self::DeleteExternal,
        }
    }
}

impl From<CloudProviderDto> for CloudProvider {
    fn from(value: CloudProviderDto) -> Self {
        match value {
            CloudProviderDto::Cloudflare => Self::Cloudflare,
            CloudProviderDto::Aws => Self::Aws,
            CloudProviderDto::GoogleCloud => Self::GoogleCloud,
        }
    }
}

impl From<CloudProvider> for CloudProviderDto {
    fn from(value: CloudProvider) -> Self {
        match value {
            CloudProvider::Cloudflare => Self::Cloudflare,
            CloudProvider::Aws => Self::Aws,
            CloudProvider::GoogleCloud => Self::GoogleCloud,
        }
    }
}

impl From<ProviderAccountScopeDto> for ProviderAccountScope {
    fn from(value: ProviderAccountScopeDto) -> Self {
        match value {
            ProviderAccountScopeDto::Cloudflare { account_id } => Self::Cloudflare { account_id },
            ProviderAccountScopeDto::Aws { account_id } => Self::Aws { account_id },
            ProviderAccountScopeDto::GoogleCloud { project_id } => Self::GoogleCloud { project_id },
        }
    }
}

impl From<ProviderAccountScope> for ProviderAccountScopeDto {
    fn from(value: ProviderAccountScope) -> Self {
        match value {
            ProviderAccountScope::Cloudflare { account_id } => Self::Cloudflare { account_id },
            ProviderAccountScope::Aws { account_id } => Self::Aws { account_id },
            ProviderAccountScope::GoogleCloud { project_id } => Self::GoogleCloud { project_id },
        }
    }
}

impl TryFrom<CredentialSourceDto> for CredentialSource {
    type Error = CoreError;

    fn try_from(value: CredentialSourceDto) -> Result<Self, Self::Error> {
        let credential_ref = |value: String| CredentialRef::new(value);
        Ok(match value {
            CredentialSourceDto::StaticSecret {
                credential_ref: value,
            } => Self::StaticSecret {
                credential_ref: credential_ref(value)?,
            },
            CredentialSourceDto::Ambient => Self::Ambient,
            CredentialSourceDto::Federated {
                subject_token_ref,
                target_principal,
                audience,
            } => Self::Federated {
                subject_token_ref: subject_token_ref.map(credential_ref).transpose()?,
                target_principal,
                audience,
            },
            CredentialSourceDto::AssumeIdentity {
                base_credential_ref,
                target_principal,
                external_id_ref,
            } => Self::AssumeIdentity {
                base_credential_ref: base_credential_ref.map(credential_ref).transpose()?,
                target_principal,
                external_id_ref: external_id_ref.map(credential_ref).transpose()?,
            },
        })
    }
}

impl From<CredentialSource> for CredentialSourceDto {
    fn from(value: CredentialSource) -> Self {
        match value {
            CredentialSource::StaticSecret { credential_ref } => Self::StaticSecret {
                credential_ref: credential_ref.to_string(),
            },
            CredentialSource::Ambient => Self::Ambient,
            CredentialSource::Federated {
                subject_token_ref,
                target_principal,
                audience,
            } => Self::Federated {
                subject_token_ref: subject_token_ref.map(|value| value.to_string()),
                target_principal,
                audience,
            },
            CredentialSource::AssumeIdentity {
                base_credential_ref,
                target_principal,
                external_id_ref,
            } => Self::AssumeIdentity {
                base_credential_ref: base_credential_ref.map(|value| value.to_string()),
                target_principal,
                external_id_ref: external_id_ref.map(|value| value.to_string()),
            },
        }
    }
}

impl From<ProviderAccount> for ProviderAccountDto {
    fn from(value: ProviderAccount) -> Self {
        Self {
            account_id: value.metadata.id.to_string(),
            display_name: value.metadata.display_name,
            owner: value.metadata.owner,
            labels: value.metadata.labels,
            generation: value.metadata.generation,
            management_policy: value.metadata.management_policy.into(),
            deletion_policy: value.metadata.deletion_policy.into(),
            provider: value.spec.provider.into(),
            scope: value.spec.scope.map(Into::into),
            credential_source: value.spec.credential_source.into(),
        }
    }
}

fn encode_cursor(id: &CloudResourceId) -> String {
    format!("{CURSOR_PREFIX}{}", URL_SAFE_NO_PAD.encode(id.as_str()))
}

fn decode_cursor(value: &str) -> Result<CloudResourceId, CoreError> {
    if value.len() > 180 {
        return Err(CoreError::Conflict(
            "provider account pagination cursor is invalid".into(),
        ));
    }
    let encoded = value.strip_prefix(CURSOR_PREFIX).ok_or_else(|| {
        CoreError::Conflict("provider account pagination cursor is invalid".into())
    })?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| CoreError::Conflict("provider account pagination cursor is invalid".into()))?;
    let decoded = String::from_utf8(bytes)
        .map_err(|_| CoreError::Conflict("provider account pagination cursor is invalid".into()))?;
    let id = CloudResourceId::new(decoded)?;
    if encode_cursor(&id) != value {
        return Err(CoreError::Conflict(
            "provider account pagination cursor is invalid".into(),
        ));
    }
    Ok(id)
}

fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(ApiResponse::<()>::err_body(code.to_string()))).into_response()
}

fn invalid_request() -> Response {
    error(StatusCode::BAD_REQUEST, "invalid_request")
}

fn json_rejection(rejection: JsonRejection) -> Response {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        error(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large")
    } else {
        invalid_request()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IfMatchError {
    Missing,
    Invalid,
}

fn parse_if_match(headers: &HeaderMap) -> Result<u64, IfMatchError> {
    let values = headers.get_all(header::IF_MATCH);
    let mut values = values.iter();
    let Some(value) = values.next() else {
        return Err(IfMatchError::Missing);
    };
    if values.next().is_some() {
        return Err(IfMatchError::Invalid);
    }
    let raw = value.to_str().map_err(|_| IfMatchError::Invalid)?;
    let value = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or(IfMatchError::Invalid)?;
    if raw != format!("\"{value}\"") {
        return Err(IfMatchError::Invalid);
    }
    Ok(value)
}

fn account_response(status: StatusCode, account: ProviderAccount) -> Response {
    let generation = account.metadata.generation;
    let mut response = (
        status,
        Json(ApiResponse::ok_body(ProviderAccountDto::from(account))),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&format!("\"{generation}\"")) {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}

fn created_account_response(account: ProviderAccount) -> Response {
    let location = format!(
        "/api/v1/center/cloud/provider-accounts/{}",
        account.metadata.id
    );
    let mut response = account_response(StatusCode::CREATED, account);
    if let Ok(value) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    response
}

pub(crate) fn parse_account_id(value: String) -> Result<CloudResourceId, ()> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        });
    if !valid {
        return Err(());
    }
    CloudResourceId::new(value).map_err(|_| ())
}

fn store_failure() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "provider_account_store_unavailable",
    )
}

pub async fn create(
    State(state): State<ApiState>,
    request: Result<Json<CreateProviderAccountRequest>, JsonRejection>,
) -> Response {
    let Some(store) = state.provider_account_store.as_ref() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_account_store_unavailable",
        );
    };
    let request = match request {
        Ok(Json(value)) => value,
        Err(rejection) => return json_rejection(rejection),
    };
    let account_id = match parse_account_id(request.account_id) {
        Ok(value) => value,
        Err(_) => return invalid_request(),
    };
    let desired = match request.desired.into_core() {
        Ok(value) => value,
        Err(_) => return invalid_request(),
    };
    match store.create(&account_id, &desired).await {
        Ok(ProviderAccountCreateResult::Created(account)) => created_account_response(*account),
        Ok(ProviderAccountCreateResult::AlreadyExists) => {
            error(StatusCode::CONFLICT, "provider_account_already_exists")
        }
        Err(_) => store_failure(),
    }
}

pub async fn list(
    State(state): State<ApiState>,
    query: Result<Query<ListProviderAccountsQuery>, QueryRejection>,
) -> Response {
    let Some(store) = state.provider_account_store.as_ref() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_account_store_unavailable",
        );
    };
    let query = match query {
        Ok(Query(value)) => value,
        Err(_) => return invalid_request(),
    };
    let after = match query.cursor.as_deref().map(decode_cursor).transpose() {
        Ok(value) => value,
        Err(_) => return invalid_request(),
    };
    let request = ProviderAccountPageRequest {
        limit: query.limit,
        after,
    };
    if request.validate().is_err() {
        return invalid_request();
    }
    match store.list(&request).await {
        Ok(page) => {
            if page.validate(&request).is_err() {
                return store_failure();
            }
            let cursor = page.next.as_ref().map(encode_cursor);
            let items = page
                .items
                .into_iter()
                .map(ProviderAccountDto::from)
                .collect();
            Json(ListResponse::success_with_token(items, cursor)).into_response()
        }
        Err(_) => store_failure(),
    }
}

pub async fn get(State(state): State<ApiState>, Path(account_id): Path<String>) -> Response {
    let Some(store) = state.provider_account_store.as_ref() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_account_store_unavailable",
        );
    };
    let account_id = match parse_account_id(account_id) {
        Ok(value) => value,
        Err(_) => return invalid_request(),
    };
    match store.get(&account_id).await {
        Ok(Some(account)) => account_response(StatusCode::OK, account),
        Ok(None) => error(StatusCode::NOT_FOUND, "provider_account_not_found"),
        Err(_) => store_failure(),
    }
}

pub async fn replace(
    State(state): State<ApiState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
    request: Result<Json<ReplaceProviderAccountRequest>, JsonRejection>,
) -> Response {
    let Some(store) = state.provider_account_store.as_ref() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_account_store_unavailable",
        );
    };
    let request = match request {
        Ok(Json(value)) => value,
        Err(rejection) => return json_rejection(rejection),
    };
    let account_id = match parse_account_id(account_id) {
        Ok(value) => value,
        Err(_) => return invalid_request(),
    };
    let expected_generation = match parse_if_match(&headers) {
        Ok(value) => value,
        Err(IfMatchError::Missing) => {
            return error(StatusCode::PRECONDITION_REQUIRED, "if_match_required")
        }
        Err(IfMatchError::Invalid) => return invalid_request(),
    };
    let desired = match request.desired.into_core() {
        Ok(value) => value,
        Err(_) => return invalid_request(),
    };
    // Provider identity and its native account boundary remain immutable until
    // a reference index can prove that changing them is safe.
    match store.get(&account_id).await {
        Ok(Some(existing)) if existing.metadata.generation != expected_generation => {
            return error(
                StatusCode::PRECONDITION_FAILED,
                "provider_account_generation_mismatch",
            )
        }
        Ok(Some(existing))
            if existing.spec.provider == desired.spec.provider
                && existing.spec.scope == desired.spec.scope => {}
        Ok(Some(_)) => return error(StatusCode::CONFLICT, "provider_account_scope_immutable"),
        Ok(None) => return error(StatusCode::NOT_FOUND, "provider_account_not_found"),
        Err(_) => return store_failure(),
    }
    match store
        .replace_if_generation(&account_id, expected_generation, &desired)
        .await
    {
        Ok(ProviderAccountReplaceResult::Stored(account)) => {
            account_response(StatusCode::OK, *account)
        }
        Ok(ProviderAccountReplaceResult::NotFound) => {
            error(StatusCode::NOT_FOUND, "provider_account_not_found")
        }
        Ok(ProviderAccountReplaceResult::GenerationMismatch { .. }) => error(
            StatusCode::PRECONDITION_FAILED,
            "provider_account_generation_mismatch",
        ),
        Err(CoreError::Conflict(_)) => invalid_request(),
        Err(_) => store_failure(),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use parking_lot::Mutex;
    use serde_json::{json, Value};
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

    async fn state(with_store: bool) -> ApiState {
        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let pending_commands = Arc::new(Mutex::new(HashMap::new()));
        let pending_proxies = Arc::new(Mutex::new(HashMap::new()));
        let store = if with_store {
            Some(Arc::new(
                edgion_center_adapter_sql::Store::open_in_memory()
                    .await
                    .unwrap(),
            )
                as Arc<dyn edgion_center_core::ProviderAccountStore>)
        } else {
            None
        };
        let mut capabilities = edgion_center_core::CenterCapabilities::for_mode(
            edgion_center_core::CenterMode::Standalone,
        );
        capabilities.provider_account_admin = true;
        ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            commander: Arc::new(Commander::new(registry.clone(), pending_commands, 5)),
            proxy: Arc::new(ProxyForwarder::new(registry.clone(), pending_proxies, 5)),
            controller_directory: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: None,
            role_admin: None,
            audit_reader: None,
            cloudflare_dns_admin: None,
            cloudflare_dns_write_admin: None,
            route53_dns_admin: None,
            provider_account_store: store,
            capability_snapshot_store: None,
            credential_inspection_service: None,
            metadata_store,
            sync_client,
            registry,
            platform_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            authz_mode: edgion_center_core::AuthzMode::AllowAll,
            platform_mode: edgion_center_core::CenterMode::Standalone,
            capabilities,
        }
    }

    fn desired(display_name: &str) -> Value {
        json!({
            "displayName": display_name,
            "owner": "platform",
            "labels": {"environment": "production"},
            "managementPolicy": "observe_only",
            "provider": "cloudflare",
            "scope": {
                "provider": "cloudflare",
                "accountId": "0123456789abcdef0123456789abcdef"
            },
            "credentialSource": {
                "type": "static_secret",
                "credentialRef": "cloudflare/main"
            }
        })
    }

    #[test]
    fn if_match_requires_one_canonical_strong_entity_tag() {
        let mut headers = HeaderMap::new();
        headers.append(header::IF_MATCH, HeaderValue::from_static("\"1\""));
        headers.append(header::IF_MATCH, HeaderValue::from_static("\"2\""));
        assert!(parse_if_match(&headers).is_err());
        for value in ["W/\"1\"", "*", "\"01\"", "1"] {
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
            assert!(parse_if_match(&headers).is_err());
        }
    }

    async fn send(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
        if_match: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        if let Some(value) = if_match {
            builder = builder.header("if-match", value);
        }
        let response = app
            .oneshot(
                builder
                    .body(Body::from(
                        body.map(|value| value.to_string()).unwrap_or_default(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, json)
    }

    #[tokio::test]
    async fn routes_require_a_composed_store() {
        let app = super::super::router(state(false).await);
        let (status, _, _) = send(
            app,
            "GET",
            "/api/v1/center/cloud/provider-accounts",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn strict_crud_cas_and_secret_free_response() {
        let app = super::super::router(state(true).await);
        let create = json!({"accountId": "Cloudflare.main-1", "desired": desired("Primary")});
        let (status, headers, body) = send(
            app.clone(),
            "POST",
            "/api/v1/center/cloud/provider-accounts",
            Some(create),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(headers.get("etag").unwrap(), "\"1\"");
        assert!(headers.get("location").is_some());
        assert_eq!(body["data"]["generation"], 1);
        assert!(body["data"]["status"].is_null());
        let encoded = body.to_string().to_ascii_lowercase();
        assert!(!encoded.contains("tokenvalue"));
        assert!(!encoded.contains("secretvalue"));

        let replace = json!({"desired": desired("Updated")});
        let (status, _, _) = send(
            app.clone(),
            "PUT",
            "/api/v1/center/cloud/provider-accounts/Cloudflare.main-1",
            Some(replace.clone()),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
        let (status, headers, body) = send(
            app.clone(),
            "PUT",
            "/api/v1/center/cloud/provider-accounts/Cloudflare.main-1",
            Some(replace.clone()),
            Some("\"1\""),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("etag").unwrap(), "\"2\"");
        assert_eq!(body["data"]["displayName"], "Updated");
        let (status, _, _) = send(
            app,
            "PUT",
            "/api/v1/center/cloud/provider-accounts/Cloudflare.main-1",
            Some(replace),
            Some("\"1\""),
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn rejects_unknown_fields_unsafe_ids_and_noncanonical_if_match() {
        let app = super::super::router(state(true).await);
        let mut request = json!({"accountId": "valid", "desired": desired("Primary")});
        request["unknown"] = json!(true);
        let (status, _, body) = send(
            app.clone(),
            "POST",
            "/api/v1/center/cloud/provider-accounts",
            Some(request),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");

        let mut secret_shaped =
            json!({"accountId": "secret-shaped", "desired": desired("Primary")});
        secret_shaped["desired"]["credentialSource"]["secretValue"] = json!("tokenvalue");
        let (status, _, body) = send(
            app.clone(),
            "POST",
            "/api/v1/center/cloud/provider-accounts",
            Some(secret_shaped),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
        assert!(!body.to_string().contains("tokenvalue"));

        let request = json!({"accountId": "unsafe/id", "desired": desired("Primary")});
        let (status, _, _) = send(
            app.clone(),
            "POST",
            "/api/v1/center/cloud/provider-accounts",
            Some(request),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let request = json!({"accountId": "valid", "desired": desired("Primary")});
        assert_eq!(
            send(
                app.clone(),
                "POST",
                "/api/v1/center/cloud/provider-accounts",
                Some(request),
                None,
            )
            .await
            .0,
            StatusCode::CREATED
        );
        let replace = json!({"desired": desired("Updated")});
        assert_eq!(
            send(
                app,
                "PUT",
                "/api/v1/center/cloud/provider-accounts/valid",
                Some(replace),
                Some("\"01\""),
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        for invalid in ["W/\"1\"", "*"] {
            let replace = json!({"desired": desired("Updated")});
            assert_eq!(
                send(
                    super::super::router(state(true).await),
                    "PUT",
                    "/api/v1/center/cloud/provider-accounts/valid",
                    Some(replace),
                    Some(invalid),
                )
                .await
                .0,
                StatusCode::BAD_REQUEST
            );
        }
    }

    #[tokio::test]
    async fn duplicate_not_found_scope_immutability_delete_and_capability_gating() {
        let shared_state = state(true).await;
        let app = super::super::router(shared_state.clone());
        let request = json!({"accountId": "account-1", "desired": desired("Primary")});
        assert_eq!(
            send(
                app.clone(),
                "POST",
                "/api/v1/center/cloud/provider-accounts",
                Some(request.clone()),
                None,
            )
            .await
            .0,
            StatusCode::CREATED
        );
        assert_eq!(
            send(
                app.clone(),
                "POST",
                "/api/v1/center/cloud/provider-accounts",
                Some(request),
                None,
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            send(
                app.clone(),
                "GET",
                "/api/v1/center/cloud/provider-accounts/missing",
                None,
                None,
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        let mut changed = desired("Moved");
        changed["scope"]["accountId"] = json!("ffffffffffffffffffffffffffffffff");
        assert_eq!(
            send(
                app.clone(),
                "PUT",
                "/api/v1/center/cloud/provider-accounts/account-1",
                Some(json!({"desired": changed})),
                Some("\"1\""),
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            send(
                app,
                "DELETE",
                "/api/v1/center/cloud/provider-accounts/account-1",
                None,
                None,
            )
            .await
            .0,
            StatusCode::METHOD_NOT_ALLOWED
        );

        let mut capability_off = shared_state;
        capability_off.capabilities.provider_account_admin = false;
        assert_eq!(
            send(
                super::super::router(capability_off),
                "GET",
                "/api/v1/center/cloud/provider-accounts",
                None,
                None,
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn request_body_is_bounded() {
        let app = super::super::router(state(true).await);
        let request = json!({
            "accountId": "too-large",
            "desired": {
                "displayName": "x".repeat(80 * 1024),
                "provider": "cloudflare",
                "scope": {"provider": "cloudflare", "accountId": "0123456789abcdef0123456789abcdef"},
                "credentialSource": {"type": "ambient"}
            }
        });
        assert_eq!(
            send(
                app,
                "POST",
                "/api/v1/center/cloud/provider-accounts",
                Some(request),
                None,
            )
            .await
            .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn pagination_is_case_sensitive_and_cursor_is_strict() {
        let app = super::super::router(state(true).await);
        for id in ["A", "a", "b"] {
            let request = json!({"accountId": id, "desired": desired(id)});
            assert_eq!(
                send(
                    app.clone(),
                    "POST",
                    "/api/v1/center/cloud/provider-accounts",
                    Some(request),
                    None,
                )
                .await
                .0,
                StatusCode::CREATED
            );
        }
        let (status, _, first) = send(
            app.clone(),
            "GET",
            "/api/v1/center/cloud/provider-accounts?limit=2",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first["data"][0]["accountId"], "A");
        assert_eq!(first["data"][1]["accountId"], "a");
        let cursor = first["continue_token"].as_str().unwrap();
        let uri = format!("/api/v1/center/cloud/provider-accounts?limit=2&cursor={cursor}");
        let (_, _, second) = send(app.clone(), "GET", &uri, None, None).await;
        assert_eq!(second["data"][0]["accountId"], "b");
        assert_eq!(
            send(
                app,
                "GET",
                "/api/v1/center/cloud/provider-accounts?cursor=v1.%2F%2F%2F",
                None,
                None,
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
}
