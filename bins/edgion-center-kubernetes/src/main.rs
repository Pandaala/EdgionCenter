#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(error) = edgion_center_kubernetes::entrypoint().await {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}
