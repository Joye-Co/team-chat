//! settings-backend track: the 成员卡住或出错 notification and the daily digest (每日摘要).

crate::copy! {
    BLOCKED_TITLE = "「{group}」有话题卡住了",
    DIGEST_TITLE = "每日摘要",
    DIGEST_WAITING = "{n} 个话题等你拍板",
    DIGEST_RESULT = "{n} 个话题有了结果",
    DIGEST_BLOCKED = "{n} 个话题卡住了",
    DIGEST_MENTION = "{n} 次有人 @你",
    DIGEST_JOIN = "，",
    DIGEST_EMPTY = "没有新的动静",
}
