//! Background poller: fetches the controllers' /effective views and feeds the
//! aggregator store. Replaces the dead fed_sync RegionRoute/GIR feed.

use std::collections::{HashMap, HashSet};

use crate::metadata_store::{CenterMetaDataStore, EffectiveGirView};
use edgion_center_core::{ControllerDirectory, ControllerOwnerRoute, ControllerPhase, CoreResult};

/// HTTP response returned through a connected Controller.
pub struct ControllerHttpResponse {
    pub status_code: u32,
    pub body: Vec<u8>,
}

/// Narrow runtime contract for Controller-bound HTTP requests.
#[async_trait::async_trait]
pub trait ControllerHttpClient: Send + Sync {
    async fn request(
        &self,
        controller_id: &str,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<ControllerHttpResponse, String>;

    /// Execute against exactly the supplied owner route and fence. Implementations
    /// must not re-resolve, retry on a newer owner, or fall back to an unfenced
    /// local session.
    async fn request_fenced(
        &self,
        _controller_id: &str,
        _method: String,
        _path: String,
        _headers: HashMap<String, String>,
        _body: Vec<u8>,
        _expected_owner: &ControllerOwnerRoute,
    ) -> Result<ControllerHttpResponse, String> {
        Err("owner-fenced Controller request is unsupported".to_string())
    }

    /// Execute only while the supplied standalone session remains current.
    async fn request_session_fenced(
        &self,
        _controller_id: &str,
        _method: String,
        _path: String,
        _headers: HashMap<String, String>,
        _body: Vec<u8>,
        _expected_session_id: &str,
    ) -> Result<ControllerHttpResponse, String> {
        Err("session-fenced Controller request is unsupported".to_string())
    }
}

/// Tolerant element-wise parse for GIR effective views.
/// A single bad element is dropped, not the whole batch.
pub fn parse_gir_effective(body: &[u8]) -> Vec<EffectiveGirView> {
    let v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = v
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    arr.into_iter()
        .filter_map(|el| serde_json::from_value::<EffectiveGirView>(el).ok())
        .collect()
}

/// Poll one controller's GIR effective endpoint and update the store.
///
/// RegionRoute is fed directly by the federation EdgionConfigData list/watch
/// stream and must not be rebuilt by polling EdgionPlugins and route usages.
/// On a non-200 response or forwarding error the store is left unchanged for that
/// endpoint (fail-open: keep the previous snapshot rather than clearing it).
pub async fn poll_controller_once<C: ControllerHttpClient + ?Sized>(
    client: &C,
    store: &CenterMetaDataStore,
    controller_id: &str,
) -> Result<(), String> {
    poll_controller_once_inner(client, store, controller_id, None, None).await
}

pub async fn poll_controller_once_owner_fenced<C: ControllerHttpClient + ?Sized>(
    client: &C,
    store: &CenterMetaDataStore,
    controller_id: &str,
    revision: &str,
    expected_owner: &ControllerOwnerRoute,
) -> Result<(), String> {
    poll_controller_once_inner(
        client,
        store,
        controller_id,
        Some(revision),
        Some(expected_owner),
    )
    .await
}

async fn poll_controller_once_inner<C: ControllerHttpClient + ?Sized>(
    client: &C,
    store: &CenterMetaDataStore,
    controller_id: &str,
    revision: Option<&str>,
    expected_owner: Option<&ControllerOwnerRoute>,
) -> Result<(), String> {
    let gir = match expected_owner {
        Some(owner) => {
            client
                .request_fenced(
                    controller_id,
                    "GET".to_string(),
                    "/api/v1/global-ip-restrictions/effective".to_string(),
                    HashMap::new(),
                    Vec::new(),
                    owner,
                )
                .await?
        }
        None => {
            client
                .request(
                    controller_id,
                    "GET".to_string(),
                    "/api/v1/global-ip-restrictions/effective".to_string(),
                    HashMap::new(),
                    Vec::new(),
                )
                .await?
        }
    };
    if gir.status_code != 200 {
        return Err(format!("GIR endpoint returned {}", gir.status_code));
    }
    let gir_body: serde_json::Value = serde_json::from_slice(&gir.body)
        .map_err(|error| format!("invalid GIR response: {error}"))?;
    if !gir_body
        .get("data")
        .is_some_and(serde_json::Value::is_array)
    {
        return Err("GIR response has no data array".to_string());
    }
    let girs = parse_gir_effective(&gir.body);
    if let Some(revision) = revision {
        if !store.replace_gir_fenced(controller_id, revision, girs) {
            return Err("Controller session changed during GIR poll".to_string());
        }
    } else {
        store.replace_gir(controller_id, girs);
    }
    Ok(())
}

/// Rebuild this replica's effective read model from the durable Controller
/// directory. The HTTP client may route through another Center replica, so the
/// caller does not need to own any of the listed federation streams locally.
///
/// Offline Controllers remain in the store as historical observations. Only
/// Controllers no longer visible in the directory (including durable eviction
/// fences) are pruned.
pub async fn poll_directory_once<C: ControllerHttpClient + ?Sized>(
    directory: &dyn ControllerDirectory,
    client: &C,
    store: &CenterMetaDataStore,
) -> CoreResult<usize> {
    let records = directory.list().await?;
    let visible: HashSet<String> = records
        .iter()
        .map(|record| record.controller_id.to_string())
        .collect();
    store.retain_controllers(&visible);

    let online: Vec<String> = records
        .into_iter()
        .filter(|record| record.phase == ControllerPhase::Online)
        .map(|record| record.controller_id.to_string())
        .collect();
    let count = online.len();
    for id in &online {
        poll_controller_once(client, store, id)
            .await
            .map_err(edgion_center_core::CoreError::Adapter)?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_center_core::{
        ControllerId, ControllerRecord, ControllerRegistration, EvictionResult, OfflineOutcome,
        OwnershipFence, SessionId,
    };
    use std::sync::Mutex;

    struct FakeClient {
        responses: Mutex<HashMap<String, Result<ControllerHttpResponse, String>>>,
    }

    struct FakeDirectory(Vec<ControllerRecord>);

    #[async_trait::async_trait]
    impl ControllerDirectory for FakeDirectory {
        async fn upsert_registration(&self, _: ControllerRegistration) -> CoreResult<()> {
            unreachable!()
        }

        async fn mark_offline(
            &self,
            _: &ControllerId,
            _: &SessionId,
            _: Option<&edgion_center_core::OwnershipFence>,
            _: i64,
        ) -> CoreResult<OfflineOutcome> {
            unreachable!()
        }

        async fn list(&self) -> CoreResult<Vec<ControllerRecord>> {
            Ok(self.0.clone())
        }

        async fn evict(&self, _: &ControllerId) -> CoreResult<EvictionResult> {
            unreachable!()
        }
    }

    #[async_trait::async_trait]
    impl ControllerHttpClient for FakeClient {
        async fn request(
            &self,
            _controller_id: &str,
            _method: String,
            path: String,
            _headers: HashMap<String, String>,
            _body: Vec<u8>,
        ) -> Result<ControllerHttpResponse, String> {
            self.responses
                .lock()
                .unwrap()
                .remove(&path)
                .unwrap_or_else(|| Err("missing fake response".to_string()))
        }

        async fn request_fenced(
            &self,
            controller_id: &str,
            method: String,
            path: String,
            headers: HashMap<String, String>,
            body: Vec<u8>,
            _expected_owner: &ControllerOwnerRoute,
        ) -> Result<ControllerHttpResponse, String> {
            self.request(controller_id, method, path, headers, body)
                .await
        }
    }

    fn owner_route() -> ControllerOwnerRoute {
        ControllerOwnerRoute {
            holder: "replica-a".to_string(),
            endpoint: "https://replica-a:12252".to_string(),
            ownership_fence: OwnershipFence {
                token: "fence-1".to_string(),
                epoch: 1,
            },
        }
    }

    #[test]
    fn tolerant_parse_gir_drops_bad_keeps_good() {
        let body = br#"{"success":true,"data":[
            {"namespace":"default","pluginName":"gir1","enable":true,"activeProfile":"strict","profiles":{}},
            {"garbage":true}
        ]}"#;
        let parsed = parse_gir_effective(body);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].plugin_name, "gir1");
    }

    #[tokio::test]
    async fn polling_contract_updates_successes_and_preserves_failed_snapshots() {
        let store = CenterMetaDataStore::new();
        let responses = HashMap::from([(
                "/api/v1/global-ip-restrictions/effective".to_string(),
                Ok(ControllerHttpResponse {
                    status_code: 200,
                    body: br#"{"data":[{"namespace":"default","pluginName":"gir1","enable":true,"activeProfile":"strict","profiles":{}}]}"#.to_vec(),
                }),
            )]);
        let client = FakeClient {
            responses: Mutex::new(responses),
        };

        poll_controller_once(&client, &store, "c1").await.unwrap();
        assert_eq!(store.list_gir_effective().len(), 1);

        let failing = FakeClient {
            responses: Mutex::new(HashMap::new()),
        };
        assert!(poll_controller_once(&failing, &store, "c1").await.is_err());
        assert_eq!(store.list_gir_effective().len(), 1);
    }

    #[tokio::test]
    async fn fresh_replica_polls_global_directory_without_local_registry_state() {
        let store = CenterMetaDataStore::new();
        let directory = FakeDirectory(vec![ControllerRecord {
            controller_id: ControllerId::new("cluster-a/controller-a").unwrap(),
            current_session_id: Some(SessionId::new("session-a").unwrap()),
            cluster: "cluster-a".to_string(),
            environments: vec!["prod".to_string()],
            tags: Vec::new(),
            connected_replica: Some("center-a/uid-a".to_string()),
            ownership_fence: Some(edgion_center_core::OwnershipFence {
                token: "token-a".to_string(),
                epoch: 1,
            }),
            sync_version: None,
            watch_server_id: None,
            resource_count: None,
            resource_counts_by_kind: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: None,
            phase: ControllerPhase::Online,
            last_seen_unix_ms: 1,
        }]);
        let responses = HashMap::from([(
                "/api/v1/global-ip-restrictions/effective".to_string(),
                Ok(ControllerHttpResponse {
                    status_code: 200,
                    body: br#"{"data":[{"namespace":"default","pluginName":"gir","enable":true,"activeProfile":"strict","profiles":{}}]}"#.to_vec(),
                }),
            )]);
        let client = FakeClient {
            responses: Mutex::new(responses),
        };

        assert_eq!(
            poll_directory_once(&directory, &client, &store)
                .await
                .unwrap(),
            1
        );
        assert_eq!(store.list_gir_effective()[0].controllers.len(), 1);
    }

    #[tokio::test]
    async fn displaced_session_cannot_publish_late_effective_snapshots() {
        let store = CenterMetaDataStore::new();
        let ids = HashSet::from(["c1".to_string()]);
        store.prepare_revisions(&HashMap::from([(
            "c1".to_string(),
            "session-a".to_string(),
        )]));
        let client = FakeClient {
            responses: Mutex::new(HashMap::from([(
                "/api/v1/global-ip-restrictions/effective".to_string(),
                Ok(ControllerHttpResponse {
                    status_code: 200,
                    body: br#"{"data":[]}"#.to_vec(),
                }),
            )])),
        };
        poll_controller_once_owner_fenced(&client, &store, "c1", "session-a", &owner_route())
            .await
            .unwrap();
        assert!(store.has_fresh_coverage(&ids, std::time::Duration::from_secs(30)));

        store.prepare_revisions(&HashMap::from([(
            "c1".to_string(),
            "session-b".to_string(),
        )]));
        assert!(!store.has_fresh_coverage(&ids, std::time::Duration::from_secs(30)));
        let late = FakeClient {
            responses: Mutex::new(HashMap::from([(
                "/api/v1/global-ip-restrictions/effective".to_string(),
                Ok(ControllerHttpResponse {
                    status_code: 200,
                    body: br#"{"data":[]}"#.to_vec(),
                }),
            )])),
        };
        assert!(poll_controller_once_owner_fenced(
            &late,
            &store,
            "c1",
            "session-a",
            &owner_route()
        )
        .await
        .is_err());
        assert!(!store.has_fresh_coverage(&ids, std::time::Duration::from_secs(30)));
    }
}
