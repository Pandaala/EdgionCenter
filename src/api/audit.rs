//! Audit-log read endpoint — exposes the append-only `audit_log` table as a
//! filtered, paginated list. Mounted at exactly
//! `/api/v1/center/admin/audit-logs` (the audit middleware excludes this path
//! from self-logging via `AUDIT_READ_PATH`).
//!
//! Read-only: the rows are written by the background `AuditSink` task; this
//! handler only calls `Store::list_audit`. When the DB is disabled the handler
//! returns an empty list (HTTP 200) rather than an error, matching the
//! "degrade gracefully" behavior expected by the dashboard.

use axum::{extract::{Query, State}, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::ListResponse;
use crate::store::audit::{AuditFilter, AuditRecord};

/// Default page size when `limit` is omitted.
const DEFAULT_LIMIT: i64 = 100;
/// Hard cap on `limit` to bound a single response.
const MAX_LIMIT: i64 = 1000;

/// Query parameters for `GET /api/v1/center/admin/audit-logs`.
///
/// All fields are optional. `limit` defaults to `DEFAULT_LIMIT` and is clamped
/// to `[1, MAX_LIMIT]`; `offset` defaults to `0` and is floored at `0`.
/// `since`/`until` are inclusive unix-second bounds on the record timestamp.
#[derive(Debug, Default, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub actor: Option<String>,
    pub controller: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
}

/// Serde view of an `AuditRecord`. camelCase to match the controllers DTO
/// convention so the frontend can consume both lists uniformly.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecordDto {
    pub ts: i64,
    pub actor: String,
    pub provider: String,
    pub method: String,
    pub path: String,
    pub target_controller: Option<String>,
    pub status: i32,
    pub source_ip: Option<String>,
    pub request_id: Option<String>,
    pub detail: Option<String>,
}

impl From<AuditRecord> for AuditRecordDto {
    fn from(r: AuditRecord) -> Self {
        Self {
            ts: r.ts,
            actor: r.actor,
            provider: r.provider,
            method: r.method,
            path: r.path,
            target_controller: r.target_controller,
            status: r.status,
            source_ip: r.source_ip,
            request_id: r.request_id,
            detail: r.detail,
        }
    }
}

/// Normalize the raw query into a `(AuditFilter, limit, offset)` tuple with the
/// limit clamped and offset floored. Pulled out so it can be unit-tested
/// without an HTTP round-trip.
fn normalize(q: AuditQuery) -> (AuditFilter, i64, i64) {
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = q.offset.unwrap_or(0).max(0);
    let filter = AuditFilter {
        actor: q.actor.filter(|s| !s.is_empty()),
        controller: q.controller.filter(|s| !s.is_empty()),
        since: q.since,
        until: q.until,
    };
    (filter, limit, offset)
}

/// `GET /api/v1/center/admin/audit-logs` — list audit records, newest first.
pub async fn audit_list_handler(State(state): State<ApiState>, Query(q): Query<AuditQuery>) -> impl IntoResponse {
    let (filter, limit, offset) = normalize(q);

    let Some(db) = &state.db else {
        // DB disabled: nothing persisted, return an empty list (not an error).
        return Json(ListResponse::<AuditRecordDto>::success(Vec::new())).into_response();
    };

    match db.list_audit(&filter, limit, offset).await {
        Ok(rows) => {
            let dtos: Vec<AuditRecordDto> = rows.into_iter().map(AuditRecordDto::from).collect();
            Json(ListResponse::success(dtos)).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ListResponse::<AuditRecordDto>::error(e.to_string())),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::sync::Arc;

    fn rec(ts: i64, actor: &str, controller: Option<&str>, status: i32) -> AuditRecord {
        AuditRecord {
            ts,
            actor: actor.to_string(),
            provider: "local".to_string(),
            method: "POST".to_string(),
            path: "/api/v1/center/admin/controllers".to_string(),
            target_controller: controller.map(|c| c.to_string()),
            status,
            source_ip: Some("10.0.0.1".to_string()),
            request_id: None,
            detail: None,
        }
    }

    /// Build an `ApiState` whose only meaningful field is `db`; everything else
    /// is a default/empty construction sufficient for the audit handler.
    fn state_with_db(db: Option<Arc<Store>>) -> ApiState {
        use crate::aggregator::ResourceAggregator;
        use crate::commander::Commander;
        use crate::fed_sync::registry::ControllerRegistry;
        use crate::metadata_store::CenterMetaDataStore;
        use crate::proxy::ProxyForwarder;
        use crate::watch_cache::{CenterSyncClient, CenterWatchCacheRegistry};
        use parking_lot::Mutex;
        use std::collections::HashMap;

        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let commander = Arc::new(Commander::new(registry.clone(), Arc::new(Mutex::new(HashMap::new())), 5));
        let proxy = Arc::new(ProxyForwarder::new(registry.clone(), Arc::new(Mutex::new(HashMap::new())), 5));
        ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            commander,
            proxy,
            db,
            metadata_store,
            sync_client,
            registry,
            db_required: false,
            authz_mode: crate::config::AuthzMode::Rbac,
            db_auth_enabled: false,
        }
    }

    /// Extract the `Vec<AuditRecordDto>` out of the handler's JSON response body.
    async fn body_items(resp: axum::response::Response) -> Vec<serde_json::Value> {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        json["data"].as_array().cloned().unwrap_or_default()
    }

    #[tokio::test]
    async fn lists_with_filters() {
        let db = Store::open_in_memory().await.unwrap();
        db.insert_audit(&rec(100, "alice", Some("c1"), 200)).await.unwrap();
        db.insert_audit(&rec(200, "bob", None, 204)).await.unwrap();
        db.insert_audit(&rec(300, "alice", Some("c2"), 500)).await.unwrap();
        let state = state_with_db(Some(Arc::new(db)));

        // No filter: all three, newest first (ts DESC).
        let q = AuditQuery { limit: Some(50), ..Default::default() };
        let resp = audit_list_handler(State(state.clone()), Query(q)).await.into_response();
        let items = body_items(resp).await;
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["ts"], 300);
        assert_eq!(items[1]["ts"], 200);
        assert_eq!(items[2]["ts"], 100);
        // camelCase DTO field is exposed for the frontend.
        assert_eq!(items[0]["targetController"], "c2");

        // Actor filter: only alice's two rows, still ts DESC.
        let q = AuditQuery {
            actor: Some("alice".to_string()),
            limit: Some(50),
            ..Default::default()
        };
        let resp = audit_list_handler(State(state.clone()), Query(q)).await.into_response();
        let items = body_items(resp).await;
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["ts"], 300);
        assert_eq!(items[1]["ts"], 100);
        assert!(items.iter().all(|r| r["actor"] == "alice"));

        // Limit clamps the page size.
        let q = AuditQuery { limit: Some(1), ..Default::default() };
        let resp = audit_list_handler(State(state), Query(q)).await.into_response();
        let items = body_items(resp).await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["ts"], 300, "newest row when limited to 1");
    }

    #[tokio::test]
    async fn empty_when_db_disabled() {
        let state = state_with_db(None);
        let resp = audit_list_handler(State(state), Query(AuditQuery::default())).await.into_response();
        let items = body_items(resp).await;
        assert!(items.is_empty());
    }
}
