//! ProxyForwarder: forwards HTTP requests to a specific controller via the gRPC bidirectional stream.

use http::StatusCode;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::federation::proto::{
    center_message::Payload as CenterPayload, CenterMessage, HttpProxyRequest, HttpProxyResponse,
};
use crate::federation::registry::ControllerRegistry;
use crate::federation::registry::SessionView;
use crate::internal_forwarding::{
    proxy_error, record_forward, sanitize_headers, ForwardErrorKind, ForwardHttpOperation,
    OwnerForwarding,
};

pub type PendingProxyMap = Arc<Mutex<HashMap<String, oneshot::Sender<HttpProxyResponse>>>>;

struct PendingProxyGuard {
    pending: PendingProxyMap,
    request_id: String,
}

impl Drop for PendingProxyGuard {
    fn drop(&mut self) {
        self.pending.lock().remove(&self.request_id);
    }
}

pub struct ProxyForwarder {
    registry: ControllerRegistry,
    pending: PendingProxyMap,
    timeout: Duration,
    forwarding: Option<OwnerForwarding>,
}

#[derive(Debug)]
pub enum FencedProxyError {
    StaleOwnership,
    Dispatch(StatusCode, String),
}

impl ProxyForwarder {
    pub fn new(registry: ControllerRegistry, pending: PendingProxyMap, timeout_secs: u64) -> Self {
        Self {
            registry,
            pending,
            timeout: Duration::from_secs(timeout_secs),
            forwarding: None,
        }
    }

    pub fn with_owner_forwarding(mut self, forwarding: OwnerForwarding) -> Self {
        self.forwarding = Some(forwarding);
        self
    }

    pub async fn forward(
        &self,
        controller_id: &str,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<HttpProxyResponse, (StatusCode, String)> {
        let headers = sanitize_headers(headers);
        let routed_method = method.clone();
        let routed_path = path.clone();
        let routed_headers = headers.clone();
        let routed_body = body.clone();
        tokio::time::timeout(self.timeout, async {
            if self.local_session_is_dispatchable(controller_id) {
                let result = self.forward_local(controller_id, method, path, headers, body).await;
                record_forward("proxy", "local", if result.is_ok() { "success" } else { "error" });
                return result;
            }
            let Some(forwarding) = &self.forwarding else {
                return self.forward_local(controller_id, routed_method, routed_path, routed_headers, routed_body).await;
            };
            let id = edgion_center_core::ControllerId::new(controller_id.to_string())
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
            let mut previous = None;
            for attempt in 0..2 {
                let route = forwarding.locator.locate(&id).await
                    .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?
                    .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Controller {} not found or offline", controller_id)))?;
                if route.holder == forwarding.local_holder {
                    return Err((StatusCode::SERVICE_UNAVAILABLE, format!("Controller {} ownership is stale on this replica", controller_id)));
                }
                if previous.as_ref().is_some_and(|old: &edgion_center_core::ControllerOwnerRoute| old == &route) {
                    return Err((StatusCode::SERVICE_UNAVAILABLE, format!("Controller {} ownership did not advance", controller_id)));
                }
                match forwarding.transport.forward_http(
                    &route,
                    controller_id,
                    ForwardHttpOperation {
                        method: routed_method.clone(),
                        path: routed_path.clone(),
                        headers: routed_headers.clone(),
                        body: routed_body.clone(),
                    },
                    self.timeout,
                ).await {
                    Ok(response) => {
                        record_forward("proxy", "remote", "success");
                        tracing::info!(operation = "proxy", route = "remote", controller_id, target_holder = %route.holder, fence_epoch = route.ownership_fence.epoch, "Forwarded operation to owning Center replica");
                        return Ok(response);
                    }
                    Err(error) if attempt == 0 && error.kind == ForwardErrorKind::StaleOwnership => previous = Some(route),
                    Err(error) => {
                        record_forward("proxy", "remote", "error");
                        return Err(proxy_error(error));
                    }
                }
            }
            unreachable!("bounded forwarding loop returns")
        }).await.map_err(|_| (
            StatusCode::GATEWAY_TIMEOUT,
            format!("Proxy request timed out after {}s", self.timeout.as_secs()),
        ))?
    }

    /// Whether this replica holds a dispatchable local session for the
    /// Controller. It answers two questions at once: writes reach it without
    /// a replica hop, AND this process's watch cache is the one fed by that
    /// Controller — so only here can convergence be observed locally.
    pub fn local_session_is_dispatchable(&self, controller_id: &str) -> bool {
        self.registry
            .get_session(controller_id)
            .is_some_and(|session| {
                session.stream_tx.is_some()
                    && (self.forwarding.is_none()
                        || session
                            .ownership
                            .as_ref()
                            .is_some_and(|owner| owner.is_valid()))
            })
    }

    pub async fn forward_local(
        &self,
        controller_id: &str,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<HttpProxyResponse, (StatusCode, String)> {
        let session = self.registry.get_session(controller_id).ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Controller {} not found or offline", controller_id),
            )
        })?;

