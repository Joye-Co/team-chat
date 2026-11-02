//! Files and 产出 on chat-core: uploads before a send, downloads into the
//! file cache, image thumbnails, and 产出 versions.
//!
//! - A send with files becomes an [`UploadJob`]. It shows at once as my
//!   pending message (and, for a new topic, as a pending topic
//!   `pending:<job id>`). The files upload one by one with progress in
//!   [`ChatProvider::file_transfer`]; then the message goes to the outbox with
//!   its attachments. A failed upload marks the message failed; `retry_send`
//!   uploads the missing files again and `discard_send` drops the job.
//! - Downloads are shared per SHA-256: several views asking for one file
//!   start one transfer. Images also get a small PNG thumbnail next to the
//!   cache, so the timeline never decodes a full photo.
//! - Every transfer emits `Change::Messages` for its topic at most every
//!   [`PROGRESS_EVERY`], and once when it ends.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use chat_core::{
    ClientError,
    artifacts::{LocalArtifactFile, NewVersion},
    files::{Progress, UploadRequest},
    http::ApiError,
    proto::{self, ArtifactKind as PKind, ErrorCode},
};
use futures::channel::oneshot;

use super::{CoreProvider, convert, lock};
use crate::{
    copy::{artifacts as ca, fill},
    data::{
        ArtifactDetail, ArtifactFolder, ChatProvider as _, ArtifactKind, ArtifactSummary, ArtifactVersionInfo, Change,
        CreatedArtifact, DraftKey, FileAttachment, FileError, MAX_FILE_BYTES, Message, NewArtifactRequest,
        NewFile, PendingFile, PendingFileId, RejectedFile, Reply, SendState, Topic, TopicFile,
        TopicStatus, TransferState, create_key, download_key, upload_key,
    },
};

/// Most often a running transfer refreshes its topic.
const PROGRESS_EVERY: Duration = Duration::from_millis(150);
/// Longest side of an image thumbnail, in pixels (2x for Retina).
pub const THUMB_MAX_PX: u32 = 640;
/// Images larger than this are shown as file rows, not decoded for a thumbnail.
pub const THUMB_SOURCE_MAX_BYTES: u64 = 40_000_000;
/// Cached messages read for the side panel's file list.
const TOPIC_FILES_SCAN: usize = 1000;

/// File and 产出 state of the provider.
#[derive(Default)]
pub struct FilesState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    transfers: HashMap<String, TransferState>,
    jobs: Vec<UploadJob>,
    rejected: HashMap<DraftKey, Vec<RejectedFile>>,
    /// SHA-256 -> local file.
    cached: HashMap<String, PathBuf>,
    /// SHA-256 -> thumbnail.
    thumbs: HashMap<String, PathBuf>,
    /// SHA-256s known not to be cached (cleared when a transfer ends).
    missing: HashSet<String>,
    /// Running downloads: SHA-256 -> waiting replies and topics to refresh.
    downloads: HashMap<String, Download>,
    /// 产出 ids asked from the server this session.
    requested: HashSet<String>,
    /// Topics whose 产出 list was asked from the server this session.
    refreshed: HashSet<String>,
    /// Last progress refresh per transfer key.
    last_emit: HashMap<String, Instant>,
}

#[derive(Default)]
struct Download {
    waiters: Vec<oneshot::Sender<Result<PathBuf, FileError>>>,
    topics: HashSet<(String, String)>,
}

/// One message with files that waits for its uploads.
#[derive(Clone)]
struct UploadJob {
    /// The message's client id while it uploads.
    id: String,
    generation: u64,
    group_id: String,
    /// `None` starts a new topic.
    topic_id: Option<String>,
    title: Option<String>,
    body: String,
    mentions: Vec<String>,
    quote_of: Option<String>,
    files: Vec<JobFile>,
    created_at: i64,
    failed: bool,
    running: bool,
    /// Bumped by retry and discard; a runner with an older run stops.
    run: u64,
}

#[derive(Clone)]
struct JobFile {
    name: String,
    mime: String,
    size: u64,
    width: Option<u32>,
    height: Option<u32>,
    source: NewFile,
    uploaded: Option<proto::FileRef>,
}

impl FilesState {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        lock(&self.inner)
    }

    /// Forgets everything of the signed-in account (sign-in and sign-out).
    pub fn reset(&self) {
        *self.inner() = Inner::default();
    }
}

pub fn file_error(e: &ClientError) -> FileError {
    match e {
        ClientError::NotFound => FileError::NotFound,
        ClientError::Invalid(_) => FileError::Rejected,
        ClientError::Api(a) => match a {
            ApiError::Network(_) => FileError::Network,
            ApiError::Server { code, status, .. } => match code {
                ErrorCode::TooLarge => FileError::TooLarge,
                ErrorCode::NotFound => FileError::NotFound,
                _ if *status == 404 => FileError::NotFound,
                _ if *status >= 500 => FileError::Network,
                _ => FileError::Rejected,
            },
            ApiError::Invalid(_) => FileError::Rejected,
            ApiError::Io(_) => FileError::Unreadable,
            ApiError::Checksum => FileError::Checksum,
            ApiError::NoToken => FileError::SignedOut,
            _ => FileError::Other,
        },
        _ => FileError::Other,
    }
}

