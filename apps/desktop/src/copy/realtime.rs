//! Real-time topic features: the status menu, follow, unread jump, typing.

crate::copy! {
    STATUS_MENU = "更多",
    RENAME = "重命名…",
    RENAME_TITLE = "重命名话题",
    TOPIC_TITLE_LABEL = "标题",
    TOPIC_TITLE_REQUIRED = "请填写标题",
    OWNER_LABEL = "负责人：{name}",
    FOLLOW = "关注话题",
    UNFOLLOW = "取消关注",
    FOLLOWING = "已关注",
    TOPIC_MUTE = "静音话题",
    TOPIC_UNMUTE = "取消静音",
    // hide and delete of a 话题 (hide-delete/1)
    TOPIC_HIDE = "隐藏话题",
    TOPIC_DELETE = "删除话题…",
    TOPIC_DELETE_CONFIRM_TITLE = "删除话题「{name}」？",
    TOPIC_DELETE_CONFIRM_BODY = "话题里的消息和产出会对所有人删除。\n删除后不能恢复。",
    TOPIC_DELETE_OK = "删除",
    JUMP_UNREAD = "{n} 条未读",
    TOPIC_TYPING = "{name} 正在输入…",
    TOPIC_TYPING_MANY = "{n} 人正在输入…",
}
