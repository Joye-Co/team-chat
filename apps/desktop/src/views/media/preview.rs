//! Screenshot self-test scenes for the media views (Media.dc.html). Every scene opens the
//! real main window on the sample team: the first 话题 gets messages that point at sample
//! 产出 written to a temp folder and read by the real pipeline ([`store::analyze`]:
//! AVFoundation frames, audio peaks, PDFKit pages). A `media-<kind>` scene then opens
//! the 产出 viewer sheet over the thread, as a click on the preview does;
//! `media-inline-*` show the inline previews alone; `media-viewer-loading` and
//! `media-viewer-failed` the sheet before and after a read that did not work.
//! `media-states` shows the upload and preview rows.

use std::{path::PathBuf, sync::Arc};

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, IntoElement, ParentElement, Pixels, Render,
    Styled, Window, WindowBounds, div, px,
};

use super::{
    MediaKind,
    inline::InlineSpec,
    state::Target,
    states::{self, Lead, Status},
    store::{self, Data, MediaKey},
    viewer::{self, Opening},
};
use crate::{
    app::{Services, main_window_options},
    copy::media as cm,
    data::*,
    selftest::scenes::{capture_bounds, open_main},
    theme::{self, Type as _, space},
    ui::{
        Sym,
        web_embed::{Phase, live},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sample {
    Html,
    Markdown,
    Images,
    Video,
    Audio,
    Pdf,
    Code,
    Table,
    States,
    /// Inline previews of a page, a document and a PDF.
    InlineDocs,
    /// Inline previews of code and a table.
    InlineData,
    /// Inline previews of images, a video and audio.
    InlineAv,
    /// The viewer sheet while the version is read.
    ViewerLoading,
    /// The viewer sheet after the version could not be read.
    ViewerFailed,
}

impl Sample {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "media-html" => Sample::Html,
            "media-markdown" => Sample::Markdown,
            "media-images" => Sample::Images,
            "media-video" => Sample::Video,
            "media-audio" => Sample::Audio,
            "media-pdf" => Sample::Pdf,
            "media-code" => Sample::Code,
            "media-table" => Sample::Table,
            "media-states" => Sample::States,
            "media-inline-docs" => Sample::InlineDocs,
            "media-inline-data" => Sample::InlineData,
            "media-inline-av" => Sample::InlineAv,
            "media-viewer-loading" => Sample::ViewerLoading,
            "media-viewer-failed" => Sample::ViewerFailed,
            _ => return None,
        })
    }

    fn key(self) -> MediaKey {
        let id = match self {
            Sample::Html => "sample-report",
            Sample::Markdown => "sample-review",
            Sample::Images => "sample-images",
            Sample::Video => "sample-video",
            Sample::Audio => "sample-audio",
            Sample::Pdf => "sample-pdf",
            Sample::Code | Sample::States | Sample::InlineData => "sample-code",
            Sample::Table => "sample-table",
            Sample::InlineDocs | Sample::InlineAv => "sample-none",
            Sample::ViewerLoading => "sample-pending",
            Sample::ViewerFailed => "sample-broken",
        };
        MediaKey::new(id, if self == Sample::Html { 3 } else { 1 })
    }

    /// The samples a scene needs on disk and read.
    fn needs(self) -> Vec<Sample> {
        match self {
            Sample::Code | Sample::InlineData => vec![Sample::Code, Sample::Table],
            Sample::States => vec![Sample::Video],
            Sample::InlineDocs => vec![Sample::Html, Sample::Markdown, Sample::Pdf],
            Sample::InlineAv => vec![Sample::Images, Sample::Video, Sample::Audio],
            Sample::ViewerLoading | Sample::ViewerFailed => vec![Sample::Code],
            s => vec![s],
        }
    }

    /// The sample whose viewer sheet the scene opens.
    fn viewer(self) -> Option<Sample> {
        match self {
            Sample::InlineDocs | Sample::InlineData | Sample::InlineAv | Sample::States => None,
            s => Some(s),
        }
    }
}

struct Post {
    author: &'static str,
    text: &'static str,
}

fn post(s: Sample) -> Post {
    match s {
        Sample::Html => Post {
            author: "m-alex",
            text: "月报更新到 v3，加了渠道拆分。",
        },
        Sample::Markdown => Post {
            author: "m-coder",
            text: "复盘写好了，后续还有两项没做。",
        },
        Sample::Code => Post {
            author: "m-coder",
            text: "改动在这个文件里，重连从最后确认的 seq 继续。",
        },
        Sample::Images => Post {
            author: "m-alex",
            text: "配图改好了，一共 9 张，第 1 张当封面。和草稿一起周四 20:00 发。",
        },
        Sample::Video | Sample::States => Post {
            author: "m-coder",
            text: "录屏在这里：断网、重连、合盖休眠都没再重复。",
        },
        Sample::Audio => Post {
            author: "m-alex",
            text: "周会录音和文字稿整理好了。",
        },
        Sample::Pdf => Post {
            author: "m-coder",
            text: "今天的巡检报告，24 项都通过。",
        },
        _ => Post {
            author: "m-alex",
            text: "原始数据也放上来了，月报里的数都从这里算。",
        },
    }
}

