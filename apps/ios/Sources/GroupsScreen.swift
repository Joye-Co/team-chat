import SwiftUI

/// The 群 tab: every 群 with its newest 话题 under it, then 私聊. A row opens its chat directly.
struct GroupsScreen: View {
    @EnvironmentObject private var model: AppModel
    @State private var path: [Route] = []

    var body: some View {
        NavigationStack(path: $path) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 20) {
                    let groups = model.groups.filter { !$0.isDm }
                    let direct = model.groups.filter { $0.isDm }
                    ForEach(groups, id: \.id) { group in
                        VStack(spacing: 2) {
                            GroupRow(group: group)
                            ForEach(group.topics, id: \.id) { TopicLine(topic: $0) }
                        }
                    }
                    if !direct.isEmpty {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(Copy.directSection).font(.footnote.weight(.semibold)).foregroundStyle(.secondary)
                                .padding(.horizontal, 12).padding(.bottom, 6)
                                .accessibilityAddTraits(.isHeader)
                            ForEach(direct, id: \.id) { GroupRow(group: $0) }
                        }
                    }
                }
                .padding(.horizontal, Theme.gutter).padding(.vertical, 8)
            }
            .background(Theme.page)
            .overlay {
                if model.groups.isEmpty {
                    ContentUnavailableView(Copy.groupsEmpty, systemImage: "bubble.left.and.bubble.right", description: Text(Copy.groupsEmptyHint))
                }
            }
            .navigationTitle(Copy.groupsTitle)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { ConnectionBadge(state: model.connection) }
                ToolbarItem(placement: .topBarTrailing) {
                    NavigationLink(value: Route.search) {
                        Image(systemName: "magnifyingglass").accessibilityLabel(Copy.search)
                    }
                }
            }
            .navigationDestination(for: Route.self) { RouteView(route: $0) }
        }
    }
}

private struct GroupRow: View {
    let group: GroupItem

    var body: some View {
        NavigationLink(value: Route.chat(group: group.id, topic: group.mainTopicId)) {
            HStack(spacing: 12) {
                Avatar(name: group.name, round: group.isDm && !group.peerIsBot, size: 44)
                VStack(alignment: .leading, spacing: 2) {
                    Text(group.name).font(.body.weight(.semibold)).lineLimit(1)
                    if !group.preview.isEmpty {
                        Text(group.preview).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
                Spacer(minLength: 8)
                VStack(alignment: .trailing, spacing: 4) {
                    Text(Format.listTime(group.lastActivityAt)).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
                    CountBadge(count: group.unread, strong: group.mentions > 0 || group.isDm)
                }
            }
            .padding(.horizontal, 12).frame(minHeight: 64)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowButtonStyle())
    }
}

/// One 话题 under its 群: the title and whose turn it is.
private struct TopicLine: View {
    let topic: TopicItem

    var body: some View {
        NavigationLink(value: Route.chat(group: topic.groupId, topic: topic.id)) {
            HStack(spacing: 10) {
                Image(systemName: topic.statusSymbol).font(.footnote).foregroundStyle(topic.isMyTurn ? Theme.waiting : .secondary)
                    .frame(width: 20)
                Text(topic.title).font(.subheadline).fontWeight(topic.unread > 0 ? .semibold : .regular).lineLimit(1)
                Spacer(minLength: 8)
                if let word = topic.statusWord {
                    Text(word).font(.footnote.weight(topic.isMyTurn ? .semibold : .regular))
                        .foregroundStyle(topic.isMyTurn ? Theme.waiting : .secondary).lineLimit(1)
                }
            }
            .padding(.leading, 68).padding(.trailing, 12).frame(minHeight: 44)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowButtonStyle())
    }
}

/// A row that answers a touch with a soft fill, like the design's pressed state.
struct RowButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(configuration.isPressed ? Theme.fill : .clear, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
            .animation(.easeOut(duration: 0.15), value: configuration.isPressed)
    }
}

/// Every 话题 of one 群.
struct TopicsScreen: View {
    @EnvironmentObject private var model: AppModel
    let groupId: String
    @State private var topics: [TopicItem] = []

    var body: some View {
        ScrollView {
            LazyVStack(spacing: 2) {
                ForEach(topics, id: \.id) { topic in
                    NavigationLink(value: Route.chat(group: topic.groupId, topic: topic.id)) {
                        HStack(spacing: 12) {
                            Image(systemName: topic.statusSymbol).foregroundStyle(topic.isMyTurn ? Theme.waiting : .secondary)
                                .frame(width: 28)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(topic.title).fontWeight(topic.unread > 0 ? .semibold : .regular).lineLimit(2)
                                HStack(spacing: 6) {
                                    if let word = topic.statusWord {
                                        Text(word).foregroundStyle(topic.isMyTurn ? Theme.waiting : .secondary)
                                    }
                                    Text(Copy.messageCount(topic.messageCount)).foregroundStyle(.secondary)
                                }
                                .font(.footnote)
                            }
                            Spacer(minLength: 8)
                            VStack(alignment: .trailing, spacing: 4) {
                                Text(Format.listTime(topic.lastActivityAt)).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
                                CountBadge(count: topic.unread, strong: topic.mentions > 0)
                            }
                        }
                        .padding(.horizontal, 12).frame(minHeight: 60)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(RowButtonStyle())
                }
            }
            .padding(.horizontal, Theme.gutter).padding(.vertical, 8)
        }
        .background(Theme.page)
        .overlay {
            if topics.isEmpty {
                ContentUnavailableView(Copy.topicsEmpty, systemImage: "number", description: Text(Copy.topicsEmptyHint))
            }
        }
        .navigationTitle(Copy.topics)
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { topics = model.topics(in: groupId) }
        .onChange(of: model.groupTicks[groupId]) { topics = model.topics(in: groupId) }
    }
}
