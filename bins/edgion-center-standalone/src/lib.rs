//! Standalone Edgion Center composition library.

pub use edgion_center_app::{aggregator, api, commander, metadata_store, poll, proxy, watch_cache};
mod cli;
mod common;
mod config;
mod fed_sync;
mod store;

use cli::EdgionCenterCli;
use common::startup::{init_crypto, install_panic_hook};

/// Initialize process-wide runtime facilities, parse CLI arguments, and run
/// the standalone SQL-backed application.
pub async fn entrypoint() -> anyhow::Result<()> {
    install_panic_hook();
    init_crypto();

    let cli = EdgionCenterCli::parse_args();
    cli.run().await
}