/// The 产出 title of a sample (a long one with `--worst-case`).
fn title(s: Sample) -> &'static str {
    let worst = crate::selftest::worst_case();
    match s {
        Sample::Html if worst => {
            "2025 年第三季度全渠道增长复盘、预算调整建议与下季度投放计划（终版，含附录和原始数据口径说明）"
        }
        Sample::Html => "10 月数据月报",
        Sample::Markdown if worst => {
            "WebSocket 断线重连后消息重复的完整复盘：时间线、根因、修复方案、回归测试和后续待办"
        }
        Sample::Markdown => "重连重复问题复盘",
        Sample::Images => "夹夹 3.0 配图",
        Sample::Video | Sample::States => "重连测试录屏",
        Sample::Audio => "周会录音 11-03",
        Sample::Pdf => "巡检报告-11-03",
        Sample::Code => "reconnect.ts",
        Sample::Table => "10 月各渠道新增",
        _ => "",
    }
}

/// One message a scene adds to the end of the thread.
#[derive(Clone, Debug)]
pub struct Extra {
    pub author: String,
    pub body: String,
    pub streaming: bool,
    /// The 产出 version the message points at.
    pub artifact: Option<MediaKey>,
}

/// The main window's first 话题 with a scene's messages at its end (the media and
/// diagram scenes): the real thread, not a drawn copy of it.
pub struct MediaThread {
    inner: Arc<dyn ChatProvider>,
    group_id: String,
    topic_id: String,
    extras: Vec<Extra>,
    /// Folder and entry of each sample on disk.
    folders: Vec<(Sample, PathBuf, String)>,
}

impl MediaThread {
    /// The thread with `extras` at its end and no 产出 of its own.
    pub fn with_messages(inner: Arc<dyn ChatProvider>, extras: Vec<Extra>) -> Arc<Self> {
        let (group_id, topic_id) = first_topic(&inner);
        Arc::new(Self {
            inner,
            group_id,
            topic_id,
            extras,
            folders: Vec::new(),
        })
    }

    fn new(inner: Arc<dyn ChatProvider>, samples: Vec<Sample>) -> Arc<Self> {
        let (group_id, topic_id) = first_topic(&inner);
        let folders = samples
            .iter()
            .filter_map(|s| write_sample(*s).map(|(dir, entry)| (*s, dir, entry)))
            .collect();
        let extras = samples
            .iter()
            .map(|s| {
                let p = post(*s);
                Extra {
                    author: p.author.into(),
                    body: p.text.into(),
                    streaming: false,
                    artifact: Some(s.key()),
                }
            })
            .collect();
        Arc::new(Self {
            inner,
            group_id,
            topic_id,
            extras,
            folders,
        })
    }

    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    pub fn topic_id(&self) -> &str {
        &self.topic_id
    }

    fn mine(&self, g: &str, t: &str) -> bool {
        g == self.group_id && t == self.topic_id
    }

    fn extra_messages(&self) -> Vec<Message> {
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        self.extras
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let at = now - 600_000 + i as i64 * 60_000;
                Message {
                    id: format!("extra-{i}"),
                    client_msg_id: format!("c-extra-{i}"),
                    group_id: self.group_id.clone(),
                    topic_id: self.topic_id.clone(),
                    author_id: x.author.clone(),
                    body: x.body.clone(),
                    mentions: Vec::new(),
                    created_seq: 900_000 + i as u64,
                    created_at: at,
                    edited_at: None,
                    deleted_at: None,
                    streaming: x.streaming,
                    send_state: SendState::Sent,
                    quote_of: None,
                    attachments: Vec::new(),
                    artifact_refs: x
                        .artifact
                        .iter()
                        .map(|key| ArtifactRef {
                            artifact_id: key.artifact_id.clone(),
                            version: key.version,
                        })
                        .collect(),
                    reactions: Vec::new(),
                }
            })
            .collect()
    }

    fn detail(&self, s: Sample, dir: &std::path::Path, entry: &str) -> ArtifactDetail {
        let key = s.key();
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        let author = post(s).author.to_string();
        let files = list(dir);
        ArtifactDetail {
            summary: ArtifactSummary {
                id: key.artifact_id.clone(),
                topic_id: self.topic_id.clone(),
                kind: ArtifactKind::Page,
                title: title(s).into(),
                version: key.version,
                size_bytes: Some(files.iter().map(|f| f.size).sum()),
                created_by: author.clone(),
                updated_at: now - 120_000,
            },
            group_id: self.group_id.clone(),
            versions: (1..=key.version)
                .rev()
                .map(|v| ArtifactVersionInfo {
                    version: v,
                    note: None,
                    created_by: author.clone(),
                    created_at: now - 1_000_000 + v as i64,
                    entry: entry.to_string(),
                    files: files.clone(),
                })
                .collect(),
        }
    }
}

