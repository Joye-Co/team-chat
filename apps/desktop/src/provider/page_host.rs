//! The 产出 page host on chat-core (inc-host track): bridge facts by author (R10), page
//! events and asks (`artifact.event`), the data snapshot (`GET …/data`) and the signals
//! the server sends back (acks, `artifact.reply`, `artifact.data`).

use std::sync::Mutex;

use chat_core::{ClientUpdate, proto::ArtifactEventKind};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use super::CoreProvider;
use crate::data::{
    ChatProvider as _,
    page_host::{
        ExpectMode, PageAck, PageBridge, PageEventDecl, PageEventKind, PageSendError, PageSignal,
    },
};

/// Receivers of page signals (the app's page host).
#[derive(Default)]
pub(super) struct PageSignals {
    txs: Mutex<Vec<UnboundedSender<PageSignal>>>,
}

impl PageSignals {
    pub(super) fn subscribe(&self) -> UnboundedReceiver<PageSignal> {
        let (tx, rx) = unbounded();
        self.txs.lock().unwrap_or_else(|e| e.into_inner()).push(tx);
        rx
    }

    fn send(&self, s: PageSignal) {
        self.txs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|tx| tx.unbounded_send(s.clone()).is_ok());
    }
}

fn mode(m: chat_core::proto::ExpectMode) -> ExpectMode {
    use chat_core::proto::ExpectMode as M;
    match m {
        M::Live => ExpectMode::Live,
        M::Run => ExpectMode::Run,
        M::Starting => ExpectMode::Starting,
        M::Offline => ExpectMode::Offline,
        M::None | M::Unknown => ExpectMode::None,
    }
}

/// The page signal of a client update, if it is one.
pub(super) fn signal(u: &ClientUpdate) -> Option<PageSignal> {
    match u {
        ClientUpdate::ArtifactEventAcked {
            client_event_id,
            result,
            ..
        } => Some(PageSignal::Acked {
            client_event_id: client_event_id.clone(),
            result: match result {
                Ok(a) => Ok(PageAck {
                    event_id: a.event_id.clone(),
                    mode: mode(a.expect.mode),
                }),
                Err(e) => Err(e.reason.clone().unwrap_or_default()),
            },
        }),
        ClientUpdate::ArtifactReply(r) => Some(PageSignal::Reply {
            event_id: r.event_id.clone(),
            ok: r.ok,
            payload: r.payload.clone(),
            declined: r
                .error
                .as_ref()
                .is_some_and(|e| e.code == chat_core::proto::ArtifactReplyCode::Declined),
        }),
        ClientUpdate::ArtifactData(d) => Some(PageSignal::Data {
            artifact_id: d.artifact_id.clone(),
            version: d.version,
            data_rev: d.data_rev,
            data: d.data.clone(),
            cause: d.cause.clone(),
        }),
        _ => None,
    }
}

impl CoreProvider {
    pub(super) fn emit_page_signal(&self, u: &ClientUpdate) {
        if let Some(s) = signal(u) {
            self.shared.page_signals.send(s);
        }
    }

    pub(super) fn page_bridge_inner(&self, artifact_id: &str, version: u32) -> Option<PageBridge> {
        let a = self.account()?;
        let row = a.client.artifact(artifact_id).ok().flatten()?;
        let v = row.versions.iter().find(|v| v.version == version)?;
        let author = self.member(&v.created_by);
        let in_group = self
            .groups()
            .into_iter()
            .find(|g| g.id == row.artifact.group_id)
            .is_some_and(|g| g.member_ids.iter().any(|m| *m == v.created_by));
        let events = v
            .events
            .iter()
            .flatten()
            .map(|e| PageEventDecl {
                name: e.name.clone(),
                kind: match e.kind() {
                    ArtifactEventKind::Ask => PageEventKind::Ask,
                    _ => PageEventKind::Emit,
                },
                label: e.label.clone(),
            })
            .collect();
        Some(PageBridge {
            author_id: v.created_by.clone(),
            author_active_bot: in_group
                && author.as_ref().is_some_and(|m| m.is_bot() && !m.deactivated),
            runner: v.run_id.is_some(),
            events,
            data_rev: v.data_rev.unwrap_or(0),
        })
    }

    pub(super) fn send_page_event_inner(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        kind: PageEventKind,
        payload: Option<serde_json::Value>,
    ) -> Result<String, PageSendError> {
        let a = self.account().ok_or(PageSendError::Offline)?;
        if a.client.connection_state() != chat_core::ConnectionState::Connected {
            return Err(PageSendError::Offline);
        }
        let k = match kind {
            PageEventKind::Emit => ArtifactEventKind::Emit,
            PageEventKind::Ask => ArtifactEventKind::Ask,
        };
        let id = a
            .client
            .send_artifact_event(artifact_id, version, name, k, payload.clone())
            .map_err(|e| {
                tracing::warn!(error = %e, "产出 page event refused");
                PageSendError::Refused
            })?;
        self.shared.page_events.record(
            artifact_id,
            super::pages::PageEvent {
                actor_id: a.me.id.clone(),
                version,
                name: name.to_string(),
                payload,
                at: crate::selftest::clock::now_ms(),
            },
        );
        Ok(id)
    }

    pub(super) fn fetch_page_data_inner(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> futures::channel::oneshot::Receiver<Option<(serde_json::Value, u64)>> {
        let (tx, rx) = futures::channel::oneshot::channel();
        let Some(a) = self.account() else {
            let _ = tx.send(None);
            return rx;
        };
        let handle = a.client.artifact_data(artifact_id, version);
        a.client.spawn(async move {
            let out = match handle.await {
                Ok(Ok(r)) => Some((r.data, r.data_rev)),
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "产出 data snapshot: fetch failed");
                    None
                }
                Err(_) => None,
            };
            let _ = tx.send(out);
        });
        rx
    }
}

