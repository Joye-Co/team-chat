import SwiftUI

/// The four tabs of the design: 首页, 群, 产出, 成员. Each tab keeps its own navigation stack.
struct RootTabs: View {
    @State private var tab = Tab.groups
    @ObservedObject private var push = PushCenter.shared

    enum Tab: Hashable { case home, groups, artifacts, members }

    var body: some View {
        TabView(selection: $tab) {
            HomeScreen().tabItem { Label(Copy.tabHome, systemImage: "house") }.tag(Tab.home)
            GroupsScreen().tabItem { Label(Copy.tabGroups, systemImage: "bubble.left.and.bubble.right") }.tag(Tab.groups)
            ArtifactsScreen().tabItem { Label(Copy.tabArtifacts, systemImage: "square.stack") }.tag(Tab.artifacts)
            MembersScreen().tabItem { Label(Copy.tabMembers, systemImage: "person.2") }.tag(Tab.members)
        }
        // A tapped notification opens its chat in the 群 tab.
        .onChange(of: push.route) { _, route in if route != nil { tab = .groups } }
    }
}

/// A screen that a later part of the plan fills in.
struct ComingSoon: View {
    let title: String

    var body: some View {
        ContentUnavailableView(title, systemImage: "hammer", description: Text(Copy.comingSoon))
            .navigationTitle(title)
    }
}
