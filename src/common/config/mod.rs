//! Minimal config subset extracted from the Edgion monorepo's
//! `core::common::config`. Center only needs the conf_sync channel security
//! types and the admin-TLS type; the full controller/gateway config tree is
//! intentionally NOT pulled in here.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// conf_sync gRPC channel security configuration.
/// The entire section is optional; omitting it = no security enhancement (backward compatible).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConfSyncSecurityConfig {
    /// Name of the active certificate set. Must match `name` of one entry in `certs`.
    pub active: Option<String>,
    /// Certificate sets (YAML `conf_sync_security.certs` list).
    pub certs: Vec<ConfSyncTlsConfig>,
    /// Legacy single-cert format [conf_sync_security.tls] — kept for backward compat.
    pub tls: Option<ConfSyncTlsConfig>,
    // Reserved: token authentication can be added here later
}

impl ConfSyncSecurityConfig {
    /// Resolve the active TLS config.
    ///
    /// Priority:
    /// 1. `certs` array: find entry where `name == active`
    /// 2. `certs` has exactly one entry and `active` is None: use it
    /// 3. Fallback to legacy `tls` field
    /// 4. `None` = plaintext gRPC
    pub fn resolve_active_tls(&self) -> Option<&ConfSyncTlsConfig> {
        if !self.certs.is_empty() {
            if let Some(active) = &self.active {
                return self.certs.iter().find(|c| c.name.as_deref() == Some(active.as_str()));
            }
            if self.certs.len() == 1 {
                return self.certs.first();
            }
            return None;
        }
        self.tls.as_ref()
    }

    /// Federation requires real mTLS. Returns the active cert set only when one
    /// is configured AND `skip_tls` is not set; otherwise refuses with a reason.
    /// Plaintext is never representable through this path.
    ///
    /// Precondition: assumes `validate()` has already passed — it does NOT
    /// re-check cert path emptiness.
    pub fn resolve_mtls_or_refuse(&self) -> Result<&ConfSyncTlsConfig, MtlsRefusal> {
        match self.resolve_active_tls() {
            None => Err(MtlsRefusal::NoTls),
            Some(t) if t.skip_tls => Err(MtlsRefusal::SkipTls),
            Some(t) => Ok(t),
        }
    }

    /// Validate configuration at startup.
    ///
    /// Returns error for:
    /// - duplicate names in `certs`
    /// - multiple certs without `active`
    /// - `active` pointing to nonexistent name
    /// - cert path fields empty on active cert (unless `skip_tls`)
    pub fn validate(&self) -> anyhow::Result<()> {
        // Check duplicate names
        let mut seen = std::collections::HashSet::new();
        for cert in &self.certs {
            if let Some(name) = &cert.name {
                if !seen.insert(name.as_str()) {
                    anyhow::bail!("conf_sync_security: duplicate cert name '{}'", name);
                }
            }
        }
        // Multiple certs require active selector
        if self.certs.len() > 1 && self.active.is_none() {
            anyhow::bail!("conf_sync_security: multiple certs configured but no 'active' selector");
        }
        // active must match an existing name
        if let Some(active) = &self.active {
            if !self.certs.is_empty() {
                let found = self.certs.iter().any(|c| c.name.as_deref() == Some(active.as_str()));
                if !found {
                    anyhow::bail!("conf_sync_security: active='{}' does not match any cert name", active);
                }
            }
        }
        // Validate the active cert's paths (skip if skip_tls)
        if let Some(tls) = self.resolve_active_tls() {
            if !tls.skip_tls {
                tls.validate()?;
            }
        }
        Ok(())
    }
}

/// mTLS certificate file paths for conf_sync gRPC channel.
#[derive(Clone, Serialize, Deserialize)]
pub struct ConfSyncTlsConfig {
    /// Unique name for this cert set. Required when using [[certs]] array format.
    #[serde(default)]
    pub name: Option<String>,
    /// Certificate file path (relative to work_dir or absolute)
    pub cert: String,
    /// Private key file path
    pub key: String,
    /// CA certificate path (used to verify the peer)
    pub ca: String,
    /// Emergency bypass: skip TLS and start in plaintext with ERROR log.
    /// Only for emergency use during cert rotation failures. Must not stay enabled.
    #[serde(default)]
    pub skip_tls: bool,
}

impl std::fmt::Debug for ConfSyncTlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfSyncTlsConfig")
            .field("name", &self.name)
            .field("cert", &self.cert)
            .field("key", &"***")
            .field("ca", &self.ca)
            .field("skip_tls", &self.skip_tls)
            .finish()
    }
}

impl ConfSyncTlsConfig {
    /// Validate that all required paths are non-empty.
    /// Called before attempting to load certificates.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.cert.trim().is_empty() {
            anyhow::bail!("conf_sync_security.tls.cert must not be empty");
        }
        if self.key.trim().is_empty() {
            anyhow::bail!("conf_sync_security.tls.key must not be empty");
        }
        if self.ca.trim().is_empty() {
            anyhow::bail!("conf_sync_security.tls.ca must not be empty");
        }
        Ok(())
    }

    /// Resolve a path: absolute paths used as-is, relative paths joined with work_dir.
    fn resolve_path(path: &str) -> PathBuf {
        edgion_resources::work_dir().resolve(path)
    }

    pub fn cert_path(&self) -> PathBuf {
        Self::resolve_path(&self.cert)
    }

    pub fn key_path(&self) -> PathBuf {
        Self::resolve_path(&self.key)
    }

    pub fn ca_path(&self) -> PathBuf {
        Self::resolve_path(&self.ca)
    }
}

/// Server-side TLS certificate file paths for the admin HTTP API.
///
/// Unlike `ConfSyncTlsConfig` (mTLS — requires `ca`), this is server-only TLS:
/// only `cert` + `key` are needed, with no client-certificate verification.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminTlsConfig {
    /// PEM server certificate path (relative to work_dir or absolute).
    pub cert: String,
    /// PEM private key path (relative to work_dir or absolute).
    pub key: String,
}

impl std::fmt::Debug for AdminTlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminTlsConfig")
            .field("cert", &self.cert)
            .field("key", &"***")
            .finish()
    }
}

impl AdminTlsConfig {
    /// Validate that the cert and key paths are non-empty.
    /// Called before attempting to load certificates at startup.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.cert.trim().is_empty() {
            anyhow::bail!("admin_tls.cert must not be empty");
        }
        if self.key.trim().is_empty() {
            anyhow::bail!("admin_tls.key must not be empty");
        }
        Ok(())
    }

    /// Resolve a path: absolute paths used as-is, relative paths joined with work_dir.
    fn resolve_path(path: &str) -> PathBuf {
        edgion_resources::work_dir().resolve(path)
    }

    /// Resolve the cert path: absolute as-is, relative joined with work_dir.
    pub fn cert_path(&self) -> PathBuf {
        Self::resolve_path(&self.cert)
    }

    /// Resolve the key path: absolute as-is, relative joined with work_dir.
    pub fn key_path(&self) -> PathBuf {
        Self::resolve_path(&self.key)
    }
}

/// Why a federation channel refused the configured security.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtlsRefusal {
    /// resolve_active_tls() == None — no TLS block configured.
    NoTls,
    /// The active cert set has skip_tls = true.
    SkipTls,
}
