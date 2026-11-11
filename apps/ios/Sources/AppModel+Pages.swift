import Foundation

/// Reads and writes of the pages beyond chat. Cache reads return at once; the calls that wait
/// for the network run off the main thread.
extension AppModel {
    /// Runs a core call that may wait for the network off the main thread.
    nonisolated func offMain<T: Sendable>(_ work: @escaping @Sendable (ChatClient) throws -> T) async -> Result<T, Error> {
        guard let client = await client else { return .failure(CoreError.Network) }
        return await Task.detached { Result { try work(client) } }.value
    }

    /// Changes in any 产出 list: the sum of the `artifacts:` ticks.
    var artifactsTick: Int { ticks.filter { $0.key.hasPrefix("artifacts:") }.values.reduce(0, +) }
    /// Changes in any run: the sum of the `runs:` ticks.
    var runsTick: Int { ticks.filter { $0.key.hasPrefix("runs:") }.values.reduce(0, +) }

    // MARK: 产出

    func artifacts(group: String? = nil, topic: String? = nil) -> [ArtifactItem] {
        (try? client?.artifacts(groupId: group, topicId: topic)) ?? []
    }

    func refreshArtifacts(group: String? = nil) {
        client?.refreshArtifacts(groupId: group)
    }

    func artifact(_ id: String) -> ArtifactDetail? {
        (try? client?.artifact(artifactId: id)) ?? nil
    }

    func loadArtifact(_ id: String) async -> ArtifactDetail? {
        try? await offMain { try $0.loadArtifact(artifactId: id) }.get()
    }

    func openVersion(_ id: String, version: UInt32?) async -> OpenedVersion? {
        try? await offMain { try $0.openArtifactVersion(artifactId: id, version: version) }.get()
    }

    // MARK: 成员

    func memberDetail(_ id: String) -> MemberDetail? {
        (try? client?.memberDetail(memberId: id)) ?? nil
    }

    /// Our 私聊's group id, made on the server when there is none yet.
    func openDirect(with member: String) async -> String? {
        try? await offMain { try $0.openDm(memberId: member) }.get()
    }

    func mainTopic(of group: String) -> String {
        client?.mainTopicOf(groupId: group) ?? ""
    }

    // MARK: 群 and 话题

    func groupInfo(_ id: String) -> GroupInfo? {
        (try? client?.groupInfo(groupId: id)) ?? nil
    }

    func runs(in topic: String) -> [RunItem] {
        (try? client?.runs(topicId: topic)) ?? []
    }

    func activeRuns() -> [RunItem] {
        (try? client?.activeRuns()) ?? []
    }

    func run(_ id: String) -> RunItem? {
        (try? client?.run(runId: id)) ?? nil
    }

    func terminal(_ run: String, after: UInt64) async -> TerminalPage? {
        try? await offMain { try $0.runTerminal(runId: run, afterSeq: after) }.get()
    }

    /// Stops a run; `false` when it could not be stopped.
    func stop(run: String) async -> Bool {
        (try? await offMain { try $0.cancelRun(runId: run) }.get()) != nil
    }

    // MARK: 搜索

    /// Messages on the server; `nil` when offline.
    func searchMessages(_ query: String) async -> [SearchHitItem]? {
        try? await offMain { try $0.searchMessages(query: query, groupId: nil) }.get()
    }

    func searchTopics(_ query: String) -> [TopicItem] {
        (try? client?.searchTopics(query: query)) ?? []
    }

    func searchArtifacts(_ query: String) -> [ArtifactItem] {
        (try? client?.searchArtifacts(query: query)) ?? []
    }

    // MARK: 单独问

    func messageContext(_ id: String) -> MessageContext? {
        (try? client?.messageContext(messageId: id)) ?? nil
    }

    /// Asks a member privately; answers the 私聊's group id.
    func askAside(member: String, about context: MessageContext, question: String) async -> String? {
        try? await offMain {
            try $0.askAside(memberId: member, groupId: context.groupId, topicId: context.topicId, uptoSeq: context.seq, question: question)
        }.get()
    }

    func asides(group: String, topic: String) -> [TopicItem] {
        (try? client?.asides(groupId: group, topicId: topic)) ?? []
    }
}

extension RunItem {
    /// The status in the interface's words (DESIGN.md §9.1).
    var statusWord: String {
        if queued { return Copy.runQueued }
        switch status {
        case "running", "created": return Copy.runWorking
        case "waiting": return Copy.runWaiting
        case "blocked": return Copy.runBlocked
        case "done": return Copy.runDone
        case "failed": return Copy.runFailed
        case "cancelled": return Copy.runStopped
        default: return Copy.runWorking
        }
    }

    var statusSymbol: String {
        if queued { return "clock" }
        switch status {
        case "running", "created": return "circle.dotted"
        case "waiting": return "hourglass"
        case "blocked": return "exclamationmark.triangle"
        case "done": return "checkmark.circle"
        case "failed": return "xmark.octagon"
        case "cancelled": return "stop.circle"
        default: return "circle.dotted"
        }
    }

    /// 0...1 by plan steps; nil without a plan.
    var progress: Double? {
        plan.isEmpty ? nil : Double(stepsDone) / Double(plan.count)
    }

    /// One line for a list: the step it is on, its summary, or the 话题.
    var line: String {
        if active, let step = currentStep { return step }
        if !summary.isEmpty { return Format.oneLine(summary) }
        return topicTitle
    }
}
