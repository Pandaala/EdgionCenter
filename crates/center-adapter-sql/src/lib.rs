//! SQL persistence adapter for standalone Edgion Center deployments.
//!
//! A single async [`Store`] sits in front of either SQLite (embedded, default)
//! or MySQL (external), selected by [`DbBackend`]. All queries use
//! sqlx *runtime* APIs (`sqlx::query` + `.bind`), never the compile-time-checked
//! macros, so a plain `cargo build` needs no live `DATABASE_URL`.

use edgion_center_core::{
    Action, Authorizer, ControllerDirectory, ControllerId, ControllerPhase, ControllerRecord,
    ControllerRegistration, CoreError, CoreResult, CreateRole, CreateUser, Decision,
    EvictionOutcome, EvictionResult, OfflineOutcome, Principal, RoleAdmin, RoleRecord, SessionId,
    UpdateUser, UserAdmin, UserRecord,
};
use serde::{Deserialize, Serialize};
use sqlx::mysql::MySqlPool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::sync::Arc;
use std::{
    collections::HashMap, collections::HashSet, sync::RwLock, time::Duration, time::Instant,
};

pub mod audit;
mod cloud_capabilities;
pub mod controllers;
mod provider_accounts;
mod users;

#[cfg(test)]
pub(crate) static MYSQL_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub use controllers::DbController;
pub use users::{Role, User};

/// Reports whether a store operation failed because of a UNIQUE constraint.
/// Keeping SQLx error inspection here prevents SQL implementation types from
/// leaking into callers of the adapter.
pub fn is_unique_violation(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<sqlx::Error>()
        .and_then(|error| error.as_database_error())
        .is_some_and(|error| error.is_unique_violation())
}

fn core_adapter_error(error: anyhow::Error) -> CoreError {
    if is_unique_violation(&error) {
        CoreError::Conflict(error.to_string())
    } else {
        CoreError::Adapter(error.to_string())
    }
}

/// Persistence backend selector for the standalone metadata store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DbBackend {
    #[default]
    Sqlite,
    Mysql,
}

/// SQL connection configuration owned by this adapter.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    pub enabled: bool,
    pub backend: DbBackend,
    pub sqlite_path: String,
    pub mysql_url: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            backend: DbBackend::Sqlite,
            sqlite_path: "data/center.db".to_string(),
            mysql_url: None,
        }
    }
}

impl std::fmt::Debug for DatabaseConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DatabaseConfig")
            .field("enabled", &self.enabled)
            .field("backend", &self.backend)
            .field("sqlite_path", &self.sqlite_path)
            .field("mysql_url", &self.mysql_url.as_ref().map(|_| "***"))
            .finish()
    }
}

/// Backend-specific connection pool. Both variants are cheap to `clone`
/// (sqlx pools are reference-counted handles).
#[derive(Clone)]
pub(crate) enum Pool {
    Sqlite(SqlitePool),
    Mysql(MySqlPool),
}

/// Async persistence handle. Cheap to clone (wraps a reference-counted pool).
#[derive(Clone)]
pub struct Store {
    pub(crate) pool: Pool,
}

