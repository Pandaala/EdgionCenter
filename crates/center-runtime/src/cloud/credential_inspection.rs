//! Bounded, provider-neutral credential inspection orchestration.

use std::{collections::BTreeSet, fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use edgion_center_core::{
    CloudResourceId, CoreError, CoreResult, CredentialInspection, CredentialInspector,
    CredentialIssue, CredentialIssueKind, CredentialState, ProviderAccount, ProviderAccountScope,
    ProviderAccountStore, ProviderIdentity,
};
use tokio::{
    sync::{Mutex, Semaphore},
    time::Instant,
};

const DEFAULT_MAX_CONCURRENT_INSPECTIONS: usize = 8;
const DEFAULT_MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_MIN_VALIDITY: Duration = Duration::from_secs(30);
const MAX_MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);
const MAX_MIN_VALIDITY: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_IDENTITY_BYTES: usize = 512;
const MAX_SCOPE_BYTES: usize = 256;
const MAX_REVISION_BYTES: usize = 512;
const MAX_ISSUES: usize = 32;
const MAX_ISSUE_CODE_BYTES: usize = 128;
const MAX_ISSUE_MESSAGE_BYTES: usize = 512;

/// Resolves an account provider to a credential-owning inspector without
/// importing provider SDKs into the runtime crate.
#[async_trait]
pub trait CredentialInspectorResolver: Send + Sync {
    /// Returns an inspector bound to this exact Center account authority, or
    /// a factory-safe inspector that resolves it per call. Implementations
    /// must never reuse one account's credential-bound client for another.
    /// Resolution is asynchronous and runs under the service deadline and
    /// concurrency permit. Implementations must not perform blocking I/O.
    async fn resolve(&self, account: &ProviderAccount) -> Option<Arc<dyn CredentialInspector>>;
}

#[derive(Clone, Copy, Debug)]
pub struct CredentialInspectionPolicy {
    pub timeout: Duration,
    pub max_concurrent_inspections: usize,
    pub min_refresh_interval: Duration,
    pub min_validity: Duration,
}

impl CredentialInspectionPolicy {
    pub fn new(timeout: Duration, max_concurrent_inspections: usize) -> Self {
        Self {
            timeout,
            max_concurrent_inspections,
            min_refresh_interval: DEFAULT_MIN_REFRESH_INTERVAL,
            min_validity: DEFAULT_MIN_VALIDITY,
        }
    }
}

/// Validated inspection authority. The opaque credential revision remains
/// private to the runtime and is intentionally unavailable to HTTP DTOs.
#[derive(Clone, PartialEq, Eq)]
pub struct CredentialInspectionAuthority {
    provider_account_id: CloudResourceId,
    provider_account_generation: u64,
    state: CredentialState,
    identity: Option<ProviderIdentity>,
    pub(super) credential_revision: Option<String>,
    expires_at_unix_ms: Option<i64>,
    issues: Vec<CredentialIssue>,
}

