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

pub type PendingCommandMap = Arc<Mutex<HashMap<String, oneshot::Sender<CommandResponse>>>>;

pub struct Commander {
    registry: ControllerRegistry,
    pending: PendingCommandMap,
    timeout: Duration,
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
        }
    }

    pub async fn send_command(
        &self,
        controller_id: &str,
        command: crate::federation::proto::command_request::Command,
    ) -> Result<CommandResponse> {
        let session = self
            .registry
            .get_session(controller_id)
            .ok_or_else(|| anyhow!("Controller {} not found or offline", controller_id))?;

        let stream_tx = session
            .stream_tx
            .as_ref()
            .ok_or_else(|| anyhow!("Controller {} is offline", controller_id))?;

        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel::<CommandResponse>();
        self.pending.lock().insert(request_id.clone(), tx);

        let msg = CenterMessage {
            payload: Some(CenterPayload::Command(CommandRequest {
                request_id: request_id.clone(),
                command: Some(command),
            })),
        };

        stream_tx.send(msg).await.map_err(|_| {
            self.pending.lock().remove(&request_id);
            anyhow!("Failed to send command: stream closed")
        })?;

        tokio::time::timeout(self.timeout, rx)
            .await
            .map_err(|_| {
                self.pending.lock().remove(&request_id);
                anyhow!("Command timed out after {}s", self.timeout.as_secs())
            })?
            .map_err(|_| {
                self.pending.lock().remove(&request_id);
                anyhow!("Command response channel dropped")
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::federation::proto::{command_request::Command, ReloadCommand};

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
}
