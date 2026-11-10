import SwiftUI

struct SignInScreen: View {
    @EnvironmentObject private var model: AppModel
    @State private var server = ""
    @State private var token = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("登录").font(.largeTitle.bold())
            Text("这一版只有开发用的登录：填服务器地址和登录凭证。").font(.subheadline).foregroundStyle(.secondary)
            TextField("服务器地址", text: $server)
                .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                .padding(14).background(Theme.fill, in: RoundedRectangle(cornerRadius: 16))
            SecureField("登录凭证", text: $token)
                .padding(14).background(Theme.fill, in: RoundedRectangle(cornerRadius: 16))
            if let error = model.signInError {
                Label(error, systemImage: "exclamationmark.circle").font(.footnote)
            }
            Button {
                model.signIn(server: server, token: token)
            } label: {
                Text("继续").font(.headline).frame(maxWidth: .infinity, minHeight: 50)
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
                section("群", model.groups.filter { !$0.isDm })
                section("私聊", model.groups.filter { $0.isDm })
            }
            .listStyle(.insetGrouped)
            .navigationTitle("群")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { ConnectionBadge(state: model.connection) }
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
                    Text(topic.title.isEmpty ? "群聊" : topic.title)
                        .fontWeight(topic.unread > 0 ? .semibold : .regular).lineLimit(1)
                    HStack(spacing: 6) {
                        if let word = statusWord(topic.status) {
                            Text(word).foregroundStyle(topic.status.hasPrefix("waiting") ? Theme.waiting : .secondary)
                        }
                        Text("\(topic.messageCount) 条消息").foregroundStyle(.secondary)
                    }
                    .font(.footnote)
                }
            }
        }
        .listStyle(.insetGrouped)
        .overlay { if topics.isEmpty { ContentUnavailableView("还没有话题", systemImage: "bubble.left") } }
        .navigationTitle(group.name)
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(for: TopicItem.self) { MessagesScreen(topic: $0) }
        .id(model.revision)
    }

    private func statusWord(_ status: String) -> String? {
        switch status {
        case "in_progress": "进行中"
        case "waiting_answer": "等回答"
        case "waiting_review": "等审"
        case "blocked": "卡住了"
        case "done": "完成"
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
        .navigationTitle(topic.title.isEmpty ? "群聊" : topic.title)
        .navigationBarTitleDisplayMode(.inline)
    }

    private var composer: some View {
        HStack(spacing: 8) {
            TextField("回复，或 @ 成员交代", text: $draft, axis: .vertical).lineLimit(1...5)
            Button {
                let body = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !body.isEmpty else { return }
                model.send(body, group: topic.groupId, topic: topic.id)
                draft = ""
            } label: {
                Image(systemName: "arrow.up").font(.headline).frame(width: 36, height: 36)
                    .background(Theme.accent, in: Circle()).foregroundStyle(.white)
            }
            .accessibilityLabel("发送")
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

struct ConnectionBadge: View {
    let state: String

    var body: some View {
        let (word, symbol): (String, String) =
            switch state {
            case "connected": ("在线", "checkmark.circle")
            case "connecting": ("正在连接", "arrow.triangle.2.circlepath")
            case "unauthorized": ("要重新登录", "exclamationmark.circle")
            case "unsupported": ("要更新应用", "exclamationmark.circle")
            default: ("离线", "wifi.slash")
            }
        Label(word, systemImage: symbol).labelStyle(.titleAndIcon).font(.footnote).foregroundStyle(.secondary)
    }
}
