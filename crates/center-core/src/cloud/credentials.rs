//! Secret-free credential selection and inspection contracts.
//!
//! Provider adapters resolve credential material internally. Neither this
//! module nor callers of `CredentialInspector` receive tokens, private keys,
//! access keys, or service-account documents.

use serde::{Deserialize, Serialize};

use crate::{CloudProvider, CoreError, CoreResult, CredentialRef, ProviderAccountSpec};

/// Selects how a provider adapter obtains credentials without embedding secret
/// material in the provider account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CredentialSource {
    /// Resolve a stable alias through the composition-specific secret resolver.
    StaticSecret { credential_ref: CredentialRef },
    /// Use the provider SDK's ambient/default credential chain.
    Ambient,
    /// Exchange an external workload identity for provider credentials.
    Federated {
        /// Optional reference to a projected subject token. It is omitted when
        /// the SDK/platform discovers the token through its ambient chain.
        subject_token_ref: Option<CredentialRef>,
        target_principal: String,
        audience: Option<String>,
    },
    /// Use an ambient or referenced base identity to assume/impersonate a
    /// provider identity, such as an AWS IAM role.
    AssumeIdentity {
        base_credential_ref: Option<CredentialRef>,
        target_principal: String,
        external_id_ref: Option<CredentialRef>,
    },
}

impl CredentialSource {
    pub fn validate(&self) -> CoreResult<()> {
        match self {
            Self::StaticSecret { .. } | Self::Ambient => Ok(()),
            Self::Federated {
                target_principal,
                audience,
                ..
            } => {
                validate_principal(target_principal)?;
                if audience
                    .as_ref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    return Err(CoreError::Conflict(
                        "federated credential audience must not be empty".to_string(),
                    ));
                }
                Ok(())
            }
            Self::AssumeIdentity {
                target_principal, ..
            } => validate_principal(target_principal),
        }
    }

    /// Returns only stable secret references, never resolved secret material.
    pub fn credential_refs(&self) -> Vec<&CredentialRef> {
        match self {
            Self::StaticSecret { credential_ref } => vec![credential_ref],
            Self::Ambient => Vec::new(),
            Self::Federated {
                subject_token_ref, ..
            } => subject_token_ref.iter().collect(),
            Self::AssumeIdentity {
                base_credential_ref,
                external_id_ref,
                ..
            } => base_credential_ref
                .iter()
                .chain(external_id_ref.iter())
                .collect(),
        }
    }
}

