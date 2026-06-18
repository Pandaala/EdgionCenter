//! CenterMetaDataStore — aggregates EdgionConfigData across all controllers.
//!
//! Implements [`CenterConfHandler<EdgionConfigData>`] for the EdgionConfigData watch cache.
//! The impl is a minimal no-op because GIR/region feeding via fed_sync has been replaced
//! by the background poller (`poll` module).  The trait impl must remain so the generic
//! EdgionConfigData watch cache (wired in `cli/mod.rs`) still compiles.
//!
//! NOTE(migration): ClusterRegionRoute and ServiceRegionRoute aggregation was removed
//! because ClusterRegionRouteEntry, ServiceRegionRouteEntry, MetaDataEntry, and
//! GlobalConnectionIpRestrictionData were deleted upstream. The cluster_routes and
//! service_routes maps are gone; restore from git history when RegionRoute is
//! re-implemented on EdgionConfigData.
//!
//! The global_ip_restrictions map (legacy fed-sync GIR feed) has been removed; GIR
//! is now populated by the background poller via `replace_gir`/`gir_effective`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::watch_cache::CenterConfHandler;
use edgion_resources::resources::edgion_config_data::EdgionConfigData;

/// One controller's effective region route (deserialized from the controller's
/// /api/v1/region-routes/effective response; field names match that DTO).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveRegionRouteView {
    pub namespace: String,
    pub plugin_name: String,
    #[serde(default)]
    pub alias: Option<String>,
    pub my_region: String,
    pub regions: serde_json::Value,
    #[serde(default)]
    pub override_ref: Option<String>,
    #[serde(default)]
    pub override_applied: bool,
}

/// Aggregated region route across controllers (one row per (ns, plugin, alias)).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CenterRegionRouteView {
    pub namespace: String,
    pub plugin_name: String,
    pub alias: Option<String>,
    pub controllers: HashMap<String, EffectiveRegionRouteView>,
}

/// One controller's effective GIR (Global IP Restriction) state (deserialized from
/// the controller's /api/v1/global-ip-restrictions/effective response; field names
/// match the controller's EffectiveGir DTO).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveGirView {
    pub namespace: String,
    pub plugin_name: String,
    pub enable: bool,
    pub active_profile: String,
    /// Passthrough blob — keeps the full ProfileRules tree without pulling the type here.
    pub profiles: serde_json::Value,
    #[serde(default)]
    pub active_profile_ref: Option<String>,
    #[serde(default)]
    pub selector_applied: bool,
}

/// Aggregated GIR view across controllers (one row per (ns, plugin_name)).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CenterGirView {
    pub namespace: String,
    pub plugin_name: String,
    /// Map of controller_id → that controller's effective GIR view.
    pub controllers: HashMap<String, EffectiveGirView>,
}

/// Aggregates EdgionConfigData across all controllers.
///
/// Internal structure:
/// - `region_routes`: route_key ("ns/plugin/alias") → { controller_id → EffectiveRegionRouteView }
/// - `gir_effective`: gir_key ("ns/pluginName") → { controller_id → EffectiveGirView }
///
/// Both maps are populated by the background poller (`poll` module); the
/// `CenterConfHandler<EdgionConfigData>` trait impl is a no-op because feeding
/// via fed_sync was replaced by polling the controller's `/effective` endpoints.
///
/// NOTE(migration): cluster_routes and service_routes maps were removed because
/// ClusterRegionRouteEntry and ServiceRegionRouteEntry were deleted upstream.
/// Restore from git history when RegionRoute is re-implemented on EdgionConfigData.
pub struct CenterMetaDataStore {
    // route_key ("ns/plugin/alias") → { controller_id → EffectiveRegionRouteView }
    // Populated by the background poller (poll module); not fed by the conf_sync path.
    region_routes: RwLock<HashMap<String, HashMap<String, EffectiveRegionRouteView>>>,
    // gir_key ("ns/pluginName") → { controller_id → EffectiveGirView }
    // Populated by the background poller; replaces the dead fed-sync GIR feed.
    gir_effective: RwLock<HashMap<String, HashMap<String, EffectiveGirView>>>,
}

impl CenterMetaDataStore {
    pub fn new() -> Self {
        Self {
            region_routes: RwLock::new(HashMap::new()),
            gir_effective: RwLock::new(HashMap::new()),
        }
    }

    /// Replace all region routes for one controller (full snapshot from a poll).
    /// Prunes all old entries for this controller across all route keys, then inserts
    /// the new snapshot; drops any outer key that becomes empty.
    pub fn replace_region_routes(&self, controller_id: &str, routes: Vec<EffectiveRegionRouteView>) {
        let mut map = self.region_routes.write();
        // Prune this controller's old entries across all keys.
        for inner in map.values_mut() {
            inner.remove(controller_id);
        }
        // Insert new entries.
        for r in routes {
            let key = region_route_key(&r);
            map.entry(key).or_default().insert(controller_id.to_string(), r);
        }
        // Drop outer keys that became empty.
        map.retain(|_, inner| !inner.is_empty());
    }

