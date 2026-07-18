use std::{collections::BTreeSet, sync::Arc, time::Duration};

use edgion_center_core::{
    AbsoluteDnsName, DnsCharacterString, DnsRecordSetValue, DnsRrsetExpectation, DnsTxtValue,
    DnsVerificationRequest, DnssecDsRecord, DnssecVerificationExpectation, ProviderDnsRecordType,
    ResolverProfileId,
};
use hickory_proto::{
    dnssec::{rdata::DNSSECRData, Proof, TrustAnchors},
    rr::{DNSClass, RData, Record, RecordType},
};
use hickory_resolver::{
    config::{
        ConnectionConfig, NameServerConfig, ResolveHosts, ResolverConfig, ResolverOpts,
        ServerOrderingStrategy,
    },
    net::{runtime::TokioRuntimeProvider, DnsError, NetError},
    Resolver, TokioResolver,
};
use tokio::time::timeout;

use crate::ResolverProfile;

const MAX_PROFILE_ENDPOINTS: usize = 8;

/// Sanitized reason for a local DNSSEC result. Provider responses and resolver
/// error strings are deliberately not retained in verification evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalDnssecReason {
    Verified,
    NotRequested,
    InvalidRequest,
    InvalidResolverProfile,
    UnsupportedRecordType,
    RrsetMismatch,
    ParentDsMismatch,
    ParentDsPresent,
    ParentDsAbsenceUnproven,
    LookupTimeout,
    TransportFailure,
    ValidationFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalDnssecSecurity {
    Secure,
    Insecure,
    Bogus,
    Indeterminate,
}

