//! The topic view: header and message timeline.

crate::copy! {
    SELECT_TOPIC = "选择一个话题，或在下方写消息开始新话题",
    NEW_TOPIC_HINT = "新话题：第一行会作为标题",
    LOAD_OLDER = "加载更早的消息",
    LOADING_OLDER = "正在加载…",
    NO_OLDER = "已经到最早的消息",
    EDITED = "已编辑",
    DELETED = "此消息已删除",
    PENDING = "发送中",
    FAILED = "发送失败",
    RETRY = "重试",
    STREAMING = "正在输入…",
    TYPING_ONE = "{name} 正在输入…",
    TYPING_MANY = "{n} 人正在输入…",
    MESSAGE_COUNT = "{n} 条消息",
    QUOTE = "引用",
    EDIT = "编辑",
    DELETE = "删除",
    DELETE_CONFIRM = "删除这条消息？",
    SET_STATUS = "设置状态",
    TOGGLE_PANEL = "详情",
}
