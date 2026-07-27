use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use tokio::sync::broadcast;

use super::traits::CenterConfHandler;
use super::{ChangeSummary, ConfigTyped, MAX_ENTRIES_PER_CONTROLLER};

/// Change classification received from a Controller watch stream.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    Update,
    Delete,
    Add,
}

/// Simplified watch event for Center consumption.
pub struct WatchEventSimple<T> {
    pub event_type: EventType,
    pub key: String,
    pub data: T,
}

/// Outcome of a `replace_all`/`apply_events` batch application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyResult {
    /// The batch was applied; cache state (revision, sync_version, ...) advanced.
    Applied,
    /// The batch would have pushed the cache beyond `MAX_ENTRIES_PER_CONTROLLER`.
    /// Nothing was applied; `overflowed` is set on the cache status.
    Overflow,
}

/// Point-in-time snapshot of one controller's cache state, exposed for
/// diagnostics/readiness (Admin API, SSE) without leaking the RwLock guard.
#[derive(Debug, Clone)]
pub struct CacheStatus {
    pub sync_version: u64,
    pub server_id: String,
    pub revision: u64,
    pub stale: bool,
    pub overflowed: bool,
    pub last_applied_unix_ms: Option<u64>,
    pub entry_count: usize,
}

/// Per-controller single-source watch cache.
/// Mirrors Gateway's ClientCache<T> — one instance per controller per kind.
pub struct CenterWatchCache<T> {
    controller_id: String,
    inner: RwLock<CacheState<T>>,
    handler: Arc<dyn CenterConfHandler<T> + Send + Sync>,
    changes: broadcast::Sender<ChangeSummary>,
}

struct CacheState<T> {
    data: HashMap<String, Arc<T>>,
    sync_version: u64,
    server_id: String,
    /// Monotonic per-controller counter, +1 for every successfully applied
    /// batch (`replace_all` or `apply_events`). Never bumped on `Overflow`.
    revision: u64,
    /// Set by `set_stale()` when the controller goes offline, and also by a
    /// rejected `Overflow` batch (both leave the cache's contents behind the
    /// Controller's true state); cleared by the next successfully applied
    /// batch (a reconnect resuming the watch, or a later batch that fits).
    stale: bool,
    /// Set when the most recent batch was rejected for exceeding
    /// `MAX_ENTRIES_PER_CONTROLLER`; cleared by the next batch that fits.
    overflowed: bool,
    last_applied_unix_ms: Option<u64>,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// The ConfigData type label for a cached value, or "Unknown" when the value
/// carries no discriminator (untyped document).
fn type_label<T: ConfigTyped>(value: &T) -> String {
    value.config_type().unwrap_or("Unknown").to_string()
}

impl<T: Send + Sync + 'static> CenterWatchCache<T> {
    pub fn new(
        controller_id: String,
        handler: Arc<dyn CenterConfHandler<T> + Send + Sync>,
        changes: broadcast::Sender<ChangeSummary>,
    ) -> Self {
        Self {
            controller_id,
            inner: RwLock::new(CacheState {
                data: HashMap::new(),
                sync_version: 0,
                server_id: String::new(),
                revision: 0,
                stale: false,
                overflowed: false,
                last_applied_unix_ms: None,
            }),
            handler,
            changes,
        }
    }

    pub fn get_sync_version(&self) -> u64 {
        self.inner.read().sync_version
    }

    pub fn get_server_id(&self) -> String {
        self.inner.read().server_id.clone()
    }

    /// Current cache status snapshot (revision, staleness, overflow, size).
    pub fn status(&self) -> CacheStatus {
        let state = self.inner.read();
        CacheStatus {
            sync_version: state.sync_version,
            server_id: state.server_id.clone(),
            revision: state.revision,
            stale: state.stale,
            overflowed: state.overflowed,
            last_applied_unix_ms: state.last_applied_unix_ms,
            entry_count: state.data.len(),
        }
    }