/// The first 群 and its first ordinary 话题 (the one `open_first_topic` opens).
fn first_topic(inner: &Arc<dyn ChatProvider>) -> (String, String) {
    let group = inner.groups().first().cloned();
    let topic = group.as_ref().and_then(|g| {
        inner
            .topics(&g.id)
            .into_iter()
            .find(|t| t.id != crate::fake::LONG_TOPIC)
    });
    (
        group.map(|g| g.id).unwrap_or_default(),
        topic.map(|t| t.id).unwrap_or_default(),
    )
}

/// The files of a sample folder, as a version lists them.
fn list(dir: &std::path::Path) -> Vec<ArtifactVersionFile> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(rel) = p.strip_prefix(dir) {
                out.push(ArtifactVersionFile {
                    path: rel.to_string_lossy().replace('\\', "/"),
                    size: e.metadata().map(|m| m.len()).unwrap_or(0),
                    mime: String::new(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

impl ChatProvider for MediaThread {
    fn session(&self) -> SessionState {
        self.inner.session()
    }
    fn login(&self, request: LoginRequest) {
        self.inner.login(request)
    }
    fn logout(&self) {
        self.inner.logout()
    }
    fn connection(&self) -> ConnectionState {
        self.inner.connection()
    }
    fn members(&self) -> Vec<Member> {
        self.inner.members()
    }
    fn member(&self, id: &str) -> Option<Member> {
        self.inner.member(id)
    }
    fn groups(&self) -> Vec<GroupSummary> {
        self.inner.groups()
    }
    fn topics(&self, group_id: &str) -> Vec<Topic> {
        self.inner.topics(group_id)
    }
    fn topic(&self, group_id: &str, topic_id: &str) -> Option<Topic> {
        self.inner.topic(group_id, topic_id)
    }
    fn todo(&self) -> Vec<TodoItem> {
        self.inner.todo()
    }
    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        let mut w = self.inner.messages(group_id, topic_id, limit);
        if self.mine(group_id, topic_id) {
            w.messages.extend(self.extra_messages());
        }
        w
    }
    fn load_older(&self, group_id: &str, topic_id: &str) {
        self.inner.load_older(group_id, topic_id)
    }
    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId> {
        self.inner.typing(group_id, topic_id)
    }
    fn artifacts(&self, group_id: &str, topic_id: &str) -> Vec<ArtifactSummary> {
        if self.mine(group_id, topic_id) {
            self.folders
                .iter()
                .map(|(s, dir, entry)| self.detail(*s, dir, entry).summary)
                .collect()
        } else {
            self.inner.artifacts(group_id, topic_id)
        }
    }
    fn artifact(&self, artifact_id: &str) -> Option<ArtifactDetail> {
        self.folders
            .iter()
            .find(|(s, _, _)| s.key().artifact_id == artifact_id)
            .map(|(s, dir, entry)| self.detail(*s, dir, entry))
            .or_else(|| self.inner.artifact(artifact_id))
    }
    fn send(&self, request: SendRequest) -> ClientMsgId {
        self.inner.send(request)
    }
    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        self.inner.start_topic(request)
    }
    fn edit(&self, group_id: &str, message_id: &str, body: String) {
        self.inner.edit(group_id, message_id, body)
    }
    fn delete(&self, group_id: &str, message_id: &str) {
        self.inner.delete(group_id, message_id)
    }
    fn set_topic_status(&self, g: &str, t: &str, status: TopicStatus, waiting_on: Vec<MemberId>) {
        self.inner.set_topic_status(g, t, status, waiting_on)
    }
    fn mark_read(&self, group_id: &str, topic_id: &str, seq: u64) {
        self.inner.mark_read(group_id, topic_id, seq)
    }
    fn set_typing(&self, group_id: &str, topic_id: &str) {
        self.inner.set_typing(group_id, topic_id)
    }
    fn draft(&self, key: &DraftKey) -> Draft {
        self.inner.draft(key)
    }
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        self.inner.subscribe()
    }
}

// --- openers --------------------------------------------------------------------------

/// The scene opener: the main window on the sample thread, then the viewer sheet of
/// the scene's sample (`media-<kind>`, `media-viewer-*`).
pub fn open(
    services: Services,
    content: gpui::Size<Pixels>,
    sample: Sample,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    if sample == Sample::States {
        return open_states(services, content, cx);
    }
    store::clear(cx);
    let shown = match sample {
        Sample::InlineDocs | Sample::InlineData | Sample::InlineAv => sample.needs(),
        Sample::ViewerLoading | Sample::ViewerFailed => vec![Sample::Code],
        Sample::Code => vec![Sample::Code, Sample::Table],
        s => vec![s],
    };
    let thread = MediaThread::new(services.provider.clone(), shown);
    for (s, dir, entry) in &thread.folders {
        store::request_folder(&s.key(), title(*s), dir.clone(), entry.clone(), cx);
    }
    let provider: Arc<dyn ChatProvider> = thread.clone();
    let services = Services::new(provider.clone(), cx);
    crate::selftest::scenes::open_first_topic(&services, cx);
    let handle = open_main(services, content, cx)?;
    if let Some(v) = sample.viewer() {
        let target = match v {
            // No provider: nothing reads the version, so the sheet stays loading.
            Sample::ViewerLoading => Target::none(),
            _ => Target {
                provider: Some(provider.clone()),
                group_id: thread.group_id.clone(),
                topic_id: thread.topic_id.clone(),
            },
        };
        let opening = match v {
            Sample::Images => Opening::at(2),
            Sample::Video => Opening::frame(3),
            _ => Opening::default(),
        };
        let name = match v {
            Sample::ViewerLoading | Sample::ViewerFailed => title(Sample::Code),
            s => title(s),
        };
        handle
            .update(cx, |_, window, cx| {
                viewer::open_in(window, v.key(), name.to_string(), target, opening, cx);
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(handle)
}

/// The upload and preview rows (`media-states`) in a thread-width column.
fn open_states(
    services: Services,
    content: gpui::Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    store::clear(cx);
    if let Some((dir, entry)) = write_sample(Sample::Video) {
        store::request_folder(&Sample::Video.key(), title(Sample::Video), dir, entry, cx);
    }
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    let _ = services;
    gpui_kit::open_window(options, cx, move |_, cx| cx.new(|_| StatesPreview))
        .map(|(handle, _)| handle)
        .map_err(|e| e.to_string())
}

/// True once every sample of the scene is read and its pages (if any) drew.
pub fn ready(sample: Sample, cx: &App) -> bool {
    let read = sample
        .needs()
        .iter()
        .all(|s| store::ready(&s.key(), cx).is_some());
    if !read {
        return false;
    }
    if sample == Sample::ViewerFailed
        && !matches!(store::get(&sample.key(), cx), Some(store::Load::Failed(_)))
    {
        return false;
    }
    // Every page drew; a page under the sheet hides behind its snapshot, so at least one
    // shows.
    let pages = live(cx);
    let pages_ok = pages.iter().all(|p| *p.read(cx).phase() == Phase::Ready)
        && (pages.is_empty() || pages.iter().any(|p| p.read(cx).shown()));
    // Pages: an HTML 产出 inline or in the sheet, and the video player in the sheet.
    let wants_pages = matches!(sample.viewer(), Some(Sample::Html | Sample::Video))
        || sample.needs().contains(&Sample::Html);
    pages_ok && (!wants_pages || !pages.is_empty())
}

/// The `media-states` harness: the rows in one thread-width column (640).
struct StatesPreview;

impl Render for StatesPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .size_full()
            .bg(k.content)
            .text_color(k.text_1)
            .flex()
            .justify_center()
            .pt(space::S48)
            .child(
                div()
                    .w(crate::ui::native::tokens::card_v2::MAX_WIDTH)
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(space::S24)
                    .child(states_column(cx))
                    .child(uploads_column(cx)),
            )
    }
}

/// A file name of the states scene: a long one with `--worst-case` (F65).
fn file_name(normal: &'static str, worst: &'static str) -> &'static str {
    if crate::selftest::worst_case() {
        worst
    } else {
        normal
    }
}

/// A size of the states scene: `worst` with `--worst-case` (0 B and 1 TB sizes).
fn bytes(normal: u64, worst: u64) -> u64 {
    if crate::selftest::worst_case() {
        worst
    } else {
        normal
    }
}

fn states_column(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let label = |t: &'static str| div().t_small().semibold().text_color(k.text_1).child(t);
    div()
        .w(px(600.))
        .flex_none()
        .flex()
        .flex_col()
        .gap(space::S12)
        .child(label(cm::STATES_TITLE))
        .child(states::row(
            "st-up",
            Lead::Icon(Sym::Video),
            file_name(
                "重连后重复-第二次.mov",
                "IMG_20251103_183022_HDR_portrait_重连后消息重复的第二次完整录屏_最终版_v12_edited.mov",
            ),
            &Status::Uploading {
                done: bytes(2_400_000, 0),
                total: bytes(3_900_000, 1_099_511_627_776),
            },
            Some(states::action("st-cancel", cm::CANCEL, None, cx)),
            cx,
        ))
        .child(states::row(
            "st-gen",
            Lead::Blank,
            "重连后重复-第二次.mov",
            &Status::Generating {
                duration: Some(21.),
            },
            None,
            cx,
        ))
        .child(states::row(
            "st-ok",
            store::ready(&Sample::Video.key(), cx)
                .and_then(|m| match &m.data {
                    Data::Video(v) => v.poster.clone().map(Lead::Thumb),
                    _ => None,
                })
                .unwrap_or(Lead::Blank),
            "重连后重复-第二次.mov",
            &Status::Ready {
                meta: "0:21 · 3.9 MB · 小王 · 刚刚".into(),
            },
            Some(states::ready_mark(cx)),
            cx,
        ))
        .child(states::row(
            "st-fail",
            Lead::Icon(Sym::File),
            file_name(
                "夹夹 3.0 发布会.key",
                "夹夹 3.0 发布会_全渠道版_含演讲者备注和全部动画_最终确认版本_请勿再改.key",
            ),
            &Status::Failed {
                size: bytes(18_600_000, 0),
            },
            Some(states::action(
                "st-sys",
                cm::OPEN_WITH_SYSTEM,
                Some(Sym::OpenExternal),
                cx,
            )),
            cx,
        ))
        .child(states::row(
            "st-big",
            Lead::Icon(Sym::Video),
            file_name(
                "周会全程-4K.mov",
                "2025 年 11 月 3 日全员周会全程录像（4K，含屏幕共享和全部问答环节）.mov",
            ),
            &Status::TooLarge {
                limit: 2_000_000_000,
                size: bytes(3_400_000_000, 1_099_511_627_776),
            },
            Some(states::action("st-rm", cm::REMOVE, None, cx)),
            cx,
        ))
}

fn uploads_column(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let spec = InlineSpec {
        key: MediaKey::new("sample-missing", 1),
        kind: MediaKind::Pdf,
        title: "巡检报告-11-02".into(),
        bytes: 2_000_000,
        target: Target::none(),
        id: "scene-loading".into(),
    };
    let big = InlineSpec {
        key: MediaKey::new("sample-big", 1),
        kind: MediaKind::Video,
        title: "周会全程.mov".into(),
        bytes: 900_000_000,
        target: Target::none(),
        id: "scene-big".into(),
    };
    div()
        .w(px(600.))
        .flex_none()
        .flex()
        .flex_col()
        .gap(space::S12)
        .child(
            div()
                .t_small()
                .semibold()
                .text_color(k.text_1)
                .child(cm::STATE_LOADING),
        )
        .child(states::big_row(&big, cx))
        .child(states::failed_row(&spec, cx))
}

/// Checks what the pipeline produced (frames, peaks, pages, rows).
pub fn verify(sample: Sample, cx: &App) -> Result<(), String> {
    for s in sample.needs() {
        let m = store::ready(&s.key(), cx).ok_or("not read")?;
        if m.preview_failed {
            return Err(format!("{s:?}: the preview failed"));
        }
        match &m.data {
            Data::Video(v) if v.frames.len() != store::FRAMES || v.poster.is_none() => {
                return Err(format!(
                    "video: {} frames, poster {}",
                    v.frames.len(),
                    v.poster.is_some()
                ));
            }
            Data::Audio(a) if a.peaks.iter().filter(|p| **p > 0.05).count() < 100 => {
                return Err("audio: no waveform".into());
            }
            Data::Pdf(p) if p.pages != 6 || p.images.len() != 6 => {
                return Err(format!("pdf: {} pages, {} drawn", p.pages, p.images.len()));
            }
            Data::Table(t) if t.total_rows != 124 => {
                return Err(format!("table: {} rows", t.total_rows));
            }
            Data::Images(p) if p.len() != 9 => return Err(format!("images: {}", p.len())),
            _ => {}
        }
    }
    Ok(())
}

// --- sample 产出 ------------------------------------------------------------------------

fn sample_dir(s: Sample) -> PathBuf {
    std::env::temp_dir()
        .join("tc-media-samples")
        .join(format!("{s:?}").to_lowercase())
}

/// Writes one sample version folder. Answers its folder and entry.
fn write_sample(s: Sample) -> Option<(PathBuf, String)> {
    let dir = sample_dir(s);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let w = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).ok();
    let entry = match s {
        Sample::Html => {
            w("index.html", REPORT_HTML.as_bytes())?;
            "index.html"
        }
        Sample::Markdown => {
            w("重连重复问题复盘.md", REVIEW_MD.as_bytes())?;
            "重连重复问题复盘.md"
        }
        Sample::Images => {
            for i in 0..9 {
                w(&format!("{:02}.png", i + 1), &poster_png(i))?;
            }
            "01.png"
        }
        Sample::Video | Sample::States => {
            w("重连测试录屏.mp4", DEMO_MP4)?;
            w("chapters.vtt", CHAPTERS_VTT.as_bytes())?;
            "重连测试录屏.mp4"
        }
        Sample::Audio => {
            w("周会录音 11-03.wav", &speech_wav(90))?;
            w("transcript.vtt", TRANSCRIPT_VTT.as_bytes())?;
            "周会录音 11-03.wav"
        }
        Sample::Pdf => {
            w("巡检报告-11-03.pdf", &report_pdf())?;
            "巡检报告-11-03.pdf"
        }
        Sample::Code => {
            std::fs::create_dir_all(dir.join("src/net")).ok()?;
            w("src/net/reconnect.ts", RECONNECT_TS.as_bytes())?;
            "src/net/reconnect.ts"
        }
        Sample::Table => {
            w("10 月各渠道新增.csv", channels_csv().as_bytes())?;
            "10 月各渠道新增.csv"
        }
        // Scenes, not samples: nothing on disk.
        Sample::InlineDocs
        | Sample::InlineData
        | Sample::InlineAv
        | Sample::ViewerLoading
        | Sample::ViewerFailed => return None,
    };
    Some((dir, entry.to_string()))
}

