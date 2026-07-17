use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    AbsoluteDnsName, CaaTag, DnsCharacterString, DnsOwnerName, DnsRecordExtension,
    DnsRecordRevision, DnsRecordSetKey, DnsRecordSetValue, DnsRoutingIdentity, DnsTtl, DnsTxtValue,
    DnsZoneRef, GoogleDnsGeoPolicy, GoogleDnsGeoPolicyItem, GoogleDnsHealthCheckRef,
    GoogleDnsHealthCheckTargets, GoogleDnsInternalLoadBalancerTarget, GoogleDnsIpProtocol,
    GoogleDnsLoadBalancerType, GoogleDnsPolicyItemData, GoogleDnsRoutingPolicy,
    GoogleDnsRoutingPolicyKind, GoogleDnsTrickleTraffic, GoogleDnsWeight, GoogleDnsWrrPolicyItem,
    ProviderDnsRecordSet, ProviderDnsRecordType, ProviderRegion,
};
use sha2::{Digest, Sha256};

use crate::{
    validation, GoogleGeoPolicy, GoogleHealthCheckTargets, GooglePrimaryBackupPolicy,
    GoogleResourceRecordSet, GoogleRoutingData, GoogleRoutingPolicy as RawRoutingPolicy,
    GoogleWrrPolicy,
};

pub(crate) fn map_record_set(
    zone: &DnsZoneRef,
    raw: GoogleResourceRecordSet,
) -> crate::Result<(ProviderDnsRecordSet, DnsRecordRevision)> {
    if raw.name.is_empty() || raw.ttl > i32::MAX as u32 {
        return Err(validation("invalid_google_record_shape"));
    }
    let record_type = map_type(&raw.record_type)?;
    let owner =
        DnsOwnerName::new(&raw.name).map_err(|_| validation("invalid_google_record_name"))?;
    let (values, extension) = if let Some(policy) = raw.routing_policy.as_ref() {
        if !raw.rrdatas.is_empty() || record_type == ProviderDnsRecordType::GoogleAlias {
            return Err(validation("invalid_google_routing_shape"));
        }
        (
            BTreeSet::new(),
            Some(DnsRecordExtension::GoogleCloud {
                routing_policy: Box::new(map_routing(record_type, policy)?),
            }),
        )
    } else if record_type == ProviderDnsRecordType::GoogleAlias {
        if raw.rrdatas.len() != 1 || !raw.signature_rrdatas.is_empty() {
            return Err(validation("invalid_google_alias_shape"));
        }
        let target = absolute(&raw.rrdatas[0], "invalid_google_alias_target")?;
        (
            BTreeSet::new(),
            Some(DnsRecordExtension::GoogleAlias { target }),
        )
    } else {
        if raw.rrdatas.is_empty() {
            return Err(validation("invalid_google_record_shape"));
        }
        let values = raw
            .rrdatas
            .iter()
            .map(|value| parse_value(record_type, value))
            .collect::<crate::Result<BTreeSet<_>>>()?;
        (values, None)
    };
    let mapped = ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner,
            record_type,
            routing: DnsRoutingIdentity::Simple,
        },
        ttl: DnsTtl::Seconds(raw.ttl),
        values,
        extension,
    };
    mapped
        .validate(zone)
        .map_err(|_| validation("invalid_google_record_set"))?;
    let digest =
        serde_json::to_vec(&raw).map_err(|_| validation("google_record_encoding_failed"))?;
    let revision = DnsRecordRevision::new(URL_SAFE_NO_PAD.encode(Sha256::digest(digest)))
        .map_err(|_| validation("invalid_google_record_revision"))?;
    Ok((mapped, revision))
}

