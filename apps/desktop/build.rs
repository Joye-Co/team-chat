//! Exports product.json fields as compile-time constants (PRODUCT_NAME, PRODUCT_SLUG,
//! PRODUCT_BUNDLE_ID, PRODUCT_URL_SCHEME) so the product name lives in one place.

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
        let value = json[key].as_str().unwrap_or_else(|| panic!("product.json is missing {key}"));
        println!("cargo:rustc-env={var}={value}");
    }
}