/// A 9-card social media set: tinted card, title band, a phone outline with tiles.
fn poster_png(i: usize) -> Vec<u8> {
    const BG: [[u8; 3]; 9] = [
        [0xE4, 0xD3, 0xC3],
        [0xC9, 0xD5, 0xD3],
        [0xDB, 0xD3, 0xC6],
        [0x6E, 0x66, 0x5E],
        [0xC7, 0xCB, 0xCF],
        [0xD5, 0xD0, 0xC8],
        [0xCF, 0xC4, 0xB8],
        [0xBF, 0xC8, 0xC4],
        [0xD9, 0xCE, 0xC0],
    ];
    let (w, h) = (414u32, 552u32);
    let bg = BG[i];
    let dark = i == 3;
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([bg[0], bg[1], bg[2], 255]));
    let mut rect = |x0: u32, y0: u32, x1: u32, y1: u32, c: [u8; 3]| {
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                img.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
            }
        }
    };
    let ink = if dark {
        [0xF1, 0xF1, 0xF3]
    } else {
        [0x1A, 0x1A, 0x1C]
    };
    rect(28, 34, 60, 40, ink);
    rect(28, 58, 200 + (i as u32 * 13) % 60, 82, ink);
    rect(
        28,
        96,
        150,
        104,
        if dark {
            [0xB0, 0xAC, 0xA6]
        } else {
            [0x7A, 0x74, 0x6C]
        },
    );
    // Phone.
    rect(112, 210, 302, 552, [0x12, 0x12, 0x14]);
    let screen = if dark {
        [0x24, 0x24, 0x28]
    } else {
        [0xFF, 0xFF, 0xFF]
    };
    rect(120, 218, 294, 552, screen);
    let tile = if dark {
        [0x3A, 0x3A, 0x40]
    } else {
        [0xEE, 0xEE, 0xF0]
    };
    for r in 0..3u32 {
        for c in 0..3u32 {
            if (r as usize * 3 + c as usize + i) % 4 == 3 {
                continue;
            }
            let (x, y) = (134 + c * 52, 236 + r * 52);
            rect(x, y, x + 44, y + 44, tile);
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// `seconds` of speech-like sound: syllable bursts of noise under a slow envelope, with
/// pauses between sentences. 16-bit mono PCM at 8 kHz.
fn speech_wav(seconds: u32) -> Vec<u8> {
    let rate = 8000u32;
    let n = rate * seconds;
    let mut seed = 0x2545F491u32;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32) * 2. - 1.
    };
    let mut samples = Vec::with_capacity(n as usize);
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let sentence = ((t * 0.31).sin() * 0.5 + 0.5).powf(0.6);
        let pause = if (t % 7.3) > 6.4 { 0.05 } else { 1. };
        let syllable = ((t * 4.7 * std::f32::consts::TAU).sin() * 0.5 + 0.5).powf(2.);
        let a = 0.08 + 0.9 * sentence * syllable * pause;
        let tone = (t * 180. * std::f32::consts::TAU).sin() * 0.4 + rnd() * 0.6;
        samples.push((tone * a * 12000.) as i16);
    }
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// A 6-page inspection report: a summary page and table pages (ASCII text, Helvetica).
fn report_pdf() -> Vec<u8> {
    let checks = [
        "Login",
        "Send message",
        "Album share",
        "Recall sync",
        "Alert push",
        "Reminders",
        "Image upload",
        "Stickers",
        "Welcome",
        "Keyword reply",
        "Data export",
        "Permissions",
    ];
    let mut pages: Vec<String> = Vec::new();
    for p in 0..6 {
        let mut c = String::new();
        let text = |c: &mut String, x: f32, y: f32, size: f32, s: &str| {
            c.push_str(&format!("BT /F1 {size} Tf {x} {y} Td ({s}) Tj ET\n"));
        };
        if p == 0 {
            text(&mut c, 56., 760., 22., "Daily check report");
            text(&mut c, 56., 736., 11., "staging  -  11-03  -  Scout");
            for (i, (k, v)) in [("Checks", "24"), ("Passed", "24"), ("Time", "2m 08s")]
                .iter()
                .enumerate()
            {
                let x = 56. + i as f32 * 168.;
                c.push_str(&format!("0.85 G {x} 640 150 70 re S\n"));
                text(&mut c, x + 12., 688., 10., k);
                text(&mut c, x + 12., 656., 20., v);
            }
            text(&mut c, 56., 600., 13., "Summary");
            for (i, l) in [
                "All 24 checks passed on staging. Median response 0.42 s.",
                "Each check ran three times; the median is reported.",
                "No alerts in the last 24 hours.",
            ]
            .iter()
            .enumerate()
            {
                text(&mut c, 56., 578. - i as f32 * 18., 11., l);
            }
        } else {
            text(
                &mut c,
                56.,
                760.,
                18.,
                &format!(
                    "Checks {}-{}",
                    (p - 1) * 12 % 24 + 1,
                    (p - 1) * 12 % 24 + 12
                ),
            );
            text(&mut c, 56., 724., 10., "Check");
            text(&mut c, 360., 724., 10., "Result");
            text(&mut c, 480., 724., 10., "Response");
            for (i, name) in checks.iter().enumerate() {
                let y = 700. - i as f32 * 30.;
                c.push_str(&format!("0.9 G 56 {} m 540 {} l S\n", y - 10., y - 10.));
                text(&mut c, 56., y, 11., name);
                text(&mut c, 360., y, 11., "pass");
                text(
                    &mut c,
                    480.,
                    y,
                    11.,
                    &format!("{:.2} s", 0.15 + ((i * 7 + p) % 13) as f32 * 0.09),
                );
            }
        }
        text(&mut c, 290., 40., 9., &format!("{} / 6", p + 1));
        pages.push(c);
    }
    // Objects: 1 catalog, 2 pages, 3 font, then (page, content) pairs.
    let mut objs: Vec<String> = Vec::new();
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 4 + i * 2))
        .collect();
    objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    objs.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        pages.len()
    ));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    for (i, c) in pages.iter().enumerate() {
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + i * 2
        ));
        objs.push(format!("<< /Length {} >>\nstream\n{}endstream", c.len(), c));
    }
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, o));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objs.len() + 1
    ));
    for o in offsets {
        out.push_str(&format!("{o:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objs.len() + 1
    ));
    out.into_bytes()
}

