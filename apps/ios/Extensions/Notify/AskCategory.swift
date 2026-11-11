import UserNotifications

/// The notification category of an answerable question: `tc.ask.<messageId>.<rev>`, with the
/// card's answers as buttons and 打开. The server names it in the push; the notification
/// extension registers it when the push arrives, and the app registers it for notifications it
/// finds delivered (a push that skipped the extension). Shared by the app and the extension.
enum AskCategory {
    static let prefix = "tc.ask."
    static let answerPrefix = "answer."
    static let openAction = "open"
    /// Per-message categories kept registered (the newest).
    static let keep = 30

    static func identifier(_ push: PushInfo) -> String? {
        guard let message = push.askMessageId, !push.actions.isEmpty else { return nil }
        return "\(prefix)\(message).\(push.askRev)"
    }

    static func make(_ push: PushInfo, openTitle: String) -> UNNotificationCategory? {
        guard let id = identifier(push) else { return nil }
        // An answer needs the phone unlocked; it can be taken back, so no confirmation.
        var actions = push.actions.map {
            UNNotificationAction(identifier: answerPrefix + $0.id, title: $0.label, options: [.authenticationRequired])
        }
        actions.append(UNNotificationAction(identifier: openAction, title: openTitle, options: [.foreground]))
        return UNNotificationCategory(identifier: id, actions: actions, intentIdentifiers: [], options: [])
    }

    /// Adds categories, keeping the others and the newest per-message ones.
    static func register(_ made: [UNNotificationCategory]) async {
        guard !made.isEmpty else { return }
        let center = UNUserNotificationCenter.current()
        let existing = await center.notificationCategories()
        let others = existing.filter { !$0.identifier.hasPrefix(prefix) }
        let asks = existing.filter { $0.identifier.hasPrefix(prefix) && !made.contains($0) }.prefix(max(0, keep - made.count))
        center.setNotificationCategories(others.union(asks).union(made))
    }

    static func push(_ userInfo: [AnyHashable: Any]) -> PushInfo? {
        guard let tc = userInfo["tc"], JSONSerialization.isValidJSONObject(tc),
            let data = try? JSONSerialization.data(withJSONObject: tc),
            let json = String(data: data, encoding: .utf8)
        else { return nil }
        return parsePush(json: json)
    }
}
