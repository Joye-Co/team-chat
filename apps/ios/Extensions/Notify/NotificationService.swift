import UserNotifications

/// A shell: passes every notification through unchanged. Later it replaces the push's short
/// text with the message it announces, read with the token and the cache in the app group.
final class NotificationService: UNNotificationServiceExtension {
    private var deliver: ((UNNotificationContent) -> Void)?
    private var content: UNNotificationContent?

    override func didReceive(
        _ request: UNNotificationRequest,
        withContentHandler contentHandler: @escaping (UNNotificationContent) -> Void
    ) {
        deliver = contentHandler
        content = request.content
        contentHandler(request.content)
    }

    override func serviceExtensionTimeWillExpire() {
        if let deliver, let content { deliver(content) }
    }
}