fn channels_csv() -> String {
    let mut s = String::from("日期,渠道,新增群,新增用户,日活,花费\n");
    let chans = [("小红书", 105), ("B 站", 61), ("抖音", 81), ("群内邀请", 0)];
    let mut n = 0;
    for d in 1..=31 {
        for (i, (c, cost)) in chans.iter().enumerate() {
            if n == 124 {
                break;
            }
            let users = 8 + (d * 7 + i * 13) % 31;
            let groups = (d + i) % 3;
            let dau = 2100 + d * 4 + i * 17;
            s.push_str(&format!(
                "10-{d:02},{c},{groups},{users},\"{},{:03}\",¥{}\n",
                dau / 1000,
                dau % 1000,
                cost + (d % 5)
            ));
            n += 1;
        }
    }
    s
}

const DEMO_MP4: &[u8] = include_bytes!("preview/demo.mp4");

const CHAPTERS_VTT: &str = "WEBVTT\n\n00:00.000 --> 00:08.000\n两个窗口都在线\n\n00:08.000 --> 00:15.000\n断网 5 秒\n\n00:15.000 --> 00:24.000\n重连，指数退避\n\n00:24.000 --> 00:33.000\n补发 3 条，没有重复\n\n00:33.000 --> 00:42.000\n合盖休眠后再试\n";