/// Checks a local path before it goes into a draft or an upload.
pub fn check_local_file(path: &Path) -> Result<u64, FileError> {
    let meta = std::fs::metadata(path).map_err(|_| FileError::Unreadable)?;
    if !meta.is_file() {
        return Err(FileError::NotAFile);
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(FileError::TooLarge);
    }
    Ok(meta.len())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The `<title>` of an HTML page, when it has a short one.
pub fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = start + lower[start..].find('>')? + 1;
    let close = open_end + lower[open_end..].find("</title")?;
    let title = html.get(open_end..close)?;
    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = title
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    (!title.is_empty()).then(|| title.chars().take(200).collect())
}

/// What a picked file or folder becomes: kind, files, entry and default title.
pub fn plan_artifact(path: &Path) -> Result<(PKind, Vec<LocalArtifactFile>, String, String), FileError> {
    let meta = std::fs::metadata(path).map_err(|_| FileError::Unreadable)?;
    let is_html = |p: &str| {
        let l = p.to_ascii_lowercase();
        l.ends_with(".html") || l.ends_with(".htm")
    };
    if meta.is_dir() {
        let files = chat_core::artifacts::local_files_in(path).map_err(|e| match e {
            ApiError::Invalid(_) => FileError::NoPage,
            _ => FileError::Unreadable,
        })?;
        for f in &files {
            if std::fs::metadata(&f.local).map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
                return Err(FileError::TooLarge);
            }
        }
        let entry = files
            .iter()
            .map(|f| f.path.as_str())
            .find(|p| p.eq_ignore_ascii_case("index.html"))
            .or_else(|| {
                let mut pages: Vec<&str> = files
                    .iter()
                    .map(|f| f.path.as_str())
                    .filter(|p| is_html(p))
                    .collect();
                pages.sort_by_key(|p| (p.matches('/').count(), p.len()));
                pages.first().copied()
            })
            .ok_or(FileError::NoPage)?
            .to_string();
        let files: Vec<LocalArtifactFile> = files
            .into_iter()
            .map(|mut f| {
                if is_html(&f.path) {
                    f.mime = Some("text/html".into());
                }
                f
            })
            .collect();
        let local = files.iter().find(|f| f.path == entry).map(|f| f.local.clone());
        let title = local
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| html_title(&String::from_utf8_lossy(&b[..b.len().min(64 * 1024)])))
            .unwrap_or_else(|| file_name(path));
        return Ok((PKind::Html, files, entry, title));
    }
    check_local_file(path)?;
    let name = file_name(path);
    if !chat_core::proto::is_artifact_path(&name) {
        return Err(FileError::Rejected);
    }
    if is_html(&name) {
        let title = std::fs::read(path)
            .ok()
            .and_then(|b| html_title(&String::from_utf8_lossy(&b[..b.len().min(64 * 1024)])))
            .unwrap_or_else(|| name.clone());
        let files = vec![LocalArtifactFile {
            path: name.clone(),
            local: path.to_path_buf(),
            mime: Some("text/html".into()),
        }];
        Ok((PKind::Html, files, name, title))
    } else {
        let files = vec![LocalArtifactFile {
            path: name.clone(),
            local: path.to_path_buf(),
            mime: None,
        }];
        Ok((PKind::File, files, name.clone(), name))
    }
}

/// Writes a small PNG thumbnail of an image (no larger than
/// [`THUMB_MAX_PX`]). Returns the source when it is already small.
pub fn make_thumbnail(source: &Path, dest: &Path) -> Option<PathBuf> {
    let (w, h) = chat_core::files::image_size(source)?;
    if w <= THUMB_MAX_PX && h <= THUMB_MAX_PX {
        return Some(source.to_path_buf());
    }
    if dest.is_file() {
        return Some(dest.to_path_buf());
    }
    let size = std::fs::metadata(source).ok()?.len();
    if size > THUMB_SOURCE_MAX_BYTES {
        return None;
    }
    let img = image::ImageReader::open(source)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let thumb = img.thumbnail(THUMB_MAX_PX, THUMB_MAX_PX);
    std::fs::create_dir_all(dest.parent()?).ok()?;
    let part = dest.with_extension(format!("part-{}", chat_core::new_id()));
    thumb.save_with_format(&part, image::ImageFormat::Png).ok()?;
    std::fs::rename(&part, dest).ok()?;
    Some(dest.to_path_buf())
}

impl CoreProvider {
    fn files(&self) -> &FilesState {
        &self.shared.files
    }

    fn generation(&self) -> u64 {
        self.state().generation
    }

    /// Sets a transfer state and refreshes `topic` (throttled while running).
    fn set_transfer(&self, key: &str, state: TransferState, topic: Option<(&str, &str)>) {
        let emit = {
            let mut i = self.files().inner();
            let running = matches!(state, TransferState::Running { .. });
            i.transfers.insert(key.to_string(), state);
            let now = Instant::now();
            let due = !running
                || i
                    .last_emit
                    .get(key)
                    .is_none_or(|t| now.duration_since(*t) >= PROGRESS_EVERY);
            if due {
                i.last_emit.insert(key.to_string(), now);
            }
            if !running {
                i.last_emit.remove(key);
            }
            due
        };
        if emit && let Some((g, t)) = topic {
            self.emit(vec![Change::Messages {
                group_id: g.to_string(),
                topic_id: t.to_string(),
            }]);
        }
    }

    // ------------------------------------------------------------------
    // Drafts
    // ------------------------------------------------------------------