pub(crate) fn render_record_set(
    zone: &DnsZoneRef,
    record: &ProviderDnsRecordSet,
) -> crate::Result<GoogleResourceRecordSet> {
    record
        .validate(zone)
        .map_err(|_| validation("invalid_google_record_set"))?;
    let mut raw = GoogleResourceRecordSet {
        name: record.key.owner.fqdn(),
        record_type: render_type(record.key.record_type).to_string(),
        ttl: match record.ttl {
            DnsTtl::Seconds(value) => value,
            _ => return Err(validation("invalid_google_record_ttl")),
        },
        rrdatas: Vec::new(),
        signature_rrdatas: Vec::new(),
        routing_policy: None,
        extra: Default::default(),
    };
    match &record.extension {
        Some(DnsRecordExtension::GoogleAlias { target }) => raw.rrdatas.push(target.fqdn()),
        Some(DnsRecordExtension::GoogleCloud { routing_policy }) => {
            raw.routing_policy = Some(render_routing(routing_policy)?);
        }
        None => {
            raw.rrdatas = record
                .values
                .iter()
                .map(render_value)
                .collect::<crate::Result<Vec<_>>>()?
        }
        Some(_) => return Err(validation("invalid_google_record_extension")),
    }
    let (roundtrip, _) = map_record_set(zone, raw.clone())?;
    if roundtrip != *record {
        return Err(validation("google_record_render_not_lossless"));
    }
    Ok(raw)
}

fn map_type(value: &str) -> crate::Result<ProviderDnsRecordType> {
    Ok(match value {
        "A" => ProviderDnsRecordType::A,
        "AAAA" => ProviderDnsRecordType::Aaaa,
        "CNAME" => ProviderDnsRecordType::Cname,
        "TXT" => ProviderDnsRecordType::Txt,
        "MX" => ProviderDnsRecordType::Mx,
        "SRV" => ProviderDnsRecordType::Srv,
        "CAA" => ProviderDnsRecordType::Caa,
        "NS" => ProviderDnsRecordType::Ns,
        "SOA" => ProviderDnsRecordType::Soa,
        "ALIAS" => ProviderDnsRecordType::GoogleAlias,
        _ => return Err(validation("unsupported_google_record_type")),
    })
}

fn render_type(value: ProviderDnsRecordType) -> &'static str {
    match value {
        ProviderDnsRecordType::A => "A",
        ProviderDnsRecordType::Aaaa => "AAAA",
        ProviderDnsRecordType::Cname => "CNAME",
        ProviderDnsRecordType::Txt => "TXT",
        ProviderDnsRecordType::Mx => "MX",
        ProviderDnsRecordType::Srv => "SRV",
        ProviderDnsRecordType::Caa => "CAA",
        ProviderDnsRecordType::Ns => "NS",
        ProviderDnsRecordType::Soa => "SOA",
        ProviderDnsRecordType::GoogleAlias => "ALIAS",
    }
}

