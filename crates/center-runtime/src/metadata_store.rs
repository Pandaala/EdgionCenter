//! CenterMetaDataStore — aggregates EdgionConfigData across all controllers.
//!
//! Implements [`CenterConfHandler<WatchedConfigData>`] for the federation
//! EdgionConfigData list/watch cache. RegionRouteOverride and
//! ServiceRegionRouteOverride are classified directly into namespace/name maps.
//!
//! The global_ip_restrictions map (legacy fed-sync GIR feed) has been removed; GIR
//! is now populated by the background poller via `replace_gir`/`gir_effective`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;
use std::time::{Duration, Instant};

use crate::watch_cache::CenterConfHandler;
use crate::watch_cache::WatchedConfigData;

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

/// One watched override resource aggregated across Controllers by namespace/name.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CenterRegionRouteOverrideView {
    pub namespace: String,
    pub name: String,
    pub controllers: HashMap<String, serde_json::Value>,
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
/// - `gir_effective`: gir_key ("ns/pluginName") → { controller_id → EffectiveGirView }
///
/// Effective GIR remains populated by its background poller. RegionRoute
/// override maps are populated only by federation list/watch.
pub struct CenterMetaDataStore {
    region_route_overrides: RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
    service_region_route_overrides: RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
    // gir_key ("ns/pluginName") → { controller_id → EffectiveGirView }
    // Populated by the background poller; replaces the dead fed-sync GIR feed.
    gir_effective: RwLock<HashMap<String, HashMap<String, EffectiveGirView>>>,
    coverage: RwLock<HashMap<String, ControllerCoverage>>,
    expected_revisions: RwLock<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default)]
struct ControllerCoverage {
    revision: Option<String>,
    gir_at: Option<Instant>,
}

impl CenterMetaDataStore {
    pub fn new() -> Self {
        Self {
            region_route_overrides: RwLock::new(HashMap::new()),
            service_region_route_overrides: RwLock::new(HashMap::new()),
            gir_effective: RwLock::new(HashMap::new()),
            coverage: RwLock::new(HashMap::new()),
            expected_revisions: RwLock::new(HashMap::new()),
        }
    }

    pub fn list_region_route_overrides(&self) -> Vec<CenterRegionRouteOverrideView> {
        list_override_map(&self.region_route_overrides)
    }

    pub fn list_service_region_route_overrides(&self) -> Vec<CenterRegionRouteOverrideView> {
        list_override_map(&self.service_region_route_overrides)
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

    /// Retain snapshots only for Controllers still present in the durable
    /// directory. This lets a fresh active-active replica rebuild its local
    /// read model while also removing projections hidden by an eviction fence.
    pub fn retain_controllers(&self, controller_ids: &HashSet<String>) {
        retain_override_controllers(&mut self.region_route_overrides.write(), controller_ids);
        retain_override_controllers(
            &mut self.service_region_route_overrides.write(),
            controller_ids,
        );
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
                    && entry.gir_at.is_some_and(|at| at.elapsed() <= max_age)
            })
        })
    }

    /// Remove all entries for a given controller from all maps.
    /// If an inner HashMap becomes empty after removal, the outer key is also removed.
    fn remove_all_for_controller(&self, controller_id: &str) {
        remove_controller_from_override_map(
            &mut self.region_route_overrides.write(),
            controller_id,
        );
        remove_controller_from_override_map(
            &mut self.service_region_route_overrides.write(),
            controller_id,
        );
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
        remove_controller_from_override_map(
            &mut self.region_route_overrides.write(),
            controller_id,
        );
        remove_controller_from_override_map(
            &mut self.service_region_route_overrides.write(),
            controller_id,
        );
        for resource in data.values() {
            self.upsert_watched_override(controller_id, resource.as_ref().clone());
        }
    }

    fn partial_update(
        &self,
        controller_id: &str,
        add: HashMap<String, Arc<WatchedConfigData>>,
        update: HashMap<String, Arc<WatchedConfigData>>,
        remove: HashMap<String, Arc<WatchedConfigData>>,
    ) {
        for key in remove.keys() {
            remove_override_key_for_controller(
                &mut self.region_route_overrides.write(),
                key,
                controller_id,
            );
            remove_override_key_for_controller(
                &mut self.service_region_route_overrides.write(),
                key,
                controller_id,
            );
        }
        for resource in add.into_values().chain(update.into_values()) {
            self.upsert_watched_override(controller_id, resource.as_ref().clone());
        }
    }

    fn controller_offline(&self, _controller_id: &str) {
        // Keep data — offline controller's config remains queryable
    }

    fn controller_removed(&self, controller_id: &str) {
        self.remove_all_for_controller(controller_id);
    }
}

