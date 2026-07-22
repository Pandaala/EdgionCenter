//! Capability snapshot persistence for standalone SQLite and MySQL deployments.

use edgion_center_core::{
    is_retired_capability_snapshot_json, validate_write, CapabilityDiscoveryFence, CapabilityScope,
    CapabilitySnapshotKey, CapabilitySnapshotStore, CapabilityStoreWrite, CoreError, CoreResult,
    DiscoveryToken, ProviderCapabilitySnapshot,
};
use sqlx::Row;
use uuid::Uuid;

use super::{core_adapter_error, Pool, Store};

const MAX_SCOPE_KEY_LEN: usize = 2_048;
fn sql_error(error: sqlx::Error) -> CoreError {
    core_adapter_error(error.into())
}

fn checked_i64(value: u64, kind: &str) -> CoreResult<i64> {
    i64::try_from(value).map_err(|_| CoreError::Conflict(format!("{kind} exceeds SQL range")))
}

fn optional_bytes(value: Option<&str>) -> Option<&[u8]> {
    value.map(str::as_bytes)
}

fn utf8(value: Vec<u8>, kind: &str) -> CoreResult<String> {
    String::from_utf8(value)
        .map_err(|_| CoreError::Adapter(format!("stored {kind} is not valid UTF-8")))
}

fn enum_text<T: serde::Serialize>(value: &T) -> CoreResult<String> {
    serde_json::to_value(value)
        .map_err(|error| CoreError::Adapter(error.to_string()))?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| CoreError::Adapter("enum did not serialize as a string".to_string()))
}

fn append_component(target: &mut Vec<u8>, value: &[u8]) -> CoreResult<()> {
    let length = u32::try_from(value.len())
        .map_err(|_| CoreError::Conflict("capability scope component is too long".to_string()))?;
    target.extend_from_slice(&length.to_be_bytes());
    target.extend_from_slice(value);
    Ok(())
}

/// Produces an unambiguous, binary key. Length prefixes avoid delimiter and JSON-escaping
/// ambiguities while preserving opaque provider identifiers exactly.
fn scope_key(scope: &CapabilityScope) -> CoreResult<Vec<u8>> {
    let mut key = Vec::new();
    match scope {
        CapabilityScope::Account => key.push(b'a'),
        CapabilityScope::Region { region } => {
            key.push(b'r');
            append_component(&mut key, region.as_str().as_bytes())?;
        }
        CapabilityScope::Resource {
            resource_kind,
            resource,
        } => {
            key.push(b'x');
            append_component(&mut key, enum_text(resource_kind)?.as_bytes())?;
            append_component(&mut key, resource.provider_account_id.as_str().as_bytes())?;
            append_component(&mut key, resource.external_id.as_bytes())?;
        }
    }
    if key.len() > MAX_SCOPE_KEY_LEN {
        return Err(CoreError::Conflict(
            "capability scope exceeds the SQL persistence limit".to_string(),
        ));
    }
    Ok(key)
}

fn decode_snapshot(
    key: &CapabilitySnapshotKey,
    scope_json: String,
    snapshot_json: String,
) -> CoreResult<Option<ProviderCapabilitySnapshot>> {
    let stored_scope: CapabilityScope = serde_json::from_str(&scope_json)
        .map_err(|error| CoreError::Adapter(format!("invalid stored capability scope: {error}")))?;
    if stored_scope != key.scope {
        return Err(CoreError::Adapter(
            "stored capability scope does not match its key".to_string(),
        ));
    }
    let snapshot: ProviderCapabilitySnapshot = match serde_json::from_str(&snapshot_json) {
        Ok(snapshot) => snapshot,
        Err(_) if is_retired_capability_snapshot_json(&snapshot_json) => return Ok(None),
        Err(error) => {
            return Err(CoreError::Adapter(format!(
                "invalid stored capability snapshot: {error}"
            )))
        }
    };
    // Refresh authority and committed snapshot are intentionally separate. While a refresh is
    // in flight, readers may still evaluate the previous snapshot's own generation and TTL.
    validate_write(key, &snapshot.fence, &snapshot).map_err(|error| {
        CoreError::Adapter(format!(
            "stored capability snapshot authority is invalid: {error}"
        ))
    })?;
    Ok(Some(snapshot))
}

