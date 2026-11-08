//! Exports product.json fields as compile-time constants so the product name lives in one place:
//! PRODUCT_NAME, PRODUCT_SLUG, PRODUCT_BUNDLE_ID, PRODUCT_URL_SCHEME, and the server URLs
//! PRODUCT_STAGING_URL and PRODUCT_PRODUCTION_URL (empty when not set yet).
//! It also writes `character_assets.rs`: the bundled character SVGs (assets/characters/) as an
//! embedded table that `src/characters/assets.rs` serves through the app's asset source.
//! And it writes `release_notes.rs`: every `docs/releases/<version>.md`, newest first, which
//! the 更新记录 and the 更新后 sheet of `src/updater/notes.rs` read without a network.

use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let product = manifest.join("../../product.json");
    println!("cargo:rerun-if-changed={}", product.display());
    let text = fs::read_to_string(&product).expect("read product.json");
    let json: serde_json::Value = serde_json::from_str(&text).expect("parse product.json");
    for (key, var) in [
        ("name", "PRODUCT_NAME"),
        ("slug", "PRODUCT_SLUG"),
        ("bundleId", "PRODUCT_BUNDLE_ID"),
        ("urlScheme", "PRODUCT_URL_SCHEME"),
    ] {
        let value = json[key]
            .as_str()
            .unwrap_or_else(|| panic!("product.json is missing {key}"));
        println!("cargo:rustc-env={var}={value}");
    }
    for (key, var) in [
        ("staging", "PRODUCT_STAGING_URL"),
        ("production", "PRODUCT_PRODUCTION_URL"),
    ] {
        let value = json["servers"][key].as_str().unwrap_or("");
        println!("cargo:rustc-env={var}={value}");
    }
    embed_characters(&manifest);
    embed_release_notes(&manifest);
}

/// `(version, text)` for every `docs/releases/<MAJOR.MINOR.PATCH>.md`, newest first.
fn embed_release_notes(manifest: &std::path::Path) {
    let root = manifest.join("../../docs/releases");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut notes = Vec::new();
    if let Ok(dir) = fs::read_dir(&root) {
        for file in dir.flatten() {
            let path = file.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let parts: Vec<u64> = stem.split('.').filter_map(|p| p.parse().ok()).collect();
            if path.extension().is_some_and(|e| e == "md") && parts.len() == 3 {
                println!("cargo:rerun-if-changed={}", path.display());
                notes.push(((parts[0], parts[1], parts[2]), stem.to_string(), path));
            }
        }
    }
    notes.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = String::from("pub static RELEASES: &[(&str, &str)] = &[\n");
    for (_, version, path) in &notes {
        out.push_str(&format!(
            "    ({version:?}, include_str!({:?})),\n",
            path.display().to_string()
        ));
    }
    out.push_str("];\n");
    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("release_notes.rs");
    fs::write(dest, out).expect("write release_notes.rs");
}

/// `(asset path, bytes)` for every `assets/characters/<id>/<status>-<px>.svg`, sorted by path.
fn embed_characters(manifest: &std::path::Path) {
    let root = manifest.join("assets/characters");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    for dir in fs::read_dir(&root).expect("read assets/characters") {
        let dir = dir.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", dir.display());
        for file in fs::read_dir(&dir).unwrap() {
            let file = file.unwrap().path();
            if file.extension().is_some_and(|e| e == "svg") {
                let rel = file.strip_prefix(manifest.join("assets")).unwrap();
                files.push((rel.to_string_lossy().replace('\\', "/"), file));
            }
        }
    }
    files.sort();
    let mut out = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (path, file) in &files {
        out.push_str(&format!(
            "    ({path:?}, include_bytes!({:?})),\n",
            file.display().to_string()
        ));
    }
    out.push_str("];\n");
    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("character_assets.rs");
    fs::write(dest, out).expect("write character_assets.rs");
}
