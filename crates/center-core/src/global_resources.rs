use serde::{Deserialize, Serialize};

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
    RequestAccessUrlAllowList,
    ProxyProtocolTrust,
    WafRuleBundle,
    WafPolicy,
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
            EdgionConfigDataType::RequestAccessUrlAllowList,
            EdgionConfigDataType::ProxyProtocolTrust,
            EdgionConfigDataType::WafRuleBundle,
            EdgionConfigDataType::WafPolicy,
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

#[cfg(test)]
mod tests {
    use super::*;

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
                EdgionConfigDataType::RequestAccessUrlAllowList,
                EdgionConfigDataType::ProxyProtocolTrust,
                EdgionConfigDataType::WafRuleBundle,
                EdgionConfigDataType::WafPolicy,
                EdgionConfigDataType::Selector,
                EdgionConfigDataType::RegionRouteOverride,
                EdgionConfigDataType::Misc
            ]
        );
    }
}
