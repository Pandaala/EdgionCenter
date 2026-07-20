//! ProviderAccount desired-state persistence for standalone SQLite and MySQL deployments.

use edgion_center_core::{
    provider_account_from_desired, validate_stored_provider_account, CloudResourceId, CoreError,
    CoreResult, ProviderAccount, ProviderAccountCreateResult, ProviderAccountDesired,
    ProviderAccountPage, ProviderAccountPageRequest, ProviderAccountReplaceResult,
    ProviderAccountStore,
};
use sqlx::Row;

use super::{core_adapter_error, Pool, Store};

const SELECT_COLUMNS: &str = "account_id, generation, desired_json";

fn sql_error(error: sqlx::Error) -> CoreError {
    core_adapter_error(error.into())
}

fn desired_json(desired: &ProviderAccountDesired) -> CoreResult<String> {
    desired.validate()?;
    serde_json::to_string(desired).map_err(|error| CoreError::Adapter(error.to_string()))
}

fn generation_i64(generation: u64) -> CoreResult<i64> {
    i64::try_from(generation)
        .map_err(|_| CoreError::Conflict("provider account generation exceeds SQL range".into()))
}

fn decode_stored_generation(generation: i64) -> CoreResult<u64> {
    if generation <= 0 {
        return Err(CoreError::Adapter(
            "stored provider account generation is outside the persistence range".into(),
        ));
    }
    Ok(generation as u64)
}

fn decode_account(
    account_id: Vec<u8>,
    generation: i64,
    desired_json: String,
) -> CoreResult<ProviderAccount> {
    let account_id = String::from_utf8(account_id)
        .map_err(|_| CoreError::Adapter("stored provider account ID is not UTF-8".into()))?;
    let account_id =
        CloudResourceId::new(account_id).map_err(|error| CoreError::Adapter(error.to_string()))?;
    let generation = decode_stored_generation(generation)?;
    let desired: ProviderAccountDesired = serde_json::from_str(&desired_json)
        .map_err(|error| CoreError::Adapter(format!("invalid stored provider account: {error}")))?;
    let account = provider_account_from_desired(account_id, generation, &desired)
        .map_err(|error| CoreError::Adapter(format!("invalid stored provider account: {error}")))?;
    validate_stored_provider_account(&account)
        .map_err(|error| CoreError::Adapter(format!("invalid stored provider account: {error}")))?;
    Ok(account)
}

fn decode_sqlite(row: &sqlx::sqlite::SqliteRow) -> CoreResult<ProviderAccount> {
    decode_account(
        row.try_get("account_id").map_err(sql_error)?,
        row.try_get("generation").map_err(sql_error)?,
        row.try_get("desired_json").map_err(sql_error)?,
    )
}

fn decode_mysql(row: &sqlx::mysql::MySqlRow) -> CoreResult<ProviderAccount> {
    decode_account(
        row.try_get("account_id").map_err(sql_error)?,
        row.try_get("generation").map_err(sql_error)?,
        row.try_get("desired_json").map_err(sql_error)?,
    )
}

impl Store {
    async fn provider_account_generation(
        &self,
        account_id: &CloudResourceId,
    ) -> CoreResult<Option<u64>> {
        let generation: Option<i64> = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query_scalar(
                "SELECT generation FROM cloud_provider_accounts WHERE account_id = ?",
            )
            .bind(account_id.as_str().as_bytes())
            .fetch_optional(pool)
            .await
            .map_err(sql_error)?,
            Pool::Mysql(pool) => sqlx::query_scalar(
                "SELECT generation FROM cloud_provider_accounts WHERE account_id = ?",
            )
            .bind(account_id.as_str().as_bytes())
            .fetch_optional(pool)
            .await
            .map_err(sql_error)?,
        };
        generation.map(decode_stored_generation).transpose()
    }
}

