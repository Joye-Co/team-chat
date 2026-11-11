import Foundation

/// The 首页 and 交代 (M2).
extension AppModel {
    /// 等你的事, 正在做 and 今天做完的, from the cache.
    func home() -> HomeData? {
        let start = Calendar.current.startOfDay(for: .now).timeIntervalSince1970 * 1000
        return try? client?.home(todayStart: UInt64(start))
    }

    /// The 私聊 and 群 where `member` can be handed something, the 私聊 first.
    func handOffPlaces(_ member: String) -> [HandOffPlace] {
        (try? client?.handOffPlaces(memberId: member)) ?? []
    }

    /// Starts a 话题 in `group` that asks `member` to do `text`. False when it could not be queued.
    func handOff(_ text: String, to member: String, in group: String) -> Bool {
        (try? client?.handOff(groupId: group, memberId: member, text: text)) != nil
    }
}
