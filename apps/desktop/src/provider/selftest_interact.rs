//! `--selftest-live <url> --token-file <path> --report <path> --interact <dir>`: the live
//! check of the 1.1 interaction loop (INTERACT §7.1) and 引用提问 (REFS §A3) against a real
//! server, with the chat-core provider and the real window. `scripts/live-interact-check.ts`
//! prepares the server side: a test human (this token), the SDK member Ledger, the card
//! 渠道明细 (a plain table) and the 7.1 report 10 月数据月报 in one 话题.
//!
//! The app opens that 话题, sends `choice.change` (有问题) on the report's `check` block
//! with the request the choice control sends, waits until Ledger's patch arrives as rev 2
//! with my event as its cause, and captures the window (`interact-rev2.png`: 按你的选择).
//! Then it adds the cell 渠道明细 · B 站 · 新增群 to the 话题 composer through the shared
//! `refs::ask_about_outcome` (what 问这里 does), types the question into the focused
//! composer, presses Enter, and checks the stored message: its `blockRefs` and the quote
//! line prefix the server wrote in front of the body (`interact-ref.png`).
//!
//! What it does not do: it does not click the choice option with the pointer (GPUI has no
//! public hit test for a real window); the event goes through the same provider call.

use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{AnyWindowHandle, AsyncApp, Keystroke, Modifiers};
use serde_json::{Value, json};

use super::CoreProvider;
use crate::{
    app::{self, Services},
    data::{
        AddRef, ChatProvider, ConnectionState, SessionState,
        cards_v2::{CardEventRequest, CardV2Block, CardV2Message},
    },
    selftest::capture,
    views::refs::{
        self, RefIntent, RefSource,
        parts::{TableData, TablePick},
    },
};

const TIMEOUT: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(20);
pub const REPORT_TITLE: &str = "10 月数据月报";
pub const TABLE_TITLE: &str = "渠道明细";
/// The question typed after the chip (the composer may lead it with `@Ledger `).
pub const QUESTION: &str = "为什么比上月少了 1 个？";
/// The window size of the captures (points).
const WIDTH: f64 = 1280.;
const HEIGHT: f64 = 1000.;

