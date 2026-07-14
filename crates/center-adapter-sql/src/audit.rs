//! `audit_log` table access — append-only attribution of mutating admin actions.
//!
//! Mirrors the runtime-sqlx style of `controllers.rs`: every query is built with
//! `sqlx::query` + `.bind` (never the compile-time macros), and the two backends
//! are dispatched by matching on `&self.pool`. Writes are issued from the
//! background `AuditSink` task (see `crate::common::audit`), so `insert_audit`
//! is off the request hot path.

use super::{Pool, Store};
use edgion_center_core::{
    AuditEvent, AuditFilter as CoreAuditFilter, AuditPage, AuditReader, AuditWriter, CoreError,
    CoreResult, Page,
};
use sqlx::Row;
use std::sync::atomic::{AtomicU64, Ordering};

pub const AUDIT_DROPPED_METRIC: &str = "edgion_center_audit_dropped_total";
static AUDIT_DROPPED: AtomicU64 = AtomicU64::new(0);

pub fn audit_dropped_total() -> u64 {
    AUDIT_DROPPED.load(Ordering::Relaxed)
}

/// A single audit-log row to be persisted. `id` is assigned by the database.
pub struct AuditRecord {
    /// Wall-clock seconds (unix epoch) the action completed.
    pub ts: i64,
    /// Authenticated principal (`sub`/username). Defaults to `<unknown>` when absent.
    pub actor: String,
    /// Auth provider that validated the request (`oidc` / `local` / empty).
    pub provider: String,
    /// HTTP method (`POST`, `PUT`, `DELETE`, `PATCH`, or `GET` when log_reads).
    pub method: String,
    /// Request path.
    pub path: String,
    /// For `/api/v1/proxy/{controller_id}/...` routes, the decoded controller id.
    pub target_controller: Option<String>,
    /// HTTP response status code.
    pub status: i32,
    /// TCP peer IP (canonicalized). Never derived from X-Forwarded-For.
    pub source_ip: Option<String>,
    /// `X-Request-Id` header value, if present.
    pub request_id: Option<String>,
    /// Optional free-form detail (reserved; currently always `None`).
    pub detail: Option<String>,
}

impl From<AuditEvent> for AuditRecord {
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

impl From<AuditRecord> for AuditEvent {
    fn from(record: AuditRecord) -> Self {
        Self {
            ts: record.ts,
            actor: record.actor,
            provider: record.provider,
            method: record.method,
            path: record.path,
            target_controller: record.target_controller,
            status: record.status,
            source_ip: record.source_ip,
            request_id: record.request_id,
            detail: record.detail,
        }
    }
}

/// Filter applied by `list_audit`. All fields are AND-combined; `None` fields are
/// not constrained.
#[derive(Default)]
pub struct AuditFilter {
    /// Exact-match actor.
    pub actor: Option<String>,
    /// Exact-match target controller.
    pub controller: Option<String>,
    /// Inclusive lower bound on `ts`.
    pub since: Option<i64>,
    /// Inclusive upper bound on `ts`.
    pub until: Option<i64>,
}

/// SQL audit capability. Writes are fail-open and non-blocking through a
/// bounded channel; reads query the same persistent log.
#[derive(Clone)]
pub struct SqlAuditLog {
    store: std::sync::Arc<Store>,
    tx: tokio::sync::mpsc::Sender<AuditEvent>,
}

impl SqlAuditLog {
    pub fn spawn(store: std::sync::Arc<Store>, capacity: usize) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AuditEvent>(capacity.max(1));
        let writer_store = store.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let Err(error) = writer_store.insert_audit(&event.into()).await {
                    tracing::warn!(component = "audit", %error, "failed to persist audit event");
                }
            }
        });
        Self { store, tx }
    }
}

impl AuditWriter for SqlAuditLog {
    fn record(&self, event: AuditEvent) {
        if let Err(error) = self.tx.try_send(event) {
            let dropped = AUDIT_DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
            metrics::counter!(AUDIT_DROPPED_METRIC).increment(1);
            if dropped == 1 || dropped.is_multiple_of(256) {
                tracing::warn!(component = "audit", %error, dropped, "audit event dropped");
            } else {
                tracing::debug!(component = "audit", %error, dropped, "audit event dropped");
            }
        }
    }
}

#[async_trait::async_trait]
impl AuditReader for SqlAuditLog {
    async fn query(&self, filter: CoreAuditFilter, page: Page) -> CoreResult<AuditPage> {
        let rows = self
            .store
            .list_audit(
                &AuditFilter {
                    actor: filter.actor,
                    controller: filter.controller,
                    since: filter.since,
                    until: filter.until,
                },
                i64::from(page.limit),
                i64::try_from(page.offset).unwrap_or(i64::MAX),
            )
            .await
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        let item_count = rows.len() as u64;
        Ok(AuditPage {
            items: rows.into_iter().map(AuditEvent::from).collect(),
            next_offset: (item_count == u64::from(page.limit))
                .then_some(page.offset.saturating_add(item_count)),
        })
    }
}