impl From<Proof> for LocalDnssecSecurity {
    fn from(value: Proof) -> Self {
        match value {
            Proof::Secure => Self::Secure,
            Proof::Insecure => Self::Insecure,
            Proof::Bogus => Self::Bogus,
            Proof::Indeterminate => Self::Indeterminate,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalRrsetValidation {
    NotChecked,
    Match,
    AuthenticatedAbsent,
    Mismatch {
        observed: BTreeSet<DnsRecordSetValue>,
    },
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalParentDsValidation {
    NotChecked,
    Match { observed: Vec<DnssecDsRecord> },
    AuthenticatedAbsent,
    Mismatch { observed: Vec<DnssecDsRecord> },
    Unexpected { observed: Vec<DnssecDsRecord> },
    AbsenceUnproven,
    Failed,
}

/// Typed local-chain result. `Secure` describes cryptographic validation only;
/// callers must also require `satisfies_expectation()` before publishing DNS
/// readiness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDnssecValidation {
    pub security: LocalDnssecSecurity,
    pub rrset: LocalRrsetValidation,
    pub parent_ds: LocalParentDsValidation,
    pub reason: LocalDnssecReason,
}

impl LocalDnssecValidation {
    pub fn satisfies_expectation(&self) -> bool {
        self.reason == LocalDnssecReason::Verified
            && matches!(
                self.rrset,
                LocalRrsetValidation::Match | LocalRrsetValidation::AuthenticatedAbsent
            )
            && matches!(
                self.parent_ds,
                LocalParentDsValidation::NotChecked
                    | LocalParentDsValidation::Match { .. }
                    | LocalParentDsValidation::AuthenticatedAbsent
            )
    }
}

/// Inspectable configuration proving that no system resolver or hosts-file
/// fallback is enabled. Endpoints are copied from one explicit ResolverProfile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDnssecResolverConfiguration {
    pub profile_id: ResolverProfileId,
    pub endpoints: BTreeSet<std::net::SocketAddr>,
    pub uses_system_configuration: bool,
    pub uses_hosts_file: bool,
    pub uses_default_root_trust_anchors: bool,
}

#[derive(Clone)]
pub struct LocalDnssecValidator {
    resolver: TokioResolver,
    configuration: LocalDnssecResolverConfiguration,
    lookup_timeout: Duration,
}

impl LocalDnssecValidator {
    /// Builds a validating stub resolver from explicit endpoints only. The
    /// target policy is checked before any resolver is constructed.
    pub fn from_profile(
        profile: &ResolverProfile,
        lookup_timeout: Duration,
    ) -> Result<Self, LocalDnssecReason> {
        if lookup_timeout.is_zero()
            || profile.endpoints.is_empty()
            || profile.endpoints.len() > MAX_PROFILE_ENDPOINTS
            || profile
                .endpoints
                .iter()
                .any(|endpoint| !profile.target_policy.permits(*endpoint))
        {
            return Err(LocalDnssecReason::InvalidResolverProfile);
        }

        let name_servers = profile
            .endpoints
            .iter()
            .map(|endpoint| {
                let mut udp = ConnectionConfig::udp();
                udp.port = endpoint.port();
                let mut tcp = ConnectionConfig::tcp();
                tcp.port = endpoint.port();
                NameServerConfig::new(endpoint.ip(), true, vec![udp, tcp])
            })
            .collect();
        let config = ResolverConfig::from_parts(None, Vec::new(), name_servers);
        let mut options = ResolverOpts::default();
        options.timeout = lookup_timeout;
        options.attempts = 1;
        options.validate = true;
        options.use_hosts_file = ResolveHosts::Never;
        options.num_concurrent_reqs = 1;
        options.server_ordering_strategy = ServerOrderingStrategy::UserProvidedOrder;
        let resolver = Resolver::builder_with_config(config, TokioRuntimeProvider::default())
            .with_options(options)
            .with_trust_anchor(Arc::new(TrustAnchors::default()))
            .build()
            .map_err(|_| LocalDnssecReason::InvalidResolverProfile)?;

        Ok(Self {
            resolver,
            configuration: LocalDnssecResolverConfiguration {
                profile_id: profile.id.clone(),
                endpoints: profile.endpoints.clone(),
                uses_system_configuration: false,
                uses_hosts_file: false,
                uses_default_root_trust_anchors: true,
            },
            lookup_timeout,
        })
    }

    pub fn configuration(&self) -> &LocalDnssecResolverConfiguration {
        &self.configuration
    }

    pub async fn validate(&self, request: &DnsVerificationRequest) -> LocalDnssecValidation {
        if request.validate().is_err() {
            return failed(LocalDnssecReason::InvalidRequest);
        }
        if matches!(request.dnssec, DnssecVerificationExpectation::NotRequested) {
            return LocalDnssecValidation {
                security: LocalDnssecSecurity::Indeterminate,
                rrset: LocalRrsetValidation::NotChecked,
                parent_ds: LocalParentDsValidation::NotChecked,
                reason: LocalDnssecReason::NotRequested,
            };
        }

        let record_type = match record_type(request.expectation.key().record_type) {
            Some(record_type) => record_type,
            None => return failed(LocalDnssecReason::UnsupportedRecordType),
        };
        let owner = request.expectation.key().owner.fqdn();
        let rrset_lookup = timeout(
            self.lookup_timeout,
            self.resolver.lookup(owner.as_str(), record_type),
        )
        .await;
        let (security, rrset) = match rrset_lookup {
            Err(_) => {
                return failed_with_security(
                    LocalDnssecSecurity::Indeterminate,
                    LocalDnssecReason::LookupTimeout,
                );
            }
            Ok(Ok(lookup)) => {
                let proof = weakest_proof(lookup.answers(), record_type);
                let observed = record_values(lookup.answers(), &owner, record_type);
                let result = match &request.expectation {
                    DnsRrsetExpectation::Present { values, .. } if values == &observed => {
                        LocalRrsetValidation::Match
                    }
                    DnsRrsetExpectation::Present { .. } | DnsRrsetExpectation::Absent { .. } => {
                        LocalRrsetValidation::Mismatch { observed }
                    }
                };
                (proof.into(), result)
            }
            Ok(Err(error)) => match negative_proof(&error) {
                Some(proof)
                    if matches!(request.expectation, DnsRrsetExpectation::Absent { .. }) =>
                {
                    (proof.into(), LocalRrsetValidation::AuthenticatedAbsent)
                }
                Some(proof) => (proof.into(), LocalRrsetValidation::Failed),
                None => return failed(map_lookup_error(&error)),
            },
        };

        if security == LocalDnssecSecurity::Bogus {
            return LocalDnssecValidation {
                security,
                rrset,
                parent_ds: LocalParentDsValidation::Failed,
                reason: LocalDnssecReason::ValidationFailure,
            };
        }
        if !matches!(
            rrset,
            LocalRrsetValidation::Match | LocalRrsetValidation::AuthenticatedAbsent
        ) {
            return LocalDnssecValidation {
                security,
                rrset,
                parent_ds: LocalParentDsValidation::NotChecked,
                reason: LocalDnssecReason::RrsetMismatch,
            };
        }

        if matches!(
            request.dnssec,
            DnssecVerificationExpectation::Unsigned {
                require_parent_ds_absent: false
            }
        ) {
            let reason = if security == LocalDnssecSecurity::Bogus {
                LocalDnssecReason::ValidationFailure
            } else {
                LocalDnssecReason::Verified
            };
            return LocalDnssecValidation {
                security,
                rrset,
                parent_ds: LocalParentDsValidation::NotChecked,
                reason,
            };
        }

        let apex = match &request.scope {
            edgion_center_core::DnsVerificationScope::DelegatedValidation {
                child_apex, ..
            } => child_apex.fqdn(),
            _ => request.binding.zone.apex.fqdn(),
        };
        let ds_lookup = timeout(
            self.lookup_timeout,
            self.resolver.lookup(apex.as_str(), RecordType::DS),
        )
        .await;
        let (parent_security, parent_ds) = match ds_lookup {
            Err(_) => {
                return LocalDnssecValidation {
                    security: LocalDnssecSecurity::Indeterminate,
                    rrset,
                    parent_ds: LocalParentDsValidation::Failed,
                    reason: LocalDnssecReason::LookupTimeout,
                };
            }
            Ok(Ok(lookup)) => {
                let proof = weakest_proof(lookup.answers(), RecordType::DS);
                let observed = ds_records(lookup.answers(), &apex);
                let result = match &request.dnssec {
                    DnssecVerificationExpectation::Signed { expected_ds }
                        if ds_sets_equal(expected_ds, &observed) =>
                    {
                        LocalParentDsValidation::Match { observed }
                    }
                    DnssecVerificationExpectation::Signed { .. } => {
                        LocalParentDsValidation::Mismatch { observed }
                    }
                    DnssecVerificationExpectation::Unsigned { .. } => {
                        LocalParentDsValidation::Unexpected { observed }
                    }
                    DnssecVerificationExpectation::NotRequested => unreachable!(),
                };
                (proof.into(), result)
            }
            Ok(Err(error)) => match negative_proof(&error) {
                Some(Proof::Secure) => (
                    LocalDnssecSecurity::Secure,
                    LocalParentDsValidation::AuthenticatedAbsent,
                ),
                Some(proof) => (proof.into(), LocalParentDsValidation::AbsenceUnproven),
                None => {
                    return LocalDnssecValidation {
                        security: LocalDnssecSecurity::Indeterminate,
                        rrset,
                        parent_ds: LocalParentDsValidation::Failed,
                        reason: map_lookup_error(&error),
                    };
                }
            },
        };

        let combined_security = weakest_security(security, parent_security);
        let reason = match (&request.dnssec, &parent_ds, combined_security) {
            (
                DnssecVerificationExpectation::Signed { .. },
                LocalParentDsValidation::Match { .. },
                LocalDnssecSecurity::Secure,
            ) => LocalDnssecReason::Verified,
            (
                DnssecVerificationExpectation::Unsigned {
                    require_parent_ds_absent: true,
                },
                LocalParentDsValidation::AuthenticatedAbsent,
                LocalDnssecSecurity::Secure | LocalDnssecSecurity::Insecure,
            ) => LocalDnssecReason::Verified,
            (
                DnssecVerificationExpectation::Unsigned {
                    require_parent_ds_absent: false,
                },
                LocalParentDsValidation::AuthenticatedAbsent,
                LocalDnssecSecurity::Secure | LocalDnssecSecurity::Insecure,
            ) => LocalDnssecReason::Verified,
            (_, LocalParentDsValidation::Mismatch { .. }, _) => LocalDnssecReason::ParentDsMismatch,
            (_, LocalParentDsValidation::Unexpected { .. }, _) => {
                LocalDnssecReason::ParentDsPresent
            }
            (_, LocalParentDsValidation::AbsenceUnproven, _) => {
                LocalDnssecReason::ParentDsAbsenceUnproven
            }
            (_, _, LocalDnssecSecurity::Bogus) => LocalDnssecReason::ValidationFailure,
            _ => LocalDnssecReason::ParentDsAbsenceUnproven,
        };
        LocalDnssecValidation {
            security: combined_security,
            rrset,
            parent_ds,
            reason,
        }
    }
}

fn record_type(value: ProviderDnsRecordType) -> Option<RecordType> {
    match value {
        ProviderDnsRecordType::A => Some(RecordType::A),
        ProviderDnsRecordType::Aaaa => Some(RecordType::AAAA),
        ProviderDnsRecordType::Cname => Some(RecordType::CNAME),
        ProviderDnsRecordType::Txt => Some(RecordType::TXT),
        _ => None,
    }
}

fn weakest_proof(records: &[Record], record_type: RecordType) -> Proof {
    records
        .iter()
        .filter(|record| record.record_type() == record_type)
        .map(|record| record.proof)
        .min()
        .unwrap_or(Proof::Indeterminate)
}

fn negative_proof(error: &NetError) -> Option<Proof> {
    match error {
        NetError::Dns(DnsError::Nsec { proof, .. }) => Some(*proof),
        NetError::Dns(DnsError::NoRecordsFound(no_records)) => no_records
            .authorities
            .as_deref()
            .and_then(|records| records.iter().map(|record| record.proof).min()),
        _ => None,
    }
}

fn map_lookup_error(error: &NetError) -> LocalDnssecReason {
    match error {
        NetError::Timeout | NetError::Busy => LocalDnssecReason::LookupTimeout,
        NetError::Dns(DnsError::Nsec {
            proof: Proof::Bogus,
            ..
        }) => LocalDnssecReason::ValidationFailure,
        NetError::Dns(_) | NetError::NoConnections | NetError::Io(_) => {
            LocalDnssecReason::TransportFailure
        }
        _ => LocalDnssecReason::ValidationFailure,
    }
}

fn record_values(
    records: &[Record],
    expected_owner: &str,
    record_type: RecordType,
) -> BTreeSet<DnsRecordSetValue> {
    records
        .iter()
        .filter(|record| {
            record.record_type() == record_type
                && record.name.to_ascii() == expected_owner
                && record.dns_class == DNSClass::IN
        })
        .filter_map(|record| match &record.data {
            RData::A(value) => Some(DnsRecordSetValue::A { address: value.0 }),
            RData::AAAA(value) => Some(DnsRecordSetValue::Aaaa { address: value.0 }),
            RData::CNAME(value) => AbsoluteDnsName::new(trim_dot(&value.0.to_ascii()))
                .ok()
                .map(|target| DnsRecordSetValue::Cname { target }),
            RData::TXT(value) => value
                .txt_data
                .iter()
                .map(|segment| DnsCharacterString::new(segment.to_vec()))
                .collect::<Result<Vec<_>, _>>()
                .and_then(DnsTxtValue::new)
                .ok()
                .map(|value| DnsRecordSetValue::Txt { value }),
            _ => None,
        })
        .collect()
}

fn ds_records(records: &[Record], expected_owner: &str) -> Vec<DnssecDsRecord> {
    records
        .iter()
        .filter(|record| {
            record.record_type() == RecordType::DS
                && record.name.to_ascii() == expected_owner
                && record.dns_class == DNSClass::IN
        })
        .filter_map(|record| match &record.data {
            RData::DNSSEC(DNSSECRData::DS(ds)) => Some(DnssecDsRecord {
                key_tag: ds.key_tag(),
                algorithm: ds.algorithm().into(),
                digest_type: ds.digest_type().into(),
                digest: ds
                    .digest()
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect(),
            }),
            _ => None,
        })
        .collect()
}

fn ds_sets_equal(left: &[DnssecDsRecord], right: &[DnssecDsRecord]) -> bool {
    let set = |records: &[DnssecDsRecord]| {
        records
            .iter()
            .map(|record| {
                (
                    record.key_tag,
                    record.algorithm,
                    record.digest_type,
                    record.digest.clone(),
                )
            })
            .collect::<BTreeSet<_>>()
    };
    left.len() == right.len() && set(left) == set(right)
}

fn weakest_security(left: LocalDnssecSecurity, right: LocalDnssecSecurity) -> LocalDnssecSecurity {
    use LocalDnssecSecurity::{Bogus, Indeterminate, Insecure, Secure};
    match (left, right) {
        (Bogus, _) | (_, Bogus) => Bogus,
        (Indeterminate, _) | (_, Indeterminate) => Indeterminate,
        (Insecure, _) | (_, Insecure) => Insecure,
        (Secure, Secure) => Secure,
    }
}

fn failed(reason: LocalDnssecReason) -> LocalDnssecValidation {
    failed_with_security(LocalDnssecSecurity::Indeterminate, reason)
}

fn failed_with_security(
    security: LocalDnssecSecurity,
    reason: LocalDnssecReason,
) -> LocalDnssecValidation {
    LocalDnssecValidation {
        security,
        rrset: LocalRrsetValidation::Failed,
        parent_ds: LocalParentDsValidation::Failed,
        reason,
    }
}

fn trim_dot(value: &str) -> &str {
    value.strip_suffix('.').unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        str::FromStr,
    };

    use edgion_center_core::ResolverProfileId;
    use hickory_proto::{
        dnssec::{rdata::DS, Algorithm, DigestType, Proof},
        rr::{rdata::A, Name, RData, Record, RecordType},
    };

    use super::*;
    use crate::{DnsTargetPolicy, IpNetwork};

    fn profile() -> ResolverProfile {
        ResolverProfile {
            id: ResolverProfileId::new("private-dnssec").unwrap(),
            revision: edgion_center_core::ResolverProfileRevision::new("r1").unwrap(),
            endpoints: BTreeSet::from([SocketAddr::from(([10, 2, 3, 4], 5353))]),
            target_policy: DnsTargetPolicy::Explicit {
                allowed_networks: vec![
                    IpNetwork::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 8).unwrap()
                ],
                allowed_ports: vec![5353],
            },
            trust_authenticated_data: false,
        }
    }