    pub(super) fn attach_files_inner(&self, key: &DraftKey, files: Vec<NewFile>) {
        let this = self.clone();
        let key = key.clone();
        let generation = self.generation();
        // File sizes come from the disk: never on the UI thread.
        std::thread::spawn(move || {
            let mut accepted = Vec::new();
            let mut rejected = Vec::new();
            for f in files {
                match &f {
                    NewFile::Path(p) => match check_local_file(p) {
                        Ok(_) => accepted.push(f),
                        Err(error) => rejected.push(RejectedFile {
                            name: file_name(p),
                            error,
                        }),
                    },
                    NewFile::Image { bytes, .. } if bytes.len() as u64 > MAX_FILE_BYTES => {
                        rejected.push(RejectedFile {
                            name: String::new(),
                            error: FileError::TooLarge,
                        })
                    }
                    NewFile::Image { .. } => accepted.push(f),
                }
            }
            if this.generation() != generation {
                return;
            }
            let added = !accepted.is_empty() && this.shared.drafts.attach(&key, accepted);
            if !rejected.is_empty() {
                this.files()
                    .inner()
                    .rejected
                    .entry(key.clone())
                    .or_default()
                    .extend(rejected.clone());
            }
            if added || !rejected.is_empty() {
                this.draft_changed(&key, true);
            }
        });
    }

