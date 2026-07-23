use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_adapter_cloudflare::{
    CloudflareZoneKind as AdapterZoneKind, CloudflareZoneStatus as AdapterZoneStatus,
    ObservedCloudflareZone,
};
use edgion_center_app::api::cloudflare_dns::{
    split_cloudflare_record_tags, CloudflareDnsAdminError, CloudflareOctetsDto,
    CloudflareRecordSetDto, CloudflareRecordTtlDto, CloudflareRecordType, CloudflareRecordValueDto,
    CloudflareZoneDto, CloudflareZoneKind, CloudflareZoneStatus,
};
use edgion_center_core::{
    CloudProvider, CloudflareCnameFlattening, CloudflareProxyOptions, DnsRecordExtension,
    DnsRecordSetValue, DnsRoutingIdentity, DnsTtl, ObservedDnsRecordSet, ProviderDnsRecordType,
    ZoneVisibility,
};

const CLOUDFLARE_ID_LEN: usize = 32;
const MAX_NAMESERVERS: usize = 20;

pub(crate) fn map_zone(
    observed: ObservedCloudflareZone,
) -> Result<CloudflareZoneDto, CloudflareDnsAdminError> {
    observed
        .zone
        .validate()
        .map_err(|_| invalid_observation())?;
    if observed.zone.provider != CloudProvider::Cloudflare
        || !valid_cloudflare_id(observed.zone.zone_id.as_str())
        || observed.name_servers.len() > MAX_NAMESERVERS
        || !zone_visibility_matches_kind(observed.kind, observed.zone.visibility)
    {
        return Err(invalid_observation());
    }
    if let Some(revision) = observed.revision.as_ref() {
        revision.validate().map_err(|_| invalid_observation())?;
    }

    Ok(CloudflareZoneDto {
        provider_account_id: observed.zone.provider_account_id,
        zone_id: observed.zone.zone_id,
        name: observed.zone.apex,
        kind: map_zone_kind(observed.kind),
        status: map_zone_status(observed.status),
        visibility: observed.zone.visibility,
        nameservers: observed.name_servers.into_iter().collect(),
        revision: observed.revision,
    })
}

pub(crate) fn map_record(
    observed: ObservedDnsRecordSet,
) -> Result<CloudflareRecordSetDto, CloudflareDnsAdminError> {
    observed.validate().map_err(|_| invalid_observation())?;
    if observed.zone.provider != CloudProvider::Cloudflare
        || !valid_cloudflare_id(observed.zone.zone_id.as_str())
        || !matches!(observed.record_set.key.routing, DnsRoutingIdentity::Simple)
        || observed.provider_object_ids.is_empty()
        || observed
            .provider_object_ids
            .iter()
            .any(|id| !valid_cloudflare_id(id.as_str()))
    {
        return Err(invalid_observation());
    }

    let record_type = map_record_type(observed.record_set.key.record_type)?;
    if matches!(
        observed.record_set.key.record_type,
        ProviderDnsRecordType::A | ProviderDnsRecordType::Aaaa | ProviderDnsRecordType::Cname
    ) && !matches!(
        observed.record_set.extension.as_ref(),
        Some(DnsRecordExtension::Cloudflare { .. })
    ) {
        return Err(invalid_observation());
    }
    let ttl = map_ttl(observed.record_set.ttl)?;
    let values = observed
        .record_set
        .values
        .into_iter()
        .map(map_record_value)
        .collect::<Result<Vec<_>, _>>()?;
    let extension = map_extension(observed.record_set.extension)?;
    let (tags, control) =
        split_cloudflare_record_tags(extension.tags).map_err(|_| invalid_observation())?;

    Ok(CloudflareRecordSetDto {
        provider_account_id: observed.zone.provider_account_id,
        zone_id: observed.zone.zone_id,
        zone_apex: observed.zone.apex,
        zone_visibility: observed.zone.visibility,
        owner: observed.record_set.key.owner,
        record_type,
        ttl,
        values,
        proxy: extension.proxy,
        cname_flattening: extension.cname_flattening,
        comment: extension.comment,
        tags,
        control,
        provider_object_ids: observed.provider_object_ids.into_iter().collect(),
        revision: observed.revision,
    })
}

