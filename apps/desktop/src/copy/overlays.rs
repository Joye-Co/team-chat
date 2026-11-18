//! v12/overlays: sheets, alerts, the Spotlight panel, the login window, the
//! update sheet and quiet status lines (docs/design/NATIVE.md §7, §9.7).

crate::copy! {
    // --- v12/overlays: sheets ---
    NEW_TOPIC_TITLE = "新话题",
    NEW_TOPIC_EXPLANATION = "写下要做的事，标题由 AI 起；@ 谁就交给谁。",
    NEW_TOPIC_GROUP = "群",
    NEW_TOPIC_BODY = "第一条消息",
    NEW_TOPIC_BODY_PLACEHOLDER = "说清楚要什么，或 @ 成员交代…",
    // Onboarding: who in the 群 can take the 话题; a click puts @名字 in the
    // first message.
    NEW_TOPIC_HAND_TO = "交给",
    NEW_TOPIC_CREATE = "开始",
    NEW_GROUP_EXPLANATION = "名称和成员之后都可以改。",
    GROUP_NAME = "名称",
    MEMBERS = "成员",
    ADD_MEMBERS_EXPLANATION = "加进来的成员能看到这个群里的全部话题。",
    START_DM_EXPLANATION = "选一位成员，单独聊。",
    START_DM_OPEN = "开始私聊",
    RENAME_GROUP_EXPLANATION = "群里的成员都会看到新名称。",
    RENAME_TOPIC_EXPLANATION = "话题里的成员都会看到新标题。",
    TOPIC_TITLE = "标题",
    SEARCH_MEMBERS = "搜索成员",
    // --- v12/overlays: alerts ---
    STOP_TITLE = "停止 {name} 的工作？",
    STOP_MESSAGE = "{name} 会停在现在这一步。\n已经做好的部分会留下。",
    STOP_OK = "停止",
    // --- v12/overlays: update sheet ---
    UPDATE_INSTALLING_TITLE = "正在安装 {version}",
    UPDATE_MANUAL_TITLE = "需要手动安装 {version}",
    UPDATE_FAILED_TITLE = "没能更新到 {version}",
    UPDATE_HIDE = "隐藏",
    // --- v12/overlays: login ---
    LOGIN_CONTINUE = "继续",
    // --- v12/fx-overlays: blocking errors and footnotes ---
    ALERT_OK = "好",
    NEW_GROUP_REFUSED = "没能新建群",
    ADD_MEMBERS_REFUSED = "没能添加成员",
    RENAME_GROUP_REFUSED = "没能重命名这个群",
    STOP_REFUSED = "没能停止",
    TAKE_OVER_REFUSED = "没能接管",
    HAND_BACK_REFUSED = "没能交还",
    PAUSE_REFUSED = "没能暂停",
    CONTINUE_REFUSED = "没能继续",
    COMMAND_REFUSED = "没能运行这条命令",
    DM_REFUSED = "没能开始私聊",
    // --- v12/fx-overlays: update ---
    MENU_CHECK = "检查更新…",
    MENU_UPDATE_TO = "更新到 {version}",
    MENU_DOWNLOADING = "正在下载 {version}（{percent}%）",
    MENU_RESTART = "安装 {version} 并重新打开",
    UPDATE_RESTART = "安装更新",
    UPDATE_READY = "新版本 {version} 已下载好",
}
