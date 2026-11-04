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

use crate::data::{Draft, DraftKey, DraftRef, SavedNavigation};

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
    /// inc-refs (1.1.0): the reference chips (引用提问). Additive: absent in a 1.0.x file,
    /// skipped when empty so a draft without chips writes the same bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<SavedRef>,
}

/// A saved reference chip ([`DraftRef`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedRef {
    pub input: chat_core::proto::BlockRefInput,
    pub group_id: String,
    pub topic_id: String,
    #[serde(default)]
    pub lead_chars: usize,
}

impl SavedRef {
    pub fn to_draft_ref(&self) -> Option<DraftRef> {
        // A file edited by hand must not put an invalid ref into the next send.
        self.input.validate().ok()?;
        Some(DraftRef {
            input: self.input.clone(),
            group_id: self.group_id.clone(),
            topic_id: self.topic_id.clone(),
            lead_chars: self.lead_chars,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedNav {
    #[serde(default)]
    pub todo: bool,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub topic_id: Option<String>,
    /// v2 (1.1.0): additive, skipped when empty so a 1.0.x file stays the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
}

impl From<&SavedNavigation> for SavedNav {
    fn from(n: &SavedNavigation) -> Self {
        SavedNav {
            todo: n.todo,
            group_id: n.group_id.clone(),
            topic_id: n.topic_id.clone(),
            page: n.page.clone(),
            member_id: n.member_id.clone(),
            artifact_id: n.artifact_id.clone(),
        }
    }
}

impl From<SavedNav> for SavedNavigation {
    fn from(n: SavedNav) -> Self {
        SavedNavigation {
            todo: n.todo,
            group_id: n.group_id,
            topic_id: n.topic_id,
            page: n.page,
            member_id: n.member_id,
            artifact_id: n.artifact_id,
        }
    }
}

impl SavedDraft {
    /// The text part of a draft, or `None` when nothing worth keeping is left.
    pub fn from_draft(key: &DraftKey, draft: &Draft) -> Option<SavedDraft> {
        if draft.body.trim().is_empty() && draft.quote_of.is_none() && draft.refs.is_empty() {
            return None;
        }
        Some(SavedDraft {
            group_id: key.group_id.clone(),
            topic_id: key.topic_id.clone(),
            body: draft.body.chars().take(MAX_BODY_CHARS).collect(),
            mentions: draft.mentions.clone(),
            quote_of: draft.quote_of.clone(),
            refs: draft
                .refs
                .iter()
                .map(|r| SavedRef {
                    input: r.input.clone(),
                    group_id: r.group_id.clone(),
                    topic_id: r.topic_id.clone(),
                    lead_chars: r.lead_chars,
                })
                .collect(),
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
            refs: Vec::new(),
        };
        let saved = SavedDraft::from_draft(&key, &draft).unwrap();
        assert!(SavedDraft::from_draft(&DraftKey::new_topic("g"), &Draft::default()).is_none());
        let nav = SavedNavigation {
            todo: false,
            group_id: Some("g".into()),
            topic_id: Some("t".into()),
            page: Some("members".into()),
            member_id: Some("m".into()),
            artifact_id: None,
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

    // --- inc-refs ---
    #[test]
    fn chips_survive_a_restart_and_old_files_still_read() {
        let key = DraftKey::topic("g", "t");
        let chip = DraftRef {
            input: chat_core::proto::BlockRefInput {
                message_id: Some("m-1".into()),
                card_rev: Some(2),
                block_id: "channels".into(),
                part: Some(chat_core::proto::PartRef {
                    kind: "cell".into(),
                    keys: Some(
                        [
                            (
                                "row".to_string(),
                                chat_core::proto::PartKey::Key("bili".into()),
                            ),
                            ("col".to_string(), chat_core::proto::PartKey::Index(1)),
                        ]
                        .into(),
                    ),
                }),
                label: "10 月数据月报 · 渠道表 · B 站 · 新增群 9".into(),
                value: Some(serde_json::json!({"row": "B 站", "display": "9", "raw": 9})),
                ..Default::default()
            },
            group_id: "g".into(),
            topic_id: "t".into(),
            lead_chars: 15,
        };
        let draft = Draft {
            refs: vec![chip.clone()],
            ..Draft::default()
        };
        // A draft with only a chip is kept.
        let saved = SavedDraft::from_draft(&key, &draft).unwrap();
        let bytes = serde_json::to_vec(&UiState::new(vec![saved], None)).unwrap();
        let back: UiState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.drafts[0].refs[0].to_draft_ref(), Some(chip));
        // A 1.0.x draft has no refs field and reads with none; writing it adds none.
        let old = br#"{"group_id":"g","topic_id":"t","body":"hi","mentions":[],"quote_of":null}"#;
        let d: SavedDraft = serde_json::from_slice(old).unwrap();
        assert!(d.refs.is_empty());
        assert!(
            !String::from_utf8(serde_json::to_vec(&d).unwrap())
                .unwrap()
                .contains("refs")
        );
        // An invalid saved ref is dropped.
        let mut bad = back.drafts[0].refs[0].clone();
        bad.input.block_id = "md:x".into();
        assert_eq!(bad.to_draft_ref(), None);
    }
    // --- end inc-refs ---

    #[test]
    fn a_1_0_navigation_reads_and_writes_the_same_bytes() {
        let old = br#"{"todo":false,"group_id":"g","topic_id":"t"}"#;
        let nav: SavedNav = serde_json::from_slice(old).unwrap();
        assert_eq!(nav.page, None);
        assert_eq!(serde_json::to_vec(&nav).unwrap(), old.to_vec());
    }
}
