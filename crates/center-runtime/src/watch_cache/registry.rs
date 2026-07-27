use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;

use super::cache::{CacheStatus, CenterWatchCache};
use super::traits::CenterConfHandler;
use super::ChangeSummary;

/// Manages CenterWatchCache instances for a single resource kind.
/// Caches persist across controller reconnects to preserve sync_version.
pub struct CenterWatchCacheRegistry<T> {
    caches: RwLock<HashMap<String, Arc<CenterWatchCache<T>>>>,
    handler: Arc<dyn CenterConfHandler<T> + Send + Sync>,
    /// Broadcasts a `ChangeSummary` for every batch applied by any cache in
    /// this registry. Bounded (256); a lagging/full subscriber drops frames
    /// rather than backpressuring the watch stream.
    changes: broadcast::Sender<ChangeSummary>,
}

impl<T: Send + Sync + 'static> CenterWatchCacheRegistry<T> {
    pub fn new(handler: Arc<dyn CenterConfHandler<T> + Send + Sync>) -> Self {
        let (changes, _rx) = broadcast::channel(256);
        Self {
            caches: RwLock::new(HashMap::new()),
            handler,
            changes,
        }
    }

    /// Get or create a cache for a controller.
    /// On reconnect, returns the existing cache so sync_version is preserved.
    pub fn get_or_create(&self, controller_id: &str) -> Arc<CenterWatchCache<T>> {
        // Fast path: read lock
        {
            let caches = self.caches.read();
            if let Some(cache) = caches.get(controller_id) {
                return cache.clone();
            }
        }

        // Slow path: write lock, double-check
        let mut caches = self.caches.write();
        caches
            .entry(controller_id.to_string())
            .or_insert_with(|| {
                Arc::new(CenterWatchCache::new(
                    controller_id.to_string(),
                    self.handler.clone(),
                    self.changes.clone(),
                ))
            })
            .clone()
    }

    /// List all controllers with their sync state.
    /// Returns (controller_id, sync_version, server_id).
    pub fn list_controllers(&self) -> Vec<(String, u64, String)> {
        self.caches
            .read()
            .iter()
            .map(|(id, cache)| (id.clone(), cache.get_sync_version(), cache.get_server_id()))
            .collect()
    }

    /// Snapshot of every known controller's cache status (revision,
    /// staleness, overflow, entry count).
    pub fn statuses(&self) -> Vec<(String, CacheStatus)> {
        self.caches
            .read()
            .iter()
            .map(|(id, cache)| (id.clone(), cache.status()))
            .collect()
    }

    /// Subscribe to the type-scoped change broadcast for this registry.
    /// A new receiver only observes summaries sent after it subscribes.
    pub fn subscribe_changes(&self) -> broadcast::Receiver<ChangeSummary> {
        self.changes.subscribe()
    }

    /// Snapshot of every known controller's cache handle, taken under a
    /// single read-lock acquisition. Backing accessor for the production
    /// read model (`read_model::CenterWatchCacheRegistry::list_all`), which
    /// aggregates entries across every controller's cache.
    pub(crate) fn caches_snapshot(&self) -> Vec<(String, Arc<CenterWatchCache<T>>)> {
        self.caches
            .read()
            .iter()
            .map(|(id, cache)| (id.clone(), cache.clone()))
            .collect()
    }

    /// Existing cache for `controller_id`, without creating one if it does
    /// not exist yet. The non-creating counterpart to `get_or_create`:
    /// unlike that method, a miss here never inserts an entry into the
    /// registry. Backing accessor for the production read model
    /// (`read_model::CenterWatchCacheRegistry::raw_document`).
    pub(crate) fn get_if_present(&self, controller_id: &str) -> Option<Arc<CenterWatchCache<T>>> {
        self.caches.read().get(controller_id).cloned()
    }

    /// Mark controller offline. Preserves cache for reconnect.
    ///
    /// Flags the cache stale (if it exists) before notifying the handler, so
    /// any reader observing `status()` concurrently with the handler
    /// callback sees the offline state reflected.
    pub fn mark_offline(&self, controller_id: &str) {
        if let Some(cache) = self.caches.read().get(controller_id) {
            cache.set_stale();
        }
        self.handler.controller_offline(controller_id);
    }

    /// Remove controller entirely. Deletes cache and notifies handler.
    pub fn remove_controller(&self, controller_id: &str) {
        self.caches.write().remove(controller_id);
        self.handler.controller_removed(controller_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as StdHashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockHandler {
        offline_count: AtomicUsize,
        removed_count: AtomicUsize,
    }

    impl MockHandler {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                offline_count: AtomicUsize::new(0),
                removed_count: AtomicUsize::new(0),
            })
        }
    }

    impl CenterConfHandler<String> for MockHandler {
        fn full_set(&self, _controller_id: &str, _data: &StdHashMap<String, Arc<String>>) {}

        fn partial_update(
            &self,
            _controller_id: &str,
            _add: StdHashMap<String, Arc<String>>,
            _update: StdHashMap<String, Arc<String>>,
            _remove: StdHashMap<String, Arc<String>>,
        ) {
        }

        fn controller_offline(&self, _controller_id: &str) {
            self.offline_count.fetch_add(1, Ordering::SeqCst);
        }

        fn controller_removed(&self, _controller_id: &str) {
            self.removed_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn get_or_create_returns_same_instance() {
        let handler = MockHandler::new();
        let registry = CenterWatchCacheRegistry::<String>::new(handler);

        let cache1 = registry.get_or_create("ctrl-1");
        let cache2 = registry.get_or_create("ctrl-1");

        assert!(Arc::ptr_eq(&cache1, &cache2));
    }

    #[test]
    fn get_or_create_different_controllers() {
        let handler = MockHandler::new();
        let registry = CenterWatchCacheRegistry::<String>::new(handler);

        let cache1 = registry.get_or_create("ctrl-1");
        let cache2 = registry.get_or_create("ctrl-2");

        assert!(!Arc::ptr_eq(&cache1, &cache2));
    }

    #[test]
    fn mark_offline_calls_handler_preserves_cache() {
        let handler = MockHandler::new();
        let registry = CenterWatchCacheRegistry::<String>::new(handler.clone());

        let cache = registry.get_or_create("ctrl-1");
        // Simulate some sync progress
        cache.replace_all(
            vec![("key1".to_string(), "val1".to_string())],
            99,
            "server-1".to_string(),
        );

        registry.mark_offline("ctrl-1");

        assert_eq!(handler.offline_count.load(Ordering::SeqCst), 1);
        assert_eq!(handler.removed_count.load(Ordering::SeqCst), 0);

        // Cache is preserved: same instance returned, sync_version still 99
        let cache_after = registry.get_or_create("ctrl-1");
        assert!(Arc::ptr_eq(&cache, &cache_after));
        assert_eq!(cache_after.get_sync_version(), 99);
    }

    #[test]
    fn remove_controller_clears_cache() {
        let handler = MockHandler::new();
        let registry = CenterWatchCacheRegistry::<String>::new(handler.clone());

        let cache = registry.get_or_create("ctrl-1");
        cache.replace_all(
            vec![("key1".to_string(), "val1".to_string())],
            42,
            "server-1".to_string(),
        );

        registry.remove_controller("ctrl-1");

        assert_eq!(handler.removed_count.load(Ordering::SeqCst), 1);

        // New get_or_create must return a fresh cache with sync_version = 0
        let fresh_cache = registry.get_or_create("ctrl-1");
        assert!(!Arc::ptr_eq(&cache, &fresh_cache));
        assert_eq!(fresh_cache.get_sync_version(), 0);
    }

    #[test]
    fn stale_set_on_offline_cleared_on_next_batch() {
        let handler = MockHandler::new();
        let registry = CenterWatchCacheRegistry::<String>::new(handler.clone());

        let cache = registry.get_or_create("ctrl-1");
        cache.replace_all(
            vec![("key1".to_string(), "val1".to_string())],
            1,
            "server-1".to_string(),
        );
        assert!(!cache.status().stale);

        registry.mark_offline("ctrl-1");
        assert!(cache.status().stale, "set_stale must run on mark_offline");
        assert_eq!(handler.offline_count.load(Ordering::SeqCst), 1);

        // Next applied batch (simulating reconnect resuming the watch)
        // clears the staleness flag.
        cache.replace_all(
            vec![("key1".to_string(), "val1b".to_string())],
            2,
            "server-1".to_string(),
        );
        assert!(!cache.status().stale);
    }

    #[test]
    fn subscribe_changes_observes_applied_batches() {
        let handler = MockHandler::new();
        let registry = CenterWatchCacheRegistry::<String>::new(handler);
        let mut rx = registry.subscribe_changes();

        let cache = registry.get_or_create("ctrl-1");
        cache.replace_all(
            vec![("key1".to_string(), "val1".to_string())],
            1,
            "server-1".to_string(),
        );

        let summary = rx.try_recv().expect("registry broadcasts applied batches");
        assert_eq!(summary.controller_id, "ctrl-1");
        assert_eq!(summary.revision, 1);
    }

    #[test]
    fn statuses_reports_every_known_controller() {
        let handler = MockHandler::new();
        let registry = CenterWatchCacheRegistry::<String>::new(handler);
        registry.get_or_create("ctrl-1");
        registry.get_or_create("ctrl-2");

        let statuses = registry.statuses();
        let ids: Vec<&str> = statuses.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"ctrl-1"));
        assert!(ids.contains(&"ctrl-2"));
    }
}
