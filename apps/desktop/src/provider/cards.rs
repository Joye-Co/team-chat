//! Cards for [`super::CoreProvider`]: an in-memory cache of card messages,
//! their presses, my press state and card images.
//!
//! Views read cards on every frame, so nothing here waits for the network
//! and repeated reads hit memory, not SQLite:
//! - Cards are refreshed from the messages the timeline reads
//!   ([`CardCache::remember`]). The `Arc` of a card stays the same while the
//!   card does not change.
//! - Presses come from the store's `card_actions` table once per message and
//!   are dropped from the cache when a `card.actioned` event arrives.
//! - Action labels of every card version seen are kept, so a press line can
//!   name a button the bot removed after the press.
//! - My press is an outbox row (`card_action`). While the row is queued the
//!   press shows as sending; a refusal is kept in memory and the row is
//!   removed, so failed presses do not pile up in the outbox.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use chat_core::{
    Client, OutboxState,
    proto::{self, ErrorCode},
};

use crate::data::{CardPress, CardPressError, CardPressPhase, CardPressState, card::Card};

/// Entries kept before a cache is cleared (it refills from the store).
const MAX_ENTRIES: usize = 5000;
/// Largest card image downloaded for the preview.
const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Default)]
pub struct CardCache {
    inner: Mutex<Inner>,
    /// File id -> image state; shared with download tasks.
    images: Arc<Mutex<HashMap<String, Image>>>,
}

#[derive(Default)]
struct Inner {
    /// Message id -> its card (`None`: not a card message).
    cards: HashMap<String, Option<Arc<Card>>>,
    /// Message id -> action id -> label, from every card version seen.
    labels: HashMap<String, HashMap<String, String>>,
    presses: HashMap<String, Arc<Vec<CardPress>>>,
    /// Message id -> my latest press.
    mine: HashMap<String, MyPress>,
}

struct MyPress {
    action_id: String,
    outbox_id: String,
    failed: Option<CardPressError>,
}

enum Image {
    Loading,
    Ready(PathBuf),
    Failed,
}

fn bound<K, V>(map: &mut HashMap<K, V>) {
    if map.len() >= MAX_ENTRIES {
        map.clear();
    }
}

impl Inner {
    fn put_card(&mut self, message_id: &str, card: Option<&Card>) -> Option<Arc<Card>> {
        let card = card.map(|c| {
            if let Some(actions) = &c.actions {
                let labels = self.labels.entry(message_id.to_string()).or_default();
                for a in actions {
                    labels.insert(a.id.clone(), a.label.clone());
                }
            }
            match self.cards.get(message_id) {
                Some(Some(old)) if **old == *c => old.clone(),
                _ => Arc::new(c.clone()),
            }
        });
        bound(&mut self.cards);
        bound(&mut self.labels);
        self.cards.insert(message_id.to_string(), card.clone());
        card
    }
}

fn card_of(m: &proto::Message) -> Option<&Card> {
    if m.deleted_at().is_some() {
        None
    } else {
        m.card.as_ref()
    }
}

fn error_of(code: &str) -> CardPressError {
    match code {
        "not_found" | "invalid" => CardPressError::Gone,
        "forbidden" => CardPressError::Forbidden,
        _ => CardPressError::Other,
    }
}

