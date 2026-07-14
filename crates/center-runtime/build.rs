//! Generate the federation gRPC bindings owned by the shared runtime crate.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let proto_dir = "proto";
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("fed_sync_descriptor.bin"))
        .compile_protos(&[format!("{proto_dir}/fed_sync.proto")], &[proto_dir])?;
    Ok(())
}
