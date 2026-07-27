//! Platform-neutral planning primitives for Center-owned GlobalResources.
//!
//! This module deliberately stops at a deterministic plan. Transport reads,
//! authorization, fencing, persistence, and mutations belong to later layers.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    ControllerPhase, ControllerRecord, CoreError, CoreResult, GlobalResource, GlobalResourceId,
    GlobalResourceRevision, GlobalResourceTargetSelector, GlobalResourcesConfig, OwnershipFence,
};

pub const GLOBAL_RESOURCE_MANAGED_BY_LABEL: &str = "edgion.io/managed-by";
pub const GLOBAL_RESOURCE_ID_ANNOTATION: &str = "center.edgion.io/global-resource-id";
pub const GLOBAL_RESOURCE_REVISION_ANNOTATION: &str = "center.edgion.io/desired-revision";
pub const GLOBAL_RESOURCE_MANAGED_BY_VALUE: &str = "center";
pub const GLOBAL_RESOURCE_PLAN_SCHEMA_VERSION: u8 = 1;
pub const MAX_GLOBAL_RESOURCE_CHANGED_PATHS: usize = 64;
pub const MAX_GLOBAL_RESOURCE_CHANGED_PATH_BYTES: usize = 512;

const MAX_MEMBERSHIP_REVISION_BYTES: usize = 128;
const MAX_OBSERVED_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_RESOURCE_VERSION_BYTES: usize = 256;
const CLUSTER_OWNED_METADATA_FIELDS: [&str; 5] = [
    "resourceVersion",
    "uid",
    "generation",
    "creationTimestamp",
    "managedFields",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalResourceOwnership {
    Unowned,
    Owned {
        desired_revision: String,
        current_revision: bool,
    },
    Foreign,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalResourceTargetResolutionState {
    Available,
    Offline,
    Ambiguous,
    Indeterminate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlobalResourceResolvedTarget {
    pub cluster: String,
    pub state: GlobalResourceTargetResolutionState,
    pub controller_id: Option<String>,
    pub controller_session_id: Option<String>,
    pub ownership_fence: Option<OwnershipFence>,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GlobalResourceTargetObservation {
    Missing,
    Present(Value),
    Denied,
    Indeterminate,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalResourcePlanState {
    Create,
    Update,
    Noop,
    Conflict,
    Denied,
    Offline,
    Ambiguous,
    Indeterminate,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalResourcePlanReason {
    Missing,
    Drift,
    InSync,
    OwnershipUnowned,
    OwnershipForeign,
    OwnershipInvalid,
    AccessDenied,
    ControllerOffline,
    ControllerAmbiguous,
    MembershipIndeterminate,
    ObservationIndeterminate,
    ObservationInvalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlobalResourceTargetPlan {
    pub cluster: String,
    pub controller_id: Option<String>,
    pub controller_session_id: Option<String>,
    pub ownership_fence: Option<OwnershipFence>,
    pub state: GlobalResourcePlanState,
    pub reason: GlobalResourcePlanReason,
    pub observed_resource_version: Option<String>,
    pub desired_normalized_revision: String,
    pub observed_normalized_revision: Option<String>,
    pub changed_paths: Vec<String>,
    pub changed_paths_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlobalResourcePlanBinding {
    pub global_resource_id: GlobalResourceId,
    pub generation: u64,
    pub desired_revision: GlobalResourceRevision,
    pub ordered_targets: Vec<String>,
    pub membership_revision: String,
    pub target_evidence_revision: String,
    pub plan_revision: String,
}

impl GlobalResourcePlanBinding {
    /// Revalidates every apply fence against current state. Callers must pass
    /// the exact byte-ordered cluster set from the plan, not a set reconstructed
    /// from successful targets only.
    pub fn validate_exact(
        &self,
        resource: &GlobalResource,
        config: &GlobalResourcesConfig,
        target_plans: &[GlobalResourceTargetPlan],
        membership_revision: &str,
    ) -> CoreResult<()> {
        resource.validate(config)?;
        validate_ordered_target_plans(target_plans)?;
        validate_membership_revision(membership_revision)?;
        let ordered_targets = target_plans
            .iter()
            .map(|target| target.cluster.clone())
            .collect::<Vec<_>>();
        let target_evidence_revision = target_evidence_revision(target_plans)?;
        if self.global_resource_id != resource.id
            || self.generation != resource.generation
            || self.desired_revision != resource.desired_revision
            || self.ordered_targets != ordered_targets
            || self.membership_revision != membership_revision
            || self.target_evidence_revision != target_evidence_revision
            || self.plan_revision
                != plan_binding_revision(
                    resource,
                    &ordered_targets,
                    membership_revision,
                    &target_evidence_revision,
                )?
        {
            return Err(CoreError::Conflict(
                "global resource plan binding no longer matches exact apply inputs".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlobalResourcePlan {
    pub binding: GlobalResourcePlanBinding,
    pub targets: Vec<GlobalResourceTargetPlan>,
}

/// Injects exact Center ownership provenance and returns a canonical normalized
/// desired document. User-supplied ownership markers are rejected, even when
/// they happen to contain the expected values.
pub fn normalized_global_resource_desired(
    resource: &GlobalResource,
    config: &GlobalResourcesConfig,
) -> CoreResult<Value> {
    resource.validate(config)?;
    let mut document = resource.desired.template_document.clone();
    let metadata = root_metadata_mut(&mut document)?;
    validate_no_reserved_global_resource_provenance(metadata)?;
    object_field_mut(metadata, "labels")?.insert(
        GLOBAL_RESOURCE_MANAGED_BY_LABEL.to_string(),
        Value::String(GLOBAL_RESOURCE_MANAGED_BY_VALUE.to_string()),
    );
    let annotations = object_field_mut(metadata, "annotations")?;
    annotations.insert(
        GLOBAL_RESOURCE_ID_ANNOTATION.to_string(),
        Value::String(resource.id.to_string()),
    );
    annotations.insert(
        GLOBAL_RESOURCE_REVISION_ANNOTATION.to_string(),
        Value::String(resource.desired_revision.as_str().to_string()),
    );
    normalize_document(document)
}

/// Normalizes a fresh observed document for a safe operator-document diff.
/// Only the resource root `status` and cluster-owned fields in the resource
/// root `metadata` are ignored. Nested fields with the same names are retained.
pub fn normalized_global_resource_observed(
    document: &Value,
    resource: &GlobalResource,
    config: &GlobalResourcesConfig,
) -> CoreResult<Value> {
    resource.validate(config)?;
    let encoded = serde_json::to_vec(document)
        .map_err(|_| CoreError::Conflict("observed global resource is invalid".to_string()))?;
    if encoded.len() > MAX_OBSERVED_DOCUMENT_BYTES {
        return Err(CoreError::Conflict(
            "observed global resource exceeds its planning limit".to_string(),
        ));
    }
    validate_observed_envelope(document, resource)?;
    normalize_document(document.clone())
}

pub fn validate_global_resource_ownership(
    document: &Value,
    resource: &GlobalResource,
) -> GlobalResourceOwnership {
    let Some(metadata) = document.get("metadata").and_then(Value::as_object) else {
        return GlobalResourceOwnership::Invalid;
    };
    let raw_label = nested_value(metadata, "labels", GLOBAL_RESOURCE_MANAGED_BY_LABEL);
    let raw_owner_id = nested_value(metadata, "annotations", GLOBAL_RESOURCE_ID_ANNOTATION);
    let raw_revision = nested_value(metadata, "annotations", GLOBAL_RESOURCE_REVISION_ANNOTATION);
    if raw_label.is_none() && raw_owner_id.is_none() && raw_revision.is_none() {
        return GlobalResourceOwnership::Unowned;
    }
    let label = raw_label.and_then(Value::as_str);
    let owner_id = raw_owner_id.and_then(Value::as_str);
    let revision = raw_revision.and_then(Value::as_str);
    let (Some(label), Some(owner_id), Some(revision)) = (label, owner_id, revision) else {
        return GlobalResourceOwnership::Invalid;
    };
    if label != GLOBAL_RESOURCE_MANAGED_BY_VALUE || owner_id != resource.id.as_str() {
        return GlobalResourceOwnership::Foreign;
    }
    if !is_sha256_revision(revision) {
        return GlobalResourceOwnership::Invalid;
    }
    GlobalResourceOwnership::Owned {
        desired_revision: revision.to_string(),
        current_revision: revision == resource.desired_revision.as_str(),
    }
}

/// Resolves a durable selector against a Controller directory snapshot.
///
/// Metadata constraints use set containment: every requested environment and
/// every requested tag must be present (AND both within and across fields).
/// If records for one cluster disagree on whether constraints match, that
/// cluster is retained as indeterminate rather than silently omitted.
pub fn resolve_global_resource_targets(
    selector: &GlobalResourceTargetSelector,
    records: &[ControllerRecord],
) -> CoreResult<Vec<GlobalResourceResolvedTarget>> {
    selector.validate()?;
    let mut seen_controllers = BTreeSet::new();
    let mut grouped = BTreeMap::<String, Vec<&ControllerRecord>>::new();
    for record in records {
        if !seen_controllers.insert(record.controller_id.as_str()) {
            return Err(CoreError::Conflict(format!(
                "duplicate Controller directory entry {}",
                record.controller_id
            )));
        }
        validate_directory_cluster(&record.cluster)?;
        grouped
            .entry(record.cluster.clone())
            .or_default()
            .push(record);
    }

    let mut selected = BTreeMap::<String, (Vec<&ControllerRecord>, bool)>::new();
    match selector {
        GlobalResourceTargetSelector::All => {
            for (cluster, cluster_records) in grouped {
                selected.insert(cluster, (cluster_records, false));
            }
        }
        GlobalResourceTargetSelector::ClusterIds { cluster_ids } => {
            for cluster in cluster_ids {
                validate_directory_cluster(cluster)?;
                selected.insert(
                    cluster.clone(),
                    (grouped.remove(cluster).unwrap_or_default(), false),
                );
            }
        }
        GlobalResourceTargetSelector::ControllerMetadata { environments, tags } => {
            for (cluster, cluster_records) in grouped {
                let matches = cluster_records
                    .iter()
                    .map(|record| metadata_matches(record, environments, tags))
                    .collect::<Vec<_>>();
                if matches.iter().all(|matched| *matched) {
                    selected.insert(cluster, (cluster_records, false));
                } else if matches.iter().any(|matched| *matched) {
                    selected.insert(cluster, (cluster_records, true));
                }
            }
        }
    }

    Ok(selected
        .into_iter()
        .map(|(cluster, (records, metadata_indeterminate))| {
            resolve_selected_cluster(cluster, records, metadata_indeterminate)
        })
        .collect())
}

pub fn plan_global_resource(
    resource: &GlobalResource,
    config: &GlobalResourcesConfig,
    targets: &[GlobalResourceResolvedTarget],
    observations: &BTreeMap<String, GlobalResourceTargetObservation>,
    membership_revision: &str,
) -> CoreResult<GlobalResourcePlan> {
    resource.validate(config)?;
    validate_membership_revision(membership_revision)?;
    validate_ordered_targets(targets)?;
    if observations.keys().any(|cluster| {
        targets
            .binary_search_by(|target| target.cluster.cmp(cluster))
            .is_err()
    }) {
        return Err(CoreError::Conflict(
            "global resource observations contain an unbound target".to_string(),
        ));
    }
    let desired = normalized_global_resource_desired(resource, config)?;
    let desired_normalized_revision = normalized_document_revision(&desired)?;
    let plans = targets
        .iter()
        .map(|target| {
            plan_target(
                resource,
                config,
                target,
                observations.get(&target.cluster),
                &desired,
                &desired_normalized_revision,
            )
        })
        .collect::<Vec<_>>();
    let ordered_targets = targets
        .iter()
        .map(|target| target.cluster.clone())
        .collect::<Vec<_>>();
    let target_evidence_revision = target_evidence_revision(&plans)?;
    let plan_revision = plan_binding_revision(
        resource,
        &ordered_targets,
        membership_revision,
        &target_evidence_revision,
    )?;
    Ok(GlobalResourcePlan {
        binding: GlobalResourcePlanBinding {
            global_resource_id: resource.id.clone(),
            generation: resource.generation,
            desired_revision: resource.desired_revision.clone(),
            ordered_targets,
            membership_revision: membership_revision.to_string(),
            target_evidence_revision,
            plan_revision,
        },
        targets: plans,
    })
}

fn plan_target(
    resource: &GlobalResource,
    config: &GlobalResourcesConfig,
    target: &GlobalResourceResolvedTarget,
    observation: Option<&GlobalResourceTargetObservation>,
    desired: &Value,
    desired_normalized_revision: &str,
) -> GlobalResourceTargetPlan {
    let blocked =
        |state, reason| target_plan_base(target, state, reason, desired_normalized_revision);
    match target.state {
        GlobalResourceTargetResolutionState::Offline => blocked(
            GlobalResourcePlanState::Offline,
            GlobalResourcePlanReason::ControllerOffline,
        ),
        GlobalResourceTargetResolutionState::Ambiguous => blocked(
            GlobalResourcePlanState::Ambiguous,
            GlobalResourcePlanReason::ControllerAmbiguous,
        ),
        GlobalResourceTargetResolutionState::Indeterminate => blocked(
            GlobalResourcePlanState::Indeterminate,
            GlobalResourcePlanReason::MembershipIndeterminate,
        ),
        GlobalResourceTargetResolutionState::Available => match observation {
            Some(GlobalResourceTargetObservation::Missing) => blocked(
                GlobalResourcePlanState::Create,
                GlobalResourcePlanReason::Missing,
            ),
            Some(GlobalResourceTargetObservation::Denied) => blocked(
                GlobalResourcePlanState::Denied,
                GlobalResourcePlanReason::AccessDenied,
            ),
            Some(GlobalResourceTargetObservation::Indeterminate) | None => blocked(
                GlobalResourcePlanState::Indeterminate,
                GlobalResourcePlanReason::ObservationIndeterminate,
            ),
            Some(GlobalResourceTargetObservation::Invalid) => blocked(
                GlobalResourcePlanState::Invalid,
                GlobalResourcePlanReason::ObservationInvalid,
            ),
            Some(GlobalResourceTargetObservation::Present(observed)) => {
                let Ok(normalized_observed) =
                    normalized_global_resource_observed(observed, resource, config)
                else {
                    return blocked(
                        GlobalResourcePlanState::Invalid,
                        GlobalResourcePlanReason::ObservationInvalid,
                    );
                };
                let Ok(observed_normalized_revision) =
                    normalized_document_revision(&normalized_observed)
                else {
                    return blocked(
                        GlobalResourcePlanState::Invalid,
                        GlobalResourcePlanReason::ObservationInvalid,
                    );
                };
                let observed_resource_version = observed_resource_version(observed);
                let (changed_paths, changed_paths_truncated) =
                    changed_paths(desired, &normalized_observed);
                let ownership = validate_global_resource_ownership(observed, resource);
                let (state, reason) = match ownership {
                    GlobalResourceOwnership::Unowned => (
                        GlobalResourcePlanState::Conflict,
                        GlobalResourcePlanReason::OwnershipUnowned,
                    ),
                    GlobalResourceOwnership::Foreign => (
                        GlobalResourcePlanState::Conflict,
                        GlobalResourcePlanReason::OwnershipForeign,
                    ),
                    GlobalResourceOwnership::Invalid => (
                        GlobalResourcePlanState::Conflict,
                        GlobalResourcePlanReason::OwnershipInvalid,
                    ),
                    GlobalResourceOwnership::Owned { .. }
                        if observed_resource_version.is_none() =>
                    {
                        (
                            GlobalResourcePlanState::Invalid,
                            GlobalResourcePlanReason::ObservationInvalid,
                        )
                    }
                    GlobalResourceOwnership::Owned { .. } if normalized_observed == *desired => (
                        GlobalResourcePlanState::Noop,
                        GlobalResourcePlanReason::InSync,
                    ),
                    GlobalResourceOwnership::Owned { .. } => (
                        GlobalResourcePlanState::Update,
                        GlobalResourcePlanReason::Drift,
                    ),
                };
                let mut plan = target_plan_base(target, state, reason, desired_normalized_revision);
                plan.observed_resource_version = observed_resource_version;
                plan.observed_normalized_revision = Some(observed_normalized_revision);
                plan.changed_paths = changed_paths;
                plan.changed_paths_truncated = changed_paths_truncated;
                plan
            }
        },
    }
}

fn target_plan_base(
    target: &GlobalResourceResolvedTarget,
    state: GlobalResourcePlanState,
    reason: GlobalResourcePlanReason,
    desired_normalized_revision: &str,
) -> GlobalResourceTargetPlan {
    GlobalResourceTargetPlan {
        cluster: target.cluster.clone(),
        controller_id: target.controller_id.clone(),
        controller_session_id: target.controller_session_id.clone(),
        ownership_fence: target.ownership_fence.clone(),
        state,
        reason,
        observed_resource_version: None,
        desired_normalized_revision: desired_normalized_revision.to_string(),
        observed_normalized_revision: None,
        changed_paths: Vec::new(),
        changed_paths_truncated: false,
    }
}

fn observed_resource_version(document: &Value) -> Option<String> {
    document
        .get("metadata")
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("resourceVersion"))
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= MAX_RESOURCE_VERSION_BYTES
                && value.trim() == *value
                && !value.chars().any(char::is_control)
        })
        .map(str::to_string)
}

fn normalized_document_revision(document: &Value) -> CoreResult<String> {
    let canonical = serde_json::to_vec(document).map_err(|_| {
        CoreError::Conflict("normalized global resource is not serializable".to_string())
    })?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(canonical))))
}

fn changed_paths(desired: &Value, observed: &Value) -> (Vec<String>, bool) {
    let mut paths = Vec::new();
    collect_changed_paths(desired, observed, "", &mut paths);
    let mut truncated = paths.len() > MAX_GLOBAL_RESOURCE_CHANGED_PATHS;
    paths.truncate(MAX_GLOBAL_RESOURCE_CHANGED_PATHS);
    for path in &mut paths {
        if path.len() > MAX_GLOBAL_RESOURCE_CHANGED_PATH_BYTES {
            *path = "/<path-truncated>".to_string();
            truncated = true;
        }
    }
    paths.dedup();
    (paths, truncated)
}

fn collect_changed_paths(desired: &Value, observed: &Value, path: &str, output: &mut Vec<String>) {
    if output.len() > MAX_GLOBAL_RESOURCE_CHANGED_PATHS {
        return;
    }
    match (desired, observed) {
        (Value::Object(desired), Value::Object(observed)) => {
            let keys = desired
                .keys()
                .chain(observed.keys())
                .collect::<BTreeSet<_>>();
            for key in keys {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                let child_path = format!("{path}/{escaped}");
                match (desired.get(key), observed.get(key)) {
                    (Some(desired), Some(observed)) => {
                        collect_changed_paths(desired, observed, &child_path, output);
                    }
                    _ => output.push(child_path),
                }
                if output.len() > MAX_GLOBAL_RESOURCE_CHANGED_PATHS {
                    break;
                }
            }
        }
        _ if desired != observed => output.push(if path.is_empty() {
            "/".to_string()
        } else {
            path.to_string()
        }),
        _ => {}
    }
}

fn resolve_selected_cluster(
    cluster: String,
    records: Vec<&ControllerRecord>,
    metadata_indeterminate: bool,
) -> GlobalResourceResolvedTarget {
    let candidates = records
        .iter()
        .map(|record| record.controller_id.to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if metadata_indeterminate {
        return GlobalResourceResolvedTarget {
            cluster,
            state: GlobalResourceTargetResolutionState::Indeterminate,
            controller_id: None,
            controller_session_id: None,
            ownership_fence: None,
            candidates,
        };
    }
    let mut eligible = Vec::new();
    let mut indeterminate = false;
    for record in records {
        if record.phase == ControllerPhase::Online {
            if let Some(session_id) = record.current_session_id.as_ref() {
                if record
                    .ownership_fence
                    .as_ref()
                    .is_some_and(|fence| !valid_ownership_fence(fence))
                {
                    indeterminate = true;
                } else {
                    eligible.push((
                        record.controller_id.to_string(),
                        session_id.to_string(),
                        record.ownership_fence.clone(),
                    ));
                }
            } else {
                indeterminate = true;
            }
        }
    }
    eligible.sort_by(|left, right| left.0.cmp(&right.0));
    let (state, controller_id, controller_session_id, ownership_fence, candidates) =
        if indeterminate {
            (
                GlobalResourceTargetResolutionState::Indeterminate,
                None,
                None,
                None,
                candidates,
            )
        } else if eligible.len() > 1 {
            (
                GlobalResourceTargetResolutionState::Ambiguous,
                None,
                None,
                None,
                eligible.into_iter().map(|entry| entry.0).collect(),
            )
        } else if let Some((controller_id, session_id, ownership_fence)) = eligible.pop() {
            (
                GlobalResourceTargetResolutionState::Available,
                Some(controller_id.clone()),
                Some(session_id),
                ownership_fence,
                vec![controller_id],
            )
        } else {
            (
                GlobalResourceTargetResolutionState::Offline,
                None,
                None,
                None,
                candidates,
            )
        };
    GlobalResourceResolvedTarget {
        cluster,
        state,
        controller_id,
        controller_session_id,
        ownership_fence,
        candidates,
    }
}

fn metadata_matches(
    record: &ControllerRecord,
    environments: &BTreeSet<String>,
    tags: &BTreeSet<String>,
) -> bool {
    let record_environments = record.environments.iter().collect::<BTreeSet<_>>();
    let record_tags = record.tags.iter().collect::<BTreeSet<_>>();
    environments
        .iter()
        .all(|value| record_environments.contains(value))
        && tags.iter().all(|value| record_tags.contains(value))
}

fn normalize_document(mut document: Value) -> CoreResult<Value> {
    let root = document.as_object_mut().ok_or_else(|| {
        CoreError::Conflict("global resource document must be an object".to_string())
    })?;
    root.remove("status");
    let metadata = root_metadata_mut(&mut document)?;
    for field in CLUSTER_OWNED_METADATA_FIELDS {
        metadata.remove(field);
    }
    Ok(canonicalize_value(document))
}

fn canonicalize_value(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let ordered = object
                .into_iter()
                .map(|(key, value)| (key, canonicalize_value(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_value).collect()),
        other => other,
    }
}

fn validate_observed_envelope(document: &Value, resource: &GlobalResource) -> CoreResult<()> {
    let root = document.as_object().ok_or_else(|| {
        CoreError::Conflict("observed global resource must be an object".to_string())
    })?;
    if root.get("kind").and_then(Value::as_str) != Some(resource.desired.resource_kind.as_str()) {
        return Err(CoreError::Conflict(
            "observed global resource kind does not match desired state".to_string(),
        ));
    }
    if !root
        .get("apiVersion")
        .and_then(Value::as_str)
        .is_some_and(valid_bounded_text)
    {
        return Err(CoreError::Conflict(
            "observed global resource apiVersion is invalid".to_string(),
        ));
    }
    let expected_metadata = resource
        .desired
        .template_document
        .get("metadata")
        .and_then(Value::as_object)
        .expect("validated GlobalResource has metadata");
    let metadata = root
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CoreError::Conflict("observed global resource metadata must be an object".to_string())
        })?;
    for field in ["name", "namespace"] {
        if metadata.get(field).and_then(Value::as_str)
            != expected_metadata.get(field).and_then(Value::as_str)
        {
            return Err(CoreError::Conflict(format!(
                "observed global resource metadata.{field} does not match desired state"
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_no_reserved_global_resource_provenance(
    metadata: &Map<String, Value>,
) -> CoreResult<()> {
    for field in ["labels", "annotations"] {
        if metadata.get(field).is_some_and(|value| !value.is_object()) {
            return Err(CoreError::Conflict(format!(
                "global resource template metadata.{field} must be an object"
            )));
        }
    }
    if nested_value(metadata, "labels", GLOBAL_RESOURCE_MANAGED_BY_LABEL).is_some()
        || nested_value(metadata, "annotations", GLOBAL_RESOURCE_ID_ANNOTATION).is_some()
        || nested_value(metadata, "annotations", GLOBAL_RESOURCE_REVISION_ANNOTATION).is_some()
    {
        return Err(CoreError::Conflict(
            "global resource template contains reserved Center ownership provenance".to_string(),
        ));
    }
    Ok(())
}

fn root_metadata_mut(document: &mut Value) -> CoreResult<&mut Map<String, Value>> {
    document
        .get_mut("metadata")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            CoreError::Conflict("global resource document metadata must be an object".to_string())
        })
}

fn object_field_mut<'a>(
    object: &'a mut Map<String, Value>,
    field: &'static str,
) -> CoreResult<&'a mut Map<String, Value>> {
    let value = object
        .entry(field.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    value.as_object_mut().ok_or_else(|| {
        CoreError::Conflict(format!(
            "global resource metadata.{field} must be an object"
        ))
    })
}

fn nested_value<'a>(metadata: &'a Map<String, Value>, field: &str, key: &str) -> Option<&'a Value> {
    metadata
        .get(field)
        .and_then(Value::as_object)
        .and_then(|values| values.get(key))
}

fn validate_ordered_targets(targets: &[GlobalResourceResolvedTarget]) -> CoreResult<()> {
    let mut previous: Option<&str> = None;
    for target in targets {
        validate_directory_cluster(&target.cluster)?;
        if previous.is_some_and(|value| value.as_bytes() >= target.cluster.as_bytes()) {
            return Err(CoreError::Conflict(
                "global resource target set must be strictly byte ordered and unique".to_string(),
            ));
        }
        match target.state {
            GlobalResourceTargetResolutionState::Available => {
                if target.controller_id.as_ref() != target.candidates.first()
                    || target.candidates.len() != 1
                    || target.controller_session_id.is_none()
                    || target
                        .ownership_fence
                        .as_ref()
                        .is_some_and(|fence| !valid_ownership_fence(fence))
                {
                    return Err(CoreError::Conflict(
                        "available global resource target has invalid Controller evidence"
                            .to_string(),
                    ));
                }
            }
            _ if target.controller_id.is_some()
                || target.controller_session_id.is_some()
                || target.ownership_fence.is_some() =>
            {
                return Err(CoreError::Conflict(
                    "unavailable global resource target must not select a Controller".to_string(),
                ));
            }
            _ => {}
        }
        previous = Some(&target.cluster);
    }
    Ok(())
}

fn validate_ordered_target_plans(targets: &[GlobalResourceTargetPlan]) -> CoreResult<()> {
    let mut previous: Option<&str> = None;
    for target in targets {
        validate_directory_cluster(&target.cluster)?;
        if previous.is_some_and(|value| value.as_bytes() >= target.cluster.as_bytes()) {
            return Err(CoreError::Conflict(
                "global resource target set must be strictly byte ordered and unique".to_string(),
            ));
        }
        if !is_sha256_revision(&target.desired_normalized_revision)
            || target
                .observed_normalized_revision
                .as_deref()
                .is_some_and(|revision| !is_sha256_revision(revision))
            || target.changed_paths.len() > MAX_GLOBAL_RESOURCE_CHANGED_PATHS
            || target
                .changed_paths
                .iter()
                .any(|path| path.len() > MAX_GLOBAL_RESOURCE_CHANGED_PATH_BYTES)
            || target
                .ownership_fence
                .as_ref()
                .is_some_and(|fence| !valid_ownership_fence(fence))
        {
            return Err(CoreError::Conflict(
                "global resource target plan evidence is invalid".to_string(),
            ));
        }
        let has_controller = target.controller_id.is_some()
            && target.controller_session_id.is_some()
            && matches!(
                target.state,
                GlobalResourcePlanState::Create
                    | GlobalResourcePlanState::Update
                    | GlobalResourcePlanState::Noop
                    | GlobalResourcePlanState::Conflict
                    | GlobalResourcePlanState::Denied
                    | GlobalResourcePlanState::Indeterminate
                    | GlobalResourcePlanState::Invalid
            );
        if !has_controller
            && (target.controller_id.is_some()
                || target.controller_session_id.is_some()
                || target.ownership_fence.is_some())
        {
            return Err(CoreError::Conflict(
                "global resource target plan Controller evidence is invalid".to_string(),
            ));
        }
        if matches!(
            target.state,
            GlobalResourcePlanState::Update | GlobalResourcePlanState::Noop
        ) && (target.observed_resource_version.is_none()
            || target.observed_normalized_revision.is_none())
        {
            return Err(CoreError::Conflict(
                "mutable global resource target plan lacks observed preconditions".to_string(),
            ));
        }
        previous = Some(&target.cluster);
    }
    Ok(())
}

fn validate_directory_cluster(cluster: &str) -> CoreResult<()> {
    if !valid_bounded_text(cluster) {
        return Err(CoreError::Conflict(
            "Controller directory contains an invalid cluster identity".to_string(),
        ));
    }
    Ok(())
}

fn valid_bounded_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn valid_ownership_fence(fence: &OwnershipFence) -> bool {
    fence.epoch > 0 && valid_bounded_text(&fence.token)
}

fn validate_membership_revision(revision: &str) -> CoreResult<()> {
    if revision.len() > MAX_MEMBERSHIP_REVISION_BYTES || !is_sha256_revision(revision) {
        return Err(CoreError::Conflict(
            "global resource membership revision is invalid".to_string(),
        ));
    }
    Ok(())
}

fn is_sha256_revision(revision: &str) -> bool {
    revision.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn plan_binding_revision(
    resource: &GlobalResource,
    ordered_targets: &[String],
    membership_revision: &str,
    target_evidence_revision: &str,
) -> CoreResult<String> {
    let canonical = serde_json::to_vec(&(
        GLOBAL_RESOURCE_PLAN_SCHEMA_VERSION,
        &resource.id,
        resource.generation,
        &resource.desired_revision,
        ordered_targets,
        membership_revision,
        target_evidence_revision,
    ))
    .map_err(|_| CoreError::Conflict("global resource plan is not serializable".to_string()))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(canonical))))
}

fn target_evidence_revision(targets: &[GlobalResourceTargetPlan]) -> CoreResult<String> {
    validate_ordered_target_plans(targets)?;
    let canonical = serde_json::to_vec(targets).map_err(|_| {
        CoreError::Conflict("global resource target evidence is not serializable".to_string())
    })?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(canonical))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        global_resource_for_create, ControllerId, GlobalResourceDesired,
        GlobalResourceInventoryKind, GlobalResourceSyncPolicy, SessionId,
    };
    use serde_json::json;

    fn resource(selector: GlobalResourceTargetSelector) -> GlobalResource {
        global_resource_for_create(
            GlobalResourceId::new("shared-allow-list").unwrap(),
            GlobalResourceDesired {
                display_name: "Shared allow list".to_string(),
                resource_kind: GlobalResourceInventoryKind::EdgionConfigData,
                target_namespace: "edgion-data".to_string(),
                template_document: json!({
                    "apiVersion": "edgion.io/v1alpha1",
                    "kind": "EdgionConfigData",
                    "metadata": {
                        "name": "shared-allow-list",
                        "namespace": "edgion-data",
                        "labels": {"purpose": "shared"}
                    },
                    "data": {"type": "IpList", "items": ["192.0.2.0/24"]}
                }),
                target_selector: selector,
                sync_policy: GlobalResourceSyncPolicy::default(),
            },
            "alice",
            100,
            &GlobalResourcesConfig::default(),
        )
        .unwrap()
    }

    fn record(
        id: &str,
        cluster: &str,
        phase: ControllerPhase,
        environments: &[&str],
        tags: &[&str],
    ) -> ControllerRecord {
        ControllerRecord {
            controller_id: ControllerId::new(id).unwrap(),
            current_session_id: (phase == ControllerPhase::Online)
                .then(|| SessionId::new(format!("session-{id}")).unwrap()),
            cluster: cluster.to_string(),
            environments: environments
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            tags: tags.iter().map(|value| (*value).to_string()).collect(),
            connected_replica: None,
            ownership_fence: None,
            sync_version: None,
            watch_server_id: None,
            resource_count: None,
            resource_counts_by_kind: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: None,
            phase,
            last_seen_unix_ms: 100,
        }
    }

    fn membership_revision() -> &'static str {
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }

    #[test]
    fn ownership_is_injected_exactly_and_spoofed_templates_fail_closed() {
        let config = GlobalResourcesConfig::default();
        let resource = resource(GlobalResourceTargetSelector::All);
        let mut desired = normalized_global_resource_desired(&resource, &config).unwrap();
        desired["metadata"]["resourceVersion"] = json!("42");
        assert_eq!(
            desired["metadata"]["labels"][GLOBAL_RESOURCE_MANAGED_BY_LABEL],
            GLOBAL_RESOURCE_MANAGED_BY_VALUE
        );
        assert_eq!(
            desired["metadata"]["annotations"][GLOBAL_RESOURCE_ID_ANNOTATION],
            resource.id.as_str()
        );
        assert_eq!(
            desired["metadata"]["annotations"][GLOBAL_RESOURCE_REVISION_ANNOTATION],
            resource.desired_revision.as_str()
        );
        assert_eq!(
            validate_global_resource_ownership(&desired, &resource),
            GlobalResourceOwnership::Owned {
                desired_revision: resource.desired_revision.as_str().to_string(),
                current_revision: true
            }
        );

        let mut spoofed = resource.clone();
        spoofed.desired.template_document["metadata"]["annotations"]
            [GLOBAL_RESOURCE_ID_ANNOTATION] = json!(resource.id.as_str());
        assert!(spoofed.desired.validate(&config).is_err());
        assert!(normalized_global_resource_desired(&spoofed, &config).is_err());

        let mut malformed = desired;
        malformed["metadata"]["annotations"][GLOBAL_RESOURCE_REVISION_ANNOTATION] = json!(42);
        assert_eq!(
            validate_global_resource_ownership(&malformed, &resource),
            GlobalResourceOwnership::Invalid
        );
    }

    #[test]
    fn normalization_ignores_only_root_cluster_fields() {
        let config = GlobalResourcesConfig::default();
        let resource = resource(GlobalResourceTargetSelector::All);
        let mut desired = normalized_global_resource_desired(&resource, &config).unwrap();
        desired["metadata"]["resourceVersion"] = json!("42");
        let mut observed = desired.clone();
        observed["status"] = json!({"accepted": true});
        observed["metadata"]["resourceVersion"] = json!("12");
        observed["metadata"]["uid"] = json!("uid");
        observed["metadata"]["managedFields"] = json!([]);
        observed["data"]["status"] = json!("business-value");
        observed["data"]["metadata"] = json!({"resourceVersion": "business-value"});
        let normalized =
            normalized_global_resource_observed(&observed, &resource, &config).unwrap();
        assert!(normalized.get("status").is_none());
        assert!(normalized["metadata"].get("resourceVersion").is_none());
        assert_eq!(normalized["data"]["status"], "business-value");
        assert_eq!(
            normalized["data"]["metadata"]["resourceVersion"],
            "business-value"
        );
    }

    #[test]
    fn selector_resolution_is_sorted_deduplicated_and_all_constraints_are_and() {
        let selector = GlobalResourceTargetSelector::ControllerMetadata {
            environments: BTreeSet::from(["prod".to_string(), "edge".to_string()]),
            tags: BTreeSet::from(["pci".to_string(), "public".to_string()]),
        };
        let records = vec![
            record(
                "controller-b",
                "cluster-b",
                ControllerPhase::Online,
                &["prod", "edge"],
                &["pci"],
            ),
            record(
                "controller-a",
                "cluster-a",
                ControllerPhase::Online,
                &["edge", "prod", "other"],
                &["public", "pci"],
            ),
        ];
        let targets = resolve_global_resource_targets(&selector, &records).unwrap();
        assert_eq!(
            targets
                .iter()
                .map(|target| target.cluster.as_str())
                .collect::<Vec<_>>(),
            ["cluster-a"]
        );
        assert_eq!(
            targets[0].state,
            GlobalResourceTargetResolutionState::Available
        );
    }

    #[test]
    fn metadata_disagreement_is_indeterminate_and_cluster_ids_preserve_absent_targets() {
        let records = vec![
            record(
                "controller-a0",
                "cluster-a",
                ControllerPhase::Online,
                &["prod"],
                &["edge"],
            ),
            record(
                "controller-a1",
                "cluster-a",
                ControllerPhase::Offline,
                &["dev"],
                &["edge"],
            ),
        ];
        let metadata = GlobalResourceTargetSelector::ControllerMetadata {
            environments: BTreeSet::from(["prod".to_string()]),
            tags: BTreeSet::new(),
        };
        let targets = resolve_global_resource_targets(&metadata, &records).unwrap();
        assert_eq!(
            targets[0].state,
            GlobalResourceTargetResolutionState::Indeterminate
        );

        let explicit = GlobalResourceTargetSelector::ClusterIds {
            cluster_ids: BTreeSet::from(["cluster-a".to_string(), "cluster-z".to_string()]),
        };
        let targets = resolve_global_resource_targets(&explicit, &records).unwrap();
        assert_eq!(targets[0].cluster, "cluster-a");
        assert_eq!(targets[1].cluster, "cluster-z");
        assert_eq!(
            targets[1].state,
            GlobalResourceTargetResolutionState::Offline
        );
    }

    #[test]
    fn resolution_never_picks_ambiguous_or_sessionless_controllers() {
        let all = GlobalResourceTargetSelector::All;
        let mut sessionless = record(
            "controller-a",
            "cluster-a",
            ControllerPhase::Online,
            &[],
            &[],
        );
        sessionless.current_session_id = None;
        let records = vec![
            sessionless,
            record(
                "controller-b0",
                "cluster-b",
                ControllerPhase::Online,
                &[],
                &[],
            ),
            record(
                "controller-b1",
                "cluster-b",
                ControllerPhase::Online,
                &[],
                &[],
            ),
        ];
        let targets = resolve_global_resource_targets(&all, &records).unwrap();
        assert_eq!(
            targets[0].state,
            GlobalResourceTargetResolutionState::Indeterminate
        );
        assert_eq!(
            targets[1].state,
            GlobalResourceTargetResolutionState::Ambiguous
        );
        assert!(targets.iter().all(|target| target.controller_id.is_none()));
    }

    #[test]
    fn plan_covers_every_state_and_never_adopts_unowned_objects() {
        let config = GlobalResourcesConfig::default();
        let resource = resource(GlobalResourceTargetSelector::All);
        let mut desired = normalized_global_resource_desired(&resource, &config).unwrap();
        desired["metadata"]["resourceVersion"] = json!("42");
        let mut drifted = desired.clone();
        drifted["data"]["items"] = json!(["198.51.100.0/24"]);
        let mut unowned = desired.clone();
        unowned["metadata"]["labels"]
            .as_object_mut()
            .unwrap()
            .remove(GLOBAL_RESOURCE_MANAGED_BY_LABEL);
        unowned["metadata"]["annotations"]
            .as_object_mut()
            .unwrap()
            .remove(GLOBAL_RESOURCE_ID_ANNOTATION);
        unowned["metadata"]["annotations"]
            .as_object_mut()
            .unwrap()
            .remove(GLOBAL_RESOURCE_REVISION_ANNOTATION);

        let states = [
            GlobalResourceTargetResolutionState::Available,
            GlobalResourceTargetResolutionState::Available,
            GlobalResourceTargetResolutionState::Available,
            GlobalResourceTargetResolutionState::Available,
            GlobalResourceTargetResolutionState::Available,
            GlobalResourceTargetResolutionState::Available,
            GlobalResourceTargetResolutionState::Offline,
            GlobalResourceTargetResolutionState::Ambiguous,
            GlobalResourceTargetResolutionState::Indeterminate,
        ];
        let targets = states
            .into_iter()
            .enumerate()
            .map(|(index, state)| GlobalResourceResolvedTarget {
                cluster: format!("cluster-{index}"),
                controller_id: (state == GlobalResourceTargetResolutionState::Available)
                    .then(|| format!("controller-{index}")),
                controller_session_id: (state == GlobalResourceTargetResolutionState::Available)
                    .then(|| format!("session-{index}")),
                ownership_fence: None,
                candidates: if state == GlobalResourceTargetResolutionState::Available {
                    vec![format!("controller-{index}")]
                } else {
                    Vec::new()
                },
                state,
            })
            .collect::<Vec<_>>();
        let observations = BTreeMap::from([
            (
                "cluster-0".to_string(),
                GlobalResourceTargetObservation::Missing,
            ),
            (
                "cluster-1".to_string(),
                GlobalResourceTargetObservation::Present(drifted),
            ),
            (
                "cluster-2".to_string(),
                GlobalResourceTargetObservation::Present(desired),
            ),
            (
                "cluster-3".to_string(),
                GlobalResourceTargetObservation::Present(unowned),
            ),
            (
                "cluster-4".to_string(),
                GlobalResourceTargetObservation::Denied,
            ),
            (
                "cluster-5".to_string(),
                GlobalResourceTargetObservation::Invalid,
            ),
        ]);
        let plan = plan_global_resource(
            &resource,
            &config,
            &targets,
            &observations,
            membership_revision(),
        )
        .unwrap();
        assert_eq!(
            plan.targets
                .iter()
                .map(|target| target.state)
                .collect::<Vec<_>>(),
            [
                GlobalResourcePlanState::Create,
                GlobalResourcePlanState::Update,
                GlobalResourcePlanState::Noop,
                GlobalResourcePlanState::Conflict,
                GlobalResourcePlanState::Denied,
                GlobalResourcePlanState::Invalid,
                GlobalResourcePlanState::Offline,
                GlobalResourcePlanState::Ambiguous,
                GlobalResourcePlanState::Indeterminate,
            ]
        );
    }

    #[test]
    fn plan_ignores_root_runtime_fields_but_preserves_nested_business_fields() {
        let config = GlobalResourcesConfig::default();
        let resource = resource(GlobalResourceTargetSelector::All);
        let desired = normalized_global_resource_desired(&resource, &config).unwrap();
        let target = GlobalResourceResolvedTarget {
            cluster: "cluster-a".to_string(),
            state: GlobalResourceTargetResolutionState::Available,
            controller_id: Some("controller-a".to_string()),
            controller_session_id: Some("session-a".to_string()),
            ownership_fence: None,
            candidates: vec!["controller-a".to_string()],
        };
        let mut runtime_only = desired.clone();
        runtime_only["status"] = json!({"accepted": true});
        runtime_only["metadata"]["resourceVersion"] = json!("42");
        let noop = plan_global_resource(
            &resource,
            &config,
            std::slice::from_ref(&target),
            &BTreeMap::from([(
                "cluster-a".to_string(),
                GlobalResourceTargetObservation::Present(runtime_only),
            )]),
            membership_revision(),
        )
        .unwrap();
        assert_eq!(noop.targets[0].state, GlobalResourcePlanState::Noop);

        let mut nested_drift = desired;
        nested_drift["metadata"]["resourceVersion"] = json!("43");
        nested_drift["data"]["status"] = json!("business-value");
        let update = plan_global_resource(
            &resource,
            &config,
            &[target],
            &BTreeMap::from([(
                "cluster-a".to_string(),
                GlobalResourceTargetObservation::Present(nested_drift),
            )]),
            membership_revision(),
        )
        .unwrap();
        assert_eq!(update.targets[0].state, GlobalResourcePlanState::Update);
    }

    #[test]
    fn owned_previous_revision_can_update_but_foreign_owner_conflicts() {
        let config = GlobalResourcesConfig::default();
        let resource = resource(GlobalResourceTargetSelector::All);
        let mut desired = normalized_global_resource_desired(&resource, &config).unwrap();
        desired["metadata"]["resourceVersion"] = json!("42");
        let mut previous = desired.clone();
        previous["metadata"]["annotations"][GLOBAL_RESOURCE_REVISION_ANNOTATION] =
            json!("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert!(matches!(
            validate_global_resource_ownership(&previous, &resource),
            GlobalResourceOwnership::Owned {
                current_revision: false,
                ..
            }
        ));
        let mut foreign = previous.clone();
        foreign["metadata"]["annotations"][GLOBAL_RESOURCE_ID_ANNOTATION] = json!("other-owner");

        let target = GlobalResourceResolvedTarget {
            cluster: "cluster-a".to_string(),
            state: GlobalResourceTargetResolutionState::Available,
            controller_id: Some("controller-a".to_string()),
            controller_session_id: Some("session-a".to_string()),
            ownership_fence: None,
            candidates: vec!["controller-a".to_string()],
        };
        let previous_plan = plan_global_resource(
            &resource,
            &config,
            std::slice::from_ref(&target),
            &BTreeMap::from([(
                "cluster-a".to_string(),
                GlobalResourceTargetObservation::Present(previous),
            )]),
            membership_revision(),
        )
        .unwrap();
        assert_eq!(
            previous_plan.targets[0].state,
            GlobalResourcePlanState::Update
        );
        let foreign_plan = plan_global_resource(
            &resource,
            &config,
            &[target],
            &BTreeMap::from([(
                "cluster-a".to_string(),
                GlobalResourceTargetObservation::Present(foreign),
            )]),
            membership_revision(),
        )
        .unwrap();
        assert_eq!(
            foreign_plan.targets[0].state,
            GlobalResourcePlanState::Conflict
        );
    }

    #[test]
    fn binding_changes_for_every_exact_input_and_rejects_unordered_targets() {
        let config = GlobalResourcesConfig::default();
        let resource = resource(GlobalResourceTargetSelector::All);
        let targets = resolve_global_resource_targets(
            &GlobalResourceTargetSelector::ClusterIds {
                cluster_ids: BTreeSet::from(["cluster-a".to_string(), "cluster-b".to_string()]),
            },
            &[],
        )
        .unwrap();
        let first = plan_global_resource(
            &resource,
            &config,
            &targets,
            &BTreeMap::new(),
            membership_revision(),
        )
        .unwrap();
        let changed_membership = plan_global_resource(
            &resource,
            &config,
            &targets,
            &BTreeMap::new(),
            "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        )
        .unwrap();
        assert_ne!(
            first.binding.plan_revision,
            changed_membership.binding.plan_revision
        );

        let changed_generation = plan_binding_revision(
            &GlobalResource {
                generation: resource.generation + 1,
                ..resource.clone()
            },
            &first.binding.ordered_targets,
            membership_revision(),
            &first.binding.target_evidence_revision,
        )
        .unwrap();
        assert_ne!(first.binding.plan_revision, changed_generation);

        let changed_target_set = plan_binding_revision(
            &resource,
            &["cluster-a".to_string()],
            membership_revision(),
            &first.binding.target_evidence_revision,
        )
        .unwrap();
        assert_ne!(first.binding.plan_revision, changed_target_set);

        let mut changed_desired = resource.desired.clone();
        changed_desired.display_name.push_str(" v2");
        let replaced =
            crate::global_resource_for_replace(&resource, changed_desired, "bob", 200, &config)
                .unwrap();
        let changed_desired_plan = plan_global_resource(
            &replaced,
            &config,
            &targets,
            &BTreeMap::new(),
            membership_revision(),
        )
        .unwrap();
        assert_ne!(
            first.binding.plan_revision,
            changed_desired_plan.binding.plan_revision
        );
        first
            .binding
            .validate_exact(&resource, &config, &first.targets, membership_revision())
            .unwrap();
        assert!(first
            .binding
            .validate_exact(&replaced, &config, &first.targets, membership_revision(),)
            .is_err());
        let mut tampered = first.binding.clone();
        tampered.plan_revision =
            "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string();
        assert!(tampered
            .validate_exact(&resource, &config, &first.targets, membership_revision(),)
            .is_err());
        let mut stale_evidence = first.targets.clone();
        stale_evidence[0].reason = GlobalResourcePlanReason::ObservationIndeterminate;
        assert!(first
            .binding
            .validate_exact(&resource, &config, &stale_evidence, membership_revision(),)
            .is_err());

        let mut unordered = targets;
        unordered.reverse();
        assert!(plan_global_resource(
            &resource,
            &config,
            &unordered,
            &BTreeMap::new(),
            membership_revision()
        )
        .is_err());
    }

    #[test]
    fn binding_covers_controller_observation_action_blocker_and_bounded_diff_evidence() {
        let config = GlobalResourcesConfig::default();
        let resource = resource(GlobalResourceTargetSelector::All);
        let target = GlobalResourceResolvedTarget {
            cluster: "cluster-a".to_string(),
            state: GlobalResourceTargetResolutionState::Available,
            controller_id: Some("controller-a".to_string()),
            controller_session_id: Some("session-a".to_string()),
            ownership_fence: Some(OwnershipFence {
                token: "fence-a".to_string(),
                epoch: 7,
            }),
            candidates: vec!["controller-a".to_string()],
        };
        let mut observed = normalized_global_resource_desired(&resource, &config).unwrap();
        observed["metadata"]["resourceVersion"] = json!("42");
        observed["data"]["items"] = json!(["198.51.100.0/24"]);
        let plan = plan_global_resource(
            &resource,
            &config,
            std::slice::from_ref(&target),
            &BTreeMap::from([(
                "cluster-a".to_string(),
                GlobalResourceTargetObservation::Present(observed),
            )]),
            membership_revision(),
        )
        .unwrap();
        assert_eq!(plan.targets[0].state, GlobalResourcePlanState::Update);
        assert_eq!(
            plan.targets[0].observed_resource_version.as_deref(),
            Some("42")
        );
        assert!(!plan.targets[0].changed_paths.is_empty());

        let mutations: [fn(&mut GlobalResourceTargetPlan); 8] = [
            |target: &mut GlobalResourceTargetPlan| {
                target.controller_id = Some("controller-b".to_string());
            },
            |target: &mut GlobalResourceTargetPlan| {
                target.controller_session_id = Some("session-b".to_string());
            },
            |target: &mut GlobalResourceTargetPlan| {
                target.ownership_fence.as_mut().unwrap().epoch += 1;
            },
            |target: &mut GlobalResourceTargetPlan| {
                target.state = GlobalResourcePlanState::Noop;
            },
            |target: &mut GlobalResourceTargetPlan| {
                target.reason = GlobalResourcePlanReason::InSync;
            },
            |target: &mut GlobalResourceTargetPlan| {
                target.observed_resource_version = Some("43".to_string());
            },
            |target: &mut GlobalResourceTargetPlan| {
                target.observed_normalized_revision = Some(
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .to_string(),
                );
            },
            |target: &mut GlobalResourceTargetPlan| {
                target
                    .changed_paths
                    .push("/metadata/labels/new".to_string());
            },
        ];
        for mutate in mutations {
            let mut stale = plan.targets.clone();
            mutate(&mut stale[0]);
            assert!(plan
                .binding
                .validate_exact(&resource, &config, &stale, membership_revision())
                .is_err());
        }

        let denied = plan_global_resource(
            &resource,
            &config,
            &[target],
            &BTreeMap::from([(
                "cluster-a".to_string(),
                GlobalResourceTargetObservation::Denied,
            )]),
            membership_revision(),
        )
        .unwrap();
        assert_ne!(
            plan.binding.target_evidence_revision,
            denied.binding.target_evidence_revision
        );

        let mut many_changes = normalized_global_resource_desired(&resource, &config).unwrap();
        many_changes["metadata"]["resourceVersion"] = json!("44");
        let data = many_changes["data"].as_object_mut().unwrap();
        for index in 0..=MAX_GLOBAL_RESOURCE_CHANGED_PATHS {
            data.insert(format!("field-{index:03}"), json!(index));
        }
        let bounded = plan_global_resource(
            &resource,
            &config,
            &[GlobalResourceResolvedTarget {
                cluster: "cluster-a".to_string(),
                state: GlobalResourceTargetResolutionState::Available,
                controller_id: Some("controller-a".to_string()),
                controller_session_id: Some("session-a".to_string()),
                ownership_fence: None,
                candidates: vec!["controller-a".to_string()],
            }],
            &BTreeMap::from([(
                "cluster-a".to_string(),
                GlobalResourceTargetObservation::Present(many_changes),
            )]),
            membership_revision(),
        )
        .unwrap();
        assert_eq!(
            bounded.targets[0].changed_paths.len(),
            MAX_GLOBAL_RESOURCE_CHANGED_PATHS
        );
        assert!(bounded.targets[0].changed_paths_truncated);
    }

    #[test]
    fn malformed_envelopes_and_membership_revisions_fail_closed() {
        let config = GlobalResourcesConfig::default();
        let resource = resource(GlobalResourceTargetSelector::All);
        let target = GlobalResourceResolvedTarget {
            cluster: "cluster-a".to_string(),
            state: GlobalResourceTargetResolutionState::Available,
            controller_id: Some("controller-a".to_string()),
            controller_session_id: Some("session-a".to_string()),
            ownership_fence: None,
            candidates: vec!["controller-a".to_string()],
        };
        let malformed = json!({
            "apiVersion": "",
            "kind": "Secret",
            "metadata": {"name": "shared-allow-list", "namespace": "edgion-data"}
        });
        let plan = plan_global_resource(
            &resource,
            &config,
            &[target],
            &BTreeMap::from([(
                "cluster-a".to_string(),
                GlobalResourceTargetObservation::Present(malformed),
            )]),
            membership_revision(),
        )
        .unwrap();
        assert_eq!(plan.targets[0].state, GlobalResourcePlanState::Invalid);
        assert!(plan_global_resource(&resource, &config, &[], &BTreeMap::new(), "latest").is_err());
    }
}
