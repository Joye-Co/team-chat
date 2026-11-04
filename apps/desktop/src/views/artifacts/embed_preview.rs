//! Screenshot self-test scenes for [`WebEmbed`]: an HTML 产出 live inline in a 话题 and in
//! the 产出 pane (with local video and audio), the same 话题 scrolled so the inline page is
//! cut off at the top, and with a menu over the inline page.
//!
//! The scenes check themselves after the capture ([`verify`]): the page's own pixels (its
//! orange chart bars, a colour the app never draws) must be in the PNG where the page is,
//! never above the clip edge, and never under the menu; the video must play and the audio
//! must load from the 产出's own files.

use std::cell::RefCell;
use std::path::Path;

use artifact_view::{ArtifactEvent, Content, VersionSpec};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled, Window, WindowBounds, div, point, px,
};

use crate::{
    app::{Services, main_window_options},
    copy::{artifacts as ca, fill, nav as cn, panel as cp},
    data::Member,
    fake,
    selftest::scenes::capture_bounds,
    theme::{self, Type as _, space},
    ui::{
        self, MemberAvatar,
        overlay::{menu_row, menu_surface},
        parts::{hairline, toolbar_band, vhairline},
        web_embed::{WebEmbed, WebEmbedSpec, WebEmbedState, live},
    },
};

const INLINE_ID: &str = "selftest-weekly";
const PANE_ID: &str = "selftest-media";
/// The page's bar colour (#E8590C). The app's theme has no such orange.
const BAR: (u8, u8, u8) = (0xE8, 0x59, 0x0C);
const PANE_WIDTH: Pixels = px(440.);
/// How far the scrolled scene moves the inline page above the clip edge.
const CUT: Pixels = px(96.);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Topic,
    Scrolled,
    Covered,
}

#[derive(Default)]
struct Probe {
    media: Option<serde_json::Value>,
    menu: Option<Bounds<Pixels>>,
    scrolled: bool,
}

thread_local! {
    static PROBE: RefCell<Probe> = RefCell::new(Probe::default());
}

pub struct EmbedPreview {
    mode: Mode,
    members: Vec<Member>,
    scroll: ScrollHandle,
    cover: bool,
    _poll: gpui::Task<()>,
}

impl EmbedPreview {
    fn new(services: &Services, mode: Mode, cx: &mut Context<Self>) -> Self {
        PROBE.with(|p| *p.borrow_mut() = Probe::default());
        let members = ["m-alex", "m-coder", fake::ME, "m-lixue"]
            .iter()
            .filter_map(|id| services.provider.member(id))
            .collect();
        // Moves the scene into its end state once the pages are ready.
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let done = this.update(cx, |p, cx| p.step(cx)).unwrap_or(true);
                if done {
                    break;
                }
            }
        });
        Self {
            mode,
            members,
            scroll: ScrollHandle::new(),
            cover: false,
            _poll: poll,
        }
    }

    /// One step towards the scene's end state. True when there is nothing left to do.
    fn step(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(inline) = embed(INLINE_ID, cx) else {
            return false;
        };
        let s = inline.read(cx);
        if *s.phase() != ui::web_embed::Phase::Ready {
            return false;
        }
        match self.mode {
            Mode::Topic => true,
            Mode::Scrolled => {
                let Some(b) = s.bounds() else { return false };
                let top = theme::TOOLBAR_HEIGHT + theme::HAIRLINE;
                let delta = b.origin.y - (top - CUT);
                let offset = self.scroll.offset();
                self.scroll.set_offset(point(offset.x, offset.y - delta));
                PROBE.with(|p| p.borrow_mut().scrolled = true);
                cx.notify();
                true
            }
            Mode::Covered => {
                if !s.has_snapshot() {
                    return false;
                }
                self.cover = true;
                cx.notify();
                true
            }
        }
    }

    fn message(
        &self,
        index: usize,
        text: &'static str,
        extra: Option<gpui::AnyElement>,
        cx: &App,
    ) -> gpui::Div {
        let k = theme::colors(cx);
        let member = self.members.get(index % self.members.len().max(1));
        div()
            .flex()
            .flex_row()
            .gap(space::S12)
            .child(MemberAvatar::new(member, theme::AVATAR_SIZE))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(
                        div()
                            .t_ui()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(k.text_1)
                            .child(SharedString::from(
                                member.map(|m| m.name.clone()).unwrap_or_default(),
                            )),
                    )
                    .child(div().t_body().text_color(k.text_1).child(text))
                    .children(extra.map(|e| div().pt(space::S8).w_full().child(e))),
            )
    }
}

