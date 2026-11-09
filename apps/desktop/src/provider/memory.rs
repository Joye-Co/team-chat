//! 沉淀 (memory) of [`CoreProvider`] (docs/protocol.md, "Memory"): 记得的事 of an AI member,
//! 团队约定 and the daily 记忆更新. Each request runs on the chat-core runtime and answers
//! through an [`AdminReply`].

use std::{future::Future, sync::Arc};

use chat_core::{
    ApiError, Client, MemoryStatusFilter,
    proto::{self, ErrorCode},
};

use super::{CoreProvider, settings::api_error};
use crate::data::{
    AdminError, AdminReply,
    memory::{
        Memory, MemoryChange, MemoryDay, MemoryEdit, MemoryKind, MemoryScope, MemorySourceKind,
        MemoryStatus, MemoryUpdate, NewMemory,
    },
    reply_now,
};

/// The most memories one scope lists.
const LIST_LIMIT: u32 = 500;

/// A write's error: a version clash or a second undo is [`AdminError::Conflict`].
fn write_error(e: &ApiError) -> AdminError {
    match e {
        ApiError::Server {
            code: ErrorCode::Conflict,
            ..
        }
        | ApiError::Unexpected { status: 409 } => AdminError::Conflict,
        other => api_error(other),
    }
}

/// A read on a server without the memory routes (404) means the server is too old.
fn read_error(e: &ApiError) -> AdminError {
    match api_error(e) {
        AdminError::NotFound => AdminError::Unsupported,
        other => other,
    }
}

pub(super) fn kind_from(k: proto::MemoryKind) -> MemoryKind {
    match k {
        proto::MemoryKind::Convention => MemoryKind::Convention,
        proto::MemoryKind::Fact => MemoryKind::Fact,
        proto::MemoryKind::Decision => MemoryKind::Decision,
        proto::MemoryKind::Preference => MemoryKind::Preference,
        proto::MemoryKind::Lesson => MemoryKind::Lesson,
        proto::MemoryKind::Playbook => MemoryKind::Playbook,
        proto::MemoryKind::Unknown => MemoryKind::Other,
    }
}

/// `None` for [`MemoryKind::Other`]: a kind the app does not know is never sent.
fn kind_to(k: MemoryKind) -> Option<proto::MemoryKind> {
    Some(match k {
        MemoryKind::Convention => proto::MemoryKind::Convention,
        MemoryKind::Fact => proto::MemoryKind::Fact,
        MemoryKind::Decision => proto::MemoryKind::Decision,
        MemoryKind::Preference => proto::MemoryKind::Preference,
        MemoryKind::Lesson => proto::MemoryKind::Lesson,
        MemoryKind::Playbook => proto::MemoryKind::Playbook,
        MemoryKind::Other => return None,
    })
}

fn status_from(s: proto::MemoryStatus) -> MemoryStatus {
    match s {
        proto::MemoryStatus::Active => MemoryStatus::Active,
        proto::MemoryStatus::Retired => MemoryStatus::Retired,
        proto::MemoryStatus::Pending => MemoryStatus::Pending,
        proto::MemoryStatus::Unknown => MemoryStatus::Other,
    }
}

pub(super) fn memory_from(m: &proto::MemoryItem) -> Memory {
    Memory {
        id: m.id.clone(),
        scope: m.scope.clone(),
        kind: kind_from(m.kind),
        text: m.text.clone(),
        when: m.when.clone().filter(|w| !w.trim().is_empty()),
        source: match m.source.kind {
            proto::MemorySourceKind::Message => MemorySourceKind::Message,
            proto::MemorySourceKind::Run => MemorySourceKind::Run,
            proto::MemorySourceKind::Card => MemorySourceKind::Card,
            proto::MemorySourceKind::Manual => MemorySourceKind::Manual,
            proto::MemorySourceKind::Unknown => MemorySourceKind::Other,
        },
        source_ids: m.source.ids.clone(),
        note: m.note.clone().filter(|n| !n.trim().is_empty()),
        author_id: m.author_id.clone(),
        human: m.human,
        status: status_from(m.status),
        uses: m.uses,
        last_used_at: m.last_used_at.map(|t| t as i64),
        version: m.version,
        created_at: m.created_at as i64,
        updated_at: m.updated_at as i64,
    }
}

