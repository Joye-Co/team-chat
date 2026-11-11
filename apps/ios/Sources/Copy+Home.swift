import Foundation

/// Words of the 首页 and of 交代 (M2).
extension Copy {
    static let homeWaiting = "等你的事"
    static let homeWorking = "正在做"
    static let homeDoneToday = "今天做完的"
    static let homeAllClear = "没有等你的事"
    static let homeAllClearHint = "成员要你拍板时，会出现在这里。"
    static let homeNothingWorking = "现在没有在做的话题"
    static let homeNothingDone = "今天还没有做完的话题"
    static func homeIn(_ name: String, _ group: String) -> String { group.isEmpty ? name : "\(name) · \(group)" }
    static let homeOpenTopic = "打开话题"

    static let handOff = "交代一件事"
    static let handOffTo = "交代给"
    static let handOffWhere = "在哪里"
    static let handOffWhat = "要做什么"
    static let handOffPlaceholder = "说清楚要什么结果，比如：把登录页的测试补齐，明天上午给我看"
    static let handOffSend = "交代"
    static let handOffCancel = "取消"
    static let handOffAi = "AI 成员"
    static let handOffPeople = "成员"
    static let handOffNoPlace = "你们还不在同一个群里"
    static func handOffSent(_ name: String, _ place: String) -> String { "已交代给 \(name)，在「\(place)」开了一个话题" }
    static let handOffFailed = "没有交代出去，再试一次。"
}
