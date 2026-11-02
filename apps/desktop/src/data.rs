//! The provider trait and the plain data types that views render.
//!
//! Views never talk to the network or the cache directly. They hold an
//! `Arc<dyn ChatProvider>`, read snapshots with the synchronous getters, start
//! work with the command methods (which return at once), and learn about new
//! state from [`Change`] batches (see [`ChatProvider::subscribe`] and
//! `app::ChangeHub`). No method blocks the UI thread and none is async.
//!
//! The types follow `docs/protocol.md` but stay independent of the protocol
//! crates, so the UI builds without them. A real provider (chat-core) maps the
//! wire types to these.

use std::{fmt, path::PathBuf, sync::Arc};

use futures::channel::mpsc::UnboundedReceiver;

pub type MemberId = String;
pub type GroupId = String;
pub type TopicId = String;
pub type MessageId = String;
/// Made by the client for every send; the server deduplicates on it.
pub type ClientMsgId = String;
/// Milliseconds since the Unix epoch, UTC.
pub type Millis = i64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemberKind {
    Human,
    Bot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemberLevel {
    Owner,
    Member,
}

/// A person or a bot. Both are equal members.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub id: MemberId,
    pub kind: MemberKind,
    pub name: String,
    /// 岗位: the member's job, shown under the name.
    pub role: Option<String>,
    pub level: MemberLevel,
    pub avatar_url: Option<String>,
    pub deactivated: bool,
    /// Presence, from transient `presence` events.
    pub online: bool,
    /// The jojo-friends character the avatar draws, when set (`characters`).
    pub character: Option<crate::characters::CharacterId>,
}

impl Member {
    pub fn is_bot(&self) -> bool {
        self.kind == MemberKind::Bot
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupKind {
    Group,
    /// A group of exactly two members.
    Dm,
}

/// One row of the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupSummary {
    pub id: GroupId,
    pub kind: GroupKind,
    /// For a DM, the other member's name.
    pub name: String,
    pub member_ids: Vec<MemberId>,
    /// For a DM, the other member (avatar and presence dot).
    pub dm_peer: Option<MemberId>,
    pub last_seq: u64,
    pub read_seq: u64,
    pub unread: u32,
    pub mentions: u32,
    /// Sort key of the sidebar: newest first.
    pub last_activity_at: Millis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TopicStatus {
    Open,
    InProgress,
    WaitingAnswer,
    WaitingReview,
    Blocked,
    Done,
}

impl TopicStatus {
    pub const ALL: [TopicStatus; 6] = [
        TopicStatus::Open,
        TopicStatus::InProgress,
        TopicStatus::WaitingAnswer,
        TopicStatus::WaitingReview,
        TopicStatus::Blocked,
        TopicStatus::Done,
    ];

    /// The wire name (`docs/protocol.md`).
    pub fn wire_name(self) -> &'static str {
        match self {
            TopicStatus::Open => "open",
            TopicStatus::InProgress => "in_progress",
            TopicStatus::WaitingAnswer => "waiting_answer",
            TopicStatus::WaitingReview => "waiting_review",
            TopicStatus::Blocked => "blocked",
            TopicStatus::Done => "done",
        }
    }
}

/// One row of the topic list.
#[derive(Clone, Debug, PartialEq)]
pub struct Topic {
    pub id: TopicId,
    pub group_id: GroupId,
    pub title: String,
    pub status: TopicStatus,
    /// Who the topic waits on (drives the status chip and 待办).
    pub waiting_on: Vec<MemberId>,
    /// The one responsible member, when set.
    pub owner_id: Option<MemberId>,
    pub created_by: MemberId,
    pub created_at: Millis,
    pub root_message_id: MessageId,
    pub last_seq: u64,
    pub last_activity_at: Millis,
    pub message_count: u32,
    /// Unread messages in this topic for me (read positions are per topic).
    pub unread: u32,
    pub mentions: u32,
}

/// Delivery state of a message I wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendState {
    /// Acknowledged by the server (or written by someone else).
    Sent,
    /// Waiting for the ack; resent with the same `client_msg_id` after a reconnect.
    Pending,
    /// Rejected by the server; the user can retry or discard.
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    /// Empty while the message is pending (no server id yet).
    pub id: MessageId,
    pub client_msg_id: ClientMsgId,
    pub group_id: GroupId,
    pub topic_id: TopicId,
    pub author_id: MemberId,
    /// Markdown. Empty when deleted.
    pub body: String,
    pub mentions: Vec<MemberId>,
    /// 0 while pending.
    pub created_seq: u64,
    pub created_at: Millis,
    pub edited_at: Option<Millis>,
    pub deleted_at: Option<Millis>,
    /// True while a bot is still writing it.
    pub streaming: bool,
    pub send_state: SendState,
    /// The message this one quotes, if any.
    pub quote_of: Option<MessageId>,
    /// Files attached to the message. For my message that still uploads,
    /// `file_id` is empty and [`ChatProvider::file_transfer`] has the progress.
    pub attachments: Vec<FileAttachment>,
    /// 产出 the message points to.
    pub artifact_refs: Vec<ArtifactRef>,
    /// Who reacted with which emoji, in the order the emoji were first used.
    pub reactions: Vec<Reaction>,
}

