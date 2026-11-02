//! The real-time parts of the chat-core provider: 待办, per-topic read
//! positions and follow, the topic owner, group mute, group and DM commands,
//! notification text, and the local UI state (drafts and the saved place).
//!
//! `mod.rs` keeps the `ChatProvider` impl; its methods for these features
//! call the functions here.

use std::{collections::HashMap, path::Path};

use chat_core::{
    Notification, TopicPatch,
    proto::{GroupKind as PGroupKind, NotifyReason, UpdateGroupPrefsRequest},
};

use super::{
    CoreProvider, Notice,
    convert::{self, pending_cid},
    persist::{self, SavedDraft, UiState},
};
use crate::{
    copy::{fill, notify as n},
    data::{
        AdminError, AdminReply, Change, CreateGroupRequest, DraftKey, GroupId, MemberId,
        SavedNavigation, TodoItem, TodoReason, reply_now,
    },
};

/// Longest notification body, in characters.
const NOTICE_BODY_CHARS: usize = 180;

impl CoreProvider {
    // ------------------------------------------------------------------
    // 待办
    // ------------------------------------------------------------------

    pub(super) fn todo_items(&self) -> Vec<TodoItem> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let rows = match a.client.todo() {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(error = %e, "todo read failed");
                return Vec::new();
            }
        };
        if rows.is_empty() {
            return Vec::new();
        }
        let names: HashMap<String, String> = crate::data::ChatProvider::groups(self)
            .into_iter()
            .map(|g| (g.id, g.name))
            .collect();
        let mut out: Vec<TodoItem> = rows
            .iter()
            .filter_map(|row| {
                let topic = convert::topic(&row.topic);
                // A topic of a group I left can stay in the cache: skip it.
                let group_name = names.get(&topic.group_id)?.clone();
                let reason = if row.reasons.contains(&chat_core::TodoReason::WaitingOnMe) {
                    TodoReason::WaitingOnMe
                } else {
                    TodoReason::Mentioned
                };
                Some(TodoItem {
                    group_id: topic.group_id.clone(),
                    group_name,
                    topic,
                    reason,
                })
            })
            .collect();
        out.sort_by_key(|x| std::cmp::Reverse(x.topic.last_activity_at));
        out
    }

    // ------------------------------------------------------------------
    // Per-topic state
    // ------------------------------------------------------------------

    fn topic_row(&self, topic_id: &str) -> Option<chat_core::TopicRow> {
        let a = self.account()?;
        let real = self.resolve_topic(topic_id);
        if pending_cid(&real).is_some() {
            return None;
        }
        a.client.store().topic(&real).ok().flatten()
    }

    pub(super) fn topic_read_seq_inner(&self, topic_id: &str) -> Option<u64> {
        if pending_cid(&self.resolve_topic(topic_id)).is_some() {
            // My own new topic: nothing in it is unread.
            return Some(u64::MAX);
        }
        self.topic_row(topic_id).map(|row| row.read_seq)
    }

    pub(super) fn topic_following_inner(&self, topic_id: &str) -> bool {
        self.topic_row(topic_id).is_some_and(|row| row.following)
    }

    pub(super) fn set_topic_following_inner(&self, topic_id: &str, follow: bool) {
        let Some(a) = self.account() else {
            return;
        };
        let real = self.resolve_topic(topic_id);
        if pending_cid(&real).is_some() {
            return;
        }
        if let Err(e) = a.client.follow_topic(&real, follow) {
            tracing::warn!(error = %e, "follow refused");
        }
    }

    pub(super) fn set_topic_owner_inner(&self, topic_id: &str, owner: Option<String>) {
        let Some(a) = self.account() else {
            return;
        };
        let real = self.resolve_topic(topic_id);
        if pending_cid(&real).is_some() {
            return;
        }
        let patch = TopicPatch {
            owner_id: Some(owner),
            ..Default::default()
        };
        if let Err(e) = a.client.update_topic(&real, patch) {
            tracing::warn!(error = %e, "owner update refused");
        }
    }

    // ------------------------------------------------------------------
    // Groups
    // ------------------------------------------------------------------

    pub(super) fn group_muted_inner(&self, group_id: &str) -> bool {
        self.account().is_some_and(|a| {
            a.client
                .store()
                .group(group_id)
                .ok()
                .flatten()
                .is_some_and(|g| g.muted)
        })
    }

    pub(super) fn set_group_muted_inner(&self, group_id: &str, muted: bool) {
        let Some(a) = self.account() else {
            return;
        };
        let handle = a.client.set_group_prefs(
            group_id,
            UpdateGroupPrefsRequest {
                muted: Some(muted),
                ..Default::default()
            },
        );
        chat_core::background().spawn(async move {
            match handle.await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::warn!(error = %e, "mute refused"),
                Err(e) => tracing::warn!(error = %e, "mute task failed"),
            }
        });
    }

    /// Group and DM commands go over HTTP; the server pushes the new group
    /// to every connection as a `directory` frame, which updates the cache.
    pub(super) fn create_group_inner(&self, request: CreateGroupRequest) -> AdminReply<GroupId> {
        let Some(a) = self.account() else {
            return reply_now(Err(AdminError::Offline));
        };
        let http = a.client.http();
        let (tx, rx) = futures::channel::oneshot::channel();
        chat_core::background().spawn(async move {
            let r = http.create_group(&request.name, &request.member_ids).await;
            if let Err(e) = &r {
                tracing::warn!(error = %e, "create group refused");
            }
            let _ = tx.send(
                r.map(|g| g.group.id)
                    .map_err(|e| super::settings::api_error(&e)),
            );
        });
        rx
    }

    pub(super) fn open_dm_inner(&self, member_id: &str) -> AdminReply<GroupId> {
        let Some(a) = self.account() else {
            return reply_now(Err(AdminError::Offline));
        };
        let http = a.client.http();
        let member_id = member_id.to_string();
        let (tx, rx) = futures::channel::oneshot::channel();
        chat_core::background().spawn(async move {
            let r = http.dm(&member_id).await;
            if let Err(e) = &r {
                tracing::warn!(error = %e, "open dm refused");
            }
            let _ = tx.send(
                r.map(|g| g.group.id)
                    .map_err(|e| super::settings::api_error(&e)),
            );
        });
        rx
    }

    /// Adds members one by one; stops at the first refusal.
    pub(super) fn add_group_members_inner(
        &self,
        group_id: &str,
        member_ids: Vec<MemberId>,
    ) -> AdminReply<()> {
        let Some(a) = self.account() else {
            return reply_now(Err(AdminError::Offline));
        };
        let http = a.client.http();
        let group_id = group_id.to_string();
        let (tx, rx) = futures::channel::oneshot::channel();
        chat_core::background().spawn(async move {
            for m in &member_ids {
                if let Err(e) = http.add_group_member(&group_id, m).await {
                    tracing::warn!(error = %e, "add group member refused");
                    let _ = tx.send(Err(super::settings::api_error(&e)));
                    return;
                }
            }
            let _ = tx.send(Ok(()));
        });
        rx
    }

    pub(super) fn leave_group_inner(&self, group_id: &str) {
        let Some(a) = self.account() else {
            return;
        };
        let http = a.client.http();
        let (group_id, me) = (group_id.to_string(), a.me.id.clone());
        chat_core::background().spawn(async move {
            if let Err(e) = http.remove_group_member(&group_id, &me).await {
                tracing::warn!(error = %e, "leave group refused");
            }
        });
    }

    // ------------------------------------------------------------------
    // Notifications
    // ------------------------------------------------------------------

    /// The text of a notification. The server already decided that this
    /// event notifies me (quiet rules); chat-core skips my own events and
    /// muted groups.
    pub(super) fn notice_from(&self, n: &Notification) -> Notice {
        let is_dm = self.account().is_some_and(|a| {
            a.client
                .store()
                .group(&n.group_id)
                .ok()
                .flatten()
                .is_some_and(|g| g.group.kind == PGroupKind::Dm)
        });
        let actor = n.actor_name.clone().unwrap_or_default();
        let group = n.title.as_str();
        let topic = n.subtitle.clone().unwrap_or_default();
        let excerpt = excerpt(&n.body);
        let (title, body) = match n.reason {
            NotifyReason::Mention => (
                fill(n::MENTION_TITLE, &[("name", &actor), ("group", group)]),
                excerpt,
            ),
            NotifyReason::Waiting => (fill(n::WAITING_TITLE, &[("group", group)]), topic),
            NotifyReason::Result if excerpt.is_empty() => {
                (fill(n::RESULT_TITLE, &[("group", group)]), topic)
            }
            NotifyReason::Result => (
                fill(n::RESULT_TITLE, &[("group", group)]),
                fill(n::RESULT_BODY, &[("topic", &topic), ("text", &excerpt)]),
            ),
            NotifyReason::Message if is_dm => (
                if actor.is_empty() {
                    group.to_string()
                } else {
                    actor
                },
                excerpt,
            ),
            NotifyReason::Message => (
                fill(n::MESSAGE_TITLE, &[("name", &actor), ("group", group)]),
                excerpt,
            ),
        };
        Notice {
            title,
            body,
            group_id: n.group_id.clone(),
            topic_id: n.topic_id.clone(),
        }
    }

    // ------------------------------------------------------------------
    // Local UI state
    // ------------------------------------------------------------------

    /// Loads the drafts and the saved place of the account in `dir`
    /// (called when an account is installed).
    pub(super) fn load_ui_state(&self, dir: &Path) {
        let state = persist::load(dir);
        self.shared.drafts.clear_all();
        for d in &state.drafts {
            let key = d.key();
            self.shared
                .drafts
                .save_text(&key, d.body.clone(), d.mentions.clone());
            self.shared.drafts.set_quote(&key, d.quote_of.clone());
        }
        self.state().navigation = state.navigation.map(SavedNavigation::from);
    }

    /// Queues the current drafts and place for the writer thread.
    pub(super) fn persist_ui_state(&self) {
        let Some(a) = self.account() else {
            return;
        };
        let drafts: Vec<SavedDraft> = self
            .shared
            .drafts
            .entries()
            .iter()
            .filter_map(|(key, draft)| {
                let key = self.durable_key(key)?;
                SavedDraft::from_draft(&key, draft)
            })
            .collect();
        let navigation = self.state().navigation.clone();
        self.shared
            .writer
            .save(&a.dir, UiState::new(drafts, navigation.as_ref()));
    }

    /// A draft key with a real topic id; `None` for a topic that is still
    /// pending (its id would not exist after a restart).
    fn durable_key(&self, key: &DraftKey) -> Option<DraftKey> {
        match &key.topic_id {
            None => Some(key.clone()),
            Some(t) => {
                let real = self.resolve_topic(t);
                pending_cid(&real).is_none().then(|| DraftKey {
                    group_id: key.group_id.clone(),
                    topic_id: Some(real),
                })
            }
        }
    }

    pub(super) fn saved_navigation_inner(&self) -> Option<SavedNavigation> {
        self.state().navigation.clone()
    }

    pub(super) fn save_navigation_inner(&self, mut navigation: SavedNavigation) {
        if let Some(t) = &navigation.topic_id {
            let real = self.resolve_topic(t);
            navigation.topic_id = pending_cid(&real).is_none().then_some(real);
        }
        {
            let mut s = self.state();
            if s.navigation.as_ref() == Some(&navigation) {
                return;
            }
            s.navigation = Some(navigation);
        }
        self.persist_ui_state();
    }

    /// After the draft of `key` changed.
    pub(super) fn draft_changed(&self, key: &DraftKey, emit: bool) {
        if emit {
            self.emit(vec![Change::Draft { key: key.clone() }]);
        }
        self.persist_ui_state();
    }
}

fn excerpt(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::markdown::clip(&one, NOTICE_BODY_CHARS)
}
