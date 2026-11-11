import SwiftUI
import UIKit
import UserNotifications

/// 通知: the system permission, the APNs device token, notification categories and actions,
/// the badge, and opening the chat a notification is about.
///
/// Pushes reach the phone while the app is not connected (the server holds them while this
/// sign-in has a connection). The server says what each push is about in its `tc` data
/// (`parsePush`); an answerable question carries up to three answers that need no more input.
@MainActor
final class PushCenter: NSObject, ObservableObject {
    static let shared = PushCenter()

    /// The chat a tapped notification asks to open; the 群 tab navigates and clears it.
    @Published var route: Route?
    @Published private(set) var status: UNAuthorizationStatus = .notDetermined
    /// The AllowNotify screen is up.
    @Published var asking = false

    nonisolated static let answerPrefix = AskCategory.answerPrefix

    private static let askedKey = "notify.asked"
    private var deviceToken: String?

    private var defaults: UserDefaults {
        Session.appGroup.flatMap { UserDefaults(suiteName: $0) } ?? .standard
    }

    /// The AllowNotify screen was shown once (answered or put off).
    var asked: Bool { defaults.bool(forKey: Self.askedKey) }

    func refreshStatus() async {
        status = await UNUserNotificationCenter.current().notificationSettings().authorizationStatus
        if status == .authorized || status == .provisional || status == .ephemeral {
            UIApplication.shared.registerForRemoteNotifications()
        }
    }

    /// Shows AllowNotify once, at a moment when a notification is clearly useful.
    func offerIfUseful() {
        guard status == .notDetermined, !asked, !asking else { return }
        asking = true
    }

    /// The system prompt, from AllowNotify's button or the settings screen.
    func requestPermission() async {
        defaults.set(true, forKey: Self.askedKey)
        asking = false
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound])
        await refreshStatus()
    }

    func putOff() {
        defaults.set(true, forKey: Self.askedKey)
        asking = false
    }

    /// Registers the answer buttons of questions already in Notification Center (pushes the
    /// notification extension did not see, such as `simctl push` in the simulator).
    func registerDeliveredCategories() async {
        let delivered = await UNUserNotificationCenter.current().deliveredNotifications()
        let made = delivered.compactMap { n in
            AskCategory.push(n.request.content.userInfo).flatMap { AskCategory.make($0, openTitle: Copy.notifyOpen) }
        }
        await AskCategory.register(made)
    }

    // MARK: Device token

    func didRegister(token data: Data) {
        let hex = data.map { String(format: "%02x", $0) }.joined()
        deviceToken = hex
        sendToken()
    }

    /// Tells the server this sign-in's phone wants pushes (after sign-in and at every launch:
    /// APNs may hand out a new token).
    func sendToken() {
        guard let hex = deviceToken, let saved = Session.saved() else { return }
        let appId = Bundle.main.bundleIdentifier ?? ""
        #if DEBUG
            let sandbox = true
        #else
            let sandbox = false
        #endif
        Task.detached {
            try? registerPushDevice(baseUrl: saved.server, token: saved.token, pushToken: hex, appId: appId, sandbox: sandbox)
        }
    }

    /// Before signing out: this phone stops getting pushes for the old sign-in.
    nonisolated static func forget(server: String, token: String) {
        _ = try? unregisterPushDevice(baseUrl: server, token: token)
    }

    /// The number of topics that wait on me, on the app icon.
    func refreshBadge() {
        guard let saved = Session.saved() else { return }
        Task.detached {
            guard let waiting = try? pushBadge(baseUrl: saved.server, token: saved.token) else { return }
            await MainActor.run { UNUserNotificationCenter.current().setBadgeCount(Int(waiting)) }
        }
    }

    // MARK: Responses

    /// A tap or an action on a notification.
    nonisolated func handle(response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        guard let push = PushCenter.info(from: info) else { return }
        let action = response.actionIdentifier
        if action.hasPrefix(Self.answerPrefix), let message = push.askMessageId {
            let actionId = String(action.dropFirst(Self.answerPrefix.count))
            let answered = await PushCenter.answer(push: push, message: message, action: actionId, request: response.notification.request.identifier)
            if answered { return }
            await PushCenter.tellAnswerFailed(push: push)
            return
        }
        if action == UNNotificationDismissActionIdentifier { return }
        await MainActor.run { self.open(push) }
    }

    func open(_ push: PushInfo) {
        let group = push.groupId
        let topic = push.topicId ?? group
        route = .chat(group: group, topic: topic)
    }

    nonisolated static func info(from userInfo: [AnyHashable: Any]) -> PushInfo? {
        AskCategory.push(userInfo)
    }

    /// Answers over HTTP: the app may be in the background with no connection.
    nonisolated static func answer(push: PushInfo, message: String, action: String, request: String) async -> Bool {
        guard let saved = Session.saved() else { return false }
        return await Task.detached {
            (try? answerFromNotification(
                baseUrl: saved.server, token: saved.token, groupId: push.groupId, messageId: message,
                rev: push.askRev, actionId: action, clientEventId: "push-\(request)".prefix(64).description)) ?? false
        }.value
    }

    /// The answer did not go through (answered elsewhere, changed, offline): say so, and a tap
    /// opens the chat.
    nonisolated static func tellAnswerFailed(push: PushInfo) async {
        let content = UNMutableNotificationContent()
        content.body = Copy.notifyAnswerFailed
        content.threadIdentifier = push.topicId ?? push.groupId
        var tc: [String: Any] = ["v": 1, "reason": push.reason, "groupId": push.groupId, "seq": push.seq]
        if let topic = push.topicId { tc["topicId"] = topic }
        content.userInfo = ["tc": tc]
        try? await UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    }
}