impl Store {
    /// Build the backend pool from config and run pending migrations.
    pub async fn connect(cfg: &DatabaseConfig) -> anyhow::Result<Store> {
        let store = match cfg.backend {
            DbBackend::Sqlite => {
                // Create the parent directory if needed (matches the legacy behavior).
                if let Some(parent) = std::path::Path::new(&cfg.sqlite_path).parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).ok();
                    }
                }
                // WAL + a busy_timeout let concurrent spawned upsert/mark_offline
                // tasks against a file DB wait out a writer lock instead of failing
                // fast with SQLITE_BUSY.
                let opts = SqliteConnectOptions::new()
                    .filename(&cfg.sqlite_path)
                    .create_if_missing(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    // NORMAL is the standard, safe pairing with WAL. The only
                    // durability cost is that the last committed transaction may be
                    // lost on an OS crash / power loss (not on a process crash).
                    // Accepted tradeoff: this DB holds best-effort controller registry
                    // state (reconstructed from live controllers on restart) plus
                    // rarely-written roles / db_auth users; the small power-loss window
                    // is acceptable for both.
                    .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
                    .busy_timeout(std::time::Duration::from_secs(5))
                    // Enforce ON DELETE CASCADE foreign keys (off by default in SQLite).
                    .foreign_keys(true);
                let pool = SqlitePool::connect_with(opts).await?;
                Store {
                    pool: Pool::Sqlite(pool),
                }
            }
            DbBackend::Mysql => {
                let url = cfg.mysql_url.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("database.mysql_url is required when backend = mysql")
                })?;
                let pool = MySqlPool::connect(url).await?;
                Store {
                    pool: Pool::Mysql(pool),
                }
            }
        };
        store.migrate().await?;
        store.reset_controller_sessions().await?;
        Ok(store)
    }

    /// Run the embedded migrations for the active backend. sqlx tracks applied
    /// migrations in `_sqlx_migrations`, so this is safe to call repeatedly.
    pub async fn migrate(&self) -> anyhow::Result<()> {
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::migrate!("src/migrations/sqlite").run(pool).await?;
            }
            Pool::Mysql(pool) => {
                sqlx::migrate!("src/migrations/mysql").run(pool).await?;
            }
        }
        Ok(())
    }

    /// Open a fresh in-memory SQLite store for tests.
    ///
    /// `max_connections(1)` keeps every query on the same connection so the
    /// in-memory database (which is per-connection in SQLite) is shared across
    /// migrate + queries.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn open_in_memory() -> anyhow::Result<Store> {
        // Enable foreign-key enforcement so ON DELETE CASCADE works in tests.
        let opts = SqliteConnectOptions::new().foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        let store = Store {
            pool: Pool::Sqlite(pool),
        };
        store.migrate().await?;
        Ok(store)
    }
}

/// SQL-backed controller projection with persistent session fencing.
pub struct SqlControllerDirectory {
    store: Arc<Store>,
}

/// Database-backed core authorizer with a short, bounded-staleness cache.
pub struct SqlAuthorizer {
    store: Arc<Store>,
    cache: RwLock<HashMap<String, (Instant, HashSet<String>)>>,
    cache_ttl: Duration,
}

/// Standalone user and role management capability.
pub struct SqlAdmin {
    store: Arc<Store>,
}

impl SqlAdmin {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    async fn hash_password(password: String) -> CoreResult<String> {
        tokio::task::spawn_blocking(move || bcrypt::hash(password, bcrypt::DEFAULT_COST))
            .await
            .map_err(|error| CoreError::Adapter(error.to_string()))?
            .map_err(|error| CoreError::Adapter(error.to_string()))
    }
}

#[async_trait::async_trait]
impl UserAdmin for SqlAdmin {
    async fn list_users(&self) -> CoreResult<Vec<UserRecord>> {
        let roles = self.store.list_roles().await.map_err(core_adapter_error)?;
        let role_names: HashMap<i64, String> =
            roles.into_iter().map(|role| (role.id, role.name)).collect();
        let users = self.store.list_users().await.map_err(core_adapter_error)?;
        let mut records = Vec::with_capacity(users.len());
        for user in users {
            let role_ids = self
                .store
                .user_role_ids(user.id)
                .await
                .map_err(core_adapter_error)?;
            let role_names = role_ids
                .iter()
                .filter_map(|role_id| role_names.get(role_id).cloned())
                .collect();
            records.push(UserRecord {
                id: user.id,
                username: user.username,
                display_name: user.display_name,
                status: user.status,
                created_at: user.created_at,
                role_ids,
                role_names,
            });
        }
        Ok(records)
    }

