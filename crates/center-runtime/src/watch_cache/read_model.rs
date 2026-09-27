//! Production read accessors over the ConfigData watch cache — the surface
//! CCI-03 (the admin/global read API) consumes.
//!
//! Every row returned here is redacted: `/spec/data/config` is stripped
//! before the `Value` is cloned out of the cache unless `config_type` is one
//! of a small fixed allowlist of known-non-secret types
//! (`CONFIG_PASSTHROUGH_TYPES`). This is a fail-closed allowlist, not a
//! `Misc`-only denylist — a mistyped, unrecognized, missing, or malformed
//! type discriminator (e.g. `"misc"`, `"Msic"`, `"Unknown"`) redacts too, so
//! a secret can never reach a global API response through this path merely
//! because it did not spell its own type as `"Misc"`. The only way to read
//! an unredacted document is `CenterWatchCache::raw_entry`, which is
//! reserved for the write path (CAS) and convergence checks — never for
//! serializing into an API response.

use std::sync::Arc;

use super::cache::CenterWatchCache;
use super::registry::CenterWatchCacheRegistry;
use super::{ConfigTyped, WatchedConfigData};

/// ConfigData types whose `/spec/data/config` body is known to carry only
/// match-targets/routing configuration (never credentials or other
/// secrets), and therefore passes through the read model unredacted.
/// Exact, case-sensitive canonical strings. Everything else — including
/// `"Misc"`, case variants, typos, and missing/unknown types — is redacted
/// by default (see module docs: fail-closed allowlist, not a `Misc`-only
/// denylist).
const CONFIG_PASSTHROUGH_TYPES: &[&str] = &[
    "IpList",
    "KeyList",
    "Selector",
    "RegionRouteOverride",
    "ProxyProtocolTrust",
    "WafPolicy",
];

/// One ConfigData row in the global read model. `doc` is the stored document
/// with `/spec/data/config` removed unless `config_type` is in
/// `CONFIG_PASSTHROUGH_TYPES` (redacted copy otherwise) — the raw body never
/// leaves the cache through this surface.
#[derive(Debug, Clone)]
pub struct ConfigDataEntry {
    pub controller_id: String,
    /// "" for cluster-scoped keys (bare name, no `/`).
    pub namespace: String,
    pub name: String,
    /// The raw `/spec/data/type` wire string, verbatim — NOT validated
    /// against the known ConfigData type strings — or "Unknown" when
    /// absent/not a string. Whether `doc` is redacted is governed solely by
    /// `CONFIG_PASSTHROUGH_TYPES`, not by any assumption that this field
    /// holds a canonical value.
    pub config_type: String,
    pub doc: serde_json::Value,
}

/// Splits a cache key into `(namespace, name)`. Keys are either
/// `"namespace/name"` (namespace-scoped) or a bare `"name"` (cluster-scoped,
/// namespace is "").
fn split_key(key: &str) -> (String, String) {
    match key.split_once('/') {
        Some((namespace, name)) => (namespace.to_string(), name.to_string()),
        None => (String::new(), key.to_string()),
    }
}

/// Redacts a stored document for the read model: `/spec/data/config` is
/// removed unless `config_type` is an exact match in
/// `CONFIG_PASSTHROUGH_TYPES`. Fail-closed — an unrecognized, mistyped,
/// case-mismatched, or missing type redacts, it does not pass through.
fn redact(value: &WatchedConfigData, config_type: &str) -> serde_json::Value {
    let mut doc = value.clone();
    if !CONFIG_PASSTHROUGH_TYPES.contains(&config_type) {
        if let Some(data) = doc
            .pointer_mut("/spec/data")
            .and_then(|v| v.as_object_mut())
        {
            data.remove("config");
        }
    }
    doc
}

/// Builds a redacted `ConfigDataEntry` from a raw cache key/value pair.
fn build_entry(controller_id: &str, key: &str, value: &WatchedConfigData) -> ConfigDataEntry {
    let (namespace, name) = split_key(key);
    let config_type = value.config_type().unwrap_or("Unknown").to_string();
    let doc = redact(value, &config_type);
    ConfigDataEntry {
        controller_id: controller_id.to_string(),
        namespace,
        name,
        config_type,
        doc,
    }
}

