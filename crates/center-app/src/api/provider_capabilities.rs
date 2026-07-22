//! Read-only, secret-free ProviderAccount capability snapshot Admin API.

use axum::{
    extract::{rejection::QueryRejection, Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use edgion_center_core::{
    CapabilityAction, CapabilityDimension, CapabilityDiscoveryIssue, CapabilityDiscoveryState,
    CapabilityEvidence, CapabilityIssueScope, CapabilityIssueSeverity, CapabilityObservation,
    CapabilityReason, CapabilityRequirement, CapabilityScope, CapabilitySnapshotKey, CloudProvider,
    CloudResourceId, CloudResourceKind, ProviderCapability, ProviderCapabilitySnapshot,
    ProviderRegion, ProviderResourceRef, TriState,
};
use serde::{Deserialize, Serialize};

use super::{provider_accounts::parse_account_id, ApiState};
use crate::common::api::ApiResponse;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityQuery {
    pub scope: RequestedScope,
    pub region: Option<String>,
    pub resource_kind: Option<CloudResourceKind>,
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedScope {
    Account,
    Region,
    Resource,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilityReadDto {
    pub provider_account_id: String,
    pub provider: CloudProvider,
    pub current_provider_account_generation: u64,
    pub scope: CapabilityScopeDto,
    pub snapshot_state: SnapshotStateDto,
    pub snapshot: Option<ProviderCapabilitySnapshotDto>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotStateDto {
    NotDiscovered,
    Observed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilitySnapshotDto {
    pub observed_account_generation: u64,
    pub account_generation_matches: bool,
    pub authority_state: SnapshotAuthorityStateDto,
    pub credential_authority_state: CredentialAuthorityStateDto,
    pub state: CapabilityDiscoveryState,
    pub discovered_at_unix_ms: i64,
    pub observations: Vec<CapabilityObservationDto>,
    pub issues: Vec<CapabilityIssueDto>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialAuthorityStateDto {
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotAuthorityStateDto {
    Unknown,
    Stale,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CapabilityScopeDto {
    Account,
    Region {
        region: String,
    },
    Resource {
        resource_kind: CloudResourceKind,
        external_id: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityObservationDto {
    pub capability: ProviderCapability,
    pub dimensions: Vec<CapabilityDimensionObservationDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDimensionObservationDto {
    pub dimension: CapabilityDimension,
    pub action: Option<CapabilityAction>,
    pub state: TriState,
    pub reason: Option<CapabilityReason>,
    pub evidence: CapabilityEvidence,
    pub observed_at_unix_ms: i64,
    pub valid_until_unix_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityIssueDto {
    pub severity: CapabilityIssueSeverity,
    pub scope: CapabilityIssueScopeDto,
    pub reason: CapabilityReason,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CapabilityIssueScopeDto {
    Account,
    Requirement {
        requirement: CapabilityRequirement,
    },
    Dimension {
        requirement: CapabilityRequirement,
        dimension: CapabilityDimension,
    },
}

impl CapabilityQuery {
    fn into_scope(self, account_id: &CloudResourceId) -> Result<CapabilityScope, ()> {
        match self.scope {
            RequestedScope::Account
                if self.region.is_none()
                    && self.resource_kind.is_none()
                    && self.external_id.is_none() =>
            {
                Ok(CapabilityScope::Account)
            }
            RequestedScope::Region
                if self.resource_kind.is_none() && self.external_id.is_none() =>
            {
                let region = self.region.ok_or(())?;
                Ok(CapabilityScope::Region {
                    region: ProviderRegion::new(region).map_err(|_| ())?,
                })
            }
            RequestedScope::Resource if self.region.is_none() => {
                let resource_kind = self.resource_kind.ok_or(())?;
                let external_id = self.external_id.ok_or(())?;
                Ok(CapabilityScope::Resource {
                    resource_kind,
                    resource: ProviderResourceRef {
                        provider_account_id: account_id.clone(),
                        external_id,
                    },
                })
            }
            _ => Err(()),
        }
    }
}

impl From<CapabilityScope> for CapabilityScopeDto {
    fn from(value: CapabilityScope) -> Self {
        match value {
            CapabilityScope::Account => Self::Account,
            CapabilityScope::Region { region } => Self::Region {
                region: region.to_string(),
            },
            CapabilityScope::Resource {
                resource_kind,
                resource,
            } => Self::Resource {
                resource_kind,
                external_id: resource.external_id,
            },
        }
    }
}

impl From<CapabilityObservation> for CapabilityObservationDto {
    fn from(value: CapabilityObservation) -> Self {
        Self {
            capability: value.capability,
            dimensions: value
                .dimensions
                .into_iter()
                .map(|dimension| CapabilityDimensionObservationDto {
                    dimension: dimension.dimension,
                    action: dimension.action,
                    state: dimension.state,
                    reason: dimension.reason,
                    evidence: dimension.evidence,
                    observed_at_unix_ms: dimension.observed_at_unix_ms,
                    valid_until_unix_ms: dimension.valid_until_unix_ms,
                })
                .collect(),
        }
    }
}

impl From<CapabilityDiscoveryIssue> for CapabilityIssueDto {
    fn from(value: CapabilityDiscoveryIssue) -> Self {
        let scope = match value.scope {
            CapabilityIssueScope::Account => CapabilityIssueScopeDto::Account,
            CapabilityIssueScope::Requirement { requirement } => {
                CapabilityIssueScopeDto::Requirement { requirement }
            }
            CapabilityIssueScope::Dimension {
                requirement,
                dimension,
            } => CapabilityIssueScopeDto::Dimension {
                requirement,
                dimension,
            },
        };
        Self {
            severity: value.severity,
            scope,
            reason: value.reason,
        }
    }
}

impl ProviderCapabilitySnapshotDto {
    fn from_snapshot(value: ProviderCapabilitySnapshot, current_generation: u64) -> Self {
        Self {
            observed_account_generation: value.fence.provider_account_generation,
            account_generation_matches: value.fence.provider_account_generation
                == current_generation,
            authority_state: if value.fence.provider_account_generation == current_generation {
                SnapshotAuthorityStateDto::Unknown
            } else {
                SnapshotAuthorityStateDto::Stale
            },
            credential_authority_state: CredentialAuthorityStateDto::Unknown,
            state: value.state,
            discovered_at_unix_ms: value.discovered_at_unix_ms,
            observations: value.observations.into_iter().map(Into::into).collect(),
            issues: value.issues.into_iter().map(Into::into).collect(),
        }
    }
}

fn success(value: ProviderCapabilityReadDto) -> Response {
    let mut response = Json(ApiResponse::ok_body(value)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn error(status: StatusCode, code: &'static str) -> Response {
    let mut response =
        (status, Json(ApiResponse::<()>::err_body(code.to_string()))).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn unavailable() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "provider_capability_store_unavailable",
    )
}

pub async fn get(
    State(state): State<ApiState>,
    Path(account_id): Path<String>,
    query: Result<Query<CapabilityQuery>, QueryRejection>,
) -> Response {
    let (Some(account_store), Some(snapshot_store)) = (
        state.provider_account_store.as_ref(),
        state.capability_snapshot_store.as_ref(),
    ) else {
        return unavailable();
    };
    let account_id = match parse_account_id(account_id) {
        Ok(value) => value,
        Err(()) => return error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let query = match query {
        Ok(Query(value)) => value,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let scope = match query.into_scope(&account_id) {
        Ok(value) => value,
        Err(()) => return error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let key = CapabilitySnapshotKey {
        provider_account_id: account_id.clone(),
        scope: scope.clone(),
    };
    if key.validate().is_err() {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let account = match account_store.get(&account_id).await {
        Ok(Some(value)) => value,
        Ok(None) => return error(StatusCode::NOT_FOUND, "provider_account_not_found"),
        Err(_) => return unavailable(),
    };
    let snapshot = match snapshot_store.get(&key).await {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    let snapshot = match snapshot {
        Some(snapshot) => {
            if snapshot.validate().is_err()
                || snapshot.provider_account_id != account_id
                || snapshot.provider != account.spec.provider
                || snapshot.scope != scope
            {
                return unavailable();
            }
            Some(ProviderCapabilitySnapshotDto::from_snapshot(
                snapshot,
                account.metadata.generation,
            ))
        }
        None => None,
    };
    success(ProviderCapabilityReadDto {
        provider_account_id: account_id.to_string(),
        provider: account.spec.provider,
        current_provider_account_generation: account.metadata.generation,
        scope: scope.into(),
        snapshot_state: if snapshot.is_some() {
            SnapshotStateDto::Observed
        } else {
            SnapshotStateDto::NotDiscovered
        },
        snapshot,
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use edgion_center_core::{
        CapabilityDimensionObservation, CapabilityDiscoveryFence, CapabilityDiscoveryReport,
        CapabilityDiscoveryRequest, CapabilityStoreWrite, CredentialSource, DeletionPolicy,
        DiscoveryToken, DnsCapability, ManagementPolicy, ProviderAccountDesired,
        ProviderAccountReplaceResult, ProviderAccountScope, ProviderAccountSpec,
        SanitizedCapabilityCode, SanitizedCapabilityMessage,
    };
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

    #[derive(Default)]
    struct ReadOnlySnapshotStore {
        snapshot: Mutex<Option<ProviderCapabilitySnapshot>>,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl edgion_center_core::CapabilitySnapshotStore for ReadOnlySnapshotStore {
        async fn get(
            &self,
            _key: &CapabilitySnapshotKey,
        ) -> edgion_center_core::CoreResult<Option<ProviderCapabilitySnapshot>> {
            if self.fail {
                Err(edgion_center_core::CoreError::Adapter(
                    "private detail".into(),
                ))
            } else {
                Ok(self.snapshot.lock().clone())
            }
        }

        async fn begin_discovery(
            &self,
            _key: &CapabilitySnapshotKey,
            _provider_account_generation: u64,
            _credential_revision: Option<&str>,
        ) -> edgion_center_core::CoreResult<CapabilityDiscoveryFence> {
            unreachable!("read-only Admin API never begins discovery")
        }

        async fn put_if_current(
            &self,
            _key: &CapabilitySnapshotKey,
            _expected_fence: &CapabilityDiscoveryFence,
            _snapshot: &ProviderCapabilitySnapshot,
        ) -> edgion_center_core::CoreResult<CapabilityStoreWrite> {
            unreachable!("read-only Admin API never writes snapshots")
        }

        async fn invalidate_account_revision(
            &self,
            _account_id: &CloudResourceId,
            _stale_provider_account_generation: u64,
            _stale_credential_revision: Option<&str>,
        ) -> edgion_center_core::CoreResult<()> {
            unreachable!("read-only Admin API never invalidates snapshots")
        }
    }

    fn account_desired() -> ProviderAccountDesired {
        ProviderAccountDesired {
            display_name: "Cloudflare primary".into(),
            owner: Some("platform".into()),
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
        }
    }

    fn snapshot(account_id: &CloudResourceId, generation: u64) -> ProviderCapabilitySnapshot {
        ProviderCapabilitySnapshot::from_report(
            &CapabilityDiscoveryRequest {
                provider_account_id: account_id.clone(),
                fence: CapabilityDiscoveryFence {
                    provider_account_generation: generation,
                    credential_revision: Some("private-credential-revision".into()),
                    discovery_epoch: 41,
                    discovery_token: DiscoveryToken::new("private-discovery-token").unwrap(),
                },
                account: account_desired().spec,
                scope: CapabilityScope::Account,
            },
            1_700_000_000_000,
            CapabilityDiscoveryReport {
                state: CapabilityDiscoveryState::Partial,
                observations: vec![CapabilityObservation {
                    capability: ProviderCapability::Dns(DnsCapability::PublicZones),
                    dimensions: vec![CapabilityDimensionObservation {
                        dimension: CapabilityDimension::AdapterSupport,
                        action: None,
                        state: TriState::Unknown,
                        reason: Some(CapabilityReason::NotDiscovered),
                        evidence: CapabilityEvidence::AdapterContract,
                        code: Some(SanitizedCapabilityCode::new("private-code").unwrap()),
                        message: Some(
                            SanitizedCapabilityMessage::new("private diagnostic message").unwrap(),
                        ),
                        observed_at_unix_ms: 1_700_000_000_000,
                        valid_until_unix_ms: 1_700_000_060_000,
                    }],
                }],
                issues: vec![CapabilityDiscoveryIssue {
                    severity: CapabilityIssueSeverity::Warning,
                    scope: CapabilityIssueScope::Account,
                    reason: CapabilityReason::ProbeFailed,
                    code: SanitizedCapabilityCode::new("private-issue-code").unwrap(),
                    message: SanitizedCapabilityMessage::new("private issue message").unwrap(),
                }],
            },
        )
        .unwrap()
    }

    async fn state(
        account_store: Option<Arc<edgion_center_adapter_sql::Store>>,
        snapshot_store: Option<Arc<ReadOnlySnapshotStore>>,
        capability: bool,
    ) -> ApiState {
        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let mut capabilities = edgion_center_core::CenterCapabilities::for_mode(
            edgion_center_core::CenterMode::Standalone,
        );
        capabilities.provider_account_admin = account_store.is_some();
        capabilities.provider_capability_read = capability;
        ApiState {
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
            provider_account_store: account_store
                .clone()
                .map(|store| store as Arc<dyn edgion_center_core::ProviderAccountStore>),
            capability_snapshot_store: snapshot_store
                .map(|store| store as Arc<dyn edgion_center_core::CapabilitySnapshotStore>),
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

    async fn request(
        app: axum::Router,
        method: &str,
        uri: &str,
    ) -> (StatusCode, http::HeaderMap, Value, usize) {
        let response = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let len = bytes.len();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, body, len)
    }

    async fn account_store(account_id: &CloudResourceId) -> Arc<edgion_center_adapter_sql::Store> {
        let store = Arc::new(
            edgion_center_adapter_sql::Store::open_in_memory()
                .await
                .unwrap(),
        );
        edgion_center_core::ProviderAccountStore::create(
            store.as_ref(),
            account_id,
            &account_desired(),
        )
        .await
        .unwrap();
        store
    }

    #[tokio::test]
    async fn route_requires_capability_and_both_stores() {
        let id = CloudResourceId::new("cf-main").unwrap();
        let accounts = account_store(&id).await;
        let snapshots = Arc::new(ReadOnlySnapshotStore::default());
        for state in [
            state(Some(accounts.clone()), Some(snapshots.clone()), false).await,
            state(Some(accounts.clone()), None, true).await,
            state(None, Some(snapshots.clone()), true).await,
        ] {
            let (status, _, _, _) = request(
                super::super::router(state),
                "GET",
                "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=account",
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn capability_route_is_not_nested_under_provider_accounts() {
        let id = CloudResourceId::new("cf-main").unwrap();
        let accounts = account_store(&id).await;
        let snapshots = Arc::new(ReadOnlySnapshotStore::default());
        let app = super::super::router(state(Some(accounts), Some(snapshots), true).await);
        let (status, _, _, _) = request(
            app,
            "GET",
            "/api/v1/center/cloud/provider-accounts/cf-main/capabilities?scope=account",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn missing_snapshot_is_explicit_not_discovered_and_account_is_checked_first() {
        let id = CloudResourceId::new("cf-main").unwrap();
        let accounts = account_store(&id).await;
        let snapshots = Arc::new(ReadOnlySnapshotStore::default());
        let app = super::super::router(state(Some(accounts), Some(snapshots), true).await);
        let (status, headers, body, _) = request(
            app.clone(),
            "GET",
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=account",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(body["data"]["snapshotState"], "not_discovered");
        assert!(body["data"]["snapshot"].is_null());
        let (status, headers, _, _) = request(
            app,
            "GET",
            "/api/v1/center/cloud/provider-capabilities/accounts/missing?scope=account",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    }

    #[tokio::test]
    async fn response_preserves_unknown_evidence_but_redacts_internal_authority() {
        let id = CloudResourceId::new("cf-main").unwrap();
        let accounts = account_store(&id).await;
        let snapshots = Arc::new(ReadOnlySnapshotStore {
            snapshot: Mutex::new(Some(snapshot(&id, 1))),
            fail: false,
        });
        let app = super::super::router(state(Some(accounts), Some(snapshots), true).await);
        let uri = "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=account";
        let (status, headers, body, _) = request(app.clone(), "GET", uri).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            body["data"]["snapshot"]["observations"][0]["dimensions"][0]["state"],
            "unknown"
        );
        assert_eq!(
            body["data"]["snapshot"]["observations"][0]["dimensions"][0]["validUntilUnixMs"],
            1_700_000_060_000_i64
        );
        assert_eq!(
            body["data"]["snapshot"]["credentialAuthorityState"],
            "unknown"
        );
        let encoded = body.to_string();
        for forbidden in [
            "contractVersion",
            "credentialRevision",
            "discoveryEpoch",
            "discoveryToken",
            "private-credential-revision",
            "private-discovery-token",
            "private diagnostic message",
            "private issue message",
            "private-code",
            "private-issue-code",
        ] {
            assert!(!encoded.contains(forbidden), "leaked {forbidden}");
        }
        let (status, headers, _, len) = request(app, "HEAD", uri).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(len, 0);
    }

    #[tokio::test]
    async fn stale_generation_remains_visible_without_becoming_usable() {
        let id = CloudResourceId::new("cf-main").unwrap();
        let accounts = account_store(&id).await;
        let snapshots = Arc::new(ReadOnlySnapshotStore {
            snapshot: Mutex::new(Some(snapshot(&id, 1))),
            fail: false,
        });
        let replaced = edgion_center_core::ProviderAccountStore::replace_if_generation(
            accounts.as_ref(),
            &id,
            1,
            &account_desired(),
        )
        .await
        .unwrap();
        assert!(matches!(replaced, ProviderAccountReplaceResult::Stored(_)));
        let (status, _, body, _) = request(
            super::super::router(state(Some(accounts), Some(snapshots), true).await),
            "GET",
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=account",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["currentProviderAccountGeneration"], 2);
        assert_eq!(body["data"]["snapshot"]["observedAccountGeneration"], 1);
        assert_eq!(body["data"]["snapshot"]["accountGenerationMatches"], false);
        assert_eq!(body["data"]["snapshot"]["authorityState"], "stale");
        assert!(body.to_string().find("allowed").is_none());
    }

    #[tokio::test]
    async fn query_scopes_are_strict_and_mutually_exclusive() {
        let id = CloudResourceId::new("cf-main").unwrap();
        let accounts = account_store(&id).await;
        let snapshots = Arc::new(ReadOnlySnapshotStore::default());
        let app = super::super::router(state(Some(accounts), Some(snapshots), true).await);
        for uri in [
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=account",
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=region&region=us-east-1",
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=resource&resourceKind=managed_zone&externalId=zone-1",
        ] {
            assert_eq!(request(app.clone(), "GET", uri).await.0, StatusCode::OK);
        }
        for uri in [
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main",
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=account&region=x",
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=region",
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=resource&resourceKind=managed_zone",
            "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=account&unknown=x",
        ] {
            assert_eq!(request(app.clone(), "GET", uri).await.0, StatusCode::BAD_REQUEST);
        }
        let oversized = "x".repeat(1025);
        let uri = format!("/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=resource&resourceKind=managed_zone&externalId={oversized}");
        assert_eq!(request(app, "GET", &uri).await.0, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn malformed_or_mismatched_snapshots_fail_closed_with_fixed_error() {
        let id = CloudResourceId::new("cf-main").unwrap();
        let accounts = account_store(&id).await;
        let mut malformed = snapshot(&id, 1);
        malformed.contract_version = 999;
        let mut mismatched = snapshot(&id, 1);
        mismatched.provider = CloudProvider::Aws;
        let cross_account = snapshot(&CloudResourceId::new("cf-other").unwrap(), 1);
        for store in [
            Arc::new(ReadOnlySnapshotStore {
                snapshot: Mutex::new(Some(malformed)),
                fail: false,
            }),
            Arc::new(ReadOnlySnapshotStore {
                snapshot: Mutex::new(Some(mismatched)),
                fail: false,
            }),
            Arc::new(ReadOnlySnapshotStore {
                snapshot: Mutex::new(Some(cross_account)),
                fail: false,
            }),
            Arc::new(ReadOnlySnapshotStore {
                snapshot: Mutex::new(None),
                fail: true,
            }),
        ] {
            let (status, _, body, _) = request(
                super::super::router(state(Some(accounts.clone()), Some(store), true).await),
                "GET",
                "/api/v1/center/cloud/provider-capabilities/accounts/cf-main?scope=account",
            )
            .await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body["error"], "provider_capability_store_unavailable");
            assert!(!body.to_string().contains("private detail"));
        }
    }
}
