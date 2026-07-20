use std::{collections::BTreeMap, sync::Arc};

use edgion_center_adapter_cloudflare::{CloudflareTokenStatus, CloudflareTokenVerification};
use edgion_center_adapter_credential_files::{
    CredentialPurpose, MountedCredentialBinding, MountedCredentialConfig,
};
use edgion_center_core::{
    provider_account_from_desired, CloudResourceId, CredentialRef, CredentialState, DeletionPolicy,
    ManagementPolicy, ProviderAccountDesired, ProviderErrorCategory,
};

use super::*;

const CENTER_ACCOUNT_ID: &str = "cf-main";
const PROVIDER_ACCOUNT_ID: &str = "0123456789abcdef0123456789abcdef";

#[derive(Clone)]
struct FakeProbe {
    verification: Result<CloudflareTokenVerification, NormalizedProviderError>,
    scope: Result<bool, NormalizedProviderError>,
}

#[async_trait]
impl CloudflareCredentialProbe for FakeProbe {
    async fn verify_api_token(
        &self,
    ) -> Result<CloudflareTokenVerification, NormalizedProviderError> {
        self.verification.clone()
    }

    async fn probe_account_zone_scope(
        &self,
        account_id: &str,
    ) -> Result<bool, NormalizedProviderError> {
        assert_eq!(account_id, PROVIDER_ACCOUNT_ID);
        self.scope.clone()
    }
}

struct FakeFactory(FakeProbe);

impl ProbeFactory for FakeFactory {
    fn build(
        &self,
        _token: CloudflareApiToken,
    ) -> Result<Arc<dyn CloudflareCredentialProbe>, NormalizedProviderError> {
        Ok(Arc::new(self.0.clone()))
    }
}

fn account() -> ProviderAccount {
    let spec = ProviderAccountSpec {
        provider: CloudProvider::Cloudflare,
        scope: Some(ProviderAccountScope::Cloudflare {
            account_id: PROVIDER_ACCOUNT_ID.into(),
        }),
        credential_source: CredentialSource::StaticSecret {
            credential_ref: CredentialRef::new("cloudflare/main").unwrap(),
        },
    };
    provider_account_from_desired(
        CloudResourceId::new(CENTER_ACCOUNT_ID).unwrap(),
        1,
        &ProviderAccountDesired {
            display_name: "Cloudflare main".into(),
            owner: None,
            labels: BTreeMap::new(),
            management_policy: ManagementPolicy::Managed,
            deletion_policy: DeletionPolicy::Retain,
            spec,
        },
    )
    .unwrap()
}

async fn mounted(token: &[u8]) -> (tempfile::TempDir, Arc<MountedCredentialResolver>) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("revision.key"), [9_u8; 32]).unwrap();
    std::fs::write(directory.path().join("token"), token).unwrap();
    let resolver = MountedCredentialResolver::from_config(&MountedCredentialConfig {
        enabled: true,
        root_directory: Some(directory.path().to_string_lossy().into_owned()),
        revision_key_file: Some("revision.key".into()),
        bindings: vec![MountedCredentialBinding {
            credential_ref: "cloudflare/main".into(),
            provider_account_id: CENTER_ACCOUNT_ID.into(),
            provider: CloudProvider::Cloudflare,
            purpose: CredentialPurpose::CloudflareApiToken,
            file: "token".into(),
        }],
    })
    .await
    .unwrap()
    .unwrap();
    (directory, Arc::new(resolver))
}

fn active_probe(scope: bool) -> FakeProbe {
    FakeProbe {
        verification: Ok(CloudflareTokenVerification {
            token_id: "sanitized-token-id".into(),
            status: CloudflareTokenStatus::Active,
        }),
        scope: Ok(scope),
    }
}

async fn inspect_with(
    resolver: Arc<MountedCredentialResolver>,
    probe: FakeProbe,
    account: &ProviderAccount,
) -> CredentialInspection {
    let resolver = CloudflareInspectorResolver {
        mounted_resolver: resolver,
        probe_factory: Arc::new(FakeFactory(probe)),
    };
    resolver
        .resolve(account)
        .await
        .unwrap()
        .inspect(&account.spec)
        .await
        .unwrap()
}

fn provider_error(category: ProviderErrorCategory) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        "provider-secret-marker",
        "provider body secret-marker",
        (category == ProviderErrorCategory::Throttled).then_some(1_000),
        None,
    )
    .unwrap()
}

#[test]
fn configuration_is_default_off_strict_and_has_no_endpoint_override() {
    assert!(!CloudflareCredentialInspectionConfig::default().enabled);
    assert!(
        serde_yaml::from_str::<CloudflareCredentialInspectionConfig>(
            "enabled: true\nbase_url: https://attacker.invalid\n"
        )
        .is_err()
    );
    assert!(compose_credential_inspection(
        &CloudflareCredentialInspectionConfig::default(),
        None,
        None,
    )
    .unwrap()
    .is_none());
    assert!(compose_credential_inspection(
        &CloudflareCredentialInspectionConfig { enabled: true },
        None,
        None,
    )
    .is_err());
}

