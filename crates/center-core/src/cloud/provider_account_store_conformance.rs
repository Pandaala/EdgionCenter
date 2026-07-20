//! Shared behavioral assertions for ProviderAccount persistence adapters.

use std::{
    collections::{BTreeMap, BTreeSet},
    future::{poll_fn, Future},
    pin::pin,
    task::Poll,
};

use super::{
    CloudProvider, CloudResourceId, CredentialRef, CredentialSource, DeletionPolicy,
    ManagementPolicy, ProviderAccountCreateResult, ProviderAccountDesired,
    ProviderAccountPageRequest, ProviderAccountReplaceResult, ProviderAccountScope,
    ProviderAccountSpec, ProviderAccountStore,
};

/// Exercises the complete first-slice ProviderAccountStore contract.
pub async fn assert_provider_account_store_conformance(
    store: &dyn ProviderAccountStore,
    prefix: &str,
) {
    assert_create_get_and_duplicate(store, prefix).await;
    assert_exact_generation_replacement(store, prefix).await;
    assert_concurrent_create_and_replace(store, prefix).await;
    assert_google_credential_variants_roundtrip(store, prefix).await;
    assert_stable_keyset_pagination(store, prefix).await;
    assert_bounds_fail_before_persistence(store, prefix).await;
}

async fn assert_create_get_and_duplicate(store: &dyn ProviderAccountStore, prefix: &str) {
    let id = account_id(prefix, "roundtrip");
    let desired = cloudflare_desired("cloudflare/production");
    let created = store.create(&id, &desired).await.expect("create account");
    let ProviderAccountCreateResult::Created(created) = created else {
        panic!("fresh account was not created");
    };
    assert_eq!(created.metadata.id, id);
    assert_eq!(created.metadata.generation, 1);
    assert_eq!(created.spec, desired.spec);
    assert!(created.status.observed_generation.is_none());
    assert_eq!(
        store.get(&id).await.expect("get account"),
        Some(created.as_ref().clone())
    );

    let conflicting = aws_desired();
    assert_eq!(
        store
            .create(&id, &conflicting)
            .await
            .expect("duplicate create"),
        ProviderAccountCreateResult::AlreadyExists
    );
    assert_eq!(
        store.get(&id).await.expect("duplicate preserved"),
        Some(*created)
    );
}

async fn assert_exact_generation_replacement(store: &dyn ProviderAccountStore, prefix: &str) {
    let id = account_id(prefix, "replace");
    let first = store
        .create(&id, &cloudflare_desired("cloudflare/replace"))
        .await
        .expect("create replacement target");
    assert!(matches!(first, ProviderAccountCreateResult::Created(_)));

    let desired = aws_desired();
    let replaced = store
        .replace_if_generation(&id, 1, &desired)
        .await
        .expect("replace account");
    let ProviderAccountReplaceResult::Stored(replaced) = replaced else {
        panic!("exact generation was not stored");
    };
    assert_eq!(replaced.metadata.id, id);
    assert_eq!(replaced.metadata.generation, 2);
    assert_eq!(replaced.spec, desired.spec);

    assert_eq!(
        store
            .replace_if_generation(&id, 1, &cloudflare_desired("cloudflare/stale"))
            .await
            .expect("stale replacement outcome"),
        ProviderAccountReplaceResult::GenerationMismatch {
            actual_generation: 2
        }
    );
    assert_eq!(
        store.get(&id).await.expect("stale write preserved"),
        Some(*replaced)
    );

    let missing = account_id(prefix, "missing");
    assert_eq!(
        store
            .replace_if_generation(&missing, 1, &desired)
            .await
            .expect("missing replacement outcome"),
        ProviderAccountReplaceResult::NotFound
    );
    assert!(store.replace_if_generation(&id, 0, &desired).await.is_err());
    assert!(store
        .replace_if_generation(&id, i64::MAX as u64, &desired)
        .await
        .is_err());
}

