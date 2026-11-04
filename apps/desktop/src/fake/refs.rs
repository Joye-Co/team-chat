//! 引用提问 for the fake providers (inc-refs): what the server does with `send.blockRefs`
//! (REFS §A6): stores the references with their source's author and 话题, writes one quote
//! line per reference in front of the body, and marks references deleted when their source
//! message is deleted.

use std::{collections::HashMap, sync::Mutex};

use chat_core::proto::{BlockRef, BlockRefInput};

use crate::{data::Message, views::refs};

#[derive(Default)]
pub struct FakeRefs {
    /// Message id (or client id while queued) → its references.
    by_message: Mutex<HashMap<String, Vec<BlockRef>>>,
}

impl FakeRefs {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<BlockRef>>> {
        self.by_message.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The stored references of `inputs`: the source's author and 话题 come from `messages`
    /// (every message the fake has); a source it does not have keeps the sender's 话题.
    pub fn stored(
        inputs: &[BlockRefInput],
        messages: &[&Message],
        topic_id: &str,
    ) -> Vec<BlockRef> {
        inputs
            .iter()
            .map(|r| {
                let src = r
                    .message_id
                    .as_ref()
                    .and_then(|id| messages.iter().find(|m| &m.id == id));
                BlockRef {
                    source: r.clone(),
                    source_author_id: src.map(|m| m.author_id.clone()),
                    source_topic_id: src.map_or(topic_id.to_string(), |m| m.topic_id.clone()),
                    deleted: None,
                }
            })
            .collect()
    }

    /// Keeps the references of a sent message; returns the body the server would store.
    pub fn keep(&self, message_id: &str, refs: Vec<BlockRef>, body: &str) -> String {
        let out = refs::with_prefix(body, &refs);
        if !refs.is_empty() {
            self.lock().insert(message_id.to_string(), refs);
        }
        out
    }

    pub fn of(&self, message: &Message) -> Vec<BlockRef> {
        let map = self.lock();
        map.get(&message.id)
            .or_else(|| map.get(&message.client_msg_id))
            .cloned()
            .unwrap_or_default()
    }

    /// The source `deleted_id` is gone: every reference to it is marked deleted (label "",
    /// no value) and the quote lines of the referencing messages in `messages` are rewritten.
    pub fn source_deleted(&self, deleted_id: &str, messages: &mut [&mut Message]) {
        let mut map = self.lock();
        for (id, list) in map.iter_mut() {
            if !list
                .iter()
                .any(|r| r.source.message_id.as_deref() == Some(deleted_id))
            {
                continue;
            }
            let old_prefix = refs::prefix(list);
            for r in list.iter_mut() {
                if r.source.message_id.as_deref() == Some(deleted_id) {
                    r.source.label = String::new();
                    r.source.value = None;
                    r.deleted = Some(chat_core::proto::True);
                }
            }
            let new_prefix = refs::prefix(list);
            if let Some(m) = messages.iter_mut().find(|m| &m.id == id)
                && let Some(rest) = m.body.strip_prefix(&old_prefix)
            {
                m.body = format!("{new_prefix}{rest}");
            }
        }
    }
}