fn parse_value(kind: ProviderDnsRecordType, value: &str) -> crate::Result<DnsRecordSetValue> {
    let fields = || value.split_ascii_whitespace().collect::<Vec<_>>();
    Ok(match kind {
        ProviderDnsRecordType::A => DnsRecordSetValue::A {
            address: value
                .parse::<Ipv4Addr>()
                .map_err(|_| validation("invalid_google_a_value"))?,
        },
        ProviderDnsRecordType::Aaaa => DnsRecordSetValue::Aaaa {
            address: value
                .parse::<Ipv6Addr>()
                .map_err(|_| validation("invalid_google_aaaa_value"))?,
        },
        ProviderDnsRecordType::Cname => DnsRecordSetValue::Cname {
            target: absolute(value, "invalid_google_cname_value")?,
        },
        ProviderDnsRecordType::Ns => DnsRecordSetValue::Ns {
            target: absolute(value, "invalid_google_ns_value")?,
        },
        ProviderDnsRecordType::Txt => DnsRecordSetValue::Txt {
            value: parse_txt(value)?,
        },
        ProviderDnsRecordType::Mx => {
            let f = fields();
            if f.len() != 2 {
                return Err(validation("invalid_google_mx_value"));
            }
            DnsRecordSetValue::Mx {
                preference: f[0]
                    .parse()
                    .map_err(|_| validation("invalid_google_mx_value"))?,
                exchange: absolute(f[1], "invalid_google_mx_value")?,
            }
        }
        ProviderDnsRecordType::Srv => {
            let f = fields();
            if f.len() != 4 {
                return Err(validation("invalid_google_srv_value"));
            }
            DnsRecordSetValue::Srv {
                priority: f[0]
                    .parse()
                    .map_err(|_| validation("invalid_google_srv_value"))?,
                weight: f[1]
                    .parse()
                    .map_err(|_| validation("invalid_google_srv_value"))?,
                port: f[2]
                    .parse()
                    .map_err(|_| validation("invalid_google_srv_value"))?,
                target: absolute(f[3], "invalid_google_srv_value")?,
            }
        }
        ProviderDnsRecordType::Soa => {
            let f = fields();
            if f.len() != 7 {
                return Err(validation("invalid_google_soa_value"));
            }
            DnsRecordSetValue::Soa {
                primary_name_server: absolute(f[0], "invalid_google_soa_value")?,
                responsible_mailbox: absolute(f[1], "invalid_google_soa_value")?,
                serial: parse_u32(f[2])?,
                refresh: parse_u32(f[3])?,
                retry: parse_u32(f[4])?,
                expire: parse_u32(f[5])?,
                minimum: parse_u32(f[6])?,
            }
        }
        ProviderDnsRecordType::Caa => {
            let mut f = value
                .splitn(3, char::is_whitespace)
                .filter(|v| !v.is_empty());
            let flags = f
                .next()
                .ok_or_else(|| validation("invalid_google_caa_value"))?
                .parse()
                .map_err(|_| validation("invalid_google_caa_value"))?;
            let tag = CaaTag::new(
                f.next()
                    .ok_or_else(|| validation("invalid_google_caa_value"))?,
            )
            .map_err(|_| validation("invalid_google_caa_value"))?;
            let text = parse_txt(
                f.next()
                    .ok_or_else(|| validation("invalid_google_caa_value"))?,
            )?;
            if text.segments().len() != 1 {
                return Err(validation("invalid_google_caa_value"));
            }
            DnsRecordSetValue::Caa {
                flags,
                tag,
                value: text.segments()[0].clone(),
            }
        }
        ProviderDnsRecordType::GoogleAlias => return Err(validation("invalid_google_alias_shape")),
    })
}

fn render_value(value: &DnsRecordSetValue) -> crate::Result<String> {
    Ok(match value {
        DnsRecordSetValue::A { address } => address.to_string(),
        DnsRecordSetValue::Aaaa { address } => address.to_string(),
        DnsRecordSetValue::Cname { target } | DnsRecordSetValue::Ns { target } => target.fqdn(),
        DnsRecordSetValue::Txt { value } => value
            .segments()
            .iter()
            .map(|v| quote(v.as_bytes()))
            .collect::<Vec<_>>()
            .join(" "),
        DnsRecordSetValue::Mx {
            preference,
            exchange,
        } => format!("{preference} {}", exchange.fqdn()),
        DnsRecordSetValue::Srv {
            priority,
            weight,
            port,
            target,
        } => format!("{priority} {weight} {port} {}", target.fqdn()),
        DnsRecordSetValue::Caa { flags, tag, value } => {
            format!("{flags} {} {}", tag.as_str(), quote(value.as_bytes()))
        }
        DnsRecordSetValue::Soa {
            primary_name_server,
            responsible_mailbox,
            serial,
            refresh,
            retry,
            expire,
            minimum,
        } => format!(
            "{} {} {serial} {refresh} {retry} {expire} {minimum}",
            primary_name_server.fqdn(),
            responsible_mailbox.fqdn()
        ),
    })
}

fn absolute(value: &str, code: &str) -> crate::Result<AbsoluteDnsName> {
    AbsoluteDnsName::new(value).map_err(|_| validation(code))
}
fn parse_u32(value: &str) -> crate::Result<u32> {
    value
        .parse()
        .map_err(|_| validation("invalid_google_soa_value"))
}
fn quote(value: &[u8]) -> String {
    let mut out = String::from("\"");
    for b in value {
        match *b {
            b'"' | b'\\' => {
                out.push('\\');
                out.push(char::from(*b));
            }
            0x20..=0x7e => out.push(char::from(*b)),
            n => out.push_str(&format!("\\{n:03}")),
        }
    }
    out.push('"');
    out
}

