//! Sanitized, explicitly refreshed ProviderAccount credential inspection API.

use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use edgion_center_core::{
    CloudProvider, CoreError, CredentialIssueKind, CredentialState, ProviderIdentity,
};
use edgion_center_runtime::cloud::CredentialInspectionAuthority;
use serde::Serialize;

use super::{provider_accounts::parse_account_id, ApiState};
use crate::common::api::ApiResponse;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInspectionDto {
    pub provider_account_id: String,
    pub provider_account_generation: u64,
    pub state: CredentialState,
    pub identity: Option<ProviderIdentityDto>,
    pub expires_at_unix_ms: Option<i64>,
    pub issues: Vec<CredentialIssueDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdentityDto {
    pub provider: CloudProvider,
    pub scope: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialIssueDto {
    pub kind: CredentialIssueKind,
}

impl From<&ProviderIdentity> for ProviderIdentityDto {
    fn from(value: &ProviderIdentity) -> Self {
        Self {
            provider: value.provider.clone(),
            scope: value.scope.clone(),
        }
    }
}

impl From<CredentialInspectionAuthority> for CredentialInspectionDto {
    fn from(value: CredentialInspectionAuthority) -> Self {
        Self {
            provider_account_id: value.provider_account_id().to_string(),
            provider_account_generation: value.provider_account_generation(),
            state: value.state(),
            identity: value.identity().map(ProviderIdentityDto::from),
            expires_at_unix_ms: value.expires_at_unix_ms(),
            issues: value
                .issues()
                .iter()
                .map(|issue| CredentialIssueDto { kind: issue.kind })
                .collect(),
        }
    }
}

fn error(status: StatusCode, code: &'static str) -> Response {
    no_store((status, Json(ApiResponse::<()>::err_body(code.to_string()))).into_response())
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub async fn refresh(State(state): State<ApiState>, Path(account_id): Path<String>) -> Response {
    let Some(service) = state.credential_inspection_service.as_ref() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "credential_inspection_unavailable",
        );
    };
    let account_id = match parse_account_id(account_id) {
        Ok(value) => value,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    match service.inspect(&account_id).await {
        Ok(inspection) => no_store(
            Json(ApiResponse::ok_body(CredentialInspectionDto::from(
                inspection,
            )))
            .into_response(),
        ),
        Err(CoreError::NotFound(_)) => error(StatusCode::NOT_FOUND, "provider_account_not_found"),
        Err(CoreError::Unsupported(_)) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "credential_inspector_unavailable",
        ),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "credential_inspection_failed",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc, time::Duration};

    use async_trait::async_trait;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use edgion_center_core::{
        CloudResourceId, CoreResult, CredentialInspection, CredentialInspector, CredentialSource,
        DeletionPolicy, ManagementPolicy, ProviderAccountCreateResult, ProviderAccountDesired,
        ProviderAccountScope, ProviderAccountSpec, ProviderAccountStore,
    };
    use edgion_center_runtime::cloud::{CredentialInspectionService, CredentialInspectorResolver};
    use parking_lot::Mutex;
    use serde_json::Value;
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

    struct Inspector;

    #[async_trait]
    impl CredentialInspector for Inspector {
        async fn inspect(&self, _: &ProviderAccountSpec) -> CoreResult<CredentialInspection> {
            Ok(CredentialInspection {
                state: CredentialState::Valid,
                identity: Some(ProviderIdentity {
                    provider: CloudProvider::Cloudflare,
                    principal: "token:7f3b".into(),
                    scope: Some("0123456789abcdef0123456789abcdef".into()),
                }),
                credential_revision: Some("private-secret-rv-9".into()),
                expires_at_unix_ms: None,
                issues: Vec::new(),
            })
        }
    }

    struct Resolver;

    #[async_trait]
    impl CredentialInspectorResolver for Resolver {
        async fn resolve(
            &self,
            _: &edgion_center_core::ProviderAccount,
        ) -> Option<Arc<dyn CredentialInspector>> {
            Some(Arc::new(Inspector))
        }
    }

    async fn state(compose_service: bool, advertise_capability: bool) -> super::super::ApiState {
        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let store = Arc::new(
            edgion_center_adapter_sql::Store::open_in_memory()
                .await
                .unwrap(),
        );
        let desired = ProviderAccountDesired {
            display_name: "Cloudflare main".into(),
            owner: None,
            labels: Default::default(),
            management_policy: ManagementPolicy::ObserveOnly,
            deletion_policy: DeletionPolicy::Retain,
            spec: ProviderAccountSpec {
                provider: CloudProvider::Cloudflare,
                scope: Some(ProviderAccountScope::Cloudflare {
                    account_id: "0123456789abcdef0123456789abcdef".into(),
                }),
                credential_source: CredentialSource::Ambient,
            },
        };
        assert!(matches!(
            store
                .create(&CloudResourceId::new("cf-main").unwrap(), &desired)
                .await
                .unwrap(),
            ProviderAccountCreateResult::Created(_)
        ));
        let account_store = store as Arc<dyn ProviderAccountStore>;
        let service = compose_service.then(|| {
            CredentialInspectionService::new(
                Duration::from_secs(1),
                account_store.clone(),
                Arc::new(Resolver),
            )
            .unwrap()
        });
        let mut capabilities = edgion_center_core::CenterCapabilities::for_mode(
            edgion_center_core::CenterMode::Standalone,
        );
        capabilities.provider_credential_inspection = advertise_capability;
        super::super::ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            commander: Arc::new(Commander::new(
                registry.clone(),
                Arc::new(Mutex::new(HashMap::new())),
                5,
            )),
            proxy: Arc::new(ProxyForwarder::new(
                registry.clone(),
                Arc::new(Mutex::new(HashMap::new())),
                5,
            )),
            controller_directory: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: None,
            role_admin: None,
            audit_reader: None,
            cloudflare_dns_admin: None,
            cloudflare_dns_write_admin: None,
            cloudflare_waf_admin: None,
            route53_dns_admin: None,
            route53_dns_write_admin: None,
            provider_account_store: Some(account_store),
            capability_snapshot_store: None,
            credential_inspection_service: service,
            metadata_store,
            sync_client,
            registry,
            platform_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            authz_mode: edgion_center_core::AuthzMode::AllowAll,
            platform_mode: edgion_center_core::CenterMode::Standalone,
            capabilities,
        }
    }

    async fn post(app: axum::Router, account: &str) -> (StatusCode, axum::http::HeaderMap, Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/center/cloud/provider-credential-inspections/accounts/{account}/refresh"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn route_requires_both_capability_and_service() {
        for (service, capability) in [(false, true), (true, false)] {
            let response = post(
                super::super::router(state(service, capability).await),
                "cf-main",
            )
            .await;
            assert_eq!(response.0, StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn response_is_sanitized_and_missing_account_is_fixed() {
        let app = super::super::router(state(true, true).await);
        let (status, headers, body) = post(app.clone(), "cf-main").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(body["data"]["state"], "valid");
        assert_eq!(body["data"]["providerAccountGeneration"], 1);
        let encoded = body.to_string();
        assert!(!encoded.contains("private-secret-rv-9"));
        assert!(!encoded.contains("token:7f3b"));
        assert!(!encoded.contains("principal"));
        assert!(!encoded.contains("credentialRevision"));
        assert!(!encoded.contains("credentialRef"));

        let (status, headers, body) = post(app, "missing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(body["error"], "provider_account_not_found");
    }
}