#[tokio::test]
async fn active_token_and_nonempty_exact_scope_are_valid() {
    let (_directory, resolver) = mounted(b"secret-token").await;
    let inspection = inspect_with(resolver, active_probe(true), &account()).await;
    assert_eq!(inspection.state, CredentialState::Valid);
    assert_eq!(
        inspection.identity,
        Some(ProviderIdentity {
            provider: CloudProvider::Cloudflare,
            principal: "api_token:sanitized-token-id".into(),
            scope: Some(PROVIDER_ACCOUNT_ID.into()),
        })
    );
    assert!(inspection
        .credential_revision
        .as_deref()
        .is_some_and(|revision| revision.starts_with("hmac-sha256-v1:")));
}

#[tokio::test]
async fn bound_inspector_rejects_changed_authority_before_credential_io() {
    let (directory, mounted_resolver) = mounted(b"secret-token").await;
    let original = account();
    let resolver = CloudflareInspectorResolver {
        mounted_resolver,
        probe_factory: Arc::new(FakeFactory(active_probe(true))),
    };
    let inspector = resolver.resolve(&original).await.unwrap();
    std::fs::remove_file(directory.path().join("token")).unwrap();

    let mut changed = original.spec.clone();
    changed.scope = Some(ProviderAccountScope::Cloudflare {
        account_id: "fedcba9876543210fedcba9876543210".into(),
    });
    let inspection = inspector.inspect(&changed).await.unwrap();

    assert_eq!(inspection.state, CredentialState::Invalid);
    assert_eq!(
        inspection.issues[0].code,
        "cloudflare_account_authority_mismatch"
    );
}

#[tokio::test]
async fn empty_zone_page_does_not_claim_account_scope() {
    let (_directory, resolver) = mounted(b"secret-token").await;
    let inspection = inspect_with(resolver, active_probe(false), &account()).await;
    assert_eq!(inspection.state, CredentialState::Unknown);
    assert!(inspection.identity.is_none());
    assert_eq!(
        inspection.issues[0].code,
        "cloudflare_account_scope_unproven"
    );
}

#[tokio::test]
async fn token_states_and_provider_errors_map_to_typed_sanitized_issues() {
    for (status, state, kind) in [
        (
            CloudflareTokenStatus::Disabled,
            CredentialState::Invalid,
            CredentialIssueKind::AuthenticationFailed,
        ),
        (
            CloudflareTokenStatus::Expired,
            CredentialState::Invalid,
            CredentialIssueKind::Expired,
        ),
        (
            CloudflareTokenStatus::Unknown,
            CredentialState::Unknown,
            CredentialIssueKind::ProviderUnavailable,
        ),
    ] {
        let (_directory, resolver) = mounted(b"secret-token").await;
        let inspection = inspect_with(
            resolver,
            FakeProbe {
                verification: Ok(CloudflareTokenVerification {
                    token_id: "token-id".into(),
                    status,
                }),
                scope: Ok(true),
            },
            &account(),
        )
        .await;
        assert_eq!(inspection.state, state);
        assert_eq!(inspection.issues[0].kind, kind);
    }

    for (category, state, kind) in [
        (
            ProviderErrorCategory::Authentication,
            CredentialState::Invalid,
            CredentialIssueKind::AuthenticationFailed,
        ),
        (
            ProviderErrorCategory::Authorization,
            CredentialState::Invalid,
            CredentialIssueKind::PermissionDenied,
        ),
        (
            ProviderErrorCategory::Transient,
            CredentialState::Unknown,
            CredentialIssueKind::ProviderUnavailable,
        ),
    ] {
        let (_directory, resolver) = mounted(b"secret-token").await;
        let inspection = inspect_with(
            resolver,
            FakeProbe {
                verification: Err(provider_error(category)),
                scope: Ok(true),
            },
            &account(),
        )
        .await;
        assert_eq!(inspection.state, state);
        assert_eq!(inspection.issues[0].kind, kind);
        let diagnostics = format!("{inspection:?}");
        assert!(!diagnostics.contains("secret-marker"));
        assert!(!diagnostics.contains("secret-token"));
    }
}

#[tokio::test]
async fn reference_scope_source_and_encoding_fail_closed() {
    let (_directory, resolver) = mounted(b"secret-token").await;
    let mut wrong_ref = account();
    wrong_ref.spec.credential_source = CredentialSource::StaticSecret {
        credential_ref: CredentialRef::new("cloudflare/missing").unwrap(),
    };
    let inspection = inspect_with(resolver, active_probe(true), &wrong_ref).await;
    assert_eq!(inspection.state, CredentialState::Invalid);
    assert_eq!(
        inspection.issues[0].kind,
        CredentialIssueKind::ReferenceNotFound
    );

    let (_directory, resolver) = mounted(b"secret-token").await;
    let mut ambient = account();
    ambient.spec.credential_source = CredentialSource::Ambient;
    let inspection = inspect_with(resolver, active_probe(true), &ambient).await;
    assert_eq!(inspection.state, CredentialState::Invalid);
    assert_eq!(
        inspection.issues[0].code,
        "cloudflare_credential_source_unsupported"
    );

    let (_directory, resolver) = mounted(&[0xff, 0xfe]).await;
    let inspection = inspect_with(resolver, active_probe(true), &account()).await;
    assert_eq!(inspection.state, CredentialState::Invalid);
    assert_eq!(inspection.issues[0].code, "cloudflare_token_not_utf8");
    assert!(inspection.credential_revision.is_some());
}
