import SwiftUI

/// 我: my name, 登录和安全 (password, sign-in email, 面容解锁, signing another device in), the
/// signed-in devices with 退出登录, notifications (M3) and signing this phone out.
struct MeScreen: View {
    @EnvironmentObject private var model: AppModel
    @AppStorage(AppModel.faceUnlockKey) private var faceUnlock = false
    @State private var devices: [DeviceItem]?
    @State private var devicesFailed = false
    @State private var password: PasswordState?
    @State private var email: String?
    @State private var editingPassword = false
    @State private var helping = false
    @State private var leaving: DeviceItem?
    @State private var failure: String?
    @State private var askSignOut = false

    var body: some View {
        List {
            Section {
                HStack(spacing: 14) {
                    Avatar(name: model.myName, size: 56)
                    Text(model.myName).font(.title3.weight(.semibold))
                }
                .padding(.vertical, 4)
            }

            Section(Copy.meSecurity) {
                Button { editingPassword = true } label: {
                    LabeledContent {
                        Text(password.map { $0.set ? Copy.mePasswordSet : Copy.mePasswordNone } ?? "")
                    } label: {
                        Label { Text(Copy.mePassword).foregroundStyle(.primary) } icon: { Image(systemName: "key").foregroundStyle(Theme.accent) }
                    }
                }
                .foregroundStyle(.primary)
                .disabled(password == nil)
                LabeledContent {
                    Text(email ?? Copy.meEmailNone).lineLimit(1).truncationMode(.middle)
                } label: {
                    Label(Copy.meEmail, systemImage: "envelope")
                }
                Toggle(isOn: $faceUnlock) { Label(Copy.meFaceUnlock, systemImage: "faceid") }
                    .onChange(of: faceUnlock) { _, on in confirmFaceUnlock(on) }
                Button { helping = true } label: {
                    Label {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(Copy.meHelpSignIn)
                            Text(Copy.meHelpSignInHint).font(.footnote).foregroundStyle(.secondary)
                        }
                    } icon: {
                        Image(systemName: "qrcode.viewfinder").foregroundStyle(Theme.accent)
                    }
                }
                .foregroundStyle(.primary)
            }

            // M3 adds the notification settings row here.
            MeNotificationsSlot()

            Section {
                if let devices {
                    ForEach(devices, id: \.id) { device in
                        DeviceRow(device: device)
                            .swipeActions(edge: .trailing) {
                                if !device.current {
                                    Button(Copy.meSignOutDevice, role: .destructive) { leaving = device }
                                }
                            }
                            .contextMenu {
                                if !device.current {
                                    Button(Copy.meSignOutDevice, systemImage: "rectangle.portrait.and.arrow.right", role: .destructive) { leaving = device }
                                }
                            }
                    }
                } else if devicesFailed {
                    Button(Copy.retry) { Task { await load() } }
                } else {
                    ProgressView().frame(maxWidth: .infinity)
                }
                if let failure { FailureLine(text: failure) }
            } header: {
                Text(Copy.meDevices)
            } footer: {
                Text(devicesFailed ? Copy.meDevicesFailed : Copy.meDevicesHint)
            }

            Section {
                Button(Copy.signOut, role: .destructive) { askSignOut = true }
                    .frame(maxWidth: .infinity)
            }
        }
        .navigationTitle(Copy.me)
        .navigationBarTitleDisplayMode(.inline)
        .task { await load() }
        .refreshable { await load() }
        .sheet(isPresented: $editingPassword) {
            SetPasswordScreen(changing: password?.set == true) { Task { password = await model.passwordState() } }
                .overlay(alignment: .bottom) {
                    if email == nil { Text(Copy.passwordNeedsEmail).font(.footnote).foregroundStyle(.secondary).padding(24) }
                }
        }
        .sheet(isPresented: $helping, onDismiss: { Task { await load() } }) { HelpSignInSheet() }
        .confirmationDialog(leaving.map { Copy.meSignOutDeviceTitle(DeviceRow.name($0)) } ?? "", isPresented: Binding(get: { leaving != nil }, set: { if !$0 { leaving = nil } }), titleVisibility: .visible) {
            Button(Copy.meSignOutDevice, role: .destructive) {
                if let device = leaving { signOut(device) }
            }
        } message: {
            Text(Copy.meSignOutDeviceBody)
        }
        .confirmationDialog(Copy.meSignOutTitle, isPresented: $askSignOut, titleVisibility: .visible) {
            Button(Copy.signOut, role: .destructive) { model.signOut() }
        } message: {
            Text(Copy.meSignOutBody)
        }
    }

    private func load() async {
        async let list = model.devices()
        async let pw = model.passwordState()
        async let mail = model.signInEmail()
        switch await list {
        case .success(let items):
            devices = items
            devicesFailed = false
        case .failure:
            devicesFailed = devices == nil
        }
        password = await pw
        email = await mail
    }

    private func signOut(_ device: DeviceItem) {
        leaving = nil
        failure = nil
        withAnimation { devices?.removeAll { $0.id == device.id } }
        Task {
            if let problem = await model.signOutDevice(device.id) {
                failure = problem
                await load()
            }
        }
    }

    /// Turning 面容解锁 on asks once, so a phone without Face ID or passcode never locks itself out.
    private func confirmFaceUnlock(_ on: Bool) {
        guard on else { return }
        Task {
            if !(await AppModel.authenticateOwner(reason: Copy.unlockReason)) { faceUnlock = false }
        }
    }
}

/// The 通知 row slot: M3 (notifications) fills it; until then it shows nothing.
struct MeNotificationsSlot: View {
    var body: some View { EmptyView() }
}

private struct DeviceRow: View {
    let device: DeviceItem

    static func name(_ device: DeviceItem) -> String {
        let n = device.name.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        return n.isEmpty ? Copy.meUnnamedDevice : n
    }

