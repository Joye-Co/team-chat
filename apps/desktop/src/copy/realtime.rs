//! Real-time topic features: the status menu, follow, unread jump, typing.

crate::copy! {
    STATUS_MENU = "更多",
    RENAME = "重命名…",
    RENAME_TITLE = "重命名话题",
    TOPIC_TITLE_REQUIRED = "请填写标题",
    OWNER_LABEL = "负责人：{name}",
    // hide and delete of a 话题 (hide-delete/1)
    TOPIC_HIDE = "隐藏",
    TOPIC_DELETE = "删除…",
    TOPIC_DELETE_CONFIRM_TITLE = "删除这个话题？",
    TOPIC_DELETE_CONFIRM_BODY = "「{name}」\n所有成员都会失去里面的消息和产出。\n删除后不能恢复。",
    TOPIC_DELETE_OK = "删除",
    JUMP_UNREAD = "{n} 条未读",
    TOPIC_TYPING = "{name} 正在输入…",
    TOPIC_TYPING_MANY = "{n} 人正在输入…",
}
