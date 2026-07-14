use edgion_center_core::{AuditEvent, AuditWriter};
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StructuredAuditEvent {
    ts: i64,
    actor: String,
    provider: String,
    method: String,
    path: String,
    target_controller: Option<String>,
    status: i32,
    source_ip: Option<String>,
    request_id: Option<String>,
    detail: Option<String>,
}

impl From<AuditEvent> for StructuredAuditEvent {
    fn from(event: AuditEvent) -> Self {
        Self {
            ts: event.ts,
            actor: event.actor,
            provider: event.provider,
            method: event.method,
            path: event.path,
            target_controller: event.target_controller,
            status: event.status,
            source_ip: event.source_ip,
            request_id: event.request_id,
            detail: event.detail,
        }
    }
}

/// Structured runtime audit sink for Kubernetes-native deployments.
///
/// The tracing subscriber controls the concrete stdout encoding (JSON in the
/// production binary). This writer emits bounded field names and never stores
/// events in an application database.
pub struct StructuredStdoutAudit;

impl AuditWriter for StructuredStdoutAudit {
    fn record(&self, event: AuditEvent) {
        let event = StructuredAuditEvent::from(event);
        tracing::info!(
            target: "edgion_center_audit",
            audit = true,
            ts = event.ts,
            actor = %event.actor,
            provider = %event.provider,
            method = %event.method,
            path = %event.path,
            target_controller = event.target_controller.as_deref(),
            status = event.status,
            source_ip = event.source_ip.as_deref(),
            request_id = event.request_id.as_deref(),
            detail = event.detail.as_deref(),
            "center runtime audit event"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdout_writer_accepts_lossless_runtime_event_without_storage() {
        let event = AuditEvent {
            ts: 1,
            actor: "alice".to_string(),
            provider: "oidc".to_string(),
            method: "POST".to_string(),
            path: "/api/v1/controllers/c1/reload".to_string(),
            target_controller: Some("c1".to_string()),
            status: 202,
            source_ip: Some("192.0.2.10".to_string()),
            request_id: Some("req-1".to_string()),
            detail: Some("accepted".to_string()),
        };
        let structured = StructuredAuditEvent::from(event.clone());
        assert_eq!(
            serde_json::to_value(&structured).unwrap(),
            serde_json::json!({
                "ts": 1,
                "actor": "alice",
                "provider": "oidc",
                "method": "POST",
                "path": "/api/v1/controllers/c1/reload",
                "targetController": "c1",
                "status": 202,
                "sourceIp": "192.0.2.10",
                "requestId": "req-1",
                "detail": "accepted"
            })
        );
        StructuredStdoutAudit.record(event);
    }
}
