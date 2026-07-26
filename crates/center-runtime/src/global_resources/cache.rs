//! Non-durable, bounded TTL cache for GlobalResources inventory snapshots.
//!
//! Values are held behind [`Arc`] so cache hits do not clone inventory payloads.
//! Callers provide an estimated byte size when inserting a value; the estimate
//! should include the retained resource payload and any significant indexes.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Configuration errors for [`BoundedTtlCache`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheConfigError {
    MaxEntries,
    MaxEstimatedBytes,
    Ttl,
}

impl std::fmt::Display for CacheConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::MaxEntries => "cache max entries must be greater than zero",
            Self::MaxEstimatedBytes => "cache max estimated bytes must be greater than zero",
            Self::Ttl => "cache TTL must be greater than zero",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CacheConfigError {}

/// Outcome of an insertion into [`BoundedTtlCache`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheInsertResult {
    Inserted,
    Replaced,
    RejectedTooLarge,
}

struct CacheEntry<V> {
    value: Arc<V>,
    estimated_bytes: usize,
    expires_at: Instant,
    last_access: u64,
}

struct CacheState<K, V> {
    entries: HashMap<K, CacheEntry<V>>,
    estimated_bytes: usize,
    access_clock: u64,
}

impl<K, V> Default for CacheState<K, V> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            estimated_bytes: 0,
            access_clock: 0,
        }
    }
}

/// A thread-safe in-memory cache bounded by entry count, estimated bytes, and TTL.
///
/// Eviction is approximate LRU. Reads and refreshes advance a monotonic access
/// sequence, and the least recently accessed entry is evicted when either
/// capacity bound is exceeded.
pub struct BoundedTtlCache<K, V> {
    state: Mutex<CacheState<K, V>>,
    max_entries: usize,
    max_estimated_bytes: usize,
    ttl: Duration,
}