fn parse_txt(value: &str) -> crate::Result<DnsTxtValue> {
    let bytes = value.as_bytes();
    let mut i = 0;
    let mut segments = Vec::new();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == bytes.len() {
            break;
        }
        if bytes[i] != b'"' {
            return Err(validation("invalid_google_txt_value"));
        }
        i += 1;
        let mut out = Vec::new();
        while i < bytes.len() && bytes[i] != b'"' {
            if bytes[i] == b'\\' {
                i += 1;
                if i >= bytes.len() {
                    return Err(validation("invalid_google_txt_value"));
                }
                if i + 2 < bytes.len() && bytes[i..i + 3].iter().all(u8::is_ascii_digit) {
                    let n = (bytes[i] - b'0') * 100
                        + (bytes[i + 1] - b'0') * 10
                        + (bytes[i + 2] - b'0');
                    out.push(n);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        if i >= bytes.len() {
            return Err(validation("invalid_google_txt_value"));
        }
        i += 1;
        segments.push(
            DnsCharacterString::new(out).map_err(|_| validation("invalid_google_txt_value"))?,
        );
    }
    DnsTxtValue::new(segments).map_err(|_| validation("invalid_google_txt_value"))
}

fn map_routing(
    kind: ProviderDnsRecordType,
    raw: &RawRoutingPolicy,
) -> crate::Result<GoogleDnsRoutingPolicy> {
    let health_check = raw
        .health_check
        .as_deref()
        .map(parse_health_check)
        .transpose()?;
    let policy = match &raw.routing_data {
        GoogleRoutingData::Geo { geo: value } => GoogleDnsRoutingPolicyKind::Geolocation {
            policy: map_geo(kind, value)?,
        },
        GoogleRoutingData::Wrr { wrr: value } => GoogleDnsRoutingPolicyKind::WeightedRoundRobin {
            items: value
                .items
                .iter()
                .map(|item| {
                    Ok(GoogleDnsWrrPolicyItem {
                        weight: GoogleDnsWeight::new(item.weight.to_string())
                            .map_err(|_| validation("invalid_google_routing_weight"))?,
                        data: map_item_data(kind, &item.rrdatas, &item.health_checked_targets)?,
                    })
                })
                .collect::<crate::Result<Vec<_>>>()?,
        },
        GoogleRoutingData::PrimaryBackup {
            primary_backup: value,
        } => GoogleDnsRoutingPolicyKind::PrimaryBackup {
            primary_targets: map_targets(&value.primary_targets)?,
            backup_geo_targets: map_geo(kind, &value.backup_geo_targets)?,
            trickle_traffic: GoogleDnsTrickleTraffic::new(value.trickle_traffic.to_string())
                .map_err(|_| validation("invalid_google_trickle_traffic"))?,
        },
    };
    Ok(GoogleDnsRoutingPolicy {
        health_check,
        policy,
    })
}

fn map_geo(
    kind: ProviderDnsRecordType,
    raw: &GoogleGeoPolicy,
) -> crate::Result<GoogleDnsGeoPolicy> {
    Ok(GoogleDnsGeoPolicy {
        items: raw
            .items
            .iter()
            .map(|item| {
                Ok(GoogleDnsGeoPolicyItem {
                    location: ProviderRegion::new(&item.location)
                        .map_err(|_| validation("invalid_google_geo_location"))?,
                    data: map_item_data(kind, &item.rrdatas, &item.health_checked_targets)?,
                })
            })
            .collect::<crate::Result<Vec<_>>>()?,
        enable_fencing: raw.enable_fencing,
    })
}

fn map_item_data(
    kind: ProviderDnsRecordType,
    values: &[String],
    targets: &Option<GoogleHealthCheckTargets>,
) -> crate::Result<GoogleDnsPolicyItemData> {
    Ok(GoogleDnsPolicyItemData {
        values: values
            .iter()
            .map(|value| parse_value(kind, value))
            .collect::<crate::Result<BTreeSet<_>>>()?,
        health_checked_targets: targets.as_ref().map(map_targets).transpose()?,
    })
}

fn map_targets(raw: &GoogleHealthCheckTargets) -> crate::Result<GoogleDnsHealthCheckTargets> {
    match (
        raw.external_endpoints.is_empty(),
        raw.internal_load_balancers.is_empty(),
    ) {
        (false, true) => Ok(GoogleDnsHealthCheckTargets::ExternalEndpoints {
            addresses: raw
                .external_endpoints
                .iter()
                .map(|value| {
                    value
                        .parse::<IpAddr>()
                        .map_err(|_| validation("invalid_google_external_endpoint"))
                })
                .collect::<crate::Result<BTreeSet<_>>>()?,
        }),
        (true, false) => {
            Ok(GoogleDnsHealthCheckTargets::InternalLoadBalancers {
                targets: raw
                    .internal_load_balancers
                    .iter()
                    .map(|value| {
                        let (network_project_id, network) = parse_network_url(&value.network_url)?;
                        let load_balancer_type = match value.load_balancer_type.as_str() {
                            "globalL7ilb" => GoogleDnsLoadBalancerType::GlobalL7,
                            "regionalL4ilb" => GoogleDnsLoadBalancerType::RegionalL4,
                            "regionalL7ilb" => GoogleDnsLoadBalancerType::RegionalL7,
                            _ => return Err(validation("invalid_google_load_balancer_type")),
                        };
                        let ip_protocol = match value.ip_protocol.as_str() {
                            "tcp" => GoogleDnsIpProtocol::Tcp,
                            "udp" => GoogleDnsIpProtocol::Udp,
                            _ => return Err(validation("invalid_google_ip_protocol")),
                        };
                        Ok(GoogleDnsInternalLoadBalancerTarget {
                            load_balancer_type,
                            ip_address: value
                                .ip_address
                                .parse()
                                .map_err(|_| validation("invalid_google_load_balancer_ip"))?,
                            port: value
                                .port
                                .parse()
                                .map_err(|_| validation("invalid_google_load_balancer_port"))?,
                            ip_protocol,
                            network_project_id,
                            network,
                            project_id: value.project.clone(),
                            region: if value.region.is_empty() {
                                None
                            } else {
                                Some(ProviderRegion::new(&value.region).map_err(|_| {
                                    validation("invalid_google_load_balancer_region")
                                })?)
                            },
                        })
                    })
                    .collect::<crate::Result<BTreeSet<_>>>()?,
            })
        }
        _ => Err(validation("invalid_google_health_check_targets")),
    }
}

fn parse_health_check(value: &str) -> crate::Result<GoogleDnsHealthCheckRef> {
    let rest = value
        .strip_prefix("https://www.googleapis.com/compute/v1/projects/")
        .ok_or_else(|| validation("invalid_google_health_check_url"))?;
    let (project_id, name) = rest
        .split_once("/global/healthChecks/")
        .ok_or_else(|| validation("invalid_google_health_check_url"))?;
    if project_id.is_empty() || name.is_empty() || name.contains('/') {
        return Err(validation("invalid_google_health_check_url"));
    }
    Ok(GoogleDnsHealthCheckRef {
        project_id: project_id.into(),
        health_check: name.into(),
    })
}

fn parse_network_url(value: &str) -> crate::Result<(String, String)> {
    let rest = value
        .strip_prefix("https://www.googleapis.com/compute/v1/projects/")
        .ok_or_else(|| validation("invalid_google_network_url"))?;
    let (project, network) = rest
        .split_once("/global/networks/")
        .ok_or_else(|| validation("invalid_google_network_url"))?;
    if project.is_empty() || network.is_empty() || network.contains('/') {
        return Err(validation("invalid_google_network_url"));
    }
    Ok((project.into(), network.into()))
}

fn render_routing(value: &GoogleDnsRoutingPolicy) -> crate::Result<RawRoutingPolicy> {
    let health_check = value.health_check.as_ref().map(|value| {
        format!(
            "https://www.googleapis.com/compute/v1/projects/{}/global/healthChecks/{}",
            value.project_id, value.health_check
        )
    });
    let routing_data = match &value.policy {
        GoogleDnsRoutingPolicyKind::Geolocation { policy } => GoogleRoutingData::Geo {
            geo: render_geo(policy)?,
        },
        GoogleDnsRoutingPolicyKind::WeightedRoundRobin { items } => GoogleRoutingData::Wrr {
            wrr: GoogleWrrPolicy {
                items: items
                    .iter()
                    .map(|item| {
                        Ok(crate::GoogleWrrPolicyItem {
                            weight: item
                                .weight
                                .as_str()
                                .parse()
                                .map_err(|_| validation("invalid_google_routing_weight"))?,
                            rrdatas: item
                                .data
                                .values
                                .iter()
                                .map(render_value)
                                .collect::<crate::Result<Vec<_>>>()?,
                            signature_rrdatas: Vec::new(),
                            health_checked_targets: item
                                .data
                                .health_checked_targets
                                .as_ref()
                                .map(render_targets)
                                .transpose()?,
                            extra: Default::default(),
                        })
                    })
                    .collect::<crate::Result<Vec<_>>>()?,
                extra: Default::default(),
            },
        },
        GoogleDnsRoutingPolicyKind::PrimaryBackup {
            primary_targets,
            backup_geo_targets,
            trickle_traffic,
        } => GoogleRoutingData::PrimaryBackup {
            primary_backup: GooglePrimaryBackupPolicy {
                primary_targets: render_targets(primary_targets)?,
                backup_geo_targets: render_geo(backup_geo_targets)?,
                trickle_traffic: trickle_traffic
                    .as_str()
                    .parse()
                    .map_err(|_| validation("invalid_google_trickle_traffic"))?,
                extra: Default::default(),
            },
        },
    };
    Ok(RawRoutingPolicy {
        health_check,
        routing_data,
        extra: Default::default(),
    })
}

fn render_geo(value: &GoogleDnsGeoPolicy) -> crate::Result<GoogleGeoPolicy> {
    Ok(GoogleGeoPolicy {
        items: value
            .items
            .iter()
            .map(|item| {
                Ok(crate::GoogleGeoPolicyItem {
                    location: item.location.as_str().into(),
                    rrdatas: item
                        .data
                        .values
                        .iter()
                        .map(render_value)
                        .collect::<crate::Result<Vec<_>>>()?,
                    signature_rrdatas: Vec::new(),
                    health_checked_targets: item
                        .data
                        .health_checked_targets
                        .as_ref()
                        .map(render_targets)
                        .transpose()?,
                    extra: Default::default(),
                })
            })
            .collect::<crate::Result<Vec<_>>>()?,
        enable_fencing: value.enable_fencing,
        extra: Default::default(),
    })
}

fn render_targets(value: &GoogleDnsHealthCheckTargets) -> crate::Result<GoogleHealthCheckTargets> {
    Ok(match value {
        GoogleDnsHealthCheckTargets::ExternalEndpoints { addresses } => GoogleHealthCheckTargets { internal_load_balancers: Vec::new(), external_endpoints: addresses.iter().map(ToString::to_string).collect(), extra: Default::default() },
        GoogleDnsHealthCheckTargets::InternalLoadBalancers { targets } => GoogleHealthCheckTargets {
            external_endpoints: Vec::new(),
            internal_load_balancers: targets.iter().map(|target| Ok(crate::GoogleLoadBalancerTarget {
                load_balancer_type: match target.load_balancer_type { GoogleDnsLoadBalancerType::GlobalL7 => "globalL7ilb", GoogleDnsLoadBalancerType::RegionalL4 => "regionalL4ilb", GoogleDnsLoadBalancerType::RegionalL7 => "regionalL7ilb", GoogleDnsLoadBalancerType::None => return Err(validation("invalid_google_load_balancer_type")) }.into(),
                ip_address: target.ip_address.to_string(), port: target.port.to_string(), ip_protocol: match target.ip_protocol { GoogleDnsIpProtocol::Tcp => "tcp", GoogleDnsIpProtocol::Udp => "udp", GoogleDnsIpProtocol::Undefined => return Err(validation("invalid_google_ip_protocol")) }.into(),
                network_url: format!("https://www.googleapis.com/compute/v1/projects/{}/global/networks/{}", target.network_project_id, target.network), project: target.project_id.clone(), region: target.region.as_ref().map(|v|v.as_str().to_string()).unwrap_or_default(), extra: Default::default(),
            })).collect::<crate::Result<Vec<_>>>()?,
            extra: Default::default(),
        },
    })
}
