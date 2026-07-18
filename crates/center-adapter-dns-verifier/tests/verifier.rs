use std::{
    collections::{BTreeSet, VecDeque},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_adapter_dns_verifier::{
    DnsQueryTransport, DnsQuestion, DnsTargetPolicy, DnsTransportError, DnsTransportProtocol,
    DnsVerificationClock, DnsWireResponse, IpNetwork, NetworkDnsPropagationVerifier,
    ResolverProfile,
};
use edgion_center_core::{
    AbsoluteDnsName, CloudProvider, CloudResourceId, DelegationState, DnsOwnerName,
    DnsPropagationVerifier, DnsRecordRevision, DnsRecordSetKey, DnsRecordSetValue,
    DnsRoutingIdentity, DnsRrsetExpectation, DnsVerificationBinding, DnsVerificationPolicy,
    DnsVerificationRequest, DnsVerificationRequestId, DnsVerificationScope, DnsZoneId, DnsZoneRef,
    DnssecVerificationExpectation, ProviderDnsRecordType, ResolverProfileId, ZoneLifecycleRevision,
    ZoneVisibility,
};
use hickory_proto::{
    op::{Message, OpCode, Query},
    rr::{
        rdata::{A, NS, SOA},
        Name, RData, Record, RecordType,
    },
};

struct FakeTransport {
    responses: Mutex<VecDeque<Message>>,
    questions: Mutex<Vec<DnsQuestion>>,
}

#[async_trait]
impl DnsQueryTransport for FakeTransport {
    async fn query(
        &self,
        endpoint: SocketAddr,
        _target_policy: &DnsTargetPolicy,
        question: &DnsQuestion,
        _exchange_timeout: Duration,
    ) -> Result<DnsWireResponse, DnsTransportError> {
        self.questions.lock().unwrap().push(question.clone());
        let message = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(DnsTransportError::Timeout)?;
        Ok(DnsWireResponse {
            endpoint,
            protocol: DnsTransportProtocol::Udp,
            message,
        })
    }
}

struct FixedClock;

impl DnsVerificationClock for FixedClock {
    fn now_unix_ms(&self) -> i64 {
        1_000
    }
}

struct PublicFakeTransport {
    questions: Mutex<Vec<DnsQuestion>>,
    delegation_queries: Mutex<u8>,
    stale_first_delegation: bool,
    stale_second_parent: bool,
}

#[async_trait]
impl DnsQueryTransport for PublicFakeTransport {
    async fn query(
        &self,
        endpoint: SocketAddr,
        _target_policy: &DnsTargetPolicy,
        question: &DnsQuestion,
        _exchange_timeout: Duration,
    ) -> Result<DnsWireResponse, DnsTransportError> {
        self.questions.lock().unwrap().push(question.clone());
        let owner = Name::from_ascii(&question.name).unwrap();
        let mut response = Message::response(1, OpCode::Query);
        response.add_query(Query::query(owner.clone(), question.record_type));
        match (question.name.as_str(), question.record_type) {
            ("ns1.example.net.", RecordType::A) => response.add_answer(Record::from_rdata(
                owner,
                60,
                RData::A(A(Ipv4Addr::new(1, 1, 1, 1))),
            )),
            ("ns2.example.net.", RecordType::A) => response.add_answer(Record::from_rdata(
                owner,
                60,
                RData::A(A(Ipv4Addr::new(9, 9, 9, 9))),
            )),
            ("a.parent.example.", RecordType::A) => response.add_answer(Record::from_rdata(
                owner,
                60,
                RData::A(A(Ipv4Addr::new(8, 8, 4, 4))),
            )),
            ("b.parent.example.", RecordType::A) => response.add_answer(Record::from_rdata(
                owner,
                60,
                RData::A(A(Ipv4Addr::new(8, 8, 4, 5))),
            )),
            ("www.example.com.", RecordType::A) => {
                response.metadata.authoritative = !question.recursion_desired;
                response.add_answer(Record::from_rdata(
                    owner,
                    60,
                    RData::A(A(Ipv4Addr::new(192, 0, 2, 10))),
                ))
            }
            ("com.", RecordType::SOA) => {
                response.metadata.authoritative = !question.recursion_desired;
                response.add_answer(Record::from_rdata(
                    owner,
                    60,
                    RData::SOA(SOA::new(
                        Name::from_ascii("a.parent.example.").unwrap(),
                        Name::from_ascii("hostmaster.parent.example.").unwrap(),
                        1,
                        60,
                        60,
                        60,
                        60,
                    )),
                ))
            }
            ("com.", RecordType::NS) => {
                for server in ["a.parent.example.", "b.parent.example."] {
                    response.add_answer(Record::from_rdata(
                        owner.clone(),
                        60,
                        RData::NS(NS(Name::from_ascii(server).unwrap())),
                    ));
                }
                &mut response
            }
            ("example.com.", RecordType::NS) if !question.recursion_desired => {
                let mut count = self.delegation_queries.lock().unwrap();
                *count += 1;
                let nameservers = if (self.stale_first_delegation && *count == 1)
                    || (self.stale_second_parent
                        && endpoint.ip() == IpAddr::V4(Ipv4Addr::new(8, 8, 4, 5)))
                {
                    vec!["old.example.net."]
                } else {
                    vec!["ns1.example.net.", "ns2.example.net."]
                };
                for nameserver in nameservers {
                    response.add_authority(Record::from_rdata(
                        owner.clone(),
                        60,
                        RData::NS(NS(Name::from_ascii(nameserver).unwrap())),
                    ));
                }
                &mut response
            }
            _ => &mut response,
        };
        Ok(DnsWireResponse {
            endpoint,
            protocol: DnsTransportProtocol::Udp,
            message: response,
        })
    }
}

fn public_request() -> DnsVerificationRequest {
    DnsVerificationRequest {
        request_id: DnsVerificationRequestId::new("public-1").unwrap(),
        binding: DnsVerificationBinding {
            zone: DnsZoneRef {
                provider_account_id: CloudResourceId::new("account").unwrap(),
                provider: CloudProvider::Aws,
                zone_id: DnsZoneId::new("public-zone").unwrap(),
                apex: AbsoluteDnsName::new("example.com").unwrap(),
                visibility: ZoneVisibility::Public,
            },
            zone_revision: ZoneLifecycleRevision::new("zone-r1").unwrap(),
            authoritative_nameservers: BTreeSet::from([
                AbsoluteDnsName::new("ns1.example.net").unwrap(),
                AbsoluteDnsName::new("ns2.example.net").unwrap(),
            ]),
            record_revision: Some(DnsRecordRevision::new("record-r1").unwrap()),
            provider_change_id: None,
        },
        scope: DnsVerificationScope::Public {
            authority_profile: profile_ref("public"),
            recursive_profiles: BTreeSet::from([profile_ref("public")]),
        },
        expectation: DnsRrsetExpectation::Present {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("www.example.com").unwrap(),
                record_type: ProviderDnsRecordType::A,
                routing: DnsRoutingIdentity::Simple,
            },
            values: BTreeSet::from([DnsRecordSetValue::A {
                address: Ipv4Addr::new(192, 0, 2, 10),
            }]),
        },
        dnssec: DnssecVerificationExpectation::NotRequested,
        policy: DnsVerificationPolicy {
            total_timeout_ms: 2_000,
            per_query_timeout_ms: 100,
            max_attempts: 1,
            max_queries: 32,
            retry_initial_ms: 1,
            retry_max_ms: 1,
            evidence_max_age_ms: 1_000,
        },
    }
}

