import Foundation

/// Words of 通知: the permission screen, the settings screen and notification actions.
extension Copy {
    // AllowNotify: asked when something first waits on me or after my first message, never at launch.
    static let allowTitle = "需要你的时候，第一时间告诉你"
    static let allowBody = "应用没开时，这几件事会发通知给你："
    static let allowWaiting = "有事等你回复"
    static let allowWaitingHint = "成员问你、要你拍板，能直接在通知上回答。"
    static let allowMention = "有人 @你"
    static let allowMentionHint = "群里和私聊里提到你的消息。"
    static let allowResult = "交代的事有结果"
    static let allowResultHint = "你关注的话题做完了，或者有了新产出。"
    static let allowTurnOn = "打开通知"
    static let allowLater = "以后再说"
    static let allowFootnote = "不想被打扰时，可以在“我 › 通知”里关掉某一类，或者设勿扰时间。"

    // NotifySettings
    static let notifyTitle = "通知"
    static let notifySystemOn = "已打开"
    static let notifySystemOff = "已关闭"
    static let notifySystem = "通知"
    static let notifySystemOffHint = "通知在系统设置里关掉了，打开后才能收到。"
    static let notifyOpenSettings = "去系统设置打开"
    static let notifyAskNow = "打开通知"
    static let notifyRemindMe = "提醒我"
    static let notifyWaiting = "有事等我回复"
    static let notifyMention = "有人 @我"
    static let notifyResult = "我交代的事有结果"
    static let notifyBlocked = "成员卡住了"
    static let notifyQuiet = "勿扰"
    static let notifyQuietOn = "勿扰时间"
    static let notifyQuietFrom = "开始"
    static let notifyQuietTo = "结束"
    static let notifyQuietHint = "勿扰时间里不发通知，事情照样记着，回来在首页看。"
    static let notifyAllDevices = "这些设置对你所有的设备都有效。"
    static let notifyLoadFailed = "没读到通知设置，下拉再试。"
    static let notifySaveFailed = "没保存上，再试一次。"

    // Notification actions
    static let notifyOpen = "打开"
    static let notifyAnswered = "已回复"
    static let notifyAnswerFailed = "没回复上，打开再试一次。"
}
