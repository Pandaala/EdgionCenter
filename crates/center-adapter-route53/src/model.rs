use std::{collections::BTreeSet, net::Ipv4Addr, net::Ipv6Addr};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    AbsoluteDnsName, CaaTag, DnsCharacterString, DnsOwnerName, DnsRecordExtension,
    DnsRecordRevision, DnsRecordSetKey, DnsRecordSetValue, DnsRoutingIdentity, DnsTtl, DnsTxtValue,
    DnsZoneId, DnsZoneRef, ProviderDnsRecordSet, ProviderDnsRecordType, Route53AliasTarget,
    Route53FailoverRole, Route53GeoLocation, Route53HealthCheckId, Route53RoutingPolicy,
};
use sha2::{Digest, Sha256};

use crate::{
    api::{Route53GeoLocationData, Route53RecordSet},
    normalize_zone_id, validation,
};

const ROUTE53_LATENCY_REGIONS: &[&str] = &[
    "us-east-1",
    "us-east-2",
    "us-west-1",
    "us-west-2",
    "ca-central-1",
    "eu-west-1",
    "eu-west-2",
    "eu-west-3",
    "eu-central-1",
    "eu-central-2",
    "ap-southeast-1",
    "ap-southeast-2",
    "ap-southeast-3",
    "ap-northeast-1",
    "ap-northeast-2",
    "ap-northeast-3",
    "eu-north-1",
    "sa-east-1",
    "cn-north-1",
    "cn-northwest-1",
    "ap-east-1",
    "me-south-1",
    "me-central-1",
    "ap-south-1",
    "ap-south-2",
    "af-south-1",
    "eu-south-1",
    "eu-south-2",
    "ap-southeast-4",
    "il-central-1",
    "ca-west-1",
    "ap-southeast-5",
    "mx-central-1",
    "ap-southeast-7",
    "us-gov-east-1",
    "us-gov-west-1",
    "ap-east-2",
    "ap-southeast-6",
];
const ISO_3166_ALPHA2: &str = concat!(
    "ADAEAFAGAIALAMAOAQARASATAUAWAXAZ",
    "BABBBDBEBFBGBHBIBJBLBMBNBOBQBRBSBTBVBWBYBZ",
    "CACCCDCFCGCHCICKCLCMCNCOCRCUCVCWCXCYCZ",
    "DEDJDKDMDODZ",
    "ECEEEGEHERESET",
    "FIFJFKFMFOFR",
    "GAGBGDGEGFGGGHGIGLGMGNGPGQGRGSGTGUGWGY",
    "HKHMHNHRHTHU",
    "IDIEILIMINIOIQIRISIT",
    "JEJMJOJP",
    "KEKGKHKIKMKNKPKRKWKYKZ",
    "LALBLCLILKLRLSLTLULVLY",
    "MAMCMDMEMFMGMHMKMLMMMNMOMPMQMRMSMTMUMVMWMXMYMZ",
    "NANCNENFNGNINLNONPNRNUNZ",
    "OM",
    "PAPEPFPGPHPKPLPMPNPRPSPTPWPY",
    "QA",
    "RERORSRURW",
    "SASBSCSDSESGSHSISJSKSLSMSNSOSRSSSTSVSXSYSZ",
    "TCTDTFTGTHTJTKTLTMTNTOTRTTTVTWTZ",
    "UAUGUMUSUYUZ",
    "VAVCVEVGVIVNVU",
    "WFWS",
    "YEYT",
    "ZAZMZW"
);
const ROUTE53_US_SUBDIVISIONS: &str = concat!(
    "AKALAZARCACOCTDEFLGAHIIDILINIAKSKYLAMEMDMAMIMNMSMOMT",
    "NENVNHNJNMNYNCNDOHOKORPARISCSDTNTXUTVTVAWAWVWIWY",
    "ASDCFMGUMHMPRPVIAAAEAP"
);

