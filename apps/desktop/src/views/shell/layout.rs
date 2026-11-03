//! The window layout kept on this Mac (`layout.json` under the data root):
//! the v1 column widths (`views/columns.rs`) and, from 1.1.0, the rail mode
//! and the theme choice of 设置 → 外观. Every writer merges its own keys, so
//! an older build keeps reading the keys it knows. Only the app reads and
//! writes it; self-tests never do.

use crate::{app::RailMode, theme::ThemeChoice};

/// The v2 settings in `layout.json`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutPrefs {
    pub rail_mode: RailMode,
    pub theme: ThemeChoice,
}

fn read() -> serde_json::Map<String, serde_json::Value> {
    std::fs::read(crate::views::columns::layout_file())
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Reads the v2 settings; anything missing or unknown is the default.
pub fn load() -> LayoutPrefs {
    parse(&read())
}

fn parse(map: &serde_json::Map<String, serde_json::Value>) -> LayoutPrefs {
    let text = |k: &str| map.get(k).and_then(|v| v.as_str()).unwrap_or_default();
    LayoutPrefs {
        rail_mode: RailMode::from_slug(text("rail_mode")).unwrap_or_default(),
        theme: ThemeChoice::from_slug(text("theme")).unwrap_or_default(),
    }
}

/// Sets `keys` in `layout.json` and keeps every other key.
pub fn merge(keys: &[(&str, serde_json::Value)]) {
    let mut map = read();
    for (k, v) in keys {
        map.insert((*k).to_string(), v.clone());
    }
    let path = crate::views::columns::layout_file();
    let json = serde_json::to_vec(&serde_json::Value::Object(map)).unwrap_or_default();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(error) = std::fs::write(&path, json) {
        tracing::warn!("layout: could not save the layout: {error}");
    }
}

/// Saves the rail mode.
pub fn save_rail_mode(mode: RailMode) {
    merge(&[("rail_mode", serde_json::json!(mode.slug()))]);
}

/// Saves the theme choice.
pub fn save_theme(choice: ThemeChoice) {
    merge(&[("theme", serde_json::json!(choice.slug()))]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_keys_sit_beside_the_column_widths() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"sidebar":250.0,"topic_list":300.0,"rail_mode":"member","theme":"dark"}"#,
        )
        .unwrap();
        let prefs = parse(v.as_object().unwrap());
        assert_eq!(prefs.rail_mode, RailMode::Member);
        assert_eq!(prefs.theme, ThemeChoice::Dark);
        // A 1.0.x file has only the widths: the defaults.
        let old: serde_json::Value = serde_json::from_str(r#"{"sidebar":250.0}"#).unwrap();
        assert_eq!(parse(old.as_object().unwrap()), LayoutPrefs::default());
    }
}