    #[test]
    fn resolver_uses_only_explicit_profile_without_system_or_hosts_fallback() {
        let validator =
            LocalDnssecValidator::from_profile(&profile(), Duration::from_secs(1)).unwrap();
        let configuration = validator.configuration();
        assert_eq!(configuration.profile_id.as_str(), "private-dnssec");
        assert_eq!(
            configuration.endpoints,
            BTreeSet::from([SocketAddr::from(([10, 2, 3, 4], 5353))])
        );
        assert!(!configuration.uses_system_configuration);
        assert!(!configuration.uses_hosts_file);
        assert!(configuration.uses_default_root_trust_anchors);
    }

    #[test]
    fn denied_endpoint_is_rejected_before_resolver_construction() {
        let mut profile = profile();
        profile.endpoints = BTreeSet::from([SocketAddr::from(([192, 168, 1, 2], 5353))]);
        assert!(matches!(
            LocalDnssecValidator::from_profile(&profile, Duration::from_secs(1)),
            Err(LocalDnssecReason::InvalidResolverProfile)
        ));
    }

    #[test]
    fn rrset_value_and_proof_are_evaluated_independently() {
        let mut record = Record::from_rdata(
            Name::from_str("www.example.com.").unwrap(),
            60,
            RData::A(A::new(192, 0, 2, 1)),
        );
        record.proof = Proof::Secure;
        assert_eq!(
            weakest_proof(&[record.clone()], RecordType::A),
            Proof::Secure
        );
        assert_eq!(
            record_values(&[record], "www.example.com.", RecordType::A),
            BTreeSet::from([DnsRecordSetValue::A {
                address: Ipv4Addr::new(192, 0, 2, 1),
            }])
        );
    }

