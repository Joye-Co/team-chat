import Foundation

/// Words of cards: questions, results and their records (M2).
extension Copy {
    static let cardAsk = "等你拍板"
    static let cardDone = "做完了"
    static let cardWorking = "正在做"
    static let cardFailed = "没做成"

    static let cardSending = "正在发出"
    static let cardOtherwiseText = "这张卡片有些内容要在电脑上看："
    static let cardEdit = "改一下"
    static let cardEditDone = "改好了"
    static let cardEdited = "改过稿子"
    static let cardChoose = "选一项"
    static let cardFill = "填写"
    static let cardRequired = "必填"
    static func cardMissing(_ label: String) -> String { "还没填：\(label)" }
    static let cardOpen = "打开"
    static let cardYes = "是"
    static let cardNo = "否"

    // The record a card turns into.
    static func cardAnsweredByMe(_ action: String) -> String { action.isEmpty ? "你已回答" : "你：\(action)" }
    static func cardAnsweredBy(_ name: String, _ action: String) -> String {
        action.isEmpty ? "\(name) 已回答" : "\(name)：\(action)"
    }
    static let cardAnsweredSomeone = "已经有人回答"
    static let cardExpired = "已过期，没有人回答"
    static let cardClosed = "已结束"
    static let cardSuperseded = "已有新的版本"
    static let cardNotForMe = "在等别人回答"

    // A refused answer.
    static func cardRefused(_ reason: String) -> String {
        switch reason {
        case "card.already_answered": "没有发出：已经有人回答了。"
        case "card.expired": "没有发出：已经过期了。"
        case "card.stale_rev", "card.superseded", "card.rev_limit": "没有发出：卡片刚更新过，看一下再回答。"
        case "forbidden": "没有发出：这个问题不是问你的。"
        default: "没有发出，再试一次。"
        }
    }

    // Approval: one more look, then Face ID.
    static let approveSheetTitle = "确认一下"
    static func approveVerb(_ verb: String, _ target: String) -> String { "\(verb)：\(target)" }
    static let approveConsequence = "会发生什么"
    static func approveTypeToConfirm(_ word: String) -> String { "输入「\(word)」确认" }
    static let approveCancel = "取消"
    static let approveFaceReason = "确认你本人在批准"
    static let approveFailed = "没有验证通过，没有发出。"

    static let evidencePass = "通过"
    static let evidenceFail = "没通过"
}