impl fmt::Debug for CredentialInspectionAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialInspectionAuthority")
            .field("provider_account_id", &self.provider_account_id)
            .field(
                "provider_account_generation",
                &self.provider_account_generation,
            )
            .field("state", &self.state)
            .field(
                "identity",
                &self
                    .identity
                    .as_ref()
                    .map(|identity| (&identity.provider, identity.scope.as_deref(), "[REDACTED]")),
            )
            .field(
                "credential_revision",
                &self.credential_revision.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .field(
                "issue_kinds",
                &self
                    .issues
                    .iter()
                    .map(|issue| issue.kind)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl CredentialInspectionAuthority {
    pub fn provider_account_id(&self) -> &CloudResourceId {
        &self.provider_account_id
    }

    pub fn provider_account_generation(&self) -> u64 {
        self.provider_account_generation
    }

    pub fn state(&self) -> CredentialState {
        self.state
    }

    pub fn identity(&self) -> Option<&ProviderIdentity> {
        self.identity.as_ref()
    }

    pub fn expires_at_unix_ms(&self) -> Option<i64> {
        self.expires_at_unix_ms
    }

    pub fn issues(&self) -> &[CredentialIssue] {
        &self.issues
    }
}

#[derive(Clone)]
pub struct CredentialInspectionService {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    policy: CredentialInspectionPolicy,
    account_store: Arc<dyn ProviderAccountStore>,
    resolver: Arc<dyn CredentialInspectorResolver>,
    flights: Mutex<Vec<(InspectionKey, Arc<Flight>)>>,
    inspection_limit: Semaphore,
}

#[derive(Clone, PartialEq, Eq)]
struct InspectionKey {
    provider_account_id: CloudResourceId,
    provider_account_generation: u64,
}

struct Flight {
    gate: Mutex<()>,
    state: Mutex<FlightState>,
}

enum FlightState {
    InFlight,
    Completed {
        completed_at: Instant,
        result: CoreResult<CredentialInspectionAuthority>,
    },
}

impl CredentialInspectionService {
    pub fn new(
        timeout: Duration,
        account_store: Arc<dyn ProviderAccountStore>,
        resolver: Arc<dyn CredentialInspectorResolver>,
    ) -> CoreResult<Self> {
        Self::with_policy(
            CredentialInspectionPolicy::new(timeout, DEFAULT_MAX_CONCURRENT_INSPECTIONS),
            account_store,
            resolver,
        )
    }

    pub fn with_concurrency(
        timeout: Duration,
        max_concurrent_inspections: usize,
        account_store: Arc<dyn ProviderAccountStore>,
        resolver: Arc<dyn CredentialInspectorResolver>,
    ) -> CoreResult<Self> {
        Self::with_policy(
            CredentialInspectionPolicy::new(timeout, max_concurrent_inspections),
            account_store,
            resolver,
        )
    }

    pub fn with_policy(
        policy: CredentialInspectionPolicy,
        account_store: Arc<dyn ProviderAccountStore>,
        resolver: Arc<dyn CredentialInspectorResolver>,
    ) -> CoreResult<Self> {
        if policy.timeout.is_zero()
            || policy.max_concurrent_inspections == 0
            || policy.min_refresh_interval.is_zero()
            || policy.min_refresh_interval > MAX_MIN_REFRESH_INTERVAL
            || policy.min_validity.is_zero()
            || policy.min_validity > MAX_MIN_VALIDITY
        {
            return Err(CoreError::Conflict(
                "credential inspection policy is outside supported bounds".into(),
            ));
        }
        Ok(Self {
            inner: Arc::new(ServiceInner {
                policy,
                account_store,
                resolver,
                flights: Mutex::new(Vec::new()),
                inspection_limit: Semaphore::new(policy.max_concurrent_inspections),
            }),
        })
    }

    pub async fn inspect(
        &self,
        provider_account_id: &CloudResourceId,
    ) -> CoreResult<CredentialInspectionAuthority> {
        provider_account_id.validate()?;
        tokio::time::timeout(
            self.inner.policy.timeout,
            self.inspect_within_deadline(provider_account_id),
        )
        .await
        .map_err(|_| CoreError::Adapter("credential inspection timed out".into()))?
    }

    async fn inspect_within_deadline(
        &self,
        provider_account_id: &CloudResourceId,
    ) -> CoreResult<CredentialInspectionAuthority> {
        let account = self
            .inner
            .account_store
            .get(provider_account_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("provider account".into()))?;
        let key = InspectionKey {
            provider_account_id: account.metadata.id.clone(),
            provider_account_generation: account.metadata.generation,
        };
        let flight = {
            let mut flights = self.inner.flights.lock().await;
            let now = Instant::now();
            let now_unix_ms = current_unix_ms().ok();
            flights.retain(|(_, flight)| {
                flight.state.try_lock().map_or(true, |state| match &*state {
                    FlightState::InFlight => true,
                    FlightState::Completed {
                        completed_at,
                        result,
                    } => cached_result_is_reusable(
                        self.inner.policy,
                        *completed_at,
                        result,
                        now,
                        now_unix_ms,
                    ),
                })
            });
            if let Some(flight) = flights
                .iter()
                .find(|(candidate, _)| candidate == &key)
                .map(|(_, flight)| flight.clone())
            {
                flight
            } else {
                let flight = Arc::new(Flight {
                    gate: Mutex::new(()),
                    state: Mutex::new(FlightState::InFlight),
                });
                flights.push((key.clone(), flight.clone()));
                flight
            }
        };

        let _guard = flight.gate.lock().await;
        {
            let mut state = flight.state.lock().await;
            if let FlightState::Completed {
                completed_at,
                result,
            } = &*state
            {
                if cached_result_is_reusable(
                    self.inner.policy,
                    *completed_at,
                    result,
                    Instant::now(),
                    current_unix_ms().ok(),
                ) {
                    return result.clone();
                }
            }
            *state = FlightState::InFlight;
        }
        let result = self.inner.perform_inspection(&account).await;
        *flight.state.lock().await = FlightState::Completed {
            completed_at: Instant::now(),
            result: result.clone(),
        };
        result
    }
}

impl ServiceInner {
    async fn perform_inspection(
        &self,
        account: &ProviderAccount,
    ) -> CoreResult<CredentialInspectionAuthority> {
        let _permit = self.inspection_limit.acquire().await.map_err(|_| {
            CoreError::Adapter("credential inspection concurrency gate closed".into())
        })?;
        let inspector = self
            .resolver
            .resolve(account)
            .await
            .ok_or(CoreError::Unsupported("provider credential inspector"))?;
        let inspection = inspector
            .inspect(&account.spec)
            .await
            .map_err(|_| CoreError::Adapter("provider credential inspection failed".into()))?;
        let completed_at_unix_ms = current_unix_ms()?;
        validate_inspection(
            account,
            &inspection,
            completed_at_unix_ms,
            self.policy.min_validity,
        )
        .map_err(|_| CoreError::Adapter("provider credential inspection was invalid".into()))?;
        Ok(CredentialInspectionAuthority {
            provider_account_id: account.metadata.id.clone(),
            provider_account_generation: account.metadata.generation,
            state: inspection.state,
            identity: inspection.identity,
            credential_revision: inspection.credential_revision,
            expires_at_unix_ms: inspection.expires_at_unix_ms,
            issues: inspection.issues,
        })
    }
}

fn current_unix_ms() -> CoreResult<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| CoreError::Adapter("system clock is before the Unix epoch".into()))?
        .as_millis()
        .try_into()
        .map_err(|_| CoreError::Adapter("system clock exceeds supported range".into()))
}