async fn assert_concurrent_create_and_replace(store: &dyn ProviderAccountStore, prefix: &str) {
    let create_id = account_id(prefix, "concurrent-create");
    let left_create = cloudflare_desired("cloudflare/concurrent-left");
    let right_create = aws_desired();
    let (left, right) = join_pair(
        store.create(&create_id, &left_create),
        store.create(&create_id, &right_create),
    )
    .await;
    let outcomes = [
        left.expect("left concurrent create"),
        right.expect("right concurrent create"),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, ProviderAccountCreateResult::Created(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, ProviderAccountCreateResult::AlreadyExists))
            .count(),
        1
    );
    let mut create_winner = None;
    for (outcome, expected) in [(&outcomes[0], &left_create), (&outcomes[1], &right_create)] {
        if let ProviderAccountCreateResult::Created(account) = outcome {
            assert_eq!(account.metadata.generation, 1);
            assert_eq!(&account.spec, &expected.spec);
            create_winner = Some(account.as_ref().clone());
        }
    }
    assert_eq!(
        store
            .get(&create_id)
            .await
            .expect("get concurrent create winner"),
        create_winner
    );

    let replace_id = account_id(prefix, "concurrent-replace");
    assert!(matches!(
        store
            .create(
                &replace_id,
                &cloudflare_desired("cloudflare/concurrent-base")
            )
            .await
            .expect("create concurrent replacement target"),
        ProviderAccountCreateResult::Created(_)
    ));
    let left_desired = aws_desired();
    let right_desired = google_federated_desired();
    let (left, right) = join_pair(
        store.replace_if_generation(&replace_id, 1, &left_desired),
        store.replace_if_generation(&replace_id, 1, &right_desired),
    )
    .await;
    let left = left.expect("left concurrent replacement");
    let right = right.expect("right concurrent replacement");
    let mut winner = None;
    let mut stored = 0;
    let mut mismatched = 0;
    for (outcome, expected) in [(&left, &left_desired), (&right, &right_desired)] {
        match outcome {
            ProviderAccountReplaceResult::Stored(account) => {
                stored += 1;
                assert_eq!(account.metadata.generation, 2);
                assert_eq!(&account.spec, &expected.spec);
                winner = Some(account.as_ref().clone());
            }
            ProviderAccountReplaceResult::GenerationMismatch {
                actual_generation: 2,
            } => mismatched += 1,
            other => panic!("unexpected concurrent replacement outcome: {other:?}"),
        }
    }
    assert_eq!(stored, 1);
    assert_eq!(mismatched, 1);
    assert_eq!(
        store
            .get(&replace_id)
            .await
            .expect("get concurrent replacement winner"),
        winner
    );
}

async fn assert_google_credential_variants_roundtrip(
    store: &dyn ProviderAccountStore,
    prefix: &str,
) {
    let id = account_id(prefix, "google-credentials");
    let federated = google_federated_desired();
    let created = store
        .create(&id, &federated)
        .await
        .expect("create Google federated account");
    let ProviderAccountCreateResult::Created(created) = created else {
        panic!("fresh Google account was not created");
    };
    assert_eq!(created.spec, federated.spec);
    assert_eq!(
        store.get(&id).await.expect("get Google federated account"),
        Some(created.as_ref().clone())
    );

    let assumed = google_assumed_identity_desired();
    let replaced = store
        .replace_if_generation(&id, 1, &assumed)
        .await
        .expect("replace with Google assumed identity");
    let ProviderAccountReplaceResult::Stored(replaced) = replaced else {
        panic!("Google assumed identity was not stored");
    };
    assert_eq!(replaced.metadata.generation, 2);
    assert_eq!(replaced.spec, assumed.spec);
    assert_eq!(
        store
            .get(&id)
            .await
            .expect("get Google assumed identity account"),
        Some(*replaced)
    );
}