pub(super) fn update_from(u: &proto::MemoryUpdate) -> MemoryUpdate {
    MemoryUpdate {
        id: u.id.clone(),
        day: u.day.clone(),
        memory_id: u.memory_id.clone(),
        change: match u.change {
            proto::MemoryChange::Added => MemoryChange::Added,
            proto::MemoryChange::Changed => MemoryChange::Changed,
            proto::MemoryChange::Retired => MemoryChange::Retired,
            proto::MemoryChange::Flagged => MemoryChange::Flagged,
            proto::MemoryChange::Unknown => MemoryChange::Other,
        },
        before: u.before.as_ref().map(memory_from),
        after: u.after.as_ref().map(memory_from),
        note: u.note.clone().filter(|n| !n.trim().is_empty()),
        undone_at: u.undone_at.map(|t| t as i64),
        at: u.at as i64,
    }
}

/// The request of a 记一条; `None` when the kind is not one the server takes.
pub(super) fn create_request(new: &NewMemory) -> Option<proto::CreateMemoryRequest> {
    Some(proto::CreateMemoryRequest {
        scope: new.scope.wire(),
        kind: kind_to(new.kind)?,
        text: new.text.trim().to_string(),
        when: new
            .when
            .as_deref()
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(str::to_string),
        source: None,
    })
}

pub(super) fn update_request(edit: &MemoryEdit) -> proto::UpdateMemoryRequest {
    proto::UpdateMemoryRequest {
        kind: edit.kind.and_then(kind_to),
        text: edit.text.as_deref().map(|t| t.trim().to_string()),
        when: edit.when.as_ref().map(|w| {
            w.as_deref()
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(str::to_string)
        }),
        status: edit.status.and_then(|s| match s {
            MemoryStatus::Active => Some(proto::MemoryStatus::Active),
            MemoryStatus::Retired => Some(proto::MemoryStatus::Retired),
            MemoryStatus::Pending | MemoryStatus::Other => None,
        }),
    }
}

impl CoreProvider {
    fn memory_request<T, F>(&self, work: impl FnOnce(Arc<Client>) -> F) -> AdminReply<T>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, AdminError>> + Send + 'static,
    {
        let Some(a) = self.account() else {
            return reply_now(Err(AdminError::Offline));
        };
        let (tx, rx) = futures::channel::oneshot::channel();
        let fut = work(a.client.clone());
        a.client.spawn(async move {
            let _ = tx.send(fut.await);
        });
        rx
    }

    pub(super) fn memories_inner(&self, scope: &MemoryScope) -> AdminReply<Vec<Memory>> {
        let scope = scope.wire();
        self.memory_request(move |client| async move {
            let r = client
                .http()
                .memories(&scope, MemoryStatusFilter::All, None, LIST_LIMIT)
                .await
                .map_err(|e| read_error(&e))?;
            Ok(r.memories.iter().map(memory_from).collect())
        })
    }

    pub(super) fn memory_updates_inner(&self, scope: &MemoryScope) -> AdminReply<MemoryDay> {
        let scope = scope.wire();
        self.memory_request(move |client| async move {
            let r = client
                .http()
                .memory_updates(&scope, None)
                .await
                .map_err(|e| read_error(&e))?;
            Ok(MemoryDay {
                day: r.day.clone(),
                updates: r.updates.iter().map(update_from).collect(),
            })
        })
    }

    pub(super) fn create_memory_inner(&self, new: NewMemory) -> AdminReply<Memory> {
        let Some(req) = create_request(&new) else {
            return reply_now(Err(AdminError::Invalid));
        };
        self.memory_request(move |client| async move {
            let m = client
                .http()
                .create_memory(&req)
                .await
                .map_err(|e| write_error(&e))?;
            Ok(memory_from(&m))
        })
    }