impl Render for EmbedPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let version = fill(ca::CARD_VERSION, &[("n", "3")]);
        let inline = WebEmbed::new(INLINE_ID, inline_spec())
            .height(px(300.))
            .pass_scroll(true)
            .on_event(record);
        let pane = WebEmbed::new(PANE_ID, pane_spec())
            .height(px(640.))
            .autoplay(true)
            .on_event(record);

        let mut topic = div()
            .w_full()
            .max_w(theme::MEASURE)
            .mx_auto()
            .px(theme::CONTENT_GUTTER)
            .py(space::S24)
            .flex()
            .flex_col()
            .gap(space::S20)
            .child(self.message(0, SAMPLE[0], None, cx))
            .child(self.message(1, SAMPLE[1], Some(inline.into_any_element()), cx));
        for (i, text) in SAMPLE[2..].iter().enumerate() {
            topic = topic.child(self.message(i + 2, text, None, cx));
        }
        // The inline page is a child of the message row's column.
        let topic_column = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .child(
                toolbar_band("embed-topic-bar")
                    .pl(theme::TRAFFIC_LIGHTS_INSET)
                    .pr(space::S16)
                    .child(div().t_title().text_color(k.text_1).child(SAMPLE_TITLE)),
            )
            .child(hairline(cx))
            .child(
                div()
                    .id("embed-topic-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(topic),
            );
        let pane_column = div()
            .w(PANE_WIDTH)
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(k.sidebar)
            .child(
                toolbar_band("embed-pane-bar")
                    .px(space::S16)
                    .gap(space::S8)
                    .child(div().t_title().text_color(k.text_1).child(cp::ARTIFACTS))
                    .child(div().t_ui().text_color(k.text_3).child(version)),
            )
            .child(hairline(cx))
            .child(div().p(space::S16).child(pane));

        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .bg(k.content)
            .text_color(k.text_1)
            .child(topic_column)
            .child(vhairline(cx))
            .child(pane_column);
        if self.cover
            && let Some(b) = embed(INLINE_ID, cx).and_then(|e| e.read(cx).bounds())
        {
            let at = point(b.origin.x + px(48.), b.origin.y + px(40.));
            let menu = Bounds {
                origin: at,
                // Three menu rows inside the menu padding (ui::overlay::menu_surface).
                size: gpui::size(
                    px(220.),
                    theme::size::MENU_ROW * 3. + crate::ui::native::metrics::MENU_PAD * 2.,
                ),
            };
            PROBE.with(|p| p.borrow_mut().menu = Some(menu));
            root = root.child(
                menu_surface(cx)
                    .id("embed-cover-menu")
                    .occlude()
                    .absolute()
                    .left(at.x)
                    .top(at.y)
                    .w(menu.size.width)
                    .child(menu_row(
                        "m-open",
                        Some(ui::Sym::CodeFile),
                        ca::OPEN,
                        true,
                        cx,
                    ))
                    .child(menu_row(
                        "m-copy",
                        Some(ui::Sym::Copy),
                        cn::MENU_COPY,
                        false,
                        cx,
                    ))
                    .child(menu_row(
                        "m-new",
                        Some(ui::Sym::Download),
                        ca::NEW_VERSION,
                        false,
                        cx,
                    )),
            );
        }
        root
    }
}

fn record(event: &ArtifactEvent, _: &mut Window, _: &mut App) {
    if let ArtifactEvent::Emit { name, payload, .. } = event
        && name == "media"
    {
        PROBE.with(|p| p.borrow_mut().media = Some(payload.clone()));
    }
}

fn embed(id: &str, cx: &App) -> Option<gpui::Entity<WebEmbedState>> {
    let spec_id = match id {
        INLINE_ID => "weekly-report",
        _ => "demo-media",
    };
    live(cx)
        .into_iter()
        .find(|e| e.read(cx).artifact_id() == spec_id)
}

