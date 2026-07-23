use crate::common::auth::AdminAuthConfig;
use crate::common::config::{AdminTlsConfig, ConfSyncSecurityConfig};
use crate::common::local_auth::LocalAuthConfig;
use edgion_center_adapter_credential_files::MountedCredentialConfig;
use edgion_center_integration_aws_waf::AwsWafConfig;
use edgion_center_integration_cloudflare::{
    CloudflareCredentialInspectionConfig, CloudflareDnsReadConfig, CloudflareDnsWriteConfig,
    CloudflareWafConfig,
};
use edgion_center_integration_cloudfront::CloudFrontAdminConfig;
use edgion_center_integration_route53::{
    Route53DnsReadConfig, Route53DnsWriteConfig, Route53ZoneLifecycleConfig,
};
use serde::{Deserialize, Serialize};

pub use edgion_center_adapter_sql::DatabaseConfig;
pub use edgion_center_core::AuthzMode;
pub use edgion_center_runtime::federation::config::CenterSyncConfig;

/// Authorization-store selector (the *authz* axis, orthogonal to authentication).
///
/// `AllowAll` (default) installs `AllowAllAuthz`: every authenticated caller is
/// treated as a full admin (login = admin). `Rbac` installs the database-backed
/// `DbAuthz`, which resolves each caller's permissions from the `users`/`roles`
/// tables by subject — regardless of which authentication provider issued the
/// token. `Rbac` requires a usable database.
/// Authorization configuration. Selects which `AuthzStore` is installed. This is
/// orthogonal to authentication: any authn provider (OIDC, single-admin, DB
/// users) can be combined with either authz mode.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AuthzConfig {
    /// Authorization mode. Defaults to `allow_all`.
    pub mode: AuthzMode,
}

/// Database-backed user login configuration (one authentication axis).
///
/// When `enabled`, `POST /api/v1/auth/login` additionally authenticates against
/// the `users` table (bcrypt). It coexists with OIDC and the single `local_auth`
/// admin. Requires a usable database and a signing secret (`jwt_secret` here or
/// in `[local_auth]`). `jwt_expiry_hours` / `cookie_secure` override the issued
/// token / cookie settings when no `[local_auth]` admin is configured.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DbAuthConfig {
    /// Enable DB-user login against the `users` table. Default: false.
    pub enabled: bool,
    /// JWT signing secret used to sign/validate DB-user login tokens. Used when
    /// `[local_auth].jwt_secret` is not set. Empty/unset means unconfigured.
    pub jwt_secret: Option<String>,
    /// Token expiry in hours for DB-user logins (when no `[local_auth]` admin
    /// supplies it). Defaults to 24 when unset.
    pub jwt_expiry_hours: Option<u64>,
    /// Emit the `Secure` cookie attribute (when no `[local_auth]` admin supplies
    /// it). Defaults to true when unset.
    pub cookie_secure: Option<bool>,
}

