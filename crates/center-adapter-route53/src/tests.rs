use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

use super::*;
use edgion_center_core::{
    authorize_zone_deletion,
    cloud_test_support::{assert_dns_provider_conformance, DnsAdapterConformanceFixture},
    AbsoluteDnsName, CloudProvider, CloudResourceId, CredentialSource, DeletionPolicy,
    DnsCharacterString, DnsGuardStrength, DnsOwnerName, DnsPageRequest, DnsRecordExtension,
    DnsRecordSetKey, DnsRecordSetValue, DnsRoutingIdentity, DnsTtl, DnsTxtValue, DnsZoneId,
    DnsZoneRef, DnssecDesiredState, DnssecExternalAction, DnssecProviderState, IdempotencyKey,
    ManagementPolicy, ObservedDnsRecordSet, ObservedDnsZone, ProviderAccountScope,
    ProviderAccountSpec, ProviderDnsRecordSet, ProviderDnsRecordType, Route53AliasTarget,
    Route53FailoverRole, Route53GeoLocation, Route53RoutingPolicy, ZoneCreationRequest,
    ZoneDeletionApproval, ZoneDeletionPlan, ZoneOrigin, ZoneReadiness, ZoneVisibility,
};

const AWS_ACCOUNT_ID: &str = "123456789012";

fn fake_change(id: &str, status: &str, submitted_at_unix_seconds: i64) -> Route53ChangeInfo {
    Route53ChangeInfo {
        id: format!("/change/{id}"),
        status: status.to_string(),
        submitted_at_unix_seconds,
        comment: None,
    }
}

struct FakeApi {
    account_id: String,
    zones: Mutex<Vec<Route53HostedZone>>,
    dnssec: Mutex<BTreeMap<String, Route53DnssecInfo>>,
    records: Mutex<BTreeMap<String, Vec<Route53RecordSet>>>,
    changes: Mutex<BTreeMap<String, Route53ChangeInfo>>,
    writes: Mutex<Vec<(String, Vec<Route53RecordChange>)>>,
    submit_status: Mutex<String>,
    submit_id: Mutex<Option<String>>,
    next_get_status: Mutex<Option<String>>,
    next_get_id: Mutex<Option<String>>,
    next_get_missing: Mutex<bool>,
    submit_unknown_before_commit: Mutex<bool>,
    submit_unknown_after_commit: Mutex<bool>,
    race_replace: Mutex<Option<(Route53RecordSet, Route53RecordSet)>>,
    next_change: AtomicUsize,
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl Route53Api for FakeApi {
    fn verified_account_id(&self) -> &str {
        &self.account_id
    }

    async fn create_hosted_zone(
        &self,
        request: &Route53CreateHostedZoneRequest,
    ) -> Route53ApiResult<Route53CreateHostedZoneResult> {
        let mut zones = self.zones.lock().unwrap();
        if let Some(zone) = zones
            .iter()
            .find(|zone| zone.caller_reference == request.caller_reference)
            .cloned()
        {
            return Ok(Route53CreateHostedZoneResult {
                hosted_zone: zone,
                change: fake_change("C900", "INSYNC", 900),
            });
        }
        let id = format!("ZCREATED{}", zones.len());
        let zone = Route53HostedZone {
            id: id.clone(),
            name: format!("{}.", request.name.trim_end_matches('.')),
            private_zone: false,
            caller_reference: request.caller_reference.clone(),
            resource_record_set_count: 2,
            name_servers: vec!["ns-1.awsdns.test.".to_string()],
            has_linked_service: false,
            has_unsupported_features: false,
        };
        zones.push(zone.clone());
        self.dnssec.lock().unwrap().insert(
            id,
            Route53DnssecInfo {
                serve_signature: "NOT_SIGNING".to_string(),
                key_signing_keys: Vec::new(),
            },
        );
        Ok(Route53CreateHostedZoneResult {
            hosted_zone: zone,
            change: fake_change("C900", "PENDING", 900),
        })
    }

    async fn get_hosted_zone(&self, zone_id: &str) -> Route53ApiResult<Option<Route53HostedZone>> {
        self.calls.lock().unwrap().push(format!("get:{zone_id}"));
        Ok(self
            .zones
            .lock()
            .unwrap()
            .iter()
            .find(|zone| normalize_zone_id(&zone.id).ok().as_deref() == Some(zone_id))
            .cloned())
    }

    async fn list_hosted_zones(
        &self,
        marker: Option<&str>,
        _max_items: u16,
    ) -> Route53ApiResult<Route53HostedZonePage> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("zones:{}", marker.unwrap_or("start")));
        let index = marker
            .map(|value| value.parse::<usize>().unwrap())
            .unwrap_or(0);
        let zones = self.zones.lock().unwrap();
        let end = (index + 1).min(zones.len());
        Ok(Route53HostedZonePage {
            items: zones[index..end].to_vec(),
            is_truncated: end < zones.len(),
            next_marker: (end < zones.len()).then(|| end.to_string()),
        })
    }

    async fn list_record_sets(
        &self,
        zone_id: &str,
        cursor: Option<&Route53RecordCursor>,
        _max_items: u16,
    ) -> Route53ApiResult<Route53RecordPage> {
        self.calls.lock().unwrap().push(format!(
            "records:{zone_id}:{}:{}:{}",
            cursor.map(|value| value.name.as_str()).unwrap_or("start"),
            cursor
                .map(|value| value.record_type.as_str())
                .unwrap_or("start"),
            cursor
                .and_then(|value| value.set_identifier.as_deref())
                .unwrap_or("none")
        ));
        let records = self
            .records
            .lock()
            .unwrap()
            .get(zone_id)
            .cloned()
            .unwrap_or_default();
        let index = match cursor {
            None => 0,
            Some(cursor) => records
                .iter()
                .position(|record| raw_cursor(record) == *cursor)
                .ok_or_else(|| validation("fake_route53_cursor_not_found"))?,
        };
        let end = (index + 1).min(records.len());
        Ok(Route53RecordPage {
            items: records[index..end].to_vec(),
            is_truncated: end < records.len(),
            next: records.get(end).map(raw_cursor),
        })
    }

    async fn change_record_sets(
        &self,
        zone_id: &str,
        batch: &Route53ChangeBatch,
    ) -> Route53ApiResult<Route53ChangeInfo> {
        let changes = &batch.changes;
        self.calls
            .lock()
            .unwrap()
            .push(format!("change:{zone_id}:{}", changes.len()));
        if *self.submit_unknown_before_commit.lock().unwrap() {
            return Err(unknown_outcome("fake_route53_ambiguous_dispatch"));
        }
        let mut records_by_zone = self.records.lock().unwrap();
        if let Some((old, new)) = self.race_replace.lock().unwrap().take() {
            let records = records_by_zone.entry(zone_id.to_string()).or_default();
            if let Some(index) = records.iter().position(|record| record == &old) {
                records[index] = new;
            }
        }
        let existing = records_by_zone.get(zone_id).cloned().unwrap_or_default();
        let mut updated = existing;
        for change in changes {
            match change.action {
                Route53ChangeAction::Create => {
                    if updated
                        .iter()
                        .any(|record| raw_cursor(record) == raw_cursor(&change.record_set))
                    {
                        return Err(conflict("fake_route53_create_conflict"));
                    }
                    updated.push(change.record_set.clone());
                }
                Route53ChangeAction::Delete => {
                    let Some(index) = updated
                        .iter()
                        .position(|record| record == &change.record_set)
                    else {
                        return Err(conflict("fake_route53_delete_conflict"));
                    };
                    updated.remove(index);
                }
            }
        }
        updated.sort_by_key(raw_cursor);
        records_by_zone.insert(zone_id.to_string(), updated);
        drop(records_by_zone);
        self.writes
            .lock()
            .unwrap()
            .push((zone_id.to_string(), changes.to_vec()));
        let sequence = self.next_change.fetch_add(1, Ordering::SeqCst) + 1;
        let info = Route53ChangeInfo {
            id: self
                .submit_id
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| format!("/change/C{sequence}")),
            status: self.submit_status.lock().unwrap().clone(),
            submitted_at_unix_seconds: sequence as i64,
            comment: Some(batch.comment.clone()),
        };
        self.changes
            .lock()
            .unwrap()
            .insert(format!("C{sequence}"), info.clone());
        if *self.submit_unknown_after_commit.lock().unwrap() {
            return Err(unknown_outcome("fake_route53_ambiguous_dispatch"));
        }
        Ok(info)
    }

    async fn delete_hosted_zone(&self, zone_id: &str) -> Route53ApiResult<Route53ChangeInfo> {
        let mut zones = self.zones.lock().unwrap();
        let index = zones
            .iter()
            .position(|zone| normalize_zone_id(&zone.id).ok().as_deref() == Some(zone_id))
            .ok_or_else(|| not_found("fake_route53_zone_not_found"))?;
        zones.remove(index);
        Ok(fake_change("C901", "PENDING", 901))
    }

    async fn get_dnssec(&self, zone_id: &str) -> Route53ApiResult<Route53DnssecInfo> {
        self.dnssec
            .lock()
            .unwrap()
            .get(zone_id)
            .cloned()
            .ok_or_else(|| not_found("fake_route53_dnssec_not_found"))
    }

    async fn enable_hosted_zone_dnssec(
        &self,
        zone_id: &str,
    ) -> Route53ApiResult<Route53ChangeInfo> {
        self.dnssec
            .lock()
            .unwrap()
            .get_mut(zone_id)
            .ok_or_else(|| not_found("fake_route53_dnssec_not_found"))?
            .serve_signature = "SIGNING".to_string();
        Ok(fake_change("C902", "PENDING", 902))
    }

    async fn disable_hosted_zone_dnssec(
        &self,
        zone_id: &str,
    ) -> Route53ApiResult<Route53ChangeInfo> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("disable-dnssec:{zone_id}"));
        self.dnssec
            .lock()
            .unwrap()
            .get_mut(zone_id)
            .ok_or_else(|| not_found("fake_route53_dnssec_not_found"))?
            .serve_signature = "DELETING".to_string();
        Ok(fake_change("C903", "PENDING", 903))
    }

    async fn get_change(&self, change_id: &str) -> Route53ApiResult<Option<Route53ChangeInfo>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("get-change:{change_id}"));
        if std::mem::take(&mut *self.next_get_missing.lock().unwrap()) {
            return Ok(None);
        }
        let mut changes = self.changes.lock().unwrap();
        let Some(info) = changes.get_mut(change_id) else {
            return Ok(None);
        };
        info.status = self
            .next_get_status
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| "INSYNC".to_string());
        let mut result = info.clone();
        if let Some(id) = self.next_get_id.lock().unwrap().take() {
            result.id = id;
        }
        Ok(Some(result))
    }
}