    pub(super) fn take_rejected_inner(&self, key: &DraftKey) -> Vec<RejectedFile> {
        self.files().inner().rejected.remove(key).unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Sends with files
    // ------------------------------------------------------------------

    /// Starts an upload job for the listed files of the draft. Returns the
    /// job id (used as the message's client id), or `None` when the draft
    /// has none of these files.
    pub(super) fn start_upload_job(
        &self,
        key: &DraftKey,
        file_ids: &[PendingFileId],
        topic_id: Option<String>,
        title: Option<String>,
        body: String,
        mentions: Vec<String>,
        quote_of: Option<String>,
    ) -> Option<String> {
        let draft = self.shared.drafts.get(key);
        let files: Vec<JobFile> = draft
            .files
            .iter()
            .filter(|f| file_ids.contains(&f.id))
            .map(job_file)
            .collect();
        if files.is_empty() {
            return None;
        }
        let job = UploadJob {
            id: chat_core::new_id(),
            generation: self.generation(),
            group_id: key.group_id.clone(),
            topic_id,
            title,
            body,
            mentions,
            quote_of,
            files,
            created_at: chat_core::now_ms() as i64,
            failed: false,
            running: false,
            run: 0,
        };
        let id = job.id.clone();
        let topic = job.topic_id.clone();
        for i in 0..job.files.len() {
            self.files()
                .inner()
                .transfers
                .insert(upload_key(&id, i), TransferState::Queued);
        }
        self.files().inner().jobs.push(job);
        let mut changes = vec![Change::Messages {
            group_id: key.group_id.clone(),
            topic_id: topic.unwrap_or_else(|| convert::pending_topic_id(&id)),
        }];
        changes.extend([
            Change::Topics {
                group_id: key.group_id.clone(),
            },
            Change::Groups,
        ]);
        self.emit(changes);
        self.run_job(&id);
        Some(id)
    }

    fn job(&self, id: &str) -> Option<UploadJob> {
        self.files().inner().jobs.iter().find(|j| j.id == id).cloned()
    }

    fn update_job(&self, id: &str, f: impl FnOnce(&mut UploadJob)) {
        if let Some(j) = self.files().inner().jobs.iter_mut().find(|j| j.id == id) {
            f(j);
        }
    }

    /// The topic id the views use for a job's message.
    fn job_view_topic(job: &UploadJob) -> String {
        job.topic_id
            .clone()
            .unwrap_or_else(|| convert::pending_topic_id(&job.id))
    }

    fn run_job(&self, id: &str) {
        let run = {
            let mut i = self.files().inner();
            let Some(j) = i.jobs.iter_mut().find(|j| j.id == id) else {
                return;
            };
            if j.running {
                return;
            }
            j.running = true;
            j.failed = false;
            j.run += 1;
            j.run
        };
        let this = self.clone();
        let id = id.to_string();
        chat_core::background().spawn(async move {
            let result = this.job_body(&id, run).await;
            let Some(job) = this.job(&id) else { return };
            if job.run != run {
                return;
            }
            let view_topic = Self::job_view_topic(&job);
            match result {
                Ok(()) => {}
                Err(error) => {
                    tracing::warn!(?error, "upload job failed");
                    this.update_job(&id, |j| {
                        j.failed = true;
                        j.running = false;
                    });
                    this.emit(vec![
                        Change::Messages {
                            group_id: job.group_id.clone(),
                            topic_id: view_topic,
                        },
                        Change::Topics {
                            group_id: job.group_id.clone(),
                        },
                    ]);
                }
            }
        });
    }

    async fn job_body(&self, id: &str, run: u64) -> Result<(), FileError> {
        let current = |this: &Self| this.job(id).filter(|j| j.run == run);
        let job = current(self).ok_or(FileError::Other)?;
        let account = self.account().ok_or(FileError::SignedOut)?;
        let view_topic = Self::job_view_topic(&job);
        let group_id = job.group_id.clone();
        for (index, file) in job.files.iter().enumerate() {
            if file.uploaded.is_some() {
                continue;
            }
            let key = upload_key(id, index);
            self.set_transfer(&key, TransferState::Running { done: 0, total: file.size }, Some((&group_id, &view_topic)));
            let (path, temp) = match &file.source {
                NewFile::Path(p) => (p.clone(), false),
                NewFile::Image {
                    extension, bytes, ..
                } => {
                    let dir = account.dir.join("uploads");
                    let path = dir.join(format!("{id}-{index}.{extension}"));
                    let bytes = bytes.clone();
                    let p = path.clone();
                    let written = super::tokio_blocking(move || {
                        std::fs::create_dir_all(&dir)?;
                        std::fs::write(&p, &bytes)
                    })
                    .await;
                    if written.is_err() {
                        self.set_transfer(&key, TransferState::Failed(FileError::Unreadable), Some((&group_id, &view_topic)));
                        return Err(FileError::Unreadable);
                    }
                    (path, true)
                }
            };
            let p = path.clone();
            if let Err(error) = super::tokio_blocking(move || check_local_file(&p)).await {
                self.set_transfer(&key, TransferState::Failed(error), Some((&group_id, &view_topic)));
                return Err(error);
            }
            // A pending topic has no server id yet; the file belongs to the group.
            let topic_for_upload = job
                .topic_id
                .as_deref()
                .map(|t| self.resolve_topic(t))
                .filter(|t| convert::pending_cid(t).is_none());
            let progress = {
                let this = self.clone();
                let key = key.clone();
                let (g, t) = (group_id.clone(), view_topic.clone());
                Arc::new(move |p: Progress| {
                    use chat_core::files::Phase;
                    // Hashing reads the file once before the upload: show it as the first half.
                    let (done, total) = match p.phase {
                        Phase::Hashing => (p.done / 2, p.total),
                        _ => (p.total / 2 + p.done / 2, p.total),
                    };
                    this.set_transfer(&key, TransferState::Running { done, total }, Some((&g, &t)));
                }) as chat_core::files::ProgressFn
            };
            let handle = account.client.upload_file(
                UploadRequest {
                    group_id: group_id.clone(),
                    topic_id: topic_for_upload,
                    path: path.clone(),
                    name: Some(file.name.clone()),
                    mime: Some(file.mime.clone()).filter(|m| !m.is_empty()),
                },
                progress,
            );
            let result = handle.await.map_err(|_| FileError::Other)?;
            if temp {
                let _ = std::fs::remove_file(&path);
            }
            if current(self).is_none() {
                return Ok(());
            }
            match result {
                Ok(meta) => {
                    let fref = meta.to_file_ref();
                    {
                        let mut i = self.files().inner();
                        i.missing.remove(&fref.sha256);
                    }
                    self.update_job(id, |j| j.files[index].uploaded = Some(fref));
                    self.set_transfer(&key, TransferState::Done, Some((&group_id, &view_topic)));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "upload failed");
                    let error = file_error(&e);
                    self.set_transfer(&key, TransferState::Failed(error), Some((&group_id, &view_topic)));
                    return Err(error);
                }
            }
        }
        // Every file is on the server. A new topic I started before may still
        // wait for its ack: hold the message until the topic has an id.
        let topic_id = loop {
            let job = current(self).ok_or(FileError::Other)?;
            if job.generation != self.generation() {
                return Err(FileError::SignedOut);
            }
            match &job.topic_id {
                None => break None,
                Some(t) => {
                    let real = self.resolve_topic(t);
                    if convert::pending_cid(&real).is_none() {
                        break Some(real);
                    }
                    // The pending topic may have failed and been discarded.
                    if self.topic(&group_id, t).is_none() {
                        return Err(FileError::NotFound);
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        let job = current(self).ok_or(FileError::Other)?;
        let attachments: Vec<proto::FileRef> = job.files.iter().filter_map(|f| f.uploaded.clone()).collect();
        let title = if topic_id.is_none() {
            Some(convert::topic_title(job.title.as_deref(), &job.body))
                .filter(|t| !t.is_empty())
                .or_else(|| attachments.first().map(|f| f.name.chars().take(80).collect()))
        } else {
            None
        };
        let sent = account.client.send(chat_core::SendRequest {
            group_id: group_id.clone(),
            topic_id: topic_id.clone(),
            title,
            body: job.body.clone(),
            mentions: job.mentions.clone(),
            reply_to: job.quote_of.clone(),
            attachments: Some(attachments),
            ..Default::default()
        });
        match sent {
            Ok(cid) => {
                let view_topic = Self::job_view_topic(&job);
                {
                    let mut s = self.state();
                    if topic_id.is_none() {
                        // The job's pending topic now follows the queued send.
                        s.aliases
                            .insert(view_topic.clone(), convert::pending_topic_id(&cid));
                    }
                }
                {
                    let mut i = self.files().inner();
                    i.jobs.retain(|j| j.id != id);
                    for n in 0..job.files.len() {
                        i.transfers.remove(&upload_key(id, n));
                    }
                }
                let real_topic = topic_id.unwrap_or_else(|| convert::pending_topic_id(&cid));
                self.emit(vec![
                    Change::Messages {
                        group_id: group_id.clone(),
                        topic_id: view_topic,
                    },
                    Change::Messages {
                        group_id: group_id.clone(),
                        topic_id: real_topic,
                    },
                    Change::Topics {
                        group_id: group_id.clone(),
                    },
                    Change::Groups,
                ]);
                Ok(())
            }
            Err(e) => {
                tracing::warn!(error = %e, "send with files refused");
                Err(file_error(&e))
            }
        }
    }

    /// True when `client_msg_id` is an upload job (then retry and discard
    /// act on it).
    pub(super) fn retry_job(&self, id: &str) -> bool {
        let Some(job) = self.job(id) else {
            return false;
        };
        if job.running {
            return true;
        }
        for (n, f) in job.files.iter().enumerate() {
            if f.uploaded.is_none() {
                self.files()
                    .inner()
                    .transfers
                    .insert(upload_key(id, n), TransferState::Queued);
            }
        }
        self.update_job(id, |j| j.failed = false);
        self.emit(vec![Change::Messages {
            group_id: job.group_id.clone(),
            topic_id: Self::job_view_topic(&job),
        }]);
        self.run_job(id);
        true
    }

    pub(super) fn discard_job(&self, id: &str) -> bool {
        let job = {
            let mut i = self.files().inner();
            let Some(pos) = i.jobs.iter().position(|j| j.id == id) else {
                return false;
            };
            let job = i.jobs.remove(pos);
            for n in 0..job.files.len() {
                i.transfers.remove(&upload_key(id, n));
            }
            job
        };
        self.emit(vec![
            Change::Messages {
                group_id: job.group_id.clone(),
                topic_id: Self::job_view_topic(&job),
            },
            Change::Topics {
                group_id: job.group_id,
            },
        ]);
        true
    }

    /// My messages that wait for uploads into this topic (`view_topic` is
    /// the id the view asked for, `real` the resolved one).
    pub(super) fn job_messages(&self, group_id: &str, view_topic: &str, real: &str, me: &str) -> Vec<Message> {
        let i = self.files().inner();
        i.jobs
            .iter()
            .filter(|j| j.group_id == group_id && j.generation == self.generation())
            .filter(|j| {
                let t = Self::job_view_topic(j);
                t == view_topic || t == real || self.resolve_topic(&t) == real
            })
            .map(|j| job_message(j, view_topic, me))
            .collect()
    }

    /// New topics that wait for their uploads, newest first.
    pub(super) fn job_topics(&self, group_id: &str, me: &str) -> Vec<Topic> {
        let i = self.files().inner();
        let mut out: Vec<Topic> = i
            .jobs
            .iter()
            .filter(|j| j.group_id == group_id && j.topic_id.is_none())
            .map(|j| job_topic(j, me))
            .collect();
        out.reverse();
        out
    }

    /// The pending topic of a job, when `cid` is one.
    pub(super) fn job_topic_of(&self, cid: &str, me: &str) -> Option<Topic> {
        let i = self.files().inner();
        i.jobs
            .iter()
            .find(|j| j.id == cid && j.topic_id.is_none())
            .map(|j| job_topic(j, me))
    }

    // ------------------------------------------------------------------
    // Downloads and the cache
    // ------------------------------------------------------------------

    pub(super) fn file_transfer_inner(&self, key: &str) -> Option<TransferState> {
        self.files().inner().transfers.get(key).copied()
    }

    pub(super) fn cached_file_inner(&self, file: &FileAttachment) -> Option<PathBuf> {
        if !file.is_uploaded() || file.sha256.is_empty() {
            return None;
        }
        {
            let mut i = self.files().inner();
            if let Some(p) = i.cached.get(&file.sha256) {
                if p.is_file() {
                    return Some(p.clone());
                }
                i.cached.remove(&file.sha256);
            }
            if i.missing.contains(&file.sha256) {
                return None;
            }
        }
        let a = self.account()?;
        let found = a.client.cached_file(&convert::to_file_ref(file));
        let mut i = self.files().inner();
        match &found {
            Some(p) => {
                i.cached.insert(file.sha256.clone(), p.clone());
            }
            None => {
                i.missing.insert(file.sha256.clone());
            }
        }
        found
    }

    /// The thumbnail of a cached image; makes it in the background the first time.
    pub(super) fn thumbnail_inner(&self, group_id: &str, topic_id: &str, file: &FileAttachment) -> Option<PathBuf> {
        if !file.is_image() {
            return None;
        }
        if let Some(t) = self.files().inner().thumbs.get(&file.sha256)
            && t.is_file()
        {
            return Some(t.clone());
        }
        let source = self.cached_file_inner(file)?;
        let a = self.account()?;
        let dest = a.dir.join("thumbs").join(format!("{}.png", file.sha256));
        let key = format!("thumb:{}", file.sha256);
        {
            let mut i = self.files().inner();
            if i.transfers.contains_key(&key) {
                return None;
            }
            i.transfers.insert(key.clone(), TransferState::Queued);
        }
        let this = self.clone();
        let (sha, g, t) = (file.sha256.clone(), group_id.to_string(), topic_id.to_string());
        std::thread::spawn(move || {
            let made = make_thumbnail(&source, &dest);
            let mut i = this.files().inner();
            match made {
                Some(p) => {
                    i.thumbs.insert(sha, p);
                    i.transfers.remove(&key);
                }
                None => {
                    i.transfers.insert(key, TransferState::Failed(FileError::Other));
                }
            }
            drop(i);
            this.emit(vec![Change::Messages {
                group_id: g,
                topic_id: t,
            }]);
        });
        None
    }

    pub(super) fn fetch_file_inner(&self, group_id: &str, topic_id: &str, file: &FileAttachment) -> Reply<PathBuf> {
        let (tx, rx) = oneshot::channel();
        if !file.is_uploaded() {
            let _ = tx.send(Err(FileError::NotFound));
            return rx;
        }
        if let Some(p) = self.cached_file_inner(file) {
            let _ = tx.send(Ok(p));
            return rx;
        }
        let Some(a) = self.account() else {
            let _ = tx.send(Err(FileError::SignedOut));
            return rx;
        };
        let sha = file.sha256.clone();
        let key = download_key(file);
        let start = {
            let mut i = self.files().inner();
            let start = !i.downloads.contains_key(&sha);
            let d = i.downloads.entry(sha.clone()).or_default();
            d.waiters.push(tx);
            d.topics.insert((group_id.to_string(), topic_id.to_string()));
            start
        };
        if !start {
            return rx;
        }
        self.set_transfer(&key, TransferState::Running { done: 0, total: file.size }, Some((group_id, topic_id)));
        let this = self.clone();
        let progress = {
            let this = self.clone();
            let key = key.clone();
            let (g, t) = (group_id.to_string(), topic_id.to_string());
            Arc::new(move |p: Progress| {
                this.set_transfer(&key, TransferState::Running { done: p.done, total: p.total }, Some((&g, &t)));
            }) as chat_core::files::ProgressFn
        };
        let handle = a.client.download_file(convert::to_file_ref(file), progress);
        chat_core::background().spawn(async move {
            let result = match handle.await {
                Ok(Ok(p)) => Ok(p),
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "download failed");
                    Err(file_error(&e))
                }
                Err(_) => Err(FileError::Other),
            };
            let download = {
                let mut i = this.files().inner();
                i.missing.remove(&sha);
                if let Ok(p) = &result {
                    i.cached.insert(sha.clone(), p.clone());
                }
                i.downloads.remove(&sha).unwrap_or_default()
            };
            let state = match &result {
                Ok(_) => TransferState::Done,
                Err(e) => TransferState::Failed(*e),
            };
            {
                let mut i = this.files().inner();
                i.transfers.insert(key.clone(), state);
                i.last_emit.remove(&key);
            }
            for w in download.waiters {
                let _ = w.send(result.clone());
            }
            this.emit(
                download
                    .topics
                    .into_iter()
                    .map(|(group_id, topic_id)| Change::Messages { group_id, topic_id })
                    .collect(),
            );
        });
        rx
    }

    pub(super) fn topic_files_inner(&self, topic_id: &str) -> Vec<TopicFile> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let real = self.resolve_topic(topic_id);
        if convert::pending_cid(&real).is_some() {
            return Vec::new();
        }
        let msgs = a
            .client
            .messages(&real, None, TOPIC_FILES_SCAN)
            .unwrap_or_default();
        let mut out = Vec::new();
        for m in msgs.iter().rev() {
            if m.deleted_at().is_some() {
                continue;
            }
            for f in m.attachments.as_deref().unwrap_or_default() {
                out.push(TopicFile {
                    file: convert::file_ref(f),
                    message_id: m.id.clone(),
                    author_id: m.author_id.clone(),
                    created_at: m.created_at.min(i64::MAX as u64) as i64,
                });
            }
        }
        out
    }

    // ------------------------------------------------------------------
    // 产出
    // ------------------------------------------------------------------

    pub(super) fn artifacts_inner(&self, topic_id: &str) -> Vec<ArtifactSummary> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let real = self.resolve_topic(topic_id);
        if convert::pending_cid(&real).is_some() {
            return Vec::new();
        }
        a.client
            .artifacts(&real)
            .unwrap_or_default()
            .iter()
            .map(|x| artifact_summary(x, topic_id))
            .collect()
    }