const TRANSCRIPT_VTT: &str = "WEBVTT\n\n00:04.000 --> 00:11.000\n<v Joye>这周先把断线重连的 PR 合了，Forge 的录屏我看过了。\n\n00:11.000 --> 00:19.000\n<v 小王>我本地也试了，合盖之后没再重复。\n\n00:19.000 --> 00:30.000\n<v Joye>qqbot v2.14 等 Scout 在 staging 验收完就发。\n\n00:30.000 --> 00:44.000\n<v 李雪>夹夹 3.0 的小红书定在周四 20:00，配图 Taro 改好了。\n\n00:44.000 --> 00:58.000\n<v Joye>B 站的预算我在月报里选了，Ledger 接着算一个方案。\n\n00:58.000 --> 01:10.000\n<v 李雪>抖音那边先停一周，看看每个新增的成本。\n\n01:10.000 --> 01:24.000\n<v 小王>下周我补一个多端同时在线的测试。\n";

const RECONNECT_TS: &str = r#"import { Message, SyncState } from "./types";

// 从最后一个确认过的 seq 继续，不再用本地最新的 seq
export function resumeFrom(s: SyncState): number {
  return s.lastAckedSeq + 1;
}

export function dedupe(batch: Message[], seen: Set<number>) {
  const fresh: Message[] = [];
  for (const msg of batch) {
    if (seen.has(msg.seq)) continue;
    seen.add(msg.seq);
    fresh.push(msg);
  }
  return fresh;
}

