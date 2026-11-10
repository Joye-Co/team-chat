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
    static let signInEmailHint = "用你的登录邮箱，我们发一个 6 位验证码给你。"
    static let signInEmail = "邮箱"
    static let signInSendCode = "发送验证码"
    static let signInServer = "服务器地址"
    static let signInOtherWay = "用登录凭证登录"
    static let signInDevHint = "开发用的登录：填服务器地址和登录凭证。"
    static let signInCredential = "登录凭证"
    static let signInContinue = "继续"
    static let signOut = "退出登录"

    static let codeTitle = "输入验证码"
    static func codeSentTo(_ email: String) -> String { "6 位验证码已经发到 \(email)" }
    static let codeField = "验证码"
    static let codeSubmit = "登录"
    static func codeResendIn(_ seconds: Int) -> String { "\(seconds) 秒后可以重新发送" }
    static let codeResend = "重新发送"
    static let codeChangeEmail = "换个邮箱"

    // Failures
    static let failNetwork = "连不上服务器，检查网络后再试。"
    static let failCode = "验证码不对，或者已经过期。"
    static let failTooMany = "试的次数太多了，过一会儿再试。"
    static let failEmail = "邮箱格式不对。"
    static let failServer = "服务器地址不对。"
    static let failOther = "出了点问题，再试一次。"

    // 群
    static let groupsTitle = "群"
    static let directSection = "私聊"
    static let groupsEmpty = "还没有群"
    static let groupsEmptyHint = "有人把你拉进群，或者给你发私聊，就会出现在这里。"
    static let groupChat = "群聊"
    static func memberCount(_ count: UInt32) -> String { "\(count) 位成员" }
    static let nameSeparator = "、"
    static let more = "更多"

    // 话题
    static let topics = "话题"
    static let topicsEmpty = "还没有话题"
    static let topicsEmptyHint = "在电脑上或群聊里开一个话题，它会出现在这里。"
    static func messageCount(_ count: UInt64) -> String { "\(count) 条消息" }
    static let statusInProgress = "进行中"
    static let statusWaitingAnswer = "等回答"
    static let statusWaitingReview = "等审"
    static let statusWaitingMyAnswer = "等你回答"
    static let statusWaitingMyReview = "等你审"
    static func statusWaitingAnswerOf(_ name: String) -> String { "等\(name)回答" }
    static func statusWaitingReviewOf(_ name: String) -> String { "等\(name)审" }
    static let statusBlocked = "卡住了"
    static let statusDone = "完成"

    // Messages
    static let composerPlaceholder = "发消息，或 @ 成员交代"
    static let send = "发送"
    static let today = "今天"
    static let yesterday = "昨天"
    static let newMessages = "新消息"
    static let loadOlder = "看更早的消息"
    static let loadingOlder = "正在加载"
    static let edited = "已编辑"
    static let deleted = "这条消息已删除"
    static let writing = "正在写"
    static let chatEmpty = "还没有消息"
    static let chatEmptyHint = "说点什么，或者 @ 一位成员交代一件事。"

    static let quote = "引用"
    static let copyText = "复制"
    static let cancelQuote = "取消引用"
    static func quoting(_ name: String) -> String { "引用 \(name)" }
    static let addReaction = "加表情"
    static func reactionLabel(_ names: String, _ emoji: String) -> String { "\(names) 回应了 \(emoji)" }
    static let you = "你"

    static let sending = "发送中"
    static let notSent = "没有发出"
    static let retry = "重试"
    static let discard = "删除"
    static let offlineBanner = "现在离线，消息会在连上后发出。"

    static let imageFailed = "图片打不开"
    static let fileOpen = "打开"

    // Connection: shown only when it is not normal
    static let connecting = "正在连接"
    static let offline = "离线"
    static let signInAgain = "要重新登录"
    static let updateApp = "要更新应用"
}
