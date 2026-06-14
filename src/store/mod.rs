//! Persistence layer for the Center metadata store.
//!
//! A single async [`Store`] sits in front of either SQLite (embedded, default)
//! or MySQL (external), selected by [`crate::config::DbBackend`]. All queries use
//! sqlx *runtime* APIs (`sqlx::query` + `.bind`), never the compile-time-checked
//! macros, so a plain `cargo build` needs no live `DATABASE_URL`.

use crate::config::{DatabaseConfig, DbBackend};
use sqlx::mysql::MySqlPool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

pub mod audit;
pub mod controllers;

pub use controllers::DbController;

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
                    .busy_timeout(std::time::Duration::from_secs(5));
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
        Ok(store)
    }

    /// Run the embedded migrations for the active backend. sqlx tracks applied
    /// migrations in `_sqlx_migrations`, so this is safe to call repeatedly.
    pub async fn migrate(&self) -> anyhow::Result<()> {
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::migrate!("src/store/migrations/sqlite").run(pool).await?;
            }
            Pool::Mysql(pool) => {
                sqlx::migrate!("src/store/migrations/mysql").run(pool).await?;
            }
        }
        Ok(())
    }

    /// Open a fresh in-memory SQLite store for tests.
    ///
    /// `max_connections(1)` keeps every query on the same connection so the
    /// in-memory database (which is per-connection in SQLite) is shared across
    /// migrate + queries.
    #[cfg(test)]
    pub async fn open_in_memory() -> anyhow::Result<Store> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let store = Store {
            pool: Pool::Sqlite(pool),
        };
        store.migrate().await?;
        Ok(store)
    }
}