/// Map a result row to an `AuditRecord`. Generic over the backend `Row` so the
/// 10-field mapping lives in one place (same pattern as `controllers::map_row`).
fn map_row<R: Row>(row: &R) -> anyhow::Result<AuditRecord>
where
    String: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
    Option<String>: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
    i64: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
    i32: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
    for<'r> &'r str: sqlx::ColumnIndex<R>,
{
    Ok(AuditRecord {
        ts: row.try_get("ts")?,
        actor: row.try_get("actor")?,
        provider: row.try_get("provider")?,
        method: row.try_get("method")?,
        path: row.try_get("path")?,
        target_controller: row.try_get("target_controller")?,
        status: row.try_get("status")?,
        source_ip: row.try_get("source_ip")?,
        request_id: row.try_get("request_id")?,
        detail: row.try_get("detail")?,
    })
}

impl Store {
    /// Append a single audit record. Called from the background writer task.
    pub async fn insert_audit(&self, rec: &AuditRecord) -> anyhow::Result<()> {
        let sql = "INSERT INTO audit_log \
                   (ts, actor, provider, method, path, target_controller, status, source_ip, request_id, detail) \
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(rec.ts)
                    .bind(&rec.actor)
                    .bind(&rec.provider)
                    .bind(&rec.method)
                    .bind(&rec.path)
                    .bind(&rec.target_controller)
                    .bind(rec.status)
                    .bind(&rec.source_ip)
                    .bind(&rec.request_id)
                    .bind(&rec.detail)
                    .execute(pool)
                    .await?;
            }
            Pool::Mysql(pool) => {
                sqlx::query(sql)
                    .bind(rec.ts)
                    .bind(&rec.actor)
                    .bind(&rec.provider)
                    .bind(&rec.method)
                    .bind(&rec.path)
                    .bind(&rec.target_controller)
                    .bind(rec.status)
                    .bind(&rec.source_ip)
                    .bind(&rec.request_id)
                    .bind(&rec.detail)
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// List audit records, newest first (`ORDER BY ts DESC`), honoring `filter`
    /// and the `limit`/`offset` window.
    pub async fn list_audit(
        &self,
        filter: &AuditFilter,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<AuditRecord>> {
        // Build the WHERE clause dynamically; the bind order below mirrors the
        // order conditions are pushed here exactly.
        let mut sql = String::from(
            "SELECT ts, actor, provider, method, path, target_controller, status, source_ip, request_id, detail \
             FROM audit_log",
        );
        let mut conds: Vec<&str> = Vec::new();
        if filter.actor.is_some() {
            conds.push("actor = ?");
        }
        if filter.controller.is_some() {
            conds.push("target_controller = ?");
        }
        if filter.since.is_some() {
            conds.push("ts >= ?");
        }
        if filter.until.is_some() {
            conds.push("ts <= ?");
        }
        if !conds.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conds.join(" AND "));
        }
        sql.push_str(" ORDER BY ts DESC LIMIT ? OFFSET ?");

        match &self.pool {
            Pool::Sqlite(pool) => {
                let mut q = sqlx::query(&sql);
                if let Some(actor) = &filter.actor {
                    q = q.bind(actor);
                }
                if let Some(controller) = &filter.controller {
                    q = q.bind(controller);
                }
                if let Some(since) = filter.since {
                    q = q.bind(since);
                }
                if let Some(until) = filter.until {
                    q = q.bind(until);
                }
                q = q.bind(limit).bind(offset);
                let rows = q.fetch_all(pool).await?;
                rows.iter().map(map_row).collect()
            }
            Pool::Mysql(pool) => {
                let mut q = sqlx::query(&sql);
                if let Some(actor) = &filter.actor {
                    q = q.bind(actor);
                }
                if let Some(controller) = &filter.controller {
                    q = q.bind(controller);
                }
                if let Some(since) = filter.since {
                    q = q.bind(since);
                }
                if let Some(until) = filter.until {
                    q = q.bind(until);
                }
                q = q.bind(limit).bind(offset);
                let rows = q.fetch_all(pool).await?;
                rows.iter().map(map_row).collect()
            }
        }
    }

