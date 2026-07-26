//! Durable Center-owned GlobalResource desired-state model and persistence port.
//!
//! Observed resources from managed clusters deliberately do not belong here.

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::global_resource_plan::validate_no_reserved_global_resource_provenance;
use crate::{CoreError, CoreResult, GlobalResourceInventoryKind, GlobalResourcesConfig};

pub const MAX_GLOBAL_RESOURCE_PAGE_SIZE: u16 = 100;
pub const MAX_GLOBAL_RESOURCE_TEMPLATE_BYTES: usize = 512 * 1024;
const MAX_GLOBAL_RESOURCE_ID_BYTES: usize = 253;
const MAX_DISPLAY_NAME_BYTES: usize = 256;
const MAX_ACTOR_BYTES: usize = 256;
const MAX_CLUSTER_SELECTOR_VALUES: usize = 256;
const MAX_CLUSTER_SELECTOR_VALUE_BYTES: usize = 256;
const MAX_GLOBAL_RESOURCE_GENERATION: u64 = i64::MAX as u64;
const FORBIDDEN_METADATA_FIELDS: [&str; 5] = [
    "resourceVersion",
    "uid",
    "generation",
    "creationTimestamp",
    "managedFields",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GlobalResourceId(String);

impl GlobalResourceId {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if !is_dns_subdomain(&value) || value.len() > MAX_GLOBAL_RESOURCE_ID_BYTES {
            return Err(CoreError::InvalidIdentifier {
                kind: "global resource",
                value,
            });
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

impl Display for GlobalResourceId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GlobalResourceRevision(String);

impl GlobalResourceRevision {
    fn from_desired(desired: &GlobalResourceDesired) -> CoreResult<Self> {
        let canonical = serde_json::to_vec(desired).map_err(|_| {
            CoreError::Conflict("global resource desired state is not serializable".to_string())
        })?;
        Ok(Self(format!(
            "sha256:{}",
            hex::encode(Sha256::digest(canonical))
        )))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalResourceSyncMode {
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalResourceAdoptionPolicy {
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalResourcePrunePolicy {
    Retain,
}

/// The first release is intentionally manual, non-adopting, and non-pruning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlobalResourceSyncPolicy {
    pub mode: GlobalResourceSyncMode,
    pub adoption: GlobalResourceAdoptionPolicy,
    pub prune: GlobalResourcePrunePolicy,
}

impl Default for GlobalResourceSyncPolicy {
    fn default() -> Self {
        Self {
            mode: GlobalResourceSyncMode::Manual,
            adoption: GlobalResourceAdoptionPolicy::Never,
            prune: GlobalResourcePrunePolicy::Retain,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum GlobalResourceTargetSelector {
    All,
    ClusterIds {
        cluster_ids: BTreeSet<String>,
    },
    ControllerMetadata {
        #[serde(default)]
        environments: BTreeSet<String>,
        #[serde(default)]
        tags: BTreeSet<String>,
    },
}

impl GlobalResourceTargetSelector {
    pub fn validate(&self) -> CoreResult<()> {
        match self {
            Self::All => Ok(()),
            Self::ClusterIds { cluster_ids } => validate_selector_values(cluster_ids, "cluster ID"),
            Self::ControllerMetadata { environments, tags } => {
                if environments.is_empty() && tags.is_empty() {
                    return Err(CoreError::Conflict(
                        "global resource controller metadata selector must not be empty"
                            .to_string(),
                    ));
                }
                if !environments.is_empty() {
                    validate_selector_values(environments, "environment")?;
                }
                if !tags.is_empty() {
                    validate_selector_values(tags, "tag")?;
                }
                Ok(())
            }
        }
    }
}

/// Caller-controlled durable intent. The document is a complete resource
/// envelope, not a spec fragment or an observed Controller response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlobalResourceDesired {
    pub display_name: String,
    pub resource_kind: GlobalResourceInventoryKind,
    pub target_namespace: String,
    pub template_document: Value,
    pub target_selector: GlobalResourceTargetSelector,
    #[serde(default)]
    pub sync_policy: GlobalResourceSyncPolicy,
}

impl GlobalResourceDesired {
    pub fn validate(&self, config: &GlobalResourcesConfig) -> CoreResult<()> {
        config.validate().map_err(CoreError::Conflict)?;
        validate_text(
            &self.display_name,
            "global resource display name",
            MAX_DISPLAY_NAME_BYTES,
        )?;
        if !config
            .platform_namespaces
            .iter()
            .any(|namespace| namespace == &self.target_namespace)
        {
            return Err(CoreError::Conflict(format!(
                "global resource namespace '{}' is not a configured platform namespace",
                self.target_namespace
            )));
        }
        self.target_selector.validate()?;
        validate_template_document(
            &self.template_document,
            self.resource_kind,
            &self.target_namespace,
        )
    }

    pub fn revision(&self, config: &GlobalResourcesConfig) -> CoreResult<GlobalResourceRevision> {
        self.validate(config)?;
        GlobalResourceRevision::from_desired(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlobalResource {
    pub id: GlobalResourceId,
    pub generation: u64,
    pub desired_revision: GlobalResourceRevision,
    pub desired: GlobalResourceDesired,
    pub created_by: String,
    pub updated_by: String,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
}

impl GlobalResource {
    pub fn validate(&self, config: &GlobalResourcesConfig) -> CoreResult<()> {
        self.id.validate()?;
        if self.generation == 0 || self.generation > MAX_GLOBAL_RESOURCE_GENERATION {
            return Err(CoreError::Conflict(
                "global resource generation is outside the persistence range".to_string(),
            ));
        }
        validate_text(&self.created_by, "global resource creator", MAX_ACTOR_BYTES)?;
        validate_text(&self.updated_by, "global resource updater", MAX_ACTOR_BYTES)?;
        if self.created_at_unix_ms < 0 || self.updated_at_unix_ms < self.created_at_unix_ms {
            return Err(CoreError::Conflict(
                "global resource timestamps are invalid".to_string(),
            ));
        }
        let expected_revision = self.desired.revision(config)?;
        if self.desired_revision != expected_revision {
            return Err(CoreError::Conflict(
                "global resource desired revision does not match desired state".to_string(),
            ));
        }
        Ok(())
    }
}

pub fn global_resource_for_create(
    id: GlobalResourceId,
    desired: GlobalResourceDesired,
    actor: impl Into<String>,
    now_unix_ms: i64,
    config: &GlobalResourcesConfig,
) -> CoreResult<GlobalResource> {
    id.validate()?;
    let actor = actor.into();
    validate_text(&actor, "global resource creator", MAX_ACTOR_BYTES)?;
    if now_unix_ms < 0 {
        return Err(CoreError::Conflict(
            "global resource timestamp is invalid".to_string(),
        ));
    }
    let desired_revision = desired.revision(config)?;
    let resource = GlobalResource {
        id,
        generation: 1,
        desired_revision,
        desired,
        created_by: actor.clone(),
        updated_by: actor,
        created_at_unix_ms: now_unix_ms,
        updated_at_unix_ms: now_unix_ms,
    };
    resource.validate(config)?;
    Ok(resource)
}

pub fn global_resource_for_replace(
    current: &GlobalResource,
    desired: GlobalResourceDesired,
    actor: impl Into<String>,
    now_unix_ms: i64,
    config: &GlobalResourcesConfig,
) -> CoreResult<GlobalResource> {
    current.validate(config)?;
    if current.generation == MAX_GLOBAL_RESOURCE_GENERATION {
        return Err(CoreError::Conflict(
            "global resource generation cannot be incremented".to_string(),
        ));
    }
    let actor = actor.into();
    validate_text(&actor, "global resource updater", MAX_ACTOR_BYTES)?;
    if now_unix_ms < current.updated_at_unix_ms {
        return Err(CoreError::Conflict(
            "global resource update timestamp precedes current state".to_string(),
        ));
    }
    let desired_revision = desired.revision(config)?;
    let resource = GlobalResource {
        id: current.id.clone(),
        generation: current.generation + 1,
        desired_revision,
        desired,
        created_by: current.created_by.clone(),
        updated_by: actor,
        created_at_unix_ms: current.created_at_unix_ms,
        updated_at_unix_ms: now_unix_ms,
    };
    resource.validate(config)?;
    Ok(resource)
}

/// Validates a caller-supplied CAS generation before an adapter starts a
/// transaction. The upper bound leaves room for the store-assigned successor.
pub fn validate_global_resource_expected_generation(expected_generation: u64) -> CoreResult<()> {
    if expected_generation == 0 || expected_generation >= MAX_GLOBAL_RESOURCE_GENERATION {
        return Err(CoreError::Conflict(
            "global resource expected generation is outside the replaceable range".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalResourcePageRequest {
    pub limit: u16,
    /// Exclusive keyset boundary using exact UTF-8 byte ordering.
    pub after: Option<GlobalResourceId>,
}

impl GlobalResourcePageRequest {
    pub fn validate(&self) -> CoreResult<()> {
        if self.limit == 0 || self.limit > MAX_GLOBAL_RESOURCE_PAGE_SIZE {
            return Err(CoreError::Conflict(
                "global resource page size is invalid".to_string(),
            ));
        }
        if let Some(after) = self.after.as_ref() {
            after.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalResourcePage {
    pub items: Vec<GlobalResource>,
    /// Present only when another page exists; equals the last returned ID.
    pub next: Option<GlobalResourceId>,
}

impl GlobalResourcePage {
    pub fn validate(
        &self,
        request: &GlobalResourcePageRequest,
        config: &GlobalResourcesConfig,
    ) -> CoreResult<()> {
        request.validate()?;
        if self.items.len() > usize::from(request.limit) {
            return Err(CoreError::Conflict(
                "global resource response page exceeds the requested size".to_string(),
            ));
        }
        let mut previous = request.after.as_ref();
        for resource in &self.items {
            resource.validate(config)?;
            if previous.is_some_and(|id| id.as_str().as_bytes() >= resource.id.as_str().as_bytes())
            {
                return Err(CoreError::Conflict(
                    "global resource response page is not strictly ordered".to_string(),
                ));
            }
            previous = Some(&resource.id);
        }
        if let Some(next) = self.next.as_ref() {
            next.validate()?;
            if self.items.last().map(|resource| &resource.id) != Some(next) {
                return Err(CoreError::Conflict(
                    "global resource next boundary does not match the page".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalResourceCreateResult {
    Created(Box<GlobalResource>),
    AlreadyExists,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalResourceReplaceResult {
    Stored(Box<GlobalResource>),
    NotFound,
    GenerationMismatch { actual_generation: u64 },
}

#[async_trait::async_trait]
pub trait GlobalResourceStore: Send + Sync {
    /// Creates generation one. Duplicate identity must preserve existing state.
    async fn create(
        &self,
        id: &GlobalResourceId,
        desired: &GlobalResourceDesired,
        actor: &str,
        now_unix_ms: i64,
    ) -> CoreResult<GlobalResourceCreateResult>;

    async fn get(&self, id: &GlobalResourceId) -> CoreResult<Option<GlobalResource>>;

    /// Returns resources in exact byte ordering by Center-owned ID. Adapters
    /// should fetch `limit + 1` records to determine whether `next` is present.
    async fn list(&self, page: &GlobalResourcePageRequest) -> CoreResult<GlobalResourcePage>;

    /// Atomically stores exactly the next generation if current generation
    /// equals `expected_generation`. Stale writes must never be retried against
    /// newer state. Implementations must call
    /// [`validate_global_resource_expected_generation`] before persistence.
    async fn replace_if_generation(
        &self,
        id: &GlobalResourceId,
        expected_generation: u64,
        desired: &GlobalResourceDesired,
        actor: &str,
        now_unix_ms: i64,
    ) -> CoreResult<GlobalResourceReplaceResult>;
}

fn validate_template_document(
    document: &Value,
    expected_kind: GlobalResourceInventoryKind,
    expected_namespace: &str,
) -> CoreResult<()> {
    let encoded = serde_json::to_vec(document)
        .map_err(|_| CoreError::Conflict("global resource template is invalid".to_string()))?;
    if encoded.len() > MAX_GLOBAL_RESOURCE_TEMPLATE_BYTES {
        return Err(CoreError::Conflict(
            "global resource template exceeds its persistence limit".to_string(),
        ));
    }
    let root = document.as_object().ok_or_else(|| {
        CoreError::Conflict("global resource template must be an object".to_string())
    })?;
    validate_required_string(root, "apiVersion", "global resource apiVersion")?;
    let kind = validate_required_string(root, "kind", "global resource kind")?;
    if kind != expected_kind.as_str() {
        return Err(CoreError::Conflict(
            "global resource template kind does not match resource kind".to_string(),
        ));
    }
    if root.contains_key("status") {
        return Err(CoreError::Conflict(
            "global resource template must not contain status".to_string(),
        ));
    }
    let metadata = root
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CoreError::Conflict("global resource template metadata must be an object".to_string())
        })?;
    validate_kubernetes_name(validate_required_string(
        metadata,
        "name",
        "global resource template metadata.name",
    )?)?;
    let namespace = validate_required_string(
        metadata,
        "namespace",
        "global resource template metadata.namespace",
    )?;
    if namespace != expected_namespace {
        return Err(CoreError::Conflict(
            "global resource template namespace does not match target namespace".to_string(),
        ));
    }
    if let Some(field) = FORBIDDEN_METADATA_FIELDS
        .iter()
        .find(|field| metadata.contains_key(**field))
    {
        return Err(CoreError::Conflict(format!(
            "global resource template must not contain metadata.{field}"
        )));
    }
    validate_no_reserved_global_resource_provenance(metadata)?;
    Ok(())
}

fn validate_required_string<'a>(
    object: &'a Map<String, Value>,
    field: &'static str,
    kind: &'static str,
) -> CoreResult<&'a str> {
    let value = object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::Conflict(format!("{kind} must be a string")))?;
    validate_text(value, kind, 256)?;
    Ok(value)
}

fn validate_selector_values(values: &BTreeSet<String>, kind: &'static str) -> CoreResult<()> {
    if values.is_empty() || values.len() > MAX_CLUSTER_SELECTOR_VALUES {
        return Err(CoreError::Conflict(format!(
            "global resource {kind} selector size is invalid"
        )));
    }
    for value in values {
        validate_text(
            value,
            "global resource selector value",
            MAX_CLUSTER_SELECTOR_VALUE_BYTES,
        )?;
    }
    Ok(())
}

fn validate_text(value: &str, kind: &'static str, max_bytes: usize) -> CoreResult<()> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(CoreError::Conflict(format!("{kind} is invalid")));
    }
    Ok(())
}

fn validate_kubernetes_name(value: &str) -> CoreResult<()> {
    if !is_dns_subdomain(value) || value.len() > 253 {
        return Err(CoreError::Conflict(
            "global resource template metadata.name is invalid".to_string(),
        ));
    }
    Ok(())
}

fn is_dns_subdomain(value: &str) -> bool {
    if value.is_empty() || value.len() > 253 {
        return false;
    }
    value.split('.').all(|label| {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        let bytes = label.as_bytes();
        let edge = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
        edge(bytes[0])
            && edge(bytes[bytes.len() - 1])
            && bytes.iter().all(|byte| edge(*byte) || *byte == b'-')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn desired() -> GlobalResourceDesired {
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
            target_selector: GlobalResourceTargetSelector::ClusterIds {
                cluster_ids: BTreeSet::from(["cluster-a".to_string(), "cluster-b".to_string()]),
            },
            sync_policy: GlobalResourceSyncPolicy::default(),
        }
    }

    #[test]
    fn complete_desired_state_has_stable_revision() {
        let config = GlobalResourcesConfig::default();
        let desired = desired();
        desired.validate(&config).unwrap();
        assert_eq!(
            desired.revision(&config).unwrap(),
            desired.revision(&config).unwrap()
        );

        let mut changed = desired.clone();
        changed.display_name.push_str(" v2");
        assert_ne!(
            desired.revision(&config).unwrap(),
            changed.revision(&config).unwrap()
        );
    }

    #[test]
    fn rejects_namespace_kind_and_cluster_owned_template_fields() {
        let config = GlobalResourcesConfig::default();
        let mut value = desired();
        value.target_namespace = "tenant-a".to_string();
        assert!(value.validate(&config).is_err());

        let mut value = desired();
        value.template_document["kind"] = json!("Secret");
        assert!(value.validate(&config).is_err());

        for path in [
            "resourceVersion",
            "uid",
            "generation",
            "creationTimestamp",
            "managedFields",
        ] {
            let mut value = desired();
            value.template_document["metadata"][path] = json!("forbidden");
            assert!(value.validate(&config).is_err(), "{path}");
        }

        let mut value = desired();
        value.template_document["status"] = json!({"accepted": true});
        assert!(value.validate(&config).is_err());

        for (field, key) in [
            ("labels", crate::GLOBAL_RESOURCE_MANAGED_BY_LABEL),
            ("annotations", crate::GLOBAL_RESOURCE_ID_ANNOTATION),
            ("annotations", crate::GLOBAL_RESOURCE_REVISION_ANNOTATION),
        ] {
            let mut value = desired();
            value.template_document["metadata"][field][key] = json!("spoofed");
            assert!(value.validate(&config).is_err(), "{field}.{key}");
        }

        for field in ["labels", "annotations"] {
            let mut value = desired();
            value.template_document["metadata"][field] = json!("not-an-object");
            assert!(value.validate(&config).is_err(), "metadata.{field}");
        }
    }

    #[test]
    fn rejects_incomplete_or_oversized_templates() {
        let config = GlobalResourcesConfig::default();
        for template in [
            json!(null),
            json!({"kind": "EdgionConfigData", "metadata": {}}),
            json!({
                "apiVersion": "edgion.io/v1alpha1",
                "kind": "EdgionConfigData",
                "metadata": {"name": "x", "namespace": "edgion-system"}
            }),
        ] {
            let mut value = desired();
            value.template_document = template;
            assert!(value.validate(&config).is_err());
        }
        let mut value = desired();
        value.template_document["data"] = json!("x".repeat(MAX_GLOBAL_RESOURCE_TEMPLATE_BYTES));
        assert!(value.validate(&config).is_err());
    }

    #[test]
    fn selectors_are_explicit_and_bounded() {
        assert!(GlobalResourceTargetSelector::All.validate().is_ok());
        assert!(GlobalResourceTargetSelector::ClusterIds {
            cluster_ids: BTreeSet::new()
        }
        .validate()
        .is_err());
        assert!(GlobalResourceTargetSelector::ControllerMetadata {
            environments: BTreeSet::new(),
            tags: BTreeSet::new()
        }
        .validate()
        .is_err());
        assert!(GlobalResourceTargetSelector::ControllerMetadata {
            environments: BTreeSet::from(["production".to_string()]),
            tags: BTreeSet::from(["edge".to_string()])
        }
        .validate()
        .is_ok());
        assert!(GlobalResourceTargetSelector::ControllerMetadata {
            environments: BTreeSet::from(["production".to_string()]),
            tags: BTreeSet::new()
        }
        .validate()
        .is_ok());
        assert!(GlobalResourceTargetSelector::ControllerMetadata {
            environments: BTreeSet::new(),
            tags: BTreeSet::from(["edge".to_string()])
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn create_and_replace_assign_generation_revision_and_audit_metadata() {
        let config = GlobalResourcesConfig::default();
        let created = global_resource_for_create(
            GlobalResourceId::new("shared-allow-list").unwrap(),
            desired(),
            "alice",
            100,
            &config,
        )
        .unwrap();
        assert_eq!(created.generation, 1);
        assert_eq!(created.created_by, "alice");
        assert_eq!(created.updated_by, "alice");

        let mut next_desired = desired();
        next_desired.display_name = "Shared allow list v2".to_string();
        let replaced =
            global_resource_for_replace(&created, next_desired, "bob", 200, &config).unwrap();
        assert_eq!(replaced.generation, 2);
        assert_ne!(replaced.desired_revision, created.desired_revision);
        assert_eq!(replaced.created_by, "alice");
        assert_eq!(replaced.updated_by, "bob");
        assert_eq!(replaced.created_at_unix_ms, 100);
        assert_eq!(replaced.updated_at_unix_ms, 200);
        assert!(global_resource_for_replace(&replaced, desired(), "bob", 199, &config).is_err());
    }

    #[test]
    fn persisted_revision_and_generation_are_revalidated() {
        let config = GlobalResourcesConfig::default();
        let resource = global_resource_for_create(
            GlobalResourceId::new("shared-allow-list").unwrap(),
            desired(),
            "alice",
            100,
            &config,
        )
        .unwrap();
        let mut invalid = resource.clone();
        invalid.generation = 0;
        assert!(invalid.validate(&config).is_err());
        let mut invalid = resource;
        invalid.desired_revision = GlobalResourceRevision("sha256:wrong".to_string());
        assert!(invalid.validate(&config).is_err());
        assert!(validate_global_resource_expected_generation(0).is_err());
        assert!(validate_global_resource_expected_generation(1).is_ok());
        assert!(
            validate_global_resource_expected_generation(MAX_GLOBAL_RESOURCE_GENERATION).is_err()
        );
    }

    #[test]
    fn page_contract_is_bounded_strictly_ordered_and_exclusive() {
        let config = GlobalResourcesConfig::default();
        let request = GlobalResourcePageRequest {
            limit: 2,
            after: Some(GlobalResourceId::new("a").unwrap()),
        };
        let make = |id: &str| {
            global_resource_for_create(
                GlobalResourceId::new(id).unwrap(),
                desired(),
                "alice",
                100,
                &config,
            )
            .unwrap()
        };
        let page = GlobalResourcePage {
            items: vec![make("b"), make("c")],
            next: Some(GlobalResourceId::new("c").unwrap()),
        };
        page.validate(&request, &config).unwrap();

        let invalid = GlobalResourcePage {
            items: vec![make("c"), make("b")],
            next: None,
        };
        assert!(invalid.validate(&request, &config).is_err());
        assert!(GlobalResourcePageRequest {
            limit: MAX_GLOBAL_RESOURCE_PAGE_SIZE + 1,
            after: None
        }
        .validate()
        .is_err());
    }
}
