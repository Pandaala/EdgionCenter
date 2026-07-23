//! Shared behavioral assertions for DNS provider adapters.

use std::collections::BTreeSet;

use super::{
    CloudProvider, CloudResourceId, DnsChangeReceipt, DnsGuardStrength, DnsMutationGuard, DnsPage,
    DnsPageRequest, DnsProvider, DnsRecordChange, DnsRecordRevision, DnsRoutingIdentity, DnsTtl,
    DnsZoneRef, NormalizedProviderError, ObservedDnsRecordSet, ObservedDnsZone,
    ProviderDnsRecordSet, ProviderDnsRecordType, ProviderErrorCategory,
};

/// Isolated provider state used by the shared DNS adapter conformance suite.
///
/// Implementations should provision two zones in one account, at least two
/// record sets in `primary_zone`, and an unused create/replace pair before
/// invoking the assertions. The replacement must preserve the create key.
#[derive(Debug, Clone)]
pub struct DnsAdapterConformanceFixture {
    pub provider: CloudProvider,
    pub provider_account_id: CloudResourceId,
    pub other_account_id: CloudResourceId,
    pub primary_zone: ObservedDnsZone,
    pub secondary_zone: ObservedDnsZone,
    pub primary_records: Vec<ObservedDnsRecordSet>,
    pub create_record: ProviderDnsRecordSet,
    pub replacement_record: ProviderDnsRecordSet,
    pub maximum_guard: DnsGuardStrength,
}

impl DnsAdapterConformanceFixture {
    pub fn validate(&self) {
        self.provider_account_id
            .validate()
            .expect("provider account ID");
        self.other_account_id.validate().expect("other account ID");
        assert_ne!(self.provider_account_id, self.other_account_id);
        self.primary_zone.validate().expect("primary zone");
        self.secondary_zone.validate().expect("secondary zone");
        assert_eq!(self.primary_zone.zone.provider, self.provider);
        assert_eq!(self.secondary_zone.zone.provider, self.provider);
        assert_eq!(
            self.primary_zone.zone.provider_account_id,
            self.provider_account_id
        );
        assert_eq!(
            self.secondary_zone.zone.provider_account_id,
            self.provider_account_id
        );
        assert_ne!(
            self.primary_zone.zone.zone_id,
            self.secondary_zone.zone.zone_id
        );
        assert!(self.primary_records.len() >= 3);
        for record in &self.primary_records {
            record.validate().expect("seed record");
            assert_eq!(record.zone, self.primary_zone.zone);
        }
        self.create_record
            .validate(&self.primary_zone.zone)
            .expect("create record");
        self.replacement_record
            .validate(&self.primary_zone.zone)
            .expect("replacement record");
        assert_eq!(self.create_record.key, self.replacement_record.key);
        assert_eq!(
            self.create_record.key.record_type,
            ProviderDnsRecordType::Txt
        );
        assert_eq!(self.create_record.key.routing, DnsRoutingIdentity::Simple);
        assert!(matches!(self.create_record.ttl, DnsTtl::Seconds(_)));
        assert!(self.create_record.extension.is_none());
        assert!(self
            .primary_records
            .iter()
            .all(|record| record.record_set.key != self.create_record.key));
    }
}

/// Runs the complete provider-neutral DNS adapter contract.
pub async fn assert_dns_provider_conformance(
    provider: &dyn DnsProvider,
    fixture: &DnsAdapterConformanceFixture,
) {
    assert_dns_inventory_conformance(provider, fixture).await;
    assert_dns_batch_atomicity_conformance(provider, fixture).await;
    assert_dns_mutation_conformance(provider, fixture).await;
}

