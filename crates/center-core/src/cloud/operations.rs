//! Durable, fenced cloud-operation contracts.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{CloudResourceId, CloudResourceKind, CoreError, CoreResult};

const MAX_OPERATION_ID_LEN: usize = 128;
const MAX_IDEMPOTENCY_KEY_LEN: usize = 512;
const MAX_STEP_NAME_LEN: usize = 128;
const MAX_REQUESTED_BY_LEN: usize = 512;
const MAX_OPERATION_STEPS: usize = 128;
const MAX_ERROR_CODE_LEN: usize = 128;
const MAX_ERROR_MESSAGE_LEN: usize = 4096;
const MAX_STEP_SUMMARY_LEN: usize = 4096;

macro_rules! identifier {
    ($name:ident, $kind:literal, $max_len:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> CoreResult<Self> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > $max_len
                    || value.trim() != value
                    || value.chars().any(char::is_control)
                {
                    return Err(CoreError::InvalidIdentifier { kind: $kind, value });
                }
                Ok(Self(value))
            }

            pub fn validate(&self) -> CoreResult<()> {
                Self::new(self.0.clone()).map(|_| ())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

identifier!(OperationId, "cloud operation", MAX_OPERATION_ID_LEN);
identifier!(IdempotencyKey, "idempotency key", MAX_IDEMPOTENCY_KEY_LEN);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudOperationAction {
    Reconcile,
    Delete,
    Import,
    Inspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudOperationPhase {
    Pending,
    Running,
    RetryScheduled,
    CancelRequested,
    Succeeded,
    Failed,
    Cancelled,
    UnknownOutcome,
}

impl CloudOperationPhase {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::UnknownOutcome
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudOperationStepPhase {
    Pending,
    Running,
    RetryScheduled,
    Succeeded,
    Failed,
    Cancelled,
    UnknownOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudOperationStepPurpose {
    Apply,
    Compensate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOperationStep {
    pub name: String,
    pub purpose: CloudOperationStepPurpose,
    pub idempotency_key: IdempotencyKey,
    pub phase: CloudOperationStepPhase,
    pub attempt: u32,
    /// Opaque token for the current execution attempt. It is assigned by the store when the
    /// step enters `Running` and retained for unknown-outcome resolution.
    pub execution_token: Option<String>,
    pub next_attempt_at_unix_ms: Option<i64>,
    pub started_at_unix_ms: Option<i64>,
    pub finished_at_unix_ms: Option<i64>,
    pub summary: Option<String>,
    pub error: Option<OperationError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationErrorKind {
    Transient,
    Throttled,
    ConflictRequiresReplan,
    Permanent,
    UnknownOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    pub kind: OperationErrorKind,
    pub code: String,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

impl OperationError {
    pub fn validate(&self) -> CoreResult<()> {
        validate_bounded_text(&self.code, "cloud operation error code", MAX_ERROR_CODE_LEN)?;
        validate_bounded_text(
            &self.message,
            "cloud operation error message",
            MAX_ERROR_MESSAGE_LEN,
        )?;
        if self
            .retry_after_ms
            .is_some_and(|delay| delay > i64::MAX as u64)
        {
            return Err(CoreError::Conflict(
                "cloud operation retry delay exceeds the persisted time range".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCloudOperation {
    pub idempotency_key: IdempotencyKey,
    pub resource_id: CloudResourceId,
    pub resource_kind: CloudResourceKind,
    pub action: CloudOperationAction,
    pub desired_generation: u64,
    pub requested_by: String,
    pub deadline_unix_ms: Option<i64>,
    pub steps: Vec<NewCloudOperationStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCloudOperationStep {
    pub name: String,
    pub purpose: CloudOperationStepPurpose,
    pub idempotency_key: IdempotencyKey,
}

impl NewCloudOperation {
    pub fn validate(&self) -> CoreResult<()> {
        self.idempotency_key.validate()?;
        self.resource_id.validate()?;
        if self.desired_generation == 0
            || self.desired_generation > i64::MAX as u64
            || self.steps.is_empty()
            || self.steps.len() > MAX_OPERATION_STEPS
            || self.deadline_unix_ms.is_some_and(|deadline| deadline <= 0)
        {
            return Err(CoreError::Conflict(
                "cloud operation request is invalid".to_string(),
            ));
        }
        validate_bounded_text(
            &self.requested_by,
            "cloud operation requester",
            MAX_REQUESTED_BY_LEN,
        )?;
        for step in &self.steps {
            validate_bounded_text(&step.name, "cloud operation step name", MAX_STEP_NAME_LEN)?;
            step.idempotency_key.validate()?;
        }
        let mut names = std::collections::BTreeSet::new();
        if self.steps.iter().any(|step| !names.insert(&step.name)) {
            return Err(CoreError::Conflict(
                "cloud operation step names must be unique".to_string(),
            ));
        }
        let mut idempotency_keys = std::collections::BTreeSet::new();
        if self
            .steps
            .iter()
            .any(|step| !idempotency_keys.insert(&step.idempotency_key))
        {
            return Err(CoreError::Conflict(
                "cloud operation step idempotency keys must be unique".to_string(),
            ));
        }
        // Compensation needs an explicit activation and reverse-order state machine. Reject it
        // until that contract exists instead of accepting operations that can never finish.
        if self
            .steps
            .iter()
            .any(|step| step.purpose == CloudOperationStepPurpose::Compensate)
        {
            return Err(CoreError::Conflict(
                "cloud operation compensation steps are not supported yet".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOperation {
    pub id: OperationId,
    pub idempotency_key: IdempotencyKey,
    pub resource_id: CloudResourceId,
    pub resource_kind: CloudResourceKind,
    pub action: CloudOperationAction,
    pub desired_generation: u64,
    pub requested_by: String,
    pub phase: CloudOperationPhase,
    pub cancel_requested: bool,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
    pub deadline_unix_ms: Option<i64>,
    pub steps: Vec<CloudOperationStep>,
}

impl CloudOperation {
    pub fn next_ready_step(&self, now_unix_ms: i64) -> Option<&CloudOperationStep> {
        if self.cancel_requested
            || !matches!(
                self.phase,
                CloudOperationPhase::Pending
                    | CloudOperationPhase::Running
                    | CloudOperationPhase::RetryScheduled
            )
        {
            return None;
        }
        let step = self
            .steps
            .iter()
            .find(|step| step.phase != CloudOperationStepPhase::Succeeded)?;
        // Compensation execution needs a separate activation and reverse-order state machine.
        // Keeping it dormant is safer than executing compensation after a successful apply.
        if step.purpose == CloudOperationStepPurpose::Compensate {
            return None;
        }
        match step.phase {
            CloudOperationStepPhase::Pending => true,
            CloudOperationStepPhase::RetryScheduled => step
                .next_attempt_at_unix_ms
                .is_some_and(|next| next <= now_unix_ms),
            _ => false,
        }
        .then_some(step)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueOperationResult {
    Created(CloudOperation),
    Existing(CloudOperation),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLease {
    pub operation_id: OperationId,
    pub holder: String,
    pub fencing_token: String,
    pub fencing_epoch: u64,
    pub valid_until_unix_ms: i64,
}

impl OperationLease {
    /// Compares stable ownership identity. Lease renewal changes `valid_until_unix_ms` without
    /// invalidating an already dispatched step from the same fenced ownership epoch.
    pub fn same_fence(&self, other: &Self) -> bool {
        self.operation_id == other.operation_id
            && self.holder == other.holder
            && self.fencing_token == other.fencing_token
            && self.fencing_epoch == other.fencing_epoch
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedOperation {
    pub operation: CloudOperation,
    pub lease: OperationLease,
}

/// Store-authoritative limits for dispatching one provider mutation.
///
/// The store converts these limits into a relative execution budget using its own clock. The
/// worker must not derive that budget from the lease's wall-clock timestamp because SQL and
/// Kubernetes adapters can use different lease time domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchPolicy {
    pub max_execution_ms: u64,
    pub completion_margin_ms: u64,
}

impl DispatchPolicy {
    pub fn validate(&self) -> CoreResult<()> {
        if self.max_execution_ms == 0 || self.completion_margin_ms == 0 {
            return Err(CoreError::Conflict(
                "cloud operation dispatch policy is invalid".to_string(),
            ));
        }
        if self
            .max_execution_ms
            .checked_add(self.completion_margin_ms)
            .is_none_or(|total| total > i64::MAX as u64)
        {
            return Err(CoreError::Conflict(
                "cloud operation dispatch policy is invalid".to_string(),
            ));
        }
        Ok(())
    }
}

impl ClaimedOperation {
    pub fn validate(&self) -> CoreResult<()> {
        if self.operation.id != self.lease.operation_id
            || self.lease.holder.trim().is_empty()
            || self.lease.fencing_token.trim().is_empty()
            || self.lease.fencing_epoch == 0
        {
            return Err(CoreError::Conflict(
                "cloud operation claim is internally inconsistent".to_string(),
            ));
        }
        Ok(())
    }
}

/// A step atomically moved to `Running` under an exact operation lease.
///
/// The attempt and execution token form a per-dispatch fence in addition to the operation
/// lease. Stores must reject completion from any older dispatch, including one made by the
/// same holder under the same lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchedStep {
    pub operation: CloudOperation,
    pub step: CloudOperationStep,
    pub execution_token: String,
    pub lease: OperationLease,
    /// Relative provider-execution window calculated by the store in its authoritative time
    /// domain. It excludes the time reserved for persisting the completion.
    pub execution_budget_ms: u64,
    pub dispatch_policy: DispatchPolicy,
}

impl DispatchedStep {
    pub fn validate(&self) -> CoreResult<()> {
        self.dispatch_policy.validate()?;
        self.operation.validate_claim_identity(&self.lease)?;
        let persisted = self
            .operation
            .steps
            .iter()
            .find(|step| step.name == self.step.name)
            .ok_or_else(|| {
                CoreError::Conflict("dispatched step is absent from its operation".to_string())
            })?;
        if self.operation.phase != CloudOperationPhase::Running
            || self.step.phase != CloudOperationStepPhase::Running
            || self.step.purpose == CloudOperationStepPurpose::Compensate
            || self.step.attempt == 0
            || self.lease.holder.trim().is_empty()
            || self.lease.fencing_token.trim().is_empty()
            || self.lease.fencing_epoch == 0
            || self.execution_token.trim().is_empty()
            || self.execution_budget_ms == 0
            || self.execution_budget_ms > self.dispatch_policy.max_execution_ms
            || self.step.execution_token.as_deref() != Some(self.execution_token.as_str())
            || persisted != &self.step
        {
            return Err(CoreError::Conflict(
                "dispatched cloud operation step is internally inconsistent".to_string(),
            ));
        }
        Ok(())
    }
}

impl CloudOperation {
    fn validate_claim_identity(&self, lease: &OperationLease) -> CoreResult<()> {
        if self.id != lease.operation_id {
            return Err(CoreError::Conflict(
                "cloud operation and lease identifiers do not match".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseUpdate {
    Renewed(OperationLease),
    Lost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepCompletion {
    Succeeded { summary: Option<String> },
    RetryScheduled { error: OperationError },
    Failed { error: OperationError },
    UnknownOutcome { error: OperationError },
}

impl StepCompletion {
    pub fn validate(&self) -> CoreResult<()> {
        match self {
            Self::Succeeded { summary } => validate_optional_summary(summary)?,
            Self::RetryScheduled { error }
            | Self::Failed { error }
            | Self::UnknownOutcome { error } => error.validate()?,
        }
        let valid = match self {
            Self::Succeeded { .. } => true,
            Self::RetryScheduled { error } => matches!(
                error.kind,
                OperationErrorKind::Transient
                    | OperationErrorKind::Throttled
                    | OperationErrorKind::ConflictRequiresReplan
            ),
            Self::Failed { error } => matches!(
                error.kind,
                OperationErrorKind::Permanent | OperationErrorKind::ConflictRequiresReplan
            ),
            Self::UnknownOutcome { error } => error.kind == OperationErrorKind::UnknownOutcome,
        };
        if !valid {
            return Err(CoreError::Conflict(
                "cloud operation completion and error kind are inconsistent".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownOutcomeResolution {
    ConfirmedSucceeded {
        summary: Option<String>,
    },
    /// Observation proved that the mutation was not applied. The store schedules the retry
    /// from its authoritative clock and the normalized error's retry hint.
    ConfirmedNotApplied {
        error: OperationError,
    },
    ConfirmedFailed {
        error: OperationError,
    },
}

impl UnknownOutcomeResolution {
    pub fn validate(&self) -> CoreResult<()> {
        match self {
            Self::ConfirmedSucceeded { summary } => validate_optional_summary(summary)?,
            Self::ConfirmedNotApplied { error } | Self::ConfirmedFailed { error } => {
                error.validate()?
            }
        }
        let valid = match self {
            Self::ConfirmedSucceeded { .. } => true,
            Self::ConfirmedNotApplied { error } => matches!(
                error.kind,
                OperationErrorKind::Transient
                    | OperationErrorKind::Throttled
                    | OperationErrorKind::ConflictRequiresReplan
            ),
            Self::ConfirmedFailed { error } => matches!(
                error.kind,
                OperationErrorKind::Permanent | OperationErrorKind::ConflictRequiresReplan
            ),
        };
        if !valid {
            return Err(CoreError::Conflict(
                "cloud unknown-outcome resolution and error kind are inconsistent".to_string(),
            ));
        }
        Ok(())
    }
}

fn validate_optional_summary(summary: &Option<String>) -> CoreResult<()> {
    if let Some(summary) = summary {
        validate_bounded_text(
            summary,
            "cloud operation step summary",
            MAX_STEP_SUMMARY_LEN,
        )?;
    }
    Ok(())
}

fn validate_bounded_text(value: &str, kind: &'static str, max_len: usize) -> CoreResult<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(CoreError::Conflict(format!("{kind} is invalid")));
    }
    Ok(())
}

#[async_trait::async_trait]
pub trait OperationStore: Send + Sync {
    /// Enqueues with a store-generated creation timestamp. The idempotency key is accepted as
    /// existing only when its canonical request payload is identical; reuse with different
    /// content is a conflict.
    async fn enqueue(&self, operation: NewCloudOperation) -> CoreResult<EnqueueOperationResult>;

    async fn get_operation(&self, operation_id: &OperationId)
        -> CoreResult<Option<CloudOperation>>;

    /// Atomically claims one ready operation. Implementations must serialize mutations for
    /// the same resource and increment the fencing epoch whenever ownership changes or an
    /// expired lease is reclaimed. Serialization uses `(resource_kind, resource_id)` as its
    /// key. A reclaimed `Running` step must first be persisted as an unknown outcome requiring
    /// observation; it must never be made directly ready for replay. An unresolved unknown
    /// outcome blocks every later mutation for the same resource key. Readiness, deadlines,
    /// and lease expiry are evaluated with the store's authoritative clock.
    async fn claim_ready(
        &self,
        holder: &str,
        lease_duration_ms: u64,
    ) -> CoreResult<Option<ClaimedOperation>>;

    async fn renew_lease(
        &self,
        lease: &OperationLease,
        lease_duration_ms: u64,
    ) -> CoreResult<LeaseUpdate>;

    /// Atomically verifies an unexpired lease, selects the first ready apply step using the
    /// store's authoritative clock, changes it to `Running`, increments its attempt, assigns
    /// a fresh execution token, and returns the persisted snapshot. It must refuse terminal,
    /// cancelled, deadline-expired, already-running, and dormant compensation steps.
    /// The returned execution budget is computed from the actual remaining lease in the
    /// store's authoritative time domain after reserving `completion_margin_ms`. The store
    /// must refuse dispatch without changing a step to `Running` when no positive budget is
    /// available.
    async fn begin_step(
        &self,
        lease: &OperationLease,
        policy: DispatchPolicy,
    ) -> CoreResult<DispatchedStep>;

    /// Applies a step outcome only when the lease remains current and unexpired and the step
    /// still matches the dispatched attempt and execution token. Lease comparison uses stable
    /// fence identity, not `valid_until_unix_ms`, because renewal extends that value. All expiry
    /// and timestamp decisions use the store's authoritative clock, never caller-provided wall
    /// time. Retry timestamps are derived from that clock and `error.retry_after_ms`.
    /// Implementations advance the operation phase atomically and mark it succeeded when
    /// every active step has succeeded. Unknown outcomes are terminal until an explicit
    /// observation or operator resolution proves whether the provider mutation took effect.
    async fn complete_step(
        &self,
        dispatched: &DispatchedStep,
        completion: StepCompletion,
    ) -> CoreResult<CloudOperation>;

    async fn mark_cancelled(&self, lease: &OperationLease) -> CoreResult<CloudOperation>;

    async fn request_cancel(&self, operation_id: &OperationId) -> CoreResult<CloudOperation>;

    /// Resolves a terminal unknown outcome only if the current step still matches the exact
    /// attempt and execution token. Timestamps and retry scheduling use the store's clock.
    async fn resolve_unknown_outcome(
        &self,
        operation_id: &OperationId,
        step_name: &str,
        attempt: u32,
        execution_token: &str,
        resolution: UnknownOutcomeResolution,
    ) -> CoreResult<CloudOperation>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_requests_reject_empty_and_duplicate_steps() {
        let request = NewCloudOperation {
            idempotency_key: IdempotencyKey::new("request-1").unwrap(),
            resource_id: CloudResourceId::new("zone-1").unwrap(),
            resource_kind: CloudResourceKind::ManagedZone,
            action: CloudOperationAction::Reconcile,
            desired_generation: 1,
            requested_by: "operator".to_string(),
            deadline_unix_ms: None,
            steps: vec![
                NewCloudOperationStep {
                    name: "observe".to_string(),
                    purpose: CloudOperationStepPurpose::Apply,
                    idempotency_key: IdempotencyKey::new("step-1").unwrap(),
                },
                NewCloudOperationStep {
                    name: "observe".to_string(),
                    purpose: CloudOperationStepPurpose::Apply,
                    idempotency_key: IdempotencyKey::new("step-2").unwrap(),
                },
            ],
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn operation_requests_reject_duplicate_step_keys_and_compensation() {
        let mut request = valid_request();
        request.steps.push(NewCloudOperationStep {
            name: "publish".to_string(),
            purpose: CloudOperationStepPurpose::Apply,
            idempotency_key: request.steps[0].idempotency_key.clone(),
        });
        assert!(request.validate().is_err());

        let mut request = valid_request();
        request.steps[0].purpose = CloudOperationStepPurpose::Compensate;
        assert!(request.validate().is_err());
    }

    #[test]
    fn persisted_identifiers_enforce_storage_bounds() {
        assert!(OperationId::new("o".repeat(MAX_OPERATION_ID_LEN)).is_ok());
        assert!(OperationId::new("o".repeat(MAX_OPERATION_ID_LEN + 1)).is_err());
        assert!(IdempotencyKey::new("k".repeat(MAX_IDEMPOTENCY_KEY_LEN)).is_ok());
        assert!(IdempotencyKey::new("k".repeat(MAX_IDEMPOTENCY_KEY_LEN + 1)).is_err());
        assert!(CloudResourceId::new("r".repeat(512)).is_ok());
        assert!(CloudResourceId::new("r".repeat(513)).is_err());
    }

    #[test]
    fn operation_requests_enforce_persisted_shape_bounds() {
        let mut request = valid_request();
        request.desired_generation = i64::MAX as u64 + 1;
        assert!(request.validate().is_err());

        let mut request = valid_request();
        request.requested_by = "operator\nadmin".to_string();
        assert!(request.validate().is_err());

        let mut request = valid_request();
        request.steps[0].name = "s".repeat(MAX_STEP_NAME_LEN + 1);
        assert!(request.validate().is_err());

        let mut request = valid_request();
        request.deadline_unix_ms = Some(-1);
        assert!(request.validate().is_err());

        let mut request = valid_request();
        request.steps = (0..=MAX_OPERATION_STEPS)
            .map(|index| NewCloudOperationStep {
                name: format!("step-{index}"),
                purpose: CloudOperationStepPurpose::Apply,
                idempotency_key: IdempotencyKey::new(format!("step-key-{index}")).unwrap(),
            })
            .collect();
        assert!(request.validate().is_err());
    }

    #[test]
    fn operation_outputs_reject_unbounded_or_unsafe_text_and_delays() {
        let invalid_error = OperationError {
            kind: OperationErrorKind::Transient,
            code: "provider\nerror".to_string(),
            message: "temporary failure".to_string(),
            retry_after_ms: Some(10),
        };
        assert!(invalid_error.validate().is_err());

        let oversized_error = OperationError {
            kind: OperationErrorKind::Transient,
            code: "temporary".to_string(),
            message: "m".repeat(MAX_ERROR_MESSAGE_LEN + 1),
            retry_after_ms: Some(10),
        };
        assert!(oversized_error.validate().is_err());

        let overflowing_delay = OperationError {
            kind: OperationErrorKind::Transient,
            code: "temporary".to_string(),
            message: "temporary failure".to_string(),
            retry_after_ms: Some(i64::MAX as u64 + 1),
        };
        assert!(StepCompletion::RetryScheduled {
            error: overflowing_delay
        }
        .validate()
        .is_err());

        assert!(StepCompletion::Succeeded {
            summary: Some("s".repeat(MAX_STEP_SUMMARY_LEN + 1))
        }
        .validate()
        .is_err());
        assert!(UnknownOutcomeResolution::ConfirmedSucceeded {
            summary: Some("unsafe\tSummary".to_string())
        }
        .validate()
        .is_err());
    }

    #[test]
    fn outcomes_reject_error_kinds_that_contradict_the_transition() {
        let transient = OperationError {
            kind: OperationErrorKind::Transient,
            code: "temporary".to_string(),
            message: "temporary failure".to_string(),
            retry_after_ms: Some(10),
        };
        let permanent = OperationError {
            kind: OperationErrorKind::Permanent,
            code: "invalid".to_string(),
            message: "invalid request".to_string(),
            retry_after_ms: None,
        };
        let unknown = OperationError {
            kind: OperationErrorKind::UnknownOutcome,
            code: "ambiguous".to_string(),
            message: "ambiguous response".to_string(),
            retry_after_ms: None,
        };

        assert!(StepCompletion::RetryScheduled {
            error: transient.clone()
        }
        .validate()
        .is_ok());
        assert!(StepCompletion::RetryScheduled {
            error: permanent.clone()
        }
        .validate()
        .is_err());
        assert!(StepCompletion::Failed {
            error: permanent.clone()
        }
        .validate()
        .is_ok());
        assert!(StepCompletion::Failed {
            error: transient.clone()
        }
        .validate()
        .is_err());
        assert!(StepCompletion::UnknownOutcome {
            error: unknown.clone()
        }
        .validate()
        .is_ok());
        assert!(StepCompletion::UnknownOutcome {
            error: permanent.clone()
        }
        .validate()
        .is_err());
        assert!(UnknownOutcomeResolution::ConfirmedNotApplied {
            error: transient.clone()
        }
        .validate()
        .is_ok());
        assert!(UnknownOutcomeResolution::ConfirmedNotApplied {
            error: permanent.clone()
        }
        .validate()
        .is_err());
        assert!(
            UnknownOutcomeResolution::ConfirmedFailed { error: permanent }
                .validate()
                .is_ok()
        );
        assert!(
            UnknownOutcomeResolution::ConfirmedFailed { error: transient }
                .validate()
                .is_err()
        );
    }

    #[test]
    fn retry_steps_are_not_ready_before_their_schedule() {
        let operation = CloudOperation {
            id: OperationId::new("op-1").unwrap(),
            idempotency_key: IdempotencyKey::new("request-1").unwrap(),
            resource_id: CloudResourceId::new("zone-1").unwrap(),
            resource_kind: CloudResourceKind::ManagedZone,
            action: CloudOperationAction::Reconcile,
            desired_generation: 1,
            requested_by: "operator".to_string(),
            phase: CloudOperationPhase::RetryScheduled,
            cancel_requested: false,
            created_at_unix_ms: 10,
            updated_at_unix_ms: 10,
            deadline_unix_ms: None,
            steps: vec![CloudOperationStep {
                name: "apply".to_string(),
                purpose: CloudOperationStepPurpose::Apply,
                idempotency_key: IdempotencyKey::new("step-1").unwrap(),
                phase: CloudOperationStepPhase::RetryScheduled,
                attempt: 1,
                execution_token: None,
                next_attempt_at_unix_ms: Some(100),
                started_at_unix_ms: Some(10),
                finished_at_unix_ms: None,
                summary: None,
                error: None,
            }],
        };
        assert!(operation.next_ready_step(99).is_none());
        assert_eq!(operation.next_ready_step(100).unwrap().name, "apply");
    }

    #[test]
    fn later_steps_do_not_overtake_a_delayed_retry() {
        let mut operation = operation_with_steps();
        operation.steps[0].phase = CloudOperationStepPhase::RetryScheduled;
        operation.steps[0].next_attempt_at_unix_ms = Some(100);
        assert!(operation.next_ready_step(99).is_none());
        assert_eq!(operation.next_ready_step(100).unwrap().name, "observe");
    }

    #[test]
    fn terminal_cancelled_and_compensation_steps_are_never_auto_dispatched() {
        let mut operation = operation_with_steps();
        operation.phase = CloudOperationPhase::Succeeded;
        assert!(operation.next_ready_step(10).is_none());

        operation.phase = CloudOperationPhase::Pending;
        operation.cancel_requested = true;
        assert!(operation.next_ready_step(10).is_none());

        operation.cancel_requested = false;
        operation.steps[0].purpose = CloudOperationStepPurpose::Compensate;
        assert!(operation.next_ready_step(10).is_none());
    }

    #[test]
    fn dispatched_step_requires_exact_persisted_execution_fence() {
        let mut operation = operation_with_steps();
        operation.phase = CloudOperationPhase::Running;
        operation.steps[0].phase = CloudOperationStepPhase::Running;
        operation.steps[0].attempt = 1;
        operation.steps[0].execution_token = Some("execution-1".to_string());
        let lease = OperationLease {
            operation_id: operation.id.clone(),
            holder: "worker-1".to_string(),
            fencing_token: "lease-1".to_string(),
            fencing_epoch: 1,
            valid_until_unix_ms: 100,
        };
        let mut dispatched = DispatchedStep {
            step: operation.steps[0].clone(),
            operation,
            execution_token: "execution-1".to_string(),
            lease,
            execution_budget_ms: 50,
            dispatch_policy: DispatchPolicy {
                max_execution_ms: 50,
                completion_margin_ms: 10,
            },
        };
        assert!(dispatched.validate().is_ok());

        dispatched.execution_token = "stale-execution".to_string();
        assert!(dispatched.validate().is_err());

        dispatched.execution_token = "execution-1".to_string();
        dispatched.lease.fencing_epoch = 0;
        assert!(dispatched.validate().is_err());

        dispatched.lease.fencing_epoch = 1;
        dispatched.execution_budget_ms = 51;
        assert!(dispatched.validate().is_err());
    }

    #[test]
    fn dispatch_policy_requires_execution_and_completion_windows() {
        assert!(DispatchPolicy {
            max_execution_ms: 50,
            completion_margin_ms: 10,
        }
        .validate()
        .is_ok());
        assert!(DispatchPolicy {
            max_execution_ms: 0,
            completion_margin_ms: 10,
        }
        .validate()
        .is_err());
        assert!(DispatchPolicy {
            max_execution_ms: 50,
            completion_margin_ms: 0,
        }
        .validate()
        .is_err());
        assert!(DispatchPolicy {
            max_execution_ms: i64::MAX as u64,
            completion_margin_ms: 1,
        }
        .validate()
        .is_err());
    }

    #[test]
    fn renewal_keeps_the_same_stable_lease_fence() {
        let original = OperationLease {
            operation_id: OperationId::new("op-1").unwrap(),
            holder: "worker-1".to_string(),
            fencing_token: "lease-1".to_string(),
            fencing_epoch: 3,
            valid_until_unix_ms: 100,
        };
        let mut renewed = original.clone();
        renewed.valid_until_unix_ms = 200;
        assert!(original.same_fence(&renewed));

        renewed.fencing_epoch += 1;
        assert!(!original.same_fence(&renewed));
    }

    fn operation_with_steps() -> CloudOperation {
        CloudOperation {
            id: OperationId::new("op-2").unwrap(),
            idempotency_key: IdempotencyKey::new("request-2").unwrap(),
            resource_id: CloudResourceId::new("zone-2").unwrap(),
            resource_kind: CloudResourceKind::ManagedZone,
            action: CloudOperationAction::Reconcile,
            desired_generation: 1,
            requested_by: "operator".to_string(),
            phase: CloudOperationPhase::Pending,
            cancel_requested: false,
            created_at_unix_ms: 10,
            updated_at_unix_ms: 10,
            deadline_unix_ms: None,
            steps: vec![
                CloudOperationStep {
                    name: "observe".to_string(),
                    purpose: CloudOperationStepPurpose::Apply,
                    idempotency_key: IdempotencyKey::new("step-1").unwrap(),
                    phase: CloudOperationStepPhase::Pending,
                    attempt: 0,
                    execution_token: None,
                    next_attempt_at_unix_ms: None,
                    started_at_unix_ms: None,
                    finished_at_unix_ms: None,
                    summary: None,
                    error: None,
                },
                CloudOperationStep {
                    name: "apply".to_string(),
                    purpose: CloudOperationStepPurpose::Compensate,
                    idempotency_key: IdempotencyKey::new("step-2").unwrap(),
                    phase: CloudOperationStepPhase::Pending,
                    attempt: 0,
                    execution_token: None,
                    next_attempt_at_unix_ms: None,
                    started_at_unix_ms: None,
                    finished_at_unix_ms: None,
                    summary: None,
                    error: None,
                },
            ],
        }
    }

    fn valid_request() -> NewCloudOperation {
        NewCloudOperation {
            idempotency_key: IdempotencyKey::new("request-valid").unwrap(),
            resource_id: CloudResourceId::new("zone-valid").unwrap(),
            resource_kind: CloudResourceKind::ManagedZone,
            action: CloudOperationAction::Reconcile,
            desired_generation: 1,
            requested_by: "operator".to_string(),
            deadline_unix_ms: None,
            steps: vec![NewCloudOperationStep {
                name: "apply".to_string(),
                purpose: CloudOperationStepPurpose::Apply,
                idempotency_key: IdempotencyKey::new("step-valid").unwrap(),
            }],
        }
    }
}
