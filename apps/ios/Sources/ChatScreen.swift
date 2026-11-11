import QuickLook
import SwiftUI

/// One chat: a group's own chat, a 私聊 or a 话题.
struct ChatScreen: View {
    @EnvironmentObject private var model: AppModel
    let groupId: String
    let topicId: String

    @State private var messages: [MessageItem] = []
    @State private var pending: [PendingItem] = []
    @State private var topic: TopicItem?
    @State private var limit = 100
    @State private var loadingOlder = false
    /// Where I had read to when the chat opened, when there was something new after it.
    @State private var unreadAfter: UInt64?
    @State private var opened = false
    @State private var draft = ""
    @State private var quote: MessageItem?
    @State private var mentioned: [MemberItem] = []
    @State private var preview: URL?
    @FocusState private var composing: Bool

    private var group: GroupItem? { model.group(groupId) }
    private var isMain: Bool { topicId == group?.mainTopicId }
    private var title: String { isMain ? (group?.name ?? "") : (topic?.title ?? "") }
    private var canLoadOlder: Bool {
        messages.count >= limit || !((topic?.historyComplete ?? true) || model.historyEnded.contains(topicId))
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if canLoadOlder && !messages.isEmpty { olderButton }
                    ForEach(Array(messages.enumerated()), id: \.element.id) { index, message in
                        let previous = index > 0 ? messages[index - 1] : nil
                        if previous == nil || !Format.sameDay(previous!.createdAt, message.createdAt) {
                            Divider(text: Format.day(message.createdAt))
                        }
                        if let unreadAfter, message.seq > unreadAfter, (previous?.seq ?? 0) <= unreadAfter, !message.isMine {
                            Divider(text: Copy.newMessages, tint: Theme.accent)
                        }
                        MessageRow(
                            message: message,
                            compact: follows(previous, message),
                            text: rendered(message),
                            quote: { startQuote(message) },
                            react: { emoji, on in
                                model.react(message.id, emoji: emoji, on: on)
                                reload()
                            },
                            mention: { mention(message) },
                            open: { file in Task { await open(file) } }
                        )
                        .id(message.id)
                    }
                    ForEach(pending, id: \.id) { item in
                        PendingRow(item: item, retry: { model.retry(item.id) }, discard: { model.discard(item.id) })
                            .id(item.id)
                    }
                    Color.clear.frame(height: 8).id(bottomId)
                }
                .padding(.horizontal, Theme.gutter)
            }
            .defaultScrollAnchor(.bottom)
            .scrollDismissesKeyboard(.interactively)
            .background(Theme.page)
            .overlay {
                if opened && messages.isEmpty && pending.isEmpty {
                    ContentUnavailableView(Copy.chatEmpty, systemImage: "bubble.left", description: Text(Copy.chatEmptyHint))
                }
            }
            .onChange(of: messages.last?.id) { withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(bottomId, anchor: .bottom) } }
            .onChange(of: pending.count) { withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(bottomId, anchor: .bottom) } }
            .onChange(of: composing) { if composing { proxy.scrollTo(bottomId, anchor: .bottom) } }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) { dock }
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) {
                VStack(spacing: 0) {
                    Text(title).font(.headline).lineLimit(1)
                    if let subtitle {
                        Text(subtitle).font(.caption).lineLimit(1)
                            .foregroundStyle(!isMain && topic?.isMyTurn == true ? AnyShapeStyle(Theme.waiting) : AnyShapeStyle(.secondary))
                    }
                }
            }
            if isMain, group?.isDm == false {
                ToolbarItem(placement: .topBarTrailing) {
                    NavigationLink(value: Route.topics(group: groupId)) {
                        Image(systemName: "number").accessibilityLabel(Copy.topics)
                    }
                }
            }
        }
        .quickLookPreview($preview)
        .onAppear {
            model.watch(topic: topicId)
            reload()
            opened = true
            // A short chat whose older messages were never fetched: fetch them now, so the
            // "older messages" button shows only when there is something to load.
            if messages.count < 30, topic?.historyComplete == false { model.loadOlder(topicId) }
        }
        .onChange(of: model.topicTicks[topicId]) { reload() }
    }

    private let bottomId = "bottom"

    private var subtitle: String? {
        if isMain {
            guard let group, !group.isDm else { return nil }
            return Copy.memberCount(group.memberCount)
        }
        return topic?.statusWord
    }

    // MARK: Reading

    private func reload() {
        let fresh = model.topic(topicId, in: groupId)
        if !opened, let fresh, fresh.unread > 0 { unreadAfter = fresh.readSeq }
        topic = fresh
        messages = model.messages(in: topicId, limit: limit)
        pending = model.pending(in: topicId)
        loadingOlder = false
        if let last = messages.last, last.seq > (fresh?.readSeq ?? 0) {
            model.markRead(group: groupId, topic: topicId, seq: last.seq)
        }
    }

    private var olderButton: some View {
        Button {
            limit += 100
            if messages.count < limit - 100 || !(topic?.historyComplete ?? true) {
                loadingOlder = true
                model.loadOlder(topicId)
            }
            reload()
        } label: {
            Text(loadingOlder ? Copy.loadingOlder : Copy.loadOlder).font(.footnote)
                .frame(maxWidth: .infinity, minHeight: 44)
        }
        .disabled(loadingOlder)
    }

    /// The same member again within a few minutes: no avatar and name the second time.
    private func follows(_ previous: MessageItem?, _ message: MessageItem) -> Bool {
        guard let previous, previous.kind != "system", message.kind != "system" else { return false }
        return previous.authorId == message.authorId
            && message.createdAt - min(previous.createdAt, message.createdAt) < 5 * 60 * 1000
            && Format.sameDay(previous.createdAt, message.createdAt)
            && message.quote == nil
    }

    /// The body as inline Markdown, with the mentions tinted.
    private func rendered(_ message: MessageItem) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        var text = (try? AttributedString(markdown: message.body, options: options)) ?? AttributedString(message.body)
        for id in message.mentions {
            guard let name = model.member(id)?.name else { continue }
            var rest = text.startIndex..<text.endIndex
            while let found = text[rest].range(of: "@\(name)") {
                text[found].foregroundColor = Theme.accent
                rest = found.upperBound..<text.endIndex
            }
        }
        return text
    }

    private func open(_ file: FileItem) async {
        if let path = await model.filePath(file) { preview = URL(fileURLWithPath: path) }
    }

    // MARK: Writing

    private func startQuote(_ message: MessageItem) {
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        quote = message
        composing = true
    }

    private func mention(_ message: MessageItem) {
        guard !message.isMine, let member = model.member(message.authorId) else { return }
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        insert(member, replacing: nil)
        composing = true
    }

    private func insert(_ member: MemberItem, replacing query: String?) {
        if let query { draft.removeLast(query.count + 1) }
        if let last = draft.last, !last.isWhitespace { draft += " " }
        draft += "@\(member.name) "
        if !mentioned.contains(where: { $0.id == member.id }) { mentioned.append(member) }
    }

    /// What is being typed after the last `@`, when the caret is still in that name.
    private var mentionQuery: String? {
        guard let at = draft.lastIndex(of: "@") else { return nil }
        let query = String(draft[draft.index(after: at)...])
        return query.count <= 12 && !query.contains(where: \.isWhitespace) ? query : nil
    }

    private var suggestions: [MemberItem] {
        guard let query = mentionQuery else { return [] }
        return model.members
            .filter { !$0.isMe && (query.isEmpty || $0.name.localizedCaseInsensitiveContains(query)) }
            .prefix(6).map { $0 }
    }

    private var canSend: Bool { !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    private func send() {
        let body = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty else { return }
        let ids = mentioned.filter { body.contains("@\($0.name)") }.map(\.id)
        model.send(body, group: groupId, topic: topicId, mentions: ids, quote: quote?.id)
        draft = ""
        quote = nil
        mentioned = []
        reload()
    }

    private var dock: some View {
        VStack(spacing: 0) {
            let people = suggestions
            if !people.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(people, id: \.id) { member in
                            Button { insert(member, replacing: mentionQuery) } label: {
                                HStack(spacing: 6) {
                                    Avatar(name: member.name, round: !member.isBot, size: 24)
                                    Text(member.name).font(.subheadline)
                                }
                                .padding(.leading, 6).padding(.trailing, 12).frame(minHeight: 36)
                                .background(Theme.fill, in: Capsule())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 12).padding(.vertical, 6)
                }
                .transition(.move(edge: .bottom).combined(with: .opacity))
            }
            if ConnectionBadge.words(model.connection) != nil, model.connection != "connecting" {
                Label(Copy.offlineBanner, systemImage: "wifi.slash").font(.footnote).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 18).padding(.top, 6)
            }
            if let quote {
                HStack(spacing: 10) {
                    RoundedRectangle(cornerRadius: 1.5).fill(Theme.accent).frame(width: 3, height: 32)
                    VStack(alignment: .leading, spacing: 0) {
                        Text(Copy.quoting(quote.authorName)).font(.caption.weight(.semibold)).foregroundStyle(Theme.accent)
                        Text(Format.oneLine(quote.body)).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer(minLength: 8)
                    Button { self.quote = nil } label: {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(.tertiary).frame(width: 44, height: 44)
                    }
                    .accessibilityLabel(Copy.cancelQuote)
                }
                .padding(.leading, 18).padding(.trailing, 4)
                .transition(.move(edge: .bottom).combined(with: .opacity))
            }
            HStack(alignment: .bottom, spacing: 8) {
                TextField(Copy.composerPlaceholder, text: $draft, axis: .vertical).lineLimit(1...6)
                    .focused($composing).padding(.vertical, 8)
                Button(action: send) {
                    Image(systemName: "arrow.up").font(.headline).frame(width: 36, height: 36)
                        .background(Theme.accent, in: Circle()).foregroundStyle(Theme.onAccent)
                }
                .accessibilityLabel(Copy.send)
                .disabled(!canSend)
                .opacity(canSend ? 1 : 0.35)
                .animation(.easeOut(duration: 0.15), value: canSend)
            }
            .padding(.leading, 18).padding(.trailing, 6).padding(.vertical, 4)
            .background(Theme.fill, in: RoundedRectangle(cornerRadius: 24))
            .padding(.horizontal, 12).padding(.vertical, 8)
        }
        .background(Theme.page)
        .animation(.easeOut(duration: 0.2), value: quote?.id)
        .animation(.easeOut(duration: 0.2), value: suggestions.map(\.id))
    }
}

