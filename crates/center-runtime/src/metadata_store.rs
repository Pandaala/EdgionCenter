//! CenterMetaDataStore — aggregates EdgionConfigData overrides across all controllers.
//!
//! Implements [`CenterConfHandler<WatchedConfigData>`] for the federation
//! EdgionConfigData list/watch cache. `RegionRouteOverride` documents are
//! classified directly into a namespace/name map.
//!
//! The store holds nothing else: it is populated exclusively by the federation
//! list/watch stream, and every row is keyed by "namespace/name" with one entry
//! per contributing controller.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::watch_cache::CenterConfHandler;
use crate::watch_cache::WatchedConfigData;

/// One watched override resource aggregated across Controllers by namespace/name.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CenterRegionRouteOverrideView {
    pub namespace: String,
    pub name: String,
    pub controllers: HashMap<String, serde_json::Value>,
}

/// Aggregates the EdgionConfigData override read model across all controllers.
///
/// Internal structure — `region_route_overrides` is "namespace/name" →
/// { controller_id → resource }, holding EdgionConfigData of type
/// `RegionRouteOverride`, populated only by the federation EdgionConfigData
/// list/watch.
///
/// There is exactly ONE override dimension. RegionRoute config is attached
/// per HTTPRoute/GRPCRoute rule through an `EdgionPlugins` ExtensionRef
/// filter, and each `EdgionPlugins` object carries its own `overrideRef`.
/// "Which layer an override belongs to" is therefore a property of which
/// object references it, not of the document's own type — one referrer makes
/// it service-scoped, many make it fleet-wide. Do not reintroduce a second
/// type-keyed map for a "service dimension".
pub struct CenterMetaDataStore {
    region_route_overrides: RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
}

impl CenterMetaDataStore {
    pub fn new() -> Self {
        Self {
            region_route_overrides: RwLock::new(HashMap::new()),
        }
    }

    pub fn list_region_route_overrides(&self) -> Vec<CenterRegionRouteOverrideView> {
        list_override_map(&self.region_route_overrides)
    }

    /// Retain rows only for Controllers still present in the durable directory,
    /// dropping projections for Controllers that were removed or hidden behind
    /// an eviction fence.
    ///
    /// This only prunes — it never reconstructs. A replica's rows come solely
    /// from the federation watch streams it owns, so a Controller owned by
    /// another replica has no rows here and this call cannot create them. Both
    /// binaries invoke it from their periodic Controller directory sweep;
    /// without that sweep nothing would ever prune, because
    /// `controller_offline` deliberately keeps an offline Controller's config
    /// queryable and `controller_removed` has no production trigger.
    pub fn retain_controllers(&self, controller_ids: &HashSet<String>) {
        retain_override_controllers(&mut self.region_route_overrides.write(), controller_ids);
    }

    /// Remove all entries for a given controller from all maps.
    /// If an inner HashMap becomes empty after removal, the outer key is also removed.
    fn remove_all_for_controller(&self, controller_id: &str) {
        remove_controller_from_override_map(
            &mut self.region_route_overrides.write(),
            controller_id,
        );
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
        if data_type != Some("RegionRouteOverride") {
            return;
        }
        self.region_route_overrides
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

    /// The watch carries every `EdgionConfigData` type, but this store projects
    /// exactly one of them. A document of any other type must be dropped rather
    /// than land in the single map — the classification is a type allowlist of
    /// one, not a "not obviously something else" filter.
    #[test]
    fn only_region_route_overrides_are_projected_and_aggregated_by_namespace_name() {
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
        let other_type = serde_json::json!({
            "metadata": { "namespace": "shop", "name": "checkout" },
            "spec": {
                "data": {
                    "type": "KeyList",
                    "config": { "keys": ["tenant-a"] }
                }
            }
        });
        let untyped = serde_json::json!({
            "metadata": { "namespace": "shop", "name": "untyped" },
            "spec": { "data": { "config": { "regions": [{ "name": "east" }] } } }
        });
        store.full_set(
            "ctrl-a",
            &HashMap::from([
                ("shop/shared".to_string(), Arc::new(region.clone())),
                ("shop/checkout".to_string(), Arc::new(other_type)),
                ("shop/untyped".to_string(), Arc::new(untyped)),
            ]),
        );
        store.full_set(
            "ctrl-b",
            &HashMap::from([("shop/shared".to_string(), Arc::new(region))]),
        );

        let region_rows = store.list_region_route_overrides();
        assert_eq!(
            region_rows.len(),
            1,
            "only the RegionRouteOverride document may be projected"
        );
        assert_eq!(region_rows[0].namespace, "shop");
        assert_eq!(region_rows[0].name, "shared");
        assert_eq!(region_rows[0].controllers.len(), 2);
    }
}
