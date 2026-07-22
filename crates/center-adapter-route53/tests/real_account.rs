//! Opt-in destructive integration coverage against a disposable public Route 53 zone.
//!
//! This test is both ignored and environment-gated. It performs no AWS SDK setup until every
//! safety variable has been validated.

use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use edgion_center_adapter_route53::{
    AwsRoute53Api, Route53Api, Route53ChangeAction, Route53ChangeBatch, Route53ChangeInfo,
    Route53CursorKey, Route53DnsAdapter, Route53MutationReceiptKey, Route53RecordChange,
    Route53RecordSet,
};
use edgion_center_core::{
    AbsoluteDnsName, CloudProvider, CloudResourceId, CredentialSource, DnsChangeReceipt,
    DnsChangeState, DnsCharacterString, DnsGuardStrength, DnsMutationGuard, DnsOwnerName,
    DnsProvider, DnsRecordChange, DnsRecordSetKey, DnsRecordSetValue, DnsRoutingIdentity, DnsTtl,
    DnsTxtValue, DnsZoneId, DnsZoneRef, ObservedDnsRecordSet, ProviderAccountScope,
    ProviderAccountSpec, ProviderDnsRecordSet, ProviderDnsRecordType, ProviderErrorCategory,
    ZoneVisibility,
};

const ENABLE_ENV: &str = "EDGION_TEST_ROUTE53";
const CONFIRM_ENV: &str = "EDGION_TEST_ROUTE53_CONFIRM";
const CONFIRM_VALUE: &str = "DELETE_ONLY_EDGION_TEST_RECORDS";
const ACCOUNT_ENV: &str = "EDGION_TEST_ROUTE53_EXPECTED_ACCOUNT_ID";
const ZONE_ID_ENV: &str = "EDGION_TEST_ROUTE53_ZONE_ID";
const ZONE_APEX_ENV: &str = "EDGION_TEST_ROUTE53_ZONE_APEX";
const POLL_ATTEMPTS: usize = 30;
const POLL_INTERVAL: Duration = Duration::from_secs(2);

struct RealTestConfig {
    expected_account_id: String,
    zone_id: String,
    zone_apex: AbsoluteDnsName,
}

struct ScenarioRecords {
    created: ProviderDnsRecordSet,
    replaced: ProviderDnsRecordSet,
    race: ProviderDnsRecordSet,
    replaced_raw: Route53RecordSet,
    race_raw: Route53RecordSet,
    stale_desired_raw: Route53RecordSet,
}

impl RealTestConfig {
    fn from_environment() -> Result<Option<Self>, String> {
        if std::env::var(ENABLE_ENV).ok().as_deref() != Some("1") {
            eprintln!("skipping real Route 53 test: set {ENABLE_ENV}=1 to opt in");
            return Ok(None);
        }
        require_env(CONFIRM_ENV).and_then(|value| {
            if value == CONFIRM_VALUE {
                Ok(())
            } else {
                Err(format!("{CONFIRM_ENV} must equal {CONFIRM_VALUE}"))
            }
        })?;

        let expected_account_id = require_env(ACCOUNT_ENV)?;
        if expected_account_id.len() != 12
            || !expected_account_id
                .bytes()
                .all(|value| value.is_ascii_digit())
        {
            return Err(format!("{ACCOUNT_ENV} must be a 12-digit AWS account ID"));
        }

        let configured_zone_id = require_env(ZONE_ID_ENV)?;
        let zone_id = configured_zone_id
            .strip_prefix("/hostedzone/")
            .unwrap_or(&configured_zone_id)
            .to_string();
        if zone_id.is_empty()
            || zone_id.len() > 64
            || !zone_id
                .bytes()
                .all(|value| value.is_ascii_uppercase() || value.is_ascii_digit())
        {
            return Err(format!(
                "{ZONE_ID_ENV} is not a valid public hosted-zone ID"
            ));
        }

        let zone_apex = AbsoluteDnsName::new(require_env(ZONE_APEX_ENV)?)
            .map_err(|error| format!("{ZONE_APEX_ENV} is invalid: {error}"))?;
        Ok(Some(Self {
            expected_account_id,
            zone_id,
            zone_apex,
        }))
    }
}

