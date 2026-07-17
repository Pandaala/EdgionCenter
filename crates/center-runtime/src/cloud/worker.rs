use std::sync::Arc;

use async_trait::async_trait;
use edgion_center_core::{
    CloudOperation, CloudOperationStep, CloudOperationStepPurpose, CoreError, CoreResult,
    DispatchPolicy, OperationError, OperationErrorKind, OperationStore, StepCompletion,
};

#[async_trait]
pub trait CloudOperationExecutor: Send + Sync {
    /// Execute one idempotent step. The step idempotency key must be forwarded
    /// to providers that support request deduplication.
    async fn execute_step(
        &self,
        operation: &CloudOperation,
        step: &CloudOperationStep,
    ) -> CoreResult<StepCompletion>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerRun {
    Idle,
    Cancelled,
    StepCompleted,
}

pub struct ReconcileWorker {
    holder: String,
    lease_duration_ms: u64,
    dispatch_policy: DispatchPolicy,
    store: Arc<dyn OperationStore>,
    executor: Arc<dyn CloudOperationExecutor>,
}

impl ReconcileWorker {
    pub fn new(
        holder: impl Into<String>,
        lease_duration_ms: u64,
        max_execution_ms: u64,
        completion_margin_ms: u64,
        store: Arc<dyn OperationStore>,
        executor: Arc<dyn CloudOperationExecutor>,
    ) -> CoreResult<Self> {
        let holder = holder.into();
        let dispatch_policy = DispatchPolicy {
            max_execution_ms,
            completion_margin_ms,
        };
        dispatch_policy.validate()?;
        if holder.trim().is_empty()
            || max_execution_ms
                .checked_add(completion_margin_ms)
                .is_none_or(|required| required >= lease_duration_ms)
        {
            return Err(CoreError::Conflict(
                "cloud reconcile worker configuration is invalid".to_string(),
            ));
        }
        Ok(Self {
            holder,
            lease_duration_ms,
            dispatch_policy,
            store,
            executor,
        })
    }