/// Adds (`on`) or removes `member` under `emoji`: one reaction per emoji per
/// member; a new emoji goes last and an emoji with nobody left goes away.
pub fn apply_reaction(list: &mut Vec<Reaction>, emoji: &str, member: &str, on: bool) {
    let ix = list.iter().position(|r| r.emoji == emoji);
    match (ix, on) {
        (Some(i), true) => {
            if !list[i].member_ids.iter().any(|m| m == member) {
                list[i].member_ids.push(member.to_string());
            }
        }
        (None, true) => list.push(Reaction {
            emoji: emoji.to_string(),
            member_ids: vec![member.to_string()],
        }),
        (Some(i), false) => {
            list[i].member_ids.retain(|m| m != member);
            if list[i].member_ids.is_empty() {
                list.remove(i);
            }
        }
        (None, false) => {}
    }
}

/// One emoji under a message and who reacted with it, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reaction {
    pub emoji: String,
    pub member_ids: Vec<MemberId>,
}

/// Largest file one message or 产出 can carry (`docs/protocol.md`).
pub const MAX_FILE_BYTES: u64 = 100_000_000;

/// A file attached to a message (the wire `FileRef`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileAttachment {
    /// Empty while the file still uploads.
    pub file_id: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Lowercase hex; empty while the file still uploads.
    pub sha256: String,
}

impl FileAttachment {
    /// An image the timeline can show inline (the server serves these inline too).
    pub fn is_image(&self) -> bool {
        matches!(
            self.mime.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        )
    }

    /// True when the file is on the server (not my upload in progress).
    pub fn is_uploaded(&self) -> bool {
        !self.file_id.is_empty()
    }
}

/// A 产出 version a message points to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub version: u32,
}

/// Why a file transfer or a 产出 action did not work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileError {
    /// Larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// A folder, or not a regular file.
    NotAFile,
    /// The local file cannot be read (gone, no permission).
    Unreadable,
    /// The server cannot be reached, or the transfer broke off.
    Network,
    /// The downloaded bytes do not match the file's checksum.
    Checksum,
    /// The server refused it (not allowed, not valid).
    Rejected,
    /// The file or 产出 does not exist (or I am not in its 群).
    NotFound,
    /// No HTML page in the picked folder, or too many files.
    NoPage,
    /// Not signed in.
    SignedOut,
    Other,
}

/// State of one upload or download, for progress bars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    /// Waiting for its turn.
    Queued,
    Running { done: u64, total: u64 },
    Done,
    Failed(FileError),
}

impl TransferState {
    /// Progress from 0 to 100, for `Progress::value`.
    pub fn percent(&self) -> f32 {
        match *self {
            TransferState::Running { done, total } if total > 0 => {
                (done as f64 * 100.0 / total as f64).clamp(0.0, 100.0) as f32
            }
            TransferState::Done => 100.0,
            _ => 0.0,
        }
    }
}

/// Key of the upload of file `index` of my unsent message `client_msg_id`.
pub fn upload_key(client_msg_id: &str, index: usize) -> String {
    format!("up:{client_msg_id}:{index}")
}

/// Key of the download of a file.
pub fn download_key(file: &FileAttachment) -> String {
    format!("down:{}", file.sha256)
}

/// Key of the 产出 upload running in a topic (the side panel's progress).
pub fn create_key(topic_id: &str) -> String {
    format!("create:{topic_id}")
}

/// A file attached to a message of a topic, for the side panel.
#[derive(Clone, Debug, PartialEq)]
pub struct TopicFile {
    pub file: FileAttachment,
    pub message_id: MessageId,
    pub author_id: MemberId,
    pub created_at: Millis,
}

/// One version of a 产出.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactVersionInfo {
    pub version: u32,
    pub note: Option<String>,
    pub created_by: MemberId,
    pub created_at: Millis,
}

/// A 产出 with its known versions, newest first.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactDetail {
    pub summary: ArtifactSummary,
    pub group_id: GroupId,
    pub versions: Vec<ArtifactVersionInfo>,
}

/// A 产出 version on disk, ready to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactFolder {
    pub artifact_id: String,
    pub version: u32,
    pub dir: PathBuf,
    /// Path of the entry file relative to `dir`, joined by `/`.
    pub entry: String,
    /// An HTML page (open in the 产出 window), or a single file (open with
    /// the default app).
    pub is_page: bool,
}

/// A new 产出, or a new version of one, from a local file or folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewArtifactRequest {
    pub group_id: GroupId,
    pub topic_id: TopicId,
    /// An HTML file, a folder with an HTML page, or any single file.
    pub path: PathBuf,
    /// `Some` adds a version to this 产出; `None` makes a new 产出.
    pub artifact_id: Option<String>,
}

/// A 产出 the app made: its id and version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatedArtifact {
    pub artifact_id: String,
    pub version: u32,
    pub title: String,
}

/// A file the composer could not take, with the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedFile {
    pub name: String,
    pub error: FileError,
}

/// The answer to a background request; `Err(Canceled)` when the provider
/// cannot do it.
pub type Reply<T> = futures::channel::oneshot::Receiver<Result<T, FileError>>;

/// A reply that fails at once with `error`.
pub fn reply_err<T>(error: FileError) -> Reply<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let _ = tx.send(Err(error));
    rx
}

/// A window of the newest messages of a topic, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MessageWindow {
    pub messages: Vec<Message>,
    /// More messages exist before `messages[0]`; call `load_older` to get them.
    pub has_older: bool,
    /// A `load_older` request is running.
    pub loading_older: bool,
}

