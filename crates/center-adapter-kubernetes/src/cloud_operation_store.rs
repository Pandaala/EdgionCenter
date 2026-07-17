use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Instant,
};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use edgion_center_core::{
    ClaimedOperation, CloudOperation, CloudOperationPhase, CloudOperationStepPhase,
    CloudOperationStepPurpose, CoreError, CoreResult, DispatchPolicy, DispatchedStep,
    EnqueueOperationResult, LeaseUpdate, NewCloudOperation, OperationId, OperationLease,
    OperationStore, StepCompletion, UnknownOutcomeResolution,
};
use k8s_openapi::{
    api::coordination::v1::{Lease, LeaseSpec},
    apimachinery::pkg::apis::meta::v1::MicroTime,
};
use kube::{api::PostParams, Api, Client, ResourceExt};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::cloud_operation_crd::{
    EdgionCloudOperation, EdgionCloudOperationErrorStatus, EdgionCloudOperationLeaseStatus,
    EdgionCloudOperationSpec, EdgionCloudOperationStatus,
};

const MAX_CONFLICT_RETRIES: usize = 8;
const RESOURCE_KEY_ANNOTATION: &str = "center.edgion.io/cloud-resource-key";
const OPERATION_ANNOTATION: &str = "center.edgion.io/cloud-operation";
const TOKEN_ANNOTATION: &str = "center.edgion.io/cloud-fencing-token";
const EPOCH_ANNOTATION: &str = "center.edgion.io/cloud-fencing-epoch";

#[derive(Debug)]
enum ResourceError {
    Conflict,
    Other(String),
}

impl From<kube::Error> for ResourceError {
    fn from(error: kube::Error) -> Self {
        match &error {
            kube::Error::Api(response) if response.code == 409 => Self::Conflict,
            _ => Self::Other(error.to_string()),
        }
    }
}

#[async_trait]
trait OperationResources: Send + Sync {
    async fn get_operation(
        &self,
        name: &str,
    ) -> Result<Option<EdgionCloudOperation>, ResourceError>;
    async fn list_operations(&self) -> Result<Vec<EdgionCloudOperation>, ResourceError>;
    async fn create_operation(
        &self,
        operation: &EdgionCloudOperation,
    ) -> Result<EdgionCloudOperation, ResourceError>;
    async fn replace_operation_status(
        &self,
        name: &str,
        operation: &EdgionCloudOperation,
    ) -> Result<EdgionCloudOperation, ResourceError>;
    async fn get_lease(&self, name: &str) -> Result<Option<Lease>, ResourceError>;
    async fn create_lease(&self, lease: &Lease) -> Result<Lease, ResourceError>;
    async fn replace_lease(&self, name: &str, lease: &Lease) -> Result<Lease, ResourceError>;
}

struct KubernetesOperationResources {
    operations: Api<EdgionCloudOperation>,
    leases: Api<Lease>,
}

#[async_trait]
impl OperationResources for KubernetesOperationResources {
    async fn get_operation(
        &self,
        name: &str,
    ) -> Result<Option<EdgionCloudOperation>, ResourceError> {
        self.operations.get_opt(name).await.map_err(Into::into)
    }

    async fn list_operations(&self) -> Result<Vec<EdgionCloudOperation>, ResourceError> {
        self.operations
            .list(&Default::default())
            .await
            .map(|list| list.items)
            .map_err(Into::into)
    }

    async fn create_operation(
        &self,
        operation: &EdgionCloudOperation,
    ) -> Result<EdgionCloudOperation, ResourceError> {
        self.operations
            .create(&PostParams::default(), operation)
            .await
            .map_err(Into::into)
    }

    async fn replace_operation_status(
        &self,
        name: &str,
        operation: &EdgionCloudOperation,
    ) -> Result<EdgionCloudOperation, ResourceError> {
        let body = serde_json::to_vec(operation)
            .map_err(|error| ResourceError::Other(error.to_string()))?;
        self.operations
            .replace_status(name, &PostParams::default(), body)
            .await
            .map_err(Into::into)
    }

    async fn get_lease(&self, name: &str) -> Result<Option<Lease>, ResourceError> {
        self.leases.get_opt(name).await.map_err(Into::into)
    }

    async fn create_lease(&self, lease: &Lease) -> Result<Lease, ResourceError> {
        self.leases
            .create(&PostParams::default(), lease)
            .await
            .map_err(Into::into)
    }

    async fn replace_lease(&self, name: &str, lease: &Lease) -> Result<Lease, ResourceError> {
        self.leases
            .replace(name, &PostParams::default(), lease)
            .await
            .map_err(Into::into)
    }
}

trait Clock: Send + Sync {
    fn now_unix_ms(&self) -> i64;
    fn monotonic_ms(&self) -> u64;
}

struct SystemClock {
    started: Instant,
}

impl Clock for SystemClock {
    fn now_unix_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64
    }

    fn monotonic_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}

/// Kubernetes-native durable cloud operation storage.
///
/// The operation CRD is the durable state machine. A standard Lease per
/// `(resource_kind, resource_id)` serializes provider mutations across
/// operations and replicas. No provider worker is started by this adapter.
#[derive(Clone)]
pub struct KubernetesOperationStore {
    resources: Arc<dyn OperationResources>,
    clock: Arc<dyn Clock>,
    lease_observations: Arc<Mutex<HashMap<String, (String, u64)>>>,
}