pub(crate) fn map_record_set(
    zone: &DnsZoneRef,
    record: Route53RecordSet,
) -> crate::Result<(ProviderDnsRecordSet, DnsRecordRevision)> {
    if record.traffic_policy_instance_id.is_some() {
        return Err(validation("route53_traffic_policy_record_unsupported"));
    }
    if record.has_cidr_routing_config {
        return Err(validation("route53_cidr_routing_unsupported"));
    }
    if record.has_geoproximity_location {
        return Err(validation("route53_geoproximity_routing_unsupported"));
    }

    let record_type = map_record_type(&record.record_type)?;
    let owner = DnsOwnerName::new(decode_domain_presentation(&record.name)?)
        .map_err(|_| validation("invalid_route53_record_name"))?;
    let (routing, routing_policy) = map_routing(&record)?;
    let alias_target = record
        .alias_target
        .as_ref()
        .map(|target| {
            Ok(Route53AliasTarget {
                target_zone_id: DnsZoneId::new(normalize_zone_id(&target.hosted_zone_id)?)
                    .map_err(|_| validation("invalid_route53_alias_zone_id"))?,
                target: AbsoluteDnsName::new(decode_domain_presentation(&target.dns_name)?)
                    .map_err(|_| validation("invalid_route53_alias_target"))?,
                evaluate_target_health: target.evaluate_target_health,
            })
        })
        .transpose()?;
    let health_check_id = record
        .health_check_id
        .as_ref()
        .map(|value| {
            Route53HealthCheckId::new(value.clone())
                .map_err(|_| validation("invalid_route53_health_check_id"))
        })
        .transpose()?;
    let extension =
        if alias_target.is_some() || routing_policy.is_some() || health_check_id.is_some() {
            Some(DnsRecordExtension::Route53 {
                alias_target,
                routing_policy,
                health_check_id,
            })
        } else {
            None
        };
    let (ttl, values) = if record.alias_target.is_some() {
        if record.ttl.is_some() || !record.resource_records.is_empty() {
            return Err(validation("invalid_route53_alias_shape"));
        }
        (DnsTtl::Inherited, BTreeSet::new())
    } else {
        let ttl = record
            .ttl
            .ok_or_else(|| validation("missing_route53_record_ttl"))?;
        if record.resource_records.is_empty() {
            return Err(validation("missing_route53_record_values"));
        }
        let values = record
            .resource_records
            .iter()
            .map(|value| parse_value(record_type, value))
            .collect::<Result<BTreeSet<_>, _>>()?;
        (DnsTtl::Seconds(ttl), values)
    };
    let mapped = ProviderDnsRecordSet {
        key: DnsRecordSetKey {
            owner,
            record_type,
            routing,
        },
        ttl,
        values,
        extension,
    };
    mapped
        .validate(zone)
        .map_err(|_| validation("invalid_route53_record_set"))?;
    let revision = canonical_revision(&mapped)?;
    Ok((mapped, revision))
}

