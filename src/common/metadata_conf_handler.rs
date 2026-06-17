//! Shared EdgionConfigData → RegionRoute parsing logic.
//!
//! Used by both the Controller's PluginMetadataHandler and Center's CenterMetaDataStore
//! to parse raw EdgionConfigData resources into typed route entries.
//!
//! NOTE(migration): ClusterRegionRoute, ServiceRegionRoute, and MetaDataEntry variants
//! were deleted upstream (edgion-resources migration from PluginMetaData to EdgionConfigData).
//! The Cluster and Service variants of ParsedRegionRoute have been removed for the same
//! reason. Only the Other catch-all remains; restore from git history when a
//! RegionRouteOverride re-implementation is planned.

use edgion_resources::resources::edgion_config_data::EdgionConfigData;

/// The result of parsing an EdgionConfigData resource for RegionRoute purposes.
///
/// NOTE(migration): Cluster and Service variants removed because ClusterRegionRouteEntry,
/// ServiceRegionRouteEntry, and the corresponding MetaDataEntry variants were deleted
/// upstream. Restore from git history when RegionRoute is re-implemented on EdgionConfigData.
#[allow(dead_code)]
pub enum ParsedRegionRoute {
    /// Not a RegionRoute type — caller should ignore or handle separately.
    Other,
}

/// Parse an EdgionConfigData into its RegionRoute type.
///
/// NOTE(migration): ClusterRegionRouteConfig and ServiceRegionRouteConfig were deleted
/// upstream (PluginMetaData → EdgionConfigData migration). This function always returns
/// Other until RegionRoute support is re-implemented on EdgionConfigData.
/// Restore the Cluster/Service dispatch from git history when that work is scheduled.
pub fn parse_region_route(pm: &EdgionConfigData) -> ParsedRegionRoute {
    let _ = pm;
    ParsedRegionRoute::Other
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_resources::resources::edgion_config_data::{
        ConfigEntry, EdgionConfigDataSpec, KeyGroup, KeyListData, KeyMatchMode, MetaDataItem,
    };

    fn build_edgion_config_data(namespace: &str, name: &str, spec: EdgionConfigDataSpec) -> EdgionConfigData {
        let mut cd = EdgionConfigData::new(name, spec);
        cd.metadata.namespace = Some(namespace.to_string());
        cd
    }

    #[test]
    fn parse_other_type_returns_other() {
        let cd = build_edgion_config_data(
            "default",
            "my-keys",
            EdgionConfigDataSpec {
                enable: true,
                active: None,
                data: ConfigEntry::KeyList(KeyListData {
                    match_mode: KeyMatchMode::Exact,
                    items: vec![KeyGroup {
                        name: "default".to_string(),
                        description: None,
                        items: vec![MetaDataItem {
                            key: "test-key".to_string(),
                            code: None,
                            priority: None,
                            id: None,
                            behavior: None,
                        }],
                    }],
                }),
            },
        );

        assert!(matches!(parse_region_route(&cd), ParsedRegionRoute::Other));
    }
}
