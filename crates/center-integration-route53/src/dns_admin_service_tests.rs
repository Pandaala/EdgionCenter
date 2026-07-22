use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_adapter_credential_files::{
    CredentialPurpose, MountedCredentialBinding, MountedCredentialConfig, MountedCredentialResolver,
};
use edgion_center_adapter_route53::{
    Route53AliasTargetData, Route53Api, Route53ChangeBatch, Route53ChangeInfo,
    Route53CreateHostedZoneRequest, Route53CreateHostedZoneResult, Route53DnssecInfo,
    Route53HostedZone, Route53HostedZonePage, Route53RecordCursor, Route53RecordPage,
    Route53RecordSet,
};
use edgion_center_app::api::route53_dns::{
    Route53DnsAdminError, Route53DnsAdminService, Route53RecordControlDto,
    Route53RecordPageRequest, Route53RecordSetKey, Route53RecordType, Route53ZonePageRequest,
};
use edgion_center_core::{
    provider_account_from_desired, CloudProvider, CloudResourceId, CoreResult, CredentialRef,
    CredentialSource, DeletionPolicy, DnsZoneId, ManagementPolicy, NormalizedProviderError,
    ProviderAccount, ProviderAccountCreateResult, ProviderAccountDesired, ProviderAccountPage,
    ProviderAccountPageRequest, ProviderAccountReplaceResult, ProviderAccountScope,
    ProviderAccountSpec, ProviderAccountStore,
};
use tokio::sync::Semaphore;

use super::{ApiFactory, Route53DnsReadService};

const CENTER_ACCOUNT: &str = "aws-main";
const AWS_ACCOUNT: &str = "123456789012";
const ZONE_ID: &str = "Z1234567890EXAMPLE";

#[test]
fn composition_is_default_off_and_enabled_dependencies_fail_closed() {
    assert!(
        super::compose_dns_admin(&super::Route53DnsReadConfig::default(), None, None)
            .unwrap()
            .is_none()
    );
    let enabled = super::Route53DnsReadConfig {
        enabled: true,
        cursor_key_ref: Some("aws/route53-cursor".into()),
        ..Default::default()
    };
    assert!(super::compose_dns_admin(&enabled, None, None).is_err());
    let store: Arc<dyn ProviderAccountStore> = Arc::new(Store(Mutex::new(None)));
    assert!(super::compose_dns_admin(&enabled, Some(store), None).is_err());
}

struct Store(Mutex<Option<ProviderAccount>>);

#[async_trait]
impl ProviderAccountStore for Store {
    async fn create(
        &self,
        _: &CloudResourceId,
        _: &ProviderAccountDesired,
    ) -> CoreResult<ProviderAccountCreateResult> {
        unreachable!()
    }

    async fn get(&self, _: &CloudResourceId) -> CoreResult<Option<ProviderAccount>> {
        Ok(self.0.lock().unwrap().clone())
    }

    async fn list(&self, _: &ProviderAccountPageRequest) -> CoreResult<ProviderAccountPage> {
        unreachable!()
    }

    async fn replace_if_generation(
        &self,
        _: &CloudResourceId,
        _: u64,
        _: &ProviderAccountDesired,
    ) -> CoreResult<ProviderAccountReplaceResult> {
        unreachable!()
    }
}

fn account(generation: u64, credential_source: CredentialSource) -> ProviderAccount {
    provider_account_from_desired(
        CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
        generation,
        &ProviderAccountDesired {
            display_name: "AWS main".into(),
            owner: None,
            labels: BTreeMap::new(),
            management_policy: ManagementPolicy::ObserveOnly,
            deletion_policy: DeletionPolicy::Retain,
            spec: ProviderAccountSpec {
                provider: CloudProvider::Aws,
                scope: Some(ProviderAccountScope::Aws {
                    account_id: AWS_ACCOUNT.into(),
                }),
                credential_source,
            },
        },
    )
    .unwrap()
}

async fn mounted() -> (tempfile::TempDir, Arc<MountedCredentialResolver>) {
    let directory = tempfile::tempdir().unwrap();
    tokio::fs::write(directory.path().join("revision.key"), [7_u8; 32])
        .await
        .unwrap();
    tokio::fs::write(directory.path().join("route53.key"), [8_u8; 32])
        .await
        .unwrap();
    let resolver = MountedCredentialResolver::from_config(&MountedCredentialConfig {
        enabled: true,
        root_directory: Some(directory.path().to_string_lossy().into_owned()),
        revision_key_file: Some("revision.key".into()),
        bindings: vec![MountedCredentialBinding {
            credential_ref: "aws/route53-cursor".into(),
            provider_account_id: CENTER_ACCOUNT.into(),
            provider: CloudProvider::Aws,
            purpose: CredentialPurpose::Route53DnsCursorHmac,
            file: "route53.key".into(),
        }],
    })
    .await
    .unwrap()
    .unwrap();
    (directory, Arc::new(resolver))
}

