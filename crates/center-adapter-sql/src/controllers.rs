//! `controllers` table access — async equivalent of the legacy rusqlite `CenterDb`.
//!
//! Semantics are preserved exactly: `env`/`tag` are stored as JSON-string arrays
//! in a TEXT column, `upsert_controller` writes `created_at` only on insert, and
//! `mark_offline` is a narrow no-op-on-missing update.

use super::{Pool, Store};
use edgion_center_core::{ControllerRegistration, ControllerRuntimeObservation};
use sqlx::Row;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// A controller registry row.
pub struct DbController {
    pub controller_id: String,
    pub cluster: String,
    pub env: Vec<String>,
    pub tag: Vec<String>,
    pub online: bool,
    pub session_id: Option<String>,
    pub connected_replica: Option<String>,
    pub last_seen_at: i64,
    /// Wall-clock seconds of the first insert. Written once on insert and never
    /// updated on conflict (see `upsert_controller`).
    #[allow(dead_code)]
    pub created_at: i64,
}

impl Store {
    /// Clear ephemeral transport ownership left by a previous standalone
    /// process. A live gRPC stream cannot survive process restart.
    pub async fn reset_controller_sessions(&self) -> anyhow::Result<()> {
        let sql = "UPDATE controllers SET online = 0, session_id = NULL, connected_replica = NULL, observed_at_ms = 0";
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(sql).execute(pool).await?;
            }
            Pool::Mysql(pool) => {
                sqlx::query(sql).execute(pool).await?;
            }
        }
        Ok(())
    }

    pub async fn project_controller_registration(
        &self,
        registration: &ControllerRegistration,
    ) -> anyhow::Result<()> {
        let now = unix_now();
        let env_json =
            serde_json::to_string(&registration.environments).unwrap_or_else(|_| "[]".to_string());
        let tag_json =
            serde_json::to_string(&registration.tags).unwrap_or_else(|_| "[]".to_string());
        let affected = match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO controllers(controller_id, cluster, env, tag, online, session_id, connected_replica, observed_at_ms, last_seen_at, created_at, evicted)
                     VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?, ?, 0)
                     ON CONFLICT(controller_id) DO UPDATE SET
                       cluster = excluded.cluster,
                       env = excluded.env,
                       tag = excluded.tag,
                       online = 1,
                       session_id = excluded.session_id,
                       connected_replica = excluded.connected_replica,
                       evicted = 0,
                       observed_at_ms = excluded.observed_at_ms,
                       last_seen_at = excluded.last_seen_at
                     WHERE controllers.observed_at_ms < excluded.observed_at_ms",
                )
                .bind(registration.controller_id.as_str())
                .bind(&registration.cluster)
                .bind(env_json)
                .bind(tag_json)
                .bind(registration.session_id.as_str())
                .bind(registration.connected_replica.as_deref())
                .bind(registration.observed_at_unix_ms)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?
                .rows_affected()
            }
            Pool::Mysql(pool) => {
                // Lock the canonical row before comparing the fencing revision.
                // MySQL's affected-row and unqualified ON DUPLICATE expressions
                // vary with client flags/version; an explicit transaction keeps
                // the ordering semantics identical to SQLite.
                let mut transaction = pool.begin().await?;
                let stored = sqlx::query_scalar::<_, i64>(
                    "SELECT observed_at_ms FROM controllers WHERE controller_id = ? FOR UPDATE",
                )
                .bind(registration.controller_id.as_str())
                .fetch_optional(&mut *transaction)
                .await?;
                if stored.is_some_and(|revision| {
                    revision >= registration.observed_at_unix_ms
                }) {
                    anyhow::bail!(
                        "stale controller registration revision for {}",
                        registration.controller_id
                    );
                }
                let affected = sqlx::query(
                    "INSERT INTO controllers(controller_id, cluster, env, tag, online, session_id, connected_replica, observed_at_ms, last_seen_at, created_at, evicted)
                     VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?, ?, 0)
                     ON DUPLICATE KEY UPDATE
                       cluster = VALUES(cluster),
                       env = VALUES(env),
                       tag = VALUES(tag),
                       online = 1,
                       session_id = VALUES(session_id),
                       connected_replica = VALUES(connected_replica),
                       evicted = 0,
                       last_seen_at = VALUES(last_seen_at),
                       observed_at_ms = VALUES(observed_at_ms)",
                )
                .bind(registration.controller_id.as_str())
                .bind(&registration.cluster)
                .bind(env_json)
                .bind(tag_json)
                .bind(registration.session_id.as_str())
                .bind(registration.connected_replica.as_deref())
                .bind(registration.observed_at_unix_ms)
                .bind(now)
                .bind(now)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
                transaction.commit().await?;
                affected
            }
        };
        if affected == 0 {
            anyhow::bail!(
                "stale controller registration revision for {}",
                registration.controller_id
            );
        }
        Ok(())
    }

    /// Refresh runtime liveness for the current live session.
    ///
    /// `observed_at_ms` is the registration/eviction fencing revision and must
    /// not be advanced by heartbeats: doing so would cause the matching stream's
    /// later offline tombstone to look stale. Standalone SQL currently persists
    /// only the liveness timestamp; richer runtime diagnostics remain available
    /// from the in-process aggregator.
    pub async fn project_controller_runtime(
        &self,
        observation: &ControllerRuntimeObservation,
    ) -> anyhow::Result<bool> {
        let last_seen_at = observation.observed_at_unix_ms.saturating_div(1_000);
        let sql = "UPDATE controllers
                   SET last_seen_at = ?
                   WHERE controller_id = ? AND session_id = ? AND online = 1 AND evicted = 0";
        let affected = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(sql)
                .bind(last_seen_at)
                .bind(observation.controller_id.as_str())
                .bind(observation.session_id.as_str())
                .execute(pool)
                .await?
                .rows_affected(),
            Pool::Mysql(pool) => sqlx::query(sql)
                .bind(last_seen_at)
                .bind(observation.controller_id.as_str())
                .bind(observation.session_id.as_str())
                .execute(pool)
                .await?
                .rows_affected(),
        };
        Ok(affected > 0)
    }

    /// Upsert a controller record (online or offline state update).
    ///
    /// `created_at` is set to `now` only on first insert; on conflict it is left
    /// untouched while `cluster`/`env`/`tag`/`online`/`last_seen_at` are refreshed.
    #[allow(dead_code)] // Compatibility API retained until the SQL adapter extraction completes.
    pub async fn upsert_controller(
        &self,
        id: &str,
        cluster: &str,
        env: &[String],
        tag: &[String],
        online: bool,
    ) -> anyhow::Result<()> {
        let now = unix_now();
        let env_json = serde_json::to_string(env).unwrap_or_else(|_| "[]".to_string());
        let tag_json = serde_json::to_string(tag).unwrap_or_else(|_| "[]".to_string());
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO controllers(controller_id, cluster, env, tag, online, last_seen_at, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(controller_id) DO UPDATE SET
                       cluster = excluded.cluster,
                       env = excluded.env,
                       tag = excluded.tag,
                       online = excluded.online,
                       last_seen_at = excluded.last_seen_at",
                )
                .bind(id)
                .bind(cluster)
                .bind(env_json)
                .bind(tag_json)
                .bind(online as i64)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?;
            }
            Pool::Mysql(pool) => {
                sqlx::query(
                    "INSERT INTO controllers(controller_id, cluster, env, tag, online, last_seen_at, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?)
                     ON DUPLICATE KEY UPDATE
                       cluster = VALUES(cluster),
                       env = VALUES(env),
                       tag = VALUES(tag),
                       online = VALUES(online),
                       last_seen_at = VALUES(last_seen_at)",
                )
                .bind(id)
                .bind(cluster)
                .bind(env_json)
                .bind(tag_json)
                .bind(online as i64)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    /// Mark a controller offline (`online=0`) and refresh `last_seen_at`.
    /// No-op if the controller row does not exist. This is the narrow-update
    /// path used from the fed-sync offline branches, which don't have the
    /// `cluster`/`env`/`tag` metadata handy — use `upsert_controller` when
    /// those fields matter (e.g. registration).
    /// Run a single-row controller statement (identified by `controller_id`)
    /// against whichever backend pool is active. `now` is bound first when
    /// `Some` (for statements that refresh `last_seen_at`), then `id`.
    ///
    /// sqlx's `query` builder is typed by backend, so the typed query must be
    /// constructed inside each arm; keeping that two-arm match in ONE place
    /// means a future same-shape statement reuses this helper instead of
    /// copy-pasting the dispatch.
    #[allow(dead_code)] // Used by the compatibility mark_offline API below.
    async fn exec_by_id(&self, sql: &str, now: Option<i64>, id: &str) -> anyhow::Result<()> {
        match &self.pool {
            Pool::Sqlite(pool) => {
                let mut q = sqlx::query(sql);
                if let Some(now) = now {
                    q = q.bind(now);
                }
                q.bind(id).execute(pool).await?;
            }
            Pool::Mysql(pool) => {
                let mut q = sqlx::query(sql);
                if let Some(now) = now {
                    q = q.bind(now);
                }
                q.bind(id).execute(pool).await?;
            }
        }
        Ok(())
    }

    #[allow(dead_code)] // Compatibility API retained until the SQL adapter extraction completes.
    pub async fn mark_offline(&self, id: &str) -> anyhow::Result<()> {
        let sql = "UPDATE controllers SET online = 0, last_seen_at = ? WHERE controller_id = ?";
        self.exec_by_id(sql, Some(unix_now()), id).await
    }

    /// Persist an offline tombstone for a session revision.
    ///
    /// A registration with the same revision cannot overwrite this tombstone,
    /// so cleanup remains correct even when the registration query commits
    /// after its caller timed out. A strictly newer revision wins.
    pub async fn mark_session_offline(
        &self,
        id: &str,
        session_id: &str,
        observed_at_ms: i64,
    ) -> anyhow::Result<bool> {
        let now = unix_now();
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO controllers(controller_id, cluster, env, tag, online, session_id, connected_replica, observed_at_ms, last_seen_at, created_at, evicted)
                     VALUES (?, '', '[]', '[]', 0, ?, NULL, ?, ?, ?, 0)
                     ON CONFLICT(controller_id) DO UPDATE SET
                       online = 0,
                       session_id = excluded.session_id,
                       connected_replica = NULL,
                       observed_at_ms = MAX(controllers.observed_at_ms, excluded.observed_at_ms),
                       last_seen_at = excluded.last_seen_at
                     WHERE controllers.session_id = excluded.session_id
                        OR controllers.observed_at_ms < excluded.observed_at_ms",
                )
                .bind(id)
                .bind(session_id)
                .bind(observed_at_ms)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?;
            }
            Pool::Mysql(pool) => {
                // Keep observed_at_ms last: MySQL evaluates single-table update
                // assignments from left to right.
                sqlx::query(
                    "INSERT INTO controllers(controller_id, cluster, env, tag, online, session_id, connected_replica, observed_at_ms, last_seen_at, created_at, evicted)
                     VALUES (?, '', '[]', '[]', 0, ?, NULL, ?, ?, ?, 0)
                     ON DUPLICATE KEY UPDATE
                       online = IF(session_id = VALUES(session_id) OR observed_at_ms < VALUES(observed_at_ms), 0, online),
                       connected_replica = IF(session_id = VALUES(session_id) OR observed_at_ms < VALUES(observed_at_ms), NULL, connected_replica),
                       last_seen_at = IF(session_id = VALUES(session_id) OR observed_at_ms < VALUES(observed_at_ms), VALUES(last_seen_at), last_seen_at),
                       session_id = IF(session_id = VALUES(session_id) OR observed_at_ms < VALUES(observed_at_ms), VALUES(session_id), session_id),
                       observed_at_ms = GREATEST(observed_at_ms, VALUES(observed_at_ms))",
                )
                .bind(id)
                .bind(session_id)
                .bind(observed_at_ms)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?;
            }
        }

        let (stored_session, stored_revision, online, evicted) = match &self.pool {
            Pool::Sqlite(pool) => {
                let row = sqlx::query(
                    "SELECT session_id, observed_at_ms, online, evicted FROM controllers WHERE controller_id = ?",
                )
                .bind(id)
                .fetch_one(pool)
                .await?;
                (
                    row.try_get::<Option<String>, _>("session_id")?,
                    row.try_get::<i64, _>("observed_at_ms")?,
                    row.try_get::<i64, _>("online")? != 0,
                    row.try_get::<i64, _>("evicted")? != 0,
                )
            }
            Pool::Mysql(pool) => {
                let row = sqlx::query(
                    "SELECT session_id, observed_at_ms, online, evicted FROM controllers WHERE controller_id = ?",
                )
                .bind(id)
                .fetch_one(pool)
                .await?;
                (
                    row.try_get::<Option<String>, _>("session_id")?,
                    row.try_get::<i64, _>("observed_at_ms")?,
                    row.try_get::<i8, _>("online")? != 0,
                    row.try_get::<i8, _>("evicted")? != 0,
                )
            }
        };
        if evicted || stored_revision > observed_at_ms {
            return Ok(false);
        }
        if stored_revision == observed_at_ms
            && stored_session.as_deref() == Some(session_id)
            && !online
        {
            return Ok(true);
        }
        anyhow::bail!("offline tombstone invariant failed for controller {id}")
    }

    /// Delete a controller record from DB.
    pub async fn delete_controller(&self, id: &str) -> anyhow::Result<()> {
        self.evict_controller(id).await.map(|_| ())
    }

    /// Persist an eviction fence and report whether a visible row existed.
    ///
    /// The row remains as a hidden revision fence so an already-scheduled
    /// offline reconciliation cannot recreate it after the Admin API returns.
    /// A strictly newer registration revision clears the fence.
    pub async fn evict_controller(&self, id: &str) -> anyhow::Result<bool> {
        let affected = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(
                "UPDATE controllers
                 SET evicted = 1, online = 0, session_id = NULL, connected_replica = NULL
                 WHERE controller_id = ? AND evicted = 0",
            )
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected(),
            Pool::Mysql(pool) => sqlx::query(
                "UPDATE controllers
                 SET evicted = 1, online = 0, session_id = NULL, connected_replica = NULL
                 WHERE controller_id = ? AND evicted = 0",
            )
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected(),
        };
        Ok(affected > 0)
    }

    /// List all controller records, ordered by `controller_id`.
    pub async fn list_controllers(&self) -> anyhow::Result<Vec<DbController>> {
        let sql = "SELECT controller_id, cluster, env, tag, online, session_id, connected_replica, last_seen_at, created_at \
                   FROM controllers WHERE evicted = 0 ORDER BY controller_id";
        // The two backends differ only in how the boolean `online` column decodes
        // (SQLite INTEGER -> i64, MySQL TINYINT -> i8); a closure over a generic
        // `Row` keeps the field-mapping logic in one place.
        fn map_row<R: Row>(row: &R, online: bool) -> anyhow::Result<DbController>
        where
            String: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
            i64: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
            for<'r> &'r str: sqlx::ColumnIndex<R>,
        {
            let env_json: String = row.try_get("env")?;
            let tag_json: String = row.try_get("tag")?;
            Ok(DbController {
                controller_id: row.try_get("controller_id")?,
                cluster: row.try_get("cluster")?,
                env: serde_json::from_str(&env_json).unwrap_or_default(),
                tag: serde_json::from_str(&tag_json).unwrap_or_default(),
                online,
                session_id: row.try_get("session_id")?,
                connected_replica: row.try_get("connected_replica")?,
                last_seen_at: row.try_get("last_seen_at")?,
                created_at: row.try_get("created_at")?,
            })
        }
        let mut out = Vec::new();
        match &self.pool {
            Pool::Sqlite(pool) => {
                for row in sqlx::query(sql).fetch_all(pool).await? {
                    // SQLite stores the flag in an INTEGER column.
                    let online = row.try_get::<i64, _>("online")? != 0;
                    out.push(map_row(&row, online)?);
                }
            }
            Pool::Mysql(pool) => {
                for row in sqlx::query(sql).fetch_all(pool).await? {
                    // MySQL TINYINT maps to i8.
                    let online = row.try_get::<i8, _>("online")? != 0;
                    out.push(map_row(&row, online)?);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_center_core::{ControllerId, SessionId};

    fn registration(
        session: &str,
        cluster: &str,
        environment: &str,
        replica: &str,
        observed_at: i64,
    ) -> ControllerRegistration {
        ControllerRegistration {
            controller_id: ControllerId::new("c1").unwrap(),
            session_id: SessionId::new(session).unwrap(),
            cluster: cluster.to_string(),
            environments: vec![environment.to_string()],
            tags: vec!["edge".to_string()],
            connected_replica: Some(replica.to_string()),
            ownership_fence: None,
            observed_at_unix_ms: observed_at,
        }
    }

    fn runtime(session: &str, observed_at: i64) -> ControllerRuntimeObservation {
        ControllerRuntimeObservation {
            controller_id: ControllerId::new("c1").unwrap(),
            session_id: SessionId::new(session).unwrap(),
            ownership_fence: None,
            sync_version: None,
            watch_server_id: None,
            resource_count: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: None,
            observed_at_unix_ms: observed_at,
        }
    }

    #[tokio::test]
    async fn upsert_then_list_roundtrips_fields() {
        let db = Store::open_in_memory().await.unwrap();
        db.upsert_controller(
            "cluster-a/ctrl-1",
            "cluster-a",
            &["prod".to_string()],
            &["edge".to_string()],
            true,
        )
        .await
        .unwrap();
        let rows = db.list_controllers().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].controller_id, "cluster-a/ctrl-1");
        assert_eq!(rows[0].cluster, "cluster-a");
        assert_eq!(rows[0].env, vec!["prod".to_string()]);
        assert_eq!(rows[0].tag, vec!["edge".to_string()]);
        assert!(rows[0].online);
        assert!(rows[0].last_seen_at > 0);
    }

    #[tokio::test]
    async fn upsert_preserves_created_at_on_conflict() {
        let db = Store::open_in_memory().await.unwrap();
        // First insert establishes created_at.
        db.upsert_controller("c1", "cluster-a", &["prod".into()], &["edge".into()], true)
            .await
            .unwrap();
        let first = db.list_controllers().await.unwrap();
        assert_eq!(first.len(), 1);
        let created_at = first[0].created_at;
        let last_seen_first = first[0].last_seen_at;
        assert!(created_at > 0, "created_at must be set on insert");

        // Second upsert of the SAME id with different cluster/online state.
        // created_at must NOT be reset; last_seen_at is refreshed (>= first).
        db.upsert_controller(
            "c1",
            "cluster-b",
            &["stage".into()],
            &["core".into()],
            false,
        )
        .await
        .unwrap();
        let second = db.list_controllers().await.unwrap();
        assert_eq!(second.len(), 1, "upsert must not create a duplicate row");
        assert_eq!(
            second[0].created_at, created_at,
            "created_at must equal the first insert's value (not reset on conflict)"
        );
        assert!(
            second[0].last_seen_at >= last_seen_first,
            "last_seen_at must be refreshed on upsert"
        );
        // Confirm the conflict path actually updated the mutable fields.
        assert_eq!(second[0].cluster, "cluster-b");
        assert!(!second[0].online);
        assert_eq!(second[0].env, vec!["stage".to_string()]);
    }

    #[tokio::test]
    async fn mark_offline_flips_online_flag_without_wiping_metadata() {
        let db = Store::open_in_memory().await.unwrap();
        db.upsert_controller("c1", "cluster-k", &["prod".into()], &["edge".into()], true)
            .await
            .unwrap();
        db.mark_offline("c1").await.unwrap();
        let rows = db.list_controllers().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].online, "online flag must flip to false");
        assert_eq!(
            rows[0].cluster, "cluster-k",
            "mark_offline must not touch cluster"
        );
        assert_eq!(rows[0].env, vec!["prod".to_string()]);
        assert_eq!(rows[0].tag, vec!["edge".to_string()]);
    }

    #[tokio::test]
    async fn mark_offline_on_missing_row_is_noop() {
        let db = Store::open_in_memory().await.unwrap();
        db.mark_offline("nope").await.unwrap();
        assert!(db.list_controllers().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn session_fencing_rejects_stale_projection_and_offline() {
        let db = Store::open_in_memory().await.unwrap();
        db.project_controller_registration(&registration(
            "s2",
            "cluster-new",
            "prod",
            "center-0",
            2,
        ))
        .await
        .unwrap();
        let stale_result = db
            .project_controller_registration(&registration(
                "s1",
                "cluster-stale",
                "dev",
                "center-old",
                1,
            ))
            .await;
        assert!(stale_result.is_err());

        assert!(!db.mark_session_offline("c1", "s1", 1).await.unwrap());
        let row = db.list_controllers().await.unwrap().pop().unwrap();
        assert!(row.online);
        assert_eq!(row.session_id.as_deref(), Some("s2"));
        assert_eq!(row.cluster, "cluster-new");
        assert_eq!(row.connected_replica.as_deref(), Some("center-0"));

        assert!(db.mark_session_offline("c1", "s2", 2).await.unwrap());
        let row = db.list_controllers().await.unwrap().pop().unwrap();
        assert!(!row.online);
        assert_eq!(row.session_id.as_deref(), Some("s2"));
    }

    #[tokio::test]
    async fn runtime_projection_refreshes_only_the_current_live_session() {
        let db = Store::open_in_memory().await.unwrap();
        db.project_controller_registration(&registration(
            "s2",
            "cluster-new",
            "prod",
            "center-0",
            2,
        ))
        .await
        .unwrap();

        assert!(db
            .project_controller_runtime(&runtime("s2", 50_000))
            .await
            .unwrap());
        let row = db.list_controllers().await.unwrap().pop().unwrap();
        assert_eq!(row.last_seen_at, 50);

        assert!(!db
            .project_controller_runtime(&runtime("s1", 60_000))
            .await
            .unwrap());
        let row = db.list_controllers().await.unwrap().pop().unwrap();
        assert_eq!(
            row.last_seen_at, 50,
            "a stale session must not refresh liveness"
        );

        assert!(db.mark_session_offline("c1", "s2", 2).await.unwrap());
        assert!(!db
            .project_controller_runtime(&runtime("s2", 70_000))
            .await
            .unwrap());
        let row = db.list_controllers().await.unwrap().pop().unwrap();
        assert_ne!(
            row.last_seen_at, 70,
            "an offline session must not refresh liveness"
        );
    }

    #[tokio::test]
    async fn offline_tombstone_blocks_same_revision_late_registration() {
        let db = Store::open_in_memory().await.unwrap();
        assert!(db.mark_session_offline("c1", "late", 10).await.unwrap());

        let late = registration("late", "cluster-late", "prod", "center-0", 10);
        assert!(db.project_controller_registration(&late).await.is_err());
        let row = db.list_controllers().await.unwrap().pop().unwrap();
        assert!(!row.online);
        assert_eq!(row.session_id.as_deref(), Some("late"));

        let newer = registration("new", "cluster-new", "prod", "center-1", 11);
        db.project_controller_registration(&newer).await.unwrap();
        let row = db.list_controllers().await.unwrap().pop().unwrap();
        assert!(row.online);
        assert_eq!(row.session_id.as_deref(), Some("new"));
    }

    #[tokio::test]
    async fn eviction_fence_survives_late_offline_and_allows_newer_registration() {
        let db = Store::open_in_memory().await.unwrap();
        let current = registration("s10", "cluster-a", "prod", "center-0", 10);
        db.project_controller_registration(&current).await.unwrap();

        assert!(db.evict_controller("c1").await.unwrap());
        assert!(!db.evict_controller("c1").await.unwrap());
        assert!(!db.mark_session_offline("c1", "s10", 10).await.unwrap());
        assert!(db.list_controllers().await.unwrap().is_empty());

        assert!(db.project_controller_registration(&current).await.is_err());
        assert!(db.list_controllers().await.unwrap().is_empty());

        let newer = registration("s11", "cluster-b", "prod", "center-1", 11);
        db.project_controller_registration(&newer).await.unwrap();
        let row = db.list_controllers().await.unwrap().pop().unwrap();
        assert!(row.online);
        assert_eq!(row.session_id.as_deref(), Some("s11"));
    }

    #[tokio::test]
    async fn startup_reset_clears_ephemeral_session_ownership() {
        let db = Store::open_in_memory().await.unwrap();
        db.project_controller_registration(&registration(
            "s1",
            "cluster-a",
            "prod",
            "center-0",
            i64::MAX - 1,
        ))
        .await
        .unwrap();

        db.reset_controller_sessions().await.unwrap();
        db.project_controller_registration(&registration("s2", "cluster-b", "prod", "center-1", 1))
            .await
            .unwrap();
        let row = db.list_controllers().await.unwrap().pop().unwrap();
        assert!(row.online);
        assert_eq!(row.session_id.as_deref(), Some("s2"));
        assert_eq!(row.connected_replica.as_deref(), Some("center-1"));
    }

    #[tokio::test]
    async fn delete_controller_removes_row() {
        let db = Store::open_in_memory().await.unwrap();
        db.upsert_controller("c1", "k", &[], &[], true)
            .await
            .unwrap();
        db.delete_controller("c1").await.unwrap();
        assert!(db.list_controllers().await.unwrap().is_empty());
    }

    /// MySQL round-trip — gated on `EDGION_TEST_MYSQL_URL`. Skips when unset so
    /// the default `cargo test` run needs no live MySQL.
    #[tokio::test]
    async fn mysql_controllers_roundtrip() {
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

        // Clean slate for a deterministic assertion.
        db.delete_controller("mysql-ctrl-1").await.unwrap();

        let mut projected = registration("mysql-s2", "cluster-m", "prod", "center-m", 2);
        projected.controller_id = ControllerId::new("mysql-ctrl-1").unwrap();
        db.project_controller_registration(&projected)
            .await
            .unwrap();
        let rows = db.list_controllers().await.unwrap();
        let row = rows
            .iter()
            .find(|r| r.controller_id == "mysql-ctrl-1")
            .expect("inserted controller must be listed");
        assert_eq!(row.cluster, "cluster-m");
        assert_eq!(row.env, vec!["prod".to_string()]);
        assert_eq!(row.tag, vec!["edge".to_string()]);
        assert!(row.online);
        assert_eq!(row.session_id.as_deref(), Some("mysql-s2"));
        assert!(row.last_seen_at > 0);

        let mut stale = registration("mysql-s1", "cluster-stale", "dev", "center-old", 1);
        stale.controller_id = ControllerId::new("mysql-ctrl-1").unwrap();
        assert!(db.project_controller_registration(&stale).await.is_err());
        assert!(!db
            .mark_session_offline("mysql-ctrl-1", "mysql-s1", 1)
            .await
            .unwrap());

        assert!(db
            .mark_session_offline("mysql-ctrl-1", "mysql-s2", 2)
            .await
            .unwrap());
        let rows = db.list_controllers().await.unwrap();
        let row = rows
            .iter()
            .find(|r| r.controller_id == "mysql-ctrl-1")
            .expect("controller must still exist after mark_offline");
        assert!(!row.online, "online flag must flip to false");
        assert_eq!(
            row.cluster, "cluster-m",
            "mark_offline must not touch cluster"
        );

        assert!(db.evict_controller("mysql-ctrl-1").await.unwrap());
        assert!(!db
            .mark_session_offline("mysql-ctrl-1", "mysql-s2", 2)
            .await
            .unwrap());
        let rows = db.list_controllers().await.unwrap();
        assert!(rows.iter().all(|r| r.controller_id != "mysql-ctrl-1"));

        let mut newer = registration("mysql-s3", "cluster-new", "prod", "center-new", 3);
        newer.controller_id = ControllerId::new("mysql-ctrl-1").unwrap();
        db.project_controller_registration(&newer).await.unwrap();
        assert!(db
            .list_controllers()
            .await
            .unwrap()
            .iter()
            .any(|row| row.controller_id == "mysql-ctrl-1" && row.online));
        db.evict_controller("mysql-ctrl-1").await.unwrap();
    }
}
