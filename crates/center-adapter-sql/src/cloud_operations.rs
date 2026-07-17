//! Durable cloud-operation storage for standalone SQLite and MySQL deployments.
//!
//! Phase one uses the adapter process wall clock as the authoritative clock for all SQL
//! timestamps, lease expiry, and retry scheduling. SQLite is a single-process deployment.
//! MySQL fencing remains safe under competing workers, but large host-clock skew can affect
//! lease availability; a database-time or bounded-skew clock strategy is a follow-on hardening.

use edgion_center_core::{
    ClaimedOperation, CloudOperation, CloudOperationPhase, CloudOperationStep,
    CloudOperationStepPhase, CoreError, CoreResult, DispatchPolicy, DispatchedStep,
    EnqueueOperationResult, IdempotencyKey, LeaseUpdate, NewCloudOperation, OperationError,
    OperationErrorKind, OperationId, OperationLease, OperationStore, StepCompletion,
    UnknownOutcomeResolution,
};
use sqlx::Row;
use uuid::Uuid;

use super::{core_adapter_error, Pool, Store};

const SELECT_COLUMNS: &str = "id, idempotency_key, resource_id, resource_kind, action, desired_generation, requested_by, phase, cancel_requested, created_at_unix_ms, updated_at_unix_ms, deadline_unix_ms, steps_json";
/// Phases that release a resource queue. Unknown outcome deliberately is not included: it is
/// terminal to ordinary execution but must block later mutations until explicitly resolved.
const RESOURCE_RELEASE_PHASES: &str = "'succeeded','failed','cancelled'";
const CLAIM_EXCLUDED_PHASES: &str = "'succeeded','failed','cancelled','unknown_outcome'";
const MAX_INDEXED_IDENTIFIER_LEN: usize = 512;

fn now_unix_ms() -> CoreResult<i64> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| CoreError::Adapter(error.to_string()))?
        .as_millis();
    i64::try_from(millis).map_err(|_| CoreError::Adapter("system clock overflow".to_string()))
}

fn checked_deadline(now: i64, duration_ms: u64) -> CoreResult<i64> {
    let duration = i64::try_from(duration_ms)
        .map_err(|_| CoreError::Conflict("lease duration exceeds SQL range".to_string()))?;
    if duration <= 0 {
        return Err(CoreError::Conflict(
            "lease duration must be positive".to_string(),
        ));
    }
    now.checked_add(duration)
        .ok_or_else(|| CoreError::Conflict("lease deadline overflow".to_string()))
}

fn enum_text<T: serde::Serialize>(value: &T) -> CoreResult<String> {
    serde_json::to_value(value)
        .map_err(|error| CoreError::Adapter(error.to_string()))?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| CoreError::Adapter("enum did not serialize as a string".to_string()))
}

fn parse_enum<T: serde::de::DeserializeOwned>(value: String) -> CoreResult<T> {
    serde_json::from_value(serde_json::Value::String(value))
        .map_err(|error| CoreError::Adapter(error.to_string()))
}

fn validate_indexed_identifier(kind: &str, value: &str) -> CoreResult<()> {
    if value.len() > MAX_INDEXED_IDENTIFIER_LEN {
        return Err(CoreError::Conflict(format!(
            "{kind} exceeds the SQL persistence limit of {MAX_INDEXED_IDENTIFIER_LEN} bytes"
        )));
    }
    Ok(())
}

fn retry_at(now: i64, error: &OperationError) -> CoreResult<i64> {
    let delay = i64::try_from(error.retry_after_ms.unwrap_or_default())
        .map_err(|_| CoreError::Conflict("retry delay exceeds SQL range".to_string()))?;
    now.checked_add(delay)
        .ok_or_else(|| CoreError::Conflict("retry deadline overflow".to_string()))
}

