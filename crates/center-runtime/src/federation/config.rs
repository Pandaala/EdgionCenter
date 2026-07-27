use serde::{Deserialize, Serialize};

/// Runtime tuning for federation proxied-request and heartbeat behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CenterSyncConfig {
    /// Deadline for a single proxied request forwarded to a Controller over
    /// the federation tunnel (HTTP proxy, reload included). The field name
    /// predates the proxy-only tunnel; it is kept to avoid a config-format
    /// migration.
    pub command_timeout_secs: u64,
    pub ping_interval_secs: u64,
}

impl Default for CenterSyncConfig {
    fn default() -> Self {
        Self {
            // Below the dashboard's 30 s axios timeout so a slow Controller
            // surfaces as a Center 504, not a browser-side abort.
            command_timeout_secs: 25,
            ping_interval_secs: 30,
        }
    }
}
