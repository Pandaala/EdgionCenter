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
use std::time::{Duration, Instant};

use crate::watch_cache::CenterConfHandler;
use crate::watch_cache::WatchedConfigData;

/// Sorted diagnostic counters keyed by effective-state resource identity.
pub type DiagnosticEntries = Vec<(String, usize)>;

/// Region-route and global-IP-restriction diagnostic summaries.
pub type MetadataStatusEntries = (DiagnosticEntries, DiagnosticEntries);

/// One controller's effective region route (deserialized from the controller's
/// /api/v1/region-routes/effective response; field names match that DTO).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveRegionRouteView {
    pub namespace: String,
    pub plugin_name: String,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub entry_index: usize,
    pub my_region: String,
    pub regions: serde_json::Value,
    #[serde(default = "empty_json_array")]
    pub key_get: serde_json::Value,
    #[serde(default)]
    pub hash_key_get: Option<serde_json::Value>,
    #[serde(default)]
    pub hash_calc: Option<serde_json::Value>,
    #[serde(default = "empty_json_array")]
    pub route_rules: serde_json::Value,
    #[serde(default)]
    pub route_by_key_conf_match: Option<serde_json::Value>,
    #[serde(default)]
    pub dye_headers: Option<serde_json::Value>,
    #[serde(default)]
    pub override_ref: Option<EffectiveConfigDataRef>,
    #[serde(default)]
    pub override_applied: bool,
    #[serde(default)]
    pub service_usages: Vec<RegionRouteServiceUsage>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveConfigDataRef {
    pub namespace: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub permitted: bool,
}

fn default_true() -> bool {
    true
}

impl<'de> serde::Deserialize<'de> for EffectiveConfigDataRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum WireRef {
            Structured {
                namespace: String,
                name: String,
                #[serde(default = "default_true")]
                permitted: bool,
            },
            Legacy(String),
        }
        match <WireRef as serde::Deserialize>::deserialize(deserializer)? {
            WireRef::Structured {
                namespace,
                name,
                permitted,
            } => Ok(Self {
                namespace,
                name,
                permitted,
            }),
            WireRef::Legacy(value) => {
                let (namespace, name) = value.split_once('/').unwrap_or(("", value.as_str()));
                Ok(Self {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                    permitted: true,
                })
            }
        }
    }
}

fn empty_json_array() -> serde_json::Value {
    serde_json::Value::Array(Vec::new())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegionRouteServiceUsage {
    pub route_kind: String,
    pub route_namespace: String,
    pub route_name: String,
    pub rule_index: usize,
    #[serde(default)]
    pub backend_services: Vec<RegionRouteBackendService>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegionRouteBackendService {
    pub namespace: String,
    pub name: String,
    #[serde(default)]
    pub port: Option<u16>,
}

/// Aggregated region route across controllers (one row per (ns, plugin, alias)).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CenterRegionRouteView {
    pub namespace: String,
    pub plugin_name: String,
    pub alias: Option<String>,
    pub entry_index: usize,
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
    pub active_profile_ref: Option<EffectiveConfigDataRef>,
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
    // route_key ("ns/plugin/entry-index") → { controller_id → EffectiveRegionRouteView }
    // Populated by the background poller (poll module); not fed by the conf_sync path.
    region_routes: RwLock<HashMap<String, HashMap<String, EffectiveRegionRouteView>>>,
    // gir_key ("ns/pluginName") → { controller_id → EffectiveGirView }
    // Populated by the background poller; replaces the dead fed-sync GIR feed.
    gir_effective: RwLock<HashMap<String, HashMap<String, EffectiveGirView>>>,
    coverage: RwLock<HashMap<String, ControllerCoverage>>,
    expected_revisions: RwLock<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default)]
struct ControllerCoverage {
    revision: Option<String>,
    region_routes_at: Option<Instant>,
    gir_at: Option<Instant>,
}

impl CenterMetaDataStore {
    pub fn new() -> Self {
        Self {
            region_routes: RwLock::new(HashMap::new()),
            gir_effective: RwLock::new(HashMap::new()),
            coverage: RwLock::new(HashMap::new()),
            expected_revisions: RwLock::new(HashMap::new()),
        }
    }

    /// Replace all region routes for one controller (full snapshot from a poll).
    /// Prunes all old entries for this controller across all route keys, then inserts
    /// the new snapshot; drops any outer key that becomes empty.
    pub fn replace_region_routes(
        &self,
        controller_id: &str,
        routes: Vec<EffectiveRegionRouteView>,
    ) {
        let mut map = self.region_routes.write();
        // Prune this controller's old entries across all keys.
        for inner in map.values_mut() {
            inner.remove(controller_id);
        }
        // Insert new entries.
        for r in routes {
            let key = region_route_key(&r);
            map.entry(key)
                .or_default()
                .insert(controller_id.to_string(), r);
        }
        // Drop outer keys that became empty.
        map.retain(|_, inner| !inner.is_empty());
        self.coverage
            .write()
            .entry(controller_id.to_string())
            .or_default()
            .region_routes_at = Some(Instant::now());
    }