/// A 待办 entry: a topic that waits on me, or one with unread mentions of me.
#[derive(Clone, Debug, PartialEq)]
pub struct TodoItem {
    pub group_id: GroupId,
    pub group_name: String,
    pub topic: Topic,
    pub reason: TodoReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodoReason {
    WaitingOnMe,
    Mentioned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    Page,
    File,
    Image,
    Decision,
}

/// One 产出 of a topic, shown in the side panel.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactSummary {
    pub id: String,
    pub topic_id: TopicId,
    pub kind: ArtifactKind,
    pub title: String,
    pub version: u32,
    pub size_bytes: Option<u64>,
    pub created_by: MemberId,
    pub updated_at: Millis,
}

/// A secret the user types or pastes. `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    /// `XXXX-XXXX` (or a legacy `XXXX-XXXX-XXXX`), single use; the server
    /// turns it into a token.
    InviteCode(String),
    /// A pasted `tc_...` token.
    Token(String),
}

impl Credential {
    /// Reads what the user pasted: a token starts with `tc_`, anything else is an invite code.
    pub fn from_input(text: &str) -> Option<Credential> {
        let text = text.trim();
        if text.is_empty() {
            None
        } else if text.starts_with("tc_") {
            Some(Credential::Token(text.to_string()))
        } else {
            Some(Credential::InviteCode(text.to_string()))
        }
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Credential::InviteCode(_) => f.write_str("InviteCode(<redacted>)"),
            Credential::Token(_) => f.write_str("Token(<redacted>)"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginRequest {
    /// Base URL such as `https://chat-api-staging.example.workers.dev`.
    pub server_url: String,
    pub credential: Credential,
    /// Shown in the member's device list.
    pub device_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginError {
    /// The invite code or token is wrong, used or revoked.
    BadCredential,
    /// The server URL is not valid or the server cannot be reached.
    ServerUnreachable,
    /// The server speaks a protocol this app does not support.
    Unsupported,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionState {
    /// No stored token; show the login view.
    SignedOut {
        last_error: Option<LoginError>,
    },
    /// `login` is running.
    SigningIn,
    SignedIn {
        me: Member,
        server_url: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not signed in, or stopped on purpose.
    Offline,
    Connecting,
    Online,
    /// Lost the connection; the next try is in `retry_in_ms`.
    Reconnecting {
        attempt: u32,
        retry_in_ms: u64,
    },
}

/// A new message from the composer.
#[derive(Clone, Debug, PartialEq)]
pub struct SendRequest {
    pub group_id: GroupId,
    pub topic_id: TopicId,
    pub body: String,
    pub mentions: Vec<MemberId>,
    pub quote_of: Option<MessageId>,
}

/// The first message of a new topic.
#[derive(Clone, Debug, PartialEq)]
pub struct StartTopicRequest {
    pub group_id: GroupId,
    /// `None` uses the first line of `body`, cut to 80 characters.
    pub title: Option<String>,
    pub body: String,
    pub mentions: Vec<MemberId>,
}

/// A new 群 from the new-group dialog. I am added as a member by the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateGroupRequest {
    pub name: String,
    /// The other members; my own id is not needed.
    pub member_ids: Vec<MemberId>,
}

/// Where the main window was, so the next start opens the same place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedNavigation {
    /// The 待办 list was open (the topic, if any, is still in `group_id`).
    pub todo: bool,
    /// The selected group, or the group of the open topic.
    pub group_id: Option<GroupId>,
    pub topic_id: Option<TopicId>,
}

/// A message search (`search` frame). Without `group_id` it covers all my groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    /// 1..200 characters.
    pub text: String,
    pub group_id: Option<GroupId>,
    /// 1..50.
    pub limit: u32,
}

/// One search result, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub group_id: GroupId,
    pub topic_id: TopicId,
    pub message_id: MessageId,
    pub author_id: MemberId,
    /// Plain text around the match.
    pub snippet: String,
    /// Matched parts of `snippet`, as ranges of code points (chars).
    pub highlights: Vec<std::ops::Range<usize>>,
    pub created_at: Millis,
    /// The message's `created_seq`.
    pub seq: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchError {
    /// Not connected (search runs on the server).
    Offline,
    /// The server refused it (for example a group I left).
    Refused,
    TimedOut,
    /// This provider has no search.
    Unavailable,
}

/// The answer to [`ChatProvider::search`]; await it on the UI executor.
pub type SearchReply = futures::channel::oneshot::Receiver<Result<Vec<SearchHit>, SearchError>>;

/// Status of a bot run (`RunStatus` in docs/protocol.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RunStatus {
    Created,
    Running,
    Waiting,
    Blocked,
    Done,
    Failed,
    Cancelled,
}

impl RunStatus {
    /// Still running (the process view follows its log).
    pub fn is_active(self) -> bool {
        matches!(
            self,
            RunStatus::Created | RunStatus::Running | RunStatus::Waiting | RunStatus::Blocked
        )
    }

    /// The wire name, for the developer view.
    pub fn wire_name(self) -> &'static str {
        match self {
            RunStatus::Created => "created",
            RunStatus::Running => "running",
            RunStatus::Waiting => "waiting",
            RunStatus::Blocked => "blocked",
            RunStatus::Done => "done",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }
}

/// One run of a bot member in a topic, from `run.updated` events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BotRunSummary {
    pub id: String,
    pub bot_id: MemberId,
    pub group_id: GroupId,
    pub topic_id: TopicId,
    pub machine_id: Option<String>,
    /// The machine's name, when known.
    pub machine_name: Option<String>,
    pub status: RunStatus,
    pub requested_by: Option<MemberId>,
    pub created_at: Millis,
    pub started_at: Option<Millis>,
    pub ended_at: Option<Millis>,
    pub updated_at: Millis,
    pub summary: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileChangeKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChangeView {
    pub path: String,
    pub kind: FileChangeKind,
}

/// One normalized step of a run, in log order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessStep {
    /// Text the bot wrote (`from_bot`), or input it received.
    Message {
        text: String,
        from_bot: bool,
        streaming: bool,
    },
    Reasoning {
        text: String,
    },
    /// A command or tool call, with its result once it came.
    Tool {
        title: String,
        /// The call's input, as compact JSON (developer view).
        input: Option<String>,
        ok: Option<bool>,
        output: Option<String>,
        exit_code: Option<i64>,
    },
    Files {
        changes: Vec<FileChangeView>,
    },
    Approval {
        title: String,
        detail: Option<String>,
        /// `None` while waiting; `Some(true)` approved.
        approved: Option<bool>,
    },
    TurnStarted,
    TurnCompleted {
        ok: bool,
        error: Option<String>,
    },
    Status {
        message: String,
    },
    Done {
        status: RunStatus,
        summary: Option<String>,
        error: Option<String>,
        duration_ms: u64,
    },
}

/// One stored run log entry as JSON, for the developer view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawRunEntry {
    pub seq: u64,
    pub json: String,
}