    #[test]
    fn locally_validated_rrset_rejects_non_in_class() {
        let mut record = Record::from_rdata(
            Name::from_str("www.example.com.").unwrap(),
            60,
            RData::A(A::new(192, 0, 2, 1)),
        );
        record.dns_class = DNSClass::CH;
        assert!(record_values(&[record], "www.example.com.", RecordType::A).is_empty());
    }

    #[test]
    fn bogus_and_indeterminate_remain_distinct() {
        assert_eq!(
            weakest_security(LocalDnssecSecurity::Secure, LocalDnssecSecurity::Bogus),
            LocalDnssecSecurity::Bogus
        );
        assert_eq!(
            weakest_security(
                LocalDnssecSecurity::Secure,
                LocalDnssecSecurity::Indeterminate
            ),
            LocalDnssecSecurity::Indeterminate
        );
    }

    #[test]
    fn secure_chain_without_expected_rrset_is_not_success() {
        let result = LocalDnssecValidation {
            security: LocalDnssecSecurity::Secure,
            rrset: LocalRrsetValidation::Mismatch {
                observed: BTreeSet::new(),
            },
            parent_ds: LocalParentDsValidation::Match {
                observed: Vec::new(),
            },
            reason: LocalDnssecReason::RrsetMismatch,
        };
        assert!(!result.satisfies_expectation());
    }

