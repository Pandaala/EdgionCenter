//! Shared behavioral assertions for capability snapshot store adapters.

use super::{
    CapabilityAction, CapabilityDimension, CapabilityDimensionObservation,
    CapabilityDiscoveryIssue, CapabilityDiscoveryReport, CapabilityDiscoveryRequest,
    CapabilityDiscoveryState, CapabilityEvidence, CapabilityIssueScope, CapabilityIssueSeverity,
    CapabilityObservation, CapabilityReason, CapabilityScope, CapabilitySnapshotKey,
    CapabilitySnapshotStore, CapabilityStoreWrite, CloudProvider, CloudResourceId,
    CloudResourceKind, CredentialSource, DnsCapability, ProviderAccountSpec, ProviderCapability,
    ProviderCapabilitySnapshot, ProviderRegion, ProviderResourceRef, SanitizedCapabilityCode,
    SanitizedCapabilityMessage, TriState,
};

pub async fn assert_roundtrip_and_fencing(store: &dyn CapabilitySnapshotStore, prefix: &str) {
    let key = account_key(prefix, "roundtrip");
    let first = store
        .begin_discovery(&key, 1, Some("revision-A"))
        .await
        .expect("first discovery fence");
    let first_snapshot = snapshot(&key, first.clone(), 1_000);
    assert_eq!(
        store
            .put_if_current(&key, &first, &first_snapshot)
            .await
            .expect("store first snapshot"),
        CapabilityStoreWrite::Stored
    );
    let conflicting_snapshot = snapshot(&key, first.clone(), 1_001);
    assert!(store
        .put_if_current(&key, &first, &conflicting_snapshot)
        .await
        .is_err());
    assert_eq!(
        store.get(&key).await.expect("immutable first snapshot"),
        Some(first_snapshot.clone())
    );
    assert_eq!(
        store.get(&key).await.expect("get first snapshot"),
        Some(first_snapshot.clone())
    );
    assert_eq!(
        store
            .put_if_current(&key, &first, &first_snapshot)
            .await
            .expect("repeat first snapshot"),
        CapabilityStoreWrite::Stored
    );

    let second = store
        .begin_discovery(&key, 2, Some("revision-b"))
        .await
        .expect("second discovery fence");
    assert!(second.discovery_epoch > first.discovery_epoch);
    assert_ne!(second.discovery_token, first.discovery_token);
    assert_eq!(second.provider_account_generation, 2);
    assert_eq!(second.credential_revision.as_deref(), Some("revision-b"));
    assert_eq!(
        store.get(&key).await.expect("read between begin and put"),
        Some(first_snapshot.clone())
    );
    assert_eq!(
        store
            .put_if_current(&key, &first, &first_snapshot)
            .await
            .expect("reject stale snapshot"),
        CapabilityStoreWrite::FenceLost
    );

    let second_snapshot = snapshot(&key, second.clone(), 2_000);
    assert_eq!(
        store
            .put_if_current(&key, &second, &second_snapshot)
            .await
            .expect("store second snapshot"),
        CapabilityStoreWrite::Stored
    );
    assert_eq!(
        store.get(&key).await.expect("get second snapshot"),
        Some(second_snapshot)
    );
}

pub async fn assert_scope_isolation(store: &dyn CapabilitySnapshotStore, prefix: &str) {
    let account = account_key(prefix, "scope");
    let region = CapabilitySnapshotKey {
        provider_account_id: account.provider_account_id.clone(),
        scope: CapabilityScope::Region {
            region: ProviderRegion::new("us-east-1").expect("region"),
        },
    };
    let other = account_key(prefix, "scope-other");
    let resource = ProviderResourceRef {
        provider_account_id: account.provider_account_id.clone(),
        external_id: "shared-external-id".to_string(),
    };
    let zone = CapabilitySnapshotKey {
        provider_account_id: account.provider_account_id.clone(),
        scope: CapabilityScope::Resource {
            resource_kind: CloudResourceKind::ManagedZone,
            resource: resource.clone(),
        },
    };
    let edge = CapabilitySnapshotKey {
        provider_account_id: account.provider_account_id.clone(),
        scope: CapabilityScope::Resource {
            resource_kind: CloudResourceKind::EdgeApplication,
            resource,
        },
    };

    let keys = [&account, &region, &other, &zone, &edge];
    let mut expected = Vec::new();
    for (index, key) in keys.into_iter().enumerate() {
        let fence = store
            .begin_discovery(key, 1, Some("same-revision"))
            .await
            .expect("isolated fence");
        let value = snapshot(key, fence.clone(), 3_000 + index as i64);
        assert_eq!(
            store
                .put_if_current(key, &fence, &value)
                .await
                .expect("isolated put"),
            CapabilityStoreWrite::Stored
        );
        expected.push(value);
    }

    for (key, expected) in keys.into_iter().zip(expected) {
        assert_eq!(
            store.get(key).await.expect("isolated snapshot"),
            Some(expected)
        );
    }
}

