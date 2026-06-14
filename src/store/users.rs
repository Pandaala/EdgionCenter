//! `users` / `roles` / bindings table access — full-tier identity store.
//!
//! Mirrors the runtime-sqlx style of `controllers.rs` / `audit.rs`: every query is
//! built with `sqlx::query` + `.bind` (never the compile-time macros), and the two
//! backends are dispatched by matching on `&self.pool`. Replace-all mutators
//! (`set_role_permissions`, `set_user_roles`) run their DELETE + bulk-INSERT inside
//! a transaction so a binding set is swapped atomically.
//!
//! These methods are consumed by later DAC tasks (HTTP / authn), so some are not
//! yet referenced by non-test code — `#[allow(dead_code)]` keeps the build quiet.

use super::{Pool, Store};
use sqlx::Row;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// A full-tier user row.
#[allow(dead_code)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub display_name: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A role row.
#[allow(dead_code)]
pub struct Role {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Map a result row to a `User`. Generic over the backend `Row` so the mapping
/// lives in one place (same pattern as `controllers::map_row`).
fn map_user<R: Row>(row: &R) -> anyhow::Result<User>
where
    String: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
    i64: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
    for<'r> &'r str: sqlx::ColumnIndex<R>,
{
    Ok(User {
        id: row.try_get("id")?,
        username: row.try_get("username")?,
        password_hash: row.try_get("password_hash")?,
        display_name: row.try_get("display_name")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// Map a result row to a `Role`.
fn map_role<R: Row>(row: &R) -> anyhow::Result<Role>
where
    String: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
    i64: sqlx::Type<R::Database> + for<'r> sqlx::Decode<'r, R::Database>,
    for<'r> &'r str: sqlx::ColumnIndex<R>,
{
    Ok(Role {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[allow(dead_code)]
impl Store {
    // ---- users ---------------------------------------------------------------

    /// Insert a new user with `created_at = updated_at = now`. Returns the new id.
    /// A duplicate `username` violates the UNIQUE constraint and returns `Err`.
    pub async fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        display_name: &str,
    ) -> anyhow::Result<i64> {
        let now = unix_now();
        let sql = "INSERT INTO users(username, password_hash, display_name, status, created_at, updated_at) \
                   VALUES (?, ?, ?, 'active', ?, ?)";
        let id = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(sql)
                .bind(username)
                .bind(password_hash)
                .bind(display_name)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?
                .last_insert_rowid(),
            Pool::Mysql(pool) => sqlx::query(sql)
                .bind(username)
                .bind(password_hash)
                .bind(display_name)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?
                .last_insert_id() as i64,
        };
        Ok(id)
    }

    /// Fetch a user by its unique `username`.
    pub async fn get_user_by_username(&self, username: &str) -> anyhow::Result<Option<User>> {
        let sql = "SELECT id, username, password_hash, display_name, status, created_at, updated_at \
                   FROM users WHERE username = ?";
        match &self.pool {
            Pool::Sqlite(pool) => match sqlx::query(sql).bind(username).fetch_optional(pool).await? {
                Some(row) => Ok(Some(map_user(&row)?)),
                None => Ok(None),
            },
            Pool::Mysql(pool) => match sqlx::query(sql).bind(username).fetch_optional(pool).await? {
                Some(row) => Ok(Some(map_user(&row)?)),
                None => Ok(None),
            },
        }
    }

    /// Fetch a user by primary key.
    pub async fn get_user(&self, id: i64) -> anyhow::Result<Option<User>> {
        let sql = "SELECT id, username, password_hash, display_name, status, created_at, updated_at \
                   FROM users WHERE id = ?";
        match &self.pool {
            Pool::Sqlite(pool) => match sqlx::query(sql).bind(id).fetch_optional(pool).await? {
                Some(row) => Ok(Some(map_user(&row)?)),
                None => Ok(None),
            },
            Pool::Mysql(pool) => match sqlx::query(sql).bind(id).fetch_optional(pool).await? {
                Some(row) => Ok(Some(map_user(&row)?)),
                None => Ok(None),
            },
        }
    }

    /// List all users, ordered by `username`.
    pub async fn list_users(&self) -> anyhow::Result<Vec<User>> {
        let sql = "SELECT id, username, password_hash, display_name, status, created_at, updated_at \
                   FROM users ORDER BY username";
        match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(sql).fetch_all(pool).await?.iter().map(map_user).collect(),
            Pool::Mysql(pool) => sqlx::query(sql).fetch_all(pool).await?.iter().map(map_user).collect(),
        }
    }

    /// Update a user's `status` and bump `updated_at`.
    pub async fn set_user_status(&self, id: i64, status: &str) -> anyhow::Result<()> {
        let sql = "UPDATE users SET status = ?, updated_at = ? WHERE id = ?";
        let now = unix_now();
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(sql).bind(status).bind(now).bind(id).execute(pool).await?;
            }
            Pool::Mysql(pool) => {
                sqlx::query(sql).bind(status).bind(now).bind(id).execute(pool).await?;
            }
        }
        Ok(())
    }

    /// Update a user's `password_hash` and bump `updated_at`.
    pub async fn set_user_password(&self, id: i64, password_hash: &str) -> anyhow::Result<()> {
        let sql = "UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?";
        let now = unix_now();
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(sql).bind(password_hash).bind(now).bind(id).execute(pool).await?;
            }
            Pool::Mysql(pool) => {
                sqlx::query(sql).bind(password_hash).bind(now).bind(id).execute(pool).await?;
            }
        }
        Ok(())
    }

