import SwiftUI

/// The first slice of the iPhone client: sign in, list 群 and 话题, read and send messages.
/// Everything below the views is chat-core, reached through the generated `ChatClient`.
@main
struct MobileApp: App {
    @StateObject private var model = AppModel()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            Group {
                if model.signedIn {
                    GroupsScreen()
                } else {
                    SignInScreen()
                }
            }
            .environmentObject(model)
            .tint(Theme.accent)
        }
        .onChange(of: phase) { _, now in
            // The system drops the connection in the background: reconnect and catch up.
            if now == .active { model.cameToForeground() }
        }
    }
}

enum Theme {
    static let accent = Color(red: 0x40 / 255, green: 0x9C / 255, blue: 1)
    static let fill = Color(uiColor: .secondarySystemBackground)
    static let waiting = Color(red: 0xB5 / 255, green: 0x47 / 255, blue: 0x08 / 255)
}
