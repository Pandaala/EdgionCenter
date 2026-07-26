//! GlobalResource desired-state persistence for standalone SQLite and MySQL deployments.

use std::sync::Arc;

use edgion_center_core::{
    global_resource_for_create, global_resource_for_replace,
    validate_global_resource_expected_generation, CoreError, CoreResult, GlobalResource,
    GlobalResourceCreateResult, GlobalResourceDesired, GlobalResourceId, GlobalResourcePage,
    GlobalResourcePageRequest, GlobalResourceReplaceResult, GlobalResourceStore,
    GlobalResourcesConfig,
};
use sqlx::Row;

use super::{core_adapter_error, Pool, Store};

const CONTRACT_VERSION: i64 = 1;
const MAX_RESOURCE_JSON_BYTES: usize = 1024 * 1024;
const SELECT_COLUMNS: &str = "global_resource_id, contract_version, generation, resource_json";

fn sql_error(error: sqlx::Error) -> CoreError {
    core_adapter_error(error.into())
}

fn generation_i64(generation: u64) -> CoreResult<i64> {
    i64::try_from(generation)
        .map_err(|_| CoreError::Conflict("global resource generation exceeds SQL range".into()))
}

fn decode_stored_generation(generation: i64) -> CoreResult<u64> {
    if generation <= 0 {
        return Err(CoreError::Adapter(
            "stored global resource generation is outside the persistence range".into(),
        ));
    }
    Ok(generation as u64)
}

fn resource_json(resource: &GlobalResource) -> CoreResult<String> {
    let encoded =
        serde_json::to_string(resource).map_err(|error| CoreError::Adapter(error.to_string()))?;
    if encoded.len() > MAX_RESOURCE_JSON_BYTES {
        return Err(CoreError::Conflict(
            "global resource exceeds its SQL persistence limit".into(),
        ));
    }
    Ok(encoded)
}

fn decode_resource(
    stored_id: Vec<u8>,
    contract_version: i64,
    stored_generation: i64,
    resource_json: Vec<u8>,
    config: &GlobalResourcesConfig,
) -> CoreResult<GlobalResource> {
    if contract_version != CONTRACT_VERSION {
        return Err(CoreError::Adapter(format!(
            "unsupported stored global resource contract version {contract_version}"
        )));
    }
    let stored_id = String::from_utf8(stored_id)
        .map_err(|_| CoreError::Adapter("stored global resource ID is not UTF-8".into()))?;
    let stored_id =
        GlobalResourceId::new(stored_id).map_err(|error| CoreError::Adapter(error.to_string()))?;
    let stored_generation = decode_stored_generation(stored_generation)?;
    let resource_json = String::from_utf8(resource_json)
        .map_err(|_| CoreError::Adapter("stored global resource JSON is not UTF-8".into()))?;
    let resource: GlobalResource = serde_json::from_str(&resource_json)
        .map_err(|error| CoreError::Adapter(format!("invalid stored global resource: {error}")))?;
    if resource.id != stored_id || resource.generation != stored_generation {
        return Err(CoreError::Adapter(
            "stored global resource envelope does not match its indexed identity".into(),
        ));
    }
    resource
        .validate(config)
        .map_err(|error| CoreError::Adapter(format!("invalid stored global resource: {error}")))?;
    Ok(resource)
}

/// SQL-backed GlobalResource desired-state store.
///
/// The wrapper owns the active platform-namespace configuration so every
/// decoded row is revalidated against the same policy used for writes.
#[derive(Clone)]
pub struct SqlGlobalResourceStore {
    store: Arc<Store>,
    config: GlobalResourcesConfig,
}

impl SqlGlobalResourceStore {
    pub fn new(store: Arc<Store>, config: GlobalResourcesConfig) -> CoreResult<Self> {
        config.validate().map_err(CoreError::Conflict)?;
        Ok(Self { store, config })
    }

