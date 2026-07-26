use edgion_center_core::{GlobalResource, GlobalResourcesConfig};
use kube::{CustomResource, KubeSchema};
use serde::{Deserialize, Serialize};

/// Cluster-scoped persistence envelope for Center-owned GlobalResource intent.
///
/// The serialized desired state contains only Center intent. Observations read
/// from managed clusters and per-target synchronization results are deliberately
/// excluded from this CRD.
#[derive(CustomResource, KubeSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[kube(
    group = "center.edgion.io",
    version = "v1alpha1",
    kind = "EdgionGlobalResource",
    plural = "edgionglobalresources",
    shortname = "egr"
)]
#[x_kube(validation = "self == oldSelf || self.generation == oldSelf.generation + 1")]
#[serde(rename_all = "camelCase")]
pub struct EdgionGlobalResourceSpec {
    #[x_kube(validation = "self == oldSelf")]
    pub contract_version: i32,
    /// Immutable Center identity retained to detect digest-name collisions.
    #[x_kube(validation = "self != ''", validation = "self == oldSelf")]
    #[schemars(length(max = 253))]
    pub global_resource_id: String,
    /// Store-owned desired-state generation. It advances on every successful
    /// replacement, including replacement with otherwise identical intent.
    #[x_kube(validation = "self > 0")]
    #[schemars(range(min = 1))]
    pub generation: i64,
    /// Content-addressed revision of `resourceJson.desired`. Unlike
    /// `generation`, this may remain unchanged for an otherwise identical
    /// replacement.
    #[x_kube(validation = "self.matches('^sha256:[0-9a-f]{64}$')")]
    #[schemars(length(min = 71, max = 71))]
    pub desired_revision: String,
    /// Canonical, serialized GlobalResource intent envelope. This includes
    /// actor/timestamp metadata but never managed-cluster observations.
    #[schemars(length(min = 1, max = 1048576))]
    pub resource_json: String,
}

impl EdgionGlobalResourceSpec {
    pub(crate) fn new(resource: &GlobalResource) -> Result<Self, String> {
        let generation = i64::try_from(resource.generation)
            .map_err(|_| "global resource generation exceeds Kubernetes range".to_string())?;
        let resource_json = serde_json::to_string(resource)
            .map_err(|_| "global resource intent is not serializable".to_string())?;
        Ok(Self {
            contract_version: 1,
            global_resource_id: resource.id.to_string(),
            generation,
            desired_revision: resource.desired_revision.as_str().to_string(),
            resource_json,
        })
    }

    pub(crate) fn to_core(&self, config: &GlobalResourcesConfig) -> Result<GlobalResource, String> {
        if self.contract_version != 1 {
            return Err("unsupported global resource CRD contract version".to_string());
        }
        let resource: GlobalResource = serde_json::from_str(&self.resource_json)
            .map_err(|_| "global resource CRD contains invalid intent".to_string())?;
        let canonical = serde_json::to_string(&resource)
            .map_err(|_| "global resource intent is not serializable".to_string())?;
        if canonical != self.resource_json {
            return Err(
                "global resource CRD intent is not the canonical observation-free shape"
                    .to_string(),
            );
        }
        if resource.id.as_str().as_bytes() != self.global_resource_id.as_bytes() {
            return Err(
                "global resource CRD identity does not match serialized intent".to_string(),
            );
        }
        let generation = u64::try_from(self.generation)
            .map_err(|_| "global resource CRD has a non-positive generation".to_string())?;
        if generation != resource.generation {
            return Err(
                "global resource CRD generation does not match serialized intent".to_string(),
            );
        }
        if self.desired_revision.as_bytes() != resource.desired_revision.as_str().as_bytes() {
            return Err(
                "global resource CRD desired revision does not match serialized intent".to_string(),
            );
        }
        resource
            .validate(config)
            .map_err(|error| error.to_string())?;
        Ok(resource)
    }
}

#[cfg(test)]
mod tests {
    use kube::CustomResourceExt;

    use super::*;

    #[test]
    fn generated_crd_is_cluster_scoped_and_fences_identity_and_revision() {
        let crd = EdgionGlobalResource::crd();
        assert_eq!(crd.spec.scope, "Cluster");
        let schema = serde_json::to_value(crd).unwrap();
        let spec = schema
            .pointer("/spec/versions/0/schema/openAPIV3Schema/properties/spec")
            .unwrap();
        assert_eq!(
            spec.pointer("/properties/globalResourceId/maxLength"),
            Some(&serde_json::json!(253))
        );
        assert_eq!(
            spec.pointer("/properties/resourceJson/maxLength"),
            Some(&serde_json::json!(1048576))
        );
        assert_eq!(
            spec.pointer("/properties/desiredRevision/minLength"),
            Some(&serde_json::json!(71))
        );
        let revision_validations = spec
            .pointer("/properties/desiredRevision/x-kubernetes-validations")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(revision_validations
            .iter()
            .any(|validation| { validation["rule"] == "self.matches('^sha256:[0-9a-f]{64}$')" }));
        for field in ["contractVersion", "globalResourceId"] {
            let validations = spec
                .pointer(&format!("/properties/{field}/x-kubernetes-validations"))
                .and_then(serde_json::Value::as_array)
                .unwrap();
            assert!(validations
                .iter()
                .any(|validation| validation["rule"] == "self == oldSelf"));
        }
        let validations = spec
            .pointer("/x-kubernetes-validations")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(validations.iter().any(|validation| {
            validation["rule"] == "self == oldSelf || self.generation == oldSelf.generation + 1"
        }));
    }

    #[test]
    fn schema_has_no_status_or_observed_payload_fields() {
        let schema = serde_json::to_value(EdgionGlobalResource::crd()).unwrap();
        let root = schema
            .pointer("/spec/versions/0/schema/openAPIV3Schema/properties")
            .unwrap();
        assert!(root.get("status").is_none());
        let spec = root.pointer("/spec/properties").unwrap();
        assert!(spec.get("observedJson").is_none());
        assert!(spec.get("observedResources").is_none());
        assert!(spec.get("targetResults").is_none());
        assert!(spec.get("operationHistory").is_none());
    }

    #[test]
    fn checked_in_manifest_matches_generated_critical_schema_constraints() {
        let generated = serde_json::to_value(EdgionGlobalResource::crd()).unwrap();
        let checked_in: serde_json::Value = serde_yaml::from_str(include_str!(
            "../../../cicd/deploy/center-kubernetes/global-resource-crd.yaml"
        ))
        .unwrap();
        for pointer in [
            "/spec/group",
            "/spec/scope",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/x-kubernetes-validations",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/globalResourceId/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/generation/minimum",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/desiredRevision/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/resourceJson/maxLength",
        ] {
            assert_eq!(
                checked_in.pointer(pointer),
                generated.pointer(pointer),
                "CRD schema drift at {pointer}"
            );
        }
    }
}
