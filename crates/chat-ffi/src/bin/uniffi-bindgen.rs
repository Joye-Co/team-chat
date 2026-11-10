//! Generates the Swift (or Kotlin) sources from the built library:
//! `cargo run -p chat-ffi --features cli --bin uniffi-bindgen -- generate --library <lib> --language swift --out-dir <dir>`
fn main() {
    uniffi::uniffi_bindgen_main()
}