    #[test]
    fn resolver_errors_map_to_sanitized_typed_reasons() {
        assert_eq!(
            map_lookup_error(&NetError::Timeout),
            LocalDnssecReason::LookupTimeout
        );
        assert_eq!(
            map_lookup_error(&NetError::NoConnections),
            LocalDnssecReason::TransportFailure
        );
    }

    #[test]
    fn parent_ds_extraction_rejects_unrelated_owner() {
        let expected = Record::from_rdata(
            Name::from_str("validation.example.com.").unwrap(),
            60,
            RData::DNSSEC(DNSSECRData::DS(DS::new(
                42,
                Algorithm::ECDSAP256SHA256,
                DigestType::SHA256,
                vec![0xAA; 32],
            ))),
        );
        let unrelated = Record::from_rdata(
            Name::from_str("unrelated.example.com.").unwrap(),
            60,
            expected.data.clone(),
        );
        let observed = ds_records(&[expected, unrelated], "validation.example.com.");
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].key_tag, 42);
    }

    #[test]
    fn duplicate_parent_ds_does_not_match_expected_set() {
        let record = DnssecDsRecord {
            key_tag: 42,
            algorithm: 13,
            digest_type: 2,
            digest: "AA".repeat(32),
        };
        assert!(!ds_sets_equal(
            std::slice::from_ref(&record),
            &[record.clone(), record.clone()]
        ));
    }
}