    pub(super) fn refresh_artifacts_inner(&self, group_id: &str, topic_id: &str) {
        let real = self.resolve_topic(topic_id);
        if convert::pending_cid(&real).is_some() {
            return;
        }
        let Some(a) = self.account() else { return };
        if !self.files().inner().refreshed.insert(real.clone()) {
            return;
        }
        let handle = a.client.refresh_artifacts(group_id, Some(&real));
        let this = self.clone();
        chat_core::background().spawn(async move {
            if let Ok(Err(e)) = handle.await {
                tracing::warn!(error = %e, "artifact list failed");
                this.files().inner().refreshed.remove(&real);
            }
        });
    }

    pub(super) fn artifact_inner(&self, artifact_id: &str) -> Option<ArtifactDetail> {
        let a = self.account()?;
        let row = a.client.artifact(artifact_id).ok().flatten()?;
        Some(ArtifactDetail {
            summary: artifact_summary(&row.artifact, &row.artifact.topic_id),
            group_id: row.artifact.group_id.clone(),
            versions: row
                .versions
                .iter()
                .map(|v| ArtifactVersionInfo {
                    version: v.version,
                    note: v.note.clone(),
                    created_by: v.created_by.clone(),
                    created_at: v.created_at.min(i64::MAX as u64) as i64,
                })
                .collect(),
        })
    }

