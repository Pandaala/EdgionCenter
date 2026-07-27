pub mod cache;
pub mod read_model;
pub mod registry;
pub mod traits;

pub use cache::{ApplyResult, CacheStatus, CenterWatchCache, EventType, WatchEventSimple};
pub use read_model::ConfigDataEntry;
pub use registry::CenterWatchCacheRegistry;
pub use traits::CenterConfHandler;

/// Platform-neutral wire representation for watched configuration resources.
///
/// The runtime only needs to retain the payload and derive its metadata key; it
/// deliberately does not link Kubernetes-generated resource types.
pub type WatchedConfigData = serde_json::Value;

/// Maximum number of live entries a single controller's watch cache may hold.
/// A batch that would push the cache beyond this bound is rejected wholesale
/// (see `ApplyResult::Overflow`) instead of being applied and silently
/// degrading memory or lookup behavior.
pub const MAX_ENTRIES_PER_CONTROLLER: usize = 10_000;

/// Derives the ConfigData type discriminator from a cached value, so the
/// watch cache can summarize *what kind* of data changed (for SSE/invalidation
/// fan-out) without ever exposing resource bodies.
pub trait ConfigTyped {
    /// The ConfigData type discriminator, or None when absent/not a string.
    fn config_type(&self) -> Option<&str>;
}

impl ConfigTyped for serde_json::Value {
    fn config_type(&self) -> Option<&str> {
        self.pointer("/spec/data/type")
            .and_then(serde_json::Value::as_str)
    }
}

/// Test-support instantiation: watch_cache's own unit tests exercise
/// `CenterWatchCache<String>` as a lightweight handler double. Production
/// data flows exclusively through `WatchedConfigData` (`serde_json::Value`);
/// a bare `String` has no type discriminator, hence `None` ("Unknown" once
/// summarized). Gated out of non-test builds — nothing in production ever
/// instantiates `CenterWatchCache<String>`.
#[cfg(any(test, feature = "test-support"))]
impl ConfigTyped for String {
    fn config_type(&self) -> Option<&str> {
        None
    }
}

/// Type-scoped change summary for SSE/invalidation fan-out. Carries only the
/// set of ConfigData types touched by one applied batch — never bodies.
#[derive(Debug, Clone)]
pub struct ChangeSummary {
    pub controller_id: String,
    pub revision: u64,
    /// "Unknown" is used for untyped/undiscriminated documents.
    pub changed_types: std::collections::BTreeSet<String>,
    pub event_unix_ms: u64,
}

/// Top-level container for all per-kind watch registries.
/// Mirrors Gateway's ConfigClient — add a field for each new resource kind.
pub struct CenterSyncClient {
    pub plugin_metadata: CenterWatchCacheRegistry<WatchedConfigData>,
    // Future: pub http_routes: CenterWatchCacheRegistry<HTTPRoute>,
}
