//! Screenshot scenes of the 首页 (the desk track owns this slice).

use crate::{
    fake::{v2 as d, v2_desk},
    selftest::scenes::{Scene, Start, open_main},
};

pub const SCENES: &[Scene] = &[
    Scene {
        name: "desk",
        about: "首页 (Main.dc.html): Jojo's priority, 需要你拍板 4, 做完了 today, 正在做 and 排队",
        start: Start::V2,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_main,
    },
    Scene {
        name: "desk-answered",
        about: "首页 after I answered: PR #61 approved and the invite option picked, as records with a drawn check",
        start: Start::V2,
        setup: |s, cx| {
            let p = &s.provider;
            p.answer_card_v2(
                d::DEV,
                d::T_RECONNECT,
                &v2_desk::review_ask(),
                crate::data::cards_v2::CardV2AnswerRequest {
                    rev: 1,
                    action_id: "approve".into(),
                    ..Default::default()
                },
            );
            p.press_card_action(d::DEV, d::T_INVITE, &v2_desk::choice_ask(), "a");
            s.nav.update(cx, |nav, cx| nav.open_desk(cx));
        },
        open: open_main,
    },
    // --- v13/ux-pages ---
    Scene {
        name: "desk-loading",
        about: "首页 before the first sync: KPI and list skeletons, the right pane skeletons",
        start: Start::V2Unsynced,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_main,
    },
    Scene {
        name: "desk-failed",
        about: "首页 with nothing cached while the server cannot be reached: the Alert with 重试",
        start: Start::V2Unreachable,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_main,
    },
    // --- end v13/ux-pages ---
    Scene {
        name: "desk-empty",
        about: "首页 on a quiet day: nothing waits on me, nothing runs, nothing done yet",
        start: Start::V2Quiet,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_main,
    },
    Scene {
        name: "desk-first-run",
        about: "首页 of a member who just joined: 从这里开始 with 和 Jojo 聊聊, 开个话题 and 去全员看看; the sidebar keeps 全员 with its 话题 folded into one row",
        start: Start::V2Newcomer,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_main,
    },
    Scene {
        name: "new-topic-hand-to",
        about: "The 新话题 sheet of a member who just joined: the 交给 row lists who can take it",
        start: Start::V2Newcomer,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: crate::views::nav::selftest::scenes::new_topic,
    },
];