#[allow(dead_code)]
impl<K, V> BoundedTtlCache<K, V>
where
    K: Clone + Eq + Hash,
{
    pub fn new(
        max_entries: usize,
        max_estimated_bytes: usize,
        ttl: Duration,
    ) -> Result<Self, CacheConfigError> {
        if max_entries == 0 {
            return Err(CacheConfigError::MaxEntries);
        }
        if max_estimated_bytes == 0 {
            return Err(CacheConfigError::MaxEstimatedBytes);
        }
        if ttl.is_zero() {
            return Err(CacheConfigError::Ttl);
        }

        Ok(Self {
            state: Mutex::new(CacheState::default()),
            max_entries,
            max_estimated_bytes,
            ttl,
        })
    }

    /// Returns a shared value and refreshes its approximate-LRU position.
    ///
    /// A read does not extend the entry TTL. Use [`Self::refresh`] when sliding
    /// expiration is explicitly desired.
    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        self.get_at(key, Instant::now())
    }

    /// Inserts a value with its retained-size estimate.
    ///
    /// Replacing a key resets its TTL. A value larger than the byte limit is
    /// rejected before the existing value, if any, is changed.
    pub fn insert(&self, key: K, value: Arc<V>, estimated_bytes: usize) -> CacheInsertResult {
        self.insert_at(key, value, estimated_bytes, Instant::now())
    }

    /// Extends an unexpired entry's TTL and refreshes its LRU position.
    pub fn refresh(&self, key: &K) -> bool {
        self.refresh_at(key, Instant::now())
    }

    pub fn remove(&self, key: &K) -> Option<Arc<V>> {
        let mut state = self.lock_state();
        let entry = state.entries.remove(key)?;
        state.estimated_bytes = state.estimated_bytes.saturating_sub(entry.estimated_bytes);
        Some(entry.value)
    }

    pub fn clear(&self) {
        let mut state = self.lock_state();
        state.entries.clear();
        state.estimated_bytes = 0;
    }

    /// Removes expired entries and returns the number of live entries.
    pub fn len(&self) -> usize {
        let mut state = self.lock_state();
        Self::purge_expired_locked(&mut state, Instant::now());
        state.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes expired entries and returns the live retained-size estimate.
    pub fn estimated_bytes(&self) -> usize {
        let mut state = self.lock_state();
        Self::purge_expired_locked(&mut state, Instant::now());
        state.estimated_bytes
    }

    /// Eagerly removes expired entries and returns the number removed.
    pub fn purge_expired(&self) -> usize {
        let mut state = self.lock_state();
        Self::purge_expired_locked(&mut state, Instant::now())
    }

    fn get_at(&self, key: &K, now: Instant) -> Option<Arc<V>> {
        let mut state = self.lock_state();
        if state
            .entries
            .get(key)
            .is_some_and(|entry| entry.expires_at <= now)
        {
            Self::remove_locked(&mut state, key);
            return None;
        }

        let access = Self::next_access(&mut state);
        let entry = state.entries.get_mut(key)?;
        entry.last_access = access;
        Some(Arc::clone(&entry.value))
    }

    fn insert_at(
        &self,
        key: K,
        value: Arc<V>,
        estimated_bytes: usize,
        now: Instant,
    ) -> CacheInsertResult {
        if estimated_bytes > self.max_estimated_bytes {
            return CacheInsertResult::RejectedTooLarge;
        }

        let mut state = self.lock_state();
        Self::purge_expired_locked(&mut state, now);
        let replaced = Self::remove_locked(&mut state, &key).is_some();
        let access = Self::next_access(&mut state);
        state.estimated_bytes = state.estimated_bytes.saturating_add(estimated_bytes);
        state.entries.insert(
            key,
            CacheEntry {
                value,
                estimated_bytes,
                expires_at: now + self.ttl,
                last_access: access,
            },
        );

        self.evict_to_limits(&mut state);
        if replaced {
            CacheInsertResult::Replaced
        } else {
            CacheInsertResult::Inserted
        }
    }

    fn refresh_at(&self, key: &K, now: Instant) -> bool {
        let mut state = self.lock_state();
        if state
            .entries
            .get(key)
            .is_some_and(|entry| entry.expires_at <= now)
        {
            Self::remove_locked(&mut state, key);
            return false;
        }

        let access = Self::next_access(&mut state);
        let Some(entry) = state.entries.get_mut(key) else {
            return false;
        };
        entry.expires_at = now + self.ttl;
        entry.last_access = access;
        true
    }

    fn evict_to_limits(&self, state: &mut CacheState<K, V>) {
        while state.entries.len() > self.max_entries
            || state.estimated_bytes > self.max_estimated_bytes
        {
            let Some(lru_key) = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            let entry = state
                .entries
                .remove(&lru_key)
                .expect("selected cache entry must still exist");
            state.estimated_bytes = state.estimated_bytes.saturating_sub(entry.estimated_bytes);
        }
    }

    fn purge_expired_locked(state: &mut CacheState<K, V>, now: Instant) -> usize {
        let before = state.entries.len();
        state.entries.retain(|_, entry| {
            if entry.expires_at <= now {
                state.estimated_bytes = state.estimated_bytes.saturating_sub(entry.estimated_bytes);
                false
            } else {
                true
            }
        });
        before - state.entries.len()
    }

    fn remove_locked(state: &mut CacheState<K, V>, key: &K) -> Option<CacheEntry<V>> {
        let entry = state.entries.remove(key)?;
        state.estimated_bytes = state.estimated_bytes.saturating_sub(entry.estimated_bytes);
        Some(entry)
    }

    fn next_access(state: &mut CacheState<K, V>) -> u64 {
        state.access_clock = state.access_clock.saturating_add(1);
        state.access_clock
    }

    fn lock_state(&self) -> MutexGuard<'_, CacheState<K, V>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(
        max_entries: usize,
        max_estimated_bytes: usize,
        ttl: Duration,
    ) -> BoundedTtlCache<String, String> {
        BoundedTtlCache::new(max_entries, max_estimated_bytes, ttl).unwrap()
    }

    #[test]
    fn rejects_invalid_limits() {
        assert_eq!(
            BoundedTtlCache::<String, String>::new(0, 1, Duration::from_secs(1))
                .err()
                .unwrap(),
            CacheConfigError::MaxEntries
        );
        assert_eq!(
            BoundedTtlCache::<String, String>::new(1, 0, Duration::from_secs(1))
                .err()
                .unwrap(),
            CacheConfigError::MaxEstimatedBytes
        );
        assert_eq!(
            BoundedTtlCache::<String, String>::new(1, 1, Duration::ZERO)
                .err()
                .unwrap(),
            CacheConfigError::Ttl
        );
    }

    #[test]
    fn expires_entries_and_cleans_up_bytes() {
        let cache = cache(4, 100, Duration::from_secs(10));
        let start = Instant::now();
        cache.insert_at("a".into(), Arc::new("value".into()), 40, start);

        assert_eq!(
            cache.get_at(&"a".into(), start + Duration::from_secs(9)),
            Some(Arc::new("value".into()))
        );
        assert_eq!(
            cache.get_at(&"a".into(), start + Duration::from_secs(10)),
            None
        );
        assert_eq!(cache.lock_state().estimated_bytes, 0);
    }

    #[test]
    fn evicts_least_recently_used_entry_at_entry_limit() {
        let cache = cache(2, 100, Duration::from_secs(60));
        let start = Instant::now();
        cache.insert_at("a".into(), Arc::new("a".into()), 10, start);
        cache.insert_at("b".into(), Arc::new("b".into()), 10, start);
        assert!(cache
            .get_at(&"a".into(), start + Duration::from_secs(1))
            .is_some());

        cache.insert_at(
            "c".into(),
            Arc::new("c".into()),
            10,
            start + Duration::from_secs(2),
        );

        assert!(cache
            .get_at(&"a".into(), start + Duration::from_secs(3))
            .is_some());
        assert!(cache
            .get_at(&"b".into(), start + Duration::from_secs(3))
            .is_none());
        assert!(cache
            .get_at(&"c".into(), start + Duration::from_secs(3))
            .is_some());
    }

    #[test]
    fn evicts_until_estimated_byte_limit_is_met() {
        let cache = cache(10, 50, Duration::from_secs(60));
        let start = Instant::now();
        cache.insert_at("a".into(), Arc::new("a".into()), 20, start);
        cache.insert_at("b".into(), Arc::new("b".into()), 20, start);
        assert!(cache
            .get_at(&"a".into(), start + Duration::from_secs(1))
            .is_some());

        cache.insert_at(
            "c".into(),
            Arc::new("c".into()),
            25,
            start + Duration::from_secs(2),
        );

        assert!(cache
            .get_at(&"a".into(), start + Duration::from_secs(3))
            .is_some());
        assert!(cache
            .get_at(&"b".into(), start + Duration::from_secs(3))
            .is_none());
        assert!(cache
            .get_at(&"c".into(), start + Duration::from_secs(3))
            .is_some());
        assert_eq!(cache.lock_state().estimated_bytes, 45);
    }

    #[test]
    fn refresh_extends_ttl_without_cloning_value() {
        let cache = cache(2, 100, Duration::from_secs(10));
        let start = Instant::now();
        let value = Arc::new("value".to_owned());
        cache.insert_at("a".into(), Arc::clone(&value), 10, start);

        assert!(cache.refresh_at(&"a".into(), start + Duration::from_secs(5)));
        let hit = cache
            .get_at(&"a".into(), start + Duration::from_secs(14))
            .unwrap();
        assert!(Arc::ptr_eq(&value, &hit));
        assert!(cache
            .get_at(&"a".into(), start + Duration::from_secs(15))
            .is_none());
    }

    #[test]
    fn replacing_entry_updates_size_and_ttl() {
        let cache = cache(2, 100, Duration::from_secs(10));
        let start = Instant::now();
        assert_eq!(
            cache.insert_at("a".into(), Arc::new("old".into()), 70, start),
            CacheInsertResult::Inserted
        );
        assert_eq!(
            cache.insert_at(
                "a".into(),
                Arc::new("new".into()),
                20,
                start + Duration::from_secs(5)
            ),
            CacheInsertResult::Replaced
        );

        assert_eq!(cache.lock_state().estimated_bytes, 20);
        assert_eq!(
            cache.get_at(&"a".into(), start + Duration::from_secs(14)),
            Some(Arc::new("new".into()))
        );
    }

    #[test]
    fn oversized_replacement_preserves_existing_entry() {
        let cache = cache(2, 50, Duration::from_secs(10));
        let start = Instant::now();
        cache.insert_at("a".into(), Arc::new("old".into()), 20, start);

        assert_eq!(
            cache.insert_at(
                "a".into(),
                Arc::new("oversized".into()),
                51,
                start + Duration::from_secs(1)
            ),
            CacheInsertResult::RejectedTooLarge
        );
        assert_eq!(
            cache.get_at(&"a".into(), start + Duration::from_secs(2)),
            Some(Arc::new("old".into()))
        );
    }

    #[test]
    fn cache_is_send_and_sync_for_thread_safe_types() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BoundedTtlCache<String, String>>();
    }
}
