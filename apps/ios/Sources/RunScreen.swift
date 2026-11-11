import ActivityKit
import Combine
import SwiftUI

/// 实时查看 on the phone: one run's plan and its terminal as it works. Only watch and 停止;
/// 接管 stays on the computer.
struct RunScreen: View {
    @EnvironmentObject private var model: AppModel
    let runId: String
    @State private var run: RunItem?
    @State private var lines: [TerminalLineItem] = []
    @State private var lastSeq: UInt64 = 0
    @State private var confirmStop = false
    @State private var stopping = false
    @State private var stopFailed = false
    @State private var onLockScreen = false
    @State private var lockScreenUnavailable = false

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if let run {
                        header(run)
                        if !run.summary.isEmpty {
                            SectionTitle(text: Copy.summary)
                            Text(run.summary).font(.subheadline).frame(maxWidth: .infinity, alignment: .leading)
                                .padding(14).background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
                        }
                        if !run.plan.isEmpty {
                            SectionTitle(text: Copy.plan, trailing: run.active ? Copy.stepOf(run.stepsDone, run.plan.count) : nil)
                            PlanList(steps: run.plan).padding(14)
                                .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
                        }
                        SectionTitle(text: Copy.liveView)
                        terminal
                        Color.clear.frame(height: 1).id(bottomId)
                    }
                }
                .padding(.horizontal, Theme.gutter).padding(.bottom, 24)
            }
            .onChange(of: lines.count) { withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(bottomId, anchor: .bottom) } }
        }
        .background(Theme.page)
        .safeAreaInset(edge: .bottom, spacing: 0) { if run?.active == true { actions } }
        .navigationTitle(Copy.liveView)
        .navigationBarTitleDisplayMode(.inline)
        .confirmationDialog(Copy.stopConfirm, isPresented: $confirmStop, titleVisibility: .visible) {
            Button(Copy.stop, role: .destructive) { Task { await stop() } }
        } message: {
            Text(Copy.stopConfirmHint(run?.memberName ?? ""))
        }
        .alert(Copy.stopFailed, isPresented: $stopFailed) { Button(Copy.close, role: .cancel) {} }
        .alert(Copy.lockScreenUnavailable, isPresented: $lockScreenUnavailable) { Button(Copy.close, role: .cancel) {} }
        .onAppear {
            run = model.run(runId)
            onLockScreen = RunActivities.shared.isShowing(runId)
        }
        .onChange(of: model.ticks["runs:\(run?.topicId ?? "")"]) { run = model.run(runId) }
        .task { await follow() }
    }

    private let bottomId = "bottom"

    private func header(_ run: RunItem) -> some View {
        HStack(spacing: 12) {
            Avatar(name: run.memberName, round: false, size: 44)
            VStack(alignment: .leading, spacing: 2) {
                Text(run.memberName).font(.headline)
                NavigationLink(value: Route.topicInfo(group: run.groupId, topic: run.topicId)) {
                    Text(run.topicTitle).font(.subheadline).foregroundStyle(Theme.accent).lineLimit(1)
                }
            }
            Spacer(minLength: 8)
            StatusChip(word: run.statusWord, live: run.active && !run.queued)
        }
        .padding(.horizontal, 4).padding(.top, 12)
    }

    private var terminal: some View {
        VStack(alignment: .leading, spacing: 6) {
            if lines.isEmpty {
                Text(run?.active == false ? Copy.liveEnded : Copy.liveWaiting)
                    .foregroundStyle(Theme.terminalDim)
            }
            ForEach(lines, id: \.seq) { line in
                if line.isCommand {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text("$").foregroundStyle(Theme.accent)
                        Text(line.text).foregroundStyle(Theme.terminalInk)
                    }
                    .fontWeight(.semibold)
                } else {
                    Text(line.text).foregroundStyle(line.failed ? Theme.bad : Theme.terminalDim)
                        .lineLimit(12)
                }
            }
            if run?.active == true && !lines.isEmpty {
                Text("▍").foregroundStyle(Theme.accent).symbolEffect(.pulse)
                    .accessibilityHidden(true)
            }
        }
        .font(.system(.footnote, design: .monospaced))
        .textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(Theme.terminal, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
        .environment(\.colorScheme, .dark)
    }

    private var actions: some View {
        HStack(spacing: 12) {
            Button {
                toggleLockScreen()
            } label: {
                Label(onLockScreen ? Copy.lockScreenHide : Copy.lockScreenShow, systemImage: onLockScreen ? "rectangle.badge.minus" : "rectangle.badge.plus")
                    .font(.subheadline.weight(.semibold)).frame(maxWidth: .infinity, minHeight: 44)
            }
            .buttonStyle(.bordered).tint(Theme.accent)
            Button(role: .destructive) { confirmStop = true } label: {
                Label(Copy.stop, systemImage: "stop.fill")
                    .font(.subheadline.weight(.semibold)).frame(minWidth: 96, minHeight: 44)
            }
            .buttonStyle(.bordered).tint(Theme.bad)
            .disabled(stopping)
        }
        .padding(.horizontal, Theme.gutter).padding(.vertical, 10)
        .background(.bar)
    }

    /// Reads the terminal while the run works, every two seconds.
    private func follow() async {
        while !Task.isCancelled {
            if let page = await model.terminal(runId, after: lastSeq) {
                let known = Set(lines.map(\.seq))
                lines += page.lines.filter { !known.contains($0.seq) }
                if lines.count > 400 { lines.removeFirst(lines.count - 400) }
                lastSeq = page.lastSeq
            }
            run = model.run(runId)
            if run?.active == false { return }
            try? await Task.sleep(for: .seconds(2))
        }
    }

    private func stop() async {
        stopping = true
        defer { stopping = false }
        if await model.stop(run: runId) {
            run = model.run(runId)
            RunActivities.shared.update(model)
        } else {
            stopFailed = true
        }
    }

    private func toggleLockScreen() {
        guard let run else { return }
        if onLockScreen {
            RunActivities.shared.end(run.id)
            onLockScreen = false
        } else if RunActivities.shared.start(run, model: model) {
            onLockScreen = true
        } else {
            lockScreenUnavailable = true
        }
    }
}

