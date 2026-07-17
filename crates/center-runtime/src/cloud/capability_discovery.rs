use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use edgion_center_core::{
    CapabilityDiscoveryFence, CapabilityDiscoveryIssue, CapabilityDiscoveryReport,
    CapabilityDiscoveryRequest, CapabilityDiscoveryState, CapabilityIssueScope,
    CapabilityIssueSeverity, CapabilityReason, CapabilityScope, CapabilitySnapshotKey,
    CapabilitySnapshotStore, CapabilityStoreWrite, CloudProvider, CloudResourceId, CoreError,
    CoreResult, ProviderAccountSpec, ProviderCapabilityDiscoverer, ProviderCapabilitySnapshot,
    SanitizedCapabilityCode, SanitizedCapabilityMessage,
};
use tokio::sync::{Mutex, Semaphore};

const DEFAULT_MAX_CONCURRENT_DISCOVERIES: usize = 16;
const MAX_CREDENTIAL_REVISION_LEN: usize = 512;
const WINNER_READ_DELAY: Duration = Duration::from_millis(10);

pub trait CapabilityJitter: Send + Sync {
    fn refresh_ahead_ms(
        &self,
        key: &CapabilitySnapshotKey,
        snapshot: &ProviderCapabilitySnapshot,
        maximum_ms: u64,
    ) -> u64;
}

pub struct StableCapabilityJitter;