#[tokio::test]
async fn scoped_route53_adapter_passes_complete_shared_contract() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = adapter(center_account.clone(), fake);
    let primary_zone = observed_zone(&center_account, "ZPRIMARY", "example.test");
    let secondary_zone = observed_zone(&center_account, "ZSECONDARY", "example.net");
    let mut primary_records = expected_primary_records(&primary_zone.zone);
    let provider_conflict_seed = primary_records
        .iter()
        .position(|record| record.record_set.key.owner.as_str() == "txt.example.test")
        .unwrap();
    primary_records.swap(0, provider_conflict_seed);
    let fixture = DnsAdapterConformanceFixture {
        provider: CloudProvider::Aws,
        provider_account_id: center_account,
        other_account_id: CloudResourceId::new("route53-other").unwrap(),
        primary_zone,
        secondary_zone,
        primary_records,
        create_record: txt_record("create.example.test", "first"),
        replacement_record: txt_record("create.example.test", "replacement"),
        maximum_guard: DnsGuardStrength::Atomic,
    };
    assert_dns_provider_conformance(&adapter, &fixture).await;
}

#[tokio::test]
async fn complete_record_snapshot_traverses_provider_pagination_once() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = adapter(center_account.clone(), fake.clone());
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;

    let records = adapter.observe_all_record_sets(&zone).await.unwrap();

    assert_eq!(records.len(), primary_raw_records().len());
    assert!(records
        .windows(2)
        .all(|pair| pair[0].record_set.key < pair[1].record_set.key));
    let calls = fake.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.starts_with("records:ZPRIMARY:"))
            .count(),
        primary_raw_records().len()
    );
    assert_eq!(
        calls.iter().filter(|call| *call == "get:ZPRIMARY").count(),
        1
    );
}

#[tokio::test]
async fn direct_zone_observation_uses_one_get_and_no_inventory_scan() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = adapter(center_account.clone(), fake.clone());

    let observed = adapter
        .observe_zone_by_id(&center_account, &DnsZoneId::new("ZPRIMARY").unwrap())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(observed.zone.zone_id.as_str(), "ZPRIMARY");
    assert_eq!(observed.zone.visibility, ZoneVisibility::Public);
    assert_eq!(fake.calls.lock().unwrap().as_slice(), ["get:ZPRIMARY"]);
}

#[test]
fn receipt_scope_preflight_rejects_tamper_and_wrong_scope_without_provider_io() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let zone_id = DnsZoneId::new("ZPRIMARY").unwrap();
    let token = ChangeToken {
        version: 1,
        center_scope: scope_hash(center_account.as_str()),
        external_scope: scope_hash(AWS_ACCOUNT_ID),
        zone_scope: scope_hash(zone_id.as_str()),
        provider_change_id: "C123".to_string(),
        request_digest: "digest".to_string(),
        submitted_at_unix_seconds: 1,
        guard: DnsGuardStrength::Atomic,
    };
    let receipt =
        DnsChangeId::new(sign_token(&token, &[17; 32], TokenDomain::MutationReceipt).unwrap())
            .unwrap();
    let verifier = Route53MutationReceiptVerifier::new(
        center_account.clone(),
        &account(),
        Route53MutationReceiptKey::new([17; 32]).unwrap(),
    )
    .unwrap();

    verifier
        .validate_scope(&center_account, &zone_id, &receipt)
        .unwrap();
    assert_eq!(
        verifier
            .validate_scope(
                &CloudResourceId::new("route53-other").unwrap(),
                &zone_id,
                &receipt,
            )
            .unwrap_err()
            .code(),
        "route53_change_not_found"
    );
    assert_eq!(
        verifier
            .validate_scope(
                &center_account,
                &DnsZoneId::new("ZSECONDARY").unwrap(),
                &receipt,
            )
            .unwrap_err()
            .code(),
        "route53_change_not_found"
    );
    let mut tampered = receipt.as_str().as_bytes().to_vec();
    let last = tampered.last_mut().unwrap();
    *last = if *last == b'A' { b'B' } else { b'A' };
    let tampered = DnsChangeId::new(String::from_utf8(tampered).unwrap()).unwrap();
    assert_eq!(
        verifier
            .validate_scope(&center_account, &zone_id, &tampered)
            .unwrap_err()
            .code(),
        "route53_change_not_found"
    );

    let mut non_aws = account();
    non_aws.provider = CloudProvider::Cloudflare;
    non_aws.scope = Some(ProviderAccountScope::Cloudflare {
        account_id: "0123456789abcdef0123456789abcdef".to_string(),
    });
    assert_eq!(
        Route53MutationReceiptVerifier::new(
            center_account,
            &non_aws,
            Route53MutationReceiptKey::new([18; 32]).unwrap(),
        )
        .err()
        .unwrap()
        .code(),
        "route53_provider_required"
    );
}

#[tokio::test]
async fn replace_uses_exact_fresh_raw_delete_then_one_create() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = adapter(center_account.clone(), fake.clone());
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let key = DnsRecordSetKey {
        owner: DnsOwnerName::new("txt.example.test").unwrap(),
        record_type: ProviderDnsRecordType::Txt,
        routing: DnsRoutingIdentity::Simple,
    };
    let previous = adapter.get_record_set(&zone, &key).await.unwrap().unwrap();
    let desired = txt_record("txt.example.test", "replacement");
    let receipt = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Replace {
                previous: previous.clone(),
                desired: desired.clone(),
                guard: edgion_center_core::DnsMutationGuard::MatchObserved {
                    revision: previous.revision,
                },
            }],
            DnsGuardStrength::BestEffort,
        )
        .await
        .unwrap();
    assert_eq!(receipt.guard_strength, DnsGuardStrength::Atomic);
    assert_eq!(receipt.state, edgion_center_core::DnsChangeState::Pending);
    assert_eq!(
        receipt.propagation,
        edgion_center_core::DnsPropagationState::Pending
    );

    {
        let writes = fake.writes.lock().unwrap();
        let (written_zone, batch) = writes.last().unwrap();
        assert_eq!(written_zone, "ZPRIMARY");
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].action, Route53ChangeAction::Delete);
        assert_eq!(batch[0].record_set, primary_raw_records()[1]);
        assert_eq!(batch[1].action, Route53ChangeAction::Create);
        assert_eq!(
            batch[1].record_set,
            ordinary_record("txt.example.test.", "TXT", 300, &[r#""replacement""#])
        );
    }

    let observed = adapter.observe_change(&zone, &receipt.id).await.unwrap();
    assert_eq!(
        observed.state,
        edgion_center_core::DnsChangeState::ProviderCommitted
    );
    assert_eq!(
        observed.propagation,
        edgion_center_core::DnsPropagationState::ProviderReportedApplied
    );
}