    fn decode_sqlite(&self, row: &sqlx::sqlite::SqliteRow) -> CoreResult<GlobalResource> {
        decode_resource(
            row.try_get("global_resource_id").map_err(sql_error)?,
            row.try_get("contract_version").map_err(sql_error)?,
            row.try_get("generation").map_err(sql_error)?,
            row.try_get::<String, _>("resource_json")
                .map(String::into_bytes)
                .map_err(sql_error)?,
            &self.config,
        )
    }

    fn decode_mysql(&self, row: &sqlx::mysql::MySqlRow) -> CoreResult<GlobalResource> {
        decode_resource(
            row.try_get("global_resource_id").map_err(sql_error)?,
            row.try_get("contract_version").map_err(sql_error)?,
            row.try_get("generation").map_err(sql_error)?,
            row.try_get("resource_json").map_err(sql_error)?,
            &self.config,
        )
    }

    async fn generation(&self, id: &GlobalResourceId) -> CoreResult<Option<u64>> {
        let generation: Option<i64> = match &self.store.pool {
            Pool::Sqlite(pool) => sqlx::query_scalar(
                "SELECT generation FROM global_resources WHERE global_resource_id = ?",
            )
            .bind(id.as_str().as_bytes())
            .fetch_optional(pool)
            .await
            .map_err(sql_error)?,
            Pool::Mysql(pool) => sqlx::query_scalar(
                "SELECT generation FROM global_resources WHERE global_resource_id = ?",
            )
            .bind(id.as_str().as_bytes())
            .fetch_optional(pool)
            .await
            .map_err(sql_error)?,
        };
        generation.map(decode_stored_generation).transpose()
    }
}

#[async_trait::async_trait]
impl GlobalResourceStore for SqlGlobalResourceStore {
    async fn create(
        &self,
        id: &GlobalResourceId,
        desired: &GlobalResourceDesired,
        actor: &str,
        now_unix_ms: i64,
    ) -> CoreResult<GlobalResourceCreateResult> {
        let resource = global_resource_for_create(
            id.clone(),
            desired.clone(),
            actor,
            now_unix_ms,
            &self.config,
        )?;
        let resource_json = resource_json(&resource)?;
        let result = match &self.store.pool {
            Pool::Sqlite(pool) => sqlx::query(
                "INSERT INTO global_resources(global_resource_id, contract_version, generation, resource_json) VALUES (?, ?, 1, ?)",
            )
            .bind(id.as_str().as_bytes())
            .bind(CONTRACT_VERSION)
            .bind(&resource_json)
            .execute(pool)
            .await
            .map(|_| ()),
            Pool::Mysql(pool) => sqlx::query(
                "INSERT INTO global_resources(global_resource_id, contract_version, generation, resource_json) VALUES (?, ?, 1, ?)",
            )
            .bind(id.as_str().as_bytes())
            .bind(CONTRACT_VERSION)
            .bind(&resource_json)
            .execute(pool)
            .await
            .map(|_| ()),
        };
        match result {
            Ok(()) => Ok(GlobalResourceCreateResult::Created(Box::new(resource))),
            Err(error)
                if error
                    .as_database_error()
                    .is_some_and(|error| error.is_unique_violation()) =>
            {
                Ok(GlobalResourceCreateResult::AlreadyExists)
            }
            Err(error) => Err(sql_error(error)),
        }
    }

    async fn get(&self, id: &GlobalResourceId) -> CoreResult<Option<GlobalResource>> {
        id.validate()?;
        let sql =
            format!("SELECT {SELECT_COLUMNS} FROM global_resources WHERE global_resource_id = ?");
        match &self.store.pool {
            Pool::Sqlite(pool) => sqlx::query(&sql)
                .bind(id.as_str().as_bytes())
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
                .as_ref()
                .map(|row| self.decode_sqlite(row))
                .transpose(),
            Pool::Mysql(pool) => sqlx::query(&sql)
                .bind(id.as_str().as_bytes())
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
                .as_ref()
                .map(|row| self.decode_mysql(row))
                .transpose(),
        }
    }

