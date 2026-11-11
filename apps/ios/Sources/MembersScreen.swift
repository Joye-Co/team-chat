import SwiftUI

/// 成员 tab: everyone on the team, people first; 在做事 narrows it to the members working now.
struct MembersScreen: View {
    @EnvironmentObject private var model: AppModel
    @State private var workingOnly = false
    @State private var active: [RunItem] = []

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 0) {
                    Picker(Copy.membersTitle, selection: $workingOnly) {
                        Text(Copy.membersAll).tag(false)
                        Text(Copy.membersWorking).tag(true)
                    }
                    .pickerStyle(.segmented)
                    .padding(.horizontal, 12).padding(.bottom, 8)
                    LazyVStack(spacing: 2) {
                        ForEach(shown, id: \.id) { member in
                            MemberLine(member: member, detail: working(member))
                        }
                    }
                }
                .padding(.horizontal, Theme.gutter).padding(.vertical, 8)
            }
            .background(Theme.page)
            .overlay {
                if workingOnly && shown.isEmpty {
                    ContentUnavailableView(Copy.membersWorkingEmpty, systemImage: "person.2")
                }
            }
            .navigationTitle(Copy.membersTitle)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    NavigationLink(value: Route.search) {
                        Image(systemName: "magnifyingglass").accessibilityLabel(Copy.search)
                    }
                }
            }
            .navigationDestination(for: Route.self) { RouteView(route: $0) }
        }
        .onAppear { active = model.activeRuns() }
        .onChange(of: model.runsTick) { active = model.activeRuns() }
    }

    private var shown: [MemberItem] {
        workingOnly ? model.members.filter { m in active.contains { $0.memberId == m.id } } : model.members
    }

    private func working(_ member: MemberItem) -> String? {
        guard let run = active.first(where: { $0.memberId == member.id && !$0.queued }) else { return nil }
        return Copy.working(run.currentStep ?? run.topicTitle)
    }
}

/// One member: who they are, this week's numbers, what they do now and did lately, and a way
/// into our 私聊.
struct MemberScreen: View {
    @EnvironmentObject private var model: AppModel
    let memberId: String
    @State private var detail: MemberDetail?
    @State private var opening = false
    @State private var failed = false
    @State private var path: Route?

    var body: some View {
        ScrollView {
            if let detail {
                VStack(spacing: 0) {
                    header(detail.member)
                    stats(detail).padding(.top, 20)
                    let now = detail.recent.filter(\.active)
                    if !now.isEmpty {
                        SectionTitle(text: Copy.now)
                        Panel { ForEach(now, id: \.id) { RunRow(run: $0, showMember: !detail.member.isBot) } }
                    }
                    let done = detail.recent.filter { !$0.active }
                    SectionTitle(text: Copy.recentWork)
                    if done.isEmpty {
                        Text(Copy.recentWorkEmpty).font(.subheadline).foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 12)
                    } else {
                        Panel { ForEach(done, id: \.id) { RunRow(run: $0, showMember: !detail.member.isBot) } }
                    }
                    if !detail.sharedGroups.isEmpty {
                        SectionTitle(text: Copy.sharedGroups)
                        Text(detail.sharedGroups.joined(separator: Copy.nameSeparator))
                            .font(.subheadline).frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 12)
                    }
                }
                .padding(.horizontal, Theme.gutter).padding(.bottom, 24)
            }
        }
        .background(Theme.page)
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(item: $path) { RouteView(route: $0) }
        .alert(Copy.directMessageFailed, isPresented: $failed) { Button(Copy.close, role: .cancel) {} }
        .onAppear { reload() }
        .onChange(of: model.runsTick) { reload() }
        .onChange(of: model.members.first { $0.id == memberId }?.online) { reload() }
    }

    private func reload() {
        detail = model.memberDetail(memberId)
    }

    private func header(_ m: MemberItem) -> some View {
        VStack(spacing: 10) {
            PresenceAvatar(name: m.name, isBot: m.isBot, online: m.online, size: 84)
            VStack(spacing: 4) {
                Text(m.name).font(.title2.weight(.bold))
                Text([m.role.isEmpty ? (m.isBot ? Copy.aiMember : "") : m.role, m.online ? Copy.online : Copy.notOnline]
                    .filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.subheadline).foregroundStyle(.secondary)
            }
            if m.isMe {
                Text(Copy.itsYou).font(.footnote).foregroundStyle(.secondary)
            } else {
                Button { Task { await openDirect() } } label: {
                    Label(Copy.directMessage, systemImage: "bubble.left.fill")
                        .font(.body.weight(.semibold)).frame(minWidth: 160, minHeight: 44)
                }
                .buttonStyle(.borderedProminent).tint(Theme.accent)
                .disabled(opening)
                .padding(.top, 6)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 12)
    }

    /// 这周, centred: three numbers with their words under them.
    private func stats(_ d: MemberDetail) -> some View {
        VStack(spacing: 10) {
            Text(Copy.thisWeek).font(.footnote.weight(.semibold)).foregroundStyle(.secondary)
            HStack(spacing: 0) {
                if d.member.isBot {
                    stat(d.weekRuns, Copy.statRuns)
                    stat(d.weekDone, Copy.statDone)
                } else {
                    stat(d.weekRuns, Copy.statHandedOut)
                    stat(d.weekDone, Copy.statDone)
                }
                stat(d.weekArtifacts, Copy.statArtifacts)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 16)
        .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
    }

    private func stat(_ n: UInt32, _ word: String) -> some View {
        VStack(spacing: 2) {
            Text("\(n)").font(.title.weight(.semibold).monospacedDigit())
            Text(word).font(.footnote).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .combine)
    }

    private func openDirect() async {
        opening = true
        defer { opening = false }
        let group = if let id = detail?.dmGroupId { id } else { await model.openDirect(with: memberId) }
        guard let group else { failed = true; return }
        path = .chat(group: group, topic: model.mainTopic(of: group))
    }
}
