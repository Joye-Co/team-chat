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

pub use crate::characters::CharacterId;

/// View types of the 群, 成员 and 产出 pages (pages track).
pub mod pages;
/// View types of the v2 screens (1.1.0).
pub mod v2;
// --- cards v2 track: Card v2 view types ---
pub mod cards_v2;
// --- end cards v2 track ---
// --- inc-host track: 产出 page host types ---
pub mod page_host;
// --- end inc-host track ---
/// 沉淀: 记得的事, 团队约定 and the daily 记忆更新.
pub mod memory;
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
    /// The avatar image as a local file (a path or a `file://` URL). The real provider gives
    /// the downloaded copy, since the server route needs the token.
    pub avatar_url: Option<String>,
    pub deactivated: bool,
    /// Presence, from transient `presence` events.
    pub online: bool,
    /// The character the avatar draws, when set and its art is in this build (`characters`).
    pub character: Option<crate::characters::CharacterId>,
}

impl Member {
    pub fn is_bot(&self) -> bool {
        self.kind == MemberKind::Bot
    }
}

/// A name as views show it: no leading or trailing space and one space
/// between words (`"  Sam   Lee "` → `"Sam Lee"`). Providers apply it at the
/// data boundary, so no view sees a raw name.
pub fn clean_name(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
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

/// Largest file one message or 产出 can carry (`docs/protocol.md`): 2 GB since 1.1, files over
/// 100 MB upload in parts. Checked before the upload starts.
pub const MAX_FILE_BYTES: u64 = chat_core::proto::MAX_UPLOAD_BYTES;

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
    /// An image the timeline can show inline (the server serves these inline too). The name
    /// must also end in an image extension: the server does not check the bytes of a file, and
    /// an image preview downloads without a click.
    pub fn is_image(&self) -> bool {
        let ext = self
            .name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        matches!(
            self.mime.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        ) && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp")
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
    Running {
        done: u64,
        total: u64,
    },
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
    // --- v11/media: what the version holds, so the timeline picks a preview before the
    // files are on disk (the wire `ArtifactVersion.entry` and `files`). ---
    /// Path of the file to open: the HTML page, or the only file.
    pub entry: String,
    pub files: Vec<ArtifactVersionFile>,
    // --- end v11/media ---
}

// --- v11/media ---
/// One file of a 产出 version (the wire `ArtifactFile` without its id).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactVersionFile {
    pub path: String,
    pub size: u64,
    pub mime: String,
}
// --- end v11/media ---

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
    /// The 6-digit code sent to my sign-in email.
    EmailCode { email: String, code: String },
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
            Credential::EmailCode { .. } => f.write_str("EmailCode(<redacted>)"),
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

/// An account signed in on this Mac (the sidebar's account menu).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountEntry {
    /// The provider's id of the account (server and member).
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub server_url: String,
    /// The account the window shows.
    pub active: bool,
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
    /// Signed in, but the saved token cannot be read from the Keychain (the
    /// access prompt was denied, or the keychain is locked). The cache still
    /// shows; [`ChatProvider::reconnect_now`] reads the token again.
    TokenUnreadable,
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StartTopicRequest {
    pub group_id: GroupId,
    /// `None` uses the first line of `body`, cut to 80 characters.
    pub title: Option<String>,
    pub body: String,
    pub mentions: Vec<MemberId>,
    /// 开个话题: the 群聊 message the first message quotes.
    pub quote_of: Option<MessageId>,
}

/// The id of a 群's own chat, the 群聊 (docs/protocol.md, "群聊: the group's own
/// chat"): a topic with a fixed id that exists once someone wrote there.
pub fn main_topic_id(group_id: &str) -> TopicId {
    chat_core::proto::main_topic_id(group_id)
}

/// Whether `topic_id` is a 群聊 (of any 群). Lists of 话题 leave it out.
pub fn is_main_topic(topic_id: &str) -> bool {
    chat_core::proto::is_main_topic_id(topic_id)
}

/// The topic `topic_id` of `group_id`; for the 群聊 before anyone wrote
/// there, a blank one with its fixed id, so the 群 opens its chat at once.
pub fn topic_or_chat(p: &dyn ChatProvider, group_id: &str, topic_id: &str) -> Option<Topic> {
    if let Some(t) = p.topic(group_id, topic_id) {
        return Some(t);
    }
    if topic_id != main_topic_id(group_id) || !p.groups().iter().any(|g| g.id == group_id) {
        return None;
    }
    Some(Topic {
        id: topic_id.to_string(),
        group_id: group_id.to_string(),
        title: String::new(),
        status: TopicStatus::Open,
        waiting_on: Vec::new(),
        owner_id: None,
        created_by: String::new(),
        created_at: 0,
        root_message_id: String::new(),
        last_seq: 0,
        last_activity_at: 0,
        message_count: 0,
        unread: 0,
        mentions: 0,
    })
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
    /// v2 (1.1.0, additive): the page when it is not a group or 话题 page:
    /// `desk`, `members`, `artifacts` or `artifact`. `None` reads as v1.
    pub page: Option<String>,
    /// The member of the 成员 page.
    pub member_id: Option<MemberId>,
    /// The 产出 of the 产出 page.
    pub artifact_id: Option<String>,
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
    /// The bot's plan for this run, in order: what it did, does now, waits
    /// for and does later. Empty when the run has none.
    pub plan: Vec<PlanItem>,
}

/// The state of one plan item (docs/protocol.md, "Run plans").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanState {
    Done,
    /// The bot works on it now.
    Current,
    /// Waits for a member (an answer, a review).
    Waiting,
    Pending,
}

/// One item of a run plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanItem {
    pub text: String,
    pub state: PlanState,
}

impl PlanItem {
    pub fn new(text: impl Into<String>, state: PlanState) -> Self {
        Self {
            text: text.into(),
            state,
        }
    }
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
    /// inc-refs: the reference chips (引用提问, REFS §A4), at most 5, oldest first.
    pub refs: Vec<DraftRef>,
}

// --- inc-refs track: 引用提问 drafts ---

/// One reference chip of a draft: what is sent (`input`) and how the chip draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftRef {
    pub input: chat_core::proto::BlockRefInput,
    /// The 群 and 话题 of the source (to scroll back to it and to tell 已更新).
    pub group_id: GroupId,
    pub topic_id: TopicId,
    /// Code points at the start of `input.label` drawn quiet (source and block); the rest is
    /// the part, drawn strong.
    pub lead_chars: usize,
}

impl DraftRef {
    /// The address that makes two chips the same (dedupe, REFS §A4).
    pub fn same_address(&self, other: &chat_core::proto::BlockRefInput) -> bool {
        let a = &self.input;
        a.message_id == other.message_id
            && a.artifact == other.artifact
            && a.block_id == other.block_id
            && a.part == other.part
    }
}

/// What adding a chip did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddRef {
    Added,
    /// The same part is a chip already, at this index.
    Duplicate(usize),
    /// Five chips already (最多引用 5 处).
    Full,
    /// This provider keeps no chips.
    Unsupported,
}

/// Most chips of one message.
pub const MAX_DRAFT_REFS: usize = chat_core::proto::MAX_BLOCK_REFS;
// --- end inc-refs track ---

/// A quoted message outside the timeline window ([`ChatProvider::quoted_message`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotedMessage {
    pub author_id: MemberId,
    /// The body, or the server's excerpt of it.
    pub body: String,
    pub deleted: bool,
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
    /// Someone changed it first (a memory line undone twice, a version clash).
    Conflict,
    Other,
}

/// The answer to a settings request. The view awaits it off the UI thread
/// (`cx.spawn`); dropping the sender means [`AdminError::Other`].
pub type AdminReply<T> = futures::channel::oneshot::Receiver<Result<T, AdminError>>;

/// Why an email sign-in request failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmailError {
    /// Not signed in, or the server cannot be reached.
    Offline,
    /// Not an email address.
    BadAddress,
    /// The code is wrong, used or expired.
    WrongCode,
    /// Another member signs in with this email.
    Taken,
    /// Too many codes or tries; try again later.
    RateLimited,
    /// The server has no mail set up for email sign-in.
    NotSetUp,
    Other,
}