    pub(super) fn request_artifact_inner(&self, group_id: &str, topic_id: &str, artifact_id: &str) {
        let Some(a) = self.account() else { return };
        if !self.files().inner().requested.insert(artifact_id.to_string()) {
            return;
        }
        let handle = a.client.load_artifact(artifact_id);
        let this = self.clone();
        let (g, t, id) = (group_id.to_string(), topic_id.to_string(), artifact_id.to_string());
        chat_core::background().spawn(async move {
            match handle.await {
                Ok(Ok(row)) => {
                    let real_topic = row.artifact.topic_id.clone();
                    this.emit(vec![
                        Change::Messages {
                            group_id: g.clone(),
                            topic_id: t,
                        },
                        Change::Artifacts {
                            group_id: g,
                            topic_id: real_topic,
                        },
                    ]);
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "artifact load failed");
                    // Not found stays asked; a network error can try again later.
                    if file_error(&e) != FileError::NotFound {
                        this.files().inner().requested.remove(&id);
                    }
                }
                Err(_) => {
                    this.files().inner().requested.remove(&id);
                }
            }
        });
    }

    /// Whether this version needs a fresh load (a version newer than the cache knows).
    fn artifact_version_known(&self, artifact_id: &str, version: Option<u32>) -> bool {
        match (version, self.artifact_inner(artifact_id)) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(v), Some(d)) => d.versions.iter().any(|x| x.version == v),
        }
    }

    pub(super) fn open_artifact_version_inner(&self, artifact_id: &str, version: Option<u32>) -> Reply<ArtifactFolder> {
        let (tx, rx) = oneshot::channel();
        let Some(a) = self.account() else {
            let _ = tx.send(Err(FileError::SignedOut));
            return rx;
        };
        let known = self.artifact_version_known(artifact_id, version);
        let id = artifact_id.to_string();
        chat_core::background().spawn(async move {
            let result = async {
                if !known {
                    a.client
                        .load_artifact(&id)
                        .await
                        .map_err(|_| FileError::Other)?
                        .map_err(|e| file_error(&e))?;
                }
                let is_page = a
                    .client
                    .artifact(&id)
                    .ok()
                    .flatten()
                    .is_none_or(|r| r.artifact.kind == PKind::Html);
                let folder = a
                    .client
                    .open_artifact_version(&id, version, chat_core::no_progress())
                    .await
                    .map_err(|_| FileError::Other)?
                    .map_err(|e| file_error(&e))?;
                let entry = folder
                    .entry
                    .strip_prefix(&folder.dir)
                    .map_err(|_| FileError::Other)?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                Ok(ArtifactFolder {
                    artifact_id: folder.artifact_id,
                    version: folder.version,
                    dir: folder.dir,
                    entry,
                    is_page,
                })
            }
            .await;
            let _ = tx.send(result);
        });
        rx
    }

    pub(super) fn create_artifact_inner(&self, request: NewArtifactRequest) -> Reply<CreatedArtifact> {
        let (tx, rx) = oneshot::channel();
        let Some(a) = self.account() else {
            let _ = tx.send(Err(FileError::SignedOut));
            return rx;
        };
        let real = self.resolve_topic(&request.topic_id);
        if convert::pending_cid(&real).is_some() {
            let _ = tx.send(Err(FileError::Other));
            return rx;
        }
        let key = create_key(&request.topic_id);
        let (g, view_topic) = (request.group_id.clone(), request.topic_id.clone());
        self.set_transfer(&key, TransferState::Running { done: 0, total: 0 }, Some((&g, &view_topic)));
        let this = self.clone();
        chat_core::background().spawn(async move {
            let result = async {
                let path = request.path.clone();
                let (kind, files, entry, default_title) =
                    super::tokio_blocking(move || plan_artifact(&path)).await?;
                let total: u64 = files
                    .iter()
                    .map(|f| std::fs::metadata(&f.local).map(|m| m.len()).unwrap_or(0))
                    .sum();
                let progress = {
                    let this = this.clone();
                    let key = key.clone();
                    let (g, t) = (g.clone(), view_topic.clone());
                    let done = Arc::new(Mutex::new((0u64, 0u64, 0u64)));
                    Arc::new(move |p: Progress| {
                        // Progress reports each file in turn: add up finished files.
                        let mut d = lock(&done);
                        if p.done < d.1 || p.total != d.2 {
                            d.0 += d.2;
                        }
                        d.1 = p.done;
                        d.2 = p.total;
                        let sum = (d.0 + p.done).min(total);
                        drop(d);
                        this.set_transfer(&key, TransferState::Running { done: sum, total }, Some((&g, &t)));
                    }) as chat_core::files::ProgressFn
                };
                let is_new = request.artifact_id.is_none();
                let r = a
                    .client
                    .create_artifact(
                        NewVersion {
                            group_id: request.group_id.clone(),
                            topic_id: real.clone(),
                            title: is_new.then(|| default_title.clone()),
                            kind,
                            entry,
                            files,
                            note: None,
                            artifact_id: request.artifact_id.clone(),
                        },
                        progress,
                    )
                    .await
                    .map_err(|_| FileError::Other)?
                    .map_err(|e| file_error(&e))?;
                let title = r.artifact.title.clone();
                let version = r.version.version;
                let body = if is_new {
                    fill(ca::NEW_BODY, &[("title", &title)])
                } else {
                    fill(ca::VERSION_BODY, &[("title", &title), ("n", &version.to_string())])
                };
                // The topic keeps a message that points to the 产出.
                if let Err(e) = a.client.send(chat_core::SendRequest {
                    group_id: request.group_id.clone(),
                    topic_id: Some(real.clone()),
                    body,
                    artifact_refs: Some(vec![proto::ArtifactRef {
                        artifact_id: r.artifact.id.clone(),
                        version,
                    }]),
                    ..Default::default()
                }) {
                    tracing::warn!(error = %e, "artifact message refused");
                }
                Ok(CreatedArtifact {
                    artifact_id: r.artifact.id.clone(),
                    version,
                    title,
                })
            }
            .await;
            let state = match &result {
                Ok(_) => TransferState::Done,
                Err(e) => TransferState::Failed(*e),
            };
            this.set_transfer(&key, state, Some((&g, &view_topic)));
            this.files().inner().transfers.remove(&key);
            this.emit(vec![Change::Artifacts {
                group_id: g.clone(),
                topic_id: view_topic.clone(),
            }]);
            let _ = tx.send(result);
        });
        rx
    }
}

