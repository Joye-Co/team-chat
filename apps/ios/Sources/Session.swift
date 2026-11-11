import Foundation

/// Where the app remembers a sign-in: the server address in the defaults, the token in the
/// Keychain, the cache in a folder. When the build is signed with an app group (Info.plist key
/// `AppGroup`), all three live in the group so the app's extensions can read them.
enum Session {
    private static let serverKey = "server"

    static var appGroup: String? { info("AppGroup") }

    /// The server this build signs in to: the sandbox passes one through the environment, a
    /// release build carries the operator's (Info.plist key `DefaultServer`). `nil` asks the
    /// user for it.
    static var defaultServer: String? {
        let env = ProcessInfo.processInfo.environment["TC_SERVER_URL"]
        return env?.isEmpty == false ? env : info("DefaultServer")
    }

    private static func info(_ key: String) -> String? {
        let value = Bundle.main.object(forInfoDictionaryKey: key) as? String
        return value?.isEmpty == false ? value : nil
    }

    /// The Keychain service: the app's bundle id, also inside an extension.
    private static var service: String {
        info("AppBundleId") ?? Bundle.main.bundleIdentifier ?? ""
    }

    private static var defaults: UserDefaults {
        appGroup.flatMap { UserDefaults(suiteName: $0) } ?? .standard
    }

    /// The folder of chat-core's cache.
    static func dataDir() throws -> URL {
        let files = FileManager.default
        let base =
            try appGroup.flatMap { files.containerURL(forSecurityApplicationGroupIdentifier: $0) }
            ?? files.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let dir = base.appendingPathComponent("core", isDirectory: true)
        try files.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    static func saved() -> (server: String, token: String)? {
        guard let server = defaults.string(forKey: serverKey), !server.isEmpty,
            let token = try? loadToken(service: service, account: server, accessGroup: appGroup)
        else { return nil }
        return (server, token)
    }

    static func save(server: String, token: String) throws {
        try saveToken(service: service, account: server, accessGroup: appGroup, token: token)
        defaults.set(server, forKey: serverKey)
    }

    /// Forgets the sign-in and deletes the cache.
    static func clear() {
        if let server = defaults.string(forKey: serverKey) {
            try? clearToken(service: service, account: server, accessGroup: appGroup)
        }
        defaults.removeObject(forKey: serverKey)
        if let dir = try? dataDir() { try? FileManager.default.removeItem(at: dir) }
    }
}
