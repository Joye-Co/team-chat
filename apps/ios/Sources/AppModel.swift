import Foundation
import SwiftUI
import UserNotifications

/// Owns the `ChatClient`. The lists every screen shows (群, 成员, the connection) are published
/// here; a chat reads its own messages and reads them again when its topic's tick changes.
@MainActor
final class AppModel: ObservableObject {
    @Published private(set) var signedIn = false
    @Published private(set) var connection = "stopped"
    @Published private(set) var myName = ""
    @Published private(set) var groups: [GroupItem] = []
    @Published private(set) var members: [MemberItem] = []
    /// Bumped when the messages of a topic changed (key: topic id).
    @Published private(set) var topicTicks: [String: Int] = [:]
    /// Bumped when the 话题 list of a group changed (key: group id).
    @Published private(set) var groupTicks: [String: Int] = [:]
    /// Bumped per key when a feature's data changed: `artifacts:<topic>`, `runs:<topic>`,
    /// `card:<message>`. Screens watch their key.
    @Published private(set) var ticks: [String: Int] = [:]
    /// The newest refusal of a card answer of mine (key: message id).
    @Published private(set) var cardRefusals: [String: String] = [:]
    /// Topics whose `loadOlder` answer said there is nothing older.
    @Published private(set) var historyEnded: Set<String> = []

    /// Feature files (`AppModel+<Feature>.swift`) reach the core through this.
    private(set) var client: ChatClient?
    private var groupsReloadQueued = false

    init() {
        // Development sign-in: the sandbox passes the server and a token through the
        // environment (`simctl launch` with SIMCTL_CHILD_*).
        let env = ProcessInfo.processInfo.environment
        if let url = env["TC_SERVER_URL"], let token = env["TC_TOKEN"], !url.isEmpty, !token.isEmpty {
            _ = signIn(server: url, token: token)
        } else if let saved = Session.saved() {
            _ = open(server: saved.server, token: saved.token)
        }
    }

    // MARK: Sign-in

    /// Signs in and remembers it for the next launch. Returns what went wrong, if anything.
    @discardableResult
    func signIn(server: String, token: String) -> String? {
        if let failure = open(server: server, token: token) { return failure }
        do {
            try Session.save(server: server, token: token)
            return nil
        } catch {
            return Copy.failOther
        }
    }

    func signOut() {
        if let saved = Session.saved() { Task.detached { PushCenter.forget(server: saved.server, token: saved.token) } }
        UNUserNotificationCenter.current().setBadgeCount(0)
        client?.stop()
        client = nil
        Session.clear()
        signedIn = false
        groups = []
        members = []
        myName = ""
        connection = "stopped"
        topicTicks = [:]
        groupTicks = [:]
        ticks = [:]
        cardRefusals = [:]
        historyEnded = []
    }

    private func open(server: String, token: String) -> String? {
        do {
            let client = try ChatClient(baseUrl: server, token: token, dataDir: Session.dataDir().path)
            client.listen(listener: Listener(model: self))
            client.start()
            self.client = client
            signedIn = true
            connection = client.connectionState()
            reloadDirectory()
            return nil
        } catch {
            return AppModel.message(for: error)
        }
    }

    /// Asks for a sign-in code. Returns what went wrong, if anything.
    nonisolated func requestCode(server: String, email: String) async -> String? {
        await Task.detached {
            do {
                _ = try requestEmailCode(baseUrl: server, email: email)
                return nil
            } catch {
                return AppModel.message(for: error, invalid: Copy.failEmail)
            }
        }.value
    }

    /// Signs in with the emailed code. Returns what went wrong, if anything.
    func signIn(server: String, email: String, code: String, device: String) async -> String? {
        let result: Result<SignedIn, Error> = await Task.detached {
            Result { try emailSignIn(baseUrl: server, email: email, code: code, deviceName: device) }
        }.value
        switch result {
        case .success(let signed): return signIn(server: server, token: signed.token)
        case .failure(let error): return AppModel.message(for: error, invalid: Copy.failCode)
        }
    }

    nonisolated static func message(for error: Error, invalid: String = Copy.failServer) -> String {
        switch error as? CoreError {
        case .Network: Copy.failNetwork
        case .Unauthorized: Copy.failCode
        case .RateLimited: Copy.failTooMany
        case .Invalid: invalid
        default: Copy.failOther
        }
    }