type Step = (&'static str, Value);

async fn wait_for<T>(
    cx: &mut AsyncApp,
    deadline: Instant,
    mut check: impl FnMut(&mut AsyncApp) -> Option<T>,
) -> Option<T> {
    loop {
        if let Some(v) = check(cx) {
            return Some(v);
        }
        if Instant::now() > deadline {
            return None;
        }
        cx.background_executor().timer(POLL).await;
    }
}

/// Resolves after the window draws its next frame (or fails after 2 s).
async fn next_frame(handle: AnyWindowHandle, cx: &mut AsyncApp) -> bool {
    let (tx, rx) = futures::channel::oneshot::channel();
    let asked = cx
        .update(|cx| {
            handle.update(cx, |_, window, _| {
                window.on_next_frame(move |_, _| {
                    let _ = tx.send(());
                });
                window.refresh();
            })
        })
        .is_ok();
    if !asked {
        return false;
    }
    let timer = cx.background_executor().timer(Duration::from_secs(2));
    futures::pin_mut!(rx, timer);
    matches!(
        futures::future::select(rx, timer).await,
        futures::future::Either::Left((Ok(()), _))
    )
}

/// A few frames and a pause, so lists measure themselves and fades end.
async fn settle(handle: AnyWindowHandle, pause: Duration, cx: &mut AsyncApp) {
    for _ in 0..3 {
        if !next_frame(handle, cx).await {
            let _ = cx.update(|cx| handle.update(cx, |_, w, _| capture::order_front(w)));
        }
    }
    cx.background_executor().timer(pause).await;
    let _ = next_frame(handle, cx).await;
}

/// Captures only this window into `png`.
async fn shoot(handle: AnyWindowHandle, png: &Path, cx: &mut AsyncApp) -> Result<(), String> {
    let number = cx
        .update(|cx| handle.update(cx, |_, window, _| capture::window_number(window)))
        .map_err(|e| e.to_string())??;
    let path = png.to_path_buf();
    cx.background_executor()
        .spawn(async move { capture::capture(number, &path) })
        .await
}

/// The newest card titled `title` that the signed-in member can see.
fn find_card(provider: &CoreProvider, title: &str) -> Option<CardV2Message> {
    for g in provider.groups() {
        for t in provider.topics(&g.id) {
            let found = provider
                .messages(&g.id, &t.id, 200)
                .messages
                .iter()
                .rev()
                .filter(|m| !m.id.is_empty())
                .find_map(|m| {
                    provider
                        .card_v2(&g.id, &m.id)
                        .filter(|c| c.card.title == title)
                });
            if found.is_some() {
                return found;
            }
        }
    }
    None
}

fn keystroke(text: &str) -> Keystroke {
    Keystroke {
        modifiers: Modifiers::default(),
        key: text.to_string(),
        key_char: Some(text.to_string()),
    }
}

pub async fn drive(
    provider: &Arc<CoreProvider>,
    services: &Services,
    root: &gpui::Entity<app::RootView>,
    shots: &Path,
    cx: &mut AsyncApp,
) -> Result<Value, Step> {
    let deadline = Instant::now() + TIMEOUT;
    let mut json = json!({ "ok": false, "interact": true });
    std::fs::create_dir_all(shots).map_err(|_| ("shots folder", json.clone()))?;

    // 1. Signed in and online; the server takes references; both cards are in the cache.
    let me = wait_for(cx, deadline, |_| {
        let SessionState::SignedIn { me, .. } = provider.session() else {
            return None;
        };
        (provider.connection() == ConnectionState::Online).then_some(me.id)
    })
    .await
    .ok_or(("sign in and connect", json.clone()))?;
    json["me"] = me.clone().into();
    let (report, table) = wait_for(cx, deadline, |_| {
        Some((
            find_card(provider, REPORT_TITLE)?,
            find_card(provider, TABLE_TITLE)?,
        ))
    })
    .await
    .ok_or(("both cards in the cache", json.clone()))?;
    json["refs_supported"] = provider.refs_supported().into();
    json["report_rev_before"] = report.card.rev.into();
    if report.card.rev != 1 || !provider.refs_supported() {
        return Err(("a rev 1 report on a server with refs/1", json));
    }
    let (g, t, mid) = (
        report.group_id.clone(),
        report.topic_id.clone(),
        report.message_id.clone(),
    );
    json["message_id"] = mid.clone().into();

    // 2. Open the 话题 in a window of a fixed size, drawn as the key window.
    let (gg, tt) = (g.clone(), t.clone());
    cx.update(|cx| {
        services
            .nav
            .update(cx, |nav, cx| nav.open_topic(gg, tt, cx))
    });
    let handle = cx
        .update(|cx| cx.windows().into_iter().next())
        .ok_or(("the main window", json.clone()))?;
    capture::keep_frames();
    cx.update(|cx| {
        handle.update(cx, |_, window, cx| {
            capture::undo_clamp(window, WIDTH, HEIGHT, false)?;
            window.bounds_changed(cx);
            window.refresh();
            capture::order_front(window)?;
            capture::draw_as_key(window)
        })
    })
    .map_err(|_| ("window setup", json.clone()))?
    .map_err(|_| ("window setup", json.clone()))?;
    wait_for(cx, deadline, |cx| {
        let n = cx.update(|cx| root.read(cx).timeline().read(cx).message_count());
        (n >= 2).then_some(())
    })
    .await
    .ok_or(("the 话题 shows both cards", json.clone()))?;
    settle(handle, Duration::from_millis(600), cx).await;
    let rev1 = shots.join("interact-rev1.png");
    shoot(handle, &rev1, cx)
        .await
        .map_err(|_| ("capture rev 1", json.clone()))?;

    // 3. 有问题: the choice control's live event.
    let t_event = Instant::now();
    let sent = provider.send_card_event(
        &g,
        &t,
        &mid,
        CardEventRequest {
            rev: 1,
            block_id: Some("check".into()),
            action_id: None,
            event: "choice.change".into(),
            value: Some(json!({ "value": "wrong" })),
            inputs: None,
            part: None,
        },
    );
    json["client_event_id"] = sent.clone().into();
    if sent.is_none() {
        return Err(("send choice.change", json));
    }

    // 4. Ledger's patch: rev 2 with my event as the cause and the drill-down group.
    let rev2 = wait_for(cx, deadline, |_| {
        provider.card_v2(&g, &mid).filter(|c| c.card.rev >= 2)
    })
    .await
    .ok_or(("rev 2 from the member", json.clone()))?;
    json["rev2_ms"] = (t_event.elapsed().as_secs_f64() * 1000.).round().into();
    json["report_rev_after"] = rev2.card.rev.into();
    json["cause_actor_is_me"] = rev2.cause.as_ref().is_some_and(|c| c.actor_id == me).into();
    json["cause_event"] = rev2.cause.as_ref().map(|c| c.event.clone()).into();
    json["drill_block"] = rev2.card.block("drill").is_some().into();
    json["by_channel_block"] = rev2.card.block("by_channel").is_some().into();
    json["note_removed"] = rev2.card.block("note").is_none().into();
    settle(handle, Duration::from_millis(900), cx).await;
    let png2 = shots.join("interact-rev2.png");
    shoot(handle, &png2, cx)
        .await
        .map_err(|_| ("capture rev 2", json.clone()))?;
    json["shot_rev2"] = png2.display().to_string().into();

    // 5. 引用提问 on a table cell: 渠道明细 · B 站 · 新增群.
    let card = table.card.clone();
    let Some(
        b @ CardV2Block::Table {
            id,
            columns,
            rows,
            row_keys,
            ..
        },
    ) = card.blocks.iter().find(|b| b.id() == Some("channels"))
    else {
        return Err(("the plain table block", json));
    };
    let data = TableData {
        columns,
        rows,
        row_keys: row_keys.as_deref().filter(|k| k.len() == rows.len()),
    };
    let row = rows
        .iter()
        .position(|r| r.first().map(String::as_str) == Some("B 站"));
    let part = row.and_then(|row| refs::parts::table(&data, TablePick::Cell { row, col: 1 }));
    let Some(part) = part else {
        return Err(("the B 站 cell", json));
    };
    let target = part.target(
        RefSource::Message {
            group_id: table.group_id.clone(),
            topic_id: table.topic_id.clone(),
            message_id: table.message_id.clone(),
        },
        &card.title,
        id.clone(),
        crate::views::cards::v21::parts::block_segment(&card, b).as_deref(),
        Some(card.rev as u64),
    );
    json["ref_label"] = target.label.clone().into();
    let asked = cx
        .update(|cx| {
            handle.update(cx, |_, window, cx| {
                refs::ask_about_outcome(target, RefIntent::Topic, window, cx)
            })
        })
        .map_err(|_| ("ask about the cell", json.clone()))?;
    json["chip_added"] = (asked.added == AddRef::Added).into();
    json["chip_in_side_chat"] = asked.side_chat.into();
    if asked.added != AddRef::Added || asked.side_chat {
        return Err(("the chip in the 话题 composer", json));
    }
    settle(handle, Duration::from_millis(200), cx).await;

    // 6. Type the question into the focused composer and press Enter.
    for ch in QUESTION.chars() {
        let s = ch.to_string();
        let _ = cx.update(|cx| {
            handle.update(cx, |_, window, cx| {
                window.dispatch_keystroke(keystroke(&s), cx);
            })
        });
    }
    let _ = next_frame(handle, cx).await;
    let t_send = Instant::now();
    let enter = Keystroke::parse("enter").map_err(|_| ("enter", json.clone()))?;
    let _ = cx.update(|cx| {
        handle.update(cx, |_, window, cx| {
            window.dispatch_keystroke(enter, cx);
        })
    });

    // 7. My message is stored with its reference and the server's quote line.
    let sent = wait_for(cx, deadline, |_| {
        provider
            .messages(&g, &t, 200)
            .messages
            .into_iter()
            .rev()
            .find(|m| m.author_id == me && !m.id.is_empty() && m.body.ends_with(QUESTION))
            .filter(|m| !provider.message_block_refs(m).is_empty())
    })
    .await
    .ok_or(("my question with its reference", json.clone()))?;
    json["question_ms"] = (t_send.elapsed().as_secs_f64() * 1000.).round().into();
    let block_refs = provider.message_block_refs(&sent);
    let prefix = refs::prefix(&block_refs);
    json["question_id"] = sent.id.clone().into();
    json["question_body"] = sent.body.clone().into();
    json["block_refs"] = serde_json::to_value(&block_refs).unwrap_or(Value::Null);
    json["body_has_prefix"] = refs::strip_prefix(&sent.body, &block_refs)
        .is_some_and(|rest| rest.ends_with(QUESTION))
        .into();
    json["prefix"] = prefix.into();
    let cell_ok = block_refs.len() == 1
        && block_refs[0].source.block_id == "channels"
        && block_refs[0]
            .source
            .part
            .as_ref()
            .is_some_and(|p| p.kind == "cell");
    json["ref_is_the_cell"] = cell_ok.into();
    settle(handle, Duration::from_millis(600), cx).await;
    let png3 = shots.join("interact-ref.png");
    shoot(handle, &png3, cx)
        .await
        .map_err(|_| ("capture the question", json.clone()))?;
    json["shot_ref"] = png3.display().to_string().into();

    let ok = json["cause_actor_is_me"] == true
        && json["drill_block"] == true
        && json["by_channel_block"] == true
        && json["note_removed"] == true
        && json["body_has_prefix"] == true
        && cell_ok;
    json["ok"] = ok.into();
    if ok {
        Ok(json)
    } else {
        Err(("the checks", json))
    }
}
