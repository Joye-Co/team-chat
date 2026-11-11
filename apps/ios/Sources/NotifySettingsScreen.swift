import SwiftUI
import UIKit

/// 我 › 通知: the system permission, what to hear about and quiet hours. The choices live on the
/// server and hold for every device of mine.
struct NotifySettingsScreen: View {
    @ObservedObject private var push = PushCenter.shared
    @Environment(\.openURL) private var openURL
    @State private var prefs: NotifyPrefs?
    @State private var failure: String?
    @State private var saving = false

    var body: some View {
        List {
            Section {
                HStack {
                    Text(Copy.notifySystem)
                    Spacer()
                    Text(systemOn ? Copy.notifySystemOn : Copy.notifySystemOff).foregroundStyle(.secondary)
                }
                if push.status == .notDetermined {
                    Button(Copy.notifyAskNow) { Task { await push.requestPermission() } }
                } else if !systemOn {
                    Button(Copy.notifyOpenSettings) {
                        if let url = URL(string: UIApplication.openNotificationSettingsURLString) { openURL(url) }
                    }
                }
            } footer: {
                if push.status == .denied { Text(Copy.notifySystemOffHint) }
            }

            if let binding = Binding($prefs) {
                Section {
                    Toggle(Copy.notifyWaiting, isOn: save(binding.waiting))
                    Toggle(Copy.notifyMention, isOn: save(binding.mention))
                    Toggle(Copy.notifyResult, isOn: save(binding.result))
                    Toggle(Copy.notifyBlocked, isOn: save(binding.blocked))
                } header: {
                    Text(Copy.notifyRemindMe)
                } footer: {
                    Text(Copy.notifyAllDevices)
                }
                Section {
                    Toggle(Copy.notifyQuietOn, isOn: save(binding.quiet))
                    if binding.wrappedValue.quiet {
                        DatePicker(Copy.notifyQuietFrom, selection: clock(save(binding.quietStart)), displayedComponents: .hourAndMinute)
                        DatePicker(Copy.notifyQuietTo, selection: clock(save(binding.quietEnd)), displayedComponents: .hourAndMinute)
                    }
                } header: {
                    Text(Copy.notifyQuiet)
                } footer: {
                    Text(Copy.notifyQuietHint)
                }
            } else if failure == nil {
                Section { ProgressView().frame(maxWidth: .infinity) }
            }
            if let failure {
                Section { Text(failure).foregroundStyle(Theme.bad) }
            }
        }
        .navigationTitle(Copy.notifyTitle)
        .navigationBarTitleDisplayMode(.inline)
        .task {
            await push.refreshStatus()
            await load()
        }
        .refreshable { await load() }
    }

    private var systemOn: Bool {
        push.status == .authorized || push.status == .provisional || push.status == .ephemeral
    }

    private func load() async {
        guard let saved = Session.saved() else { return }
        let result = await Task.detached { Result { try notifyPrefs(baseUrl: saved.server, token: saved.token) } }.value
        switch result {
        case .success(let p):
            prefs = p
            failure = nil
        case .failure:
            if prefs == nil { failure = Copy.notifyLoadFailed }
        }
    }

    /// A binding that saves the whole settings after each change and keeps the server's answer.
    private func save<T>(_ b: Binding<T>) -> Binding<T> {
        Binding(
            get: { b.wrappedValue },
            set: { value in
                b.wrappedValue = value
                guard let next = prefs, let saved = Session.saved() else { return }
                let zone = TimeZone.current.identifier
                Task {
                    let result = await Task.detached {
                        Result { try setNotifyPrefs(baseUrl: saved.server, token: saved.token, prefs: next, timeZone: zone) }
                    }.value
                    switch result {
                    case .success(let p):
                        if prefs == next { prefs = p }
                        failure = nil
                    case .failure:
                        failure = Copy.notifySaveFailed
                    }
                }
            })
    }

    /// "HH:MM" as a date today, for the time pickers.
    private func clock(_ b: Binding<String>) -> Binding<Date> {
        Binding(
            get: {
                let parts = b.wrappedValue.split(separator: ":").compactMap { Int($0) }
                let comps = DateComponents(hour: parts.first ?? 22, minute: parts.count > 1 ? parts[1] : 0)
                return Calendar.current.date(from: comps) ?? Date()
            },
            set: { date in
                let c = Calendar.current.dateComponents([.hour, .minute], from: date)
                b.wrappedValue = String(format: "%02d:%02d", c.hour ?? 0, c.minute ?? 0)
            })
    }
}
