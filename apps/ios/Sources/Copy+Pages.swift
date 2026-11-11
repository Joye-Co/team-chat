import Foundation

/// Words of the pages beyond chat: 产出, 成员, 搜索, 单独问, 实时查看 and 详情.
extension Copy {
    // 产出
    static let artifactsTitle = "产出"
    static let artifactsEmpty = "还没有产出"
    static let artifactsEmptyHint = "成员在话题里做出的页面和文件会出现在这里。"
    static let allGroups = "全部群"
    static let artifactPage = "页面"
    static let artifactFile = "文件"
    static func artifactVersion(_ v: UInt32) -> String { "v\(v)" }
    static func artifactVersionLong(_ v: UInt32) -> String { "第 \(v) 版" }
    static let artifactCurrent = "最新"
    static let artifactVersions = "版本"
    static let artifactOpening = "正在打开"
    static let artifactFailed = "打不开这份产出"
    static let artifactFailedHint = "检查网络后再试。"
    static let artifactOpenFile = "打开文件"
    static let artifactOrigin = "来龙去脉"
    static func artifactMadeBy(_ name: String) -> String { "\(name) 做的" }
    static func fileCount(_ n: UInt32) -> String { "\(n) 个文件" }
    static let tryAgain = "再试一次"
    static let openTopic = "打开话题"

    // 成员
    static let membersTitle = "成员"
    static let membersAll = "全部"
    static let membersWorking = "在做事"
    static let membersWorkingEmpty = "现在没有成员在做事"
    static let online = "在线"
    static let notOnline = "不在线"
    static let aiMember = "AI 成员"
    static func working(_ what: String) -> String { "在做：\(what)" }
    static let thisWeek = "这周"
    static let statDone = "做完"
    static let statRuns = "做了"
    static let statHandedOut = "交代"
    static let statArtifacts = "产出"
    static let statWorking = "正在做"
    static let now = "现在"
    static let recentWork = "最近"
    static let recentWorkEmpty = "这周还没有做过事"
    static let sharedGroups = "一起在的群"
    static let directMessage = "私聊"
    static let directMessageFailed = "没能打开私聊，检查网络后再试。"
    static let itsYou = "这是你"

    // 搜索
    static let searchPrompt = "搜消息、话题、产出和成员"
    static let searchAll = "全部"
    static let searchMessages = "消息"
    static let searchHint = "输入要找的字"
    static let searchNothing = "没有找到"
    static func searchNothingFor(_ q: String) -> String { "没有和“\(q)”相关的内容" }
    static let searchOffline = "现在离线，只搜了手机里已有的话题、产出和成员。"
    static let searching = "正在搜索"

    // 单独问
    static let askAside = "单独问"
    static let askAsideHint = "只有你能看到问题和回答。成员读过这个话题到这条消息为止的内容。"
    static let askAsideWho = "问谁"
    static let askAsidePlaceholder = "想问什么？"
    static let askAsideSend = "问"
    static func askAsideSent(_ name: String) -> String { "已经问了 \(name)，回答会出现在下面。" }
    static let askAsideFailed = "没有问出去，检查网络后再试。"
    static let askAsideEarlier = "你问过的"
    static let askAsideNoMember = "这个群里没有能单独问的成员。"
    static let close = "关闭"
    static let about = "关于"

    // 详情
    static let details = "详情"
    static let groupInfoTitle = "群资料"
    static let purpose = "这个群做什么"
    static let purposeEmpty = "还没写这个群做什么"
    static func topicsCount(_ n: UInt32) -> String { "\(n) 个话题" }
    static func artifactsCount(_ n: UInt32) -> String { "\(n) 份产出" }
    static let recentArtifacts = "最近的产出"
    static let workingNow = "正在做"
    static let allTopics = "全部话题"
    static let plan = "计划"
    static let runsSection = "成员做的事"
    static let topicArtifacts = "产出"

    // 实时查看
    static let liveView = "实时查看"
    static let liveWaiting = "还没有动静，成员一开始做就会显示在这里。"
    static let liveEnded = "这次已经结束"
    static let stop = "停止"
    static let stopConfirm = "停止这次的工作？"
    static func stopConfirmHint(_ name: String) -> String { "\(name) 会停在现在这一步，已经做的不会撤回。" }
    static let stopped = "已经停了"
    static let stopFailed = "没能停止，检查网络后再试。"
    static let lockScreenShow = "在锁屏上显示"
    static let lockScreenHide = "不在锁屏上显示"
    static let lockScreenUnavailable = "这台手机没有打开实时活动。"
    static func stepOf(_ done: UInt32, _ total: Int) -> String { "第 \(min(Int(done) + 1, total)) 步，共 \(total) 步" }
    static let summary = "进展"

    // Run status words (DESIGN.md §9.1)
    static let runWorking = "正在做"
    static let runQueued = "排队"
    static let runWaiting = "等人"
    static let runBlocked = "卡住了"
    static let runDone = "做完了"
    static let runFailed = "失败"
    static let runStopped = "停止了"
}
