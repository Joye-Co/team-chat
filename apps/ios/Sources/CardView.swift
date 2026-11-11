import SwiftUI

/// A message's card (Card v2 JSON, or v1). Owned by M2; until then the card's title and the
/// message's text version.
struct CardView: View {
    let message: MessageItem
    let json: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let title { Text(title).fontWeight(.semibold) }
            if !message.body.isEmpty { Text(message.body).font(.subheadline).foregroundStyle(.secondary) }
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
    }

    private var title: String? {
        guard let data = json.data(using: .utf8),
            let card = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        return card["title"] as? String
    }
}
