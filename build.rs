//! Build-time support for the compatibility binary.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ensure_embed_dashboard_placeholder();

    Ok(())
}

/// The built dashboard (`web/dist/`) is produced by `vite build` (or staged at
/// image-build time) and is gitignored — it is not vendored. But rust-embed's
/// `#[folder = "web/dist/"]` must find the folder at compile time. When the
/// `embed-dashboard` feature is on and no real build has been staged, write a
/// minimal placeholder so the feature still compiles standalone (e.g. local
/// `cargo check --features embed-dashboard`). A real built `index.html` is never
/// overwritten — the placeholder is only created when it is missing.
fn ensure_embed_dashboard_placeholder() {
    if std::env::var_os("CARGO_FEATURE_EMBED_DASHBOARD").is_none() {
        return;
    }
    let dir = std::path::Path::new("web/dist");
    let index = dir.join("index.html");
    if index.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(
        &index,
        "<!doctype html>\n<html lang=\"en\">\n  <head><meta charset=\"UTF-8\" />\
         <title>Edgion Center</title></head>\n  <body>\n    <div id=\"root\">\
         Edgion Center dashboard is not built into this binary.</div>\n  </body>\n</html>\n",
    );
}
