use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use edgion_center_core::{
    AbsoluteDnsName, DelegationObservation, DelegationState, DnsCharacterString,
    DnsPropagationVerifier, DnsQueryOutcome, DnsRecordSetKey, DnsRecordSetValue,
    DnsRrsetExpectation, DnsTxtValue, DnsVerificationBudgetUse, DnsVerificationError,
    DnsVerificationErrorKind, DnsVerificationEvidence, DnsVerificationRequest,
    DnsVerificationResult, DnsVerificationScope, DnssecEvidenceSource, DnssecValidationState,
    DnssecVerificationEvidence, DnssecVerificationExpectation, NameserverCheck,
    ProviderDnsRecordType, RecursiveResolverCheck, ResolverProfileId, ResolverProfileRef,
    ResolverProfileRevision, SanitizedDnsFailureCode,
};
use hickory_proto::{
    op::{Message, ResponseCode},
    rr::{DNSClass, RData, RecordType},
};
use tokio::time::{sleep, timeout};

use crate::{
    DnsQueryTransport, DnsQuestion, DnsTargetPolicy, DnsTransportError, LocalDnssecSecurity,
    LocalDnssecValidator, LocalParentDsValidation,
};

const MAX_PROFILE_ENDPOINTS: usize = 8;
const MAX_NAMESERVER_ADDRESSES: usize = 8;

#[derive(Debug, Clone)]
pub struct ResolverProfile {
    pub id: ResolverProfileId,
    pub revision: ResolverProfileRevision,
    pub endpoints: BTreeSet<SocketAddr>,
    pub target_policy: DnsTargetPolicy,
    /// AD is accepted only as evidence asserted by this configured resolver.
    /// It is never converted to local DNSSEC chain validation.
    pub trust_authenticated_data: bool,
}

