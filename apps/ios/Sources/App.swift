import SwiftUI

/// The iPhone client. Everything below the views is chat-core, reached through the generated
/// `ChatClient`.
@main
struct MobileApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var model = AppModel()
    @ObservedObject private var push = PushCenter.shared
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            Group {
                if model.signedIn {
                    RootTabs()
                } else {
                    SignInFlow()
                }
            }
            .environmentObject(model)
            .tint(Theme.accent)
            .sheet(isPresented: Binding(get: { push.asking && model.signedIn }, set: { if !$0 { push.putOff() } })) {
                AllowNotifyScreen()
            }
            .onChange(of: model.groups) { _, _ in model.offerNotificationsIfUseful() }
        }
        .onChange(of: phase) { _, now in
            // The system drops the connection in the background: reconnect and catch up.
            if now == .active {
                model.cameToForeground()
                if model.signedIn { model.notifyCameToForeground() }
            }
        }
    }
}

/// Colors and sizes of the design (the mobile design's System board). Text styles are the
/// system's, so Dynamic Type works.
enum Theme {
    static let accent = Color(red: 0x40 / 255, green: 0x9C / 255, blue: 1)
    static let onAccent = Color.white
    /// Rows, fields and bubbles on the white page.
    static let fill = Color(uiColor: UIColor { $0.userInterfaceStyle == .dark ? UIColor(white: 0.11, alpha: 1) : UIColor(red: 0.953, green: 0.953, blue: 0.957, alpha: 1) })
    static let page = Color(uiColor: UIColor { $0.userInterfaceStyle == .dark ? .black : .white })
    /// 等你: amber text on a soft amber fill.
    static let waiting = Color(uiColor: UIColor { $0.userInterfaceStyle == .dark ? UIColor(red: 1, green: 0.62, blue: 0.26, alpha: 1) : UIColor(red: 0.71, green: 0.28, blue: 0.03, alpha: 1) })
    static let bad = Color(uiColor: UIColor { $0.userInterfaceStyle == .dark ? UIColor(red: 1, green: 0.27, blue: 0.23, alpha: 1) : UIColor(red: 0.84, green: 0, blue: 0.08, alpha: 1) })

    static let rowRadius: CGFloat = 16
    static let gutter: CGFloat = 16

    /// A stable tint per member, from the design's avatar colors.
    static func memberTint(_ key: String) -> Color {
        let tints: [(Double, Double, Double)] = [
            (0.86, 0.90, 1.00), (0.85, 0.95, 0.89), (1.00, 0.91, 0.84),
            (0.93, 0.88, 1.00), (1.00, 0.88, 0.91), (0.84, 0.95, 0.97),
        ]
        let index = key.unicodeScalars.reduce(0) { ($0 &* 31 &+ Int($1.value)) % 9973 } % tints.count
        let t = tints[index]
        return Color(red: t.0, green: t.1, blue: t.2)
    }

    static let memberInk = Color(red: 0.18, green: 0.20, blue: 0.45)
}
