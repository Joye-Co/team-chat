import Foundation
import SwiftUI

/// Owns the `ChatClient` and tells the views when to read the cache again.
@MainActor
final class AppModel: ObservableObject {
    @Published private(set) var signedIn = false
    @Published private(set) var connection = "stopped"
    @Published private(set) var myName = ""
    @Published private(set) var groups: [GroupItem] = []
    /// Bumped on every data change; views that show topics or messages re-read on it.
    @Published private(set) var revision = 0
    @Published var signInError: String?

    private var client: ChatClient?

    init() {
        // Development sign-in: the sandbox passes the server and a token through the
        // environment (`simctl launch` with SIMCTL_CHILD_*). The real sign-in screens
        // (email code, password, scanning the computer's code) come after this slice.
        let env = ProcessInfo.processInfo.environment
        if let url = env["TC_SERVER_URL"], let token = env["TC_TOKEN"], !url.isEmpty, !token.isEmpty {
            signIn(server: url, token: token)
        }
    }

    func signIn(server: String, token: String) {
        do {
            let dir = try FileManager.default
                .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("core", isDirectory: true)
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let client = try ChatClient(baseUrl: server, token: token, dataDir: dir.path)
            client.listen(listener: Listener(model: self))
            client.start()
            self.client = client
            signedIn = true
            signInError = nil
            reload()
        } catch {
            signInError = error.localizedDescription
        }
    }

    func cameToForeground() {
        client?.reconnectNow()
    }

    func topics(in group: String) -> [TopicItem] {
        (try? client?.topics(groupId: group, limit: 100)) ?? []
    }

    func messages(in topic: String) -> [MessageItem] {
        (try? client?.messages(topicId: topic, limit: 200)) ?? []
    }

    func send(_ body: String, group: String, topic: String) {
        _ = try? client?.sendMessage(groupId: group, topicId: topic, body: body)
    }

    fileprivate func changed(_ kind: UpdateKind) {
        reload()
    }

    private func reload() {
        guard let client else { return }
        connection = client.connectionState()
        myName = client.myName() ?? ""
        groups = (try? client.groups()) ?? []
        revision += 1
    }
}

/// chat-core calls this on a background thread.
private final class Listener: UpdateListener, @unchecked Sendable {
    weak var model: AppModel?

    init(model: AppModel) {
        self.model = model
    }

    func changed(kind: UpdateKind) {
        Task { @MainActor [weak model] in model?.changed(kind) }
    }
}
