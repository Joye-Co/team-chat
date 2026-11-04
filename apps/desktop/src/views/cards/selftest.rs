//! `desktop --selftest-cards --report <path> [--shots <dir>]`: draws every
//! sample card (`samples.rs`) with the fake provider through [`CardExtra`],
//! in light and then dark mode, presses buttons through the provider, checks
//! what was drawn, writes a JSON report and exits (0 when every check passed).
//!
//! No pixel assertions. With `--shots <dir>` it also saves one PNG of the
//! window per mode (`cards-light.png`, `cards-dark.png`) to look at.
//! The window opens without a Dock icon and without taking focus.

use std::{path::PathBuf, sync::Arc, time::Duration};

use gpui::{
    ActivationPolicy, AnyWindowHandle, App, AppContext as _, Bounds, Context, ElementId, Entity,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Window, WindowBounds, WindowOptions, div, px,
    size,
};
use gpui_component::{Theme, ThemeMode};

use super::{CardExtra, RenderStats, STATS, samples};
use crate::{
    app::{Changes, Services},
    data::{ChatProvider, Member, Message, SessionState},
    selftest::capture,
    theme,
    views::{
        nav::selftest::Finish,
        timeline::extras::{ExtrasContext, MessageExtra},
    },
};

/// Time for the window to draw after a change.
const SETTLE: Duration = Duration::from_millis(900);
const TIMEOUT: Duration = Duration::from_secs(60);

/// Every sample card in one scrolling column, drawn by the timeline extra.
struct Gallery {
    provider: Arc<dyn ChatProvider>,
    members: Vec<Member>,
    messages: Vec<Message>,
    me: Option<String>,
    scroll: ScrollHandle,
}

impl Gallery {
    fn new(services: &Services, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
            if changes.touches_topic(samples::GROUP, samples::TOPIC) {
                this.refresh();
                cx.notify();
            }
        })
        .detach();
        let mut this = Self {
            provider: services.provider.clone(),
            members: Vec::new(),
            messages: Vec::new(),
            me: None,
            scroll: ScrollHandle::new(),
        };
        this.refresh();
        this
    }

    fn refresh(&mut self) {
        self.members = self.provider.members();
        self.messages = self
            .provider
            .messages(samples::GROUP, samples::TOPIC, 100)
            .messages;
        self.me = match self.provider.session() {
            SessionState::SignedIn { me, .. } => Some(me.id),
            _ => None,
        };
    }
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let ctx = ExtrasContext {
            provider: &self.provider,
            members: &self.members,
            window_messages: &self.messages,
            me: self.me.as_deref(),
        };
        let mut rows = Vec::new();
        for message in &self.messages {
            let name = ctx
                .member(&message.author_id)
                .map(|m| m.name.clone())
                .unwrap_or_default();
            let hidden = CardExtra.hides_body(message, &ctx);
            let card = CardExtra.render(message, &ctx, window, cx);
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(crate::ui::native::tokens::space::S2)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.foreground)
                            .child(name),
                    )
                    .children((!hidden).then(|| div().text_sm().child(message.body.clone())))
                    .children(card),
            );
        }
        div()
            .id(ElementId::Name("cards-gallery".into()))
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .bg(colors.background)
            .text_color(colors.foreground)
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(crate::ui::native::tokens::space::S16)
            .children(rows)
    }
}

fn scroll(gallery: &Entity<Gallery>, bottom: bool, cx: &mut gpui::AsyncApp) {
    gallery.update(cx, |g, cx| {
        if bottom {
            g.scroll.scroll_to_bottom();
        } else {
            g.scroll.set_offset(gpui::point(px(0.), px(0.)));
        }
        cx.notify();
    });
}

