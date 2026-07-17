use edgion_center_core::{
    CapabilityDiscoveryFence, CapabilityScope, CapabilitySnapshotKey, CloudResourceId,
    CloudResourceKind, DiscoveryToken, ProviderResourceRef,
};
use kube::{CustomResource, KubeSchema};
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(CustomResource, KubeSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[kube(
    group = "center.edgion.io",
    version = "v1alpha1",
    kind = "EdgionProviderCapabilitySnapshot",
    plural = "edgionprovidercapabilitysnapshots",
    namespaced,
    status = "EdgionProviderCapabilitySnapshotStatus",
    shortname = "ecap"
)]
#[serde(rename_all = "camelCase")]
pub struct EdgionProviderCapabilitySnapshotSpec {
    #[x_kube(validation = "self == oldSelf")]
    pub contract_version: i32,
    #[x_kube(validation = "self != ''", validation = "self == oldSelf")]
    #[schemars(length(max = 512))]
    pub provider_account_id: String,
    #[x_kube(validation = "self == oldSelf")]
    pub scope: EdgionCapabilityScopeSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgionCapabilityScopeSpec {
    #[schemars(length(min = 1, max = 32))]
    pub scope_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 128))]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 64))]
    pub resource_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 512))]
    pub provider_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 1024))]
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgionProviderCapabilitySnapshotStatus {
    #[schemars(range(min = 0))]
    pub last_discovery_epoch: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<EdgionCapabilityAuthorityStatus>,
    /// A serialized core snapshot. Keeping the payload behind the core serde
    /// contract prevents Kubernetes schema pruning from changing its meaning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = 524288))]
    pub snapshot_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgionCapabilityAuthorityStatus {
    #[schemars(range(min = 1))]
    pub provider_account_generation: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 512))]
    pub credential_revision: Option<String>,
    #[schemars(range(min = 1))]
    pub discovery_epoch: i64,
    #[schemars(length(min = 1, max = 512))]
    pub discovery_token: String,
}

impl EdgionProviderCapabilitySnapshotSpec {
    pub(crate) fn from_key(key: &CapabilitySnapshotKey) -> Self {
        Self {
            contract_version: 1,
            provider_account_id: key.provider_account_id.to_string(),
            scope: EdgionCapabilityScopeSpec::from(&key.scope),
        }
    }

    pub(crate) fn to_key(&self) -> Result<CapabilitySnapshotKey, String> {
        if self.contract_version != 1 {
            return Err("unsupported capability snapshot CRD contract version".to_string());
        }
        let key = CapabilitySnapshotKey {
            provider_account_id: CloudResourceId::new(self.provider_account_id.clone())
                .map_err(|error| error.to_string())?,
            scope: self.scope.to_core()?,
        };
        key.validate().map_err(|error| error.to_string())?;
        Ok(key)
    }
}

impl From<&CapabilityScope> for EdgionCapabilityScopeSpec {
    fn from(scope: &CapabilityScope) -> Self {
        match scope {
            CapabilityScope::Account => Self {
                scope_type: "account".to_string(),
                region: None,
                resource_kind: None,
                provider_account_id: None,
                external_id: None,
            },
            CapabilityScope::Region { region } => Self {
                scope_type: "region".to_string(),
                region: Some(region.to_string()),
                resource_kind: None,
                provider_account_id: None,
                external_id: None,
            },
            CapabilityScope::Resource {
                resource_kind,
                resource,
            } => Self {
                scope_type: "resource".to_string(),
                region: None,
                resource_kind: Some(json_scalar(resource_kind)),
                provider_account_id: Some(resource.provider_account_id.to_string()),
                external_id: Some(resource.external_id.clone()),
            },
        }
    }
}