fn map_zone_kind(kind: AdapterZoneKind) -> CloudflareZoneKind {
    match kind {
        AdapterZoneKind::Full => CloudflareZoneKind::Full,
        AdapterZoneKind::Partial => CloudflareZoneKind::Partial,
        AdapterZoneKind::Secondary => CloudflareZoneKind::Secondary,
        AdapterZoneKind::Internal => CloudflareZoneKind::Internal,
    }
}

fn map_zone_status(status: AdapterZoneStatus) -> CloudflareZoneStatus {
    match status {
        AdapterZoneStatus::Initializing => CloudflareZoneStatus::Initializing,
        AdapterZoneStatus::Pending => CloudflareZoneStatus::Pending,
        AdapterZoneStatus::Active => CloudflareZoneStatus::Active,
        AdapterZoneStatus::Moved => CloudflareZoneStatus::Moved,
    }
}

fn zone_visibility_matches_kind(kind: AdapterZoneKind, visibility: ZoneVisibility) -> bool {
    matches!(
        (kind, visibility),
        (AdapterZoneKind::Internal, ZoneVisibility::Private)
            | (
                AdapterZoneKind::Full | AdapterZoneKind::Partial | AdapterZoneKind::Secondary,
                ZoneVisibility::Public
            )
    )
}

fn map_record_type(
    record_type: ProviderDnsRecordType,
) -> Result<CloudflareRecordType, CloudflareDnsAdminError> {
    Ok(match record_type {
        ProviderDnsRecordType::A => CloudflareRecordType::A,
        ProviderDnsRecordType::Aaaa => CloudflareRecordType::Aaaa,
        ProviderDnsRecordType::Cname => CloudflareRecordType::Cname,
        ProviderDnsRecordType::Txt => CloudflareRecordType::Txt,
        ProviderDnsRecordType::Mx => CloudflareRecordType::Mx,
        ProviderDnsRecordType::Srv => CloudflareRecordType::Srv,
        ProviderDnsRecordType::Caa => CloudflareRecordType::Caa,
        ProviderDnsRecordType::Ns => CloudflareRecordType::Ns,
        ProviderDnsRecordType::Soa => CloudflareRecordType::Soa,
    })
}

fn map_ttl(ttl: DnsTtl) -> Result<CloudflareRecordTtlDto, CloudflareDnsAdminError> {
    match ttl {
        DnsTtl::Automatic => Ok(CloudflareRecordTtlDto::Automatic),
        DnsTtl::Seconds(seconds) => Ok(CloudflareRecordTtlDto::Seconds(seconds)),
        DnsTtl::Inherited => Err(invalid_observation()),
    }
}

fn map_record_value(
    value: DnsRecordSetValue,
) -> Result<CloudflareRecordValueDto, CloudflareDnsAdminError> {
    Ok(match value {
        DnsRecordSetValue::A { address } => CloudflareRecordValueDto::A { address },
        DnsRecordSetValue::Aaaa { address } => CloudflareRecordValueDto::Aaaa { address },
        DnsRecordSetValue::Cname { target } => CloudflareRecordValueDto::Cname { target },
        DnsRecordSetValue::Txt { value } => CloudflareRecordValueDto::Txt {
            segments: value
                .segments()
                .iter()
                .map(|segment| octets(segment.as_bytes()))
                .collect(),
        },
        DnsRecordSetValue::Mx {
            preference,
            exchange,
        } => CloudflareRecordValueDto::Mx {
            preference,
            exchange,
        },
        DnsRecordSetValue::Srv {
            priority,
            weight,
            port,
            target,
        } => CloudflareRecordValueDto::Srv {
            priority,
            weight,
            port,
            target,
        },
        DnsRecordSetValue::Caa { flags, tag, value } => CloudflareRecordValueDto::Caa {
            flags,
            tag,
            value: octets(value.as_bytes()),
        },
        DnsRecordSetValue::Ns { target } => CloudflareRecordValueDto::Ns { target },
        DnsRecordSetValue::Soa {
            primary_name_server,
            responsible_mailbox,
            serial,
            refresh,
            retry,
            expire,
            minimum,
        } => CloudflareRecordValueDto::Soa {
            primary_name_server,
            responsible_mailbox,
            serial,
            refresh,
            retry,
            expire,
            minimum,
        },
    })
}

