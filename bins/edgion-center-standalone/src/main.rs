//! Standalone SQLite/MySQL composition entry point.

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(err) = edgion_center_standalone::entrypoint().await {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}