// --- media metadata and posters (R9) ---

use std::{collections::HashMap, path::PathBuf, sync::OnceLock};

use crate::data::{Change, FileAttachment};

/// `FileRef.media` by file id, kept when messages are converted (the view type has no
/// room for it).
static FILE_MEDIA: OnceLock<Mutex<HashMap<String, chat_core::proto::MediaInfo>>> =
    OnceLock::new();

/// Notes the media of a message file the cache converts.
pub(super) fn note_file_media(f: &chat_core::proto::FileRef) {
    if let Some(m) = &f.media
        && !f.file_id.is_empty()
    {
        FILE_MEDIA
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(f.file_id.clone(), m.clone());
    }
}

impl CoreProvider {
    pub(super) fn file_media_inner(
        &self,
        file: &FileAttachment,
    ) -> Option<chat_core::proto::MediaInfo> {
        FILE_MEDIA
            .get()?
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&file.file_id)
            .cloned()
    }

    pub(super) fn artifact_media_inner(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Option<chat_core::proto::MediaInfo> {
        let a = self.account()?;
        let row = a.client.artifact(artifact_id).ok().flatten()?;
        row.versions
            .into_iter()
            .find(|v| v.version == version)?
            .media
    }

    pub(super) fn file_poster_inner(
        &self,
        group_id: &str,
        topic_id: &str,
        file: &FileAttachment,
    ) -> Option<PathBuf> {
        // The sender's poster first (receivers compute nothing).
        if let Some(id) = self
            .file_media_inner(file)
            .and_then(|m| m.poster_file_id)
        {
            return self.card_image(group_id, topic_id, &id);
        }
        let kind = crate::views::media::file_kind(&file.name, &file.mime);
        let path = self.cached_file(file)?;
        let this = self.clone();
        let change = Change::Messages {
            group_id: group_id.to_string(),
            topic_id: topic_id.to_string(),
        };
        crate::views::media::poster::poster(&path, kind, move || this.emit(vec![change]))
    }

    pub(super) fn artifact_poster_inner(&self, artifact_id: &str, version: u32) -> Option<PathBuf> {
        let a = self.account()?;
        let row = a.client.artifact(artifact_id).ok().flatten()?;
        let v = row.versions.iter().find(|v| v.version == version)?;
        let (g, t) = (row.artifact.group_id.clone(), row.artifact.topic_id.clone());
        if let Some(id) = v.media.as_ref().and_then(|m| m.poster_file_id.clone()) {
            return self.card_image(&g, &t, &id);
        }
        let mime = v
            .files
            .iter()
            .find(|f| f.path == v.entry)
            .map(|f| f.mime.clone())
            .unwrap_or_default();
        let kind = crate::views::media::file_kind(&v.entry, &mime);
        let dir = a.client.cached_artifact_version(artifact_id, version)?;
        let path = dir.join(v.entry.replace('/', std::path::MAIN_SEPARATOR_STR));
        let this = self.clone();
        let change = Change::Artifacts {
            group_id: g.clone(),
            topic_id: t.clone(),
        };
        // MEDIA §3 fallback: the sender sent no preview (or it is still pending): the first
        // receiver that can make one sends it as `artifact.previewReady`.
        let fallback = v
            .media
            .as_ref()
            .is_some_and(|m| m.preview == chat_core::proto::MediaPreview::Pending)
            .then(|| (a.client.clone(), path.clone(), v.entry.clone(), mime.clone()));
        let (id, version) = (artifact_id.to_string(), version);
        crate::views::media::poster::poster(&path, kind, move || {
            this.emit(vec![change]);
            if let Some((client, path, name, mime)) = fallback {
                let c = client.clone();
                client.spawn(async move {
                    let (info, poster) =
                        crate::views::media::poster::describe(&path, &name, &mime);
                    send_version_media(&c, &g, &t, &id, version, info, poster).await;
                });
            }
        })
    }
}

/// Upload keys whose preview the sender is making now (正在生成预览).
static GENERATING: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

pub(super) fn set_generating(key: &str, on: bool) {
    let mut g = GENERATING
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if on {
        g.insert(key.to_string());
    } else {
        g.remove(key);
    }
}

pub(super) fn is_generating(key: &str) -> bool {
    GENERATING.get().is_some_and(|g| {
        g.lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(key)
    })
}

/// Uploads a version's poster (when there is one) and sends its media metadata
/// (`artifact.previewReady`). Best effort: a failure only leaves the preview to the
/// receivers' fallback.
pub(super) async fn send_version_media(
    client: &std::sync::Arc<chat_core::Client>,
    group_id: &str,
    topic_id: &str,
    artifact_id: &str,
    version: u32,
    mut info: chat_core::proto::MediaInfo,
    poster: Option<PathBuf>,
) {
    if let Some(path) = poster.filter(|p| p.extension().is_some_and(|e| e == "jpg")) {
        let up = client.upload_file(
            chat_core::UploadRequest {
                group_id: group_id.to_string(),
                topic_id: Some(topic_id.to_string()),
                path,
                name: Some("poster.jpg".into()),
                mime: Some("image/jpeg".into()),
            },
            chat_core::no_progress(),
        );
        match up.await {
            Ok(Ok(m)) => info.poster_file_id = Some(m.id),
            _ => info.preview = chat_core::proto::MediaPreview::Failed,
        }
    }
    if info.validate().is_err() {
        return;
    }
    if let Ok(Err(e)) = client.put_artifact_media(artifact_id, version, info).await {
        tracing::info!(error = %e, "产出 media not sent");
    }
}
