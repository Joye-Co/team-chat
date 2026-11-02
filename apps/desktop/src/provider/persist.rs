//! Local UI state of one account on this Mac: composer drafts (text, picked
//! mentions, quote) and the place of the main window. It lives in
//! `ui-state.json` in the account folder, so it survives restarts and is
//! deleted with the cache. Pending files of a draft are not kept.
//!
//! Writes go to one writer thread: the UI thread never touches the disk, and
//! a burst of changes (typing) writes the newest state once.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{Sender, channel},
};

use serde::{Deserialize, Serialize};

use crate::data::{Draft, DraftKey, SavedNavigation};

const FILE: &str = "ui-state.json";
/// Most drafts kept; the rest are dropped (newest edits win in practice).
const MAX_DRAFTS: usize = 200;
/// Longest draft body kept, in characters.
const MAX_BODY_CHARS: usize = 100_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UiState {
    #[serde(default)]
    pub drafts: Vec<SavedDraft>,
    #[serde(default)]
    pub navigation: Option<SavedNav>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedDraft {
    pub group_id: String,
    #[serde(default)]
    pub topic_id: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub mentions: Vec<String>,
    #[serde(default)]
    pub quote_of: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedNav {
    #[serde(default)]
    pub todo: bool,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub topic_id: Option<String>,
}

impl From<&SavedNavigation> for SavedNav {
    fn from(n: &SavedNavigation) -> Self {
        SavedNav {
            todo: n.todo,
            group_id: n.group_id.clone(),
            topic_id: n.topic_id.clone(),
        }
    }
}

impl From<SavedNav> for SavedNavigation {
    fn from(n: SavedNav) -> Self {
        SavedNavigation {
            todo: n.todo,
            group_id: n.group_id,
            topic_id: n.topic_id,
        }
    }
}

impl SavedDraft {
    /// The text part of a draft, or `None` when nothing worth keeping is left.
    pub fn from_draft(key: &DraftKey, draft: &Draft) -> Option<SavedDraft> {
        if draft.body.trim().is_empty() && draft.quote_of.is_none() {
            return None;
        }
        Some(SavedDraft {
            group_id: key.group_id.clone(),
            topic_id: key.topic_id.clone(),
            body: draft.body.chars().take(MAX_BODY_CHARS).collect(),
            mentions: draft.mentions.clone(),
            quote_of: draft.quote_of.clone(),
        })
    }

    pub fn key(&self) -> DraftKey {
        DraftKey {
            group_id: self.group_id.clone(),
            topic_id: self.topic_id.clone(),
        }
    }
}

impl UiState {
    pub fn new(mut drafts: Vec<SavedDraft>, navigation: Option<&SavedNavigation>) -> Self {
        // Stable order, so an unchanged state writes the same bytes.
        drafts.sort_by(|a, b| (&a.group_id, &a.topic_id).cmp(&(&b.group_id, &b.topic_id)));
        drafts.truncate(MAX_DRAFTS);
        UiState {
            drafts,
            navigation: navigation.map(SavedNav::from),
        }
    }
}

/// Reads the state of an account folder. A missing or broken file gives the
/// empty state (a broken file is logged and later replaced).
pub fn load(dir: &Path) -> UiState {
    let path = dir.join(FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "ui state file is not valid; starting empty");
            UiState::default()
        }),
        Err(_) => UiState::default(),
    }
}

fn write(dir: &Path, state: &UiState) -> std::io::Result<()> {
    if !dir.is_dir() {
        // The account folder was wiped (sign-out): keep nothing.
        return Ok(());
    }
    let path = dir.join(FILE);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(state)?)?;
    std::fs::rename(tmp, path)
}

/// The writer thread. Cheap to clone.
#[derive(Clone)]
pub struct Writer {
    tx: Sender<(PathBuf, UiState)>,
}

impl Writer {
    pub fn new() -> Self {
        let (tx, rx) = channel::<(PathBuf, UiState)>();
        let spawned = std::thread::Builder::new()
            .name("ui-state-writer".into())
            .spawn(move || {
                let mut last: HashMap<PathBuf, UiState> = HashMap::new();
                while let Ok(first) = rx.recv() {
                    let mut newest: HashMap<PathBuf, UiState> = HashMap::new();
                    newest.insert(first.0, first.1);
                    while let Ok((dir, state)) = rx.try_recv() {
                        newest.insert(dir, state);
                    }
                    for (dir, state) in newest {
                        if last.get(&dir) == Some(&state) {
                            continue;
                        }
                        match write(&dir, &state) {
                            Ok(()) => {
                                last.insert(dir, state);
                            }
                            Err(e) => tracing::warn!(error = %e, "cannot write the ui state"),
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "cannot start the ui state writer");
        }
        Writer { tx }
    }

    /// Queues the newest state of an account folder.
    pub fn save(&self, dir: &Path, state: UiState) {
        let _ = self.tx.send((dir.to_path_buf(), state));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_skips_empty_drafts() {
        let dir = std::env::temp_dir().join(format!("ui-state-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = DraftKey::topic("g", "t");
        let draft = Draft {
            body: "你好".into(),
            mentions: vec!["m".into()],
            quote_of: Some("q".into()),
            files: Vec::new(),
        };
        let saved = SavedDraft::from_draft(&key, &draft).unwrap();
        assert!(SavedDraft::from_draft(&DraftKey::new_topic("g"), &Draft::default()).is_none());
        let nav = SavedNavigation {
            todo: false,
            group_id: Some("g".into()),
            topic_id: Some("t".into()),
        };
        let state = UiState::new(vec![saved.clone()], Some(&nav));
        write(&dir, &state).unwrap();
        let back = load(&dir);
        assert_eq!(back, state);
        assert_eq!(back.drafts[0].key(), key);
        assert_eq!(SavedNavigation::from(back.navigation.unwrap()), nav);
        // A broken file reads as empty.
        std::fs::write(dir.join(FILE), b"{oops").unwrap();
        assert_eq!(load(&dir), UiState::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