    async fn create_user(&self, user: CreateUser) -> CoreResult<i64> {
        let password_hash = Self::hash_password(user.password).await?;
        self.store
            .create_user_with_roles(
                &user.username,
                &password_hash,
                &user.display_name,
                &user.role_ids,
            )
            .await
            .map_err(core_adapter_error)
    }

    async fn update_user(&self, id: i64, update: UpdateUser) -> CoreResult<()> {
        let password_hash = match update.password {
            Some(password) => Some(Self::hash_password(password).await?),
            None => None,
        };
        self.store
            .update_user_atomic(
                id,
                update.status.as_deref(),
                password_hash.as_deref(),
                update.role_ids.as_deref(),
            )
            .await
            .map_err(core_adapter_error)
    }

    async fn delete_user(&self, id: i64) -> CoreResult<()> {
        self.store.delete_user(id).await.map_err(core_adapter_error)
    }
}

#[async_trait::async_trait]
impl RoleAdmin for SqlAdmin {
    async fn list_roles(&self) -> CoreResult<Vec<RoleRecord>> {
        let roles = self.store.list_roles().await.map_err(core_adapter_error)?;
        let mut records = Vec::with_capacity(roles.len());
        for role in roles {
            let permission_keys = self
                .store
                .role_permissions(role.id)
                .await
                .map_err(core_adapter_error)?;
            records.push(RoleRecord {
                id: role.id,
                name: role.name,
                description: role.description,
                permission_keys,
            });
        }
        Ok(records)
    }

    async fn create_role(&self, role: CreateRole) -> CoreResult<i64> {
        self.store
            .create_role_with_permissions(&role.name, &role.description, &role.permission_keys)
            .await
            .map_err(core_adapter_error)
    }

    async fn set_permissions(&self, id: i64, permission_keys: Vec<String>) -> CoreResult<()> {
        self.store
            .set_role_permissions(id, &permission_keys)
            .await
            .map_err(core_adapter_error)
    }

    async fn delete_role(&self, id: i64) -> CoreResult<()> {
        self.store.delete_role(id).await.map_err(core_adapter_error)
    }
}

impl SqlAuthorizer {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            cache: RwLock::new(HashMap::new()),
            cache_ttl: Duration::from_secs(30),
        }
    }

    fn cached(&self, subject: &str, now: Instant) -> Option<HashSet<String>> {
        let cache = self.cache.read().ok()?;
        let (recorded_at, permissions) = cache.get(subject)?;
        (now.duration_since(*recorded_at) < self.cache_ttl).then(|| permissions.clone())
    }
}

#[async_trait::async_trait]
impl Authorizer for SqlAuthorizer {
    async fn authorize(&self, principal: &Principal, action: &Action) -> CoreResult<Decision> {
        let now = Instant::now();
        let permissions = if let Some(permissions) = self.cached(&principal.subject, now) {
            permissions
        } else {
            let permissions: HashSet<String> = self
                .store
                .permission_keys_for_user(&principal.subject)
                .await
                .map_err(|error| CoreError::Adapter(error.to_string()))?
                .into_iter()
                .collect();
            if let Ok(mut cache) = self.cache.write() {
                cache.insert(principal.subject.clone(), (now, permissions.clone()));
            }
            permissions
        };
        Ok(if permissions.contains(&action.permission) {
            Decision::allow()
        } else {
            Decision::deny(format!("missing permission {}", action.permission))
        })
    }

    async fn granted_permissions(
        &self,
        principal: &Principal,
        candidates: &[String],
    ) -> CoreResult<Option<Vec<String>>> {
        let now = Instant::now();
        let permissions = if let Some(permissions) = self.cached(&principal.subject, now) {
            permissions
        } else {
            let permissions: HashSet<String> = self
                .store
                .permission_keys_for_user(&principal.subject)
                .await
                .map_err(|error| CoreError::Adapter(error.to_string()))?
                .into_iter()
                .collect();
            if let Ok(mut cache) = self.cache.write() {
                cache.insert(principal.subject.clone(), (now, permissions.clone()));
            }
            permissions
        };
        Ok(Some(
            candidates
                .iter()
                .filter(|candidate| permissions.contains(candidate.as_str()))
                .cloned()
                .collect(),
        ))
    }
}