impl CapabilityJitter for StableCapabilityJitter {
    fn refresh_ahead_ms(
        &self,
        key: &CapabilitySnapshotKey,
        snapshot: &ProviderCapabilitySnapshot,
        maximum_ms: u64,
    ) -> u64 {
        if maximum_ms == 0 {
            return 0;
        }
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in key
            .provider_account_id
            .as_str()
            .bytes()
            .chain(snapshot.fence.discovery_token.as_str().bytes())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash % maximum_ms.saturating_add(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityRefreshPolicy {
    pub maximum_complete_ttl: Duration,
    pub maximum_partial_ttl: Duration,
    pub failed_ttl: Duration,
    pub unrevisioned_ttl: Duration,
    pub maximum_refresh_ahead: Duration,
}

impl Default for CapabilityRefreshPolicy {
    fn default() -> Self {
        Self {
            maximum_complete_ttl: Duration::from_secs(15 * 60),
            maximum_partial_ttl: Duration::from_secs(2 * 60),
            failed_ttl: Duration::from_secs(30),
            unrevisioned_ttl: Duration::from_secs(60),
            maximum_refresh_ahead: Duration::from_secs(5),
        }
    }
}

impl CapabilityRefreshPolicy {
    fn validate(&self) -> CoreResult<()> {
        if self.maximum_complete_ttl.is_zero()
            || self.maximum_partial_ttl.is_zero()
            || self.failed_ttl.is_zero()
            || self.unrevisioned_ttl.is_zero()
            || self.maximum_partial_ttl > self.maximum_complete_ttl
            || self.failed_ttl > self.maximum_partial_ttl
            || self.unrevisioned_ttl > self.maximum_complete_ttl
            || self.maximum_refresh_ahead >= self.failed_ttl
        {
            return Err(CoreError::Conflict(
                "capability refresh cache policy is invalid".to_string(),
            ));
        }
        Ok(())
    }
}

/// Resolves the provider adapter without introducing vendor dependencies into
/// the runtime crate.
pub trait CapabilityDiscovererResolver: Send + Sync {
    fn resolve(&self, provider: &CloudProvider) -> Option<Arc<dyn ProviderCapabilityDiscoverer>>;
}

pub trait CapabilityClock: Send + Sync {
    fn now_unix_ms(&self) -> i64;
}

struct SystemCapabilityClock;

impl CapabilityClock for SystemCapabilityClock {
    fn now_unix_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRefreshInput {
    pub provider_account_id: CloudResourceId,
    pub provider_account_generation: u64,
    pub credential_revision: Option<String>,
    pub account: ProviderAccountSpec,
    pub scope: CapabilityScope,
}

impl CapabilityRefreshInput {
    fn snapshot_key(&self) -> CapabilitySnapshotKey {
        CapabilitySnapshotKey {
            provider_account_id: self.provider_account_id.clone(),
            scope: self.scope.clone(),
        }
    }

    fn validate(&self) -> CoreResult<()> {
        self.snapshot_key().validate()?;
        self.account.credential_source.validate()?;
        if self.provider_account_generation == 0
            || self.provider_account_generation > i64::MAX as u64
        {
            return Err(CoreError::Conflict(
                "capability refresh account generation is invalid".to_string(),
            ));
        }
        validate_credential_revision(self.credential_revision.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityRefreshOutcome {
    Cached(ProviderCapabilitySnapshot),
    Refreshed(ProviderCapabilitySnapshot),
    WonByOther(ProviderCapabilitySnapshot),
}

#[derive(Clone)]
pub struct CapabilityDiscoveryService {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    discovery_timeout: Duration,
    store: Arc<dyn CapabilitySnapshotStore>,
    resolver: Arc<dyn CapabilityDiscovererResolver>,
    clock: Arc<dyn CapabilityClock>,
    jitter: Arc<dyn CapabilityJitter>,
    cache_policy: CapabilityRefreshPolicy,
    flights: Mutex<Vec<(FlightKey, Weak<Flight>)>>,
    discovery_limit: Semaphore,
}

#[derive(Clone, PartialEq, Eq)]
struct FlightKey {
    snapshot_key: CapabilitySnapshotKey,
    provider: CloudProvider,
    provider_account_generation: u64,
    credential_revision: Option<String>,
}

struct Flight {
    gate: Mutex<()>,
    result: Mutex<Option<CoreResult<CapabilityRefreshOutcome>>>,
}

impl CapabilityDiscoveryService {
    pub fn new(
        discovery_timeout: Duration,
        store: Arc<dyn CapabilitySnapshotStore>,
        resolver: Arc<dyn CapabilityDiscovererResolver>,
    ) -> CoreResult<Self> {
        Self::new_with_clock(
            discovery_timeout,
            store,
            resolver,
            Arc::new(SystemCapabilityClock),
        )
    }

    pub fn new_with_clock(
        discovery_timeout: Duration,
        store: Arc<dyn CapabilitySnapshotStore>,
        resolver: Arc<dyn CapabilityDiscovererResolver>,
        clock: Arc<dyn CapabilityClock>,
    ) -> CoreResult<Self> {
        Self::new_with_components(
            discovery_timeout,
            DEFAULT_MAX_CONCURRENT_DISCOVERIES,
            CapabilityRefreshPolicy::default(),
            store,
            resolver,
            clock,
            Arc::new(StableCapabilityJitter),
        )
    }

    pub fn new_with_components(
        discovery_timeout: Duration,
        max_concurrent_discoveries: usize,
        cache_policy: CapabilityRefreshPolicy,
        store: Arc<dyn CapabilitySnapshotStore>,
        resolver: Arc<dyn CapabilityDiscovererResolver>,
        clock: Arc<dyn CapabilityClock>,
        jitter: Arc<dyn CapabilityJitter>,
    ) -> CoreResult<Self> {
        cache_policy.validate()?;
        if discovery_timeout.is_zero() || max_concurrent_discoveries == 0 {
            return Err(CoreError::Conflict(
                "capability discovery timeout and concurrency limit must be positive".to_string(),
            ));
        }
        Ok(Self {
            inner: Arc::new(ServiceInner {
                discovery_timeout,
                store,
                resolver,
                clock,
                jitter,
                cache_policy,
                flights: Mutex::new(Vec::new()),
                discovery_limit: Semaphore::new(max_concurrent_discoveries),
            }),
        })
    }

    /// Returns a cache entry only when every account/scope authority field
    /// still matches and its provider/runtime freshness window remains open.
    pub async fn get_or_refresh(
        &self,
        input: CapabilityRefreshInput,
    ) -> CoreResult<CapabilityRefreshOutcome> {
        input.validate()?;
        let key = input.snapshot_key();
        if let Some(snapshot) = self.inner.store.get(&key).await? {
            if self.inner.is_reusable(&key, &input, &snapshot) {
                return Ok(CapabilityRefreshOutcome::Cached(snapshot));
            }
        }
        self.refresh(input).await
    }

    /// Removes snapshots only for the exact stale account authority. This is
    /// safe for delayed credential-rotation events because the store must not
    /// delete a newer generation or credential revision.
    pub async fn invalidate_stale_revision(
        &self,
        account_id: &CloudResourceId,
        stale_provider_account_generation: u64,
        stale_credential_revision: Option<&str>,
    ) -> CoreResult<()> {
        account_id.validate()?;
        if stale_provider_account_generation == 0
            || stale_provider_account_generation > i64::MAX as u64
        {
            return Err(CoreError::Conflict(
                "stale provider account generation is invalid".to_string(),
            ));
        }
        validate_credential_revision(stale_credential_revision)?;
        self.inner
            .store
            .invalidate_account_revision(
                account_id,
                stale_provider_account_generation,
                stale_credential_revision,
            )
            .await
    }

    /// Refreshes one account/scope snapshot. Concurrent callers for the same
    /// account authority share a keyed flight. If the active caller is
    /// cancelled, its mutex guard is released and the next waiter takes over.
    pub async fn refresh(
        &self,
        input: CapabilityRefreshInput,
    ) -> CoreResult<CapabilityRefreshOutcome> {
        input.validate()?;
        let key = FlightKey {
            snapshot_key: input.snapshot_key(),
            provider: input.account.provider.clone(),
            provider_account_generation: input.provider_account_generation,
            credential_revision: input.credential_revision.clone(),
        };

        let flight = {
            let mut flights = self.inner.flights.lock().await;
            flights.retain(|(_, flight)| flight.strong_count() > 0);
            if let Some(flight) = flights
                .iter()
                .find(|(candidate, _)| candidate == &key)
                .and_then(|(_, flight)| flight.upgrade())
            {
                flight
            } else {
                let flight = Arc::new(Flight {
                    gate: Mutex::new(()),
                    result: Mutex::new(None),
                });
                flights.push((key.clone(), Arc::downgrade(&flight)));
                flight
            }
        };

        let _guard = flight.gate.lock().await;
        if let Some(result) = flight.result.lock().await.clone() {
            return result;
        }
        let result = self.inner.perform_refresh(&input).await;
        *flight.result.lock().await = Some(result.clone());
        let mut flights = self.inner.flights.lock().await;
        flights.retain(|(candidate, value)| {
            candidate != &key
                || value
                    .upgrade()
                    .is_some_and(|value| !Arc::ptr_eq(&value, &flight))
        });
        result
    }
}

impl ServiceInner {
    fn is_reusable(
        &self,
        key: &CapabilitySnapshotKey,
        input: &CapabilityRefreshInput,
        snapshot: &ProviderCapabilitySnapshot,
    ) -> bool {
        if snapshot.validate().is_err()
            || snapshot.provider_account_id != input.provider_account_id
            || snapshot.provider != input.account.provider
            || snapshot.scope != input.scope
            || snapshot.fence.provider_account_generation != input.provider_account_generation
            || snapshot.fence.credential_revision != input.credential_revision
        {
            return false;
        }
        let state_ttl = match snapshot.state {
            CapabilityDiscoveryState::Complete => self.cache_policy.maximum_complete_ttl,
            CapabilityDiscoveryState::Partial => self.cache_policy.maximum_partial_ttl,
            CapabilityDiscoveryState::Failed => self.cache_policy.failed_ttl,
        };
        let state_ttl = if input.credential_revision.is_none() {
            state_ttl.min(self.cache_policy.unrevisioned_ttl)
        } else {
            state_ttl
        };
        let runtime_deadline = snapshot
            .discovered_at_unix_ms
            .saturating_add(duration_ms_i64(state_ttl));
        let observation_deadline = snapshot
            .observations
            .iter()
            .flat_map(|capability| &capability.dimensions)
            .map(|observation| observation.valid_until_unix_ms)
            .min()
            .unwrap_or(runtime_deadline);
        let deadline = runtime_deadline.min(observation_deadline);
        let maximum_jitter = self.cache_policy.maximum_refresh_ahead.min(
            state_ttl
                .checked_sub(Duration::from_millis(1))
                .unwrap_or_default(),
        );
        let refresh_ahead = self.jitter.refresh_ahead_ms(
            key,
            snapshot,
            u64::try_from(maximum_jitter.as_millis()).unwrap_or(u64::MAX),
        );
        let now = self.clock.now_unix_ms();
        now >= snapshot.discovered_at_unix_ms
            && now < deadline.saturating_sub(i64::try_from(refresh_ahead).unwrap_or(i64::MAX))
    }

    async fn perform_refresh(
        &self,
        input: &CapabilityRefreshInput,
    ) -> CoreResult<CapabilityRefreshOutcome> {
        let _permit = self
            .discovery_limit
            .acquire()
            .await
            .map_err(|_| CoreError::Adapter("capability discovery service is closed".into()))?;
        let key = input.snapshot_key();
        let fence = self
            .store
            .begin_discovery(
                &key,
                input.provider_account_generation,
                input.credential_revision.as_deref(),
            )
            .await?;
        validate_fence(input, &fence)?;

        let request = CapabilityDiscoveryRequest {
            provider_account_id: input.provider_account_id.clone(),
            fence: fence.clone(),
            account: input.account.clone(),
            scope: input.scope.clone(),
        };
        request.validate()?;

        let report = match self.resolver.resolve(&input.account.provider) {
            Some(discoverer) => {
                match tokio::time::timeout(self.discovery_timeout, discoverer.discover(&request))
                    .await
                {
                    Ok(Ok(report)) if report.validate().is_ok() => report,
                    Ok(Ok(_)) => fixed_failure_report(
                        CapabilityReason::InvalidProviderResponse,
                        "invalid_provider_response",
                        "provider capability discovery returned an invalid response",
                    )?,
                    Ok(Err(_)) => fixed_failure_report(
                        CapabilityReason::ProbeFailed,
                        "discovery_failed",
                        "provider capability discovery failed",
                    )?,
                    Err(_) => fixed_failure_report(
                        CapabilityReason::ProviderUnavailable,
                        "discovery_timeout",
                        "provider capability discovery timed out",
                    )?,
                }
            }
            None => fixed_failure_report(
                CapabilityReason::AdapterNotImplemented,
                "discoverer_unavailable",
                "provider capability discoverer is not configured",
            )?,
        };

        let snapshot = ProviderCapabilitySnapshot::from_report(
            &request,
            self.clock.now_unix_ms().max(1),
            report,
        )?;
        match self.store.put_if_current(&key, &fence, &snapshot).await? {
            CapabilityStoreWrite::Stored => Ok(CapabilityRefreshOutcome::Refreshed(snapshot)),
            CapabilityStoreWrite::FenceLost => {
                let winner = tokio::time::timeout(self.discovery_timeout, async {
                    loop {
                        if let Some(winner) = self.store.get(&key).await? {
                            if validate_winner(input, Some(&fence), &winner).is_ok() {
                                return Ok::<_, CoreError>(winner);
                            }
                        }
                        tokio::time::sleep(WINNER_READ_DELAY).await;
                    }
                })
                .await
                .map_err(|_| {
                    CoreError::Conflict(
                        "capability refresh lost its fence without an authoritative winner"
                            .to_string(),
                    )
                })??;
                Ok(CapabilityRefreshOutcome::WonByOther(winner))
            }
        }
    }
}

fn duration_ms_i64(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn validate_credential_revision(revision: Option<&str>) -> CoreResult<()> {
    if revision.is_some_and(|revision| {
        revision.is_empty()
            || revision.len() > MAX_CREDENTIAL_REVISION_LEN
            || revision.trim() != revision
            || revision.chars().any(char::is_control)
    }) {
        return Err(CoreError::Conflict(
            "capability refresh credential revision is invalid".to_string(),
        ));
    }
    Ok(())
}

fn validate_fence(
    input: &CapabilityRefreshInput,
    fence: &CapabilityDiscoveryFence,
) -> CoreResult<()> {
    fence.validate()?;
    if fence.provider_account_generation != input.provider_account_generation
        || fence.credential_revision != input.credential_revision
    {
        return Err(CoreError::Conflict(
            "capability store returned a mismatched discovery fence".to_string(),
        ));
    }
    Ok(())
}

fn validate_winner(
    input: &CapabilityRefreshInput,
    lost_fence: Option<&CapabilityDiscoveryFence>,
    winner: &ProviderCapabilitySnapshot,
) -> CoreResult<()> {
    winner.validate()?;
    if winner.provider_account_id != input.provider_account_id
        || winner.provider != input.account.provider
        || winner.scope != input.scope
        || winner.fence.provider_account_generation != input.provider_account_generation
        || winner.fence.credential_revision != input.credential_revision
        || lost_fence.is_some_and(|lost| winner.fence.discovery_epoch <= lost.discovery_epoch)
    {
        return Err(CoreError::Conflict(
            "capability refresh fence winner is not authoritative for the request".to_string(),
        ));
    }
    Ok(())
}

fn fixed_failure_report(
    reason: CapabilityReason,
    code: &'static str,
    message: &'static str,
) -> CoreResult<CapabilityDiscoveryReport> {
    Ok(CapabilityDiscoveryReport {
        state: CapabilityDiscoveryState::Failed,
        observations: Vec::new(),
        issues: vec![CapabilityDiscoveryIssue {
            severity: CapabilityIssueSeverity::Blocking,
            scope: CapabilityIssueScope::Account,
            reason,
            code: SanitizedCapabilityCode::new(code)?,
            message: SanitizedCapabilityMessage::new(message)?,
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use edgion_center_core::{CredentialSource, DiscoveryToken, ProviderCapabilitySnapshot};
    use parking_lot::Mutex as ParkingMutex;
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    struct Clock(AtomicI64);

    impl CapabilityClock for Clock {
        fn now_unix_ms(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    struct Discoverer {
        calls: AtomicUsize,
        delay: Duration,
        result: ParkingMutex<Option<CoreResult<CapabilityDiscoveryReport>>>,
    }

    #[async_trait]
    impl ProviderCapabilityDiscoverer for Discoverer {
        async fn discover(
            &self,
            _: &CapabilityDiscoveryRequest,
        ) -> CoreResult<CapabilityDiscoveryReport> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.result.lock().clone().unwrap_or_else(|| {
                Ok(CapabilityDiscoveryReport {
                    state: CapabilityDiscoveryState::Complete,
                    observations: Vec::new(),
                    issues: Vec::new(),
                })
            })
        }
    }

    struct Resolver(Arc<Discoverer>);

    impl CapabilityDiscovererResolver for Resolver {
        fn resolve(&self, _: &CloudProvider) -> Option<Arc<dyn ProviderCapabilityDiscoverer>> {
            Some(self.0.clone())
        }
    }

    struct Store {
        begin_calls: AtomicUsize,
        put_calls: AtomicUsize,
        invalidate_calls: AtomicUsize,
        put_delay: ParkingMutex<Duration>,
        snapshot: ParkingMutex<Option<ProviderCapabilitySnapshot>>,
        write: ParkingMutex<CapabilityStoreWrite>,
    }

    #[async_trait]
    impl CapabilitySnapshotStore for Store {
        async fn get(
            &self,
            _: &CapabilitySnapshotKey,
        ) -> CoreResult<Option<ProviderCapabilitySnapshot>> {
            Ok(self.snapshot.lock().clone())
        }

        async fn begin_discovery(
            &self,
            _: &CapabilitySnapshotKey,
            provider_account_generation: u64,
            credential_revision: Option<&str>,
        ) -> CoreResult<CapabilityDiscoveryFence> {
            let epoch = self.begin_calls.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            Ok(CapabilityDiscoveryFence {
                provider_account_generation,
                credential_revision: credential_revision.map(str::to_string),
                discovery_epoch: epoch,
                discovery_token: DiscoveryToken::new(format!("token-{epoch}"))?,
            })
        }

        async fn put_if_current(
            &self,
            _: &CapabilitySnapshotKey,
            _: &CapabilityDiscoveryFence,
            snapshot: &ProviderCapabilitySnapshot,
        ) -> CoreResult<CapabilityStoreWrite> {
            self.put_calls.fetch_add(1, Ordering::SeqCst);
            let write = *self.write.lock();
            if write == CapabilityStoreWrite::Stored {
                *self.snapshot.lock() = Some(snapshot.clone());
            }
            let delay = *self.put_delay.lock();
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            Ok(write)
        }

        async fn invalidate_account_revision(
            &self,
            _: &CloudResourceId,
            _: u64,
            _: Option<&str>,
        ) -> CoreResult<()> {
            self.invalidate_calls.fetch_add(1, Ordering::SeqCst);
            self.snapshot.lock().take();
            Ok(())
        }
    }

    fn input() -> CapabilityRefreshInput {
        CapabilityRefreshInput {
            provider_account_id: CloudResourceId::new("account-1").unwrap(),
            provider_account_generation: 2,
            credential_revision: Some("credential-3".to_string()),
            account: ProviderAccountSpec {
                provider: CloudProvider::Cloudflare,
                scope: None,
                credential_source: CredentialSource::Ambient,
            },
            scope: CapabilityScope::Account,
        }
    }

    fn fixture(
        delay: Duration,
        timeout: Duration,
    ) -> (
        CapabilityDiscoveryService,
        Arc<Store>,
        Arc<Discoverer>,
        Arc<Clock>,
    ) {
        let store = Arc::new(Store {
            begin_calls: AtomicUsize::new(0),
            put_calls: AtomicUsize::new(0),
            invalidate_calls: AtomicUsize::new(0),
            put_delay: ParkingMutex::new(Duration::ZERO),
            snapshot: ParkingMutex::new(None),
            write: ParkingMutex::new(CapabilityStoreWrite::Stored),
        });
        let discoverer = Arc::new(Discoverer {
            calls: AtomicUsize::new(0),
            delay,
            result: ParkingMutex::new(None),
        });
        let clock = Arc::new(Clock(AtomicI64::new(10_000)));
        let service = CapabilityDiscoveryService::new_with_clock(
            timeout,
            store.clone(),
            Arc::new(Resolver(discoverer.clone())),
            clock.clone(),
        )
        .unwrap();
        (service, store, discoverer, clock)
    }

    #[tokio::test]
    async fn concurrent_refreshes_share_one_flight() {
        let (service, store, discoverer, _) =
            fixture(Duration::from_millis(20), Duration::from_secs(1));
        let mut tasks = Vec::new();
        for _ in 0..32 {
            let service = service.clone();
            tasks.push(tokio::spawn(async move { service.refresh(input()).await }));
        }
        for task in tasks {
            assert!(task.await.unwrap().is_ok());
        }
        assert_eq!(discoverer.calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.begin_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.put_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn timeout_is_persisted_as_a_fixed_sanitized_failure() {
        let (service, store, _, _) = fixture(Duration::from_millis(50), Duration::from_millis(5));
        let outcome = service.refresh(input()).await.unwrap();
        let CapabilityRefreshOutcome::Refreshed(snapshot) = outcome else {
            panic!("expected a stored refresh");
        };
        assert_eq!(snapshot.state, CapabilityDiscoveryState::Failed);
        assert_eq!(snapshot.issues[0].code.as_str(), "discovery_timeout");
        assert_eq!(
            store.snapshot.lock().as_ref().unwrap().issues[0]
                .message
                .as_str(),
            "provider capability discovery timed out"
        );
    }

    #[tokio::test]
    async fn adapter_error_text_is_not_persisted() {
        let (service, store, discoverer, _) = fixture(Duration::ZERO, Duration::from_secs(1));
        *discoverer.result.lock() = Some(Err(CoreError::Adapter(
            "provider returned secret-token-value".to_string(),
        )));
        service.refresh(input()).await.unwrap();
        let json = serde_json::to_string(store.snapshot.lock().as_ref().unwrap()).unwrap();
        assert!(!json.contains("secret-token-value"));
        assert!(json.contains("discovery_failed"));
    }

    #[tokio::test]
    async fn fence_loser_returns_only_a_newer_authoritative_winner() {
        let (service, store, _, _) = fixture(Duration::ZERO, Duration::from_secs(1));
        let request_input = input();
        let winner_request = CapabilityDiscoveryRequest {
            provider_account_id: request_input.provider_account_id.clone(),
            fence: CapabilityDiscoveryFence {
                provider_account_generation: 2,
                credential_revision: Some("credential-3".to_string()),
                discovery_epoch: 2,
                discovery_token: DiscoveryToken::new("winner-token").unwrap(),
            },
            account: request_input.account.clone(),
            scope: request_input.scope.clone(),
        };
        let winner = ProviderCapabilitySnapshot::from_report(
            &winner_request,
            10_000,
            CapabilityDiscoveryReport {
                state: CapabilityDiscoveryState::Complete,
                observations: Vec::new(),
                issues: Vec::new(),
            },
        )
        .unwrap();
        *store.snapshot.lock() = Some(winner.clone());
        *store.write.lock() = CapabilityStoreWrite::FenceLost;

        assert_eq!(
            service.refresh(request_input).await.unwrap(),
            CapabilityRefreshOutcome::WonByOther(winner)
        );
    }

    #[tokio::test]
    async fn fence_loser_fails_closed_when_winner_is_not_newer() {
        let (service, store, _, _) = fixture(Duration::ZERO, Duration::from_secs(1));
        *store.write.lock() = CapabilityStoreWrite::FenceLost;
        let error = service.refresh(input()).await.unwrap_err();
        assert!(matches!(error, CoreError::Conflict(_)));
    }

    #[tokio::test]
    async fn fresh_snapshot_is_reused_without_provider_or_store_mutation() {
        let (service, store, discoverer, _) = fixture(Duration::ZERO, Duration::from_secs(1));
        service.refresh(input()).await.unwrap();

        let outcome = service.get_or_refresh(input()).await.unwrap();
        assert!(matches!(outcome, CapabilityRefreshOutcome::Cached(_)));
        assert_eq!(discoverer.calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.begin_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.put_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unrevisioned_credentials_have_a_short_runtime_ttl() {
        let (service, store, discoverer, clock) = fixture(Duration::ZERO, Duration::from_secs(1));
        let mut request_input = input();
        request_input.credential_revision = None;
        service.refresh(request_input.clone()).await.unwrap();

        clock.0.store(70_001, Ordering::SeqCst);
        let outcome = service.get_or_refresh(request_input).await.unwrap();
        assert!(matches!(outcome, CapabilityRefreshOutcome::Refreshed(_)));
        assert_eq!(discoverer.calls.load(Ordering::SeqCst), 2);
        assert_eq!(store.begin_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn stale_revision_invalidation_is_forwarded_after_validation() {
        let (service, store, _, _) = fixture(Duration::ZERO, Duration::from_secs(1));
        service
            .invalidate_stale_revision(
                &CloudResourceId::new("account-1").unwrap(),
                2,
                Some("credential-3"),
            )
            .await
            .unwrap();
        assert_eq!(store.invalidate_calls.load(Ordering::SeqCst), 1);

        assert!(service
            .invalidate_stale_revision(
                &CloudResourceId::new("account-1").unwrap(),
                2,
                Some(" invalid"),
            )
            .await
            .is_err());
        assert_eq!(store.invalidate_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn joiner_after_snapshot_write_still_reuses_the_inflight_result() {
        let (service, store, discoverer, _) = fixture(Duration::ZERO, Duration::from_secs(1));
        *store.put_delay.lock() = Duration::from_millis(30);

        let leader = {
            let service = service.clone();
            tokio::spawn(async move { service.refresh(input()).await })
        };
        while store.snapshot.lock().is_none() {
            tokio::task::yield_now().await;
        }
        let joiner = {
            let service = service.clone();
            tokio::spawn(async move { service.refresh(input()).await })
        };

        assert!(leader.await.unwrap().is_ok());
        assert!(joiner.await.unwrap().is_ok());
        assert_eq!(discoverer.calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.begin_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.put_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn waiter_takes_over_when_the_active_caller_is_cancelled() {
        let (service, store, discoverer, _) =
            fixture(Duration::from_millis(40), Duration::from_secs(1));
        let leader = {
            let service = service.clone();
            tokio::spawn(async move { service.refresh(input()).await })
        };
        while discoverer.calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        leader.abort();
        assert!(leader.await.unwrap_err().is_cancelled());

        let outcome = tokio::time::timeout(Duration::from_secs(1), service.refresh(input()))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(outcome, CapabilityRefreshOutcome::Refreshed(_)));
        assert_eq!(discoverer.calls.load(Ordering::SeqCst), 2);
        assert_eq!(store.begin_calls.load(Ordering::SeqCst), 2);
        assert_eq!(store.put_calls.load(Ordering::SeqCst), 1);
    }
}