#[tokio::test]
async fn public_verification_covers_every_expected_nameserver_and_exact_delegation() {
    let transport = Arc::new(PublicFakeTransport {
        questions: Mutex::new(Vec::new()),
        delegation_queries: Mutex::new(0),
        stale_first_delegation: false,
        stale_second_parent: false,
    });
    let endpoint = SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 53));
    let verifier = NetworkDnsPropagationVerifier::new(
        transport.clone(),
        [ResolverProfile {
            id: ResolverProfileId::new("public").unwrap(),
            revision: edgion_center_core::ResolverProfileRevision::new("r1").unwrap(),
            endpoints: BTreeSet::from([endpoint]),
            target_policy: DnsTargetPolicy::PublicDns,
            trust_authenticated_data: false,
        }],
    )
    .unwrap()
    .with_clock(Arc::new(FixedClock));

    let request = public_request();
    let evidence = verifier.verify(&request).await.unwrap();
    assert_eq!(evidence.authoritative.len(), 2);
    assert!(evidence.authoritative.iter().all(|check| matches!(
        check.outcome,
        edgion_center_core::DnsQueryOutcome::Match {
            authoritative: true,
            ..
        }
    )));
    assert_eq!(evidence.delegation.state, DelegationState::Delegated);
    assert_eq!(
        evidence.delegation.parent_nameservers,
        request.binding.authoritative_nameservers
    );
    let direct_names: BTreeSet<_> = transport
        .questions
        .lock()
        .unwrap()
        .iter()
        .filter(|question| !question.recursion_desired)
        .map(|question| question.name.clone())
        .collect();
    assert_eq!(
        direct_names,
        BTreeSet::from([
            "com.".into(),
            "example.com.".into(),
            "www.example.com.".into(),
        ])
    );
}