// 指数退避，上限 30 秒
export const backoff = (attempt: number) =>
  Math.min(30_000, 500 * 2 ** attempt);
"#;

const REVIEW_MD: &str = r#"# 重连重复问题复盘

## 原因

原因是客户端重连后从旧的 seq 重新拉取，服务器把已经收到的消息又推了一遍。

## 修法

1. 按 `seq` 去重，同一条只进列表一次。
2. 重连时从最后一个确认过的 seq 继续。
3. 重连间隔改成指数退避，上限 30 秒。

```ts
// reconnect.ts
export function resumeFrom(s: SyncState) {
  return s.lastAckedSeq + 1;
}
```

## 验证

| 场景 | 结果 | 重连用时 | 重复 |
| --- | --- | ---: | ---: |
| 断网 5 秒 | 通过 | 0.6 秒 | 0 |
| 断网 60 秒 | 通过 | 1.8 秒 | 0 |
| 合盖休眠 10 分钟 | 通过 | 1.2 秒 | 0 |
| 切换 Wi-Fi | 通过 | 0.9 秒 | 0 |
| 服务器重启 | 通过 | 1.5 秒 | 0 |
| 弱网 | 通过 | 1.7 秒 | 0 |

![重连时从最后确认的 seq 继续](seq.png)

## 后续