fn require_env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required when {ENABLE_ENV}=1"))
}

#[tokio::test]
#[ignore = "mutates a pre-provisioned disposable public Route 53 zone; see REAL_ACCOUNT_TESTS.md"]
async fn disposable_public_zone_create_read_and_cleanup() {
    let config = match RealTestConfig::from_environment() {
        Ok(Some(config)) => config,
        Ok(None) => return,
        Err(error) => panic!("Route 53 real-account safety gate rejected the run: {error}"),
    };

    // Loading the ambient credential chain can contact AWS/instance metadata, so it must remain
    // below the complete opt-in and safety validation above.
    let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .load()
        .await;
    let api = Arc::new(
        AwsRoute53Api::new(&sdk_config)
            .await
            .unwrap_or_else(|error| panic!("AWS identity verification failed: {error:?}")),
    );
    let center_account_id = CloudResourceId::new("route53-real-account").unwrap();
    let account = ProviderAccountSpec {
        provider: CloudProvider::Aws,
        scope: Some(ProviderAccountScope::Aws {
            account_id: config.expected_account_id.clone(),
        }),
        credential_source: CredentialSource::Ambient,
    };
    let adapter = Route53DnsAdapter::new_with_record_write_key(
        center_account_id.clone(),
        &account,
        api.clone(),
        Route53CursorKey::new([0x53; 32]).unwrap(),
        Route53MutationReceiptKey::new([0x54; 32]).unwrap(),
    )
    .unwrap_or_else(|error| panic!("AWS account safety binding failed: {error:?}"));
    let zone = DnsZoneRef {
        provider_account_id: center_account_id,
        provider: CloudProvider::Aws,
        zone_id: DnsZoneId::new(config.zone_id).unwrap(),
        apex: config.zone_apex,
        visibility: ZoneVisibility::Public,
    };

    let owner = unique_owner(&zone)
        .unwrap_or_else(|error| panic!("unique test owner preflight failed: {error}"));
    let created = txt_record(&owner, "created")
        .unwrap_or_else(|error| panic!("create TXT marker preflight failed: {error}"));
    let replaced = txt_record(&owner, "replaced")
        .unwrap_or_else(|error| panic!("replace TXT marker preflight failed: {error}"));
    let race = txt_record(&owner, "race")
        .unwrap_or_else(|error| panic!("race TXT marker preflight failed: {error}"));
    let stale_desired = txt_record(&owner, "stale-writer")
        .unwrap_or_else(|error| panic!("stale-writer TXT marker preflight failed: {error}"));
    let replaced_raw = raw_txt_record(&owner, "replaced")
        .unwrap_or_else(|error| panic!("replace raw RRset preflight failed: {error}"));
    let race_raw = raw_txt_record(&owner, "race")
        .unwrap_or_else(|error| panic!("race raw RRset preflight failed: {error}"));
    let stale_desired_raw = raw_txt_record(&owner, "stale-writer")
        .unwrap_or_else(|error| panic!("stale-writer raw RRset preflight failed: {error}"));
    let owned_variants = [
        created.clone(),
        replaced.clone(),
        race.clone(),
        stale_desired,
    ];
    let records = ScenarioRecords {
        created,
        replaced,
        race,
        replaced_raw,
        race_raw,
        stale_desired_raw,
    };
    let mut ownership_armed = false;

    let scenario = run_scenario(
        &adapter,
        api.as_ref(),
        &zone,
        &records,
        &mut ownership_armed,
    )
    .await;
    let cleanup = if ownership_armed {
        cleanup_owned_record(&adapter, &zone, &owned_variants).await
    } else {
        Ok(())
    };

    if let Err(error) = cleanup {
        panic!(
            "Route 53 cleanup failed for {} (manual inspection required; the hosted zone was not deleted): {error}",
            owner.as_str()
        );
    }
    if let Err(error) = scenario {
        panic!("Route 53 real-account scenario failed after successful cleanup: {error}");
    }
}

