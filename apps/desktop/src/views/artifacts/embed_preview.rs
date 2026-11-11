//! Screenshot self-test scenes for [`WebEmbed`] in the real main window (E2, B3): an
//! HTML 产出 live inline in the first 话题 and another one (with local video and audio)
//! in the 产出 pane, the same 话题 scrolled so the inline page goes under the toolbar,
//! and with the message menu over the inline page.
//!
//! The scenes check themselves after the capture ([`verify`]): the inline page is shown,
//! clipped under the toolbar (scrolled) or hidden with its snapshot in its place
//! (covered); the pane's video plays and its audio loads from the 产出's own files.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, App, AsyncApp, Entity, Pixels, point, px};

use crate::{
    app::Services,
    data::ChatProvider,
    selftest::scenes::{open_first_topic, open_main},
    ui::web_embed::{Phase, WebEmbedState, live},
    views::media::{
        preview::{Extra, MediaThread, PageArt},
        store::{self, MediaKey},
    },
};

const INLINE_ID: &str = "selftest-weekly";
const PANE_ID: &str = "selftest-media";
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
    done: bool,
    error: Option<String>,
}

thread_local! {
    static PROBE: RefCell<Probe> = RefCell::new(Probe::default());
}

fn finish(r: Result<(), String>) {
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        p.done = true;
        p.error = r.err();
    });
}

/// The live page of `artifact_id`, loaded.
fn embed(artifact_id: &str, cx: &App) -> Option<Entity<WebEmbedState>> {
    live(cx).into_iter().find(|e| {
        let s = e.read(cx);
        s.artifact_id() == artifact_id && *s.phase() == Phase::Ready
    })
}

/// Writes the sample 产出 pages to a temp folder: the weekly report and the media page.
fn write_pages() -> Option<(PathBuf, PathBuf)> {
    let root = std::env::temp_dir().join("team-chat-embed-scene");
    let _ = std::fs::remove_dir_all(&root);
    let (weekly, media) = (root.join("weekly"), root.join("media"));
    std::fs::create_dir_all(&weekly).ok()?;
    std::fs::create_dir_all(&media).ok()?;
    std::fs::write(weekly.join("index.html"), INLINE_HTML).ok()?;
    std::fs::write(media.join("index.html"), PANE_HTML).ok()?;
    std::fs::write(media.join("clip.mp4"), CLIP_MP4).ok()?;
    std::fs::write(media.join("tone.wav"), tone_wav()).ok()?;
    Some((weekly, media))
}

/// Opens the main window on the first 话题 with the sample messages, the weekly report
/// inline and the media page in the 产出 pane, then moves the scene to its end state.
pub fn open(
    services: Services,
    content: gpui::Size<Pixels>,
    mode: Mode,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    PROBE.with(|p| *p.borrow_mut() = Probe::default());
    let (weekly, media) = write_pages().ok_or("cannot write the sample pages")?;
    store::forget(&MediaKey::new(INLINE_ID, 1), cx);
    // Messages after the page: a few in the plain scene, all of them when the 话题
    // scrolls the page under the toolbar.
    let after = match mode {
        Mode::Scrolled => &SAMPLE[2..],
        _ => &SAMPLE[2..4],
    };
    let authors = ["m-alex", "m-coder"];
    let mut extras = vec![
        Extra {
            author: authors[0].into(),
            body: SAMPLE[0].into(),
            streaming: false,
            artifact: None,
        },
        Extra {
            author: authors[1].into(),
            body: SAMPLE[1].into(),
            streaming: false,
            artifact: Some(MediaKey::new(INLINE_ID, 1)),
        },
    ];
    extras.extend(after.iter().enumerate().map(|(i, t)| Extra {
        author: authors[i % 2].into(),
        body: (*t).into(),
        streaming: false,
        artifact: None,
    }));
    let thread = MediaThread::with_pages(
        services.provider.clone(),
        extras,
        vec![
            PageArt {
                id: INLINE_ID,
                title: SAMPLE_TITLE.into(),
                author: authors[1],
                dir: weekly,
            },
            PageArt {
                id: PANE_ID,
                title: MEDIA_TITLE.into(),
                author: authors[1],
                dir: media,
            },
        ],
    );
    let provider: Arc<dyn ChatProvider> = thread;
    let services = Services::new(provider, cx);
    open_first_topic(&services, cx);
    services.nav.update(cx, |nav, cx| {
        nav.toggle_pane(
            crate::app::RightPane::Artifact {
                id: PANE_ID.into(),
                version: None,
            },
            cx,
        )
    });
    let handle = open_main(services, content, cx)?;
    cx.spawn(async move |cx: &mut AsyncApp| finish(run(handle, mode, cx).await))
        .detach();
    Ok(handle)
}