    // MARK: Reads

    func cameToForeground() {
        client?.reconnectNow()
    }

    func group(_ id: String) -> GroupItem? {
        groups.first { $0.id == id }
    }

    func member(_ id: String) -> MemberItem? {
        members.first { $0.id == id }
    }

    func topics(in group: String) -> [TopicItem] {
        (try? client?.topics(groupId: group, limit: 200)) ?? []
    }

    func topic(_ id: String, in group: String) -> TopicItem? {
        (try? client?.topic(groupId: group, topicId: id)) ?? nil
    }

    func messages(in topic: String, limit: Int) -> [MessageItem] {
        (try? client?.messages(topicId: topic, limit: UInt32(limit))) ?? []
    }

    func pending(in topic: String) -> [PendingItem] {
        (try? client?.pending(topicId: topic)) ?? []
    }

    /// The file on disk, downloaded when it is not in the cache.
    func filePath(_ file: FileItem) async -> String? {
        if let cached = file.cachedPath { return cached }
        guard let client else { return nil }
        let ref = file.refJson
        return await Task.detached { try? client.filePath(refJson: ref) }.value
    }

    // MARK: Writes

    func send(_ body: String, group: String, topic: String, mentions: [String], quote: String?) {
        _ = try? client?.send(groupId: group, topicId: topic, body: body, mentions: mentions, quoteId: quote)
        offerNotificationsAfterSend()
    }

    func retry(_ id: String) {
        try? client?.retry(id: id)
    }

    func discard(_ id: String) {
        try? client?.discard(id: id)
    }

    func react(_ message: String, emoji: String, on: Bool) {
        try? client?.react(messageId: message, emoji: emoji, on: on)
    }

    func markRead(group: String, topic: String, seq: UInt64) {
        try? client?.markRead(groupId: group, topicId: topic, seq: seq)
    }

    func loadOlder(_ topic: String) {
        try? client?.loadOlder(topicId: topic)
    }

    // MARK: Updates

    /// A chat registers its topic so its tick exists before the first change.
    func watch(topic: String) {
        if topicTicks[topic] == nil { topicTicks[topic] = 0 }
    }

    fileprivate func changed(_ update: Update) {
        guard let client else { return }
        switch update {
        case .connection:
            connection = client.connectionState()
        case .directory:
            reloadDirectory()
        case .group:
            queueGroupsReload()
        case .topics(let group):
            groupTicks[group, default: 0] += 1
            queueGroupsReload()
        case .messages(_, let topic):
            topicTicks[topic, default: 0] += 1
            queueGroupsReload()
        case .history(let topic, let hasMore):
            if !hasMore { historyEnded.insert(topic) }
            topicTicks[topic, default: 0] += 1
        case .presence:
            members = (try? client.members()) ?? members
        case .artifacts(_, let topic):
            ticks["artifacts:\(topic)", default: 0] += 1
        case .runs(_, let topic):
            ticks["runs:\(topic)", default: 0] += 1
        case .cardAnswered(let message, let ok, let reason):
            if ok { cardRefusals[message] = nil } else { cardRefusals[message] = reason ?? "" }
            ticks["card:\(message)", default: 0] += 1
        case .other:
            break
        }
    }

    private func reloadDirectory() {
        guard let client else { return }
        myName = client.myName() ?? ""
        members = (try? client.members()) ?? []
        groups = (try? client.groups()) ?? []
    }

    /// Many updates arrive together (a catch-up, a busy group): read the list once for them.
    private func queueGroupsReload() {
        guard !groupsReloadQueued else { return }
        groupsReloadQueued = true
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(80))
            groupsReloadQueued = false
            guard let client else { return }
            let next = (try? client.groups()) ?? groups
            if next != groups { groups = next }
        }
    }
}

/// chat-core calls this on a background thread.
private final class Listener: UpdateListener, @unchecked Sendable {
    weak var model: AppModel?

    init(model: AppModel) {
        self.model = model
    }

    func changed(update: Update) {
        Task { @MainActor [weak model] in model?.changed(update) }
    }
}