        self.dispatch_to_session(controller_id, method, path, headers, body, session)
            .await
    }

    pub async fn forward_fenced_local(
        &self,
        controller_id: &str,
        request: ForwardHttpOperation,
        holder: &str,
        fence: &edgion_center_core::OwnershipFence,
    ) -> Result<HttpProxyResponse, FencedProxyError> {
        let session = self
            .registry
            .get_session(controller_id)
            .ok_or(FencedProxyError::StaleOwnership)?;
        if !session.matches_ownership(holder, fence) {
            return Err(FencedProxyError::StaleOwnership);
        }
        self.dispatch_to_session(
            controller_id,
            request.method,
            request.path,
            request.headers,
            request.body,
            session,
        )
        .await
        .map_err(|(status, message)| FencedProxyError::Dispatch(status, message))
    }

    async fn dispatch_to_session(
        &self,
        controller_id: &str,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
        session: SessionView,
    ) -> Result<HttpProxyResponse, (StatusCode, String)> {
        let stream_tx = session.stream_tx.as_ref().ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Controller {} is offline", controller_id),
            )
        })?;

        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel::<HttpProxyResponse>();
        self.pending.lock().insert(request_id.clone(), tx);
        let _pending_guard = PendingProxyGuard {
            pending: self.pending.clone(),
            request_id: request_id.clone(),
        };

        let msg = CenterMessage {
            payload: Some(CenterPayload::HttpProxy(HttpProxyRequest {
                request_id: request_id.clone(),
                method,
                path,
                headers,
                body,
            })),
        };

        tokio::select! {
            biased;
            _ = session.session_cancel.cancelled() => {
                self.pending.lock().remove(&request_id);
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("Controller {} is offline", controller_id),
                ));
            }
            result = stream_tx.send(msg) => {
                result.map_err(|_| {
                    self.pending.lock().remove(&request_id);
                    (
                        StatusCode::BAD_GATEWAY,
                        "Failed to send proxy request: stream closed".to_string(),
                    )
                })?;
            }
        }

        tokio::select! {
            biased;
            _ = session.session_cancel.cancelled() => {
                Err((
                    StatusCode::BAD_GATEWAY,
                    format!("Controller {} disconnected while proxy result was pending", controller_id),
                ))
            }
            result = tokio::time::timeout(self.timeout, rx) => {
                result
                    .map_err(|_| (
                        StatusCode::GATEWAY_TIMEOUT,
                        format!("Proxy request timed out after {}s", self.timeout.as_secs()),
                    ))?
                    .map_err(|_| (
                        StatusCode::BAD_GATEWAY,
                        "Proxy response channel dropped".to_string(),
                    ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal_forwarding::{ForwardError, InternalForwardTransport};
    use edgion_center_core::{
        ControllerId, ControllerOwnerLocator, ControllerOwnerRoute, CoreResult, OwnershipFence,
    };

    fn owner_route(epoch: u64) -> ControllerOwnerRoute {
        ControllerOwnerRoute {
            holder: format!("center-{epoch}/uid-{epoch}"),
            endpoint: format!("https://10.0.0.{epoch}:12252"),
            ownership_fence: OwnershipFence {
                token: format!("token-{epoch}"),
                epoch,
            },
        }
    }

    /// Hands out a strictly newer owner on every call, so a retry always has
    /// somewhere new to go and cannot be stopped by the no-advance guard.
    struct AdvancingOwner(Mutex<u64>);

    #[async_trait::async_trait]
    impl ControllerOwnerLocator for AdvancingOwner {
        async fn locate(&self, _: &ControllerId) -> CoreResult<Option<ControllerOwnerRoute>> {
            let mut epoch = self.0.lock();
            *epoch += 1;
            Ok(Some(owner_route(*epoch)))
        }
    }

    struct FailingTransport {
        attempts: Mutex<u32>,
        kind: ForwardErrorKind,
    }

    #[async_trait::async_trait]
    impl InternalForwardTransport for FailingTransport {
        async fn forward_http(
            &self,
            _: &ControllerOwnerRoute,
            _: &str,
            _: ForwardHttpOperation,
            _: Duration,
        ) -> Result<HttpProxyResponse, ForwardError> {
            *self.attempts.lock() += 1;
            Err(ForwardError {
                kind: self.kind,
                message: "forward failed".to_string(),
            })
        }

        async fn forward_evict(
            &self,
            _: &ControllerOwnerRoute,
            _: &str,
        ) -> Result<(), ForwardError> {
            unreachable!()
        }
    }

    fn forwarder_over(transport: Arc<FailingTransport>) -> ProxyForwarder {
        ProxyForwarder::new(
            ControllerRegistry::new(),
            Arc::new(Mutex::new(HashMap::new())),
            1,
        )
        .with_owner_forwarding(OwnerForwarding {
            locator: Arc::new(AdvancingOwner(Mutex::new(0))),
            transport,
            local_holder: "center-local/uid".to_string(),
        })
    }

    /// Returns how many times the request actually reached the transport. The
    /// error message is asserted here because `Deadline` maps to the same 504
    /// the outer request budget produces — only the message distinguishes a
    /// transport failure from an expired budget.
    async fn forward_and_count(kind: ForwardErrorKind) -> u32 {
        let transport = Arc::new(FailingTransport {
            attempts: Mutex::new(0),
            kind,
        });
        let error = forwarder_over(transport.clone())
            .forward(
                "c1",
                "PUT".to_string(),
                "/api/v1/namespaced/edgionconfigdata/ns/name".to_string(),
                HashMap::new(),
                Vec::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.1, "forward failed",
            "{kind:?} ended on the wrong error"
        );
        let attempts = *transport.attempts.lock();
        attempts
    }

    /// A failure that may have already reached the Controller must never be
    /// replayed: the mutation could have executed. Only `StaleOwnership`, which
    /// the owning replica raises strictly before dispatch, is retryable.
    #[tokio::test]
    async fn uncertain_dispatch_failures_are_never_replayed() {
        for kind in [
            ForwardErrorKind::Unavailable,
            ForwardErrorKind::Deadline,
            ForwardErrorKind::Rejected,
        ] {
            assert_eq!(forward_and_count(kind).await, 1, "{kind:?} must not retry");
        }
    }

    /// `AdvancingOwner` hands out a strictly newer owner each call, so this
    /// stops at two because the retry budget is one — not because the
    /// no-advance guard fired.
    #[tokio::test]
    async fn stale_ownership_is_retried_exactly_once_against_a_newer_owner() {
        assert_eq!(forward_and_count(ForwardErrorKind::StaleOwnership).await, 2);
    }

    #[tokio::test]
    async fn missing_controller_returns_not_found_without_pending_state() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let proxy = ProxyForwarder::new(ControllerRegistry::new(), pending.clone(), 1);
        let error = proxy
            .forward(
                "missing",
                "GET".to_string(),
                "/health".to_string(),
                HashMap::new(),
                Vec::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.0, StatusCode::NOT_FOUND);
        assert!(pending.lock().is_empty());
    }

    #[tokio::test]
    async fn canceled_session_cannot_enqueue_proxy_request() {
        let registry = ControllerRegistry::new();
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(1);
        let registration = registry.register_cancellable(
            "c1".to_string(),
            crate::federation::proto::RegisterRequest::default(),
            stream_tx,
            "s1".to_string(),
        );
        registration.session_cancel.cancel();
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let proxy = ProxyForwarder::new(registry, pending.clone(), 1);
        let error = proxy
            .forward(
                "c1",
                "GET".to_string(),
                "/health".to_string(),
                HashMap::new(),
                Vec::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
        assert!(pending.lock().is_empty());
        assert!(stream_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancellation_wins_when_proxy_send_becomes_ready_at_same_time() {
        let registry = ControllerRegistry::new();
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(1);
        stream_tx.try_send(CenterMessage::default()).unwrap();
        let registration = registry.register_cancellable(
            "c1".to_string(),
            crate::federation::proto::RegisterRequest::default(),
            stream_tx,
            "s1".to_string(),
        );
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let proxy = ProxyForwarder::new(registry, pending.clone(), 1);
        let task = tokio::spawn(async move {
            proxy
                .forward(
                    "c1",
                    "GET".to_string(),
                    "/health".to_string(),
                    HashMap::new(),
                    Vec::new(),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while pending.lock().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        registration.session_cancel.cancel();
        let _filler = stream_rx.recv().await.unwrap();
        assert!(task.await.unwrap().is_err());
        assert!(pending.lock().is_empty());
        assert!(stream_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancellation_after_enqueue_wakes_waiter_and_cleans_pending() {
        let registry = ControllerRegistry::new();
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(1);
        let registration = registry.register_cancellable(
            "c1".to_string(),
            crate::federation::proto::RegisterRequest::default(),
            stream_tx,
            "s1".to_string(),
        );
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let proxy = ProxyForwarder::new(registry, pending.clone(), 30);
        let task = tokio::spawn(async move {
            proxy
                .forward(
                    "c1",
                    "GET".to_string(),
                    "/health".to_string(),
                    HashMap::new(),
                    Vec::new(),
                )
                .await
        });
        let _enqueued = stream_rx.recv().await.unwrap();
        registration.session_cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancellation must wake waiter")
            .unwrap();
        assert!(result.is_err());
        assert!(pending.lock().is_empty());
    }
}