impl CenterMetaDataStore {
    fn upsert_watched_override(&self, controller_id: &str, resource: serde_json::Value) {
        let Some(namespace) = resource
            .pointer("/metadata/namespace")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let Some(name) = resource
            .pointer("/metadata/name")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let key = format!("{namespace}/{name}");
        let data_type = resource
            .pointer("/spec/data/type")
            .and_then(serde_json::Value::as_str);

        remove_override_key_for_controller(
            &mut self.region_route_overrides.write(),
            &key,
            controller_id,
        );
        remove_override_key_for_controller(
            &mut self.service_region_route_overrides.write(),
            &key,
            controller_id,
        );
        let target = match data_type {
            Some("RegionRouteOverride") => &self.region_route_overrides,
            Some("ServiceRegionRouteOverride") => &self.service_region_route_overrides,
            _ => return,
        };
        target
            .write()
            .entry(key)
            .or_default()
            .insert(controller_id.to_string(), resource);
    }
}

fn list_override_map(
    source: &RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
) -> Vec<CenterRegionRouteOverrideView> {
    let map = source.read();
    let mut rows: Vec<_> = map
        .iter()
        .filter_map(|(key, controllers)| {
            let (namespace, name) = key.split_once('/')?;
            Some(CenterRegionRouteOverrideView {
                namespace: namespace.to_string(),
                name: name.to_string(),
                controllers: controllers.clone(),
            })
        })
        .collect();
    rows.sort_by(|left, right| (&left.namespace, &left.name).cmp(&(&right.namespace, &right.name)));
    rows
}

fn remove_override_key_for_controller(
    map: &mut HashMap<String, HashMap<String, serde_json::Value>>,
    key: &str,
    controller_id: &str,
) {
    if let Some(controllers) = map.get_mut(key) {
        controllers.remove(controller_id);
        if controllers.is_empty() {
            map.remove(key);
        }
    }
}

fn remove_controller_from_override_map(
    map: &mut HashMap<String, HashMap<String, serde_json::Value>>,
    controller_id: &str,
) {
    map.retain(|_, controllers| {
        controllers.remove(controller_id);
        !controllers.is_empty()
    });
}

fn retain_override_controllers(
    map: &mut HashMap<String, HashMap<String, serde_json::Value>>,
    controller_ids: &HashSet<String>,
) {
    map.retain(|_, controllers| {
        controllers.retain(|id, _| controller_ids.contains(id));
        !controllers.is_empty()
    });
}

/// Build the canonical storage key for a GIR entry: "namespace/plugin_name".
fn gir_key(g: &EffectiveGirView) -> String {
    format!("{}/{}", g.namespace, g.plugin_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_store_handles_remove_values() {
        let store = CenterMetaDataStore::new();
        let region = serde_json::json!({
            "metadata": { "namespace": "shop", "name": "shared" },
            "spec": {
                "data": {
                    "type": "RegionRouteOverride",
                    "config": { "regions": [{ "name": "east" }] }
                }
            }
        });
        store.full_set(
            "ctrl-a",
            &HashMap::from([("shop/shared".to_string(), Arc::new(region.clone()))]),
        );
        assert_eq!(store.list_region_route_overrides().len(), 1);

        // partial_update's `remove` now carries the removed *values*, not
        // just keys; the handler only needs the keys but must still accept
        // and correctly apply the new signature.
        store.partial_update(
            "ctrl-a",
            HashMap::new(),
            HashMap::new(),
            HashMap::from([("shop/shared".to_string(), Arc::new(region))]),
        );

        assert!(
            store.list_region_route_overrides().is_empty(),
            "removed override row must be gone after partial_update with value-carrying remove"
        );
    }

    #[test]
    fn watched_overrides_are_split_and_aggregated_by_namespace_name() {
        let store = CenterMetaDataStore::new();
        let region = serde_json::json!({
            "metadata": { "namespace": "shop", "name": "shared" },
            "spec": {
                "data": {
                    "type": "RegionRouteOverride",
                    "config": { "regions": [{ "name": "east" }] }
                }
            }
        });
        let service = serde_json::json!({
            "metadata": { "namespace": "shop", "name": "checkout" },
            "spec": {
                "data": {
                    "type": "ServiceRegionRouteOverride",
                    "config": { "regions": [{ "name": "east", "failoverTo": "west" }] }
                }
            }
        });
        store.full_set(
            "ctrl-a",
            &HashMap::from([
                ("shop/shared".to_string(), Arc::new(region.clone())),
                ("shop/checkout".to_string(), Arc::new(service.clone())),
            ]),
        );
        store.full_set(
            "ctrl-b",
            &HashMap::from([("shop/shared".to_string(), Arc::new(region))]),
        );

        let region_rows = store.list_region_route_overrides();
        assert_eq!(region_rows.len(), 1);
        assert_eq!(region_rows[0].namespace, "shop");
        assert_eq!(region_rows[0].name, "shared");
        assert_eq!(region_rows[0].controllers.len(), 2);

        let service_rows = store.list_service_region_route_overrides();
        assert_eq!(service_rows.len(), 1);
        assert_eq!(service_rows[0].name, "checkout");
        assert_eq!(service_rows[0].controllers.len(), 1);
    }

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
    fn readiness_coverage_requires_gir_snapshot_for_every_online_controller() {
        let store = CenterMetaDataStore::new();
        let ids = HashSet::from(["c1".to_string(), "c2".to_string()]);
        store.replace_gir("c1", Vec::new());
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
