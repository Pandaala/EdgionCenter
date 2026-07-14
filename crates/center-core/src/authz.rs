use serde::{Deserialize, Serialize};

use crate::CoreResult;

/// Authorization-store selector exposed by the shared Center configuration.
///
/// Additional platform-specific authorization implementations may be added as
/// adapters, but the externally visible mode remains a domain-level concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthzMode {
    /// Login implies full administrative access.
    #[default]
    AllowAll,
    /// Resolve permissions from the standalone SQL role store.
    Rbac,
}

/// An authenticated identity after token validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Principal {
    pub subject: String,
    pub provider: String,
    #[serde(default)]
    pub groups: Vec<String>,
}

/// A platform-neutral operation that an adapter can map to its policy model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    /// Stable Center permission key, for example `controllers:read`.
    pub permission: String,
    /// Optional canonical controller id targeted by the operation.
    pub controller_id: Option<String>,
}

/// Authorization outcome. Adapter failures are returned separately as errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl Decision {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

/// Resolves a validated principal and Center action through a platform policy.
#[async_trait::async_trait]
pub trait Authorizer: Send + Sync {
    async fn authorize(&self, principal: &Principal, action: &Action) -> CoreResult<Decision>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_wire_values_are_stable() {
        assert_eq!(AuthzMode::default(), AuthzMode::AllowAll);
        assert_eq!(
            serde_json::to_string(&AuthzMode::AllowAll).unwrap(),
            "\"allow_all\""
        );
        assert_eq!(serde_json::to_string(&AuthzMode::Rbac).unwrap(), "\"rbac\"");
    }

    #[test]
    fn decisions_distinguish_policy_denial_from_adapter_errors() {
        assert!(Decision::allow().allowed);
        assert_eq!(
            Decision::deny("missing permission").reason.as_deref(),
            Some("missing permission")
        );
    }
}