/// A line of small text between messages: a day, or where the new messages start.
private struct Divider: View {
    let text: String
    var tint: Color = .secondary

    var body: some View {
        Text(text).font(.caption.weight(.medium)).foregroundStyle(tint)
            .frame(maxWidth: .infinity).padding(.top, 16).padding(.bottom, 4)
            .accessibilityAddTraits(.isHeader)
    }
}

private let quickReactions = ["👍", "❤️", "😂", "🎉", "👀", "✅"]

private struct MessageRow: View {
    let message: MessageItem
    let compact: Bool
    let text: AttributedString
    let quote: () -> Void
    let react: (String, Bool) -> Void
    let mention: () -> Void
    let open: (FileItem) -> Void
    @State private var drag: CGFloat = 0

    var body: some View {
        if message.kind == "system" {
            Text(message.body).font(.footnote).foregroundStyle(.secondary).multilineTextAlignment(.center)
                .frame(maxWidth: .infinity).padding(.vertical, 8)
        } else {
            row
                .offset(x: drag)
                .overlay(alignment: .trailing) {
                    Image(systemName: "arrowshape.turn.up.left.fill").font(.footnote).foregroundStyle(Theme.accent)
                        .opacity(min(1, -drag / 56)).scaleEffect(drag < -56 ? 1.15 : 0.9)
                        .animation(.spring(duration: 0.2), value: drag < -56)
                        .accessibilityHidden(true)
                }
                .gesture(swipe)
                .accessibilityAction(named: Copy.quote, quote)
        }
    }