impl CenterWatchCache<WatchedConfigData> {
    /// All entries in this controller's cache, optionally filtered to an
    /// exact `config_type`, in deterministic `(namespace, name)` order.
    /// Documents whose type is not in `CONFIG_PASSTHROUGH_TYPES` are
    /// redacted (see module docs).
    ///
    /// `controller_id_hint` is stamped onto every returned row's
    /// `controller_id` field: the cache does not expose its own controller
    /// id, so callers (which already know which controller's cache they
    /// asked for) supply it here.
    pub fn list_entries(
        &self,
        controller_id_hint: &str,
        type_filter: Option<&str>,
    ) -> Vec<ConfigDataEntry> {
        let mut entries: Vec<ConfigDataEntry> = self
            .entries_snapshot()
            .into_iter()
            .map(|(key, value)| build_entry(controller_id_hint, &key, &value))
            .filter(|entry| type_filter.is_none_or(|wanted| entry.config_type == wanted))
            .collect();
        entries.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
        entries
    }

    /// Raw (unredacted) document for `key`, straight from the cache.
    ///
    /// This is for the write path (CCI-08 compare-and-swap) and convergence
    /// checks only — the returned `Value` still contains the full
    /// `/spec/data/config` payload regardless of `config_type`. It must
    /// never be serialized into a global API response; use `list_entries`
    /// for that.
    pub fn raw_entry(&self, key: &str) -> Option<Arc<WatchedConfigData>> {
        self.raw_get(key)
    }
}

impl CenterWatchCacheRegistry<WatchedConfigData> {
    /// Aggregate `list_entries` across every controller's cache in this
    /// registry, in deterministic `(namespace, name, controller_id)` order.
    /// Documents whose type is not in `CONFIG_PASSTHROUGH_TYPES` are
    /// redacted (see module docs).
    pub fn list_all(&self, type_filter: Option<&str>) -> Vec<ConfigDataEntry> {
        let mut entries: Vec<ConfigDataEntry> = self
            .caches_snapshot()
            .into_iter()
            .flat_map(|(controller_id, cache)| cache.list_entries(&controller_id, type_filter))
            .collect();
        entries.sort_by(|a, b| {
            (&a.namespace, &a.name, &a.controller_id).cmp(&(
                &b.namespace,
                &b.name,
                &b.controller_id,
            ))
        });
        entries
    }

    /// Raw, unredacted document for `key` ("namespace/name", or bare name
    /// for cluster-scoped) in `controller_id`'s cache, without creating a
    /// cache for an unknown controller. For the CCI-08 write path and its
    /// convergence checks only — never serialize the result into a global
    /// API response.
    pub fn raw_document(&self, controller_id: &str, key: &str) -> Option<Arc<WatchedConfigData>> {
        self.get_if_present(controller_id)
            .and_then(|cache| cache.raw_entry(key))
    }

