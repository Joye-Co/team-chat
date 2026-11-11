import Foundation

/// A file on its way up, shown in its chat until the message exists.
struct UploadItem: Identifiable, Equatable {
    let id = UUID()
    let name: String
    var failed = false
}

extension AppModel {
    /// Uploads a local file and sends it into a chat. The copy at `url` is ours and is removed
    /// afterwards.
    func sendFile(at url: URL, name: String, group: String, topic: String, caption: String = "", quote: String? = nil) {
        guard let client else { return }
        let item = UploadItem(name: name)
        uploads[topic, default: []].append(item)
        let path = url.path
        Task {
            let ok = await Task.detached {
                defer { try? FileManager.default.removeItem(atPath: path) }
                return (try? client.sendFile(groupId: group, topicId: topic, path: path, name: name, caption: caption, quoteId: quote)) != nil
            }.value
            if ok {
                uploads[topic]?.removeAll { $0.id == item.id }
            } else if let index = uploads[topic]?.firstIndex(where: { $0.id == item.id }) {
                uploads[topic]?[index].failed = true
            }
        }
    }

    func dismissUpload(_ id: UUID, in topic: String) {
        uploads[topic]?.removeAll { $0.id == id }
    }

    /// A private copy of a picked file, so the upload can read it after the picker is gone.
    nonisolated static func stage(data: Data, name: String) throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("outgoing", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let url = dir.appendingPathComponent("\(UUID().uuidString)-\(name)")
        try data.write(to: url)
        return url
    }
}