impl KubernetesOperationStore {
    pub fn new(client: Client, namespace: &str) -> CoreResult<Self> {
        if namespace.trim().is_empty() || namespace.chars().any(char::is_control) {
            return Err(CoreError::Adapter(
                "Kubernetes cloud operation namespace must be non-empty".to_string(),
            ));
        }
        Ok(Self {
            resources: Arc::new(KubernetesOperationResources {
                operations: Api::namespaced(client.clone(), namespace),
                leases: Api::namespaced(client, namespace),
            }),
            clock: Arc::new(SystemClock {
                started: Instant::now(),
            }),
            lease_observations: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    #[cfg(test)]
    fn with_resources(resources: Arc<dyn OperationResources>, clock: Arc<dyn Clock>) -> Self {
        Self {
            resources,
            clock,
            lease_observations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn adapter_error(error: ResourceError) -> CoreError {
        match error {
            ResourceError::Conflict => {
                CoreError::Conflict("Kubernetes resourceVersion conflict".to_string())
            }
            ResourceError::Other(message) => CoreError::Adapter(message),
        }
    }

    async fn load_operation(&self, id: &OperationId) -> CoreResult<Option<EdgionCloudOperation>> {
        let name = resource_name_for_operation(id)?;
        self.resources
            .get_operation(&name)
            .await
            .map_err(Self::adapter_error)
    }

    fn core(resource: &EdgionCloudOperation) -> CoreResult<CloudOperation> {
        resource.to_core().map_err(|message| {
            CoreError::Conflict(format!("invalid cloud operation CRD: {message}"))
        })
    }

    fn current_lease(resource: &EdgionCloudOperation) -> CoreResult<Option<OperationLease>> {
        resource.lease().map_err(|message| {
            CoreError::Conflict(format!("invalid cloud operation lease: {message}"))
        })
    }

    async fn initialize_missing_status(
        &self,
        mut resource: EdgionCloudOperation,
        now: i64,
    ) -> CoreResult<EdgionCloudOperation> {
        if resource.status.is_some() {
            return Ok(resource);
        }
        let request = resource.spec.to_request().map_err(|message| {
            CoreError::Conflict(format!("invalid cloud operation spec: {message}"))
        })?;
        request.validate()?;
        resource.status = Some(EdgionCloudOperationStatus::new(&request, now));
        let name = resource.name_any();
        self.resources
            .replace_operation_status(&name, &resource)
            .await
            .map_err(Self::adapter_error)
    }

    async fn acquire_resource_lease(
        &self,
        operation: &CloudOperation,
        holder: &str,
        lease_duration_ms: u64,
        now: i64,
    ) -> CoreResult<Option<OperationLease>> {
        validate_claim_config(holder, lease_duration_ms)?;
        let resource_key = resource_key(operation);
        let name = resource_lease_name(&resource_key);
        for _ in 0..MAX_CONFLICT_RETRIES {
            let current = self
                .resources
                .get_lease(&name)
                .await
                .map_err(Self::adapter_error)?;
            if let Some(lease) = current.as_ref() {
                verify_resource_key(lease, &resource_key)?;
                if self.resource_lease_active(&name, lease)? {
                    return Ok(None);
                }
            }
            let epoch = current
                .as_ref()
                .map(lease_epoch)
                .transpose()?
                .unwrap_or_default()
                .checked_add(1)
                .ok_or_else(|| CoreError::Conflict("cloud fencing epoch exhausted".to_string()))?;
            let token = Uuid::new_v4().to_string();
            let valid_until = now.checked_add(lease_duration_ms as i64).ok_or_else(|| {
                CoreError::Conflict("cloud operation lease deadline overflowed".to_string())
            })?;
            let replacement = resource_lease(
                &name,
                current.as_ref(),
                &resource_key,
                operation.id.as_str(),
                holder,
                &token,
                epoch,
                now,
                lease_duration_ms,
            )?;
            let result = if current.is_some() {
                self.resources.replace_lease(&name, &replacement).await
            } else {
                self.resources.create_lease(&replacement).await
            };
            match result {
                Ok(_) => {
                    return Ok(Some(OperationLease {
                        operation_id: operation.id.clone(),
                        holder: holder.to_string(),
                        fencing_token: token,
                        fencing_epoch: epoch,
                        valid_until_unix_ms: valid_until,
                    }))
                }
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(
            "cloud resource Lease remained conflicted".to_string(),
        ))
    }

    async fn verify_resource_fence(
        &self,
        operation: &CloudOperation,
        lease: &OperationLease,
    ) -> CoreResult<Lease> {
        let key = resource_key(operation);
        let name = resource_lease_name(&key);
        let current = self
            .resources
            .get_lease(&name)
            .await
            .map_err(Self::adapter_error)?
            .ok_or_else(|| CoreError::Conflict("cloud resource Lease is absent".to_string()))?;
        verify_resource_key(&current, &key)?;
        if !resource_lease_matches(&current, lease)?
            || !self.resource_lease_active(&name, &current)?
        {
            return Err(CoreError::Conflict(
                "cloud resource Lease fence was lost or expired".to_string(),
            ));
        }
        Ok(current)
    }

    fn resource_lease_active(&self, name: &str, lease: &Lease) -> CoreResult<bool> {
        let Some(spec) = lease.spec.as_ref() else {
            return Ok(false);
        };
        if spec
            .holder_identity
            .as_deref()
            .unwrap_or_default()
            .is_empty()
        {
            return Ok(false);
        }
        let duration_seconds = spec.lease_duration_seconds.unwrap_or_default();
        if duration_seconds <= 0 {
            return Ok(false);
        }
        let marker = lease
            .metadata
            .resource_version
            .clone()
            .or_else(|| {
                lease
                    .metadata
                    .annotations
                    .as_ref()
                    .and_then(|values| values.get(TOKEN_ANNOTATION).cloned())
            })
            .unwrap_or_default();
        let now = self.clock.monotonic_ms();
        let mut observations = self.lease_observations.lock().unwrap();
        let observed_at = match observations.get(name) {
            Some((seen, observed_at)) if seen == &marker => *observed_at,
            _ => {
                observations.insert(name.to_string(), (marker, now));
                now
            }
        };
        Ok(now.saturating_sub(observed_at) < duration_seconds as u64 * 1000)
    }

    async fn release_resource_lease(
        &self,
        operation: &CloudOperation,
        lease: &OperationLease,
    ) -> CoreResult<()> {
        let key = resource_key(operation);
        let name = resource_lease_name(&key);
        for _ in 0..MAX_CONFLICT_RETRIES {
            let Some(mut current) = self
                .resources
                .get_lease(&name)
                .await
                .map_err(Self::adapter_error)?
            else {
                return Ok(());
            };
            verify_resource_key(&current, &key)?;
            if !resource_lease_matches(&current, lease)? {
                return Ok(());
            }
            let annotations = current.metadata.annotations.get_or_insert_default();
            annotations.remove(TOKEN_ANNOTATION);
            annotations.remove(OPERATION_ANNOTATION);
            let spec = current.spec.get_or_insert_default();
            spec.holder_identity = None;
            match self.resources.replace_lease(&name, &current).await {
                Ok(_) => return Ok(()),
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(
            "cloud resource Lease release remained conflicted".to_string(),
        ))
    }

    async fn persist_recovered_unknown(
        &self,
        mut resource: EdgionCloudOperation,
        lease: &OperationLease,
        now: i64,
    ) -> CoreResult<()> {
        let operation = Self::core(&resource)?;
        let running = operation
            .steps
            .iter()
            .position(|step| step.phase == CloudOperationStepPhase::Running);
        let Some(index) = running else {
            return Ok(());
        };
        let status = resource.status.as_mut().expect("validated status");
        status.phase = "unknown_outcome".to_string();
        status.updated_at_unix_ms = now;
        status.lease = None;
        let step = &mut status.steps[index];
        step.phase = "unknown_outcome".to_string();
        step.finished_at_unix_ms = Some(now);
        step.error = Some(EdgionCloudOperationErrorStatus {
            kind: "unknown_outcome".to_string(),
            code: "worker_lease_expired".to_string(),
            message: "worker ownership expired while the provider outcome was unknown".to_string(),
            retry_after_ms: None,
        });
        let name = resource.name_any();
        if let Err(error) = self
            .resources
            .replace_operation_status(&name, &resource)
            .await
        {
            let _ = self.release_resource_lease(&operation, lease).await;
            return Err(Self::adapter_error(error));
        }
        if let Err(error) = self.release_resource_lease(&operation, lease).await {
            tracing::error!(%error, "Recovered unknown outcome but failed to release its exact resource Lease");
        }
        Ok(())
    }

    async fn persist_deadline_failed(
        &self,
        mut resource: EdgionCloudOperation,
        lease: &OperationLease,
        now: i64,
    ) -> CoreResult<CloudOperation> {
        let operation = Self::core(&resource)?;
        let index = operation.steps.iter().position(|step| {
            step.purpose == CloudOperationStepPurpose::Apply
                && step.phase != CloudOperationStepPhase::Succeeded
        });
        let status = resource.status.as_mut().expect("validated status");
        status.phase = "failed".to_string();
        status.updated_at_unix_ms = now;
        status.lease = None;
        if let Some(index) = index {
            let step = &mut status.steps[index];
            step.phase = "failed".to_string();
            step.finished_at_unix_ms = Some(now);
            step.error = Some(EdgionCloudOperationErrorStatus {
                kind: "permanent".to_string(),
                code: "operation_deadline_exceeded".to_string(),
                message: "operation deadline elapsed before step dispatch".to_string(),
                retry_after_ms: None,
            });
        }
        let name = resource.name_any();
        let persisted = match self
            .resources
            .replace_operation_status(&name, &resource)
            .await
        {
            Ok(persisted) => persisted,
            Err(error) => {
                let _ = self.release_resource_lease(&operation, lease).await;
                return Err(Self::adapter_error(error));
            }
        };
        if let Err(error) = self.release_resource_lease(&operation, lease).await {
            tracing::error!(%error, "Persisted deadline failure but failed to release its exact resource Lease");
        }
        Self::core(&persisted)
    }

    async fn release_undispatched_claim(
        &self,
        mut resource: EdgionCloudOperation,
        lease: &OperationLease,
        now: i64,
    ) -> CoreResult<()> {
        let operation = Self::core(&resource)?;
        let status = resource.status.as_mut().expect("validated status");
        status.lease = None;
        status.updated_at_unix_ms = now;
        status.phase = if status.cancel_requested {
            "cancel_requested"
        } else if status
            .steps
            .iter()
            .any(|step| step.phase == "retry_scheduled")
        {
            "retry_scheduled"
        } else {
            "pending"
        }
        .to_string();
        let name = resource.name_any();
        if let Err(error) = self
            .resources
            .replace_operation_status(&name, &resource)
            .await
        {
            let _ = self.release_resource_lease(&operation, lease).await;
            return Err(Self::adapter_error(error));
        }
        if let Err(error) = self.release_resource_lease(&operation, lease).await {
            tracing::error!(%error, "Released undispatched claim state but failed to release its exact resource Lease");
        }
        Ok(())
    }
}

#[async_trait]
impl OperationStore for KubernetesOperationStore {
    async fn enqueue(&self, request: NewCloudOperation) -> CoreResult<EnqueueOperationResult> {
        request.validate()?;
        let name = cloud_operation_resource_name(request.idempotency_key.as_str());
        let operation_id = operation_id_for_key(request.idempotency_key.as_str());
        let spec = EdgionCloudOperationSpec::from_request(operation_id, &request);
        for _ in 0..MAX_CONFLICT_RETRIES {
            if let Some(mut existing) = self
                .resources
                .get_operation(&name)
                .await
                .map_err(Self::adapter_error)?
            {
                let stored = existing.spec.to_request().map_err(|message| {
                    CoreError::Conflict(format!("invalid cloud operation spec: {message}"))
                })?;
                if stored != request {
                    return Err(CoreError::Conflict(
                        "cloud operation idempotency key was reused with a different request"
                            .to_string(),
                    ));
                }
                if existing.status.is_none() {
                    match self
                        .initialize_missing_status(existing, self.clock.now_unix_ms())
                        .await
                    {
                        Ok(initialized) => existing = initialized,
                        Err(CoreError::Conflict(_)) => continue,
                        Err(error) => return Err(error),
                    }
                }
                return Ok(EnqueueOperationResult::Existing(Self::core(&existing)?));
            }
            let now = self.clock.now_unix_ms();
            let mut resource = EdgionCloudOperation::new(&name, spec.clone());
            resource.status = Some(EdgionCloudOperationStatus::new(&request, now));
            match self.resources.create_operation(&resource).await {
                Ok(mut created) => {
                    if created.status.is_none() {
                        created.status = resource.status;
                        created = self
                            .resources
                            .replace_operation_status(&name, &created)
                            .await
                            .map_err(Self::adapter_error)?;
                    }
                    return Ok(EnqueueOperationResult::Created(Self::core(&created)?));
                }
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(
            "cloud operation enqueue remained conflicted".to_string(),
        ))
    }

    async fn get_operation(
        &self,
        operation_id: &OperationId,
    ) -> CoreResult<Option<CloudOperation>> {
        self.load_operation(operation_id)
            .await?
            .as_ref()
            .map(Self::core)
            .transpose()
    }

    async fn claim_ready(
        &self,
        holder: &str,
        lease_duration_ms: u64,
    ) -> CoreResult<Option<ClaimedOperation>> {
        validate_claim_config(holder, lease_duration_ms)?;
        let now = self.clock.now_unix_ms();
        let mut resources = self
            .resources
            .list_operations()
            .await
            .map_err(Self::adapter_error)?;
        resources.sort_by(|left, right| {
            let left_status = left.status.as_ref();
            let right_status = right.status.as_ref();
            left_status
                .map(|status| status.created_at_unix_ms)
                .cmp(&right_status.map(|status| status.created_at_unix_ms))
                .then_with(|| left.spec.operation_id.cmp(&right.spec.operation_id))
        });
        let mut blocked_resources = BTreeSet::new();
        for mut resource in resources {
            let raw_resource_key = format!(
                "{}:{}",
                resource.spec.resource_kind, resource.spec.resource_id
            );
            if blocked_resources.contains(&raw_resource_key) {
                continue;
            }
            if resource.status.is_none() {
                match self.initialize_missing_status(resource, now).await {
                    Ok(initialized) => resource = initialized,
                    Err(error) => {
                        tracing::error!(
                            operation = %raw_resource_key,
                            %error,
                            "Skipping malformed cloud operation without starving other resources"
                        );
                        blocked_resources.insert(raw_resource_key);
                        continue;
                    }
                }
            }
            let operation = match Self::core(&resource) {
                Ok(operation) => operation,
                Err(error) => {
                    tracing::error!(
                        operation = %raw_resource_key,
                        %error,
                        "Skipping malformed cloud operation without starving other resources"
                    );
                    blocked_resources.insert(raw_resource_key);
                    continue;
                }
            };
            if matches!(
                operation.phase,
                CloudOperationPhase::Succeeded
                    | CloudOperationPhase::Failed
                    | CloudOperationPhase::Cancelled
            ) {
                continue;
            }
            let key = resource_key(&operation);
            if !blocked_resources.insert(key) {
                continue;
            }
            if operation.phase == CloudOperationPhase::UnknownOutcome {
                continue;
            }
            let ready = operation.cancel_requested
                || operation.next_ready_step(now).is_some()
                || operation
                    .steps
                    .iter()
                    .any(|step| step.phase == CloudOperationStepPhase::Running);
            if !ready {
                continue;
            }
            let Some(lease) = self
                .acquire_resource_lease(&operation, holder, lease_duration_ms, now)
                .await?
            else {
                continue;
            };
            if operation
                .steps
                .iter()
                .any(|step| step.phase == CloudOperationStepPhase::Running)
            {
                self.persist_recovered_unknown(resource, &lease, now)
                    .await?;
                continue;
            }
            if operation
                .deadline_unix_ms
                .is_some_and(|deadline| deadline <= now)
            {
                self.persist_deadline_failed(resource, &lease, now).await?;
                continue;
            }
            let status = resource.status.as_mut().expect("validated status");
            status.phase = if status.cancel_requested {
                "cancel_requested".to_string()
            } else {
                "running".to_string()
            };
            status.updated_at_unix_ms = now;
            status.lease = Some(EdgionCloudOperationLeaseStatus::from(&lease));
            let name = resource.name_any();
            match self
                .resources
                .replace_operation_status(&name, &resource)
                .await
            {
                Ok(persisted) => {
                    let claim = ClaimedOperation {
                        operation: Self::core(&persisted)?,
                        lease,
                    };
                    claim.validate()?;
                    return Ok(Some(claim));
                }
                Err(error) => {
                    self.release_resource_lease(&operation, &lease).await?;
                    if matches!(error, ResourceError::Conflict) {
                        continue;
                    }
                    return Err(Self::adapter_error(error));
                }
            }
        }
        Ok(None)
    }

    async fn renew_lease(
        &self,
        lease: &OperationLease,
        lease_duration_ms: u64,
    ) -> CoreResult<LeaseUpdate> {
        validate_claim_config(&lease.holder, lease_duration_ms)?;
        let now = self.clock.now_unix_ms();
        let Some(mut operation_resource) = self.load_operation(&lease.operation_id).await? else {
            return Ok(LeaseUpdate::Lost);
        };
        let operation = Self::core(&operation_resource)?;
        if Self::current_lease(&operation_resource)?
            .as_ref()
            .is_none_or(|stored| !stored.same_fence(lease))
        {
            return Ok(LeaseUpdate::Lost);
        }
        let mut resource_lease = match self.verify_resource_fence(&operation, lease).await {
            Ok(lease) => lease,
            Err(CoreError::Conflict(_)) => return Ok(LeaseUpdate::Lost),
            Err(error) => return Err(error),
        };
        let valid_until_unix_ms = now
            .checked_add(lease_duration_ms as i64)
            .ok_or_else(|| CoreError::Conflict("cloud lease renewal overflowed".to_string()))?;
        let spec = resource_lease.spec.get_or_insert_default();
        spec.renew_time = Some(MicroTime(datetime(now)?));
        spec.lease_duration_seconds = Some(duration_seconds(lease_duration_ms)?);
        let lease_name = resource_lease.name_any();
        self.resources
            .replace_lease(&lease_name, &resource_lease)
            .await
            .map_err(Self::adapter_error)?;
        let renewed = OperationLease {
            valid_until_unix_ms,
            ..lease.clone()
        };
        let status = operation_resource
            .status
            .as_mut()
            .expect("validated status");
        status.lease = Some(EdgionCloudOperationLeaseStatus::from(&renewed));
        status.updated_at_unix_ms = now;
        let operation_name = operation_resource.name_any();
        match self
            .resources
            .replace_operation_status(&operation_name, &operation_resource)
            .await
        {
            Ok(_) => Ok(LeaseUpdate::Renewed(renewed)),
            Err(ResourceError::Conflict) => {
                let _ = self.release_resource_lease(&operation, &renewed).await;
                Ok(LeaseUpdate::Lost)
            }
            Err(error) => {
                let _ = self.release_resource_lease(&operation, &renewed).await;
                Err(Self::adapter_error(error))
            }
        }
    }

    async fn begin_step(
        &self,
        lease: &OperationLease,
        policy: DispatchPolicy,
    ) -> CoreResult<DispatchedStep> {
        policy.validate()?;
        let now = self.clock.now_unix_ms();
        let mut resource = self
            .load_operation(&lease.operation_id)
            .await?
            .ok_or_else(|| {
                CoreError::NotFound(format!("cloud operation {}", lease.operation_id))
            })?;
        let operation = Self::core(&resource)?;
        let stored_lease = Self::current_lease(&resource)?.ok_or_else(|| {
            CoreError::Conflict("cloud operation is not currently claimed".to_string())
        })?;
        if !stored_lease.same_fence(lease) || stored_lease.valid_until_unix_ms <= now {
            return Err(CoreError::Conflict(
                "cloud operation lease was lost or expired".to_string(),
            ));
        }
        self.verify_resource_fence(&operation, lease).await?;
        if operation
            .deadline_unix_ms
            .is_some_and(|deadline| deadline <= now)
        {
            self.persist_deadline_failed(resource, lease, now).await?;
            return Err(CoreError::Conflict(
                "cloud operation deadline elapsed before dispatch".to_string(),
            ));
        }
        let remaining_ms = stored_lease.valid_until_unix_ms.saturating_sub(now);
        let completion_margin_ms = i64::try_from(policy.completion_margin_ms).unwrap_or(i64::MAX);
        if remaining_ms <= completion_margin_ms {
            self.release_undispatched_claim(resource, lease, now)
                .await?;
            return Err(CoreError::Conflict(
                "cloud operation lease has insufficient time remaining for dispatch".to_string(),
            ));
        }
        let step_name = operation
            .next_ready_step(now)
            .map(|step| step.name.clone())
            .ok_or_else(|| {
                CoreError::Conflict("cloud operation has no ready apply step".to_string())
            })?;
        let index = operation
            .steps
            .iter()
            .position(|step| step.name == step_name)
            .expect("selected step exists");
        let execution_token = Uuid::new_v4().to_string();
        let status = resource.status.as_mut().expect("validated status");
        status.phase = "running".to_string();
        status.updated_at_unix_ms = now;
        let step = &mut status.steps[index];
        step.phase = "running".to_string();
        step.attempt = step
            .attempt
            .checked_add(1)
            .ok_or_else(|| CoreError::Conflict("cloud step attempt exhausted".to_string()))?;
        step.execution_token = Some(execution_token.clone());
        step.started_at_unix_ms = Some(now);
        step.finished_at_unix_ms = None;
        step.next_attempt_at_unix_ms = None;
        step.error = None;
        let name = resource.name_any();
        let persisted = match self
            .resources
            .replace_operation_status(&name, &resource)
            .await
        {
            Ok(persisted) => persisted,
            Err(error) => {
                let _ = self.release_resource_lease(&operation, lease).await;
                return Err(Self::adapter_error(error));
            }
        };
        let operation = Self::core(&persisted)?;
        let step = operation.steps[index].clone();
        // Close the two-object race between persisting `Running` and allowing provider
        // dispatch. A takeover in that window leaves the step Running for conservative
        // unknown-outcome recovery, but must never receive a dispatch authorization.
        self.verify_resource_fence(&operation, &stored_lease)
            .await?;
        let dispatched = DispatchedStep {
            operation,
            step,
            execution_token,
            lease: stored_lease,
            execution_budget_ms: policy
                .max_execution_ms
                .min(remaining_ms.saturating_sub(completion_margin_ms) as u64),
            dispatch_policy: policy,
        };
        dispatched.validate()?;
        Ok(dispatched)
    }

    async fn complete_step(
        &self,
        dispatched: &DispatchedStep,
        completion: StepCompletion,
    ) -> CoreResult<CloudOperation> {
        dispatched.validate()?;
        completion.validate()?;
        let now = self.clock.now_unix_ms();
        let mut resource = self
            .load_operation(&dispatched.operation.id)
            .await?
            .ok_or_else(|| {
                CoreError::NotFound(format!("cloud operation {}", dispatched.operation.id))
            })?;
        let operation = Self::core(&resource)?;
        let index = operation
            .steps
            .iter()
            .position(|step| step.name == dispatched.step.name)
            .ok_or_else(|| CoreError::Conflict("dispatched cloud step disappeared".to_string()))?;
        let current = &operation.steps[index];
        let persisted_phase = match &completion {
            StepCompletion::Succeeded { .. } => CloudOperationStepPhase::Succeeded,
            StepCompletion::RetryScheduled { .. } => CloudOperationStepPhase::RetryScheduled,
            StepCompletion::Failed { .. } => CloudOperationStepPhase::Failed,
            StepCompletion::UnknownOutcome { .. } => CloudOperationStepPhase::UnknownOutcome,
        };
        if current.attempt == dispatched.step.attempt
            && current.execution_token.as_deref() == Some(dispatched.execution_token.as_str())
            && current.phase == persisted_phase
        {
            return Ok(operation);
        }
        let stored_lease = Self::current_lease(&resource)?.ok_or_else(|| {
            CoreError::Conflict("cloud operation completion has no active lease".to_string())
        })?;
        if !stored_lease.same_fence(&dispatched.lease) || stored_lease.valid_until_unix_ms <= now {
            return Err(CoreError::Conflict(
                "stale cloud operation completion".to_string(),
            ));
        }
        self.verify_resource_fence(&operation, &dispatched.lease)
            .await?;
        if current.phase != CloudOperationStepPhase::Running
            || current.attempt != dispatched.step.attempt
            || current.execution_token.as_deref() != Some(dispatched.execution_token.as_str())
        {
            return Err(CoreError::Conflict(
                "stale cloud step completion".to_string(),
            ));
        }
        let status = resource.status.as_mut().expect("validated status");
        status.updated_at_unix_ms = now;
        status.lease = None;
        let step = &mut status.steps[index];
        step.finished_at_unix_ms = Some(now);
        match completion {
            StepCompletion::Succeeded { summary } => {
                step.phase = "succeeded".to_string();
                step.summary = summary;
                step.error = None;
                let all_apply_succeeded =
                    operation.steps.iter().enumerate().all(|(position, step)| {
                        step.purpose == CloudOperationStepPurpose::Compensate
                            || position == index
                            || step.phase == CloudOperationStepPhase::Succeeded
                    });
                status.phase = if all_apply_succeeded {
                    "succeeded"
                } else {
                    "pending"
                }
                .to_string();
            }
            StepCompletion::RetryScheduled { error } => {
                step.phase = "retry_scheduled".to_string();
                step.next_attempt_at_unix_ms = Some(
                    now.saturating_add(
                        error
                            .retry_after_ms
                            .unwrap_or_default()
                            .min(i64::MAX as u64) as i64,
                    ),
                );
                step.error = Some((&error).into());
                status.phase = "retry_scheduled".to_string();
            }
            StepCompletion::Failed { error } => {
                step.phase = "failed".to_string();
                step.error = Some((&error).into());
                status.phase = "failed".to_string();
            }
            StepCompletion::UnknownOutcome { error } => {
                step.phase = "unknown_outcome".to_string();
                step.error = Some((&error).into());
                status.phase = "unknown_outcome".to_string();
            }
        }
        let name = resource.name_any();
        let persisted = self
            .resources
            .replace_operation_status(&name, &resource)
            .await
            .map_err(Self::adapter_error)?;
        if let Err(error) = self
            .release_resource_lease(&operation, &dispatched.lease)
            .await
        {
            tracing::error!(%error, "Persisted cloud step completion but failed to release its exact resource Lease");
        }
        Self::core(&persisted)
    }

    async fn mark_cancelled(&self, lease: &OperationLease) -> CoreResult<CloudOperation> {
        let now = self.clock.now_unix_ms();
        let mut resource = self
            .load_operation(&lease.operation_id)
            .await?
            .ok_or_else(|| {
                CoreError::NotFound(format!("cloud operation {}", lease.operation_id))
            })?;
        let operation = Self::core(&resource)?;
        let stored = Self::current_lease(&resource)?.ok_or_else(|| {
            CoreError::Conflict("cloud operation cancellation has no active lease".to_string())
        })?;
        if !stored.same_fence(lease) || stored.valid_until_unix_ms <= now {
            return Err(CoreError::Conflict(
                "stale cloud operation cancellation".to_string(),
            ));
        }
        self.verify_resource_fence(&operation, lease).await?;
        if operation
            .steps
            .iter()
            .any(|step| step.phase == CloudOperationStepPhase::Running)
        {
            return Err(CoreError::Conflict(
                "running cloud operation step cannot be cancelled as undispatched".to_string(),
            ));
        }
        let status = resource.status.as_mut().expect("validated status");
        status.phase = "cancelled".to_string();
        status.cancel_requested = true;
        status.updated_at_unix_ms = now;
        status.lease = None;
        for step in &mut status.steps {
            if matches!(step.phase.as_str(), "pending" | "retry_scheduled") {
                step.phase = "cancelled".to_string();
                step.finished_at_unix_ms = Some(now);
            }
        }
        let name = resource.name_any();
        let persisted = self
            .resources
            .replace_operation_status(&name, &resource)
            .await
            .map_err(Self::adapter_error)?;
        if let Err(error) = self.release_resource_lease(&operation, lease).await {
            tracing::error!(%error, "Persisted cloud cancellation but failed to release its exact resource Lease");
        }
        Self::core(&persisted)
    }

    async fn request_cancel(&self, operation_id: &OperationId) -> CoreResult<CloudOperation> {
        for _ in 0..MAX_CONFLICT_RETRIES {
            let mut resource = self
                .load_operation(operation_id)
                .await?
                .ok_or_else(|| CoreError::NotFound(format!("cloud operation {operation_id}")))?;
            let operation = Self::core(&resource)?;
            if operation.phase.is_terminal() {
                return Ok(operation);
            }
            let now = self.clock.now_unix_ms();
            let status = resource.status.as_mut().expect("validated status");
            status.cancel_requested = true;
            status.phase = "cancel_requested".to_string();
            status.updated_at_unix_ms = now;
            let name = resource.name_any();
            match self
                .resources
                .replace_operation_status(&name, &resource)
                .await
            {
                Ok(persisted) => return Self::core(&persisted),
                Err(ResourceError::Conflict) => continue,
                Err(error) => return Err(Self::adapter_error(error)),
            }
        }
        Err(CoreError::Conflict(
            "cloud cancellation request remained conflicted".to_string(),
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
        let mut resource = self
            .load_operation(operation_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("cloud operation {operation_id}")))?;
        let operation = Self::core(&resource)?;
        let index = operation
            .steps
            .iter()
            .position(|step| step.name == step_name)
            .ok_or_else(|| CoreError::NotFound(format!("cloud operation step {step_name}")))?;
        let current = &operation.steps[index];
        if operation.phase != CloudOperationPhase::UnknownOutcome
            || current.phase != CloudOperationStepPhase::UnknownOutcome
            || current.attempt != attempt
            || current.execution_token.as_deref() != Some(execution_token)
        {
            return Err(CoreError::Conflict(
                "unknown outcome resolution did not match the exact execution fence".to_string(),
            ));
        }
        let now = self.clock.now_unix_ms();
        let status = resource.status.as_mut().expect("validated status");
        status.updated_at_unix_ms = now;
        let step = &mut status.steps[index];
        match resolution {
            UnknownOutcomeResolution::ConfirmedSucceeded { summary } => {
                step.phase = "succeeded".to_string();
                step.summary = summary;
                step.error = None;
                status.phase = if operation.steps.iter().enumerate().all(|(position, step)| {
                    step.purpose == CloudOperationStepPurpose::Compensate
                        || position == index
                        || step.phase == CloudOperationStepPhase::Succeeded
                }) {
                    "succeeded"
                } else {
                    "pending"
                }
                .to_string();
            }
            UnknownOutcomeResolution::ConfirmedNotApplied { error } => {
                step.phase = "retry_scheduled".to_string();
                step.next_attempt_at_unix_ms = Some(
                    now.saturating_add(
                        error
                            .retry_after_ms
                            .unwrap_or_default()
                            .min(i64::MAX as u64) as i64,
                    ),
                );
                step.error = Some((&error).into());
                status.phase = "retry_scheduled".to_string();
            }
            UnknownOutcomeResolution::ConfirmedFailed { error } => {
                step.phase = "failed".to_string();
                step.error = Some((&error).into());
                status.phase = "failed".to_string();
            }
        }
        let name = resource.name_any();
        let persisted = self
            .resources
            .replace_operation_status(&name, &resource)
            .await
            .map_err(Self::adapter_error)?;
        Self::core(&persisted)
    }
}

pub fn cloud_operation_resource_name(idempotency_key: &str) -> String {
    format!("cloudop-{}", digest(idempotency_key))
}

fn operation_id_for_key(idempotency_key: &str) -> String {
    format!("op-{}", digest(idempotency_key))
}

fn resource_name_for_operation(operation_id: &OperationId) -> CoreResult<String> {
    operation_id
        .as_str()
        .strip_prefix("op-")
        .filter(|suffix| {
            suffix.len() == 32 && suffix.chars().all(|value| value.is_ascii_hexdigit())
        })
        .map(|suffix| format!("cloudop-{suffix}"))
        .ok_or_else(|| CoreError::NotFound(format!("cloud operation {operation_id}")))
}

fn resource_key(operation: &CloudOperation) -> String {
    format!(
        "{}:{}",
        serde_json::to_value(operation.resource_kind)
            .expect("serializable resource kind")
            .as_str()
            .expect("resource kind string"),
        operation.resource_id
    )
}

fn resource_lease_name(resource_key: &str) -> String {
    format!("cloudres-{}", digest(resource_key))
}

fn digest(value: &str) -> String {
    let encoded = hex::encode(Sha256::digest(value.as_bytes()));
    encoded[..32].to_string()
}

fn validate_claim_config(holder: &str, duration_ms: u64) -> CoreResult<()> {
    if holder.trim().is_empty()
        || holder.len() > 253
        || holder.chars().any(char::is_control)
        || duration_ms == 0
        || duration_ms > i32::MAX as u64 * 1000
    {
        return Err(CoreError::Conflict(
            "Kubernetes cloud operation lease configuration is invalid".to_string(),
        ));
    }
    Ok(())
}

fn duration_seconds(duration_ms: u64) -> CoreResult<i32> {
    let seconds = duration_ms.div_ceil(1000).max(1);
    i32::try_from(seconds)
        .map_err(|_| CoreError::Conflict("cloud Lease duration is too large".to_string()))
}

fn datetime(unix_ms: i64) -> CoreResult<DateTime<Utc>> {
    Utc.timestamp_millis_opt(unix_ms)
        .single()
        .ok_or_else(|| CoreError::Conflict("cloud operation timestamp is invalid".to_string()))
}

#[allow(clippy::too_many_arguments)]
fn resource_lease(
    name: &str,
    current: Option<&Lease>,
    resource_key: &str,
    operation_id: &str,
    holder: &str,
    token: &str,
    epoch: u64,
    now: i64,
    duration_ms: u64,
) -> CoreResult<Lease> {
    let mut annotations = current
        .and_then(|lease| lease.metadata.annotations.clone())
        .unwrap_or_default();
    annotations.insert(
        RESOURCE_KEY_ANNOTATION.to_string(),
        resource_key.to_string(),
    );
    annotations.insert(OPERATION_ANNOTATION.to_string(), operation_id.to_string());
    annotations.insert(TOKEN_ANNOTATION.to_string(), token.to_string());
    annotations.insert(EPOCH_ANNOTATION.to_string(), epoch.to_string());
    Ok(Lease {
        metadata: kube::api::ObjectMeta {
            name: Some(name.to_string()),
            namespace: current.and_then(|lease| lease.metadata.namespace.clone()),
            resource_version: current.and_then(|lease| lease.metadata.resource_version.clone()),
            annotations: Some(annotations),
            ..Default::default()
        },
        spec: Some(LeaseSpec {
            holder_identity: Some(holder.to_string()),
            lease_duration_seconds: Some(duration_seconds(duration_ms)?),
            acquire_time: Some(MicroTime(datetime(now)?)),
            renew_time: Some(MicroTime(datetime(now)?)),
            lease_transitions: Some(
                current
                    .and_then(|lease| lease.spec.as_ref())
                    .and_then(|spec| spec.lease_transitions)
                    .unwrap_or_default()
                    .saturating_add(1),
            ),
            ..Default::default()
        }),
    })
}

fn verify_resource_key(lease: &Lease, expected: &str) -> CoreResult<()> {
    match lease
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(RESOURCE_KEY_ANNOTATION))
    {
        Some(key) if key == expected => Ok(()),
        Some(key) => Err(CoreError::Conflict(format!(
            "cloud Lease name collision between {key:?} and {expected:?}"
        ))),
        None => Err(CoreError::Conflict(
            "managed cloud resource Lease omitted its resource key".to_string(),
        )),
    }
}

fn lease_epoch(lease: &Lease) -> CoreResult<u64> {
    lease
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(EPOCH_ANNOTATION))
        .map_or(Ok(0), |value| {
            value.parse().map_err(|_| {
                CoreError::Conflict("cloud resource Lease has invalid fencing epoch".to_string())
            })
        })
}

fn resource_lease_matches(lease: &Lease, expected: &OperationLease) -> CoreResult<bool> {
    let annotations = lease.metadata.annotations.as_ref();
    Ok(lease
        .spec
        .as_ref()
        .and_then(|spec| spec.holder_identity.as_deref())
        == Some(expected.holder.as_str())
        && annotations.and_then(|values| values.get(TOKEN_ANNOTATION).map(String::as_str))
            == Some(expected.fencing_token.as_str())
        && annotations.and_then(|values| values.get(OPERATION_ANNOTATION).map(String::as_str))
            == Some(expected.operation_id.as_str())
        && lease_epoch(lease)? == expected.fencing_epoch)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicI64, AtomicU64, Ordering},
            Mutex,
        },
    };

    use edgion_center_core::{
        CloudOperationAction, CloudResourceId, CloudResourceKind, IdempotencyKey,
        NewCloudOperationStep,
    };

    use super::*;

    #[derive(Default)]
    struct MemoryResources {
        operations: Mutex<HashMap<String, EdgionCloudOperation>>,
        leases: Mutex<HashMap<String, Lease>>,
        revision: AtomicU64,
        fail_status_replaces: AtomicU64,
        steal_lease_after_status_replaces: AtomicU64,
    }

    impl MemoryResources {
        fn next_revision(&self) -> String {
            self.revision.fetch_add(1, Ordering::SeqCst).to_string()
        }

        fn replace<T: Clone>(
            &self,
            values: &Mutex<HashMap<String, T>>,
            name: &str,
            expected_revision: Option<&str>,
            mut value: T,
            set_revision: impl Fn(&mut T, String),
            current_revision: impl Fn(&T) -> Option<&str>,
        ) -> Result<T, ResourceError> {
            let mut values = values.lock().unwrap();
            let Some(current) = values.get(name) else {
                return Err(ResourceError::Conflict);
            };
            if current_revision(current) != expected_revision {
                return Err(ResourceError::Conflict);
            }
            set_revision(&mut value, self.next_revision());
            values.insert(name.to_string(), value.clone());
            Ok(value)
        }
    }

    #[async_trait]
    impl OperationResources for MemoryResources {
        async fn get_operation(
            &self,
            name: &str,
        ) -> Result<Option<EdgionCloudOperation>, ResourceError> {
            Ok(self.operations.lock().unwrap().get(name).cloned())
        }

        async fn list_operations(&self) -> Result<Vec<EdgionCloudOperation>, ResourceError> {
            Ok(self.operations.lock().unwrap().values().cloned().collect())
        }

        async fn create_operation(
            &self,
            operation: &EdgionCloudOperation,
        ) -> Result<EdgionCloudOperation, ResourceError> {
            let name = operation.name_any();
            let mut operations = self.operations.lock().unwrap();
            if operations.contains_key(&name) {
                return Err(ResourceError::Conflict);
            }
            let mut operation = operation.clone();
            operation.metadata.resource_version = Some(self.next_revision());
            operations.insert(name, operation.clone());
            Ok(operation)
        }

        async fn replace_operation_status(
            &self,
            name: &str,
            operation: &EdgionCloudOperation,
        ) -> Result<EdgionCloudOperation, ResourceError> {
            if self
                .fail_status_replaces
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(ResourceError::Other(
                    "injected status persistence failure".to_string(),
                ));
            }
            let replaced = self.replace(
                &self.operations,
                name,
                operation.metadata.resource_version.as_deref(),
                operation.clone(),
                |value, revision| value.metadata.resource_version = Some(revision),
                |value| value.metadata.resource_version.as_deref(),
            )?;
            if self
                .steal_lease_after_status_replaces
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                if let Some(lease) = self.leases.lock().unwrap().values_mut().next() {
                    lease
                        .metadata
                        .annotations
                        .get_or_insert_default()
                        .insert(TOKEN_ANNOTATION.to_string(), "stolen-token".to_string());
                    lease.metadata.resource_version = Some(self.next_revision());
                }
            }
            Ok(replaced)
        }

        async fn get_lease(&self, name: &str) -> Result<Option<Lease>, ResourceError> {
            Ok(self.leases.lock().unwrap().get(name).cloned())
        }

        async fn create_lease(&self, lease: &Lease) -> Result<Lease, ResourceError> {
            let name = lease.name_any();
            let mut leases = self.leases.lock().unwrap();
            if leases.contains_key(&name) {
                return Err(ResourceError::Conflict);
            }
            let mut lease = lease.clone();
            lease.metadata.resource_version = Some(self.next_revision());
            leases.insert(name, lease.clone());
            Ok(lease)
        }

        async fn replace_lease(&self, name: &str, lease: &Lease) -> Result<Lease, ResourceError> {
            self.replace(
                &self.leases,
                name,
                lease.metadata.resource_version.as_deref(),
                lease.clone(),
                |value, revision| value.metadata.resource_version = Some(revision),
                |value| value.metadata.resource_version.as_deref(),
            )
        }
    }

    struct FixedClock(AtomicI64);

    impl FixedClock {
        fn advance(&self, millis: i64) {
            self.0.fetch_add(millis, Ordering::SeqCst);
        }
    }

    impl Clock for FixedClock {
        fn now_unix_ms(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }

        fn monotonic_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst).max(0) as u64
        }
    }

