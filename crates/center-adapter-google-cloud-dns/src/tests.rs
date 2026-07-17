use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use edgion_center_core::{
    cloud_test_support::{assert_dns_provider_conformance, DnsAdapterConformanceFixture},
    AbsoluteDnsName, CloudProvider, CloudResourceId, CredentialSource, DnsCharacterString,
    DnsGuardStrength, DnsOwnerName, DnsRecordChange, DnsRecordExtension, DnsRecordSetKey,
    DnsRecordSetValue, DnsRoutingIdentity, DnsTtl, DnsTxtValue, DnsZoneId, DnsZoneRef,
    DnssecExternalAction, DnssecProviderState, ObservedDnsZone, ProviderAccountScope,
    ProviderAccountSpec, ProviderDnsRecordSet, ProviderDnsRecordType, ProviderErrorCategory,
    ZoneLifecycleProvider, ZoneReadiness, ZoneVisibility,
};

use super::*;

const PROJECT: &str = "edgion-dns-test";

struct FakeApi {
    project: String,
    zones: Vec<GoogleManagedZone>,
    records: Mutex<BTreeMap<String, Vec<GoogleResourceRecordSet>>>,
    changes: Mutex<BTreeMap<(String, String), GoogleChange>>,
    writes: Mutex<Vec<GoogleChangeRequest>>,
    sequence: Mutex<u64>,
}

#[async_trait]
impl GoogleCloudDnsApi for FakeApi {
    fn verified_project_id(&self) -> &str {
        &self.project
    }
    async fn get_managed_zone(
        &self,
        zone: &str,
    ) -> GoogleCloudDnsApiResult<Option<GoogleManagedZone>> {
        Ok(self
            .zones
            .iter()
            .find(|v| v.id == zone || v.name == zone)
            .cloned())
    }
    async fn list_managed_zones(
        &self,
        page_token: Option<&str>,
        max_results: u16,
    ) -> GoogleCloudDnsApiResult<GoogleManagedZonePage> {
        let offset = page_token
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|_| validation("invalid_fake_zone_cursor"))?;
        if offset > self.zones.len() {
            return Err(validation("invalid_fake_zone_cursor"));
        }
        let end = offset
            .saturating_add(max_results.into())
            .min(self.zones.len());
        Ok(GoogleManagedZonePage {
            items: self.zones[offset..end].to_vec(),
            next_page_token: (end < self.zones.len()).then(|| end.to_string()),
        })
    }
    async fn list_record_sets(
        &self,
        zone: &str,
        page_token: Option<&str>,
        max_results: u16,
    ) -> GoogleCloudDnsApiResult<GoogleRecordSetPage> {
        let offset = page_token
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|_| validation("invalid_fake_record_cursor"))?;
        let records = self
            .records
            .lock()
            .unwrap()
            .get(zone)
            .cloned()
            .unwrap_or_default();
        if offset > records.len() {
            return Err(validation("invalid_fake_record_cursor"));
        }
        let end = offset.saturating_add(max_results.into()).min(records.len());
        Ok(GoogleRecordSetPage {
            items: records[offset..end].to_vec(),
            next_page_token: (end < records.len()).then(|| end.to_string()),
        })
    }
    async fn create_change(
        &self,
        zone: &str,
        request: &GoogleChangeRequest,
    ) -> GoogleCloudDnsApiResult<GoogleChange> {
        let mut all = self.records.lock().unwrap();
        let records = all
            .get_mut(zone)
            .ok_or_else(|| not_found("google_zone_not_found"))?;
        let mut working = records.clone();
        for deletion in &request.deletions {
            let Some(index) = working.iter().position(|v| v == deletion) else {
                return Err(conflict("preconditionFailed"));
            };
            working.remove(index);
        }
        for addition in &request.additions {
            if working
                .iter()
                .any(|v| v.name == addition.name && v.record_type == addition.record_type)
            {
                return Err(conflict("alreadyExists"));
            }
            working.push(addition.clone());
        }
        working.sort_by(|a, b| (&a.name, &a.record_type).cmp(&(&b.name, &b.record_type)));
        *records = working;
        let mut sequence = self.sequence.lock().unwrap();
        *sequence += 1;
        let change = GoogleChange {
            id: sequence.to_string(),
            status: "done".into(),
            start_time: "2026-07-17T00:00:00Z".into(),
            is_serving: true,
            additions: request.additions.clone(),
            deletions: request.deletions.clone(),
        };
        self.changes
            .lock()
            .unwrap()
            .insert((zone.into(), change.id.clone()), change.clone());
        self.writes.lock().unwrap().push(request.clone());
        Ok(change)
    }
    async fn get_change(
        &self,
        zone: &str,
        id: &str,
    ) -> GoogleCloudDnsApiResult<Option<GoogleChange>> {
        Ok(self
            .changes
            .lock()
            .unwrap()
            .get(&(zone.into(), id.into()))
            .cloned())
    }

    async fn list_dns_keys(&self, _zone: &str) -> GoogleCloudDnsApiResult<Vec<GoogleDnsKey>> {
        Ok(vec![GoogleDnsKey {
            key_tag: 12345,
            algorithm: 13,
            key_type: "keySigning".into(),
            digests: vec![GoogleDnsKeyDigest {
                digest_type: "sha256".into(),
                digest: "AA".repeat(32),
            }],
        }])
    }
}

