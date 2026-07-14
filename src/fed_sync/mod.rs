pub mod registry {
    pub use edgion_center_runtime::federation::registry::*;

    pub struct FedRegistryMetrics;

    impl RegistryMetrics for FedRegistryMetrics {
        fn record_session_reentry(&self) {
            crate::common::observe::fed_metrics::record_session_reentry();
        }

        fn record_eviction(&self) {
            crate::common::observe::fed_metrics::record_evict_stale(
                crate::common::observe::fed_metrics::labels::evict_source::REGISTRY,
            );
        }
    }
}
pub mod server;