/// Why a run log, or its raw session, could not be loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunLoadError {
    Offline,
    /// Not allowed (the raw session is for owners only).
    Forbidden,
    /// Not there (yet).
    NotFound,
    Failed,
    Unavailable,
}

/// What the process view shows for one run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunProcess {
    pub steps: Vec<ProcessStep>,
    /// The newest stored entries (at most a few hundred), oldest first.
    pub raw: Vec<RawRunEntry>,
    pub last_seq: u64,
    /// (session id, runtime) from the log, for the developer view.
    pub session: Option<(String, String)>,
    /// A fetch is running.
    pub loading: bool,
    /// At least one fetch finished.
    pub loaded: bool,
    pub error: Option<RunLoadError>,
}

/// The answer to [`ChatProvider::download_run_session`]: where the file was saved.
pub type SessionDownload = futures::channel::oneshot::Receiver<Result<PathBuf, RunLoadError>>;

/// What changed. Views re-read only the snapshots that a change names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Change {
    Session,
    Connection,
    /// The member directory or someone's presence.
    Members,
    /// The group list, its order, names or unread and mention counts.
    Groups,
    /// The topics of one group (new topic, title, status, unread).
    Topics {
        group_id: GroupId,
    },
    /// Messages of one topic (new, edited, deleted, streamed, ack, older page loaded).
    Messages {
        group_id: GroupId,
        topic_id: TopicId,
    },
    /// Who is typing in one topic.
    Typing {
        group_id: GroupId,
        topic_id: TopicId,
    },
    /// The 产出 of one topic.
    Artifacts {
        group_id: GroupId,
        topic_id: TopicId,
    },
    /// The 待办 list.
    Todo,
    /// The bot runs of one topic (a run started or changed status).
    Runs {
        group_id: GroupId,
        topic_id: TopicId,
    },
    /// New steps in the log of one run (see [`ChatProvider::follow_run`]).
    RunLog {
        run_id: String,
    },
    /// The quote or pending files of one composer draft (not its text, which
    /// only the composer writes).
    Draft {
        key: DraftKey,
    },
}

/// Which composer a draft belongs to: one per topic, and one per 群 for a new
/// topic (`topic_id: None`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DraftKey {
    pub group_id: GroupId,
    pub topic_id: Option<TopicId>,
}

impl DraftKey {
    pub fn topic(group_id: impl Into<GroupId>, topic_id: impl Into<TopicId>) -> Self {
        Self {
            group_id: group_id.into(),
            topic_id: Some(topic_id.into()),
        }
    }

    pub fn new_topic(group_id: impl Into<GroupId>) -> Self {
        Self {
            group_id: group_id.into(),
            topic_id: None,
        }
    }
}

/// Id of a file waiting in a draft, made by the provider.
pub type PendingFileId = String;

/// A file or image the user pasted or dropped into the composer.
#[derive(Clone, PartialEq, Eq)]
pub enum NewFile {
    /// A file on disk (dropped, or copied in Finder).
    Path(PathBuf),
    /// Image data from the clipboard.
    Image {
        /// MIME type, such as `image/png`.
        mime: String,
        /// File extension without the dot, such as `png`.
        extension: String,
        bytes: Arc<[u8]>,
    },
}

impl fmt::Debug for NewFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NewFile::Path(path) => f.debug_tuple("Path").field(path).finish(),
            NewFile::Image { mime, bytes, .. } => f
                .debug_struct("Image")
                .field("mime", mime)
                .field("len", &bytes.len())
                .finish(),
        }
    }
}

/// A file waiting in a draft until the message is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingFile {
    pub id: PendingFileId,
    /// Shown on the chip.
    pub name: String,
    pub is_image: bool,
    pub size_bytes: Option<u64>,
    pub source: NewFile,
}