fn job_file(f: &PendingFile) -> JobFile {
    match &f.source {
        NewFile::Path(p) => {
            let mime = chat_core::files::guess_mime(&f.name).to_string();
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            JobFile {
                name: f.name.clone(),
                mime,
                size: f.size_bytes.unwrap_or(size),
                width: None,
                height: None,
                source: f.source.clone(),
                uploaded: None,
            }
        }
        NewFile::Image {
            mime,
            extension,
            bytes,
        } => {
            let size = chat_core::files::image_size_of(bytes);
            let name = if f.name.contains('.') {
                f.name.clone()
            } else {
                format!("{}.{extension}", f.name)
            };
            JobFile {
                name,
                mime: mime.clone(),
                size: bytes.len() as u64,
                width: size.map(|s| s.0),
                height: size.map(|s| s.1),
                source: f.source.clone(),
                uploaded: None,
            }
        }
    }
}

fn job_message(j: &UploadJob, view_topic: &str, me: &str) -> Message {
    Message {
        id: String::new(),
        client_msg_id: j.id.clone(),
        group_id: j.group_id.clone(),
        topic_id: view_topic.to_string(),
        author_id: me.to_string(),
        body: j.body.clone(),
        mentions: j.mentions.clone(),
        created_seq: 0,
        created_at: j.created_at,
        edited_at: None,
        deleted_at: None,
        streaming: false,
        send_state: if j.failed {
            SendState::Failed
        } else {
            SendState::Pending
        },
        quote_of: j.quote_of.clone(),
        attachments: j
            .files
            .iter()
            .map(|f| match &f.uploaded {
                Some(r) => convert::file_ref(r),
                None => FileAttachment {
                    file_id: String::new(),
                    name: f.name.clone(),
                    size: f.size,
                    mime: f.mime.clone(),
                    width: f.width,
                    height: f.height,
                    sha256: String::new(),
                },
            })
            .collect(),
        artifact_refs: Vec::new(),
    }
}