/// The Live Activities the app started: one per run, updated whenever a run changes and ended
/// when the run ends.
@MainActor
final class RunActivities {
    static let shared = RunActivities()
    private var watching: AnyCancellable?

    func isShowing(_ runId: String) -> Bool {
        Activity<RunActivity>.activities.contains { $0.attributes.runId == runId && $0.activityState == .active }
    }

    /// Starts showing a run; `false` when Live Activities are off on this phone.
    func start(_ run: RunItem, model: AppModel) -> Bool {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return false }
        if isShowing(run.id) { return true }
        let attributes = RunActivity(runId: run.id, topicId: run.topicId, title: run.topicTitle, memberName: run.memberName)
        do {
            _ = try Activity.request(attributes: attributes, content: .init(state: state(run), staleDate: nil))
        } catch {
            return false
        }
        watch(model)
        return true
    }

    func end(_ runId: String) {
        for activity in Activity<RunActivity>.activities where activity.attributes.runId == runId {
            Task { await activity.end(nil, dismissalPolicy: .immediate) }
        }
    }

    /// Reads every shown run again and updates or ends its activity.
    func update(_ model: AppModel) {
        for activity in Activity<RunActivity>.activities {
            guard let run = model.run(activity.attributes.runId) else { continue }
            let content = ActivityContent(state: state(run), staleDate: nil)
            Task {
                if run.active {
                    await activity.update(content)
                } else {
                    await activity.end(content, dismissalPolicy: .after(.now + 15 * 60))
                }
            }
        }
    }

    private func watch(_ model: AppModel) {
        guard watching == nil else { return }
        watching = model.$ticks
            .debounce(for: .milliseconds(300), scheduler: RunLoop.main)
            .sink { [weak self, weak model] _ in
                guard let self, let model else { return }
                self.update(model)
            }
    }

    private func state(_ run: RunItem) -> RunActivity.ContentState {
        RunActivity.ContentState(
            step: run.active ? (run.currentStep ?? run.line) : (run.summary.isEmpty ? run.line : Format.oneLine(run.summary)),
            progress: run.active ? run.progress : 1,
            statusWord: run.statusWord,
            ended: !run.active
        )
    }
}