    /// Delete a user. FK `ON DELETE CASCADE` removes its `user_roles` / `api_tokens`.
    pub async fn delete_user(&self, id: i64) -> anyhow::Result<()> {
        let sql = "DELETE FROM users WHERE id = ?";
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(sql).bind(id).execute(pool).await?;
            }
            Pool::Mysql(pool) => {
                sqlx::query(sql).bind(id).execute(pool).await?;
            }
        }
        Ok(())
    }

    // ---- roles ---------------------------------------------------------------

    /// Insert a new role with `created_at = updated_at = now`. Returns the new id.
    pub async fn create_role(&self, name: &str, description: &str) -> anyhow::Result<i64> {
        let now = unix_now();
        let sql = "INSERT INTO roles(name, description, created_at, updated_at) VALUES (?, ?, ?, ?)";
        let id = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(sql)
                .bind(name)
                .bind(description)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?
                .last_insert_rowid(),
            Pool::Mysql(pool) => sqlx::query(sql)
                .bind(name)
                .bind(description)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?
                .last_insert_id() as i64,
        };
        Ok(id)
    }

    /// List all roles, ordered by `name`.
    pub async fn list_roles(&self) -> anyhow::Result<Vec<Role>> {
        let sql = "SELECT id, name, description, created_at, updated_at FROM roles ORDER BY name";
        match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(sql).fetch_all(pool).await?.iter().map(map_role).collect(),
            Pool::Mysql(pool) => sqlx::query(sql).fetch_all(pool).await?.iter().map(map_role).collect(),
        }
    }

    /// Delete a role. FK `ON DELETE CASCADE` removes its `user_roles` /
    /// `role_permissions` bindings.
    pub async fn delete_role(&self, id: i64) -> anyhow::Result<()> {
        let sql = "DELETE FROM roles WHERE id = ?";
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(sql).bind(id).execute(pool).await?;
            }
            Pool::Mysql(pool) => {
                sqlx::query(sql).bind(id).execute(pool).await?;
            }
        }
        Ok(())
    }

    // ---- role permissions ----------------------------------------------------

    /// Replace the full permission-key set of a role atomically (delete-then-insert
    /// in a transaction). An empty slice clears the role's permissions.
    pub async fn set_role_permissions(&self, role_id: i64, keys: &[String]) -> anyhow::Result<()> {
        let del = "DELETE FROM role_permissions WHERE role_id = ?";
        let ins = "INSERT INTO role_permissions(role_id, permission_key) VALUES (?, ?)";
        match &self.pool {
            Pool::Sqlite(pool) => {
                let mut tx = pool.begin().await?;
                sqlx::query(del).bind(role_id).execute(&mut *tx).await?;
                for key in keys {
                    sqlx::query(ins).bind(role_id).bind(key).execute(&mut *tx).await?;
                }
                tx.commit().await?;
            }
            Pool::Mysql(pool) => {
                let mut tx = pool.begin().await?;
                sqlx::query(del).bind(role_id).execute(&mut *tx).await?;
                for key in keys {
                    sqlx::query(ins).bind(role_id).bind(key).execute(&mut *tx).await?;
                }
                tx.commit().await?;
            }
        }
        Ok(())
    }

    /// List a role's permission keys, ordered by `permission_key`.
    pub async fn role_permissions(&self, role_id: i64) -> anyhow::Result<Vec<String>> {
        let sql = "SELECT permission_key FROM role_permissions WHERE role_id = ? ORDER BY permission_key";
        let mut out = Vec::new();
        match &self.pool {
            Pool::Sqlite(pool) => {
                for row in sqlx::query(sql).bind(role_id).fetch_all(pool).await? {
                    out.push(row.try_get::<String, _>("permission_key")?);
                }
            }
            Pool::Mysql(pool) => {
                for row in sqlx::query(sql).bind(role_id).fetch_all(pool).await? {
                    out.push(row.try_get::<String, _>("permission_key")?);
                }
            }
        }
        Ok(out)
    }

    // ---- user roles ----------------------------------------------------------

    /// Replace the full role set of a user atomically (delete-then-insert in a
    /// transaction). An empty slice clears the user's roles.
    pub async fn set_user_roles(&self, user_id: i64, role_ids: &[i64]) -> anyhow::Result<()> {
        let del = "DELETE FROM user_roles WHERE user_id = ?";
        let ins = "INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)";
        match &self.pool {
            Pool::Sqlite(pool) => {
                let mut tx = pool.begin().await?;
                sqlx::query(del).bind(user_id).execute(&mut *tx).await?;
                for role_id in role_ids {
                    sqlx::query(ins).bind(user_id).bind(role_id).execute(&mut *tx).await?;
                }
                tx.commit().await?;
            }
            Pool::Mysql(pool) => {
                let mut tx = pool.begin().await?;
                sqlx::query(del).bind(user_id).execute(&mut *tx).await?;
                for role_id in role_ids {
                    sqlx::query(ins).bind(user_id).bind(role_id).execute(&mut *tx).await?;
                }
                tx.commit().await?;
            }
        }
        Ok(())
    }

    /// List the role ids bound to a user, ordered by `role_id`.
    pub async fn user_role_ids(&self, user_id: i64) -> anyhow::Result<Vec<i64>> {
        let sql = "SELECT role_id FROM user_roles WHERE user_id = ? ORDER BY role_id";
        let mut out = Vec::new();
        match &self.pool {
            Pool::Sqlite(pool) => {
                for row in sqlx::query(sql).bind(user_id).fetch_all(pool).await? {
                    out.push(row.try_get::<i64, _>("role_id")?);
                }
            }
            Pool::Mysql(pool) => {
                for row in sqlx::query(sql).bind(user_id).fetch_all(pool).await? {
                    out.push(row.try_get::<i64, _>("role_id")?);
                }
            }
        }
        Ok(out)
    }

    /// Resolve a user's effective (DISTINCT) permission keys by joining
    /// `users → user_roles → role_permissions`, ordered by `permission_key`.
    pub async fn permission_keys_for_user(&self, username: &str) -> anyhow::Result<Vec<String>> {
        let sql = "SELECT DISTINCT rp.permission_key \
                   FROM users u \
                   JOIN user_roles ur ON ur.user_id = u.id \
                   JOIN role_permissions rp ON rp.role_id = ur.role_id \
                   WHERE u.username = ? \
                   ORDER BY rp.permission_key";
        let mut out = Vec::new();
        match &self.pool {
            Pool::Sqlite(pool) => {
                for row in sqlx::query(sql).bind(username).fetch_all(pool).await? {
                    out.push(row.try_get::<String, _>("permission_key")?);
                }
            }
            Pool::Mysql(pool) => {
                for row in sqlx::query(sql).bind(username).fetch_all(pool).await? {
                    out.push(row.try_get::<String, _>("permission_key")?);
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
    async fn user_role_permission_join() {
        let db = Store::open_in_memory().await.unwrap();
        let alice = db.create_user("alice", "hash", "Alice").await.unwrap();
        let admin = db.create_role("admin", "Administrators").await.unwrap();
        db.set_role_permissions(admin, &["controllers:read".into(), "controllers:write".into()])
            .await
            .unwrap();
        db.set_user_roles(alice, &[admin]).await.unwrap();

        let keys = db.permission_keys_for_user("alice").await.unwrap();
        assert_eq!(keys, vec!["controllers:read".to_string(), "controllers:write".to_string()]);

        // A second role overlapping on `controllers:read` must not produce dups.
        let viewer = db.create_role("viewer", "Read-only").await.unwrap();
        db.set_role_permissions(viewer, &["controllers:read".into()]).await.unwrap();
        db.set_user_roles(alice, &[admin, viewer]).await.unwrap();
        let keys = db.permission_keys_for_user("alice").await.unwrap();
        assert_eq!(
            keys,
            vec!["controllers:read".to_string(), "controllers:write".to_string()],
            "DISTINCT must collapse the overlapping key"
        );
    }

    #[tokio::test]
    async fn unique_username_rejected() {
        let db = Store::open_in_memory().await.unwrap();
        db.create_user("alice", "h1", "").await.unwrap();
        let err = db.create_user("alice", "h2", "").await;
        assert!(err.is_err(), "duplicate username must violate UNIQUE");
    }

    #[tokio::test]
    async fn set_role_permissions_replaces() {
        let db = Store::open_in_memory().await.unwrap();
        let r = db.create_role("r", "").await.unwrap();
        db.set_role_permissions(r, &["a".into(), "b".into()]).await.unwrap();
        assert_eq!(db.role_permissions(r).await.unwrap(), vec!["a".to_string(), "b".to_string()]);
        db.set_role_permissions(r, &["c".into()]).await.unwrap();
        assert_eq!(db.role_permissions(r).await.unwrap(), vec!["c".to_string()]);
        // Empty slice clears.
        db.set_role_permissions(r, &[]).await.unwrap();
        assert!(db.role_permissions(r).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_role_cascades_bindings() {
        let db = Store::open_in_memory().await.unwrap();
        let alice = db.create_user("alice", "h", "").await.unwrap();
        let admin = db.create_role("admin", "").await.unwrap();
        db.set_role_permissions(admin, &["controllers:read".into()]).await.unwrap();
        db.set_user_roles(alice, &[admin]).await.unwrap();

        db.delete_role(admin).await.unwrap();

        assert!(db.role_permissions(admin).await.unwrap().is_empty(), "role_permissions must cascade");
        assert!(db.user_role_ids(alice).await.unwrap().is_empty(), "user_roles must cascade");
        assert!(
            db.permission_keys_for_user("alice").await.unwrap().is_empty(),
            "no effective permissions after the role is gone"
        );
    }

    #[tokio::test]
    async fn set_user_status_and_password_bump_updated_at() {
        let db = Store::open_in_memory().await.unwrap();
        let id = db.create_user("alice", "h0", "").await.unwrap();
        let before = db.get_user(id).await.unwrap().unwrap();
        assert_eq!(before.status, "active");
        assert_eq!(before.created_at, before.updated_at, "created_at == updated_at on insert");

        db.set_user_status(id, "disabled").await.unwrap();
        let after = db.get_user(id).await.unwrap().unwrap();
        assert_eq!(after.status, "disabled");
        assert!(after.updated_at >= before.updated_at, "updated_at must persist/advance");

        db.set_user_password(id, "h1").await.unwrap();
        let after2 = db.get_user(id).await.unwrap().unwrap();
        assert_eq!(after2.password_hash, "h1");
        assert!(after2.updated_at >= after.updated_at);
    }

    #[tokio::test]
    async fn list_and_lookup_users() {
        let db = Store::open_in_memory().await.unwrap();
        db.create_user("bob", "h", "Bob").await.unwrap();
        db.create_user("alice", "h", "Alice").await.unwrap();
        let users = db.list_users().await.unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].username, "alice", "ORDER BY username");
        assert_eq!(users[1].username, "bob");
        let by_name = db.get_user_by_username("alice").await.unwrap().unwrap();
        assert_eq!(by_name.display_name, "Alice");
        assert!(db.get_user_by_username("nobody").await.unwrap().is_none());
    }

    /// MySQL JOIN round-trip — gated on `EDGION_TEST_MYSQL_URL`. Skips when unset.
    #[tokio::test]
    async fn mysql_user_role_permission_join() {
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

        // Unique names so the test is isolated from prior runs / parallel CI.
        let suffix = std::process::id();
        let uname = format!("alice-{suffix}");
        let admin_name = format!("admin-{suffix}");
        let viewer_name = format!("viewer-{suffix}");

        let alice = db.create_user(&uname, "hash", "Alice").await.unwrap();
        let admin = db.create_role(&admin_name, "").await.unwrap();
        let viewer = db.create_role(&viewer_name, "").await.unwrap();
        db.set_role_permissions(admin, &["controllers:read".into(), "controllers:write".into()])
            .await
            .unwrap();
        db.set_role_permissions(viewer, &["controllers:read".into()]).await.unwrap();
        db.set_user_roles(alice, &[admin, viewer]).await.unwrap();

        let keys = db.permission_keys_for_user(&uname).await.unwrap();
        assert_eq!(
            keys,
            vec!["controllers:read".to_string(), "controllers:write".to_string()],
            "JOIN + DISTINCT must resolve effective keys on MySQL"
        );

        // Cascade: deleting the user removes its bindings; deleting roles cleans up.
        db.delete_user(alice).await.unwrap();
        db.delete_role(admin).await.unwrap();
        db.delete_role(viewer).await.unwrap();
        assert!(db.permission_keys_for_user(&uname).await.unwrap().is_empty());
    }
}
