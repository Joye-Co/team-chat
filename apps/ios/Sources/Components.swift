import SwiftUI

/// Where a tap leads.
enum Route: Hashable {
    /// A chat: a group's own chat, a 私聊 or a 话题.
    case chat(group: String, topic: String)
    /// The 话题 of a group.
    case topics(group: String)
    case search
    case member(id: String)
    case artifact(id: String)
    case me
    case groupInfo(group: String)
    case topicInfo(group: String, topic: String)
    case run(id: String)
}

/// The screen of a route, for every navigation stack. Each feature owns its screen file.
struct RouteView: View {
    let route: Route

    var body: some View {
        switch route {
        case .chat(let group, let topic): ChatScreen(groupId: group, topicId: topic)
        case .topics(let group): TopicsScreen(groupId: group)
        case .search: SearchScreen()
        case .member(let id): MemberScreen(memberId: id)
        case .artifact(let id): ArtifactScreen(artifactId: id)
        case .me: MeScreen()
        case .groupInfo(let group): GroupInfoScreen(groupId: group)
        case .topicInfo(let group, let topic): TopicInfoScreen(groupId: group, topicId: topic)
        case .run(let id): RunScreen(runId: id)
        }
    }
}

/// The first letter of a name on a tint: round for a person, squared for a 群 or an AI member.
struct Avatar: View {
    let name: String
    var round = true
    var size: CGFloat = 40

    var body: some View {
        Text(String(name.prefix(1)))
            .font(.system(size: size * 0.42, weight: .semibold))
            .frame(width: size, height: size)
            .foregroundStyle(Theme.memberInk)
            .background(Theme.memberTint(name), in: RoundedRectangle(cornerRadius: round ? size / 2 : size * 0.28))
            .accessibilityHidden(true)
    }
}

/// The connection state, shown only when it is not normal.
struct ConnectionBadge: View {
    let state: String

    var body: some View {
        if let shown = ConnectionBadge.words(state) {
            Label(shown.word, systemImage: shown.symbol)
                .labelStyle(.titleAndIcon).font(.footnote).foregroundStyle(.secondary)
        }
    }

    static func words(_ state: String) -> (word: String, symbol: String)? {
        switch state {
        case "connected", "stopped": nil
        case "connecting": (Copy.connecting, "arrow.triangle.2.circlepath")
        case "unauthorized": (Copy.signInAgain, "exclamationmark.circle")
        case "unsupported": (Copy.updateApp, "exclamationmark.circle")
        default: (Copy.offline, "wifi.slash")
        }
    }
}

/// An unread count: tinted for a mention, quiet otherwise.
struct CountBadge: View {
    let count: UInt64
    var strong = false

    var body: some View {
        if count > 0 {
            Text(count > 99 ? "99+" : "\(count)")
                .font(.caption.weight(.semibold).monospacedDigit())
                .padding(.horizontal, 7).frame(minWidth: 22, minHeight: 22)
                .foregroundStyle(strong ? Theme.onAccent : .secondary)
                .background(strong ? Theme.accent : Theme.fill, in: Capsule())
        }
    }
}

extension TopicItem {
    /// The status in the interface's words; `nil` for a topic that is just open.
    var statusWord: String? {
        switch status {
        case "in_progress": Copy.statusInProgress
        case "waiting_answer":
            waitingOnMe ? Copy.statusWaitingMyAnswer : waitingNames.first.map(Copy.statusWaitingAnswerOf) ?? Copy.statusWaitingAnswer
        case "waiting_review":
            waitingOnMe ? Copy.statusWaitingMyReview : waitingNames.first.map(Copy.statusWaitingReviewOf) ?? Copy.statusWaitingReview
        case "blocked": Copy.statusBlocked
        case "done": Copy.statusDone
        default: nil
        }
    }

    var statusSymbol: String {
        switch status {
        case "in_progress": "circle.dotted"
        case "waiting_answer", "waiting_review": "hourglass"
        case "blocked": "exclamationmark.triangle"
        case "done": "checkmark.circle"
        default: "number"
        }
    }

    /// It is my turn: an answer or a review of mine is what it waits for.
    var isMyTurn: Bool { waitingOnMe && status.hasPrefix("waiting") }
}

enum Format {
    private static func date(_ ms: UInt64) -> Date { Date(timeIntervalSince1970: Double(ms) / 1000) }

    /// 10:48
    static func time(_ ms: UInt64) -> String {
        date(ms).formatted(date: .omitted, time: .shortened)
    }

    /// For a list row: the time today, 昨天, or the date.
    static func listTime(_ ms: UInt64) -> String {
        guard ms > 0 else { return "" }
        let d = date(ms)
        if Calendar.current.isDateInToday(d) { return time(ms) }
        if Calendar.current.isDateInYesterday(d) { return Copy.yesterday }
        return d.formatted(.dateTime.month().day())
    }

    /// For a day divider in a chat.
    static func day(_ ms: UInt64) -> String {
        let d = date(ms)
        if Calendar.current.isDateInToday(d) { return Copy.today }
        if Calendar.current.isDateInYesterday(d) { return Copy.yesterday }
        let sameYear = Calendar.current.isDate(d, equalTo: .now, toGranularity: .year)
        return d.formatted(sameYear ? .dateTime.month().day().weekday() : .dateTime.year().month().day())
    }

    static func sameDay(_ a: UInt64, _ b: UInt64) -> Bool {
        Calendar.current.isDate(date(a), inSameDayAs: date(b))
    }

    /// Text on one line: line breaks and runs of spaces become one space.
    static func oneLine(_ text: String) -> String {
        text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }

    static func size(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }
}
