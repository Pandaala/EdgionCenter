//! Controller target resolution for GlobalResources, shared with the durable
//! GlobalResource sync planner.
//!
//! This module used to also serve the live cross-cluster inventory fan-out
//! (list/detail HTTP requests broadcast to every Controller). That fan-out
//! has been retired in favor of the federation watch read model; only the
//! authoritative-target resolution core survives here because
//! `global_resource_planner` depends on it for its own apply-path target
//! resolution.

use crate::federation::registry::ControllerRegistry;
use crate::poll::ControllerHttpClient;
use edgion_center_core::{
    ControllerOwnerLocator, ControllerOwnerRoute, ControllerPhase, ControllerRecord, CoreError,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_MAX_TARGET_CLUSTERS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterResolutionState {
    Available,
    Offline,
    Ambiguous,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterResolution {
    pub cluster: String,
    pub state: ClusterResolutionState,
    pub controller_id: Option<String>,
    pub candidates: Vec<String>,
}

#[derive(Clone)]
pub(crate) enum TargetResolutionMode {
    Standalone(ControllerRegistry),
    Kubernetes(Arc<dyn ControllerOwnerLocator>),
}

#[derive(Clone)]
pub(crate) struct ResolvedTarget {
    pub(crate) resolution: ClusterResolution,
    pub(crate) session_id: Option<String>,
    pub(crate) owner_route: Option<ControllerOwnerRoute>,
}

pub(crate) async fn resolve_authoritative_targets(
    records: Vec<ControllerRecord>,
    mode: &TargetResolutionMode,
    requested: &HashSet<&str>,
) -> Result<Vec<ResolvedTarget>, CoreError> {
    let mut ids = HashSet::new();
    let mut grouped = BTreeMap::<String, Vec<ControllerRecord>>::new();
    for record in records {
        if !ids.insert(record.controller_id.to_string()) {
            return Err(CoreError::Conflict(format!(
                "duplicate Controller directory entry {}",
                record.controller_id
            )));
        }
        if record.cluster.is_empty() || record.cluster.trim() != record.cluster {
            return Err(CoreError::Conflict(format!(
                "Controller directory entry {} has an invalid cluster identity",
                record.controller_id
            )));
        }
        if !requested.is_empty() && !requested.contains(record.cluster.as_str()) {
            continue;
        }
        grouped
            .entry(record.cluster.clone())
            .or_default()
            .push(record);
    }
    if grouped.len() > DEFAULT_MAX_TARGET_CLUSTERS {
        return Err(CoreError::Conflict(format!(
            "GlobalResources inventory cannot target more than {DEFAULT_MAX_TARGET_CLUSTERS} clusters"
        )));
    }

    let mut output = Vec::with_capacity(grouped.len());
    for (cluster, records) in grouped {
        let mut known = records
            .iter()
            .map(|record| record.controller_id.to_string())
            .collect::<Vec<_>>();
        known.sort();
        let mut eligible = Vec::<(String, String, Option<ControllerOwnerRoute>)>::new();
        let mut indeterminate = Vec::new();
        for record in records {
            if record.phase != ControllerPhase::Online {
                continue;
            }
            let Some(session_id) = record.current_session_id.as_ref() else {
                indeterminate.push(record.controller_id.to_string());
                continue;
            };
            match mode {
                TargetResolutionMode::Standalone(registry) => {
                    if registry
                        .is_dispatchable_session(record.controller_id.as_str(), session_id.as_str())
                    {
                        eligible.push((
                            record.controller_id.to_string(),
                            session_id.to_string(),
                            None,
                        ));
                    } else {
                        indeterminate.push(record.controller_id.to_string());
                    }
                }
                TargetResolutionMode::Kubernetes(locator) => {
                    let Some(expected_holder) = record.connected_replica.as_deref() else {
                        indeterminate.push(record.controller_id.to_string());
                        continue;
                    };
                    let Some(expected_fence) = record.ownership_fence.as_ref() else {
                        indeterminate.push(record.controller_id.to_string());
                        continue;
                    };
                    if expected_fence.token.is_empty() || expected_fence.epoch == 0 {
                        indeterminate.push(record.controller_id.to_string());
                        continue;
                    }
                    match tokio::time::timeout(
                        Duration::from_secs(2),
                        locator.locate(&record.controller_id),
                    )
                    .await
                    {
                        Ok(Ok(Some(route)))
                            if route.holder == expected_holder
                                && route.ownership_fence == *expected_fence =>
                        {
                            eligible.push((
                                record.controller_id.to_string(),
                                session_id.to_string(),
                                Some(route),
                            ));
                        }
                        _ => indeterminate.push(record.controller_id.to_string()),
                    }
                }
            }
        }
        eligible.sort_by(|left, right| left.0.cmp(&right.0));
        indeterminate.sort();
        let target = if !indeterminate.is_empty() {
            let mut candidates = eligible
                .iter()
                .map(|entry| entry.0.clone())
                .chain(indeterminate)
                .collect::<Vec<_>>();
            candidates.sort();
            ResolvedTarget {
                resolution: ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Indeterminate,
                    controller_id: None,
                    candidates,
                },
                session_id: None,
                owner_route: None,
            }
        } else if eligible.len() > 1 {
            ResolvedTarget {
                resolution: ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Ambiguous,
                    controller_id: None,
                    candidates: eligible.iter().map(|entry| entry.0.clone()).collect(),
                },
                session_id: None,
                owner_route: None,
            }
        } else if let Some((controller_id, session_id, owner_route)) = eligible.pop() {
            ResolvedTarget {
                resolution: ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Available,
                    controller_id: Some(controller_id.clone()),
                    candidates: vec![controller_id],
                },
                session_id: Some(session_id),
                owner_route,
            }
        } else {
            ResolvedTarget {
                resolution: ClusterResolution {
                    cluster,
                    state: ClusterResolutionState::Offline,
                    controller_id: None,
                    candidates: known,
                },
                session_id: None,
                owner_route: None,
            }
        };
        output.push(target);
    }
    Ok(output)
}