extension PushCenter: UNUserNotificationCenterDelegate {
    // The completion-handler forms, finished on the main thread: with the async forms the system
    // gets the answer on a background thread and UIKit aborts ("Call must be made on main
    // thread") when a tap launches the app.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        // In front, the chat shows the message itself; a push still arrives while the
        // connection is down, so show it then, with its answers.
        let done = Unchecked(completionHandler)
        let info = Unchecked(notification.request.content.userInfo)
        Task { @MainActor in
            if let push = AskCategory.push(info.value), let made = AskCategory.make(push, openTitle: Copy.notifyOpen) {
                await AskCategory.register([made])
            }
            done.value([.banner, .list, .sound, .badge])
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let done = Unchecked(completionHandler)
        let response = Unchecked(response)
        Task { @MainActor in
            await self.handle(response: response.value)
            done.value()
        }
    }
}

/// Carries a value the compiler cannot prove Sendable (a system callback) across to the main
/// actor; each one is used once, there.
private struct Unchecked<T>: @unchecked Sendable {
    let value: T
    init(_ value: T) { self.value = value }
}

/// Registers with APNs and receives the device token (`UIApplicationDelegateAdaptor`).
final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        MainActor.assumeIsolated {
            UNUserNotificationCenter.current().delegate = PushCenter.shared
        }
        return true
    }

    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        MainActor.assumeIsolated { PushCenter.shared.didRegister(token: deviceToken) }
    }

    func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
        // Unsigned simulator builds have no push entitlement: notifications still work from
        // `simctl push`; nothing to register.
    }
}

extension AppModel {
    /// Notifications are useful once something waits on me: offer AllowNotify then.
    func offerNotificationsIfUseful() {
        let waits = groups.contains { $0.topics.contains { $0.waitingOnMe } }
        if waits { PushCenter.shared.offerIfUseful() }
    }

    /// After I sent something (I care about the answer): offer AllowNotify.
    func offerNotificationsAfterSend() {
        PushCenter.shared.offerIfUseful()
    }

    /// The chat a tapped notification asks for, once. A push without a 话题 opens the group's own chat.
    func takeNotifiedRoute() -> Route? {
        guard case .chat(let group, let topic)? = PushCenter.shared.route else { return nil }
        PushCenter.shared.route = nil
        return .chat(group: group, topic: topic == group ? (self.group(group)?.mainTopicId ?? topic) : topic)
    }

    /// Back in front: the permission may have changed in Settings, the badge may be stale.
    func notifyCameToForeground() {
        Task {
            await PushCenter.shared.registerDeliveredCategories()
            await PushCenter.shared.refreshStatus()
            PushCenter.shared.refreshBadge()
            PushCenter.shared.sendToken()
        }
    }
}