/// The answer to an email sign-in request (see [`AdminReply`]).
pub type EmailReply<T> = futures::channel::oneshot::Receiver<Result<T, EmailError>>;

/// An [`EmailReply`] that is already answered.
pub fn email_reply_now<T>(result: Result<T, EmailError>) -> EmailReply<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let _ = tx.send(result);
    rx
}

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
    /// `<server>/download#code=<code>`: the latest DMG for this invitee. The code is in the
    /// fragment, which browsers never send, so no server log sees it.
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
    /// The one computer that runs this member (1.2.2); `None` = any computer with the labels.
    pub machine_id: Option<String>,
    /// The model id the CLI runs; `None` = the CLI's default.
    pub model: Option<String>,
    /// 思考强度 as the CLI spells it; `None` = the CLI's default.
    pub effort: Option<String>,
}

/// What a save of the AI member edit sheet changes; a `None` field stays as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BotMemberEdit {
    pub name: Option<String>,
    /// `Some(None)` clears the 岗位.
    pub role: Option<Option<String>>,
    pub runner: Option<RunnerChange>,
}

impl BotMemberEdit {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.role.is_none() && self.runner.is_none()
    }
}

/// A change of how a member works on a machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunnerChange {
    /// `PUT /v1/bots/:id/runner`.
    Set(RunnerSetup),
    /// `DELETE /v1/bots/:id/runner`.
    Remove,
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
    /// What each available runtime reports it can run (feature "effort").
    pub clis: Vec<CliChoice>,
}

/// The models and 思考强度 one runtime of a computer offers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliChoice {
    pub runtime: String,
    /// Empty = the computer did not say: the model is typed.
    pub models: Vec<ModelChoice>,
    pub efforts: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelChoice {
    pub id: String,
    pub name: Option<String>,
    /// Empty = the runtime's efforts.
    pub efforts: Vec<String>,
    pub is_default: bool,
}

