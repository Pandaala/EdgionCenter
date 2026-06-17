pub mod cache;
pub mod registry;
pub mod traits;

pub use cache::{CenterWatchCache, WatchEventSimple};
pub use registry::CenterWatchCacheRegistry;
pub use traits::CenterConfHandler;

// Renamed from PluginMetaData to EdgionConfigData (upstream migration).
use edgion_resources::resources::edgion_config_data::EdgionConfigData;

/// Top-level container for all per-kind watch registries.
/// Mirrors Gateway's ConfigClient — add a field for each new resource kind.
pub struct CenterSyncClient {
    // Field name kept intentionally; only the generic type changed from PluginMetaData to EdgionConfigData.
    pub plugin_metadata: CenterWatchCacheRegistry<EdgionConfigData>,
    // Future: pub http_routes: CenterWatchCacheRegistry<HTTPRoute>,
}