struct FakeApi {
    verified_account: String,
    zones: Vec<Route53HostedZone>,
    records: Vec<Route53RecordSet>,
    calls: AtomicUsize,
    active: AtomicUsize,
    peak: AtomicUsize,
    delay: Duration,
    rotate_store: Option<Arc<Store>>,
    rotate_key: Option<PathBuf>,
}

impl FakeApi {
    fn new() -> Self {
        Self {
            verified_account: AWS_ACCOUNT.into(),
            zones: vec![Route53HostedZone {
                id: format!("/hostedzone/{ZONE_ID}"),
                name: "example.com.".into(),
                private_zone: false,
                caller_reference: "caller".into(),
                resource_record_set_count: 2,
                name_servers: vec!["ns1.example.net.".into()],
                has_linked_service: false,
                has_unsupported_features: false,
            }],
            records: vec![
                Route53RecordSet {
                    name: "www.example.com.".into(),
                    record_type: "A".into(),
                    ttl: Some(300),
                    resource_records: vec!["192.0.2.10".into()],
                    alias_target: None,
                    set_identifier: Some("weighted-blue".into()),
                    weight: Some(10),
                    failover: None,
                    region: None,
                    geolocation: None,
                    multivalue_answer: None,
                    health_check_id: Some("health-check-1".into()),
                    traffic_policy_instance_id: None,
                    has_cidr_routing_config: false,
                    has_geoproximity_location: false,
                },
                Route53RecordSet {
                    name: "alias.example.com.".into(),
                    record_type: "A".into(),
                    ttl: None,
                    resource_records: Vec::new(),
                    alias_target: Some(Route53AliasTargetData {
                        hosted_zone_id: "Z2FDTNDATAQYW2".into(),
                        dns_name: "d111111abcdef8.cloudfront.net.".into(),
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
                },
            ],
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            delay: Duration::ZERO,
            rotate_store: None,
            rotate_key: None,
        }
    }

    async fn call(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        if let Some(store) = &self.rotate_store {
            *store.0.lock().unwrap() = Some(account(2, CredentialSource::Ambient));
        }
        if let Some(path) = &self.rotate_key {
            std::fs::write(path, [9_u8; 32]).unwrap();
        }
    }
}

#[async_trait]
impl Route53Api for FakeApi {
    fn verified_account_id(&self) -> &str {
        &self.verified_account
    }

    async fn create_hosted_zone(
        &self,
        _: &Route53CreateHostedZoneRequest,
    ) -> Result<Route53CreateHostedZoneResult, NormalizedProviderError> {
        unreachable!()
    }

    async fn get_hosted_zone(
        &self,
        zone_id: &str,
    ) -> Result<Option<Route53HostedZone>, NormalizedProviderError> {
        Ok(self
            .zones
            .iter()
            .find(|zone| zone.id.ends_with(zone_id))
            .cloned())
    }

    async fn list_hosted_zones(
        &self,
        _: Option<&str>,
        _: u16,
    ) -> Result<Route53HostedZonePage, NormalizedProviderError> {
        self.call().await;
        Ok(Route53HostedZonePage {
            items: self.zones.clone(),
            is_truncated: false,
            next_marker: None,
        })
    }

    async fn list_record_sets(
        &self,
        _: &str,
        _: Option<&Route53RecordCursor>,
        _: u16,
    ) -> Result<Route53RecordPage, NormalizedProviderError> {
        self.call().await;
        Ok(Route53RecordPage {
            items: self.records.clone(),
            is_truncated: false,
            next: None,
        })
    }

    async fn change_record_sets(
        &self,
        _: &str,
        _: &Route53ChangeBatch,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        unreachable!()
    }

    async fn get_change(
        &self,
        _: &str,
    ) -> Result<Option<Route53ChangeInfo>, NormalizedProviderError> {
        unreachable!()
    }

    async fn delete_hosted_zone(
        &self,
        _: &str,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        unreachable!()
    }

    async fn get_dnssec(&self, _: &str) -> Result<Route53DnssecInfo, NormalizedProviderError> {
        unreachable!()
    }

    async fn enable_hosted_zone_dnssec(
        &self,
        _: &str,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        unreachable!()
    }

    async fn disable_hosted_zone_dnssec(
        &self,
        _: &str,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        unreachable!()
    }
}

struct FakeFactory(Arc<dyn Route53Api>);

#[async_trait]
impl ApiFactory for FakeFactory {
    async fn build(&self) -> Result<Arc<dyn Route53Api>, NormalizedProviderError> {
        Ok(self.0.clone())
    }
}

fn test_service(
    store: Arc<Store>,
    resolver: Arc<MountedCredentialResolver>,
    api: Arc<dyn Route53Api>,
    per_account_concurrency: usize,
) -> Route53DnsReadService {
    Route53DnsReadService {
        account_store: store,
        mounted_resolver: resolver,
        cursor_key_ref: CredentialRef::new("aws/route53-cursor").unwrap(),
        timeout: Duration::from_secs(5),
        global: Arc::new(Semaphore::new(4)),
        per_account_concurrency,
        accounts: Mutex::new(std::collections::HashMap::new()),
        api_factory: Arc::new(FakeFactory(api)),
    }
}

#[tokio::test]
async fn ambient_account_lists_zones_and_preserves_route53_record_metadata() {
    let (_directory, resolver) = mounted().await;
    let store = Arc::new(Store(Mutex::new(Some(account(
        1,
        CredentialSource::Ambient,
    )))));
    let api = Arc::new(FakeApi::new());
    let service = test_service(store, resolver, api.clone(), 1);
    let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
    let zones = service
        .list_zones(
            &account_id,
            &Route53ZonePageRequest {
                limit: 10,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(zones.items.len(), 1);
    assert_eq!(zones.items[0].zone_id.as_str(), ZONE_ID);
    let zone = service
        .get_zone(&account_id, &DnsZoneId::new(ZONE_ID).unwrap())
        .await
        .unwrap();
    assert_eq!(zone.apex.as_str(), "example.com");

    let records = service
        .list_record_sets(
            &account_id,
            &DnsZoneId::new(ZONE_ID).unwrap(),
            &Route53RecordPageRequest {
                limit: 10,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(records.items.len(), 2);
    assert_eq!(
        records.items[0].control,
        Route53RecordControlDto::ExternalOrManual
    );
    assert!(records.items.iter().any(|record| matches!(
        record.record_set.extension,
        Some(edgion_center_core::DnsRecordExtension::Route53 {
            routing_policy: Some(edgion_center_core::Route53RoutingPolicy::Weighted { weight: 10 }),
            health_check_id: Some(_),
            ..
        })
    )));
    assert!(records.items.iter().any(|record| matches!(
        record.record_set.extension,
        Some(edgion_center_core::DnsRecordExtension::Route53 {
            alias_target: Some(_),
            ..
        })
    )));
    assert_eq!(api.calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn nonambient_and_sts_scope_mismatch_fail_before_route53_reads() {
    let (_directory, resolver) = mounted().await;
    let store = Arc::new(Store(Mutex::new(Some(account(
        1,
        CredentialSource::StaticSecret {
            credential_ref: CredentialRef::new("aws/keys").unwrap(),
        },
    )))));
    let api = Arc::new(FakeApi::new());
    let service = test_service(store, resolver.clone(), api.clone(), 1);
    assert_eq!(
        service
            .list_zones(
                &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                &Route53ZonePageRequest {
                    limit: 10,
                    cursor: None,
                },
            )
            .await,
        Err(Route53DnsAdminError::InvalidRequest)
    );
    assert_eq!(api.calls.load(Ordering::SeqCst), 0);

    let store = Arc::new(Store(Mutex::new(Some(account(
        1,
        CredentialSource::Ambient,
    )))));
    let mut wrong = FakeApi::new();
    wrong.verified_account = "999999999999".into();
    let api = Arc::new(wrong);
    let service = test_service(store, resolver, api.clone(), 1);
    assert_eq!(
        service
            .list_zones(
                &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                &Route53ZonePageRequest {
                    limit: 10,
                    cursor: None,
                },
            )
            .await,
        Err(Route53DnsAdminError::Unavailable)
    );
    assert_eq!(api.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn post_read_account_or_cursor_rotation_discards_observation() {
    let (_directory, resolver) = mounted().await;
    let store = Arc::new(Store(Mutex::new(Some(account(
        1,
        CredentialSource::Ambient,
    )))));
    let mut rotating = FakeApi::new();
    rotating.rotate_store = Some(store.clone());
    let api = Arc::new(rotating);
    let service = test_service(store, resolver, api, 1);
    assert_eq!(
        service
            .list_zones(
                &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                &Route53ZonePageRequest {
                    limit: 10,
                    cursor: None,
                },
            )
            .await,
        Err(Route53DnsAdminError::Unavailable)
    );

    let (directory, resolver) = mounted().await;
    let store = Arc::new(Store(Mutex::new(Some(account(
        1,
        CredentialSource::Ambient,
    )))));
    let mut rotating = FakeApi::new();
    rotating.rotate_key = Some(directory.path().join("route53.key"));
    let api = Arc::new(rotating);
    let service = test_service(store, resolver, api, 1);
    assert_eq!(
        service
            .list_zones(
                &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                &Route53ZonePageRequest {
                    limit: 10,
                    cursor: None,
                },
            )
            .await,
        Err(Route53DnsAdminError::Unavailable)
    );
}

#[tokio::test]
async fn timeout_and_per_account_admission_bound_provider_reads() {
    let (_directory, resolver) = mounted().await;
    let store = Arc::new(Store(Mutex::new(Some(account(
        1,
        CredentialSource::Ambient,
    )))));
    let mut delayed = FakeApi::new();
    delayed.delay = Duration::from_millis(50);
    let api = Arc::new(delayed);
    let mut timed = test_service(store, resolver, api, 1);
    timed.timeout = Duration::from_millis(10);
    assert_eq!(
        timed
            .list_zones(
                &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                &Route53ZonePageRequest {
                    limit: 10,
                    cursor: None,
                },
            )
            .await,
        Err(Route53DnsAdminError::Unavailable)
    );

    let (_directory, resolver) = mounted().await;
    let store = Arc::new(Store(Mutex::new(Some(account(
        1,
        CredentialSource::Ambient,
    )))));
    let mut delayed = FakeApi::new();
    delayed.delay = Duration::from_millis(20);
    let api = Arc::new(delayed);
    let service = test_service(store, resolver, api.clone(), 1);
    let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
    let request = Route53ZonePageRequest {
        limit: 10,
        cursor: None,
    };
    let (first, second) = tokio::join!(
        service.list_zones(&account_id, &request),
        service.list_zones(&account_id, &request),
    );
    first.unwrap();
    second.unwrap();
    assert_eq!(api.peak.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn exact_set_identifier_selects_one_routed_record() {
    let (_directory, resolver) = mounted().await;
    let store = Arc::new(Store(Mutex::new(Some(account(
        1,
        CredentialSource::Ambient,
    )))));
    let api = Arc::new(FakeApi::new());
    let service = test_service(store, resolver, api, 1);
    let record = service
        .get_record_set(
            &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
            &DnsZoneId::new(ZONE_ID).unwrap(),
            &Route53RecordSetKey {
                owner: edgion_center_core::DnsOwnerName::new("www.example.com").unwrap(),
                record_type: Route53RecordType::A,
                set_identifier: Some("weighted-blue".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        record.record_set.key.routing,
        edgion_center_core::DnsRoutingIdentity::Route53 {
            set_identifier: "weighted-blue".into()
        }
    );
}

#[tokio::test]
async fn tampered_cursor_is_invalid_and_changed_inventory_requires_restart() {
    let (_directory, resolver) = mounted().await;
    let store = Arc::new(Store(Mutex::new(Some(account(
        1,
        CredentialSource::Ambient,
    )))));
    let mut first_api = FakeApi::new();
    let mut second_zone = first_api.zones[0].clone();
    second_zone.id = "/hostedzone/ZSECOND".into();
    second_zone.name = "second.example.".into();
    first_api.zones.push(second_zone.clone());
    let service = test_service(store.clone(), resolver.clone(), Arc::new(first_api), 1);
    let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
    let first = service
        .list_zones(
            &account_id,
            &Route53ZonePageRequest {
                limit: 1,
                cursor: None,
            },
        )
        .await
        .unwrap();
    let cursor = first.next_cursor.unwrap();

    let tampered = edgion_center_core::DnsPageToken::new(format!("{}x", cursor.as_str())).unwrap();
    assert_eq!(
        service
            .list_zones(
                &account_id,
                &Route53ZonePageRequest {
                    limit: 1,
                    cursor: Some(tampered),
                },
            )
            .await,
        Err(Route53DnsAdminError::InvalidRequest)
    );

    let mut changed_api = FakeApi::new();
    second_zone.name = "changed.example.".into();
    changed_api.zones.push(second_zone);
    let changed = test_service(store, resolver, Arc::new(changed_api), 1);
    assert_eq!(
        changed
            .list_zones(
                &account_id,
                &Route53ZonePageRequest {
                    limit: 1,
                    cursor: Some(cursor),
                },
            )
            .await,
        Err(Route53DnsAdminError::RestartRequired)
    );
}