/// Audit-log behavior for mutating admin actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuditConfig {
    /// Whether audit logging is enabled. Requires `database.enabled = true`;
    /// with no database, audit logging is skipped (a WARN is logged at startup).
    pub enabled: bool,
    /// Whether read (GET) requests are also recorded. Mutations are always
    /// recorded when enabled; reads are recorded only when this is true.
    pub log_reads: bool,
    /// Retention window in days for periodic pruning. `0` disables pruning
    /// (records are kept indefinitely).
    pub retention_days: u32,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_reads: false,
            retention_days: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct CenterConfig {
    pub server: CenterServerConfig,
    pub sync: CenterSyncConfig,
    /// SQLite database configuration.
    #[serde(default)]
    pub database: DatabaseConfig,
    /// Optional OIDC Admin API authentication.
    /// Omit to disable OIDC auth. When present, Admin API endpoints (except
    /// health/ready) require a valid JWT Bearer token from the OIDC provider.
    #[serde(default)]
    pub auth: Option<AdminAuthConfig>,
    /// Optional local Admin API authentication.
    /// Center does not inject any default credentials; omit this section only
    /// when OIDC `auth` is configured, otherwise the Admin API will reject all
    /// requests. Both `auth` and `local_auth` can be enabled simultaneously.
    #[serde(default)]
    pub local_auth: Option<LocalAuthConfig>,
    /// TLS config for the federation gRPC server.
    /// Federation requires mTLS; the server refuses to start without a valid TLS block.
    #[serde(default)]
    pub grpc_security: ConfSyncSecurityConfig,
    /// Server-side peer-identity verification. Required (with a non-empty
    /// trust_domain) under mTLS.
    #[serde(default)]
    pub peer_identity: Option<PeerIdentityConfig>,
    /// Dashboard web UI hosting. Controls where the embedded dashboard is served
    /// from; see [`WebConfig`].
    #[serde(default)]
    pub web: WebConfig,
    /// Audit logging of mutating admin actions. See [`AuditConfig`].
    #[serde(default)]
    pub audit: AuditConfig,
    /// Authorization mode (allow_all/rbac). See [`AuthzConfig`].
    #[serde(default)]
    pub authz: AuthzConfig,
    /// Database-backed user login. See [`DbAuthConfig`].
    #[serde(default)]
    pub db_auth: DbAuthConfig,
    /// Deployment-owned mounted credentials. Disabled unless explicitly enabled.
    #[serde(default)]
    pub mounted_credentials: MountedCredentialConfig,
    /// Cloudflare token verification and account-scoped DNS Read probe.
    #[serde(default)]
    pub cloudflare_credential_inspection: CloudflareCredentialInspectionConfig,
    /// Account-bound, read-only Cloudflare DNS inventory. Disabled by default.
    #[serde(default)]
    pub cloudflare_dns_read: CloudflareDnsReadConfig,
    /// Account-bound synchronous Cloudflare DNS writes. Disabled by default.
    #[serde(default)]
    pub cloudflare_dns_write: CloudflareDnsWriteConfig,
    /// Account-bound Cloudflare Zone WAF reads and writes. Both routes are disabled by default.
    #[serde(default)]
    pub cloudflare_waf: CloudflareWafConfig,
    /// Account-bound, read-only Route 53 DNS inventory. Disabled by default.
    #[serde(default)]
    pub route53_dns_read: Route53DnsReadConfig,
    /// Account-bound synchronous Route 53 RRset writes. Disabled by default.
    #[serde(default)]
    pub route53_dns_write: Route53DnsWriteConfig,
    /// Account-bound synchronous Route 53 public hosted-zone lifecycle. Disabled by default.
    #[serde(default)]
    pub route53_zone_lifecycle: Route53ZoneLifecycleConfig,
    /// Account-bound CloudFront Distribution inventory and fixed lifecycle. Disabled by default.
    #[serde(default)]
    pub cloudfront: CloudFrontAdminConfig,
    /// Account-bound AWS WAFv2 inventory and mutation boundary. Disabled by default.
    #[serde(default)]
    pub aws_waf: AwsWafConfig,
}

#[allow(clippy::derivable_impls)]
impl Default for CenterConfig {
    fn default() -> Self {
        Self {
            server: CenterServerConfig::default(),
            sync: CenterSyncConfig::default(),
            database: DatabaseConfig::default(),
            auth: None,
            local_auth: None,
            grpc_security: ConfSyncSecurityConfig::default(),
            peer_identity: None,
            web: WebConfig::default(),
            audit: AuditConfig::default(),
            authz: AuthzConfig::default(),
            db_auth: DbAuthConfig::default(),
            mounted_credentials: MountedCredentialConfig::default(),
            cloudflare_credential_inspection: CloudflareCredentialInspectionConfig::default(),
            cloudflare_dns_read: CloudflareDnsReadConfig::default(),
            cloudflare_dns_write: CloudflareDnsWriteConfig::default(),
            cloudflare_waf: CloudflareWafConfig::default(),
            route53_dns_read: Route53DnsReadConfig::default(),
            route53_dns_write: Route53DnsWriteConfig::default(),
            route53_zone_lifecycle: Route53ZoneLifecycleConfig::default(),
            cloudfront: CloudFrontAdminConfig::default(),
            aws_waf: AwsWafConfig::default(),
        }
    }
}