#[async_trait::async_trait]
impl ProviderAccountStore for Store {
    async fn create(
        &self,
        account_id: &CloudResourceId,
        desired: &ProviderAccountDesired,
    ) -> CoreResult<ProviderAccountCreateResult> {
        account_id.validate()?;
        let desired_json = desired_json(desired)?;
        let account = provider_account_from_desired(account_id.clone(), 1, desired)?;
        let result = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(
                "INSERT INTO cloud_provider_accounts(account_id, generation, desired_json) VALUES (?, 1, ?)",
            )
            .bind(account_id.as_str().as_bytes())
            .bind(&desired_json)
            .execute(pool)
            .await
            .map(|_| ()),
            Pool::Mysql(pool) => sqlx::query(
                "INSERT INTO cloud_provider_accounts(account_id, generation, desired_json) VALUES (?, 1, ?)",
            )
            .bind(account_id.as_str().as_bytes())
            .bind(&desired_json)
            .execute(pool)
            .await
            .map(|_| ()),
        };
        match result {
            Ok(_) => Ok(ProviderAccountCreateResult::Created(Box::new(account))),
            Err(error)
                if error
                    .as_database_error()
                    .is_some_and(|e| e.is_unique_violation()) =>
            {
                Ok(ProviderAccountCreateResult::AlreadyExists)
            }
            Err(error) => Err(sql_error(error)),
        }
    }

    async fn get(&self, account_id: &CloudResourceId) -> CoreResult<Option<ProviderAccount>> {
        account_id.validate()?;
        let sql =
            format!("SELECT {SELECT_COLUMNS} FROM cloud_provider_accounts WHERE account_id = ?");
        match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(&sql)
                .bind(account_id.as_str().as_bytes())
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
                .as_ref()
                .map(decode_sqlite)
                .transpose(),
            Pool::Mysql(pool) => sqlx::query(&sql)
                .bind(account_id.as_str().as_bytes())
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
                .as_ref()
                .map(decode_mysql)
                .transpose(),
        }
    }

    async fn list(&self, page: &ProviderAccountPageRequest) -> CoreResult<ProviderAccountPage> {
        page.validate()?;
        let fetch_limit = i64::from(page.limit) + 1;
        let sql = if page.after.is_some() {
            format!("SELECT {SELECT_COLUMNS} FROM cloud_provider_accounts WHERE account_id > ? ORDER BY account_id ASC LIMIT ?")
        } else {
            format!("SELECT {SELECT_COLUMNS} FROM cloud_provider_accounts ORDER BY account_id ASC LIMIT ?")
        };
        let mut items = match &self.pool {
            Pool::Sqlite(pool) => {
                let query = sqlx::query(&sql);
                let query = if let Some(after) = page.after.as_ref() {
                    query.bind(after.as_str().as_bytes())
                } else {
                    query
                };
                query
                    .bind(fetch_limit)
                    .fetch_all(pool)
                    .await
                    .map_err(sql_error)?
                    .iter()
                    .map(decode_sqlite)
                    .collect::<CoreResult<Vec<_>>>()?
            }
            Pool::Mysql(pool) => {
                let query = sqlx::query(&sql);
                let query = if let Some(after) = page.after.as_ref() {
                    query.bind(after.as_str().as_bytes())
                } else {
                    query
                };
                query
                    .bind(fetch_limit)
                    .fetch_all(pool)
                    .await
                    .map_err(sql_error)?
                    .iter()
                    .map(decode_mysql)
                    .collect::<CoreResult<Vec<_>>>()?
            }
        };
        let has_more = items.len() > usize::from(page.limit);
        if has_more {
            items.pop();
        }
        let next = has_more.then(|| {
            items
                .last()
                .expect("positive page limit returns an item before the extra row")
                .metadata
                .id
                .clone()
        });
        let result = ProviderAccountPage { items, next };
        result.validate(page)?;
        Ok(result)
    }

    async fn replace_if_generation(
        &self,
        account_id: &CloudResourceId,
        expected_generation: u64,
        desired: &ProviderAccountDesired,
    ) -> CoreResult<ProviderAccountReplaceResult> {
        account_id.validate()?;
        let expected = generation_i64(expected_generation)?;
        let next = expected_generation.checked_add(1).ok_or_else(|| {
            CoreError::Conflict("provider account generation exceeds SQL range".into())
        })?;
        let next_i64 = generation_i64(next)?;
        if expected <= 0 {
            return Err(CoreError::Conflict(
                "expected provider account generation must be positive".into(),
            ));
        }
        let desired_json = desired_json(desired)?;
        let account = provider_account_from_desired(account_id.clone(), next, desired)?;
        let rows_affected = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query("UPDATE cloud_provider_accounts SET generation = ?, desired_json = ? WHERE account_id = ? AND generation = ?")
                .bind(next_i64)
                .bind(&desired_json)
                .bind(account_id.as_str().as_bytes())
                .bind(expected)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
            Pool::Mysql(pool) => sqlx::query("UPDATE cloud_provider_accounts SET generation = ?, desired_json = ? WHERE account_id = ? AND generation = ?")
                .bind(next_i64)
                .bind(&desired_json)
                .bind(account_id.as_str().as_bytes())
                .bind(expected)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
        };
        if rows_affected == 1 {
            return Ok(ProviderAccountReplaceResult::Stored(Box::new(account)));
        }
        match self.provider_account_generation(account_id).await? {
            Some(actual_generation) => {
                Ok(ProviderAccountReplaceResult::GenerationMismatch { actual_generation })
            }
            None => Ok(ProviderAccountReplaceResult::NotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use edgion_center_core::cloud_test_support::assert_provider_account_store_conformance;
    use uuid::Uuid;

    use super::*;

    #[tokio::test]
    async fn sqlite_provider_account_conformance() {
        let store = Store::open_in_memory().await.unwrap();
        assert_provider_account_store_conformance(
            &store,
            &format!("sql-sqlite-{}", Uuid::new_v4()),
        )
        .await;
    }

    #[tokio::test]
    async fn sqlite_rejects_corrupt_persisted_desired_state() {
        let store = Store::open_in_memory().await.unwrap();
        let Pool::Sqlite(pool) = &store.pool else {
            unreachable!()
        };
        let id = CloudResourceId::new(format!("sql-corrupt-{}", Uuid::new_v4())).unwrap();
        sqlx::query("INSERT INTO cloud_provider_accounts(account_id, generation, desired_json) VALUES (?, 1, ?)")
            .bind(id.as_str().as_bytes())
            .bind("{}")
            .execute(pool)
            .await
            .unwrap();
        assert!(matches!(store.get(&id).await, Err(CoreError::Adapter(_))));
    }

    #[test]
    fn stored_generation_decoder_rejects_non_positive_values_as_adapter_errors() {
        for generation in [i64::MIN, -1, 0] {
            assert!(matches!(
                decode_stored_generation(generation),
                Err(CoreError::Adapter(_))
            ));
        }
        assert_eq!(decode_stored_generation(1).unwrap(), 1);
        assert_eq!(decode_stored_generation(i64::MAX).unwrap(), i64::MAX as u64);
    }

    #[tokio::test]
    async fn sqlite_schema_enforces_generation_and_payload_bounds() {
        let store = Store::open_in_memory().await.unwrap();
        let Pool::Sqlite(pool) = &store.pool else {
            unreachable!()
        };
        for generation in [0_i64, -1] {
            let result = sqlx::query("INSERT INTO cloud_provider_accounts(account_id, generation, desired_json) VALUES (?, ?, ?)")
                .bind(format!("invalid-generation-{generation}").into_bytes())
                .bind(generation)
                .bind("{}")
                .execute(pool)
                .await;
            assert!(result.is_err());
        }
        let result = sqlx::query("INSERT INTO cloud_provider_accounts(account_id, generation, desired_json) VALUES (?, 1, ?)")
            .bind(b"oversized-payload".as_slice())
            .bind("x".repeat(65_537))
            .execute(pool)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mysql_provider_account_conformance() {
        use crate::{DatabaseConfig, DbBackend};

        let Ok(url) = std::env::var("EDGION_TEST_MYSQL_URL") else {
            eprintln!("skipping: EDGION_TEST_MYSQL_URL unset");
            return;
        };
        let _external_database_guard = crate::MYSQL_TEST_LOCK.lock().await;
        let store = Store::connect(&DatabaseConfig {
            enabled: true,
            backend: DbBackend::Mysql,
            sqlite_path: String::new(),
            mysql_url: Some(url),
        })
        .await
        .unwrap();
        assert_provider_account_store_conformance(&store, &format!("sql-mysql-{}", Uuid::new_v4()))
            .await;
    }
}