/// What the composer of one topic (or of a new topic) holds while unsent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub body: String,
    /// Members picked from the @ list.
    pub mentions: Vec<MemberId>,
    /// The message the reply quotes.
    pub quote_of: Option<MessageId>,
    pub files: Vec<PendingFile>,
}

// --- settings window: profile, 成员 admin, 机器, 通知, diagnostics ---

/// Why a settings request failed. The settings views map each to copy text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminError {
    /// This provider (or the server) does not support the request yet.
    Unsupported,
    /// Not signed in, or the server cannot be reached.
    Offline,
    /// Only an owner may do this.
    Forbidden,
    NotFound,
    /// The change would leave the team with no active owner.
    LastOwner,
    /// The server refused the input (name too long, bad labels, ...).
    Invalid,
    /// The file is too big (avatar over 1 MiB, diagnostics over 5 MiB).
    TooLarge,
    /// Too many requests; try again later.
    RateLimited,
    Other,
}

/// The answer to a settings request. The view awaits it off the UI thread
/// (`cx.spawn`); dropping the sender means [`AdminError::Other`].
pub type AdminReply<T> = futures::channel::oneshot::Receiver<Result<T, AdminError>>;

/// An [`AdminReply`] that is already answered.
pub fn reply_now<T>(result: Result<T, AdminError>) -> AdminReply<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let _ = tx.send(result);
    rx
}

/// A secret shown once (invite code, credential, install command). `Debug`
/// never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretText(String);

impl SecretText {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }
    /// The secret itself, for the copy button and the one-time display.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

/// `POST /v1/invites`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteRequest {
    pub name: String,
    pub kind: MemberKind,
    pub level: MemberLevel,
    pub role: Option<String>,
}

/// A new invite: the code is shown once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteCreated {
    pub code: SecretText,
    /// `<server>/download?code=<code>`: the latest DMG for this invitee.
    pub download_url: SecretText,
    pub expires_at: Millis,
}

/// A new bot member made with `POST /v1/bots`.
#[derive(Clone, Debug, PartialEq)]
pub struct BotCreated {
    pub member: Member,
    /// Its credential, shown once. `None` for a runner-backed member (the
    /// provider revokes the credential, because the server runs it).
    pub credential: Option<SecretText>,
}

/// How a runner-backed member works (`PUT /v1/bots/:id/runner`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunnerSetup {
    /// A runtime name that a machine reports.
    pub runtime: String,
    /// Machine labels the run needs.
    pub labels: Vec<String>,
    /// Repository URL or a folder on the machine.
    pub repo: Option<String>,
    pub base_branch: Option<String>,
}

/// A connected (or known) machine, for the 机器 page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MachineInfo {
    pub id: String,
    pub name: String,
    pub labels: Vec<String>,
    pub connected: bool,
    pub healthy: bool,
    pub last_heartbeat_at: Option<Millis>,
    pub version: Option<String>,
    pub disk_free_bytes: Option<u64>,
    pub disk_total_bytes: Option<u64>,
    /// Runs on it now.
    pub active_runs: u32,
    pub max_parallel_runs: Option<u32>,
    /// Runtime names it can run.
    pub runtimes: Vec<String>,
}

/// A new machine: its credential and install command are shown once.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineCreated {
    pub machine: MachineInfo,
    pub credential: SecretText,
    /// Shell lines that install the machine service with this credential.
    pub install_command: SecretText,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotifyLevel {
    All,
    Mentions,
    None,
}

/// My notification preferences of one group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupPrefs {
    pub muted: bool,
    pub notify: NotifyLevel,
}

/// A topic I follow, for the 通知 page.
#[derive(Clone, Debug, PartialEq)]
pub struct FollowedTopic {
    pub group_id: GroupId,
    pub group_name: String,
    pub topic: Topic,
}


/// Card types (docs/protocol.md "Cards"). The schema is the protocol crate's,
/// so readers keep its `CardBlock::Unknown` for block types added later.
pub mod card {
    pub use chat_core::proto::{
        ActionStyle, Card, CardAction, CardBlock, CardField, CardListItem, CardListStatus,
        CardStatus, CardTone, LinkTarget,
    };
}

/// One press of a card button (a `card.actioned` event).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardPress {
    /// Who pressed.
    pub actor_id: MemberId,
    pub action_id: String,
    /// The button's label when it was pressed, if the provider saw that card
    /// version (a bot often removes the buttons after a press).
    pub label: Option<String>,
    pub at: Millis,
}