impl Store {
    async fn get_capability_snapshot(
        &self,
        key: &CapabilitySnapshotKey,
    ) -> CoreResult<Option<ProviderCapabilitySnapshot>> {
        key.validate()?;
        let encoded_scope = scope_key(&key.scope)?;
        let sql = "SELECT scope_json, snapshot_json FROM cloud_capability_snapshots WHERE provider_account_id = ? AND scope_key = ?";
        match &self.pool {
            Pool::Sqlite(pool) => {
                let Some(row) = sqlx::query(sql)
                    .bind(key.provider_account_id.as_str().as_bytes())
                    .bind(&encoded_scope)
                    .fetch_optional(pool)
                    .await
                    .map_err(sql_error)?
                else {
                    return Ok(None);
                };
                let Some(snapshot_json) = row
                    .try_get::<Option<String>, _>("snapshot_json")
                    .map_err(sql_error)?
                else {
                    return Ok(None);
                };
                decode_snapshot(
                    key,
                    row.try_get("scope_json").map_err(sql_error)?,
                    snapshot_json,
                )
            }
            Pool::Mysql(pool) => {
                let Some(row) = sqlx::query(sql)
                    .bind(key.provider_account_id.as_str().as_bytes())
                    .bind(&encoded_scope)
                    .fetch_optional(pool)
                    .await
                    .map_err(sql_error)?
                else {
                    return Ok(None);
                };
                let Some(snapshot_json) = row
                    .try_get::<Option<String>, _>("snapshot_json")
                    .map_err(sql_error)?
                else {
                    return Ok(None);
                };
                decode_snapshot(
                    key,
                    row.try_get("scope_json").map_err(sql_error)?,
                    snapshot_json,
                )
            }
        }
    }
}

#[async_trait::async_trait]
impl CapabilitySnapshotStore for Store {
    async fn get(
        &self,
        key: &CapabilitySnapshotKey,
    ) -> CoreResult<Option<ProviderCapabilitySnapshot>> {
        self.get_capability_snapshot(key).await
    }