// --- openers ----------------------------------------------------------------

pub fn open(
    services: Services,
    content: gpui::Size<Pixels>,
    mode: Mode,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, move |_, cx| {
        cx.new(|cx| EmbedPreview::new(&services, mode, cx))
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

/// True once the scene reached its end state and the pages drew.
pub fn ready(mode: Mode, cx: &App) -> bool {
    let (Some(inline), Some(pane)) = (embed(INLINE_ID, cx), embed(PANE_ID, cx)) else {
        return false;
    };
    let (i, p) = (inline.read(cx), pane.read(cx));
    let loaded = *i.phase() == ui::web_embed::Phase::Ready
        && *p.phase() == ui::web_embed::Phase::Ready
        && p.shown()
        && PROBE.with(|p| p.borrow().media.is_some());
    loaded
        && match mode {
            Mode::Topic => i.shown(),
            Mode::Scrolled => i.shown() && PROBE.with(|p| p.borrow().scrolled),
            Mode::Covered => i.covered() && !i.shown() && i.has_snapshot(),
        }
}

/// Checks the captured PNG and the page state. See the module docs.
pub fn verify(mode: Mode, png: &Path, cx: &mut App) -> Result<(), String> {
    let (inline, pane) = (embed(INLINE_ID, cx), embed(PANE_ID, cx));
    let ib = inline
        .and_then(|e| e.read(cx).bounds())
        .ok_or("no inline page")?;
    let pb = pane
        .and_then(|e| e.read(cx).bounds())
        .ok_or("no pane page")?;
    let image = image::open(png)
        .map_err(|e| format!("cannot read {}: {e}", png.display()))?
        .to_rgba8();
    let window_w = cx
        .windows()
        .first()
        .and_then(|w| {
            w.update(cx, |_, window, _| window.viewport_size().width)
                .ok()
        })
        .unwrap_or(px(1440.));
    let scale = image.width() as f32 / f32::from(window_w);
    let count = |b: Bounds<Pixels>| -> usize {
        let x0 = (f32::from(b.origin.x) * scale).max(0.) as u32;
        let y0 = (f32::from(b.origin.y) * scale).max(0.) as u32;
        let x1 = ((f32::from(b.origin.x + b.size.width) * scale) as u32).min(image.width());
        let y1 = ((f32::from(b.origin.y + b.size.height) * scale) as u32).min(image.height());
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let p = image.get_pixel(x, y).0;
                if p[0].abs_diff(BAR.0) < 24
                    && p[1].abs_diff(BAR.1) < 24
                    && p[2].abs_diff(BAR.2) < 24
                {
                    n += 1;
                }
            }
        }
        n
    };
    let media = PROBE
        .with(|p| p.borrow().media.clone())
        .ok_or("no media event")?;
    let playing = media["video"]["currentTime"].as_f64().unwrap_or(0.) > 0.
        && media["video"]["videoWidth"].as_u64().unwrap_or(0) > 0;
    let audio = media["audio"]["duration"].as_f64().unwrap_or(0.);
    if !playing || !(0.9..1.1).contains(&audio) {
        return Err(format!("local media did not play: {media}"));
    }
    if count(pb) < 200 {
        return Err("the pane page is not in the capture".into());
    }
    let clip_top = theme::TOOLBAR_HEIGHT + theme::HAIRLINE;
    match mode {
        Mode::Topic => {
            if count(ib) < 2000 {
                return Err("the inline page is not in the capture".into());
            }
        }
        Mode::Scrolled => {
            let above = Bounds {
                origin: ib.origin,
                size: gpui::size(ib.size.width, (clip_top - ib.origin.y).max(px(0.))),
            };
            let below = Bounds {
                origin: point(ib.origin.x, clip_top + px(1.)),
                size: gpui::size(ib.size.width, ib.size.height - above.size.height - px(1.)),
            };
            if above.size.height < px(40.) {
                return Err("the inline page did not scroll under the clip edge".into());
            }
            if count(above) > 0 {
                return Err("the inline page draws above its clip edge".into());
            }
            if count(below) < 500 {
                return Err("the visible part of the inline page is not in the capture".into());
            }
        }
        Mode::Covered => {
            let menu = PROBE.with(|p| p.borrow().menu).ok_or("no menu")?;
            // Inside the rounded corners and the shadow edge.
            let inner = Bounds {
                origin: point(menu.origin.x + px(8.), menu.origin.y + px(8.)),
                size: gpui::size(menu.size.width - px(16.), menu.size.height - px(16.)),
            };
            if count(inner) > 0 {
                return Err("the inline page draws over the menu".into());
            }
            // The snapshot draws in the page's place around the menu.
            if count(ib) < 1000 {
                return Err("the snapshot of the covered page is not in the capture".into());
            }
        }
    }
    Ok(())
}