/// Whether this Mac is one of the team's computers (设置 › 机器 › 用这台电脑).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThisMac {
    /// This build has no daemon inside it (a debug build, the fake data): nothing to offer.
    Unavailable,
    /// No daemon service on this Mac yet.
    NotSetUp,
    /// The daemon service runs a daemon outside the app (one copied by hand): it does not
    /// update with the app.
    Elsewhere,
    /// The daemon service runs the app's own daemon, which updates with the app.
    Working,
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
    /// The server in use, signed in or not; `None` before the first login.
    fn current_server(&self) -> Option<String> {
        None
    }
    /// The servers this Mac has a saved session on.
    fn signed_in_servers(&self) -> Vec<String> {
        Vec::new()
    }
    /// 登录 › 用邮箱登录: sends a 6-digit code to `email` on `server_url`
    /// (no session needed). The answer does not tell whether the address
    /// belongs to a member. Sign in with [`Credential::EmailCode`].
    fn request_sign_in_code(&self, _server_url: &str, _email: &str) -> EmailReply<()> {
        email_reply_now(Err(EmailError::NotSetUp))
    }
    /// 账号 › 登录邮箱: my sign-in email, if I added one.
    fn my_email(&self) -> EmailReply<Option<String>> {
        email_reply_now(Err(EmailError::NotSetUp))
    }
    /// 账号 › 登录邮箱: sends a code to `email` before it becomes my sign-in email.
    fn add_email_code(&self, _email: &str) -> EmailReply<()> {
        email_reply_now(Err(EmailError::NotSetUp))
    }
    /// 账号 › 登录邮箱: confirms the code; answers the saved address.
    fn verify_email(&self, _email: &str, _code: &str) -> EmailReply<String> {
        email_reply_now(Err(EmailError::NotSetUp))
    }
    /// 账号 › 登录邮箱: removes my sign-in email.
    fn remove_email(&self) -> EmailReply<()> {
        email_reply_now(Err(EmailError::NotSetUp))
    }
    /// Uses another server. The session of the server in use stays saved
    /// (no sign-out): a server with a saved session signs in at once, any
    /// other one shows the login window for it. Emits `Change::Session`.
    fn switch_server(&self, _server_url: &str) {}

    // --- accounts ---

    /// Every account signed in on this Mac, the active one marked. A
    /// provider with one account lists the signed-in one.
    fn accounts(&self) -> Vec<AccountEntry> {
        match self.session() {
            SessionState::SignedIn { me, server_url } => vec![AccountEntry {
                id: format!("{server_url}#{}", me.id),
                name: me.name,
                role: me.role.filter(|r| !r.trim().is_empty()),
                server_url,
                active: true,
            }],
            _ => Vec::new(),
        }
    }
    /// Shows another signed-in account (from [`ChatProvider::accounts`]);
    /// the one shown now stays signed in. Emits `Change::Session`.
    fn switch_account(&self, _id: &str) {}
    /// 添加账号…: the login view shows while the account signed in now stays
    /// signed in; a sign-in adds the new account and shows it.
    fn add_account(&self) {}
    /// 取消 in the login view of 添加账号…: back to the account it left.
    fn cancel_add_account(&self) {}
    /// The login view shows for 添加账号… (it has 取消).
    fn adding_account(&self) -> bool {
        false
    }

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
    /// A quoted message that is not in the timeline window: from the cache, or else the
    /// author and excerpt the server put in the quoting message's `replyTo`.
    fn quoted_message(
        &self,
        _group_id: &str,
        _quoting_id: &str,
        _quoted_id: &str,
    ) -> Option<QuotedMessage> {
        None
    }
    /// Renames a topic (`topic.update` with `title`). The new title arrives as
    /// `Change::Topics`.
    fn rename_topic(&self, _group_id: &str, _topic_id: &str, _title: String) {}
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
    fn thumbnail(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _file: &FileAttachment,
    ) -> Option<PathBuf> {
        None
    }
    /// Downloads a file into the cache (or finds it there). Progress is in
    /// `file_transfer(download_key(file))`; the provider emits
    /// `Change::Messages` for the topic while it runs and when it ends.
    fn fetch_file(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _file: &FileAttachment,
    ) -> Reply<PathBuf> {
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
    fn open_artifact_version(
        &self,
        _artifact_id: &str,
        _version: Option<u32>,
    ) -> Reply<ArtifactFolder> {
        reply_err(FileError::Other)
    }
    /// Uploads a file or folder as a new 产出 or a new version, and posts a
    /// message that points to it in the topic.
    fn create_artifact(&self, _request: NewArtifactRequest) -> Reply<CreatedArtifact> {
        reply_err(FileError::Other)
    }

    // --- groups and navigation (all have defaults, so providers can add them later) ---

    /// Creates a 群 (`POST /v1/groups`). Answers with its id; the group itself arrives as
    /// `Change::Groups`.
    fn create_group(&self, _request: CreateGroupRequest) -> AdminReply<GroupId> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Finds or creates the DM with a member (`POST /v1/dms`). Answers with its id and
    /// emits `Change::Groups`.
    fn open_dm(&self, _member_id: &str) -> AdminReply<GroupId> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Adds members to a 群 (`POST /v1/groups/:id/members`, one per member). The new
    /// members arrive as `Change::Groups`.
    fn add_group_members(&self, _group_id: &str, _member_ids: Vec<MemberId>) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Renames a 群 (`PATCH /v1/groups/:id`); `name` is already checked with
    /// `views::nav::dialogs::validate_group_name`. The new name arrives as
    /// `Change::Groups`. A DM cannot be renamed.
    fn rename_group(&self, _group_id: &str, _name: String) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
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
    /// I muted this topic (话题静音): only mentions of me in it notify, like a
    /// muted 群. Independent of following.
    fn topic_muted(&self, _group_id: &str, _topic_id: &str) -> bool {
        false
    }
    /// Mutes or unmutes a topic for me. Emits `Change::Topics`.
    fn set_topic_muted(&self, _group_id: &str, _topic_id: &str, _muted: bool) {}
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
    /// Stops an active run (`POST /v1/runs/:id/cancel`; any person in the run's
    /// 群 may). Returns at once. [`ChatProvider::run_stopping`] is true until the
    /// answer is in; the provider emits `Change::Runs` for the topic when the stop
    /// starts and when it ends, and the run's end shows in [`ChatProvider::runs`]
    /// (from the answer, then `run.updated`). A run that already ended answers
    /// `Ok`: that is not an error for the user.
    fn cancel_run(&self, _group_id: &str, _topic_id: &str, _run_id: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// A stop of this run was asked and its answer is not in yet. Cheap.
    fn run_stopping(&self, _run_id: &str) -> bool {
        false
    }
    /// Saves the raw session of a run (owners only) into `dir`.
    fn download_run_session(&self, _run_id: &str, _dir: PathBuf) -> SessionDownload {
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = tx.send(Err(RunLoadError::Unavailable));
        rx
    }
    /// Saves what a run's prompt carried (owners only) into `dir` as text.
    fn download_run_context(&self, _run_id: &str, _dir: PathBuf) -> SessionDownload {
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
    /// `character` is its look (avatar and status art).
    fn create_bot_member(
        &self,
        _name: String,
        _role: Option<String>,
        _character: Option<CharacterId>,
        _runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: how a bot member works on a machine (`GET /v1/bots/:id/runner`);
    /// `None` when it does not.
    fn bot_runner(&self, _member_id: &str) -> AdminReply<Option<RunnerSetup>> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: saves an edit of a bot member: `PATCH /v1/members/:id` for the
    /// name and 岗位, then `PUT` or `DELETE /v1/bots/:id/runner`. A `Set` keeps
    /// the runner fields the sheet does not show. Emits `Change::Members`.
    fn update_bot_member(&self, _member_id: &str, _edit: BotMemberEdit) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: changes a bot's character (`PATCH /v1/members/:id`, `character`).
    fn set_member_character(
        &self,
        _member_id: &str,
        _character: Option<CharacterId>,
    ) -> AdminReply<Member> {
        reply_now(Err(AdminError::Unsupported))
    }
    // --- memory (docs/protocol.md "Memory"; all have defaults) ---

    /// The memories of `scope` people see: active, pending (today's, made active by the
    /// daily consolidation) and retired, newest change first.
    fn memories(&self, _scope: &memory::MemoryScope) -> AdminReply<Vec<memory::Memory>> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// The newest day's 记忆更新 of `scope`.
    fn memory_updates(&self, _scope: &memory::MemoryScope) -> AdminReply<memory::MemoryDay> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 记一条 (writers of the scope): active at once.
    fn create_memory(&self, _new: memory::NewMemory) -> AdminReply<memory::Memory> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Edits, 停用 or 恢复 a memory (writers): a new version.
    fn update_memory(
        &self,
        _memory_id: &str,
        _edit: memory::MemoryEdit,
    ) -> AdminReply<memory::Memory> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 撤回 one line of a day's 记忆更新 (writers). A flagged line cannot be undone
    /// (`Invalid`); a second undo is `Conflict`.
    fn undo_memory_update(&self, _update_id: &str) -> AdminReply<memory::MemoryUpdate> {
        reply_now(Err(AdminError::Unsupported))
    }

    /// Owners: 汇报给 and 限额 of a member (`PATCH /v1/members/:id` with `reportsTo` and
    /// `weeklyBudgetCents`; `None` clears). Answers with the stored member.
    fn set_member_plan(&self, _member_id: &str, _plan: pages::MemberPlan) -> AdminReply<Member> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// The machines and their health, fresh from the server.
    fn machines(&self) -> AdminReply<Vec<MachineInfo>> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: the monitor of one machine (`GET /v1/machines/:id/monitor`); `range` is "15m",
    /// "1h" or "24h", `day_start` local midnight in ms.
    fn machine_monitor(
        &self,
        _machine_id: &str,
        _range: &'static str,
        _day_start: u64,
    ) -> AdminReply<chat_core::proto::MachineMonitor> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner (feature "accounts"): starts a CLI sign-in on a machine; answers its id.
    fn start_machine_login(&self, _machine_id: &str, _runtime: &str) -> AdminReply<String> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: where a sign-in on a machine stands.
    fn machine_login(
        &self,
        _machine_id: &str,
        _login_id: &str,
    ) -> AdminReply<chat_core::proto::monitor::MachineLogin> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: the code the sign-in page showed.
    fn machine_login_code(&self, _machine_id: &str, _login_id: &str, _code: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: stops a sign-in.
    fn cancel_machine_login(&self, _machine_id: &str, _login_id: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: a machine's public key and the API keys it holds (names and last 4).
    fn machine_secrets(
        &self,
        _machine_id: &str,
    ) -> AdminReply<chat_core::proto::monitor::MachineSecretsResponse> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: gives a machine an API key, sealed to its public key on this Mac.
    fn set_machine_secret(
        &self,
        _machine_id: &str,
        _public_key: &str,
        _name: &str,
        _value: SecretText,
        _members: Vec<String>,
    ) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: removes an API key from a machine.
    fn delete_machine_secret(&self, _machine_id: &str, _name: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: an AI member's weekly budget alone (用量 › 改预算); `None` clears it.
    fn set_member_budget(&self, _member_id: &str, _cents: Option<u64>) -> AdminReply<Member> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: 用量 of AI members' runs that ended in [from, to).
    fn team_usage(&self, _from: u64, _to: u64) -> AdminReply<chat_core::proto::TeamUsage> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: 活动记录, newest first; `before` pages back, the filters narrow it.
    fn team_activity(
        &self,
        _before: Option<u64>,
        _actor: Option<String>,
        _kinds: Vec<String>,
        _machine: Option<String>,
    ) -> AdminReply<chat_core::proto::ActivityResponse> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Owner: registers a machine.
    fn create_machine(&self, _name: String, _labels: Vec<String>) -> AdminReply<MachineCreated> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Whether this Mac runs the app's own daemon (read from disk, fast).
    fn this_mac(&self) -> ThisMac {
        ThisMac::Unavailable
    }
    /// Owner: makes this Mac one of the team's computers, with no terminal: registers it
    /// (unless it has a daemon config already), writes the config and installs the service
    /// from the app's own daemon.
    fn use_this_mac(&self) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// The team machine id of this Mac (its daemon config), if it is one.
    fn this_machine_id(&self) -> Option<String> {
        None
    }
    /// Restarts this Mac's daemon service (it then runs the daemon file now on disk).
    fn restart_this_mac(&self) -> AdminReply<()> {
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

    // --- v2 screens (1.1.0; all have defaults, see `data::v2`) ---

    /// Jojo's one-line priority for the 首页, from its latest desk note.
    fn desk_note(&self) -> Option<v2::DeskNote> {
        None
    }
    /// 需要你拍板: asks addressed to me, newest first. The default lists the
    /// 话题 that wait on me (no card); providers with cards add them.
    fn asks_for_me(&self) -> Vec<v2::AskItem> {
        self.todo()
            .into_iter()
            .filter(|t| t.reason == TodoReason::WaitingOnMe)
            .map(|t| v2::AskItem {
                group_id: t.group_id,
                topic_id: t.topic.id.clone(),
                message_id: None,
                from: t
                    .topic
                    .owner_id
                    .clone()
                    .unwrap_or_else(|| t.topic.created_by.clone()),
                kind: match t.topic.status {
                    TopicStatus::WaitingReview => v2::AskKind::Review,
                    _ => v2::AskKind::Topic,
                },
                title: t.topic.title.clone(),
                at: t.topic.last_activity_at,
            })
            .collect()
    }
    /// The newest open ask for me in one 话题: the pinned decision card above
    /// the composer. A 话题-level ask (no card) takes the 话题's newest open
    /// ask card when there is one, so the banner names that card's question
    /// and asker (T-10), not the 话题 creator.
    fn pinned_ask(&self, group_id: &str, topic_id: &str) -> Option<v2::AskItem> {
        let ask = self
            .asks_for_me()
            .into_iter()
            .filter(|a| a.group_id == group_id && a.topic_id == topic_id)
            .max_by_key(|a| a.at)?;
        if ask.message_id.is_some() {
            return Some(ask);
        }
        Some(match open_ask_card(self, group_id, topic_id) {
            Some(m) => v2::AskItem {
                title: self
                    .card_v2(group_id, &m.id)
                    .map(|c| c.card.title.clone())
                    .or_else(|| self.card(group_id, &m.id).map(|c| c.title.clone()))
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or(ask.title),
                message_id: Some(m.id),
                from: m.author_id,
                at: m.created_at,
                ..ask
            },
            None => ask,
        })
    }
    /// 做完了 today, newest first. The default lists 话题 marked done since
    /// local midnight, with the proof of each 话题's newest result
    /// ([`ChatProvider::topic_result_proof`]).
    fn done_today(&self) -> Vec<v2::DoneItem> {
        let midnight = crate::selftest::clock::local_now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|t| t.and_local_timezone(chrono::Local).single())
            .map(|t| t.timestamp_millis())
            .unwrap_or(0);
        let mut out: Vec<v2::DoneItem> = self
            .groups()
            .iter()
            .flat_map(|g| self.topics(&g.id))
            .filter(|t| t.status == TopicStatus::Done && t.last_activity_at >= midnight)
            .map(|t| v2::DoneItem {
                group_id: t.group_id.clone(),
                topic_id: t.id.clone(),
                member_id: t.owner_id.clone().unwrap_or_else(|| t.created_by.clone()),
                title: t.title.clone(),
                at: t.last_activity_at,
                proof: self.topic_result_proof(&t.group_id, &t.id),
            })
            .collect();
        out.sort_by_key(|d| std::cmp::Reverse(d.at));
        out
    }
    /// Proof chips of a 话题's newest result (its done card). Empty by default.
    fn topic_result_proof(&self, _group_id: &str, _topic_id: &str) -> Vec<v2::Proof> {
        Vec::new()
    }
    /// 正在做: active runs of every 话题 I can see, newest first.
    fn active_runs(&self) -> Vec<BotRunSummary> {
        let mut out: Vec<BotRunSummary> = self
            .groups()
            .iter()
            .flat_map(|g| {
                self.topics(&g.id)
                    .into_iter()
                    .flat_map(|t| self.runs(&g.id, &t.id))
                    .collect::<Vec<_>>()
            })
            .filter(|r| r.status.is_active())
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        out
    }
    /// 排队: work that starts later. Shown only when there is some.
    fn queue(&self) -> Vec<v2::QueuedItem> {
        Vec::new()
    }
    /// 改动 of a 话题 (its PR), when it has one.
    fn topic_changes(&self, _group_id: &str, _topic_id: &str) -> Option<v2::TopicChanges> {
        None
    }
    /// What one run used.
    fn run_usage(&self, _run_id: &str) -> Option<v2::RunUsage> {
        None
    }
    /// 用时和成本 of a 话题. The default adds up the runs' times and usage.
    fn topic_cost(&self, group_id: &str, topic_id: &str) -> Option<v2::TopicCost> {
        let runs = self.runs(group_id, topic_id);
        if runs.is_empty() {
            return None;
        }
        let mut cost = v2::TopicCost {
            duration_ms: 0,
            cost_cents: None,
            currency: v2::Currency::default(),
            started_at: runs.iter().filter_map(|r| r.started_at).min(),
            ended_at: runs.iter().filter_map(|r| r.ended_at).max(),
        };
        // Costs in two currencies are not added up (and never converted).
        let mut currency: Option<v2::Currency> = None;
        let mut mixed = false;
        for r in &runs {
            match self.run_usage(&r.id) {
                Some(u) => {
                    cost.duration_ms += u.duration_ms;
                    if let Some(c) = u.cost_cents {
                        mixed |= currency.is_some_and(|k| k != u.currency);
                        currency = Some(u.currency);
                        cost.cost_cents = Some(cost.cost_cents.unwrap_or(0) + c);
                    }
                }
                None => {
                    if let (Some(a), Some(b)) = (r.started_at, r.ended_at) {
                        cost.duration_ms += (b - a).max(0) as u64;
                    }
                }
            }
        }
        if mixed {
            cost.cost_cents = None;
        }
        cost.currency = currency.unwrap_or_default();
        Some(cost)
    }
    /// 用到的决定: decisions a 话题 used.
    fn topic_decisions(&self, _group_id: &str, _topic_id: &str) -> Vec<v2::Decision> {
        Vec::new()
    }
    /// 群里的决定, newest first.
    fn group_decisions(&self, _group_id: &str) -> Vec<v2::Decision> {
        Vec::new()
    }
    /// The one-line purpose of a 群 under its title.
    fn group_purpose(&self, _group_id: &str) -> Option<String> {
        None
    }
    /// Sets the purpose line of a 群 (`PATCH /v1/groups/:id` with `purpose`; `""` clears).
    /// Same rights as a rename. Emits `Change::Groups` when stored.
    fn set_group_purpose(&self, _group_id: &str, _purpose: String) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// The member 单独问 talks to by default: Jojo, else the first bot.
    fn side_chat_default_member(&self) -> Option<MemberId> {
        let members = self.members();
        members
            .iter()
            .find(|m| m.is_bot() && !m.deactivated && m.name.eq_ignore_ascii_case("jojo"))
            .or_else(|| members.iter().find(|m| m.is_bot() && !m.deactivated))
            .map(|m| m.id.clone())
    }
    /// My 单独问 aside with `member_id` about a 话题. Empty until I ask.
    fn side_chat(&self, _group_id: &str, _topic_id: &str, member_id: &str) -> v2::SideChatThread {
        v2::SideChatThread {
            member_id: member_id.to_string(),
            messages: Vec::new(),
        }
    }
    /// Asks `member_id` privately about a 话题 (it reads the 话题 up to now). `refs` are the
    /// chips of the question (parts of the 话题's 群); their quote lines go before `body`.
    /// Emits `Change::Messages` for the 话题 when the aside changes.
    fn send_side_chat(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _member_id: &str,
        _body: String,
        _refs: Vec<DraftRef>,
    ) {
    }
    /// Posts the suggestion of an aside answer into the 话题 (only on press).
    fn post_side_chat_suggestion(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _member_id: &str,
        _message_id: &str,
    ) {
    }
    /// 实时查看: the terminal of a run rebuilt from its tool events.
    fn machine_screen(&self, _run_id: &str) -> Option<v2::MachineScreen> {
        None
    }
    /// 验收标准 the run reported.
    fn acceptance_criteria(&self, _run_id: &str) -> Vec<v2::AcceptanceItem> {
        Vec::new()
    }
    /// The time each step of a run started, in step order (for 回放).
    fn run_step_times(&self, _run_id: &str) -> Vec<Millis> {
        Vec::new()
    }
    /// 接管: pauses a run so I can use the machine (`run.pause`).
    fn take_over_run(&self, _group_id: &str, _topic_id: &str, _run_id: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 交还: lets a paused run go on (`run.resume`).
    fn hand_back_run(&self, _group_id: &str, _topic_id: &str, _run_id: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// The 成员 page detail of a member. The default counts this week's
    /// runs from the cache.
    fn member_stats(&self, member_id: &str) -> Option<v2::MemberStats> {
        let week_ago = crate::selftest::clock::local_now().timestamp_millis() - 7 * 24 * 3_600_000;
        let mut stats = v2::MemberStats::default();
        let mut topics = std::collections::HashSet::new();
        for g in self.groups() {
            for t in self.topics(&g.id) {
                for r in self.runs(&g.id, &t.id) {
                    if r.bot_id != member_id {
                        continue;
                    }
                    if r.status.is_active() {
                        stats.active.push((g.id.clone(), t.id.clone()));
                    }
                    if r.created_at >= week_ago {
                        topics.insert((g.id.clone(), t.id.clone()));
                        if r.status == RunStatus::Done {
                            stats.done_this_week += 1;
                        }
                    }
                }
            }
        }
        stats.topics_this_week = topics.len() as u32;
        Some(stats)
    }
    /// Every 产出 I can see, newest first, for the 产出 destination.
    fn all_artifacts(&self) -> Vec<(GroupId, ArtifactSummary)> {
        let mut out: Vec<(GroupId, ArtifactSummary)> = self
            .groups()
            .iter()
            .flat_map(|g| {
                self.topics(&g.id)
                    .into_iter()
                    .flat_map(|t| self.artifacts(&g.id, &t.id))
                    .map(|a| (g.id.clone(), a))
                    .collect::<Vec<_>>()
            })
            .collect();
        out.sort_by_key(|(_, a)| std::cmp::Reverse(a.updated_at));
        out
    }
    /// 来龙去脉 of a 产出.
    fn artifact_story(&self, _artifact_id: &str) -> Option<v2::ArtifactStory> {
        None
    }

    // --- cards v2 track (docs/protocol.md "Cards v2"; all have defaults) ---

    /// The v2 card of a card message, with its message context, from
    /// memory. Called on every frame for each visible card, so it must be
    /// cheap; the same `Arc` of the card comes back while it does not change.
    fn card_v2(&self, _group_id: &str, _message_id: &str) -> Option<cards_v2::CardV2Message> {
        None
    }
    /// Stored answers to a v2 card (`card.answered`), oldest first. Cheap.
    fn card_v2_answers(&self, _group_id: &str, _message_id: &str) -> Vec<cards_v2::CardV2Answer> {
        Vec::new()
    }
    /// My answer to a v2 card while it is on its way or after it was refused.
    fn card_v2_pending(
        &self,
        _group_id: &str,
        _message_id: &str,
    ) -> Option<cards_v2::CardV2Pending> {
        None
    }
    /// Sends my answer to a v2 card (`card.answer`). Emits `Change::Messages`
    /// for the topic when the answer state changes.
    fn answer_card_v2(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _message_id: &str,
        _answer: cards_v2::CardV2AnswerRequest,
    ) {
    }
    /// Takes back my answer while it is still queued (撤回), or forgets a
    /// refused one. Emits `Change::Messages` for the topic.
    fn withdraw_card_v2_answer(&self, _group_id: &str, _topic_id: &str, _message_id: &str) {}

    // --- inc-cards21: the interaction loop (INTERACT §3, §4.4) ---

    /// Sends one interaction with a v2 card (`card.event`). An answer goes through the
    /// outbox and is resent with the same id; a live event is sent now or fails at once.
    /// Returns the `clientEventId`, or `None` when it could not be made. Emits
    /// `Change::Messages` for the topic.
    fn send_card_event(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _message_id: &str,
        _request: cards_v2::CardEventRequest,
    ) -> Option<String> {
        None
    }
    /// My events on a v2 card this session, oldest first (at most 20). Cheap.
    fn card_v2_events(&self, _group_id: &str, _message_id: &str) -> Vec<cards_v2::MyCardEvent> {
        Vec::new()
    }
    /// 不等了: stop waiting for the author's response to one of my events.
    fn drop_card_event(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _message_id: &str,
        _client_event_id: &str,
    ) {
    }
    /// 重试 of a refused or stuck answer with the same `clientEventId`. False when it
    /// cannot be resent (then the view sends a new event).
    fn retry_card_event(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _message_id: &str,
        _client_event_id: &str,
    ) -> bool {
        false
    }
    // --- end inc-cards21 ---

    // --- end cards v2 track ---

    // --- topic track (v11/topic): the 话题 page ---

    /// Proof chips of one result message (a PR, test counts, a recording,
    /// 产出), from its done card. Empty when the message has none; the 话题
    /// page then derives what it can from the 话题's change and 产出 refs.
    fn message_proof(&self, _group_id: &str, _message_id: &str) -> Vec<v2::Proof> {
        Vec::new()
    }
    /// The length of a video or audio file, when known.
    fn media_duration(&self, _file: &FileAttachment) -> Option<u64> {
        None
    }
    /// A short, real preview of a 产出 version (topic rail thumbnail and the
    /// 产出 pane).
    fn artifact_preview(&self, _artifact_id: &str, _version: u32) -> Option<v2::ArtifactPreview> {
        None
    }
    /// Who approved the 话题's change and when, while it merges or after.
    fn changes_approval(&self, _group_id: &str, _topic_id: &str) -> Option<(MemberId, Millis)> {
        None
    }
    /// Approves the 话题's change (批准合并 on the pinned card). Emits
    /// `Change::Topics` and `Change::Messages` for the 话题.
    fn approve_changes(&self, _group_id: &str, _topic_id: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// Takes my approval back while the change has not merged (撤回).
    fn withdraw_changes_approval(&self, _group_id: &str, _topic_id: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// r2-topic: my approval is still queued on this computer, so 撤回 can
    /// take it back; `false` once the server stored it.
    fn changes_approval_queued(&self, _group_id: &str, _topic_id: &str) -> bool {
        false
    }

    // --- pages track: the 群, 成员 and 产出 pages (1.1.0; all have defaults) ---

    /// "Now" for the pages' elapsed times (`已 3 分钟`). Sample providers
    /// fix it so screenshots do not depend on the hour they are taken.
    fn clock_now(&self) -> Millis {
        crate::selftest::clock::local_now().timestamp_millis()
    }
    /// What a 进行中 card on the 群 page shows of a 话题. The default
    /// previews the live terminal of an active run, else the options of the
    /// ask that waits on me.
    fn topic_glance(&self, group_id: &str, topic_id: &str) -> pages::TopicGlance {
        let mut glance = pages::TopicGlance::default();
        let live = self
            .runs(group_id, topic_id)
            .into_iter()
            .filter(|r| r.status.is_active())
            .find(|r| {
                self.machine_screen(&r.id)
                    .is_some_and(|s| !s.lines.is_empty())
            });
        if let Some(run) = live {
            glance.preview = Some(pages::TopicPreview::Screen { run_id: run.id });
            return glance;
        }
        let ask = self.pinned_ask(group_id, topic_id);
        let message_id = ask.as_ref().and_then(|a| a.message_id.clone());
        // A v2 ask: the options of its first decision block.
        let v2_options: Option<Vec<pages::ChoiceOption>> = message_id
            .as_deref()
            .and_then(|m| self.card_v2(group_id, m))
            .and_then(|c| {
                c.card.all_blocks().into_iter().find_map(|b| match b {
                    cards_v2::CardV2Block::Decision { options, .. } => Some(
                        options
                            .iter()
                            .take(2)
                            .enumerate()
                            .map(|(i, o)| pages::ChoiceOption {
                                key: ((b'A' + i as u8) as char).to_string(),
                                label: o.label.clone(),
                                sketch: None,
                            })
                            .collect(),
                    ),
                    _ => None,
                })
            });
        if let (Some(ask), Some(options)) = (ask.as_ref(), v2_options) {
            if options.len() == 2 && ask.kind == v2::AskKind::Choice {
                glance.preview = Some(pages::TopicPreview::Choice { options });
            }
            return glance;
        }
        let card = message_id.as_deref().and_then(|m| self.card(group_id, m));
        if let (Some(ask), Some(card)) = (ask, card) {
            let options: Vec<pages::ChoiceOption> = card
                .actions
                .iter()
                .flatten()
                .take(2)
                .enumerate()
                .map(|(i, a)| pages::ChoiceOption {
                    key: ((b'A' + i as u8) as char).to_string(),
                    label: a.label.clone(),
                    sketch: None,
                })
                .collect();
            if options.len() == 2 && ask.kind == v2::AskKind::Choice {
                glance.preview = Some(pages::TopicPreview::Choice { options });
            }
        }
        glance
    }
    /// 最近完成 of a 群: 话题 done in the last seven days, newest first.
    fn group_recent_done(&self, group_id: &str) -> Vec<pages::RecentDone> {
        let week_ago = self.clock_now() - 7 * 24 * 3_600_000;
        let mut out: Vec<pages::RecentDone> = self
            .topics(group_id)
            .into_iter()
            .filter(|t| t.status == TopicStatus::Done && t.last_activity_at >= week_ago)
            .map(|t| pages::RecentDone {
                group_id: t.group_id.clone(),
                topic_id: t.id.clone(),
                member_id: t.owner_id.clone().unwrap_or_else(|| t.created_by.clone()),
                title: t.title.clone(),
                note: None,
                at: t.last_activity_at,
            })
            .collect();
        out.sort_by_key(|d| std::cmp::Reverse(d.at));
        out
    }
    /// 最近的产出 of a 群, newest first.
    fn group_artifacts(&self, group_id: &str) -> Vec<ArtifactSummary> {
        self.all_artifacts()
            .into_iter()
            .filter(|(g, _)| g == group_id)
            .map(|(_, a)| a)
            .collect()
    }
    /// A small picture of a 产出's first lines for lists, when known.
    fn artifact_thumb(&self, _artifact_id: &str) -> Option<pages::ArtifactThumb> {
        None
    }
    /// True when 记一条 can add a decision to a 群 by hand.
    fn can_record_decision(&self, _group_id: &str) -> bool {
        false
    }
    /// 记一条: adds a decision to a 群 (members read it before they work).
    fn record_decision(&self, _group_id: &str, _text: String) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// The 成员 page detail beyond [`ChatProvider::member_stats`]. The
    /// default derives this week's average time, 产出 and deliveries from
    /// the cached runs.
    fn member_extras(&self, member_id: &str) -> pages::MemberExtras {
        let week_ago = self.clock_now() - 7 * 24 * 3_600_000;
        let mut extras = pages::MemberExtras::default();
        let mut spans = Vec::new();
        let me = match self.session() {
            SessionState::SignedIn { me, .. } => Some(me.id),
            _ => None,
        };
        for g in self.groups() {
            for t in self.topics(&g.id) {
                let runs: Vec<BotRunSummary> = self
                    .runs(&g.id, &t.id)
                    .into_iter()
                    .filter(|r| r.bot_id == member_id && r.created_at >= week_ago)
                    .collect();
                if runs.is_empty() {
                    continue;
                }
                for r in &runs {
                    if r.status == RunStatus::Done {
                        let ms = self.run_usage(&r.id).map(|u| u.duration_ms).or_else(|| {
                            match (r.started_at, r.ended_at) {
                                (Some(a), Some(b)) => Some((b - a).max(0) as u64),
                                _ => None,
                            }
                        });
                        spans.extend(ms);
                    }
                }
                if let Some(ch) = self.topic_changes(&g.id, &t.id) {
                    extras.prs_this_week += 1;
                    extras.deliveries.push(pages::Delivery {
                        kind: pages::DeliveryKind::Pr {
                            number: ch.number,
                            additions: ch.additions,
                            deletions: ch.deletions,
                        },
                        title: ch.title.clone(),
                        at: runs.iter().map(|r| r.updated_at).max().unwrap_or(0),
                        topic: Some((g.id.clone(), t.id.clone())),
                        state: Some(match ch.state {
                            v2::ChangeState::Merged => pages::DeliveryState::Merged,
                            _ if t.waiting_on.iter().any(|w| Some(w) == me.as_ref()) => {
                                pages::DeliveryState::WaitsOnMe
                            }
                            _ => pages::DeliveryState::Open,
                        }),
                    });
                }
                for a in self.artifacts(&g.id, &t.id) {
                    if a.created_by == member_id && a.updated_at >= week_ago {
                        extras.pages_this_week += 1;
                        extras.deliveries.push(pages::Delivery {
                            kind: pages::DeliveryKind::Artifact {
                                artifact_id: a.id.clone(),
                                version: a.version,
                            },
                            title: a.title.clone(),
                            at: a.updated_at,
                            topic: Some((g.id.clone(), t.id.clone())),
                            state: None,
                        });
                    }
                }
                extras.decisions.extend(self.topic_decisions(&g.id, &t.id));
            }
        }
        if !spans.is_empty() {
            extras.avg_topic_ms = Some(spans.iter().sum::<u64>() / spans.len() as u64);
        }
        extras.deliveries.sort_by_key(|d| std::cmp::Reverse(d.at));
        extras.deliveries.truncate(5);
        extras.tools = self
            .member_stats(member_id)
            .map(|s| s.tools)
            .unwrap_or_default()
            .into_iter()
            .map(|name| pages::MemberTool {
                name,
                access: None,
                detail: None,
            })
            .collect();
        extras
    }
    /// 重试 on 最近的问题: asks the member to run again.
    fn retry_member_problem(&self, _member_id: &str, _run_id: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 页面里的回应 of a 产出, oldest first.
    fn artifact_responses(&self, _artifact_id: &str) -> Vec<pages::ArtifactResponse> {
        Vec::new()
    }
    /// Where the read of the server's page log of a 产出 stands. Cheap.
    fn artifact_responses_load(&self, _artifact_id: &str) -> pages::ResponsesLoad {
        pages::ResponsesLoad::Ready
    }
    /// Reads the server's page log of a 产出 again now (重试).
    fn reload_artifact_responses(&self, _artifact_id: &str) {}
    /// Events a version of a 产出 page may send back (`emitEvent`): only
    /// declared names, and only on versions a bot member made.
    fn artifact_page_events(&self, _artifact_id: &str, _version: u32) -> Vec<String> {
        Vec::new()
    }
    /// Forwards an event from a 产出 page to the bot that made it
    /// (`artifact.event`).
    fn send_artifact_event(
        &self,
        _artifact_id: &str,
        _version: u32,
        _name: &str,
        _payload: Option<serde_json::Value>,
    ) {
    }

    // --- end pages track ---

    // --- dfix-motion track ---

    /// The 单独问 topics (a topic in my DM whose `contextOf` names another
    /// 话题), as (群, 话题). They are not ordinary 话题: the rail and the 首页
    /// leave them out. None by default.
    fn side_chat_topics(&self) -> std::collections::HashSet<(GroupId, TopicId)> {
        std::collections::HashSet::new()
    }

    // --- end dfix-motion track ---

    // --- inc-host track: the 产出 page host (INTERACT §5; all have defaults) ---

    /// What the page of a version may do and who made it. `None` when unknown. The
    /// default reads the 产出, its 群's members and [`ChatProvider::artifact_page_events`]
    /// (every declared name an `emit`).
    fn page_bridge(&self, artifact_id: &str, version: u32) -> Option<page_host::PageBridge> {
        page_host::default_bridge(self, artifact_id, version)
    }
    /// Sends a page event or ask to the member that made the version (`artifact.event`)
    /// now; never queued. The answer arrives as [`page_host::PageSignal::Acked`] with the
    /// returned client event id.
    fn send_page_event(
        &self,
        _artifact_id: &str,
        _version: u32,
        _name: &str,
        _kind: page_host::PageEventKind,
        _payload: Option<serde_json::Value>,
    ) -> Result<String, page_host::PageSendError> {
        Err(page_host::PageSendError::Refused)
    }
    /// The latest data snapshot of a version (`GET …/data`): `(data, dataRev)`, `None`
    /// when it could not be read.
    fn fetch_page_data(
        &self,
        _artifact_id: &str,
        _version: u32,
    ) -> futures::channel::oneshot::Receiver<Option<(serde_json::Value, u64)>> {
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = tx.send(None);
        rx
    }
    /// A new stream of page signals (acks, replies, data pushes). `None` when the provider
    /// has none (samples).
    fn page_signals(&self) -> Option<UnboundedReceiver<page_host::PageSignal>> {
        None
    }

    /// The media metadata of a message file (R9), when its sender sent it.
    fn file_media(&self, _file: &FileAttachment) -> Option<chat_core::proto::MediaInfo> {
        None
    }
    /// The media metadata of a 产出 version (R9), when its sender sent it.
    fn artifact_media(
        &self,
        _artifact_id: &str,
        _version: u32,
    ) -> Option<chat_core::proto::MediaInfo> {
        None
    }
    /// The poster of a video or PDF file (the sender's, else made here from the file on
    /// disk), or the image itself. `None` while it is not there yet.
    fn file_poster(
        &self,
        _group_id: &str,
        _topic_id: &str,
        file: &FileAttachment,
    ) -> Option<PathBuf> {
        let kind = crate::views::media::file_kind(&file.name, &file.mime);
        let path = self.cached_file(file)?;
        crate::views::media::poster::poster(&path, kind, || {})
    }
    /// True while the sender makes the preview of an upload (`upload_key`): 正在生成预览.
    fn upload_generating(&self, _key: &str) -> bool {
        false
    }
    /// The poster or snapshot of a non-page 产出 version (lists and thumbnails).
    fn artifact_poster(&self, _artifact_id: &str, _version: u32) -> Option<PathBuf> {
        None
    }

    // --- end inc-host track ---
    // --- inc-refs track: 引用提问 (REFS part A) ---

    /// The server takes references to a part (`welcome.features` has `refs/1`). Without it the
    /// views hide every 引用提问 affordance. False by default.
    fn refs_supported(&self) -> bool {
        false
    }
    /// Adds a reference chip to a draft: deduped by address, at most 5. Emits `Change::Draft`
    /// when it was added.
    fn add_draft_ref(&self, _key: &DraftKey, _r: DraftRef) -> AddRef {
        AddRef::Unsupported
    }
    /// Removes the chip at `index`. Emits `Change::Draft`.
    fn remove_draft_ref(&self, _key: &DraftKey, _index: usize) {}
    /// The references of a sent (or queued) message, in send order. Empty by default.
    fn message_block_refs(&self, _message: &Message) -> Vec<chat_core::proto::BlockRef> {
        Vec::new()
    }
    /// [`ChatProvider::send_with_files`] with references (`send.blockRefs`); the server writes
    /// their quote lines in front of the body. The default drops the references.
    fn send_with_refs(
        &self,
        request: SendRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
        _refs: Vec<chat_core::proto::BlockRefInput>,
    ) -> ClientMsgId {
        self.send_with_files(request, key, files)
    }

    // --- end inc-refs track ---

    // --- settings-backend track: 设置 panes 账号 (devices), 通知 and 成员 (approval rules) ---
    // Every method has a default, so test and preview providers need no change.

    /// 通知: my notification settings (`GET /v1/me/notify-settings`), fresh from the server.
    fn notify_preferences(&self) -> AdminReply<NotifyPreferences> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 通知: saves every field of `prefs` (`PATCH /v1/me/notify-settings`) and answers with what
    /// the server stored. The time zone sent is this Mac's.
    fn set_notify_preferences(&self, _prefs: NotifyPreferences) -> AdminReply<NotifyPreferences> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 成员 › 默认规则: the team's approval rules for AI members (`GET /v1/settings/approvals`).
    /// Every member may read them.
    fn approval_defaults(&self) -> AdminReply<ApprovalDefaults> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 成员 › 默认规则, owners only: saves every field (`PATCH /v1/settings/approvals`). A member
    /// who is not an owner gets `AdminError::Forbidden`.
    fn set_approval_defaults(&self, _defaults: ApprovalDefaults) -> AdminReply<ApprovalDefaults> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 通用 › AI 服务, owners only: the team's provider and model, never the key
    /// (`GET /v1/settings/ai`).
    fn ai_service(&self) -> AdminReply<AiServiceSettings> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 通用 › AI 服务 保存 (`PUT /v1/settings/ai`); answers with what the server stored.
    fn set_ai_service(&self, _draft: AiServiceDraft) -> AdminReply<AiServiceSettings> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 通用 › AI 服务 测试连接 (`POST /v1/settings/ai/test`): nothing is saved.
    fn test_ai_service(&self, _draft: AiServiceDraft) -> AdminReply<AiTestOutcome> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 账号 › 登录的设备: my signed-in devices (`GET /v1/me/devices`), newest use first.
    fn devices(&self) -> AdminReply<Vec<DeviceInfo>> {
        reply_now(Err(AdminError::Unsupported))
    }
    /// 账号 › 登录的设备 `−`: signs out one of my devices (`DELETE /v1/me/devices/:id`). Use
    /// `sign_out` for this Mac (the row with `this_mac`).
    fn sign_out_device(&self, _device_id: &str) -> AdminReply<()> {
        reply_now(Err(AdminError::Unsupported))
    }

    // --- end settings-backend track ---

    // --- changes ---

    /// A new stream of change batches. Each call gives a new receiver; all
    /// receivers get every batch. The app drains it on the main thread through
    /// `app::ChangeHub`, so views do not call this themselves.
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>>;
}

/// Messages scanned for the open ask card of a 话题.
const ASK_CARD_SCAN: usize = 50;

/// The newest card in a 话题 that still waits on my answer
/// ([`card_waits_on`]), never my own card.
pub fn open_ask_card<P: ChatProvider + ?Sized>(
    p: &P,
    group_id: &str,
    topic_id: &str,
) -> Option<Message> {
    let me = my_id(p)?;
    p.messages(group_id, topic_id, ASK_CARD_SCAN)
        .messages
        .into_iter()
        .rev()
        .filter(|m| m.deleted_at.is_none() && m.author_id != me)
        .find(|m| card_waits_on(p, group_id, &m.id, &me))
}

/// The card of message `message_id` waits on `me`: a v2 card with an ask that
/// `me` has not answered at its rev, or a v1 card with buttons that `me` has
/// not pressed.
pub fn card_waits_on<P: ChatProvider + ?Sized>(
    p: &P,
    group_id: &str,
    message_id: &str,
    me: &str,
) -> bool {
    match p.card_v2(group_id, message_id) {
        Some(c) => {
            c.card.ask.is_some()
                && !p
                    .card_v2_answers(group_id, message_id)
                    .iter()
                    .any(|a| a.actor_id == me && a.rev == c.card.rev)
        }
        None => {
            p.card(group_id, message_id)
                .is_some_and(|c| c.actions.as_ref().is_some_and(|a| !a.is_empty()))
                && !p
                    .card_presses(group_id, message_id)
                    .iter()
                    .any(|x| x.actor_id == me)
        }
    }
}

/// My member id, when signed in.
fn my_id<P: ChatProvider + ?Sized>(p: &P) -> Option<MemberId> {
    match p.session() {
        SessionState::SignedIn { me, .. } => Some(me.id),
        _ => None,
    }
}

// --- settings-backend track: view types of 设置 › 账号 (devices), 通知 and 成员 (approval rules) ---

/// A wall-clock time in this Mac's time zone (the 每日摘要 and 勿扰 time pop-ups).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockTime {
    pub hour: u8,
    pub minute: u8,
}

impl ClockTime {
    pub const fn new(hour: u8, minute: u8) -> Self {
        Self { hour, minute }
    }
    /// "HH:MM" on a 24 h clock; `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let m = chat_core::proto::clock_minutes(text)?;
        Some(Self::new((m / 60) as u8, (m % 60) as u8))
    }
    /// "HH:MM", as the server and the pop-ups show it.
    pub fn wire(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

/// 设置 › 通知: group 提醒我 (one switch per row, 每日摘要 with its time) and group 勿扰.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifyPreferences {
    /// 需要我拍板的事.
    pub waiting: bool,
    /// 我交代的事做完了.
    pub result: bool,
    /// 成员卡住或出错.
    pub blocked: bool,
    /// 有人 @我.
    pub mention: bool,
    /// 每日摘要: `None` = switch off; `Some(time)` = on at that time.
    pub digest_at: Option<ClockTime>,
    /// 勿扰 switch.
    pub quiet_enabled: bool,
    /// 勿扰 从.
    pub quiet_start: ClockTime,
    /// 勿扰 到 (before 从 = the next morning).
    pub quiet_end: ClockTime,
}

impl NotifyPreferences {
    /// The time the 每日摘要 pop-up shows while the switch is off.
    pub const DIGEST_DEFAULT: ClockTime = ClockTime::new(9, 0);
}

impl Default for NotifyPreferences {
    fn default() -> Self {
        Self {
            waiting: true,
            result: true,
            blocked: true,
            mention: true,
            digest_at: None,
            quiet_enabled: false,
            quiet_start: ClockTime::new(22, 0),
            quiet_end: ClockTime::new(8, 0),
        }
    }
}

/// Who answers an AI member's approval requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ApproverRole {
    /// Owners only.
    Owners,
    /// The member who asked for the work, or an owner (the released rule).
    #[default]
    Requester,
}

/// 设置 › 成员 › 默认规则: what an AI member asks before, the spend limit and who approves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApprovalDefaults {
    /// 合并到 main.
    pub merge_main: bool,
    /// 发布到生产.
    pub deploy_prod: bool,
    /// 改生产环境配置 (not in the pane today; default on).
    pub prod_config: bool,
    /// 删除数据或文件 (not in the pane today; default on).
    pub delete_data: bool,
    /// 对外发布, social media and email (not in the pane today; default on).
    pub external_publish: bool,
    /// 单次花费超过 ¥…, whole yuan 1..=1_000_000; `None` = no limit.
    pub max_spend_cny: Option<u32>,
    pub approver: ApproverRole,
}

impl Default for ApprovalDefaults {
    fn default() -> Self {
        Self {
            merge_main: true,
            deploy_prod: true,
            prod_config: true,
            delete_data: true,
            external_publish: true,
            max_spend_cny: Some(50),
            approver: ApproverRole::Requester,
        }
    }
}

/// 通用 › AI 服务: who answers the server's own model calls (AI 话题标题).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AiProviderKind {
    Deepseek,
    Anthropic,
    /// Any provider with an OpenAI-style `/chat/completions` API at its own address.
    OpenaiCompatible,
}

impl AiProviderKind {
    /// In the order of the 服务商 menu: the cheapest first.
    pub const ALL: [AiProviderKind; 3] = [
        AiProviderKind::Deepseek,
        AiProviderKind::Anthropic,
        AiProviderKind::OpenaiCompatible,
    ];

    /// The model put in the field when the owner picks this provider.
    pub fn default_model(self) -> &'static str {
        match self {
            AiProviderKind::Deepseek => "deepseek-chat",
            AiProviderKind::Anthropic => "claude-haiku-5-5",
            AiProviderKind::OpenaiCompatible => "",
        }
    }

    /// True when the provider needs its own 接口地址.
    pub fn needs_base_url(self) -> bool {
        self == AiProviderKind::OpenaiCompatible
    }
}

/// The team's AI 服务 as the server keeps it. The key itself never reaches the client.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiServiceSettings {
    /// `None` = not set up: topics keep their first line as the title.
    pub provider: Option<AiProviderKind>,
    pub model: String,
    pub base_url: Option<String>,
    /// The key's last 4 characters; `None` when no key is saved.
    pub key_hint: Option<String>,
}

/// What the owner typed, for 测试连接 and 保存. `api_key: None` keeps the saved key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiServiceDraft {
    pub provider: AiProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

/// Why a draft cannot be sent yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiDraftProblem {
    ModelRequired,
    ModelInvalid,
    BaseUrlInvalid,
    KeyRequired,
    KeyInvalid,
}

impl AiServiceDraft {
    /// The same rules as the server (docs/protocol.md, "AI 服务"). Without a new key the saved
    /// one is used, which works only while the provider stays the same.
    pub fn check(&self, saved: &AiServiceSettings) -> Result<(), AiDraftProblem> {
        let token = |s: &str| !s.is_empty() && s.bytes().all(|b| (0x21..=0x7e).contains(&b));
        if self.model.is_empty() {
            return Err(AiDraftProblem::ModelRequired);
        }
        if self.model.len() > 128 || !token(&self.model) {
            return Err(AiDraftProblem::ModelInvalid);
        }
        if self.provider.needs_base_url() {
            let ok = self.base_url.as_deref().is_some_and(|u| {
                u.len() <= 256 && u.starts_with("https://") && token(&u["https://".len()..])
            });
            if !ok {
                return Err(AiDraftProblem::BaseUrlInvalid);
            }
        }
        match &self.api_key {
            Some(k) if k.len() < 8 || k.len() > 512 || !token(k) => Err(AiDraftProblem::KeyInvalid),
            Some(_) => Ok(()),
            None if saved.key_hint.is_some() && saved.provider == Some(self.provider) => Ok(()),
            None => Err(AiDraftProblem::KeyRequired),
        }
    }
}

/// The answer of 测试连接.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiTestOutcome {
    Ok {
        latency_ms: u64,
    },
    /// The provider's own error text (status and message), shown as it came.
    Failed(String),
}

/// The SF Symbol of a 登录的设备 row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// `laptopcomputer`.
    Laptop,
    /// `desktopcomputer`.
    Desktop,
    /// `iphone`.
    Phone,
}

impl DeviceKind {
    /// The SF Symbol name.
    pub fn symbol(self) -> &'static str {
        match self {
            DeviceKind::Laptop => "laptopcomputer",
            DeviceKind::Desktop => "desktopcomputer",
            DeviceKind::Phone => "iphone",
        }
    }
}

/// One signed-in device (账号 › 登录的设备).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// The device's record id (never a credential).
    pub id: String,
    /// The name given at sign in; may be empty.
    pub name: String,
    /// From the name: "iPhone" is a phone, "iMac", "Mac mini", "Mac Studio" and "Mac Pro" are
    /// desktops, anything else a laptop.
    pub kind: DeviceKind,
    pub signed_in_at: u64,
    /// `None` = never used since sign in.
    pub last_seen_at: Option<u64>,
    /// 这台 Mac: show `这台 Mac` instead of the last seen time; sign out with `sign_out`.
    pub this_mac: bool,
}

