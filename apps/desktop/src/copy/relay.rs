//! 按顺序做 (docs/plans/usable/design-relay.zh.md §3.6): the bar above a 话题 whose work goes
//! from member to member in a written order, the composer hints for writing one, and who is
//! at work in the 话题. Never 接力 as a noun; the step states are the closed set of status
//! words (在做, 排队, 做完了, 卡住了) and 停止.

crate::copy! {
    // The bar.
    RELAY_TITLE = "按顺序做",
    RELAY_STEP_OF = "第 {k}/{n} 步",
    RELAY_ALL_DONE = "都做完了",
    RELAY_STOPPED_AT = "停在 {name}：{reason}",
    RELAY_STOPPED_BY = "{name} 停止了",
    RELAY_STOP = "停止",
    RELAY_RETRY = "重试",
    RELAY_STOP_REFUSED = "没能停止",
    RELAY_RETRY_REFUSED = "没能重试",
    // A step's state.
    RELAY_STEP_WORKING = "在做",
    RELAY_STEP_QUEUED = "排队",
    RELAY_STEP_DONE = "做完了",
    RELAY_STEP_BLOCKED = "卡住了",
    RELAY_STEP_STOPPED = "停止了",
    // Why it stopped, or what the current step waits for.
    RELAY_WHY_UNAVAILABLE = "{name} 现在接不了这件事",
    RELAY_WHY_QUEUED = "等 {name} 手上的做完",
    RELAY_WHY_BLOCKED = "卡住了",
    RELAY_WHY_STOPPED_BY = "被 {name} 停止",
    RELAY_WHY_LIMIT = "交出次数到上限了",
    RELAY_WHY_NOT_PUSHED = "{name} 的代码还没推上去",
    RELAY_WHY_REQUESTER_LEFT = "发起的人已经不在这个群",
    RELAY_WHY_UNDELIVERED = "没有交到 {name} 手上",
    RELAY_WHY_UNKNOWN = "停住了",
    // The composer.
    RELAY_HINT_ORDER = "按顺序做：{names}",
    RELAY_HINT_DISCOVER = "想让他们按顺序做？用 → 连起来",
    RELAY_HINT_IN_TOPIC = "按顺序做要在话题里用",
    RELAY_HINT_RUNNING = "这个话题已经在按顺序做，先停止它",
    RELAY_REFUSED_BAD_STEPS = "按顺序做要写 2 到 6 步，同一个成员不能挨着",
    RELAY_NAMES_SEP = "、",
    RELAY_ARROW = "→",
    // Who is at work in the 话题 (详情 → 成员).
    PEOPLE_WORKING = "在做",
    PEOPLE_TOOK_PART = "参与过",
}
