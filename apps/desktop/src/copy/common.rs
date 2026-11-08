//! Words used in many places.

crate::copy! {
    SEARCH_PLACEHOLDER = "搜索",
    BOT_MARK = "机",
    ONLINE = "在线",
    OFFLINE = "离线",
    JUST_NOW = "刚刚",
    MINUTES_AGO = "{n} 分钟前",
    YESTERDAY = "昨天",
    TOMORROW = "明天",
    MONTH_DAY = "{m}月{d}日",
    TIME_UNKNOWN = "未知",
    CONNECTING = "正在连接…",
    RECONNECTING = "连接已断开，正在重连…",
    CONNECTION_OFFLINE = "离线",
    // §2.12, A5: one word per state, everywhere. A 话题 in progress reads
    // 在做 (with `n/m` when a member works on a plan), never 进行中.
    STATUS_IN_PROGRESS = "在做",
    // Nobody works on it now: the clock, muted.
    STATUS_QUEUED = "排队",
    STATUS_WAITING_ANSWER_ME = "等你回答",
    STATUS_WAITING_ANSWER_NAMED = "等{name}回答",
    STATUS_WAITING_ANSWER = "等回答",
    STATUS_WAITING_REVIEW_ME = "等你审",
    STATUS_WAITING_REVIEW_NAMED = "等{name}审",
    STATUS_WAITING_REVIEW = "等审",
    STATUS_WAITING_ANSWER_ME_SHORT = "等你答",
    COUNT_CAP = "99+",
    STATUS_BLOCKED = "卡住了",
    STATUS_DONE = "完成",
    // A run result (D17); 完成 is a done 话题 (review 5 X-03).
    STATUS_DELIVERED = "已交付",
    CANCEL = "取消",
    SAVE = "保存",
    CLOSE = "关闭",
    PANE_TOPIC = "详情",
    MEMBER_DEACTIVATED = "已停用",
    MORE_ITEMS = "等 {n} 人",
    // --- v13/ux-pages: page states (COMPLETENESS.md §2.14, §2.15) ---
    PAGE_LOAD_FAILED = "连不上服务器",
    PAGE_LOAD_FAILED_LINE = "这台电脑上还没有这一页的内容，连上后会显示。",
    PAGE_RETRY = "重试",
    // --- end v13/ux-pages ---
}
