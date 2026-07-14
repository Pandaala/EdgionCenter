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
    pub fencing_epoch: u64,
    /// Lease validity established by the adapter. Consumers turn this TTL
    /// into a local monotonic deadline; wall-clock timestamps are never used
    /// for ownership correctness.
    pub valid_for_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenewalOutcome {
    Renewed(Leadership),
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseOutcome {
    Released,
    Lost,
}

#[async_trait::async_trait]
pub trait Coordinator: Send + Sync {
    async fn acquire(&self, role: CoordinationRole) -> CoreResult<Leadership>;

    async fn renew(&self, leadership: &Leadership) -> CoreResult<RenewalOutcome>;

    async fn release(&self, leadership: &Leadership) -> CoreResult<ReleaseOutcome>;
}