    /// Claims and advances at most one durable step. A supervisor may call
    /// this repeatedly or schedule it from a bounded worker pool.
    pub async fn run_once(&self) -> CoreResult<WorkerRun> {
        let Some(claim) = self
            .store
            .claim_ready(&self.holder, self.lease_duration_ms)
            .await?
        else {
            return Ok(WorkerRun::Idle);
        };
        claim.validate()?;
        if claim.lease.holder != self.holder {
            return Err(CoreError::Conflict(
                "cloud operation store returned a lease for another holder".to_string(),
            ));
        }

        if claim.operation.cancel_requested {
            self.store.mark_cancelled(&claim.lease).await?;
            return Ok(WorkerRun::Cancelled);
        }

        let dispatched = self
            .store
            .begin_step(&claim.lease, self.dispatch_policy)
            .await?;
        dispatched.validate()?;
        if !claim.lease.same_fence(&dispatched.lease) {
            return Err(CoreError::Conflict(
                "cloud operation store returned a step under a different lease fence".to_string(),
            ));
        }
        if dispatched.dispatch_policy != self.dispatch_policy {
            return Err(CoreError::Conflict(
                "cloud operation store changed the requested dispatch policy".to_string(),
            ));
        }
        if dispatched.step.purpose == CloudOperationStepPurpose::Compensate {
            return Err(CoreError::Conflict(
                "compensation steps require an explicit activation state machine".to_string(),
            ));
        }

        let completion = match tokio::time::timeout(
            std::time::Duration::from_millis(dispatched.execution_budget_ms),
            self.executor
                .execute_step(&dispatched.operation, &dispatched.step),
        )
        .await
        {
            Ok(Ok(completion)) => completion,
            Ok(Err(_)) => StepCompletion::UnknownOutcome {
                error: OperationError {
                    kind: OperationErrorKind::UnknownOutcome,
                    code: "executor_error".to_string(),
                    message: "step executor failed without a normalized outcome".to_string(),
                    retry_after_ms: None,
                },
            },
            Err(_) => StepCompletion::UnknownOutcome {
                error: OperationError {
                    kind: OperationErrorKind::UnknownOutcome,
                    code: "step_timeout".to_string(),
                    message: "step execution exceeded its bounded lease window".to_string(),
                    retry_after_ms: None,
                },
            },
        };

        completion.validate()?;
        self.store.complete_step(&dispatched, completion).await?;
        Ok(WorkerRun::StepCompleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_center_core::{
        ClaimedOperation, CloudOperationAction, CloudOperationPhase, CloudOperationStepPhase,
        CloudOperationStepPurpose, CloudResourceId, CloudResourceKind, DispatchedStep,
        EnqueueOperationResult, IdempotencyKey, LeaseUpdate, NewCloudOperation, OperationId,
        OperationLease, UnknownOutcomeResolution,
    };
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Store {
        claim: Mutex<Option<ClaimedOperation>>,
        completion: Mutex<Option<StepCompletion>>,
        dispatched: Mutex<Option<DispatchedStep>>,
        dispatch_lease_override: Mutex<Option<OperationLease>>,
        dispatch_budget_override: Mutex<Option<u64>>,
        dispatch_policy_override: Mutex<Option<DispatchPolicy>>,
        cancelled: Mutex<bool>,
    }

    #[async_trait]
    impl OperationStore for Store {
        async fn enqueue(&self, _: NewCloudOperation) -> CoreResult<EnqueueOperationResult> {
            unreachable!()
        }

        async fn get_operation(&self, _: &OperationId) -> CoreResult<Option<CloudOperation>> {
            unreachable!()
        }

        async fn claim_ready(&self, _: &str, _: u64) -> CoreResult<Option<ClaimedOperation>> {
            Ok(self.claim.lock().take())
        }

        async fn renew_lease(&self, _: &OperationLease, _: u64) -> CoreResult<LeaseUpdate> {
            unreachable!()
        }

        async fn begin_step(
            &self,
            lease: &OperationLease,
            policy: DispatchPolicy,
        ) -> CoreResult<DispatchedStep> {
            let mut operation = operation(false);
            operation.phase = CloudOperationPhase::Running;
            operation.steps[0].phase = CloudOperationStepPhase::Running;
            operation.steps[0].attempt = 1;
            operation.steps[0].execution_token = Some("execution-1".to_string());
            let step = operation.steps[0].clone();
            let dispatched = DispatchedStep {
                operation,
                step,
                execution_token: "execution-1".to_string(),
                lease: self
                    .dispatch_lease_override
                    .lock()
                    .take()
                    .unwrap_or_else(|| lease.clone()),
                execution_budget_ms: self
                    .dispatch_budget_override
                    .lock()
                    .take()
                    .unwrap_or(policy.max_execution_ms),
                dispatch_policy: self
                    .dispatch_policy_override
                    .lock()
                    .take()
                    .unwrap_or(policy),
            };
            *self.dispatched.lock() = Some(dispatched.clone());
            Ok(dispatched)
        }

        async fn complete_step(
            &self,
            dispatched: &DispatchedStep,
            completion: StepCompletion,
        ) -> CoreResult<CloudOperation> {
            assert_eq!(dispatched.step.attempt, 1);
            assert_eq!(dispatched.execution_token, "execution-1");
            *self.completion.lock() = Some(completion);
            Ok(operation(false))
        }

        async fn mark_cancelled(&self, _: &OperationLease) -> CoreResult<CloudOperation> {
            *self.cancelled.lock() = true;
            Ok(operation(true))
        }

        async fn request_cancel(&self, _: &OperationId) -> CoreResult<CloudOperation> {
            unreachable!()
        }

        async fn resolve_unknown_outcome(
            &self,
            _: &OperationId,
            _: &str,
            _: u32,
            _: &str,
            _: UnknownOutcomeResolution,
        ) -> CoreResult<CloudOperation> {
            unreachable!()
        }
    }

    struct Executor;

    #[async_trait]
    impl CloudOperationExecutor for Executor {
        async fn execute_step(
            &self,
            _: &CloudOperation,
            step: &CloudOperationStep,
        ) -> CoreResult<StepCompletion> {
            Ok(StepCompletion::Succeeded {
                summary: Some(format!("{} applied", step.name)),
            })
        }
    }

    struct FailingExecutor;

    #[async_trait]
    impl CloudOperationExecutor for FailingExecutor {
        async fn execute_step(
            &self,
            _: &CloudOperation,
            _: &CloudOperationStep,
        ) -> CoreResult<StepCompletion> {
            Err(CoreError::Conflict(
                "provider response contained secret-token".to_string(),
            ))
        }
    }

    struct CountingExecutor(Arc<AtomicUsize>);

    #[async_trait]
    impl CloudOperationExecutor for CountingExecutor {
        async fn execute_step(
            &self,
            _: &CloudOperation,
            _: &CloudOperationStep,
        ) -> CoreResult<StepCompletion> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(StepCompletion::Succeeded { summary: None })
        }
    }

    struct InvalidOutcomeExecutor;

    #[async_trait]
    impl CloudOperationExecutor for InvalidOutcomeExecutor {
        async fn execute_step(
            &self,
            _: &CloudOperation,
            _: &CloudOperationStep,
        ) -> CoreResult<StepCompletion> {
            Ok(StepCompletion::RetryScheduled {
                error: OperationError {
                    kind: OperationErrorKind::Permanent,
                    code: "invalid".to_string(),
                    message: "invalid request".to_string(),
                    retry_after_ms: None,
                },
            })
        }
    }

    struct SlowExecutor;

    #[async_trait]
    impl CloudOperationExecutor for SlowExecutor {
        async fn execute_step(
            &self,
            _: &CloudOperation,
            _: &CloudOperationStep,
        ) -> CoreResult<StepCompletion> {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Ok(StepCompletion::Succeeded { summary: None })
        }
    }

    fn operation(cancel_requested: bool) -> CloudOperation {
        CloudOperation {
            id: OperationId::new("op-1").unwrap(),
            idempotency_key: IdempotencyKey::new("request-1").unwrap(),
            resource_id: CloudResourceId::new("zone-1").unwrap(),
            resource_kind: CloudResourceKind::ManagedZone,
            action: CloudOperationAction::Reconcile,
            desired_generation: 1,
            requested_by: "operator".to_string(),
            phase: if cancel_requested {
                CloudOperationPhase::CancelRequested
            } else {
                CloudOperationPhase::Pending
            },
            cancel_requested,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
            deadline_unix_ms: None,
            steps: vec![CloudOperationStep {
                name: "apply-zone".to_string(),
                purpose: CloudOperationStepPurpose::Apply,
                idempotency_key: IdempotencyKey::new("op-1/apply-zone").unwrap(),
                phase: CloudOperationStepPhase::Pending,
                attempt: 0,
                execution_token: None,
                next_attempt_at_unix_ms: None,
                started_at_unix_ms: None,
                finished_at_unix_ms: None,
                summary: None,
                error: None,
            }],
        }
    }

    fn claim(cancel_requested: bool) -> ClaimedOperation {
        ClaimedOperation {
            operation: operation(cancel_requested),
            lease: OperationLease {
                operation_id: OperationId::new("op-1").unwrap(),
                holder: "worker-1".to_string(),
                fencing_token: "token-1".to_string(),
                fencing_epoch: 1,
                valid_until_unix_ms: 1000,
            },
        }
    }

    fn store(cancel_requested: bool) -> Arc<Store> {
        Arc::new(Store {
            claim: Mutex::new(Some(claim(cancel_requested))),
            completion: Mutex::new(None),
            dispatched: Mutex::new(None),
            dispatch_lease_override: Mutex::new(None),
            dispatch_budget_override: Mutex::new(None),
            dispatch_policy_override: Mutex::new(None),
            cancelled: Mutex::new(false),
        })
    }

    #[tokio::test]
    async fn worker_claims_and_persists_one_step() {
        let store = store(false);
        let worker = ReconcileWorker::new(
            "worker-1",
            1000,
            500,
            100,
            store.clone(),
            Arc::new(Executor),
        )
        .unwrap();
        assert_eq!(worker.run_once().await.unwrap(), WorkerRun::StepCompleted);
        assert!(store.dispatched.lock().is_some());
        assert!(matches!(
            *store.completion.lock(),
            Some(StepCompletion::Succeeded { .. })
        ));
        assert_eq!(worker.run_once().await.unwrap(), WorkerRun::Idle);
    }

    #[tokio::test]
    async fn cancellation_is_persisted_without_dispatching_a_step() {
        let store = store(true);
        let worker = ReconcileWorker::new(
            "worker-1",
            1000,
            500,
            100,
            store.clone(),
            Arc::new(Executor),
        )
        .unwrap();
        assert_eq!(worker.run_once().await.unwrap(), WorkerRun::Cancelled);
        assert!(*store.cancelled.lock());
        assert!(store.dispatched.lock().is_none());
        assert!(store.completion.lock().is_none());
    }

    #[tokio::test]
    async fn raw_executor_errors_are_not_persisted() {
        let store = store(false);
        let worker = ReconcileWorker::new(
            "worker-1",
            1000,
            500,
            100,
            store.clone(),
            Arc::new(FailingExecutor),
        )
        .unwrap();
        assert_eq!(worker.run_once().await.unwrap(), WorkerRun::StepCompleted);
        let completion = store.completion.lock().clone().unwrap();
        let StepCompletion::UnknownOutcome { error } = completion else {
            panic!("executor errors must produce an unknown outcome");
        };
        assert_eq!(error.code, "executor_error");
        assert!(!error.message.contains("secret-token"));
    }

    #[tokio::test]
    async fn worker_uses_the_store_authoritative_execution_budget() {
        let store = store(false);
        *store.dispatch_budget_override.lock() = Some(1);
        let worker = ReconcileWorker::new(
            "worker-1",
            1000,
            500,
            100,
            store.clone(),
            Arc::new(SlowExecutor),
        )
        .unwrap();

        assert_eq!(worker.run_once().await.unwrap(), WorkerRun::StepCompleted);
        assert!(matches!(
            *store.completion.lock(),
            Some(StepCompletion::UnknownOutcome { ref error }) if error.code == "step_timeout"
        ));
    }

    #[test]
    fn worker_configuration_reserves_a_completion_margin() {
        let store = store(false);
        assert!(
            ReconcileWorker::new("worker-1", 600, 500, 100, store.clone(), Arc::new(Executor),)
                .is_err()
        );
        assert!(
            ReconcileWorker::new("worker-1", 601, 500, 100, store, Arc::new(Executor),).is_ok()
        );
    }

    #[tokio::test]
    async fn a_claim_for_another_holder_is_rejected_before_dispatch() {
        let store = store(false);
        let worker = ReconcileWorker::new(
            "worker-2",
            1000,
            500,
            100,
            store.clone(),
            Arc::new(Executor),
        )
        .unwrap();
        assert!(worker.run_once().await.is_err());
        assert!(store.dispatched.lock().is_none());
        assert!(store.completion.lock().is_none());
    }

    #[tokio::test]
    async fn a_step_dispatched_under_another_fence_is_rejected_before_execution() {
        let original = claim(false).lease;
        let mut wrong_holder = original.clone();
        wrong_holder.holder = "worker-2".to_string();
        let mut wrong_token = original.clone();
        wrong_token.fencing_token = "token-2".to_string();
        let mut wrong_epoch = original;
        wrong_epoch.fencing_epoch += 1;

        for mismatched_lease in [wrong_holder, wrong_token, wrong_epoch] {
            let store = store(false);
            *store.dispatch_lease_override.lock() = Some(mismatched_lease);
            let calls = Arc::new(AtomicUsize::new(0));
            let worker = ReconcileWorker::new(
                "worker-1",
                1000,
                500,
                100,
                store.clone(),
                Arc::new(CountingExecutor(calls.clone())),
            )
            .unwrap();

            assert!(worker.run_once().await.is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert!(store.completion.lock().is_none());
        }
    }

    #[tokio::test]
    async fn a_store_cannot_expand_the_requested_execution_policy() {
        let store = store(false);
        *store.dispatch_policy_override.lock() = Some(DispatchPolicy {
            max_execution_ms: 600,
            completion_margin_ms: 100,
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let worker = ReconcileWorker::new(
            "worker-1",
            1000,
            500,
            100,
            store.clone(),
            Arc::new(CountingExecutor(calls.clone())),
        )
        .unwrap();

        assert!(worker.run_once().await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(store.completion.lock().is_none());
    }

    #[tokio::test]
    async fn contradictory_executor_outcomes_are_rejected_before_persistence() {
        let store = store(false);
        let worker = ReconcileWorker::new(
            "worker-1",
            1000,
            500,
            100,
            store.clone(),
            Arc::new(InvalidOutcomeExecutor),
        )
        .unwrap();

        assert!(worker.run_once().await.is_err());
        assert!(store.completion.lock().is_none());
    }
}