// --- the sample 产出 -----------------------------------------------------------

const SAMPLE_TITLE: &str = "周报：本周合并的改动";
const SAMPLE: &[&str] = &[
    "这周的改动合并完了吗？做个周报给大家看看。",
    "做好了，周报在下面，可以直接看。",
    "柱子是每天合并的改动，浅色是还在审的。",
    "周四那根高一些，是登录那块拆成了三个改动。",
    "好的，下周一我再更新一版。",
    "演示视频和提示音放在右边的产出里了。",
    "收到。",
    "审核平均 1.4 天，比上周快了半天。",
    "在审的 6 个里有两个等设计确认。",
    "我今天下午看一下那两个。",
    "好，看完在这里说一声。",
    "下周的周报还用这个格式。",
    "可以，数字我会自动更新。",
];

fn inline_spec() -> WebEmbedSpec {
    WebEmbedSpec::new(
        "weekly-report",
        VersionSpec::new(
            3,
            Content::single("index.html", INLINE_HTML.as_bytes().to_vec()),
        ),
    )
}

fn pane_spec() -> WebEmbedSpec {
    let files = vec![
        ("index.html".to_string(), PANE_HTML.as_bytes().to_vec()),
        ("clip.mp4".to_string(), CLIP_MP4.to_vec()),
        ("tone.wav".to_string(), tone_wav()),
    ];
    WebEmbedSpec::new("demo-media", VersionSpec::new(1, Content::memory(files)))
}

/// A 2 s test pattern (H.264, 384 x 216).
const CLIP_MP4: &[u8] = include_bytes!("preview/clip.mp4");