impl CardCache {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        lock(&self.inner)
    }

    /// Forgets everything (sign-in or sign-out).
    pub fn clear(&self) {
        *self.lock() = Inner::default();
        lock(&self.images).clear();
    }

    /// Refreshes the cards of messages just read from the store.
    pub fn remember(&self, messages: &[proto::Message]) {
        let mut inner = self.lock();
        for m in messages.iter().filter(|m| !m.id.is_empty()) {
            inner.put_card(&m.id, card_of(m));
        }
    }

    pub fn card(&self, client: &Client, message_id: &str) -> Option<Arc<Card>> {
        if let Some(card) = self.lock().cards.get(message_id) {
            return card.clone();
        }
        let message = client.store().message(message_id).ok().flatten();
        self.lock()
            .put_card(message_id, message.as_ref().and_then(card_of))
    }

    pub fn presses(&self, client: &Client, message_id: &str) -> Vec<CardPress> {
        if let Some(p) = self.lock().presses.get(message_id) {
            return p.as_ref().clone();
        }
        let rows = client.card_actions(message_id).unwrap_or_default();
        let mut inner = self.lock();
        let labels = inner.labels.get(message_id);
        let mut presses: Vec<CardPress> = rows
            .into_iter()
            .map(|r| CardPress {
                label: labels.and_then(|l| l.get(&r.action_id)).cloned(),
                actor_id: r.actor_id,
                action_id: r.action_id,
                at: r.at.min(i64::MAX as u64) as i64,
            })
            .collect();
        presses.sort_by_key(|p| p.at);
        bound(&mut inner.presses);
        inner
            .presses
            .insert(message_id.to_string(), Arc::new(presses.clone()));
        presses
    }

    /// A `card.actioned` event for this message arrived. Returns true when it
    /// was my press, which is then answered.
    pub fn on_actioned(&self, message_id: &str, actor_is_me: bool, action_id: &str) -> bool {
        let mut inner = self.lock();
        inner.presses.remove(message_id);
        if actor_is_me
            && inner
                .mine
                .get(message_id)
                .is_some_and(|p| p.action_id == action_id && p.failed.is_none())
        {
            inner.mine.remove(message_id);
            return true;
        }
        false
    }

    pub fn press_state(&self, client: &Client, message_id: &str) -> Option<CardPressState> {
        let (action_id, outbox_id, failed) = {
            let inner = self.lock();
            let p = inner.mine.get(message_id)?;
            (p.action_id.clone(), p.outbox_id.clone(), p.failed)
        };
        if let Some(error) = failed {
            return Some(CardPressState {
                action_id,
                phase: CardPressPhase::Failed(error),
            });
        }
        match client.store().outbox_get(&outbox_id).ok().flatten() {
            Some(item) if item.state == OutboxState::Failed => {
                let error = error_of(item.last_error.as_deref().unwrap_or(""));
                Some(CardPressState {
                    action_id,
                    phase: CardPressPhase::Failed(error),
                })
            }
            Some(_) => Some(CardPressState {
                action_id,
                phase: CardPressPhase::Sending,
            }),
            None => {
                // Acked: the press went through.
                let mut inner = self.lock();
                if inner
                    .mine
                    .get(message_id)
                    .is_some_and(|p| p.outbox_id == outbox_id)
                {
                    inner.mine.remove(message_id);
                }
                None
            }
        }
    }

    /// Queues a press. A press of this card that is still queued wins; a
    /// refused one is replaced.
    pub fn press(&self, client: &Client, message_id: &str, action_id: &str) {
        if let Some(state) = self.press_state(client, message_id)
            && state.phase == CardPressPhase::Sending
        {
            return;
        }
        match client.card_action(message_id, action_id) {
            Ok(outbox_id) => {
                let mut inner = self.lock();
                bound(&mut inner.mine);
                inner.mine.insert(
                    message_id.to_string(),
                    MyPress {
                        action_id: action_id.to_string(),
                        outbox_id,
                        failed: None,
                    },
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "card press refused");
                self.lock().mine.insert(
                    message_id.to_string(),
                    MyPress {
                        action_id: action_id.to_string(),
                        outbox_id: String::new(),
                        failed: Some(CardPressError::Gone),
                    },
                );
            }
        }
    }

    /// The server refused the outbox row `outbox_id`. Returns true when it
    /// was a card press (the row is then removed and the error kept here).
    pub fn on_refused(&self, client: &Client, outbox_id: &str, code: ErrorCode) -> bool {
        let found = {
            let mut inner = self.lock();
            match inner.mine.values_mut().find(|p| p.outbox_id == outbox_id) {
                Some(p) => {
                    p.failed = Some(match code {
                        ErrorCode::NotFound | ErrorCode::Invalid => CardPressError::Gone,
                        ErrorCode::Forbidden => CardPressError::Forbidden,
                        _ => CardPressError::Other,
                    });
                    true
                }
                None => false,
            }
        };
        if found && let Err(e) = client.discard(outbox_id) {
            tracing::warn!(error = %e, "cannot drop a refused card press");
        }
        found
    }

    /// The local copy of a card image, or `None` while it downloads. Starts
    /// the download once; `done` runs on a core thread when it finishes.
    pub fn image(
        &self,
        client: &Arc<Client>,
        file_id: &str,
        done: impl FnOnce() + Send + 'static,
    ) -> Option<PathBuf> {
        {
            let mut images = lock(&self.images);
            match images.get(file_id) {
                Some(Image::Ready(path)) => return Some(path.clone()),
                Some(Image::Loading | Image::Failed) => return None,
                None => {}
            }
            bound(&mut images);
            images.insert(file_id.to_string(), Image::Loading);
        }
        let (c, id, images) = (client.clone(), file_id.to_string(), self.images.clone());
        client.spawn(async move {
            let image = match load_image(&c, &id).await {
                Ok(path) => Image::Ready(path),
                Err(reason) => {
                    tracing::info!(reason, "card image not loaded");
                    Image::Failed
                }
            };
            lock(&images).insert(id, image);
            done();
        });
        None
    }
}

