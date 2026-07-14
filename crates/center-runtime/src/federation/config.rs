use serde::{Deserialize, Serialize};

/// Runtime tuning for federation command and heartbeat behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CenterSyncConfig {
    pub command_timeout_secs: u64,
    pub ping_interval_secs: u64,
}

impl Default for CenterSyncConfig {
    fn default() -> Self {
        Self {
            command_timeout_secs: 30,
            ping_interval_secs: 30,
        }
    }
}
