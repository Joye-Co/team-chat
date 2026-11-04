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
}