fn validate_principal(principal: &str) -> CoreResult<()> {
    if principal.trim().is_empty()
        || principal.trim() != principal
        || principal.chars().any(char::is_control)
    {
        return Err(CoreError::Conflict(
            "credential target principal is invalid".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
    Valid,
    Invalid,
    Unknown,
}

/// Non-secret identity returned after a provider-authentication probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdentity {
    pub provider: CloudProvider,
    /// Provider principal identifier safe for operator visibility and audit.
    pub principal: String,
    /// Provider account, project, or organization scope when discoverable.
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialIssueKind {
    ReferenceNotFound,
    AuthenticationFailed,
    PermissionDenied,
    Expired,
    InvalidConfiguration,
    ProviderUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialIssue {
    pub kind: CredentialIssueKind,
    /// Stable adapter-defined diagnostic code. It must not contain a provider
    /// response body or credential material.
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInspection {
    pub state: CredentialState,
    pub identity: Option<ProviderIdentity>,
    /// Opaque non-secret revision used to detect rotation and rebuild clients.
    pub credential_revision: Option<String>,
    pub expires_at_unix_ms: Option<i64>,
    #[serde(default)]
    pub issues: Vec<CredentialIssue>,
}

impl CredentialInspection {
    /// Returns true when a provider adapter must discard cached clients and
    /// resolve credentials again. Provider-account identity remains stable.
    pub fn requires_client_refresh(&self, previous: &Self) -> bool {
        self.credential_revision != previous.credential_revision
            || self.identity != previous.identity
            || self.state != previous.state
    }
}

#[async_trait::async_trait]
pub trait CredentialInspector: Send + Sync {
    /// Resolve and test an account's credentials without returning the resolved
    /// credential material to the caller.
    async fn inspect(&self, account: &ProviderAccountSpec) -> CoreResult<CredentialInspection>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_sources_serialize_references_but_not_material() {
        let source = CredentialSource::AssumeIdentity {
            base_credential_ref: Some(CredentialRef::new("aws/base").unwrap()),
            target_principal: "arn:aws:iam::123456789012:role/dns-manager".to_string(),
            external_id_ref: Some(CredentialRef::new("aws/external-id").unwrap()),
        };
        let json = serde_json::to_string(&source).unwrap();
        assert!(json.contains("aws/base"));
        assert!(json.contains("dns-manager"));
        assert!(!json.contains("secretAccessKey"));
        assert!(!json.contains("sessionToken"));
    }

    #[test]
    fn credential_source_validation_rejects_empty_identity_fields() {
        let invalid = CredentialSource::Federated {
            subject_token_ref: None,
            target_principal: " ".to_string(),
            audience: None,
        };
        assert!(invalid.validate().is_err());

        let invalid_audience = CredentialSource::Federated {
            subject_token_ref: None,
            target_principal: "service-account@example.iam.gserviceaccount.com".to_string(),
            audience: Some(String::new()),
        };
        assert!(invalid_audience.validate().is_err());
    }

    #[test]
    fn credential_reference_enumeration_supports_rotation_dependencies() {
        let base = CredentialRef::new("aws/base").unwrap();
        let external = CredentialRef::new("aws/external-id").unwrap();
        let source = CredentialSource::AssumeIdentity {
            base_credential_ref: Some(base.clone()),
            target_principal: "arn:aws:iam::123456789012:role/dns-manager".to_string(),
            external_id_ref: Some(external.clone()),
        };
        assert_eq!(source.credential_refs(), vec![&base, &external]);
    }

    #[test]
    fn inspection_wire_contract_distinguishes_authentication_and_authorization() {
        let authentication = CredentialInspection {
            state: CredentialState::Invalid,
            identity: None,
            credential_revision: Some("version-7".to_string()),
            expires_at_unix_ms: None,
            issues: vec![CredentialIssue {
                kind: CredentialIssueKind::AuthenticationFailed,
                code: "invalid_token".to_string(),
                message: "provider rejected the credential".to_string(),
            }],
        };
        let authorization = CredentialInspection {
            issues: vec![CredentialIssue {
                kind: CredentialIssueKind::PermissionDenied,
                code: "dns_read_denied".to_string(),
                message: "credential cannot inspect DNS zones".to_string(),
            }],
            ..authentication.clone()
        };
        assert_ne!(authentication.issues[0].kind, authorization.issues[0].kind);
        assert_eq!(
            serde_json::to_value(authentication).unwrap()["issues"][0]["kind"],
            "authentication_failed"
        );
        assert_eq!(
            serde_json::to_value(authorization).unwrap()["issues"][0]["kind"],
            "permission_denied"
        );
    }

    #[test]
    fn credential_rotation_refreshes_clients_without_changing_account_references() {
        let previous = CredentialInspection {
            state: CredentialState::Valid,
            identity: Some(ProviderIdentity {
                provider: CloudProvider::Aws,
                principal: "arn:aws:sts::123456789012:assumed-role/dns/center".to_string(),
                scope: Some("123456789012".to_string()),
            }),
            credential_revision: Some("secret-rv-10".to_string()),
            expires_at_unix_ms: Some(1000),
            issues: Vec::new(),
        };
        let rotated = CredentialInspection {
            credential_revision: Some("secret-rv-11".to_string()),
            expires_at_unix_ms: Some(2000),
            ..previous.clone()
        };
        assert!(rotated.requires_client_refresh(&previous));

        let renewed_same_revision = CredentialInspection {
            expires_at_unix_ms: Some(3000),
            ..rotated.clone()
        };
        assert!(!renewed_same_revision.requires_client_refresh(&rotated));
    }
}