pub async fn assert_exact_revision_invalidation(store: &dyn CapabilitySnapshotStore, prefix: &str) {
    let old = account_key(prefix, "invalidate");
    let newer_scope = CapabilitySnapshotKey {
        provider_account_id: old.provider_account_id.clone(),
        scope: CapabilityScope::Region {
            region: ProviderRegion::new("new-generation").expect("region"),
        },
    };
    let second_old_scope = CapabilitySnapshotKey {
        provider_account_id: old.provider_account_id.clone(),
        scope: CapabilityScope::Region {
            region: ProviderRegion::new("also-old").expect("region"),
        },
    };
    let newer_revision_scope = CapabilitySnapshotKey {
        provider_account_id: old.provider_account_id.clone(),
        scope: CapabilityScope::Region {
            region: ProviderRegion::new("new-revision").expect("region"),
        },
    };
    let other = account_key(prefix, "invalidate-other");

    put(store, &old, 4, Some("Rev-A"), 4_000).await;
    put(store, &second_old_scope, 4, Some("Rev-A"), 4_500).await;
    put(store, &newer_scope, 5, Some("Rev-A"), 5_000).await;
    put(store, &newer_revision_scope, 4, Some("rev-a"), 6_000).await;
    put(store, &other, 4, Some("Rev-A"), 7_000).await;

    store
        .invalidate_account_revision(&old.provider_account_id, 4, Some("Rev-A"))
        .await
        .expect("invalidate exact stale revision");
    assert_eq!(store.get(&old).await.expect("old removed"), None);
    assert_eq!(
        store
            .get(&second_old_scope)
            .await
            .expect("second old scope removed"),
        None
    );
    assert!(store
        .get(&newer_scope)
        .await
        .expect("new generation")
        .is_some());
    assert!(store
        .get(&newer_revision_scope)
        .await
        .expect("case-sensitive revision")
        .is_some());
    assert!(store.get(&other).await.expect("other account").is_some());

    let race = account_key(prefix, "invalidate-race");
    let stale = store
        .begin_discovery(&race, 8, Some("old"))
        .await
        .expect("stale race fence");
    let current = store
        .begin_discovery(&race, 9, Some("new"))
        .await
        .expect("current race fence");
    store
        .invalidate_account_revision(&race.provider_account_id, 8, Some("old"))
        .await
        .expect("delayed invalidation");
    assert_eq!(
        store
            .put_if_current(&race, &stale, &snapshot(&race, stale.clone(), 8_000))
            .await
            .expect("stale race put"),
        CapabilityStoreWrite::FenceLost
    );
    let current_snapshot = snapshot(&race, current.clone(), 9_000);
    assert_eq!(
        store
            .put_if_current(&race, &current, &current_snapshot)
            .await
            .expect("current race put"),
        CapabilityStoreWrite::Stored
    );
    assert_eq!(
        store.get(&race).await.expect("race winner"),
        Some(current_snapshot)
    );

    let revoked = account_key(prefix, "invalidate-active");
    let revoked_fence = store
        .begin_discovery(&revoked, 10, Some("revoked"))
        .await
        .expect("revoked active fence");
    store
        .invalidate_account_revision(&revoked.provider_account_id, 10, Some("revoked"))
        .await
        .expect("revoke active authority");
    assert_eq!(
        store
            .put_if_current(
                &revoked,
                &revoked_fence,
                &snapshot(&revoked, revoked_fence.clone(), 10_000),
            )
            .await
            .expect("revoked writer is fenced"),
        CapabilityStoreWrite::FenceLost
    );
    assert_eq!(
        store.get(&revoked).await.expect("revoked snapshot absent"),
        None
    );
    let recovered = store
        .begin_discovery(&revoked, 11, Some("replacement"))
        .await
        .expect("replacement authority");
    assert!(recovered.discovery_epoch > revoked_fence.discovery_epoch);

    let old_committed = account_key(prefix, "old-committed-new-active");
    put(store, &old_committed, 12, Some("old"), 12_000).await;
    let new_active = store
        .begin_discovery(&old_committed, 13, Some("new"))
        .await
        .expect("new active authority");
    store
        .invalidate_account_revision(&old_committed.provider_account_id, 12, Some("old"))
        .await
        .expect("remove old committed snapshot");
    assert_eq!(
        store
            .get(&old_committed)
            .await
            .expect("old committed removed"),
        None
    );
    let new_snapshot = snapshot(&old_committed, new_active.clone(), 13_000);
    assert_eq!(
        store
            .put_if_current(&old_committed, &new_active, &new_snapshot)
            .await
            .expect("new active remains valid"),
        CapabilityStoreWrite::Stored
    );

    let new_committed = account_key(prefix, "new-committed-old-active");
    put(store, &new_committed, 15, Some("new"), 15_000).await;
    let delayed_old = store
        .begin_discovery(&new_committed, 14, Some("old"))
        .await
        .expect("delayed old authority");
    store
        .invalidate_account_revision(&new_committed.provider_account_id, 14, Some("old"))
        .await
        .expect("revoke delayed old authority");
    let preserved = store
        .get(&new_committed)
        .await
        .expect("new committed preserved")
        .expect("new committed snapshot");
    assert_eq!(preserved.fence.provider_account_generation, 15);
    assert_eq!(preserved.fence.credential_revision.as_deref(), Some("new"));
    assert_eq!(
        store
            .put_if_current(
                &new_committed,
                &delayed_old,
                &snapshot(&new_committed, delayed_old.clone(), 14_000),
            )
            .await
            .expect("delayed old writer fenced"),
        CapabilityStoreWrite::FenceLost
    );
}

