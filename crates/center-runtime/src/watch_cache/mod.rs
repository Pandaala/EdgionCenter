pub mod cache;
pub mod registry;
pub mod traits;

pub use cache::{CenterWatchCache, EventType, WatchEventSimple};
pub use registry::CenterWatchCacheRegistry;
pub use traits::CenterConfHandler;

/// Platform-neutral wire representation for watched configuration resources.
///
/// The runtime only needs to retain the payload and derive its metadata key; it
/// deliberately does not link Kubernetes-generated resource types.
pub type WatchedConfigData = serde_json::Value;

/// Top-level container for all per-kind watch registries.
/// Mirrors Gateway's ConfigClient — add a field for each new resource kind.
pub struct CenterSyncClient {
    pub plugin_metadata: CenterWatchCacheRegistry<WatchedConfigData>,
    // Future: pub http_routes: CenterWatchCacheRegistry<HTTPRoute>,
}