    fn request(key: &str, resource_id: &str) -> NewCloudOperation {
        NewCloudOperation {
            idempotency_key: IdempotencyKey::new(key).unwrap(),
            resource_id: CloudResourceId::new(resource_id).unwrap(),
            resource_kind: CloudResourceKind::ManagedZone,
            action: CloudOperationAction::Reconcile,
            desired_generation: 1,
            requested_by: "operator".to_string(),
            deadline_unix_ms: None,
            steps: vec![NewCloudOperationStep {
                name: "apply-zone".to_string(),
                purpose: CloudOperationStepPurpose::Apply,
                idempotency_key: IdempotencyKey::new(format!("{key}/apply-zone")).unwrap(),
            }],
        }
    }

    fn dispatch_policy() -> DispatchPolicy {
        DispatchPolicy {
            max_execution_ms: 500,
            completion_margin_ms: 100,
        }
    }

    fn store() -> (
        KubernetesOperationStore,
        Arc<MemoryResources>,
        Arc<FixedClock>,
    ) {
        let resources = Arc::new(MemoryResources::default());
        let clock = Arc::new(FixedClock(AtomicI64::new(1_700_000_000_000)));
        (
            KubernetesOperationStore::with_resources(resources.clone(), clock.clone()),
            resources,
            clock,
        )
    }

