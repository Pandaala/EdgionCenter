use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult};

macro_rules! identifier {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> CoreResult<Self> {
                let value = value.into();
                if value.trim().is_empty() || value.chars().any(char::is_control) {
                    return Err(CoreError::InvalidIdentifier { kind: $kind, value });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

identifier!(ControllerId, "controller");
identifier!(SessionId, "session");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerRegistration {
    pub controller_id: ControllerId,
    pub session_id: SessionId,
    pub cluster: String,
    #[serde(default)]
    pub environments: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub connected_replica: Option<String>,
    pub ownership_fence: Option<OwnershipFence>,
    pub observed_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipFence {
    pub token: String,
    pub epoch: u64,
}

/// Network route to the Center replica that currently owns a Controller.
///
/// `holder` is deliberately opaque to the core. Platform adapters are
/// responsible for validating it and resolving it to a routable endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerOwnerRoute {
    pub holder: String,
    pub endpoint: String,
    pub ownership_fence: OwnershipFence,
}

#[async_trait::async_trait]
pub trait ControllerOwnerLocator: Send + Sync {
    async fn locate(&self, id: &ControllerId) -> CoreResult<Option<ControllerOwnerRoute>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerPhase {
    Online,
    Offline,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerRecord {
    pub controller_id: ControllerId,
    pub current_session_id: Option<SessionId>,
    pub cluster: String,
    #[serde(default)]
    pub environments: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub connected_replica: Option<String>,
    /// Distributed ownership fence for the projected live session.
    /// Standalone records do not use distributed ownership.
    pub ownership_fence: Option<crate::OwnershipFence>,
    /// Runtime diagnostics for the current fenced session. Kubernetes mode
    /// persists these fields so every replica exposes the same read model.
    pub sync_version: Option<u64>,
    pub watch_server_id: Option<String>,
    pub resource_count: Option<u64>,
    pub stats_updated_unix_ms: Option<i64>,
    pub watch_updated_unix_ms: Option<i64>,
    pub phase: ControllerPhase,
    pub last_seen_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerRuntimeObservation {
    pub controller_id: ControllerId,
    pub session_id: SessionId,
    pub ownership_fence: Option<OwnershipFence>,
    pub sync_version: Option<u64>,
    pub watch_server_id: Option<String>,
    pub resource_count: Option<u64>,
    pub stats_updated_unix_ms: Option<i64>,
    pub watch_updated_unix_ms: Option<i64>,
    pub observed_at_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictionOutcome {
    Evicted,
    AlreadyAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictionTarget {
    pub session_id: Option<SessionId>,
    pub connected_replica: Option<String>,
    pub ownership_fence: Option<OwnershipFence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictionResult {
    pub outcome: EvictionOutcome,
    /// Exact live session observed by the durable eviction CAS. Cleanup must
    /// never target a newer fence that appeared after this linearization point.
    pub target: Option<EvictionTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflineOutcome {
    Marked,
    NotCurrent,
}

#[async_trait::async_trait]
pub trait ControllerDirectory: Send + Sync {
    async fn upsert_registration(&self, registration: ControllerRegistration) -> CoreResult<()>;

    async fn mark_offline(
        &self,
        id: &ControllerId,
        observed_session: &SessionId,
        ownership_fence: Option<&OwnershipFence>,
        observed_at_unix_ms: i64,
    ) -> CoreResult<OfflineOutcome>;

    async fn list(&self) -> CoreResult<Vec<ControllerRecord>>;

    /// Persist runtime diagnostics for the current session. Adapters without
    /// a shared runtime projection may keep the default no-op.
    async fn project_runtime(
        &self,
        _observation: ControllerRuntimeObservation,
    ) -> CoreResult<bool> {
        Ok(false)
    }

    async fn evict(&self, id: &ControllerId) -> CoreResult<EvictionResult>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_empty_and_control_characters() {
        assert!(ControllerId::new("  ").is_err());
        assert!(SessionId::new("session\n2").is_err());
        assert_eq!(
            ControllerId::new("cluster-a/controller-0")
                .unwrap()
                .as_str(),
            "cluster-a/controller-0"
        );
    }
}