#[tokio::test]
async fn lifecycle_observation_never_confuses_provider_state_with_dns_readiness() {
    let account = CloudResourceId::new("google-account").unwrap();
    let adapter = adapter(account.clone(), Arc::new(fake_api()));
    let observation = ZoneLifecycleProvider::observe_zone(
        &adapter,
        &observed_zone(&account, "1001", "example.test", ZoneVisibility::Public).zone,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        observation.readiness,
        ZoneReadiness::AwaitingAuthoritativeVerification
    );
    assert_eq!(
        observation.delegation.state,
        edgion_center_core::DelegationState::NotChecked
    );
    assert_eq!(observation.non_default_record_count, 2);
}

#[tokio::test]
async fn lifecycle_dnssec_exposes_ds_as_an_external_parent_action() {
    let mut fake = fake_api();
    fake.zones[0].dnssec_state = GoogleDnsSecState::On;
    let account = CloudResourceId::new("google-account").unwrap();
    let adapter = adapter(account.clone(), Arc::new(fake));
    let observation = ZoneLifecycleProvider::observe_zone(
        &adapter,
        &observed_zone(&account, "1001", "example.test", ZoneVisibility::Public).zone,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(observation.dnssec.state, DnssecProviderState::AwaitingDs);
    assert!(
        matches!(observation.dnssec.external_action, DnssecExternalAction::PublishDs { ref records } if records.len() == 1)
    );
}

#[tokio::test]
async fn adapter_passes_shared_conformance_and_uses_exact_raw_deletion() {
    let account_id = CloudResourceId::new("google-dns-main").unwrap();
    let fake = Arc::new(fake_api());
    let adapter = adapter(account_id.clone(), fake.clone());
    let primary = observed_zone(&account_id, "1001", "example.test", ZoneVisibility::Public);
    let secondary = observed_zone(&account_id, "1002", "private.test", ZoneVisibility::Private);
    let primary_records = adapter
        .list_record_sets(
            &primary.zone,
            &edgion_center_core::DnsPageRequest {
                limit: 100,
                token: None,
            },
        )
        .await
        .unwrap()
        .items;
    let fixture = DnsAdapterConformanceFixture {
        provider: CloudProvider::GoogleCloud,
        provider_account_id: account_id,
        other_account_id: CloudResourceId::new("google-dns-other").unwrap(),
        primary_zone: primary.clone(),
        secondary_zone: secondary,
        primary_records,
        create_record: txt("create.example.test", "first"),
        replacement_record: txt("create.example.test", "replacement"),
        maximum_guard: DnsGuardStrength::Atomic,
    };
    assert_dns_provider_conformance(&adapter, &fixture).await;

    let key = DnsRecordSetKey {
        owner: DnsOwnerName::new("txt.example.test").unwrap(),
        record_type: ProviderDnsRecordType::Txt,
        routing: DnsRoutingIdentity::Simple,
    };
    let previous = adapter
        .get_record_set(&primary.zone, &key)
        .await
        .unwrap()
        .unwrap();
    adapter
        .apply_record_changes(
            &primary.zone,
            &[DnsRecordChange::Delete {
                guard: edgion_center_core::DnsMutationGuard::MatchObserved {
                    revision: previous.revision.clone(),
                },
                previous,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap();
    let write = fake.writes.lock().unwrap().last().unwrap().clone();
    assert_eq!(
        write.deletions[0].signature_rrdatas,
        vec!["sig-output-only".to_string()]
    );
}

#[test]
fn maps_alias_and_complete_routing_policy_round_trip() {
    let account = CloudResourceId::new("google-dns-main").unwrap();
    let zone = observed_zone(&account, "1001", "example.test", ZoneVisibility::Public).zone;
    let alias = rr("example.test.", "ALIAS", 300, &["target.example.net."]);
    let (mapped, _) = model::map_record_set(&zone, alias.clone()).unwrap();
    assert!(matches!(
        mapped.extension,
        Some(DnsRecordExtension::GoogleAlias { .. })
    ));
    assert_eq!(model::render_record_set(&zone, &mapped).unwrap(), alias);

    let routed = GoogleResourceRecordSet {
        name: "geo.example.test.".into(),
        record_type: "A".into(),
        ttl: 60,
        rrdatas: Vec::new(),
        signature_rrdatas: Vec::new(),
        routing_policy: Some(GoogleRoutingPolicy {
            health_check: Some("https://www.googleapis.com/compute/v1/projects/edgion-dns-test/global/healthChecks/web".into()),
            routing_data: GoogleRoutingData::Geo {
                geo: GoogleGeoPolicy {
                    enable_fencing: true,
                    items: vec![GoogleGeoPolicyItem {
                        location: "us-central1".into(),
                        rrdatas: vec!["192.0.2.1".into()],
                        signature_rrdatas: Vec::new(),
                        health_checked_targets: Some(GoogleHealthCheckTargets {
                            internal_load_balancers: Vec::new(),
                            external_endpoints: vec!["192.0.2.2".into()],
                            extra: Default::default(),
                        }),
                        extra: Default::default(),
                    }],
                    extra: Default::default(),
                },
            },
            extra: Default::default(),
        }),
        extra: Default::default(),
    };
    let (mapped, _) = model::map_record_set(&zone, routed.clone()).unwrap();
    assert!(matches!(
        mapped.extension,
        Some(DnsRecordExtension::GoogleCloud { .. })
    ));
    assert_eq!(model::render_record_set(&zone, &mapped).unwrap(), routed);
    let wire = serde_json::to_value(&routed).unwrap();
    assert!(wire["routingPolicy"]["geo"].is_object());
    assert!(wire["routingPolicy"].get("routingData").is_none());
}

#[tokio::test]
async fn complex_zones_and_alias_with_dnssec_fail_closed() {
    let account = CloudResourceId::new("google-dns-main").unwrap();
    let mut fake = fake_api();
    fake.zones[0].kind = GoogleZoneKind::Forwarding;
    let complex_adapter = adapter(account.clone(), Arc::new(fake));
    let error = complex_adapter
        .list_zones(
            &account,
            &edgion_center_core::DnsPageRequest {
                limit: 10,
                token: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Validation);

    let mut fake = fake_api();
    fake.zones[0].dnssec_state = GoogleDnsSecState::On;
    fake.records
        .lock()
        .unwrap()
        .get_mut("1001")
        .unwrap()
        .push(rr("example.test.", "ALIAS", 60, &["target.example.net."]));
    let adapter = adapter(account, Arc::new(fake));
    let zone = observed_zone(
        &CloudResourceId::new("google-dns-main").unwrap(),
        "1001",
        "example.test",
        ZoneVisibility::Public,
    )
    .zone;
    assert_eq!(
        adapter
            .list_record_sets(
                &zone,
                &edgion_center_core::DnsPageRequest {
                    limit: 100,
                    token: None
                }
            )
            .await
            .unwrap_err()
            .code(),
        "google_alias_dnssec_unsupported"
    );
}

#[tokio::test]
async fn fake_change_is_atomic_when_a_later_action_conflicts() {
    let fake = fake_api();
    let before = fake.records.lock().unwrap().get("1001").unwrap().clone();
    let error = fake
        .create_change(
            "1001",
            &GoogleChangeRequest {
                deletions: vec![before[1].clone()],
                additions: vec![before[2].clone()],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Conflict);
    assert_eq!(*fake.records.lock().unwrap().get("1001").unwrap(), before);
}

#[tokio::test]
async fn creating_alias_in_dnssec_zone_fails_before_transport() {
    let account = CloudResourceId::new("google-dns-main").unwrap();
    let mut fake = fake_api();
    fake.zones[0].dnssec_state = GoogleDnsSecState::On;
    let fake = Arc::new(fake);
    let adapter = adapter(account.clone(), fake.clone());
    let zone = observed_zone(&account, "1001", "example.test", ZoneVisibility::Public).zone;
    let desired = ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner: DnsOwnerName::new("example.test").unwrap(),
            record_type: ProviderDnsRecordType::GoogleAlias,
            routing: DnsRoutingIdentity::Simple,
        },
        ttl: DnsTtl::Seconds(60),
        values: BTreeSet::new(),
        extension: Some(DnsRecordExtension::GoogleAlias {
            target: AbsoluteDnsName::new("target.example.net").unwrap(),
        }),
    };
    let error = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: desired,
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "google_alias_dnssec_unsupported");
    assert!(fake.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn constructor_and_authenticated_tokens_are_scope_bound() {
    let account = CloudResourceId::new("google-dns-main").unwrap();
    let wrong_api = Arc::new(FakeApi {
        project: "different-project".into(),
        ..fake_api()
    });
    assert_eq!(
        GoogleCloudDnsAdapter::new(
            account.clone(),
            &ProviderAccountSpec {
                provider: CloudProvider::GoogleCloud,
                scope: Some(ProviderAccountScope::GoogleCloud {
                    project_id: PROJECT.into()
                }),
                credential_source: CredentialSource::Ambient,
            },
            wrong_api,
            GoogleCloudDnsCursorKey::new([3; 32]).unwrap(),
        )
        .err()
        .expect("verified project mismatch")
        .code(),
        "google_verified_project_mismatch"
    );

    let adapter = adapter(account.clone(), Arc::new(fake_api()));
    let first = adapter
        .list_zones(
            &account,
            &edgion_center_core::DnsPageRequest {
                limit: 1,
                token: None,
            },
        )
        .await
        .unwrap();
    let mut token: String = first.next.unwrap().into();
    token.replace_range(..1, if token.starts_with('A') { "B" } else { "A" });
    let error = adapter
        .list_zones(
            &account,
            &edgion_center_core::DnsPageRequest {
                limit: 1,
                token: Some(edgion_center_core::DnsPageToken::new(token).unwrap()),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Validation);

    let zone = observed_zone(&account, "1001", "example.test", ZoneVisibility::Public).zone;
    let receipt = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: txt("tamper.example.test", "value"),
                guard: edgion_center_core::DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .unwrap();
    let mut id: String = receipt.id.into();
    id.replace_range(..1, if id.starts_with('A') { "B" } else { "A" });
    let error = adapter
        .observe_change(&zone, &edgion_center_core::DnsChangeId::new(id).unwrap())
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::NotFound);
}

#[test]
fn reports_public_and_private_capabilities_separately() {
    let profile = GoogleCloudDnsAdapter::capability_profile();
    assert!(profile
        .public_zones
        .contains(&edgion_center_core::DnsCapability::PublicZones));
    assert!(profile
        .public_zones
        .contains(&edgion_center_core::DnsCapability::ApexAlias));
    assert!(!profile
        .private_zones
        .contains(&edgion_center_core::DnsCapability::ApexAlias));
    assert!(profile
        .private_zones
        .contains(&edgion_center_core::DnsCapability::PrivateZones));
    assert!(profile
        .public_zones
        .contains(&edgion_center_core::DnsCapability::GeolocationRouting));
    assert!(profile
        .private_zones
        .contains(&edgion_center_core::DnsCapability::AtomicChanges));
}

#[test]
fn change_digest_ignores_only_google_output_fields() {
    let base = GoogleChangeRequest {
        additions: vec![rr("www.example.test.", "A", 30, &["192.0.2.1"])],
        deletions: Vec::new(),
    };
    let mut output = base.clone();
    output.additions[0]
        .signature_rrdatas
        .push("generated-signature".into());
    output.additions[0]
        .extra
        .insert("kind".into(), serde_json::json!("dns#resourceRecordSet"));
    assert_eq!(
        semantic_change_digest(&base).unwrap(),
        semantic_change_digest(&output).unwrap()
    );
    output.additions[0]
        .extra
        .insert("futureSemanticField".into(), serde_json::json!(true));
    assert_ne!(
        semantic_change_digest(&base).unwrap(),
        semantic_change_digest(&output).unwrap()
    );
}

#[test]
fn routing_wire_union_requires_exactly_one_policy() {
    let multiple = serde_json::json!({
        "geo": {"items": [], "enableFencing": false},
        "wrr": {"items": []}
    });
    assert!(serde_json::from_value::<GoogleRoutingPolicy>(multiple).is_err());
    assert!(serde_json::from_value::<GoogleRoutingPolicy>(serde_json::json!({})).is_err());
}

fn adapter(account: CloudResourceId, api: Arc<FakeApi>) -> GoogleCloudDnsAdapter {
    GoogleCloudDnsAdapter::new(
        account,
        &ProviderAccountSpec {
            provider: CloudProvider::GoogleCloud,
            scope: Some(ProviderAccountScope::GoogleCloud {
                project_id: PROJECT.into(),
            }),
            credential_source: CredentialSource::Ambient,
        },
        api,
        GoogleCloudDnsCursorKey::new([9; 32]).unwrap(),
    )
    .unwrap()
}
fn observed_zone(
    account: &CloudResourceId,
    id: &str,
    apex: &str,
    visibility: ZoneVisibility,
) -> ObservedDnsZone {
    ObservedDnsZone {
        zone: DnsZoneRef {
            provider_account_id: account.clone(),
            provider: CloudProvider::GoogleCloud,
            zone_id: DnsZoneId::new(id).unwrap(),
            apex: AbsoluteDnsName::new(apex).unwrap(),
            visibility,
        },
        revision: None,
    }
}
fn txt(name: &str, value: &str) -> ProviderDnsRecordSet {
    ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner: DnsOwnerName::new(name).unwrap(),
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
fn rr(name: &str, kind: &str, ttl: u32, values: &[&str]) -> GoogleResourceRecordSet {
    GoogleResourceRecordSet {
        name: name.into(),
        record_type: kind.into(),
        ttl,
        rrdatas: values.iter().map(|v| (*v).into()).collect(),
        signature_rrdatas: Vec::new(),
        routing_policy: None,
        extra: Default::default(),
    }
}
fn fake_api() -> FakeApi {
    let mut txt = rr("txt.example.test.", "TXT", 300, &["\"seed\" \"segment\""]);
    txt.signature_rrdatas = vec!["sig-output-only".into()];
    FakeApi {
        project: PROJECT.into(),
        zones: vec![
            GoogleManagedZone {
                id: "1001".into(),
                name: "primary".into(),
                dns_name: "example.test.".into(),
                visibility: GoogleZoneVisibility::Public,
                kind: GoogleZoneKind::Authoritative,
                dnssec_state: GoogleDnsSecState::Off,
                name_servers: vec!["ns1.example.net".into(), "ns2.example.net".into()],
            },
            GoogleManagedZone {
                id: "1002".into(),
                name: "private".into(),
                dns_name: "private.test.".into(),
                visibility: GoogleZoneVisibility::Private,
                kind: GoogleZoneKind::Authoritative,
                dnssec_state: GoogleDnsSecState::Off,
                name_servers: Vec::new(),
            },
        ],
        records: Mutex::new(BTreeMap::from([
            (
                "1001".into(),
                vec![
                    rr(
                        "example.test.",
                        "SOA",
                        900,
                        &["ns.example.test. hostmaster.example.test. 1 7200 900 1209600 300"],
                    ),
                    txt,
                    rr("www.example.test.", "A", 300, &["192.0.2.10"]),
                ],
            ),
            (
                "1002".into(),
                vec![rr("www.private.test.", "A", 300, &["10.0.0.1"])],
            ),
        ])),
        changes: Mutex::new(BTreeMap::new()),
        writes: Mutex::new(Vec::new()),
        sequence: Mutex::new(0),
    }
}
