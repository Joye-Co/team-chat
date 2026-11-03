//! An in-memory store of composer drafts that a provider can embed.
//!
//! It keeps the text, picked mentions, quote and pending files of each
//! composer ([`DraftKey`]). It does no file I/O: a path is kept as it is, and
//! its size is read later by the upload.

use std::{collections::HashMap, sync::Mutex};

use crate::{
    copy::{composer as c, fill},
    data::{
        AddRef, Draft, DraftKey, DraftRef, MAX_DRAFT_REFS, MemberId, MessageId, NewFile,
        PendingFile,
    },
};

/// Most files one draft holds; more are ignored.
pub const MAX_PENDING_FILES: usize = 20;

#[derive(Default)]
pub struct DraftStore {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    drafts: HashMap<DraftKey, Draft>,
    next_id: u64,
}

impl DraftStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut inner)
    }

    pub fn get(&self, key: &DraftKey) -> Draft {
        self.with(|i| i.drafts.get(key).cloned().unwrap_or_default())
    }

    pub fn save_text(&self, key: &DraftKey, body: String, mentions: Vec<MemberId>) {
        self.with(|i| {
            let draft = i.drafts.entry(key.clone()).or_default();
            draft.body = body;
            draft.mentions = mentions;
            i.prune(key);
        })
    }

    /// Returns true when the quote changed.
    pub fn set_quote(&self, key: &DraftKey, quote_of: Option<MessageId>) -> bool {
        self.with(|i| {
            let draft = i.drafts.entry(key.clone()).or_default();
            let changed = draft.quote_of != quote_of;
            draft.quote_of = quote_of;
            i.prune(key);
            changed
        })
    }

    /// Adds files, skipping a path that is already pending. Returns true when
    /// at least one file was added.
    pub fn attach(&self, key: &DraftKey, files: Vec<NewFile>) -> bool {
        self.with(|i| {
            let mut added = false;
            for file in files {
                let draft = i.drafts.entry(key.clone()).or_default();
                if draft.files.len() >= MAX_PENDING_FILES {
                    break;
                }
                if matches!(&file, NewFile::Path(_)) && draft.files.iter().any(|f| f.source == file)
                {
                    continue;
                }
                let images = draft.files.iter().filter(|f| f.is_image).count();
                let pending = pending_file(i.next_id, images, file);
                i.next_id += 1;
                i.drafts.entry(key.clone()).or_default().files.push(pending);
                added = true;
            }
            i.prune(key);
            added
        })
    }

    /// Returns true when the file was pending.
    pub fn remove_file(&self, key: &DraftKey, file_id: &str) -> bool {
        self.with(|i| {
            let Some(draft) = i.drafts.get_mut(key) else {
                return false;
            };
            let before = draft.files.len();
            draft.files.retain(|f| f.id != file_id);
            let changed = draft.files.len() != before;
            i.prune(key);
            changed
        })
    }

    // --- inc-refs: reference chips (REFS §A4) ---

    /// Adds a chip unless the same address is there (the index is returned) or 5 are.
    pub fn add_ref(&self, key: &DraftKey, r: DraftRef) -> AddRef {
        self.with(|i| {
            let draft = i.drafts.entry(key.clone()).or_default();
            if let Some(at) = draft.refs.iter().position(|d| d.same_address(&r.input)) {
                return AddRef::Duplicate(at);
            }
            if draft.refs.len() >= MAX_DRAFT_REFS {
                return AddRef::Full;
            }
            draft.refs.push(r);
            AddRef::Added
        })
    }

    /// Removes the chip at `index`. Returns true when there was one.
    pub fn remove_ref(&self, key: &DraftKey, index: usize) -> bool {
        self.with(|i| {
            let Some(draft) = i.drafts.get_mut(key) else {
                return false;
            };
            let removed = index < draft.refs.len();
            if removed {
                draft.refs.remove(index);
            }
            i.prune(key);
            removed
        })
    }

    /// Puts back the chips of a saved draft (restart).
    pub fn set_refs(&self, key: &DraftKey, refs: Vec<DraftRef>) {
        self.with(|i| {
            let draft = i.drafts.entry(key.clone()).or_default();
            draft.refs = refs.into_iter().take(MAX_DRAFT_REFS).collect();
            i.prune(key);
        })
    }
    // --- end inc-refs ---

    /// Returns true when there was a draft.
    pub fn clear(&self, key: &DraftKey) -> bool {
        self.with(|i| i.drafts.remove(key).is_some())
    }

    /// Every draft (for persistence), in no order.
    pub fn entries(&self) -> Vec<(DraftKey, Draft)> {
        self.with(|i| {
            i.drafts
                .iter()
                .map(|(k, d)| (k.clone(), d.clone()))
                .collect()
        })
    }

    /// Forgets every draft (sign-out, another account).
    pub fn clear_all(&self) {
        self.with(|i| i.drafts.clear())
    }
}

impl Inner {
    /// Drops an empty draft so the map does not grow with every topic visited.
    fn prune(&mut self, key: &DraftKey) {
        if self.drafts.get(key).is_some_and(|d| *d == Draft::default()) {
            self.drafts.remove(key);
        }
    }
}