fn job_topic(j: &UploadJob, me: &str) -> Topic {
    let title = convert::topic_title(j.title.as_deref(), &j.body);
    let title = if title.is_empty() {
        j.files.first().map(|f| f.name.chars().take(80).collect()).unwrap_or_default()
    } else {
        title
    };
    Topic {
        id: convert::pending_topic_id(&j.id),
        group_id: j.group_id.clone(),
        title,
        status: TopicStatus::Open,
        waiting_on: Vec::new(),
        owner_id: None,
        created_by: me.to_string(),
        created_at: j.created_at,
        root_message_id: String::new(),
        last_seq: 0,
        last_activity_at: j.created_at,
        message_count: 1,
        unread: 0,
        mentions: 0,
    }
}

fn artifact_summary(a: &proto::Artifact, view_topic: &str) -> ArtifactSummary {
    ArtifactSummary {
        id: a.id.clone(),
        topic_id: view_topic.to_string(),
        kind: match a.kind {
            PKind::Html => ArtifactKind::Page,
            PKind::File => ArtifactKind::File,
        },
        title: a.title.clone(),
        version: a.current_version,
        size_bytes: None,
        created_by: a.created_by.clone(),
        updated_at: a.updated_at.min(i64::MAX as u64) as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_html_title() {
        assert_eq!(
            html_title("<html><head><TITLE>\n 周报 &amp; 计划 </title></head>").as_deref(),
            Some("周报 & 计划")
        );
        assert_eq!(html_title("<title></title>"), None);
        assert_eq!(html_title("<p>no title</p>"), None);
    }

    #[test]
    fn plans_a_folder_with_its_page_and_title() {
        let dir = std::env::temp_dir().join(format!("tc-plan-{}", chat_core::new_id()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<title>原型</title>").unwrap();
        std::fs::write(dir.join("assets/app.js"), "1").unwrap();
        std::fs::write(dir.join(".hidden"), "x").unwrap();
        let (kind, files, entry, title) = plan_artifact(&dir).unwrap();
        assert_eq!(kind, PKind::Html);
        assert_eq!(entry, "index.html");
        assert_eq!(title, "原型");
        let mut paths: Vec<_> = files.iter().map(|f| f.path.clone()).collect();
        paths.sort();
        assert_eq!(paths, ["assets/app.js", "index.html"]);
        assert_eq!(
            files.iter().find(|f| f.path == "index.html").unwrap().mime.as_deref(),
            Some("text/html")
        );

        let empty = dir.join("assets");
        assert_eq!(plan_artifact(&empty).unwrap_err(), FileError::NoPage);
        let (kind, _, entry, title) = plan_artifact(&dir.join("assets/app.js")).unwrap();
        assert_eq!((kind, entry.as_str(), title.as_str()), (PKind::File, "app.js", "app.js"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_folders_and_missing_files() {
        let dir = std::env::temp_dir();
        assert_eq!(check_local_file(&dir), Err(FileError::NotAFile));
        assert_eq!(
            check_local_file(&dir.join("tc-no-such-file")),
            Err(FileError::Unreadable)
        );
    }

    #[test]
    fn makes_a_small_thumbnail() {
        let dir = std::env::temp_dir().join(format!("tc-thumb-{}", chat_core::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.png");
        image::RgbImage::from_pixel(2000, 1000, image::Rgb([200, 10, 10]))
            .save(&big)
            .unwrap();
        let thumb = make_thumbnail(&big, &dir.join("t/big.png")).unwrap();
        assert_eq!(chat_core::files::image_size(&thumb), Some((THUMB_MAX_PX, 320)));
        let small = dir.join("small.png");
        image::RgbImage::from_pixel(10, 10, image::Rgb([0, 0, 0])).save(&small).unwrap();
        assert_eq!(make_thumbnail(&small, &dir.join("t/small.png")), Some(small));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