pub(crate) fn canonical_revision(
    record_set: &ProviderDnsRecordSet,
) -> crate::Result<DnsRecordRevision> {
    let canonical = serde_json::to_vec(record_set)
        .map_err(|_| validation("route53_revision_encoding_failed"))?;
    DnsRecordRevision::new(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
    .map_err(|_| validation("invalid_route53_record_revision"))
}

pub(crate) fn render_record_set(
    zone: &DnsZoneRef,
    record_set: &ProviderDnsRecordSet,
) -> crate::Result<Route53RecordSet> {
    record_set
        .validate(zone)
        .map_err(|_| validation("invalid_route53_record_set"))?;
    let mut rendered = Route53RecordSet {
        name: record_set.key.owner.fqdn(),
        record_type: render_record_type(record_set.key.record_type)?.to_string(),
        ttl: None,
        resource_records: Vec::new(),
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
    };
    match &record_set.extension {
        Some(DnsRecordExtension::Route53 {
            alias_target,
            routing_policy,
            health_check_id,
        }) => {
            rendered.alias_target =
                alias_target
                    .as_ref()
                    .map(|target| crate::Route53AliasTargetData {
                        hosted_zone_id: target.target_zone_id.as_str().to_string(),
                        dns_name: target.target.fqdn(),
                        evaluate_target_health: target.evaluate_target_health,
                    });
            rendered.health_check_id = health_check_id
                .as_ref()
                .map(|value| value.as_str().to_string());
            if let DnsRoutingIdentity::Route53 { set_identifier } = &record_set.key.routing {
                rendered.set_identifier = Some(set_identifier.clone());
            }
            match routing_policy {
                Some(Route53RoutingPolicy::Weighted { weight }) => rendered.weight = Some(*weight),
                Some(Route53RoutingPolicy::Failover { role }) => {
                    rendered.failover = Some(
                        match role {
                            Route53FailoverRole::Primary => "PRIMARY",
                            Route53FailoverRole::Secondary => "SECONDARY",
                        }
                        .to_string(),
                    );
                }
                Some(Route53RoutingPolicy::Latency { region }) => {
                    rendered.region = Some(region.clone());
                }
                Some(Route53RoutingPolicy::Geolocation { location }) => {
                    rendered.geolocation = Some(render_geolocation(location));
                }
                Some(Route53RoutingPolicy::Multivalue) => rendered.multivalue_answer = Some(true),
                None => {}
            }
        }
        None => {}
        Some(_) => return Err(validation("invalid_route53_record_extension")),
    }
    if rendered.alias_target.is_some() {
        if record_set.ttl != DnsTtl::Inherited || !record_set.values.is_empty() {
            return Err(validation("invalid_route53_alias_shape"));
        }
    } else {
        rendered.ttl = match record_set.ttl {
            DnsTtl::Seconds(value) => Some(value),
            DnsTtl::Automatic | DnsTtl::Inherited => {
                return Err(validation("invalid_route53_record_ttl"));
            }
        };
        rendered.resource_records = record_set
            .values
            .iter()
            .map(render_value)
            .collect::<crate::Result<Vec<_>>>()?;
    }
    let (round_trip, _) = map_record_set(zone, rendered.clone())?;
    if round_trip != *record_set {
        return Err(validation("route53_record_render_not_lossless"));
    }
    Ok(rendered)
}

fn render_record_type(value: ProviderDnsRecordType) -> crate::Result<&'static str> {
    match value {
        ProviderDnsRecordType::A => Ok("A"),
        ProviderDnsRecordType::Aaaa => Ok("AAAA"),
        ProviderDnsRecordType::Cname => Ok("CNAME"),
        ProviderDnsRecordType::Txt => Ok("TXT"),
        ProviderDnsRecordType::Mx => Ok("MX"),
        ProviderDnsRecordType::Srv => Ok("SRV"),
        ProviderDnsRecordType::Caa => Ok("CAA"),
        ProviderDnsRecordType::Ns => Ok("NS"),
        ProviderDnsRecordType::Soa => Ok("SOA"),
        ProviderDnsRecordType::GoogleAlias => Err(validation("unsupported_route53_record_type")),
    }
}

fn render_geolocation(value: &Route53GeoLocation) -> Route53GeoLocationData {
    match value {
        Route53GeoLocation::Default => Route53GeoLocationData {
            continent_code: None,
            country_code: Some("*".to_string()),
            subdivision_code: None,
        },
        Route53GeoLocation::Continent { code } => Route53GeoLocationData {
            continent_code: Some(code.clone()),
            country_code: None,
            subdivision_code: None,
        },
        Route53GeoLocation::Country { code } => Route53GeoLocationData {
            continent_code: None,
            country_code: Some(code.clone()),
            subdivision_code: None,
        },
        Route53GeoLocation::UsSubdivision { code } => Route53GeoLocationData {
            continent_code: None,
            country_code: Some("US".to_string()),
            subdivision_code: Some(code.clone()),
        },
    }
}

