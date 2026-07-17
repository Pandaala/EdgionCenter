//! Opt-in destructive coverage against a disposable Cloud DNS managed zone.

use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use edgion_center_adapter_google_cloud_dns::{
    GoogleCloudDnsAdapter, GoogleCloudDnsCursorKey, GoogleCloudDnsHttpApi,
};
use edgion_center_core::{
    AbsoluteDnsName, CloudProvider, CloudResourceId, CredentialSource, DnsChangeReceipt,
    DnsChangeState, DnsCharacterString, DnsGuardStrength, DnsMutationGuard, DnsOwnerName,
    DnsProvider, DnsRecordChange, DnsRecordSetKey, DnsRecordSetValue, DnsRoutingIdentity, DnsTtl,
    DnsTxtValue, DnsZoneId, DnsZoneRef, ProviderAccountScope, ProviderAccountSpec,
    ProviderDnsRecordSet, ProviderDnsRecordType, ZoneVisibility,
};

const ENABLE: &str = "EDGION_TEST_GOOGLE_DNS";
const CONFIRM: &str = "EDGION_TEST_GOOGLE_DNS_CONFIRM";
const PROJECT: &str = "EDGION_TEST_GOOGLE_DNS_PROJECT_ID";
const ZONE: &str = "EDGION_TEST_GOOGLE_DNS_ZONE_ID";
const APEX: &str = "EDGION_TEST_GOOGLE_DNS_ZONE_APEX";

#[tokio::test]
#[ignore = "mutates a pre-provisioned disposable Cloud DNS zone; see REAL_PROJECT_TESTS.md"]
async fn disposable_zone_create_read_delete() {
    if std::env::var(ENABLE).ok().as_deref() != Some("1") {
        return;
    }
    assert_eq!(required(CONFIRM), "DELETE_ONLY_EDGION_TEST_RECORDS");
    let project = required(PROJECT);
    let zone_id = required(ZONE);
    assert!(
        zone_id.len() <= 20 && zone_id.bytes().all(|value| value.is_ascii_digit()),
        "{ZONE} must be a numeric managed-zone ID"
    );
    let apex = AbsoluteDnsName::new(required(APEX)).expect("invalid test apex");
    let account = ProviderAccountSpec {
        provider: CloudProvider::GoogleCloud,
        scope: Some(ProviderAccountScope::GoogleCloud {
            project_id: project.clone(),
        }),
        credential_source: CredentialSource::Ambient,
    };
    account.validate().expect("invalid Google project scope");
    let api = Arc::new(GoogleCloudDnsHttpApi::ambient(project.clone()).expect("ADC setup failed"));
    let center_id = CloudResourceId::new("google-dns-real-project").unwrap();
    let adapter = GoogleCloudDnsAdapter::new(
        center_id.clone(),
        &account,
        api,
        GoogleCloudDnsCursorKey::new([0x47; 32]).unwrap(),
    )
    .expect("adapter setup failed");
    let zone = DnsZoneRef {
        provider_account_id: center_id,
        provider: CloudProvider::GoogleCloud,
        zone_id: DnsZoneId::new(zone_id).unwrap(),
        apex,
        visibility: ZoneVisibility::Public,
    };
    adapter
        .get_zone(&zone)
        .await
        .expect("zone lookup failed")
        .expect("zone absent");

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let owner = DnsOwnerName::new(format!("edgion-it-{nonce}.{}", zone.apex)).unwrap();
    let marker = format!("edgion-cloud-dns-{nonce}");
    eprintln!("Cloud DNS disposable owner={owner} marker={marker}");
    let record = txt_record(owner, &marker);
    assert!(adapter
        .get_record_set(&zone, &record.key)
        .await
        .unwrap()
        .is_none());

    let create = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: record.clone(),
                guard: DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await;
    let create_error = match create {
        Ok(receipt) => {
            await_done(&adapter, &zone, receipt).await;
            None
        }
        Err(error) => Some(error),
    };
    let mut observed = None;
    for _ in 0..30 {
        observed = adapter
            .get_record_set(&zone, &record.key)
            .await
            .expect("read failed");
        if observed.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let observed = observed
        .unwrap_or_else(|| panic!("created record absent after submission: {create_error:?}"));
    assert_eq!(observed.record_set, record);
    let deletion = adapter
        .apply_record_changes(
            &zone,
            &[DnsRecordChange::Delete {
                guard: DnsMutationGuard::MatchObserved {
                    revision: observed.revision.clone(),
                },
                previous: observed,
            }],
            DnsGuardStrength::Atomic,
        )
        .await;
    let deletion_error = match deletion {
        Ok(receipt) => {
            await_done(&adapter, &zone, receipt).await;
            None
        }
        Err(error) => Some(error),
    };
    for _ in 0..30 {
        if adapter
            .get_record_set(&zone, &record.key)
            .await
            .unwrap()
            .is_none()
        {
            if let Some(error) = create_error.as_ref() {
                panic!("create returned an error even though cleanup reconciled its outcome: {error:?}");
            }
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("cleanup did not remove the owned RRset: {deletion_error:?}");
}

fn required(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| panic!("{name} is required when {ENABLE}=1"))
}

fn txt_record(owner: DnsOwnerName, marker: &str) -> ProviderDnsRecordSet {
    ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner,
            record_type: ProviderDnsRecordType::Txt,
            routing: DnsRoutingIdentity::Simple,
        },
        ttl: DnsTtl::Seconds(60),
        values: BTreeSet::from([DnsRecordSetValue::Txt {
            value: DnsTxtValue::new(vec![
                DnsCharacterString::new(marker.as_bytes().to_vec()).unwrap()
            ])
            .unwrap(),
        }]),
        extension: None,
    }
}

async fn await_done(
    adapter: &GoogleCloudDnsAdapter,
    zone: &DnsZoneRef,
    mut receipt: DnsChangeReceipt,
) {
    for _ in 0..30 {
        if receipt.state == DnsChangeState::ProviderCommitted {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        receipt = adapter
            .observe_change(zone, &receipt.id)
            .await
            .expect("change poll failed");
    }
    panic!("Cloud DNS change did not complete before the test deadline");
}
