import SwiftUI

/// 我: my name, signed-in devices, password, notifications, sign out. Owned by M4 (M3 adds the
/// notification settings row).
struct MeScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        List {
            Section { Text(model.myName).font(.headline) }
            Section { NavigationLink(value: Route.notifySettings) { Label(Copy.notifyTitle, systemImage: "bell") } }
            Section {
                Button(Copy.signOut, role: .destructive) { model.signOut() }
            }
        }
        .navigationTitle(Copy.me)
        .navigationBarTitleDisplayMode(.inline)
    }
}
