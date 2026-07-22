use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CenterMode {
    Standalone,
    Kubernetes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CenterCapabilities {
    pub user_admin: bool,
    pub role_admin: bool,
    pub audit_query: bool,
    pub controller_history: bool,
    pub native_rbac: bool,
    pub leader_election: bool,
    pub password_login: bool,
    /// Cloudflare-specific, read-only DNS zone inventory Admin API.
    pub cloudflare_dns_read: bool,
    /// Cloudflare-specific, synchronous DNS zone write Admin API.
    pub cloudflare_dns_write: bool,
    /// Provider-neutral, secret-free ProviderAccount desired-state Admin API.
    pub provider_account_admin: bool,
    /// Read-only, sanitized ProviderAccount capability snapshot Admin API.
    pub provider_capability_read: bool,
    /// Explicit, bounded ProviderAccount credential inspection Admin API.
    pub provider_credential_inspection: bool,
}

impl CenterCapabilities {
    pub const fn for_mode(mode: CenterMode) -> Self {
        match mode {
            CenterMode::Standalone => Self {
                user_admin: true,
                role_admin: true,
                audit_query: true,
                controller_history: true,
                native_rbac: false,
                leader_election: false,
                password_login: true,
                cloudflare_dns_read: false,
                cloudflare_dns_write: false,
                provider_account_admin: false,
                provider_capability_read: false,
                provider_credential_inspection: false,
            },
            CenterMode::Kubernetes => Self {
                user_admin: false,
                role_admin: false,
                audit_query: false,
                controller_history: true,
                native_rbac: true,
                leader_election: true,
                password_login: false,
                cloudflare_dns_read: false,
                cloudflare_dns_write: false,
                provider_account_admin: false,
                provider_capability_read: false,
                provider_credential_inspection: false,
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub const fn resolved(
        user_admin: bool,
        role_admin: bool,
        audit_query: bool,
        controller_history: bool,
        native_rbac: bool,
        leader_election: bool,
        password_login: bool,
        cloudflare_dns_read: bool,
        provider_account_admin: bool,
        provider_capability_read: bool,
        provider_credential_inspection: bool,
    ) -> Self {
        Self {
            user_admin,
            role_admin,
            audit_query,
            controller_history,
            native_rbac,
            leader_election,
            password_login,
            cloudflare_dns_read,
            cloudflare_dns_write: false,
            provider_account_admin,
            provider_capability_read,
            provider_credential_inspection,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_resolve_explicit_management_surfaces() {
        let standalone = CenterCapabilities::for_mode(CenterMode::Standalone);
        let kubernetes = CenterCapabilities::for_mode(CenterMode::Kubernetes);
        assert!(standalone.user_admin && standalone.audit_query);
        assert!(!standalone.native_rbac);
        assert!(!kubernetes.user_admin && !kubernetes.audit_query);
        assert!(kubernetes.native_rbac && kubernetes.leader_election);
        assert!(!standalone.cloudflare_dns_read && !kubernetes.cloudflare_dns_read);
        assert!(!standalone.cloudflare_dns_write && !kubernetes.cloudflare_dns_write);
        assert!(!standalone.provider_account_admin && !kubernetes.provider_account_admin);
        assert!(!standalone.provider_capability_read && !kubernetes.provider_capability_read);
        assert!(
            !standalone.provider_credential_inspection
                && !kubernetes.provider_credential_inspection
        );
    }
}
