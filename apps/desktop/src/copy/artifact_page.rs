//! 产出 page in the main window (Artifact.dc.html). The artifact page track owns this file.

crate::copy! {
    ARTIFACTS_TITLE = "产出",
    ARTIFACT_STORY = "来龙去脉",
    ARTIFACT_BACK_TOPIC = "回到话题",
    ARTIFACT_BACK_GROUP = "回到群",
    ARTIFACT_BACK_DESK = "回到首页",
    ARTIFACT_BACK_MEMBERS = "回到成员",
    ARTIFACT_BACK_LIST = "回到产出",
    ARTIFACT_FROM_TOPIC = "来自话题",
    ARTIFACT_MADE_BY = "做的人",
    ARTIFACT_ASKED_BY = "提出的人",
    ARTIFACT_DECISIONS = "用到的决定",
    ARTIFACT_VERSIONS = "版本",
    ARTIFACT_CITED_BY = "被引用",
    ARTIFACT_RESPONSES = "页面里的回应",
    ARTIFACT_OPEN_WINDOW = "在新窗口打开",
    ARTIFACT_OPEN = "打开",
    ARTIFACT_SHARE = "分享",
    ARTIFACT_COPIED = "已复制",
    ARTIFACT_SIDE_CHAT = "单独问",
    ARTIFACT_BACK_TO = "回到 v{v}",
    // The tail after the 群 name; the name cuts first (§2.21).
    ARTIFACT_TOPIC_DONE = " · 完成 {time}",
    ARTIFACT_TOPIC_ACTIVE = " · {state}",
    ARTIFACT_DECISION_META = "{name} · {time} · {group}",
    ARTIFACT_SHARE_TEXT = "「{title}」v{v}\n{link}",
    ARTIFACT_SHARE_TEXT_NO_LINK = "「{title}」v{v}",
    ARTIFACTS_EMPTY = "还没有产出",
    ARTIFACT_PAGE_SNAPSHOT = "这个产出在单独的窗口里打开",
    ARTIFACT_RESPONSE = "{name} {action}",
    ARTIFACT_RESPONSE_VALUE = "{name} {action}「{value}」",
    // --- track cards: the Finder-like 产出 list (NATIVE.md §9.6) ---
    ARTIFACTS_COUNT = "{n} 个",
    ARTIFACTS_VIEW_GRID = "图标",
    ARTIFACTS_VIEW_LIST = "列表",
    ARTIFACTS_SORT_TIME = "按时间",
    ARTIFACTS_SORT_NAME = "按名称",
    ARTIFACTS_SORT_KIND = "按类型",
    ARTIFACTS_COL_NAME = "名称",
    ARTIFACTS_COL_KIND = "类型",
    ARTIFACTS_COL_TOPIC = "话题",
    // --- end track cards ---
    // --- v13/ux-pages: the 产出 recipe (COMPLETENESS.md §2.18) ---
    ARTIFACTS_COL_BY = "做的人",
    ARTIFACTS_COL_VERSION = "版本",
    ARTIFACTS_COL_UPDATED = "更新",
    ARTIFACTS_FOOTER = "{n} 个产出",
    ARTIFACTS_FOOTER_MATCH = "找到 {n} 个，共 {total} 个产出",
    ARTIFACTS_VERSION = "v{v}",
    ARTIFACTS_TILE_META = "v{v} · {topic}",
    ARTIFACTS_EMPTY_BODY = "成员做的网页、文件和图片都在这里，每个都有版本。",
    ARTIFACTS_NO_MATCH_TITLE = "没有找到「{q}」",
    ARTIFACTS_CLEAR_SEARCH = "清空搜索",
    ARTIFACTS_MORE = "更多操作",
    ARTIFACTS_DAY_TODAY = "今天",
    ARTIFACTS_DAY_YESTERDAY = "昨天",
    ARTIFACTS_DAY = "{m}月{d}日",
    ARTIFACT_NO_NOTE = "无说明",
    // --- end v13/ux-pages ---
    // --- v13/wire: 页面里的回应 from the server's page log ---
    ARTIFACT_RESPONSES_EMPTY = "还没有人在页面里回应",
    ARTIFACT_RESPONSES_FAILED = "没能读到页面里的回应",
    ARTIFACT_RESPONSES_RETRY = "重试",
    // --- end v13/wire ---
    // --- v14/t3-pages: the 产出 inspector (COMPLETENESS.md §2.5) ---
    ARTIFACT_INFO = "产出信息",
    ARTIFACT_INFO_LINE = "来龙去脉、版本和这一页能做的事",
    ARTIFACT_CLOSE_INFO = "关闭产出信息",
    ARTIFACT_NO_TITLE = "（无标题）",
    ARTIFACT_NOBODY = "没有记录",
    ARTIFACT_PAGE_CAN = "这一页",
    ARTIFACT_DECISIONS_EMPTY = "没有用到群里的决定",
    ARTIFACT_CITED_EMPTY = "还没有话题引用它",
    ARTIFACT_RESPONSES_NONE = "这一页不收回应",
    ARTIFACT_OLD_VERSION_TITLE = "你在看 v{v}，不是最新版",
    ARTIFACT_IN_PANE = "在右侧打开",
    // --- end v14/t3-pages ---
    // --- v15/r3-pages: the quiet-page check (artifact-quiet-page) ---
    ARTIFACT_QUIET_TITLE = "渠道预算",
    ARTIFACT_QUIET_CHECKING = "正在检查",
    ARTIFACT_QUIET_PASSED = "没有替你发出回应",
    ARTIFACT_QUIET_FAILED = "替你发出了回应",
    // --- end v15/r3-pages ---
    // The toolbar items' words (D20); the long forms stay as their tooltips.
    ARTIFACT_OPEN_WINDOW_SHORT = "新窗口",
    ARTIFACT_INFO_SHORT = "信息",
    // The list's filter: its toolbar item, the 群 heading of its menu (类型 and 做的人
    // are the column names), and the ways out.
    ARTIFACTS_KIND_DOC = "文档",
    ARTIFACTS_FILTER = "筛选",
    ARTIFACTS_FILTER_GROUP = "群",
    ARTIFACTS_FILTER_CLEAR = "清除筛选",
    ARTIFACTS_FILTER_NO_MATCH = "没有符合筛选的产出",
    // A search that found nothing while a filter narrows the list.
    ARTIFACTS_NO_MATCH_FILTERED = "在当前筛选里没有找到「{q}」",
    // The 产出 pane's button that opens this 产出 on the 产出 page.
    ARTIFACT_OPEN_IN_PAGE = "在产出里打开",
}
