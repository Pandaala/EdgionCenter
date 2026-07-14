//! Shared Edgion Center application layer.
//!
//! This crate owns HTTP APIs, OIDC/authz/audit middleware, observability, and
//! process-neutral composition helpers. It has no SQLx or Kube dependency;
//! standalone and Kubernetes binaries supply adapters through core ports.

pub mod aggregator {
    pub use edgion_center_runtime::aggregator::*;

    pub struct FedAggregatorMetrics;

    impl AggregatorMetrics for FedAggregatorMetrics {
        fn set_controller_count(&self, cluster: &str, count: u64) {
            crate::common::observe::fed_metrics::set_aggregator_controllers(cluster, count);
        }

        fn record_eviction(&self) {
            crate::common::observe::fed_metrics::record_evict_stale(
                crate::common::observe::fed_metrics::labels::evict_source::AGGREGATOR,
            );
        }
    }
}

pub mod api;
pub mod commander {
    pub use edgion_center_runtime::commander::*;
}
pub mod common;
pub mod fed_sync;
pub mod metadata_store {
    pub use edgion_center_runtime::metadata_store::*;
}
pub mod poll {
    pub use edgion_center_runtime::poll::*;
}
pub mod proxy {
    pub use edgion_center_runtime::proxy::*;
}
pub mod watch_cache {
    pub use edgion_center_runtime::watch_cache::*;
}

#[cfg(test)]
pub(crate) mod store {
    pub use edgion_center_adapter_sql::audit;
    pub use edgion_center_adapter_sql::Store;
}