async fn assert_stable_keyset_pagination(store: &dyn ProviderAccountStore, prefix: &str) {
    let target_ids =
        ["page-A", "page-a", "page-b", "page-z"].map(|suffix| account_id(prefix, suffix));
    for (index, account_id) in target_ids.iter().enumerate() {
        let result = store
            .create(
                account_id,
                &cloudflare_desired(&format!("cloudflare/page-{index}")),
            )
            .await
            .expect("create paged account");
        assert!(matches!(result, ProviderAccountCreateResult::Created(_)));
    }

    let mut after = None;
    let mut visited = Vec::new();
    loop {
        let request = ProviderAccountPageRequest {
            limit: 2,
            after: after.clone(),
        };
        let page = store.list(&request).await.expect("list account page");
        page.validate(&request).expect("valid account page");
        visited.extend(page.items.into_iter().map(|account| account.metadata.id));
        let Some(next) = page.next else {
            break;
        };
        after = Some(next);
        assert!(
            visited.len() < 10_000,
            "provider account pagination did not terminate"
        );
    }
    assert!(visited
        .windows(2)
        .all(|pair| { pair[0].as_str().as_bytes() < pair[1].as_str().as_bytes() }));
    assert_eq!(visited.iter().collect::<BTreeSet<_>>().len(), visited.len());
    for expected in target_ids {
        assert!(visited.contains(&expected));
    }

    let request = ProviderAccountPageRequest {
        limit: 100,
        after: Some(account_id(prefix, "page-A")),
    };
    let page = store.list(&request).await.expect("list exclusive boundary");
    assert!(!page
        .items
        .iter()
        .any(|account| account.metadata.id == request.after.clone().unwrap()));
}

async fn assert_bounds_fail_before_persistence(store: &dyn ProviderAccountStore, prefix: &str) {
    let invalid_id = account_id(prefix, "invalid");
    let mut invalid = cloudflare_desired("cloudflare/invalid");
    invalid.display_name = "x".repeat(257);
    assert!(store.create(&invalid_id, &invalid).await.is_err());
    assert_eq!(store.get(&invalid_id).await.expect("invalid absent"), None);

    let mut invalid = cloudflare_desired("cloudflare/invalid");
    invalid.labels = (0..65)
        .map(|index| (format!("key-{index}"), String::new()))
        .collect();
    assert!(store.create(&invalid_id, &invalid).await.is_err());

    let mut invalid = cloudflare_desired("cloudflare/invalid");
    invalid.labels = BTreeMap::from([(" unsafe".to_string(), "value".to_string())]);
    assert!(store.create(&invalid_id, &invalid).await.is_err());

    let mut invalid = cloudflare_desired("cloudflare/invalid");
    invalid.labels = BTreeMap::from([("key".to_string(), "value\n".to_string())]);
    assert!(store.create(&invalid_id, &invalid).await.is_err());

    let mut invalid = cloudflare_desired("cloudflare/invalid");
    invalid.deletion_policy = DeletionPolicy::DeleteExternal;
    assert!(store.create(&invalid_id, &invalid).await.is_err());

    let mut invalid = cloudflare_desired("cloudflare/invalid");
    invalid.spec.credential_source = CredentialSource::StaticSecret {
        credential_ref: CredentialRef::new("x".repeat(513)).expect("core allows long aliases"),
    };
    assert!(store.create(&invalid_id, &invalid).await.is_err());

    let mut invalid = cloudflare_desired("cloudflare/invalid");
    invalid.spec.credential_source = CredentialSource::Federated {
        subject_token_ref: None,
        target_principal: "p".repeat(64 * 1024),
        audience: None,
    };
    assert!(store.create(&invalid_id, &invalid).await.is_err());
}

fn account_id(prefix: &str, suffix: &str) -> CloudResourceId {
    CloudResourceId::new(format!("{prefix}/provider-account/{suffix}"))
        .expect("conformance account ID")
}

