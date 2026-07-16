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
    pub issuer: Option<String>,
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
    /// Platform-neutral operation classification used by native policy
    /// adapters. SQL permission checks may ignore it.
    #[serde(default)]
    pub operation: Option<ActionOperation>,
    /// Concrete Admin API path for Kubernetes non-resource authorization.
    #[serde(default)]
    pub request_path: Option<String>,
    /// Concrete lowercase HTTP verb for non-resource native authorization.
    #[serde(default)]
    pub request_verb: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionOperation {
    List,
    Get,
    Create,
    Update,
    Delete,
    Execute,
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

    /// Optionally enumerate granted Center permission keys without forcing the
    /// caller to issue one remote authorization request per candidate. Native
    /// policies that cannot enumerate safely return `None`.
    async fn granted_permissions(
        &self,
        _principal: &Principal,
        _candidates: &[String],
    ) -> CoreResult<Option<Vec<String>>> {
        Ok(None)
    }
}

/// Explicit opt-in policy for simple standalone deployments.
pub struct AllowAllAuthorizer;

#[async_trait::async_trait]
impl Authorizer for AllowAllAuthorizer {
    async fn authorize(&self, _principal: &Principal, _action: &Action) -> CoreResult<Decision> {
        Ok(Decision::allow())
    }

    async fn granted_permissions(
        &self,
        _principal: &Principal,
        candidates: &[String],
    ) -> CoreResult<Option<Vec<String>>> {
        Ok(Some(candidates.to_vec()))
    }
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

    #[tokio::test]
    async fn allow_all_authorizer_is_an_explicit_core_policy() {
        let decision = AllowAllAuthorizer
            .authorize(
                &Principal {
                    subject: "alice".to_string(),
                    provider: "local".to_string(),
                    issuer: None,
                    groups: Vec::new(),
                },
                &Action {
                    permission: "future:permission".to_string(),
                    controller_id: None,
                    operation: None,
                    request_path: None,
                    request_verb: None,
                },
            )
            .await
            .unwrap();
        assert!(decision.allowed);
    }
}
