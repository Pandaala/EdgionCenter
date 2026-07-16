use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use edgion_center_core::{
    CoordinationRole, Coordinator, CoreError, CoreResult, Leadership, ReleaseOutcome,
    RenewalOutcome,
};
use k8s_openapi::{
    api::coordination::v1::{Lease, LeaseSpec},
    apimachinery::pkg::apis::meta::v1::MicroTime,
};
use kube::{api::PostParams, Api, Client};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const ROLE_ANNOTATION: &str = "center.edgion.io/coordination-role";
pub(crate) const TOKEN_ANNOTATION: &str = "center.edgion.io/fencing-token";
pub(crate) const EPOCH_ANNOTATION: &str = "center.edgion.io/fencing-epoch";
const MAX_CONFLICT_RETRIES: usize = 8;

type LeaseObservation = (String, Duration);
type LeaseObservations = Arc<Mutex<HashMap<String, LeaseObservation>>>;

#[derive(Debug)]
enum LeaseError {
    Conflict,
    Other(String),
}

impl From<kube::Error> for LeaseError {
    fn from(error: kube::Error) -> Self {
        match &error {
            kube::Error::Api(response) if response.code == 409 => Self::Conflict,
            _ => Self::Other(error.to_string()),
        }
    }
}

#[async_trait]
trait LeaseResources: Send + Sync {
    async fn get(&self, name: &str) -> Result<Option<Lease>, LeaseError>;
    async fn create(&self, lease: &Lease) -> Result<Lease, LeaseError>;
    async fn replace(&self, name: &str, lease: &Lease) -> Result<Lease, LeaseError>;
}

struct KubernetesLeaseResources {
    api: Api<Lease>,
}

#[async_trait]
impl LeaseResources for KubernetesLeaseResources {
    async fn get(&self, name: &str) -> Result<Option<Lease>, LeaseError> {
        self.api.get_opt(name).await.map_err(Into::into)
    }

    async fn create(&self, lease: &Lease) -> Result<Lease, LeaseError> {
        self.api
            .create(&PostParams::default(), lease)
            .await
            .map_err(Into::into)
    }

    async fn replace(&self, name: &str, lease: &Lease) -> Result<Lease, LeaseError> {
        self.api
            .replace(name, &PostParams::default(), lease)
            .await
            .map_err(Into::into)
    }
}

trait Clock: Send + Sync {
    fn wall_now(&self) -> DateTime<Utc>;
    fn monotonic_now(&self) -> Duration;
}

struct SystemClock {
    started: Instant,
}

impl Clock for SystemClock {
    fn wall_now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn monotonic_now(&self) -> Duration {
        self.started.elapsed()
    }
}

/// Optimistic, fencing-token based coordination using namespaced Kubernetes
/// Lease objects. A fresh acquire by the same replica rotates the token, so an
/// older local session can no longer renew or release the role.
#[derive(Clone)]
pub struct KubernetesLeaseCoordinator {
    resources: Arc<dyn LeaseResources>,
    clock: Arc<dyn Clock>,
    holder: String,
    lease_duration: Duration,
    observations: LeaseObservations,
}

