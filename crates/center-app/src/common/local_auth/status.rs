use serde::{Deserialize, Serialize};

/// High-level state of the configured authentication providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    Ready,
    Initializing,
    Disabled,
}

/// Authentication readiness exposed to CLI and dashboard clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatusResponse {
    pub auth_required: bool,
    pub active_providers: Vec<String>,
    pub status: AuthStatus,
}
