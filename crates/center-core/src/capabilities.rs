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
            },
            CenterMode::Kubernetes => Self {
                user_admin: false,
                role_admin: false,
                audit_query: false,
                controller_history: true,
                native_rbac: true,
                leader_election: true,
                password_login: false,
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
    ) -> Self {
        Self {
            user_admin,
            role_admin,
            audit_query,
            controller_history,
            native_rbac,
            leader_election,
            password_login,
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
    }
}
