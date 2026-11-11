import SwiftUI

/// Search over messages (on the server), and 话题, 产出 and 成员 (in the phone's cache).
struct SearchScreen: View {
    @EnvironmentObject private var model: AppModel
    @State private var query = ""
    @State private var scope = Scope.all
    @State private var hits: [SearchHitItem] = []
    @State private var topics: [TopicItem] = []
    @State private var artifacts: [ArtifactItem] = []
    @State private var members: [MemberItem] = []
    @State private var searching = false
    @State private var offline = false
    @State private var searched = ""

    enum Scope: Hashable { case all, messages, topics, artifacts, members }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if offline {
                    Label(Copy.searchOffline, systemImage: "wifi.slash").font(.footnote).foregroundStyle(.secondary)
                        .padding(.horizontal, 12).padding(.top, 8)
                }
                if show(.members) && !members.isEmpty {
                    SectionTitle(text: Copy.membersTitle)
                    ForEach(members.prefix(limit), id: \.id) { MemberLine(member: $0) }
                }
                if show(.topics) && !topics.isEmpty {
                    SectionTitle(text: Copy.topics)
                    ForEach(topics.prefix(limit), id: \.id) { TopicHit(topic: $0, groupName: model.group($0.groupId)?.name ?? "") }
                }
                if show(.artifacts) && !artifacts.isEmpty {
                    SectionTitle(text: Copy.artifactsTitle)
                    ForEach(artifacts.prefix(limit), id: \.id) { ArtifactRow(artifact: $0) }
                }
                if show(.messages) && !hits.isEmpty {
                    SectionTitle(text: Copy.searchMessages)
                    ForEach(hits, id: \.messageId) { MessageHit(hit: $0) }
                }
            }
            .padding(.horizontal, Theme.gutter).padding(.bottom, 16)
        }
        .background(Theme.page)
        .overlay { emptyState }
        .navigationTitle(Copy.search)
        .navigationBarTitleDisplayMode(.inline)
        .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: Copy.searchPrompt)
        .searchScopes($scope, activation: .onSearchPresentation) {
            Text(Copy.searchAll).tag(Scope.all)
            Text(Copy.searchMessages).tag(Scope.messages)
            Text(Copy.topics).tag(Scope.topics)
            Text(Copy.artifactsTitle).tag(Scope.artifacts)
            Text(Copy.membersTitle).tag(Scope.members)
        }
        .task(id: query) { await search() }
    }

    private var limit: Int { scope == .all ? 4 : 50 }

    private func show(_ s: Scope) -> Bool { scope == .all || scope == s }

    private var nothing: Bool {
        (!show(.messages) || hits.isEmpty) && (!show(.topics) || topics.isEmpty)
            && (!show(.artifacts) || artifacts.isEmpty) && (!show(.members) || members.isEmpty)
    }

    @ViewBuilder
    private var emptyState: some View {
        if query.trimmingCharacters(in: .whitespaces).isEmpty {
            ContentUnavailableView(Copy.searchHint, systemImage: "magnifyingglass")
        } else if searching && nothing {
            ProgressView(Copy.searching)
        } else if nothing && searched == query {
            ContentUnavailableView(Copy.searchNothing, systemImage: "magnifyingglass", description: Text(Copy.searchNothingFor(query)))
        }
    }

    private func search() async {
        let q = query.trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else {
            hits = []; topics = []; artifacts = []; members = []; offline = false; searched = ""
            return
        }
        // Wait for the typing to settle.
        try? await Task.sleep(for: .milliseconds(250))
        guard !Task.isCancelled else { return }
        topics = model.searchTopics(q)
        artifacts = model.searchArtifacts(q)
        let lower = q.lowercased()
        members = model.members.filter { $0.name.lowercased().contains(lower) || $0.role.lowercased().contains(lower) }
        searching = true
        let found = await model.searchMessages(q)
        guard !Task.isCancelled else { return }
        searching = false
        offline = found == nil
        hits = found ?? []
        searched = query
    }
}

private struct TopicHit: View {
    let topic: TopicItem
    let groupName: String

    var body: some View {
        NavigationLink(value: Route.chat(group: topic.groupId, topic: topic.id)) {
            HStack(spacing: 12) {
                Image(systemName: topic.statusSymbol).foregroundStyle(topic.isMyTurn ? Theme.waiting : .secondary).frame(width: 28)
                VStack(alignment: .leading, spacing: 2) {
                    Text(topic.title).font(.body.weight(.semibold)).lineLimit(1)
                    Text([groupName, topic.statusWord ?? ""].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 8)
                Text(Format.listTime(topic.lastActivityAt)).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
            }
            .padding(.horizontal, 12).frame(minHeight: 60)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowButtonStyle())
    }
}

private struct MessageHit: View {
    let hit: SearchHitItem

    var body: some View {
        NavigationLink(value: Route.chat(group: hit.groupId, topic: hit.topicId)) {
            HStack(alignment: .top, spacing: 12) {
                Avatar(name: hit.authorName, size: 36)
                VStack(alignment: .leading, spacing: 3) {
                    HStack(alignment: .firstTextBaseline) {
                        Text(hit.authorName).font(.subheadline.weight(.semibold)).lineLimit(1)
                        Spacer(minLength: 8)
                        Text(Format.listTime(hit.createdAt)).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
                    }
                    Text(snippet).font(.subheadline).lineLimit(3)
                    Text(place).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 10)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowButtonStyle())
    }

    private var place: String {
        [hit.groupName, hit.inMain ? Copy.groupChat : hit.topicTitle].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// The snippet with its matches in the accent color.
    private var snippet: AttributedString {
        // Highlights count Unicode code points; line breaks become spaces one for one.
        let chars = hit.snippet.unicodeScalars.map { CharacterSet.newlines.contains($0) ? " " : $0 }
        func text(_ part: ArraySlice<Unicode.Scalar>) -> String { String(String.UnicodeScalarView(part)) }
        var out = AttributedString()
        var at = 0
        for h in hit.highlights.sorted(by: { $0.start < $1.start }) {
            let start = min(Int(h.start), chars.count), end = min(start + Int(h.length), chars.count)
            guard start >= at else { continue }
            out += AttributedString(text(chars[at..<start]))
            var match = AttributedString(text(chars[start..<end]))
            match.foregroundColor = Theme.accent
            match.font = .subheadline.weight(.semibold)
            out += match
            at = end
        }
        out += AttributedString(text(chars[min(at, chars.count)...]))
        return out
    }
}
