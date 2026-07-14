//! `controllers` table access — async equivalent of the legacy rusqlite `CenterDb`.
//!
//! Semantics are preserved exactly: `env`/`tag` are stored as JSON-string arrays
//! in a TEXT column, `upsert_controller` writes `created_at` only on insert, and
//! `mark_offline` is a narrow no-op-on-missing update.

use super::{Pool, Store};
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
    pub last_seen_at: i64,
    /// Wall-clock seconds of the first insert. Written once on insert and never
    /// updated on conflict (see `upsert_controller`).
    #[allow(dead_code)]
    pub created_at: i64,
}

impl Store {
    /// Upsert a controller record (online or offline state update).
    ///
    /// `created_at` is set to `now` only on first insert; on conflict it is left
    /// untouched while `cluster`/`env`/`tag`/`online`/`last_seen_at` are refreshed.
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

    pub async fn mark_offline(&self, id: &str) -> anyhow::Result<()> {
        let sql = "UPDATE controllers SET online = 0, last_seen_at = ? WHERE controller_id = ?";
        self.exec_by_id(sql, Some(unix_now()), id).await
    }

    /// Delete a controller record from DB.
    pub async fn delete_controller(&self, id: &str) -> anyhow::Result<()> {
        self.evict_controller(id).await.map(|_| ())
    }

    /// Delete a controller and report whether a row existed.
    pub async fn evict_controller(&self, id: &str) -> anyhow::Result<bool> {
        let affected = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query("DELETE FROM controllers WHERE controller_id = ?")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
            Pool::Mysql(pool) => sqlx::query("DELETE FROM controllers WHERE controller_id = ?")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
        };
        Ok(affected > 0)
    }

    /// List all controller records, ordered by `controller_id`.
    pub async fn list_controllers(&self) -> anyhow::Result<Vec<DbController>> {
        let sql = "SELECT controller_id, cluster, env, tag, online, last_seen_at, created_at \
                   FROM controllers ORDER BY controller_id";
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
        db.upsert_controller("c1", "cluster-b", &["stage".into()], &["core".into()], false)
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
        assert_eq!(rows[0].cluster, "cluster-k", "mark_offline must not touch cluster");
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
    async fn delete_controller_removes_row() {
        let db = Store::open_in_memory().await.unwrap();
        db.upsert_controller("c1", "k", &[], &[], true).await.unwrap();
        db.delete_controller("c1").await.unwrap();
        assert!(db.list_controllers().await.unwrap().is_empty());
    }

    /// MySQL round-trip — gated on `EDGION_TEST_MYSQL_URL`. Skips when unset so
    /// the default `cargo test` run needs no live MySQL.
    #[tokio::test]
    async fn mysql_controllers_roundtrip() {
        use crate::config::{DatabaseConfig, DbBackend};

        let Ok(url) = std::env::var("EDGION_TEST_MYSQL_URL") else {
            eprintln!("skipping: EDGION_TEST_MYSQL_URL unset");
            return;
        };
        let cfg = DatabaseConfig {
            enabled: true,
            backend: DbBackend::Mysql,
            sqlite_path: String::new(),
            mysql_url: Some(url),
        };
        let db = Store::connect(&cfg).await.unwrap();

        // Clean slate for a deterministic assertion.
        db.delete_controller("mysql-ctrl-1").await.unwrap();

        db.upsert_controller(
            "mysql-ctrl-1",
            "cluster-m",
            &["prod".to_string()],
            &["edge".to_string()],
            true,
        )
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
        assert!(row.last_seen_at > 0);

        db.mark_offline("mysql-ctrl-1").await.unwrap();
        let rows = db.list_controllers().await.unwrap();
        let row = rows
            .iter()
            .find(|r| r.controller_id == "mysql-ctrl-1")
            .expect("controller must still exist after mark_offline");
        assert!(!row.online, "online flag must flip to false");
        assert_eq!(row.cluster, "cluster-m", "mark_offline must not touch cluster");

        db.delete_controller("mysql-ctrl-1").await.unwrap();
        let rows = db.list_controllers().await.unwrap();
        assert!(rows.iter().all(|r| r.controller_id != "mysql-ctrl-1"));
    }
}