    /// Delete all records with `ts < before_ts`. Returns the number of rows removed.
    #[allow(dead_code)]
    pub async fn prune_audit(&self, before_ts: i64) -> anyhow::Result<u64> {
        let sql = "DELETE FROM audit_log WHERE ts < ?";
        let affected = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(sql)
                .bind(before_ts)
                .execute(pool)
                .await?
                .rows_affected(),
            Pool::Mysql(pool) => sqlx::query(sql)
                .bind(before_ts)
                .execute(pool)
                .await?
                .rows_affected(),
        };
        Ok(affected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn sql_audit_writer_drops_without_blocking_when_channel_is_closed() {
        let store = std::sync::Arc::new(Store::open_in_memory().await.unwrap());
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(rx);
        let log = SqlAuditLog { store, tx };
        let before = audit_dropped_total();
        log.record(rec(1, "alice", None, 200).into());
        assert_eq!(audit_dropped_total() - before, 1);
    }

    #[tokio::test]
    async fn insert_then_list_filters() {
        let db = Store::open_in_memory().await.unwrap();
        // Three rows, distinct actors and timestamps.
        db.insert_audit(&rec(100, "alice", Some("c1"), 200))
            .await
            .unwrap();
        db.insert_audit(&rec(200, "bob", None, 204)).await.unwrap();
        db.insert_audit(&rec(300, "alice", Some("c2"), 500))
            .await
            .unwrap();

        // No filter: all three, newest first.
        let all = db.list_audit(&AuditFilter::default(), 50, 0).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].ts, 300, "results must be ts DESC");
        assert_eq!(all[1].ts, 200);
        assert_eq!(all[2].ts, 100);

        // Actor filter: only alice's two rows, still ts DESC.
        let f = AuditFilter {
            actor: Some("alice".to_string()),
            ..AuditFilter::default()
        };
        let alice = db.list_audit(&f, 50, 0).await.unwrap();
        assert_eq!(alice.len(), 2);
        assert!(alice.iter().all(|r| r.actor == "alice"));
        assert_eq!(alice[0].ts, 300);
        assert_eq!(alice[1].ts, 100);
        assert_eq!(alice[0].status, 500);
        assert_eq!(alice[0].target_controller.as_deref(), Some("c2"));
    }

    #[tokio::test]
    async fn list_audit_honors_time_window_and_limit() {
        let db = Store::open_in_memory().await.unwrap();
        for ts in [10, 20, 30, 40] {
            db.insert_audit(&rec(ts, "alice", None, 200)).await.unwrap();
        }
        let f = AuditFilter {
            since: Some(20),
            until: Some(30),
            ..AuditFilter::default()
        };
        let rows = db.list_audit(&f, 50, 0).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ts, 30);
        assert_eq!(rows[1].ts, 20);

        // Limit + offset paginate over the full DESC set.
        let page = db.list_audit(&AuditFilter::default(), 2, 1).await.unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].ts, 30);
        assert_eq!(page[1].ts, 20);
    }

    #[tokio::test]
    async fn prune_audit_deletes_older_rows() {
        let db = Store::open_in_memory().await.unwrap();
        for ts in [100, 200, 300] {
            db.insert_audit(&rec(ts, "alice", None, 200)).await.unwrap();
        }
        let deleted = db.prune_audit(250).await.unwrap();
        assert_eq!(deleted, 2, "rows with ts < 250 must be deleted");
        let remaining = db.list_audit(&AuditFilter::default(), 50, 0).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].ts, 300);
    }

    /// MySQL round-trip — gated on `EDGION_TEST_MYSQL_URL`. Skips when unset.
    #[tokio::test]
    async fn mysql_audit_roundtrip() {
        use crate::{DatabaseConfig, DbBackend};

        let Ok(url) = std::env::var("EDGION_TEST_MYSQL_URL") else {
            eprintln!("skipping: EDGION_TEST_MYSQL_URL unset");
            return;
        };
        let _external_database_guard = crate::MYSQL_TEST_LOCK.lock().await;
        let cfg = DatabaseConfig {
            enabled: true,
            backend: DbBackend::Mysql,
            sqlite_path: String::new(),
            mysql_url: Some(url),
        };
        let db = Store::connect(&cfg).await.unwrap();

        // Use a unique actor so the test is isolated from prior runs.
        let actor = format!("mysql-audit-{}", std::process::id());
        db.insert_audit(&rec(1_000, &actor, Some("c1"), 200))
            .await
            .unwrap();
        db.insert_audit(&rec(2_000, &actor, Some("c2"), 500))
            .await
            .unwrap();

        let f = AuditFilter {
            actor: Some(actor.clone()),
            ..AuditFilter::default()
        };
        let rows = db.list_audit(&f, 50, 0).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ts, 2_000, "ts DESC");
        assert_eq!(rows[0].status, 500);
        assert_eq!(rows[0].target_controller.as_deref(), Some("c2"));

        let deleted = db.prune_audit(1_500).await.unwrap();
        assert!(deleted >= 1);
    }
}
