//! What the views drew in the current screenshot scene, for the scene checks of the
//! self-test report (`screenshots.json`, `checks`). A view marks a kind of element with an
//! id while it renders it; a check reads the ids after the frames. Off outside the
//! screenshot self-test: `mark` is then one relaxed atomic load.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

static ON: AtomicBool = AtomicBool::new(false);
static DRAWN: Mutex<BTreeMap<&'static str, BTreeSet<String>>> = Mutex::new(BTreeMap::new());

/// Starts recording, empty (the runner calls it before each scene).
pub fn start() {
    DRAWN.lock().unwrap_or_else(|e| e.into_inner()).clear();
    ON.store(true, Ordering::Relaxed);
}

/// Records that a `kind` element with `id` was drawn.
pub fn mark(kind: &'static str, id: impl Into<String>) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    DRAWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(kind)
        .or_default()
        .insert(id.into());
}

/// The ids of the `kind` elements drawn since `start`.
pub fn ids(kind: &str) -> BTreeSet<String> {
    DRAWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(kind)
        .cloned()
        .unwrap_or_default()
}

/// `Ok(line)` when `ok`, else `Err(line)`: one check of a scene.
pub fn check(ok: bool, line: impl Into<String>) -> Result<String, String> {
    let line = line.into();
    if ok { Ok(line) } else { Err(line) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_only_while_recording() {
        mark("x", "before");
        start();
        mark("x", "a");
        mark("x", "a");
        mark("x", "b");
        assert_eq!(ids("x").len(), 2);
        assert!(ids("y").is_empty());
        start();
        assert!(ids("x").is_empty());
    }
}