- [x] 按 seq 去重
- [x] 指数退避
- [ ] 补一个多端同时在线的测试
- [ ] 观察一周旧客户端的兼容
"#;

const REPORT_HTML: &str = r##"<!doctype html><html><head><meta charset="utf-8"><style>
:root{color-scheme:light dark;--t1:#111113;--t2:#52525B;--t3:#6E6E76;--line:#EBEBED;--ok:#177245;--bg:#fff}
@media (prefers-color-scheme: dark){:root{--t1:#F1F1F3;--t2:#A9A9B2;--t3:#8C8C95;--line:#252529;--ok:#6AD69C;--bg:#111113}}
html,body{margin:0;background:transparent;color:var(--t1);font:13px/1.5 -apple-system,"PingFang SC",sans-serif;-webkit-font-smoothing:antialiased}
.w{padding:16px 18px}h1{font-size:16px;margin:0;font-weight:600}.s{color:var(--t3);font-size:12px;margin:2px 0 14px}
.k{display:flex;gap:10px}.k div{flex:1;border:1px solid var(--line);border-radius:8px;padding:8px 10px}
.k i{display:block;font-style:normal;color:var(--t2);font-size:12px}.k b{display:block;font:600 17px ui-monospace,Menlo,monospace}.k em{font-style:normal;color:var(--ok);font:12px ui-monospace,Menlo,monospace}
h2{font-size:12px;font-weight:600;margin:16px 0 6px;color:var(--t2)}svg{width:100%;height:110px}
table{width:100%;border-collapse:collapse;font-size:12px}td,th{padding:6px 0;border-bottom:1px solid var(--line);text-align:right}td:first-child,th:first-child{text-align:left}th{color:var(--t2);font-weight:500}
td{font-family:ui-monospace,Menlo,monospace}td:first-child{font-family:inherit}
.q{margin-top:14px;border:1px solid var(--line);border-radius:10px;padding:10px 12px}.q p{margin:0 0 8px;color:var(--t2)}
.o{display:grid;grid-template-columns:1fr 1fr;gap:6px}.o span{border:1px solid var(--line);border-radius:7px;padding:5px 9px}.o .on{border-color:var(--t1);font-weight:600}
</style></head><body><div class="w">
<h1>10 月数据月报</h1><div class="s">夹夹 · 10 月 1 日–31 日 · Ledger 整理</div>
<div class="k"><div><i>新增群</i><b>38</b><em>+12%</em></div><div><i>日活</i><b>2,340</b><em>+4%</em></div><div><i>平均回复</i><b>1.2 秒</b><em>-0.3 秒 · 更快</em></div></div>
<h2>四周走势 — 日活 — 新增群</h2>
<svg viewBox="0 0 400 110" preserveAspectRatio="none"><path d="M0 80 L60 70 L120 74 L180 55 L240 48 L300 40 L360 30 L400 26" fill="none" stroke="currentColor" stroke-width="2"/><path d="M0 96 L60 94 L120 90 L180 92 L240 86 L300 84 L360 80 L400 78" fill="none" stroke="currentColor" stroke-opacity=".45" stroke-width="1.5"/></svg>
<h2>渠道拆分 · v3 新加</h2>
<table><tr><th>渠道</th><th>新增群</th><th>新增用户</th><th>花费</th><th>每个新增</th></tr>
<tr><td>小红书</td><td>14</td><td>612</td><td>¥3,200</td><td>¥5.2</td></tr><tr><td>B 站</td><td>9</td><td>488</td><td>¥1,800</td><td>¥3.7</td></tr>
<tr><td>抖音</td><td>6</td><td>301</td><td>¥2,400</td><td>¥8.0</td></tr><tr><td>群内邀请</td><td>9</td><td>396</td><td>¥0</td><td>–</td></tr></table>
<div class="q"><p>下个月加预算投哪个渠道？</p><div class="o"><span>A 小红书</span><span class="on">B B 站</span><span>C 抖音</span><span>D 群内邀请</span></div></div>
</div></body></html>"##;