impl DeviceKind {
    /// The kind a device name suggests.
    pub fn from_name(name: &str) -> Self {
        let n = name.to_lowercase();
        if n.contains("iphone") || n.contains("ipad") {
            DeviceKind::Phone
        } else if [
            "imac",
            "mac mini",
            "macmini",
            "mac studio",
            "mac pro",
            "macpro",
        ]
        .iter()
        .any(|k| n.contains(k))
            && !n.contains("macbook")
        {
            DeviceKind::Desktop
        } else {
            DeviceKind::Laptop
        }
    }
}

// --- end settings-backend track ---

#[cfg(test)]
mod pinned_ask_tests {
    use super::*;
    use crate::views::cards::samples as cs;

    /// T-10: a 话题-level ask takes the newest open card's question and asker,
    /// not the 话题 creator; a pressed card no longer counts.
    #[test]
    fn topic_ask_names_the_newest_open_card() {
        let p = crate::fake::FakeProvider::new();
        let ask = p
            .pinned_ask(cs::GROUP, cs::TOPIC)
            .expect("the cards 话题 waits on me");
        let card = open_ask_card(&*p, cs::GROUP, cs::TOPIC).expect("an open ask card");
        assert_eq!(ask.message_id.as_deref(), Some(card.id.as_str()));
        assert_eq!(ask.from, card.author_id);
        assert_eq!(ask.at, card.created_at);
        let title = p.card(cs::GROUP, &card.id).map(|c| c.title.clone());
        assert_eq!(Some(ask.title), title);
        let newest = p.messages(cs::GROUP, cs::TOPIC, 50).messages;
        assert_eq!(newest.last().map(|m| &m.id), Some(&card.id));

        let me = my_id(&*p).expect("signed in");
        assert!(card_waits_on(&*p, cs::GROUP, &card.id, &me));
        p.press_card_action(cs::GROUP, cs::TOPIC, &card.id, "staging-now");
        assert!(!card_waits_on(&*p, cs::GROUP, &card.id, &me));
        let next = p
            .pinned_ask(cs::GROUP, cs::TOPIC)
            .expect("an older card still waits");
        assert_ne!(next.message_id.as_deref(), Some(card.id.as_str()));
    }
}