    private var row: some View {
        HStack(alignment: .top, spacing: 10) {
            if compact {
                Color.clear.frame(width: 36, height: 1)
            } else {
                Avatar(name: message.authorName, round: !message.authorIsBot, size: 36)
                    .onLongPressGesture(perform: mention)
            }
            VStack(alignment: .leading, spacing: 4) {
                if !compact {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(message.authorName).font(.subheadline.weight(.semibold))
                        Text(meta).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
                    }
                }
                if let quoted = message.quote { QuoteBlock(quote: quoted) }
                content.contextMenu { menu }
                ForEach(message.files, id: \.fileId) { file in
                    if file.isImage { FileImage(file: file, open: { open(file) }) } else { FileChip(file: file, open: { open(file) }) }
                }
                if !message.reactions.isEmpty { reactions }
            }
            Spacer(minLength: 0)
        }
        .padding(.top, compact ? 2 : 12)
        .contentShape(Rectangle())
    }

    private var meta: String {
        [message.authorRole, Format.time(message.createdAt), message.edited ? Copy.edited : ""]
            .filter { !$0.isEmpty }.joined(separator: " · ")
    }

    @ViewBuilder
    private var content: some View {
        if message.deleted {
            Text(Copy.deleted).italic().foregroundStyle(.secondary)
        } else if !message.body.isEmpty && message.cardJson == nil {
            Text(text).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
        } else if let json = message.cardJson {
            CardView(message: message, json: json)
        } else if message.streaming {
            Label(Copy.writing, systemImage: "ellipsis").font(.subheadline).foregroundStyle(.secondary)
                .symbolEffect(.variableColor.iterative, options: .repeating)
        }
    }

    @ViewBuilder
    private var menu: some View {
        ControlGroup {
            ForEach(quickReactions, id: \.self) { emoji in
                Button(emoji) { toggle(emoji) }
            }
        }
        .controlGroupStyle(.compactMenu)
        Button(Copy.quote, systemImage: "arrowshape.turn.up.left", action: quote)
        if !message.body.isEmpty {
            Button(Copy.copyText, systemImage: "doc.on.doc") { UIPasteboard.general.string = message.body }
        }
    }

    private var reactions: some View {
        HStack(spacing: 6) {
            ForEach(message.reactions, id: \.emoji) { reaction in
                Button { react(reaction.emoji, !reaction.mine) } label: {
                    Text("\(reaction.emoji) \(reaction.count)").font(.footnote.monospacedDigit())
                        .padding(.horizontal, 10).frame(minHeight: 30)
                        .foregroundStyle(reaction.mine ? Theme.accent : .primary)
                        .background(reaction.mine ? Theme.accent.opacity(0.14) : Theme.fill, in: Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.reactionLabel(names(reaction), reaction.emoji))
            }
            Menu {
                ForEach(quickReactions, id: \.self) { emoji in Button(emoji) { toggle(emoji) } }
            } label: {
                Image(systemName: "face.smiling").font(.footnote).foregroundStyle(.secondary)
                    .padding(.horizontal, 10).frame(minHeight: 30).background(Theme.fill, in: Capsule())
            }
            .accessibilityLabel(Copy.addReaction)
        }
        .padding(.top, 2)
    }

    private func names(_ reaction: ReactionItem) -> String {
        var names = reaction.names.filter { !$0.isEmpty }
        if reaction.mine && names.isEmpty { names = [Copy.you] }
        return names.joined(separator: Copy.nameSeparator)
    }

    private func toggle(_ emoji: String) {
        let mine = message.reactions.first { $0.emoji == emoji }?.mine ?? false
        react(emoji, !mine)
    }

    /// Swipe a message to the left to quote it.
    private var swipe: some Gesture {
        DragGesture(minimumDistance: 24)
            .onChanged { value in
                guard abs(value.translation.width) > abs(value.translation.height) * 2 else { return }
                drag = max(-80, min(0, value.translation.width * 0.7))
            }
            .onEnded { _ in
                if drag < -56 { quote() }
                withAnimation(.spring(duration: 0.3)) { drag = 0 }
            }
    }
}

