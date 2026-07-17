use edgion_center_core::{
    CloudOperation, CloudOperationStep, CloudResourceId, IdempotencyKey, NewCloudOperation,
    NewCloudOperationStep, OperationError, OperationId, OperationLease,
};
use kube::{CustomResource, KubeSchema};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, KubeSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[kube(
    group = "center.edgion.io",
    version = "v1alpha1",
    kind = "EdgionCloudOperation",
    plural = "edgioncloudoperations",
    namespaced,
    status = "EdgionCloudOperationStatus",
    shortname = "eco"
)]
#[serde(rename_all = "camelCase")]
pub struct EdgionCloudOperationSpec {
    #[x_kube(validation = "self != ''", validation = "self == oldSelf")]
    #[schemars(length(max = 128))]
    pub operation_id: String,
    #[x_kube(validation = "self != ''", validation = "self == oldSelf")]
    #[schemars(length(max = 512))]
    pub idempotency_key: String,
    #[x_kube(validation = "self != ''", validation = "self == oldSelf")]
    #[schemars(length(max = 512))]
    pub resource_id: String,
    #[x_kube(validation = "self == oldSelf")]
    pub resource_kind: String,
    #[x_kube(validation = "self == oldSelf")]
    pub action: String,
    #[x_kube(validation = "self > 0", validation = "self == oldSelf")]
    pub desired_generation: i64,
    #[x_kube(validation = "self != ''", validation = "self == oldSelf")]
    #[schemars(length(max = 512))]
    pub requested_by: String,
    #[x_kube(validation = "self == oldSelf")]
    pub deadline_unix_ms: Option<i64>,
    #[x_kube(validation = "self == oldSelf")]
    #[schemars(length(min = 1, max = 128))]
    pub steps: Vec<EdgionCloudOperationStepSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgionCloudOperationStepSpec {
    #[schemars(length(min = 1, max = 128))]
    pub name: String,
    pub purpose: String,
    #[schemars(length(min = 1, max = 512))]
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgionCloudOperationStatus {
    pub phase: String,
    pub cancel_requested: bool,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
    pub steps: Vec<EdgionCloudOperationStepStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<EdgionCloudOperationLeaseStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgionCloudOperationStepStatus {
    pub phase: String,
    pub attempt: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_attempt_at_unix_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_unix_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_unix_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<EdgionCloudOperationErrorStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgionCloudOperationErrorStatus {
    pub kind: String,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgionCloudOperationLeaseStatus {
    pub holder: String,
    pub fencing_token: String,
    pub fencing_epoch: i64,
    pub valid_until_unix_ms: i64,
}

impl EdgionCloudOperationSpec {
    pub(crate) fn from_request(operation_id: String, request: &NewCloudOperation) -> Self {
        Self {
            operation_id,
            idempotency_key: request.idempotency_key.to_string(),
            resource_id: request.resource_id.to_string(),
            resource_kind: json_scalar(&request.resource_kind),
            action: json_scalar(&request.action),
            desired_generation: i64::try_from(request.desired_generation)
                .expect("validated cloud operation generation"),
            requested_by: request.requested_by.clone(),
            deadline_unix_ms: request.deadline_unix_ms,
            steps: request
                .steps
                .iter()
                .map(|step| EdgionCloudOperationStepSpec {
                    name: step.name.clone(),
                    purpose: json_scalar(&step.purpose),
                    idempotency_key: step.idempotency_key.to_string(),
                })
                .collect(),
        }
    }

    pub(crate) fn to_request(&self) -> Result<NewCloudOperation, String> {
        Ok(NewCloudOperation {
            idempotency_key: IdempotencyKey::new(self.idempotency_key.clone())
                .map_err(|error| error.to_string())?,
            resource_id: CloudResourceId::new(self.resource_id.clone())
                .map_err(|error| error.to_string())?,
            resource_kind: parse_scalar(&self.resource_kind)?,
            action: parse_scalar(&self.action)?,
            desired_generation: u64::try_from(self.desired_generation)
                .map_err(|_| "cloud operation generation is negative".to_string())?,
            requested_by: self.requested_by.clone(),
            deadline_unix_ms: self.deadline_unix_ms,
            steps: self
                .steps
                .iter()
                .map(|step| {
                    Ok(NewCloudOperationStep {
                        name: step.name.clone(),
                        purpose: parse_scalar(&step.purpose)?,
                        idempotency_key: IdempotencyKey::new(step.idempotency_key.clone())
                            .map_err(|error| error.to_string())?,
                    })
                })
                .collect::<Result<_, String>>()?,
        })
    }
}

impl EdgionCloudOperationStatus {
    pub(crate) fn new(request: &NewCloudOperation, now: i64) -> Self {
        Self {
            phase: "pending".to_string(),
            cancel_requested: false,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
            steps: request
                .steps
                .iter()
                .map(|_| EdgionCloudOperationStepStatus {
                    phase: "pending".to_string(),
                    attempt: 0,
                    execution_token: None,
                    next_attempt_at_unix_ms: None,
                    started_at_unix_ms: None,
                    finished_at_unix_ms: None,
                    summary: None,
                    error: None,
                })
                .collect(),
            lease: None,
        }
    }
}

impl EdgionCloudOperation {
    pub(crate) fn to_core(&self) -> Result<CloudOperation, String> {
        let status = self
            .status
            .as_ref()
            .ok_or("cloud operation omitted status")?;
        if self.spec.steps.len() != status.steps.len() {
            return Err("cloud operation spec/status step count differs".to_string());
        }
        Ok(CloudOperation {
            id: OperationId::new(self.spec.operation_id.clone())
                .map_err(|error| error.to_string())?,
            idempotency_key: IdempotencyKey::new(self.spec.idempotency_key.clone())
                .map_err(|error| error.to_string())?,
            resource_id: CloudResourceId::new(self.spec.resource_id.clone())
                .map_err(|error| error.to_string())?,
            resource_kind: parse_scalar(&self.spec.resource_kind)?,
            action: parse_scalar(&self.spec.action)?,
            desired_generation: u64::try_from(self.spec.desired_generation)
                .map_err(|_| "cloud operation generation is negative".to_string())?,
            requested_by: self.spec.requested_by.clone(),
            phase: parse_scalar(&status.phase)?,
            cancel_requested: status.cancel_requested,
            created_at_unix_ms: status.created_at_unix_ms,
            updated_at_unix_ms: status.updated_at_unix_ms,
            deadline_unix_ms: self.spec.deadline_unix_ms,
            steps: self
                .spec
                .steps
                .iter()
                .zip(&status.steps)
                .map(|(spec, status)| {
                    Ok(CloudOperationStep {
                        name: spec.name.clone(),
                        purpose: parse_scalar(&spec.purpose)?,
                        idempotency_key: IdempotencyKey::new(spec.idempotency_key.clone())
                            .map_err(|error| error.to_string())?,
                        phase: parse_scalar(&status.phase)?,
                        attempt: u32::try_from(status.attempt)
                            .map_err(|_| "cloud operation attempt is negative".to_string())?,
                        execution_token: status.execution_token.clone(),
                        next_attempt_at_unix_ms: status.next_attempt_at_unix_ms,
                        started_at_unix_ms: status.started_at_unix_ms,
                        finished_at_unix_ms: status.finished_at_unix_ms,
                        summary: status.summary.clone(),
                        error: status.error.as_ref().map(to_core_error).transpose()?,
                    })
                })
                .collect::<Result<_, String>>()?,
        })
    }

    pub(crate) fn lease(&self) -> Result<Option<OperationLease>, String> {
        self.status
            .as_ref()
            .and_then(|status| status.lease.as_ref())
            .map(|lease| {
                Ok(OperationLease {
                    operation_id: OperationId::new(self.spec.operation_id.clone())
                        .map_err(|error| error.to_string())?,
                    holder: lease.holder.clone(),
                    fencing_token: lease.fencing_token.clone(),
                    fencing_epoch: u64::try_from(lease.fencing_epoch)
                        .map_err(|_| "cloud fencing epoch is negative".to_string())?,
                    valid_until_unix_ms: lease.valid_until_unix_ms,
                })
            })
            .transpose()
    }
}

impl From<&OperationLease> for EdgionCloudOperationLeaseStatus {
    fn from(lease: &OperationLease) -> Self {
        Self {
            holder: lease.holder.clone(),
            fencing_token: lease.fencing_token.clone(),
            fencing_epoch: i64::try_from(lease.fencing_epoch)
                .expect("cloud fencing epoch fits Kubernetes int64"),
            valid_until_unix_ms: lease.valid_until_unix_ms,
        }
    }
}

impl From<&OperationError> for EdgionCloudOperationErrorStatus {
    fn from(error: &OperationError) -> Self {
        Self {
            kind: json_scalar(&error.kind),
            code: error.code.clone(),
            message: error.message.clone(),
            retry_after_ms: error
                .retry_after_ms
                .map(|value| i64::try_from(value).expect("validated retry hint")),
        }
    }
}

fn to_core_error(error: &EdgionCloudOperationErrorStatus) -> Result<OperationError, String> {
    Ok(OperationError {
        kind: parse_scalar(&error.kind)?,
        code: error.code.clone(),
        message: error.message.clone(),
        retry_after_ms: error
            .retry_after_ms
            .map(|value| {
                u64::try_from(value).map_err(|_| "cloud retry hint is negative".to_string())
            })
            .transpose()?,
    })
}

fn json_scalar<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .expect("serializable cloud operation enum")
        .as_str()
        .expect("cloud operation enum serializes as string")
        .to_string()
}

fn parse_scalar<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use kube::CustomResourceExt;

    use super::*;

    #[test]
    fn generated_crd_is_namespaced_immutable_and_has_status_subresource() {
        let crd = EdgionCloudOperation::crd();
        assert_eq!(crd.spec.scope, "Namespaced");
        let version = &crd.spec.versions[0];
        assert!(version
            .subresources
            .as_ref()
            .and_then(|subresources| subresources.status.as_ref())
            .is_some());
        let schema = serde_json::to_value(crd).unwrap();
        let validations = schema
            .pointer("/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/operationId/x-kubernetes-validations")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(validations
            .iter()
            .any(|validation| validation["rule"] == "self == oldSelf"));
        assert_eq!(
            schema.pointer(
                "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/steps/maxItems"
            ),
            Some(&serde_json::json!(128))
        );
    }

    #[test]
    fn checked_in_manifest_matches_generated_critical_schema_constraints() {
        let generated = serde_json::to_value(EdgionCloudOperation::crd()).unwrap();
        let checked_in: serde_json::Value = serde_yaml::from_str(include_str!(
            "../../../cicd/deploy/center-kubernetes/cloud-operation-crd.yaml"
        ))
        .unwrap();
        for pointer in [
            "/spec/group",
            "/spec/scope",
            "/spec/versions/0/subresources/status",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/operationId/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/idempotencyKey/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/resourceId/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/desiredGeneration/format",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/steps/maxItems",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/steps/items/properties/attempt/format",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/steps/items/properties/error/properties/retryAfterMs/format",
        ] {
            assert_eq!(
                checked_in.pointer(pointer),
                generated.pointer(pointer),
                "CRD schema drift at {pointer}"
            );
        }
    }
}