/// Verifies inventory round trips, response scope, and opaque cursor binding.
pub async fn assert_dns_inventory_conformance(
    provider: &dyn DnsProvider,
    fixture: &DnsAdapterConformanceFixture,
) {
    fixture.validate();
    let zone = provider
        .get_zone(&fixture.primary_zone.zone)
        .await
        .expect("get primary zone")
        .expect("primary zone exists");
    zone.validate().expect("valid observed zone");
    assert_eq!(zone, fixture.primary_zone);

    let first_zone_page = provider
        .list_zones(
            &fixture.provider_account_id,
            &DnsPageRequest {
                limit: 1,
                token: None,
            },
        )
        .await
        .expect("first zone page");
    validate_zone_page(&first_zone_page, fixture, 1);
    let zone_token = first_zone_page
        .next
        .clone()
        .expect("two zones require a cursor");
    let second_zone_page = provider
        .list_zones(
            &fixture.provider_account_id,
            &DnsPageRequest {
                limit: 1,
                token: Some(zone_token.clone()),
            },
        )
        .await
        .expect("second zone page");
    validate_zone_page(&second_zone_page, fixture, 1);
    assert_ne!(first_zone_page.items, second_zone_page.items);
    let actual_zones = first_zone_page
        .items
        .iter()
        .chain(&second_zone_page.items)
        .map(|zone| zone.zone.clone())
        .collect::<Vec<_>>();
    assert_eq!(actual_zones.len(), 2);
    assert!(actual_zones.contains(&fixture.primary_zone.zone));
    assert!(actual_zones.contains(&fixture.secondary_zone.zone));

    expect_validation(
        provider
            .list_zones(
                &fixture.other_account_id,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(zone_token.clone()),
                },
            )
            .await,
        "zone cursor reused for another account",
    );
    match provider
        .list_zones(
            &fixture.other_account_id,
            &DnsPageRequest {
                limit: 10,
                token: None,
            },
        )
        .await
    {
        Ok(page) => page
            .validate(10, |zone| {
                zone.validate()?;
                if zone.zone.provider_account_id != fixture.other_account_id {
                    return Err(crate::CoreError::Conflict(
                        "DNS zone escaped the requested account".to_string(),
                    ));
                }
                Ok(())
            })
            .expect("other account zone filtering"),
        Err(error) => {
            error.validate().expect("normalized provider error");
            assert!(matches!(
                error.category(),
                ProviderErrorCategory::Validation
                    | ProviderErrorCategory::NotFound
                    | ProviderErrorCategory::Authorization
            ));
        }
    }
    expect_validation(
        provider
            .list_record_sets(
                &fixture.primary_zone.zone,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(zone_token),
                },
            )
            .await,
        "zone cursor reused for another list method",
    );

    let first_record_page = provider
        .list_record_sets(
            &fixture.primary_zone.zone,
            &DnsPageRequest {
                limit: 1,
                token: None,
            },
        )
        .await
        .expect("first record page");
    validate_record_page(&first_record_page, &fixture.primary_zone.zone, 1);
    let record_token = first_record_page
        .next
        .clone()
        .expect("two records require a cursor");
    let second_record_page = provider
        .list_record_sets(
            &fixture.primary_zone.zone,
            &DnsPageRequest {
                limit: 1,
                token: Some(record_token.clone()),
            },
        )
        .await
        .expect("second record page");
    validate_record_page(&second_record_page, &fixture.primary_zone.zone, 1);
    assert_ne!(first_record_page.items, second_record_page.items);
    let replayed_record_page = provider
        .list_record_sets(
            &fixture.primary_zone.zone,
            &DnsPageRequest {
                limit: 1,
                token: first_record_page.next.clone(),
            },
        )
        .await
        .expect("replay record page");
    assert_eq!(replayed_record_page, second_record_page);
    expect_validation(
        provider
            .list_record_sets(
                &fixture.secondary_zone.zone,
                &DnsPageRequest {
                    limit: 1,
                    token: Some(record_token),
                },
            )
            .await,
        "record cursor reused for another zone",
    );

    let mut actual_keys = BTreeSet::new();
    let mut token = None;
    for _ in 0..=fixture.primary_records.len() {
        let page = provider
            .list_record_sets(
                &fixture.primary_zone.zone,
                &DnsPageRequest { limit: 2, token },
            )
            .await
            .expect("traverse record pages");
        validate_record_page_max(&page, &fixture.primary_zone.zone, 2);
        for record in page.items {
            assert!(actual_keys.insert(record.record_set.key));
        }
        token = page.next;
        if token.is_none() {
            break;
        }
    }
    assert!(token.is_none(), "record pagination did not terminate");
    let expected_keys = fixture
        .primary_records
        .iter()
        .map(|record| record.record_set.key.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_keys, expected_keys);

    for expected in &fixture.primary_records {
        let actual = provider
            .get_record_set(&fixture.primary_zone.zone, &expected.record_set.key)
            .await
            .expect("get record set")
            .expect("seed record exists");
        actual.validate().expect("valid observed record set");
        assert_eq!(&actual, expected);
    }

    let mut wrong_provider = fixture.primary_zone.zone.clone();
    wrong_provider.provider = different_provider(&fixture.provider);
    expect_validation(
        provider.get_zone(&wrong_provider).await,
        "declared provider differs from resolved account",
    );
    let mut wrong_account = fixture.primary_zone.zone.clone();
    wrong_account.provider_account_id = fixture.other_account_id.clone();
    expect_validation(
        provider.get_zone(&wrong_account).await,
        "zone reused for another account",
    );
}

