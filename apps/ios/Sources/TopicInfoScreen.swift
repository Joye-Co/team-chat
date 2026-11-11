import SwiftUI

/// A 话题's 详情: its state, the members' work on it (plan, live view), its 产出 and my 单独问.
struct TopicInfoScreen: View {
    @EnvironmentObject private var model: AppModel
    let groupId: String
    let topicId: String
    @State private var topic: TopicItem?
    @State private var runs: [RunItem] = []
    @State private var artifacts: [ArtifactItem] = []
    @State private var asides: [TopicItem] = []

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                if let topic {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(topic.title).font(.title2.weight(.bold))
                        HStack(spacing: 8) {
                            if let word = topic.statusWord {
                                StatusChip(word: word, live: topic.status == "in_progress")
                            }
                            NavigationLink(value: Route.groupInfo(group: groupId)) {
                                Text(model.group(groupId)?.name ?? "").font(.subheadline).foregroundStyle(Theme.accent)
                            }
                            Text(Copy.messageCount(topic.messageCount)).font(.subheadline).foregroundStyle(.secondary)
                        }
                    }
                    .padding(.horizontal, 12).padding(.top, 12)
                }
                if let current = runs.first(where: \.active), !current.plan.isEmpty {
                    SectionTitle(text: Copy.plan, trailing: Copy.stepOf(current.stepsDone, current.plan.count))
                    PlanList(steps: current.plan).padding(14)
                        .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
                }
                if !runs.isEmpty {
                    SectionTitle(text: Copy.runsSection)
                    Panel { ForEach(runs.prefix(8), id: \.id) { RunRow(run: $0) } }
                }
                if !artifacts.isEmpty {
                    SectionTitle(text: Copy.topicArtifacts)
                    Panel { ForEach(artifacts, id: \.id) { ArtifactRow(artifact: $0, showWhere: false) } }
                }
                if !asides.isEmpty {
                    SectionTitle(text: Copy.askAsideEarlier)
                    Panel { ForEach(asides, id: \.id) { AsideRow(topic: $0) } }
                }
            }
            .padding(.horizontal, Theme.gutter).padding(.bottom, 24)
        }
        .background(Theme.page)
        .navigationTitle(Copy.details)
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            reload()
            model.refreshArtifacts(group: groupId)
        }
        .onChange(of: model.ticks["runs:\(topicId)"]) { reload() }
        .onChange(of: model.ticks["artifacts:\(topicId)"]) { reload() }
        .onChange(of: model.topicTicks[topicId]) { reload() }
    }

    private func reload() {
        topic = model.topic(topicId, in: groupId)
        runs = model.runs(in: topicId)
        artifacts = model.artifacts(group: groupId, topic: topicId)
        asides = model.asides(group: groupId, topic: topicId)
    }
}

/// The steps of a plan with their state.
struct PlanList: View {
    let steps: [PlanStep]

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(steps.enumerated()), id: \.offset) { _, step in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Image(systemName: symbol(step.state))
                        .foregroundStyle(color(step.state))
                        .symbolEffect(.pulse, options: .repeating, isActive: step.state == "current")
                        .frame(width: 20)
                    Text(step.text).font(.subheadline)
                        .fontWeight(step.state == "current" ? .semibold : .regular)
                        .foregroundStyle(step.state == "pending" ? .secondary : .primary)
                        .strikethrough(false)
                }
                .accessibilityElement(children: .combine)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func symbol(_ state: String) -> String {
        switch state {
        case "done": "checkmark.circle.fill"
        case "current": "circle.dotted.circle"
        case "waiting": "hourglass.circle"
        default: "circle"
        }
    }

    private func color(_ state: String) -> Color {
        switch state {
        case "done": Theme.online
        case "current": Theme.accent
        case "waiting": Theme.waiting
        default: .secondary
        }
    }
}

/// One of my 单独问: opens its chat in my 私聊 with the member.
struct AsideRow: View {
    let topic: TopicItem

    var body: some View {
        NavigationLink(value: Route.chat(group: topic.groupId, topic: topic.id)) {
            HStack(spacing: 12) {
                Image(systemName: "bubble.left.and.text.bubble.right").foregroundStyle(Theme.accent).frame(width: 28)
                Text(topic.title).font(.subheadline).lineLimit(2)
                Spacer(minLength: 8)
                Text(Format.listTime(topic.lastActivityAt)).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
                CountBadge(count: topic.unread, strong: true)
            }
            .padding(.horizontal, 12).frame(minHeight: 52)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowButtonStyle())
    }
}
