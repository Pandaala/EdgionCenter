//! Cloudflare-specific credential inspection composition.
//!
//! This crate is the only production glue between mounted credential bytes,
//! the Cloudflare transport, and provider-neutral inspection orchestration. It
//! does not expose a configurable provider endpoint or mount DNS Admin APIs.

use std::{fmt, str, sync::Arc, time::Duration};

use async_trait::async_trait;
use edgion_center_adapter_cloudflare::{
    CloudflareApiToken, CloudflareCredentialProbe, CloudflareHttpApi, CloudflareTokenStatus,
};
use edgion_center_adapter_credential_files::{
    CredentialPurpose, CredentialResolutionError, MountedCredentialResolver,
    ResolveCredentialRequest,
};
use edgion_center_core::{
    CloudProvider, CloudResourceId, CoreError, CoreResult, CredentialInspection,
    CredentialInspector, CredentialIssue, CredentialIssueKind, CredentialSource, CredentialState,
    NormalizedProviderError, ProviderAccount, ProviderAccountScope, ProviderAccountSpec,
    ProviderAccountStore, ProviderErrorCategory, ProviderIdentity,
};
use edgion_center_runtime::cloud::{CredentialInspectionService, CredentialInspectorResolver};
use serde::{Deserialize, Serialize};

mod dns_admin;
mod dns_admin_service;
mod dns_write_service;

pub use dns_admin_service::{compose_dns_admin, CloudflareDnsReadConfig};
pub use dns_write_service::{compose_dns_write_admin, CloudflareDnsWriteConfig};

const INSPECTION_TIMEOUT: Duration = Duration::from_secs(30);

/// Strict shared switch. Disabled composition performs no credential reads and
/// constructs no Cloudflare HTTP client.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloudflareCredentialInspectionConfig {
    pub enabled: bool,
}

impl fmt::Debug for CloudflareCredentialInspectionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CloudflareCredentialInspectionConfig")
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// Composes the provider-neutral service only after all explicitly enabled
/// dependencies are present. Production always uses `api.cloudflare.com`.
pub fn compose_credential_inspection(
    config: &CloudflareCredentialInspectionConfig,
    account_store: Option<Arc<dyn ProviderAccountStore>>,
    mounted_resolver: Option<Arc<MountedCredentialResolver>>,
) -> CoreResult<Option<CredentialInspectionService>> {
    if !config.enabled {
        return Ok(None);
    }
    let account_store = account_store.ok_or_else(|| {
        CoreError::Conflict(
            "Cloudflare credential inspection requires a provider account store".into(),
        )
    })?;
    let mounted_resolver = mounted_resolver.ok_or_else(|| {
        CoreError::Conflict("Cloudflare credential inspection requires mounted credentials".into())
    })?;
    let resolver = Arc::new(CloudflareInspectorResolver {
        mounted_resolver,
        probe_factory: Arc::new(ProductionProbeFactory),
    });
    CredentialInspectionService::new(INSPECTION_TIMEOUT, account_store, resolver).map(Some)
}

trait ProbeFactory: Send + Sync {
    fn build(
        &self,
        token: CloudflareApiToken,
    ) -> Result<Arc<dyn CloudflareCredentialProbe>, NormalizedProviderError>;
}

struct ProductionProbeFactory;

impl ProbeFactory for ProductionProbeFactory {
    fn build(
        &self,
        token: CloudflareApiToken,
    ) -> Result<Arc<dyn CloudflareCredentialProbe>, NormalizedProviderError> {
        CloudflareHttpApi::new(token)
            .map(|client| Arc::new(client) as Arc<dyn CloudflareCredentialProbe>)
    }
}

struct CloudflareInspectorResolver {
    mounted_resolver: Arc<MountedCredentialResolver>,
    probe_factory: Arc<dyn ProbeFactory>,
}

#[async_trait]
impl CredentialInspectorResolver for CloudflareInspectorResolver {
    async fn resolve(&self, account: &ProviderAccount) -> Option<Arc<dyn CredentialInspector>> {
        if account.spec.provider != CloudProvider::Cloudflare {
            return None;
        }
        Some(Arc::new(CloudflareMountedTokenInspector {
            provider_account_id: account.metadata.id.clone(),
            expected_spec: account.spec.clone(),
            mounted_resolver: self.mounted_resolver.clone(),
            probe_factory: self.probe_factory.clone(),
        }))
    }
}

