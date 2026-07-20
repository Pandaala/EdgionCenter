use edgion_center_core::{
    provider_account_from_desired, CloudResourceId, ProviderAccount, ProviderAccountDesired,
};
use kube::{CustomResource, KubeSchema};
use serde::{Deserialize, Serialize};

#[derive(CustomResource, KubeSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[kube(
    group = "center.edgion.io",
    version = "v1alpha1",
    kind = "EdgionProviderAccount",
    plural = "edgionprovideraccounts",
    namespaced,
    shortname = "epa"
)]
#[x_kube(validation = "self == oldSelf || self.desiredGeneration == oldSelf.desiredGeneration + 1")]
#[serde(rename_all = "camelCase")]
pub struct EdgionProviderAccountSpec {
    #[x_kube(validation = "self == oldSelf")]
    pub contract_version: i32,
    /// Immutable Center identity retained to detect digest-name collisions.
    #[x_kube(validation = "self != ''", validation = "self == oldSelf")]
    #[schemars(length(max = 512))]
    pub account_id: String,
    /// Store-owned desired-state generation. It changes on every successful
    /// replacement, including a replacement with otherwise identical intent.
    #[x_kube(validation = "self > 0")]
    #[schemars(range(min = 1))]
    pub desired_generation: i64,
    /// Serialized ProviderAccountDesired. Secret values are not part of that
    /// core contract; credential sources contain only references/selectors.
    #[schemars(length(min = 1, max = 65536))]
    pub desired_json: String,
}

impl EdgionProviderAccountSpec {
    pub(crate) fn new(
        account_id: &CloudResourceId,
        desired_generation: u64,
        desired: &ProviderAccountDesired,
    ) -> Result<Self, String> {
        let desired_generation = i64::try_from(desired_generation)
            .map_err(|_| "provider account generation exceeds Kubernetes range".to_string())?;
        let desired_json = serde_json::to_string(desired)
            .map_err(|_| "provider account desired state is not serializable".to_string())?;
        Ok(Self {
            contract_version: 1,
            account_id: account_id.to_string(),
            desired_generation,
            desired_json,
        })
    }

    pub(crate) fn to_core(&self) -> Result<ProviderAccount, String> {
        if self.contract_version != 1 {
            return Err("unsupported provider account CRD contract version".to_string());
        }
        let account_id =
            CloudResourceId::new(self.account_id.clone()).map_err(|error| error.to_string())?;
        let generation = u64::try_from(self.desired_generation)
            .map_err(|_| "provider account CRD has a non-positive generation".to_string())?;
        let desired: ProviderAccountDesired = serde_json::from_str(&self.desired_json)
            .map_err(|_| "provider account CRD contains invalid desired state".to_string())?;
        let canonical = serde_json::to_string(&desired)
            .map_err(|_| "provider account desired state is not serializable".to_string())?;
        if canonical != self.desired_json {
            return Err(
                "provider account CRD desired state is not the canonical secret-free shape"
                    .to_string(),
            );
        }
        provider_account_from_desired(account_id, generation, &desired)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use kube::CustomResourceExt;

    use super::*;

    #[test]
    fn generated_crd_is_namespaced_and_preserves_immutable_identity() {
        let crd = EdgionProviderAccount::crd();
        assert_eq!(crd.spec.scope, "Namespaced");
        let schema = serde_json::to_value(crd).unwrap();
        for field in ["contractVersion", "accountId"] {
            let validations = schema
                .pointer(&format!("/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/{field}/x-kubernetes-validations"))
                .and_then(serde_json::Value::as_array)
                .unwrap();
            assert!(validations
                .iter()
                .any(|validation| validation["rule"] == "self == oldSelf"));
        }
    }

    #[test]
    fn checked_in_manifest_matches_generated_critical_schema_constraints() {
        let generated = serde_json::to_value(EdgionProviderAccount::crd()).unwrap();
        let checked_in: serde_json::Value = serde_yaml::from_str(include_str!(
            "../../../cicd/deploy/center-kubernetes/provider-account-crd.yaml"
        ))
        .unwrap();
        for pointer in [
            "/spec/group",
            "/spec/scope",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/x-kubernetes-validations",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/accountId/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/desiredGeneration/format",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/desiredGeneration/minimum",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/desiredJson/maxLength",
        ] {
            assert_eq!(
                checked_in.pointer(pointer),
                generated.pointer(pointer),
                "CRD schema drift at {pointer}"
            );
        }
    }
}