#[tokio::test]
async fn exact_delete_rejects_a_race_after_fresh_inventory_without_partial_apply() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = adapter(center_account.clone(), fake.clone());
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let key = DnsRecordSetKey {
        owner: DnsOwnerName::new("txt.example.test").unwrap(),
        record_type: ProviderDnsRecordType::Txt,
        routing: DnsRoutingIdentity::Simple,
    };
    let previous = adapter.get_record_set(&zone, &key).await.unwrap().unwrap();
    let old_raw = primary_raw_records()[1].clone();
    let mut concurrent_raw = old_raw.clone();
    concurrent_raw.ttl = Some(301);
    *fake.race_replace.lock().unwrap() = Some((old_raw, concurrent_raw.clone()));
    let error = adapter
        .apply_record_changes(
            &zone,
            &[
                DnsRecordChange::Create {
                    record_set: txt_record("must-not-commit.example.test", "value"),
                    guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                },
                DnsRecordChange::Replace {
                    previous: previous.clone(),
                    desired: txt_record("txt.example.test", "replacement"),
                    guard: edgion_center_core::DnsMutationGuard::MatchObserved {
                        revision: previous.revision,
                    },
                },
            ],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Conflict);
    assert!(fake.writes.lock().unwrap().is_empty());
    let records = fake.records.lock().unwrap();
    assert_eq!(
        records["ZPRIMARY"]
            .iter()
            .find(|record| raw_cursor(record) == raw_cursor(&concurrent_raw))
            .unwrap(),
        &concurrent_raw
    );
    assert!(records["ZPRIMARY"]
        .iter()
        .all(|record| record.name != "must-not-commit.example.test."));
}