fn new_steps(operation: &NewCloudOperation) -> Vec<CloudOperationStep> {
    operation
        .steps
        .iter()
        .map(|step| CloudOperationStep {
            name: step.name.clone(),
            purpose: step.purpose,
            idempotency_key: step.idempotency_key.clone(),
            phase: CloudOperationStepPhase::Pending,
            attempt: 0,
            execution_token: None,
            next_attempt_at_unix_ms: None,
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
            summary: None,
            error: None,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn decode_operation(
    id: String,
    idempotency_key: String,
    resource_id: String,
    resource_kind: String,
    action: String,
    desired_generation: i64,
    requested_by: String,
    phase: String,
    cancel_requested: bool,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
    deadline_unix_ms: Option<i64>,
    steps_json: String,
) -> CoreResult<CloudOperation> {
    Ok(CloudOperation {
        id: OperationId::new(id)?,
        idempotency_key: IdempotencyKey::new(idempotency_key)?,
        resource_id: edgion_center_core::CloudResourceId::new(resource_id)?,
        resource_kind: parse_enum(resource_kind)?,
        action: parse_enum(action)?,
        desired_generation: u64::try_from(desired_generation)
            .map_err(|_| CoreError::Adapter("negative desired generation".to_string()))?,
        requested_by,
        phase: parse_enum(phase)?,
        cancel_requested,
        created_at_unix_ms,
        updated_at_unix_ms,
        deadline_unix_ms,
        steps: serde_json::from_str(&steps_json)
            .map_err(|error| CoreError::Adapter(error.to_string()))?,
    })
}

fn decode_sqlite(row: &sqlx::sqlite::SqliteRow) -> CoreResult<CloudOperation> {
    decode_operation(
        row.try_get("id").map_err(sql_error)?,
        row.try_get("idempotency_key").map_err(sql_error)?,
        row.try_get("resource_id").map_err(sql_error)?,
        row.try_get("resource_kind").map_err(sql_error)?,
        row.try_get("action").map_err(sql_error)?,
        row.try_get("desired_generation").map_err(sql_error)?,
        row.try_get("requested_by").map_err(sql_error)?,
        row.try_get("phase").map_err(sql_error)?,
        row.try_get::<i64, _>("cancel_requested")
            .map_err(sql_error)?
            != 0,
        row.try_get("created_at_unix_ms").map_err(sql_error)?,
        row.try_get("updated_at_unix_ms").map_err(sql_error)?,
        row.try_get("deadline_unix_ms").map_err(sql_error)?,
        row.try_get("steps_json").map_err(sql_error)?,
    )
}

fn decode_mysql(row: &sqlx::mysql::MySqlRow) -> CoreResult<CloudOperation> {
    decode_operation(
        row.try_get("id").map_err(sql_error)?,
        row.try_get("idempotency_key").map_err(sql_error)?,
        row.try_get("resource_id").map_err(sql_error)?,
        row.try_get("resource_kind").map_err(sql_error)?,
        row.try_get("action").map_err(sql_error)?,
        row.try_get("desired_generation").map_err(sql_error)?,
        row.try_get("requested_by").map_err(sql_error)?,
        row.try_get("phase").map_err(sql_error)?,
        row.try_get::<i8, _>("cancel_requested")
            .map_err(sql_error)?
            != 0,
        row.try_get("created_at_unix_ms").map_err(sql_error)?,
        row.try_get("updated_at_unix_ms").map_err(sql_error)?,
        row.try_get("deadline_unix_ms").map_err(sql_error)?,
        row.try_get("steps_json").map_err(sql_error)?,
    )
}

fn sql_error(error: sqlx::Error) -> CoreError {
    core_adapter_error(error.into())
}

impl Store {
    async fn operation_by_id(&self, id: &OperationId) -> CoreResult<Option<CloudOperation>> {
        let sql = format!("SELECT {SELECT_COLUMNS} FROM cloud_operations WHERE id = ?");
        match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(&sql)
                .bind(id.as_str())
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
                .as_ref()
                .map(decode_sqlite)
                .transpose(),
            Pool::Mysql(pool) => sqlx::query(&sql)
                .bind(id.as_str())
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
                .as_ref()
                .map(decode_mysql)
                .transpose(),
        }
    }

    async fn operation_by_request_key(
        &self,
        key: &IdempotencyKey,
    ) -> CoreResult<Option<(CloudOperation, String)>> {
        let sql = format!(
            "SELECT {SELECT_COLUMNS}, request_json FROM cloud_operations WHERE idempotency_key = ?"
        );
        match &self.pool {
            Pool::Sqlite(pool) => {
                let row = sqlx::query(&sql)
                    .bind(key.as_str())
                    .fetch_optional(pool)
                    .await
                    .map_err(sql_error)?;
                row.map(|row| {
                    Ok((
                        decode_sqlite(&row)?,
                        row.try_get("request_json").map_err(sql_error)?,
                    ))
                })
                .transpose()
            }
            Pool::Mysql(pool) => {
                let row = sqlx::query(&sql)
                    .bind(key.as_str())
                    .fetch_optional(pool)
                    .await
                    .map_err(sql_error)?;
                row.map(|row| {
                    Ok((
                        decode_mysql(&row)?,
                        row.try_get("request_json").map_err(sql_error)?,
                    ))
                })
                .transpose()
            }
        }
    }

    async fn lease_for_token(&self, token: &str) -> CoreResult<Option<ClaimedOperation>> {
        let sql = format!("SELECT {SELECT_COLUMNS}, lease_holder, lease_token, fencing_epoch, lease_valid_until_unix_ms FROM cloud_operations WHERE lease_token = ?");
        match &self.pool {
            Pool::Sqlite(pool) => {
                let row = sqlx::query(&sql)
                    .bind(token)
                    .fetch_optional(pool)
                    .await
                    .map_err(sql_error)?;
                row.map(|row| {
                    let operation = decode_sqlite(&row)?;
                    Ok(ClaimedOperation {
                        lease: OperationLease {
                            operation_id: operation.id.clone(),
                            holder: row.try_get("lease_holder").map_err(sql_error)?,
                            fencing_token: row.try_get("lease_token").map_err(sql_error)?,
                            fencing_epoch: u64::try_from(
                                row.try_get::<i64, _>("fencing_epoch").map_err(sql_error)?,
                            )
                            .map_err(|_| {
                                CoreError::Adapter("negative fencing epoch".to_string())
                            })?,
                            valid_until_unix_ms: row
                                .try_get("lease_valid_until_unix_ms")
                                .map_err(sql_error)?,
                        },
                        operation,
                    })
                })
                .transpose()
            }
            Pool::Mysql(pool) => {
                let row = sqlx::query(&sql)
                    .bind(token)
                    .fetch_optional(pool)
                    .await
                    .map_err(sql_error)?;
                row.map(|row| {
                    let operation = decode_mysql(&row)?;
                    Ok(ClaimedOperation {
                        lease: OperationLease {
                            operation_id: operation.id.clone(),
                            holder: row.try_get("lease_holder").map_err(sql_error)?,
                            fencing_token: row.try_get("lease_token").map_err(sql_error)?,
                            fencing_epoch: u64::try_from(
                                row.try_get::<i64, _>("fencing_epoch").map_err(sql_error)?,
                            )
                            .map_err(|_| {
                                CoreError::Adapter("negative fencing epoch".to_string())
                            })?,
                            valid_until_unix_ms: row
                                .try_get("lease_valid_until_unix_ms")
                                .map_err(sql_error)?,
                        },
                        operation,
                    })
                })
                .transpose()
            }
        }
    }
}

#[async_trait::async_trait]
impl OperationStore for Store {
    async fn enqueue(&self, request: NewCloudOperation) -> CoreResult<EnqueueOperationResult> {
        request.validate()?;
        validate_indexed_identifier("idempotency key", request.idempotency_key.as_str())?;
        validate_indexed_identifier("resource identifier", request.resource_id.as_str())?;
        let desired_generation = i64::try_from(request.desired_generation)
            .map_err(|_| CoreError::Conflict("desired generation exceeds SQL range".to_string()))?;
        let request_json = serde_json::to_string(&request)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        if let Some((existing, existing_request)) = self
            .operation_by_request_key(&request.idempotency_key)
            .await?
        {
            return if existing_request == request_json {
                Ok(EnqueueOperationResult::Existing(existing))
            } else {
                Err(CoreError::Conflict(
                    "idempotency key is already bound to a different request".to_string(),
                ))
            };
        }

        let now = now_unix_ms()?;
        let operation = CloudOperation {
            id: OperationId::new(Uuid::new_v4().to_string())?,
            idempotency_key: request.idempotency_key.clone(),
            resource_id: request.resource_id.clone(),
            resource_kind: request.resource_kind,
            action: request.action,
            desired_generation: request.desired_generation,
            requested_by: request.requested_by.clone(),
            phase: CloudOperationPhase::Pending,
            cancel_requested: false,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
            deadline_unix_ms: request.deadline_unix_ms,
            steps: new_steps(&request),
        };
        let steps_json = serde_json::to_string(&operation.steps)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        let sql = "INSERT INTO cloud_operations(id, idempotency_key, request_json, resource_id, resource_kind, action, desired_generation, requested_by, phase, cancel_requested, created_at_unix_ms, updated_at_unix_ms, deadline_unix_ms, steps_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)";
        let result: Result<(), sqlx::Error> = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(sql)
                .bind(operation.id.as_str())
                .bind(operation.idempotency_key.as_str())
                .bind(&request_json)
                .bind(operation.resource_id.as_str())
                .bind(enum_text(&operation.resource_kind)?)
                .bind(enum_text(&operation.action)?)
                .bind(desired_generation)
                .bind(&operation.requested_by)
                .bind(enum_text(&operation.phase)?)
                .bind(now)
                .bind(now)
                .bind(operation.deadline_unix_ms)
                .bind(&steps_json)
                .execute(pool)
                .await
                .map(|_| ()),
            Pool::Mysql(pool) => sqlx::query(sql)
                .bind(operation.id.as_str())
                .bind(operation.idempotency_key.as_str())
                .bind(&request_json)
                .bind(operation.resource_id.as_str())
                .bind(enum_text(&operation.resource_kind)?)
                .bind(enum_text(&operation.action)?)
                .bind(desired_generation)
                .bind(&operation.requested_by)
                .bind(enum_text(&operation.phase)?)
                .bind(now)
                .bind(now)
                .bind(operation.deadline_unix_ms)
                .bind(&steps_json)
                .execute(pool)
                .await
                .map(|_| ()),
        };
        match result {
            Ok(_) => Ok(EnqueueOperationResult::Created(operation)),
            Err(error) => {
                if error
                    .as_database_error()
                    .is_some_and(|database| database.is_unique_violation())
                {
                    let (existing, existing_request) = self
                        .operation_by_request_key(&request.idempotency_key)
                        .await?
                        .ok_or_else(|| {
                            CoreError::Conflict("operation identifier collision".to_string())
                        })?;
                    if existing_request == request_json {
                        Ok(EnqueueOperationResult::Existing(existing))
                    } else {
                        Err(CoreError::Conflict(
                            "idempotency key is already bound to a different request".to_string(),
                        ))
                    }
                } else {
                    Err(sql_error(error))
                }
            }
        }
    }

    async fn get_operation(
        &self,
        operation_id: &OperationId,
    ) -> CoreResult<Option<CloudOperation>> {
        self.operation_by_id(operation_id).await
    }

    async fn claim_ready(
        &self,
        holder: &str,
        lease_duration_ms: u64,
    ) -> CoreResult<Option<ClaimedOperation>> {
        if holder.trim().is_empty() {
            return Err(CoreError::Conflict(
                "lease holder must not be empty".to_string(),
            ));
        }
        validate_indexed_identifier("lease holder", holder)?;
        let now = now_unix_ms()?;
        let valid_until = checked_deadline(now, lease_duration_ms)?;
        expire_abandoned_steps(self, now).await?;
        expire_deadlines(self, now).await?;
        let token = Uuid::new_v4().to_string();
        let candidate = format!(
            "SELECT o.id FROM cloud_operations o WHERE o.phase NOT IN ({CLAIM_EXCLUDED_PHASES}) AND (o.deadline_unix_ms IS NULL OR o.deadline_unix_ms > ?) AND (o.lease_valid_until_unix_ms IS NULL OR o.lease_valid_until_unix_ms <= ?) AND (o.cancel_requested = 1 OR o.phase = 'pending' OR (o.phase = 'retry_scheduled' AND o.next_attempt_at_unix_ms <= ?) OR o.phase = 'running') AND NOT EXISTS (SELECT 1 FROM cloud_operations p WHERE p.resource_kind = o.resource_kind AND p.resource_id = o.resource_id AND p.phase NOT IN ({RESOURCE_RELEASE_PHASES}) AND p.queue_order < o.queue_order) ORDER BY o.queue_order LIMIT 1"
        );
        let affected = match &self.pool {
            Pool::Sqlite(pool) => {
                let sql = format!("UPDATE cloud_operations SET phase = CASE WHEN cancel_requested = 1 THEN 'cancel_requested' ELSE 'running' END, lease_holder = ?, lease_token = ?, fencing_epoch = fencing_epoch + 1, lease_valid_until_unix_ms = ?, updated_at_unix_ms = ? WHERE id = ({candidate}) AND fencing_epoch < 9223372036854775807");
                sqlx::query(&sql)
                    .bind(holder)
                    .bind(&token)
                    .bind(valid_until)
                    .bind(now)
                    .bind(now)
                    .bind(now)
                    .bind(now)
                    .execute(pool)
                    .await
                    .map_err(sql_error)?
                    .rows_affected()
            }
            Pool::Mysql(pool) => {
                let mut tx = pool.begin().await.map_err(sql_error)?;
                let lock_sql = format!("{candidate} FOR UPDATE");
                let id: Option<String> = sqlx::query_scalar(&lock_sql)
                    .bind(now)
                    .bind(now)
                    .bind(now)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(sql_error)?;
                let affected = if let Some(id) = id {
                    sqlx::query("UPDATE cloud_operations SET phase = CASE WHEN cancel_requested = 1 THEN 'cancel_requested' ELSE 'running' END, lease_holder = ?, lease_token = ?, fencing_epoch = fencing_epoch + 1, lease_valid_until_unix_ms = ?, updated_at_unix_ms = ? WHERE id = ? AND (deadline_unix_ms IS NULL OR deadline_unix_ms > ?) AND (lease_valid_until_unix_ms IS NULL OR lease_valid_until_unix_ms <= ?) AND fencing_epoch < 9223372036854775807")
                        .bind(holder).bind(&token).bind(valid_until).bind(now).bind(id).bind(now).bind(now).execute(&mut *tx).await.map_err(sql_error)?.rows_affected()
                } else {
                    0
                };
                tx.commit().await.map_err(sql_error)?;
                affected
            }
        };
        if affected == 0 {
            return Ok(None);
        }
        self.lease_for_token(&token).await
    }

    async fn renew_lease(
        &self,
        lease: &OperationLease,
        lease_duration_ms: u64,
    ) -> CoreResult<LeaseUpdate> {
        let now = now_unix_ms()?;
        let valid_until = checked_deadline(now, lease_duration_ms)?;
        let sql = "UPDATE cloud_operations SET lease_valid_until_unix_ms = ?, updated_at_unix_ms = ? WHERE id = ? AND lease_holder = ? AND lease_token = ? AND fencing_epoch = ? AND lease_valid_until_unix_ms > ?";
        let epoch = i64::try_from(lease.fencing_epoch)
            .map_err(|_| CoreError::Conflict("fencing epoch exceeds SQL range".to_string()))?;
        let affected = match &self.pool {
            Pool::Sqlite(pool) => sqlx::query(sql)
                .bind(valid_until)
                .bind(now)
                .bind(lease.operation_id.as_str())
                .bind(&lease.holder)
                .bind(&lease.fencing_token)
                .bind(epoch)
                .bind(now)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
            Pool::Mysql(pool) => sqlx::query(sql)
                .bind(valid_until)
                .bind(now)
                .bind(lease.operation_id.as_str())
                .bind(&lease.holder)
                .bind(&lease.fencing_token)
                .bind(epoch)
                .bind(now)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
        };
        if affected == 0 {
            Ok(LeaseUpdate::Lost)
        } else {
            Ok(LeaseUpdate::Renewed(OperationLease {
                valid_until_unix_ms: valid_until,
                ..lease.clone()
            }))
        }
    }

    async fn begin_step(
        &self,
        lease: &OperationLease,
        policy: DispatchPolicy,
    ) -> CoreResult<DispatchedStep> {
        policy.validate()?;
        let now = now_unix_ms()?;
        // A lease may have been claimed immediately before the deadline. Sweep again so an
        // undispatched operation is CAS-terminated and its lease is released.
        expire_deadlines(self, now).await?;
        let current_claim = self
            .lease_for_token(&lease.fencing_token)
            .await?
            .filter(|claim| claim.lease.same_fence(lease))
            .ok_or_else(|| CoreError::Conflict("cloud operation lease is stale".to_string()))?;
        let now = now_unix_ms()?;
        let remaining_ms = current_claim.lease.valid_until_unix_ms.saturating_sub(now);
        let margin_ms = i64::try_from(policy.completion_margin_ms).map_err(|_| {
            CoreError::Conflict("completion margin exceeds SQL time range".to_string())
        })?;
        if remaining_ms <= margin_ms {
            mutate_fenced(self, &current_claim.lease, now, true, |operation| {
                operation.phase = if operation.cancel_requested {
                    CloudOperationPhase::CancelRequested
                } else if operation
                    .steps
                    .iter()
                    .any(|step| step.phase == CloudOperationStepPhase::RetryScheduled)
                {
                    CloudOperationPhase::RetryScheduled
                } else {
                    CloudOperationPhase::Pending
                };
                Ok(None)
            })
            .await?;
            return Err(CoreError::Conflict(
                "cloud operation lease has no safe provider-execution budget".to_string(),
            ));
        }
        let available_ms = u64::try_from(remaining_ms - margin_ms)
            .map_err(|_| CoreError::Conflict("cloud operation lease is expired".to_string()))?;
        let execution_budget_ms = policy.max_execution_ms.min(available_ms);
        let current_lease = current_claim.lease;
        let (operation, execution_token) =
            mutate_fenced(self, &current_lease, now, false, |operation| {
                if operation.cancel_requested
                    || operation
                        .deadline_unix_ms
                        .is_some_and(|deadline| deadline <= now)
                {
                    return Err(CoreError::Conflict(
                        "operation is cancelled or deadline-expired".to_string(),
                    ));
                }
                let index = operation
                    .steps
                    .iter()
                    .position(|step| step.phase != CloudOperationStepPhase::Succeeded)
                    .ok_or_else(|| {
                        CoreError::Conflict("operation has no unfinished step".to_string())
                    })?;
                let step = &mut operation.steps[index];
                if step.purpose == edgion_center_core::CloudOperationStepPurpose::Compensate
                    || !matches!(
                        step.phase,
                        CloudOperationStepPhase::Pending | CloudOperationStepPhase::RetryScheduled
                    )
                    || step
                        .next_attempt_at_unix_ms
                        .is_some_and(|ready| ready > now)
                {
                    return Err(CoreError::Conflict(
                        "operation has no ready apply step".to_string(),
                    ));
                }
                step.attempt = step
                    .attempt
                    .checked_add(1)
                    .ok_or_else(|| CoreError::Conflict("step attempt overflow".to_string()))?;
                let token = Uuid::new_v4().to_string();
                step.phase = CloudOperationStepPhase::Running;
                step.execution_token = Some(token.clone());
                step.started_at_unix_ms = Some(now);
                step.finished_at_unix_ms = None;
                step.next_attempt_at_unix_ms = None;
                step.error = None;
                operation.phase = CloudOperationPhase::Running;
                Ok(Some(token))
            })
            .await?;
        let execution_token = execution_token.expect("begin mutation returns token");
        let step = operation
            .steps
            .iter()
            .find(|step| step.execution_token.as_deref() == Some(&execution_token))
            .expect("persisted dispatched step")
            .clone();
        Ok(DispatchedStep {
            operation,
            step,
            execution_token,
            lease: current_lease,
            execution_budget_ms,
            dispatch_policy: policy,
        })
    }

    async fn complete_step(
        &self,
        dispatched: &DispatchedStep,
        completion: StepCompletion,
    ) -> CoreResult<CloudOperation> {
        dispatched.validate()?;
        completion.validate()?;
        let now = now_unix_ms()?;
        let (operation, _) = mutate_fenced(self, &dispatched.lease, now, true, |operation| {
            let step = operation
                .steps
                .iter_mut()
                .find(|step| step.name == dispatched.step.name)
                .ok_or_else(|| {
                    CoreError::Conflict("dispatched step no longer exists".to_string())
                })?;
            if step.phase != CloudOperationStepPhase::Running
                || step.attempt != dispatched.step.attempt
                || step.execution_token.as_deref() != Some(dispatched.execution_token.as_str())
            {
                return Err(CoreError::Conflict(
                    "dispatched step fence is stale".to_string(),
                ));
            }
            step.finished_at_unix_ms = Some(now);
            match &completion {
                StepCompletion::Succeeded { summary } => {
                    step.phase = CloudOperationStepPhase::Succeeded;
                    step.summary = summary.clone();
                    step.error = None;
                }
                StepCompletion::RetryScheduled { error } => {
                    let ready = retry_at(now, error)?;
                    step.phase = CloudOperationStepPhase::RetryScheduled;
                    step.next_attempt_at_unix_ms = Some(ready);
                    step.error = Some(error.clone());
                }
                StepCompletion::Failed { error } => {
                    step.phase = CloudOperationStepPhase::Failed;
                    step.error = Some(error.clone());
                }
                StepCompletion::UnknownOutcome { error } => {
                    step.phase = CloudOperationStepPhase::UnknownOutcome;
                    step.error = Some(error.clone());
                }
            }
            operation.phase = if operation
                .steps
                .iter()
                .filter(|step| step.purpose == edgion_center_core::CloudOperationStepPurpose::Apply)
                .all(|step| step.phase == CloudOperationStepPhase::Succeeded)
            {
                CloudOperationPhase::Succeeded
            } else if operation
                .steps
                .iter()
                .any(|step| step.phase == CloudOperationStepPhase::UnknownOutcome)
            {
                CloudOperationPhase::UnknownOutcome
            } else if operation
                .steps
                .iter()
                .any(|step| step.phase == CloudOperationStepPhase::Failed)
            {
                CloudOperationPhase::Failed
            } else if operation.cancel_requested {
                CloudOperationPhase::CancelRequested
            } else if operation
                .steps
                .iter()
                .any(|step| step.phase == CloudOperationStepPhase::RetryScheduled)
            {
                CloudOperationPhase::RetryScheduled
            } else {
                CloudOperationPhase::Pending
            };
            Ok(None)
        })
        .await?;
        Ok(operation)
    }

    async fn mark_cancelled(&self, lease: &OperationLease) -> CoreResult<CloudOperation> {
        let now = now_unix_ms()?;
        let (operation, _) = mutate_fenced(self, lease, now, true, |operation| {
            if operation
                .steps
                .iter()
                .any(|step| step.phase == CloudOperationStepPhase::Running)
            {
                return Err(CoreError::Conflict(
                    "a running step must record or resolve its outcome before cancellation"
                        .to_string(),
                ));
            }
            for step in &mut operation.steps {
                if step.phase != CloudOperationStepPhase::Succeeded {
                    step.phase = CloudOperationStepPhase::Cancelled;
                    step.finished_at_unix_ms = Some(now);
                }
            }
            operation.cancel_requested = true;
            operation.phase = CloudOperationPhase::Cancelled;
            Ok(None)
        })
        .await?;
        Ok(operation)
    }

    async fn request_cancel(&self, operation_id: &OperationId) -> CoreResult<CloudOperation> {
        let now = now_unix_ms()?;
        for _ in 0..4 {
            let operation = self
                .operation_by_id(operation_id)
                .await?
                .ok_or_else(|| CoreError::NotFound(format!("cloud operation {operation_id}")))?;
            if operation.phase.is_terminal() || operation.cancel_requested {
                return Ok(operation);
            }
            let sql = "UPDATE cloud_operations SET cancel_requested = 1, phase = 'cancel_requested', updated_at_unix_ms = ? WHERE id = ? AND cancel_requested = 0 AND phase = ?";
            let current_phase = enum_text(&operation.phase)?;
            let affected = match &self.pool {
                Pool::Sqlite(pool) => sqlx::query(sql)
                    .bind(now)
                    .bind(operation_id.as_str())
                    .bind(&current_phase)
                    .execute(pool)
                    .await
                    .map_err(sql_error)?
                    .rows_affected(),
                Pool::Mysql(pool) => sqlx::query(sql)
                    .bind(now)
                    .bind(operation_id.as_str())
                    .bind(&current_phase)
                    .execute(pool)
                    .await
                    .map_err(sql_error)?
                    .rows_affected(),
            };
            if affected == 1 {
                return self
                    .operation_by_id(operation_id)
                    .await?
                    .ok_or_else(|| CoreError::NotFound(format!("cloud operation {operation_id}")));
            }
        }
        Err(CoreError::Conflict(
            "cloud operation changed while cancellation was requested".to_string(),
        ))
    }

    async fn resolve_unknown_outcome(
        &self,
        operation_id: &OperationId,
        step_name: &str,
        attempt: u32,
        execution_token: &str,
        resolution: UnknownOutcomeResolution,
    ) -> CoreResult<CloudOperation> {
        resolution.validate()?;
        let now = now_unix_ms()?;
        mutate_unleased(self, operation_id, now, |operation| {
            if operation.phase != CloudOperationPhase::UnknownOutcome {
                return Err(CoreError::Conflict(
                    "operation is not awaiting unknown-outcome resolution".to_string(),
                ));
            }
            let step = operation
                .steps
                .iter_mut()
                .find(|step| step.name == step_name)
                .ok_or_else(|| CoreError::NotFound(format!("cloud operation step {step_name}")))?;
            if step.phase != CloudOperationStepPhase::UnknownOutcome
                || step.attempt != attempt
                || step.execution_token.as_deref() != Some(execution_token)
            {
                return Err(CoreError::Conflict(
                    "unknown-outcome resolution fence is stale".to_string(),
                ));
            }
            match &resolution {
                UnknownOutcomeResolution::ConfirmedSucceeded { summary } => {
                    step.phase = CloudOperationStepPhase::Succeeded;
                    step.summary = summary.clone();
                    step.error = None;
                    step.finished_at_unix_ms = Some(now);
                }
                UnknownOutcomeResolution::ConfirmedNotApplied { error } => {
                    step.phase = CloudOperationStepPhase::RetryScheduled;
                    step.next_attempt_at_unix_ms = Some(retry_at(now, error)?);
                    step.error = Some(error.clone());
                }
                UnknownOutcomeResolution::ConfirmedFailed { error } => {
                    step.phase = CloudOperationStepPhase::Failed;
                    step.error = Some(error.clone());
                    step.finished_at_unix_ms = Some(now);
                }
            }
            operation.phase = if operation
                .steps
                .iter()
                .filter(|step| step.purpose == edgion_center_core::CloudOperationStepPurpose::Apply)
                .all(|step| step.phase == CloudOperationStepPhase::Succeeded)
            {
                CloudOperationPhase::Succeeded
            } else if operation
                .steps
                .iter()
                .any(|step| step.phase == CloudOperationStepPhase::Failed)
            {
                CloudOperationPhase::Failed
            } else if operation.cancel_requested {
                CloudOperationPhase::CancelRequested
            } else if operation
                .steps
                .iter()
                .any(|step| step.phase == CloudOperationStepPhase::RetryScheduled)
            {
                CloudOperationPhase::RetryScheduled
            } else {
                CloudOperationPhase::Pending
            };
            Ok(())
        })
        .await
    }
}

type MutationToken = Option<String>;

async fn mutate_fenced<F>(
    store: &Store,
    lease: &OperationLease,
    now: i64,
    release_lease: bool,
    mutate: F,
) -> CoreResult<(CloudOperation, MutationToken)>
where
    F: Fn(&mut CloudOperation) -> CoreResult<MutationToken>,
{
    let epoch = i64::try_from(lease.fencing_epoch)
        .map_err(|_| CoreError::Conflict("fencing epoch exceeds SQL range".to_string()))?;
    for _ in 0..4 {
        let mut operation = store
            .operation_by_id(&lease.operation_id)
            .await?
            .ok_or_else(|| {
                CoreError::NotFound(format!("cloud operation {}", lease.operation_id))
            })?;
        let previous_cancel_requested = operation.cancel_requested;
        let previous_steps = serde_json::to_string(&operation.steps)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        let token = mutate(&mut operation)?;
        operation.updated_at_unix_ms = now;
        let steps = serde_json::to_string(&operation.steps)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        let next_attempt = operation
            .steps
            .iter()
            .find_map(|step| step.next_attempt_at_unix_ms);
        let sql = if release_lease {
            "UPDATE cloud_operations SET phase = ?, cancel_requested = ?, updated_at_unix_ms = ?, next_attempt_at_unix_ms = ?, steps_json = ?, lease_holder = NULL, lease_token = NULL, lease_valid_until_unix_ms = NULL WHERE id = ? AND lease_holder = ? AND lease_token = ? AND fencing_epoch = ? AND lease_valid_until_unix_ms > ? AND cancel_requested = ? AND steps_json = ?"
        } else {
            "UPDATE cloud_operations SET phase = ?, cancel_requested = ?, updated_at_unix_ms = ?, next_attempt_at_unix_ms = ?, steps_json = ? WHERE id = ? AND lease_holder = ? AND lease_token = ? AND fencing_epoch = ? AND lease_valid_until_unix_ms > ? AND cancel_requested = ? AND steps_json = ?"
        };
        let affected = match &store.pool {
            Pool::Sqlite(pool) => sqlx::query(sql)
                .bind(enum_text(&operation.phase)?)
                .bind(operation.cancel_requested as i64)
                .bind(now)
                .bind(next_attempt)
                .bind(&steps)
                .bind(lease.operation_id.as_str())
                .bind(&lease.holder)
                .bind(&lease.fencing_token)
                .bind(epoch)
                .bind(now)
                .bind(previous_cancel_requested as i64)
                .bind(&previous_steps)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
            Pool::Mysql(pool) => sqlx::query(sql)
                .bind(enum_text(&operation.phase)?)
                .bind(operation.cancel_requested as i8)
                .bind(now)
                .bind(next_attempt)
                .bind(&steps)
                .bind(lease.operation_id.as_str())
                .bind(&lease.holder)
                .bind(&lease.fencing_token)
                .bind(epoch)
                .bind(now)
                .bind(previous_cancel_requested as i8)
                .bind(&previous_steps)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
        };
        if affected == 1 {
            return Ok((operation, token));
        }
    }
    Err(CoreError::Conflict(
        "cloud operation lease is stale, expired, or changed concurrently".to_string(),
    ))
}

async fn mutate_unleased<F>(
    store: &Store,
    id: &OperationId,
    now: i64,
    mutate: F,
) -> CoreResult<CloudOperation>
where
    F: Fn(&mut CloudOperation) -> CoreResult<()>,
{
    for _ in 0..4 {
        let mut operation = store
            .operation_by_id(id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("cloud operation {id}")))?;
        let previous_steps = serde_json::to_string(&operation.steps)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        mutate(&mut operation)?;
        operation.updated_at_unix_ms = now;
        let steps = serde_json::to_string(&operation.steps)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        let next_attempt = operation
            .steps
            .iter()
            .find_map(|step| step.next_attempt_at_unix_ms);
        let sql = "UPDATE cloud_operations SET phase = ?, updated_at_unix_ms = ?, next_attempt_at_unix_ms = ?, steps_json = ? WHERE id = ? AND steps_json = ? AND lease_token IS NULL";
        let affected = match &store.pool {
            Pool::Sqlite(pool) => sqlx::query(sql)
                .bind(enum_text(&operation.phase)?)
                .bind(now)
                .bind(next_attempt)
                .bind(&steps)
                .bind(id.as_str())
                .bind(&previous_steps)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
            Pool::Mysql(pool) => sqlx::query(sql)
                .bind(enum_text(&operation.phase)?)
                .bind(now)
                .bind(next_attempt)
                .bind(&steps)
                .bind(id.as_str())
                .bind(&previous_steps)
                .execute(pool)
                .await
                .map_err(sql_error)?
                .rows_affected(),
        };
        if affected == 1 {
            return Ok(operation);
        }
    }
    Err(CoreError::Conflict(
        "cloud operation changed during resolution".to_string(),
    ))
}