    pub(super) fn update_memory_inner(&self, id: &str, edit: MemoryEdit) -> AdminReply<Memory> {
        let req = update_request(&edit);
        let id = id.to_string();
        self.memory_request(move |client| async move {
            let m = client
                .http()
                .update_memory(&id, &req)
                .await
                .map_err(|e| write_error(&e))?;
            Ok(memory_from(&m))
        })
    }

    pub(super) fn undo_memory_update_inner(&self, id: &str) -> AdminReply<MemoryUpdate> {
        let id = id.to_string();
        self.memory_request(move |client| async move {
            let u = client
                .http()
                .undo_memory_update(&id)
                .await
                .map_err(|e| write_error(&e))?;
            Ok(update_from(&u))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(kind: proto::MemoryKind, status: proto::MemoryStatus) -> proto::MemoryItem {
        proto::MemoryItem {
            id: "M1".into(),
            scope: "member:b1".into(),
            kind,
            text: "先跑压测再合并".into(),
            when: Some("  ".into()),
            source: proto::MemorySource {
                kind: proto::MemorySourceKind::Unknown,
                ids: vec!["R1".into()],
            },
            note: Some("两次回滚都因为没跑压测".into()),
            author_id: "b1".into(),
            human: false,
            status,
            confidence: 0.7,
            uses: 3,
            last_used_at: None,
            version: 2,
            created_at: 10,
            updated_at: 20,
        }
    }

    #[test]
    fn unknown_wire_values_read_as_other_and_blank_when_is_none() {
        let m = memory_from(&wire(
            proto::MemoryKind::Unknown,
            proto::MemoryStatus::Unknown,
        ));
        assert_eq!(m.kind, MemoryKind::Other);
        assert_eq!(m.status, MemoryStatus::Other);
        assert_eq!(m.source, MemorySourceKind::Other);
        assert_eq!(m.when, None);
        assert_eq!(m.note.as_deref(), Some("两次回滚都因为没跑压测"));
        let m = memory_from(&wire(
            proto::MemoryKind::Playbook,
            proto::MemoryStatus::Pending,
        ));
        assert_eq!(
            (m.kind, m.status, m.updated_at),
            (MemoryKind::Playbook, MemoryStatus::Pending, 20)
        );
    }

    #[test]
    fn requests_trim_and_never_send_what_the_server_refuses() {
        let new = NewMemory {
            scope: MemoryScope::Member("b1".into()),
            kind: MemoryKind::Lesson,
            text: "  先跑压测  ".into(),
            when: Some(" ".into()),
        };
        let req = create_request(&new).unwrap();
        assert_eq!(req.scope, "member:b1");
        assert_eq!(req.text, "先跑压测");
        assert_eq!(req.when, None);
        assert!(req.validate().is_ok());
        assert!(
            create_request(&NewMemory {
                kind: MemoryKind::Other,
                ..new
            })
            .is_none()
        );

        let edit = MemoryEdit {
            when: Some(Some("  ".into())),
            status: Some(MemoryStatus::Pending),
            ..Default::default()
        };
        let req = update_request(&edit);
        // A blank `when` clears it; a person never makes a memory pending.
        assert_eq!(req.when, Some(None));
        assert_eq!(req.status, None);
        let retire = update_request(&MemoryEdit {
            status: Some(MemoryStatus::Retired),
            ..Default::default()
        });
        assert_eq!(retire.status, Some(proto::MemoryStatus::Retired));
        assert!(retire.validate().is_ok());
    }

    #[test]
    fn a_conflict_is_its_own_error() {
        let conflict = ApiError::Server {
            status: 409,
            code: ErrorCode::Conflict,
            message: "conflict".into(),
        };
        assert_eq!(write_error(&conflict), AdminError::Conflict);
        let missing = ApiError::Unexpected { status: 404 };
        assert_eq!(read_error(&missing), AdminError::Unsupported);
        assert_eq!(write_error(&missing), AdminError::NotFound);
    }
}