    async fn list(&self, page: &GlobalResourcePageRequest) -> CoreResult<GlobalResourcePage> {
        page.validate()?;
        let fetch_limit = i64::from(page.limit) + 1;
        let sql = if page.after.is_some() {
            format!(
                "SELECT {SELECT_COLUMNS} FROM global_resources WHERE global_resource_id > ? ORDER BY global_resource_id ASC LIMIT ?"
            )
        } else {
            format!(
                "SELECT {SELECT_COLUMNS} FROM global_resources ORDER BY global_resource_id ASC LIMIT ?"
            )
        };
        let mut items = match &self.store.pool {
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
                    .map(|row| self.decode_sqlite(row))
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
                    .map(|row| self.decode_mysql(row))
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
                .id
                .clone()
        });
        let result = GlobalResourcePage { items, next };
        result.validate(page, &self.config)?;
        Ok(result)
    }

    async fn replace_if_generation(
        &self,
        id: &GlobalResourceId,
        expected_generation: u64,
        desired: &GlobalResourceDesired,
        actor: &str,
        now_unix_ms: i64,
    ) -> CoreResult<GlobalResourceReplaceResult> {
        id.validate()?;
        validate_global_resource_expected_generation(expected_generation)?;
        let Some(current) = self.get(id).await? else {
            return Ok(GlobalResourceReplaceResult::NotFound);
        };
        if current.generation != expected_generation {
            return Ok(GlobalResourceReplaceResult::GenerationMismatch {
                actual_generation: current.generation,
            });
        }
        let next = global_resource_for_replace(
            &current,
            desired.clone(),
            actor,
            now_unix_ms,
            &self.config,
        )?;
        let expected = generation_i64(expected_generation)?;
        let next_generation = generation_i64(next.generation)?;
        let resource_json = resource_json(&next)?;
        let rows_affected = match &self.store.pool {
            Pool::Sqlite(pool) => sqlx::query(
                "UPDATE global_resources SET generation = ?, resource_json = ? WHERE global_resource_id = ? AND generation = ?",
            )
            .bind(next_generation)
            .bind(&resource_json)
            .bind(id.as_str().as_bytes())
            .bind(expected)
            .execute(pool)
            .await
            .map_err(sql_error)?
            .rows_affected(),
            Pool::Mysql(pool) => sqlx::query(
                "UPDATE global_resources SET generation = ?, resource_json = ? WHERE global_resource_id = ? AND generation = ?",
            )
            .bind(next_generation)
            .bind(&resource_json)
            .bind(id.as_str().as_bytes())
            .bind(expected)
            .execute(pool)
            .await
            .map_err(sql_error)?
            .rows_affected(),
        };
        if rows_affected == 1 {
            return Ok(GlobalResourceReplaceResult::Stored(Box::new(next)));
        }
        match self.generation(id).await? {
            Some(actual_generation) => {
                Ok(GlobalResourceReplaceResult::GenerationMismatch { actual_generation })
            }
            None => Ok(GlobalResourceReplaceResult::NotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use edgion_center_core::{
        GlobalResourceAdoptionPolicy, GlobalResourceInventoryKind, GlobalResourcePrunePolicy,
        GlobalResourceSyncMode, GlobalResourceSyncPolicy, GlobalResourceTargetSelector,
    };
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn id(prefix: &str, suffix: &str) -> GlobalResourceId {
        GlobalResourceId::new(format!("{prefix}-{suffix}")).unwrap()
    }

    fn desired(namespace: &str, display_name: &str) -> GlobalResourceDesired {
        GlobalResourceDesired {
            display_name: display_name.to_string(),
            resource_kind: GlobalResourceInventoryKind::EdgionConfigData,
            target_namespace: namespace.to_string(),
            template_document: json!({
                "apiVersion": "edgion.io/v1alpha1",
                "kind": "EdgionConfigData",
                "metadata": {
                    "name": "shared-ip-list",
                    "namespace": namespace
                },
                "data": {
                    "type": "IpList",
                    "items": ["192.0.2.0/24"]
                }
            }),
            target_selector: GlobalResourceTargetSelector::ClusterIds {
                cluster_ids: BTreeSet::from(["cluster-a".to_string()]),
            },
            sync_policy: GlobalResourceSyncPolicy {
                mode: GlobalResourceSyncMode::Manual,
                adoption: GlobalResourceAdoptionPolicy::Never,
                prune: GlobalResourcePrunePolicy::Retain,
            },
        }
    }

    async fn exercise_store(store: &SqlGlobalResourceStore, prefix: &str) {
        let resource_id = id(prefix, "roundtrip");
        let initial = desired("edgion-data", "Shared IP list");
        let created = store
            .create(&resource_id, &initial, "alice", 100)
            .await
            .unwrap();
        let GlobalResourceCreateResult::Created(created) = created else {
            panic!("fresh resource was not created");
        };
        assert_eq!(created.generation, 1);
        assert_eq!(created.created_by, "alice");
        assert_eq!(
            store.get(&resource_id).await.unwrap(),
            Some(created.as_ref().clone())
        );
        assert_eq!(
            store
                .create(
                    &resource_id,
                    &desired("edgion-data", "Conflicting"),
                    "mallory",
                    200,
                )
                .await
                .unwrap(),
            GlobalResourceCreateResult::AlreadyExists
        );

        let replacement = desired("edgion-data", "Shared IP list v2");
        let replaced = store
            .replace_if_generation(&resource_id, 1, &replacement, "bob", 200)
            .await
            .unwrap();
        let GlobalResourceReplaceResult::Stored(replaced) = replaced else {
            panic!("exact generation was not stored");
        };
        assert_eq!(replaced.generation, 2);
        assert_eq!(replaced.created_by, "alice");
        assert_eq!(replaced.updated_by, "bob");
        assert_eq!(
            store
                .replace_if_generation(&resource_id, 1, &initial, "alice", 300)
                .await
                .unwrap(),
            GlobalResourceReplaceResult::GenerationMismatch {
                actual_generation: 2
            }
        );
        assert_eq!(
            store
                .replace_if_generation(&id(prefix, "missing"), 1, &initial, "alice", 300,)
                .await
                .unwrap(),
            GlobalResourceReplaceResult::NotFound
        );

        for suffix in ["page-a", "page-b", "page-c"] {
            let result = store
                .create(
                    &id(prefix, suffix),
                    &desired("edgion-data", suffix),
                    "alice",
                    100,
                )
                .await
                .unwrap();
            assert!(matches!(result, GlobalResourceCreateResult::Created(_)));
        }
        let first = store
            .list(&GlobalResourcePageRequest {
                limit: 2,
                after: Some(id(prefix, "page")),
            })
            .await
            .unwrap();
        assert_eq!(first.items.len(), 2);
        assert!(first.next.is_some());
        let second = store
            .list(&GlobalResourcePageRequest {
                limit: 2,
                after: first.next,
            })
            .await
            .unwrap();
        assert!(!second.items.is_empty());
    }

    #[tokio::test]
    async fn sqlite_global_resource_store_roundtrip_pagination_and_cas() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let store = SqlGlobalResourceStore::new(store, GlobalResourcesConfig::default()).unwrap();
        exercise_store(&store, &format!("sqlite-{}", Uuid::new_v4())).await;
    }

    #[tokio::test]
    async fn sqlite_supports_custom_platform_namespace() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let store = SqlGlobalResourceStore::new(
            store,
            GlobalResourcesConfig {
                platform_namespaces: vec!["edge-global".to_string()],
            },
        )
        .unwrap();
        let resource_id = id(&format!("custom-{}", Uuid::new_v4()), "resource");
        assert!(matches!(
            store
                .create(
                    &resource_id,
                    &desired("edge-global", "Custom namespace"),
                    "alice",
                    100,
                )
                .await
                .unwrap(),
            GlobalResourceCreateResult::Created(_)
        ));
    }

    #[tokio::test]
    async fn sqlite_rejects_corrupt_envelope_and_schema_violations() {
        let base = Arc::new(Store::open_in_memory().await.unwrap());
        let store =
            SqlGlobalResourceStore::new(base.clone(), GlobalResourcesConfig::default()).unwrap();
        let Pool::Sqlite(pool) = &base.pool else {
            unreachable!()
        };
        let resource_id = id(&format!("corrupt-{}", Uuid::new_v4()), "resource");
        sqlx::query(
            "INSERT INTO global_resources(global_resource_id, contract_version, generation, resource_json) VALUES (?, 1, 1, ?)",
        )
        .bind(resource_id.as_str().as_bytes())
        .bind("{}")
        .execute(pool)
        .await
        .unwrap();
        assert!(matches!(
            store.get(&resource_id).await,
            Err(CoreError::Adapter(_))
        ));

        for (suffix, generation, payload) in [
            ("zero-generation", 0_i64, "{}".to_string()),
            ("empty-payload", 1, String::new()),
            (
                "oversized-payload",
                1,
                "x".repeat(MAX_RESOURCE_JSON_BYTES + 1),
            ),
        ] {
            let result = sqlx::query(
                "INSERT INTO global_resources(global_resource_id, contract_version, generation, resource_json) VALUES (?, 1, ?, ?)",
            )
            .bind(id(&format!("schema-{}", Uuid::new_v4()), suffix).as_str().as_bytes())
            .bind(generation)
            .bind(payload)
            .execute(pool)
            .await;
            assert!(result.is_err(), "{suffix}");
        }
    }

    #[tokio::test]
    async fn concurrent_replacement_has_one_cas_winner() {
        let base = Arc::new(Store::open_in_memory().await.unwrap());
        let store = SqlGlobalResourceStore::new(base, GlobalResourcesConfig::default()).unwrap();
        let resource_id = id(&format!("concurrent-{}", Uuid::new_v4()), "resource");
        store
            .create(
                &resource_id,
                &desired("edgion-data", "Initial"),
                "alice",
                100,
            )
            .await
            .unwrap();
        let left_desired = desired("edgion-data", "Left");
        let right_desired = desired("edgion-data", "Right");
        let (left, right) = tokio::join!(
            store.replace_if_generation(&resource_id, 1, &left_desired, "left", 200,),
            store.replace_if_generation(&resource_id, 1, &right_desired, "right", 200,)
        );
        let outcomes = [left.unwrap(), right.unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|result| matches!(result, GlobalResourceReplaceResult::Stored(_)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|result| matches!(
                    result,
                    GlobalResourceReplaceResult::GenerationMismatch {
                        actual_generation: 2
                    }
                ))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn mysql_global_resource_store_conformance() {
        use crate::{DatabaseConfig, DbBackend};

        let Ok(url) = std::env::var("EDGION_TEST_MYSQL_URL") else {
            eprintln!("skipping: EDGION_TEST_MYSQL_URL unset");
            return;
        };
        let _external_database_guard = crate::MYSQL_TEST_LOCK.lock().await;
        let base = Arc::new(
            Store::connect(&DatabaseConfig {
                enabled: true,
                backend: DbBackend::Mysql,
                sqlite_path: String::new(),
                mysql_url: Some(url),
            })
            .await
            .unwrap(),
        );
        let store = SqlGlobalResourceStore::new(base, GlobalResourcesConfig::default()).unwrap();
        exercise_store(&store, &format!("mysql-{}", Uuid::new_v4())).await;
    }
}
