//! CenterMetaDataStore — aggregates EdgionConfigData across all controllers.
//!
//! Implements [`CenterConfHandler<EdgionConfigData>`] for the GIR map.
//!
//! NOTE(migration): ClusterRegionRoute and ServiceRegionRoute aggregation was removed
//! because ClusterRegionRouteEntry, ServiceRegionRouteEntry, MetaDataEntry, and
//! GlobalConnectionIpRestrictionData were deleted upstream (PluginMetaData →
//! EdgionConfigData migration). The cluster_routes and service_routes maps are gone;
//! restore from git history when RegionRoute is re-implemented on EdgionConfigData.
//!
//! GIR aggregation feeding is also removed: GlobalConnectionIpRestrictionData was
//! deleted upstream; GIR config is now carried by EdgionStreamPlugins (not EdgionConfigData).
//! The global_ip_restrictions map and query APIs are kept so callers compile and the
//! write fan-out path (global_connection_ip_restriction_handlers) stays functional;
//! the map will simply never be populated by the fed-sync path until EdgionStreamPlugins
//! watch support is added. The GIR write endpoints remain wired (FIXME in handlers).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::watch_cache::CenterConfHandler;
// NOTE(migration): parse_region_route is kept for the handler trait impl but is now a no-op
// stub; ClusterRegionRoute and ServiceRegionRoute variants were deleted upstream.
use crate::common::metadata_conf_handler::{parse_region_route, ParsedRegionRoute};
// Renamed from PluginMetaData to EdgionConfigData (upstream migration).
use edgion_resources::resources::edgion_config_data::EdgionConfigData;
// ProfileRules re-pointed from plugin_metadata to edgion_stream_plugins (upstream migration).
use edgion_resources::resources::edgion_stream_plugins::ProfileRules;

/// Per-Controller view of a GlobalConnectionIpRestriction PM aggregated at Center.
/// (Naming disambiguation: distinct from `GlobalConnectionIpRestrictionEntry` in
/// edgion_stream_plugins — that one is the stream-plugin ref carrier.)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerPmEntry {
    pub pm_namespace: String,
    pub pm_name: String,
    pub enable: bool,
    pub active_profile: String,
    pub profiles: HashMap<String, ProfileRules>,
    pub description: Option<String>,
    /// sha256 over canonical JSON of the PM data; used for consistency/freshness display.
    pub content_hash: String,
    /// Unix ms of last modification on the Controller (Center-assigned on observation).
    pub last_modified: i64,
}

/// Aggregated view of one GlobalConnectionIpRestriction PM across all Controllers.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CenterGlobalIpRestrictionEntryView {
    pub namespace: String,
    pub name: String,
    /// Map of controller_id → ControllerPmEntry
    pub controllers: HashMap<String, Arc<ControllerPmEntry>>,
}

/// Aggregates EdgionConfigData across all controllers.
///
/// Internal structure:
/// - `global_ip_restrictions`: pm_key → { controller_id → ControllerPmEntry }
///
/// NOTE(migration): cluster_routes and service_routes maps were removed because
/// ClusterRegionRouteEntry and ServiceRegionRouteEntry were deleted upstream.
/// Restore from git history when RegionRoute is re-implemented on EdgionConfigData.
pub struct CenterMetaDataStore {
    // pm_key ("ns/name") → { controller_id → entry }
    // NOTE(migration): GIR feeding removed — GlobalConnectionIpRestrictionData was deleted
    // upstream; this map is kept so the GIR query/write APIs stay functional, but it
    // will not be populated by fed-sync until EdgionStreamPlugins watch is added.
    global_ip_restrictions: RwLock<HashMap<String, HashMap<String, Arc<ControllerPmEntry>>>>,
}

impl CenterMetaDataStore {
    pub fn new() -> Self {
        Self {
            global_ip_restrictions: RwLock::new(HashMap::new()),
        }
    }

    /// List all GlobalConnectionIpRestriction PMs aggregated across Controllers.
    /// Returns one view per `pm_key`, with the outer key already split into
    /// `namespace`/`name` so callers don't pay for an extra `pm_key` clone.
    pub fn list_global_ip_restrictions(&self) -> Vec<CenterGlobalIpRestrictionEntryView> {
        self.global_ip_restrictions
            .read()
            .iter()
            .map(|(pm_key, controllers)| {
                let (namespace, name) = split_pm_key(pm_key);
                CenterGlobalIpRestrictionEntryView {
                    namespace,
                    name,
                    controllers: controllers.clone(),
                }
            })
            .collect()
    }

    /// Get a single PM's entries across Controllers.
    pub fn get_global_ip_restriction(&self, pm_key: &str) -> Option<HashMap<String, Arc<ControllerPmEntry>>> {
        self.global_ip_restrictions.read().get(pm_key).cloned()
    }

    /// Remove all entries for a given controller from all maps.
    /// If an inner HashMap becomes empty after removal, the outer key is also removed.
    fn remove_all_for_controller(&self, controller_id: &str) {
        {
            let mut gir = self.global_ip_restrictions.write();
            gir.retain(|_, inner| {
                inner.remove(controller_id);
                !inner.is_empty()
            });
        }
    }
}