fn render_value(value: &DnsRecordSetValue) -> crate::Result<String> {
    Ok(match value {
        DnsRecordSetValue::A { address } => address.to_string(),
        DnsRecordSetValue::Aaaa { address } => address.to_string(),
        DnsRecordSetValue::Cname { target } | DnsRecordSetValue::Ns { target } => target.fqdn(),
        DnsRecordSetValue::Txt { value } => value
            .segments()
            .iter()
            .map(|segment| render_character_string(segment.as_bytes()))
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
            format!(
                "{flags} {} {}",
                tag.as_str(),
                render_character_string(value.as_bytes())
            )
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

fn render_character_string(value: &[u8]) -> String {
    let mut rendered = String::from("\"");
    for byte in value {
        match *byte {
            b'\"' | b'\\' => {
                rendered.push('\\');
                rendered.push(char::from(*byte));
            }
            0x20..=0x7e => rendered.push(char::from(*byte)),
            value => rendered.push_str(&format!("\\{value:03o}")),
        }
    }
    rendered.push('"');
    rendered
}

fn map_record_type(value: &str) -> crate::Result<ProviderDnsRecordType> {
    match value {
        "A" => Ok(ProviderDnsRecordType::A),
        "AAAA" => Ok(ProviderDnsRecordType::Aaaa),
        "CNAME" => Ok(ProviderDnsRecordType::Cname),
        "TXT" => Ok(ProviderDnsRecordType::Txt),
        "MX" => Ok(ProviderDnsRecordType::Mx),
        "SRV" => Ok(ProviderDnsRecordType::Srv),
        "CAA" => Ok(ProviderDnsRecordType::Caa),
        "NS" => Ok(ProviderDnsRecordType::Ns),
        "SOA" => Ok(ProviderDnsRecordType::Soa),
        _ => Err(validation("unsupported_route53_record_type")),
    }
}

fn map_routing(
    record: &Route53RecordSet,
) -> crate::Result<(DnsRoutingIdentity, Option<Route53RoutingPolicy>)> {
    let mut policies = Vec::new();
    if let Some(weight) = record.weight {
        policies.push(Route53RoutingPolicy::Weighted { weight });
    }
    if let Some(failover) = record.failover.as_deref() {
        let role = match failover {
            "PRIMARY" => Route53FailoverRole::Primary,
            "SECONDARY" => Route53FailoverRole::Secondary,
            _ => return Err(validation("unsupported_route53_failover_role")),
        };
        policies.push(Route53RoutingPolicy::Failover { role });
    }
    if let Some(region) = record.region.as_ref() {
        if !ROUTE53_LATENCY_REGIONS.contains(&region.as_str()) {
            return Err(validation("unsupported_route53_latency_region"));
        }
        policies.push(Route53RoutingPolicy::Latency {
            region: region.clone(),
        });
    }
    if let Some(location) = record.geolocation.as_ref() {
        policies.push(Route53RoutingPolicy::Geolocation {
            location: map_geolocation(location)?,
        });
    }
    if record.multivalue_answer == Some(true) {
        policies.push(Route53RoutingPolicy::Multivalue);
    }
    if policies.len() > 1 {
        return Err(validation("multiple_route53_routing_policies"));
    }
    let policy = policies.pop();
    match (&record.set_identifier, &policy) {
        (None, None) => Ok((DnsRoutingIdentity::Simple, None)),
        (Some(identifier), Some(_)) => Ok((
            DnsRoutingIdentity::Route53 {
                set_identifier: identifier.clone(),
            },
            policy,
        )),
        _ => Err(validation("invalid_route53_routing_identity")),
    }
}

fn map_geolocation(value: &Route53GeoLocationData) -> crate::Result<Route53GeoLocation> {
    match (
        value.continent_code.as_deref(),
        value.country_code.as_deref(),
        value.subdivision_code.as_deref(),
    ) {
        (Some(code), None, None)
            if matches!(code, "AF" | "AN" | "AS" | "EU" | "OC" | "NA" | "SA") =>
        {
            Ok(Route53GeoLocation::Continent {
                code: code.to_string(),
            })
        }
        (None, Some("*"), None) => Ok(Route53GeoLocation::Default),
        (None, Some("US"), Some(code)) if catalog_contains(ROUTE53_US_SUBDIVISIONS, code) => {
            Ok(Route53GeoLocation::UsSubdivision {
                code: code.to_string(),
            })
        }
        (None, Some(code), None) if catalog_contains(ISO_3166_ALPHA2, code) => {
            Ok(Route53GeoLocation::Country {
                code: code.to_string(),
            })
        }
        _ => Err(validation("invalid_route53_geolocation")),
    }
}

fn catalog_contains(catalog: &str, code: &str) -> bool {
    code.len() == 2
        && catalog
            .as_bytes()
            .chunks_exact(2)
            .any(|candidate| candidate == code.as_bytes())
}

fn parse_value(
    record_type: ProviderDnsRecordType,
    value: &str,
) -> crate::Result<DnsRecordSetValue> {
    match record_type {
        ProviderDnsRecordType::A => Ok(DnsRecordSetValue::A {
            address: value
                .parse::<Ipv4Addr>()
                .map_err(|_| validation("invalid_route53_a_value"))?,
        }),
        ProviderDnsRecordType::Aaaa => Ok(DnsRecordSetValue::Aaaa {
            address: value
                .parse::<Ipv6Addr>()
                .map_err(|_| validation("invalid_route53_aaaa_value"))?,
        }),
        ProviderDnsRecordType::Cname => Ok(DnsRecordSetValue::Cname {
            target: absolute_name(value, "invalid_route53_cname_value")?,
        }),
        ProviderDnsRecordType::Txt => Ok(DnsRecordSetValue::Txt {
            value: parse_character_strings(value)?,
        }),
        ProviderDnsRecordType::Mx => {
            let fields = fields(value, 2, "invalid_route53_mx_value")?;
            Ok(DnsRecordSetValue::Mx {
                preference: fields[0]
                    .parse()
                    .map_err(|_| validation("invalid_route53_mx_value"))?,
                exchange: absolute_name(fields[1], "invalid_route53_mx_value")?,
            })
        }
        ProviderDnsRecordType::Srv => {
            let fields = fields(value, 4, "invalid_route53_srv_value")?;
            Ok(DnsRecordSetValue::Srv {
                priority: parse_u16(fields[0], "invalid_route53_srv_value")?,
                weight: parse_u16(fields[1], "invalid_route53_srv_value")?,
                port: parse_u16(fields[2], "invalid_route53_srv_value")?,
                target: absolute_name(fields[3], "invalid_route53_srv_value")?,
            })
        }
        ProviderDnsRecordType::Caa => parse_caa(value),
        ProviderDnsRecordType::Ns => Ok(DnsRecordSetValue::Ns {
            target: absolute_name(value, "invalid_route53_ns_value")?,
        }),
        ProviderDnsRecordType::Soa => {
            let fields = fields(value, 7, "invalid_route53_soa_value")?;
            Ok(DnsRecordSetValue::Soa {
                primary_name_server: absolute_name(fields[0], "invalid_route53_soa_value")?,
                responsible_mailbox: absolute_name(fields[1], "invalid_route53_soa_value")?,
                serial: parse_u32(fields[2], "invalid_route53_soa_value")?,
                refresh: parse_u32(fields[3], "invalid_route53_soa_value")?,
                retry: parse_u32(fields[4], "invalid_route53_soa_value")?,
                expire: parse_u32(fields[5], "invalid_route53_soa_value")?,
                minimum: parse_u32(fields[6], "invalid_route53_soa_value")?,
            })
        }
        ProviderDnsRecordType::GoogleAlias => Err(validation("unsupported_route53_record_type")),
    }
}

fn parse_caa(value: &str) -> crate::Result<DnsRecordSetValue> {
    let (flags, remaining) =
        take_presentation_field(value).ok_or_else(|| validation("invalid_route53_caa_value"))?;
    let (tag, presentation) = take_presentation_field(remaining)
        .ok_or_else(|| validation("invalid_route53_caa_value"))?;
    let flags = flags
        .parse::<u8>()
        .map_err(|_| validation("invalid_route53_caa_value"))?;
    let tag = CaaTag::new(tag).map_err(|_| validation("invalid_route53_caa_value"))?;
    let strings = parse_character_strings(presentation)?;
    if strings.segments().len() != 1 {
        return Err(validation("invalid_route53_caa_value"));
    }
    Ok(DnsRecordSetValue::Caa {
        flags,
        tag,
        value: strings.segments()[0].clone(),
    })
}

fn take_presentation_field(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start_matches(char::is_whitespace);
    let boundary = value.find(char::is_whitespace)?;
    let field = &value[..boundary];
    if field.is_empty() {
        return None;
    }
    Some((
        field,
        value[boundary..].trim_start_matches(char::is_whitespace),
    ))
}

fn fields<'a>(value: &'a str, count: usize, code: &str) -> crate::Result<Vec<&'a str>> {
    let fields = value.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != count {
        return Err(validation(code));
    }
    Ok(fields)
}

fn absolute_name(value: &str, code: &str) -> crate::Result<AbsoluteDnsName> {
    AbsoluteDnsName::new(decode_domain_presentation(value)?).map_err(|_| validation(code))
}

pub(crate) fn decode_domain_presentation(value: &str) -> crate::Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'\\' {
            decoded.push(bytes[cursor]);
            cursor += 1;
            continue;
        }
        cursor += 1;
        let octet = parse_octal_escape(bytes, cursor)
            .ok_or_else(|| validation("invalid_route53_domain_escape"))?;
        decoded.push(octet);
        cursor += 3;
    }
    String::from_utf8(decoded).map_err(|_| validation("unsupported_route53_domain_encoding"))
}

