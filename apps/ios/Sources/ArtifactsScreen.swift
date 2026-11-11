import SwiftUI

/// 产出 tab and one 产出. Owned by M5.
struct ArtifactsScreen: View {
    var body: some View {
        NavigationStack {
            ComingSoon(title: Copy.tabArtifacts)
                .navigationDestination(for: Route.self) { RouteView(route: $0) }
        }
    }
}

struct ArtifactScreen: View {
    let artifactId: String

    var body: some View { ComingSoon(title: Copy.tabArtifacts) }
}
