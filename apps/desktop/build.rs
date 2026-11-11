//! Exports product.json fields as compile-time constants so the product name lives in one place:
//! PRODUCT_NAME, PRODUCT_SLUG, PRODUCT_BUNDLE_ID and PRODUCT_URL_SCHEME.
//!
//! Private deployment data comes from an optional ops checkout: `$TC_OPS_DIR`, else
//! `$HOME/.config/team-chat/ops` when that directory exists. Open-source builds have neither and
//! build fine without it:
//! - `deploy/servers.json` (`{"e2e": "…", "staging": "…", "production": "…"}`) gives the server
//!   URLs PRODUCT_STAGING_URL and PRODUCT_PRODUCTION_URL. Without it they come from a `servers`
//!   key in product.json when there is one, else they are empty (the app then hides them).
//! - `characters/<id>/<status>-<px>.svg` and `characters/boxes.json`, the optional character
//!   pack, become `character_assets.rs` (an embedded table that `src/characters/assets.rs`
//!   serves through the app's asset source) and `character_boxes.rs`. Without the pack both
//!   are empty and members with a character draw their initial instead.
//! - `roster/roster.json` becomes `bot_roster.rs`, the default character of a new bot by name.
//!   Without it the table is empty.
//!
//! On Windows it also compiles the exe's icon and version resources (`embed_windows_resources`).
//!
//! It also writes `release_notes.rs`: every `docs/releases/<version>.md`, newest first, which
//! the 更新记录 and the 更新后 sheet of `src/updater/notes.rs` read without a network.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

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
    let ops = ops_dir();
    let servers = ops
        .as_deref()
        .and_then(|ops| read_json(&ops.join("deploy/servers.json")))
        .unwrap_or_else(|| json["servers"].clone());
    for (key, var) in [
        ("staging", "PRODUCT_STAGING_URL"),
        ("production", "PRODUCT_PRODUCTION_URL"),
    ] {
        let value = servers[key].as_str().unwrap_or("");
        println!("cargo:rustc-env={var}={value}");
    }
    embed_characters(ops.as_deref());
    embed_roster(ops.as_deref());
    embed_release_notes(&manifest);
    embed_windows_resources(&manifest, json["name"].as_str().unwrap_or_default());
}

/// The application manifest of the Windows exe. Without one Windows treats the process as
/// not DPI aware and stretches its windows as a bitmap on any display above 100 % (GPUI
/// never sets the awareness itself), draws the system's controls (message boxes, the tray
/// menu, the 产出 window's version list) in the Windows 2000 look, and reports itself as
/// Windows 8. Same content as GPUI's own `gpui.manifest.xml`, plus long paths.
#[cfg(windows)]
const WINDOWS_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

/// Windows: gives the exe its icon and its version block (file and product version from the
/// Cargo version, the product name from product.json), which Explorer, the taskbar and
/// 应用和功能 show. Needs the resource compiler of the Windows SDK, so it runs only when the
/// build host is Windows too; a cross-build from another system (a compile check) skips it.
fn embed_windows_resources(manifest: &Path, name: &str) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // A plain path without `..`: the resource compiler reads it from a script.
    let Some(root) = manifest.parent().and_then(Path::parent) else {
        return;
    };
    let icon = root.join("assets").join("brand").join("AppIcon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(&icon.display().to_string());
        res.set("ProductName", name);
        res.set("FileDescription", name);
        res.set("InternalName", name);
        res.set("OriginalFilename", &format!("{name}.exe"));
        res.set_manifest(WINDOWS_MANIFEST);
        res.compile().expect("compile the Windows resources");
    }
    #[cfg(not(windows))]
    let _ = name;
}

/// The private ops checkout: `$TC_OPS_DIR`, else `$HOME/.config/team-chat/ops` when it is a
/// directory (or a symlink to one).
fn ops_dir() -> Option<PathBuf> {
    println!("cargo:rerun-if-env-changed=TC_OPS_DIR");
    println!("cargo:rerun-if-env-changed=HOME");
    if let Some(dir) = env::var_os("TC_OPS_DIR").filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        assert!(
            dir.is_dir(),
            "TC_OPS_DIR={} is not a directory",
            dir.display()
        );
        return Some(dir);
    }
    let fallback = PathBuf::from(env::var_os("HOME")?).join(".config/team-chat/ops");
    fallback.is_dir().then_some(fallback)
}

/// The JSON at `path` when the file exists (watched for changes); a broken file fails the build.
fn read_json(path: &Path) -> Option<serde_json::Value> {
    if !path.is_file() {
        return None;
    }
    println!("cargo:rerun-if-changed={}", path.display());
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Some(serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display())))
}

fn write_out(name: &str, text: String) {
    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join(name);
    fs::write(&dest, text).unwrap_or_else(|e| panic!("write {}: {e}", dest.display()));
}

/// `(version, text)` for every `docs/releases/<MAJOR.MINOR.PATCH>.md`, newest first.
fn embed_release_notes(manifest: &Path) {
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
    notes.sort_by_key(|n| std::cmp::Reverse(n.0));
    let mut out = String::from("pub static RELEASES: &[(&str, &str)] = &[\n");
    for (_, version, path) in &notes {
        out.push_str(&format!(
            "    ({version:?}, include_str!({:?})),\n",
            path.display().to_string()
        ));
    }
    out.push_str("];\n");
    write_out("release_notes.rs", out);
}

/// `(asset path, bytes)` for every `characters/<id>/<status>-<px>.svg` of the optional
/// character pack, sorted by path, and the pack's `boxes.json`. Both are empty without it.
fn embed_characters(ops: Option<&Path>) {
    let mut files = Vec::new();
    let mut boxes = None;
    if let Some(root) = ops.map(|o| o.join("characters")).filter(|r| r.is_dir()) {
        println!("cargo:rerun-if-changed={}", root.display());
        for dir in fs::read_dir(&root).expect("read the character pack") {
            let dir = dir.unwrap().path();
            if !dir.is_dir() {
                continue;
            }
            println!("cargo:rerun-if-changed={}", dir.display());
            for file in fs::read_dir(&dir).unwrap() {
                let file = file.unwrap().path();
                if file.extension().is_some_and(|e| e == "svg") {
                    let rel = file.strip_prefix(&root).unwrap();
                    let rel = format!("characters/{}", rel.to_string_lossy().replace('\\', "/"));
                    files.push((rel, file));
                }
            }
        }
        let path = root.join("boxes.json");
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
            boxes = Some(path);
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
    write_out("character_assets.rs", out);
    let boxes = match boxes {
        Some(path) => format!(
            "pub static BOXES_JSON: &str = include_str!({:?});\n",
            path.display().to_string()
        ),
        None => "pub static BOXES_JSON: &str = \"{}\";\n".to_string(),
    };
    write_out("character_boxes.rs", boxes);
}

/// `(lowercase name, character id)` for every bot of `roster/roster.json`; empty without it.
fn embed_roster(ops: Option<&Path>) {
    let mut out = String::from("pub static ROSTER: &[(&str, &str)] = &[\n");
    if let Some(roster) = ops.and_then(|o| read_json(&o.join("roster/roster.json"))) {
        for bot in roster["bots"].as_array().into_iter().flatten() {
            if let (Some(name), Some(c)) = (bot["name"].as_str(), bot["character"].as_str()) {
                let name = name.trim().to_lowercase();
                out.push_str(&format!("    ({name:?}, {c:?}),\n"));
            }
        }
    }
    out.push_str("];\n");
    write_out("bot_roster.rs", out);
}