fn parse_u16(value: &str, code: &str) -> crate::Result<u16> {
    value.parse().map_err(|_| validation(code))
}

fn parse_u32(value: &str, code: &str) -> crate::Result<u32> {
    value.parse().map_err(|_| validation(code))
}

fn parse_character_strings(value: &str) -> crate::Result<DnsTxtValue> {
    let bytes = value.as_bytes();
    let mut cursor = 0;
    let mut segments = Vec::new();
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        if bytes[cursor] != b'"' {
            return Err(validation("invalid_route53_character_string"));
        }
        cursor += 1;
        let mut segment = Vec::new();
        let mut closed = false;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'"' => {
                    cursor += 1;
                    closed = true;
                    break;
                }
                b'\\' => {
                    cursor += 1;
                    if cursor == bytes.len() {
                        return Err(validation("invalid_route53_character_string"));
                    }
                    if bytes[cursor].is_ascii_digit() {
                        segment.push(
                            parse_octal_escape(bytes, cursor)
                                .ok_or_else(|| validation("invalid_route53_character_string"))?,
                        );
                        cursor += 3;
                    } else {
                        segment.push(bytes[cursor]);
                        cursor += 1;
                    }
                }
                byte => {
                    segment.push(byte);
                    cursor += 1;
                }
            }
        }
        if !closed {
            return Err(validation("invalid_route53_character_string"));
        }
        segments.push(
            DnsCharacterString::new(segment)
                .map_err(|_| validation("invalid_route53_character_string"))?,
        );
    }
    if segments.is_empty() {
        return Err(validation("invalid_route53_character_string"));
    }
    DnsTxtValue::new(segments).map_err(|_| validation("invalid_route53_character_string"))
}