/// Verifies guard negotiation, exact-revision mutation, and receipt polling.
pub async fn assert_dns_mutation_conformance(
    provider: &dyn DnsProvider,
    fixture: &DnsAdapterConformanceFixture,
) {
    fixture.validate();
    let zone = &fixture.primary_zone.zone;

    let scope_probe = DnsRecordChange::Create {
        record_set: fixture.create_record.clone(),
        guard: DnsMutationGuard::MustNotExist,
    };
    let mut wrong_provider = zone.clone();
    wrong_provider.provider = different_provider(&fixture.provider);
    expect_validation_or_conflict(
        provider
            .apply_record_changes(
                &wrong_provider,
                std::slice::from_ref(&scope_probe),
                DnsGuardStrength::BestEffort,
            )
            .await,
        "mutation with mismatched provider",
    );
    let mut wrong_account = zone.clone();
    wrong_account.provider_account_id = fixture.other_account_id.clone();
    expect_validation_or_conflict(
        provider
            .apply_record_changes(
                &wrong_account,
                std::slice::from_ref(&scope_probe),
                DnsGuardStrength::BestEffort,
            )
            .await,
        "mutation with mismatched account",
    );
    assert!(provider
        .get_record_set(zone, &fixture.create_record.key)
        .await
        .expect("read after rejected cross-scope mutations")
        .is_none());

    if fixture.maximum_guard == DnsGuardStrength::BestEffort {
        let unsupported = DnsRecordChange::Create {
            record_set: fixture.create_record.clone(),
            guard: DnsMutationGuard::MustNotExist,
        };
        expect_validation_or_conflict(
            provider
                .apply_record_changes(zone, &[unsupported], DnsGuardStrength::Atomic)
                .await,
            "unsupported atomic guard",
        );
        assert!(provider
            .get_record_set(zone, &fixture.create_record.key)
            .await
            .expect("read after rejected guard")
            .is_none());
    }

    let created_receipt = provider
        .apply_record_changes(
            zone,
            &[DnsRecordChange::Create {
                record_set: fixture.create_record.clone(),
                guard: DnsMutationGuard::MustNotExist,
            }],
            fixture.maximum_guard,
        )
        .await
        .expect("create record");
    validate_receipt(&created_receipt, fixture.maximum_guard);
    await_committed_receipt(provider, zone, &created_receipt, fixture.maximum_guard).await;
    expect_scope_rejection(
        provider
            .observe_change(&fixture.secondary_zone.zone, &created_receipt.id)
            .await,
        "change receipt reused for another zone",
    );

    let created = provider
        .get_record_set(zone, &fixture.create_record.key)
        .await
        .expect("read created record")
        .expect("created record exists");
    created.validate().expect("valid created record");
    assert_eq!(created.record_set, fixture.create_record);
    expect_validation_or_conflict(
        provider
            .apply_record_changes(
                zone,
                &[DnsRecordChange::Create {
                    record_set: fixture.create_record.clone(),
                    guard: DnsMutationGuard::MustNotExist,
                }],
                fixture.maximum_guard,
            )
            .await,
        "duplicate create",
    );

    let stale_revision =
        DnsRecordRevision::new(format!("{}-stale", created.revision)).expect("stale revision");
    let mut stale_previous = created.clone();
    stale_previous.revision = stale_revision.clone();
    expect_validation_or_conflict(
        provider
            .apply_record_changes(
                zone,
                &[DnsRecordChange::Replace {
                    previous: stale_previous,
                    desired: fixture.replacement_record.clone(),
                    guard: DnsMutationGuard::MatchObserved {
                        revision: stale_revision,
                    },
                }],
                fixture.maximum_guard,
            )
            .await,
        "stale replacement revision",
    );
    assert_eq!(
        provider
            .get_record_set(zone, &fixture.create_record.key)
            .await
            .expect("read after stale replacement")
            .expect("record remains")
            .record_set,
        fixture.create_record
    );

    let replaced_receipt = provider
        .apply_record_changes(
            zone,
            &[DnsRecordChange::Replace {
                previous: created.clone(),
                desired: fixture.replacement_record.clone(),
                guard: DnsMutationGuard::MatchObserved {
                    revision: created.revision.clone(),
                },
            }],
            fixture.maximum_guard,
        )
        .await
        .expect("replace record");
    validate_receipt(&replaced_receipt, fixture.maximum_guard);
    await_committed_receipt(provider, zone, &replaced_receipt, fixture.maximum_guard).await;
    let replaced = provider
        .get_record_set(zone, &fixture.create_record.key)
        .await
        .expect("read replacement")
        .expect("replacement exists");
    assert_eq!(replaced.record_set, fixture.replacement_record);
    assert_ne!(replaced.revision, created.revision);

    expect_validation_or_conflict(
        provider
            .apply_record_changes(
                zone,
                &[DnsRecordChange::Delete {
                    previous: created.clone(),
                    guard: DnsMutationGuard::MatchObserved {
                        revision: created.revision.clone(),
                    },
                }],
                fixture.maximum_guard,
            )
            .await,
        "stale deletion revision",
    );

    let deleted_receipt = provider
        .apply_record_changes(
            zone,
            &[DnsRecordChange::Delete {
                previous: replaced.clone(),
                guard: DnsMutationGuard::MatchObserved {
                    revision: replaced.revision,
                },
            }],
            fixture.maximum_guard,
        )
        .await
        .expect("delete record");
    validate_receipt(&deleted_receipt, fixture.maximum_guard);
    await_committed_receipt(provider, zone, &deleted_receipt, fixture.maximum_guard).await;
    assert!(provider
        .get_record_set(zone, &fixture.create_record.key)
        .await
        .expect("read deleted record")
        .is_none());
}