#[tokio::test]
async fn resultant_inventory_conflicts_fail_before_provider_dispatch() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;

    let weighted_api = fake_api();
    weighted_api.records.lock().unwrap().insert(
        "ZPRIMARY".to_string(),
        (0..100)
            .map(|index| {
                weighted_record(
                    &format!("member-{index:03}"),
                    1,
                    &format!("192.0.2.{}", index + 1),
                )
            })
            .collect(),
    );
    let weighted_api = Arc::new(weighted_api);
    let weighted_adapter = adapter(center_account.clone(), weighted_api.clone());
    let weighted_desired =
        model::map_record_set(&zone, weighted_record("member-100", 1, "192.0.2.201"))
            .unwrap()
            .0;
    let error = weighted_adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: weighted_desired,
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "route53_resultant_weighted_limit");
    assert!(weighted_api.writes.lock().unwrap().is_empty());

    let failover_api = fake_api();
    let mut primary = ordinary_record("failover.example.test.", "A", 60, &["192.0.2.10"]);
    primary.set_identifier = Some("primary-a".to_string());
    primary.failover = Some("PRIMARY".to_string());
    failover_api
        .records
        .lock()
        .unwrap()
        .insert("ZPRIMARY".to_string(), vec![primary]);
    let failover_api = Arc::new(failover_api);
    let failover_adapter = adapter(center_account.clone(), failover_api.clone());
    let mut duplicate_primary = ordinary_record("failover.example.test.", "A", 60, &["192.0.2.11"]);
    duplicate_primary.set_identifier = Some("primary-b".to_string());
    duplicate_primary.failover = Some("PRIMARY".to_string());
    let failover_desired = model::map_record_set(&zone, duplicate_primary).unwrap().0;
    let error = failover_adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: failover_desired,
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "route53_resultant_selector_conflict");
    assert!(failover_api.writes.lock().unwrap().is_empty());

    let cname_api = fake_api();
    cname_api.records.lock().unwrap().insert(
        "ZPRIMARY".to_string(),
        vec![ordinary_record(
            "conflict.example.test.",
            "A",
            60,
            &["192.0.2.20"],
        )],
    );
    let cname_api = Arc::new(cname_api);
    let cname_adapter = adapter(center_account, cname_api.clone());
    let cname_desired = model::map_record_set(
        &zone,
        ordinary_record(
            "conflict.example.test.",
            "CNAME",
            60,
            &["target.example.net."],
        ),
    )
    .unwrap()
    .0;
    let error = cname_adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: cname_desired,
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "route53_resultant_cname_conflict");
    assert!(cname_api.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn replace_preserves_routing_alias_and_health_check_shape_before_dispatch() {
    let mut weighted = weighted_record("member-a", 10, "192.0.2.30");
    let mut failover = weighted.clone();
    failover.weight = None;
    failover.failover = Some("PRIMARY".to_string());
    assert_replace_shape_conflict(weighted, failover).await;

    let alias = primary_raw_records()
        .into_iter()
        .find(|record| record.alias_target.is_some())
        .unwrap();
    let ordinary = ordinary_record("alias.example.test.", "A", 60, &["192.0.2.31"]);
    assert_replace_shape_conflict(alias, ordinary).await;

    weighted = ordinary_record("health.example.test.", "A", 60, &["192.0.2.32"]);
    weighted.health_check_id = Some("health-check-a".to_string());
    let mut changed_health = weighted.clone();
    changed_health.health_check_id = Some("health-check-b".to_string());
    assert_replace_shape_conflict(weighted, changed_health).await;
}

async fn assert_replace_shape_conflict(
    previous_raw: Route53RecordSet,
    desired_raw: Route53RecordSet,
) {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let fake = fake_api();
    fake.records
        .lock()
        .unwrap()
        .insert("ZPRIMARY".to_string(), vec![previous_raw.clone()]);
    let fake = Arc::new(fake);
    let adapter = adapter(center_account, fake.clone());
    let (previous_record_set, previous_revision) =
        model::map_record_set(&zone, previous_raw).unwrap();
    let previous = ObservedDnsRecordSet {
        zone: zone.clone(),
        record_set: previous_record_set,
        provider_object_ids: BTreeSet::new(),
        revision: previous_revision.clone(),
    };
    let desired = model::map_record_set(&zone, desired_raw).unwrap().0;

    let error = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Replace {
                previous,
                desired,
                guard: edgion_center_core::DnsMutationGuard::MatchObserved {
                    revision: previous_revision,
                },
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "route53_replace_shape_conflict");
    assert!(fake.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ambiguous_dispatch_is_unknown_outcome_and_is_never_retried() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    *fake.submit_unknown_after_commit.lock().unwrap() = true;
    let primary_adapter = adapter(center_account.clone(), fake.clone());
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let error = primary_adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt_record("ambiguous.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
    assert_eq!(
        fake.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("change:"))
            .count(),
        1
    );
    assert!(fake.records.lock().unwrap()["ZPRIMARY"]
        .iter()
        .any(|record| record.name == "ambiguous.example.test."));

    let not_committed = Arc::new(fake_api());
    *not_committed.submit_unknown_before_commit.lock().unwrap() = true;
    let secondary_adapter = adapter(center_account.clone(), not_committed.clone());
    let error = secondary_adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt_record("ambiguous-not-applied.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
    assert_eq!(
        not_committed
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("change:"))
            .count(),
        1
    );
    assert!(not_committed.records.lock().unwrap()["ZPRIMARY"]
        .iter()
        .all(|record| record.name != "ambiguous-not-applied.example.test."));
}

#[tokio::test]
async fn malformed_submit_status_is_unknown_but_unknown_observe_status_is_validation() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    *fake.submit_status.lock().unwrap() = "PAUSED".to_string();
    let adapter = adapter(center_account.clone(), fake.clone());
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let error = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt_record("bad-status.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);

    *fake.submit_status.lock().unwrap() = "PENDING".to_string();
    *fake.submit_id.lock().unwrap() = Some(String::new());
    let error = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt_record("bad-id.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);

    let receipt = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt_record("observe-status.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap();
    *fake.next_get_status.lock().unwrap() = Some("PAUSED".to_string());
    let error = adapter
        .observe_change(&zone, &receipt.id)
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Validation);
}

#[tokio::test]
async fn receipts_are_tamper_key_account_and_version_bound_before_get_change() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let primary_adapter = adapter(center_account.clone(), fake.clone());
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let receipt = primary_adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt_record("receipt.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap();
    let get_calls = || {
        fake.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("get-change:"))
            .count()
    };

    let mut bytes = URL_SAFE_NO_PAD.decode(receipt.id.as_str()).unwrap();
    bytes[0] ^= 1;
    let tampered = DnsChangeId::new(URL_SAFE_NO_PAD.encode(bytes)).unwrap();
    assert!(primary_adapter
        .observe_change(&zone, &tampered)
        .await
        .is_err());
    assert_eq!(get_calls(), 0);

    let wrong_key = Route53DnsAdapter::new_with_write_keys(
        center_account.clone(),
        &account(),
        fake.clone(),
        Route53CursorKey::new([7; 32]).unwrap(),
        Route53MutationReceiptKey::new([18; 32]).unwrap(),
        Route53LifecycleTokenKey::new([27; 32]).unwrap(),
    )
    .unwrap();
    assert!(wrong_key.observe_change(&zone, &receipt.id).await.is_err());
    assert_eq!(get_calls(), 0);

    let other_center = CloudResourceId::new("route53-other-center").unwrap();
    let other_adapter = adapter(other_center.clone(), fake.clone());
    let other_zone = DnsZoneRef {
        provider_account_id: other_center,
        ..zone.clone()
    };
    assert!(other_adapter
        .observe_change(&other_zone, &receipt.id)
        .await
        .is_err());
    assert_eq!(get_calls(), 0);

    let mut token: ChangeToken =
        verify_token(receipt.id.as_str(), &[17; 32], TokenDomain::MutationReceipt).unwrap();
    let cross_domain =
        DnsChangeId::new(sign_token(&token, &[17; 32], TokenDomain::Lifecycle).unwrap()).unwrap();
    assert!(primary_adapter
        .observe_change(&zone, &cross_domain)
        .await
        .is_err());
    assert_eq!(get_calls(), 0);

    token.version = 2;
    let unknown_version =
        DnsChangeId::new(sign_token(&token, &[17; 32], TokenDomain::MutationReceipt).unwrap())
            .unwrap();
    assert!(primary_adapter
        .observe_change(&zone, &unknown_version)
        .await
        .is_err());
    assert_eq!(get_calls(), 0);
}

#[tokio::test]
async fn change_status_and_identity_observation_fail_closed() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    *fake.submit_status.lock().unwrap() = "INSYNC".to_string();
    let adapter = adapter(center_account.clone(), fake.clone());
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let committed = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt_record("insync.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap();
    assert_eq!(
        committed.state,
        edgion_center_core::DnsChangeState::ProviderCommitted
    );

    *fake.submit_status.lock().unwrap() = "PENDING".to_string();
    let pending = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt_record("pending.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap();
    *fake.next_get_status.lock().unwrap() = Some("PENDING".to_string());
    let still_pending = adapter.observe_change(&zone, &pending.id).await.unwrap();
    assert_eq!(still_pending.id, pending.id);
    assert_eq!(
        still_pending.state,
        edgion_center_core::DnsChangeState::Pending
    );

    *fake.next_get_id.lock().unwrap() = Some("/change/COTHER".to_string());
    assert_eq!(
        adapter
            .observe_change(&zone, &pending.id)
            .await
            .unwrap_err()
            .code(),
        "route53_change_identity_mismatch"
    );
    *fake.next_get_missing.lock().unwrap() = true;
    assert_eq!(
        adapter
            .observe_change(&zone, &pending.id)
            .await
            .unwrap_err()
            .category(),
        ProviderErrorCategory::NotFound
    );
    fake.changes.lock().unwrap().get_mut("C2").unwrap().comment = None;
    assert_eq!(
        adapter
            .observe_change(&zone, &pending.id)
            .await
            .unwrap_err()
            .code(),
        "route53_change_metadata_mismatch"
    );
}

#[test]
fn provider_change_quotas_are_checked_before_dispatch() {
    let base = ordinary_record("many.example.test.", "TXT", 60, &[r#""value""#]);
    let mut too_many = base.clone();
    too_many.resource_records = vec![r#""value""#.to_string(); 1001];
    assert_eq!(
        validate_change_request(&[Route53RecordChange {
            action: Route53ChangeAction::Create,
            record_set: too_many,
        }])
        .unwrap_err()
        .code(),
        "route53_change_element_limit"
    );

    let mut too_large = base;
    too_large.resource_records = vec!["x".repeat(32_001)];
    assert_eq!(
        validate_change_request(&[Route53RecordChange {
            action: Route53ChangeAction::Create,
            record_set: too_large,
        }])
        .unwrap_err()
        .code(),
        "route53_record_value_size_limit"
    );

    let mut half = ordinary_record("half.example.test.", "TXT", 60, &[r#""value""#]);
    half.resource_records = vec![r#""value""#.to_string(); 501];
    assert_eq!(
        validate_change_request(&[
            Route53RecordChange {
                action: Route53ChangeAction::Delete,
                record_set: half.clone(),
            },
            Route53RecordChange {
                action: Route53ChangeAction::Create,
                record_set: half,
            },
        ])
        .unwrap_err()
        .code(),
        "route53_change_element_limit"
    );
}

#[test]
fn delete_planning_preserves_every_raw_route53_field() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let mut raws = primary_raw_records()
        .into_iter()
        .skip(1)
        .collect::<Vec<_>>();

    let mut latency = ordinary_record("latency.example.test.", "A", 60, &["192.0.2.30"]);
    latency.set_identifier = Some("latency-a".to_string());
    latency.region = Some("us-east-1".to_string());
    latency.health_check_id = Some("health-latency".to_string());
    raws.push(latency);

    let mut failover = ordinary_record("failover.example.test.", "A", 60, &["192.0.2.31"]);
    failover.set_identifier = Some("primary".to_string());
    failover.failover = Some("PRIMARY".to_string());
    failover.health_check_id = Some("health-primary".to_string());
    raws.push(failover);

    let mut geo = ordinary_record("geo.example.test.", "A", 60, &["192.0.2.32"]);
    geo.set_identifier = Some("geo-us-ca".to_string());
    geo.geolocation = Some(Route53GeoLocationData {
        continent_code: None,
        country_code: Some("US".to_string()),
        subdivision_code: Some("CA".to_string()),
    });
    raws.push(geo);

    let mut multivalue = ordinary_record("mv.example.test.", "A", 60, &["192.0.2.33"]);
    multivalue.set_identifier = Some("mv-a".to_string());
    multivalue.multivalue_answer = Some(true);
    multivalue.health_check_id = Some("health-mv".to_string());
    raws.push(multivalue);

    for raw in raws {
        let (record_set, revision) = model::map_record_set(&zone, raw.clone()).unwrap();
        let previous = ObservedDnsRecordSet {
            zone: zone.clone(),
            record_set,
            provider_object_ids: BTreeSet::new(),
            revision: revision.clone(),
        };
        let request = plan_change_batch(
            &zone,
            &[DnsRecordChange::Delete {
                previous: previous.clone(),
                guard: edgion_center_core::DnsMutationGuard::MatchObserved { revision },
            }],
            vec![(previous, raw.clone())],
        )
        .unwrap();
        assert_eq!(
            request,
            vec![Route53RecordChange {
                action: Route53ChangeAction::Delete,
                record_set: raw,
            }]
        );
    }
}

#[tokio::test]
async fn apex_soa_and_ns_mutations_fail_before_any_provider_call() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = adapter(center_account.clone(), fake.clone());
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let soa = model::map_record_set(&zone, primary_raw_records()[0].clone())
        .unwrap()
        .0;
    let apex_ns = ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner: DnsOwnerName::new("example.test").unwrap(),
            record_type: ProviderDnsRecordType::Ns,
            routing: DnsRoutingIdentity::Simple,
        },
        ttl: DnsTtl::Seconds(300),
        values: BTreeSet::from([DnsRecordSetValue::Ns {
            target: AbsoluteDnsName::new("ns.example.net").unwrap(),
        }]),
        extension: None,
    };
    for record_set in [soa, apex_ns] {
        let error = adapter
            .apply_record_changes(
                &zone,
                &[DnsRecordChange::Create {
                    record_set,
                    guard: edgion_center_core::DnsMutationGuard::MustNotExist,
                }],
                DnsGuardStrength::Atomic,
            )
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::Validation);
    }
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn maps_soa_txt_alias_routing_and_compound_provider_cursors() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = adapter(center_account.clone(), fake.clone());
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let records = adapter
        .list_record_sets(
            &zone,
            &DnsPageRequest {
                limit: 100,
                token: None,
            },
        )
        .await
        .unwrap()
        .items;
    let soa = records
        .iter()
        .find(|record| record.record_set.key.record_type == ProviderDnsRecordType::Soa)
        .unwrap();
    assert!(matches!(
        soa.record_set.values.iter().next(),
        Some(DnsRecordSetValue::Soa {
            serial: 1,
            refresh: 7200,
            retry: 900,
            expire: 1_209_600,
            minimum: 86_400,
            ..
        })
    ));
    let txt = records
        .iter()
        .find(|record| record.record_set.key.owner.as_str() == "txt.example.test")
        .unwrap();
    let Some(DnsRecordSetValue::Txt { value }) = txt.record_set.values.iter().next() else {
        panic!("expected TXT value");
    };
    assert_eq!(value.segments()[0].as_bytes(), b"seed");
    assert_eq!(value.segments()[1].as_bytes(), b"segment");
    assert!(records.iter().any(|record| {
        matches!(
            record.record_set.extension,
            Some(DnsRecordExtension::Route53 {
                routing_policy: Some(Route53RoutingPolicy::Weighted { weight: 10 }),
                ..
            })
        )
    }));
    assert!(records.iter().any(|record| {
        matches!(
            record.record_set.extension,
            Some(DnsRecordExtension::Route53 {
                alias_target: Some(_),
                ..
            })
        )
    }));
    assert!(records
        .iter()
        .all(|record| record.provider_object_ids.is_empty()));
    let calls = fake.calls.lock().unwrap();
    assert!(calls.iter().any(|call| call.ends_with(":weighted-a")));
    assert!(calls.iter().any(|call| call.ends_with(":weighted-b")));
}

#[tokio::test]
async fn private_and_provider_managed_zones_fail_closed() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let private_adapter = adapter(center_account.clone(), fake.clone());
    let private = DnsZoneRef {
        provider_account_id: center_account.clone(),
        provider: CloudProvider::Aws,
        zone_id: DnsZoneId::new("ZPRIVATE").unwrap(),
        apex: AbsoluteDnsName::new("private.test").unwrap(),
        visibility: ZoneVisibility::Public,
    };
    assert_eq!(
        private_adapter.get_zone(&private).await.unwrap_err().code(),
        "route53_private_zone_unsupported"
    );

    let managed = fake_api();
    managed.zones.lock().unwrap()[1].has_linked_service = true;
    let managed = adapter(center_account, Arc::new(managed));
    assert_eq!(
        managed
            .list_zones(
                &CloudResourceId::new("route53-main").unwrap(),
                &DnsPageRequest {
                    limit: 10,
                    token: None,
                },
            )
            .await
            .unwrap_err()
            .code(),
        "route53_linked_service_zone_unsupported"
    );
}

#[tokio::test]
async fn unsupported_record_shape_fails_the_whole_inventory() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = fake_api();
    fake.records.lock().unwrap().insert(
        "ZPRIMARY".to_string(),
        vec![ordinary_record(
            "bad.example.test.",
            "HTTPS",
            300,
            &["1 . alpn=h2"],
        )],
    );
    let adapter = adapter(center_account.clone(), Arc::new(fake));
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    assert_eq!(
        adapter
            .list_record_sets(
                &zone,
                &DnsPageRequest {
                    limit: 10,
                    token: None,
                },
            )
            .await
            .unwrap_err()
            .code(),
        "unsupported_route53_record_type"
    );
}

#[test]
fn maps_supported_ordinary_record_presentations() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let cases = [
        ordinary_record("v6.example.test.", "AAAA", 300, &["2001:db8::1"]),
        ordinary_record("mail.example.test.", "MX", 300, &["10 mail.example.net."]),
        ordinary_record(
            "_https._tcp.example.test.",
            "SRV",
            300,
            &["0 5 443 target.example.net."],
        ),
        ordinary_record(
            "example.test.",
            "CAA",
            300,
            &[r#"0 issue "letsencrypt.org""#],
        ),
        ordinary_record("child.example.test.", "NS", 300, &["ns.example.net."]),
    ];
    let mapped = cases
        .into_iter()
        .map(|record| model::map_record_set(&zone, record).unwrap().0)
        .collect::<Vec<_>>();
    assert!(matches!(
        mapped[0].values.iter().next(),
        Some(DnsRecordSetValue::Aaaa { .. })
    ));
    assert!(matches!(
        mapped[1].values.iter().next(),
        Some(DnsRecordSetValue::Mx { preference: 10, .. })
    ));
    assert!(matches!(
        mapped[2].values.iter().next(),
        Some(DnsRecordSetValue::Srv { port: 443, .. })
    ));
    assert!(matches!(
        mapped[3].values.iter().next(),
        Some(DnsRecordSetValue::Caa { flags: 0, .. })
    ));
    assert!(matches!(
        mapped[4].values.iter().next(),
        Some(DnsRecordSetValue::Ns { .. })
    ));
}

#[test]
fn maps_every_supported_route53_routing_family() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let mut cases = Vec::new();

    let mut latency = ordinary_record("latency.example.test.", "A", 60, &["192.0.2.1"]);
    latency.set_identifier = Some("latency-a".to_string());
    latency.region = Some("us-east-1".to_string());
    cases.push((
        latency,
        Route53RoutingPolicy::Latency {
            region: "us-east-1".to_string(),
        },
    ));

    let mut failover = ordinary_record("failover.example.test.", "A", 60, &["192.0.2.2"]);
    failover.set_identifier = Some("primary".to_string());
    failover.failover = Some("PRIMARY".to_string());
    failover.health_check_id = Some("health-check-a".to_string());
    cases.push((
        failover,
        Route53RoutingPolicy::Failover {
            role: Route53FailoverRole::Primary,
        },
    ));

    for (identifier, location, raw) in [
        (
            "geo-default",
            Route53GeoLocation::Default,
            Route53GeoLocationData {
                continent_code: None,
                country_code: Some("*".to_string()),
                subdivision_code: None,
            },
        ),
        (
            "geo-continent",
            Route53GeoLocation::Continent {
                code: "EU".to_string(),
            },
            Route53GeoLocationData {
                continent_code: Some("EU".to_string()),
                country_code: None,
                subdivision_code: None,
            },
        ),
        (
            "geo-country",
            Route53GeoLocation::Country {
                code: "DE".to_string(),
            },
            Route53GeoLocationData {
                continent_code: None,
                country_code: Some("DE".to_string()),
                subdivision_code: None,
            },
        ),
        (
            "geo-subdivision",
            Route53GeoLocation::UsSubdivision {
                code: "CA".to_string(),
            },
            Route53GeoLocationData {
                continent_code: None,
                country_code: Some("US".to_string()),
                subdivision_code: Some("CA".to_string()),
            },
        ),
    ] {
        let mut record = ordinary_record("geo.example.test.", "A", 60, &["192.0.2.3"]);
        record.set_identifier = Some(identifier.to_string());
        record.geolocation = Some(raw);
        cases.push((record, Route53RoutingPolicy::Geolocation { location }));
    }

    let mut multivalue = ordinary_record("mv.example.test.", "A", 60, &["192.0.2.4"]);
    multivalue.set_identifier = Some("mv-a".to_string());
    multivalue.multivalue_answer = Some(true);
    cases.push((multivalue, Route53RoutingPolicy::Multivalue));

    for (raw, expected) in cases {
        let (mapped, _) = model::map_record_set(&zone, raw).unwrap();
        let Some(DnsRecordExtension::Route53 { routing_policy, .. }) = mapped.extension else {
            panic!("missing Route 53 extension");
        };
        assert_eq!(routing_policy, Some(expected));
    }
}

#[test]
fn verified_aws_account_is_required_before_any_provider_call() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(FakeApi {
        account_id: "999999999999".to_string(),
        ..fake_api()
    });
    let error = Route53DnsAdapter::new(
        center_account,
        &account(),
        fake.clone(),
        Route53CursorKey::new([9; 32]).unwrap(),
    )
    .err()
    .unwrap();
    assert_eq!(error.code(), "route53_verified_account_mismatch");
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[test]
fn revision_is_independent_of_provider_value_order() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let first = ordinary_record(
        "txt.example.test.",
        "TXT",
        300,
        &[r#""first""#, r#""second""#],
    );
    let mut second = first.clone();
    second.resource_records.reverse();
    let (_, first_revision) = model::map_record_set(&zone, first).unwrap();
    let (_, second_revision) = model::map_record_set(&zone, second).unwrap();
    assert_eq!(first_revision, second_revision);
}

#[test]
fn revision_covers_route53_delete_identity_fields() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let base = weighted_record("member-a", 10, "192.0.2.1");
    let base_revision = model::map_record_set(&zone, base.clone()).unwrap().1;

    let mut variants = Vec::new();
    let mut changed = base.clone();
    changed.ttl = Some(61);
    variants.push(changed);
    let mut changed = base.clone();
    changed.resource_records = vec!["192.0.2.2".to_string()];
    variants.push(changed);
    let mut changed = base.clone();
    changed.set_identifier = Some("member-b".to_string());
    variants.push(changed);
    let mut changed = base.clone();
    changed.weight = Some(20);
    variants.push(changed);
    let mut changed = base;
    changed.health_check_id = Some("health-check-a".to_string());
    variants.push(changed);

    for changed in variants {
        assert_ne!(
            model::map_record_set(&zone, changed).unwrap().1,
            base_revision
        );
    }

    let alias = Route53RecordSet {
        name: "alias.example.test.".to_string(),
        record_type: "A".to_string(),
        ttl: None,
        resource_records: Vec::new(),
        alias_target: Some(Route53AliasTargetData {
            hosted_zone_id: "ZALIAS".to_string(),
            dns_name: "target.example.net.".to_string(),
            evaluate_target_health: false,
        }),
        set_identifier: None,
        weight: None,
        failover: None,
        region: None,
        geolocation: None,
        multivalue_answer: None,
        health_check_id: None,
        traffic_policy_instance_id: None,
        has_cidr_routing_config: false,
        has_geoproximity_location: false,
    };
    let alias_revision = model::map_record_set(&zone, alias.clone()).unwrap().1;
    for mutate in [0, 1, 2] {
        let mut changed = alias.clone();
        let target = changed.alias_target.as_mut().unwrap();
        match mutate {
            0 => target.hosted_zone_id = "ZOTHER".to_string(),
            1 => target.dns_name = "other.example.net.".to_string(),
            _ => target.evaluate_target_health = true,
        }
        assert_ne!(
            model::map_record_set(&zone, changed).unwrap().1,
            alias_revision
        );
    }
}

