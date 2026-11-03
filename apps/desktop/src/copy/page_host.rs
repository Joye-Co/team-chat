//! 产出 pages that talk with their member (inc-host track, INTERACT §4.4 / §5.5 / §8.4,
//! REFS §A7, MEDIA §1 states).

crate::copy! {
    PAGE_SENDS_TO = "页面里的操作会发给 {name}",
    PAGE_RUN_LOG = "回答会记在 {name} 的工作记录里",
    PAGE_READ_ONLY = "页面能做的：读这份数据",
    PAGE_WAITING = "等 {name} 回应",
    PAGE_STILL_WORKING = "{name} 还在处理",
    PAGE_NO_ANSWER = "{name} 没有回应",
    PAGE_OFFLINE = "{name} 不在线，这一步没有发出",
    PAGE_BUSY = "{name} 现在接不了，稍后再试",
    PAGE_PUSHED = "{name} 推来了新数据",
    PAGE_ANSWERED = "{name} 在页面里回答了",
    PAGE_RETRY = "重试",
    PAGE_STOP_WAITING = "不等了",
    PAGE_WATCH = "实时查看",
    PAGE_UPDATED_TO = "已更新到 v{n}",
    PAGE_NEWER = "有 v{n}",
    PAGE_REF_MENU = "就这里提问",
    PAGE_REF_ADDED = "已放进话题的输入框",
    PAGE_REF_LABEL = "{title} v{n} · {label}",
}