/// Verifies that an adapter declaring all-or-nothing submission does not
/// commit an earlier change when a later change conflicts with provider state.
pub async fn assert_dns_batch_atomicity_conformance(
    provider: &dyn DnsProvider,
    fixture: &DnsAdapterConformanceFixture,
) {
    fixture.validate();
    let zone = &fixture.primary_zone.zone;
    assert!(provider
        .get_record_set(zone, &fixture.create_record.key)
        .await
        .expect("read unused record")
        .is_none());
    let existing = fixture.primary_records.first().expect("seed record");
    let result = provider
        .apply_record_changes(
            zone,
            &[
                DnsRecordChange::Create {
                    record_set: fixture.create_record.clone(),
                    guard: DnsMutationGuard::MustNotExist,
                },
                DnsRecordChange::Create {
                    record_set: existing.record_set.clone(),
                    guard: DnsMutationGuard::MustNotExist,
                },
            ],
            fixture.maximum_guard,
        )
        .await;
    expect_validation_or_conflict(result, "conflicting all-or-nothing batch");
    assert!(provider
        .get_record_set(zone, &fixture.create_record.key)
        .await
        .expect("read after rejected batch")
        .is_none());
    assert_eq!(
        provider
            .get_record_set(zone, &existing.record_set.key)
            .await
            .expect("read existing record")
            .expect("existing record remains"),
        *existing
    );
}

fn validate_zone_page(
    page: &DnsPage<ObservedDnsZone>,
    fixture: &DnsAdapterConformanceFixture,
    limit: u16,
) {
    page.validate(limit, |zone| {
        zone.validate()?;
        if zone.zone.provider != fixture.provider
            || zone.zone.provider_account_id != fixture.provider_account_id
        {
            return Err(crate::CoreError::Conflict(
                "DNS zone escaped the requested provider account".to_string(),
            ));
        }
        Ok(())
    })
    .expect("valid zone page");
    assert_eq!(page.items.len(), 1);
}

fn validate_record_page(page: &DnsPage<ObservedDnsRecordSet>, zone: &DnsZoneRef, limit: u16) {
    validate_record_page_max(page, zone, limit);
    assert_eq!(page.items.len(), 1);
}

fn validate_record_page_max(page: &DnsPage<ObservedDnsRecordSet>, zone: &DnsZoneRef, limit: u16) {
    page.validate(limit, |record| {
        record.validate()?;
        if &record.zone != zone {
            return Err(crate::CoreError::Conflict(
                "DNS record escaped the requested zone".to_string(),
            ));
        }
        Ok(())
    })
    .expect("valid record page");
}