#[test]
fn local_cursor_fails_closed_when_inventory_changes() {
    let key = Route53CursorKey::new([3; 32]).unwrap();
    let first = paginate(
        vec!["a".to_string(), "b".to_string()],
        &DnsPageRequest {
            limit: 1,
            token: None,
        },
        CursorScope {
            center_account_id: "center".to_string(),
            aws_account_id: AWS_ACCOUNT_ID.to_string(),
            method: CursorMethod::Zones,
        },
        &key,
    )
    .unwrap();
    let error = paginate(
        vec!["b".to_string(), "c".to_string()],
        &DnsPageRequest {
            limit: 1,
            token: first.next,
        },
        CursorScope {
            center_account_id: "center".to_string(),
            aws_account_id: AWS_ACCOUNT_ID.to_string(),
            method: CursorMethod::Zones,
        },
        &key,
    )
    .unwrap_err();
    assert_eq!(error.code(), "route53_inventory_changed");
}

#[test]
fn provider_continuations_reject_unknown_or_oversized_fields() {
    for next in [
        Route53RecordCursor {
            name: "next.example.test.".to_string(),
            record_type: "UNKNOWN".to_string(),
            set_identifier: None,
        },
        Route53RecordCursor {
            name: "a".repeat(1025),
            record_type: "A".to_string(),
            set_identifier: None,
        },
        Route53RecordCursor {
            name: "next.example.test.".to_string(),
            record_type: "A".to_string(),
            set_identifier: Some("a".repeat(129)),
        },
    ] {
        assert!(validate_record_page(&Route53RecordPage {
            items: vec![ordinary_record("a.example.test.", "A", 60, &["192.0.2.1"])],
            is_truncated: true,
            next: Some(next),
        })
        .is_err());
    }
}

