use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const DEFAULT_PLATFORM_NAMESPACES: [&str; 2] = ["edgion-system", "edgion-global"];
pub const MAX_PLATFORM_NAMESPACES: usize = 32;
pub const GLOBAL_RESOURCE_CATALOG_VERSION: u8 = 1;
const FORBIDDEN_PLATFORM_NAMESPACES: [&str; 4] =
    ["default", "kube-system", "kube-public", "kube-node-lease"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GlobalResourceInventoryKind {
    HTTPRoute,
    GRPCRoute,
    EdgionPlugins,
    EdgionConfigData,
    ReferenceGrant,
}

impl GlobalResourceInventoryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HTTPRoute => "HTTPRoute",
            Self::GRPCRoute => "GRPCRoute",
            Self::EdgionPlugins => "EdgionPlugins",
            Self::EdgionConfigData => "EdgionConfigData",
            Self::ReferenceGrant => "ReferenceGrant",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgionConfigDataType {
    KeyList,
    IpList,
    Selector,
    RegionRouteOverride,
    Misc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalResourceCatalogEntry {
    pub kind: GlobalResourceInventoryKind,
    pub path_kind: &'static str,
    pub config_data_types: &'static [EdgionConfigDataType],
}

pub const GLOBAL_RESOURCE_KINDS: [GlobalResourceCatalogEntry; 5] = [
    GlobalResourceCatalogEntry {
        kind: GlobalResourceInventoryKind::HTTPRoute,
        path_kind: "httproute",
        config_data_types: &[],
    },
    GlobalResourceCatalogEntry {
        kind: GlobalResourceInventoryKind::GRPCRoute,
        path_kind: "grpcroute",
        config_data_types: &[],
    },
    GlobalResourceCatalogEntry {
        kind: GlobalResourceInventoryKind::EdgionPlugins,
        path_kind: "edgionplugins",
        config_data_types: &[],
    },
    GlobalResourceCatalogEntry {
        kind: GlobalResourceInventoryKind::EdgionConfigData,
        path_kind: "edgionconfigdata",
        config_data_types: &[
            EdgionConfigDataType::KeyList,
            EdgionConfigDataType::IpList,
            EdgionConfigDataType::Selector,
            EdgionConfigDataType::RegionRouteOverride,
            EdgionConfigDataType::Misc,
        ],
    },
    GlobalResourceCatalogEntry {
        kind: GlobalResourceInventoryKind::ReferenceGrant,
        path_kind: "referencegrant",
        config_data_types: &[],
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GlobalResourcesConfig {
    pub platform_namespaces: Vec<String>,
}

impl Default for GlobalResourcesConfig {
    fn default() -> Self {
        Self {
            platform_namespaces: DEFAULT_PLATFORM_NAMESPACES
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
        }
    }
}

impl GlobalResourcesConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.platform_namespaces.is_empty() {
            return Err("global_resources.platform_namespaces must not be empty".to_string());
        }
        if self.platform_namespaces.len() > MAX_PLATFORM_NAMESPACES {
            return Err(format!(
                "global_resources.platform_namespaces cannot contain more than {MAX_PLATFORM_NAMESPACES} entries"
            ));
        }

        let mut seen = HashSet::new();
        for namespace in &self.platform_namespaces {
            if namespace.is_empty() {
                return Err(
                    "global_resources.platform_namespaces must not contain empty values"
                        .to_string(),
                );
            }
            if namespace.trim() != namespace {
                return Err(format!(
                    "global_resources.platform_namespaces contains whitespace around '{namespace}'"
                ));
            }
            if !is_dns_label(namespace) {
                return Err(format!(
                    "global_resources.platform_namespaces contains malformed namespace '{namespace}'"
                ));
            }
            if FORBIDDEN_PLATFORM_NAMESPACES.contains(&namespace.as_str())
                || namespace.starts_with("kube-")
            {
                return Err(format!(
                    "global_resources.platform_namespaces cannot include system namespace '{namespace}'"
                ));
            }
            if !seen.insert(namespace.as_str()) {
                return Err(format!(
                    "global_resources.platform_namespaces contains duplicate namespace '{namespace}'"
                ));
            }
        }
        Ok(())
    }

    pub fn revision(&self) -> String {
        let mut namespaces = self.platform_namespaces.clone();
        namespaces.sort();
        let canonical = serde_json::to_vec(&(
            GLOBAL_RESOURCE_CATALOG_VERSION,
            namespaces,
            &GLOBAL_RESOURCE_KINDS,
        ))
        .expect("static GlobalResources catalog is serializable");
        format!("sha256:{}", hex::encode(Sha256::digest(canonical)))
    }
}

fn is_dns_label(value: &str) -> bool {
    if value.is_empty() || value.len() > 63 {
        return false;
    }
    let bytes = value.as_bytes();
    let edge_valid = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    edge_valid(bytes[0])
        && edge_valid(bytes[bytes.len() - 1])
        && bytes.iter().all(|byte| edge_valid(*byte) || *byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_two_edgion_platform_namespaces() {
        let config = GlobalResourcesConfig::default();
        assert_eq!(
            config.platform_namespaces,
            ["edgion-system", "edgion-global"]
        );
        config.validate().unwrap();
    }

    #[test]
    fn catalog_is_exact_and_config_data_types_are_explicit() {
        assert_eq!(
            GLOBAL_RESOURCE_KINDS
                .iter()
                .map(|entry| entry.kind.as_str())
                .collect::<Vec<_>>(),
            [
                "HTTPRoute",
                "GRPCRoute",
                "EdgionPlugins",
                "EdgionConfigData",
                "ReferenceGrant"
            ]
        );
        assert_eq!(
            GLOBAL_RESOURCE_KINDS[3].config_data_types,
            [
                EdgionConfigDataType::KeyList,
                EdgionConfigDataType::IpList,
                EdgionConfigDataType::Selector,
                EdgionConfigDataType::RegionRouteOverride,
                EdgionConfigDataType::Misc
            ]
        );
    }

    #[test]
    fn validation_rejects_empty_duplicate_malformed_and_system_namespaces() {
        for namespaces in [
            vec![],
            vec!["edgion-data".into(), "edgion-data".into()],
            vec!["Edgion-Data".into()],
            vec!["-edgion".into()],
            vec!["edgion_1".into()],
            vec![" edgion-data".into()],
            vec!["default".into()],
            vec!["kube-custom".into()],
            (0..=MAX_PLATFORM_NAMESPACES)
                .map(|index| format!("edgion-{index}"))
                .collect(),
        ] {
            assert!(GlobalResourcesConfig {
                platform_namespaces: namespaces
            }
            .validate()
            .is_err());
        }
    }

    #[test]
    fn revision_is_stable_across_namespace_order_but_changes_with_content() {
        let first = GlobalResourcesConfig {
            platform_namespaces: vec!["edgion-system".into(), "edgion-data".into()],
        };
        let reordered = GlobalResourcesConfig {
            platform_namespaces: vec!["edgion-data".into(), "edgion-system".into()],
        };
        let changed = GlobalResourcesConfig {
            platform_namespaces: vec!["edgion-system".into()],
        };
        assert_eq!(first.revision(), reordered.revision());
        assert_ne!(first.revision(), changed.revision());
    }
}