impl SqlControllerDirectory {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    fn adapter_error(error: impl std::fmt::Display) -> CoreError {
        CoreError::Adapter(error.to_string())
    }
}

#[async_trait::async_trait]
impl ControllerDirectory for SqlControllerDirectory {
    async fn upsert_registration(&self, registration: ControllerRegistration) -> CoreResult<()> {
        self.store
            .project_controller_registration(&registration)
            .await
            .map_err(Self::adapter_error)
    }

    async fn mark_offline(
        &self,
        id: &ControllerId,
        observed_session: &SessionId,
        _ownership_fence: Option<&edgion_center_core::OwnershipFence>,
        observed_at_unix_ms: i64,
    ) -> CoreResult<OfflineOutcome> {
        let marked = self
            .store
            .mark_session_offline(id.as_str(), observed_session.as_str(), observed_at_unix_ms)
            .await
            .map_err(Self::adapter_error)?;
        Ok(if marked {
            OfflineOutcome::Marked
        } else {
            OfflineOutcome::NotCurrent
        })
    }

    async fn list(&self) -> CoreResult<Vec<ControllerRecord>> {
        self.store
            .list_controllers()
            .await
            .map_err(Self::adapter_error)?
            .into_iter()
            .map(|row| {
                let controller_id = ControllerId::new(row.controller_id)?;
                Ok(ControllerRecord {
                    current_session_id: if row.online {
                        row.session_id.map(SessionId::new).transpose()?
                    } else {
                        None
                    },
                    controller_id,
                    cluster: row.cluster,
                    environments: row.env,
                    tags: row.tag,
                    connected_replica: row.connected_replica,
                    ownership_fence: None,
                    sync_version: None,
                    watch_server_id: None,
                    resource_count: None,
                    stats_updated_unix_ms: None,
                    watch_updated_unix_ms: None,
                    phase: if row.online {
                        ControllerPhase::Online
                    } else {
                        ControllerPhase::Offline
                    },
                    last_seen_unix_ms: row.last_seen_at.saturating_mul(1000),
                })
            })
            .collect()
    }

    async fn project_runtime(
        &self,
        observation: edgion_center_core::ControllerRuntimeObservation,
    ) -> CoreResult<bool> {
        self.store
            .project_controller_runtime(&observation)
            .await
            .map_err(Self::adapter_error)
    }

    async fn evict(&self, id: &ControllerId) -> CoreResult<EvictionResult> {
        let removed = self
            .store
            .evict_controller(id.as_str())
            .await
            .map_err(Self::adapter_error)?;
        Ok(EvictionResult {
            outcome: if removed {
                EvictionOutcome::Evicted
            } else {
                EvictionOutcome::AlreadyAbsent
            },
            target: None,
        })
    }
}

#[cfg(test)]
mod directory_tests {
    use super::*;

    fn registration(controller: &str, session: &str, observed_at: i64) -> ControllerRegistration {
        ControllerRegistration {
            controller_id: ControllerId::new(controller).unwrap(),
            session_id: SessionId::new(session).unwrap(),
            cluster: "cluster-a".to_string(),
            environments: vec!["prod".to_string()],
            tags: vec!["edge".to_string()],
            connected_replica: Some("center-0".to_string()),
            ownership_fence: None,
            observed_at_unix_ms: observed_at,
        }
    }

