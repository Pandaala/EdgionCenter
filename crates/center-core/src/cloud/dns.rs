//! Provider-neutral DNS adapter contract.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Display,
    net::{Ipv4Addr, Ipv6Addr},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{CloudProvider, CloudResourceId, NormalizedProviderError, ZoneVisibility};
use crate::{CoreError, CoreResult};

const MAX_OPAQUE_ID_LEN: usize = 512;
const MAX_PAGE_TOKEN_LEN: usize = 4096;
const MAX_PAGE_SIZE: u16 = 1000;
const MAX_CHANGES: usize = 1000;
const MAX_TXT_SEGMENTS: usize = 64;
const MAX_TXT_TOTAL_BYTES: usize = 4096;
const MAX_TTL_SECONDS: u32 = i32::MAX as u32;

/// Provider failures cross this boundary only after CLD-07 sanitization and
/// classification. Raw SDK errors, bodies, and headers are not accepted.
pub type DnsProviderResult<T> = Result<T, NormalizedProviderError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProviderDnsRecordType {
    A,
    Aaaa,
    Cname,
    Txt,
    Mx,
    Srv,
    Caa,
    Ns,
    Soa,
}

macro_rules! opaque_id {
    ($name:ident, $kind:literal, $max:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> CoreResult<Self> {
                let value = value.into();
                validate_text(&value, $kind, $max)?;
                Ok(Self(value))
            }

            pub fn validate(&self) -> CoreResult<()> {
                Self::new(self.0.clone()).map(|_| ())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = CoreError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

opaque_id!(DnsZoneId, "DNS zone ID", MAX_OPAQUE_ID_LEN);
opaque_id!(DnsRecordObjectId, "DNS record object ID", MAX_OPAQUE_ID_LEN);
opaque_id!(DnsRecordRevision, "DNS record revision", MAX_OPAQUE_ID_LEN);
opaque_id!(DnsChangeId, "DNS change ID", MAX_OPAQUE_ID_LEN);
opaque_id!(DnsPageToken, "DNS page token", MAX_PAGE_TOKEN_LEN);
opaque_id!(Route53HealthCheckId, "Route 53 health check ID", 64);

/// Canonical lowercase ASCII A-label name without a trailing dot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AbsoluteDnsName(String);

impl AbsoluteDnsName {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        normalize_name(&value.into(), false, false).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn fqdn(&self) -> String {
        format!("{}.", self.0)
    }
}

impl TryFrom<String> for AbsoluteDnsName {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<AbsoluteDnsName> for String {
    fn from(value: AbsoluteDnsName) -> Self {
        value.0
    }
}

impl Display for AbsoluteDnsName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Canonical DNS owner name. It additionally permits underscore labels and a
/// wildcard only as the complete first label.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DnsOwnerName(String);

impl DnsOwnerName {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        normalize_name(&value.into(), true, true).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn fqdn(&self) -> String {
        format!("{}.", self.0)
    }

    pub fn is_within(&self, apex: &AbsoluteDnsName) -> bool {
        self.0 == apex.0 || self.0.ends_with(&format!(".{}", apex.0))
    }
}

impl TryFrom<String> for DnsOwnerName {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DnsOwnerName> for String {
    fn from(value: DnsOwnerName) -> Self {
        value.0
    }
}

impl Display for DnsOwnerName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsZoneRef {
    pub provider_account_id: CloudResourceId,
    pub provider: CloudProvider,
    pub zone_id: DnsZoneId,
    pub apex: AbsoluteDnsName,
    pub visibility: ZoneVisibility,
}

impl DnsZoneRef {
    pub fn validate(&self) -> CoreResult<()> {
        self.provider_account_id.validate()?;
        self.zone_id.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedDnsZone {
    pub zone: DnsZoneRef,
    pub revision: Option<DnsRecordRevision>,
}

impl ObservedDnsZone {
    pub fn validate(&self) -> CoreResult<()> {
        self.zone.validate()?;
        if let Some(revision) = self.revision.as_ref() {
            revision.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
pub struct DnsCharacterString(Vec<u8>);

impl DnsCharacterString {
    pub fn new(value: impl Into<Vec<u8>>) -> CoreResult<Self> {
        let value = value.into();
        if value.len() > 255 {
            return Err(CoreError::Conflict(
                "DNS character-string cannot exceed 255 octets".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn validate(&self) -> CoreResult<()> {
        Self::new(self.0.clone()).map(|_| ())
    }
}

impl TryFrom<Vec<u8>> for DnsCharacterString {
    type Error = CoreError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DnsCharacterString> for Vec<u8> {
    fn from(value: DnsCharacterString) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec<DnsCharacterString>", into = "Vec<DnsCharacterString>")]
pub struct DnsTxtValue(Vec<DnsCharacterString>);

impl DnsTxtValue {
    pub fn new(segments: Vec<DnsCharacterString>) -> CoreResult<Self> {
        let total = segments
            .iter()
            .map(|segment| segment.0.len())
            .sum::<usize>();
        if segments.is_empty() || segments.len() > MAX_TXT_SEGMENTS || total > MAX_TXT_TOTAL_BYTES {
            return Err(CoreError::Conflict(
                "DNS TXT value is too large".to_string(),
            ));
        }
        Ok(Self(segments))
    }

    pub fn segments(&self) -> &[DnsCharacterString] {
        &self.0
    }

    pub fn validate(&self) -> CoreResult<()> {
        for segment in &self.0 {
            segment.validate()?;
        }
        Self::new(self.0.clone()).map(|_| ())
    }
}

impl TryFrom<Vec<DnsCharacterString>> for DnsTxtValue {
    type Error = CoreError;

    fn try_from(value: Vec<DnsCharacterString>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DnsTxtValue> for Vec<DnsCharacterString> {
    fn from(value: DnsTxtValue) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CaaTag(String);

impl CaaTag {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into().to_ascii_lowercase();
        if value.is_empty()
            || value.len() > 15
            || !value
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric())
            || !value
                .chars()
                .last()
                .is_some_and(|character| character.is_ascii_alphanumeric())
            || !value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err(CoreError::Conflict("CAA tag is invalid".to_string()));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn validate(&self) -> CoreResult<()> {
        if Self::new(self.0.clone())? != *self {
            return Err(CoreError::Conflict("CAA tag is not canonical".to_string()));
        }
        Ok(())
    }
}

impl TryFrom<String> for CaaTag {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CaaTag> for String {
    fn from(value: CaaTag) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "UPPERCASE")]
pub enum DnsRecordSetValue {
    A {
        address: Ipv4Addr,
    },
    Aaaa {
        address: Ipv6Addr,
    },
    Cname {
        target: AbsoluteDnsName,
    },
    Txt {
        value: DnsTxtValue,
    },
    Mx {
        preference: u16,
        exchange: AbsoluteDnsName,
    },
    Srv {
        priority: u16,
        weight: u16,
        port: u16,
        target: AbsoluteDnsName,
    },
    Caa {
        flags: u8,
        tag: CaaTag,
        value: DnsCharacterString,
    },
    Ns {
        target: AbsoluteDnsName,
    },
    Soa {
        primary_name_server: AbsoluteDnsName,
        responsible_mailbox: AbsoluteDnsName,
        serial: u32,
        refresh: u32,
        retry: u32,
        expire: u32,
        minimum: u32,
    },
}

impl DnsRecordSetValue {
    pub fn record_type(&self) -> ProviderDnsRecordType {
        match self {
            Self::A { .. } => ProviderDnsRecordType::A,
            Self::Aaaa { .. } => ProviderDnsRecordType::Aaaa,
            Self::Cname { .. } => ProviderDnsRecordType::Cname,
            Self::Txt { .. } => ProviderDnsRecordType::Txt,
            Self::Mx { .. } => ProviderDnsRecordType::Mx,
            Self::Srv { .. } => ProviderDnsRecordType::Srv,
            Self::Caa { .. } => ProviderDnsRecordType::Caa,
            Self::Ns { .. } => ProviderDnsRecordType::Ns,
            Self::Soa { .. } => ProviderDnsRecordType::Soa,
        }
    }

    fn validate(&self) -> CoreResult<()> {
        match self {
            Self::Txt { value } => value.validate(),
            Self::Caa { tag, value, .. } => {
                tag.validate()?;
                value.validate()
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DnsRoutingIdentity {
    #[default]
    Simple,
    Route53 {
        set_identifier: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRecordSetKey {
    pub owner: DnsOwnerName,
    pub record_type: ProviderDnsRecordType,
    #[serde(default)]
    pub routing: DnsRoutingIdentity,
}

impl DnsRecordSetKey {
    pub fn validate(&self) -> CoreResult<()> {
        if let DnsRoutingIdentity::Route53 { set_identifier } = &self.routing {
            validate_text(set_identifier, "Route 53 set identifier", 128)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "seconds", rename_all = "snake_case")]
pub enum DnsTtl {
    Automatic,
    Inherited,
    Seconds(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareProxyOptions {
    DnsOnly,
    Proxied,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareCnameFlattening {
    #[default]
    ProviderDefault,
    Flatten,
    DoNotFlatten,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53AliasTarget {
    pub target_zone_id: DnsZoneId,
    pub target: AbsoluteDnsName,
    pub evaluate_target_health: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route53FailoverRole {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Route53GeoLocation {
    Default,
    Continent { code: String },
    Country { code: String },
    UsSubdivision { code: String },
}

impl Route53GeoLocation {
    fn validate(&self) -> CoreResult<()> {
        match self {
            Self::Default => Ok(()),
            Self::Continent { code }
                if matches!(
                    code.as_str(),
                    "AF" | "AN" | "AS" | "EU" | "OC" | "NA" | "SA"
                ) =>
            {
                Ok(())
            }
            Self::Country { code } | Self::UsSubdivision { code }
                if code.len() == 2 && code.bytes().all(|value| value.is_ascii_uppercase()) =>
            {
                Ok(())
            }
            _ => Err(CoreError::Conflict(
                "Route 53 geolocation selector is invalid".to_string(),
            )),
        }
    }

    fn identity(&self) -> String {
        match self {
            Self::Default => "default".to_string(),
            Self::Continent { code } => format!("continent:{code}"),
            Self::Country { code } => format!("country:{code}"),
            Self::UsSubdivision { code } => format!("us-subdivision:{code}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Route53RoutingPolicy {
    Weighted { weight: u8 },
    Failover { role: Route53FailoverRole },
    Latency { region: String },
    Geolocation { location: Route53GeoLocation },
    Multivalue,
}

impl Route53RoutingPolicy {
    fn validate(&self) -> CoreResult<()> {
        match self {
            Self::Latency { region } => validate_text(region, "Route 53 latency region", 64),
            Self::Geolocation { location } => location.validate(),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum DnsRecordExtension {
    Cloudflare {
        /// Present only for A, AAAA, and CNAME. Keeping this optional avoids
        /// inventing proxy semantics for TXT, MX, SRV, CAA, and NS records.
        proxy: Option<CloudflareProxyOptions>,
        #[serde(default)]
        cname_flattening: CloudflareCnameFlattening,
        /// Common metadata for every provider object aggregated into this
        /// canonical RRset. Adapters reject heterogeneous member metadata.
        comment: Option<String>,
        #[serde(default)]
        tags: BTreeSet<String>,
    },
    Route53 {
        alias_target: Option<Route53AliasTarget>,
        routing_policy: Option<Route53RoutingPolicy>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        health_check_id: Option<Route53HealthCheckId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDnsRecordSet {
    pub key: DnsRecordSetKey,
    pub ttl: DnsTtl,
    pub values: BTreeSet<DnsRecordSetValue>,
    pub extension: Option<DnsRecordExtension>,
}

impl ProviderDnsRecordSet {
    pub fn validate(&self, zone: &DnsZoneRef) -> CoreResult<()> {
        zone.validate()?;
        self.key.validate()?;
        if !self.key.owner.is_within(&zone.apex) {
            return Err(CoreError::Conflict(
                "DNS record owner is outside the managed zone".to_string(),
            ));
        }
        if let DnsTtl::Seconds(seconds) = self.ttl {
            if seconds > MAX_TTL_SECONDS {
                return Err(CoreError::Conflict("DNS TTL is invalid".to_string()));
            }
        }
        if zone.provider == CloudProvider::Aws
            && ((self.key.record_type == ProviderDnsRecordType::Ns
                && self.key.owner.as_str().starts_with("*."))
                || (self.key.record_type == ProviderDnsRecordType::Cname
                    && self.key.owner.as_str() == zone.apex.as_str()))
        {
            return Err(CoreError::Conflict(
                "Route 53 wildcard NS and zone-apex CNAME records are unsupported".to_string(),
            ));
        }
        let route53_alias = matches!(
            self.extension,
            Some(DnsRecordExtension::Route53 {
                alias_target: Some(_),
                ..
            })
        );
        if route53_alias {
            if !self.values.is_empty()
                || self.ttl != DnsTtl::Inherited
                || !matches!(
                    self.key.record_type,
                    ProviderDnsRecordType::A
                        | ProviderDnsRecordType::Aaaa
                        | ProviderDnsRecordType::Cname
                        | ProviderDnsRecordType::Txt
                        | ProviderDnsRecordType::Mx
                        | ProviderDnsRecordType::Srv
                        | ProviderDnsRecordType::Caa
                )
            {
                return Err(CoreError::Conflict(
                    "provider alias values, type, or TTL are invalid".to_string(),
                ));
            }
        } else {
            if self.values.is_empty() || self.ttl == DnsTtl::Inherited {
                return Err(CoreError::Conflict(
                    "ordinary DNS record sets require values and a TTL".to_string(),
                ));
            }
            if self
                .values
                .iter()
                .any(|value| value.record_type() != self.key.record_type)
            {
                return Err(CoreError::Conflict(
                    "DNS record value does not match its record type".to_string(),
                ));
            }
            for value in &self.values {
                value.validate()?;
            }
            if self.key.record_type == ProviderDnsRecordType::Cname && self.values.len() != 1 {
                return Err(CoreError::Conflict(
                    "CNAME record sets require exactly one value".to_string(),
                ));
            }
            if self.key.record_type == ProviderDnsRecordType::Soa && self.values.len() != 1 {
                return Err(CoreError::Conflict(
                    "SOA record sets require exactly one value".to_string(),
                ));
            }
        }
        match &self.extension {
            Some(DnsRecordExtension::Cloudflare {
                proxy,
                cname_flattening,
                comment,
                tags,
            }) => {
                let proxy_capable_type = matches!(
                    self.key.record_type,
                    ProviderDnsRecordType::A
                        | ProviderDnsRecordType::Aaaa
                        | ProviderDnsRecordType::Cname
                );
                if zone.provider != CloudProvider::Cloudflare
                    || !matches!(self.key.routing, DnsRoutingIdentity::Simple)
                    || self.ttl == DnsTtl::Inherited
                    || proxy_capable_type != proxy.is_some()
                    || self.key.record_type == ProviderDnsRecordType::Soa
                {
                    return Err(CoreError::Conflict(
                        "Cloudflare proxy options are invalid for this record set".to_string(),
                    ));
                }
                if *proxy == Some(CloudflareProxyOptions::Proxied) && self.ttl != DnsTtl::Automatic
                {
                    return Err(CoreError::Conflict(
                        "proxied Cloudflare records require automatic TTL".to_string(),
                    ));
                }
                if let DnsTtl::Seconds(seconds) = self.ttl {
                    if !(30..=86_400).contains(&seconds) {
                        return Err(CoreError::Conflict(
                            "Cloudflare DNS TTL is outside the provider range".to_string(),
                        ));
                    }
                }
                if self.key.record_type != ProviderDnsRecordType::Cname
                    && *cname_flattening != CloudflareCnameFlattening::ProviderDefault
                {
                    return Err(CoreError::Conflict(
                        "Cloudflare CNAME flattening applies only to CNAME records".to_string(),
                    ));
                }
                if *proxy == Some(CloudflareProxyOptions::Proxied)
                    && *cname_flattening != CloudflareCnameFlattening::ProviderDefault
                {
                    return Err(CoreError::Conflict(
                        "proxied Cloudflare CNAME records cannot override flattening".to_string(),
                    ));
                }
                if self.key.record_type == ProviderDnsRecordType::Cname
                    && self.key.owner.as_str() == zone.apex.as_str()
                    && *cname_flattening == CloudflareCnameFlattening::DoNotFlatten
                {
                    return Err(CoreError::Conflict(
                        "Cloudflare zone-apex CNAME records are always flattened".to_string(),
                    ));
                }
                if comment.as_ref().is_some_and(|value| {
                    value.chars().count() > 500 || value.chars().any(char::is_control)
                }) || !valid_cloudflare_tags(tags)
                {
                    return Err(CoreError::Conflict(
                        "Cloudflare DNS record metadata is invalid".to_string(),
                    ));
                }
            }
            Some(DnsRecordExtension::Route53 {
                alias_target,
                routing_policy,
                health_check_id,
            }) => {
                if zone.provider != CloudProvider::Aws {
                    return Err(CoreError::Conflict(
                        "Route 53 extension requires an AWS provider account".to_string(),
                    ));
                }
                if let Some(target) = alias_target {
                    target.target_zone_id.validate()?;
                }
                if let Some(health_check_id) = health_check_id {
                    health_check_id.validate()?;
                }
                if let Some(policy) = routing_policy {
                    policy.validate()?;
                }
                match (&self.key.routing, routing_policy) {
                    (DnsRoutingIdentity::Simple, None)
                        if alias_target.is_some() || health_check_id.is_some() => {}
                    (DnsRoutingIdentity::Route53 { .. }, Some(_)) => {}
                    _ => {
                        return Err(CoreError::Conflict(
                            "Route 53 routing identity, policy, and alias are inconsistent"
                                .to_string(),
                        ));
                    }
                }
                if routing_policy.is_some()
                    && matches!(
                        self.key.record_type,
                        ProviderDnsRecordType::Ns | ProviderDnsRecordType::Soa
                    )
                {
                    return Err(CoreError::Conflict(
                        "Route 53 routed records cannot use NS or SOA".to_string(),
                    ));
                }
                if matches!(routing_policy, Some(Route53RoutingPolicy::Multivalue))
                    && (alias_target.is_some()
                        || matches!(
                            self.key.record_type,
                            ProviderDnsRecordType::Cname
                                | ProviderDnsRecordType::Ns
                                | ProviderDnsRecordType::Soa
                        ))
                {
                    return Err(CoreError::Conflict(
                        "Route 53 multivalue record type or alias is unsupported".to_string(),
                    ));
                }
                if matches!(
                    routing_policy,
                    Some(
                        Route53RoutingPolicy::Weighted { .. }
                            | Route53RoutingPolicy::Latency { .. }
                    )
                ) && alias_target.is_none()
                    && self.values.len() != 1
                {
                    return Err(CoreError::Conflict(
                        "Route 53 weighted and latency records require one value".to_string(),
                    ));
                }
                if matches!(routing_policy, Some(Route53RoutingPolicy::Failover { .. }))
                    && alias_target
                        .as_ref()
                        .is_some_and(|target| !target.evaluate_target_health)
                {
                    return Err(CoreError::Conflict(
                        "Route 53 failover aliases must evaluate target health".to_string(),
                    ));
                }
            }
            None => {
                if zone.provider == CloudProvider::Cloudflare
                    && matches!(
                        self.key.record_type,
                        ProviderDnsRecordType::A
                            | ProviderDnsRecordType::Aaaa
                            | ProviderDnsRecordType::Cname
                    )
                {
                    return Err(CoreError::Conflict(
                        "Cloudflare proxiable records require explicit DNS-only or proxied intent"
                            .to_string(),
                    ));
                }
                if self.ttl == DnsTtl::Automatic {
                    return Err(CoreError::Conflict(
                        "automatic DNS TTL requires an explicit provider extension".to_string(),
                    ));
                }
                if self.key.record_type == ProviderDnsRecordType::Cname
                    && self.key.owner.as_str() == zone.apex.as_str()
                {
                    return Err(CoreError::Conflict(
                        "portable CNAME records cannot target the zone apex".to_string(),
                    ));
                }
                if matches!(self.key.routing, DnsRoutingIdentity::Route53 { .. }) {
                    return Err(CoreError::Conflict(
                        "Route 53 routing identity requires a typed routing policy".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn validate_for_observed_zone(&self, zone: &ObservedDnsZone) -> CoreResult<()> {
        zone.validate()?;
        self.validate(&zone.zone)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedDnsRecordSet {
    pub zone: DnsZoneRef,
    pub record_set: ProviderDnsRecordSet,
    #[serde(default)]
    pub provider_object_ids: BTreeSet<DnsRecordObjectId>,
    pub revision: DnsRecordRevision,
}

impl ObservedDnsRecordSet {
    pub fn validate(&self) -> CoreResult<()> {
        self.record_set.validate(&self.zone)?;
        self.revision.validate()?;
        for id in &self.provider_object_ids {
            id.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DnsMutationGuard {
    MustNotExist,
    MatchObserved { revision: DnsRecordRevision },
}

impl DnsMutationGuard {
    fn validate(&self) -> CoreResult<()> {
        if let Self::MatchObserved { revision } = self {
            revision.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsGuardStrength {
    BestEffort,
    Atomic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DnsRecordChange {
    Create {
        record_set: ProviderDnsRecordSet,
        guard: DnsMutationGuard,
    },
    Replace {
        previous: ObservedDnsRecordSet,
        desired: ProviderDnsRecordSet,
        guard: DnsMutationGuard,
    },
    Delete {
        previous: ObservedDnsRecordSet,
        guard: DnsMutationGuard,
    },
}

impl DnsRecordChange {
    fn key(&self) -> &DnsRecordSetKey {
        match self {
            Self::Create { record_set, .. } => &record_set.key,
            Self::Replace { desired, .. } => &desired.key,
            Self::Delete { previous, .. } => &previous.record_set.key,
        }
    }

    fn desired_key(&self) -> Option<&DnsRecordSetKey> {
        match self {
            Self::Create { record_set, .. } => Some(&record_set.key),
            Self::Replace { desired, .. } => Some(&desired.key),
            Self::Delete { .. } => None,
        }
    }

    fn desired_record_set(&self) -> Option<&ProviderDnsRecordSet> {
        match self {
            Self::Create { record_set, .. } => Some(record_set),
            Self::Replace { desired, .. } => Some(desired),
            Self::Delete { .. } => None,
        }
    }

    fn validate(&self, zone: &DnsZoneRef) -> CoreResult<()> {
        match self {
            Self::Create { record_set, guard } => {
                record_set.validate(zone)?;
                guard.validate()?;
                if !matches!(guard, DnsMutationGuard::MustNotExist) {
                    return Err(CoreError::Conflict(
                        "DNS create requires a must-not-exist guard".to_string(),
                    ));
                }
            }
            Self::Replace {
                previous,
                desired,
                guard,
            } => {
                previous.validate()?;
                desired.validate(zone)?;
                guard.validate()?;
                if &previous.zone != zone || previous.record_set.key != desired.key {
                    return Err(CoreError::Conflict(
                        "DNS replacement must preserve zone and record identity".to_string(),
                    ));
                }
                require_exact_guard(guard, &previous.revision)?;
            }
            Self::Delete { previous, guard } => {
                previous.validate()?;
                guard.validate()?;
                if &previous.zone != zone {
                    return Err(CoreError::Conflict(
                        "DNS deletion targets a different zone".to_string(),
                    ));
                }
                require_exact_guard(guard, &previous.revision)?;
            }
        }
        Ok(())
    }
}

pub fn validate_dns_changes(zone: &DnsZoneRef, changes: &[DnsRecordChange]) -> CoreResult<()> {
    zone.validate()?;
    if changes.is_empty() || changes.len() > MAX_CHANGES {
        return Err(CoreError::Conflict(
            "DNS change batch size is invalid".to_string(),
        ));
    }
    let mut keys = BTreeSet::new();
    let mut owners_and_types = BTreeSet::new();
    let mut route53_groups = BTreeMap::new();
    for change in changes {
        change.validate(zone)?;
        let key = change.key();
        if key.owner.as_str() == zone.apex.as_str()
            && matches!(
                key.record_type,
                ProviderDnsRecordType::Ns | ProviderDnsRecordType::Soa
            )
        {
            return Err(CoreError::Conflict(
                "apex SOA and delegation NS changes require the zone lifecycle contract"
                    .to_string(),
            ));
        }
        if !keys.insert(change.key()) {
            return Err(CoreError::Conflict(
                "DNS change batch contains a duplicate record identity".to_string(),
            ));
        }
        if let Some(key) = change.desired_key() {
            if key.record_type == ProviderDnsRecordType::Cname {
                owners_and_types.insert((key.owner.clone(), true));
            } else {
                owners_and_types.insert((key.owner.clone(), false));
            }
        }
        if zone.provider == CloudProvider::Aws {
            if let Some(record_set) = change.desired_record_set() {
                validate_route53_group_member(&mut route53_groups, record_set)?;
            }
        }
    }
    for (owner, cname) in &owners_and_types {
        if *cname && owners_and_types.contains(&(owner.clone(), false)) {
            return Err(CoreError::Conflict(
                "CNAME cannot coexist with another record type in one batch".to_string(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route53RoutingFamily {
    Simple,
    Weighted,
    Failover,
    Latency,
    Geolocation,
    Multivalue,
}

struct Route53GroupState {
    family: Route53RoutingFamily,
    explicit_ttl: Option<u32>,
    unique_members: BTreeSet<String>,
    member_count: usize,
}

fn validate_route53_group_member(
    groups: &mut BTreeMap<(DnsOwnerName, ProviderDnsRecordType), Route53GroupState>,
    record_set: &ProviderDnsRecordSet,
) -> CoreResult<()> {
    let (family, unique_member) = match &record_set.extension {
        Some(DnsRecordExtension::Route53 {
            routing_policy: Some(Route53RoutingPolicy::Weighted { .. }),
            ..
        }) => (Route53RoutingFamily::Weighted, None),
        Some(DnsRecordExtension::Route53 {
            routing_policy: Some(Route53RoutingPolicy::Failover { role }),
            ..
        }) => (Route53RoutingFamily::Failover, Some(format!("{role:?}"))),
        Some(DnsRecordExtension::Route53 {
            routing_policy: Some(Route53RoutingPolicy::Latency { region }),
            ..
        }) => (Route53RoutingFamily::Latency, Some(region.clone())),
        Some(DnsRecordExtension::Route53 {
            routing_policy: Some(Route53RoutingPolicy::Geolocation { location }),
            ..
        }) => (Route53RoutingFamily::Geolocation, Some(location.identity())),
        Some(DnsRecordExtension::Route53 {
            routing_policy: Some(Route53RoutingPolicy::Multivalue),
            ..
        }) => (Route53RoutingFamily::Multivalue, None),
        _ => (Route53RoutingFamily::Simple, None),
    };
    let explicit_ttl = match record_set.ttl {
        DnsTtl::Seconds(seconds) => Some(seconds),
        DnsTtl::Automatic | DnsTtl::Inherited => None,
    };
    let state = groups
        .entry((record_set.key.owner.clone(), record_set.key.record_type))
        .or_insert_with(|| Route53GroupState {
            family,
            explicit_ttl,
            unique_members: BTreeSet::new(),
            member_count: 0,
        });
    if state.family != family {
        return Err(CoreError::Conflict(
            "Route 53 record sets cannot mix routing families".to_string(),
        ));
    }
    if let Some(explicit_ttl) = explicit_ttl {
        match state.explicit_ttl {
            Some(existing) if existing != explicit_ttl => {
                return Err(CoreError::Conflict(
                    "Edgion Route 53 routing groups require a consistent explicit TTL".to_string(),
                ));
            }
            None => state.explicit_ttl = Some(explicit_ttl),
            Some(_) => {}
        }
    }
    state.member_count += 1;
    if state.family == Route53RoutingFamily::Weighted && state.member_count > 100 {
        return Err(CoreError::Conflict(
            "Route 53 weighted routing groups cannot exceed 100 members".to_string(),
        ));
    }
    if let Some(member) = unique_member {
        if !state.unique_members.insert(member) {
            return Err(CoreError::Conflict(
                "Route 53 routing group contains a duplicate selector".to_string(),
            ));
        }
    }
    Ok(())
}

fn require_exact_guard(guard: &DnsMutationGuard, expected: &DnsRecordRevision) -> CoreResult<()> {
    match guard {
        DnsMutationGuard::MatchObserved { revision } if revision == expected => Ok(()),
        _ => Err(CoreError::Conflict(
            "DNS replacement and deletion require the exact observed revision".to_string(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsBatchAtomicity {
    AllOrNothing,
    /// Reserved for a future result shape that reports each change outcome.
    /// The current single-receipt provider port rejects this value because it
    /// cannot safely describe partial success.
    PerChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsPropagationState {
    Unknown,
    Pending,
    ProviderReportedApplied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsChangeState {
    Pending,
    ProviderCommitted,
    Failed,
    UnknownOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsChangeReceipt {
    pub id: DnsChangeId,
    pub state: DnsChangeState,
    pub submission_atomicity: DnsBatchAtomicity,
    pub propagation: DnsPropagationState,
    pub guard_strength: DnsGuardStrength,
}

impl DnsChangeReceipt {
    pub fn validate(&self) -> CoreResult<()> {
        self.id.validate()?;
        if self.submission_atomicity == DnsBatchAtomicity::PerChange {
            return Err(CoreError::Conflict(
                "per-change DNS submission requires explicit change outcomes".to_string(),
            ));
        }
        match self.state {
            DnsChangeState::Pending
                if self.propagation == DnsPropagationState::ProviderReportedApplied =>
            {
                return Err(CoreError::Conflict(
                    "pending DNS changes cannot report provider application".to_string(),
                ));
            }
            DnsChangeState::Failed | DnsChangeState::UnknownOutcome
                if self.propagation != DnsPropagationState::Unknown =>
            {
                return Err(CoreError::Conflict(
                    "failed or unknown DNS changes cannot claim propagation state".to_string(),
                ));
            }
            _ => {}
        }
        Ok(())
    }

    pub fn validate_against_request(&self, minimum_guard: DnsGuardStrength) -> CoreResult<()> {
        self.validate()?;
        if self.guard_strength < minimum_guard {
            return Err(CoreError::Conflict(
                "DNS provider returned weaker concurrency protection than requested".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsPageRequest {
    pub limit: u16,
    pub token: Option<DnsPageToken>,
}

impl DnsPageRequest {
    pub fn validate(&self) -> CoreResult<()> {
        if self.limit == 0 || self.limit > MAX_PAGE_SIZE {
            return Err(CoreError::Conflict("DNS page size is invalid".to_string()));
        }
        if let Some(token) = self.token.as_ref() {
            token.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsPage<T> {
    pub items: Vec<T>,
    pub next: Option<DnsPageToken>,
}

impl<T> DnsPage<T> {
    pub fn validate(
        &self,
        requested_limit: u16,
        mut validate_item: impl FnMut(&T) -> CoreResult<()>,
    ) -> CoreResult<()> {
        if requested_limit == 0
            || requested_limit > MAX_PAGE_SIZE
            || self.items.len() > requested_limit as usize
        {
            return Err(CoreError::Conflict(
                "DNS response page size is invalid".to_string(),
            ));
        }
        if let Some(token) = self.next.as_ref() {
            token.validate()?;
        }
        for item in &self.items {
            validate_item(item)?;
        }
        Ok(())
    }
}

/// Poll-based provider port. A provider-reported applied change is not proof
/// of authoritative DNS readiness; CLD-14 performs that verification.
///
/// Implementations validate every input. Page tokens are opaque cursors bound
/// to the exact account/zone and list method that issued them; cross-scope
/// reuse fails. Multi-page traversal has no snapshot-isolation guarantee.
/// `apply_record_changes` rejects the request before mutation when it cannot
/// meet `minimum_guard`, and the returned strength may never be weaker.
#[async_trait]
pub trait DnsProvider: Send + Sync {
    async fn get_zone(&self, zone: &DnsZoneRef) -> DnsProviderResult<Option<ObservedDnsZone>>;

    async fn list_zones(
        &self,
        provider_account_id: &CloudResourceId,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<DnsPage<ObservedDnsZone>>;

    async fn get_record_set(
        &self,
        zone: &DnsZoneRef,
        key: &DnsRecordSetKey,
    ) -> DnsProviderResult<Option<ObservedDnsRecordSet>>;

    async fn list_record_sets(
        &self,
        zone: &DnsZoneRef,
        page: &DnsPageRequest,
    ) -> DnsProviderResult<DnsPage<ObservedDnsRecordSet>>;

    async fn apply_record_changes(
        &self,
        zone: &DnsZoneRef,
        changes: &[DnsRecordChange],
        minimum_guard: DnsGuardStrength,
    ) -> DnsProviderResult<DnsChangeReceipt>;

    async fn observe_change(
        &self,
        zone: &DnsZoneRef,
        change_id: &DnsChangeId,
    ) -> DnsProviderResult<DnsChangeReceipt>;
}

fn normalize_name(value: &str, allow_underscore: bool, allow_wildcard: bool) -> CoreResult<String> {
    let value = value.strip_suffix('.').unwrap_or(value);
    if value.is_empty() || value.ends_with('.') {
        return Err(CoreError::Conflict("DNS name is invalid".to_string()));
    }
    let mut labels = Vec::new();
    for (index, label) in value.split('.').enumerate() {
        let normalized = if label == "*" && allow_wildcard && index == 0 {
            "*".to_string()
        } else if label.contains('_') {
            if !allow_underscore
                || label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                })
            {
                return Err(CoreError::Conflict(
                    "DNS owner label is invalid".to_string(),
                ));
            }
            label.to_ascii_lowercase()
        } else {
            let ascii = idna::domain_to_ascii(label)
                .map_err(|_| CoreError::Conflict("DNS IDNA label is invalid".to_string()))?
                .to_ascii_lowercase();
            if ascii.is_empty()
                || ascii.len() > 63
                || ascii.starts_with('-')
                || ascii.ends_with('-')
                || !ascii
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
            {
                return Err(CoreError::Conflict("DNS label is invalid".to_string()));
            }
            ascii
        };
        labels.push(normalized);
    }
    let normalized = labels.join(".");
    if normalized.len() > 253 {
        return Err(CoreError::Conflict("DNS name is too long".to_string()));
    }
    Ok(normalized)
}

fn validate_text(value: &str, kind: &'static str, max_len: usize) -> CoreResult<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(CoreError::Conflict(format!("{kind} is invalid")));
    }
    Ok(())
}

fn valid_cloudflare_tag(tag: &str) -> bool {
    if tag.is_empty() || tag.chars().count() > 133 || tag.chars().any(char::is_control) {
        return false;
    }
    let Some((name, value)) = tag.split_once(':') else {
        return false;
    };
    !name.is_empty()
        && name.chars().count() <= 32
        && value.chars().count() <= 100
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn valid_cloudflare_tags(tags: &BTreeSet<String>) -> bool {
    if tags.len() > 20 || tags.iter().any(|tag| !valid_cloudflare_tag(tag)) {
        return false;
    }
    let names = tags
        .iter()
        .map(|tag| {
            tag.split_once(':')
                .expect("validated Cloudflare tag")
                .0
                .to_ascii_lowercase()
        })
        .collect::<BTreeSet<_>>();
    names.len() == tags.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone_for(provider: CloudProvider) -> DnsZoneRef {
        DnsZoneRef {
            provider_account_id: CloudResourceId::new("account-a").unwrap(),
            provider,
            zone_id: DnsZoneId::new("zone-a").unwrap(),
            apex: AbsoluteDnsName::new("Example.COM.").unwrap(),
            visibility: ZoneVisibility::Public,
        }
    }

    fn zone() -> DnsZoneRef {
        zone_for(CloudProvider::Aws)
    }

    fn a_record(values: impl IntoIterator<Item = Ipv4Addr>) -> ProviderDnsRecordSet {
        ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("www.example.com.").unwrap(),
                record_type: ProviderDnsRecordType::A,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: values
                .into_iter()
                .map(|address| DnsRecordSetValue::A { address })
                .collect(),
            extension: None,
        }
    }

    fn observed(record_set: ProviderDnsRecordSet) -> ObservedDnsRecordSet {
        ObservedDnsRecordSet {
            zone: zone(),
            record_set,
            provider_object_ids: BTreeSet::new(),
            revision: DnsRecordRevision::new("revision-a").unwrap(),
        }
    }

    #[test]
    fn names_canonicalize_idna_case_and_trailing_dot() {
        assert_eq!(
            AbsoluteDnsName::new("BÜCHER.Example.").unwrap(),
            AbsoluteDnsName::new("xn--bcher-kva.example").unwrap()
        );
        assert_eq!(
            DnsOwnerName::new("_ACME-Challenge.Example.COM.")
                .unwrap()
                .as_str(),
            "_acme-challenge.example.com"
        );
        assert!(!DnsOwnerName::new("www.badexample.com")
            .unwrap()
            .is_within(&zone().apex));
        assert!(AbsoluteDnsName::new("example.com..").is_err());
    }

    #[test]
    fn typed_values_are_order_independent_and_type_safe() {
        let first = a_record(["192.0.2.2".parse().unwrap(), "192.0.2.1".parse().unwrap()]);
        let second = a_record(["192.0.2.1".parse().unwrap(), "192.0.2.2".parse().unwrap()]);
        assert_eq!(first, second);
        assert!(first.validate(&zone()).is_ok());
        let mut wrong = first;
        wrong.values = BTreeSet::from([DnsRecordSetValue::Ns {
            target: AbsoluteDnsName::new("ns.example.com").unwrap(),
        }]);
        assert!(wrong.validate(&zone()).is_err());
        assert!(DnsTxtValue::new(vec![DnsCharacterString::new(Vec::new()).unwrap()]).is_ok());
    }

    #[test]
    fn provider_alias_and_proxy_semantics_are_explicit() {
        assert!(a_record(["192.0.2.1".parse().unwrap()])
            .validate(&zone_for(CloudProvider::Cloudflare))
            .is_err());
        let mut alias = a_record([]);
        alias.key.owner = DnsOwnerName::new("example.com").unwrap();
        alias.ttl = DnsTtl::Inherited;
        alias.extension = Some(DnsRecordExtension::Route53 {
            alias_target: Some(Route53AliasTarget {
                target_zone_id: DnsZoneId::new("Z123").unwrap(),
                target: AbsoluteDnsName::new("dualstack.lb.example.net").unwrap(),
                evaluate_target_health: true,
            }),
            routing_policy: None,
            health_check_id: None,
        });
        assert!(alias.validate(&zone_for(CloudProvider::Aws)).is_ok());

        let mut same_zone_cname_alias = alias.clone();
        same_zone_cname_alias.key.owner = DnsOwnerName::new("alias.example.com").unwrap();
        same_zone_cname_alias.key.record_type = ProviderDnsRecordType::Cname;
        assert!(same_zone_cname_alias
            .validate(&zone_for(CloudProvider::Aws))
            .is_ok());

        let mut proxied = a_record(["192.0.2.1".parse().unwrap()]);
        proxied.extension = Some(DnsRecordExtension::Cloudflare {
            proxy: Some(CloudflareProxyOptions::Proxied),
            cname_flattening: CloudflareCnameFlattening::ProviderDefault,
            comment: Some("Managed by Edgion Center".to_string()),
            tags: BTreeSet::from(["owner:platform".to_string()]),
        });
        proxied.ttl = DnsTtl::Automatic;
        assert!(proxied
            .validate(&zone_for(CloudProvider::Cloudflare))
            .is_ok());
        assert_ne!(
            proxied.extension,
            Some(DnsRecordExtension::Cloudflare {
                proxy: Some(CloudflareProxyOptions::DnsOnly),
                cname_flattening: CloudflareCnameFlattening::ProviderDefault,
                comment: Some("Managed by Edgion Center".to_string()),
                tags: BTreeSet::from(["owner:platform".to_string()]),
            })
        );

        let mut bad_ttl = proxied.clone();
        bad_ttl.ttl = DnsTtl::Seconds(300);
        assert!(bad_ttl
            .validate(&zone_for(CloudProvider::Cloudflare))
            .is_err());

        let cloudflare_txt = ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("metadata.example.com").unwrap(),
                record_type: ProviderDnsRecordType::Txt,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Automatic,
            values: BTreeSet::from([DnsRecordSetValue::Txt {
                value: DnsTxtValue::new(vec![DnsCharacterString::new(b"value".to_vec()).unwrap()])
                    .unwrap(),
            }]),
            extension: Some(DnsRecordExtension::Cloudflare {
                proxy: None,
                cname_flattening: CloudflareCnameFlattening::ProviderDefault,
                comment: Some("verification".to_string()),
                tags: BTreeSet::from(["owner:platform".to_string()]),
            }),
        };
        assert!(cloudflare_txt
            .validate(&zone_for(CloudProvider::Cloudflare))
            .is_ok());
        let mut duplicate_tag_name = cloudflare_txt.clone();
        if let Some(DnsRecordExtension::Cloudflare { tags, .. }) =
            duplicate_tag_name.extension.as_mut()
        {
            tags.insert("Owner:other".to_string());
        }
        assert!(duplicate_tag_name
            .validate(&zone_for(CloudProvider::Cloudflare))
            .is_err());

        let mut routed = a_record(["192.0.2.1".parse().unwrap()]);
        routed.key.routing = DnsRoutingIdentity::Route53 {
            set_identifier: "primary".to_string(),
        };
        assert!(routed.validate(&zone_for(CloudProvider::Aws)).is_err());
        routed.extension = Some(DnsRecordExtension::Route53 {
            alias_target: None,
            routing_policy: Some(Route53RoutingPolicy::Failover {
                role: Route53FailoverRole::Primary,
            }),
            health_check_id: None,
        });
        assert!(routed.validate(&zone_for(CloudProvider::Aws)).is_ok());
    }

    #[test]
    fn route53_batch_requires_one_consistent_routing_family() {
        fn routed(set_identifier: &str, policy: Route53RoutingPolicy) -> ProviderDnsRecordSet {
            let mut record = a_record(["192.0.2.1".parse().unwrap()]);
            record.key.routing = DnsRoutingIdentity::Route53 {
                set_identifier: set_identifier.to_string(),
            };
            record.extension = Some(DnsRecordExtension::Route53 {
                alias_target: None,
                routing_policy: Some(policy),
                health_check_id: None,
            });
            record
        }

        let weighted_a = routed("a", Route53RoutingPolicy::Weighted { weight: 10 });
        let weighted_b = routed("b", Route53RoutingPolicy::Weighted { weight: 20 });
        let create = |record_set| DnsRecordChange::Create {
            record_set,
            guard: DnsMutationGuard::MustNotExist,
        };
        assert!(validate_dns_changes(
            &zone_for(CloudProvider::Aws),
            &[create(weighted_a.clone()), create(weighted_b)]
        )
        .is_ok());

        let failover = routed(
            "primary",
            Route53RoutingPolicy::Failover {
                role: Route53FailoverRole::Primary,
            },
        );
        assert!(validate_dns_changes(
            &zone_for(CloudProvider::Aws),
            &[create(weighted_a), create(failover.clone())]
        )
        .is_err());
        let duplicate_primary = routed(
            "also-primary",
            Route53RoutingPolicy::Failover {
                role: Route53FailoverRole::Primary,
            },
        );
        assert!(validate_dns_changes(
            &zone_for(CloudProvider::Aws),
            &[create(failover), create(duplicate_primary)]
        )
        .is_err());

        let oversized = (0..101)
            .map(|index| {
                create(routed(
                    &format!("weighted-{index}"),
                    Route53RoutingPolicy::Weighted { weight: 1 },
                ))
            })
            .collect::<Vec<_>>();
        assert!(validate_dns_changes(&zone_for(CloudProvider::Aws), &oversized).is_err());
    }

    #[test]
    fn soa_is_typed_single_valued_and_serializable() {
        let record = soa_record();
        assert!(record.validate(&zone_for(CloudProvider::Aws)).is_ok());
        assert_eq!(
            serde_json::to_value(&record.values).unwrap()[0]["type"],
            "SOA"
        );

        let mut multiple = record.clone();
        multiple.values.insert(DnsRecordSetValue::Soa {
            primary_name_server: AbsoluteDnsName::new("ns-2.example.net").unwrap(),
            responsible_mailbox: AbsoluteDnsName::new("hostmaster.example.com").unwrap(),
            serial: 2,
            refresh: 7200,
            retry: 900,
            expire: 1_209_600,
            minimum: 86400,
        });
        assert!(multiple.validate(&zone_for(CloudProvider::Aws)).is_err());
    }

    #[test]
    fn route53_geolocation_and_multivalue_combinations_are_typed() {
        for location in [
            Route53GeoLocation::Default,
            Route53GeoLocation::Continent {
                code: "EU".to_string(),
            },
            Route53GeoLocation::Country {
                code: "DE".to_string(),
            },
            Route53GeoLocation::UsSubdivision {
                code: "CA".to_string(),
            },
        ] {
            assert!(location.validate().is_ok());
        }
        assert!(Route53GeoLocation::Continent {
            code: "XX".to_string()
        }
        .validate()
        .is_err());
        assert!(Route53GeoLocation::Country {
            code: "de".to_string()
        }
        .validate()
        .is_err());

        let routed = |set_identifier: &str, policy: Route53RoutingPolicy| {
            let mut record = a_record(["192.0.2.1".parse().unwrap(), "192.0.2.2".parse().unwrap()]);
            record.key.routing = DnsRoutingIdentity::Route53 {
                set_identifier: set_identifier.to_string(),
            };
            record.extension = Some(DnsRecordExtension::Route53 {
                alias_target: None,
                routing_policy: Some(policy),
                health_check_id: None,
            });
            record
        };
        let multivalue = routed("mv-a", Route53RoutingPolicy::Multivalue);
        assert!(multivalue.validate(&zone_for(CloudProvider::Aws)).is_ok());
        let weighted = routed("weighted-a", Route53RoutingPolicy::Weighted { weight: 1 });
        assert!(weighted.validate(&zone_for(CloudProvider::Aws)).is_err());

        let create = |record_set| DnsRecordChange::Create {
            record_set,
            guard: DnsMutationGuard::MustNotExist,
        };
        let geo_a = routed(
            "geo-a",
            Route53RoutingPolicy::Geolocation {
                location: Route53GeoLocation::Country {
                    code: "DE".to_string(),
                },
            },
        );
        let geo_b = routed(
            "geo-b",
            Route53RoutingPolicy::Geolocation {
                location: Route53GeoLocation::Country {
                    code: "DE".to_string(),
                },
            },
        );
        assert!(validate_dns_changes(
            &zone_for(CloudProvider::Aws),
            &[create(geo_a), create(geo_b)]
        )
        .is_err());
    }

    #[test]
    fn route53_health_check_is_backward_compatible_and_alias_aware() {
        let extension = DnsRecordExtension::Route53 {
            alias_target: Some(Route53AliasTarget {
                target_zone_id: DnsZoneId::new("Z123").unwrap(),
                target: AbsoluteDnsName::new("dualstack.lb.example.net").unwrap(),
                evaluate_target_health: true,
            }),
            routing_policy: Some(Route53RoutingPolicy::Failover {
                role: Route53FailoverRole::Primary,
            }),
            health_check_id: Some(Route53HealthCheckId::new("health-check-a").unwrap()),
        };
        let mut alias = a_record([]);
        alias.ttl = DnsTtl::Inherited;
        alias.key.routing = DnsRoutingIdentity::Route53 {
            set_identifier: "primary".to_string(),
        };
        alias.extension = Some(extension);
        assert!(alias.validate(&zone_for(CloudProvider::Aws)).is_ok());

        let legacy = serde_json::json!({
            "provider": "route53",
            "alias_target": null,
            "routing_policy": {
                "type": "latency",
                "region": "us-east-1"
            }
        });
        let decoded: DnsRecordExtension = serde_json::from_value(legacy).unwrap();
        assert!(matches!(
            decoded,
            DnsRecordExtension::Route53 {
                health_check_id: None,
                ..
            }
        ));

        if let Some(DnsRecordExtension::Route53 {
            alias_target: Some(target),
            ..
        }) = alias.extension.as_mut()
        {
            target.evaluate_target_health = false;
        }
        assert!(alias.validate(&zone_for(CloudProvider::Aws)).is_err());

        let mut simple = a_record(["192.0.2.1".parse().unwrap()]);
        simple.extension = Some(DnsRecordExtension::Route53 {
            alias_target: None,
            routing_policy: None,
            health_check_id: Some(Route53HealthCheckId::new("health-check-b").unwrap()),
        });
        assert!(simple.validate(&zone_for(CloudProvider::Aws)).is_ok());
    }

    #[test]
    fn route53_provider_boundaries_and_routing_ttl_are_order_independent() {
        let zone = zone_for(CloudProvider::Aws);
        let wildcard_ns = ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("*.example.com").unwrap(),
                record_type: ProviderDnsRecordType::Ns,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: BTreeSet::from([DnsRecordSetValue::Ns {
                target: AbsoluteDnsName::new("ns.example.net").unwrap(),
            }]),
            extension: None,
        };
        assert!(wildcard_ns.validate(&zone).is_err());

        let mut apex_cname = ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("example.com").unwrap(),
                record_type: ProviderDnsRecordType::Cname,
                routing: DnsRoutingIdentity::Route53 {
                    set_identifier: "latency-a".to_string(),
                },
            },
            ttl: DnsTtl::Seconds(0),
            values: BTreeSet::from([DnsRecordSetValue::Cname {
                target: AbsoluteDnsName::new("target.example.net").unwrap(),
            }]),
            extension: Some(DnsRecordExtension::Route53 {
                alias_target: None,
                routing_policy: Some(Route53RoutingPolicy::Latency {
                    region: "us-east-1".to_string(),
                }),
                health_check_id: None,
            }),
        };
        assert!(apex_cname.validate(&zone).is_err());
        apex_cname.key.owner = DnsOwnerName::new("www.example.com").unwrap();
        assert!(apex_cname.validate(&zone).is_ok());

        let weighted = |set_identifier: &str, ttl: DnsTtl, alias: bool| {
            let mut record = a_record(if alias {
                Vec::new()
            } else {
                vec!["192.0.2.1".parse().unwrap()]
            });
            record.ttl = ttl;
            record.key.routing = DnsRoutingIdentity::Route53 {
                set_identifier: set_identifier.to_string(),
            };
            record.extension = Some(DnsRecordExtension::Route53 {
                alias_target: alias.then(|| Route53AliasTarget {
                    target_zone_id: DnsZoneId::new("Z123").unwrap(),
                    target: AbsoluteDnsName::new("dualstack.lb.example.net").unwrap(),
                    evaluate_target_health: true,
                }),
                routing_policy: Some(Route53RoutingPolicy::Weighted { weight: 1 }),
                health_check_id: None,
            });
            record
        };
        let create = |record_set| DnsRecordChange::Create {
            record_set,
            guard: DnsMutationGuard::MustNotExist,
        };
        let alias = create(weighted("alias", DnsTtl::Inherited, true));
        let ttl_60 = create(weighted("ttl-60", DnsTtl::Seconds(60), false));
        let ttl_300 = create(weighted("ttl-300", DnsTtl::Seconds(300), false));
        assert!(validate_dns_changes(&zone, &[alias, ttl_60, ttl_300]).is_err());
    }

    #[test]
    fn apex_control_records_are_not_ordinary_rrset_mutations() {
        let zone = zone_for(CloudProvider::Aws);
        let soa = soa_record();
        let observed = ObservedDnsRecordSet {
            zone: zone.clone(),
            record_set: soa.clone(),
            provider_object_ids: BTreeSet::new(),
            revision: DnsRecordRevision::new("revision-soa").unwrap(),
        };
        let exact = DnsMutationGuard::MatchObserved {
            revision: observed.revision.clone(),
        };
        for change in [
            DnsRecordChange::Create {
                record_set: soa.clone(),
                guard: DnsMutationGuard::MustNotExist,
            },
            DnsRecordChange::Replace {
                previous: observed.clone(),
                desired: soa,
                guard: exact.clone(),
            },
            DnsRecordChange::Delete {
                previous: observed,
                guard: exact,
            },
        ] {
            assert!(validate_dns_changes(&zone, &[change]).is_err());
        }

        let apex_ns = ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("example.com").unwrap(),
                record_type: ProviderDnsRecordType::Ns,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(172_800),
            values: BTreeSet::from([DnsRecordSetValue::Ns {
                target: AbsoluteDnsName::new("ns.example.net").unwrap(),
            }]),
            extension: None,
        };
        let observed_ns = ObservedDnsRecordSet {
            zone: zone.clone(),
            record_set: apex_ns.clone(),
            provider_object_ids: BTreeSet::new(),
            revision: DnsRecordRevision::new("revision-ns").unwrap(),
        };
        let exact_ns = DnsMutationGuard::MatchObserved {
            revision: observed_ns.revision.clone(),
        };
        for change in [
            DnsRecordChange::Create {
                record_set: apex_ns.clone(),
                guard: DnsMutationGuard::MustNotExist,
            },
            DnsRecordChange::Replace {
                previous: observed_ns.clone(),
                desired: apex_ns,
                guard: exact_ns.clone(),
            },
            DnsRecordChange::Delete {
                previous: observed_ns,
                guard: exact_ns,
            },
        ] {
            assert!(validate_dns_changes(&zone, &[change]).is_err());
        }

        let delegation = ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("child.example.com").unwrap(),
                record_type: ProviderDnsRecordType::Ns,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: BTreeSet::from([DnsRecordSetValue::Ns {
                target: AbsoluteDnsName::new("ns.example.net").unwrap(),
            }]),
            extension: None,
        };
        assert!(validate_dns_changes(
            &zone,
            &[DnsRecordChange::Create {
                record_set: delegation,
                guard: DnsMutationGuard::MustNotExist,
            }]
        )
        .is_ok());
    }

    fn soa_record() -> ProviderDnsRecordSet {
        ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("example.com").unwrap(),
                record_type: ProviderDnsRecordType::Soa,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(900),
            values: BTreeSet::from([DnsRecordSetValue::Soa {
                primary_name_server: AbsoluteDnsName::new("ns-1.example.net").unwrap(),
                responsible_mailbox: AbsoluteDnsName::new("hostmaster.example.com").unwrap(),
                serial: 1,
                refresh: 7200,
                retry: 900,
                expire: 1_209_600,
                minimum: 86400,
            }]),
            extension: None,
        }
    }

    #[test]
    fn batch_validation_rejects_duplicate_identity_and_cname_coexistence() {
        let record = a_record(["192.0.2.1".parse().unwrap()]);
        let create = DnsRecordChange::Create {
            record_set: record.clone(),
            guard: DnsMutationGuard::MustNotExist,
        };
        assert!(validate_dns_changes(&zone(), &[create.clone(), create]).is_err());

        let cname = ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: record.key.owner.clone(),
                record_type: ProviderDnsRecordType::Cname,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: BTreeSet::from([DnsRecordSetValue::Cname {
                target: AbsoluteDnsName::new("target.example.net").unwrap(),
            }]),
            extension: None,
        };
        assert!(validate_dns_changes(
            &zone(),
            &[
                DnsRecordChange::Create {
                    record_set: record,
                    guard: DnsMutationGuard::MustNotExist,
                },
                DnsRecordChange::Create {
                    record_set: cname,
                    guard: DnsMutationGuard::MustNotExist,
                },
            ]
        )
        .is_err());
    }

    #[test]
    fn replacement_and_deletion_require_the_exact_observed_revision() {
        let previous = observed(a_record(["192.0.2.1".parse().unwrap()]));
        let desired = a_record(["192.0.2.2".parse().unwrap()]);
        let stale = DnsRecordChange::Replace {
            previous: previous.clone(),
            desired: desired.clone(),
            guard: DnsMutationGuard::MatchObserved {
                revision: DnsRecordRevision::new("stale").unwrap(),
            },
        };
        assert!(validate_dns_changes(&zone(), &[stale]).is_err());
        let exact = DnsRecordChange::Replace {
            previous: previous.clone(),
            desired,
            guard: DnsMutationGuard::MatchObserved {
                revision: previous.revision.clone(),
            },
        };
        assert!(validate_dns_changes(&zone(), &[exact]).is_ok());

        let delete = DnsRecordChange::Delete {
            previous: previous.clone(),
            guard: DnsMutationGuard::MatchObserved {
                revision: previous.revision,
            },
        };
        let cname = ProviderDnsRecordSet {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("www.example.com").unwrap(),
                record_type: ProviderDnsRecordType::Cname,
                routing: DnsRoutingIdentity::Simple,
            },
            ttl: DnsTtl::Seconds(300),
            values: BTreeSet::from([DnsRecordSetValue::Cname {
                target: AbsoluteDnsName::new("target.example.net").unwrap(),
            }]),
            extension: None,
        };
        assert!(validate_dns_changes(
            &zone(),
            &[
                delete,
                DnsRecordChange::Create {
                    record_set: cname,
                    guard: DnsMutationGuard::MustNotExist,
                },
            ]
        )
        .is_ok());
    }

    #[test]
    fn pagination_and_receipts_fail_closed() {
        assert!(DnsPageRequest {
            limit: 0,
            token: None
        }
        .validate()
        .is_err());
        assert!(DnsPageRequest {
            limit: 1000,
            token: None
        }
        .validate()
        .is_ok());
        assert!(DnsChangeReceipt {
            id: DnsChangeId::new("change-a").unwrap(),
            state: DnsChangeState::Pending,
            submission_atomicity: DnsBatchAtomicity::AllOrNothing,
            propagation: DnsPropagationState::ProviderReportedApplied,
            guard_strength: DnsGuardStrength::BestEffort,
        }
        .validate()
        .is_err());
        let weak = DnsChangeReceipt {
            id: DnsChangeId::new("change-b").unwrap(),
            state: DnsChangeState::ProviderCommitted,
            submission_atomicity: DnsBatchAtomicity::AllOrNothing,
            propagation: DnsPropagationState::Unknown,
            guard_strength: DnsGuardStrength::BestEffort,
        };
        assert!(weak
            .validate_against_request(DnsGuardStrength::Atomic)
            .is_err());
        let partial = DnsChangeReceipt {
            id: DnsChangeId::new("change-c").unwrap(),
            state: DnsChangeState::ProviderCommitted,
            submission_atomicity: DnsBatchAtomicity::PerChange,
            propagation: DnsPropagationState::Unknown,
            guard_strength: DnsGuardStrength::BestEffort,
        };
        assert!(partial.validate().is_err());
    }
}