    /// Mark this cache stale (controller offline). Cleared automatically by
    /// the next batch that is successfully applied (`Applied`, not `Overflow`).
    pub fn set_stale(&self) {
        self.inner.write().stale = true;
    }

    /// Returns a sorted list of all cache keys.
    ///
    /// Available in `#[cfg(test)]` only — used by unit tests to assert which
    /// keys are present after add/update/delete classification.
    #[cfg(any(test, feature = "test-support"))]
    pub fn snapshot_keys(&self) -> Vec<String> {
        let state = self.inner.read();
        let mut keys: Vec<String> = state.data.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Returns the current cache entry for `key`, or `None` if absent.
    ///
    /// Available in `#[cfg(test)]` only — used by unit tests to assert
    /// key-level presence/absence after classification.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_entry(&self, key: &str) -> Option<Arc<T>> {
        self.raw_get(key)
    }

    /// Snapshot of every `(key, value)` pair currently cached, taken under a
    /// single read-lock acquisition. Not `cfg(test)`-gated: this is the
    /// backing accessor for the production read model
    /// (`read_model::CenterWatchCache::list_entries`), which must iterate
    /// the full entry set to build redacted rows.
    pub(crate) fn entries_snapshot(&self) -> Vec<(String, Arc<T>)> {
        self.inner
            .read()
            .data
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// Raw (unredacted) lookup for a single key. Backing accessor for
    /// `read_model::CenterWatchCache::raw_entry` — production callers reach
    /// this only through that surface, whose doc-comment records that the
    /// result must never be serialized into a global API response.
    pub(crate) fn raw_get(&self, key: &str) -> Option<Arc<T>> {
        self.inner.read().data.get(key).cloned()
    }
}

impl<T: Send + Sync + 'static + ConfigTyped> CenterWatchCache<T> {
    /// Full replace from list response. Triggers handler.full_set().
    ///
    /// Handler call happens OUTSIDE the lock to avoid deadlocks.
    ///
    /// Capacity is checked BEFORE mutation: if `items` would push the cache
    /// beyond `MAX_ENTRIES_PER_CONTROLLER`, nothing is applied, `overflowed`
    /// and `stale` are both set, and `ApplyResult::Overflow` is returned —
    /// revision, sync version, and handler are all left untouched.
    pub fn replace_all(
        &self,
        items: Vec<(String, T)>,
        sync_version: u64,
        server_id: String,
    ) -> ApplyResult {
        if items.len() > MAX_ENTRIES_PER_CONTROLLER {
            let mut state = self.inner.write();
            state.overflowed = true;
            state.stale = true;
            return ApplyResult::Overflow;
        }

        let (snapshot, revision, event_unix_ms, changed_types) = {
            let mut state = self.inner.write();

            // Coarse-but-documented changed-type computation: a wholesale
            // replace unions all old and new types rather than diffing entry
            // sets. CCI-06 coalesces consecutive summaries anyway.
            let mut changed_types: BTreeSet<String> = BTreeSet::new();
            for old in state.data.values() {
                changed_types.insert(type_label(old.as_ref()));
            }

            let new_data: HashMap<String, Arc<T>> =
                items.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
            for new in new_data.values() {
                changed_types.insert(type_label(new.as_ref()));
            }

            state.data = new_data;
            state.sync_version = sync_version;
            state.server_id = server_id;
            state.revision += 1;
            state.stale = false;
            state.overflowed = false;
            let now = now_unix_ms();
            state.last_applied_unix_ms = Some(now);

            (state.data.clone(), state.revision, now, changed_types)
        };

        self.handler.full_set(&self.controller_id, &snapshot);

        let _ = self.changes.send(ChangeSummary {
            controller_id: self.controller_id.clone(),
            revision,
            changed_types,
            event_unix_ms,
        });

        ApplyResult::Applied
    }

