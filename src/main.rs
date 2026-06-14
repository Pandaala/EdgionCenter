//! Edgion federation Hub (Center) — standalone binary.
//!
//! Extracted from the Edgion monorepo (`src/core/center/`). This is the
//! irreducible federation job: controller relay (fed-sync gRPC, mTLS),
//! resource aggregation + watch cache, and the admin-request proxy.

mod aggregator;
mod api;
mod cli;
mod commander;
mod common;
mod config;
mod fed_sync;
mod metadata_store;
mod proxy;
mod store;
mod watch_cache;

use crate::cli::EdgionCenterCli;
use crate::common::startup::{init_crypto, install_panic_hook};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    install_panic_hook();
    init_crypto();

    let cli = EdgionCenterCli::parse_args();
    if let Err(err) = cli.run().await {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}