async fn expire_abandoned_steps(store: &Store, now: i64) -> CoreResult<()> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM cloud_operations WHERE phase IN ('running','cancel_requested') AND lease_valid_until_unix_ms <= ?");
    let operations = match &store.pool {
        Pool::Sqlite(pool) => sqlx::query(&sql)
            .bind(now)
            .fetch_all(pool)
            .await
            .map_err(sql_error)?
            .iter()
            .map(decode_sqlite)
            .collect::<CoreResult<Vec<_>>>()?,
        Pool::Mysql(pool) => sqlx::query(&sql)
            .bind(now)
            .fetch_all(pool)
            .await
            .map_err(sql_error)?
            .iter()
            .map(decode_mysql)
            .collect::<CoreResult<Vec<_>>>()?,
    };
    for mut operation in operations {
        let Some(step) = operation
            .steps
            .iter_mut()
            .find(|step| step.phase == CloudOperationStepPhase::Running)
        else {
            continue;
        };
        step.phase = CloudOperationStepPhase::UnknownOutcome;
        step.finished_at_unix_ms = Some(now);
        step.error = Some(OperationError {
            kind: OperationErrorKind::UnknownOutcome,
            code: "lease_expired_during_dispatch".to_string(),
            message: "step ownership expired before a durable outcome was recorded".to_string(),
            retry_after_ms: None,
        });
        operation.phase = CloudOperationPhase::UnknownOutcome;
        let steps = serde_json::to_string(&operation.steps)
            .map_err(|error| CoreError::Adapter(error.to_string()))?;
        let update = "UPDATE cloud_operations SET phase = 'unknown_outcome', updated_at_unix_ms = ?, steps_json = ?, lease_holder = NULL, lease_token = NULL, lease_valid_until_unix_ms = NULL WHERE id = ? AND phase IN ('running','cancel_requested') AND lease_valid_until_unix_ms <= ?";
        match &store.pool {
            Pool::Sqlite(pool) => {
                sqlx::query(update)
                    .bind(now)
                    .bind(&steps)
                    .bind(operation.id.as_str())
                    .bind(now)
                    .execute(pool)
                    .await
                    .map_err(sql_error)?;
            }
            Pool::Mysql(pool) => {
                sqlx::query(update)
                    .bind(now)
                    .bind(&steps)
                    .bind(operation.id.as_str())
                    .bind(now)
                    .execute(pool)
                    .await
                    .map_err(sql_error)?;
            }
        }
    }
    Ok(())
}

