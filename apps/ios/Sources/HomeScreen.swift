import SwiftUI

/// 首页: what waits on me (answered right here), what is being done, what got done today, and
/// 交代一件事.
struct HomeScreen: View {
    @EnvironmentObject private var model: AppModel
    @State private var data: HomeData?
    @State private var handingOff = false
    @State private var toast: String?

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 28) {
                    ConnectionBadge(state: model.connection)
                    waiting
                    section(Copy.homeWorking, count: data?.working.count ?? 0) {
                        if let rows = data?.working, !rows.isEmpty {
                            rowsGroup(rows) { row in HomeRow(item: row, symbol: "circle.dotted", tint: Theme.accent) }
                        } else {
                            quiet(Copy.homeNothingWorking)
                        }
                    }
                    section(Copy.homeDoneToday, count: data?.doneToday.count ?? 0) {
                        if let rows = data?.doneToday, !rows.isEmpty {
                            rowsGroup(rows) { row in HomeRow(item: row, symbol: "checkmark.circle.fill", tint: .green) }
                        } else {
                            quiet(Copy.homeNothingDone)
                        }
                    }
                }
                .padding(.horizontal, Theme.gutter)
                .padding(.top, 4)
                .padding(.bottom, 96)
            }
            .background(Theme.page)
            .navigationTitle(Copy.tabHome)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    NavigationLink(value: Route.me) { Image(systemName: "person.crop.circle").accessibilityLabel(Copy.me) }
                }
            }
            .navigationDestination(for: Route.self) { RouteView(route: $0) }
            .overlay(alignment: .bottom) { bottom }
            .sheet(isPresented: $handingOff) {
                HandOffSheet { sent in
                    handingOff = false
                    if let sent { show(sent) }
                }
                .presentationDetents([.large])
            }
            .onAppear(perform: reload)
            .onChange(of: model.groups) { reload() }
            .onChange(of: model.ticks) { reload() }
            .onChange(of: model.members.count) { reload() }
            .refreshable {
                model.cameToForeground()
                reload()
            }
        }
    }

    private func reload() {
        data = model.home()
    }

    // MARK: Sections

    @ViewBuilder
    private var waiting: some View {
        section(Copy.homeWaiting, count: data?.waiting.count ?? 0, strong: true) {
            if let rows = data?.waiting, !rows.isEmpty {
                VStack(spacing: 12) {
                    ForEach(rows, id: \.topicId) { row in WaitingItem(item: row) }
                }
            } else {
                VStack(spacing: 6) {
                    Image(systemName: "checkmark.circle").font(.title2).foregroundStyle(.secondary)
                    Text(Copy.homeAllClear).font(.subheadline.weight(.medium))
                    Text(Copy.homeAllClearHint).font(.footnote).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity).padding(.vertical, 22)
                .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
            }
        }
    }

    private func section<Content: View>(_ title: String, count: Int, strong: Bool = false, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text(title).font(.title3.weight(.semibold))
                if count > 0 {
                    Text("\(count)").font(.subheadline.weight(.semibold).monospacedDigit())
                        .foregroundStyle(strong ? Theme.waiting : .secondary)
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityAddTraits(.isHeader)
            content()
        }
    }

    private func rowsGroup<Row: View>(_ rows: [HomeItem], @ViewBuilder row: @escaping (HomeItem) -> Row) -> some View {
        VStack(spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.element.topicId) { i, item in
                NavigationLink(value: Route.chat(group: item.groupId, topic: item.topicId)) { row(item) }
                    .buttonStyle(.plain)
                if i < rows.count - 1 { Divider().padding(.leading, 52) }
            }
        }
        .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
    }

    private func quiet(_ text: String) -> some View {
        Text(text).font(.subheadline).foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 14).padding(.vertical, 14)
            .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
    }

    // MARK: 交代 and the toast

    private var bottom: some View {
        VStack(spacing: 10) {
            if let toast {
                Label(toast, systemImage: "checkmark.circle.fill")
                    .font(.footnote.weight(.medium))
                    .padding(.horizontal, 14).padding(.vertical, 10)
                    .background(.regularMaterial, in: Capsule())
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
            Button { handingOff = true } label: {
                Label(Copy.handOff, systemImage: "plus")
                    .font(.body.weight(.semibold))
                    .padding(.horizontal, 22).frame(minHeight: 50)
                    .foregroundStyle(Theme.onAccent)
                    .background(Theme.accent, in: Capsule())
                    .shadow(color: .black.opacity(0.12), radius: 12, y: 4)
            }
            .buttonStyle(.plain)
        }
        .padding(.bottom, 14)
        .animation(.snappy, value: toast)
    }

    private func show(_ text: String) {
        toast = text
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(3))
            if toast == text { toast = nil }
        }
    }
}

/// One thing that waits on me: who asks, where, and the card to answer right here.
private struct WaitingItem: View {
    let item: HomeItem

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            NavigationLink(value: Route.chat(group: item.groupId, topic: item.topicId)) {
                HStack(spacing: 10) {
                    Avatar(name: item.memberName, round: !item.memberIsBot, size: 28)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(Copy.homeIn(item.memberName, item.groupName)).font(.subheadline.weight(.medium)).lineLimit(1)
                        if item.card == nil {
                            Text(item.title).font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                        }
                    }
                    Spacer(minLength: 4)
                    Text(Format.listTime(item.at)).font(.caption).foregroundStyle(.secondary)
                    Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(.tertiary)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.homeOpenTopic + " " + item.title)
            if let card = item.card, let json = card.cardJson {
                CardView(message: card, json: json)
            } else {
                Label(waitingWord, systemImage: "hourglass")
                    .font(.footnote.weight(.medium)).foregroundStyle(Theme.waiting)
            }
        }
    }

    private var waitingWord: String {
        item.status == "waiting_review" ? Copy.statusWaitingMyReview : Copy.statusWaitingMyAnswer
    }
}