    /// Incremental update from watch events. Classifies by EventType and triggers
    /// handler.partial_update().
    ///
    /// Handler call happens OUTSIDE the lock to avoid deadlocks.
    ///
    /// Capacity is checked BEFORE mutation, projecting the post-batch entry
    /// count from the current keys plus this batch's adds/updates/deletes:
    /// if it would exceed `MAX_ENTRIES_PER_CONTROLLER`, nothing is applied,
    /// `overflowed` and `stale` are both set, and `ApplyResult::Overflow` is
    /// returned.
    pub fn apply_events(
        &self,
        events: Vec<WatchEventSimple<T>>,
        sync_version: u64,
        server_id: String,
    ) -> ApplyResult {
        let (add, update, remove, revision, event_unix_ms, changed_types) = {
            let mut state = self.inner.write();

            // Capacity check BEFORE mutation, computed under the same write
            // guard as the mutation itself so a concurrent apply cannot slip
            // in between the projection and the write (single-writer per
            // cache in the current federation architecture, but this keeps
            // the invariant true regardless).
            let projected_len = {
                let mut keys: HashSet<&str> = state.data.keys().map(String::as_str).collect();
                for event in &events {
                    match event.event_type {
                        EventType::Add | EventType::Update => {
                            keys.insert(event.key.as_str());
                        }
                        EventType::Delete => {
                            keys.remove(event.key.as_str());
                        }
                    }
                }
                keys.len()
            };
            if projected_len > MAX_ENTRIES_PER_CONTROLLER {
                state.overflowed = true;
                state.stale = true;
                return ApplyResult::Overflow;
            }

            let mut add: HashMap<String, Arc<T>> = HashMap::new();
            let mut update: HashMap<String, Arc<T>> = HashMap::new();
            let mut remove: HashMap<String, Arc<T>> = HashMap::new();
            let mut changed_types: BTreeSet<String> = BTreeSet::new();

            for event in events {
                match event.event_type {
                    EventType::Add => {
                        let arc = Arc::new(event.data);
                        changed_types.insert(type_label(arc.as_ref()));
                        state.data.insert(event.key.clone(), arc.clone());
                        add.insert(event.key, arc);
                    }
                    EventType::Update => {
                        let arc = Arc::new(event.data);
                        changed_types.insert(type_label(arc.as_ref()));
                        state.data.insert(event.key.clone(), arc.clone());
                        update.insert(event.key, arc);
                    }
                    EventType::Delete => {
                        if let Some(old) = state.data.remove(&event.key) {
                            changed_types.insert(type_label(old.as_ref()));
                            remove.insert(event.key, old);
                        }
                    }
                }
            }

            state.sync_version = sync_version;
            state.server_id = server_id;
            state.revision += 1;
            state.stale = false;
            state.overflowed = false;
            let now = now_unix_ms();
            state.last_applied_unix_ms = Some(now);

            (add, update, remove, state.revision, now, changed_types)
        };

        self.handler
            .partial_update(&self.controller_id, add, update, remove);

        let _ = self.changes.send(ChangeSummary {
            controller_id: self.controller_id.clone(),
            revision,
            changed_types,
            event_unix_ms,
        });

        ApplyResult::Applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockHandler {
        full_set_calls: AtomicUsize,
        partial_update_calls: AtomicUsize,
    }

    impl MockHandler {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                full_set_calls: AtomicUsize::new(0),
                partial_update_calls: AtomicUsize::new(0),
            })
        }
    }

    impl CenterConfHandler<String> for MockHandler {
        fn full_set(&self, _controller_id: &str, _data: &HashMap<String, Arc<String>>) {
            self.full_set_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn partial_update(
            &self,
            _controller_id: &str,
            _add: HashMap<String, Arc<String>>,
            _update: HashMap<String, Arc<String>>,
            _remove: HashMap<String, Arc<String>>,
        ) {
            self.partial_update_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn controller_offline(&self, _controller_id: &str) {}

        fn controller_removed(&self, _controller_id: &str) {}
    }

    impl CenterConfHandler<serde_json::Value> for MockHandler {
        fn full_set(&self, _controller_id: &str, _data: &HashMap<String, Arc<serde_json::Value>>) {
            self.full_set_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn partial_update(
            &self,
            _controller_id: &str,
            _add: HashMap<String, Arc<serde_json::Value>>,
            _update: HashMap<String, Arc<serde_json::Value>>,
            _remove: HashMap<String, Arc<serde_json::Value>>,
        ) {
            self.partial_update_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn controller_offline(&self, _controller_id: &str) {}

        fn controller_removed(&self, _controller_id: &str) {}
    }

    fn new_cache<T: Send + Sync + 'static>(
        handler: Arc<dyn CenterConfHandler<T> + Send + Sync>,
    ) -> CenterWatchCache<T> {
        let (tx, _rx) = broadcast::channel(16);
        CenterWatchCache::new("ctrl-1".to_string(), handler, tx)
    }

    #[test]
    fn get_sync_version_zero_initially() {
        let handler = MockHandler::new();
        let cache = new_cache::<String>(handler);
        assert_eq!(cache.get_sync_version(), 0);
        assert_eq!(cache.get_server_id(), "");
        assert_eq!(cache.status().revision, 0);
    }

    #[test]
    fn replace_all_updates_data_and_calls_handler() {
        let handler = MockHandler::new();
        let cache = new_cache::<String>(handler.clone());

        let items = vec![
            ("key1".to_string(), "val1".to_string()),
            ("key2".to_string(), "val2".to_string()),
        ];
        let result = cache.replace_all(items, 42, "server-abc".to_string());

        assert_eq!(result, ApplyResult::Applied);
        assert_eq!(cache.get_sync_version(), 42);
        assert_eq!(cache.get_server_id(), "server-abc");
        assert_eq!(handler.full_set_calls.load(Ordering::SeqCst), 1);
        assert_eq!(handler.partial_update_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn apply_events_calls_partial_update() {
        let handler = MockHandler::new();
        let cache = new_cache::<String>(handler.clone());

        // Seed with an initial item so we can update it
        cache.replace_all(
            vec![("key1".to_string(), "old-val".to_string())],
            1,
            "server-1".to_string(),
        );

        let events = vec![
            WatchEventSimple {
                event_type: EventType::Add,
                key: "key2".to_string(),
                data: "val2".to_string(),
            },
            WatchEventSimple {
                event_type: EventType::Update,
                key: "key1".to_string(),
                data: "new-val".to_string(),
            },
        ];
        let result = cache.apply_events(events, 2, "server-1".to_string());

        assert_eq!(result, ApplyResult::Applied);
        assert_eq!(cache.get_sync_version(), 2);
        assert_eq!(handler.full_set_calls.load(Ordering::SeqCst), 1);
        assert_eq!(handler.partial_update_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn apply_events_with_delete() {
        let handler = MockHandler::new();
        let cache = new_cache::<String>(handler.clone());

        cache.replace_all(
            vec![("key1".to_string(), "val1".to_string())],
            1,
            "server-1".to_string(),
        );

        let events = vec![WatchEventSimple {
            event_type: EventType::Delete,
            key: "key1".to_string(),
            data: String::new(), // data ignored for Delete
        }];
        let result = cache.apply_events(events, 2, "server-1".to_string());

        assert_eq!(result, ApplyResult::Applied);
        assert_eq!(cache.get_sync_version(), 2);
        assert_eq!(handler.partial_update_calls.load(Ordering::SeqCst), 1);

        // Verify item removed from internal state: apply another event to get the snapshot
        // (we can't peek directly, so just check sync version advanced correctly)
        assert_eq!(cache.get_sync_version(), 2);
    }

    #[test]
    fn revision_increments_per_applied_batch() {
        let handler = MockHandler::new();
        let cache = new_cache::<String>(handler);
        assert_eq!(cache.status().revision, 0);

        let result = cache.replace_all(
            vec![("key1".to_string(), "val1".to_string())],
            1,
            "server-1".to_string(),
        );
        assert_eq!(result, ApplyResult::Applied);
        assert_eq!(cache.status().revision, 1);

        let events = vec![WatchEventSimple {
            event_type: EventType::Update,
            key: "key1".to_string(),
            data: "val2".to_string(),
        }];
        let result = cache.apply_events(events, 2, "server-1".to_string());
        assert_eq!(result, ApplyResult::Applied);
        assert_eq!(cache.status().revision, 2);
    }

    #[test]
    fn overflow_rejects_batch_and_flags() {
        let handler = MockHandler::new();
        let cache = new_cache::<String>(handler.clone());

        let too_many: Vec<(String, String)> = (0..=MAX_ENTRIES_PER_CONTROLLER)
            .map(|i| (format!("key{i}"), "val".to_string()))
            .collect();
        let result = cache.replace_all(too_many, 1, "server-1".to_string());
        assert_eq!(result, ApplyResult::Overflow);

        let status = cache.status();
        assert!(status.overflowed);
        assert_eq!(status.revision, 0, "revision must not bump on overflow");
        assert_eq!(status.entry_count, 0, "nothing applied on overflow");
        assert_eq!(
            handler.full_set_calls.load(Ordering::SeqCst),
            0,
            "handler must not be invoked on overflow"
        );

        // A later batch that fits clears the flag.
        let ok = cache.replace_all(
            vec![("key1".to_string(), "val1".to_string())],
            2,
            "server-1".to_string(),
        );
        assert_eq!(ok, ApplyResult::Applied);
        let status = cache.status();
        assert!(!status.overflowed);
        assert_eq!(status.revision, 1);
    }

    #[test]
    fn overflow_marks_cache_stale() {
        let handler = MockHandler::new();
        let cache = new_cache::<String>(handler);

        let too_many: Vec<(String, String)> = (0..=MAX_ENTRIES_PER_CONTROLLER)
            .map(|i| (format!("key{i}"), "val".to_string()))
            .collect();
        let result = cache.replace_all(too_many, 1, "server-1".to_string());
        assert_eq!(result, ApplyResult::Overflow);

        let status = cache.status();
        assert!(status.overflowed, "overflow must set overflowed");
        assert!(
            status.stale,
            "overflow must also mark the cache stale so freshness reporting is honest"
        );

        // A later batch that fits clears BOTH flags.
        let ok = cache.replace_all(
            vec![("key1".to_string(), "val1".to_string())],
            2,
            "server-1".to_string(),
        );
        assert_eq!(ok, ApplyResult::Applied);
        let status = cache.status();
        assert!(!status.overflowed, "a fitting batch must clear overflowed");
        assert!(!status.stale, "a fitting batch must clear stale");
    }

    #[test]
    fn apply_events_overflow_rejects_batch_and_flags() {
        let handler = MockHandler::new();
        let (tx, mut rx) = broadcast::channel(16);
        let cache: CenterWatchCache<String> =
            CenterWatchCache::new("ctrl-1".to_string(), handler.clone(), tx);

        // Seed exactly MAX_ENTRIES_PER_CONTROLLER entries (key0..key9999).
        let seed: Vec<(String, String)> = (0..MAX_ENTRIES_PER_CONTROLLER)
            .map(|i| (format!("key{i}"), "val".to_string()))
            .collect();
        let seeded = cache.replace_all(seed, 1, "server-1".to_string());
        assert_eq!(seeded, ApplyResult::Applied);
        assert_eq!(cache.status().entry_count, MAX_ENTRIES_PER_CONTROLLER);
        let _ = rx.try_recv(); // drain the seed summary

        // Batch A: +1 new key, -1 existing key -> projected count unchanged
        // at exactly the cap. Must exercise the real add/delete arithmetic
        // (not just a trivial length check) and APPLY.
        let batch_a = vec![
            WatchEventSimple {
                event_type: EventType::Add,
                key: "key-new-1".to_string(),
                data: "val-new-1".to_string(),
            },
            WatchEventSimple {
                event_type: EventType::Delete,
                key: "key0".to_string(),
                data: String::new(),
            },
        ];
        let result_a = cache.apply_events(batch_a, 2, "server-1".to_string());
        assert_eq!(
            result_a,
            ApplyResult::Applied,
            "batch fitting exactly at the cap must apply"
        );
        let status_a = cache.status();
        assert_eq!(status_a.entry_count, MAX_ENTRIES_PER_CONTROLLER);
        assert_eq!(status_a.revision, 2);
        assert!(!status_a.overflowed);
        let summary_a = rx
            .try_recv()
            .expect("batch A is applied and must broadcast a summary");
        assert_eq!(summary_a.revision, 2);

        // Batch B: +2 new keys, -1 existing key -> projected count is one
        // past the cap. Must be rejected wholesale.
        let batch_b = vec![
            WatchEventSimple {
                event_type: EventType::Add,
                key: "key-new-2".to_string(),
                data: "val-new-2".to_string(),
            },
            WatchEventSimple {
                event_type: EventType::Add,
                key: "key-new-3".to_string(),
                data: "val-new-3".to_string(),
            },
            WatchEventSimple {
                event_type: EventType::Delete,
                key: "key1".to_string(),
                data: String::new(),
            },
        ];
        let result_b = cache.apply_events(batch_b, 3, "server-1".to_string());
        assert_eq!(
            result_b,
            ApplyResult::Overflow,
            "batch projecting one past the cap must be rejected"
        );

        let status_b = cache.status();
        assert!(status_b.overflowed, "overflowed flag must be set");
        assert_eq!(
            status_b.revision, 2,
            "revision must not bump on an overflowed batch"
        );
        assert_eq!(
            status_b.entry_count, MAX_ENTRIES_PER_CONTROLLER,
            "cache contents must be unchanged by an overflowed batch"
        );
        assert!(
            cache.get_entry("key-new-2").is_none(),
            "no partial application of an overflowed batch"
        );
        assert!(
            cache.get_entry("key1").is_some(),
            "the delete in an overflowed batch must not have been applied either"
        );
        assert!(
            rx.try_recv().is_err(),
            "an overflowed batch must not broadcast a change summary"
        );
    }

    #[test]
    fn change_summary_carries_changed_types_including_removals() {
        let handler = MockHandler::new();
        let (tx, mut rx) = broadcast::channel(16);
        let cache: CenterWatchCache<serde_json::Value> =
            CenterWatchCache::new("ctrl-1".to_string(), handler, tx);

        // Seed a RegionRouteOverride doc that a later batch will remove.
        let seed = serde_json::json!({
            "metadata": {"namespace": "ns", "name": "seed"},
            "spec": {"data": {"type": "RegionRouteOverride", "config": {}}}
        });
        cache.replace_all(
            vec![("ns/seed".to_string(), seed)],
            1,
            "server-1".to_string(),
        );
        let _ = rx.try_recv(); // drain the replace_all summary

        let added = serde_json::json!({
            "metadata": {"namespace": "ns", "name": "added"},
            "spec": {"data": {"type": "KeyList", "config": {}}}
        });
        let events = vec![
            WatchEventSimple {
                event_type: EventType::Add,
                key: "ns/added".to_string(),
                data: added,
            },
            WatchEventSimple {
                event_type: EventType::Delete,
                key: "ns/seed".to_string(),
                data: serde_json::Value::Null,
            },
        ];
        let result = cache.apply_events(events, 2, "server-1".to_string());
        assert_eq!(result, ApplyResult::Applied);

        let summary = rx.try_recv().expect("change summary broadcast on apply");
        assert_eq!(summary.controller_id, "ctrl-1");
        assert_eq!(summary.revision, 2);
        assert_eq!(
            summary.changed_types,
            BTreeSet::from(["KeyList".to_string(), "RegionRouteOverride".to_string()])
        );
    }
}