pub(crate) async fn request_target(
    client: &dyn ControllerHttpClient,
    controller_id: &str,
    session_id: Option<&str>,
    owner_route: Option<&ControllerOwnerRoute>,
    path: String,
) -> Result<crate::poll::ControllerHttpResponse, String> {
    if let Some(route) = owner_route {
        return client
            .request_fenced(
                controller_id,
                "GET".to_string(),
                path,
                HashMap::new(),
                Vec::new(),
                route,
            )
            .await;
    }
    client
        .request_session_fenced(
            controller_id,
            "GET".to_string(),
            path,
            HashMap::new(),
            Vec::new(),
            session_id.ok_or_else(|| "resolved target has no session fence".to_string())?,
        )
        .await
}

pub(crate) fn stable_revision(value: &impl Serialize) -> String {
    let canonical = serde_json::to_vec(value).expect("revision input is serializable");
    let digest = Sha256::digest(canonical);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::federation::proto::RegisterRequest;
    use edgion_center_core::{ControllerId, OwnershipFence, SessionId};
    use tokio::sync::mpsc;

    fn record(id: &str, cluster: &str, phase: ControllerPhase) -> ControllerRecord {
        ControllerRecord {
            controller_id: ControllerId::new(id).unwrap(),
            current_session_id: (phase == ControllerPhase::Online)
                .then(|| SessionId::new(format!("session-{id}")).unwrap()),
            cluster: cluster.into(),
            environments: Vec::new(),
            tags: Vec::new(),
            connected_replica: None,
            ownership_fence: None,
            sync_version: None,
            watch_server_id: None,
            resource_count: None,
            resource_counts_by_kind: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: None,
            phase,
            last_seen_unix_ms: 1,
        }
    }

    fn register_request(id: &str, cluster: &str) -> RegisterRequest {
        RegisterRequest {
            controller_id: id.to_string(),
            cluster: cluster.to_string(),
            env: Vec::new(),
            tag: Vec::new(),
        }
    }

    struct FixedOwnerLocator {
        route: Option<ControllerOwnerRoute>,
    }

    #[async_trait::async_trait]
    impl ControllerOwnerLocator for FixedOwnerLocator {
        async fn locate(
            &self,
            _id: &ControllerId,
        ) -> Result<Option<ControllerOwnerRoute>, CoreError> {
            Ok(self.route.clone())
        }
    }

    #[tokio::test]
    async fn standalone_resolution_rejects_a_stale_directory_session() {
        let registry = ControllerRegistry::new();
        let (tx, _rx) = mpsc::channel(1);
        registry.register(
            "controller-a".into(),
            register_request("controller-a", "cluster-a"),
            tx,
            "different-session".into(),
        );

        let targets = resolve_authoritative_targets(
            vec![record("controller-a", "cluster-a", ControllerPhase::Online)],
            &TargetResolutionMode::Standalone(registry),
            &HashSet::new(),
        )
        .await
        .unwrap();

        assert_eq!(
            targets[0].resolution.state,
            ClusterResolutionState::Indeterminate
        );
        assert!(targets[0].session_id.is_none());
    }

    #[tokio::test]
    async fn kubernetes_resolution_rejects_an_owner_fence_mismatch() {
        let mut directory_record = record("controller-a", "cluster-a", ControllerPhase::Online);
        directory_record.connected_replica = Some("center-a".into());
        directory_record.ownership_fence = Some(OwnershipFence {
            token: "expected".into(),
            epoch: 7,
        });
        let locator = Arc::new(FixedOwnerLocator {
            route: Some(ControllerOwnerRoute {
                holder: "center-a".into(),
                endpoint: "http://center-a".into(),
                ownership_fence: OwnershipFence {
                    token: "stale".into(),
                    epoch: 6,
                },
            }),
        });

        let targets = resolve_authoritative_targets(
            vec![directory_record],
            &TargetResolutionMode::Kubernetes(locator),
            &HashSet::new(),
        )
        .await
        .unwrap();

        assert_eq!(
            targets[0].resolution.state,
            ClusterResolutionState::Indeterminate
        );
        assert!(targets[0].owner_route.is_none());
    }

    #[test]
    fn membership_revision_changes_with_session_and_owner_fence() {
        let first = stable_revision(&[(
            "cluster-a",
            "controller-a",
            "session-a",
            "holder-a",
            "token-a",
            1_u64,
        )]);
        let new_session = stable_revision(&[(
            "cluster-a",
            "controller-a",
            "session-b",
            "holder-a",
            "token-a",
            1_u64,
        )]);
        let new_fence = stable_revision(&[(
            "cluster-a",
            "controller-a",
            "session-a",
            "holder-a",
            "token-b",
            2_u64,
        )]);
        assert_ne!(first, new_session);
        assert_ne!(first, new_fence);
    }
}