/// How long a page may take to show. A CI runner without a graphics card starts two web
/// views and a video in about 12 s; 15 s was too close on Windows.
const PAGE_WAIT: Duration = Duration::from_secs(45);

async fn wait_for<T>(
    cx: &mut AsyncApp,
    timeout: Duration,
    mut f: impl FnMut(&mut App) -> Option<T>,
) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = cx.update(|cx| f(cx)) {
            return Some(v);
        }
        if Instant::now() > deadline {
            return None;
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
}

async fn eval(cx: &mut AsyncApp, page: &Entity<WebEmbedState>, script: &str) -> String {
    let (tx, rx) = futures::channel::oneshot::channel();
    let script = script.to_string();
    cx.update(|cx| {
        page.read(cx).evaluate_text(&script, move |v| {
            let _ = tx.send(v);
        })
    });
    rx.await.ok().flatten().unwrap_or_default()
}

/// The pane page's media: the video plays, the audio is read (its length in seconds).
const MEDIA: &str = "(function(){var v=document.getElementById('v'),a=document.getElementById('a');\
return JSON.stringify({playing:v.currentTime>0&&v.videoWidth>0,audio:a.duration||0});})()";

async fn run(handle: AnyWindowHandle, mode: Mode, cx: &mut AsyncApp) -> Result<(), String> {
    // The scrolled scene opens at the end of a long 话题: whether a sliver of the page is
    // in view then depends on the window's content height (32 less under the Windows
    // caption strip, where it starts out of view and is not shown). Wheel the thread
    // back until the page is there.
    let mut inline = None;
    let started = Instant::now();
    while inline.is_none() && started.elapsed() < PAGE_WAIT {
        inline = wait_for(cx, Duration::from_secs(2), |cx| {
            embed(INLINE_ID, cx).filter(|e| e.read(cx).shown() && e.read(cx).bounds().is_some())
        })
        .await;
        if inline.is_none() && mode == Mode::Scrolled {
            let _ = cx.update(|cx| {
                handle.update(cx, |_, window, cx| {
                    window.dispatch_event(
                        gpui::PlatformInput::Scroll(gpui::ScrollEvent {
                            position: point(px(700.), px(400.)),
                            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(120.))),
                            ..Default::default()
                        }),
                        cx,
                    );
                })
            });
        }
    }
    let inline = inline.ok_or("the inline page did not load")?;
    let pane = wait_for(cx, PAGE_WAIT, |cx| {
        embed(PANE_ID, cx).filter(|e| e.read(cx).shown())
    })
    .await
    .ok_or("the 产出 pane page did not load")?;
    // Local media from the 产出's own files: a real click on the video starts it (a
    // page in the pane plays media only after the viewer's input).
    let at: serde_json::Value = serde_json::from_str(
        &eval(
            cx,
            &pane,
            "(function(){var r=document.getElementById('v').getBoundingClientRect();\
return JSON.stringify({x:r.left+r.width/2,y:r.top+r.height/2});})()",
        )
        .await,
    )
    .unwrap_or_default();
    let (x, y) = (
        at["x"].as_f64().unwrap_or(-1.),
        at["y"].as_f64().unwrap_or(-1.),
    );
    if !cx.update(|cx| pane.read(cx).post_click(x, y, true)) {
        return Err("the click on the video could not be posted".into());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let m: serde_json::Value =
            serde_json::from_str(&eval(cx, &pane, MEDIA).await).unwrap_or_default();
        let audio = m["audio"].as_f64().unwrap_or(0.);
        if m["playing"].as_bool() == Some(true) && (0.9..1.1).contains(&audio) {
            break;
        }
        if Instant::now() > deadline {
            return Err(format!("local media did not play: {m}"));
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
    let clip_top = crate::ui::native::window_chrome::sheet_top();
    match mode {
        Mode::Topic => {}
        Mode::Scrolled => {
            // Wheel the real thread until the page's top sits CUT above the clip edge.
            for _ in 0..12 {
                let top = cx
                    .update(|cx| inline.read(cx).bounds())
                    .map(|b| b.origin.y)
                    .ok_or("the inline page left the layout")?;
                let want = clip_top - CUT;
                let delta = want - top;
                if delta.abs() < px(3.) {
                    break;
                }
                let at = cx
                    .update(|cx| inline.read(cx).bounds())
                    .map(|b| point(b.origin.x - px(24.), b.origin.y + b.size.height / 2.))
                    .unwrap_or(point(px(700.), px(400.)));
                let _ = cx.update(|cx| {
                    handle.update(cx, |_, window, cx| {
                        window.dispatch_event(
                            gpui::PlatformInput::Scroll(gpui::ScrollEvent {
                                position: at,
                                delta: gpui::ScrollDelta::Pixels(point(px(0.), delta)),
                                ..Default::default()
                            }),
                            cx,
                        );
                    })
                });
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
            }
            let b = cx
                .update(|cx| inline.read(cx).bounds())
                .ok_or("the inline page left the layout")?;
            if b.origin.y > clip_top - px(40.) {
                return Err(format!(
                    "the inline page did not scroll under the clip edge (top {:?})",
                    b.origin.y
                ));
            }
        }
        Mode::Covered => {
            // A secondary click on the message over the page opens its menu there;
            // the page hides under it and its snapshot shows.
            wait_for(cx, Duration::from_secs(5), |cx| {
                inline.read(cx).has_snapshot().then_some(())
            })
            .await
            .ok_or("the inline page took no snapshot")?;
            let b = cx
                .update(|cx| inline.read(cx).bounds())
                .ok_or("the inline page left the layout")?;
            let at = point(b.origin.x + px(48.), b.origin.y + px(24.));
            let _ = cx.update(|cx| {
                handle.update(cx, |_, window, cx| {
                    for e in [
                        gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                            button: gpui::MouseButton::Right,
                            position: at,
                            modifiers: gpui::Modifiers::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                            button: gpui::MouseButton::Right,
                            position: at,
                            modifiers: gpui::Modifiers::default(),
                            click_count: 1,
                        }),
                    ] {
                        window.dispatch_event(e, cx);
                    }
                })
            });
            wait_for(cx, Duration::from_secs(5), |cx| {
                let s = inline.read(cx);
                (s.covered() && !s.shown()).then_some(())
            })
            .await
            .ok_or("no menu covered the inline page")?;
        }
    }
    // A page that loaded but drew nothing must not pass: both pages have painted (a
    // page hidden under the menu of the covered scene draws no frames, its snapshot
    // shows instead).
    if mode != Mode::Covered {
        // Looked up again: scrolling can give the page a new view.
        let inline = cx.update(|cx| embed(INLINE_ID, cx)).unwrap_or(inline);
        let pane = cx.update(|cx| embed(PANE_ID, cx)).unwrap_or(pane);
        painted(cx, &inline, "the inline page").await?;
        painted(cx, &pane, "the 产出 pane page").await?;
    }
    // Let the last frame draw.
    cx.background_executor()
        .timer(Duration::from_millis(250))
        .await;
    Ok(())
}