/// My latest press of a button on one card, while it is not answered yet or
/// after the server refused it. A press that went through is a [`CardPress`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardPressState {
    pub action_id: String,
    pub phase: CardPressPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardPressPhase {
    /// Queued or sent, no answer yet (also while offline).
    Sending,
    Failed(CardPressError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardPressError {
    /// The button or the card no longer exists.
    Gone,
    /// I may not press it.
    Forbidden,
    Other,
}

/// The source of all chat state for the views.
///
/// Rules for implementations:
/// - Getters return a snapshot from memory or the local cache at once. They
///   never wait for the network. Unknown ids give empty results.
/// - Commands return at once and do their work in the background. Their effect
///   arrives as [`Change`] batches. `send` and `start_topic` show the message
///   as [`SendState::Pending`] right away.
/// - Changes are coalesced into batches; a batch may name the same change
///   twice. Order inside a batch has no meaning.
/// - Never log or expose credentials or tokens.
pub trait ChatProvider: Send + Sync + 'static {
    // --- session ---

    /// Current session. `SignedOut` means the login view shows.
    fn session(&self) -> SessionState;
    /// Signs in with an invite code or a pasted token. Emits `Change::Session`
    /// (SigningIn, then SignedIn or SignedOut with an error). A real provider
    /// stores the token in the macOS Keychain.
    fn login(&self, request: LoginRequest);
    /// Forgets the token and closes the connection.
    fn logout(&self);
    /// State of the live connection, for the status line.
    fn connection(&self) -> ConnectionState;
    /// Signs out like [`ChatProvider::logout`] (this token is revoked on the
    /// server) and, with `wipe_cache`, also deletes the local copy of the
    /// chats on this Mac.
    fn sign_out(&self, _wipe_cache: bool) {
        self.logout();
    }
    /// Reconnects at once instead of waiting for the next retry.
    fn reconnect_now(&self) {}

    // --- directory ---

    /// Every member I can see, including bots and deactivated members.
    fn members(&self) -> Vec<Member>;
    fn member(&self, id: &str) -> Option<Member> {
        self.members().into_iter().find(|m| m.id == id)
    }
    /// My groups and DMs, newest activity first, with unread and mention counts.
    fn groups(&self) -> Vec<GroupSummary>;
    /// The topics of one group, newest activity first.
    fn topics(&self, group_id: &str) -> Vec<Topic>;
    fn topic(&self, group_id: &str, topic_id: &str) -> Option<Topic> {
        self.topics(group_id).into_iter().find(|t| t.id == topic_id)
    }
    /// Topics that wait on me and topics with unread mentions of me.
    fn todo(&self) -> Vec<TodoItem>;

    // --- messages ---

    /// The newest `limit` messages of a topic (more if older pages were
    /// loaded), oldest first, including my pending messages.
    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow;
    /// Starts loading the page before the oldest message in the window.
    /// Emits `Change::Messages` when it arrives.
    fn load_older(&self, group_id: &str, topic_id: &str);
    /// Members typing in a topic now (not me).
    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId>;
    /// The 产出 of a topic, newest first.
    fn artifacts(&self, group_id: &str, topic_id: &str) -> Vec<ArtifactSummary>;

    // --- commands ---

    /// Sends a message into an existing topic.
    fn send(&self, request: SendRequest) -> ClientMsgId;
    /// Starts a topic whose root is this message.
    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId;
    /// Sends a [`SendState::Failed`] message of mine again, with the same
    /// `client_msg_id` (the server deduplicates on it). Emits
    /// `Change::Messages`. Providers that can fail a send must implement it.
    fn retry_send(&self, group_id: &str, client_msg_id: &str) {
        let _ = (group_id, client_msg_id);
    }
    /// Replaces the body of my message.
    fn edit(&self, group_id: &str, message_id: &str, body: String);
    /// Deletes my message (or any message, for an owner).
    fn delete(&self, group_id: &str, message_id: &str);
    /// Adds (`on`) or removes my reaction to a message: one per emoji per
    /// member. Emits `Change::Messages` for the topic at once (the change
    /// shows before the server confirms it) and again when it is confirmed.
    fn react(&self, _group_id: &str, _topic_id: &str, _message_id: &str, _emoji: &str, _on: bool) {}
    /// Sets the status and who the topic waits on.
    fn set_topic_status(
        &self,
        group_id: &str,
        topic_id: &str,
        status: TopicStatus,
        waiting_on: Vec<MemberId>,
    );
    /// Marks a topic read up to `seq` (only moves forward).
    fn mark_read(&self, group_id: &str, topic_id: &str, seq: u64);
    /// Tells others I am typing. The provider limits it to one per 3 s per topic.
    fn set_typing(&self, group_id: &str, topic_id: &str);
    /// Drops a failed or pending message of mine that the server has not acked.
    fn discard_send(&self, _group_id: &str, _client_msg_id: &str) {}

    // --- composer drafts ---
    //
    // Drafts live in the provider so they survive switching topics (and,
    // later, restarts). The defaults keep nothing; a real provider overrides
    // them (`crate::drafts::DraftStore` is an in-memory store to embed).

    /// The draft of one composer. Empty when there is none.
    fn draft(&self, _key: &DraftKey) -> Draft {
        Draft::default()
    }
    /// Saves the text and picked mentions. Emits nothing: only the composer
    /// writes the text.
    fn save_draft_text(&self, _key: &DraftKey, _body: String, _mentions: Vec<MemberId>) {}
    /// Sets or clears the quoted message (from the timeline's "quote" action).
    /// Emits `Change::Draft`.
    fn set_quote(&self, _key: &DraftKey, _quote_of: Option<MessageId>) {}
    /// Adds pasted or dropped files to the draft. Emits `Change::Draft`.
    /// The upload starts when the message is sent.
    fn attach_files(&self, _key: &DraftKey, _files: Vec<NewFile>) {}
    /// Removes one pending file. Emits `Change::Draft`.
    fn remove_file(&self, _key: &DraftKey, _file_id: &str) {}
    /// Forgets the whole draft (after a send). Emits `Change::Draft`.
    fn clear_draft(&self, _key: &DraftKey) {}
    /// Sends a message with pending files of the draft `key`. The default
    /// ignores the files.
    fn send_with_files(
        &self,
        request: SendRequest,
        _key: &DraftKey,
        _files: Vec<PendingFileId>,
    ) -> ClientMsgId {
        self.send(request)
    }
    /// Starts a topic with pending files of the draft `key`. The default
    /// ignores the files.
    fn start_topic_with_files(
        &self,
        request: StartTopicRequest,
        _key: &DraftKey,
        _files: Vec<PendingFileId>,
    ) -> ClientMsgId {
        self.start_topic(request)
    }

    // --- files and 产出 (all have defaults, so providers can add them later) ---

    /// Files the last `attach_files` of `key` could not take (folders, files
    /// over [`MAX_FILE_BYTES`]). Reading clears them. A provider emits
    /// `Change::Draft` for `key` when there are some.
    fn take_rejected_files(&self, _key: &DraftKey) -> Vec<RejectedFile> {
        Vec::new()
    }
    /// The state of an upload ([`upload_key`]) or a download ([`download_key`]).
    /// Cheap: views call it while they render.
    fn file_transfer(&self, _key: &str) -> Option<TransferState> {
        None
    }
    /// The local copy of a file when it is in the cache. Cheap and never
    /// touches the network.
    fn cached_file(&self, _file: &FileAttachment) -> Option<PathBuf> {
        None
    }
    /// A small image (at most 640 px) of a cached image file, for inline
    /// previews. `None` while it is not cached or being made; the provider
    /// emits `Change::Messages` for the topic when it is ready. Cheap.
    fn thumbnail(&self, _group_id: &str, _topic_id: &str, _file: &FileAttachment) -> Option<PathBuf> {
        None
    }
    /// Downloads a file into the cache (or finds it there). Progress is in
    /// `file_transfer(download_key(file))`; the provider emits
    /// `Change::Messages` for the topic while it runs and when it ends.
    fn fetch_file(&self, _group_id: &str, _topic_id: &str, _file: &FileAttachment) -> Reply<PathBuf> {
        reply_err(FileError::Other)
    }
    /// Cancels a running download of `file`: the partial file is removed and
    /// its row goes back to rest (`file_transfer` returns `None`). Emits
    /// `Change::Messages` for the topics that show it.
    fn cancel_download(&self, _file: &FileAttachment) {}
    /// Files attached to the cached messages of a topic, newest first.
    fn topic_files(&self, _group_id: &str, _topic_id: &str) -> Vec<TopicFile> {
        Vec::new()
    }
    /// Asks the server for the 产出 list of a topic. Emits `Change::Artifacts`.
    fn refresh_artifacts(&self, _group_id: &str, _topic_id: &str) {}
    /// A 产出 with its versions from the cache. `None` when it is not known
    /// yet: call [`ChatProvider::request_artifact`].
    fn artifact(&self, _artifact_id: &str) -> Option<ArtifactDetail> {
        None
    }
    /// Loads a 产出 and its versions from the server once. Emits
    /// `Change::Messages` and `Change::Artifacts` for its topic.
    fn request_artifact(&self, _group_id: &str, _topic_id: &str, _artifact_id: &str) {}
    /// Puts a version (`None` = the current one) of a 产出 on disk.
    fn open_artifact_version(&self, _artifact_id: &str, _version: Option<u32>) -> Reply<ArtifactFolder> {
        reply_err(FileError::Other)
    }
    /// Uploads a file or folder as a new 产出 or a new version, and posts a
    /// message that points to it in the topic.
    fn create_artifact(&self, _request: NewArtifactRequest) -> Reply<CreatedArtifact> {
        reply_err(FileError::Other)
    }

    // --- groups and navigation (all have defaults, so providers can add them later) ---

    /// Creates a 群 (`POST /v1/groups`). The new group arrives as `Change::Groups`.
    fn create_group(&self, _request: CreateGroupRequest) {}
    /// Finds or creates the DM with a member (`POST /v1/dms`). Emits `Change::Groups`.
    fn open_dm(&self, _member_id: &str) {}
    /// Leaves a group (`DELETE /v1/groups/:id/members/<me>`). Emits `Change::Groups`.
    fn leave_group(&self, _group_id: &str) {}
    /// My mute preference of a group (`GroupSummary.muted`).
    fn group_muted(&self, _group_id: &str) -> bool {
        false
    }
    /// Sets my mute preference (`PATCH /v1/groups/:id/prefs`). Emits `Change::Groups`.
    fn set_group_muted(&self, _group_id: &str, _muted: bool) {}
    /// The place the main window showed last time, from local storage.
    fn saved_navigation(&self) -> Option<SavedNavigation> {
        None
    }
    /// Stores the place of the main window locally (not on the server).
    fn save_navigation(&self, _navigation: SavedNavigation) {}

    // --- per-topic state (all have defaults) ---

    /// My read position in a topic: messages with `created_seq <= seq` are
    /// read. `None` when the provider does not track it; views then place the
    /// new-messages divider from `Topic::unread`.
    fn topic_read_seq(&self, _group_id: &str, _topic_id: &str) -> Option<u64> {
        None
    }
    /// I follow this topic: its results (status 完成, a new 产出) notify me.
    fn topic_following(&self, _group_id: &str, _topic_id: &str) -> bool {
        false
    }
    /// Follows or unfollows a topic. Emits `Change::Topics`.
    fn set_topic_following(&self, _group_id: &str, _topic_id: &str, _follow: bool) {}
    /// Sets or clears the one responsible member. Emits `Change::Topics`.
    fn set_topic_owner(&self, _group_id: &str, _topic_id: &str, _owner: Option<MemberId>) {}

    // --- search and bot runs (all have defaults) ---

    /// Searches messages on the server. Returns at once; the reply comes later.
    fn search(&self, _query: SearchQuery) -> SearchReply {
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = tx.send(Err(SearchError::Unavailable));
        rx
    }
    /// The bot runs of a topic, newest first, from the cache.
    fn runs(&self, _group_id: &str, _topic_id: &str) -> Vec<BotRunSummary> {
        Vec::new()
    }
    /// The steps of a run loaded so far. Empty until [`ChatProvider::follow_run`].
    fn run_process(&self, _run_id: &str) -> RunProcess {
        RunProcess::default()
    }
    /// Fetches the log entries of a run that are newer than the ones loaded.
    /// Emits `Change::RunLog`. Does nothing while a fetch of that run runs.
    fn follow_run(&self, _run_id: &str) {}
    /// Saves the raw session of a run (owners only) into `dir`.
    fn download_run_session(&self, _run_id: &str, _dir: PathBuf) -> SessionDownload {
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = tx.send(Err(RunLoadError::Unavailable));
        rx
    }

    // --- settings window (all have defaults, so providers can add them later) ---

    /// Changes my name and 岗位 (`PATCH /v1/members/<me>`). Emits `Change::Members`.
    fn update_profile(&self, _name: String, _role: Option<String>) -> AdminReply<Member> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Uploads my avatar from a png, jpeg or webp file of at most 1 MiB.
    fn upload_avatar(&self, _path: PathBuf) -> AdminReply<Member> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: makes an invite code.
    fn create_invite(&self, _request: InviteRequest) -> AdminReply<InviteCreated> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: changes a member's level. Emits `Change::Members`.
    fn set_member_level(&self, _member_id: &str, _level: MemberLevel) -> AdminReply<Member> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: deactivates (or reactivates) a member. Emits `Change::Members`.
    fn set_member_deactivated(&self, _member_id: &str, _deactivated: bool) -> AdminReply<Member> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: adds a bot member. With `runner`, it is runner-backed (the server
    /// runs it on a machine) and no credential is returned.
    fn create_bot_member(
        &self,
        _name: String,
        _role: Option<String>,
        _runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// The machines and their health, fresh from the server.
    fn machines(&self) -> AdminReply<Vec<MachineInfo>> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: registers a machine.
    fn create_machine(&self, _name: String, _labels: Vec<String>) -> AdminReply<MachineCreated> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// My mute and notify level of a group, from the cache.
    fn group_prefs(&self, group_id: &str) -> GroupPrefs {
        let dm = self
            .groups()
            .iter()
            .any(|g| g.id == group_id && g.kind == GroupKind::Dm);
        GroupPrefs {
            muted: self.group_muted(group_id),
            notify: if dm {
                NotifyLevel::All
            } else {
                NotifyLevel::Mentions
            },
        }
    }
    /// Sets my mute and notify level (`PATCH /v1/groups/:id/prefs`). Emits `Change::Groups`.
    fn set_group_prefs(
        &self,
        _group_id: &str,
        _muted: Option<bool>,
        _notify: Option<NotifyLevel>,
    ) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Topics I follow, newest activity first. The default reads
    /// [`ChatProvider::topic_following`] of every topic of my groups.
    fn followed_topics(&self) -> Vec<FollowedTopic> {
        let mut out = Vec::new();
        for g in self.groups() {
            for topic in self.topics(&g.id) {
                if self.topic_following(&g.id, &topic.id) {
                    out.push(FollowedTopic {
                        group_id: g.id.clone(),
                        group_name: g.name.clone(),
                        topic,
                    });
                }
            }
        }
        out.sort_by_key(|x| std::cmp::Reverse(x.topic.last_activity_at));
        out
    }
    /// Uploads recent logs (secrets removed) with the user's description.
    /// Returns the report id.
    fn send_diagnostics(&self, _description: String) -> AdminReply<String> {
        reply_now(Err(AdminError::Unsupported))
    }

    // --- cards (docs/protocol.md "Cards"; all have defaults) ---

    /// The card of a card message, from memory. Called on every frame for
    /// each visible card, so it must be cheap. The same `Arc` comes back
    /// while the card does not change.
    fn card(&self, _group_id: &str, _message_id: &str) -> Option<Arc<card::Card>> {
        None
    }
    /// Every press of the card's buttons, oldest first. Cheap, like `card`.
    fn card_presses(&self, _group_id: &str, _message_id: &str) -> Vec<CardPress> {
        Vec::new()
    }
    /// My unanswered or refused press on the card, if any.
    fn card_press_state(&self, _group_id: &str, _message_id: &str) -> Option<CardPressState> {
        None
    }
    /// Presses a card button (`card.action`). Emits `Change::Messages` for
    /// the topic when the press state, the presses or the card change.
    fn press_card_action(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _message_id: &str,
        _action_id: &str,
    ) {
    }
    /// A local copy of a card image (`fileId` of a ready image file of the
    /// group). `None` until it is downloaded; the provider starts the download
    /// and emits `Change::Messages` for `topic_id` when the file is there.
    fn card_image(&self, _group_id: &str, _topic_id: &str, _file_id: &str) -> Option<PathBuf> {
        None
    }

    // --- changes ---

    /// A new stream of change batches. Each call gives a new receiver; all
    /// receivers get every batch. The app drains it on the main thread through
    /// `app::ChangeHub`, so views do not call this themselves.
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>>;
}
