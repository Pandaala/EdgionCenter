use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use edgion_center_core::{
    ControllerId, ControllerOwnerLocator, ControllerOwnerRoute, CoordinationRole, CoreError,
    CoreResult, OwnershipFence,
};
use k8s_openapi::api::{coordination::v1::Lease, core::v1::Pod};
use kube::{Api, Client};

use crate::lease::{lease_name, role_key, EPOCH_ANNOTATION, ROLE_ANNOTATION, TOKEN_ANNOTATION};

#[async_trait]
trait OwnerResources: Send + Sync {
    async fn get_lease(&self, name: &str) -> Result<Option<Lease>, kube::Error>;
    async fn get_pod(&self, name: &str) -> Result<Option<Pod>, kube::Error>;
}

struct KubernetesOwnerResources {
    leases: Api<Lease>,
    pods: Api<Pod>,
}

#[async_trait]
impl OwnerResources for KubernetesOwnerResources {
    async fn get_lease(&self, name: &str) -> Result<Option<Lease>, kube::Error> {
        self.leases.get_opt(name).await
    }

    async fn get_pod(&self, name: &str) -> Result<Option<Pod>, kube::Error> {
        self.pods.get_opt(name).await
    }
}

trait Clock: Send + Sync {
    fn now(&self) -> Duration;
}

struct SystemClock(Instant);

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

/// Resolves a Controller's authoritative Lease owner to a verified Pod IP.
///
/// The Controller CRD is intentionally not consulted: it is a display
/// projection and can lag a Lease transition. The holder identity is bound to
/// a Pod UID so a replacement Pod with the same name cannot inherit a route.
#[derive(Clone)]
pub struct KubernetesControllerOwnerLocator {
    resources: Arc<dyn OwnerResources>,
    clock: Arc<dyn Clock>,
    internal_port: u16,
    observations: Arc<Mutex<HashMap<String, (String, Duration)>>>,
}