/// Fetches the file record, checks that it is a ready image of a sane size,
/// and downloads it into the file cache (or finds it there).
async fn load_image(client: &Client, file_id: &str) -> Result<PathBuf, &'static str> {
    let meta = client
        .http()
        .file_meta(file_id)
        .await
        .map_err(|_| "file record")?;
    if meta.status != proto::FileStatus::Ready
        || !meta.mime.starts_with("image/")
        || meta.size > MAX_IMAGE_BYTES
    {
        return Err("not a ready image");
    }
    let file = meta.to_file_ref();
    if let Some(path) = client.cached_file(&file) {
        return Ok(path);
    }
    client
        .download_file(file, chat_core::no_progress())
        .await
        .map_err(|_| "download task")?
        .map_err(|_| "download")
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusal_codes_map_to_errors() {
        assert_eq!(error_of("not_found"), CardPressError::Gone);
        assert_eq!(error_of("forbidden"), CardPressError::Forbidden);
        assert_eq!(error_of("internal"), CardPressError::Other);
    }

    #[test]
    fn unchanged_card_keeps_its_arc_and_labels_survive_edits() {
        let mut inner = Inner::default();
        let card: Card = serde_json::from_value(serde_json::json!({
            "v": 1, "title": "t", "blocks": [],
            "actions": [{ "id": "ok", "label": "好" }]
        }))
        .unwrap();
        let a = inner.put_card("m", Some(&card)).unwrap();
        let b = inner.put_card("m", Some(&card)).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let mut edited = card.clone();
        edited.actions = None;
        let c = inner.put_card("m", Some(&edited)).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(inner.labels["m"]["ok"], "好");
        assert!(inner.put_card("m", None).is_none());
    }

    /// Waits up to 30 s for `f` to hold.
    fn wait(what: &str, mut f: impl FnMut() -> bool) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !f() {
            assert!(std::time::Instant::now() < end, "timed out waiting for {what}");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// The whole card path against a real server, with
    /// `src/views/cards/live_bot.ts` running as the card's author:
    /// `CARDS_LIVE_URL=http://127.0.0.1:8803 CARDS_LIVE_TOKEN_FILE=<member token file>
    /// CARDS_LIVE_INFO=<file the bot wrote> cargo test -p desktop live_cards -- --ignored`
    #[test]
    #[ignore = "needs a server and live_bot.ts"]
    fn live_cards() {
        use crate::{
            data::{
                ChatProvider, Credential, LoginRequest, SessionState,
                card::{CardBlock, CardTone},
            },
            provider::{CoreProvider, ProviderConfig, session::TokenStorage},
        };
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
        let url = env("CARDS_LIVE_URL");
        let token = std::fs::read_to_string(env("CARDS_LIVE_TOKEN_FILE")).expect("token file");
        let info: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(env("CARDS_LIVE_INFO")).unwrap()).unwrap();
        let (g, t, m) = (
            info["groupId"].as_str().unwrap(),
            info["topicId"].as_str().unwrap(),
            info["messageId"].as_str().unwrap(),
        );
        let dir = std::env::temp_dir().join(format!("cards-live-{}", std::process::id()));
        let p = CoreProvider::new(ProviderConfig {
            data_root: dir.clone(),
            tokens: TokenStorage::memory(),
            restore: false,
        });
        p.login(LoginRequest {
            server_url: url,
            credential: Credential::Token(token.trim().to_string()),
            device_name: "cards-live".into(),
        });
        wait("sign-in", || matches!(p.session(), SessionState::SignedIn { .. }));
        let SessionState::SignedIn { me, .. } = p.session() else {
            unreachable!()
        };

        // The card arrives and every block type parses.
        wait("the card", || {
            let _ = p.messages(g, t, 50);
            p.card(g, m).is_some()
        });
        let card = p.card(g, m).unwrap();
        assert_eq!(card.actions.as_ref().map(Vec::len), Some(3));
        assert_eq!(card.blocks.len(), 9);
        assert!(!card.blocks.iter().any(|b| matches!(b, CardBlock::Unknown)));
        assert!(Arc::ptr_eq(&card, &p.card(g, m).unwrap()), "same Arc while unchanged");

        // The image block downloads into the file cache.
        let file_id = card
            .blocks
            .iter()
            .find_map(|b| match b {
                CardBlock::Image { file_id, .. } => Some(file_id.clone()),
                _ => None,
            })
            .unwrap();
        wait("the card image", || {
            p.card_image(g, t, &file_id).is_some_and(|path| path.is_file())
        });

        // A press shows as sending, then the bot's edit replaces the card.
        p.press_card_action(g, t, m, "approve");
        assert_eq!(
            p.card_press_state(g, m).map(|s| s.phase),
            Some(CardPressPhase::Sending)
        );
        wait("the edited card", || {
            let _ = p.messages(g, t, 50);
            p.card(g, m).is_some_and(|c| c.actions.is_none())
                && p.card_press_state(g, m).is_none()
                && p.card_presses(g, m).len() == 1
        });
        let edited = p.card(g, m).unwrap();
        assert_eq!(edited.tone, Some(CardTone::Success));
        assert_eq!(edited.status.as_ref().unwrap().label, "已处理");
        let press = &p.card_presses(g, m)[0];
        assert_eq!(press.actor_id, me.id);
        assert_eq!(press.action_id, "approve");
        assert_eq!(press.label.as_deref(), Some("批准"));

        // The button is gone now: the server refuses, the UI says so, and
        // the refused row does not stay in the outbox.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        p.press_card_action(g, t, m, "approve");
        wait("the refusal", || {
            p.card_press_state(g, m).map(|s| s.phase)
                == Some(CardPressPhase::Failed(CardPressError::Gone))
        });
        let client = p.client().unwrap();
        wait("the outbox to drop the refused press", || {
            client
                .store()
                .outbox_all()
                .unwrap()
                .iter()
                .all(|i| i.op.kind() != "card_action")
        });
        p.logout();
        let _ = std::fs::remove_dir_all(dir);
    }
}