#[test]
fn wildcard_and_catalog_values_are_validated_before_canonical_inventory() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let wildcard = ordinary_record(r"\052.example.test.", "A", 60, &["192.0.2.1"]);
    let (mapped, _) = model::map_record_set(&zone, wildcard).unwrap();
    assert_eq!(mapped.key.owner.as_str(), "*.example.test");

    let mut bad_region = ordinary_record("latency.example.test.", "A", 60, &["192.0.2.2"]);
    bad_region.set_identifier = Some("bad-region".to_string());
    bad_region.region = Some("not-a-region".to_string());
    assert_eq!(
        model::map_record_set(&zone, bad_region).unwrap_err().code(),
        "unsupported_route53_latency_region"
    );

    let mut bad_country = ordinary_record("geo.example.test.", "A", 60, &["192.0.2.3"]);
    bad_country.set_identifier = Some("bad-country".to_string());
    bad_country.geolocation = Some(Route53GeoLocationData {
        continent_code: None,
        country_code: Some("ZZ".to_string()),
        subdivision_code: None,
    });
    assert_eq!(
        model::map_record_set(&zone, bad_country)
            .unwrap_err()
            .code(),
        "invalid_route53_geolocation"
    );
}

#[tokio::test]
async fn lifecycle_observation_is_conservative_for_public_and_private_zones() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let provider = adapter(center_account.clone(), Arc::clone(&fake));
    let public = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let observed = provider.observe_zone(&public).await.unwrap().unwrap();
    assert_eq!(observed.delegation.state, DelegationState::NotChecked);
    assert_eq!(
        observed.authoritative_verification,
        AuthoritativeDnsVerification::NotChecked
    );
    assert_eq!(
        observed.readiness,
        ZoneReadiness::AwaitingAuthoritativeVerification
    );
    assert_eq!(observed.dnssec.state, DnssecProviderState::Disabled);
    assert_eq!(observed.non_default_record_count, 4);
    let revision = observed.revision;
    fake.zones.lock().unwrap()[1].name_servers.reverse();
    assert_eq!(
        provider
            .observe_zone(&public)
            .await
            .unwrap()
            .unwrap()
            .revision,
        revision
    );

    let private = DnsZoneRef {
        provider_account_id: center_account,
        provider: CloudProvider::Aws,
        zone_id: DnsZoneId::new("ZPRIVATE").unwrap(),
        apex: AbsoluteDnsName::new("private.test").unwrap(),
        visibility: ZoneVisibility::Private,
    };
    let observed = provider.observe_zone(&private).await.unwrap().unwrap();
    assert_eq!(observed.delegation.state, DelegationState::NotApplicable);
    assert_eq!(observed.dnssec.state, DnssecProviderState::Unsupported);
}

#[tokio::test]
async fn lifecycle_create_uses_stable_caller_reference_and_never_claims_readiness() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let provider = adapter(center_account.clone(), Arc::clone(&fake));
    let request = ZoneCreationRequest {
        provider_account_id: center_account.clone(),
        provider: CloudProvider::Aws,
        apex: AbsoluteDnsName::new("created.test").unwrap(),
        visibility: ZoneVisibility::Public,
        idempotency_key: IdempotencyKey::new("create-request-1").unwrap(),
    };
    let first = provider.create_zone(&request).await.unwrap();
    assert_eq!(first.state, ZoneLifecycleMutationState::Pending);
    let second = provider.create_zone(&request).await.unwrap();
    assert_eq!(second.state, ZoneLifecycleMutationState::Succeeded);
    assert_eq!(fake.zones.lock().unwrap().len(), 4);

    fake.changes
        .lock()
        .unwrap()
        .insert("C900".to_string(), fake_change("C900", "PENDING", 900));
    let completed = provider.observe_mutation(&first.mutation_id).await.unwrap();
    assert_eq!(completed.state, ZoneLifecycleMutationState::Succeeded);
    let created_zone = fake
        .zones
        .lock()
        .unwrap()
        .iter()
        .find(|zone| zone.name == "created.test.")
        .cloned()
        .unwrap();
    let reference = DnsZoneRef {
        provider_account_id: center_account,
        provider: CloudProvider::Aws,
        zone_id: DnsZoneId::new(normalize_zone_id(&created_zone.id).unwrap()).unwrap(),
        apex: AbsoluteDnsName::new("created.test").unwrap(),
        visibility: ZoneVisibility::Public,
    };
    let observed = provider.observe_zone(&reference).await.unwrap().unwrap();
    assert_eq!(
        observed.readiness,
        ZoneReadiness::AwaitingAuthoritativeVerification
    );
}

#[tokio::test]
async fn lifecycle_dnssec_fails_closed_without_ksk_and_exposes_ds_handoff() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let provider = adapter(center_account.clone(), Arc::clone(&fake));
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let observed = provider.observe_zone(&zone).await.unwrap().unwrap();
    assert_eq!(
        provider
            .set_dnssec(&zone, DnssecDesiredState::Enabled, &observed.revision)
            .await
            .unwrap_err()
            .code(),
        "route53_dnssec_active_ksk_required"
    );

    fake.dnssec.lock().unwrap().insert(
        "ZPRIMARY".to_string(),
        Route53DnssecInfo {
            serve_signature: "NOT_SIGNING".to_string(),
            key_signing_keys: vec![Route53KeySigningKey {
                status: "ACTIVE".to_string(),
                ds_record: Some(
                    "2371 13 2 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_string(),
                ),
            }],
        },
    );
    let revision = provider
        .observe_zone(&zone)
        .await
        .unwrap()
        .unwrap()
        .revision;
    let receipt = provider
        .set_dnssec(&zone, DnssecDesiredState::Enabled, &revision)
        .await
        .unwrap();
    assert_eq!(receipt.state, ZoneLifecycleMutationState::Pending);
    let observed = provider.observe_zone(&zone).await.unwrap().unwrap();
    assert_eq!(observed.dnssec.state, DnssecProviderState::AwaitingDs);
    match observed.dnssec.external_action {
        DnssecExternalAction::PublishDs { records } => {
            assert_eq!(records[0].key_tag, 2371);
            assert!(records[0]
                .digest
                .bytes()
                .all(|byte| !byte.is_ascii_lowercase()));
        }
        action => panic!("unexpected external action: {action:?}"),
    }
    assert_eq!(
        provider
            .set_dnssec(&zone, DnssecDesiredState::Disabled, &observed.revision)
            .await
            .unwrap_err()
            .code(),
        "route53_dnssec_parent_ds_removal_verification_required"
    );
    assert!(!fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call.starts_with("disable-dnssec:")));
}

