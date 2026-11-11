//! Message search inside the switcher: the Cmd+F scope token (DESIGN.md
//! §7.12).

crate::copy! {
    SCOPE_IN_GROUP = "在 {name}",
    SCOPE_REMOVE = "在全部群里搜索",
    PLACEHOLDER_IN_GROUP = "搜索这个群的话题和消息",
    // The search bar of a 话题 opened at a hit.
    BAR_POSITION = "{n} / {total}",
    BAR_OLDER = "上一个结果",
    BAR_NEWER = "下一个结果",
    BAR_CLOSE = "关闭搜索，回到最新",
    // --- v14/t4 palette ---
    // The palette footer (COMPLETENESS 2.11): one Kbd cap per key.
    KEY_UP = "↑",
    KEY_DOWN = "↓",
    KEY_RETURN = crate::copy::per_system("↩", "Enter"),
    KEY_COMMAND = crate::copy::per_system("⌘", "Ctrl"),
    KEY_ESCAPE = crate::copy::per_system("esc", "Esc"),
    HINT_SELECT = "选择",
    HINT_OPEN = "打开",
    HINT_CLOSE = "关闭",
    HINT_GIVE_HELPER = "交给 {name} 安排",
    // A row with no date (2.15 dates).
    TIME_UNKNOWN = "未知",
    // Palette states (2.11): no match, failed message search.
    NO_MATCH = "没有找到“{query}”",
    NO_MATCH_CLEAR = "清空搜索",
    SEARCH_FAILED = "没能搜索消息",
    SEARCH_OFFLINE = "连上以后才能搜索消息",
    SEARCH_FAILED_DESC = "服务器没有回应，请稍后重试",
    SEARCH_OFFLINE_DESC = "检查网络，连上以后重试",
    SEARCH_RETRY = "重试",
    // Status words of a palette row (2.12).
    STATUS_NEEDS_YOU = "等你",
    STATUS_WORKING = "在做",
    STATUS_WORKING_STEP = "在做 {step}",
    STATUS_DONE = "完成",
    STATUS_IDLE = "空闲",
    // 交代 when the query names someone who is not a member.
    NOBODY_NAMED = "没有叫 {name} 的成员",
    // --- end v14/t4 palette ---
}