    async fn begin_discovery(
        &self,
        key: &CapabilitySnapshotKey,
        provider_account_generation: u64,
        credential_revision: Option<&str>,
    ) -> CoreResult<CapabilityDiscoveryFence> {
        key.validate()?;
        let generation = checked_i64(provider_account_generation, "provider account generation")?;
        if generation <= 0 {
            return Err(CoreError::Conflict(
                "provider account generation must be positive".to_string(),
            ));
        }
        let probe_fence = CapabilityDiscoveryFence {
            provider_account_generation,
            credential_revision: credential_revision.map(str::to_owned),
            discovery_epoch: 1,
            discovery_token: DiscoveryToken::new("validation-token")?,
        };
        probe_fence.validate()?;

        let encoded_scope = scope_key(&key.scope)?;
        let scope_json = serde_json::to_string(&key.scope)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        let token = Uuid::new_v4().to_string();
        let epoch = match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query_scalar::<_, i64>(
                    "INSERT INTO cloud_capability_snapshots (provider_account_id, scope_key, scope_json, provider_account_generation, credential_revision, discovery_epoch, discovery_token, snapshot_json, snapshot_provider_account_generation, snapshot_credential_revision, snapshot_discovery_epoch, snapshot_discovery_token, snapshot_write_token) VALUES (?, ?, ?, ?, ?, 1, ?, NULL, NULL, NULL, NULL, NULL, NULL) ON CONFLICT(provider_account_id, scope_key) DO UPDATE SET scope_json = excluded.scope_json, provider_account_generation = excluded.provider_account_generation, credential_revision = excluded.credential_revision, discovery_epoch = discovery_epoch + 1, discovery_token = excluded.discovery_token WHERE discovery_epoch < 9223372036854775807 RETURNING discovery_epoch",
                )
                .bind(key.provider_account_id.as_str().as_bytes())
                .bind(&encoded_scope)
                .bind(&scope_json)
                .bind(generation)
                .bind(optional_bytes(credential_revision))
                .bind(token.as_bytes())
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
                .ok_or_else(|| {
                    CoreError::Conflict("capability discovery epoch is exhausted".to_string())
                })?
            }
            Pool::Mysql(pool) => {
                // MySQL evaluates assignments left-to-right. Keep discovery_epoch last so every
                // preceding assignment observes the old value for the overflow guard.
                let mut transaction = pool.begin().await.map_err(sql_error)?;
                sqlx::query(
                    "INSERT INTO cloud_capability_snapshots (provider_account_id, scope_key, scope_json, provider_account_generation, credential_revision, discovery_epoch, discovery_token, snapshot_json, snapshot_provider_account_generation, snapshot_credential_revision, snapshot_discovery_epoch, snapshot_discovery_token, snapshot_write_token) VALUES (?, ?, ?, ?, ?, 1, ?, NULL, NULL, NULL, NULL, NULL, NULL) ON DUPLICATE KEY UPDATE scope_json = IF(discovery_epoch < 9223372036854775807, VALUES(scope_json), scope_json), provider_account_generation = IF(discovery_epoch < 9223372036854775807, VALUES(provider_account_generation), provider_account_generation), credential_revision = IF(discovery_epoch < 9223372036854775807, VALUES(credential_revision), credential_revision), discovery_token = IF(discovery_epoch < 9223372036854775807, VALUES(discovery_token), discovery_token), discovery_epoch = IF(discovery_epoch < 9223372036854775807, discovery_epoch + 1, discovery_epoch)",
                )
                .bind(key.provider_account_id.as_str().as_bytes())
                .bind(&encoded_scope)
                .bind(&scope_json)
                .bind(generation)
                .bind(optional_bytes(credential_revision))
                .bind(token.as_bytes())
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
                let epoch = sqlx::query_scalar::<_, i64>(
                    "SELECT discovery_epoch FROM cloud_capability_snapshots WHERE provider_account_id = ? AND scope_key = ? AND provider_account_generation = ? AND credential_revision <=> ? AND discovery_token = ?",
                )
                .bind(key.provider_account_id.as_str().as_bytes())
                .bind(&encoded_scope)
                .bind(generation)
                .bind(optional_bytes(credential_revision))
                .bind(token.as_bytes())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(sql_error)?
                .ok_or_else(|| {
                    CoreError::Conflict("capability discovery epoch is exhausted".to_string())
                })?;
                transaction.commit().await.map_err(sql_error)?;
                epoch
            }
        };
        let fence = CapabilityDiscoveryFence {
            provider_account_generation,
            credential_revision: credential_revision.map(str::to_owned),
            discovery_epoch: u64::try_from(epoch).map_err(|_| {
                CoreError::Adapter("stored capability discovery epoch is invalid".to_string())
            })?,
            discovery_token: DiscoveryToken::new(token)?,
        };
        fence.validate()?;
        Ok(fence)
    }

    async fn put_if_current(
        &self,
        key: &CapabilitySnapshotKey,
        expected_fence: &CapabilityDiscoveryFence,
        snapshot: &ProviderCapabilitySnapshot,
    ) -> CoreResult<CapabilityStoreWrite> {
        validate_write(key, expected_fence, snapshot)?;
        let encoded_scope = scope_key(&key.scope)?;
        let generation = checked_i64(
            expected_fence.provider_account_generation,
            "provider account generation",
        )?;
        let epoch = checked_i64(expected_fence.discovery_epoch, "capability discovery epoch")?;
        let snapshot_json = serde_json::to_string(snapshot)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        // The extra token forces an actual row change for an idempotent retry. MySQL otherwise
        // reports zero changed rows, which is indistinguishable from a lost fence.
        let write_token = Uuid::new_v4().to_string();
        let sql = match &self.pool {
            Pool::Sqlite(_) => {
                "UPDATE cloud_capability_snapshots SET snapshot_json = ?, snapshot_provider_account_generation = ?, snapshot_credential_revision = ?, snapshot_discovery_epoch = ?, snapshot_discovery_token = ?, snapshot_write_token = ? WHERE provider_account_id = ? AND scope_key = ? AND provider_account_generation = ? AND credential_revision IS ? AND discovery_epoch = ? AND discovery_token = ? AND (snapshot_discovery_epoch IS NOT ? OR snapshot_discovery_token IS NOT ? OR snapshot_json = ?)"
            }
            Pool::Mysql(_) => {
                "UPDATE cloud_capability_snapshots SET snapshot_json = ?, snapshot_provider_account_generation = ?, snapshot_credential_revision = ?, snapshot_discovery_epoch = ?, snapshot_discovery_token = ?, snapshot_write_token = ? WHERE provider_account_id = ? AND scope_key = ? AND provider_account_generation = ? AND credential_revision <=> ? AND discovery_epoch = ? AND discovery_token = ? AND (NOT (snapshot_discovery_epoch <=> ?) OR NOT (snapshot_discovery_token <=> ?) OR snapshot_json = ?)"
            }
        };
        let affected = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(sql)
                .bind(&snapshot_json)
                .bind(generation)
                .bind(optional_bytes(
                    expected_fence.credential_revision.as_deref(),
                ))
                .bind(epoch)
                .bind(expected_fence.discovery_token.as_str().as_bytes())
                .bind(write_token.as_bytes())
                .bind(key.provider_account_id.as_str().as_bytes())
                .bind(&encoded_scope)
                .bind(generation)
                .bind(optional_bytes(
                    expected_fence.credential_revision.as_deref(),
                ))
                .bind(epoch)
                .bind(expected_fence.discovery_token.as_str().as_bytes())
                .bind(epoch)
                .bind(expected_fence.discovery_token.as_str().as_bytes())
                .bind(&snapshot_json)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
            Pool::Mysql(pool) => sqlx::query(sql)
                .bind(&snapshot_json)
                .bind(generation)
                .bind(optional_bytes(
                    expected_fence.credential_revision.as_deref(),
                ))
                .bind(epoch)
                .bind(expected_fence.discovery_token.as_str().as_bytes())
                .bind(write_token.as_bytes())
                .bind(key.provider_account_id.as_str().as_bytes())
                .bind(&encoded_scope)
                .bind(generation)
                .bind(optional_bytes(
                    expected_fence.credential_revision.as_deref(),
                ))
                .bind(epoch)
                .bind(expected_fence.discovery_token.as_str().as_bytes())
                .bind(epoch)
                .bind(expected_fence.discovery_token.as_str().as_bytes())
                .bind(&snapshot_json)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
        };
        if affected == 1 {
            return Ok(CapabilityStoreWrite::Stored);
        }

        let sql = "SELECT provider_account_generation, credential_revision, discovery_epoch, discovery_token, snapshot_json FROM cloud_capability_snapshots WHERE provider_account_id = ? AND scope_key = ?";
        let row = match &self.pool {
            Pool::Sqlite(pool) => {
                let row = sqlx::query(sql)
                    .bind(key.provider_account_id.as_str().as_bytes())
                    .bind(&encoded_scope)
                    .fetch_optional(pool)
                    .await
                    .map_err(sql_error)?;
                row.map(|row| {
                    Ok((
                        row.try_get::<i64, _>("provider_account_generation")
                            .map_err(sql_error)?,
                        row.try_get::<Option<Vec<u8>>, _>("credential_revision")
                            .map_err(sql_error)?,
                        row.try_get::<i64, _>("discovery_epoch")
                            .map_err(sql_error)?,
                        row.try_get::<Vec<u8>, _>("discovery_token")
                            .map_err(sql_error)?,
                        row.try_get::<Option<String>, _>("snapshot_json")
                            .map_err(sql_error)?,
                    ))
                })
                .transpose()?
            }
            Pool::Mysql(pool) => {
                let row = sqlx::query(sql)
                    .bind(key.provider_account_id.as_str().as_bytes())
                    .bind(&encoded_scope)
                    .fetch_optional(pool)
                    .await
                    .map_err(sql_error)?;
                row.map(|row| {
                    Ok((
                        row.try_get::<i64, _>("provider_account_generation")
                            .map_err(sql_error)?,
                        row.try_get::<Option<Vec<u8>>, _>("credential_revision")
                            .map_err(sql_error)?,
                        row.try_get::<i64, _>("discovery_epoch")
                            .map_err(sql_error)?,
                        row.try_get::<Vec<u8>, _>("discovery_token")
                            .map_err(sql_error)?,
                        row.try_get::<Option<String>, _>("snapshot_json")
                            .map_err(sql_error)?,
                    ))
                })
                .transpose()?
            }
        };
        let Some((stored_generation, stored_revision, stored_epoch, stored_token, stored_json)) =
            row
        else {
            return Ok(CapabilityStoreWrite::FenceLost);
        };
        let stored_revision = stored_revision
            .map(|value| utf8(value, "credential revision"))
            .transpose()?;
        let stored_token = utf8(stored_token, "discovery token")?;
        let same_authority = stored_generation == generation
            && stored_revision == expected_fence.credential_revision
            && stored_epoch == epoch
            && stored_token == expected_fence.discovery_token.as_str();
        if !same_authority {
            return Ok(CapabilityStoreWrite::FenceLost);
        }
        match stored_json {
            Some(stored) if stored == snapshot_json => Ok(CapabilityStoreWrite::Stored),
            Some(_) => Err(CoreError::Conflict(
                "capability discovery fence is already committed to a different snapshot"
                    .to_string(),
            )),
            None => Err(CoreError::Conflict(
                "capability snapshot changed during an authoritative write".to_string(),
            )),
        }
    }

    async fn invalidate_account_revision(
        &self,
        account_id: &edgion_center_core::CloudResourceId,
        stale_provider_account_generation: u64,
        stale_credential_revision: Option<&str>,
    ) -> CoreResult<()> {
        account_id.validate()?;
        let generation = checked_i64(
            stale_provider_account_generation,
            "stale provider account generation",
        )?;
        if generation <= 0 {
            return Err(CoreError::Conflict(
                "stale provider account generation must be positive".to_string(),
            ));
        }
        // Reuse fence validation for the revision bounds and structural rules.
        CapabilityDiscoveryFence {
            provider_account_generation: stale_provider_account_generation,
            credential_revision: stale_credential_revision.map(str::to_owned),
            discovery_epoch: 1,
            discovery_token: DiscoveryToken::new("validation-token")?,
        }
        .validate()?;
        let revocation_token = Uuid::new_v4().to_string();
        match &self.pool {
            Pool::Sqlite(pool) => {
                sqlx::query("UPDATE cloud_capability_snapshots SET discovery_epoch = CASE WHEN provider_account_generation = ? AND credential_revision IS ? THEN discovery_epoch + 1 ELSE discovery_epoch END, discovery_token = CASE WHEN provider_account_generation = ? AND credential_revision IS ? THEN ? ELSE discovery_token END, snapshot_json = CASE WHEN snapshot_provider_account_generation = ? AND snapshot_credential_revision IS ? THEN NULL ELSE snapshot_json END WHERE provider_account_id = ? AND ((provider_account_generation = ? AND credential_revision IS ?) OR (snapshot_provider_account_generation = ? AND snapshot_credential_revision IS ?))")
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .bind(revocation_token.as_bytes())
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .bind(account_id.as_str().as_bytes())
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .execute(pool)
                    .await
                    .map_err(sql_error)?;
            }
            Pool::Mysql(pool) => {
                sqlx::query("UPDATE cloud_capability_snapshots SET discovery_epoch = CASE WHEN provider_account_generation = ? AND credential_revision <=> ? THEN discovery_epoch + 1 ELSE discovery_epoch END, discovery_token = CASE WHEN provider_account_generation = ? AND credential_revision <=> ? THEN ? ELSE discovery_token END, snapshot_json = CASE WHEN snapshot_provider_account_generation = ? AND snapshot_credential_revision <=> ? THEN NULL ELSE snapshot_json END WHERE provider_account_id = ? AND ((provider_account_generation = ? AND credential_revision <=> ?) OR (snapshot_provider_account_generation = ? AND snapshot_credential_revision <=> ?))")
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .bind(revocation_token.as_bytes())
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .bind(account_id.as_str().as_bytes())
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .bind(generation)
                    .bind(optional_bytes(stale_credential_revision))
                    .execute(pool)
                    .await
                    .map_err(sql_error)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use edgion_center_core::cloud_test_support::{
        assert_exact_revision_invalidation, assert_roundtrip_and_fencing, assert_scope_isolation,
    };
    use edgion_center_core::{CapabilitySnapshotStore, CloudResourceId};

    use super::*;

    fn prefix(kind: &str) -> String {
        format!("sql-{kind}-{}", Uuid::new_v4())
    }

    async fn run_conformance(store: &Store, name: &str) {
        assert_roundtrip_and_fencing(store, &prefix(&format!("{name}-roundtrip"))).await;
        assert_scope_isolation(store, &prefix(&format!("{name}-scope"))).await;
        assert_exact_revision_invalidation(store, &prefix(&format!("{name}-invalidate"))).await;
    }

    #[tokio::test]
    async fn sqlite_capability_snapshot_conformance() {
        let store = Store::open_in_memory().await.unwrap();
        run_conformance(&store, "sqlite").await;
    }

    #[test]
    fn retired_snapshots_are_unavailable_but_malformed_snapshots_fail() {
        let key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new("legacy-account").unwrap(),
            scope: CapabilityScope::Account,
        };
        let scope_json = serde_json::to_string(&key.scope).unwrap();
        let retired = r#"{"observations":[{"capability":{"family":"cache","name":"purge"}}]}"#;

        assert_eq!(
            decode_snapshot(&key, scope_json.clone(), retired.to_string()).unwrap(),
            None
        );
        assert!(decode_snapshot(&key, scope_json, "not-json".to_string()).is_err());
    }

    #[tokio::test]
    async fn concurrent_begins_allocate_unique_epochs_and_only_latest_writes() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new(prefix("concurrent")).unwrap(),
            scope: CapabilityScope::Account,
        };
        let mut tasks = Vec::new();
        for _ in 0..24 {
            let store = store.clone();
            let key = key.clone();
            tasks.push(tokio::spawn(async move {
                store.begin_discovery(&key, 1, Some("same-revision")).await
            }));
        }
        let mut fences = Vec::new();
        for task in tasks {
            fences.push(task.await.unwrap().unwrap());
        }
        fences.sort_by_key(|fence| fence.discovery_epoch);
        assert_eq!(fences.first().unwrap().discovery_epoch, 1);
        assert_eq!(fences.last().unwrap().discovery_epoch, 24);
        for pair in fences.windows(2) {
            assert_eq!(pair[1].discovery_epoch, pair[0].discovery_epoch + 1);
            assert_ne!(pair[1].discovery_token, pair[0].discovery_token);
        }

        let winner = fences.pop().unwrap();
        for stale in fences {
            let value = conformance_snapshot(&key, stale.clone());
            assert_eq!(
                store.put_if_current(&key, &stale, &value).await.unwrap(),
                CapabilityStoreWrite::FenceLost
            );
        }
        let value = conformance_snapshot(&key, winner.clone());
        assert_eq!(
            store.put_if_current(&key, &winner, &value).await.unwrap(),
            CapabilityStoreWrite::Stored
        );
    }

    #[tokio::test]
    async fn sqlite_epoch_exhaustion_preserves_committed_snapshot() {
        let store = Store::open_in_memory().await.unwrap();
        let key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new(prefix("epoch-exhaustion")).unwrap(),
            scope: CapabilityScope::Account,
        };
        let fence = store.begin_discovery(&key, 7, None).await.unwrap();
        let committed = conformance_snapshot(&key, fence.clone());
        assert_eq!(
            store
                .put_if_current(&key, &fence, &committed)
                .await
                .unwrap(),
            CapabilityStoreWrite::Stored
        );
        let Pool::Sqlite(pool) = &store.pool else {
            unreachable!()
        };
        sqlx::query("UPDATE cloud_capability_snapshots SET discovery_epoch = 9223372036854775807 WHERE provider_account_id = ? AND scope_key = ?")
            .bind(key.provider_account_id.as_str().as_bytes())
            .bind(scope_key(&key.scope).unwrap())
            .execute(pool)
            .await
            .unwrap();

        assert!(store.begin_discovery(&key, 8, None).await.is_err());
        assert_eq!(store.get(&key).await.unwrap(), Some(committed.clone()));
        assert!(store
            .invalidate_account_revision(&key.provider_account_id, 7, None)
            .await
            .is_err());
        assert_eq!(store.get(&key).await.unwrap(), Some(committed));
    }

    fn conformance_snapshot(
        key: &CapabilitySnapshotKey,
        fence: CapabilityDiscoveryFence,
    ) -> ProviderCapabilitySnapshot {
        use edgion_center_core::{
            CapabilityDiscoveryIssue, CapabilityDiscoveryReport, CapabilityDiscoveryRequest,
            CapabilityDiscoveryState, CapabilityIssueScope, CapabilityIssueSeverity,
            CapabilityReason, CloudProvider, CredentialSource, ProviderAccountSpec,
            SanitizedCapabilityCode, SanitizedCapabilityMessage,
        };
        ProviderCapabilitySnapshot::from_report(
            &CapabilityDiscoveryRequest {
                provider_account_id: key.provider_account_id.clone(),
                fence,
                account: ProviderAccountSpec {
                    provider: CloudProvider::Cloudflare,
                    scope: None,
                    credential_source: CredentialSource::Ambient,
                },
                scope: key.scope.clone(),
            },
            1_000,
            CapabilityDiscoveryReport {
                state: CapabilityDiscoveryState::Failed,
                observations: Vec::new(),
                issues: vec![CapabilityDiscoveryIssue {
                    severity: CapabilityIssueSeverity::Blocking,
                    scope: CapabilityIssueScope::Account,
                    reason: CapabilityReason::ProbeFailed,
                    code: SanitizedCapabilityCode::new("sql_test_failure").unwrap(),
                    message: SanitizedCapabilityMessage::new("SQL test failure.").unwrap(),
                }],
            },
        )
        .unwrap()
    }

    #[tokio::test]
    async fn mysql_capability_snapshot_conformance() {
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
        run_conformance(&store, "mysql").await;
    }
}