private struct QuoteBlock: View {
    let quote: QuoteItem

    var body: some View {
        HStack(spacing: 8) {
            RoundedRectangle(cornerRadius: 1.5).fill(.tertiary).frame(width: 3)
            VStack(alignment: .leading, spacing: 0) {
                Text(quote.authorName).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                Text(Format.oneLine(quote.excerpt)).font(.footnote).foregroundStyle(.secondary).lineLimit(2)
            }
        }
        .fixedSize(horizontal: false, vertical: true)
    }
}

private struct FileImage: View {
    @EnvironmentObject private var model: AppModel
    let file: FileItem
    let open: () -> Void
    @State private var image: UIImage?
    @State private var failed = false

    private var size: CGSize {
        let width = min(240, CGFloat(file.width > 0 ? file.width : 240))
        let ratio = file.width > 0 && file.height > 0 ? CGFloat(file.height) / CGFloat(file.width) : 0.75
        return CGSize(width: width, height: min(320, width * ratio))
    }

    var body: some View {
        Button(action: open) {
            ZStack {
                Theme.fill
                if let image {
                    Image(uiImage: image).resizable().scaledToFill().transition(.opacity)
                } else if failed {
                    Label(Copy.imageFailed, systemImage: "photo").font(.footnote).foregroundStyle(.secondary)
                } else {
                    ProgressView()
                }
            }
            .frame(width: size.width, height: size.height)
            .clipShape(RoundedRectangle(cornerRadius: 14))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(file.name)
        .task(id: file.fileId) {
            guard image == nil, let path = await model.filePath(file) else {
                failed = image == nil
                return
            }
            let loaded = await Task.detached { UIImage(contentsOfFile: path) }.value
            withAnimation(.easeOut(duration: 0.2)) { image = loaded }
            failed = loaded == nil
        }
    }
}

private struct FileChip: View {
    let file: FileItem
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            HStack(spacing: 10) {
                Image(systemName: "doc").foregroundStyle(.secondary)
                VStack(alignment: .leading, spacing: 0) {
                    Text(file.name).font(.subheadline).lineLimit(1)
                    Text(Format.size(file.size)).font(.caption).foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 12).frame(minHeight: 48)
            .background(Theme.fill, in: RoundedRectangle(cornerRadius: 14))
        }
        .buttonStyle(.plain)
        .accessibilityHint(Copy.fileOpen)
    }
}

/// A message of mine on its way, or one that was not sent.
private struct PendingRow: View {
    let item: PendingItem
    let retry: () -> Void
    let discard: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Color.clear.frame(width: 36, height: 1)
            VStack(alignment: .leading, spacing: 4) {
                Text(item.body).foregroundStyle(.secondary)
                if item.failed {
                    HStack(spacing: 14) {
                        Label(Copy.notSent, systemImage: "exclamationmark.circle.fill").foregroundStyle(Theme.bad)
                        Button(Copy.retry, action: retry)
                        Button(Copy.discard, role: .destructive, action: discard)
                    }
                    .font(.footnote)
                } else {
                    Text(Copy.sending).font(.footnote).foregroundStyle(.tertiary)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.top, 8)
    }
}
