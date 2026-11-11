import SwiftUI

/// 成员 tab and one member's page. Owned by M5.
struct MembersScreen: View {
    var body: some View {
        NavigationStack {
            ComingSoon(title: Copy.tabMembers)
                .navigationDestination(for: Route.self) { RouteView(route: $0) }
        }
    }
}

struct MemberScreen: View {
    let memberId: String

    var body: some View { ComingSoon(title: Copy.tabMembers) }
}