impl Default for CenterMetaDataStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CenterConfHandler<EdgionConfigData> for CenterMetaDataStore {
    fn full_set(&self, controller_id: &str, data: &HashMap<String, Arc<EdgionConfigData>>) {
        // NOTE(migration): ClusterRegionRoute and ServiceRegionRoute feeding removed —
        // ClusterRegionRouteConfig, ServiceRegionRouteConfig, and MetaDataEntry were
        // deleted upstream (PluginMetaData → EdgionConfigData). Restore from git history.
        //
        // NOTE(migration): GIR feeding removed — GlobalConnectionIpRestrictionData was
        // deleted upstream; GIR config is now carried by EdgionStreamPlugins. The map
        // is kept so reads stay functional; feeding will be re-wired when EdgionStreamPlugins
        // watch support is added.
        let new_gir: HashMap<String, ControllerPmEntry> = HashMap::new();

        // Suppress unused-variable warning; parse_region_route is a no-op stub post-migration.
        for pm in data.values() {
            let _ = parse_region_route(pm);
        }

        // Single lock acquisition for global_ip_restrictions (prune removed keys; insert new).
        {
            let mut gir = self.global_ip_restrictions.write();
            gir.retain(|_, controllers| {
                controllers.remove(controller_id);
                !controllers.is_empty()
            });
            for (pm_key, entry) in new_gir {
                gir.entry(pm_key).or_default().insert(controller_id.to_string(), Arc::new(entry));
            }
        }
    }

    fn partial_update(
        &self,
        controller_id: &str,
        add: HashMap<String, Arc<EdgionConfigData>>,
        update: HashMap<String, Arc<EdgionConfigData>>,
        remove: HashSet<String>,
    ) {
        // NOTE(migration): ClusterRegionRoute and ServiceRegionRoute feeding removed —
        // ClusterRegionRouteConfig, ServiceRegionRouteConfig, and MetaDataEntry were
        // deleted upstream (PluginMetaData → EdgionConfigData). Restore from git history.
        //
        // NOTE(migration): GIR feeding removed — GlobalConnectionIpRestrictionData was
        // deleted upstream. Restore when EdgionStreamPlugins watch support is added.
        let gir_upserts: HashMap<String, ControllerPmEntry> = HashMap::new();

        // Suppress unused-variable warning; parse_region_route is a no-op stub post-migration.
        for (_, pm) in add.iter().chain(update.iter()) {
            match parse_region_route(pm) {
                ParsedRegionRoute::Other => {}
            }
        }

        // Apply global_ip_restrictions changes in one lock (removals still prune).
        {
            let mut gir = self.global_ip_restrictions.write();
            for pm_key in &remove {
                if let Some(controllers) = gir.get_mut(pm_key.as_str()) {
                    controllers.remove(controller_id);
                    if controllers.is_empty() {
                        gir.remove(pm_key.as_str());
                    }
                }
            }
            for (pm_key, entry) in gir_upserts {
                gir.entry(pm_key).or_default().insert(controller_id.to_string(), Arc::new(entry));
            }
        }
    }

    fn controller_offline(&self, _controller_id: &str) {
        // Keep data — offline controller's config remains queryable
    }

    fn controller_removed(&self, controller_id: &str) {
        self.remove_all_for_controller(controller_id);
    }
}

/// Split a pm_key of the form "namespace/name" into (namespace, name).
/// If there is no '/', returns ("", pm_key).
fn split_pm_key(pm_key: &str) -> (String, String) {
    match pm_key.split_once('/') {
        Some((ns, name)) => (ns.to_string(), name.to_string()),
        None => (String::new(), pm_key.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── JSON guard tests: Arc<Entry> must serialize transparently (no extra nesting) ──

    /// Verify that Arc<ControllerPmEntry> serializes identically to ControllerPmEntry
    /// — no extra wrapper, all camelCase keys (pmNamespace, pmName, activeProfile,
    /// contentHash, lastModified) present at the right level.
    /// This test was preserved from Task 02 (Arc-wrap + serde(rc) guard).
    #[test]
    fn json_guard_gir_entry_view_arc_transparent() {
        let entry = ControllerPmEntry {
            pm_namespace: "prod".to_string(),
            pm_name: "gir-test".to_string(),
            enable: true,
            active_profile: "default".to_string(),
            profiles: HashMap::new(),
            description: None,
            content_hash: "deadbeef".to_string(),
            last_modified: 42,
        };
        let view = CenterGlobalIpRestrictionEntryView {
            namespace: "prod".to_string(),
            name: "gir-test".to_string(),
            controllers: {
                let mut m = HashMap::new();
                m.insert("ctrl-1".to_string(), Arc::new(entry));
                m
            },
        };

        let json = serde_json::to_string(&view).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(v["namespace"], "prod");
        assert_eq!(v["name"], "gir-test");
        assert!(v["controllers"].is_object());

        // Arc must not add extra nesting.
        let ctrl = &v["controllers"]["ctrl-1"];
        assert!(ctrl.is_object(), "Arc<ControllerPmEntry> must serialize transparently");
        assert_eq!(ctrl["pmNamespace"], "prod");
        assert_eq!(ctrl["pmName"], "gir-test");
        assert_eq!(ctrl["enable"], true);
        assert_eq!(ctrl["activeProfile"], "default");
        assert_eq!(ctrl["contentHash"], "deadbeef");
        assert_eq!(ctrl["lastModified"], 42);
    }
}