    /// The `server_id` this process last observed on `controller_id`'s watch
    /// stream, without creating a cache for an unknown controller.
    ///
    /// This is the completion signal for a Center-initiated reload. A
    /// Controller mints a fresh `server_id` every time it rebuilds its
    /// ConfigSyncServer, and the value stored here only ever advances through
    /// a successfully applied list/event batch — so observing a *change*
    /// means the Controller really did restart and Center re-listed against
    /// the new instance. A reload that was dispatched but wedged leaves this
    /// value untouched, which is exactly what lets the reload path report
    /// "not observed" instead of a bare success (see
    /// `center-app::api::reload_ops`).
    ///
    /// Returns `None` — never `Some("")` — for a controller this process has
    /// no cache for, or whose cache has not applied a batch yet, so a caller
    /// cannot mistake "no baseline to compare against" for an observed id.
    pub fn cached_server_id(&self, controller_id: &str) -> Option<String> {
        self.get_if_present(controller_id)
            .map(|cache| cache.get_server_id())
            .filter(|server_id| !server_id.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch_cache::cache::{ApplyResult, CenterWatchCache};
    use crate::watch_cache::registry::CenterWatchCacheRegistry;
    use crate::watch_cache::traits::CenterConfHandler;
    use std::collections::HashMap;
    use tokio::sync::broadcast;

    /// No-op handler: read_model tests only exercise read accessors, not
    /// handler dispatch.
    struct NoopHandler;

    impl CenterConfHandler<serde_json::Value> for NoopHandler {
        fn full_set(&self, _controller_id: &str, _data: &HashMap<String, Arc<serde_json::Value>>) {}

        fn partial_update(
            &self,
            _controller_id: &str,
            _add: HashMap<String, Arc<serde_json::Value>>,
            _update: HashMap<String, Arc<serde_json::Value>>,
            _remove: HashMap<String, Arc<serde_json::Value>>,
        ) {
        }

        fn controller_offline(&self, _controller_id: &str) {}

        fn controller_removed(&self, _controller_id: &str) {}
    }

    fn new_cache(controller_id: &str) -> CenterWatchCache<serde_json::Value> {
        let (tx, _rx) = broadcast::channel(16);
        CenterWatchCache::new(controller_id.to_string(), Arc::new(NoopHandler), tx)
    }

    fn doc(config_type: &str) -> serde_json::Value {
        serde_json::json!({
            "spec": {"data": {"type": config_type, "config": {}}}
        })
    }

    #[test]
    fn current_variants_preserve_safe_config_and_redact_sensitive_payloads() {
        let cache = new_cache("ctrl-1");
        let proxy = serde_json::json!({"mode": "trustedSources", "trustedCidrs": ["192.0.2.0/24"]});
        let policy = serde_json::json!({"defaultProfile": "main", "profiles": {"main": {"bundleRefs": [{"name": "rules"}]}}});
        let documents = [
            ("ProxyProtocolTrust", proxy),
            ("WafPolicy", policy),
            (
                "WafRuleBundle",
                serde_json::json!({"rules": [{"content": "private-rule-content"}]}),
            ),
            (
                "RequestAccessUrlAllowList",
                serde_json::json!({"items": [{"conditions": {"allOf": [{"resolvedValues": ["private-match-value"]}]}}]}),
            ),
        ];
        let entries = documents
            .iter()
            .map(|(kind, config)| {
                (
                    format!("ns/{kind}"),
                    serde_json::json!({"spec": {"data": {"type": kind, "config": config}}}),
                )
            })
            .collect();
        assert_eq!(
            cache.replace_all(entries, 1, "server-1".into()),
            ApplyResult::Applied
        );
        for (kind, config) in documents {
            let rows = cache.list_entries("ctrl-1", Some(kind));
            assert_eq!(rows.len(), 1);
            if matches!(kind, "ProxyProtocolTrust" | "WafPolicy") {
                assert_eq!(rows[0].doc.pointer("/spec/data/config"), Some(&config));
            } else {
                assert!(rows[0].doc.pointer("/spec/data/config").is_none());
            }
            assert_eq!(
                cache
                    .raw_entry(&format!("ns/{kind}"))
                    .unwrap()
                    .pointer("/spec/data/config"),
                Some(&config)
            );
        }
    }

    #[test]
    fn list_entries_orders_and_filters_by_type() {
        let cache = new_cache("ctrl-1");
        let items = vec![
            ("b/second".to_string(), doc("KeyList")),
            ("a/first".to_string(), doc("Misc")),
            ("a/zzz".to_string(), doc("KeyList")),
            ("bare-name".to_string(), doc("RegionRouteOverride")),
        ];
        let result = cache.replace_all(items, 1, "server-1".to_string());
        assert_eq!(result, ApplyResult::Applied);

        // No filter: full set, sorted by (namespace, name). The cluster-scoped
        // "bare-name" key has namespace "" and therefore sorts first.
        let all = cache.list_entries("ctrl-1", None);
        let order: Vec<(&str, &str)> = all
            .iter()
            .map(|e| (e.namespace.as_str(), e.name.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![
                ("", "bare-name"),
                ("a", "first"),
                ("a", "zzz"),
                ("b", "second"),
            ]
        );
        assert!(all.iter().all(|e| e.controller_id == "ctrl-1"));

        // Exact type filter narrows to only that type.
        let key_lists = cache.list_entries("ctrl-1", Some("KeyList"));
        assert_eq!(key_lists.len(), 2);
        assert!(key_lists.iter().all(|e| e.config_type == "KeyList"));
        // "a/zzz" and "b/second" — sorted by (namespace, name), so "a" < "b"
        // puts "zzz" ahead of "second" despite the reverse alphabetical name.
        let key_list_names: Vec<&str> = key_lists.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(key_list_names, vec!["zzz", "second"]);
    }

    #[test]
    fn misc_config_is_redacted_and_unrecoverable() {
        const SECRET: &str = "swordfish-super-secret-token-9f3a";
        let cache = new_cache("ctrl-1");
        let misc_doc = serde_json::json!({
            "spec": {"data": {"type": "Misc", "config": {"token": SECRET}}}
        });
        let result = cache.replace_all(
            vec![("ns/misc-one".to_string(), misc_doc)],
            1,
            "server-1".to_string(),
        );
        assert_eq!(result, ApplyResult::Applied);

        let entries = cache.list_entries("ctrl-1", None);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.config_type, "Misc");
        assert!(
            entry.doc.pointer("/spec/data/config").is_none(),
            "Misc redaction must remove the /spec/data/config pointer"
        );

        // Serialize the whole entry (not just `doc`) and confirm the secret
        // is absent everywhere, not merely at the expected pointer.
        let serialized = serde_json::to_string(&serde_json::json!({
            "controller_id": entry.controller_id,
            "namespace": entry.namespace,
            "name": entry.name,
            "config_type": entry.config_type,
            "doc": entry.doc,
        }))
        .expect("entry must serialize to JSON");
        assert!(
            !serialized.contains(SECRET),
            "secret must not appear anywhere in the serialized entry: {serialized}"
        );
    }

    /// Fail-closed allowlist regression: redaction must not be a `Misc ==`
    /// denylist. A case-mismatched type (`"misc"`), a missing type, and a
    /// bogus/unrecognized type must all redact `/spec/data/config` just like
    /// `"Misc"` does — and a genuine allowlisted type (`IpList`) must still
    /// pass its config through untouched.
    #[test]
    fn unrecognized_type_config_is_redacted() {
        const SECRET: &str = "swordfish-super-secret-token-9f3a";

        let lowercase_misc = serde_json::json!({
            "spec": {"data": {"type": "misc", "config": {"token": SECRET}}}
        });
        let missing_type = serde_json::json!({
            "spec": {"data": {"config": {"token": SECRET}}}
        });
        let bogus_type = serde_json::json!({
            "spec": {"data": {"type": "TotallyMadeUpType", "config": {"token": SECRET}}}
        });
        let ip_list_marker = "10.0.0.0/8-allow-rule";
        let allowlisted = serde_json::json!({
            "spec": {"data": {"type": "IpList", "config": {"cidrs": [ip_list_marker]}}}
        });

        let cache = new_cache("ctrl-1");
        let result = cache.replace_all(
            vec![
                ("ns/lowercase".to_string(), lowercase_misc),
                ("ns/missing".to_string(), missing_type),
                ("ns/bogus".to_string(), bogus_type),
                ("ns/allowlisted".to_string(), allowlisted),
            ],
            1,
            "server-1".to_string(),
        );
        assert_eq!(result, ApplyResult::Applied);

        let entries = cache.list_entries("ctrl-1", None);
        assert_eq!(entries.len(), 4);

        for entry in &entries {
            if entry.name == "allowlisted" {
                continue;
            }
            assert!(
                entry.doc.pointer("/spec/data/config").is_none(),
                "entry {:?} (config_type={}) must be redacted",
                entry.name,
                entry.config_type
            );
        }

        // Serialize every listed entry and confirm the secret is absent
        // everywhere, regardless of how malformed/unrecognized its type is.
        let serialized_all = serde_json::to_string(
            &entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "controller_id": e.controller_id,
                        "namespace": e.namespace,
                        "name": e.name,
                        "config_type": e.config_type,
                        "doc": e.doc,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .expect("entries must serialize to JSON");
        assert!(
            !serialized_all.contains(SECRET),
            "secret must not appear anywhere in any serialized listed entry: {serialized_all}"
        );

        // Positive case: an allowlisted type's config passes through intact.
        let allowlisted_entry = entries
            .iter()
            .find(|e| e.name == "allowlisted")
            .expect("allowlisted entry must be present");
        assert_eq!(allowlisted_entry.config_type, "IpList");
        assert_eq!(
            allowlisted_entry
                .doc
                .pointer("/spec/data/config/cidrs/0")
                .and_then(serde_json::Value::as_str),
            Some(ip_list_marker),
            "IpList config must pass through unredacted"
        );
    }

    #[test]
    fn raw_entry_returns_full_doc() {
        const SECRET: &str = "swordfish-super-secret-token-9f3a";
        let cache = new_cache("ctrl-1");
        let misc_doc = serde_json::json!({
            "spec": {"data": {"type": "Misc", "config": {"token": SECRET}}}
        });
        cache.replace_all(
            vec![("ns/misc-one".to_string(), misc_doc)],
            1,
            "server-1".to_string(),
        );

        let raw = cache
            .raw_entry("ns/misc-one")
            .expect("raw_entry must return the stored document");
        let serialized = serde_json::to_string(raw.as_ref()).expect("raw doc serializes");
        assert!(
            serialized.contains(SECRET),
            "raw_entry must expose the unredacted document"
        );

        assert!(cache.raw_entry("ns/missing").is_none());
    }

    #[test]
    fn raw_document_does_not_create_a_cache_for_an_unknown_controller() {
        let handler = Arc::new(NoopHandler);
        let registry = CenterWatchCacheRegistry::<serde_json::Value>::new(handler);

        let cache = registry.get_or_create("ctrl-1");
        cache.replace_all(
            vec![("ns/one".to_string(), doc("Misc"))],
            1,
            "server-1".to_string(),
        );

        assert!(registry
            .raw_document("unknown-controller", "ns/one")
            .is_none());

        // The trap `get_or_create` would fall into: looking up an unknown
        // controller must not have inserted a cache for it.
        assert_eq!(
            registry.statuses().len(),
            1,
            "raw_document must not create a cache for an unknown controller"
        );
    }

    #[test]
    fn raw_document_returns_the_unredacted_body() {
        const SECRET: &str = "swordfish-super-secret-token-9f3a";
        let handler = Arc::new(NoopHandler);
        let registry = CenterWatchCacheRegistry::<serde_json::Value>::new(handler);

        let cache = registry.get_or_create("ctrl-1");
        let misc_doc = serde_json::json!({
            "spec": {"data": {"type": "Misc", "config": {"token": SECRET}}}
        });
        cache.replace_all(
            vec![("ns/misc-one".to_string(), misc_doc)],
            1,
            "server-1".to_string(),
        );

        let raw = registry
            .raw_document("ctrl-1", "ns/misc-one")
            .expect("raw_document must return the stored document");
        let serialized = serde_json::to_string(raw.as_ref()).expect("raw doc serializes");
        assert!(
            serialized.contains(SECRET),
            "raw_document must expose the unredacted document"
        );
    }

    /// Both guarantees `cached_server_id`'s doc comment makes, because a caller
    /// uses it as a reload-completion baseline: an unknown controller must not
    /// gain a cache from being asked about, and "no batch applied yet" must be
    /// `None` rather than `Some("")` — a caller that mistook the empty string
    /// for an observed id would read the first real id as convergence.
    #[test]
    fn cached_server_id_reports_absence_without_creating_a_cache() {
        let handler = Arc::new(NoopHandler);
        let registry = CenterWatchCacheRegistry::<serde_json::Value>::new(handler);

        // A cache that exists but has never applied a batch: present, but with
        // no server_id to compare against.
        let cache = registry.get_or_create("ctrl-1");
        assert_eq!(
            registry.cached_server_id("ctrl-1"),
            None,
            "a cache with no applied batch has no baseline, and must not report an empty one"
        );

        assert_eq!(registry.cached_server_id("unknown-controller"), None);
        assert_eq!(
            registry.statuses().len(),
            1,
            "cached_server_id must not create a cache for an unknown controller"
        );

        cache.replace_all(Vec::new(), 1, "server-1".to_string());
        assert_eq!(
            registry.cached_server_id("ctrl-1").as_deref(),
            Some("server-1"),
            "an applied batch stamps the id even when it carries no entries"
        );

        // A reload mints a new id; the accessor must surface the change, which
        // is the whole signal the reload path waits on.
        cache.replace_all(Vec::new(), 2, "server-2".to_string());
        assert_eq!(
            registry.cached_server_id("ctrl-1").as_deref(),
            Some("server-2")
        );
    }

    #[test]
    fn list_all_carries_controller_identity() {
        let handler = Arc::new(NoopHandler);
        let registry = CenterWatchCacheRegistry::<serde_json::Value>::new(handler);

        let cache_a = registry.get_or_create("ctrl-a");
        cache_a.replace_all(
            vec![("ns/shared".to_string(), doc("KeyList"))],
            1,
            "server-1".to_string(),
        );

        let cache_b = registry.get_or_create("ctrl-b");
        cache_b.replace_all(
            vec![("ns/shared".to_string(), doc("KeyList"))],
            1,
            "server-1".to_string(),
        );

        let all = registry.list_all(None);
        assert_eq!(all.len(), 2);
        // Same (namespace, name) in both controllers: tie-broken by
        // controller_id, ascending.
        assert_eq!(all[0].controller_id, "ctrl-a");
        assert_eq!(all[1].controller_id, "ctrl-b");
        assert!(all
            .iter()
            .all(|e| e.namespace == "ns" && e.name == "shared"));

        // Type filter is applied per-controller before aggregation.
        let none = registry.list_all(Some("Misc"));
        assert!(none.is_empty());
    }
}
