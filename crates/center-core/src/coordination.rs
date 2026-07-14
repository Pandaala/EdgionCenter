use serde::{Deserialize, Serialize};

use crate::CoreResult;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationRole {
    ControllerOwner(String),
    Maintenance(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leadership {
    pub role: CoordinationRole,
    pub holder: String,
    pub fencing_token: String,
    pub expires_at_unix_ms: Option<i64>,
}

#[async_trait::async_trait]
pub trait Coordinator: Send + Sync {
    async fn acquire(&self, role: CoordinationRole) -> CoreResult<Leadership>;
}