    pub fn replace_region_routes_fenced(
        &self,
        controller_id: &str,
        revision: &str,
        routes: Vec<EffectiveRegionRouteView>,
    ) -> bool {
        let expected = self.expected_revisions.read();
        if expected.get(controller_id).map(String::as_str) != Some(revision) {
            return false;
        }
        let mut map = self.region_routes.write();
        for inner in map.values_mut() {
            inner.remove(controller_id);
        }
        for route in routes {
            map.entry(region_route_key(&route))
                .or_default()
                .insert(controller_id.to_string(), route);
        }
        map.retain(|_, inner| !inner.is_empty());
        let mut coverage = self.coverage.write();
        let entry = coverage.entry(controller_id.to_string()).or_default();
        entry.revision = Some(revision.to_string());
        entry.region_routes_at = Some(Instant::now());
        true
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
                    entry_index: any.entry_index,
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
            map.entry(key)
                .or_default()
                .insert(controller_id.to_string(), g);
        }
        // Drop outer keys that became empty.
        map.retain(|_, inner| !inner.is_empty());
        self.coverage
            .write()
            .entry(controller_id.to_string())
            .or_default()
            .gir_at = Some(Instant::now());
    }

    pub fn replace_gir_fenced(
        &self,
        controller_id: &str,
        revision: &str,
        girs: Vec<EffectiveGirView>,
    ) -> bool {
        let expected = self.expected_revisions.read();
        if expected.get(controller_id).map(String::as_str) != Some(revision) {
            return false;
        }
        let mut map = self.gir_effective.write();
        for inner in map.values_mut() {
            inner.remove(controller_id);
        }
        for gir in girs {
            map.entry(gir_key(&gir))
                .or_default()
                .insert(controller_id.to_string(), gir);
        }
        map.retain(|_, inner| !inner.is_empty());
        let mut coverage = self.coverage.write();
        let entry = coverage.entry(controller_id.to_string()).or_default();
        entry.revision = Some(revision.to_string());
        entry.gir_at = Some(Instant::now());
        true
    }

    /// Publish the exact session/fence revision expected for the next sweep.
    /// Changed identities immediately lose their old data and coverage, so a
    /// late response from a displaced session cannot make the replica ready.
    pub fn prepare_revisions(&self, revisions: &HashMap<String, String>) {
        let changed: HashSet<String> = {
            let mut expected = self.expected_revisions.write();
            let changed = revisions
                .iter()
                .filter_map(|(id, revision)| {
                    (expected.get(id) != Some(revision)).then_some(id.clone())
                })
                .chain(
                    expected
                        .keys()
                        .filter(|id| !revisions.contains_key(*id))
                        .cloned(),
                )
                .collect();
            *expected = revisions.clone();
            changed
        };
        if changed.is_empty() {
            return;
        }
        self.region_routes.write().retain(|_, controllers| {
            controllers.retain(|id, _| !changed.contains(id));
            !controllers.is_empty()
        });
        self.gir_effective.write().retain(|_, controllers| {
            controllers.retain(|id, _| !changed.contains(id));
            !controllers.is_empty()
        });
        self.coverage.write().retain(|id, _| !changed.contains(id));
    }

    /// Return whether a sweep would preserve every currently published
    /// controller revision. The Kubernetes composition uses this to lower
    /// readiness before `prepare_revisions` clears changed snapshots.
    pub fn revisions_match(&self, revisions: &HashMap<String, String>) -> bool {
        *self.expected_revisions.read() == *revisions
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

    /// Lightweight diagnostic summary used by the Admin API. Values are
    /// derived from the live read model rather than legacy placeholder maps.
    pub fn status_entries(&self) -> MetadataStatusEntries {
        let mut routes: Vec<_> = self
            .region_routes
            .read()
            .iter()
            .map(|(key, controllers)| (key.clone(), controllers.len()))
            .collect();
        let mut restrictions: Vec<_> = self
            .gir_effective
            .read()
            .iter()
            .map(|(key, controllers)| (key.clone(), controllers.len()))
            .collect();
        routes.sort_by(|left, right| left.0.cmp(&right.0));
        restrictions.sort_by(|left, right| left.0.cmp(&right.0));
        (routes, restrictions)
    }

    /// Retain snapshots only for Controllers still present in the durable
    /// directory. This lets a fresh active-active replica rebuild its local
    /// read model while also removing projections hidden by an eviction fence.
    pub fn retain_controllers(&self, controller_ids: &HashSet<String>) {
        {
            let mut routes = self.region_routes.write();
            routes.retain(|_, controllers| {
                controllers.retain(|id, _| controller_ids.contains(id));
                !controllers.is_empty()
            });
        }
        {
            let mut restrictions = self.gir_effective.write();
            restrictions.retain(|_, controllers| {
                controllers.retain(|id, _| controller_ids.contains(id));
                !controllers.is_empty()
            });
        }
        self.coverage
            .write()
            .retain(|id, _| controller_ids.contains(id));
        self.expected_revisions
            .write()
            .retain(|id, _| controller_ids.contains(id));
    }

    pub fn has_fresh_coverage(&self, controller_ids: &HashSet<String>, max_age: Duration) -> bool {
        let coverage = self.coverage.read();
        controller_ids.iter().all(|id| {
            coverage.get(id).is_some_and(|entry| {
                entry.revision.as_ref() == self.expected_revisions.read().get(id)
                    && entry
                        .region_routes_at
                        .is_some_and(|at| at.elapsed() <= max_age)
                    && entry.gir_at.is_some_and(|at| at.elapsed() <= max_age)
            })
        })
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
        self.coverage.write().remove(controller_id);
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

impl CenterConfHandler<WatchedConfigData> for CenterMetaDataStore {
    fn full_set(&self, controller_id: &str, data: &HashMap<String, Arc<WatchedConfigData>>) {
        // No-op: GIR and RegionRoute feeding via fed_sync (EdgionConfigData watch) has been
        // replaced by the background poller (`poll` module) which calls replace_region_routes
        // and replace_gir directly. The trait impl must remain so the generic EdgionConfigData
        // watch cache in cli/mod.rs compiles.
        let _ = (controller_id, data);
    }

    fn partial_update(
        &self,
        controller_id: &str,
        add: HashMap<String, Arc<WatchedConfigData>>,
        update: HashMap<String, Arc<WatchedConfigData>>,
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

/// Build the canonical storage key for a region route: "namespace/plugin_name/entry_index".
/// The index is stable within the ordered requestPlugins list and prevents duplicate or
/// absent aliases from silently overwriting another RegionRoute entry.
fn region_route_key(r: &EffectiveRegionRouteView) -> String {
    format!("{}/{}/{}", r.namespace, r.plugin_name, r.entry_index)
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
        assert_eq!(
            list.len(),
            1,
            "should aggregate to one row per (ns, plugin_name)"
        );
        assert_eq!(
            list[0].controllers.len(),
            2,
            "both controllers should appear"
        );
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
            entry_index: 0,
            my_region: "east".into(),
            regions: serde_json::json!([]),
            key_get: serde_json::json!([]),
            hash_key_get: None,
            hash_calc: None,
            route_rules: serde_json::json!([]),
            route_by_key_conf_match: None,
            dye_headers: None,
            override_ref: None,
            override_applied: false,
            service_usages: Vec::new(),
        };
        store.replace_region_routes("ctrl-a", vec![r.clone()]);
        store.replace_region_routes("ctrl-b", vec![r.clone()]);
        let list = store.list_region_routes();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].controllers.len(), 2);
    }

    #[test]
    fn region_route_entry_index_prevents_duplicate_alias_overwrite() {
        let store = CenterMetaDataStore::new();
        let base = EffectiveRegionRouteView {
            namespace: "default".into(),
            plugin_name: "ep1".into(),
            alias: None,
            entry_index: 0,
            my_region: "east".into(),
            regions: serde_json::json!([]),
            key_get: serde_json::json!([]),
            hash_key_get: None,
            hash_calc: None,
            route_rules: serde_json::json!([]),
            route_by_key_conf_match: None,
            dye_headers: None,
            override_ref: None,
            override_applied: false,
            service_usages: Vec::new(),
        };
        let mut second = base.clone();
        second.entry_index = 1;
        store.replace_region_routes("ctrl-a", vec![base, second]);
        assert_eq!(store.list_region_routes().len(), 2);
    }

    #[test]
    fn effective_config_data_ref_accepts_legacy_and_structured_wire_shapes() {
        let legacy: EffectiveConfigDataRef = serde_json::from_str("\"ops/overlay\"").unwrap();
        assert_eq!(legacy.namespace, "ops");
        assert_eq!(legacy.name, "overlay");
        let structured: EffectiveConfigDataRef =
            serde_json::from_str(r#"{"namespace":"ops","name":"overlay"}"#).unwrap();
        assert_eq!(structured, legacy);
    }

    #[test]
    fn readiness_coverage_requires_both_snapshots_for_every_online_controller() {
        let store = CenterMetaDataStore::new();
        let ids = HashSet::from(["c1".to_string(), "c2".to_string()]);
        store.replace_region_routes("c1", Vec::new());
        store.replace_gir("c1", Vec::new());
        assert!(!store.has_fresh_coverage(&ids, Duration::from_secs(30)));
        store.replace_region_routes("c2", Vec::new());
        assert!(!store.has_fresh_coverage(&ids, Duration::from_secs(30)));
        store.replace_gir("c2", Vec::new());
        assert!(store.has_fresh_coverage(&ids, Duration::from_secs(30)));
    }

    #[test]
    fn revision_change_is_visible_before_snapshot_rebuild() {
        let store = CenterMetaDataStore::new();
        let revision_a = HashMap::from([("c1".to_string(), "session-a".to_string())]);
        let revision_b = HashMap::from([("c1".to_string(), "session-b".to_string())]);
        assert!(!store.revisions_match(&revision_a));
        store.prepare_revisions(&revision_a);
        assert!(store.revisions_match(&revision_a));
        assert!(!store.revisions_match(&revision_b));
    }
}