fn validate_receipt(receipt: &DnsChangeReceipt, minimum_guard: DnsGuardStrength) {
    receipt
        .validate_against_request(minimum_guard)
        .expect("valid change receipt");
    assert_eq!(
        receipt.submission_atomicity,
        super::DnsBatchAtomicity::AllOrNothing,
        "the initial DNS contract does not support partial-success receipts"
    );
    assert_eq!(receipt.guard_strength, minimum_guard);
    assert!(matches!(
        receipt.state,
        super::DnsChangeState::Pending | super::DnsChangeState::ProviderCommitted
    ));
}

async fn await_committed_receipt(
    provider: &dyn DnsProvider,
    zone: &DnsZoneRef,
    initial: &DnsChangeReceipt,
    guard: DnsGuardStrength,
) -> DnsChangeReceipt {
    validate_receipt(initial, guard);
    for _ in 0..20 {
        let observed = provider
            .observe_change(zone, &initial.id)
            .await
            .expect("observe DNS change");
        validate_receipt(&observed, guard);
        assert_eq!(observed.id, initial.id);
        if observed.state == super::DnsChangeState::ProviderCommitted {
            return observed;
        }
    }
    panic!("DNS change did not reach provider-committed state within the conformance bound");
}

fn expect_validation<T>(result: Result<T, NormalizedProviderError>, context: &str) {
    let error = result
        .err()
        .unwrap_or_else(|| panic!("{context} must fail"));
    error.validate().expect("normalized provider error");
    assert_eq!(
        error.category(),
        ProviderErrorCategory::Validation,
        "{context}"
    );
}

fn expect_validation_or_conflict<T>(result: Result<T, NormalizedProviderError>, context: &str) {
    let error = result
        .err()
        .unwrap_or_else(|| panic!("{context} must fail"));
    error.validate().expect("normalized provider error");
    assert!(
        matches!(
            error.category(),
            ProviderErrorCategory::Validation | ProviderErrorCategory::Conflict
        ),
        "{context}: unexpected category {:?}",
        error.category()
    );
}

fn expect_scope_rejection<T>(result: Result<T, NormalizedProviderError>, context: &str) {
    let error = result
        .err()
        .unwrap_or_else(|| panic!("{context} must fail"));
    error.validate().expect("normalized provider error");
    assert!(
        matches!(
            error.category(),
            ProviderErrorCategory::Validation
                | ProviderErrorCategory::Conflict
                | ProviderErrorCategory::NotFound
        ),
        "{context}: unexpected category {:?}",
        error.category()
    );
}

