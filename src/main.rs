//! Compatibility entry point for the current standalone EdgionCenter runtime.
//!
//! Business and runtime behavior lives in the library so future standalone and
//! Kubernetes binaries can remain thin composition roots.

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(err) = edgion_center::run().await {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}
