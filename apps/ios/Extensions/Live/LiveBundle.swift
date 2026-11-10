import ActivityKit
import SwiftUI
import WidgetKit

@main
struct LiveBundle: WidgetBundle {
    var body: some Widget {
        RunLiveActivity()
    }
}

/// A shell: one run on the lock screen and in the Dynamic Island. The app does not start it yet.
struct RunLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: RunActivity.self) { context in
            VStack(alignment: .leading, spacing: 6) {
                Text(context.attributes.title).font(.headline).lineLimit(1)
                Text(context.state.step).font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
                if let progress = context.state.progress {
                    ProgressView(value: progress)
                }
            }
            .padding(16)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Text(context.attributes.memberName).font(.subheadline.weight(.semibold)).lineLimit(1)
                }
                DynamicIslandExpandedRegion(.bottom) {
                    Text(context.state.step).font(.subheadline).lineLimit(2)
                }
            } compactLeading: {
                Image(systemName: "circle.dotted")
            } compactTrailing: {
                if let progress = context.state.progress {
                    ProgressView(value: progress).progressViewStyle(.circular)
                }
            } minimal: {
                Image(systemName: "circle.dotted")
            }
        }
    }
}
