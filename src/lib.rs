//! Shared EdgionCenter application library.
//!
//! The compatibility binary and future platform-specific binaries use this
//! library as their runtime entry point. Platform composition will move out of
//! this crate incrementally as adapters are extracted.

mod aggregator {
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
mod api;
mod cli;
mod commander {
    pub use edgion_center_runtime::commander::*;
}
mod common;
mod config;
mod core_ports;
mod fed_sync;
mod metadata_store {
    pub use edgion_center_runtime::metadata_store::*;
}
mod poll {
    pub use edgion_center_runtime::poll::*;
}
mod proxy {
    pub use edgion_center_runtime::proxy::*;
}
mod store;
mod watch_cache {
    pub use edgion_center_runtime::watch_cache::*;
}

use cli::EdgionCenterCli;
use common::startup::{init_crypto, install_panic_hook};

/// Initialize process-wide runtime facilities, parse CLI arguments, and run
/// the current compatibility application.
pub async fn run() -> anyhow::Result<()> {
    install_panic_hook();
    init_crypto();

    let cli = EdgionCenterCli::parse_args();
    cli.run().await
}
