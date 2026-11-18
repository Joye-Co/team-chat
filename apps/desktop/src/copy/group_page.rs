//! 群 page (Group.dc.html). The group page track owns this file.

crate::copy! {
    GROUP_PAGE_ACTIVE = "在做",
    GROUP_PAGE_RECENT_DONE = "最近完成",
    GROUP_PAGE_DECISIONS = "群里的决定",
    GROUP_PAGE_DECISIONS_NOTE = "成员做事前会先读这里。",
    GROUP_PAGE_RECENT_OUTPUTS = "最近的产出",
    GROUP_PAGE_MEMBERS = "{n} 位成员",
    GROUP_PAGE_COMPOSER = "在{group}里开个话题，或 @ 成员交代…",
    GROUP_PAGE_NEEDS_REVIEW = "需要你审",
    GROUP_PAGE_ASKED_BY = "{name} 提出",
    GROUP_PAGE_INFO = "群资料",
    GROUP_PAGE_COLLAPSE = "收起右栏",
    GROUP_PAGE_RECORD = "记一条",
    GROUP_PAGE_RECORD_PLACEHOLDER = "比如：先发 staging，再发生产",
    GROUP_PAGE_SAVE = "记下",
    GROUP_PAGE_JUST_NOW = "刚刚",
    GROUP_PAGE_IN_USE = "在线",
    GROUP_PAGE_MACHINE_USERS = "{names} 在用",
    GROUP_PAGE_SEE_COMPUTER = "看 {name} 的电脑",
    GROUP_PAGE_LIVE = "实时",
    GROUP_PAGE_OUTPUTS = "产出",
    GROUP_PAGE_SETTINGS = "群设置",
    GROUP_PAGE_ARTIFACT_META = "v{v} · {kind} · {name}",
    GROUP_PAGE_KIND_PAGE = "网页",
    GROUP_PAGE_KIND_DECISION = "决定",
    GROUP_PAGE_DECISION_FROM = "{question} → {choice}",
    // dfix-topic: the hint beside the send button of the 群 page composer.
    GROUP_PAGE_COMPOSER_HINT = "{name} 会先看一眼，再交给合适的成员",
    // --- pages track (v12 native) ---
    GROUP_PAGE_MEMBERS_TITLE = "成员",
    GROUP_PAGE_WHERE = "在哪干活",
    GROUP_PAGE_RECORD_MORE = "记一条…",
    GROUP_PAGE_RECORD_TITLE = "记一条群里的决定",
    GROUP_PAGE_OPEN_TOPIC = "打开话题",
    // --- end pages track ---
    // --- v13/ux-pages: the 群 recipe (COMPLETENESS.md §2.18) ---
    GROUP_PAGE_NEW_TOPIC = "新话题",
    // 1.1.1: back from the 群 page to the 群聊.
    GROUP_PAGE_BACK_TO_CHAT = "回到群里聊",
    GROUP_PAGE_SHOW_ALL = "看全部 {n} 个话题",
    GROUP_PAGE_SHOW_LESS = "收起",
    GROUP_PAGE_EMPTY_TITLE = "这个群还没有话题",
    GROUP_PAGE_EMPTY_BODY = "开个话题交给群里的成员。在做和做完的话题都会列在这里。",
    GROUP_PAGE_MEMBERS_EMPTY = "群里还没有成员",
    GROUP_PAGE_OUTPUTS_EMPTY = "还没有产出",
    GROUP_PAGE_WHERE_EMPTY = "现在没有电脑在干活",
    // --- end v13/ux-pages ---
    // --- v13/wire ---
    GROUP_PAGE_PURPOSE_MORE = "修改群说明…",
    GROUP_PAGE_PURPOSE_TITLE = "群说明",
    GROUP_PAGE_PURPOSE_EXPLANATION = "显示在群名后面，说这个群做什么。群里的人都能看到。",
    GROUP_PAGE_PURPOSE_LABEL = "说明",
    GROUP_PAGE_PURPOSE_PLACEHOLDER = "比如：orderbot 的上线、巡检和告警",
    GROUP_PAGE_PURPOSE_TOO_LONG = "最多 200 个字",
    GROUP_PAGE_PURPOSE_REFUSED = "没能保存群说明",
    GROUP_PAGE_RECORD_REFUSED = "没能记下这条决定",
    GROUP_PAGE_RECORD_TOO_LONG = "最多 1000 个字",
    // --- end v13/wire ---
    // --- v14/t3-pages ---
    GROUP_PAGE_STATUS_DONE = "完成",
    GROUP_PAGE_DONE_EMPTY = "这周还没有做完的话题",
    GROUP_PAGE_ACTIVE_EMPTY = "现在没有在做的话题",
    GROUP_PAGE_DECISIONS_EMPTY = "还没有记下的决定",
    // --- end v14/t3-pages ---
    // --- v15/r3-pages ---
    GROUP_PAGE_OFFLINE = "连上后显示",
    // --- end v15/r3-pages ---
    // --- v16/p-pages ---
    GROUP_PAGE_SECTION_YOU = "等你",
    GROUP_PAGE_SECTION_STUCK = "卡住了",
    GROUP_PAGE_SECTION_OTHERS = "等别人",
    GROUP_PAGE_SECTION_QUEUED = "排队",
    GROUP_PAGE_UNKNOWN_MACHINE = "未知电脑",
    // --- end v16/p-pages ---
    // The word of the toolbar item that opens the 群's chat (D20).
    GROUP_PAGE_CHAT = "群聊",
    // The field of the 记一条 sheet.
    GROUP_PAGE_RECORD_LABEL = "决定",
}
