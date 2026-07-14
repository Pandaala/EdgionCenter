use edgion_center_app::common::{
    auth::AdminAuthConfig,
    config::{ConfSyncSecurityConfig, ConfSyncTlsConfig},
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

    #[test]
    fn unknown_fields_fail_closed() {
        assert!(serde_yaml::from_str::<KubernetesCenterConfig>("database: {}\n").is_err());
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