struct CloudflareMountedTokenInspector {
    provider_account_id: CloudResourceId,
    expected_spec: ProviderAccountSpec,
    mounted_resolver: Arc<MountedCredentialResolver>,
    probe_factory: Arc<dyn ProbeFactory>,
}

#[async_trait]
impl CredentialInspector for CloudflareMountedTokenInspector {
    async fn inspect(&self, account: &ProviderAccountSpec) -> CoreResult<CredentialInspection> {
        if account != &self.expected_spec || account.provider != CloudProvider::Cloudflare {
            return Ok(issue(
                CredentialState::Invalid,
                None,
                CredentialIssueKind::InvalidConfiguration,
                "cloudflare_account_authority_mismatch",
                "Cloudflare credential account authority did not match",
            ));
        }
        let provider_scope = match account.scope.as_ref() {
            Some(ProviderAccountScope::Cloudflare { account_id }) => account_id.as_str(),
            _ => {
                return Ok(issue(
                    CredentialState::Invalid,
                    None,
                    CredentialIssueKind::InvalidConfiguration,
                    "cloudflare_account_scope_invalid",
                    "Cloudflare account scope is invalid",
                ));
            }
        };
        let credential_ref = match &account.credential_source {
            CredentialSource::StaticSecret { credential_ref } => credential_ref,
            _ => {
                return Ok(issue(
                    CredentialState::Invalid,
                    None,
                    CredentialIssueKind::InvalidConfiguration,
                    "cloudflare_credential_source_unsupported",
                    "Cloudflare credential source is unsupported",
                ));
            }
        };

        let resolved = match self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: &self.provider_account_id,
                provider: &CloudProvider::Cloudflare,
                purpose: CredentialPurpose::CloudflareApiToken,
                credential_ref,
            })
            .await
        {
            Ok(resolved) => resolved,
            Err(error) => return Ok(resolution_issue(error)),
        };
        let revision = Some(resolved.revision().as_str().to_owned());
        let token = match resolved.with_bytes(|bytes| str::from_utf8(bytes).map(str::to_owned)) {
            Ok(token) => token,
            Err(_) => {
                return Ok(issue(
                    CredentialState::Invalid,
                    revision,
                    CredentialIssueKind::InvalidConfiguration,
                    "cloudflare_token_not_utf8",
                    "Cloudflare API token encoding is invalid",
                ));
            }
        };
        let token = match CloudflareApiToken::new(token) {
            Ok(token) => token,
            Err(_) => {
                return Ok(issue(
                    CredentialState::Invalid,
                    revision,
                    CredentialIssueKind::InvalidConfiguration,
                    "cloudflare_token_format_invalid",
                    "Cloudflare API token format is invalid",
                ));
            }
        };
        let probe = match self.probe_factory.build(token) {
            Ok(probe) => probe,
            Err(error) => return Ok(provider_issue(error, revision)),
        };
        let verification = match probe.verify_api_token().await {
            Ok(verification) => verification,
            Err(error) => return Ok(provider_issue(error, revision)),
        };
        match verification.status {
            CloudflareTokenStatus::Disabled => {
                return Ok(issue(
                    CredentialState::Invalid,
                    revision,
                    CredentialIssueKind::AuthenticationFailed,
                    "cloudflare_token_disabled",
                    "Cloudflare API token is disabled",
                ));
            }
            CloudflareTokenStatus::Expired => {
                return Ok(issue(
                    CredentialState::Invalid,
                    revision,
                    CredentialIssueKind::Expired,
                    "cloudflare_token_expired",
                    "Cloudflare API token is expired",
                ));
            }
            CloudflareTokenStatus::Unknown => {
                return Ok(issue(
                    CredentialState::Unknown,
                    revision,
                    CredentialIssueKind::ProviderUnavailable,
                    "cloudflare_token_status_unknown",
                    "Cloudflare API token status is unknown",
                ));
            }
            CloudflareTokenStatus::Active => {}
        }
        let scope_proven = match probe.probe_account_zone_scope(provider_scope).await {
            Ok(proven) => proven,
            Err(error) => return Ok(provider_issue(error, revision)),
        };
        if !scope_proven {
            return Ok(issue(
                CredentialState::Unknown,
                revision,
                CredentialIssueKind::ProviderUnavailable,
                "cloudflare_account_scope_unproven",
                "Cloudflare account scope could not be proven",
            ));
        }

        Ok(CredentialInspection {
            state: CredentialState::Valid,
            identity: Some(ProviderIdentity {
                provider: CloudProvider::Cloudflare,
                principal: format!("api_token:{}", verification.token_id),
                scope: Some(provider_scope.to_owned()),
            }),
            credential_revision: revision,
            expires_at_unix_ms: None,
            issues: Vec::new(),
        })
    }
}

