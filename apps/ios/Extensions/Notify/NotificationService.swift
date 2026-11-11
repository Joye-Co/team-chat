import UserNotifications

/// Fills a push before the system shows it. The server's alert already says what happened; here
/// the extension, with the sign-in from the shared Keychain and app group (`Session`):
/// - hides the content when this phone is no longer signed in;
/// - registers the category of an answerable question, so its answers show as buttons (only
///   answers the server marked safe: they need no more input and can be taken back), plus 打开;
/// - groups the notifications of one 话题 and ranks questions that wait on me first.
final class NotificationService: UNNotificationServiceExtension {
    private var deliver: ((UNNotificationContent) -> Void)?
    private var best: UNMutableNotificationContent?

    override func didReceive(
        _ request: UNNotificationRequest,
        withContentHandler contentHandler: @escaping (UNNotificationContent) -> Void
    ) {
        deliver = contentHandler
        guard let content = request.content.mutableCopy() as? UNMutableNotificationContent else {
            contentHandler(request.content)
            return
        }
        best = content
        guard Session.saved() != nil else {
            content.title = ""
            content.subtitle = ""
            content.body = Copy.signedOut
            content.categoryIdentifier = ""
            finish(content)
            return
        }
        guard let push = AskCategory.push(content.userInfo) else {
            finish(content)
            return
        }
        content.threadIdentifier = push.topicId ?? push.groupId
        content.relevanceScore = push.reason == "waiting" ? 1 : push.reason == "mention" ? 0.8 : 0.5
        guard let category = AskCategory.make(push, openTitle: Copy.open) else {
            finish(content)
            return
        }
        content.categoryIdentifier = category.identifier
        Task {
            await AskCategory.register([category])
            self.finish(content)
        }
    }

    override func serviceExtensionTimeWillExpire() {
        if let best { finish(best) }
    }

    private func finish(_ content: UNNotificationContent) {
        guard let deliver else { return }
        self.deliver = nil
        deliver(content)
    }
}