#[tokio::test]
async fn delegation_mismatch_is_retried_before_evidence_is_accepted() {
    let transport = Arc::new(PublicFakeTransport {
        questions: Mutex::new(Vec::new()),
        delegation_queries: Mutex::new(0),
        stale_first_delegation: true,
        stale_second_parent: false,
    });
    let endpoint = SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 53));
    let verifier = NetworkDnsPropagationVerifier::new(
        transport.clone(),
        [ResolverProfile {
            id: ResolverProfileId::new("public").unwrap(),
            revision: edgion_center_core::ResolverProfileRevision::new("r1").unwrap(),
            endpoints: BTreeSet::from([endpoint]),
            target_policy: DnsTargetPolicy::PublicDns,
            trust_authenticated_data: false,
        }],
    )
    .unwrap()
    .with_clock(Arc::new(FixedClock));
    let mut request = public_request();
    request.policy.max_attempts = 2;

    let evidence = verifier.verify(&request).await.unwrap();
    assert_eq!(evidence.delegation.state, DelegationState::Delegated);
    assert_eq!(*transport.delegation_queries.lock().unwrap(), 3);
}

#[tokio::test]
async fn one_stale_parent_nameserver_prevents_delegated_evidence() {
    let transport = Arc::new(PublicFakeTransport {
        questions: Mutex::new(Vec::new()),
        delegation_queries: Mutex::new(0),
        stale_first_delegation: false,
        stale_second_parent: true,
    });
    let endpoint = SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 53));
    let verifier = NetworkDnsPropagationVerifier::new(
        transport,
        [ResolverProfile {
            id: ResolverProfileId::new("public").unwrap(),
            revision: edgion_center_core::ResolverProfileRevision::new("r1").unwrap(),
            endpoints: BTreeSet::from([endpoint]),
            target_policy: DnsTargetPolicy::PublicDns,
            trust_authenticated_data: false,
        }],
    )
    .unwrap()
    .with_clock(Arc::new(FixedClock));

    let evidence = verifier.verify(&public_request()).await.unwrap();
    assert_eq!(evidence.delegation.state, DelegationState::Mismatch);
    assert!(evidence
        .to_zone_authority_evidence(&public_request(), evidence.completed_at_unix_ms)
        .is_err());
}

#[tokio::test]
async fn query_budget_exhaustion_is_explicit_and_never_ready() {
    let transport = Arc::new(PublicFakeTransport {
        questions: Mutex::new(Vec::new()),
        delegation_queries: Mutex::new(0),
        stale_first_delegation: false,
        stale_second_parent: false,
    });
    let endpoint = SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 53));
    let verifier = NetworkDnsPropagationVerifier::new(
        transport,
        [ResolverProfile {
            id: ResolverProfileId::new("public").unwrap(),
            revision: edgion_center_core::ResolverProfileRevision::new("r1").unwrap(),
            endpoints: BTreeSet::from([endpoint]),
            target_policy: DnsTargetPolicy::PublicDns,
            trust_authenticated_data: false,
        }],
    )
    .unwrap()
    .with_clock(Arc::new(FixedClock));
    let mut request = public_request();
    request.policy.max_queries = 3;

    let evidence = verifier.verify(&request).await.unwrap();
    assert!(evidence.budget.exhausted);
    assert!(evidence
        .to_zone_authority_evidence(&request, evidence.completed_at_unix_ms)
        .is_err());
}

#[tokio::test]
async fn timeout_is_returned_as_evidence_instead_of_a_trait_error() {
    let transport = Arc::new(FakeTransport {
        responses: Mutex::new(VecDeque::new()),
        questions: Mutex::new(Vec::new()),
    });
    let endpoint = SocketAddr::from((Ipv4Addr::LOCALHOST, 5300));
    let verifier =
        NetworkDnsPropagationVerifier::new(
            transport,
            [ResolverProfile {
                id: ResolverProfileId::new("private").unwrap(),
                revision: edgion_center_core::ResolverProfileRevision::new("r1").unwrap(),
                endpoints: BTreeSet::from([endpoint]),
                target_policy: DnsTargetPolicy::Explicit {
                    allowed_networks: vec![
                        IpNetwork::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 32).unwrap()
                    ],
                    allowed_ports: vec![5300],
                },
                trust_authenticated_data: false,
            }],
        )
        .unwrap()
        .with_clock(Arc::new(FixedClock));

    let evidence = verifier.verify(&private_request()).await.unwrap();
    assert!(matches!(
        evidence.recursive[0].outcome,
        edgion_center_core::DnsQueryOutcome::Timeout
    ));
}

