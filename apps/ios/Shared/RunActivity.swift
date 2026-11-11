import ActivityKit

/// What a Live Activity shows about one run: the app starts and updates it, the Live extension
/// draws it. Every text is given by the app (from its copy catalog or the server), so the
/// extension types none.
struct RunActivity: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        /// The current step, in the member's words; the summary once it ended.
        var step: String
        /// 0...1, or nil when the run does not report progress.
        var progress: Double?
        /// 正在做, 做完了, ... from the app's catalog.
        var statusWord: String
        var ended: Bool
    }

    var runId: String
    /// The 话题 the run belongs to.
    var topicId: String
    var title: String
    /// The member that is doing the work.
    var memberName: String
}