/// A 1 s 440 Hz tone, 16-bit mono PCM at 8 kHz.
fn tone_wav() -> Vec<u8> {
    let rate = 8000u32;
    let samples: Vec<i16> = (0..rate)
        .map(|i| {
            let t = i as f32 / rate as f32;
            ((t * 440. * std::f32::consts::TAU).sin() * 8000.) as i16
        })
        .collect();
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

const INLINE_HTML: &str = concat!(
    r#"<!doctype html><html><head><meta charset="utf-8"><style>"#,
    r#"
:root{color-scheme:light dark;--t1:#1d1d1f;--t2:#6e6e73;--line:rgba(0,0,0,.09);--soft:#F4A261;--bar:#E8590C}
@media (prefers-color-scheme: dark){:root{--t1:#f2f2f4;--t2:#9a9aa1;--line:rgba(255,255,255,.12);--soft:#8a4a22}}
html,body{margin:0;background:transparent;color:var(--t1);font:13px/1.5 -apple-system,"PingFang SC",sans-serif;-webkit-font-smoothing:antialiased}
.wrap{padding:16px 18px}
h1{font-size:15px;line-height:22px;margin:0;font-weight:600}
.sub{color:var(--t2);font-size:12px;margin:2px 0 12px}
.chart{display:flex;align-items:flex-end;gap:14px;height:120px;border-bottom:1px solid var(--line);padding:0 4px}
.day{flex:1;display:flex;align-items:flex-end;gap:3px;height:100%}
.bar{flex:1;background:var(--bar);border-radius:4px 4px 0 0}
.bar.s{background:var(--soft)}
.lbl{display:flex;gap:14px;color:var(--t2);font-size:11px;margin-top:4px;padding:0 4px}.lbl span{flex:1;text-align:center}
.kpis{display:flex;gap:10px;margin-top:14px}
.kpi{flex:1;border:1px solid var(--line);border-radius:8px;padding:7px 10px}
.kpi b{display:block;font-size:17px;line-height:22px;font-weight:600;font-variant-numeric:tabular-nums}
.kpi i{font-style:normal;color:var(--t2);font-size:11px}
"#,
    r#"</style></head><body><div class="wrap">
<h1>本周合并的改动</h1><div class="sub">第 3 版 · 周一到周五 · 浅色是还在审的</div>
<div class="chart">
<div class="day"><div class="bar" style="height:42%"></div><div class="bar s" style="height:18%"></div></div>
<div class="day"><div class="bar" style="height:55%"></div><div class="bar s" style="height:25%"></div></div>
<div class="day"><div class="bar" style="height:38%"></div><div class="bar s" style="height:30%"></div></div>
<div class="day"><div class="bar" style="height:92%"></div><div class="bar s" style="height:20%"></div></div>
<div class="day"><div class="bar" style="height:64%"></div><div class="bar s" style="height:12%"></div></div>
</div>
<div class="lbl"><span>周一</span><span>周二</span><span>周三</span><span>周四</span><span>周五</span></div>
<div class="kpis"><div class="kpi"><b>23</b><i>合并</i></div><div class="kpi"><b>6</b><i>在审</i></div><div class="kpi"><b>1.4 天</b><i>平均审核</i></div></div>
</div></body></html>"#
);

const PANE_HTML: &str = concat!(
    r#"<!doctype html><html><head><meta charset="utf-8"><style>"#,
    r#"
:root{color-scheme:light dark;--t1:#1d1d1f;--t2:#6e6e73;--line:rgba(0,0,0,.09);--bar:#E8590C}
@media (prefers-color-scheme: dark){:root{--t1:#f2f2f4;--t2:#9a9aa1;--line:rgba(255,255,255,.12)}}
html,body{margin:0;background:transparent;color:var(--t1);font:13px/1.6 -apple-system,"PingFang SC",sans-serif;-webkit-font-smoothing:antialiased}
.wrap{padding:16px 18px}
h1{font-size:15px;line-height:22px;margin:0;font-weight:600;display:flex;align-items:center;gap:8px}
h1:before{content:"";width:4px;height:16px;border-radius:2px;background:var(--bar)}
.sub{color:var(--t2);font-size:12px;margin:2px 0 12px}
video{display:block;width:100%;border-radius:8px;background:#000;aspect-ratio:16/9}
audio{display:block;width:100%;margin-top:12px}
h2{font-size:13px;margin:16px 0 4px;font-weight:600}
ul{margin:0;padding-left:18px;color:var(--t1)}
code{font:12px ui-monospace,Menlo,monospace;background:var(--line);padding:1px 4px;border-radius:4px}
.bar{height:10px;border-radius:5px;background:var(--bar);margin-top:14px;width:62%}
"#,
    r#"</style></head><body><div class="wrap">
<h1>登录改动演示</h1><div class="sub">第 1 版 · 视频和提示音都在这个产出里</div>
<video id="v" src="clip.mp4" muted autoplay loop playsinline></video>
<audio id="a" src="tone.wav" controls preload="auto"></audio>
<h2>改了什么</h2>
<ul><li>邀请码输错时保留输入，光标停在末尾</li><li>服务器地址折叠成一行 <code>host · 更改</code></li><li>重连时侧栏底部显示提示</li></ul>
<div class="bar"></div>
</div>
<script>
(function(){
  var v=document.getElementById('v'), a=document.getElementById('a'), sent=false;
  function report(){
    if(sent) return;
    if(!(v.currentTime>0 && a.readyState>=1)) return;
    sent=true;
    window.artifact.emitEvent('media',{
      video:{readyState:v.readyState,currentTime:v.currentTime,duration:v.duration,videoWidth:v.videoWidth,paused:v.paused},
      audio:{readyState:a.readyState,duration:a.duration}
    });
  }
  v.addEventListener('timeupdate',report); a.addEventListener('loadedmetadata',report);
  setInterval(report,200);
})();
</script></body></html>"#
);