/// Waits up to 10 s for the drawn cards to satisfy `done`. Frames stop while
/// the window is covered or on another Space, so after half a second without
/// progress it draws a frame itself (`Window::draw`; nothing is presented).
async fn wait_for(
    handle: &AnyWindowHandle,
    cx: &mut gpui::AsyncApp,
    done: impl Fn(&RenderStats) -> bool,
) {
    let start = std::time::Instant::now();
    while !done(&stats()) && start.elapsed() < Duration::from_secs(10) {
        let force = start.elapsed() > Duration::from_millis(500);
        let _ = handle.update(cx, |_, window, cx| {
            window.refresh();
            if force {
                window.draw(cx).clear(cx);
            }
        });
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
}

fn stats() -> RenderStats {
    STATS.with(|s| s.borrow().clone().unwrap_or_default())
}

/// One check of the report.
fn check(checks: &mut Vec<serde_json::Value>, name: &str, ok: bool, detail: String) {
    checks.push(serde_json::json!({ "name": name, "ok": ok, "detail": detail }));
}

pub fn run(
    services: Services,
    report: PathBuf,
    shots: Option<PathBuf>,
    finish: Finish,
    cx: &mut App,
) {
    cx.set_activation_policy(ActivationPolicy::Accessory);
    STATS.with(|s| *s.borrow_mut() = Some(RenderStats::default()));
    Theme::change(ThemeMode::Light, None, cx);

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(720.), px(900.)),
            cx,
        ))),
        focus: false,
        show: true,
        ..Default::default()
    };
    let opened = {
        let services = services.clone();
        gpui_kit::open_window(options, cx, move |_, cx| {
            cx.new(|cx| Gallery::new(&services, cx))
        })
    };
    let (handle, gallery): (AnyWindowHandle, Entity<Gallery>) = match opened {
        Ok(pair) => pair,
        Err(error) => {
            let json = serde_json::json!({ "ok": false, "error": error.to_string() });
            finish(&report, json, false, cx);
            return;
        }
    };
    // A covered window gets no frames; this does not activate the app.
    let _ = handle.update(cx, |_, window, _| capture::order_front(window));

    let timeout_report = report.clone();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(TIMEOUT).await;
        cx.update(|cx| {
            let json = serde_json::json!({ "ok": false, "error": "timed out" });
            finish(&timeout_report, json, false, cx);
        });
    })
    .detach();

    let provider = services.provider.clone();
    cx.spawn(async move |cx| {
        let mut checks = Vec::new();
        let mut shots_taken = Vec::new();
        let ids: Vec<String> = provider
            .messages(samples::GROUP, samples::TOPIC, 100)
            .messages
            .iter()
            .map(|m| m.id.clone())
            .collect();
        let shoot = |name: &str, cx: &mut gpui::AsyncApp, taken: &mut Vec<String>| {
            let Some(dir) = &shots else { return };
            let _ = std::fs::create_dir_all(dir);
            let png = dir.join(format!("cards-{name}.png"));
            let result = handle
                .update(cx, |_, window, _| capture::window_number(window))
                .map_err(|e| e.to_string())
                .and_then(|n| n)
                .and_then(|n| capture::capture(n, &png));
            match result {
                Ok(()) => taken.push(png.display().to_string()),
                Err(e) => taken.push(format!("{name}: {e}")),
            }
        };

        // 1. Light mode: every card and every block type is drawn.
        let all_drawn = |s: &RenderStats| ids.iter().all(|id| s.cards.contains_key(id));
        wait_for(&handle, cx, |s| all_drawn(s)).await;
        let light = stats();
        let expected_types = [
            "text", "fields", "table", "list", "progress", "code", "image", "divider", "link",
        ];
        let missing: Vec<_> = expected_types
            .iter()
            .filter(|t| !light.block_types.contains(*t))
            .collect();
        check(
            &mut checks,
            "light: every card drawn",
            ids.iter().all(|id| light.cards.contains_key(id))
                && ids.len() == samples::samples().len(),
            format!(
                "{} of {} cards, {} renders",
                light.cards.len(),
                ids.len(),
                light.renders
            ),
        );
        check(
            &mut checks,
            "light: every block type drawn",
            missing.is_empty(),
            format!("missing {missing:?}"),
        );
        let drawn = |stats: &RenderStats, ix: usize| {
            ids.get(ix)
                .and_then(|id| stats.cards.get(id))
                .cloned()
                .unwrap_or_default()
        };
        check(
            &mut checks,
            "buttons and press states",
            drawn(&light, 0).buttons == 3
                && drawn(&light, 1).failed
                && drawn(&light, 2).sending
                && drawn(&light, 3).press_lines == super::actions::SHOWN_PRESSES,
            format!(
                "buttons {}, failed {}, sending {}, press lines {}",
                drawn(&light, 0).buttons,
                drawn(&light, 1).failed,
                drawn(&light, 2).sending,
                drawn(&light, 3).press_lines
            ),
        );
        shoot("light", cx, &mut shots_taken);
        if shots.is_some() {
            scroll(&gallery, true, cx);
            cx.background_executor().timer(SETTLE).await;
            shoot("light-bottom", cx, &mut shots_taken);
            scroll(&gallery, false, cx);
        }

        // 2. Dark mode.
        let _ = handle.update(cx, |_, window, cx| {
            Theme::change(ThemeMode::Dark, Some(window), cx);
            window.refresh();
        });
        let before = light.renders;
        wait_for(&handle, cx, |s| s.renders > before && all_drawn(s)).await;
        cx.background_executor().timer(SETTLE).await;
        let dark = stats();
        let dark_mode = cx.update(|cx| theme::is_dark(cx));
        check(
            &mut checks,
            "dark: cards drawn again in dark mode",
            dark_mode && dark.renders > light.renders && dark.cards.len() == ids.len(),
            format!(
                "dark {dark_mode}, renders {} -> {}",
                light.renders, dark.renders
            ),
        );
        shoot("dark", cx, &mut shots_taken);
        if shots.is_some() {
            scroll(&gallery, true, cx);
            cx.background_executor().timer(SETTLE).await;
            shoot("dark-bottom", cx, &mut shots_taken);
            scroll(&gallery, false, cx);
        }

        // 3. A press: the fake bot edits the card, which is drawn in place.
        if let Some(id) = ids.first() {
            provider.press_card_action(samples::GROUP, samples::TOPIC, id, "approve");
        }
        // 4. A press on a button the server no longer has.
        if let Some(id) = ids.get(4) {
            provider.press_card_action(samples::GROUP, samples::TOPIC, id, "gone-details");
        }
        // Wait for the redraw (frames can be late while the window is covered).
        let (first, fifth) = (ids.first().cloned(), ids.get(4).cloned());
        let redrawn = |s: &RenderStats| {
            let get = |id: &Option<String>| id.as_ref().and_then(|id| s.cards.get(id));
            get(&first).is_some_and(|d| d.buttons == 0) && get(&fifth).is_some_and(|d| d.failed)
        };
        wait_for(&handle, cx, redrawn).await;
        let after = stats();
        let edited = drawn(&after, 0);
        check(
            &mut checks,
            "press: card edited in place",
            edited.status.as_deref() == Some("已处理：批准")
                && edited.buttons == 0
                && edited.press_lines == 1,
            format!("{edited:?}"),
        );
        check(
            &mut checks,
            "press: refusal shown",
            drawn(&after, 4).failed,
            format!("{:?}", drawn(&after, 4)),
        );
        check(
            &mut checks,
            "unknown block drawn as nothing",
            after.block_types.contains("unknown"),
            String::new(),
        );

        let ok = checks.iter().all(|c| c["ok"] == true);
        let mut block_types: Vec<_> = after.block_types.iter().copied().collect();
        block_types.sort();
        let json = serde_json::json!({
            "ok": ok,
            "cards": ids.len(),
            "renders": after.renders,
            "block_types": block_types,
            "checks": checks,
            "screenshots": shots_taken,
        });
        cx.update(|cx| finish(&report, json, ok, cx));
    })
    .detach();
}