fn cached_result_is_reusable(
    policy: CredentialInspectionPolicy,
    completed_at: Instant,
    result: &CoreResult<CredentialInspectionAuthority>,
    now: Instant,
    now_unix_ms: Option<i64>,
) -> bool {
    if now.duration_since(completed_at) >= policy.min_refresh_interval {
        return false;
    }
    let Ok(authority) = result else {
        return true;
    };
    if authority.state != CredentialState::Valid {
        return true;
    }
    let Some(expires_at) = authority.expires_at_unix_ms else {
        return true;
    };
    let Some(now_unix_ms) = now_unix_ms else {
        return false;
    };
    let min_validity_ms = policy
        .min_validity
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX);
    expires_at > now_unix_ms.saturating_add(min_validity_ms)
}

fn validate_inspection(
    account: &ProviderAccount,
    inspection: &CredentialInspection,
    completed_at_unix_ms: i64,
    min_validity: Duration,
) -> CoreResult<()> {
    if inspection.issues.len() > MAX_ISSUES
        || inspection
            .expires_at_unix_ms
            .is_some_and(|expires| expires <= 0)
    {
        return Err(CoreError::Conflict(
            "credential inspection shape is invalid".into(),
        ));
    }
    if let Some(revision) = inspection.credential_revision.as_deref() {
        validate_text(revision, MAX_REVISION_BYTES, "credential revision")?;
    }
    match inspection.state {
        CredentialState::Valid
            if inspection.identity.is_none() || !inspection.issues.is_empty() =>
        {
            return Err(CoreError::Conflict(
                "valid credential inspection has inconsistent evidence".into(),
            ));
        }
        CredentialState::Invalid | CredentialState::Unknown if inspection.issues.is_empty() => {
            return Err(CoreError::Conflict(
                "non-valid credential inspection requires an issue".into(),
            ));
        }
        _ => {}
    }
    if inspection.state == CredentialState::Valid {
        if let Some(expires_at) = inspection.expires_at_unix_ms {
            let min_valid_until = completed_at_unix_ms
                .saturating_add(min_validity.as_millis().try_into().unwrap_or(i64::MAX));
            if expires_at <= min_valid_until {
                return Err(CoreError::Conflict(
                    "valid credential inspection expires too soon".into(),
                ));
            }
        }
    }
    if let Some(identity) = inspection.identity.as_ref() {
        if identity.provider != account.spec.provider {
            return Err(CoreError::Conflict(
                "credential inspection provider does not match the account".into(),
            ));
        }
        validate_text(
            &identity.principal,
            MAX_IDENTITY_BYTES,
            "provider principal",
        )?;
        let expected_scope = configured_scope(account)?;
        let observed_scope = identity.scope.as_deref().ok_or_else(|| {
            CoreError::Conflict("credential inspection omitted provider scope".into())
        })?;
        validate_text(observed_scope, MAX_SCOPE_BYTES, "provider identity scope")?;
        if observed_scope != expected_scope {
            return Err(CoreError::Conflict(
                "credential inspection scope does not match the account".into(),
            ));
        }
    }
    let mut issue_keys = BTreeSet::new();
    for issue in &inspection.issues {
        if issue.code.is_empty()
            || issue.code.len() > MAX_ISSUE_CODE_BYTES
            || !issue
                .code
                .bytes()
                .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == b'_')
        {
            return Err(CoreError::Conflict(
                "credential inspection issue code is invalid".into(),
            ));
        }
        validate_text(
            &issue.message,
            MAX_ISSUE_MESSAGE_BYTES,
            "credential inspection issue message",
        )?;
        if !issue_keys.insert((issue_kind_tag(issue.kind), issue.code.as_str())) {
            return Err(CoreError::Conflict(
                "credential inspection contains duplicate issues".into(),
            ));
        }
    }
    Ok(())
}