impl KubernetesControllerOwnerLocator {
    pub fn new(client: Client, namespace: &str, internal_port: u16) -> CoreResult<Self> {
        if namespace.trim().is_empty() || namespace.chars().any(char::is_control) {
            return Err(CoreError::Adapter(
                "Kubernetes owner locator namespace must be non-empty".to_string(),
            ));
        }
        if internal_port == 0 {
            return Err(CoreError::Adapter(
                "Kubernetes owner locator internal port must be non-zero".to_string(),
            ));
        }
        Ok(Self {
            resources: Arc::new(KubernetesOwnerResources {
                leases: Api::namespaced(client.clone(), namespace),
                pods: Api::namespaced(client, namespace),
            }),
            clock: Arc::new(SystemClock(Instant::now())),
            internal_port,
            observations: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    #[cfg(test)]
    fn with_resources(
        resources: Arc<dyn OwnerResources>,
        clock: Arc<dyn Clock>,
        internal_port: u16,
    ) -> Self {
        Self {
            resources,
            clock,
            internal_port,
            observations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn active_owner(
        &self,
        lease_name: &str,
        lease: &Lease,
        expected_role: &str,
    ) -> CoreResult<Option<(String, OwnershipFence)>> {
        let annotations = lease.metadata.annotations.as_ref().ok_or_else(|| {
            CoreError::Conflict("managed Controller Lease omitted annotations".to_string())
        })?;
        if annotations.get(ROLE_ANNOTATION).map(String::as_str) != Some(expected_role) {
            return Err(CoreError::Conflict(
                "Controller Lease role annotation did not match its name".to_string(),
            ));
        }
        let Some(spec) = lease.spec.as_ref() else {
            return Ok(None);
        };
        let Some(holder) = spec
            .holder_identity
            .as_deref()
            .filter(|holder| !holder.is_empty())
        else {
            return Ok(None);
        };
        let Some(duration_seconds) = spec.lease_duration_seconds.filter(|duration| *duration > 0)
        else {
            return Ok(None);
        };
        let marker = lease
            .metadata
            .resource_version
            .clone()
            .or_else(|| annotations.get(TOKEN_ANNOTATION).cloned())
            .unwrap_or_default();
        let now = self.clock.now();
        let mut observations = self.observations.lock().unwrap();
        let observed_at = match observations.get(lease_name) {
            Some((seen, observed_at)) if seen == &marker => *observed_at,
            _ => {
                observations.insert(lease_name.to_string(), (marker, now));
                now
            }
        };
        if now.saturating_sub(observed_at) >= Duration::from_secs(duration_seconds as u64) {
            return Ok(None);
        }
        drop(observations);
        let token = annotations
            .get(TOKEN_ANNOTATION)
            .filter(|token| !token.is_empty())
            .cloned()
            .ok_or_else(|| {
                CoreError::Conflict("active Controller Lease omitted fencing token".to_string())
            })?;
        let epoch = annotations
            .get(EPOCH_ANNOTATION)
            .ok_or_else(|| {
                CoreError::Conflict("active Controller Lease omitted fencing epoch".to_string())
            })?
            .parse::<u64>()
            .map_err(|_| {
                CoreError::Conflict("active Controller Lease has invalid fencing epoch".to_string())
            })?;
        if epoch == 0 {
            return Err(CoreError::Conflict(
                "active Controller Lease has zero fencing epoch".to_string(),
            ));
        }
        Ok(Some((holder.to_string(), OwnershipFence { token, epoch })))
    }

    fn parse_holder(holder: &str) -> CoreResult<(&str, &str)> {
        let mut parts = holder.split('/');
        let pod_name = parts.next().unwrap_or_default();
        let pod_uid = parts.next().unwrap_or_default();
        if pod_name.is_empty() || pod_uid.is_empty() || parts.next().is_some() {
            return Err(CoreError::Conflict(
                "Controller Lease holder must be podName/podUID".to_string(),
            ));
        }
        Ok((pod_name, pod_uid))
    }

    fn pod_ip(pod: &Pod, expected_uid: &str) -> Option<String> {
        if pod.metadata.uid.as_deref() != Some(expected_uid)
            || pod.metadata.deletion_timestamp.is_some()
            || pod
                .status
                .as_ref()
                .and_then(|status| status.phase.as_deref())
                != Some("Running")
        {
            return None;
        }
        pod.status
            .as_ref()
            .and_then(|status| status.pod_ip.as_deref())
            .filter(|ip| !ip.is_empty())
            .map(str::to_string)
    }

    fn endpoint(&self, pod_ip: &str) -> String {
        if pod_ip.contains(':') {
            format!("https://[{pod_ip}]:{}", self.internal_port)
        } else {
            format!("https://{pod_ip}:{}", self.internal_port)
        }
    }
}

#[async_trait]
impl ControllerOwnerLocator for KubernetesControllerOwnerLocator {
    async fn locate(&self, id: &ControllerId) -> CoreResult<Option<ControllerOwnerRoute>> {
        let role = CoordinationRole::ControllerOwner(id.to_string());
        let name = lease_name(&role);
        let Some(lease) = self
            .resources
            .get_lease(&name)
            .await
            .map_err(|error| CoreError::Adapter(error.to_string()))?
        else {
            return Ok(None);
        };
        let Some((holder, ownership_fence)) = self.active_owner(&name, &lease, &role_key(&role))?
        else {
            return Ok(None);
        };
        let (pod_name, pod_uid) = Self::parse_holder(&holder)?;
        let Some(pod) = self
            .resources
            .get_pod(pod_name)
            .await
            .map_err(|error| CoreError::Adapter(error.to_string()))?
        else {
            return Ok(None);
        };
        let Some(pod_ip) = Self::pod_ip(&pod, pod_uid) else {
            return Ok(None);
        };
        Ok(Some(ControllerOwnerRoute {
            holder,
            endpoint: self.endpoint(&pod_ip),
            ownership_fence,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        pin::pin,
        sync::{
            atomic::{AtomicU64, Ordering},
            Mutex,
        },
    };

    use chrono::{DateTime, Duration as ChronoDuration, Utc};
    use http::{Request, Response};
    use k8s_openapi::{
        api::core::v1::PodStatus,
        apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta, Time},
    };
    use kube::client::Body;
    use tower_test::mock;

    use super::*;

    struct FixedClock(AtomicU64);

    impl Clock for FixedClock {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(Ordering::SeqCst))
        }
    }

    impl FixedClock {
        fn advance(&self, seconds: u64) {
            self.0.fetch_add(seconds, Ordering::SeqCst);
        }
    }

    struct MemoryResources {
        lease: Option<Lease>,
        pod: Option<Pod>,
        pod_reads: Mutex<usize>,
    }

    #[async_trait]
    impl OwnerResources for MemoryResources {
        async fn get_lease(&self, _name: &str) -> Result<Option<Lease>, kube::Error> {
            Ok(self.lease.clone())
        }

        async fn get_pod(&self, _name: &str) -> Result<Option<Pod>, kube::Error> {
            *self.pod_reads.lock().unwrap() += 1;
            Ok(self.pod.clone())
        }
    }

    fn controller_id() -> ControllerId {
        ControllerId::new("cluster-a/controller-0").unwrap()
    }

    fn active_lease(now: DateTime<Utc>, holder: &str) -> Lease {
        let role = CoordinationRole::ControllerOwner(controller_id().to_string());
        Lease {
            metadata: ObjectMeta {
                name: Some(lease_name(&role)),
                annotations: Some(BTreeMap::from([
                    (ROLE_ANNOTATION.to_string(), role_key(&role)),
                    (TOKEN_ANNOTATION.to_string(), "token-7".to_string()),
                    (EPOCH_ANNOTATION.to_string(), "7".to_string()),
                ])),
                ..ObjectMeta::default()
            },
            spec: Some(k8s_openapi::api::coordination::v1::LeaseSpec {
                holder_identity: Some(holder.to_string()),
                lease_duration_seconds: Some(15),
                renew_time: Some(MicroTime(now - ChronoDuration::seconds(2))),
                ..Default::default()
            }),
        }
    }

    fn running_pod(name: &str, uid: &str, ip: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                uid: Some(uid.to_string()),
                ..ObjectMeta::default()
            },
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                pod_ip: Some(ip.to_string()),
                ..PodStatus::default()
            }),
            ..Pod::default()
        }
    }

    fn make_locator(
        _now: DateTime<Utc>,
        lease: Option<Lease>,
        pod: Option<Pod>,
    ) -> (KubernetesControllerOwnerLocator, Arc<MemoryResources>) {
        let resources = Arc::new(MemoryResources {
            lease,
            pod,
            pod_reads: Mutex::new(0),
        });
        (
            KubernetesControllerOwnerLocator::with_resources(
                resources.clone(),
                Arc::new(FixedClock(AtomicU64::new(0))),
                12252,
            ),
            resources,
        )
    }

    #[tokio::test]
    async fn resolves_active_lease_to_uid_bound_running_pod() {
        let now = Utc::now();
        let holder = "center-0/uid-0";
        let (locator, _) = make_locator(
            now,
            Some(active_lease(now, holder)),
            Some(running_pod("center-0", "uid-0", "10.0.0.8")),
        );

        let route = locator.locate(&controller_id()).await.unwrap().unwrap();
        assert_eq!(route.holder, holder);
        assert_eq!(route.endpoint, "https://10.0.0.8:12252");
        assert_eq!(route.ownership_fence.token, "token-7");
        assert_eq!(route.ownership_fence.epoch, 7);
    }

    #[tokio::test]
    async fn expired_or_released_lease_is_not_routed_and_does_not_read_pod() {
        let now = Utc::now();
        let expired = active_lease(now - ChronoDuration::hours(24), "center-0/uid-0");
        let resources = Arc::new(MemoryResources {
            lease: Some(expired),
            pod: Some(running_pod("center-0", "uid-0", "10.0.0.8")),
            pod_reads: Mutex::new(0),
        });
        let clock = Arc::new(FixedClock(AtomicU64::new(0)));
        let locator = KubernetesControllerOwnerLocator::with_resources(
            resources.clone(),
            clock.clone(),
            12252,
        );
        assert!(locator.locate(&controller_id()).await.unwrap().is_some());
        clock.advance(15);
        assert!(locator.locate(&controller_id()).await.unwrap().is_none());
        assert_eq!(*resources.pod_reads.lock().unwrap(), 1);

        let mut released = active_lease(now, "center-0/uid-0");
        released.spec.as_mut().unwrap().holder_identity = None;
        let (locator, resources) = make_locator(now, Some(released), None);
        assert!(locator.locate(&controller_id()).await.unwrap().is_none());
        assert_eq!(*resources.pod_reads.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn replacement_terminating_and_non_running_pods_fail_closed() {
        let now = Utc::now();
        let replacement = running_pod("center-0", "replacement-uid", "10.0.0.8");
        let mut terminating = running_pod("center-0", "uid-0", "10.0.0.8");
        terminating.metadata.deletion_timestamp = Some(Time(now));
        let mut pending = running_pod("center-0", "uid-0", "10.0.0.8");
        pending.status.as_mut().unwrap().phase = Some("Pending".to_string());
        let mut missing_ip = running_pod("center-0", "uid-0", "10.0.0.8");
        missing_ip.status.as_mut().unwrap().pod_ip = None;

        for pod in [replacement, terminating, pending, missing_ip] {
            let (locator, _) =
                make_locator(now, Some(active_lease(now, "center-0/uid-0")), Some(pod));
            assert!(locator.locate(&controller_id()).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn malformed_authority_is_rejected_instead_of_routed() {
        let now = Utc::now();
        let mut missing_token = active_lease(now, "center-0/uid-0");
        missing_token
            .metadata
            .annotations
            .as_mut()
            .unwrap()
            .remove(TOKEN_ANNOTATION);
        let (locator, _) = make_locator(now, Some(missing_token), None);
        assert!(matches!(
            locator.locate(&controller_id()).await,
            Err(CoreError::Conflict(_))
        ));

        let (locator, _) = make_locator(now, Some(active_lease(now, "center-0")), None);
        assert!(matches!(
            locator.locate(&controller_id()).await,
            Err(CoreError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn ipv6_endpoint_is_bracketed() {
        let now = Utc::now();
        let (locator, _) = make_locator(
            now,
            Some(active_lease(now, "center-0/uid-0")),
            Some(running_pod("center-0", "uid-0", "2001:db8::8")),
        );
        assert_eq!(
            locator
                .locate(&controller_id())
                .await
                .unwrap()
                .unwrap()
                .endpoint,
            "https://[2001:db8::8]:12252"
        );
    }

    #[tokio::test]
    async fn real_client_reads_namespaced_lease_then_pod() {
        let now = Utc::now();
        let lease = active_lease(now, "center-0/uid-0");
        let pod = running_pod("center-0", "uid-0", "10.0.0.8");
        let (service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let server = tokio::spawn(async move {
            let mut handle = pin!(handle);
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), http::Method::GET);
            assert_eq!(
                request.uri().path(),
                format!(
                    "/apis/coordination.k8s.io/v1/namespaces/management/leases/{}",
                    lease_name(&CoordinationRole::ControllerOwner(
                        controller_id().to_string()
                    ))
                )
            );
            send.send_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&lease).unwrap()))
                    .unwrap(),
            );

            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), http::Method::GET);
            assert_eq!(
                request.uri().path(),
                "/api/v1/namespaces/management/pods/center-0"
            );
            send.send_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&pod).unwrap()))
                    .unwrap(),
            );
        });

        let locator = KubernetesControllerOwnerLocator::new(
            Client::new(service, "management"),
            "management",
            12252,
        )
        .unwrap();
        assert!(locator.locate(&controller_id()).await.unwrap().is_some());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn configuration_rejects_empty_namespace_and_zero_port() {
        let (service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = Client::new(service, "management");
        assert!(KubernetesControllerOwnerLocator::new(client.clone(), "", 12252).is_err());
        assert!(KubernetesControllerOwnerLocator::new(client, "management", 0).is_err());
    }
}
