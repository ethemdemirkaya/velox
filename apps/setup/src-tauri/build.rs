//! Embeds the payload into the installer:
//! * `target/release/velox.exe` and `velox-host.exe`, zstd-compressed
//! * every file of `extension/`
//!
//! Build the desktop app in release mode first. Without it the installer
//! still compiles (CI lints) but refuses to install.

use std::path::{Path, PathBuf};

fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, p));
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../..");
    let root = root.canonicalize().unwrap_or(root);
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let release = root.join("target").join("release");

    for name in ["velox.exe", "velox-host.exe"] {
        let src = release.join(name);
        println!("cargo:rerun-if-changed={}", src.display());
        let data = match std::fs::read(&src) {
            Ok(b) => zstd::encode_all(&b[..], 19).expect("zstd"),
            Err(_) => {
                println!("cargo:warning=payload {name} not found – build the desktop app in release mode first");
                Vec::new()
            }
        };
        std::fs::write(out.join(format!("{name}.zst")), data).unwrap();
    }

    let ext = root.join("extension");
    println!("cargo:rerun-if-changed={}", ext.display());
    let mut files = Vec::new();
    walk(&ext, &ext, &mut files);
    files.sort();
    let mut code = String::from("pub static EXTENSION: &[(&str, &[u8])] = &[\n");
    for (rel, p) in &files {
        println!("cargo:rerun-if-changed={}", p.display());
        code.push_str(&format!("    ({rel:?}, include_bytes!({:?})),\n", p.to_string_lossy()));
    }
    code.push_str("];\n");
    std::fs::write(out.join("extension_files.rs"), code).unwrap();

    tauri_build::build();
}