    private var symbol: String {
        switch device.kind {
        case "phone": "iphone"
        case "desktop": "desktopcomputer"
        default: "laptopcomputer"
        }
    }

    private var used: String {
        if device.current { return Copy.meThisDevice }
        guard let at = device.lastUsedAt else { return Copy.meNeverUsed }
        let date = Date(timeIntervalSince1970: TimeInterval(at) / 1000)
        return Copy.meDeviceUsed(date.formatted(.relative(presentation: .named)))
    }

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: symbol).font(.title3).foregroundStyle(.secondary).frame(width: 32)
            VStack(alignment: .leading, spacing: 2) {
                Text(DeviceRow.name(device)).lineLimit(1)
                Text(used).font(.footnote).foregroundStyle(device.current ? Theme.accent : .secondary)
            }
            Spacer(minLength: 0)
            if device.current {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(Theme.accent)
            }
        }
        .padding(.vertical, 2)
    }
}

/// 帮另一台设备登录: scan (or paste) the code another device shows, see its name, allow it.
private struct HelpSignInSheet: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var typed = ""
    @State private var typing = !CodeScanner.available
    @State private var code: String?
    @State private var device: String?
    @State private var done = false
    @State private var busy = false
    @State private var failure: String?
    @State private var cameraProblem: String?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if done, let device {
                        Label(Copy.approveDone(device), systemImage: "checkmark.circle.fill")
                            .font(.headline).foregroundStyle(Theme.accent)
                        PrimaryButton(title: Copy.done) { dismiss() }
                    } else if let device, let code {
                        Text(Copy.approveTitle).font(.title2.bold())
                        Text(Copy.approveBody(device)).font(.subheadline).foregroundStyle(.secondary)
                        if let failure { FailureLine(text: failure) }
                        PrimaryButton(title: Copy.approveAllow, busy: busy) { approve(code) }
                    } else {
                        Text(Copy.meHelpSignInHint).font(.subheadline).foregroundStyle(.secondary)
                        if !typing {
                            CodeScanner(onCode: peek, onProblem: { cameraProblem = $0 })
                                .aspectRatio(1, contentMode: .fit)
                                .clipShape(RoundedRectangle(cornerRadius: 24))
                        }
                        if let cameraProblem { FailureLine(text: cameraProblem) }
                        if typing {
                            TextField(Copy.scanCodeField, text: $typed)
                                .textInputAutocapitalization(.characters).autocorrectionDisabled()
                                .font(.title3.monospaced()).submitLabel(.next).onSubmit { peek(typed) }
                                .modifier(FieldStyle())
                        }
                        if let failure { FailureLine(text: failure) }
                        if typing {
                            PrimaryButton(title: Copy.scanSubmit, busy: busy, enabled: !typed.isEmpty) { peek(typed) }
                        }
                        HStack(spacing: 16) {
                            Button(Copy.scanPaste, systemImage: "doc.on.clipboard") {
                                if let text = UIPasteboard.general.string { typing = true; typed = text; peek(text) }
                            }
                            if !typing { Button(Copy.scanTypeCode, systemImage: "keyboard") { typing = true } }
                        }
                        .font(.subheadline).frame(maxWidth: .infinity, minHeight: 44)
                    }
                }
                .padding(24)
            }
            .background(Theme.page)
            .navigationTitle(Copy.meHelpSignIn)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button(Copy.cancel) { dismiss() } } }
        }
    }

    private func peek(_ text: String) {
        guard !busy else { return }
        guard let parsed = parseSignInCode(text: text) else {
            failure = Copy.scanNotACode
            return
        }
        busy = true
        failure = nil
        Task {
            switch await model.peekSignIn(parsed.code) {
            case .success(let name):
                code = parsed.code
                device = name.isEmpty ? Copy.meUnnamedDevice : name
            case .failure(let problem):
                failure = problem.message
            }
            busy = false
        }
    }

    private func approve(_ code: String) {
        busy = true
        failure = nil
        Task {
            failure = await model.approveSignIn(code)
            busy = false
            if failure == nil { withAnimation { done = true } }
        }
    }
}

/// 面容解锁: covers the app until the owner unlocks it, on launch and when it comes back.
struct FaceLock: ViewModifier {
    @AppStorage(AppModel.faceUnlockKey) private var enabled = false
    @Environment(\.scenePhase) private var phase
    @State private var locked = UserDefaults.standard.bool(forKey: AppModel.faceUnlockKey)
    /// Debug builds open 我 or 设置密码 from the launch environment (`TC_OPEN=me`), for screenshots.
    @State private var debugOpen: String? = {
        #if DEBUG
            ProcessInfo.processInfo.environment["TC_OPEN"]
        #else
            nil
        #endif
    }()

    func body(content: Content) -> some View {
        content
            .overlay {
                if enabled && locked {
                    VStack(spacing: 20) {
                        Image(systemName: "lock.fill").font(.system(size: 44)).foregroundStyle(.secondary)
                        Text(Copy.unlockTitle).font(.title3.weight(.semibold))
                        Button(Copy.unlockButton, action: unlock)
                            .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Theme.page)
                    .task { unlock() }
                }
            }
            .onChange(of: phase) { _, now in
                if now == .background && enabled { locked = true }
            }
            .sheet(isPresented: Binding(get: { debugOpen == "me" || debugOpen == "set-password" }, set: { if !$0 { debugOpen = nil } })) {
                if debugOpen == "set-password" {
                    SetPasswordScreen(changing: true) {}
                } else {
                    NavigationStack { MeScreen() }
                }
            }
    }

    private func unlock() {
        Task {
            if await AppModel.authenticateOwner(reason: Copy.unlockReason) { locked = false }
        }
    }
}
