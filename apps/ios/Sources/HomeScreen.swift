import SwiftUI

/// 首页: what waits on me, what is being done, what got done today, and 交代一件事.
/// Owned by M2.
struct HomeScreen: View {
    var body: some View {
        NavigationStack {
            ComingSoon(title: Copy.tabHome)
                .toolbar {
                    ToolbarItem(placement: .topBarTrailing) {
                        NavigationLink(value: Route.me) { Image(systemName: "person.crop.circle").accessibilityLabel(Copy.me) }
                    }
                }
                .navigationDestination(for: Route.self) { RouteView(route: $0) }
        }
    }
}
