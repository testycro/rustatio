use std::path::Path;

/// Name of the admin daemon executable bundled as a sidecar (`bundle.externalBin`).
const SIDECAR: &str = "rustatio-daemon";

/// `tauri-build` refuses to run when a sidecar file is missing. The real one is produced by
/// `scripts/build_daemon_sidecar.py` (`PyInstaller`); until then an empty placeholder lets
/// `cargo check`, `clippy` and `cargo test` work. At runtime the app ignores a placeholder.
fn ensure_sidecar_placeholders() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };

    let mut triples = vec![target.clone()];
    // Universal macOS builds look for one file covering both architectures
    if target.ends_with("apple-darwin") {
        triples.push("universal-apple-darwin".to_string());
    }

    let extension = if target.contains("windows") { ".exe" } else { "" };
    let dir = Path::new("binaries");

    for triple in triples {
        let path = dir.join(format!("{SIDECAR}-{triple}{extension}"));
        if path.exists() {
            continue;
        }

        if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&path, b"")) {
            println!("cargo:warning=cannot create sidecar placeholder {}: {e}", path.display());
        }
    }
}

fn main() {
    ensure_sidecar_placeholders();
    println!("cargo:rerun-if-changed=binaries");
    tauri_build::build();
}