async fn run_scenario(
    adapter: &Route53DnsAdapter,
    raw_api: &AwsRoute53Api,
    zone: &DnsZoneRef,
    records: &ScenarioRecords,
    ownership_armed: &mut bool,
) -> Result<(), String> {
    let ScenarioRecords {
        created,
        replaced,
        race,
        replaced_raw,
        race_raw,
        stale_desired_raw,
    } = records;
    let observed_zone = adapter
        .get_zone(zone)
        .await
        .map_err(|error| format!("dedicated-zone lookup failed: {error:?}"))?
        .ok_or_else(|| "configured dedicated zone does not exist".to_string())?;
    if observed_zone.zone != *zone {
        return Err("configured zone ID/apex/public identity did not match AWS".to_string());
    }

    if adapter
        .get_record_set(zone, &created.key)
        .await
        .map_err(|error| format!("preflight record lookup failed: {error:?}"))?
        .is_some()
    {
        return Err(format!(
            "unique owner unexpectedly exists; refusing to mutate {}",
            created.key.owner
        ));
    }

    eprintln!(
        "Route 53 disposable test owner={} created_marker={} replaced_marker={} race_marker={} stale_writer_marker={}",
        created.key.owner.as_str(),
        txt_marker(&created.key.owner, "created")?,
        txt_marker(&created.key.owner, "replaced")?,
        txt_marker(&created.key.owner, "race")?,
        txt_marker(&created.key.owner, "stale-writer")?,
    );
    // Arm cleanup before submission because a transport error can hide an accepted mutation.
    *ownership_armed = true;
    let receipt = adapter
        .apply_record_changes(
            zone,
            &[DnsRecordChange::Create {
                record_set: created.clone(),
                guard: DnsMutationGuard::MustNotExist,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .map_err(|error| format!("create submission failed or had unknown outcome: {error:?}"))?;
    await_committed(adapter, zone, receipt).await?;

    let observed = adapter
        .get_record_set(zone, &created.key)
        .await
        .map_err(|error| format!("created-record lookup failed: {error:?}"))?
        .ok_or_else(|| {
            "provider committed the change but the created record is absent".to_string()
        })?;
    if observed.record_set != *created {
        return Err("created record does not exactly match the requested RRset".to_string());
    }

    let receipt = adapter
        .apply_record_changes(
            zone,
            &[DnsRecordChange::Replace {
                guard: DnsMutationGuard::MatchObserved {
                    revision: observed.revision.clone(),
                },
                previous: observed,
                desired: replaced.clone(),
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .map_err(|error| format!("exact replace failed or had unknown outcome: {error:?}"))?;
    await_committed(adapter, zone, receipt).await?;

    let observed = adapter
        .get_record_set(zone, &replaced.key)
        .await
        .map_err(|error| format!("replaced-record lookup failed: {error:?}"))?
        .ok_or_else(|| {
            "provider committed the replace but the replacement record is absent".to_string()
        })?;
    if observed.record_set != *replaced {
        return Err("replaced record does not exactly match the requested RRset".to_string());
    }

    let race_change = raw_api
        .change_record_sets(
            zone.zone_id.as_str(),
            &exact_replace_batch(replaced_raw, race_raw, "edgion-real-race-writer"),
        )
        .await
        .map_err(|error| format!("external race-writer submission failed: {error:?}"))?;
    await_raw_committed(raw_api, race_change).await?;
    assert_exact_record(adapter, zone, race, "race-writer").await?;

    let stale_result = raw_api
        .change_record_sets(
            zone.zone_id.as_str(),
            &exact_replace_batch(
                replaced_raw,
                stale_desired_raw,
                "edgion-real-stale-exact-batch",
            ),
        )
        .await;
    let stale_error = match stale_result {
        Err(error) => error,
        Ok(change) => {
            await_raw_committed(raw_api, change).await?;
            return Err("Route 53 unexpectedly accepted the stale exact batch".to_string());
        }
    };
    if stale_error.category() != ProviderErrorCategory::Conflict {
        return Err(format!(
            "stale exact batch returned {:?}, expected Conflict: {stale_error:?}",
            stale_error.category()
        ));
    }
    assert_exact_record(adapter, zone, race, "post-stale-batch").await?;
    Ok(())
}

async fn assert_exact_record(
    adapter: &Route53DnsAdapter,
    zone: &DnsZoneRef,
    expected: &ProviderDnsRecordSet,
    phase: &str,
) -> Result<(), String> {
    let observed = adapter
        .get_record_set(zone, &expected.key)
        .await
        .map_err(|error| format!("{phase} record lookup failed: {error:?}"))?
        .ok_or_else(|| format!("{phase} record is absent"))?;
    if observed.record_set == *expected {
        Ok(())
    } else {
        Err(format!("{phase} record does not exactly match its marker"))
    }
}

fn exact_replace_batch(
    previous: &Route53RecordSet,
    desired: &Route53RecordSet,
    comment: &str,
) -> Route53ChangeBatch {
    Route53ChangeBatch {
        changes: vec![
            Route53RecordChange {
                action: Route53ChangeAction::Delete,
                record_set: previous.clone(),
            },
            Route53RecordChange {
                action: Route53ChangeAction::Create,
                record_set: desired.clone(),
            },
        ],
        comment: comment.to_string(),
    }
}

async fn await_raw_committed(
    api: &AwsRoute53Api,
    mut change: Route53ChangeInfo,
) -> Result<(), String> {
    for _ in 0..POLL_ATTEMPTS {
        match change.status.as_str() {
            "INSYNC" => return Ok(()),
            "PENDING" => {
                tokio::time::sleep(POLL_INTERVAL).await;
                change = api
                    .get_change(&change.id)
                    .await
                    .map_err(|error| format!("raw Route 53 change polling failed: {error:?}"))?
                    .ok_or_else(|| "raw Route 53 change disappeared while polling".to_string())?;
            }
            status => return Err(format!("raw Route 53 returned unsupported status {status}")),
        }
    }
    Err(format!(
        "raw Route 53 change did not commit within {} seconds",
        POLL_ATTEMPTS * POLL_INTERVAL.as_secs() as usize
    ))
}

async fn cleanup_owned_record(
    adapter: &Route53DnsAdapter,
    zone: &DnsZoneRef,
    owned_variants: &[ProviderDnsRecordSet],
) -> Result<(), String> {
    let Some(expected_key) = owned_variants.first().map(|record| &record.key) else {
        return Err("cleanup requires at least one owned record variant".to_string());
    };
    // A lost create response can race early reads. Use the full change-poll budget and require
    // consecutive absence observations before concluding there is nothing to clean up.
    let mut observed = None;
    let mut consecutive_absence = 0usize;
    for attempt in 0..POLL_ATTEMPTS {
        observed = adapter
            .get_record_set(zone, expected_key)
            .await
            .map_err(|error| format!("cleanup lookup failed: {error:?}"))?;
        if observed.is_some() {
            break;
        }
        consecutive_absence += 1;
        if attempt + 1 < POLL_ATTEMPTS {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
    let Some(observed) = observed else {
        return if consecutive_absence == POLL_ATTEMPTS {
            Ok(())
        } else {
            Err("cleanup could not confirm consecutive record absence".to_string())
        };
    };
    refuse_foreign_record(&observed, owned_variants)?;

    let receipt = adapter
        .apply_record_changes(
            zone,
            &[DnsRecordChange::Delete {
                guard: DnsMutationGuard::MatchObserved {
                    revision: observed.revision.clone(),
                },
                previous: observed,
            }],
            DnsGuardStrength::Atomic,
        )
        .await
        .map_err(|error| format!("cleanup delete submission failed: {error:?}"))?;
    await_committed(adapter, zone, receipt).await?;

    match adapter
        .get_record_set(zone, expected_key)
        .await
        .map_err(|error| format!("post-cleanup lookup failed: {error:?}"))?
    {
        None => Ok(()),
        Some(observed) => {
            refuse_foreign_record(&observed, owned_variants)?;
            Err("owned test RRset still exists after committed cleanup".to_string())
        }
    }
}

fn refuse_foreign_record(
    observed: &ObservedDnsRecordSet,
    owned_variants: &[ProviderDnsRecordSet],
) -> Result<(), String> {
    if owned_variants.contains(&observed.record_set) {
        Ok(())
    } else {
        Err(format!(
            "refusing to delete {} because its current content is not the unique test marker",
            observed.record_set.key.owner
        ))
    }
}

async fn await_committed(
    adapter: &Route53DnsAdapter,
    zone: &DnsZoneRef,
    mut receipt: DnsChangeReceipt,
) -> Result<(), String> {
    for _ in 0..POLL_ATTEMPTS {
        match receipt.state {
            DnsChangeState::ProviderCommitted => return Ok(()),
            DnsChangeState::Pending => {
                tokio::time::sleep(POLL_INTERVAL).await;
                receipt = adapter
                    .observe_change(zone, &receipt.id)
                    .await
                    .map_err(|error| format!("change polling failed: {error:?}"))?;
            }
            DnsChangeState::Failed => return Err("Route 53 reported a failed change".to_string()),
            DnsChangeState::UnknownOutcome => {
                return Err("Route 53 reported an unknown change outcome".to_string())
            }
        }
    }
    Err(format!(
        "Route 53 change did not commit within {} seconds",
        POLL_ATTEMPTS * POLL_INTERVAL.as_secs() as usize
    ))
}

fn unique_owner(zone: &DnsZoneRef) -> Result<DnsOwnerName, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))?
        .as_nanos();
    let label = format!("edgion-it-{:x}-{nanos:x}", std::process::id());
    if label.len() > 63 {
        return Err("generated owner label exceeds the DNS 63-byte limit".to_string());
    }
    let owner = format!("{label}.{}", zone.apex);
    if owner.len() > 253 {
        return Err("zone apex leaves no room for the unique test owner".to_string());
    }
    DnsOwnerName::new(owner).map_err(|error| format!("generated owner is invalid: {error}"))
}

fn txt_record(owner: &DnsOwnerName, phase: &str) -> Result<ProviderDnsRecordSet, String> {
    let marker = txt_marker(owner, phase)?;
    let character_string = DnsCharacterString::new(marker.into_bytes())
        .map_err(|error| format!("{phase} marker is invalid: {error}"))?;
    let txt_value = DnsTxtValue::new(vec![character_string])
        .map_err(|error| format!("{phase} TXT value is invalid: {error}"))?;
    Ok(ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner: owner.clone(),
            record_type: ProviderDnsRecordType::Txt,
            routing: DnsRoutingIdentity::Simple,
        },
        ttl: DnsTtl::Seconds(60),
        values: BTreeSet::from([DnsRecordSetValue::Txt { value: txt_value }]),
        extension: None,
    })
}

fn raw_txt_record(owner: &DnsOwnerName, phase: &str) -> Result<Route53RecordSet, String> {
    let marker = txt_marker(owner, phase)?;
    Ok(Route53RecordSet {
        name: owner.fqdn(),
        record_type: "TXT".to_string(),
        ttl: Some(60),
        resource_records: vec![format!("\"{marker}\"")],
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
    })
}

fn txt_marker(owner: &DnsOwnerName, phase: &str) -> Result<String, String> {
    let marker = format!("edgion-route53-real-account:{phase}:{}", owner.as_str());
    if marker.len() > 255 {
        return Err(format!(
            "{phase} marker exceeds the Route 53 TXT character-string limit"
        ));
    }
    Ok(marker)
}
