import Foundation

/// The copy catalog: every string a user can read comes from here, and only from here.
///
/// Rules (docs/PRODUCT.md): Simplified Chinese; the product nouns are only 成员, 群, 话题 and
/// 产出; no developer words; the product name is never typed (it comes from product.json through
/// Info.plist). `apps/ios/check-copy.ts` enforces this and fails when another Swift file holds
/// Chinese text.
enum Copy {
    // Sign-in
    static let signInTitle = "登录"
    static let signInDevHint = "这一版只有开发用的登录：填服务器地址和登录凭证。"
    static let signInServer = "服务器地址"
    static let signInCredential = "登录凭证"
    static let signInContinue = "继续"
    static let signOut = "退出登录"

    // 群
    static let groupsTitle = "群"
    static let groupsSection = "群"
    static let directSection = "私聊"
    static let groupChat = "群聊"
    static let topicsEmpty = "还没有话题"
    static func messageCount(_ count: UInt64) -> String { "\(count) 条消息" }
    static let nameSeparator = "、"

    // 话题 status
    static let statusInProgress = "进行中"
    static let statusWaitingAnswer = "等回答"
    static let statusWaitingReview = "等审"
    static let statusBlocked = "卡住了"
    static let statusDone = "完成"

    // Messages
    static let composerPlaceholder = "回复，或 @ 成员交代"
    static let send = "发送"

    // Connection: shown only when it is not normal
    static let connecting = "正在连接"
    static let offline = "离线"
    static let signInAgain = "要重新登录"
    static let updateApp = "要更新应用"
}