impl ResolverProfile {
    fn validate(&self) -> Result<(), DnsVerificationError> {
        if self.endpoints.is_empty()
            || self.endpoints.len() > MAX_PROFILE_ENDPOINTS
            || self.trust_authenticated_data
            || self
                .endpoints
                .iter()
                .any(|endpoint| !self.target_policy.permits(*endpoint))
        {
            return Err(error(
                DnsVerificationErrorKind::InvalidRequest,
                "invalid_resolver_profile",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsVerificationMetricResult {
    Match,
    Mismatch,
    Timeout,
    Failure,
    UnsafeAddress,
    BudgetExhausted,
}

pub trait DnsVerificationMetrics: Send + Sync {
    fn record_query(&self, result: DnsVerificationMetricResult, duration: Duration);
    fn record_verification(&self, successful: bool, duration: Duration);
}

#[derive(Debug, Default)]
pub struct NoopDnsVerificationMetrics;

impl DnsVerificationMetrics for NoopDnsVerificationMetrics {
    fn record_query(&self, _result: DnsVerificationMetricResult, _duration: Duration) {}

    fn record_verification(&self, _successful: bool, _duration: Duration) {}
}

pub trait DnsVerificationClock: Send + Sync {
    fn now_unix_ms(&self) -> i64;
}

#[derive(Debug, Default)]
pub struct SystemDnsVerificationClock;

impl DnsVerificationClock for SystemDnsVerificationClock {
    fn now_unix_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(i64::MAX)
    }
}

pub struct NetworkDnsPropagationVerifier {
    transport: Arc<dyn DnsQueryTransport>,
    profiles: BTreeMap<ResolverProfileId, ResolverProfile>,
    clock: Arc<dyn DnsVerificationClock>,
    metrics: Arc<dyn DnsVerificationMetrics>,
}

impl NetworkDnsPropagationVerifier {
    pub fn new(
        transport: Arc<dyn DnsQueryTransport>,
        profiles: impl IntoIterator<Item = ResolverProfile>,
    ) -> DnsVerificationResult<Self> {
        let mut by_id = BTreeMap::new();
        for profile in profiles {
            profile.validate()?;
            if by_id.insert(profile.id.clone(), profile).is_some() {
                return Err(error(
                    DnsVerificationErrorKind::InvalidRequest,
                    "duplicate_resolver_profile",
                ));
            }
        }
        Ok(Self {
            transport,
            profiles: by_id,
            clock: Arc::new(SystemDnsVerificationClock),
            metrics: Arc::new(NoopDnsVerificationMetrics),
        })
    }

    pub fn with_clock(mut self, clock: Arc<dyn DnsVerificationClock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_metrics(mut self, metrics: Arc<dyn DnsVerificationMetrics>) -> Self {
        self.metrics = metrics;
        self
    }

    fn profile(&self, id: &ResolverProfileId) -> DnsVerificationResult<&ResolverProfile> {
        self.profiles.get(id).ok_or_else(|| {
            error(
                DnsVerificationErrorKind::UnknownResolverProfile,
                "resolver_profile_not_found",
            )
        })
    }

    fn referenced_profile(
        &self,
        reference: &ResolverProfileRef,
    ) -> DnsVerificationResult<&ResolverProfile> {
        let profile = self.profile(&reference.id)?;
        if profile.revision != reference.revision {
            return Err(error(
                DnsVerificationErrorKind::UnknownResolverProfile,
                "resolver_profile_revision_mismatch",
            ));
        }
        Ok(profile)
    }

    fn authority_profile(
        &self,
        request: &DnsVerificationRequest,
    ) -> DnsVerificationResult<&ResolverProfile> {
        let reference = authority_profile_ref(&request.scope).ok_or_else(|| {
            error(
                DnsVerificationErrorKind::InvalidRequest,
                "authority_profile_missing",
            )
        })?;
        self.referenced_profile(reference)
    }

    async fn exchange(
        &self,
        endpoint: SocketAddr,
        policy: &DnsTargetPolicy,
        question: &DnsQuestion,
        request: &DnsVerificationRequest,
        budget: &mut Budget,
    ) -> DnsQueryOutcome {
        if !budget.reserve_query(request) {
            return DnsQueryOutcome::BudgetExhausted;
        }
        let Some(remaining) = budget.remaining() else {
            return DnsQueryOutcome::BudgetExhausted;
        };
        let exchange_timeout =
            Duration::from_millis(request.policy.per_query_timeout_ms).min(remaining);
        let result = timeout(
            exchange_timeout,
            self.transport
                .query(endpoint, policy, question, exchange_timeout),
        )
        .await
        .map_err(|_| DnsTransportError::Timeout)
        .and_then(|result| result);
        match result {
            Ok(response) => {
                let outcome =
                    message_outcome(&response.message, &question.name, question.record_type);
                budget.last_message = Some(response.message);
                outcome
            }
            Err(DnsTransportError::TargetDenied) => DnsQueryOutcome::UnsafeAddress,
            Err(DnsTransportError::Timeout) => DnsQueryOutcome::Timeout,
            Err(error) => DnsQueryOutcome::TransportFailure {
                code: failure_code(error),
            },
        }
    }

    async fn query_profile(
        &self,
        profile: &ResolverProfile,
        question: &DnsQuestion,
        request: &DnsVerificationRequest,
        budget: &mut Budget,
        goal: ProfileQueryGoal<'_>,
    ) -> (DnsQueryOutcome, bool, u8) {
        let mut last = DnsQueryOutcome::Timeout;
        let authenticated = false;
        let mut attempts = 0;
        for attempt in 1..=request.policy.max_attempts {
            attempts = attempt;
            for endpoint in &profile.endpoints {
                let started = Instant::now();
                let outcome = self
                    .exchange(*endpoint, &profile.target_policy, question, request, budget)
                    .await;
                last = outcome;
                self.metrics.record_query(
                    if goal.satisfied(&last) {
                        DnsVerificationMetricResult::Match
                    } else if matches!(
                        last,
                        DnsQueryOutcome::Match { .. }
                            | DnsQueryOutcome::NoData { .. }
                            | DnsQueryOutcome::NxDomain { .. }
                    ) {
                        DnsVerificationMetricResult::Mismatch
                    } else {
                        metric_result(&last)
                    },
                    started.elapsed(),
                );
                if goal.satisfied(&last) || matches!(last, DnsQueryOutcome::UnsafeAddress) {
                    return (last, authenticated, attempts);
                }
            }
            if attempt < request.policy.max_attempts && !budget.exhausted {
                budget.sleep_backoff(request, attempt).await;
            }
        }
        (last, authenticated, attempts)
    }

    async fn resolve_nameserver(
        &self,
        nameserver: &AbsoluteDnsName,
        request: &DnsVerificationRequest,
        budget: &mut Budget,
    ) -> DnsVerificationResult<BTreeSet<IpAddr>> {
        let profile = self.authority_profile(request)?;
        let mut addresses = BTreeSet::new();
        for record_type in [RecordType::A, RecordType::AAAA] {
            let question = DnsQuestion::recursive(fqdn(nameserver.as_str()), record_type);
            for endpoint in &profile.endpoints {
                if !budget.reserve_query(request) {
                    break;
                }
                let Some(remaining) = budget.remaining() else {
                    break;
                };
                let started = Instant::now();
                let before = addresses.len();
                let exchange_timeout =
                    Duration::from_millis(request.policy.per_query_timeout_ms).min(remaining);
                match timeout(
                    exchange_timeout,
                    self.transport.query(
                        *endpoint,
                        &profile.target_policy,
                        &question,
                        exchange_timeout,
                    ),
                )
                .await
                .map_err(|_| DnsTransportError::Timeout)
                .and_then(|result| result)
                {
                    Ok(response) => {
                        addresses.extend(resolved_addresses(
                            &response.message,
                            nameserver.as_str(),
                            record_type,
                        ));
                        self.metrics.record_query(
                            if addresses.len() > before {
                                DnsVerificationMetricResult::Match
                            } else {
                                DnsVerificationMetricResult::Failure
                            },
                            started.elapsed(),
                        );
                        if !addresses.is_empty() {
                            break;
                        }
                    }
                    Err(error) => self.metrics.record_query(
                        if error == DnsTransportError::Timeout {
                            DnsVerificationMetricResult::Timeout
                        } else {
                            DnsVerificationMetricResult::Failure
                        },
                        started.elapsed(),
                    ),
                }
            }
        }
        Ok(addresses
            .into_iter()
            .take(MAX_NAMESERVER_ADDRESSES)
            .collect())
    }

    async fn authoritative_check(
        &self,
        nameserver: &AbsoluteDnsName,
        request: &DnsVerificationRequest,
        budget: &mut Budget,
    ) -> DnsVerificationResult<NameserverCheck> {
        let addresses = self.resolve_nameserver(nameserver, request, budget).await?;
        let mut outcome = if budget.exhausted {
            DnsQueryOutcome::BudgetExhausted
        } else if addresses.is_empty() {
            DnsQueryOutcome::TransportFailure {
                code: SanitizedDnsFailureCode::new("nameserver_resolution_failed").unwrap(),
            }
        } else {
            DnsQueryOutcome::UnsafeAddress
        };
        let mut attempts = 1;
        let question = expectation_question(&request.expectation, false);
        for address in addresses {
            let endpoint = SocketAddr::new(address, 53);
            if !DnsTargetPolicy::PublicDns.permits(endpoint) {
                continue;
            }
            for attempt in 1..=request.policy.max_attempts {
                attempts = attempt;
                let started = Instant::now();
                outcome = self
                    .exchange(
                        endpoint,
                        &DnsTargetPolicy::PublicDns,
                        &question,
                        request,
                        budget,
                    )
                    .await;
                outcome = normalize_outcome(outcome, &request.expectation);
                self.metrics.record_query(
                    if outcome_satisfies(&outcome, &request.expectation, true) {
                        DnsVerificationMetricResult::Match
                    } else {
                        metric_result(&outcome)
                    },
                    started.elapsed(),
                );
                if outcome_satisfies(&outcome, &request.expectation, true)
                    || matches!(outcome, DnsQueryOutcome::UnsafeAddress)
                {
                    break;
                }
                if attempt < request.policy.max_attempts && !budget.exhausted {
                    budget.sleep_backoff(request, attempt).await;
                }
            }
            if outcome_satisfies(&outcome, &request.expectation, true) {
                break;
            }
        }
        Ok(NameserverCheck {
            nameserver: nameserver.clone(),
            attempts,
            outcome,
        })
    }

    async fn verify_public_parent_delegation(
        &self,
        request: &DnsVerificationRequest,
        budget: &mut Budget,
    ) -> DnsVerificationResult<DelegationObservation> {
        let checked_at = Some(self.clock.now_unix_ms().to_string());
        let bootstrap = self.authority_profile(request)?;
        let Some(candidate) = immediate_parent(request.binding.zone.apex.as_str()) else {
            return Ok(delegation_failure(
                request,
                checked_at,
                "parent_zone_unavailable",
            ));
        };
        let discovery = DnsQuestion::recursive(fqdn(candidate), RecordType::SOA);
        let mut parent_apex = None;
        for endpoint in &bootstrap.endpoints {
            let _ = self
                .exchange(
                    *endpoint,
                    &bootstrap.target_policy,
                    &discovery,
                    request,
                    budget,
                )
                .await;
            parent_apex = budget
                .last_message
                .as_ref()
                .and_then(soa_owner)
                .filter(|owner| is_same_or_ancestor(candidate, owner.as_str()));
            if parent_apex.is_some() {
                break;
            }
        }
        let Some(parent_apex) = parent_apex else {
            return Ok(delegation_failure(
                request,
                checked_at,
                "parent_soa_discovery_failed",
            ));
        };
        let ns_discovery = DnsQuestion::recursive(fqdn(parent_apex.as_str()), RecordType::NS);
        let mut parent_servers = BTreeSet::new();
        for endpoint in &bootstrap.endpoints {
            let _ = self
                .exchange(
                    *endpoint,
                    &bootstrap.target_policy,
                    &ns_discovery,
                    request,
                    budget,
                )
                .await;
            if let Some(message) = budget
                .last_message
                .as_ref()
                .filter(|message| message.metadata.response_code == ResponseCode::NoError)
            {
                parent_servers.extend(ns_records(message.answers.iter(), parent_apex.as_str()));
            }
            if !parent_servers.is_empty() {
                break;
            }
        }
        if parent_servers.is_empty() {
            return Ok(delegation_failure(
                request,
                checked_at,
                "parent_nameserver_discovery_failed",
            ));
        }
        self.verify_delegation_from_authorities(
            request,
            budget,
            &parent_apex,
            parent_servers,
            &request.binding.zone.apex,
            &request.binding.authoritative_nameservers,
            checked_at,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn verify_delegation_from_authorities(
        &self,
        request: &DnsVerificationRequest,
        budget: &mut Budget,
        authority_apex: &AbsoluteDnsName,
        authority_servers: BTreeSet<AbsoluteDnsName>,
        delegated_apex: &AbsoluteDnsName,
        expected: &BTreeSet<AbsoluteDnsName>,
        checked_at: Option<String>,
    ) -> DnsVerificationResult<DelegationObservation> {
        let soa_question =
            DnsQuestion::authoritative(fqdn(authority_apex.as_str()), RecordType::SOA);
        let ns_question = DnsQuestion::authoritative(fqdn(delegated_apex.as_str()), RecordType::NS);
        let mut observed = BTreeSet::new();
        for server in authority_servers {
            let mut server_authenticated = false;
            let mut server_exact = false;
            for address in self.resolve_nameserver(&server, request, budget).await? {
                let endpoint = SocketAddr::new(address, 53);
                if !DnsTargetPolicy::PublicDns.permits(endpoint) {
                    continue;
                }
                let _ = self
                    .exchange(
                        endpoint,
                        &DnsTargetPolicy::PublicDns,
                        &soa_question,
                        request,
                        budget,
                    )
                    .await;
                if !budget.last_message.as_ref().is_some_and(|message| {
                    message.metadata.authoritative
                        && soa_owner(message).as_ref() == Some(authority_apex)
                }) {
                    continue;
                }
                server_authenticated = true;
                for attempt in 1..=request.policy.max_attempts {
                    let _ = self
                        .exchange(
                            endpoint,
                            &DnsTargetPolicy::PublicDns,
                            &ns_question,
                            request,
                            budget,
                        )
                        .await;
                    if let Some(message) = budget
                        .last_message
                        .as_ref()
                        .filter(|message| message.metadata.response_code == ResponseCode::NoError)
                    {
                        observed = ns_records(
                            message.answers.iter().chain(message.authorities.iter()),
                            delegated_apex.as_str(),
                        );
                        if &observed == expected {
                            server_exact = true;
                            break;
                        }
                    }
                    if attempt < request.policy.max_attempts && !budget.exhausted {
                        budget.sleep_backoff(request, attempt).await;
                    }
                }
                if server_exact {
                    break;
                }
            }
            if !server_authenticated || !server_exact {
                return Ok(DelegationObservation {
                    state: if !server_authenticated {
                        DelegationState::CheckFailed
                    } else if observed.is_empty() {
                        DelegationState::Absent
                    } else {
                        DelegationState::Mismatch
                    },
                    expected_nameservers: expected.clone(),
                    parent_nameservers: observed,
                    checked_at,
                    failure: Some(if server_authenticated {
                        "parent_nameserver_delegation_mismatch".into()
                    } else {
                        "parent_nameserver_authority_unverified".into()
                    }),
                });
            }
        }
        Ok(DelegationObservation {
            state: DelegationState::Delegated,
            expected_nameservers: expected.clone(),
            parent_nameservers: expected.clone(),
            checked_at,
            failure: None,
        })
    }
}

#[async_trait]
impl DnsPropagationVerifier for NetworkDnsPropagationVerifier {
    async fn verify(
        &self,
        request: &DnsVerificationRequest,
    ) -> DnsVerificationResult<DnsVerificationEvidence> {
        request.validate().map_err(|_| {
            error(
                DnsVerificationErrorKind::InvalidRequest,
                "invalid_dns_verification_request",
            )
        })?;
        let started_at_unix_ms = self.clock.now_unix_ms();
        let started = Instant::now();
        let future = async {
            let mut budget = Budget::new(Duration::from_millis(request.policy.total_timeout_ms));
            let mut authoritative = Vec::new();
            if matches!(
                request.scope,
                DnsVerificationScope::Public { .. }
                    | DnsVerificationScope::DelegatedValidation { .. }
            ) {
                for nameserver in authoritative_nameservers(request) {
                    authoritative.push(
                        self.authoritative_check(nameserver, request, &mut budget)
                            .await?,
                    );
                }
            }

            let mut recursive = Vec::new();
            for profile_ref in scope_profiles(&request.scope) {
                let profile = self.referenced_profile(&profile_ref)?;
                let (outcome, _, attempts) = self
                    .query_profile(
                        profile,
                        &expectation_question(&request.expectation, true),
                        request,
                        &mut budget,
                        ProfileQueryGoal::Expectation(&request.expectation),
                    )
                    .await;
                let outcome = normalize_outcome(outcome, &request.expectation);
                recursive.push(RecursiveResolverCheck {
                    profile: profile_ref,
                    attempts,
                    outcome,
                });
            }

            let checks_public_authority = matches!(
                request.scope,
                DnsVerificationScope::Public { .. }
                    | DnsVerificationScope::DelegatedValidation { .. }
            );
            let (delegation, dnssec) = if !checks_public_authority {
                (
                    DelegationObservation {
                        state: DelegationState::NotApplicable,
                        expected_nameservers: BTreeSet::new(),
                        parent_nameservers: BTreeSet::new(),
                        checked_at: Some(self.clock.now_unix_ms().to_string()),
                        failure: None,
                    },
                    DnssecVerificationEvidence {
                        observed_parent_ds: Vec::new(),
                        parent_source: DnssecEvidenceSource::NotChecked,
                        parent_soa_present_for_absence: false,
                        validation: if matches!(
                            request.dnssec,
                            DnssecVerificationExpectation::NotRequested
                        ) {
                            DnssecValidationState::NotRequested
                        } else {
                            DnssecValidationState::Indeterminate
                        },
                        validator_profile: None,
                    },
                )
            } else {
                let bootstrap = self.authority_profile(request)?;
                let delegation = match &request.scope {
                    DnsVerificationScope::Public { .. } => {
                        self.verify_public_parent_delegation(request, &mut budget)
                            .await?
                    }
                    DnsVerificationScope::DelegatedValidation {
                        child_apex,
                        child_nameservers,
                        ..
                    } => {
                        self.verify_delegation_from_authorities(
                            request,
                            &mut budget,
                            &request.binding.zone.apex,
                            request.binding.authoritative_nameservers.clone(),
                            child_apex,
                            child_nameservers,
                            Some(self.clock.now_unix_ms().to_string()),
                        )
                        .await?
                    }
                    _ => unreachable!("private scopes were handled above"),
                };

                let dnssec =
                    if matches!(request.dnssec, DnssecVerificationExpectation::NotRequested) {
                        DnssecVerificationEvidence {
                            observed_parent_ds: Vec::new(),
                            parent_source: DnssecEvidenceSource::NotChecked,
                            parent_soa_present_for_absence: false,
                            validation: DnssecValidationState::NotRequested,
                            validator_profile: None,
                        }
                    } else {
                        if !budget.reserve_query(request) || !budget.reserve_query(request) {
                            DnssecVerificationEvidence {
                                observed_parent_ds: Vec::new(),
                                parent_source: DnssecEvidenceSource::NotChecked,
                                parent_soa_present_for_absence: false,
                                validation: DnssecValidationState::Indeterminate,
                                validator_profile: authority_profile_ref(&request.scope).cloned(),
                            }
                        } else {
                            let remaining = budget.remaining();
                            match remaining.and_then(|remaining| {
                                LocalDnssecValidator::from_profile(
                                    bootstrap,
                                    Duration::from_millis(request.policy.per_query_timeout_ms)
                                        .min(remaining),
                                )
                                .ok()
                                .map(|validator| (validator, remaining))
                            }) {
                                Some((validator, remaining)) => {
                                    let result =
                                        timeout(remaining, validator.validate(request)).await.ok();
                                    let mut evidence = result.map(dnssec_evidence).unwrap_or(
                                        DnssecVerificationEvidence {
                                            observed_parent_ds: Vec::new(),
                                            parent_source: DnssecEvidenceSource::NotChecked,
                                            parent_soa_present_for_absence: false,
                                            validation: DnssecValidationState::Indeterminate,
                                            validator_profile: None,
                                        },
                                    );
                                    evidence.validator_profile =
                                        authority_profile_ref(&request.scope).cloned();
                                    evidence
                                }
                                None => DnssecVerificationEvidence {
                                    observed_parent_ds: Vec::new(),
                                    parent_source: DnssecEvidenceSource::NotChecked,
                                    parent_soa_present_for_absence: false,
                                    validation: DnssecValidationState::Indeterminate,
                                    validator_profile: authority_profile_ref(&request.scope)
                                        .cloned(),
                                },
                            }
                        }
                    };
                (delegation, dnssec)
            };
            let elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
            if elapsed_ms >= request.policy.total_timeout_ms {
                budget.exhausted = true;
            }
            budget.elapsed_ms = elapsed_ms.min(request.policy.total_timeout_ms);
            Ok(DnsVerificationEvidence {
                request_id: request.request_id.clone(),
                binding: request.binding.clone(),
                expectation: request.expectation.clone(),
                started_at_unix_ms,
                completed_at_unix_ms: self.clock.now_unix_ms(),
                authority_profile: authority_profile_ref(&request.scope).cloned(),
                authoritative,
                recursive,
                delegation,
                dnssec,
                budget: DnsVerificationBudgetUse {
                    queries: budget.queries,
                    attempts: budget.queries,
                    elapsed_ms: budget.elapsed_ms,
                    exhausted: budget.exhausted,
                },
            })
        };
        let result = future.await;
        let successful = result.as_ref().is_ok_and(|evidence| match &request.scope {
            DnsVerificationScope::Public { .. } => evidence
                .to_zone_authority_evidence(request, evidence.completed_at_unix_ms)
                .is_ok(),
            DnsVerificationScope::DelegatedValidation {
                child_nameservers, ..
            } => {
                evidence
                    .validate_against(request, evidence.completed_at_unix_ms)
                    .is_ok()
                    && !evidence.budget.exhausted
                    && evidence.authoritative_published(request)
                    && evidence.recursive_visible(request)
                    && evidence.delegation.state == DelegationState::Delegated
                    && evidence.delegation.expected_nameservers == *child_nameservers
                    && evidence.delegation.parent_nameservers == *child_nameservers
                    && dnssec_satisfies(&evidence.dnssec, &request.dnssec)
            }
            DnsVerificationScope::Private { .. } | DnsVerificationScope::SplitHorizon { .. } => {
                !evidence.budget.exhausted
                    && matches!(request.dnssec, DnssecVerificationExpectation::NotRequested)
                    && evidence
                        .recursive
                        .iter()
                        .all(|check| outcome_satisfies(&check.outcome, &request.expectation, false))
            }
        });
        self.metrics
            .record_verification(successful, started.elapsed());
        result
    }
}

struct Budget {
    queries: u16,
    elapsed_ms: u64,
    exhausted: bool,
    // Reserved for future detailed response evidence. Kept private so AD can
    // never escape without being tied to an explicit resolver profile.
    last_message: Option<Message>,
    deadline: Instant,
}

impl Budget {
    fn new(total: Duration) -> Self {
        Self {
            queries: 0,
            elapsed_ms: 0,
            exhausted: false,
            last_message: None,
            deadline: Instant::now() + total,
        }
    }

    fn remaining(&mut self) -> Option<Duration> {
        let remaining = self.deadline.checked_duration_since(Instant::now());
        if remaining.is_none() || remaining == Some(Duration::ZERO) {
            self.exhausted = true;
            None
        } else {
            remaining
        }
    }

    fn reserve_query(&mut self, request: &DnsVerificationRequest) -> bool {
        if self.queries >= request.policy.max_queries || self.remaining().is_none() {
            self.exhausted = true;
            return false;
        }
        self.queries += 1;
        self.last_message = None;
        true
    }

    async fn sleep_backoff(&mut self, request: &DnsVerificationRequest, attempt: u8) {
        let requested = backoff(request, attempt);
        let Some(remaining) = self.remaining() else {
            return;
        };
        if requested >= remaining {
            self.exhausted = true;
            return;
        }
        sleep(requested).await;
    }
}

fn expectation_question(expectation: &DnsRrsetExpectation, recursive: bool) -> DnsQuestion {
    let key = expectation.key();
    let name = fqdn(&key.owner.to_string());
    let record_type = record_type(key);
    if recursive {
        DnsQuestion::recursive(name, record_type)
    } else {
        DnsQuestion::authoritative(name, record_type)
    }
}

fn record_type(key: &DnsRecordSetKey) -> RecordType {
    match key.record_type {
        ProviderDnsRecordType::A => RecordType::A,
        ProviderDnsRecordType::Aaaa => RecordType::AAAA,
        ProviderDnsRecordType::Cname => RecordType::CNAME,
        ProviderDnsRecordType::Txt => RecordType::TXT,
        _ => unreachable!("request validation rejects non-portable verification types"),
    }
}

fn message_outcome(
    message: &Message,
    expected_owner: &str,
    record_type: RecordType,
) -> DnsQueryOutcome {
    match message.metadata.response_code {
        ResponseCode::NXDomain => DnsQueryOutcome::NxDomain {
            authoritative: message.metadata.authoritative,
            soa_present: has_soa(message),
        },
        ResponseCode::ServFail => DnsQueryOutcome::ServFail,
        ResponseCode::Refused => DnsQueryOutcome::Refused,
        ResponseCode::NoError => {
            let values = values(message, expected_owner, record_type);
            if values.is_empty() {
                DnsQueryOutcome::NoData {
                    authoritative: message.metadata.authoritative,
                    soa_present: has_soa(message),
                }
            } else {
                let ttl_seconds = message
                    .answers
                    .iter()
                    .filter(|record| {
                        record.record_type() == record_type
                            && record.dns_class == DNSClass::IN
                            && record.name.to_ascii() == expected_owner
                    })
                    .map(|record| record.ttl)
                    .min();
                DnsQueryOutcome::Match {
                    values,
                    ttl_seconds,
                    authoritative: message.metadata.authoritative,
                }
            }
        }
        _ => DnsQueryOutcome::TransportFailure {
            code: SanitizedDnsFailureCode::new("dns_response_code").unwrap(),
        },
    }
}

fn values(
    message: &Message,
    expected_owner: &str,
    record_type: RecordType,
) -> BTreeSet<DnsRecordSetValue> {
    message
        .answers
        .iter()
        .filter(|record| {
            record.record_type() == record_type
                && record.dns_class == DNSClass::IN
                && record.name.to_ascii() == expected_owner
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
            RData::NS(value) => AbsoluteDnsName::new(trim_dot(&value.0.to_ascii()))
                .ok()
                .map(|target| DnsRecordSetValue::Ns { target }),
            _ => None,
        })
        .collect()
}

fn dnssec_evidence(result: crate::LocalDnssecValidation) -> DnssecVerificationEvidence {
    let satisfies = result.satisfies_expectation();
    let (observed_parent_ds, parent_soa_present_for_absence) = match result.parent_ds {
        LocalParentDsValidation::Match { observed }
        | LocalParentDsValidation::Mismatch { observed }
        | LocalParentDsValidation::Unexpected { observed } => (observed, false),
        LocalParentDsValidation::AuthenticatedAbsent => (Vec::new(), true),
        _ => (Vec::new(), false),
    };
    let validation = match result.security {
        LocalDnssecSecurity::Secure => DnssecValidationState::SecureLocalChain,
        LocalDnssecSecurity::Insecure => DnssecValidationState::Insecure,
        LocalDnssecSecurity::Bogus => DnssecValidationState::Bogus,
        LocalDnssecSecurity::Indeterminate => DnssecValidationState::Indeterminate,
    };
    DnssecVerificationEvidence {
        observed_parent_ds,
        // This source asserts local root-chain validation, never resolver AD
        // and never a direct query to the parent authority.
        parent_source: if satisfies {
            DnssecEvidenceSource::LocallyValidatedRecursive
        } else {
            DnssecEvidenceSource::NotChecked
        },
        parent_soa_present_for_absence,
        validation,
        validator_profile: None,
    }
}

fn immediate_parent(name: &str) -> Option<&str> {
    name.split_once('.').map(|(_, parent)| parent)
}

fn soa_owner(message: &Message) -> Option<AbsoluteDnsName> {
    if message.metadata.response_code != ResponseCode::NoError {
        return None;
    }
    message
        .answers
        .iter()
        .find(|record| record.record_type() == RecordType::SOA && record.dns_class == DNSClass::IN)
        .and_then(|record| AbsoluteDnsName::new(trim_dot(&record.name.to_ascii())).ok())
}

fn ns_records<'a>(
    records: impl Iterator<Item = &'a hickory_proto::rr::Record>,
    owner: &str,
) -> BTreeSet<AbsoluteDnsName> {
    let owner = fqdn(owner);
    records
        .filter(|record| record.name.to_ascii() == owner && record.dns_class == DNSClass::IN)
        .filter_map(|record| match &record.data {
            RData::NS(value) => AbsoluteDnsName::new(trim_dot(&value.0.to_ascii())).ok(),
            _ => None,
        })
        .collect()
}

fn is_same_or_ancestor(name: &str, possible_ancestor: &str) -> bool {
    name == possible_ancestor || name.ends_with(&format!(".{possible_ancestor}"))
}

fn delegation_failure(
    request: &DnsVerificationRequest,
    checked_at: Option<String>,
    code: &'static str,
) -> DelegationObservation {
    DelegationObservation {
        state: DelegationState::CheckFailed,
        expected_nameservers: request.binding.authoritative_nameservers.clone(),
        parent_nameservers: BTreeSet::new(),
        checked_at,
        failure: Some(code.into()),
    }
}

fn has_soa(message: &Message) -> bool {
    message
        .authorities
        .iter()
        .any(|record| record.record_type() == RecordType::SOA && record.dns_class == DNSClass::IN)
}

fn resolved_addresses(message: &Message, owner: &str, record_type: RecordType) -> BTreeSet<IpAddr> {
    if message.metadata.response_code != ResponseCode::NoError {
        return BTreeSet::new();
    }
    let owner = fqdn(owner);
    message
        .answers
        .iter()
        .filter(|record| {
            record.name.to_ascii() == owner
                && record.dns_class == DNSClass::IN
                && record.record_type() == record_type
        })
        .filter_map(|record| match (&record.data, record_type) {
            (RData::A(value), RecordType::A) => Some(IpAddr::V4(value.0)),
            (RData::AAAA(value), RecordType::AAAA) => Some(IpAddr::V6(value.0)),
            _ => None,
        })
        .collect()
}

fn outcome_satisfies(
    outcome: &DnsQueryOutcome,
    expectation: &DnsRrsetExpectation,
    authoritative: bool,
) -> bool {
    match (expectation, outcome) {
        (
            DnsRrsetExpectation::Present {
                values: expected, ..
            },
            DnsQueryOutcome::Match {
                values,
                authoritative: answer_authoritative,
                ..
            },
        ) => values == expected && (!authoritative || *answer_authoritative),
        (
            DnsRrsetExpectation::Absent { .. },
            DnsQueryOutcome::NoData {
                authoritative: answer_authoritative,
                soa_present: true,
            }
            | DnsQueryOutcome::NxDomain {
                authoritative: answer_authoritative,
                soa_present: true,
            },
        ) => !authoritative || *answer_authoritative,
        _ => false,
    }
}

fn normalize_outcome(
    outcome: DnsQueryOutcome,
    expectation: &DnsRrsetExpectation,
) -> DnsQueryOutcome {
    if outcome_satisfies(&outcome, expectation, false) {
        return outcome;
    }
    match outcome {
        DnsQueryOutcome::Match { values, .. } => DnsQueryOutcome::Mismatch { values },
        DnsQueryOutcome::NoData { .. } | DnsQueryOutcome::NxDomain { .. } => {
            DnsQueryOutcome::Mismatch {
                values: BTreeSet::new(),
            }
        }
        other => other,
    }
}

#[derive(Clone, Copy)]
enum ProfileQueryGoal<'a> {
    Expectation(&'a DnsRrsetExpectation),
}

impl ProfileQueryGoal<'_> {
    fn satisfied(self, outcome: &DnsQueryOutcome) -> bool {
        match self {
            Self::Expectation(expectation) => outcome_satisfies(outcome, expectation, false),
        }
    }
}

fn scope_profiles(scope: &DnsVerificationScope) -> BTreeSet<ResolverProfileRef> {
    match scope {
        DnsVerificationScope::Public {
            recursive_profiles, ..
        }
        | DnsVerificationScope::DelegatedValidation {
            recursive_profiles, ..
        } => recursive_profiles.clone(),
        DnsVerificationScope::Private { resolver_profile } => {
            BTreeSet::from([resolver_profile.clone()])
        }
        DnsVerificationScope::SplitHorizon { view } => BTreeSet::from([view.clone()]),
    }
}

fn authority_profile_ref(scope: &DnsVerificationScope) -> Option<&ResolverProfileRef> {
    match scope {
        DnsVerificationScope::Public {
            authority_profile, ..
        }
        | DnsVerificationScope::DelegatedValidation {
            authority_profile, ..
        } => Some(authority_profile),
        DnsVerificationScope::Private { .. } | DnsVerificationScope::SplitHorizon { .. } => None,
    }
}

fn authoritative_nameservers(request: &DnsVerificationRequest) -> &BTreeSet<AbsoluteDnsName> {
    match &request.scope {
        DnsVerificationScope::DelegatedValidation {
            child_nameservers, ..
        } => child_nameservers,
        _ => &request.binding.authoritative_nameservers,
    }
}

fn dnssec_satisfies(
    evidence: &DnssecVerificationEvidence,
    expectation: &DnssecVerificationExpectation,
) -> bool {
    match expectation {
        DnssecVerificationExpectation::NotRequested => {
            evidence.validation == DnssecValidationState::NotRequested
                && evidence.validator_profile.is_none()
        }
        DnssecVerificationExpectation::Unsigned {
            require_parent_ds_absent: false,
        } => matches!(
            evidence.validation,
            DnssecValidationState::SecureLocalChain
                | DnssecValidationState::AuthenticatedByTrustedResolver
                | DnssecValidationState::Insecure
        ),
        DnssecVerificationExpectation::Unsigned {
            require_parent_ds_absent: true,
        } => {
            evidence.observed_parent_ds.is_empty()
                && evidence.parent_soa_present_for_absence
                && matches!(
                    evidence.parent_source,
                    DnssecEvidenceSource::DirectParentAuthoritative
                        | DnssecEvidenceSource::TrustedRecursiveAuthenticated
                        | DnssecEvidenceSource::LocallyValidatedRecursive
                )
                && evidence.validation != DnssecValidationState::Bogus
        }
        DnssecVerificationExpectation::Signed { expected_ds } => {
            ds_equal(expected_ds, &evidence.observed_parent_ds)
                && matches!(
                    (evidence.parent_source, evidence.validation),
                    (
                        DnssecEvidenceSource::LocallyValidatedRecursive,
                        DnssecValidationState::SecureLocalChain
                    ) | (
                        DnssecEvidenceSource::TrustedRecursiveAuthenticated,
                        DnssecValidationState::AuthenticatedByTrustedResolver
                    )
                )
        }
    }
}

fn ds_equal(
    left: &[edgion_center_core::DnssecDsRecord],
    right: &[edgion_center_core::DnssecDsRecord],
) -> bool {
    let tuples = |records: &[edgion_center_core::DnssecDsRecord]| {
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
    left.len() == right.len() && tuples(left) == tuples(right)
}

fn metric_result(outcome: &DnsQueryOutcome) -> DnsVerificationMetricResult {
    match outcome {
        DnsQueryOutcome::Match { .. } => DnsVerificationMetricResult::Match,
        DnsQueryOutcome::Mismatch { .. } => DnsVerificationMetricResult::Mismatch,
        DnsQueryOutcome::Timeout => DnsVerificationMetricResult::Timeout,
        DnsQueryOutcome::UnsafeAddress => DnsVerificationMetricResult::UnsafeAddress,
        DnsQueryOutcome::BudgetExhausted => DnsVerificationMetricResult::BudgetExhausted,
        _ => DnsVerificationMetricResult::Failure,
    }
}

fn failure_code(error: DnsTransportError) -> SanitizedDnsFailureCode {
    let code = match error {
        DnsTransportError::TargetDenied => "unsafe_address",
        DnsTransportError::InvalidPolicy => "invalid_target_policy",
        DnsTransportError::InvalidQuestion => "invalid_question",
        DnsTransportError::Timeout => "query_timeout",
        DnsTransportError::Io => "transport_io",
        DnsTransportError::Encode => "request_encode",
        DnsTransportError::Decode => "response_decode",
        DnsTransportError::ResponseMismatch => "response_mismatch",
        DnsTransportError::ResponseTooLarge => "response_too_large",
    };
    SanitizedDnsFailureCode::new(code).expect("static failure code is valid")
}

fn error(kind: DnsVerificationErrorKind, code: &'static str) -> DnsVerificationError {
    DnsVerificationError {
        kind,
        code: SanitizedDnsFailureCode::new(code).expect("static failure code is valid"),
    }
}

fn fqdn(name: &str) -> String {
    format!("{}.", trim_dot(name))
}

fn trim_dot(name: &str) -> &str {
    name.strip_suffix('.').unwrap_or(name)
}

fn backoff(request: &DnsVerificationRequest, attempt: u8) -> Duration {
    let multiplier = 1_u64
        .checked_shl(u32::from(attempt - 1))
        .unwrap_or(u64::MAX);
    Duration::from_millis(
        request
            .policy
            .retry_initial_ms
            .saturating_mul(multiplier)
            .min(request.policy.retry_max_ms),
    )
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use crate::{
        LocalDnssecReason, LocalDnssecValidation, LocalParentDsValidation, LocalRrsetValidation,
    };
    use hickory_proto::{
        op::OpCode,
        rr::{
            rdata::{A, CNAME, SOA},
            DNSClass, Name, Record,
        },
    };

    use super::*;

    #[test]
    fn local_chain_evidence_uses_its_own_source_type() {
        let evidence = dnssec_evidence(LocalDnssecValidation {
            security: LocalDnssecSecurity::Secure,
            rrset: LocalRrsetValidation::Match,
            parent_ds: LocalParentDsValidation::Match {
                observed: Vec::new(),
            },
            reason: LocalDnssecReason::Verified,
        });
        assert_eq!(
            evidence.parent_source,
            DnssecEvidenceSource::LocallyValidatedRecursive
        );
        assert_eq!(evidence.validation, DnssecValidationState::SecureLocalChain);
    }

    #[test]
    fn failed_local_chain_does_not_claim_parent_authority() {
        let evidence = dnssec_evidence(LocalDnssecValidation {
            security: LocalDnssecSecurity::Bogus,
            rrset: LocalRrsetValidation::Failed,
            parent_ds: LocalParentDsValidation::Failed,
            reason: LocalDnssecReason::ValidationFailure,
        });
        assert_eq!(evidence.parent_source, DnssecEvidenceSource::NotChecked);
        assert_eq!(evidence.validation, DnssecValidationState::Bogus);
    }

    #[test]
    fn cname_verification_does_not_follow_or_collect_target_answers() {
        let owner = Name::from_ascii("alias.example.com.").unwrap();
        let target = Name::from_ascii("target.example.net.").unwrap();
        let mut message = Message::response(1, OpCode::Query);
        message.metadata.authoritative = true;
        message.add_answer(Record::from_rdata(
            owner,
            60,
            RData::CNAME(CNAME(target.clone())),
        ));
        message.add_answer(Record::from_rdata(
            target,
            60,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 20))),
        ));

        let outcome = message_outcome(&message, "alias.example.com.", RecordType::CNAME);
        assert!(matches!(
            outcome,
            DnsQueryOutcome::Match { values, .. }
                if values == BTreeSet::from([DnsRecordSetValue::Cname {
                    target: AbsoluteDnsName::new("target.example.net").unwrap(),
                }])
        ));
    }

    #[test]
    fn chaos_class_records_cannot_satisfy_positive_or_negative_checks() {
        let owner = Name::from_ascii("www.example.com.").unwrap();
        let mut positive = Message::response(1, OpCode::Query);
        let mut a =
            Record::from_rdata(owner.clone(), 60, RData::A(A(Ipv4Addr::new(192, 0, 2, 10))));
        a.dns_class = DNSClass::CH;
        positive.add_answer(a);
        assert!(matches!(
            message_outcome(&positive, "www.example.com.", RecordType::A),
            DnsQueryOutcome::NoData {
                soa_present: false,
                ..
            }
        ));

        let mut negative = Message::error_msg(1, OpCode::Query, ResponseCode::NXDomain);
        negative.metadata.authoritative = true;
        let mut soa = Record::from_rdata(
            Name::from_ascii("example.com.").unwrap(),
            60,
            RData::SOA(SOA::new(
                Name::from_ascii("ns.example.com.").unwrap(),
                Name::from_ascii("hostmaster.example.com.").unwrap(),
                1,
                60,
                60,
                60,
                60,
            )),
        );
        soa.dns_class = DNSClass::CH;
        negative.add_authority(soa);
        assert!(matches!(
            message_outcome(&negative, "missing.example.com.", RecordType::A),
            DnsQueryOutcome::NxDomain {
                soa_present: false,
                ..
            }
        ));
    }

    #[test]
    fn nameserver_resolution_rejects_non_in_and_wrong_type_answers() {
        let owner = Name::from_ascii("ns.example.net.").unwrap();
        let mut message = Message::response(1, OpCode::Query);
        let mut chaos =
            Record::from_rdata(owner.clone(), 60, RData::A(A(Ipv4Addr::new(1, 1, 1, 1))));
        chaos.dns_class = DNSClass::CH;
        message.add_answer(chaos);
        message.add_answer(Record::from_rdata(
            owner,
            60,
            RData::AAAA(hickory_proto::rr::rdata::AAAA(
                "2606:4700:4700::1111".parse().unwrap(),
            )),
        ));
        assert!(resolved_addresses(&message, "ns.example.net", RecordType::A).is_empty());
    }
}