    #[test]
    fn database_config_rejects_unknown_fields() {
        let error = serde_json::from_value::<DatabaseConfig>(serde_json::json!({
            "enabled": true,
            "backend": "sqlite",
            "sqlite_path": "center.db",
            "unknown": true
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown"));
    }

    #[test]
    fn database_config_debug_redacts_mysql_credentials() {
        let config = DatabaseConfig {
            enabled: true,
            backend: DbBackend::Mysql,
            sqlite_path: String::new(),
            mysql_url: Some("mysql://alice:top-secret@db/center".to_string()),
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("top-secret"));
        assert!(debug.contains("***"));
    }

    #[tokio::test]
    async fn directory_contract_fences_stale_sessions_and_evicts_idempotently() {
        let directory =
            SqlControllerDirectory::new(Arc::new(Store::open_in_memory().await.unwrap()));
        directory
            .upsert_registration(registration("c1", "s1", 1))
            .await
            .unwrap();
        directory
            .upsert_registration(registration("c1", "s2", 2))
            .await
            .unwrap();

        let stale = SessionId::new("s1").unwrap();
        let current = SessionId::new("s2").unwrap();
        let id = ControllerId::new("c1").unwrap();
        assert_eq!(
            directory.mark_offline(&id, &stale, None, 1).await.unwrap(),
            OfflineOutcome::NotCurrent
        );
        assert_eq!(
            directory.list().await.unwrap()[0].phase,
            ControllerPhase::Online
        );
        assert_eq!(
            directory
                .mark_offline(&id, &current, None, 2)
                .await
                .unwrap(),
            OfflineOutcome::Marked
        );
        assert_eq!(
            directory.list().await.unwrap()[0].phase,
            ControllerPhase::Offline
        );
        assert_eq!(
            directory.evict(&id).await.unwrap().outcome,
            EvictionOutcome::Evicted
        );
        assert_eq!(
            directory.evict(&id).await.unwrap().outcome,
            EvictionOutcome::AlreadyAbsent
        );
    }

    #[tokio::test]
    async fn sql_authorizer_delegates_permissions_and_denies_by_default() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let user = store.create_user("alice", "hash", "Alice").await.unwrap();
        let role = store.create_role("reader", "").await.unwrap();
        store
            .set_role_permissions(role, &["controllers:read".to_string()])
            .await
            .unwrap();
        store.set_user_roles(user, &[role]).await.unwrap();
        let authorizer = SqlAuthorizer::new(store);
        let principal = Principal {
            subject: "alice".to_string(),
            provider: "local".to_string(),
            issuer: None,
            groups: Vec::new(),
        };
        assert!(
            authorizer
                .authorize(
                    &principal,
                    &Action {
                        permission: "controllers:read".to_string(),
                        controller_id: None,
                        operation: None,
                        request_path: None,
                        request_verb: None,
                    },
                )
                .await
                .unwrap()
                .allowed
        );
        assert!(
            !authorizer
                .authorize(
                    &principal,
                    &Action {
                        permission: "controllers:write".to_string(),
                        controller_id: None,
                        operation: None,
                        request_path: None,
                        request_verb: None,
                    },
                )
                .await
                .unwrap()
                .allowed
        );
    }

    #[tokio::test]
    async fn sql_admin_hashes_passwords_and_reports_conflicts_without_sql_types() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let admin = SqlAdmin::new(store.clone());
        let id = admin
            .create_user(CreateUser {
                username: "alice".to_string(),
                password: "correct horse battery staple".to_string(),
                display_name: "Alice".to_string(),
                role_ids: Vec::new(),
            })
            .await
            .unwrap();
        let persisted = store.get_user(id).await.unwrap().unwrap();
        assert_ne!(persisted.password_hash, "correct horse battery staple");
        assert!(bcrypt::verify("correct horse battery staple", &persisted.password_hash).unwrap());
        assert_eq!(admin.list_users().await.unwrap()[0].username, "alice");

        let duplicate = admin
            .create_user(CreateUser {
                username: "alice".to_string(),
                password: "another password".to_string(),
                display_name: String::new(),
                role_ids: Vec::new(),
            })
            .await;
        assert!(matches!(duplicate, Err(CoreError::Conflict(_))));
    }
}
