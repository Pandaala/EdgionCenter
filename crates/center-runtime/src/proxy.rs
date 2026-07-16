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
                let result = self.forward_local(controller_id, method, path, headers, body, None).await;
                record_forward("proxy", "local", if result.is_ok() { "success" } else { "error" });
                return result;
            }
            let Some(forwarding) = &self.forwarding else {
                return self.forward_local(controller_id, routed_method, routed_path, routed_headers, routed_body, None).await;
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

    /// Forward to exactly the owner route observed by the caller. This path
    /// deliberately performs no owner re-resolution or stale-owner replay so
    /// the response can be attributed to the caller's CRD revision.
    pub async fn forward_expected_route(
        &self,
        route: &edgion_center_core::ControllerOwnerRoute,
        controller_id: &str,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<HttpProxyResponse, (StatusCode, String)> {
        let headers = sanitize_headers(headers);
        tokio::time::timeout(self.timeout, async {
            let forwarding = self.forwarding.as_ref().ok_or_else(|| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "owner-fenced forwarding is not configured".to_string(),
                )
            })?;
            if route.holder == forwarding.local_holder {
                let result = self
                    .forward_local(
                        controller_id,
                        method,
                        path,
                        headers,
                        body,
                        Some((&route.holder, &route.ownership_fence)),
                    )
                    .await;
                record_forward(
                    "proxy_fenced",
                    "local",
                    if result.is_ok() { "success" } else { "error" },
                );
                return result;
            }

            let result = forwarding
                .transport
                .forward_http(
                    route,
                    controller_id,
                    ForwardHttpOperation {
                        method,
                        path,
                        headers,
                        body,
                    },
                    self.timeout,
                )
                .await;
            record_forward(
                "proxy_fenced",
                "remote",
                if result.is_ok() { "success" } else { "error" },
            );
            result.map_err(proxy_error)
        })
        .await
        .map_err(|_| {
            (
                StatusCode::GATEWAY_TIMEOUT,
                format!(
                    "Fenced proxy request timed out after {}s",
                    self.timeout.as_secs()
                ),
            )
        })?
    }

    fn local_session_is_dispatchable(&self, controller_id: &str) -> bool {
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
        expected: Option<(&str, &edgion_center_core::OwnershipFence)>,
    ) -> Result<HttpProxyResponse, (StatusCode, String)> {
        let session = self.registry.get_session(controller_id).ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Controller {} not found or offline", controller_id),
            )
        })?;

        if expected.is_some_and(|(holder, fence)| !session.matches_ownership(holder, fence)) {
            return Err((
                StatusCode::PRECONDITION_FAILED,
                "stale controller ownership".to_string(),
            ));
        }

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

#[async_trait::async_trait]
impl crate::poll::ControllerHttpClient for ProxyForwarder {
    async fn request(
        &self,
        controller_id: &str,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<crate::poll::ControllerHttpResponse, String> {
        self.forward(controller_id, method, path, headers, body)
            .await
            .map(|response| crate::poll::ControllerHttpResponse {
                status_code: response.status_code,
                body: response.body,
            })
            .map_err(|(_, message)| message)
    }

    async fn request_fenced(
        &self,
        controller_id: &str,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
        expected_owner: &edgion_center_core::ControllerOwnerRoute,
    ) -> Result<crate::poll::ControllerHttpResponse, String> {
        self.forward_expected_route(expected_owner, controller_id, method, path, headers, body)
            .await
            .map(|response| crate::poll::ControllerHttpResponse {
                status_code: response.status_code,
                body: response.body,
            })
            .map_err(|(_, message)| message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        federation::proto::{command_request::Command, CommandResponse},
        internal_forwarding::{ForwardError, InternalForwardTransport},
    };
    use edgion_center_core::{
        ControllerId, ControllerOwnerLocator, ControllerOwnerRoute, CoreResult, OwnershipFence,
    };

    struct NeverLocate;

    #[async_trait::async_trait]
    impl ControllerOwnerLocator for NeverLocate {
        async fn locate(&self, _: &ControllerId) -> CoreResult<Option<ControllerOwnerRoute>> {
            panic!("fenced forwarding must not re-resolve ownership")
        }
    }

    struct RecordingTransport(Mutex<Vec<ControllerOwnerRoute>>);

    #[async_trait::async_trait]
    impl InternalForwardTransport for RecordingTransport {
        async fn forward_command(
            &self,
            _: &ControllerOwnerRoute,
            _: &str,
            _: Command,
            _: Duration,
        ) -> Result<CommandResponse, ForwardError> {
            unreachable!()
        }

        async fn forward_http(
            &self,
            route: &ControllerOwnerRoute,
            _: &str,
            _: ForwardHttpOperation,
            _: Duration,
        ) -> Result<HttpProxyResponse, ForwardError> {
            self.0.lock().push(route.clone());
            Ok(HttpProxyResponse {
                request_id: "poll".to_string(),
                status_code: 200,
                headers: HashMap::new(),
                body: br#"{"data":[]}"#.to_vec(),
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

    #[tokio::test]
    async fn fenced_poll_dispatches_exact_route_without_reresolution() {
        let expected = ControllerOwnerRoute {
            holder: "center-a/uid-a".to_string(),
            endpoint: "https://10.0.0.1:12252".to_string(),
            ownership_fence: OwnershipFence {
                token: "token-a".to_string(),
                epoch: 1,
            },
        };
        let transport = Arc::new(RecordingTransport(Mutex::new(Vec::new())));
        let proxy = ProxyForwarder::new(
            ControllerRegistry::new(),
            Arc::new(Mutex::new(HashMap::new())),
            1,
        )
        .with_owner_forwarding(OwnerForwarding {
            locator: Arc::new(NeverLocate),
            transport: transport.clone(),
            local_holder: "center-request/uid".to_string(),
        });

        proxy
            .forward_expected_route(
                &expected,
                "c1",
                "GET".to_string(),
                "/api/v1/region-routes/effective".to_string(),
                HashMap::new(),
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(transport.0.lock().as_slice(), &[expected]);
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