    #[test]
    fn names_are_bounded_deterministic_and_collision_guarded_by_full_keys() {
        let first = cloud_operation_resource_name("Tenant A/request/1");
        assert_eq!(first, cloud_operation_resource_name("Tenant A/request/1"));
        assert_ne!(first, cloud_operation_resource_name("tenant-a-request-1"));
        assert!(first.len() <= 63);
        assert!(resource_lease_name(&"x".repeat(10_000)).len() <= 63);
    }

    #[tokio::test]
    async fn enqueue_deduplicates_exact_request_and_rejects_key_reuse() {
        let (store, _, _) = store();
        let original = request("request-1", "zone-1");
        let created = store.enqueue(original.clone()).await.unwrap();
        assert!(matches!(created, EnqueueOperationResult::Created(_)));
        let existing = store.enqueue(original).await.unwrap();
        assert!(matches!(existing, EnqueueOperationResult::Existing(_)));

        let error = store
            .enqueue(request("request-1", "different-zone"))
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::Conflict(_)));
    }

    #[tokio::test]
    async fn enqueue_and_claim_recover_create_status_crash() {
        let (store, resources, _) = store();
        let request = request("status-crash", "zone-status");
        let operation = match store.enqueue(request.clone()).await.unwrap() {
            EnqueueOperationResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        let name = cloud_operation_resource_name(request.idempotency_key.as_str());
        resources
            .operations
            .lock()
            .unwrap()
            .get_mut(&name)
            .unwrap()
            .status = None;

        assert!(matches!(
            store.enqueue(request).await.unwrap(),
            EnqueueOperationResult::Existing(_)
        ));
        assert!(store.get_operation(&operation.id).await.unwrap().is_some());

        resources
            .operations
            .lock()
            .unwrap()
            .get_mut(&name)
            .unwrap()
            .status = None;
        let claim = store.claim_ready("worker-status", 10_000).await.unwrap();
        assert!(claim.is_some());
    }

    #[tokio::test]
    async fn claim_begin_and_complete_require_exact_operation_and_resource_fences() {
        let (store, _, _) = store();
        let operation = match store.enqueue(request("request-2", "zone-2")).await.unwrap() {
            EnqueueOperationResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        let claim = store
            .claim_ready("center-0/uid-0/worker-0", 10_000)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claim.operation.id, operation.id);
        let dispatched = store
            .begin_step(&claim.lease, dispatch_policy())
            .await
            .unwrap();
        assert_eq!(dispatched.step.phase, CloudOperationStepPhase::Running);
        assert_eq!(dispatched.step.attempt, 1);

        let mut stale = dispatched.clone();
        stale.lease.fencing_token = "stale-token".to_string();
        assert!(matches!(
            store
                .complete_step(&stale, StepCompletion::Succeeded { summary: None })
                .await,
            Err(CoreError::Conflict(_))
        ));

        let completion = StepCompletion::Succeeded {
            summary: Some("zone applied".to_string()),
        };
        let completed = store
            .complete_step(&dispatched, completion.clone())
            .await
            .unwrap();
        assert_eq!(completed.phase, CloudOperationPhase::Succeeded);
        assert_eq!(
            store.complete_step(&dispatched, completion).await.unwrap(),
            completed
        );
        assert!(store.claim_ready("other", 10_000).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn expired_running_step_becomes_unknown_and_is_never_replayed() {
        let (store, _, clock) = store();
        let operation = match store.enqueue(request("request-3", "zone-3")).await.unwrap() {
            EnqueueOperationResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        let claim = store.claim_ready("worker-a", 1_000).await.unwrap().unwrap();
        let dispatched = store
            .begin_step(&claim.lease, dispatch_policy())
            .await
            .unwrap();
        clock.advance(1_001);

        assert!(store
            .claim_ready("worker-b", 1_000)
            .await
            .unwrap()
            .is_none());
        let recovered = store.get_operation(&operation.id).await.unwrap().unwrap();
        assert_eq!(recovered.phase, CloudOperationPhase::UnknownOutcome);
        assert_eq!(
            recovered.steps[0].phase,
            CloudOperationStepPhase::UnknownOutcome
        );
        assert_eq!(
            recovered.steps[0].execution_token,
            Some(dispatched.execution_token.clone())
        );
        assert!(matches!(
            store
                .complete_step(&dispatched, StepCompletion::Succeeded { summary: None })
                .await,
            Err(CoreError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn oldest_operation_serializes_mutations_for_the_same_resource() {
        let (store, _, _) = store();
        store
            .enqueue(request("first", "shared-zone"))
            .await
            .unwrap();
        store
            .enqueue(request("second", "shared-zone"))
            .await
            .unwrap();
        let first = store
            .claim_ready("worker-a", 10_000)
            .await
            .unwrap()
            .unwrap();
        assert!(store
            .claim_ready("worker-b", 10_000)
            .await
            .unwrap()
            .is_none());
        let dispatched = store
            .begin_step(&first.lease, dispatch_policy())
            .await
            .unwrap();
        store
            .complete_step(&dispatched, StepCompletion::Succeeded { summary: None })
            .await
            .unwrap();
        let second = store
            .claim_ready("worker-b", 10_000)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first.operation.id, second.operation.id);
    }

    #[tokio::test]
    async fn malformed_operation_blocks_only_its_own_resource() {
        let (store, resources, clock) = store();
        let malformed = request("malformed", "blocked-zone");
        store.enqueue(malformed.clone()).await.unwrap();
        clock.advance(1);
        store
            .enqueue(request("same-resource", "blocked-zone"))
            .await
            .unwrap();
        clock.advance(1);
        store
            .enqueue(request("healthy", "healthy-zone"))
            .await
            .unwrap();
        let name = cloud_operation_resource_name(malformed.idempotency_key.as_str());
        resources
            .operations
            .lock()
            .unwrap()
            .get_mut(&name)
            .unwrap()
            .status
            .as_mut()
            .unwrap()
            .phase = "forged_phase".to_string();

        let claim = store
            .claim_ready("worker-healthy", 10_000)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claim.operation.resource_id.as_str(), "healthy-zone");
    }

    #[tokio::test]
    async fn deadline_is_persisted_before_dispatch_and_releases_the_resource() {
        let (store, _, clock) = store();
        let mut expired = request("expired", "deadline-zone");
        expired.deadline_unix_ms = Some(clock.now_unix_ms());
        let operation = match store.enqueue(expired).await.unwrap() {
            EnqueueOperationResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        assert!(store
            .claim_ready("worker-a", 10_000)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_operation(&operation.id)
                .await
                .unwrap()
                .unwrap()
                .phase,
            CloudOperationPhase::Failed
        );

        let mut boundary = request("deadline-boundary", "boundary-zone");
        boundary.deadline_unix_ms = Some(clock.now_unix_ms() + 100);
        let boundary = match store.enqueue(boundary).await.unwrap() {
            EnqueueOperationResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        let claim = store
            .claim_ready("worker-b", 10_000)
            .await
            .unwrap()
            .unwrap();
        clock.advance(100);
        assert!(matches!(
            store.begin_step(&claim.lease, dispatch_policy()).await,
            Err(CoreError::Conflict(_))
        ));
        assert_eq!(
            store
                .get_operation(&boundary.id)
                .await
                .unwrap()
                .unwrap()
                .phase,
            CloudOperationPhase::Failed
        );
    }

    #[tokio::test]
    async fn begin_refuses_short_lease_and_returns_operation_to_the_queue() {
        let (store, _, clock) = store();
        store
            .enqueue(request("short-lease", "short-zone"))
            .await
            .unwrap();
        let claim = store.claim_ready("worker-a", 150).await.unwrap().unwrap();
        clock.advance(50);
        assert!(matches!(
            store.begin_step(&claim.lease, dispatch_policy()).await,
            Err(CoreError::Conflict(_))
        ));
        let reclaimed = store.claim_ready("worker-b", 1_000).await.unwrap().unwrap();
        assert!(store
            .begin_step(&reclaimed.lease, dispatch_policy())
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn claim_status_failure_best_effort_releases_exact_resource_lease() {
        let (store, resources, _) = store();
        store
            .enqueue(request("claim-failure", "failure-zone"))
            .await
            .unwrap();
        resources.fail_status_replaces.store(1, Ordering::SeqCst);
        assert!(matches!(
            store.claim_ready("worker-a", 10_000).await,
            Err(CoreError::Adapter(_))
        ));
        let leases = resources.leases.lock().unwrap();
        let lease = leases.values().next().unwrap();
        assert!(lease
            .spec
            .as_ref()
            .and_then(|spec| spec.holder_identity.as_deref())
            .is_none());
        assert!(lease
            .metadata
            .annotations
            .as_ref()
            .is_none_or(|values| !values.contains_key(TOKEN_ANNOTATION)));
    }

    #[tokio::test]
    async fn begin_rechecks_resource_fence_after_running_status_cas() {
        let (store, resources, _) = store();
        let operation = match store
            .enqueue(request("begin-race", "begin-race-zone"))
            .await
            .unwrap()
        {
            EnqueueOperationResult::Created(operation) => operation,
            _ => unreachable!(),
        };
        let claim = store
            .claim_ready("worker-a", 10_000)
            .await
            .unwrap()
            .unwrap();
        resources
            .steal_lease_after_status_replaces
            .store(1, Ordering::SeqCst);

        assert!(matches!(
            store.begin_step(&claim.lease, dispatch_policy()).await,
            Err(CoreError::Conflict(_))
        ));
        let persisted = store.get_operation(&operation.id).await.unwrap().unwrap();
        assert_eq!(persisted.phase, CloudOperationPhase::Running);
        assert_eq!(persisted.steps[0].phase, CloudOperationStepPhase::Running);
    }

    #[tokio::test]
    async fn recovered_unknown_status_failure_releases_takeover_lease() {
        let (store, resources, clock) = store();
        store
            .enqueue(request("recovery-failure", "recovery-zone"))
            .await
            .unwrap();
        let claim = store.claim_ready("worker-a", 1_000).await.unwrap().unwrap();
        store
            .begin_step(&claim.lease, dispatch_policy())
            .await
            .unwrap();
        clock.advance(1_001);
        resources.fail_status_replaces.store(1, Ordering::SeqCst);

        assert!(matches!(
            store.claim_ready("worker-b", 1_000).await,
            Err(CoreError::Adapter(_))
        ));
        let leases = resources.leases.lock().unwrap();
        assert!(leases
            .values()
            .next()
            .unwrap()
            .spec
            .as_ref()
            .and_then(|spec| spec.holder_identity.as_deref())
            .is_none());
    }

    #[tokio::test]
    async fn renew_status_failure_releases_the_renewed_resource_fence() {
        let (store, resources, _) = store();
        store
            .enqueue(request("renew-failure", "renew-zone"))
            .await
            .unwrap();
        let claim = store
            .claim_ready("worker-a", 10_000)
            .await
            .unwrap()
            .unwrap();
        resources.fail_status_replaces.store(1, Ordering::SeqCst);

        assert!(matches!(
            store.renew_lease(&claim.lease, 10_000).await,
            Err(CoreError::Adapter(_))
        ));
        let leases = resources.leases.lock().unwrap();
        assert!(leases
            .values()
            .next()
            .unwrap()
            .spec
            .as_ref()
            .and_then(|spec| spec.holder_identity.as_deref())
            .is_none());
    }
}