impl EdgionCapabilityScopeSpec {
    fn to_core(&self) -> Result<CapabilityScope, String> {
        match self.scope_type.as_str() {
            "account"
                if self.region.is_none()
                    && self.resource_kind.is_none()
                    && self.provider_account_id.is_none()
                    && self.external_id.is_none() =>
            {
                Ok(CapabilityScope::Account)
            }
            "region"
                if self.resource_kind.is_none()
                    && self.provider_account_id.is_none()
                    && self.external_id.is_none() =>
            {
                Ok(CapabilityScope::Region {
                    region: edgion_center_core::ProviderRegion::new(
                        self.region
                            .clone()
                            .ok_or("capability region scope omitted region")?,
                    )
                    .map_err(|error| error.to_string())?,
                })
            }
            "resource" if self.region.is_none() => Ok(CapabilityScope::Resource {
                resource_kind: parse_scalar::<CloudResourceKind>(
                    self.resource_kind
                        .as_deref()
                        .ok_or("capability resource scope omitted resource kind")?,
                )?,
                resource: ProviderResourceRef {
                    provider_account_id: CloudResourceId::new(
                        self.provider_account_id
                            .clone()
                            .ok_or("capability resource scope omitted provider account")?,
                    )
                    .map_err(|error| error.to_string())?,
                    external_id: self
                        .external_id
                        .clone()
                        .ok_or("capability resource scope omitted external id")?,
                },
            }),
            _ => Err("capability snapshot CRD scope shape is invalid".to_string()),
        }
    }
}

impl EdgionCapabilityAuthorityStatus {
    pub(crate) fn from_core(fence: &CapabilityDiscoveryFence) -> Self {
        Self {
            provider_account_generation: i64::try_from(fence.provider_account_generation)
                .expect("validated capability account generation"),
            credential_revision: fence.credential_revision.clone(),
            discovery_epoch: i64::try_from(fence.discovery_epoch)
                .expect("validated capability discovery epoch"),
            discovery_token: fence.discovery_token.as_str().to_string(),
        }
    }

    pub(crate) fn to_core(&self) -> Result<CapabilityDiscoveryFence, String> {
        let fence = CapabilityDiscoveryFence {
            provider_account_generation: u64::try_from(self.provider_account_generation)
                .map_err(|_| "negative capability account generation".to_string())?,
            credential_revision: self.credential_revision.clone(),
            discovery_epoch: u64::try_from(self.discovery_epoch)
                .map_err(|_| "negative capability discovery epoch".to_string())?,
            discovery_token: DiscoveryToken::new(self.discovery_token.clone())
                .map_err(|error| error.to_string())?,
        };
        fence.validate().map_err(|error| error.to_string())?;
        Ok(fence)
    }
}

fn json_scalar<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .expect("serializable capability CRD scalar")
        .as_str()
        .expect("capability CRD scalar string")
        .to_string()
}

fn parse_scalar<T: DeserializeOwned>(value: &str) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use kube::CustomResourceExt;

    use super::*;

    #[test]
    fn generated_crd_is_namespaced_immutable_and_has_status_subresource() {
        let crd = EdgionProviderCapabilitySnapshot::crd();
        assert_eq!(crd.spec.scope, "Namespaced");
        let version = &crd.spec.versions[0];
        assert!(version
            .subresources
            .as_ref()
            .and_then(|subresources| subresources.status.as_ref())
            .is_some());
        let schema = serde_json::to_value(crd).unwrap();
        for field in ["contractVersion", "providerAccountId", "scope"] {
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
        let generated = serde_json::to_value(EdgionProviderCapabilitySnapshot::crd()).unwrap();
        let checked_in: serde_json::Value = serde_yaml::from_str(include_str!(
            "../../../cicd/deploy/center-kubernetes/cloud-capability-snapshot-crd.yaml"
        ))
        .unwrap();
        for pointer in [
            "/spec/group",
            "/spec/scope",
            "/spec/versions/0/subresources/status",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/providerAccountId/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/scope/properties/region/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/scope/properties/providerAccountId/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/scope/properties/externalId/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/spec/properties/contractVersion/format",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/lastDiscoveryEpoch/format",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/lastDiscoveryEpoch/minimum",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/authority/properties/providerAccountGeneration/minimum",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/authority/properties/discoveryEpoch/format",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/authority/properties/discoveryEpoch/minimum",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/authority/properties/credentialRevision/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/authority/properties/discoveryToken/maxLength",
            "/spec/versions/0/schema/openAPIV3Schema/properties/status/properties/snapshotJson/maxLength",
        ] {
            assert_eq!(
                checked_in.pointer(pointer),
                generated.pointer(pointer),
                "CRD schema drift at {pointer}"
            );
        }
    }
}