/// A 话题 in 正在做 or 今天做完的.
private struct HomeRow: View {
    let item: HomeItem
    let symbol: String
    let tint: Color

    var body: some View {
        HStack(spacing: 12) {
            Avatar(name: item.memberName.isEmpty ? item.title : item.memberName, round: !item.memberIsBot, size: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title).font(.subheadline.weight(.medium)).lineLimit(2).foregroundStyle(.primary)
                Text(subtitle).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 4)
            Image(systemName: symbol).foregroundStyle(tint)
                .symbolEffect(.pulse, options: .repeating, isActive: symbol == "circle.dotted")
        }
        .padding(.horizontal, 12).padding(.vertical, 12)
        .contentShape(Rectangle())
    }

    private var subtitle: String {
        let who = Copy.homeIn(item.memberName, item.groupName)
        let note = item.note.isEmpty ? "" : " · \(item.note)"
        return who + note
    }
}

// MARK: - 交代

/// 交代一件事: pick a 成员, the 群 (or the 私聊 with them), and say what to do. Sends a new 话题
/// that mentions them.
struct HandOffSheet: View {
    @EnvironmentObject private var model: AppModel
    /// Called with the toast text when sent, `nil` when cancelled.
    let done: (String?) -> Void

    @State private var member: MemberItem?
    @State private var place: HandOffPlace?
    @State private var places: [HandOffPlace] = []
    @State private var text = ""
    @State private var failed = false
    @FocusState private var writing: Bool

    private var candidates: [MemberItem] {
        model.members.filter { !$0.isMe }.sorted { ($0.isBot ? 0 : 1, $0.name) < ($1.isBot ? 0 : 1, $1.name) }
    }

    private var ready: Bool {
        member != nil && place != nil && !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 22) {
                    step(Copy.handOffTo) {
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 14) {
                                ForEach(candidates, id: \.id) { m in memberChip(m) }
                            }
                            .padding(.vertical, 2)
                        }
                    }
                    if let member {
                        step(Copy.handOffWhere) {
                            if places.isEmpty {
                                Text(Copy.handOffNoPlace).font(.subheadline).foregroundStyle(.secondary)
                            } else {
                                VStack(spacing: 0) {
                                    ForEach(Array(places.enumerated()), id: \.element.groupId) { i, p in
                                        placeRow(p, member: member)
                                        if i < places.count - 1 { Divider().padding(.leading, 44) }
                                    }
                                }
                                .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
                            }
                        }
                    }
                    step(Copy.handOffWhat) {
                        TextField(Copy.handOffPlaceholder, text: $text, axis: .vertical)
                            .lineLimit(4...10)
                            .focused($writing)
                            .padding(14)
                            .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
                    }
                    if failed {
                        Text(Copy.handOffFailed).font(.footnote).foregroundStyle(Theme.bad)
                    }
                }
                .padding(Theme.gutter)
            }
            .scrollDismissesKeyboard(.interactively)
            .background(Theme.page)
            .navigationTitle(Copy.handOff)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(Copy.handOffCancel) { done(nil) } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(Copy.handOffSend, action: send).fontWeight(.semibold).disabled(!ready)
                }
            }
        }
    }

    private func step<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
            content()
        }
    }

    private func memberChip(_ m: MemberItem) -> some View {
        let chosen = member?.id == m.id
        return Button {
            member = m
            places = model.handOffPlaces(m.id)
            place = places.first { !$0.isDm } ?? places.first
            failed = false
        } label: {
            VStack(spacing: 6) {
                Avatar(name: m.name, round: !m.isBot, size: 52)
                    .overlay(
                        RoundedRectangle(cornerRadius: m.isBot ? 52 * 0.28 + 3 : 29)
                            .strokeBorder(chosen ? Theme.accent : .clear, lineWidth: 2.5)
                            .padding(-3)
                    )
                Text(m.name).font(.caption).lineLimit(1).frame(width: 64)
                    .foregroundStyle(chosen ? Theme.accent : Color.primary)
                Text(m.role).font(.caption2).lineLimit(1).frame(width: 64).foregroundStyle(.secondary)
            }
        }
        .buttonStyle(.plain)
        .sensoryFeedback(.selection, trigger: chosen)
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }

    private func placeRow(_ p: HandOffPlace, member: MemberItem) -> some View {
        Button {
            place = p
        } label: {
            HStack(spacing: 12) {
                Image(systemName: p.isDm ? "person.crop.circle" : "number")
                    .frame(width: 20).foregroundStyle(.secondary)
                Text(p.isDm ? "\(Copy.directSection) · \(p.name)" : p.name).font(.subheadline).foregroundStyle(.primary)
                Spacer()
                if place?.groupId == p.groupId {
                    Image(systemName: "checkmark").font(.subheadline.weight(.semibold)).foregroundStyle(Theme.accent)
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 13)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func send() {
        guard let member, let place else { return }
        if model.handOff(text, to: member.id, in: place.groupId) {
            done(Copy.handOffSent(member.name, place.isDm ? Copy.directSection : place.name))
        } else {
            failed = true
        }
    }
}