fn pending_file(id: u64, images_before: usize, source: NewFile) -> PendingFile {
    let id = format!("pf-{id}");
    match &source {
        NewFile::Path(path) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let is_image = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "bmp" | "tif" | "tiff"
                )
            });
            PendingFile {
                id,
                name,
                is_image,
                size_bytes: None,
                source,
            }
        }
        NewFile::Image {
            extension, bytes, ..
        } => PendingFile {
            id,
            name: format!(
                "{}.{extension}",
                fill(c::PASTED_IMAGE, &[("n", &(images_before + 1).to_string())])
            ),
            is_image: true,
            size_bytes: Some(bytes.len() as u64),
            source,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, sync::Arc};

    #[test]
    fn drafts_are_per_key_and_pruned() {
        let store = DraftStore::new();
        let a = DraftKey::topic("g", "t1");
        let b = DraftKey::topic("g", "t2");
        let n = DraftKey::new_topic("g");
        store.save_text(&a, "你好".into(), vec!["m-1".into()]);
        store.save_text(&n, "新话题".into(), vec![]);
        assert_eq!(store.get(&a).body, "你好");
        assert_eq!(store.get(&a).mentions, vec!["m-1".to_string()]);
        assert_eq!(store.get(&b), Draft::default());
        assert_eq!(store.get(&n).body, "新话题");
        store.save_text(&a, String::new(), vec![]);
        assert!(!store.inner.lock().unwrap().drafts.contains_key(&a));
    }

    #[test]
    fn quote_and_files() {
        let store = DraftStore::new();
        let k = DraftKey::topic("g", "t");
        assert!(store.set_quote(&k, Some("msg-1".into())));
        assert!(!store.set_quote(&k, Some("msg-1".into())));
        let path = NewFile::Path(PathBuf::from("/tmp/设计稿.PNG"));
        let image = NewFile::Image {
            mime: "image/png".into(),
            extension: "png".into(),
            bytes: Arc::from(vec![1u8, 2, 3]),
        };
        assert!(store.attach(&k, vec![path.clone(), image]));
        assert!(!store.attach(&k, vec![path]), "same path twice");
        let draft = store.get(&k);
        assert_eq!(draft.quote_of.as_deref(), Some("msg-1"));
        assert_eq!(draft.files.len(), 2);
        assert_eq!(draft.files[0].name, "设计稿.PNG");
        assert!(draft.files[0].is_image);
        assert_eq!(draft.files[1].size_bytes, Some(3));
        assert!(draft.files[1].name.ends_with(".png"));
        let id = draft.files[0].id.clone();
        assert!(store.remove_file(&k, &id));
        assert!(!store.remove_file(&k, &id));
        assert_eq!(store.get(&k).files.len(), 1);
        assert!(store.clear(&k));
        assert_eq!(store.get(&k), Draft::default());
    }

    // --- inc-refs ---
    fn chip(block: &str) -> DraftRef {
        DraftRef {
            input: chat_core::proto::BlockRefInput {
                message_id: Some("m-1".into()),
                block_id: block.into(),
                label: format!("卡 · {block}"),
                ..Default::default()
            },
            group_id: "g".into(),
            topic_id: "t".into(),
            lead_chars: 4,
        }
    }

    #[test]
    fn chips_dedupe_cap_at_five_and_survive_text_changes() {
        let store = DraftStore::new();
        let k = DraftKey::topic("g", "t");
        assert_eq!(store.add_ref(&k, chip("a")), AddRef::Added);
        assert_eq!(store.add_ref(&k, chip("a")), AddRef::Duplicate(0));
        for b in ["b", "c", "d", "e"] {
            assert_eq!(store.add_ref(&k, chip(b)), AddRef::Added);
        }
        assert_eq!(store.add_ref(&k, chip("f")), AddRef::Full);
        assert_eq!(store.add_ref(&k, chip("c")), AddRef::Duplicate(2));
        store.save_text(&k, String::new(), vec![]);
        assert_eq!(store.get(&k).refs.len(), 5, "an empty text keeps the chips");
        assert!(store.remove_ref(&k, 4));
        assert!(!store.remove_ref(&k, 4));
        let left: Vec<_> = store
            .get(&k)
            .refs
            .iter()
            .map(|r| r.input.block_id.clone())
            .collect();
        assert_eq!(left, ["a", "b", "c", "d"]);
        for _ in 0..4 {
            store.remove_ref(&k, 0);
        }
        assert!(
            !store.inner.lock().unwrap().drafts.contains_key(&k),
            "pruned"
        );
    }
    // --- end inc-refs ---

    #[test]
    fn file_count_is_limited() {
        let store = DraftStore::new();
        let k = DraftKey::new_topic("g");
        let files = (0..MAX_PENDING_FILES + 5)
            .map(|i| NewFile::Path(PathBuf::from(format!("/tmp/f{i}.txt"))))
            .collect();
        store.attach(&k, files);
        assert_eq!(store.get(&k).files.len(), MAX_PENDING_FILES);
    }
}