/// Dashboard web UI hosting configuration.
///
/// Asset-source resolution at runtime (highest precedence first):
/// 1. `EDGION_WEB_DIR` env var → serve that directory from the filesystem.
/// 2. `web.dir` (this field) → serve that directory from the filesystem.
/// 3. neither set → serve the assets embedded in the binary (requires the
///    `embed-dashboard` build feature). When the feature is off and no directory
///    is configured, no UI is served (pure-API mode).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct WebConfig {
    /// Filesystem directory to serve the dashboard from. When unset, the embedded
    /// assets are served. Overridden by the `EDGION_WEB_DIR` environment variable.
    pub dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PeerIdentityConfig {
    /// SPIFFE trust domain expected in the peer cert's URI SAN
    /// (`spiffe://{trust_domain}/controllers/{cluster}/{name}`).
    pub trust_domain: String,
}

#[allow(clippy::derivable_impls)]
impl Default for PeerIdentityConfig {
    fn default() -> Self {
        Self {
            trust_domain: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CenterServerConfig {
    /// gRPC listen address for federation sync
    pub grpc_addr: String,
    /// HTTP listen address for admin API
    pub http_addr: String,
    /// HTTP listen address for the dedicated liveness/readiness probe listener
    pub probe_addr: String,
    /// HTTP listen address for the dedicated Prometheus metrics listener
    pub metrics_addr: String,
    /// Optional server-side TLS for the admin HTTP API.
    /// When omitted, the admin listener serves plain HTTP.
    /// When set, the admin listener serves HTTPS; probe and metrics listeners remain HTTP.
    pub admin_tls: Option<AdminTlsConfig>,
    /// CIDR allowlist for the admin HTTP API. Empty/unset = allow all.
    /// Matched against the TCP peer address only; X-Forwarded-For is never trusted.
    #[serde(default)]
    pub allow_admin_ips: Vec<String>,
    /// L4 TCP-layer allowlist: peers not matching are dropped before the TLS
    /// handshake on the HTTPS admin listener. Independent from `allow_admin_ips`
    /// (L7). Empty/unset = allow all. Matched against the TCP peer address only.
    #[serde(default)]
    pub allow_tcp_ips: Vec<String>,
}

impl Default for CenterServerConfig {
    fn default() -> Self {
        Self {
            grpc_addr: "0.0.0.0:12251".to_string(),
            http_addr: "0.0.0.0:12201".to_string(),
            probe_addr: "0.0.0.0:12200".to_string(),
            metrics_addr: "0.0.0.0:12290".to_string(),
            admin_tls: None,
            allow_admin_ips: Vec::new(),
            allow_tcp_ips: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_center_config_defaults() {
        let config = CenterConfig::default();
        assert_eq!(config.server.grpc_addr, "0.0.0.0:12251");
        assert_eq!(config.server.http_addr, "0.0.0.0:12201");
        assert_eq!(config.server.probe_addr, "0.0.0.0:12200");
        assert_eq!(config.server.metrics_addr, "0.0.0.0:12290");
        assert_eq!(config.sync.ping_interval_secs, 30);
        assert_eq!(config.sync.command_timeout_secs, 30);
    }

    #[test]
    fn center_config_rejects_unknown_top_level_fields() {
        let error = serde_yaml::from_str::<CenterConfig>("unknown_mode_field: true\n")
            .expect_err("unknown fields must fail closed");
        assert!(error.to_string().contains("unknown_mode_field"));
    }

    #[test]
    fn center_config_rejects_unknown_nested_security_fields() {
        for yaml in [
            "authz:\n  modee: rbac\n",
            "server:\n  http_adrr: 127.0.0.1:12201\n",
            "auth:\n  enabledd: false\n",
            "local_auth:\n  enabledd: false\n",
            "sync:\n  ping_interval_secss: 1\n",
            "mounted_credentials:\n  enabledd: true\n",
            "cloudflare_credential_inspection:\n  enabledd: true\n",
            "cloudflare_dns_read:\n  enabledd: true\n",
            "aws_waf:\n  read_enabledd: true\n",
        ] {
            serde_yaml::from_str::<CenterConfig>(yaml)
                .expect_err("unknown nested fields must fail closed");
        }
    }

    #[test]
    fn mounted_credentials_are_default_off_and_parse_strictly_in_production() {
        assert!(!CenterConfig::default().mounted_credentials.enabled);
        let config = parse_via_production(
            "mounted_credentials:\n  enabled: true\n  root_directory: /var/run/edgion-center/cloud-credentials\n  revision_key_file: revision.key\n  bindings:\n    - credential_ref: cloudflare/main\n      provider_account_id: cf-main\n      provider: cloudflare\n      purpose: cloudflare_api_token\n      file: cloudflare/token\n",
        );
        assert!(config.mounted_credentials.enabled);
        assert_eq!(config.mounted_credentials.bindings.len(), 1);
    }

    #[test]
    fn cloudflare_credential_inspection_is_default_off_and_strict() {
        assert!(
            !CenterConfig::default()
                .cloudflare_credential_inspection
                .enabled
        );
        let config = parse_via_production("cloudflare_credential_inspection:\n  enabled: true\n");
        assert!(config.cloudflare_credential_inspection.enabled);
        assert!(serde_yaml::from_str::<CenterConfig>(
            "cloudflare_credential_inspection:\n  base_url: https://example.invalid\n"
        )
        .is_err());
    }

    #[test]
    fn cloudflare_dns_read_is_default_off_and_strict() {
        assert!(!CenterConfig::default().cloudflare_dns_read.enabled);
        let config = parse_via_production(
            "cloudflare_dns_read:\n  enabled: true\n  cursor_key_ref: cloudflare/dns-cursor-a\n  cursor_fallback_key_ref: cloudflare/dns-cursor-b\n  cursor_max_lifetime_secs: 900\n  cursor_clock_skew_secs: 30\n  operation_timeout_secs: 30\n  global_concurrency: 16\n  per_account_concurrency: 2\n",
        );
        assert!(config.cloudflare_dns_read.enabled);
        assert_eq!(
            config
                .cloudflare_dns_read
                .cursor_fallback_key_ref
                .as_deref(),
            Some("cloudflare/dns-cursor-b")
        );
        assert_eq!(config.cloudflare_dns_read.cursor_max_lifetime_secs, 900);
        assert_eq!(config.cloudflare_dns_read.cursor_clock_skew_secs, 30);
        assert_eq!(config.cloudflare_dns_read.operation_timeout_secs, 30);
    }

    #[test]
    fn cloudflare_dns_write_is_default_off_and_strict() {
        assert!(!CenterConfig::default().cloudflare_dns_write.enabled);
        let config = parse_via_production(
            "cloudflare_dns_write:\n  enabled: true\n  operation_timeout_secs: 30\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        );
        assert!(config.cloudflare_dns_write.enabled);
        assert_eq!(config.cloudflare_dns_write.operation_timeout_secs, 30);
        assert_eq!(config.cloudflare_dns_write.global_concurrency, 4);
        assert_eq!(config.cloudflare_dns_write.per_account_concurrency, 1);
    }

    #[test]
    fn cloudflare_waf_routes_are_independently_default_off_and_strict() {
        let default = CenterConfig::default();
        assert!(!default.cloudflare_waf.read_enabled);
        assert!(!default.cloudflare_waf.write_enabled);
        let config: CenterConfig = serde_yaml::from_str(
            "cloudflare_waf:\n  read_enabled: false\n  write_enabled: true\n  operation_timeout_secs: 30\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(!config.cloudflare_waf.read_enabled);
        assert!(config.cloudflare_waf.write_enabled);
    }

    #[test]
    fn route53_dns_read_is_default_off_and_strict() {
        assert!(!CenterConfig::default().route53_dns_read.enabled);
        let config = parse_via_production(
            "route53_dns_read:\n  enabled: true\n  cursor_key_ref: aws/route53-dns-cursor\n  operation_timeout_secs: 60\n  global_concurrency: 8\n  per_account_concurrency: 2\n",
        );
        assert!(config.route53_dns_read.enabled);
        assert_eq!(
            config.route53_dns_read.cursor_key_ref.as_deref(),
            Some("aws/route53-dns-cursor")
        );
        assert!(serde_yaml::from_str::<CenterConfig>(
            "route53_dns_read:\n  enabled: true\n  endpoint_url: https://example.invalid\n"
        )
        .is_err());
    }

    #[test]
    fn route53_dns_write_is_default_off_and_strict() {
        assert!(!CenterConfig::default().route53_dns_write.enabled);
        let config = parse_via_production(
            "route53_dns_write:\n  enabled: true\n  cursor_key_ref: aws/route53-dns-cursor\n  mutation_receipt_key_ref: aws/route53-dns-mutation\n  operation_timeout_secs: 60\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        );
        assert!(config.route53_dns_write.enabled);
        assert_eq!(
            config.route53_dns_write.mutation_receipt_key_ref.as_deref(),
            Some("aws/route53-dns-mutation")
        );
        assert!(serde_yaml::from_str::<CenterConfig>(
            "route53_dns_write:\n  enabled: true\n  endpoint_url: https://example.invalid\n"
        )
        .is_err());
    }

    #[test]
    fn route53_zone_lifecycle_is_default_off_and_strict() {
        assert!(!CenterConfig::default().route53_zone_lifecycle.enabled);
        let config = parse_via_production(
            "route53_zone_lifecycle:\n  enabled: true\n  cursor_key_ref: aws/route53-dns-cursor\n  lifecycle_token_key_ref: aws/route53-zone-lifecycle\n  operation_timeout_secs: 60\n  global_concurrency: 2\n  per_account_concurrency: 1\n",
        );
        assert!(config.route53_zone_lifecycle.enabled);
        assert_eq!(
            config
                .route53_zone_lifecycle
                .lifecycle_token_key_ref
                .as_deref(),
            Some("aws/route53-zone-lifecycle")
        );
        assert!(serde_yaml::from_str::<CenterConfig>(
            "route53_zone_lifecycle:\n  enabled: true\n  endpoint_url: https://example.invalid\n"
        )
        .is_err());
    }

    #[test]
    fn cloudfront_is_default_off_and_strict() {
        assert!(!CenterConfig::default().cloudfront.read_enabled);
        assert!(!CenterConfig::default().cloudfront.write_enabled);
        let config = parse_via_production(
            "cloudfront:\n  read_enabled: true\n  write_enabled: true\n  fingerprint_key_ref: aws/cloudfront-fingerprint\n  operation_timeout_secs: 60\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        );
        assert!(config.cloudfront.read_enabled);
        assert!(config.cloudfront.write_enabled);
        assert_eq!(
            config.cloudfront.fingerprint_key_ref.as_deref(),
            Some("aws/cloudfront-fingerprint")
        );
        assert!(serde_yaml::from_str::<CenterConfig>(
            "cloudfront:\n  read_enabled: true\n  endpoint_url: https://example.invalid\n"
        )
        .is_err());
    }

    #[test]
    fn aws_waf_capabilities_are_independently_default_off_and_strict() {
        let default = CenterConfig::default();
        assert!(!default.aws_waf.read_enabled);
        assert!(!default.aws_waf.write_enabled);
        assert!(!default.aws_waf.attach_enabled);
        assert!(!default.aws_waf.detach_enabled);
        assert!(!default.aws_waf.security_weaken_enabled);

        let config = parse_via_production(
            "aws_waf:\n  attach_enabled: true\n  ownership_hmac_key_ref: aws/waf-owner\n  operation_timeout_secs: 60\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        );
        assert!(config.aws_waf.attach_enabled);
        assert!(!config.aws_waf.write_enabled);
        assert_eq!(
            config.aws_waf.ownership_hmac_key_ref.as_deref(),
            Some("aws/waf-owner")
        );
        assert!(serde_yaml::from_str::<CenterConfig>(
            "aws_waf:\n  attach_enabled: true\n  ownership_hmac_key_reff: aws/waf-owner\n"
        )
        .is_err());
    }

    #[test]
    fn test_center_config_parses_from_yaml() {
        let yaml = r#"
server:
  grpc_addr: "0.0.0.0:50100"
  http_addr: "0.0.0.0:5900"
sync:
  ping_interval_secs: 60
"#;
        let config: CenterConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.server.grpc_addr, "0.0.0.0:50100");
        assert_eq!(config.sync.ping_interval_secs, 60);
        assert_eq!(config.sync.command_timeout_secs, 30); // default
    }

    #[test]
    fn audit_config_defaults() {
        let c = CenterConfig::default();
        assert!(c.audit.enabled, "audit enabled by default");
        assert!(!c.audit.log_reads, "reads excluded by default");
        assert_eq!(c.audit.retention_days, 0, "retention disabled by default");
    }

    #[test]
    fn audit_config_parses_from_yaml() {
        let yaml = r#"
audit:
  enabled: false
  log_reads: true
  retention_days: 30
"#;
        let c: CenterConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!c.audit.enabled);
        assert!(c.audit.log_reads);
        assert_eq!(c.audit.retention_days, 30);
    }

    #[test]
    fn audit_config_absent_uses_defaults() {
        // Omitting the whole [audit] section must yield the documented defaults.
        let yaml = "server:\n  http_addr: \"0.0.0.0:5900\"\n";
        let c: CenterConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(c.audit.enabled);
        assert!(!c.audit.log_reads);
        assert_eq!(c.audit.retention_days, 0);
    }

    /// Helper: deserialize a YAML doc through the SAME path production uses
    /// (`singleton_map_recursive`), not the plain `serde_yaml::from_str`.
    fn parse_via_production(yaml: &str) -> CenterConfig {
        let de = serde_yaml::Deserializer::from_str(yaml);
        serde_yaml::with::singleton_map_recursive::deserialize(de)
            .expect("config must parse through the production singleton_map_recursive path")
    }

    #[test]
    fn authz_mode_defaults_allow_all() {
        // Default value and an omitted [authz] section both yield allow_all.
        assert_eq!(CenterConfig::default().authz.mode, AuthzMode::AllowAll);
        let c = parse_via_production("server:\n  http_addr: \"0.0.0.0:5900\"\n");
        assert_eq!(c.authz.mode, AuthzMode::AllowAll);
    }

    #[test]
    fn authz_mode_rbac_parses() {
        let c = parse_via_production("authz:\n  mode: rbac\n");
        assert_eq!(c.authz.mode, AuthzMode::Rbac);
    }

    #[test]
    fn db_auth_defaults_disabled() {
        // Default value and an omitted [db_auth] section both yield disabled.
        let d = CenterConfig::default();
        assert!(!d.db_auth.enabled);
        assert!(d.db_auth.jwt_secret.is_none());
        let c = parse_via_production("server:\n  http_addr: \"0.0.0.0:5900\"\n");
        assert!(!c.db_auth.enabled);
    }

    #[test]
    fn db_auth_enabled_parses() {
        let yaml = r#"
db_auth:
  enabled: true
  jwt_secret: "a_long_enough_jwt_secret_value_abcdef"
  jwt_expiry_hours: 8
  cookie_secure: false
"#;
        let c = parse_via_production(yaml);
        assert!(c.db_auth.enabled);
        assert_eq!(
            c.db_auth.jwt_secret.as_deref(),
            Some("a_long_enough_jwt_secret_value_abcdef")
        );
        assert_eq!(c.db_auth.jwt_expiry_hours, Some(8));
        assert_eq!(c.db_auth.cookie_secure, Some(false));
    }

    #[test]
    fn default_peer_identity_is_none() {
        let c = CenterConfig::default();
        assert!(c.peer_identity.is_none());
    }

    #[test]
    fn parses_peer_identity() {
        let yaml = r#"
peer_identity:
  trust_domain: "edgion.io"
"#;
        let c: CenterConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(c.peer_identity.unwrap().trust_domain, "edgion.io");
    }

    #[test]
    fn test_admin_tls_deserializes_from_yaml() {
        let yaml = r#"
server:
  http_addr: "0.0.0.0:5900"
  admin_tls:
    cert: "certs/admin/server.crt"
    key: "certs/admin/server.key"
"#;
        let cfg: CenterConfig = serde_yaml::from_str(yaml).unwrap();
        let tls = cfg.server.admin_tls.expect("admin_tls should parse");
        assert_eq!(tls.cert, "certs/admin/server.crt");
        assert_eq!(tls.key, "certs/admin/server.key");
    }

    #[test]
    fn test_admin_tls_absent_is_none() {
        let yaml = r#"
server:
  http_addr: "0.0.0.0:5900"
"#;
        let cfg: CenterConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(
            cfg.server.admin_tls.is_none(),
            "omitted admin_tls must be None"
        );
    }

    /// Parity guard: the production config load (`EdgionCenterCli::run`) does NOT use
    /// `serde_yaml::from_str` — it uses `singleton_map_recursive::deserialize`, which
    /// changes how enums/maps are treated. The other `admin_tls` tests use `from_str`,
    /// so this test exercises the REAL production deserializer to ensure
    /// `Option<AdminTlsConfig>` round-trips identically (Some with correct fields when
    /// present, None when absent).
    #[test]
    fn test_admin_tls_parses_via_production_singleton_deserializer() {
        let yaml = r#"
server:
  http_addr: "0.0.0.0:5900"
  admin_tls:
    cert: "certs/admin/server.crt"
    key: "certs/admin/server.key"
"#;
        let de = serde_yaml::Deserializer::from_str(yaml);
        let cfg: CenterConfig = serde_yaml::with::singleton_map_recursive::deserialize(de)
            .expect("admin_tls must parse through the production singleton_map_recursive path");
        let tls = cfg
            .server
            .admin_tls
            .expect("admin_tls Some via singleton_map_recursive");
        assert_eq!(tls.cert, "certs/admin/server.crt");
        assert_eq!(tls.key, "certs/admin/server.key");

        let de_absent =
            serde_yaml::Deserializer::from_str("server:\n  http_addr: \"0.0.0.0:5900\"\n");
        let cfg_absent: CenterConfig =
            serde_yaml::with::singleton_map_recursive::deserialize(de_absent).unwrap();
        assert!(
            cfg_absent.server.admin_tls.is_none(),
            "omitted admin_tls must be None via singleton_map_recursive"
        );
    }
}

#[cfg(test)]
mod allow_tcp_ips_tests {
    use super::*;

    #[test]
    fn default_allow_tcp_ips_is_empty() {
        let c = CenterServerConfig::default();
        assert!(c.allow_tcp_ips.is_empty());
    }

    #[test]
    fn yaml_without_field_defaults_to_empty() {
        // Omitting allow_tcp_ips must deserialize to an empty Vec (backward compat).
        let yaml = "grpc_addr: \"0.0.0.0:1\"\nhttp_addr: \"0.0.0.0:2\"\nprobe_addr: \"0.0.0.0:3\"\nmetrics_addr: \"0.0.0.0:4\"\n";
        let c: CenterServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(c.allow_tcp_ips.is_empty());
    }

    #[test]
    fn yaml_with_field_parses() {
        let yaml = "allow_tcp_ips:\n  - \"10.0.0.0/8\"\n";
        let c: CenterServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(c.allow_tcp_ips, vec!["10.0.0.0/8".to_string()]);
    }
}
