//! CommandDispatcher: sends CommandRequest to a specific controller and awaits response.

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::federation::proto::{
    center_message::Payload as CenterPayload, CenterMessage, CommandRequest, CommandResponse,
};
use crate::federation::registry::ControllerRegistry;
use crate::federation::registry::SessionView;
use crate::internal_forwarding::{
    command_error, record_forward, ForwardErrorKind, OwnerForwarding,
};

pub type PendingCommandMap = Arc<Mutex<HashMap<String, oneshot::Sender<CommandResponse>>>>;

struct PendingCommandGuard {
    pending: PendingCommandMap,
    request_id: String,
}

impl Drop for PendingCommandGuard {
    fn drop(&mut self) {
        self.pending.lock().remove(&self.request_id);
    }
}

pub struct Commander {
    registry: ControllerRegistry,
    pending: PendingCommandMap,
    timeout: Duration,
    forwarding: Option<OwnerForwarding>,
}

#[derive(Debug)]
pub enum FencedCommandError {
    StaleOwnership,
    Dispatch(anyhow::Error),
}

impl Commander {
    pub fn new(
        registry: ControllerRegistry,
        pending: PendingCommandMap,
        timeout_secs: u64,
    ) -> Self {
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

    pub async fn send_command(
        &self,
        controller_id: &str,
        command: crate::federation::proto::command_request::Command,
    ) -> Result<CommandResponse> {
        let command_for_route = command.clone();
        tokio::time::timeout(self.timeout, async {
            if self.local_session_is_dispatchable(controller_id) {
                let result = self.send_local_command(controller_id, command, None).await;
                record_forward("command", "local", if result.is_ok() { "success" } else { "error" });
                return result;
            }
            let Some(forwarding) = &self.forwarding else {
                return self.send_local_command(controller_id, command_for_route, None).await;
            };
            let id = edgion_center_core::ControllerId::new(controller_id.to_string())?;
            let mut previous = None;
            for attempt in 0..2 {
                let route = forwarding.locator.locate(&id).await?
                    .ok_or_else(|| anyhow!("Controller {} not found or offline", controller_id))?;
                if route.holder == forwarding.local_holder {
                    return Err(anyhow!("Controller {} ownership is stale on this replica", controller_id));
                }
                if previous.as_ref().is_some_and(|old: &edgion_center_core::ControllerOwnerRoute| old == &route) {
                    return Err(anyhow!("Controller {} ownership did not advance", controller_id));
                }
                match forwarding.transport.forward_command(
                    &route,
                    controller_id,
                    command_for_route.clone(),
                    self.timeout,
                ).await {
                    Ok(response) => {
                        record_forward("command", "remote", "success");
                        tracing::info!(operation = "command", route = "remote", controller_id, target_holder = %route.holder, fence_epoch = route.ownership_fence.epoch, "Forwarded operation to owning Center replica");
                        return Ok(response);
                    }
                    Err(error) if attempt == 0 && error.kind == ForwardErrorKind::StaleOwnership => {
                        previous = Some(route);
                    }
                    Err(error) => {
                        record_forward("command", "remote", "error");
                        return Err(command_error(error));
                    }
                }
            }
            unreachable!("bounded forwarding loop returns")
        }).await.map_err(|_| anyhow!("Command timed out after {}s", self.timeout.as_secs()))?
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

    pub async fn send_local_command(
        &self,
        controller_id: &str,
        command: crate::federation::proto::command_request::Command,
        expected: Option<(&str, &edgion_center_core::OwnershipFence)>,
    ) -> Result<CommandResponse> {
        let session = self
            .registry
            .get_session(controller_id)
            .ok_or_else(|| anyhow!("Controller {} not found or offline", controller_id))?;

        if expected.is_some_and(|(holder, fence)| !session.matches_ownership(holder, fence)) {
            return Err(anyhow!("stale controller ownership"));
        }

        self.dispatch_to_session(controller_id, command, session)
            .await
    }

    pub async fn send_fenced_local_command(
        &self,
        controller_id: &str,
        command: crate::federation::proto::command_request::Command,
        holder: &str,
        fence: &edgion_center_core::OwnershipFence,
    ) -> Result<CommandResponse, FencedCommandError> {
        let session = self
            .registry
            .get_session(controller_id)
            .ok_or(FencedCommandError::StaleOwnership)?;
        if !session.matches_ownership(holder, fence) {
            return Err(FencedCommandError::StaleOwnership);
        }
        self.dispatch_to_session(controller_id, command, session)
            .await
            .map_err(FencedCommandError::Dispatch)
    }

    async fn dispatch_to_session(
        &self,
        controller_id: &str,
        command: crate::federation::proto::command_request::Command,
        session: SessionView,
    ) -> Result<CommandResponse> {
        let stream_tx = session
            .stream_tx
            .as_ref()
            .ok_or_else(|| anyhow!("Controller {} is offline", controller_id))?;

        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel::<CommandResponse>();
        self.pending.lock().insert(request_id.clone(), tx);
        let _pending_guard = PendingCommandGuard {
            pending: self.pending.clone(),
            request_id: request_id.clone(),
        };

        let msg = CenterMessage {
            payload: Some(CenterPayload::Command(CommandRequest {
                request_id: request_id.clone(),
                command: Some(command),
            })),
        };

        tokio::select! {
            biased;
            _ = session.session_cancel.cancelled() => {
                self.pending.lock().remove(&request_id);
                return Err(anyhow!("Controller {} is offline", controller_id));
            }
            result = stream_tx.send(msg) => {
                result.map_err(|_| {
                    self.pending.lock().remove(&request_id);
                    anyhow!("Failed to send command: stream closed")
                })?;
            }
        }

        tokio::select! {
            biased;
            _ = session.session_cancel.cancelled() => {
                Err(anyhow!("Controller {} disconnected while command result was pending", controller_id))
            }
            result = tokio::time::timeout(self.timeout, rx) => {
                result
                    .map_err(|_| anyhow!("Command timed out after {}s", self.timeout.as_secs()))?
                    .map_err(|_| anyhow!("Command response channel dropped"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::federation::proto::{command_request::Command, ReloadCommand};
    use crate::internal_forwarding::{ForwardError, ForwardErrorKind, InternalForwardTransport};
    use async_trait::async_trait;
    use edgion_center_core::{
        ControllerId, ControllerOwnerLocator, ControllerOwnerRoute, CoreResult, OwnershipFence,
    };
    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct Routes(Mutex<VecDeque<ControllerOwnerRoute>>);

    #[async_trait]
    impl ControllerOwnerLocator for Routes {
        async fn locate(&self, _: &ControllerId) -> CoreResult<Option<ControllerOwnerRoute>> {
            Ok(self.0.lock().pop_front())
        }
    }

    struct Transport {
        calls: AtomicUsize,
        results: Mutex<VecDeque<Result<CommandResponse, ForwardError>>>,
    }

    #[async_trait]
    impl InternalForwardTransport for Transport {
        async fn forward_command(
            &self,
            _: &ControllerOwnerRoute,
            _: &str,
            _: Command,
            _: Duration,
        ) -> Result<CommandResponse, ForwardError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.results.lock().pop_front().expect("test result")
        }

        async fn forward_http(
            &self,
            _: &ControllerOwnerRoute,
            _: &str,
            _: crate::internal_forwarding::ForwardHttpOperation,
            _: Duration,
        ) -> Result<crate::federation::proto::HttpProxyResponse, ForwardError> {
            unreachable!()
        }

        async fn forward_evict(
            &self,
            _: &ControllerOwnerRoute,
            _: &str,
        ) -> Result<(), ForwardError> {
            unreachable!()
        }
    }

    fn route(holder: &str, epoch: u64) -> ControllerOwnerRoute {
        ControllerOwnerRoute {
            holder: holder.to_string(),
            endpoint: "https://10.0.0.2:12252".to_string(),
            ownership_fence: OwnershipFence {
                token: format!("token-{epoch}"),
                epoch,
            },
        }
    }

    fn routed_commander(
        routes: Vec<ControllerOwnerRoute>,
        results: Vec<Result<CommandResponse, ForwardError>>,
    ) -> (Commander, Arc<Transport>) {
        let transport = Arc::new(Transport {
            calls: AtomicUsize::new(0),
            results: Mutex::new(results.into()),
        });
        let forwarding = OwnerForwarding {
            locator: Arc::new(Routes(Mutex::new(routes.into()))),
            transport: transport.clone(),
            local_holder: "center-a/uid-a".to_string(),
        };
        let commander = Commander::new(
            ControllerRegistry::new(),
            Arc::new(Mutex::new(HashMap::new())),
            1,
        )
        .with_owner_forwarding(forwarding);
        (commander, transport)
    }

    #[tokio::test]
    async fn remote_owner_dispatches_when_session_is_not_local() {
        let expected = CommandResponse {
            request_id: "op".to_string(),
            success: true,
            message: String::new(),
        };
        let (commander, transport) =
            routed_commander(vec![route("center-b/uid-b", 1)], vec![Ok(expected.clone())]);
        let actual = commander
            .send_command("c1", Command::Reload(ReloadCommand {}))
            .await
            .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn explicit_stale_fence_reresolves_once_but_unavailable_never_replays() {
        let stale = ForwardError::stale("stale");
        let expected = CommandResponse {
            request_id: "op".to_string(),
            success: true,
            message: String::new(),
        };
        let (commander, transport) = routed_commander(
            vec![route("center-b/uid-b", 1), route("center-c/uid-c", 2)],
            vec![Err(stale), Ok(expected)],
        );
        commander
            .send_command("c1", Command::Reload(ReloadCommand {}))
            .await
            .unwrap();
        assert_eq!(transport.calls.load(Ordering::SeqCst), 2);

        let unavailable = ForwardError {
            kind: ForwardErrorKind::Unavailable,
            message: "connection lost".to_string(),
        };
        let (commander, transport) = routed_commander(
            vec![route("center-b/uid-b", 1), route("center-c/uid-c", 2)],
            vec![Err(unavailable)],
        );
        assert!(commander
            .send_command("c1", Command::Reload(ReloadCommand {}))
            .await
            .is_err());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn missing_controller_fails_without_leaking_pending_requests() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let commander = Commander::new(ControllerRegistry::new(), pending.clone(), 1);
        let error = commander
            .send_command("missing", Command::Reload(ReloadCommand {}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not found or offline"));
        assert!(pending.lock().is_empty());
    }

    #[tokio::test]
    async fn canceled_session_cannot_enqueue_command() {
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
        let commander = Commander::new(registry, pending.clone(), 1);
        let result = commander
            .send_command("c1", Command::Reload(ReloadCommand {}))
            .await;
        assert!(result.is_err());
        assert!(pending.lock().is_empty());
        assert!(stream_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancellation_wins_when_command_send_becomes_ready_at_same_time() {
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
        let commander = Commander::new(registry, pending.clone(), 1);
        let task = tokio::spawn(async move {
            commander
                .send_command("c1", Command::Reload(ReloadCommand {}))
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
        let commander = Commander::new(registry, pending.clone(), 30);
        let task = tokio::spawn(async move {
            commander
                .send_command("c1", Command::Reload(ReloadCommand {}))
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
