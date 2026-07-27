use edgion_center_adapter_credential_files::MountedCredentialConfig;
use edgion_center_app::common::{
    auth::AdminAuthConfig,
    config::{ConfSyncSecurityConfig, ConfSyncTlsConfig},
};
use edgion_center_integration_aws_waf::AwsWafConfig;
use edgion_center_integration_cloudflare::{
    CloudflareCredentialInspectionConfig, CloudflareDnsReadConfig, CloudflareDnsWriteConfig,
    CloudflareWafConfig,
};
use edgion_center_integration_cloudfront::CloudFrontAdminConfig;
use edgion_center_integration_route53::{
    Route53DnsReadConfig, Route53DnsWriteConfig, Route53ZoneLifecycleConfig,
};
use edgion_center_runtime::federation::config::CenterSyncConfig;
use serde::{Deserialize, Serialize};

fn default_http_addr() -> String {
    "0.0.0.0:12201".to_string()
}
fn default_grpc_addr() -> String {
    "0.0.0.0:12251".to_string()
}
fn default_probe_addr() -> String {
    "0.0.0.0:12200".to_string()
}
fn default_metrics_addr() -> String {
    "0.0.0.0:12290".to_string()
}
fn default_lease_duration() -> u64 {
    15
}
fn default_internal_addr() -> String {
    "0.0.0.0:12252".to_string()
}
fn default_internal_port() -> u16 {
    12252
}
fn default_internal_request_bytes() -> usize {
    4 * 1024 * 1024
}
fn default_internal_response_bytes() -> usize {
    16 * 1024 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InternalForwardingConfig {
    pub bind_addr: String,
    pub port: u16,
    pub server_name: String,
    pub expected_peer_spiffe_id: String,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub tls: Option<ConfSyncTlsConfig>,
}

impl Default for InternalForwardingConfig {
    fn default() -> Self {
        Self {
            bind_addr: default_internal_addr(),
            port: default_internal_port(),
            server_name: String::new(),
            expected_peer_spiffe_id: String::new(),
            max_request_bytes: default_internal_request_bytes(),
            max_response_bytes: default_internal_response_bytes(),
            tls: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KubernetesServerConfig {
    pub http_addr: String,
    pub grpc_addr: String,
    pub probe_addr: String,
    pub metrics_addr: String,
}

impl Default for KubernetesServerConfig {
    fn default() -> Self {
        Self {
            http_addr: default_http_addr(),
            grpc_addr: default_grpc_addr(),
            probe_addr: default_probe_addr(),
            metrics_addr: default_metrics_addr(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct KubernetesWebConfig {
    pub dir: Option<String>,
}

/// Strict, database-free composition configuration. Namespace and replica
/// identity come from the Downward API so a copied ConfigMap cannot make two
/// replicas share a fencing identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KubernetesCenterConfig {
    pub server: KubernetesServerConfig,
    pub sync: CenterSyncConfig,
    pub auth: AdminAuthConfig,
    pub grpc_security: ConfSyncSecurityConfig,
    pub trust_domain: String,
    pub lease_duration_secs: u64,
    pub web: KubernetesWebConfig,
    pub audit_log_reads: bool,
    pub internal_forwarding: InternalForwardingConfig,
    /// Deployment-owned mounted credentials. Disabled unless explicitly enabled.
    pub mounted_credentials: MountedCredentialConfig,
    /// Cloudflare token verification and account-scoped DNS Read probe.
    pub cloudflare_credential_inspection: CloudflareCredentialInspectionConfig,
    /// Account-bound, read-only Cloudflare DNS inventory. Disabled by default.
    pub cloudflare_dns_read: CloudflareDnsReadConfig,
    /// Account-bound synchronous Cloudflare DNS writes. Disabled by default.
    pub cloudflare_dns_write: CloudflareDnsWriteConfig,
    /// Account-bound Cloudflare Zone WAF reads and writes. Both routes are disabled by default.
    pub cloudflare_waf: CloudflareWafConfig,
    /// Account-bound, read-only Route 53 DNS inventory. Disabled by default.
    pub route53_dns_read: Route53DnsReadConfig,
    /// Account-bound synchronous Route 53 RRset writes. Disabled by default.
    pub route53_dns_write: Route53DnsWriteConfig,
    /// Account-bound synchronous Route 53 public hosted-zone lifecycle. Disabled by default.
    pub route53_zone_lifecycle: Route53ZoneLifecycleConfig,
    /// Account-bound CloudFront Distribution inventory and fixed lifecycle. Disabled by default.
    pub cloudfront: CloudFrontAdminConfig,
    /// Account-bound AWS WAFv2 inventory and mutation boundary. Disabled by default.
    pub aws_waf: AwsWafConfig,
}

impl Default for KubernetesCenterConfig {
    fn default() -> Self {
        Self {
            server: KubernetesServerConfig::default(),
            sync: CenterSyncConfig::default(),
            auth: AdminAuthConfig::default(),
            grpc_security: ConfSyncSecurityConfig::default(),
            trust_domain: String::new(),
            lease_duration_secs: default_lease_duration(),
            web: KubernetesWebConfig::default(),
            audit_log_reads: false,
            internal_forwarding: InternalForwardingConfig::default(),
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

#[derive(Debug)]
pub struct RuntimeIdentity {
    pub namespace: String,
    pub holder: String,
}

impl KubernetesCenterConfig {
    pub fn validate(&self) -> anyhow::Result<RuntimeIdentity> {
        if !self.auth.enabled {
            anyhow::bail!("Kubernetes mode requires auth.enabled=true (OIDC)");
        }
        if let Some(error) = self.auth.validate() {
            anyhow::bail!("invalid auth config: {error}");
        }
        if !self.auth.discovery.starts_with("https://") {
            anyhow::bail!("Kubernetes mode requires an https:// OIDC discovery URL");
        }
        if !self.auth.ssl_verify {
            anyhow::bail!("Kubernetes mode requires OIDC TLS certificate verification");
        }
        self.grpc_security.validate()?;
        self.grpc_security
            .resolve_mtls_or_refuse()
            .map_err(|error| anyhow::anyhow!("Kubernetes federation requires mTLS: {error:?}"))?;
        let internal_tls = self
            .internal_forwarding
            .tls
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("internal_forwarding.tls is required"))?;
        internal_tls.validate()?;
        if internal_tls.skip_tls {
            anyhow::bail!("internal_forwarding TLS cannot be disabled");
        }
        if self.internal_forwarding.server_name.trim().is_empty() {
            anyhow::bail!("internal_forwarding.server_name must not be empty");
        }
        if !self
            .internal_forwarding
            .expected_peer_spiffe_id
            .starts_with("spiffe://")
        {
            anyhow::bail!("internal_forwarding.expected_peer_spiffe_id must be a SPIFFE URI");
        }
        if self.internal_forwarding.port == 0 {
            anyhow::bail!("internal_forwarding.port must not be zero");
        }
        if self.internal_forwarding.max_request_bytes == 0
            || self.internal_forwarding.max_response_bytes
                < self.internal_forwarding.max_request_bytes
        {
            anyhow::bail!("internal_forwarding message limits are invalid");
        }
        let federation_tls = self
            .grpc_security
            .resolve_mtls_or_refuse()
            .expect("validated above");
        if federation_tls.ca_path() == internal_tls.ca_path() {
            anyhow::bail!("internal_forwarding must use a CA separate from federation mTLS");
        }
        if self.trust_domain.trim().is_empty() {
            anyhow::bail!("trust_domain must not be empty");
        }
        if !(5..=300).contains(&self.lease_duration_secs) {
            anyhow::bail!("lease_duration_secs must be between 5 and 300");
        }
        for (name, value) in [
            ("http_addr", &self.server.http_addr),
            ("grpc_addr", &self.server.grpc_addr),
            ("probe_addr", &self.server.probe_addr),
            ("metrics_addr", &self.server.metrics_addr),
            (
                "internal_forwarding.bind_addr",
                &self.internal_forwarding.bind_addr,
            ),
        ] {
            value
                .parse::<std::net::SocketAddr>()
                .map_err(|error| anyhow::anyhow!("invalid {name}: {error}"))?;
        }
        let namespace = std::env::var("POD_NAMESPACE")
            .map_err(|_| anyhow::anyhow!("POD_NAMESPACE must be supplied by the Downward API"))?;
        if namespace.trim().is_empty() {
            anyhow::bail!("POD_NAMESPACE must not be empty");
        }
        let pod_uid = std::env::var("POD_UID")
            .map_err(|_| anyhow::anyhow!("POD_UID must be supplied by the Downward API"))?;
        if pod_uid.trim().is_empty() {
            anyhow::bail!("POD_UID must not be empty");
        }
        let pod_name = std::env::var("POD_NAME")
            .map_err(|_| anyhow::anyhow!("POD_NAME must be supplied by the Downward API"))?;
        if pod_name.trim().is_empty() {
            anyhow::bail!("POD_NAME must not be empty");
        }
        Ok(RuntimeIdentity {
            namespace,
            holder: format!("{pod_name}/{pod_uid}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_config(manifest: &str) -> KubernetesCenterConfig {
        let manifest: serde_yaml::Value = serde_yaml::from_str(manifest).unwrap();
        let config = manifest["data"]["config.yaml"]
            .as_str()
            .expect("ConfigMap data.config.yaml");
        serde_yaml::from_str(config).expect("example must match the strict runtime schema")
    }

    #[test]
    fn retained_cloud_deployment_examples_parse_with_the_runtime_schema() {
        let aws = example_config(include_str!(
            "../../../cicd/deploy/examples/aws-route53-ambient/config.yaml"
        ));
        assert!(aws.route53_dns_read.enabled);
        assert!(aws.route53_dns_write.enabled);
        assert!(aws.route53_zone_lifecycle.enabled);
        assert!(aws.cloudfront.read_enabled && aws.cloudfront.write_enabled);
        assert!(aws.aws_waf.read_enabled && aws.aws_waf.write_enabled);

        let cloudflare = example_config(include_str!(
            "../../../cicd/deploy/examples/cloudflare-mounted-credentials/config.yaml"
        ));
        assert!(cloudflare.cloudflare_dns_read.enabled);
        assert!(cloudflare.cloudflare_dns_write.enabled);
        assert!(cloudflare.cloudflare_waf.read_enabled);
    }

    #[test]
    fn unknown_fields_fail_closed() {
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>("database: {}\n").is_err());
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "mounted_credentials:\n  enabledd: true\n"
        )
        .is_err());
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "cloudflare_credential_inspection:\n  base_url: https://example.invalid\n"
        )
        .is_err());
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "cloudflare_dns_read:\n  enabledd: true\n"
        )
        .is_err());
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "cloudflare_waf:\n  read_enabledd: true\n"
        )
        .is_err());
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "aws_waf:\n  security_weaken_enabledd: true\n"
        )
        .is_err());
    }

    #[test]
    fn mounted_credentials_are_default_off_and_parse_strictly() {
        assert!(
            !KubernetesCenterConfig::default()
                .mounted_credentials
                .enabled
        );
        let config: KubernetesCenterConfig = serde_yaml::from_str(
            "mounted_credentials:\n  enabled: true\n  root_directory: /var/run/edgion-center/cloud-credentials\n  revision_key_file: revision.key\n  bindings:\n    - credential_ref: cloudflare/main\n      provider_account_id: cf-main\n      provider: cloudflare\n      purpose: cloudflare_api_token\n      file: cloudflare/token\n",
        )
        .unwrap();
        assert!(config.mounted_credentials.enabled);
        assert_eq!(config.mounted_credentials.bindings.len(), 1);
    }

    #[test]
    fn cloudflare_credential_inspection_is_default_off_and_strict() {
        assert!(
            !KubernetesCenterConfig::default()
                .cloudflare_credential_inspection
                .enabled
        );
        let config: KubernetesCenterConfig =
            serde_yaml::from_str("cloudflare_credential_inspection:\n  enabled: true\n").unwrap();
        assert!(config.cloudflare_credential_inspection.enabled);
    }

    #[test]
    fn cloudflare_dns_read_is_default_off_and_strict() {
        assert!(
            !KubernetesCenterConfig::default()
                .cloudflare_dns_read
                .enabled
        );
        let config: KubernetesCenterConfig = serde_yaml::from_str(
            "cloudflare_dns_read:\n  enabled: true\n  cursor_key_ref: cloudflare/dns-cursor-a\n  cursor_fallback_key_ref: cloudflare/dns-cursor-b\n  cursor_max_lifetime_secs: 900\n  cursor_clock_skew_secs: 30\n  operation_timeout_secs: 30\n  global_concurrency: 16\n  per_account_concurrency: 2\n",
        )
        .unwrap();
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
    }

    #[test]
    fn cloudflare_dns_write_is_default_off_and_strict() {
        assert!(
            !KubernetesCenterConfig::default()
                .cloudflare_dns_write
                .enabled
        );
        let config: KubernetesCenterConfig = serde_yaml::from_str(
            "cloudflare_dns_write:\n  enabled: true\n  operation_timeout_secs: 30\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(config.cloudflare_dns_write.enabled);
        assert_eq!(config.cloudflare_dns_write.operation_timeout_secs, 30);
        assert_eq!(config.cloudflare_dns_write.global_concurrency, 4);
        assert_eq!(config.cloudflare_dns_write.per_account_concurrency, 1);
    }

    #[test]
    fn cloudflare_waf_routes_are_independently_default_off_and_strict() {
        let default = KubernetesCenterConfig::default();
        assert!(!default.cloudflare_waf.read_enabled);
        assert!(!default.cloudflare_waf.write_enabled);
        let config: KubernetesCenterConfig = serde_yaml::from_str(
            "cloudflare_waf:\n  read_enabled: true\n  write_enabled: false\n  operation_timeout_secs: 30\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(config.cloudflare_waf.read_enabled);
        assert!(!config.cloudflare_waf.write_enabled);
    }

    #[test]
    fn route53_dns_read_is_default_off_and_strict() {
        assert!(!KubernetesCenterConfig::default().route53_dns_read.enabled);
        let config: KubernetesCenterConfig = serde_yaml::from_str(
            "route53_dns_read:\n  enabled: true\n  cursor_key_ref: aws/route53-dns-cursor\n  operation_timeout_secs: 60\n  global_concurrency: 8\n  per_account_concurrency: 2\n",
        )
        .unwrap();
        assert!(config.route53_dns_read.enabled);
        assert_eq!(
            config.route53_dns_read.cursor_key_ref.as_deref(),
            Some("aws/route53-dns-cursor")
        );
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "route53_dns_read:\n  enabled: true\n  endpoint_url: https://example.invalid\n"
        )
        .is_err());
    }

    #[test]
    fn route53_dns_write_is_default_off_and_strict() {
        assert!(!KubernetesCenterConfig::default().route53_dns_write.enabled);
        let config: KubernetesCenterConfig = serde_yaml::from_str(
            "route53_dns_write:\n  enabled: true\n  cursor_key_ref: aws/route53-dns-cursor\n  mutation_receipt_key_ref: aws/route53-dns-mutation\n  operation_timeout_secs: 60\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(config.route53_dns_write.enabled);
        assert_eq!(
            config.route53_dns_write.mutation_receipt_key_ref.as_deref(),
            Some("aws/route53-dns-mutation")
        );
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "route53_dns_write:\n  enabled: true\n  endpoint_url: https://example.invalid\n"
        )
        .is_err());
    }

    #[test]
    fn route53_zone_lifecycle_is_default_off_and_strict() {
        assert!(
            !KubernetesCenterConfig::default()
                .route53_zone_lifecycle
                .enabled
        );
        let config: KubernetesCenterConfig = serde_yaml::from_str(
            "route53_zone_lifecycle:\n  enabled: true\n  cursor_key_ref: aws/route53-dns-cursor\n  lifecycle_token_key_ref: aws/route53-zone-lifecycle\n  operation_timeout_secs: 60\n  global_concurrency: 2\n  per_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(config.route53_zone_lifecycle.enabled);
        assert_eq!(
            config
                .route53_zone_lifecycle
                .lifecycle_token_key_ref
                .as_deref(),
            Some("aws/route53-zone-lifecycle")
        );
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "route53_zone_lifecycle:\n  enabled: true\n  endpoint_url: https://example.invalid\n"
        )
        .is_err());
    }

    #[test]
    fn cloudfront_is_default_off_and_strict() {
        assert!(!KubernetesCenterConfig::default().cloudfront.read_enabled);
        assert!(!KubernetesCenterConfig::default().cloudfront.write_enabled);
        let config: KubernetesCenterConfig = serde_yaml::from_str(
            "cloudfront:\n  read_enabled: true\n  write_enabled: true\n  fingerprint_key_ref: aws/cloudfront-fingerprint\n  operation_timeout_secs: 60\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(config.cloudfront.read_enabled);
        assert!(config.cloudfront.write_enabled);
        assert_eq!(
            config.cloudfront.fingerprint_key_ref.as_deref(),
            Some("aws/cloudfront-fingerprint")
        );
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "cloudfront:\n  read_enabled: true\n  endpoint_url: https://example.invalid\n"
        )
        .is_err());
    }

    #[test]
    fn aws_waf_capabilities_are_independently_default_off_and_strict() {
        let default = KubernetesCenterConfig::default();
        assert!(!default.aws_waf.read_enabled);
        assert!(!default.aws_waf.write_enabled);
        assert!(!default.aws_waf.attach_enabled);
        assert!(!default.aws_waf.detach_enabled);
        assert!(!default.aws_waf.security_weaken_enabled);

        let config: KubernetesCenterConfig = serde_yaml::from_str(
            "aws_waf:\n  detach_enabled: true\n  ownership_hmac_key_ref: aws/waf-owner\n  operation_timeout_secs: 60\n  global_concurrency: 4\n  per_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(config.aws_waf.detach_enabled);
        assert!(!config.aws_waf.write_enabled);
        assert_eq!(
            config.aws_waf.ownership_hmac_key_ref.as_deref(),
            Some("aws/waf-owner")
        );
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>(
            "aws_waf:\n  detach_enabled: true\n  ownership_hmac_key_reff: aws/waf-owner\n"
        )
        .is_err());
    }

    #[test]
    fn defaults_are_not_startable_without_explicit_security() {
        assert!(
            !KubernetesCenterConfig::default().auth.enabled
                || KubernetesCenterConfig::default().auth.discovery.is_empty()
        );
        assert!(KubernetesCenterConfig::default()
            .grpc_security
            .resolve_mtls_or_refuse()
            .is_err());
    }

    #[test]
    fn canonical_federation_port_is_stable() {
        assert_eq!(KubernetesServerConfig::default().grpc_addr, "0.0.0.0:12251");
        for manifest in [
            include_str!("../../../cicd/deploy/center-kubernetes/config.yaml"),
            include_str!("../../../cicd/deploy/center-kubernetes/deployment.yaml"),
            include_str!("../../../cicd/deploy/center-kubernetes/service.yaml"),
        ] {
            assert!(manifest.contains("12251"));
            assert!(!manifest.contains("12202"));
        }
    }

    #[test]
    fn kubernetes_oidc_rejects_insecure_transport() {
        let mut config: KubernetesCenterConfig = serde_yaml::from_str(
            r#"
auth:
  discovery: http://issuer.example/.well-known/openid-configuration
grpc_security:
  tls:
    cert: /tls/tls.crt
    key: /tls/tls.key
    ca: /tls/ca.crt
trust_domain: edgion.io
internal_forwarding:
  server_name: edgion-center-internal.edgion-system.svc
  expected_peer_spiffe_id: spiffe://edgion.io/ns/edgion-system/sa/edgion-center
  tls:
    cert: /internal/tls.crt
    key: /internal/tls.key
    ca: /internal/ca.crt
"#,
        )
        .unwrap();
        assert!(config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("https://"));
        config.auth.discovery =
            "https://issuer.example/.well-known/openid-configuration".to_string();
        config.auth.ssl_verify = false;
        assert!(config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("certificate verification"));
    }

    #[test]
    fn runtime_holder_is_bound_to_the_pod_incarnation() {
        std::env::set_var("POD_NAMESPACE", "edgion-system");
        std::env::set_var("POD_NAME", "center-0");
        std::env::set_var("POD_UID", "uid-a");
        let config: KubernetesCenterConfig = serde_yaml::from_str(
            r#"
auth:
  discovery: https://issuer.example/.well-known/openid-configuration
grpc_security:
  tls:
    cert: /tls/tls.crt
    key: /tls/tls.key
    ca: /tls/ca.crt
trust_domain: edgion.io
internal_forwarding:
  server_name: edgion-center-internal.edgion-system.svc
  expected_peer_spiffe_id: spiffe://edgion.io/ns/edgion-system/sa/edgion-center
  tls:
    cert: /internal/tls.crt
    key: /internal/tls.key
    ca: /internal/ca.crt
"#,
        )
        .unwrap();
        let first = config.validate().unwrap();
        std::env::set_var("POD_UID", "uid-b");
        let second = config.validate().unwrap();
        assert_eq!(first.namespace, "edgion-system");
        assert_eq!(first.holder, "center-0/uid-a");
        assert_eq!(second.holder, "center-0/uid-b");
        std::env::remove_var("POD_NAMESPACE");
        std::env::remove_var("POD_NAME");
        std::env::remove_var("POD_UID");
    }
}
