//! The settings that stay on this Mac (NATIVE.md §8): the 通用 choices, the
//! 消息文字大小 of 外观 and the pane the window opened last. They are kept
//! under one `settings` key in `layout.json`, beside the column widths; a
//! self-test reads the defaults and never writes.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};

/// The smallest and largest 消息文字大小, in points; 14 is the default (§3).
pub const MESSAGE_SIZE_MIN: u8 = 12;
pub const MESSAGE_SIZE_MAX: u8 = 18;
pub const MESSAGE_SIZE_DEFAULT: u8 = 14;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalPrefs {
    /// 启动时: open the last 话题 (else 首页).
    pub launch_last_topic: bool,
    /// 登录时启动.
    pub launch_at_login: bool,
    /// 菜单栏里显示.
    pub menu_bar: bool,
    /// 声音.
    pub sound: bool,
    /// 消息文字大小 in points.
    pub message_size: u8,
    /// The slug of the pane shown last.
    pub last_pane: Option<String>,
}

impl Default for LocalPrefs {
    fn default() -> Self {
        Self {
            launch_last_topic: false,
            launch_at_login: false,
            menu_bar: true,
            sound: true,
            message_size: MESSAGE_SIZE_DEFAULT,
            last_pane: None,
        }
    }
}

static CACHE: Mutex<Option<LocalPrefs>> = Mutex::new(None);
static SAVING: AtomicBool = AtomicBool::new(true);

/// Self-tests and screenshot scenes: use the defaults and never write.
pub fn stop_saving() {
    SAVING.store(false, Ordering::SeqCst);
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some(LocalPrefs::default());
}

/// False in self-tests and screenshot scenes.
pub fn saving() -> bool {
    SAVING.load(Ordering::SeqCst)
}

fn read_file() -> LocalPrefs {
    std::fs::read(crate::views::columns::layout_file())
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("settings").cloned())
        .and_then(|v| serde_json::from_value::<LocalPrefs>(v).ok())
        .map(|mut p| {
            p.message_size = p.message_size.clamp(MESSAGE_SIZE_MIN, MESSAGE_SIZE_MAX);
            p
        })
        .unwrap_or_default()
}

/// The current settings.
pub fn get() -> LocalPrefs {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.get_or_insert_with(read_file).clone()
}

/// Changes the settings and saves them (unless a self-test runs).
pub fn update(f: impl FnOnce(&mut LocalPrefs)) {
    let next = {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let prefs = cache.get_or_insert_with(read_file);
        f(prefs);
        prefs.clone()
    };
    if SAVING.load(Ordering::SeqCst)
        && let Ok(value) = serde_json::to_value(&next)
    {
        crate::views::shell::layout::merge(&[("settings", value)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_read_as_defaults() {
        let p: LocalPrefs = serde_json::from_str(r#"{"sound":false}"#).unwrap();
        assert!(!p.sound);
        assert!(p.menu_bar);
        assert_eq!(p.message_size, MESSAGE_SIZE_DEFAULT);
        assert_eq!(p.last_pane, None);
    }
}