#[tokio::test]
async fn lifecycle_delete_rechecks_revision_and_safe_provider_state() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let provider = adapter(center_account.clone(), Arc::clone(&fake));
    let zone = DnsZoneRef {
        provider_account_id: center_account,
        provider: CloudProvider::Aws,
        zone_id: DnsZoneId::new("ZPRIVATE").unwrap(),
        apex: AbsoluteDnsName::new("private.test").unwrap(),
        visibility: ZoneVisibility::Private,
    };
    let observed = provider.observe_zone(&zone).await.unwrap().unwrap();
    let plan = ZoneDeletionPlan::from_observation(
        &observed,
        ZoneOrigin::CenterCreated,
        ManagementPolicy::Managed,
        DeletionPolicy::DeleteExternal,
    );
    let request = authorize_zone_deletion(
        &plan,
        ZoneDeletionApproval {
            approved_revision: observed.revision,
            approved_zone: zone.clone(),
            approved_by: "operator".to_string(),
            approved_at: "2026-07-17T00:00:00Z".to_string(),
            acknowledgements: Default::default(),
        },
    )
    .unwrap();
    fake.zones.lock().unwrap()[0].resource_record_set_count = 3;
    assert_eq!(
        provider.delete_zone(&request).await.unwrap_err().code(),
        "route53_zone_lifecycle_revision_conflict"
    );

    let unsafe_observation = provider.observe_zone(&zone).await.unwrap().unwrap();
    let unsafe_plan = ZoneDeletionPlan::from_observation(
        &unsafe_observation,
        ZoneOrigin::CenterCreated,
        ManagementPolicy::Managed,
        DeletionPolicy::DeleteExternal,
    );
    assert!(authorize_zone_deletion(
        &unsafe_plan,
        ZoneDeletionApproval {
            approved_revision: unsafe_observation.revision,
            approved_zone: zone.clone(),
            approved_by: "operator".to_string(),
            approved_at: "2026-07-17T00:00:00Z".to_string(),
            acknowledgements: Default::default(),
        },
    )
    .is_err());

    fake.zones.lock().unwrap()[0].resource_record_set_count = 2;
    let observed = provider.observe_zone(&zone).await.unwrap().unwrap();
    let plan = ZoneDeletionPlan::from_observation(
        &observed,
        ZoneOrigin::CenterCreated,
        ManagementPolicy::Managed,
        DeletionPolicy::DeleteExternal,
    );
    let safe_request = authorize_zone_deletion(
        &plan,
        ZoneDeletionApproval {
            approved_revision: observed.revision,
            approved_zone: zone.clone(),
            approved_by: "operator".to_string(),
            approved_at: "2026-07-17T00:00:00Z".to_string(),
            acknowledgements: Default::default(),
        },
    )
    .unwrap();
    assert_eq!(
        provider.delete_zone(&safe_request).await.unwrap().state,
        ZoneLifecycleMutationState::Pending
    );
    assert!(provider
        .observe_zone(safe_request.zone())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn direct_public_lifecycle_delete_requires_fresh_exact_safe_observation() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let provider = adapter(center_account.clone(), Arc::clone(&fake));
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    let observed = provider.observe_zone(&zone).await.unwrap().unwrap();
    assert_eq!(
        provider
            .delete_zone_with_exact_guard(&zone, &observed.revision)
            .await
            .unwrap_err()
            .code(),
        "route53_zone_deletion_precondition_failed"
    );
    fake.zones
        .lock()
        .unwrap()
        .iter_mut()
        .find(|value| normalize_zone_id(&value.id).ok().as_deref() == Some("ZPRIMARY"))
        .unwrap()
        .resource_record_set_count = 2;
    let observed = provider.observe_zone(&zone).await.unwrap().unwrap();
    assert_eq!(
        provider
            .delete_zone_with_exact_guard(&zone, &observed.revision)
            .await
            .unwrap()
            .state,
        ZoneLifecycleMutationState::Pending
    );
}

fn adapter(center_account: CloudResourceId, api: Arc<FakeApi>) -> Route53DnsAdapter {
    Route53DnsAdapter::new_with_write_keys(
        center_account,
        &account(),
        api,
        Route53CursorKey::new([7; 32]).unwrap(),
        Route53MutationReceiptKey::new([17; 32]).unwrap(),
        Route53LifecycleTokenKey::new([27; 32]).unwrap(),
    )
    .unwrap()
}