async fn expire_deadlines(store: &Store, now: i64) -> CoreResult<()> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM cloud_operations WHERE phase IN ('pending','running','retry_scheduled','cancel_requested') AND deadline_unix_ms IS NOT NULL AND deadline_unix_ms <= ?");
    let operations = match &store.pool {
        Pool::Sqlite(pool) => sqlx::query(&sql)
            .bind(now)
            .fetch_all(pool)
            .await
            .map_err(sql_error)?
            .iter()
            .map(decode_sqlite)
            .collect::<CoreResult<Vec<_>>>()?,
        Pool::Mysql(pool) => sqlx::query(&sql)
            .bind(now)
            .fetch_all(pool)
            .await
            .map_err(sql_error)?
            .iter()
            .map(decode_mysql)
            .collect::<CoreResult<Vec<_>>>()?,
    };
    for operation in operations {
        expire_deadline_snapshot(store, operation, now).await?;
    }
    Ok(())
}

async fn expire_deadline_snapshot(
    store: &Store,
    mut operation: CloudOperation,
    now: i64,
) -> CoreResult<bool> {
    // An already-dispatched provider mutation cannot be declared failed solely from a wall
    // clock deadline. Its exact outcome or lease-expiry unknown outcome must be recorded.
    if operation
        .steps
        .iter()
        .any(|step| step.phase == CloudOperationStepPhase::Running)
    {
        return Ok(false);
    }
    let previous_phase = enum_text(&operation.phase)?;
    let previous_steps = serde_json::to_string(&operation.steps)
        .map_err(|error| CoreError::Adapter(error.to_string()))?;
    if let Some(step) = operation
        .steps
        .iter_mut()
        .find(|step| step.phase != CloudOperationStepPhase::Succeeded)
    {
        step.phase = CloudOperationStepPhase::Failed;
        step.finished_at_unix_ms = Some(now);
        step.error = Some(OperationError {
            kind: OperationErrorKind::Permanent,
            code: "operation_deadline_exceeded".to_string(),
            message: "operation deadline elapsed before dispatch".to_string(),
            retry_after_ms: None,
        });
    }
    let steps = serde_json::to_string(&operation.steps)
        .map_err(|error| CoreError::Adapter(error.to_string()))?;
    let update = "UPDATE cloud_operations SET phase = 'failed', updated_at_unix_ms = ?, next_attempt_at_unix_ms = NULL, steps_json = ?, lease_holder = NULL, lease_token = NULL, lease_valid_until_unix_ms = NULL WHERE id = ? AND phase = ? AND steps_json = ? AND deadline_unix_ms <= ?";
    let affected = match &store.pool {
        Pool::Sqlite(pool) => sqlx::query(update)
            .bind(now)
            .bind(&steps)
            .bind(operation.id.as_str())
            .bind(&previous_phase)
            .bind(&previous_steps)
            .bind(now)
            .execute(pool)
            .await
            .map_err(sql_error)?
            .rows_affected(),
        Pool::Mysql(pool) => sqlx::query(update)
            .bind(now)
            .bind(&steps)
            .bind(operation.id.as_str())
            .bind(&previous_phase)
            .bind(&previous_steps)
            .bind(now)
            .execute(pool)
            .await
            .map_err(sql_error)?
            .rows_affected(),
    };
    Ok(affected == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_center_core::{
        CloudOperationStepPurpose, CloudResourceId, CloudResourceKind, NewCloudOperationStep,
    };

    fn request(key: &str, resource: &str, generation: u64) -> NewCloudOperation {
        NewCloudOperation {
            idempotency_key: IdempotencyKey::new(key).unwrap(),
            resource_id: CloudResourceId::new(resource).unwrap(),
            resource_kind: CloudResourceKind::ManagedZone,
            action: edgion_center_core::CloudOperationAction::Reconcile,
            desired_generation: generation,
            requested_by: "sql-test".to_string(),
            deadline_unix_ms: None,
            steps: vec![
                NewCloudOperationStep {
                    name: "observe".to_string(),
                    purpose: CloudOperationStepPurpose::Apply,
                    idempotency_key: IdempotencyKey::new(format!("{key}/observe")).unwrap(),
                },
                NewCloudOperationStep {
                    name: "apply".to_string(),
                    purpose: CloudOperationStepPurpose::Apply,
                    idempotency_key: IdempotencyKey::new(format!("{key}/apply")).unwrap(),
                },
            ],
        }
    }

    fn created(result: EnqueueOperationResult) -> CloudOperation {
        match result {
            EnqueueOperationResult::Created(operation) => operation,
            EnqueueOperationResult::Existing(_) => panic!("expected a newly created operation"),
        }
    }

    fn transient(code: &str, retry_after_ms: Option<u64>) -> OperationError {
        OperationError {
            kind: OperationErrorKind::Transient,
            code: code.to_string(),
            message: "retryable test failure".to_string(),
            retry_after_ms,
        }
    }

    fn dispatch_policy() -> DispatchPolicy {
        DispatchPolicy {
            max_execution_ms: 1_000,
            completion_margin_ms: 1,
        }
    }

    #[tokio::test]
    async fn enqueue_is_idempotent_only_for_the_same_payload() {
        let store = Store::open_in_memory().await.unwrap();
        let original_request = request("request-1", "zone-1", 1);
        let first = created(store.enqueue(original_request.clone()).await.unwrap());
        let second = store.enqueue(original_request).await.unwrap();
        let EnqueueOperationResult::Existing(second) = second else {
            panic!("same request must be deduplicated");
        };
        assert_eq!(first.id, second.id);

        let conflict = store
            .enqueue(request("request-1", "zone-1", 2))
            .await
            .unwrap_err();
        assert!(matches!(conflict, CoreError::Conflict(_)));
    }

    #[tokio::test]
    async fn same_resource_operations_are_serialized_across_all_steps() {
        let store = Store::open_in_memory().await.unwrap();
        let first = created(
            store
                .enqueue(request("request-a", "zone-shared", 1))
                .await
                .unwrap(),
        );
        let second = created(
            store
                .enqueue(request("request-b", "zone-shared", 2))
                .await
                .unwrap(),
        );

        let claim = store.claim_ready("worker-a", 5_000).await.unwrap().unwrap();
        assert_eq!(claim.operation.id, first.id);
        let dispatched = store
            .begin_step(&claim.lease, dispatch_policy())
            .await
            .unwrap();
        let after_first_step = store
            .complete_step(
                &dispatched,
                StepCompletion::Succeeded {
                    summary: Some("observed".to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(after_first_step.phase, CloudOperationPhase::Pending);

        let claim = store.claim_ready("worker-b", 5_000).await.unwrap().unwrap();
        assert_eq!(claim.operation.id, first.id);
        let dispatched = store
            .begin_step(&claim.lease, dispatch_policy())
            .await
            .unwrap();
        let completed = store
            .complete_step(&dispatched, StepCompletion::Succeeded { summary: None })
            .await
            .unwrap();
        assert_eq!(completed.phase, CloudOperationPhase::Succeeded);

        let claim = store.claim_ready("worker-c", 5_000).await.unwrap().unwrap();
        assert_eq!(claim.operation.id, second.id);
    }

    #[tokio::test]
    async fn queue_order_preserves_enqueue_order_when_timestamps_are_identical() {
        let store = Store::open_in_memory().await.unwrap();
        let mut expected = Vec::new();
        for generation in 1..=24 {
            let operation = created(
                store
                    .enqueue(request(
                        &format!("same-clock-{generation}"),
                        "zone-same-clock",
                        generation,
                    ))
                    .await
                    .unwrap(),
            );
            expected.push(operation.id);
        }
        let Pool::Sqlite(pool) = &store.pool else {
            unreachable!()
        };
        sqlx::query("UPDATE cloud_operations SET created_at_unix_ms = 100, updated_at_unix_ms = 100 WHERE resource_id = 'zone-same-clock'")
            .execute(pool)
            .await
            .unwrap();

        for expected_id in expected {
            let claim = store
                .claim_ready("same-clock-worker", 5_000)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(claim.operation.id, expected_id);
            store.request_cancel(&claim.operation.id).await.unwrap();
            store.mark_cancelled(&claim.lease).await.unwrap();
        }
    }

    #[tokio::test]
    async fn concurrent_enqueues_are_claimed_in_database_queue_order() {
        let store = Store::open_in_memory().await.unwrap();
        let mut tasks = Vec::new();
        for generation in 1..=32 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .enqueue(request(
                        &format!("concurrent-{generation}"),
                        "zone-concurrent",
                        generation,
                    ))
                    .await
                    .unwrap()
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }

        let Pool::Sqlite(pool) = &store.pool else {
            unreachable!()
        };
        let expected: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM cloud_operations WHERE resource_id = 'zone-concurrent' ORDER BY queue_order",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        assert_eq!(expected.len(), 32);

        for expected_id in expected {
            let claim = store
                .claim_ready("concurrent-worker", 5_000)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(claim.operation.id.as_str(), expected_id);
            store.request_cancel(&claim.operation.id).await.unwrap();
            store.mark_cancelled(&claim.lease).await.unwrap();
        }
    }

    #[tokio::test]
    async fn stale_lease_and_dispatch_fences_cannot_complete() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .enqueue(request("request-fence", "zone-fence", 1))
            .await
            .unwrap();
        let claim = store.claim_ready("worker-a", 5_000).await.unwrap().unwrap();
        let dispatched = store
            .begin_step(&claim.lease, dispatch_policy())
            .await
            .unwrap();

        let mut stale_lease = dispatched.clone();
        stale_lease.lease.fencing_token = "stale-token".to_string();
        assert!(store
            .complete_step(&stale_lease, StepCompletion::Succeeded { summary: None })
            .await
            .is_err());

        let mut stale_dispatch = dispatched.clone();
        stale_dispatch.execution_token = "stale-execution".to_string();
        assert!(store
            .complete_step(&stale_dispatch, StepCompletion::Succeeded { summary: None })
            .await
            .is_err());

        let completed = store
            .complete_step(&dispatched, StepCompletion::Succeeded { summary: None })
            .await
            .unwrap();
        assert_eq!(completed.steps[0].attempt, 1);
    }

    #[tokio::test]
    async fn insufficient_lease_budget_releases_claim_without_dispatch() {
        let store = Store::open_in_memory().await.unwrap();
        let operation = created(
            store
                .enqueue(request("request-budget", "zone-budget", 1))
                .await
                .unwrap(),
        );
        let claim = store
            .claim_ready("short-lease-worker", 20)
            .await
            .unwrap()
            .unwrap();
        let policy = DispatchPolicy {
            max_execution_ms: 100,
            completion_margin_ms: 20,
        };
        assert!(store.begin_step(&claim.lease, policy).await.is_err());
        let unchanged = store.get_operation(&operation.id).await.unwrap().unwrap();
        assert_eq!(unchanged.phase, CloudOperationPhase::Pending);
        assert_eq!(unchanged.steps[0].phase, CloudOperationStepPhase::Pending);
        assert_eq!(unchanged.steps[0].attempt, 0);
        assert_eq!(
            store.renew_lease(&claim.lease, 5_000).await.unwrap(),
            LeaseUpdate::Lost
        );

        let fresh = store
            .claim_ready("fresh-worker", 5_000)
            .await
            .unwrap()
            .unwrap();
        let dispatched = store
            .begin_step(
                &fresh.lease,
                DispatchPolicy {
                    max_execution_ms: 500,
                    completion_margin_ms: 100,
                },
            )
            .await
            .unwrap();
        assert!(dispatched.execution_budget_ms > 0);
        assert!(dispatched.execution_budget_ms <= 500);
        dispatched.validate().unwrap();
    }

    #[tokio::test]
    async fn expired_running_step_requires_resolution_before_replay() {
        let store = Store::open_in_memory().await.unwrap();
        let operation = created(
            store
                .enqueue(request("request-expire", "zone-expire", 1))
                .await
                .unwrap(),
        );
        let blocked = created(
            store
                .enqueue(request("request-after-unknown", "zone-expire", 2))
                .await
                .unwrap(),
        );
        let claim = store.claim_ready("worker-a", 10).await.unwrap().unwrap();
        let dispatched = store
            .begin_step(&claim.lease, dispatch_policy())
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        assert!(store
            .claim_ready("worker-b", 5_000)
            .await
            .unwrap()
            .is_none());
        let unknown = store.get_operation(&operation.id).await.unwrap().unwrap();
        assert_eq!(unknown.phase, CloudOperationPhase::UnknownOutcome);
        assert_eq!(
            unknown.steps[0].phase,
            CloudOperationStepPhase::UnknownOutcome
        );

        let resolved = store
            .resolve_unknown_outcome(
                &operation.id,
                &dispatched.step.name,
                dispatched.step.attempt,
                &dispatched.execution_token,
                UnknownOutcomeResolution::ConfirmedNotApplied {
                    error: transient("not-applied", Some(0)),
                },
            )
            .await
            .unwrap();
        assert_eq!(resolved.phase, CloudOperationPhase::RetryScheduled);
        let reclaimed = store.claim_ready("worker-b", 5_000).await.unwrap().unwrap();
        assert_eq!(reclaimed.operation.id, operation.id);
        assert_ne!(reclaimed.operation.id, blocked.id);
        let redispatched = store
            .begin_step(&reclaimed.lease, dispatch_policy())
            .await
            .unwrap();
        assert_eq!(redispatched.step.attempt, 2);
        assert_ne!(redispatched.execution_token, dispatched.execution_token);
    }

    #[tokio::test]
    async fn deadline_sweep_terminates_work_without_overwriting_success() {
        let store = Store::open_in_memory().await.unwrap();
        let mut expired_request = request("request-deadline", "zone-deadline", 1);
        expired_request.deadline_unix_ms = Some(now_unix_ms().unwrap() + 15);
        let expired = created(store.enqueue(expired_request).await.unwrap());
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert!(store
            .claim_ready("deadline-worker", 5_000)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_operation(&expired.id)
                .await
                .unwrap()
                .unwrap()
                .phase,
            CloudOperationPhase::Failed
        );

        let mut successful_request = request("request-before-deadline", "zone-success", 1);
        successful_request.deadline_unix_ms = Some(now_unix_ms().unwrap() + 80);
        let successful = created(store.enqueue(successful_request).await.unwrap());
        for _ in 0..2 {
            let claim = store
                .claim_ready("success-worker", 5_000)
                .await
                .unwrap()
                .unwrap();
            let dispatched = store
                .begin_step(&claim.lease, dispatch_policy())
                .await
                .unwrap();
            store
                .complete_step(&dispatched, StepCompletion::Succeeded { summary: None })
                .await
                .unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(90)).await;
        assert!(store
            .claim_ready("sweep-worker", 5_000)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_operation(&successful.id)
                .await
                .unwrap()
                .unwrap()
                .phase,
            CloudOperationPhase::Succeeded
        );
    }

    #[tokio::test]
    async fn deadline_crossed_after_claim_releases_undispatched_lease() {
        let store = Store::open_in_memory().await.unwrap();
        let mut request = request("request-claimed-deadline", "zone-claimed-deadline", 1);
        request.deadline_unix_ms = Some(now_unix_ms().unwrap() + 20);
        let operation = created(store.enqueue(request).await.unwrap());
        let claim = store
            .claim_ready("deadline-owner", 5_000)
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(store
            .begin_step(&claim.lease, dispatch_policy())
            .await
            .is_err());
        assert_eq!(
            store
                .get_operation(&operation.id)
                .await
                .unwrap()
                .unwrap()
                .phase,
            CloudOperationPhase::Failed
        );
        assert_eq!(
            store.renew_lease(&claim.lease, 5_000).await.unwrap(),
            LeaseUpdate::Lost
        );
    }

    #[tokio::test]
    async fn stale_deadline_snapshot_cannot_overwrite_a_concurrent_transition() {
        let store = Store::open_in_memory().await.unwrap();
        let mut request = request("request-deadline-cas", "zone-deadline-cas", 1);
        request.deadline_unix_ms = Some(now_unix_ms().unwrap() - 1);
        let operation = created(store.enqueue(request).await.unwrap());
        let stale_snapshot = store.get_operation(&operation.id).await.unwrap().unwrap();

        let cancelled = store.request_cancel(&operation.id).await.unwrap();
        assert_eq!(cancelled.phase, CloudOperationPhase::CancelRequested);
        assert!(
            !expire_deadline_snapshot(&store, stale_snapshot, now_unix_ms().unwrap())
                .await
                .unwrap()
        );
        let current = store.get_operation(&operation.id).await.unwrap().unwrap();
        assert_eq!(current.phase, CloudOperationPhase::CancelRequested);
        assert!(current.cancel_requested);
    }

    #[tokio::test]
    async fn cancellation_is_idempotent_and_fenced() {
        let store = Store::open_in_memory().await.unwrap();
        let operation = created(
            store
                .enqueue(request("request-cancel", "zone-cancel", 1))
                .await
                .unwrap(),
        );
        let requested = store.request_cancel(&operation.id).await.unwrap();
        assert!(requested.cancel_requested);
        assert_eq!(requested.phase, CloudOperationPhase::CancelRequested);
        let requested_again = store.request_cancel(&operation.id).await.unwrap();
        assert_eq!(requested_again.phase, CloudOperationPhase::CancelRequested);

        let claim = store
            .claim_ready("cancel-worker", 5_000)
            .await
            .unwrap()
            .unwrap();
        let cancelled = store.mark_cancelled(&claim.lease).await.unwrap();
        assert_eq!(cancelled.phase, CloudOperationPhase::Cancelled);
        assert!(cancelled
            .steps
            .iter()
            .all(|step| step.phase == CloudOperationStepPhase::Cancelled));
        assert_eq!(
            store.request_cancel(&operation.id).await.unwrap().phase,
            CloudOperationPhase::Cancelled
        );
    }

    #[tokio::test]
    async fn cancelling_an_expired_running_step_preserves_unknown_outcome() {
        let store = Store::open_in_memory().await.unwrap();
        let operation = created(
            store
                .enqueue(request("request-cancel-running", "zone-cancel-running", 1))
                .await
                .unwrap(),
        );
        let claim = store.claim_ready("worker-a", 10).await.unwrap().unwrap();
        let dispatched = store
            .begin_step(&claim.lease, dispatch_policy())
            .await
            .unwrap();
        store.request_cancel(&operation.id).await.unwrap();
        assert!(store.mark_cancelled(&claim.lease).await.is_err());
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        assert!(store
            .claim_ready("worker-b", 5_000)
            .await
            .unwrap()
            .is_none());
        let unknown = store.get_operation(&operation.id).await.unwrap().unwrap();
        assert!(unknown.cancel_requested);
        assert_eq!(unknown.phase, CloudOperationPhase::UnknownOutcome);

        let resolved = store
            .resolve_unknown_outcome(
                &operation.id,
                &dispatched.step.name,
                dispatched.step.attempt,
                &dispatched.execution_token,
                UnknownOutcomeResolution::ConfirmedNotApplied {
                    error: transient("cancelled-not-applied", Some(0)),
                },
            )
            .await
            .unwrap();
        assert_eq!(resolved.phase, CloudOperationPhase::CancelRequested);
        let claim = store.claim_ready("worker-b", 5_000).await.unwrap().unwrap();
        assert_eq!(
            store.mark_cancelled(&claim.lease).await.unwrap().phase,
            CloudOperationPhase::Cancelled
        );
    }
}
