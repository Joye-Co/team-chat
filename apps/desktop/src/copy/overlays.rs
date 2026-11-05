//! v12/overlays: sheets, alerts, the Spotlight panel, the login window, the
//! update sheet and quiet status lines (docs/design/NATIVE.md §7, §9.7).

crate::copy! {
    // --- v12/overlays: sheets ---
    NEW_TOPIC_TITLE = "新话题",
    NEW_TOPIC_EXPLANATION = "写下要做的事；在第一条消息里 @ 成员，就交给了他。",
    NEW_TOPIC_GROUP = "群",
    NEW_TOPIC_NAME = "标题",
    NEW_TOPIC_NAME_PLACEHOLDER = "可以不填，默认取第一行",
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
    STOP_MESSAGE = "{name} 会停在现在这一步，已经做好的部分会留下。",
    STOP_OK = "停止",
    // --- v12/overlays: update sheet ---
    UPDATE_TITLE = "新版本 {version} 可以更新了",
    UPDATE_EXPLANATION = "更新时应用会重新打开，大约需要一分钟。",
    UPDATE_DOWNLOADING_TITLE = "正在下载 {version}",
    UPDATE_INSTALLING_TITLE = "正在安装 {version}",
    UPDATE_MANUAL_TITLE = "需要手动安装 {version}",
    UPDATE_FAILED_TITLE = "没能更新到 {version}",
    UPDATE_HIDE = "隐藏",
    UPDATE_STATUS = "正在下载 {version} · {percent}%",
    // --- v12/overlays: login ---
    LOGIN_CONTINUE = "继续",
    // --- v12/overlays: quiet status ---
    STATUS_RETRY = "重连",
    // --- v12/fx-overlays: blocking errors and footnotes ---
    ALERT_OK = "好",
    NEW_GROUP_REFUSED = "没能新建群",
    ADD_MEMBERS_REFUSED = "没能添加成员",
    RENAME_GROUP_REFUSED = "没能重命名这个群",
    RENAME_TOPIC_REFUSED = "没能改这个话题的标题",
    PROFILE_REFUSED = "没能保存你的资料",
    DEVICE_SIGN_OUT_MENU = "退出这台设备的登录…",
    INVITE_REFUSED = "没能邀请成员",
    BOT_REFUSED = "没能添加 AI 成员",
    LOGIN_REFUSED = "没能加入团队",
    STOP_REFUSED = "没能停止",
    TAKE_OVER_REFUSED = "没能接管",
    HAND_BACK_REFUSED = "没能交还",
    DM_REFUSED = "没能开始私聊",
    // --- v12/fx-overlays: update ---
    MENU_CHECK = "检查更新…",
    MENU_UPDATE_TO = "更新到 {version}",
    MENU_DOWNLOADING = "正在下载 {version}（{percent}%）",
    MENU_RESTART = "重启安装 {version}",
    UPDATE_READY_TITLE = "新版本 {version} 已下载好",
    UPDATE_READY_EXPLANATION = "重启后就是新版本，大约需要一分钟。",
    UPDATE_RESTART = "重启安装",
    UPDATE_READY = "新版本 {version} 已下载好",
}