/// Waits until `page` has drawn: the browser runs animation-frame callbacks only when it
/// draws the page, so two of them in a row mean a picture of the page exists.
async fn painted(
    cx: &mut AsyncApp,
    page: &Entity<WebEmbedState>,
    what: &str,
) -> Result<(), String> {
    const MARK: &str = "window.__tcDrew || (window.__tcDrew = 0, \
        requestAnimationFrame(function () { requestAnimationFrame(function () { \
        window.__tcDrew = 1; }); })); String(window.__tcDrew)";
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last = String::new();
    while Instant::now() < deadline {
        let drew = eval(cx, page, MARK).await;
        if drew == "1" {
            return Ok(());
        }
        last = drew;
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
    Err(format!(
        "{what} did not paint (its script answered {last:?})"
    ))
}

/// True once the scene reached its end state.
pub fn ready(_mode: Mode, _cx: &App) -> bool {
    PROBE.with(|p| p.borrow().done)
}

/// The result of the scene's check (see the module docs).
pub fn verify(_mode: Mode, _png: &Path, _cx: &mut App) -> Result<(), String> {
    PROBE.with(|p| match &p.borrow().error {
        Some(e) => Err(e.clone()),
        None => Ok(()),
    })
}

// --- the sample 产出 -----------------------------------------------------------

const SAMPLE_TITLE: &str = "本周合并的改动";
const MEDIA_TITLE: &str = "登录改动演示";
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
:root{color-scheme:light dark;--t1:var(--tc-fg,#0a0a0a);--t2:var(--tc-muted,#737373);--line:var(--tc-border,#e5e5e5);--bar:var(--tc-primary,#171717);--soft:color-mix(in srgb,var(--bar) 20%,transparent)}
html,body{margin:0;background:transparent;color:var(--t1);font:13px/1.5 -apple-system,"PingFang SC","Segoe UI Variable Text","Segoe UI","Microsoft YaHei UI",sans-serif;-webkit-font-smoothing:antialiased}
.wrap{padding:0}
.sub{color:var(--t2);font-size:12px;margin:0 0 12px}
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
<div class="sub">周一到周五 · 浅色是还在审的</div>
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
:root{color-scheme:light dark;--t1:var(--tc-fg,#0a0a0a);--t2:var(--tc-muted,#737373);--line:var(--tc-border,#e5e5e5);--bar:var(--tc-primary,#171717)}
html,body{margin:0;background:transparent;color:var(--t1);font:13px/1.6 -apple-system,"PingFang SC","Segoe UI Variable Text","Segoe UI","Microsoft YaHei UI",sans-serif;-webkit-font-smoothing:antialiased}
.wrap{padding:0}
.sub{color:var(--t2);font-size:12px;margin:0 0 12px}
video{display:block;width:100%;border-radius:8px;background:#000;aspect-ratio:16/9}
audio{display:block;width:100%;margin-top:12px}
h2{font-size:13px;margin:16px 0 4px;font-weight:600}
ul{margin:0;padding-left:18px;color:var(--t1)}
code{font:12px ui-monospace,Menlo,"Cascadia Mono",Consolas,monospace;background:var(--line);padding:1px 4px;border-radius:4px}
"#,
    r#"</style></head><body><div class="wrap">
<div class="sub">视频和提示音都在这个产出里</div>
<video id="v" src="clip.mp4" muted autoplay loop playsinline></video>
<audio id="a" src="tone.wav" controls preload="auto"></audio>
<h2>改了什么</h2>
<ul><li>邀请码输错时保留输入，光标停在末尾</li><li>服务器地址折叠成一行 <code>host · 更改</code></li><li>重连时侧栏底部显示提示</li></ul>
</div>
<script>
(function(){
  var v=document.getElementById('v'), a=document.getElementById('a'), sent=false;
  v.addEventListener('click',function(){v.play();});
  function report(){
    if(sent) return;
    if(!(v.currentTime>0 && a.readyState>=1)) return;
    sent=true;
    if(window.artifact&&window.artifact.emitEvent) window.artifact.emitEvent('media',{
      video:{readyState:v.readyState,currentTime:v.currentTime,duration:v.duration,videoWidth:v.videoWidth,paused:v.paused},
      audio:{readyState:a.readyState,duration:a.duration}
    });
  }
  v.addEventListener('timeupdate',report); a.addEventListener('loadedmetadata',report);
  setInterval(report,200);
})();
</script></body></html>"#
);