    /// Return aggregated effective region routes across all controllers.
    /// Each element represents one unique (ns, plugin, alias) key, with a map
    /// of controller_id → that controller's effective view for the same key.
    pub fn list_region_routes(&self) -> Vec<CenterRegionRouteView> {
        let map = self.region_routes.read();
        map.values()
            .filter_map(|inner| {
                let any = inner.values().next()?;
                Some(CenterRegionRouteView {
                    namespace: any.namespace.clone(),
                    plugin_name: any.plugin_name.clone(),
                    alias: any.alias.clone(),
                    controllers: inner.clone(),
                })
            })
            .collect()
    }

    /// Replace all GIR effective views for one controller (full snapshot from a poll).
    /// Prunes all old entries for this controller across all gir keys, then inserts
    /// the new snapshot; drops any outer key that becomes empty.
    pub fn replace_gir(&self, controller_id: &str, girs: Vec<EffectiveGirView>) {
        let mut map = self.gir_effective.write();
        // Prune this controller's old entries across all keys.
        for inner in map.values_mut() {
            inner.remove(controller_id);
        }
        // Insert new entries.
        for g in girs {
            let key = gir_key(&g);
            map.entry(key).or_default().insert(controller_id.to_string(), g);
        }
        // Drop outer keys that became empty.
        map.retain(|_, inner| !inner.is_empty());
    }

    /// Return aggregated effective GIR views across all controllers.
    /// Each element represents one unique (ns, plugin_name) key, with a map
    /// of controller_id → that controller's effective GIR view for the same key.
    pub fn list_gir_effective(&self) -> Vec<CenterGirView> {
        let map = self.gir_effective.read();
        map.values()
            .filter_map(|inner| {
                let any = inner.values().next()?;
                Some(CenterGirView {
                    namespace: any.namespace.clone(),
                    plugin_name: any.plugin_name.clone(),
                    controllers: inner.clone(),
                })
            })
            .collect()
    }

    /// Remove all entries for a given controller from all maps.
    /// If an inner HashMap becomes empty after removal, the outer key is also removed.
    fn remove_all_for_controller(&self, controller_id: &str) {
        {
            let mut rr = self.region_routes.write();
            rr.retain(|_, inner| {
                inner.remove(controller_id);
                !inner.is_empty()
            });
        }
        {
            let mut ge = self.gir_effective.write();
            ge.retain(|_, inner| {
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
        // No-op: GIR and RegionRoute feeding via fed_sync (EdgionConfigData watch) has been
        // replaced by the background poller (`poll` module) which calls replace_region_routes
        // and replace_gir directly. The trait impl must remain so the generic EdgionConfigData
        // watch cache in cli/mod.rs compiles.
        let _ = (controller_id, data);
    }

    fn partial_update(
        &self,
        controller_id: &str,
        add: HashMap<String, Arc<EdgionConfigData>>,
        update: HashMap<String, Arc<EdgionConfigData>>,
        remove: HashSet<String>,
    ) {
        // No-op: see full_set comment.
        let _ = (controller_id, add, update, remove);
    }

    fn controller_offline(&self, _controller_id: &str) {
        // Keep data — offline controller's config remains queryable
    }

    fn controller_removed(&self, controller_id: &str) {
        self.remove_all_for_controller(controller_id);
    }
}

/// Build the canonical storage key for a region route: "namespace/plugin_name/alias".
/// When alias is None the trailing segment is an empty string ("ns/plugin/").
fn region_route_key(r: &EffectiveRegionRouteView) -> String {
    format!("{}/{}/{}", r.namespace, r.plugin_name, r.alias.as_deref().unwrap_or(""))
}

/// Build the canonical storage key for a GIR entry: "namespace/plugin_name".
fn gir_key(g: &EffectiveGirView) -> String {
    format!("{}/{}", g.namespace, g.plugin_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── GIR effective aggregation tests ──

    #[test]
    fn gir_replace_and_list_aggregates_by_plugin_key() {
        let store = CenterMetaDataStore::new();
        let g = EffectiveGirView {
            namespace: "default".into(),
            plugin_name: "gir1".into(),
            enable: true,
            active_profile: "strict".into(),
            profiles: serde_json::json!({}),
            active_profile_ref: None,
            selector_applied: false,
        };
        store.replace_gir("ctrl-a", vec![g.clone()]);
        store.replace_gir("ctrl-b", vec![g.clone()]);
        let list = store.list_gir_effective();
        assert_eq!(list.len(), 1, "should aggregate to one row per (ns, plugin_name)");
        assert_eq!(list[0].controllers.len(), 2, "both controllers should appear");
        assert!(list[0].controllers.contains_key("ctrl-a"));
        assert!(list[0].controllers.contains_key("ctrl-b"));
    }

    // ── RegionRoute aggregation tests ──

    #[test]
    fn region_route_replace_and_list_aggregates_by_route_key() {
        let store = CenterMetaDataStore::new();
        let r = EffectiveRegionRouteView {
            namespace: "default".into(),
            plugin_name: "ep1".into(),
            alias: Some("rr1".into()),
            my_region: "east".into(),
            regions: serde_json::json!([]),
            override_ref: None,
            override_applied: false,
        };
        store.replace_region_routes("ctrl-a", vec![r.clone()]);
        store.replace_region_routes("ctrl-b", vec![r.clone()]);
        let list = store.list_region_routes();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].controllers.len(), 2);
    }
}