#[test]
fn write_constructors_reject_cross_purpose_key_reuse() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let api = Arc::new(fake_api());
    let error = Route53DnsAdapter::new_with_record_write_key(
        center_account.clone(),
        &account(),
        api.clone(),
        Route53CursorKey::new([9; 32]).unwrap(),
        Route53MutationReceiptKey::new([9; 32]).unwrap(),
    )
    .err()
    .unwrap();
    assert_eq!(error.code(), "route53_signing_key_reuse");

    for (cursor, mutation, lifecycle) in [
        ([9; 32], [9; 32], [29; 32]),
        ([9; 32], [19; 32], [9; 32]),
        ([9; 32], [19; 32], [19; 32]),
    ] {
        let error = Route53DnsAdapter::new_with_write_keys(
            center_account.clone(),
            &account(),
            api.clone(),
            Route53CursorKey::new(cursor).unwrap(),
            Route53MutationReceiptKey::new(mutation).unwrap(),
            Route53LifecycleTokenKey::new(lifecycle).unwrap(),
        )
        .err()
        .unwrap();
        assert_eq!(error.code(), "route53_signing_key_reuse");
    }
    assert!(api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn record_write_constructor_does_not_grant_lifecycle_authority() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = Route53DnsAdapter::new_with_record_write_key(
        center_account,
        &account(),
        fake.clone(),
        Route53CursorKey::new([7; 32]).unwrap(),
        Route53MutationReceiptKey::new([17; 32]).unwrap(),
    )
    .unwrap();
    let mutation_id = ZoneLifecycleMutationId::new("opaque-lifecycle-token").unwrap();
    let error = adapter.observe_mutation(&mutation_id).await.unwrap_err();
    assert_eq!(error.code(), "route53_mutation_authority_unavailable");
    assert!(fake.calls.lock().unwrap().is_empty());
    assert!(fake.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn read_only_adapter_rejects_mutation_before_provider_dispatch() {
    let center_account = CloudResourceId::new("route53-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = Route53DnsAdapter::new_read_only(
        center_account.clone(),
        &account(),
        fake.clone(),
        Route53CursorKey::new([7; 32]).unwrap(),
    )
    .unwrap();
    let zone = observed_zone(&center_account, "ZPRIMARY", "example.test").zone;
    assert!(adapter
        .list_record_sets(
            &zone,
            &DnsPageRequest {
                limit: 10,
                token: None,
            },
        )
        .await
        .is_ok());
    let error = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt_record("blocked.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::BestEffort,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "route53_mutation_authority_unavailable");
    assert!(fake.writes.lock().unwrap().is_empty());
}

fn account() -> ProviderAccountSpec {
    ProviderAccountSpec {
        provider: CloudProvider::Aws,
        scope: Some(ProviderAccountScope::Aws {
            account_id: AWS_ACCOUNT_ID.to_string(),
        }),
        credential_source: CredentialSource::Ambient,
    }
}

fn fake_api() -> FakeApi {
    FakeApi {
        account_id: AWS_ACCOUNT_ID.to_string(),
        zones: Mutex::new(vec![
            Route53HostedZone {
                id: "/hostedzone/ZPRIVATE".to_string(),
                name: "private.test.".to_string(),
                private_zone: true,
                caller_reference: "private-ref".to_string(),
                resource_record_set_count: 2,
                name_servers: Vec::new(),
                has_linked_service: false,
                has_unsupported_features: false,
            },
            Route53HostedZone {
                id: "/hostedzone/ZPRIMARY".to_string(),
                name: "example.test.".to_string(),
                private_zone: false,
                caller_reference: "primary-ref".to_string(),
                resource_record_set_count: primary_raw_records().len() as u64,
                name_servers: vec![
                    "ns-1.awsdns.test.".to_string(),
                    "ns-2.awsdns.test.".to_string(),
                ],
                has_linked_service: false,
                has_unsupported_features: false,
            },
            Route53HostedZone {
                id: "ZSECONDARY".to_string(),
                name: "example.net.".to_string(),
                private_zone: false,
                caller_reference: "secondary-ref".to_string(),
                resource_record_set_count: 3,
                name_servers: vec!["ns-3.awsdns.test.".to_string()],
                has_linked_service: false,
                has_unsupported_features: false,
            },
        ]),
        dnssec: Mutex::new(BTreeMap::from([
            (
                "ZPRIMARY".to_string(),
                Route53DnssecInfo {
                    serve_signature: "NOT_SIGNING".to_string(),
                    key_signing_keys: Vec::new(),
                },
            ),
            (
                "ZSECONDARY".to_string(),
                Route53DnssecInfo {
                    serve_signature: "NOT_SIGNING".to_string(),
                    key_signing_keys: Vec::new(),
                },
            ),
        ])),
        records: Mutex::new(BTreeMap::from([
            ("ZPRIMARY".to_string(), primary_raw_records()),
            (
                "ZSECONDARY".to_string(),
                vec![ordinary_record(
                    "www.example.net.",
                    "A",
                    300,
                    &["192.0.2.20"],
                )],
            ),
        ])),
        changes: Mutex::new(BTreeMap::new()),
        writes: Mutex::new(Vec::new()),
        submit_status: Mutex::new("PENDING".to_string()),
        submit_id: Mutex::new(None),
        next_get_status: Mutex::new(None),
        next_get_id: Mutex::new(None),
        next_get_missing: Mutex::new(false),
        submit_unknown_before_commit: Mutex::new(false),
        submit_unknown_after_commit: Mutex::new(false),
        race_replace: Mutex::new(None),
        next_change: AtomicUsize::new(0),
        calls: Mutex::new(Vec::new()),
    }
}

fn primary_raw_records() -> Vec<Route53RecordSet> {
    vec![
        ordinary_record(
            "example.test.",
            "SOA",
            900,
            &["ns-1.example.net. hostmaster.example.test. 1 7200 900 1209600 86400"],
        ),
        ordinary_record("txt.example.test.", "TXT", 300, &[r#""seed" "segment""#]),
        ordinary_record("www.example.test.", "A", 300, &["192.0.2.10"]),
        weighted_record("weighted-a", 10, "192.0.2.11"),
        weighted_record("weighted-b", 20, "192.0.2.12"),
        Route53RecordSet {
            name: "alias.example.test.".to_string(),
            record_type: "A".to_string(),
            ttl: None,
            resource_records: Vec::new(),
            alias_target: Some(Route53AliasTargetData {
                hosted_zone_id: "ZALIAS".to_string(),
                dns_name: "dualstack.lb.example.net.".to_string(),
                evaluate_target_health: true,
            }),
            set_identifier: None,
            weight: None,
            failover: None,
            region: None,
            geolocation: None,
            multivalue_answer: None,
            health_check_id: None,
            traffic_policy_instance_id: None,
            has_cidr_routing_config: false,
            has_geoproximity_location: false,
        },
    ]
}

fn ordinary_record(name: &str, record_type: &str, ttl: u32, values: &[&str]) -> Route53RecordSet {
    Route53RecordSet {
        name: name.to_string(),
        record_type: record_type.to_string(),
        ttl: Some(ttl),
        resource_records: values.iter().map(|value| (*value).to_string()).collect(),
        alias_target: None,
        set_identifier: None,
        weight: None,
        failover: None,
        region: None,
        geolocation: None,
        multivalue_answer: None,
        health_check_id: None,
        traffic_policy_instance_id: None,
        has_cidr_routing_config: false,
        has_geoproximity_location: false,
    }
}

fn weighted_record(identifier: &str, weight: u8, address: &str) -> Route53RecordSet {
    let mut record = ordinary_record("weighted.example.test.", "A", 60, &[address]);
    record.set_identifier = Some(identifier.to_string());
    record.weight = Some(weight);
    record
}

fn raw_cursor(record: &Route53RecordSet) -> Route53RecordCursor {
    Route53RecordCursor {
        name: record.name.clone(),
        record_type: record.record_type.clone(),
        set_identifier: record.set_identifier.clone(),
    }
}

fn observed_zone(account: &CloudResourceId, id: &str, apex: &str) -> ObservedDnsZone {
    ObservedDnsZone {
        zone: DnsZoneRef {
            provider_account_id: account.clone(),
            provider: CloudProvider::Aws,
            zone_id: DnsZoneId::new(id).unwrap(),
            apex: AbsoluteDnsName::new(apex).unwrap(),
            visibility: ZoneVisibility::Public,
        },
        revision: None,
    }
}

fn expected_primary_records(zone: &DnsZoneRef) -> Vec<ObservedDnsRecordSet> {
    let simple = |owner: &str, record_type, ttl, value| ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner: DnsOwnerName::new(owner).unwrap(),
            record_type,
            routing: DnsRoutingIdentity::Simple,
        },
        ttl: DnsTtl::Seconds(ttl),
        values: BTreeSet::from([value]),
        extension: None,
    };
    let routed = |identifier: &str, weight: u8, address: &str| ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner: DnsOwnerName::new("weighted.example.test").unwrap(),
            record_type: ProviderDnsRecordType::A,
            routing: DnsRoutingIdentity::Route53 {
                set_identifier: identifier.to_string(),
            },
        },
        ttl: DnsTtl::Seconds(60),
        values: BTreeSet::from([DnsRecordSetValue::A {
            address: address.parse().unwrap(),
        }]),
        extension: Some(DnsRecordExtension::Route53 {
            alias_target: None,
            routing_policy: Some(Route53RoutingPolicy::Weighted { weight }),
            health_check_id: None,
        }),
    };
    vec![
        simple(
            "example.test",
            ProviderDnsRecordType::Soa,
            900,
            DnsRecordSetValue::Soa {
                primary_name_server: AbsoluteDnsName::new("ns-1.example.net").unwrap(),
                responsible_mailbox: AbsoluteDnsName::new("hostmaster.example.test").unwrap(),
                serial: 1,
                refresh: 7200,
                retry: 900,
                expire: 1_209_600,
                minimum: 86_400,
            },
        ),
        ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("txt.example.test").unwrap(),
                record_type: ProviderDnsRecordType::Txt,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: BTreeSet::from([DnsRecordSetValue::Txt {
                value: DnsTxtValue::new(vec![
                    DnsCharacterString::new(b"seed".to_vec()).unwrap(),
                    DnsCharacterString::new(b"segment".to_vec()).unwrap(),
                ])
                .unwrap(),
            }]),
            extension: None,
        },
        simple(
            "www.example.test",
            ProviderDnsRecordType::A,
            300,
            DnsRecordSetValue::A {
                address: "192.0.2.10".parse().unwrap(),
            },
        ),
        routed("weighted-a", 10, "192.0.2.11"),
        routed("weighted-b", 20, "192.0.2.12"),
        ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("alias.example.test").unwrap(),
                record_type: ProviderDnsRecordType::A,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Inherited,
            values: BTreeSet::new(),
            extension: Some(DnsRecordExtension::Route53 {
                alias_target: Some(Route53AliasTarget {
                    target_zone_id: DnsZoneId::new("ZALIAS").unwrap(),
                    target: AbsoluteDnsName::new("dualstack.lb.example.net").unwrap(),
                    evaluate_target_health: true,
                }),
                routing_policy: None,
                health_check_id: None,
            }),
        },
    ]
    .into_iter()
    .map(|record_set| {
        let revision = model::canonical_revision(&record_set).unwrap();
        ObservedDnsRecordSet {
            zone: zone.clone(),
            record_set,
            provider_object_ids: BTreeSet::new(),
            revision,
        }
    })
    .collect()
}

fn txt_record(owner: &str, value: &str) -> ProviderDnsRecordSet {
    ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner: DnsOwnerName::new(owner).unwrap(),
            record_type: ProviderDnsRecordType::Txt,
            routing: DnsRoutingIdentity::Simple,
        },
        ttl: DnsTtl::Seconds(300),
        values: BTreeSet::from([DnsRecordSetValue::Txt {
            value: DnsTxtValue::new(vec![
                DnsCharacterString::new(value.as_bytes().to_vec()).unwrap()
            ])
            .unwrap(),
        }]),
        extension: None,
    }
}