fn resolution_issue(error: CredentialResolutionError) -> CredentialInspection {
    let (state, kind, code, message) = match error {
        CredentialResolutionError::ReferenceNotFound => (
            CredentialState::Invalid,
            CredentialIssueKind::ReferenceNotFound,
            "cloudflare_credential_reference_not_found",
            "Cloudflare credential reference was not found",
        ),
        CredentialResolutionError::ScopeMismatch => (
            CredentialState::Invalid,
            CredentialIssueKind::InvalidConfiguration,
            "cloudflare_credential_scope_mismatch",
            "Cloudflare credential binding scope did not match",
        ),
        CredentialResolutionError::Unreadable => (
            CredentialState::Unknown,
            CredentialIssueKind::ProviderUnavailable,
            "cloudflare_credential_unavailable",
            "Cloudflare credential material is unavailable",
        ),
        CredentialResolutionError::NotRegular
        | CredentialResolutionError::TooLarge
        | CredentialResolutionError::Empty
        | CredentialResolutionError::UnsafePermissions
        | CredentialResolutionError::RevisionKeyConflict => (
            CredentialState::Invalid,
            CredentialIssueKind::InvalidConfiguration,
            "cloudflare_credential_file_invalid",
            "Cloudflare credential file is invalid",
        ),
    };
    issue(state, None, kind, code, message)
}

fn provider_issue(
    error: NormalizedProviderError,
    revision: Option<String>,
) -> CredentialInspection {
    let (state, kind, code, message) = match error.category() {
        ProviderErrorCategory::Authentication => (
            CredentialState::Invalid,
            CredentialIssueKind::AuthenticationFailed,
            "cloudflare_authentication_failed",
            "Cloudflare rejected the credential",
        ),
        ProviderErrorCategory::Authorization => (
            CredentialState::Invalid,
            CredentialIssueKind::PermissionDenied,
            "cloudflare_dns_read_denied",
            "Cloudflare denied the account-scoped zone probe",
        ),
        ProviderErrorCategory::Validation => (
            CredentialState::Unknown,
            CredentialIssueKind::ProviderUnavailable,
            "cloudflare_probe_response_invalid",
            "Cloudflare credential probe returned invalid evidence",
        ),
        ProviderErrorCategory::Quota
        | ProviderErrorCategory::Conflict
        | ProviderErrorCategory::NotFound
        | ProviderErrorCategory::Transient
        | ProviderErrorCategory::Throttled
        | ProviderErrorCategory::UnknownOutcome => (
            CredentialState::Unknown,
            CredentialIssueKind::ProviderUnavailable,
            "cloudflare_probe_unavailable",
            "Cloudflare credential probe is unavailable",
        ),
    };
    issue(state, revision, kind, code, message)
}

fn issue(
    state: CredentialState,
    credential_revision: Option<String>,
    kind: CredentialIssueKind,
    code: &str,
    message: &str,
) -> CredentialInspection {
    CredentialInspection {
        state,
        identity: None,
        credential_revision,
        expires_at_unix_ms: None,
        issues: vec![CredentialIssue {
            kind,
            code: code.into(),
            message: message.into(),
        }],
    }
}

#[cfg(test)]
mod tests;
