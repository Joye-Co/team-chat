import ActivityKit

/// What a Live Activity shows about one run: the app starts and updates it, the Live extension
/// draws it. The texts are data from the server, not interface copy.
struct RunActivity: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        /// The current step, in the member's words.
        var step: String
        /// 0...1, or nil when the run does not report progress.
        var progress: Double?
    }

    /// The 话题 the run belongs to.
    var topicId: String
    var title: String
    /// The member that is doing the work.
    var memberName: String
}
