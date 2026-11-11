import SwiftUI

struct SignInScreen: View {
    @EnvironmentObject private var model: AppModel
    @State private var server = ""
    @State private var token = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(Copy.signInTitle).font(.largeTitle.bold())
            Text(Copy.signInDevHint).font(.subheadline).foregroundStyle(.secondary)
            TextField(Copy.signInServer, text: $server)
                .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                .padding(14).background(Theme.fill, in: RoundedRectangle(cornerRadius: 16))
            SecureField(Copy.signInCredential, text: $token)
                .padding(14).background(Theme.fill, in: RoundedRectangle(cornerRadius: 16))
            if let error = model.signInError {
                Label(error, systemImage: "exclamationmark.circle").font(.footnote)
            }
            Button {
                model.signIn(server: server, token: token)
            } label: {
                Text(Copy.signInContinue).font(.headline).frame(maxWidth: .infinity, minHeight: 50)
            }
            .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
            .disabled(server.isEmpty || token.isEmpty)
            Spacer()
        }
        .padding(24)
    }
}

struct GroupsScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            List {
                section(Copy.groupsSection, model.groups.filter { !$0.isDm })
                section(Copy.directSection, model.groups.filter { $0.isDm })
            }
            .listStyle(.insetGrouped)
            .navigationTitle(Copy.groupsTitle)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { ConnectionBadge(state: model.connection) }
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button(Copy.signOut, systemImage: "rectangle.portrait.and.arrow.right", role: .destructive) {
                            model.signOut()
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle")
                    }
                }
            }
            .navigationDestination(for: GroupItem.self) { TopicsScreen(group: $0) }
        }
    }

    @ViewBuilder
    private func section(_ title: String, _ items: [GroupItem]) -> some View {
        if !items.isEmpty {
            Section(title) {
                ForEach(items, id: \.id) { group in
                    NavigationLink(value: group) {
                        HStack(spacing: 12) {
                            Avatar(name: group.name, round: group.isDm)
                            Text(group.name).fontWeight(group.unread > 0 ? .semibold : .regular)
                            Spacer()
                            if group.unread > 0 {
                                Text("\(group.unread)").font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }
        }
    }
}

struct TopicsScreen: View {
    @EnvironmentObject private var model: AppModel
    let group: GroupItem

    var body: some View {
        let topics = model.topics(in: group.id)
        List(topics, id: \.id) { topic in
            NavigationLink(value: topic) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(topic.title.isEmpty ? Copy.groupChat : topic.title)
                        .fontWeight(topic.unread > 0 ? .semibold : .regular).lineLimit(1)
                    HStack(spacing: 6) {
                        if let word = statusWord(topic.status) {
                            Text(word).foregroundStyle(topic.status.hasPrefix("waiting") ? Theme.waiting : .secondary)
                        }
                        Text(Copy.messageCount(topic.messageCount)).foregroundStyle(.secondary)
                    }
                    .font(.footnote)
                }
            }
        }
        .listStyle(.insetGrouped)
        .overlay { if topics.isEmpty { ContentUnavailableView(Copy.topicsEmpty, systemImage: "bubble.left") } }
        .navigationTitle(group.name)
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(for: TopicItem.self) { MessagesScreen(topic: $0) }
        .id(model.revision)
    }

    private func statusWord(_ status: String) -> String? {
        switch status {
        case "in_progress": Copy.statusInProgress
        case "waiting_answer": Copy.statusWaitingAnswer
        case "waiting_review": Copy.statusWaitingReview
        case "blocked": Copy.statusBlocked
        case "done": Copy.statusDone
        default: nil
        }
    }
}

struct MessagesScreen: View {
    @EnvironmentObject private var model: AppModel
    let topic: TopicItem
    @State private var draft = ""

    var body: some View {
        let messages = model.messages(in: topic.id)
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 18) {
                    ForEach(messages, id: \.id) { message in
                        HStack(alignment: .top, spacing: 10) {
                            Avatar(name: message.authorName, round: !message.authorIsBot, mine: message.isMine)
                            VStack(alignment: .leading, spacing: 2) {
                                HStack(alignment: .firstTextBaseline, spacing: 6) {
                                    Text(message.authorName).font(.subheadline.weight(.semibold))
                                    Text(time(message.createdAt)).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
                                }
                                Text(message.body).textSelection(.enabled)
                            }
                        }
                        .id(message.id)
                    }
                }
                .padding(16)
            }
            .onChange(of: messages.last?.id) { _, last in
                if let last { withAnimation { proxy.scrollTo(last, anchor: .bottom) } }
            }
            .onAppear { if let last = messages.last?.id { proxy.scrollTo(last, anchor: .bottom) } }
        }
        .safeAreaInset(edge: .bottom) { composer }
        .navigationTitle(topic.title.isEmpty ? Copy.groupChat : topic.title)
        .navigationBarTitleDisplayMode(.inline)
    }

    private var composer: some View {
        HStack(spacing: 8) {
            TextField(Copy.composerPlaceholder, text: $draft, axis: .vertical).lineLimit(1...5)
            Button {
                let body = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !body.isEmpty else { return }
                model.send(body, group: topic.groupId, topic: topic.id)
                draft = ""
            } label: {
                Image(systemName: "arrow.up").font(.headline).frame(width: 36, height: 36)
                    .background(Theme.accent, in: Circle()).foregroundStyle(.white)
            }
            .accessibilityLabel(Copy.send)
            .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
        .padding(.leading, 18).padding(.trailing, 6).padding(.vertical, 6)
        .background(Theme.fill, in: RoundedRectangle(cornerRadius: 24))
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(.background)
    }

    private func time(_ ms: UInt64) -> String {
        Date(timeIntervalSince1970: Double(ms) / 1000).formatted(date: .omitted, time: .shortened)
    }
}

struct Avatar: View {
    let name: String
    var round = true
    var mine = false

    var body: some View {
        Text(String(name.prefix(1))).font(.subheadline.weight(.semibold))
            .frame(width: 36, height: 36)
            .foregroundStyle(mine ? .white : .primary)
            .background(mine ? Theme.accent : Theme.fill, in: RoundedRectangle(cornerRadius: round ? 18 : 10))
            .accessibilityHidden(true)
    }
}

/// The connection state, shown only when it is not normal.
struct ConnectionBadge: View {
    let state: String

    var body: some View {
        let shown: (word: String, symbol: String)? =
            switch state {
            case "connected", "stopped": nil
            case "connecting": (Copy.connecting, "arrow.triangle.2.circlepath")
            case "unauthorized": (Copy.signInAgain, "exclamationmark.circle")
            case "unsupported": (Copy.updateApp, "exclamationmark.circle")
            default: (Copy.offline, "wifi.slash")
            }
        if let shown {
            Label(shown.word, systemImage: shown.symbol)
                .labelStyle(.titleAndIcon).font(.footnote).foregroundStyle(.secondary)
        }
    }
}
