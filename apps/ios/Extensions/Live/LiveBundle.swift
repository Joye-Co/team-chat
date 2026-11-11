import ActivityKit
import SwiftUI
import WidgetKit

@main
struct LiveBundle: WidgetBundle {
    var body: some Widget {
        RunLiveActivity()
    }
}

private let accent = Color(red: 0x40 / 255, green: 0x9C / 255, blue: 1)

/// One run on the lock screen and in the Dynamic Island: who works on which 话题, the step and
/// how far it is.
struct RunLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: RunActivity.self) { context in
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Initial(name: context.attributes.memberName, size: 28)
                    VStack(alignment: .leading, spacing: 0) {
                        Text(context.attributes.memberName).font(.subheadline.weight(.semibold)).lineLimit(1)
                        Text(context.attributes.title).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer(minLength: 8)
                    Text(context.state.statusWord).font(.caption.weight(.semibold))
                        .foregroundStyle(context.state.ended ? Color.secondary : accent)
                }
                Text(context.state.step).font(.subheadline).lineLimit(2)
                if let progress = context.state.progress {
                    ProgressView(value: progress).tint(accent)
                }
            }
            .padding(16)
            .activityBackgroundTint(nil)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    HStack(spacing: 6) {
                        Initial(name: context.attributes.memberName, size: 24)
                        Text(context.attributes.memberName).font(.subheadline.weight(.semibold)).lineLimit(1)
                    }
                }
                DynamicIslandExpandedRegion(.trailing) {
                    Text(context.state.statusWord).font(.caption.weight(.semibold)).foregroundStyle(accent)
                }
                DynamicIslandExpandedRegion(.bottom) {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(context.state.step).font(.subheadline).lineLimit(2)
                        if let progress = context.state.progress {
                            ProgressView(value: progress).tint(accent)
                        }
                    }
                }
            } compactLeading: {
                Initial(name: context.attributes.memberName, size: 20)
            } compactTrailing: {
                if let progress = context.state.progress {
                    ProgressView(value: progress).progressViewStyle(.circular).tint(accent).frame(width: 18, height: 18)
                } else {
                    Image(systemName: context.state.ended ? "checkmark" : "circle.dotted").foregroundStyle(accent)
                }
            } minimal: {
                Image(systemName: context.state.ended ? "checkmark" : "circle.dotted").foregroundStyle(accent)
            }
        }
    }
}

/// The first letter of the member's name on a soft tint, like the app's avatars.
private struct Initial: View {
    let name: String
    let size: CGFloat

    var body: some View {
        Text(String(name.prefix(1)))
            .font(.system(size: size * 0.45, weight: .semibold))
            .frame(width: size, height: size)
            .foregroundStyle(Color(red: 0.18, green: 0.20, blue: 0.45))
            .background(Color(red: 0.86, green: 0.90, blue: 1.0), in: RoundedRectangle(cornerRadius: size * 0.28))
    }
}