fn parse_octal_escape(bytes: &[u8], cursor: usize) -> Option<u8> {
    let digits = bytes.get(cursor..cursor.checked_add(3)?)?;
    if digits.iter().any(|digit| !(b'0'..=b'7').contains(digit)) {
        return None;
    }
    let value = u16::from(digits[0] - b'0') * 64
        + u16::from(digits[1] - b'0') * 8
        + u16::from(digits[2] - b'0');
    u8::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_strings_preserve_segments_and_binary_escapes() {
        let value =
            parse_character_strings(r#""hello" "a\040b" "\000" "\177" "\255" "\344" "\377""#)
                .unwrap();
        assert_eq!(value.segments().len(), 7);
        assert_eq!(value.segments()[0].as_bytes(), b"hello");
        assert_eq!(value.segments()[1].as_bytes(), b"a b");
        assert_eq!(value.segments()[2].as_bytes(), &[0o000]);
        assert_eq!(value.segments()[3].as_bytes(), &[0o177]);
        assert_eq!(value.segments()[4].as_bytes(), &[0o255]);
        assert_eq!(value.segments()[5].as_bytes(), &[0o344]);
        assert_eq!(value.segments()[6].as_bytes(), &[0o377]);
    }

    #[test]
    fn malformed_character_strings_fail_closed() {
        for value in [
            "plain",
            r#""unterminated"#,
            r#""bad\178""#,
            r#""bad\400""#,
            r#""bad\999""#,
            "",
        ] {
            assert!(
                parse_character_strings(value).is_err(),
                "accepted {value:?}"
            );
        }
    }

    #[test]
    fn caa_parser_accepts_presentation_whitespace() {
        let value = parse_caa("  0   issue  \"letsencrypt.org\"").expect("valid CAA");
        assert!(matches!(value, DnsRecordSetValue::Caa { flags: 0, .. }));
    }

    #[test]
    fn domain_presentation_decodes_wildcard_and_rejects_bad_escapes() {
        assert_eq!(
            decode_domain_presentation(r"\052.example.test.").unwrap(),
            "*.example.test."
        );
        for value in [r"\05.example.test.", r"\999.example.test.", r"bad\"] {
            assert!(decode_domain_presentation(value).is_err());
        }
    }

    #[test]
    fn embedded_route53_catalogs_are_well_formed() {
        assert_eq!(ISO_3166_ALPHA2.len(), 249 * 2);
        assert_eq!(ROUTE53_US_SUBDIVISIONS.len() % 2, 0);
        assert!(catalog_contains(ISO_3166_ALPHA2, "DE"));
        assert!(catalog_contains(ROUTE53_US_SUBDIVISIONS, "CA"));
        assert!(!catalog_contains(ISO_3166_ALPHA2, "ZZ"));
    }
}