struct MappedExtension {
    proxy: Option<CloudflareProxyOptions>,
    cname_flattening: CloudflareCnameFlattening,
    comment: Option<String>,
    tags: std::collections::BTreeSet<String>,
}

fn map_extension(
    extension: Option<DnsRecordExtension>,
) -> Result<MappedExtension, CloudflareDnsAdminError> {
    match extension {
        Some(DnsRecordExtension::Cloudflare {
            proxy,
            cname_flattening,
            comment,
            tags,
        }) => Ok(MappedExtension {
            proxy,
            cname_flattening,
            comment,
            tags,
        }),
        None => Ok(MappedExtension {
            proxy: None,
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: None,
            tags: std::collections::BTreeSet::new(),
        }),
        Some(DnsRecordExtension::Route53 { .. }) => Err(invalid_observation()),
    }
}

fn octets(value: &[u8]) -> CloudflareOctetsDto {
    CloudflareOctetsDto {
        base64: URL_SAFE_NO_PAD.encode(value),
    }
}

fn valid_cloudflare_id(value: &str) -> bool {
    value.len() == CLOUDFLARE_ID_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid_observation() -> CloudflareDnsAdminError {
    CloudflareDnsAdminError::InvalidProviderObservation
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use edgion_center_adapter_cloudflare::{
        CloudflareZoneKind, CloudflareZoneStatus, ObservedCloudflareZone,
    };
    use edgion_center_app::api::cloudflare_dns::{
        CloudflareDnsAdminError, CloudflareRecordControlDto, CloudflareRecordTtlDto,
        CloudflareRecordType, CloudflareRecordValueDto, CloudflareZoneKind as DtoZoneKind,
    };
    use edgion_center_core::{
        AbsoluteDnsName, CaaTag, CloudProvider, CloudResourceId, CloudflareCnameFlattening,
        CloudflareProxyOptions, DnsCharacterString, DnsOwnerName, DnsRecordExtension,
        DnsRecordObjectId, DnsRecordRevision, DnsRecordSetKey, DnsRecordSetValue,
        DnsRoutingIdentity, DnsTtl, DnsTxtValue, DnsZoneId, DnsZoneRef, ObservedDnsRecordSet,
        ProviderDnsRecordSet, ProviderDnsRecordType, ZoneVisibility,
    };

    use super::{map_record, map_zone};

    const ACCOUNT_ID: &str = "provider-account-cf";
    const ZONE_ID: &str = "0123456789abcdef0123456789abcdef";
    const RECORD_ID: &str = "abcdef0123456789abcdef0123456789";

    fn zone() -> DnsZoneRef {
        DnsZoneRef {
            provider_account_id: CloudResourceId::new(ACCOUNT_ID).unwrap(),
            provider: CloudProvider::Cloudflare,
            zone_id: DnsZoneId::new(ZONE_ID).unwrap(),
            apex: AbsoluteDnsName::new("example.com").unwrap(),
            visibility: ZoneVisibility::Public,
        }
    }

    fn observed_record(
        record_type: ProviderDnsRecordType,
        values: BTreeSet<DnsRecordSetValue>,
        ttl: DnsTtl,
        extension: Option<DnsRecordExtension>,
    ) -> ObservedDnsRecordSet {
        ObservedDnsRecordSet {
            zone: zone(),
            record_set: ProviderDnsRecordSet {
                key: DnsRecordSetKey {
                    owner: DnsOwnerName::new("www.example.com").unwrap(),
                    record_type,
                    routing: DnsRoutingIdentity::Simple,
                },
                ttl,
                values,
                extension,
            },
            provider_object_ids: BTreeSet::from([DnsRecordObjectId::new(RECORD_ID).unwrap()]),
            revision: DnsRecordRevision::new("sha256:revision").unwrap(),
        }
    }

    #[test]
    fn maps_zone_without_losing_provider_fields() {
        let observed = ObservedCloudflareZone {
            zone: zone(),
            kind: CloudflareZoneKind::Full,
            status: CloudflareZoneStatus::Active,
            name_servers: BTreeSet::from([
                AbsoluteDnsName::new("ada.ns.cloudflare.com").unwrap(),
                AbsoluteDnsName::new("bob.ns.cloudflare.com").unwrap(),
            ]),
            revision: Some(DnsRecordRevision::new("sha256:zone").unwrap()),
        };

        let dto = map_zone(observed).unwrap();

        assert_eq!(dto.provider_account_id.as_str(), ACCOUNT_ID);
        assert_eq!(dto.zone_id.as_str(), ZONE_ID);
        assert_eq!(dto.kind, DtoZoneKind::Full);
        assert_eq!(dto.nameservers.len(), 2);
        assert_eq!(dto.revision.unwrap().as_str(), "sha256:zone");
    }

    #[test]
    fn rejects_zone_with_non_cloudflare_scope_or_inconsistent_visibility() {
        let mut wrong_provider = zone();
        wrong_provider.provider = CloudProvider::Aws;
        let observation = ObservedCloudflareZone {
            zone: wrong_provider,
            kind: CloudflareZoneKind::Full,
            status: CloudflareZoneStatus::Active,
            name_servers: BTreeSet::new(),
            revision: None,
        };
        assert_eq!(
            map_zone(observation),
            Err(CloudflareDnsAdminError::InvalidProviderObservation)
        );

        let mut private = zone();
        private.visibility = ZoneVisibility::Private;
        let observation = ObservedCloudflareZone {
            zone: private,
            kind: CloudflareZoneKind::Partial,
            status: CloudflareZoneStatus::Pending,
            name_servers: BTreeSet::new(),
            revision: None,
        };
        assert_eq!(
            map_zone(observation),
            Err(CloudflareDnsAdminError::InvalidProviderObservation)
        );
    }

    #[test]
    fn maps_binary_txt_caa_and_all_cloudflare_extension_fields_losslessly() {
        let txt = DnsRecordSetValue::Txt {
            value: DnsTxtValue::new(vec![
                DnsCharacterString::new(vec![0, 0xff, b'a']).unwrap(),
                DnsCharacterString::new(b"second".to_vec()).unwrap(),
            ])
            .unwrap(),
        };
        let extension = DnsRecordExtension::Cloudflare {
            proxy: None,
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: Some("managed record".to_string()),
            tags: BTreeSet::from(["owner:center".to_string(), "team:edge".to_string()]),
        };
        let dto = map_record(observed_record(
            ProviderDnsRecordType::Txt,
            BTreeSet::from([txt]),
            DnsTtl::Seconds(300),
            Some(extension),
        ))
        .unwrap();

        assert_eq!(dto.record_type, CloudflareRecordType::Txt);
        assert_eq!(dto.ttl, CloudflareRecordTtlDto::Seconds(300));
        assert_eq!(dto.comment.as_deref(), Some("managed record"));
        assert_eq!(dto.tags, vec!["owner:center", "team:edge"]);
        assert_eq!(dto.control, CloudflareRecordControlDto::Manual);
        match &dto.values[0] {
            CloudflareRecordValueDto::Txt { segments } => {
                assert_eq!(
                    URL_SAFE_NO_PAD.decode(&segments[0].base64).unwrap(),
                    [0, 0xff, b'a']
                );
                assert_eq!(
                    URL_SAFE_NO_PAD.decode(&segments[1].base64).unwrap(),
                    b"second"
                );
            }
            value => panic!("unexpected value: {value:?}"),
        }

        let caa = DnsRecordSetValue::Caa {
            flags: 128,
            tag: CaaTag::new("issue").unwrap(),
            value: DnsCharacterString::new(vec![0xff, b'x']).unwrap(),
        };
        let dto = map_record(observed_record(
            ProviderDnsRecordType::Caa,
            BTreeSet::from([caa]),
            DnsTtl::Seconds(600),
            None,
        ))
        .unwrap();
        match &dto.values[0] {
            CloudflareRecordValueDto::Caa { value, .. } => {
                assert_eq!(URL_SAFE_NO_PAD.decode(&value.base64).unwrap(), [0xff, b'x']);
            }
            value => panic!("unexpected value: {value:?}"),
        }
    }

    #[test]
    fn projects_remote_control_markers_without_leaking_reserved_tags() {
        let alias = "a".repeat(43);
        let extension = |tags| DnsRecordExtension::Cloudflare {
            proxy: None,
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: None,
            tags,
        };

        let remote = map_record(observed_record(
            ProviderDnsRecordType::Txt,
            BTreeSet::from([DnsRecordSetValue::Txt {
                value: DnsTxtValue::new(vec![DnsCharacterString::new(b"remote".to_vec()).unwrap()])
                    .unwrap(),
            }]),
            DnsTtl::Seconds(300),
            Some(extension(BTreeSet::from([
                "owner:center".to_string(),
                format!("edgion-center-remote:{alias}"),
            ]))),
        ))
        .unwrap();
        assert_eq!(remote.tags, vec!["owner:center"]);
        assert_eq!(
            remote.control,
            CloudflareRecordControlDto::Remote {
                caller_alias: alias
            }
        );

        for reserved in [
            "edgion-center-remote:short",
            "edgion-center-unknown:opaque-provider-value",
        ] {
            let invalid = map_record(observed_record(
                ProviderDnsRecordType::Txt,
                BTreeSet::from([DnsRecordSetValue::Txt {
                    value: DnsTxtValue::new(vec![
                        DnsCharacterString::new(b"invalid".to_vec()).unwrap()
                    ])
                    .unwrap(),
                }]),
                DnsTtl::Seconds(300),
                Some(extension(BTreeSet::from([
                    "owner:center".to_string(),
                    reserved.to_string(),
                ]))),
            ))
            .unwrap();
            assert_eq!(invalid.tags, vec!["owner:center"]);
            assert_eq!(
                invalid.control,
                CloudflareRecordControlDto::InvalidRemoteMarker
            );
            assert!(invalid.tags.iter().all(|tag| tag != reserved));
            assert!(invalid
                .tags
                .iter()
                .all(|tag| !tag.contains("opaque-provider-value")));
        }
    }

    #[test]
    fn maps_proxied_record_and_rejects_invalid_observations() {
        let extension = DnsRecordExtension::Cloudflare {
            proxy: Some(CloudflareProxyOptions::Proxied),
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: None,
            tags: BTreeSet::new(),
        };
        let dto = map_record(observed_record(
            ProviderDnsRecordType::A,
            BTreeSet::from([DnsRecordSetValue::A {
                address: "192.0.2.10".parse().unwrap(),
            }]),
            DnsTtl::Automatic,
            Some(extension),
        ))
        .unwrap();
        assert_eq!(dto.proxy, Some(CloudflareProxyOptions::Proxied));
        assert_eq!(dto.ttl, CloudflareRecordTtlDto::Automatic);

        let mut wrong_provider = observed_record(
            ProviderDnsRecordType::Ns,
            BTreeSet::from([DnsRecordSetValue::Ns {
                target: AbsoluteDnsName::new("ns.example.net").unwrap(),
            }]),
            DnsTtl::Seconds(300),
            None,
        );
        wrong_provider.zone.provider = CloudProvider::Aws;
        assert_eq!(
            map_record(wrong_provider),
            Err(CloudflareDnsAdminError::InvalidProviderObservation)
        );

        let mut invalid_id = observed_record(
            ProviderDnsRecordType::Ns,
            BTreeSet::from([DnsRecordSetValue::Ns {
                target: AbsoluteDnsName::new("ns.example.net").unwrap(),
            }]),
            DnsTtl::Seconds(300),
            None,
        );
        invalid_id.provider_object_ids =
            BTreeSet::from([DnsRecordObjectId::new("not-a-cloudflare-id").unwrap()]);
        assert_eq!(
            map_record(invalid_id),
            Err(CloudflareDnsAdminError::InvalidProviderObservation)
        );
    }

    #[test]
    fn maps_every_supported_record_value_variant() {
        let values = vec![
            (
                ProviderDnsRecordType::A,
                DnsRecordSetValue::A {
                    address: "192.0.2.1".parse().unwrap(),
                },
                CloudflareRecordType::A,
            ),
            (
                ProviderDnsRecordType::Aaaa,
                DnsRecordSetValue::Aaaa {
                    address: "2001:db8::1".parse().unwrap(),
                },
                CloudflareRecordType::Aaaa,
            ),
            (
                ProviderDnsRecordType::Cname,
                DnsRecordSetValue::Cname {
                    target: AbsoluteDnsName::new("target.example.net").unwrap(),
                },
                CloudflareRecordType::Cname,
            ),
            (
                ProviderDnsRecordType::Txt,
                DnsRecordSetValue::Txt {
                    value: DnsTxtValue::new(
                        vec![DnsCharacterString::new(b"txt".to_vec()).unwrap()],
                    )
                    .unwrap(),
                },
                CloudflareRecordType::Txt,
            ),
            (
                ProviderDnsRecordType::Mx,
                DnsRecordSetValue::Mx {
                    preference: 10,
                    exchange: AbsoluteDnsName::new("mail.example.net").unwrap(),
                },
                CloudflareRecordType::Mx,
            ),
            (
                ProviderDnsRecordType::Srv,
                DnsRecordSetValue::Srv {
                    priority: 1,
                    weight: 2,
                    port: 443,
                    target: AbsoluteDnsName::new("service.example.net").unwrap(),
                },
                CloudflareRecordType::Srv,
            ),
            (
                ProviderDnsRecordType::Caa,
                DnsRecordSetValue::Caa {
                    flags: 0,
                    tag: CaaTag::new("issue").unwrap(),
                    value: DnsCharacterString::new(b"ca.example".to_vec()).unwrap(),
                },
                CloudflareRecordType::Caa,
            ),
            (
                ProviderDnsRecordType::Ns,
                DnsRecordSetValue::Ns {
                    target: AbsoluteDnsName::new("ns.example.net").unwrap(),
                },
                CloudflareRecordType::Ns,
            ),
            (
                ProviderDnsRecordType::Soa,
                DnsRecordSetValue::Soa {
                    primary_name_server: AbsoluteDnsName::new("ns.example.net").unwrap(),
                    responsible_mailbox: AbsoluteDnsName::new("hostmaster.example.com").unwrap(),
                    serial: 1,
                    refresh: 2,
                    retry: 3,
                    expire: 4,
                    minimum: 5,
                },
                CloudflareRecordType::Soa,
            ),
        ];

        for (record_type, value, expected_type) in values {
            let extension = matches!(
                record_type,
                ProviderDnsRecordType::A
                    | ProviderDnsRecordType::Aaaa
                    | ProviderDnsRecordType::Cname
            )
            .then(|| DnsRecordExtension::Cloudflare {
                proxy: Some(CloudflareProxyOptions::DnsOnly),
                cname_flattening: CloudflareCnameFlattening::ProviderDefault,
                comment: None,
                tags: BTreeSet::new(),
            });
            let dto = map_record(observed_record(
                record_type,
                BTreeSet::from([value]),
                DnsTtl::Seconds(300),
                extension,
            ))
            .unwrap();
            assert_eq!(dto.record_type, expected_type);
            assert_eq!(dto.values.len(), 1);
        }
    }

    #[test]
    fn rejects_inherited_ttl_and_non_simple_routing() {
        let mut inherited = observed_record(
            ProviderDnsRecordType::Ns,
            BTreeSet::from([DnsRecordSetValue::Ns {
                target: AbsoluteDnsName::new("ns.example.net").unwrap(),
            }]),
            DnsTtl::Inherited,
            None,
        );
        assert_eq!(
            map_record(inherited.clone()),
            Err(CloudflareDnsAdminError::InvalidProviderObservation)
        );

        inherited.record_set.ttl = DnsTtl::Seconds(300);
        inherited.record_set.key.routing = DnsRoutingIdentity::Route53 {
            set_identifier: "weighted-one".to_string(),
        };
        assert_eq!(
            map_record(inherited),
            Err(CloudflareDnsAdminError::InvalidProviderObservation)
        );
    }
}