fn configured_scope(account: &ProviderAccount) -> CoreResult<&str> {
    match account.spec.scope.as_ref() {
        Some(ProviderAccountScope::Cloudflare { account_id })
        | Some(ProviderAccountScope::Aws { account_id }) => Ok(account_id),
        None => Err(CoreError::Conflict(
            "credential inspection requires provider account scope".into(),
        )),
    }
}

fn validate_text(value: &str, max_bytes: usize, kind: &'static str) -> CoreResult<()> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(CoreError::Conflict(format!("{kind} is invalid")));
    }
    Ok(())
}

fn issue_kind_tag(kind: CredentialIssueKind) -> u8 {
    match kind {
        CredentialIssueKind::ReferenceNotFound => 0,
        CredentialIssueKind::AuthenticationFailed => 1,
        CredentialIssueKind::PermissionDenied => 2,
        CredentialIssueKind::Expired => 3,
        CredentialIssueKind::InvalidConfiguration => 4,
        CredentialIssueKind::ProviderUnavailable => 5,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use edgion_center_core::{
        provider_account_from_desired, CloudProvider, CredentialRef, CredentialSource,
        DeletionPolicy, ManagementPolicy, ProviderAccountCreateResult, ProviderAccountDesired,
        ProviderAccountPage, ProviderAccountPageRequest, ProviderAccountReplaceResult,
        ProviderAccountSpec,
    };

    use super::*;

    struct AccountStore(Option<ProviderAccount>);

    #[async_trait]
    impl ProviderAccountStore for AccountStore {
        async fn create(
            &self,
            _: &CloudResourceId,
            _: &ProviderAccountDesired,
        ) -> CoreResult<ProviderAccountCreateResult> {
            unreachable!()
        }

        async fn get(&self, id: &CloudResourceId) -> CoreResult<Option<ProviderAccount>> {
            Ok(self.0.clone().filter(|account| account.metadata.id == *id))
        }

        async fn list(&self, _: &ProviderAccountPageRequest) -> CoreResult<ProviderAccountPage> {
            unreachable!()
        }

        async fn replace_if_generation(
            &self,
            _: &CloudResourceId,
            _: u64,
            _: &ProviderAccountDesired,
        ) -> CoreResult<ProviderAccountReplaceResult> {
            unreachable!()
        }
    }

    struct Inspector {
        calls: AtomicUsize,
        delay: Duration,
        inspection: CredentialInspection,
    }

    #[async_trait]
    impl CredentialInspector for Inspector {
        async fn inspect(&self, _: &ProviderAccountSpec) -> CoreResult<CredentialInspection> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            Ok(self.inspection.clone())
        }
    }

    struct Resolver(Option<Arc<dyn CredentialInspector>>);

    #[async_trait]
    impl CredentialInspectorResolver for Resolver {
        async fn resolve(&self, _: &ProviderAccount) -> Option<Arc<dyn CredentialInspector>> {
            self.0.clone()
        }
    }

    struct SlowResolver {
        delay: Duration,
        inspector: Option<Arc<dyn CredentialInspector>>,
    }

    #[async_trait]
    impl CredentialInspectorResolver for SlowResolver {
        async fn resolve(&self, _: &ProviderAccount) -> Option<Arc<dyn CredentialInspector>> {
            tokio::time::sleep(self.delay).await;
            self.inspector.clone()
        }
    }

    struct GenerationStore {
        account: ProviderAccount,
        generation: std::sync::atomic::AtomicU64,
    }

    #[async_trait]
    impl ProviderAccountStore for GenerationStore {
        async fn create(
            &self,
            _: &CloudResourceId,
            _: &ProviderAccountDesired,
        ) -> CoreResult<ProviderAccountCreateResult> {
            unreachable!()
        }

        async fn get(&self, id: &CloudResourceId) -> CoreResult<Option<ProviderAccount>> {
            if self.account.metadata.id != *id {
                return Ok(None);
            }
            let mut account = self.account.clone();
            account.metadata.generation = self.generation.load(Ordering::SeqCst);
            Ok(Some(account))
        }

        async fn list(&self, _: &ProviderAccountPageRequest) -> CoreResult<ProviderAccountPage> {
            unreachable!()
        }

        async fn replace_if_generation(
            &self,
            _: &CloudResourceId,
            _: u64,
            _: &ProviderAccountDesired,
        ) -> CoreResult<ProviderAccountReplaceResult> {
            unreachable!()
        }
    }

    fn account() -> ProviderAccount {
        provider_account_from_desired(
            CloudResourceId::new("cloudflare-main").unwrap(),
            7,
            &ProviderAccountDesired {
                display_name: "Cloudflare main".into(),
                owner: None,
                labels: BTreeMap::new(),
                management_policy: ManagementPolicy::ObserveOnly,
                deletion_policy: DeletionPolicy::Retain,
                spec: ProviderAccountSpec {
                    provider: CloudProvider::Cloudflare,
                    scope: Some(ProviderAccountScope::Cloudflare {
                        account_id: "0123456789abcdef0123456789abcdef".into(),
                    }),
                    credential_source: CredentialSource::StaticSecret {
                        credential_ref: CredentialRef::new("cloudflare/main").unwrap(),
                    },
                },
            },
        )
        .unwrap()
    }

    fn valid_inspection() -> CredentialInspection {
        CredentialInspection {
            state: CredentialState::Valid,
            identity: Some(ProviderIdentity {
                provider: CloudProvider::Cloudflare,
                principal: "token:7f3b".into(),
                scope: Some("0123456789abcdef0123456789abcdef".into()),
            }),
            credential_revision: Some("secret-rv-7".into()),
            expires_at_unix_ms: None,
            issues: Vec::new(),
        }
    }

    fn service(
        account: Option<ProviderAccount>,
        inspector: Option<Arc<dyn CredentialInspector>>,
        timeout: Duration,
    ) -> CredentialInspectionService {
        CredentialInspectionService::new(
            timeout,
            Arc::new(AccountStore(account)),
            Arc::new(Resolver(inspector)),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn loads_account_and_keeps_revision_internal() {
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
            inspection: valid_inspection(),
        });
        let result = service(Some(account()), Some(inspector), Duration::from_secs(1))
            .inspect(&CloudResourceId::new("cloudflare-main").unwrap())
            .await
            .unwrap();
        assert_eq!(result.provider_account_generation(), 7);
        assert_eq!(result.state(), CredentialState::Valid);
        assert_eq!(result.credential_revision.as_deref(), Some("secret-rv-7"));
        assert!(!format!("{result:?}").contains("secret-rv-7"));
        assert!(!format!("{result:?}").contains("token:7f3b"));
    }

    #[tokio::test]
    async fn missing_account_adapter_and_timeout_are_distinct_failures() {
        let id = CloudResourceId::new("cloudflare-main").unwrap();
        assert!(matches!(
            service(None, None, Duration::from_secs(1))
                .inspect(&id)
                .await,
            Err(CoreError::NotFound(_))
        ));
        assert!(matches!(
            service(Some(account()), None, Duration::from_secs(1))
                .inspect(&id)
                .await,
            Err(CoreError::Unsupported(_))
        ));
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(50),
            inspection: valid_inspection(),
        });
        assert!(matches!(
            service(Some(account()), Some(inspector), Duration::from_millis(1))
                .inspect(&id)
                .await,
            Err(CoreError::Adapter(_))
        ));
    }

    #[tokio::test]
    async fn concurrent_same_generation_inspections_share_one_call() {
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(20),
            inspection: valid_inspection(),
        });
        let service = service(
            Some(account()),
            Some(inspector.clone()),
            Duration::from_secs(1),
        );
        let id = CloudResourceId::new("cloudflare-main").unwrap();
        let (first, second) = tokio::join!(service.inspect(&id), service.inspect(&id));
        assert!(first.is_ok() && second.is_ok());
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn completed_results_are_cooled_down_but_a_new_generation_is_not_reused() {
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
            inspection: valid_inspection(),
        });
        let store = Arc::new(GenerationStore {
            account: account(),
            generation: std::sync::atomic::AtomicU64::new(7),
        });
        let service = CredentialInspectionService::new(
            Duration::from_secs(1),
            store.clone(),
            Arc::new(Resolver(Some(inspector.clone()))),
        )
        .unwrap();
        let id = CloudResourceId::new("cloudflare-main").unwrap();
        service.inspect(&id).await.unwrap();
        service.inspect(&id).await.unwrap();
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 1);

        store.generation.store(8, Ordering::SeqCst);
        assert_eq!(
            service
                .inspect(&id)
                .await
                .unwrap()
                .provider_account_generation(),
            8
        );
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cached_valid_result_is_not_reused_past_its_safe_expiry_window() {
        let now = current_unix_ms().unwrap();
        let mut inspection = valid_inspection();
        inspection.expires_at_unix_ms = Some(now + 100);
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
            inspection,
        });
        let policy = CredentialInspectionPolicy {
            min_refresh_interval: Duration::from_secs(1),
            min_validity: Duration::from_millis(10),
            ..CredentialInspectionPolicy::new(Duration::from_secs(1), 1)
        };
        let service = CredentialInspectionService::with_policy(
            policy,
            Arc::new(AccountStore(Some(account()))),
            Arc::new(Resolver(Some(inspector.clone()))),
        )
        .unwrap();
        let id = CloudResourceId::new("cloudflare-main").unwrap();

        service.inspect(&id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(110)).await;
        assert!(service.inspect(&id).await.is_err());
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn refresh_after_cooldown_remains_singleflight_while_provider_call_runs() {
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(30),
            inspection: valid_inspection(),
        });
        let policy = CredentialInspectionPolicy {
            min_refresh_interval: Duration::from_millis(10),
            min_validity: Duration::from_millis(10),
            ..CredentialInspectionPolicy::new(Duration::from_secs(1), 2)
        };
        let service = CredentialInspectionService::with_policy(
            policy,
            Arc::new(AccountStore(Some(account()))),
            Arc::new(Resolver(Some(inspector.clone()))),
        )
        .unwrap();
        let id = CloudResourceId::new("cloudflare-main").unwrap();

        service.inspect(&id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let first_service = service.clone();
        let first_id = id.clone();
        let first = tokio::spawn(async move { first_service.inspect(&first_id).await });
        while inspector.calls.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
        let second = service.inspect(&id).await;

        assert!(first.await.unwrap().is_ok());
        assert!(second.is_ok());
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 2);
    }

    struct FailingInspector(AtomicUsize);

    #[async_trait]
    impl CredentialInspector for FailingInspector {
        async fn inspect(&self, _: &ProviderAccountSpec) -> CoreResult<CredentialInspection> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(CoreError::Adapter(
                "provider detail must be redacted".into(),
            ))
        }
    }

    #[tokio::test]
    async fn completed_failures_are_also_cooled_down() {
        let inspector = Arc::new(FailingInspector(AtomicUsize::new(0)));
        let service = service(
            Some(account()),
            Some(inspector.clone()),
            Duration::from_secs(1),
        );
        let id = CloudResourceId::new("cloudflare-main").unwrap();
        assert!(service.inspect(&id).await.is_err());
        assert!(service.inspect(&id).await.is_err());
        assert_eq!(inspector.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn deadline_covers_async_resolution_and_singleflight_followers() {
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(100),
            inspection: valid_inspection(),
        });
        let service = CredentialInspectionService::new(
            Duration::from_millis(10),
            Arc::new(AccountStore(Some(account()))),
            Arc::new(SlowResolver {
                delay: Duration::from_millis(2),
                inspector: Some(inspector),
            }),
        )
        .unwrap();
        let id = CloudResourceId::new("cloudflare-main").unwrap();
        let (leader, follower) = tokio::join!(service.inspect(&id), service.inspect(&id));
        assert!(matches!(leader, Err(CoreError::Adapter(_))));
        assert!(matches!(follower, Err(CoreError::Adapter(_))));

        let service = CredentialInspectionService::new(
            Duration::from_millis(5),
            Arc::new(AccountStore(Some(account()))),
            Arc::new(SlowResolver {
                delay: Duration::from_millis(50),
                inspector: None,
            }),
        )
        .unwrap();
        assert!(matches!(
            service.inspect(&id).await,
            Err(CoreError::Adapter(_))
        ));
    }

    #[tokio::test]
    async fn valid_expiry_must_exceed_the_configured_completion_skew() {
        let now: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .try_into()
            .unwrap();
        let mut near_expiry = valid_inspection();
        near_expiry.expires_at_unix_ms = Some(now + 5_000);
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
            inspection: near_expiry,
        });
        let id = CloudResourceId::new("cloudflare-main").unwrap();
        assert!(
            service(Some(account()), Some(inspector), Duration::from_secs(1))
                .inspect(&id)
                .await
                .is_err()
        );

        let mut safe_expiry = valid_inspection();
        safe_expiry.expires_at_unix_ms = Some(now + 60_000);
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
            inspection: safe_expiry,
        });
        assert!(
            service(Some(account()), Some(inspector), Duration::from_secs(1))
                .inspect(&id)
                .await
                .is_ok()
        );
    }

    #[test]
    fn policy_rejects_disabled_or_unreasonably_large_safety_intervals() {
        let base = CredentialInspectionPolicy::new(Duration::from_secs(1), 1);
        for policy in [
            CredentialInspectionPolicy {
                min_refresh_interval: Duration::ZERO,
                ..base
            },
            CredentialInspectionPolicy {
                min_refresh_interval: MAX_MIN_REFRESH_INTERVAL + Duration::from_secs(1),
                ..base
            },
            CredentialInspectionPolicy {
                min_validity: Duration::ZERO,
                ..base
            },
            CredentialInspectionPolicy {
                min_validity: MAX_MIN_VALIDITY + Duration::from_secs(1),
                ..base
            },
        ] {
            assert!(CredentialInspectionService::with_policy(
                policy,
                Arc::new(AccountStore(Some(account()))),
                Arc::new(Resolver(None)),
            )
            .is_err());
        }
    }

    #[tokio::test]
    async fn rejects_provider_scope_and_diagnostic_contract_violations() {
        let id = CloudResourceId::new("cloudflare-main").unwrap();
        let mut invalid = valid_inspection();
        invalid.identity.as_mut().unwrap().scope = Some("wrong".into());
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
            inspection: invalid,
        });
        assert!(
            service(Some(account()), Some(inspector), Duration::from_secs(1))
                .inspect(&id)
                .await
                .is_err()
        );

        let invalid = CredentialInspection {
            state: CredentialState::Invalid,
            identity: None,
            credential_revision: None,
            expires_at_unix_ms: None,
            issues: vec![CredentialIssue {
                kind: CredentialIssueKind::AuthenticationFailed,
                code: "UPPERCASE".into(),
                message: "provider rejected the credential".into(),
            }],
        };
        let inspector = Arc::new(Inspector {
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
            inspection: invalid,
        });
        assert!(
            service(Some(account()), Some(inspector), Duration::from_secs(1))
                .inspect(&id)
                .await
                .is_err()
        );
    }
}