fn private_request() -> DnsVerificationRequest {
    DnsVerificationRequest {
        request_id: DnsVerificationRequestId::new("private-1").unwrap(),
        binding: DnsVerificationBinding {
            zone: DnsZoneRef {
                provider_account_id: CloudResourceId::new("account").unwrap(),
                provider: CloudProvider::GoogleCloud,
                zone_id: DnsZoneId::new("private-zone").unwrap(),
                apex: edgion_center_core::AbsoluteDnsName::new("internal.example").unwrap(),
                visibility: ZoneVisibility::Private,
            },
            zone_revision: ZoneLifecycleRevision::new("zone-r1").unwrap(),
            authoritative_nameservers: BTreeSet::new(),
            record_revision: Some(DnsRecordRevision::new("record-r1").unwrap()),
            provider_change_id: None,
        },
        scope: DnsVerificationScope::Private {
            resolver_profile: profile_ref("private"),
        },
        expectation: DnsRrsetExpectation::Present {
            key: DnsRecordSetKey {
                owner: DnsOwnerName::new("app.internal.example").unwrap(),
                record_type: ProviderDnsRecordType::A,
                routing: DnsRoutingIdentity::Simple,
            },
            values: BTreeSet::from([DnsRecordSetValue::A {
                address: Ipv4Addr::new(10, 0, 0, 10),
            }]),
        },
        dnssec: DnssecVerificationExpectation::NotRequested,
        policy: DnsVerificationPolicy {
            total_timeout_ms: 1_000,
            per_query_timeout_ms: 100,
            max_attempts: 1,
            max_queries: 4,
            retry_initial_ms: 1,
            retry_max_ms: 1,
            evidence_max_age_ms: 1_000,
        },
    }
}

fn profile_ref(id: &str) -> edgion_center_core::ResolverProfileRef {
    edgion_center_core::ResolverProfileRef {
        id: ResolverProfileId::new(id).unwrap(),
        revision: edgion_center_core::ResolverProfileRevision::new("r1").unwrap(),
    }
}

fn answer_with_unrelated_record() -> Message {
    let name = Name::from_ascii("app.internal.example.").unwrap();
    let mut response = Message::response(1, OpCode::Query);
    response.add_query(Query::query(name.clone(), RecordType::A));
    response.add_answer(Record::from_rdata(
        name,
        60,
        RData::A(A(Ipv4Addr::new(10, 0, 0, 10))),
    ));
    response.add_answer(Record::from_rdata(
        Name::from_ascii("unrelated.internal.example.").unwrap(),
        60,
        RData::A(A(Ipv4Addr::new(10, 0, 0, 99))),
    ));
    response
}

#[tokio::test]
async fn private_profile_never_falls_back_to_bootstrap_or_delegation_queries() {
    private_request().validate().unwrap();
    let transport = Arc::new(FakeTransport {
        responses: Mutex::new(VecDeque::from([answer_with_unrelated_record()])),
        questions: Mutex::new(Vec::new()),
    });
    let endpoint = SocketAddr::from((Ipv4Addr::LOCALHOST, 5300));
    let profile = ResolverProfile {
        id: ResolverProfileId::new("private").unwrap(),
        revision: edgion_center_core::ResolverProfileRevision::new("r1").unwrap(),
        endpoints: BTreeSet::from([endpoint]),
        target_policy: DnsTargetPolicy::Explicit {
            allowed_networks: vec![IpNetwork::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 32).unwrap()],
            allowed_ports: vec![5300],
        },
        trust_authenticated_data: false,
    };
    let verifier = NetworkDnsPropagationVerifier::new(transport.clone(), [profile])
        .unwrap()
        .with_clock(Arc::new(FixedClock));

    let evidence = verifier.verify(&private_request()).await.unwrap();
    assert!(evidence.authoritative.is_empty());
    assert_eq!(evidence.recursive.len(), 1);
    assert!(matches!(
        evidence.recursive[0].outcome,
        edgion_center_core::DnsQueryOutcome::Match { ref values, .. }
            if values == &BTreeSet::from([DnsRecordSetValue::A { address: Ipv4Addr::new(10, 0, 0, 10) }])
    ));
    assert_eq!(transport.questions.lock().unwrap().len(), 1);
    assert_eq!(
        transport.questions.lock().unwrap()[0].name,
        "app.internal.example."
    );
}

#[test]
fn raw_dns_profile_cannot_claim_authenticated_data_trust() {
    let endpoint = SocketAddr::from((Ipv4Addr::LOCALHOST, 5300));
    let result = NetworkDnsPropagationVerifier::new(
        Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::new()),
            questions: Mutex::new(Vec::new()),
        }),
        [ResolverProfile {
            id: ResolverProfileId::new("private").unwrap(),
            revision: edgion_center_core::ResolverProfileRevision::new("r1").unwrap(),
            endpoints: BTreeSet::from([endpoint]),
            target_policy: DnsTargetPolicy::Explicit {
                allowed_networks: vec![IpNetwork::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 32).unwrap()],
                allowed_ports: vec![5300],
            },
            trust_authenticated_data: true,
        }],
    );
    assert!(result.is_err());
}