fn cloudflare_desired(credential_ref: &str) -> ProviderAccountDesired {
    ProviderAccountDesired {
        display_name: "Cloudflare production".to_string(),
        owner: Some("platform".to_string()),
        labels: BTreeMap::from([
            ("environment".to_string(), "production".to_string()),
            ("team".to_string(), "edge".to_string()),
        ]),
        management_policy: ManagementPolicy::ObserveOnly,
        deletion_policy: DeletionPolicy::Retain,
        spec: ProviderAccountSpec {
            provider: CloudProvider::Cloudflare,
            scope: Some(ProviderAccountScope::Cloudflare {
                account_id: "0123456789abcdef0123456789abcdef".to_string(),
            }),
            credential_source: CredentialSource::StaticSecret {
                credential_ref: CredentialRef::new(credential_ref).expect("credential reference"),
            },
        },
    }
}

fn aws_desired() -> ProviderAccountDesired {
    ProviderAccountDesired {
        display_name: "AWS DNS".to_string(),
        owner: None,
        labels: BTreeMap::new(),
        management_policy: ManagementPolicy::ObserveOnly,
        deletion_policy: DeletionPolicy::Retain,
        spec: ProviderAccountSpec {
            provider: CloudProvider::Aws,
            scope: Some(ProviderAccountScope::Aws {
                account_id: "123456789012".to_string(),
            }),
            credential_source: CredentialSource::Ambient,
        },
    }
}

fn google_federated_desired() -> ProviderAccountDesired {
    ProviderAccountDesired {
        display_name: "Google Cloud DNS federated".to_string(),
        owner: Some("dns-platform".to_string()),
        labels: BTreeMap::from([("identity".to_string(), "federated".to_string())]),
        management_policy: ManagementPolicy::ObserveOnly,
        deletion_policy: DeletionPolicy::Retain,
        spec: ProviderAccountSpec {
            provider: CloudProvider::GoogleCloud,
            scope: Some(ProviderAccountScope::GoogleCloud {
                project_id: "edgion-dns-prod".to_string(),
            }),
            credential_source: CredentialSource::Federated {
                subject_token_ref: Some(
                    CredentialRef::new("google/projected-token").expect("subject token ref"),
                ),
                target_principal: "dns-center@edgion-dns-prod.iam.gserviceaccount.com".to_string(),
                audience: Some("//iam.googleapis.com/projects/123/locations/global/workloadIdentityPools/center/providers/kubernetes".to_string()),
            },
        },
    }
}

fn google_assumed_identity_desired() -> ProviderAccountDesired {
    ProviderAccountDesired {
        display_name: "Google Cloud DNS impersonated".to_string(),
        owner: Some("dns-platform".to_string()),
        labels: BTreeMap::from([("identity".to_string(), "impersonated".to_string())]),
        management_policy: ManagementPolicy::ObserveOnly,
        deletion_policy: DeletionPolicy::Retain,
        spec: ProviderAccountSpec {
            provider: CloudProvider::GoogleCloud,
            scope: Some(ProviderAccountScope::GoogleCloud {
                project_id: "edgion-dns-prod".to_string(),
            }),
            credential_source: CredentialSource::AssumeIdentity {
                base_credential_ref: Some(
                    CredentialRef::new("google/base-identity").expect("base credential ref"),
                ),
                target_principal: "dns-admin@edgion-dns-prod.iam.gserviceaccount.com".to_string(),
                external_id_ref: Some(
                    CredentialRef::new("google/external-id").expect("external ID ref"),
                ),
            },
        },
    }
}

async fn join_pair<A, B>(left: impl Future<Output = A>, right: impl Future<Output = B>) -> (A, B) {
    let mut left = pin!(left);
    let mut right = pin!(right);
    let mut left_output = None;
    let mut right_output = None;
    poll_fn(|context| {
        if left_output.is_none() {
            if let Poll::Ready(output) = left.as_mut().poll(context) {
                left_output = Some(output);
            }
        }
        if right_output.is_none() {
            if let Poll::Ready(output) = right.as_mut().poll(context) {
                right_output = Some(output);
            }
        }
        match (left_output.take(), right_output.take()) {
            (Some(left), Some(right)) => Poll::Ready((left, right)),
            (left, right) => {
                left_output = left;
                right_output = right;
                Poll::Pending
            }
        }
    })
    .await
}