fn different_provider(provider: &CloudProvider) -> CloudProvider {
    match provider {
        CloudProvider::Cloudflare => CloudProvider::Aws,
        CloudProvider::Aws => CloudProvider::Cloudflare,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        net::Ipv4Addr,
        sync::Mutex,
    };

    use async_trait::async_trait;

    use super::*;
    use crate::cloud::{
        AbsoluteDnsName, DnsBatchAtomicity, DnsChangeId, DnsChangeState, DnsPageToken,
        DnsPropagationState, DnsRecordObjectId, DnsRecordSetKey, DnsRecordSetValue,
        DnsRoutingIdentity, DnsTtl, DnsZoneId, ProviderDnsRecordType, ZoneVisibility,
    };

    struct FakeDnsProvider {
        provider: CloudProvider,
        account: CloudResourceId,
        maximum_guard: DnsGuardStrength,
        state: Mutex<FakeState>,
    }

    struct FakeState {
        zones: Vec<ObservedDnsZone>,
        records: BTreeMap<(DnsZoneId, DnsRecordSetKey), ObservedDnsRecordSet>,
        receipts: BTreeMap<(DnsZoneId, DnsChangeId), DnsChangeReceipt>,
        sequence: u64,
    }

    impl FakeDnsProvider {
        fn new(fixture: &DnsAdapterConformanceFixture) -> Self {
            let records = fixture
                .primary_records
                .iter()
                .cloned()
                .map(|record| {
                    (
                        (record.zone.zone_id.clone(), record.record_set.key.clone()),
                        record,
                    )
                })
                .collect();
            Self {
                provider: fixture.provider.clone(),
                account: fixture.provider_account_id.clone(),
                maximum_guard: fixture.maximum_guard,
                state: Mutex::new(FakeState {
                    zones: vec![fixture.primary_zone.clone(), fixture.secondary_zone.clone()],
                    records,
                    receipts: BTreeMap::new(),
                    sequence: 100,
                }),
            }
        }

        fn validate_zone(&self, zone: &DnsZoneRef) -> Result<(), NormalizedProviderError> {
            zone.validate().map_err(|_| validation("invalid_zone"))?;
            if zone.provider != self.provider || zone.provider_account_id != self.account {
                return Err(validation("zone_scope_mismatch"));
            }
            let state = self.state.lock().expect("fake DNS state");
            if !state.zones.iter().any(|candidate| candidate.zone == *zone) {
                return Err(not_found("zone_not_found"));
            }
            Ok(())
        }

        fn next_revision(state: &mut FakeState) -> DnsRecordRevision {
            state.sequence += 1;
            DnsRecordRevision::new(format!("revision-{}", state.sequence)).expect("revision")
        }

        fn receipt(
            state: &mut FakeState,
            zone: &DnsZoneRef,
            guard_strength: DnsGuardStrength,
        ) -> DnsChangeReceipt {
            state.sequence += 1;
            let receipt = DnsChangeReceipt {
                id: DnsChangeId::new(format!("change-{}", state.sequence)).expect("change ID"),
                state: DnsChangeState::ProviderCommitted,
                submission_atomicity: DnsBatchAtomicity::AllOrNothing,
                propagation: DnsPropagationState::ProviderReportedApplied,
                guard_strength,
            };
            state
                .receipts
                .insert((zone.zone_id.clone(), receipt.id.clone()), receipt.clone());
            receipt
        }
    }

    #[async_trait]
    impl DnsProvider for FakeDnsProvider {
        async fn get_zone(
            &self,
            zone: &DnsZoneRef,
        ) -> Result<Option<ObservedDnsZone>, NormalizedProviderError> {
            self.validate_zone(zone)?;
            Ok(self
                .state
                .lock()
                .expect("fake DNS state")
                .zones
                .iter()
                .find(|candidate| candidate.zone == *zone)
                .cloned())
        }

        async fn list_zones(
            &self,
            provider_account_id: &CloudResourceId,
            page: &DnsPageRequest,
        ) -> Result<DnsPage<ObservedDnsZone>, NormalizedProviderError> {
            page.validate().map_err(|_| validation("invalid_page"))?;
            if provider_account_id != &self.account {
                return Err(validation("account_scope_mismatch"));
            }
            let offset = decode_token(page.token.as_ref(), "zones", &self.account, None)?;
            let state = self.state.lock().expect("fake DNS state");
            Ok(make_page(&state.zones, offset, page.limit, |next| {
                encode_token("zones", &self.account, None, next)
            }))
        }

        async fn get_record_set(
            &self,
            zone: &DnsZoneRef,
            key: &DnsRecordSetKey,
        ) -> Result<Option<ObservedDnsRecordSet>, NormalizedProviderError> {
            self.validate_zone(zone)?;
            key.validate()
                .map_err(|_| validation("invalid_record_key"))?;
            Ok(self
                .state
                .lock()
                .expect("fake DNS state")
                .records
                .get(&(zone.zone_id.clone(), key.clone()))
                .cloned())
        }

        async fn list_record_sets(
            &self,
            zone: &DnsZoneRef,
            page: &DnsPageRequest,
        ) -> Result<DnsPage<ObservedDnsRecordSet>, NormalizedProviderError> {
            self.validate_zone(zone)?;
            page.validate().map_err(|_| validation("invalid_page"))?;
            let offset = decode_token(
                page.token.as_ref(),
                "records",
                &self.account,
                Some(&zone.zone_id),
            )?;
            let state = self.state.lock().expect("fake DNS state");
            let records = state
                .records
                .iter()
                .filter(|((zone_id, _), _)| zone_id == &zone.zone_id)
                .map(|(_, record)| record.clone())
                .collect::<Vec<_>>();
            Ok(make_page(&records, offset, page.limit, |next| {
                encode_token("records", &self.account, Some(&zone.zone_id), next)
            }))
        }

        async fn apply_record_changes(
            &self,
            zone: &DnsZoneRef,
            changes: &[DnsRecordChange],
            minimum_guard: DnsGuardStrength,
        ) -> Result<DnsChangeReceipt, NormalizedProviderError> {
            self.validate_zone(zone)?;
            super::super::validate_dns_changes(zone, changes)
                .map_err(|_| validation("invalid_changes"))?;
            if minimum_guard > self.maximum_guard {
                return Err(validation("unsupported_guard"));
            }
            let mut state = self.state.lock().expect("fake DNS state");
            for change in changes {
                let (key, expected_absent, revision) = match change {
                    DnsRecordChange::Create { record_set, .. } => (&record_set.key, true, None),
                    DnsRecordChange::Replace { previous, .. }
                    | DnsRecordChange::Delete { previous, .. } => {
                        (&previous.record_set.key, false, Some(&previous.revision))
                    }
                };
                let current = state.records.get(&(zone.zone_id.clone(), key.clone()));
                if (expected_absent && current.is_some())
                    || (!expected_absent && current.map(|record| &record.revision) != revision)
                {
                    return Err(conflict("guard_conflict"));
                }
            }
            for change in changes {
                match change {
                    DnsRecordChange::Create { record_set, .. } => {
                        let revision = Self::next_revision(&mut state);
                        let observed = observed(zone, record_set.clone(), revision);
                        state
                            .records
                            .insert((zone.zone_id.clone(), record_set.key.clone()), observed);
                    }
                    DnsRecordChange::Replace { desired, .. } => {
                        let revision = Self::next_revision(&mut state);
                        let observed = observed(zone, desired.clone(), revision);
                        state
                            .records
                            .insert((zone.zone_id.clone(), desired.key.clone()), observed);
                    }
                    DnsRecordChange::Delete { previous, .. } => {
                        state
                            .records
                            .remove(&(zone.zone_id.clone(), previous.record_set.key.clone()));
                    }
                }
            }
            Ok(Self::receipt(&mut state, zone, self.maximum_guard))
        }

        async fn observe_change(
            &self,
            zone: &DnsZoneRef,
            change_id: &DnsChangeId,
        ) -> Result<DnsChangeReceipt, NormalizedProviderError> {
            self.validate_zone(zone)?;
            self.state
                .lock()
                .expect("fake DNS state")
                .receipts
                .get(&(zone.zone_id.clone(), change_id.clone()))
                .cloned()
                .ok_or_else(|| not_found("change_not_found"))
        }
    }

    #[tokio::test]
    async fn conforming_fake_passes_shared_inventory_contract() {
        let fixture = fixture();
        let provider = FakeDnsProvider::new(&fixture);
        assert_dns_inventory_conformance(&provider, &fixture).await;
    }

    #[tokio::test]
    async fn conforming_fake_passes_shared_mutation_contract() {
        let fixture = fixture();
        let provider = FakeDnsProvider::new(&fixture);
        assert_dns_mutation_conformance(&provider, &fixture).await;
    }

    #[tokio::test]
    async fn best_effort_fake_rejects_atomic_without_mutation() {
        let mut fixture = fixture();
        fixture.maximum_guard = DnsGuardStrength::BestEffort;
        let provider = FakeDnsProvider::new(&fixture);
        assert_dns_mutation_conformance(&provider, &fixture).await;
    }

    #[tokio::test]
    async fn conforming_fake_passes_shared_batch_contract() {
        let fixture = fixture();
        let provider = FakeDnsProvider::new(&fixture);
        assert_dns_batch_atomicity_conformance(&provider, &fixture).await;
    }

    fn fixture() -> DnsAdapterConformanceFixture {
        let account = CloudResourceId::new("dns-conformance-account").expect("account");
        let primary = zone(&account, "zone-primary", "example.test");
        let secondary = zone(&account, "zone-secondary", "example.net");
        let first = record(&primary.zone, "a.example.test", [192, 0, 2, 1]);
        let second = record(&primary.zone, "b.example.test", [192, 0, 2, 2]);
        let third = record(&primary.zone, "c.example.test", [192, 0, 2, 5]);
        let create = txt_record_set("create.example.test", "first");
        let replacement = txt_record_set("create.example.test", "replacement");
        DnsAdapterConformanceFixture {
            provider: CloudProvider::Aws,
            provider_account_id: account,
            other_account_id: CloudResourceId::new("dns-conformance-other").expect("other"),
            primary_zone: primary,
            secondary_zone: secondary,
            primary_records: vec![first, second, third],
            create_record: create,
            replacement_record: replacement,
            maximum_guard: DnsGuardStrength::Atomic,
        }
    }

    fn zone(account: &CloudResourceId, id: &str, apex: &str) -> ObservedDnsZone {
        ObservedDnsZone {
            zone: DnsZoneRef {
                provider_account_id: account.clone(),
                provider: CloudProvider::Aws,
                zone_id: DnsZoneId::new(id).expect("zone ID"),
                apex: AbsoluteDnsName::new(apex).expect("apex"),
                visibility: ZoneVisibility::Public,
            },
            revision: Some(DnsRecordRevision::new(format!("{id}-revision")).expect("revision")),
        }
    }

    fn record(zone: &DnsZoneRef, owner: &str, address: [u8; 4]) -> ObservedDnsRecordSet {
        observed(
            zone,
            record_set(owner, address),
            DnsRecordRevision::new(format!("{owner}-revision")).expect("revision"),
        )
    }

    fn record_set(owner: &str, address: [u8; 4]) -> ProviderDnsRecordSet {
        ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: super::super::DnsOwnerName::new(owner).expect("owner"),
                record_type: ProviderDnsRecordType::A,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: BTreeSet::from([DnsRecordSetValue::A {
                address: Ipv4Addr::from(address),
            }]),
            extension: None,
        }
    }

    fn txt_record_set(owner: &str, value: &str) -> ProviderDnsRecordSet {
        ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: super::super::DnsOwnerName::new(owner).expect("owner"),
                record_type: ProviderDnsRecordType::Txt,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: BTreeSet::from([DnsRecordSetValue::Txt {
                value: super::super::DnsTxtValue::new(vec![super::super::DnsCharacterString::new(
                    value.as_bytes().to_vec(),
                )
                .expect("TXT character string")])
                .expect("TXT value"),
            }]),
            extension: None,
        }
    }

    fn observed(
        zone: &DnsZoneRef,
        record_set: ProviderDnsRecordSet,
        revision: DnsRecordRevision,
    ) -> ObservedDnsRecordSet {
        ObservedDnsRecordSet {
            zone: zone.clone(),
            provider_object_ids: BTreeSet::from([DnsRecordObjectId::new(format!(
                "object-{revision}"
            ))
            .expect("object ID")]),
            record_set,
            revision,
        }
    }

    fn make_page<T: Clone>(
        values: &[T],
        offset: usize,
        limit: u16,
        token: impl FnOnce(usize) -> DnsPageToken,
    ) -> DnsPage<T> {
        let end = (offset + usize::from(limit)).min(values.len());
        DnsPage {
            items: values[offset.min(values.len())..end].to_vec(),
            next: (end < values.len()).then(|| token(end)),
        }
    }

    fn encode_token(
        method: &str,
        account: &CloudResourceId,
        zone: Option<&DnsZoneId>,
        offset: usize,
    ) -> DnsPageToken {
        DnsPageToken::new(format!(
            "{method}|{}|{}|{offset}",
            account.as_str(),
            zone.map(DnsZoneId::as_str).unwrap_or("-")
        ))
        .expect("page token")
    }

    fn decode_token(
        token: Option<&DnsPageToken>,
        method: &str,
        account: &CloudResourceId,
        zone: Option<&DnsZoneId>,
    ) -> Result<usize, NormalizedProviderError> {
        let Some(token) = token else {
            return Ok(0);
        };
        let parts = token.as_str().split('|').collect::<Vec<_>>();
        if parts.len() != 4
            || parts[0] != method
            || parts[1] != account.as_str()
            || parts[2] != zone.map(DnsZoneId::as_str).unwrap_or("-")
        {
            return Err(validation("cursor_scope_mismatch"));
        }
        parts[3].parse().map_err(|_| validation("invalid_cursor"))
    }

    fn validation(code: &str) -> NormalizedProviderError {
        error(ProviderErrorCategory::Validation, code)
    }

    fn conflict(code: &str) -> NormalizedProviderError {
        error(ProviderErrorCategory::Conflict, code)
    }

    fn not_found(code: &str) -> NormalizedProviderError {
        error(ProviderErrorCategory::NotFound, code)
    }

    fn error(category: ProviderErrorCategory, code: &str) -> NormalizedProviderError {
        NormalizedProviderError::new(category, code, "Sanitized fake provider error", None, None)
            .expect("normalized provider error")
    }
}
