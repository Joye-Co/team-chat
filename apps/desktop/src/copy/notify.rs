//! System notifications.

crate::copy! {
    MENTION_TITLE = "{name} 在「{group}」提到了你",
    WAITING_TITLE = "「{group}」有话题在等你",
    RESULT_TITLE = "「{group}」你关注的话题有了结果",
    RESULT_BODY = "{topic}：{text}",
    MESSAGE_TITLE = "{name}（{group}）",
    // usable/takeover: a held run is not forgotten (2 hours to the holder, 24 to the rest).
    HOLD_REMIND = "{member} 还停着，等你交还",
    HOLD_REMIND_LONG = "{who} 接管 {member} 已经 {h} 小时",
}
