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
    pub observed_at_unix_ms: i64,
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
    pub phase: ControllerPhase,
    pub last_seen_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictionOutcome {
    Evicted,
    AlreadyAbsent,
}

#[async_trait::async_trait]
pub trait ControllerDirectory: Send + Sync {
    async fn upsert_registration(&self, registration: ControllerRegistration) -> CoreResult<()>;

    async fn mark_offline(&self, id: &ControllerId, observed_session: &SessionId)
        -> CoreResult<()>;

    async fn list(&self) -> CoreResult<Vec<ControllerRecord>>;

    async fn evict(&self, id: &ControllerId) -> CoreResult<EvictionOutcome>;
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
