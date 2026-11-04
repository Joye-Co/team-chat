//! 引用提问 for [`super::CoreProvider`] (the inc-refs track owns this file): the references of
//! messages the timeline shows, the chips of drafts, and sends with `blockRefs`.
//!
//! Rows read references on every frame, so they come from memory: [`RefsCache::remember`]
//! keeps them from the messages the timeline reads; a queued send of mine reads them from
//! its outbox row.

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use chat_core::{
    OutboxOp,
    proto::{self, BlockRef, BlockRefInput},
};

use super::{CoreProvider, convert::pending_cid};
use crate::data::{
    AddRef, Change, ClientMsgId, DraftKey, DraftRef, Message, PendingFileId, SendRequest,
};

const MAX_ENTRIES: usize = 5000;

#[derive(Default)]
pub struct RefsCache {
    inner: Mutex<HashMap<String, Vec<BlockRef>>>,
}

impl RefsCache {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Vec<BlockRef>>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keeps the references of the messages the timeline read (also an empty list, so a
    /// message without references never reads the store again).
    pub fn remember(&self, messages: &[proto::Message]) {
        let mut map = self.lock();
        if map.len() >= MAX_ENTRIES {
            map.clear();
        }
        for m in messages.iter().filter(|m| !m.id.is_empty()) {
            map.insert(m.id.clone(), m.block_refs.clone().unwrap_or_default());
        }
    }
}

/// The references a queued send carries, as the server will store them (the source topic is
/// the message's own until the ack fills it).
fn queued_refs(refs: &[BlockRefInput], topic_id: &str) -> Vec<BlockRef> {
    refs.iter()
        .map(|r| BlockRef {
            source: r.clone(),
            source_author_id: None,
            source_topic_id: topic_id.to_string(),
            deleted: None,
        })
        .collect()
}

impl CoreProvider {
    pub(super) fn refs_supported_inner(&self) -> bool {
        self.account()
            .is_some_and(|a| a.client.server_has("refs/1"))
    }

    pub(super) fn add_draft_ref_inner(&self, key: &DraftKey, r: DraftRef) -> AddRef {
        let added = self.shared.drafts.add_ref(key, r);
        if added == AddRef::Added {
            self.draft_changed(key, true);
        }
        added
    }

    pub(super) fn remove_draft_ref_inner(&self, key: &DraftKey, index: usize) {
        if self.shared.drafts.remove_ref(key, index) {
            self.draft_changed(key, true);
        }
    }

    pub(super) fn message_block_refs_inner(&self, message: &Message) -> Vec<BlockRef> {
        if !message.id.is_empty() {
            if let Some(r) = self.shared.refs.lock().get(&message.id) {
                return r.clone();
            }
            let Some(a) = self.account() else {
                return Vec::new();
            };
            let refs = a
                .client
                .store()
                .message(&message.id)
                .ok()
                .flatten()
                .and_then(|m| m.block_refs)
                .unwrap_or_default();
            let mut map = self.shared.refs.lock();
            if map.len() >= MAX_ENTRIES {
                map.clear();
            }
            map.insert(message.id.clone(), refs.clone());
            return refs;
        }
        // My message on its way: the outbox row holds what goes out.
        let Some(a) = self.account() else {
            return Vec::new();
        };
        match a.client.store().outbox_get(&message.client_msg_id) {
            Ok(Some(item)) => match &item.op {
                OutboxOp::Send {
                    block_refs: Some(r),
                    ..
                } => queued_refs(r, &message.topic_id),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// A send with references. With files, the upload job carries them to its send.
    pub(super) fn send_with_refs_inner(
        &self,
        request: SendRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
        refs: Vec<BlockRefInput>,
    ) -> ClientMsgId {
        use crate::data::ChatProvider as _;
        if refs.is_empty() {
            return self.send_with_files(request, key, files);
        }
        if !files.is_empty() {
            let job = self.start_upload_job(
                key,
                &files,
                Some(request.topic_id.clone()),
                None,
                request.body.clone(),
                request.mentions.clone(),
                request.quote_of.clone(),
            );
            if let Some(id) = job {
                self.set_job_refs(&id, refs);
                return id;
            }
        }
        let Some(a) = self.account() else {
            return String::new();
        };
        let real = self.resolve_topic(&request.topic_id);
        let req = chat_core::SendRequest {
            group_id: request.group_id.clone(),
            topic_id: Some(real.clone()),
            body: request.body.clone(),
            mentions: request.mentions.clone(),
            reply_to: request.quote_of.clone(),
            block_refs: Some(refs),
            ..Default::default()
        };
        let result = match pending_cid(&real) {
            // The topic has no server id yet: queue the follow-up behind its first message.
            Some(root) => a.client.send_after(
                root,
                chat_core::SendRequest {
                    topic_id: None,
                    ..req
                },
            ),
            None => a.client.send(req),
        };
        match result {
            Ok(cid) => {
                self.emit(vec![Change::Messages {
                    group_id: request.group_id,
                    topic_id: request.topic_id,
                }]);
                cid
            }
            Err(e) => {
                tracing::warn!(error = %e, "send with references refused");
                String::new()
            }
        }
    }
}