impl KubernetesLeaseCoordinator {
    pub fn new(
        client: Client,
        namespace: &str,
        holder: impl Into<String>,
        lease_duration: Duration,
    ) -> CoreResult<Self> {
        let holder = holder.into();
        validate_config(namespace, &holder, lease_duration)?;
        Ok(Self {
            resources: Arc::new(KubernetesLeaseResources {
                api: Api::namespaced(client, namespace),
            }),
            clock: Arc::new(SystemClock {
                started: Instant::now(),
            }),
            holder,
            lease_duration,
            observations: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    #[cfg(test)]
    fn with_resources(
        resources: Arc<dyn LeaseResources>,
        clock: Arc<dyn Clock>,
        holder: &str,
        lease_duration: Duration,
    ) -> Self {
        Self {
            resources,
            clock,
            holder: holder.to_string(),
            lease_duration,
            observations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn adapter_error(error: LeaseError) -> CoreError {
        match error {
            LeaseError::Conflict => {
                CoreError::Conflict("Kubernetes Lease resourceVersion conflict".to_string())
            }
            LeaseError::Other(message) => CoreError::Adapter(message),
        }
    }

    fn duration_seconds(&self) -> i32 {
        self.lease_duration.as_secs() as i32
    }

    fn leadership(&self, role: CoordinationRole, token: String, epoch: u64) -> Leadership {
        Leadership {
            role,
            holder: self.holder.clone(),
            fencing_token: token,
            fencing_epoch: epoch,
            valid_for_millis: self.lease_duration.as_millis() as u64,
        }
    }

    fn verify_role(lease: &Lease, role_key: &str) -> CoreResult<()> {
        match lease
            .metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(ROLE_ANNOTATION))
        {
            Some(stored) if stored == role_key => Ok(()),
            Some(stored) => Err(CoreError::Conflict(format!(
                "Kubernetes Lease name collision between {stored:?} and {role_key:?}"
            ))),
            None => Err(CoreError::Conflict(
                "managed Kubernetes Lease omitted coordination role annotation".to_string(),
            )),
        }
    }

    fn token(lease: &Lease) -> Option<&str> {
        lease
            .metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(TOKEN_ANNOTATION))
            .map(String::as_str)
    }

    fn epoch(lease: &Lease) -> CoreResult<u64> {
        lease
            .metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(EPOCH_ANNOTATION))
            .map_or(Ok(0), |epoch| {
                epoch.parse::<u64>().map_err(|_| {
                    CoreError::Conflict("Kubernetes Lease has invalid fencing epoch".to_string())
                })
            })
    }

    fn is_active(&self, name: &str, lease: &Lease, now: Duration) -> bool {
        let Some(spec) = lease.spec.as_ref() else {
            return false;
        };
        if spec
            .holder_identity
            .as_deref()
            .unwrap_or_default()
            .is_empty()
        {
            return false;
        }
        let duration = i64::from(spec.lease_duration_seconds.unwrap_or_default());
        if duration <= 0 {
            return false;
        }
        // Never compare another replica's wall-clock renewTime to ours. Track
        // how long the same resourceVersion remains unchanged, matching the
        // skew-tolerant model used by Kubernetes leader election. A new
        // contender waits one conservative full duration on first observation.
        let marker = lease
            .metadata
            .resource_version
            .clone()
            .or_else(|| Self::token(lease).map(str::to_string))
            .unwrap_or_default();
        let mut observations = self.observations.lock().unwrap();
        let observed_at = match observations.get(name) {
            Some((seen, observed_at)) if seen == &marker => *observed_at,
            _ => {
                observations.insert(name.to_string(), (marker, now));
                now
            }
        };
        now.saturating_sub(observed_at) < Duration::from_secs(duration as u64)
    }

    fn acquired_lease(
        &self,
        name: &str,
        role_key: &str,
        current: Option<&Lease>,
        now: DateTime<Utc>,
        token: &str,
        epoch: u64,
    ) -> Lease {
        let mut annotations = current
            .and_then(|lease| lease.metadata.annotations.clone())
            .unwrap_or_default();
        annotations.insert(ROLE_ANNOTATION.to_string(), role_key.to_string());
        annotations.insert(TOKEN_ANNOTATION.to_string(), token.to_string());
        annotations.insert(EPOCH_ANNOTATION.to_string(), epoch.to_string());
        let previous_holder = current
            .and_then(|lease| lease.spec.as_ref())
            .and_then(|spec| spec.holder_identity.as_deref());
        let transitions = current
            .and_then(|lease| lease.spec.as_ref())
            .and_then(|spec| spec.lease_transitions)
            .unwrap_or_default()
            .saturating_add(i32::from(
                previous_holder.is_some_and(|holder| holder != self.holder),
            ));
        Lease {
            metadata: kube::api::ObjectMeta {
                name: Some(name.to_string()),
                namespace: current.and_then(|lease| lease.metadata.namespace.clone()),
                resource_version: current.and_then(|lease| lease.metadata.resource_version.clone()),
                annotations: Some(annotations),
                ..kube::api::ObjectMeta::default()
            },
            spec: Some(LeaseSpec {
                holder_identity: Some(self.holder.clone()),
                lease_duration_seconds: Some(self.duration_seconds()),
                acquire_time: Some(MicroTime(now)),
                renew_time: Some(MicroTime(now)),
                lease_transitions: Some(transitions),
                ..LeaseSpec::default()
            }),
        }
    }
}

#[async_trait]
impl Coordinator for KubernetesLeaseCoordinator {
    async fn acquire(&self, role: CoordinationRole) -> CoreResult<Leadership> {
        let role_key = role_key(&role);
        let name = lease_name(&role);
        for _ in 0..MAX_CONFLICT_RETRIES {
            let wall_now = self.clock.wall_now();
            let monotonic_now = self.clock.monotonic_now();
            let token = Uuid::new_v4().to_string();
            match self.resources.get(&name).await {
                Ok(Some(current)) => {
                    Self::verify_role(&current, &role_key)?;
                    let current_holder = current
                        .spec
                        .as_ref()
                        .and_then(|spec| spec.holder_identity.as_deref());
                    if self.is_active(&name, &current, monotonic_now)
                        && current_holder != Some(&self.holder)
                    {
                        return Err(CoreError::Conflict(format!(
                            "coordination role {role_key:?} is held by another replica"
                        )));
                    }
                    let epoch = Self::epoch(&current)?.checked_add(1).ok_or_else(|| {
                        CoreError::Conflict("Kubernetes fencing epoch exhausted".to_string())
                    })?;
                    let replacement = self.acquired_lease(
                        &name,
                        &role_key,
                        Some(&current),
                        wall_now,
                        &token,
                        epoch,
                    );
                    match self.resources.replace(&name, &replacement).await {
                        Ok(_) => return Ok(self.leadership(role, token, epoch)),
                        Err(LeaseError::Conflict) => continue,
                        Err(error) => return Err(Self::adapter_error(error)),
                    }
                }
                Ok(None) => {
                    let epoch = 1;
                    let lease =
                        self.acquired_lease(&name, &role_key, None, wall_now, &token, epoch);
                    match self.resources.create(&lease).await {
                        Ok(_) => return Ok(self.leadership(role, token, epoch)),
                        Err(LeaseError::Conflict) => continue,
                        Err(error) => return Err(Self::adapter_error(error)),
                    }
                }
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(format!(
            "coordination role {role_key:?} remained conflicted after {MAX_CONFLICT_RETRIES} attempts"
        )))
    }

    async fn renew(&self, leadership: &Leadership) -> CoreResult<RenewalOutcome> {
        let role_key = role_key(&leadership.role);
        let name = lease_name(&leadership.role);
        for _ in 0..MAX_CONFLICT_RETRIES {
            let Some(mut lease) = self
                .resources
                .get(&name)
                .await
                .map_err(Self::adapter_error)?
            else {
                return Ok(RenewalOutcome::Lost);
            };
            Self::verify_role(&lease, &role_key)?;
            let owns = lease
                .spec
                .as_ref()
                .and_then(|spec| spec.holder_identity.as_deref())
                == Some(&self.holder)
                && Self::token(&lease) == Some(leadership.fencing_token.as_str())
                && Self::epoch(&lease)? == leadership.fencing_epoch;
            if !owns {
                return Ok(RenewalOutcome::Lost);
            }
            let now = self.clock.wall_now();
            let spec = lease.spec.get_or_insert_with(LeaseSpec::default);
            spec.renew_time = Some(MicroTime(now));
            spec.lease_duration_seconds = Some(self.duration_seconds());
            match self.resources.replace(&name, &lease).await {
                Ok(_) => {
                    return Ok(RenewalOutcome::Renewed(self.leadership(
                        leadership.role.clone(),
                        leadership.fencing_token.clone(),
                        leadership.fencing_epoch,
                    )))
                }
                Err(LeaseError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(format!(
            "coordination role {role_key:?} renewal remained conflicted"
        )))
    }

    async fn release(&self, leadership: &Leadership) -> CoreResult<ReleaseOutcome> {
        let role_key = role_key(&leadership.role);
        let name = lease_name(&leadership.role);
        for _ in 0..MAX_CONFLICT_RETRIES {
            let Some(mut lease) = self
                .resources
                .get(&name)
                .await
                .map_err(Self::adapter_error)?
            else {
                return Ok(ReleaseOutcome::Lost);
            };
            Self::verify_role(&lease, &role_key)?;
            let owns = lease
                .spec
                .as_ref()
                .and_then(|spec| spec.holder_identity.as_deref())
                == Some(&self.holder)
                && Self::token(&lease) == Some(leadership.fencing_token.as_str())
                && Self::epoch(&lease)? == leadership.fencing_epoch;
            if !owns {
                return Ok(ReleaseOutcome::Lost);
            }
            lease
                .metadata
                .annotations
                .get_or_insert_with(BTreeMap::new)
                .remove(TOKEN_ANNOTATION);
            let spec = lease.spec.get_or_insert_with(LeaseSpec::default);
            spec.holder_identity = None;
            spec.renew_time = Some(MicroTime(self.clock.wall_now()));
            match self.resources.replace(&name, &lease).await {
                Ok(_) => return Ok(ReleaseOutcome::Released),
                Err(LeaseError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(format!(
            "coordination role {role_key:?} release remained conflicted"
        )))
    }
}

fn validate_config(namespace: &str, holder: &str, duration: Duration) -> CoreResult<()> {
    if namespace.trim().is_empty() || namespace.chars().any(char::is_control) {
        return Err(CoreError::Adapter(
            "Kubernetes coordination namespace must be non-empty".to_string(),
        ));
    }
    if holder.trim().is_empty() || holder.len() > 253 || holder.chars().any(char::is_control) {
        return Err(CoreError::Adapter(
            "Kubernetes coordination holder must be 1..=253 bytes".to_string(),
        ));
    }
    if duration < Duration::from_secs(3) || duration > Duration::from_secs(i32::MAX as u64) {
        return Err(CoreError::Adapter(
            "Kubernetes Lease duration must be between 3 seconds and i32::MAX seconds".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn role_key(role: &CoordinationRole) -> String {
    match role {
        CoordinationRole::ControllerOwner(id) => format!("controller-owner:{id}"),
        CoordinationRole::Maintenance(name) => format!("maintenance:{name}"),
    }
}

pub(crate) fn lease_name(role: &CoordinationRole) -> String {
    let key = role_key(role);
    let kind = match role {
        CoordinationRole::ControllerOwner(_) => "controller-owner",
        CoordinationRole::Maintenance(_) => "maintenance",
    };
    let digest = hex::encode(Sha256::digest(key.as_bytes()));
    format!("{kind}-{}", &digest[..16])
}

#[cfg(test)]
mod tests {
    use std::{pin::pin, sync::Mutex};

    use http::{Request, Response};
    use kube::client::Body;
    use tower_test::mock;

    use super::*;

    struct FixedClock(Mutex<(DateTime<Utc>, Duration)>);

    impl FixedClock {
        fn new(now: DateTime<Utc>) -> Self {
            Self(Mutex::new((now, Duration::ZERO)))
        }

        fn advance(&self, duration: chrono::Duration) {
            let mut state = self.0.lock().unwrap();
            state.0 += duration;
            state.1 += duration.to_std().unwrap();
        }

        fn step_wall(&self, duration: chrono::Duration) {
            self.0.lock().unwrap().0 += duration;
        }
    }

    impl Clock for FixedClock {
        fn wall_now(&self) -> DateTime<Utc> {
            self.0.lock().unwrap().0
        }

        fn monotonic_now(&self) -> Duration {
            self.0.lock().unwrap().1
        }
    }

    #[derive(Default)]
    struct MemoryLeases(Mutex<Option<Lease>>);

    #[async_trait]
    impl LeaseResources for MemoryLeases {
        async fn get(&self, _name: &str) -> Result<Option<Lease>, LeaseError> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn create(&self, lease: &Lease) -> Result<Lease, LeaseError> {
            let mut current = self.0.lock().unwrap();
            if current.is_some() {
                return Err(LeaseError::Conflict);
            }
            let mut lease = lease.clone();
            lease.metadata.resource_version = Some("1".to_string());
            *current = Some(lease.clone());
            Ok(lease)
        }

        async fn replace(&self, _name: &str, lease: &Lease) -> Result<Lease, LeaseError> {
            let mut current = self.0.lock().unwrap();
            let expected = current
                .as_ref()
                .and_then(|lease| lease.metadata.resource_version.as_deref());
            if expected != lease.metadata.resource_version.as_deref() {
                return Err(LeaseError::Conflict);
            }
            let version = expected.unwrap_or("0").parse::<u64>().unwrap() + 1;
            let mut lease = lease.clone();
            lease.metadata.resource_version = Some(version.to_string());
            *current = Some(lease.clone());
            Ok(lease)
        }
    }

    fn role() -> CoordinationRole {
        CoordinationRole::ControllerOwner("cluster-a/controller-0".to_string())
    }

    fn coordinator(
        resources: Arc<MemoryLeases>,
        clock: Arc<FixedClock>,
        holder: &str,
    ) -> KubernetesLeaseCoordinator {
        KubernetesLeaseCoordinator::with_resources(
            resources,
            clock,
            holder,
            Duration::from_secs(15),
        )
    }

    #[tokio::test]
    async fn acquire_renew_release_uses_fencing_token() {
        let resources = Arc::new(MemoryLeases::default());
        let clock = Arc::new(FixedClock::new(Utc::now()));
        let coordinator = coordinator(resources, clock.clone(), "center-0");
        let first = coordinator.acquire(role()).await.unwrap();
        clock.advance(chrono::Duration::seconds(3));
        let renewed = coordinator.renew(&first).await.unwrap();
        assert!(matches!(renewed, RenewalOutcome::Renewed(_)));
        assert_eq!(
            coordinator.release(&first).await.unwrap(),
            ReleaseOutcome::Released
        );
        assert_eq!(
            coordinator.renew(&first).await.unwrap(),
            RenewalOutcome::Lost
        );
    }

    #[tokio::test]
    async fn live_other_holder_conflicts_then_expiry_allows_takeover() {
        let resources = Arc::new(MemoryLeases::default());
        let clock = Arc::new(FixedClock::new(Utc::now()));
        let first = coordinator(resources.clone(), clock.clone(), "center-0");
        let second = coordinator(resources, clock.clone(), "center-1");
        let old = first.acquire(role()).await.unwrap();
        assert!(matches!(
            second.acquire(role()).await,
            Err(CoreError::Conflict(_))
        ));
        clock.step_wall(chrono::Duration::days(365));
        assert!(matches!(
            second.acquire(role()).await,
            Err(CoreError::Conflict(_))
        ));
        clock.advance(chrono::Duration::seconds(16));
        let new = second.acquire(role()).await.unwrap();
        assert_eq!(new.holder, "center-1");
        assert_eq!(first.renew(&old).await.unwrap(), RenewalOutcome::Lost);
        assert_eq!(first.release(&old).await.unwrap(), ReleaseOutcome::Lost);
    }

    #[tokio::test]
    async fn same_replica_reacquire_rotates_token_and_fences_old_session() {
        let resources = Arc::new(MemoryLeases::default());
        let clock = Arc::new(FixedClock::new(Utc::now()));
        let coordinator = coordinator(resources, clock, "center-0");
        let old = coordinator.acquire(role()).await.unwrap();
        let new = coordinator.acquire(role()).await.unwrap();
        assert_ne!(old.fencing_token, new.fencing_token);
        assert_eq!(old.fencing_epoch + 1, new.fencing_epoch);
        assert_eq!(coordinator.renew(&old).await.unwrap(), RenewalOutcome::Lost);
        assert!(matches!(
            coordinator.renew(&new).await.unwrap(),
            RenewalOutcome::Renewed(_)
        ));
        let lease = coordinator
            .resources
            .get(&lease_name(&role()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lease.spec.unwrap().lease_transitions, Some(0));
    }

    #[test]
    fn names_are_bounded_and_configuration_fails_closed() {
        assert!(lease_name(&CoordinationRole::ControllerOwner("x".repeat(10_000))).len() <= 63);
        assert!(validate_config("", "center-0", Duration::from_secs(15)).is_err());
        assert!(validate_config("management", "", Duration::from_secs(15)).is_err());
        assert!(validate_config("management", "center-0", Duration::from_secs(2)).is_err());
    }

    #[tokio::test]
    async fn real_kube_client_uses_namespaced_coordination_lease_api() {
        let (service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let server = tokio::spawn(async move {
            let mut handle = pin!(handle);
            let name = lease_name(&role());
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), http::Method::GET);
            assert_eq!(
                request.uri().path(),
                format!("/apis/coordination.k8s.io/v1/namespaces/management/leases/{name}")
            );
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "apiVersion": "v1",
                            "kind": "Status",
                            "status": "Failure",
                            "reason": "NotFound",
                            "message": "not found",
                            "code": 404
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            );

            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), http::Method::POST);
            assert_eq!(
                request.uri().path(),
                "/apis/coordination.k8s.io/v1/namespaces/management/leases"
            );
            let mut created = Lease::default();
            created.metadata.name = Some(name);
            created.metadata.namespace = Some("management".to_string());
            created.metadata.resource_version = Some("1".to_string());
            send.send_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&created).unwrap()))
                    .unwrap(),
            );
        });

        let coordinator = KubernetesLeaseCoordinator::new(
            Client::new(service, "management"),
            "management",
            "center-0",
            Duration::from_secs(15),
        )
        .unwrap();
        let leadership = coordinator.acquire(role()).await.unwrap();
        assert_eq!(leadership.holder, "center-0");
        assert!(!leadership.fencing_token.is_empty());
        server.await.unwrap();
    }
}