async fn put(
    store: &dyn CapabilitySnapshotStore,
    key: &CapabilitySnapshotKey,
    generation: u64,
    revision: Option<&str>,
    discovered_at: i64,
) {
    let fence = store
        .begin_discovery(key, generation, revision)
        .await
        .expect("begin discovery");
    let value = snapshot(key, fence.clone(), discovered_at);
    assert_eq!(
        store
            .put_if_current(key, &fence, &value)
            .await
            .expect("put snapshot"),
        CapabilityStoreWrite::Stored
    );
}

fn account_key(prefix: &str, suffix: &str) -> CapabilitySnapshotKey {
    CapabilitySnapshotKey {
        provider_account_id: CloudResourceId::new(format!("{prefix}-{suffix}"))
            .expect("provider account id"),
        scope: CapabilityScope::Account,
    }
}

fn snapshot(
    key: &CapabilitySnapshotKey,
    fence: super::CapabilityDiscoveryFence,
    discovered_at_unix_ms: i64,
) -> ProviderCapabilitySnapshot {
    let request = CapabilityDiscoveryRequest {
        provider_account_id: key.provider_account_id.clone(),
        fence,
        account: ProviderAccountSpec {
            provider: CloudProvider::Cloudflare,
            scope: None,
            credential_source: CredentialSource::Ambient,
        },
        scope: key.scope.clone(),
    };
    ProviderCapabilitySnapshot::from_report(
        &request,
        discovered_at_unix_ms,
        CapabilityDiscoveryReport {
            state: CapabilityDiscoveryState::Partial,
            observations: vec![CapabilityObservation {
                capability: ProviderCapability::Dns(DnsCapability::RecordSets),
                dimensions: vec![
                    affirmative_dimension(
                        CapabilityDimension::AdapterSupport,
                        None,
                        discovered_at_unix_ms,
                    ),
                    affirmative_dimension(
                        CapabilityDimension::ProviderSupport,
                        None,
                        discovered_at_unix_ms,
                    ),
                    affirmative_dimension(
                        CapabilityDimension::Entitlement,
                        None,
                        discovered_at_unix_ms,
                    ),
                    affirmative_dimension(
                        CapabilityDimension::Location,
                        None,
                        discovered_at_unix_ms,
                    ),
                    affirmative_dimension(
                        CapabilityDimension::Access,
                        Some(CapabilityAction::Create),
                        discovered_at_unix_ms,
                    ),
                    CapabilityDimensionObservation {
                        dimension: CapabilityDimension::Quota,
                        action: Some(CapabilityAction::Create),
                        state: TriState::NotApplicable,
                        reason: None,
                        evidence: CapabilityEvidence::QuotaProbe,
                        code: None,
                        message: None,
                        observed_at_unix_ms: discovered_at_unix_ms,
                        valid_until_unix_ms: discovered_at_unix_ms + 60_000,
                    },
                ],
            }],
            issues: vec![CapabilityDiscoveryIssue {
                severity: CapabilityIssueSeverity::Warning,
                scope: CapabilityIssueScope::Account,
                reason: CapabilityReason::ProviderUnavailable,
                code: SanitizedCapabilityCode::new("conformance_partial").expect("diagnostic code"),
                message: SanitizedCapabilityMessage::new("Conformance discovery was partial.")
                    .expect("diagnostic message"),
            }],
        },
    )
    .expect("conformance snapshot")
}

fn affirmative_dimension(
    dimension: CapabilityDimension,
    action: Option<CapabilityAction>,
    observed_at_unix_ms: i64,
) -> CapabilityDimensionObservation {
    CapabilityDimensionObservation {
        dimension,
        action,
        state: TriState::Affirmative,
        reason: None,
        evidence: match dimension {
            CapabilityDimension::AdapterSupport => CapabilityEvidence::AdapterContract,
            CapabilityDimension::Access => CapabilityEvidence::PermissionProbe,
            CapabilityDimension::Quota => CapabilityEvidence::QuotaProbe,
            _ => CapabilityEvidence::ProviderProbe,
        },
        code: (dimension == CapabilityDimension::Access)
            .then(|| SanitizedCapabilityCode::new("permission_confirmed").expect("code")),
        message: (dimension == CapabilityDimension::Access).then(|| {
            SanitizedCapabilityMessage::new("Write permission was confirmed.").expect("message")
        }),
        observed_at_unix_ms,
        valid_until_unix_ms: observed_at_unix_ms + 60_000,
    }
}
