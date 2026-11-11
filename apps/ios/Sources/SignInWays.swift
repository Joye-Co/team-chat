import SwiftUI

/// LoginStart: the ways in. Email code first (the one everybody has), then password, scanning
/// the computer's code, an invite code, and asking a computer to let this phone in.
struct LoginStartScreen: View {
    @Binding var server: String
    let go: (SignInFlow.Step) -> Void

    private var asksServer: Bool { Session.defaultServer == nil }
    private var hasServer: Bool { !server.trimmingCharacters(in: .whitespaces).isEmpty }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(Copy.startTitle).font(.largeTitle.bold())
                Text(Copy.startHint).font(.subheadline).foregroundStyle(.secondary)
                if asksServer {
                    TextField(Copy.signInServer, text: $server)
                        .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                        .modifier(FieldStyle())
                }
                VStack(spacing: 0) {
                    way(Copy.wayEmail, Copy.wayEmailHint, "envelope", .email)
                    Divider().padding(.leading, 60)
                    way(Copy.wayPassword, Copy.wayPasswordHint, "key", .password)
                    Divider().padding(.leading, 60)
                    // Scanning works without a server: the computer's code carries it.
                    way(Copy.wayScan, Copy.wayScanHint, "qrcode.viewfinder", .scan, needsServer: false)
                }
                .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
                VStack(spacing: 0) {
                    way(Copy.wayInvite, Copy.wayInviteHint, "ticket", .invite)
                    Divider().padding(.leading, 60)
                    way(Copy.wayAsk, Copy.wayAskHint, "laptopcomputer.and.iphone", .ask)
                }
                .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
                Button(Copy.signInOtherWay) { go(.credential) }
                    .font(.footnote).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 44)
            }
            .padding(24)
        }
        .background(Theme.page)
    }

    private func way(_ title: String, _ hint: String, _ symbol: String, _ step: SignInFlow.Step, needsServer: Bool = true) -> some View {
        Button { go(step) } label: {
            HStack(spacing: 14) {
                Image(systemName: symbol)
                    .font(.body.weight(.medium)).foregroundStyle(Theme.accent)
                    .frame(width: 32, height: 32)
                    .background(Theme.accent.opacity(0.12), in: RoundedRectangle(cornerRadius: 9))
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.body.weight(.medium)).foregroundStyle(.primary)
                    Text(hint).font(.footnote).foregroundStyle(.secondary).multilineTextAlignment(.leading)
                }
                Spacer(minLength: 8)
                Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 14).padding(.vertical, 12)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(needsServer && !hasServer)
        .opacity(needsServer && !hasServer ? 0.45 : 1)
    }
}

/// The full-width capsule button of the sign-in screens, with its spinner.
struct PrimaryButton: View {
    let title: String
    var busy = false
    var enabled = true
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            ZStack {
                Text(title).opacity(busy ? 0 : 1)
                if busy { ProgressView().tint(Theme.onAccent) }
            }
            .font(.headline).frame(maxWidth: .infinity, minHeight: 50)
        }
        .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
        .disabled(!enabled || busy)
    }
}

/// LoginPassword: the sign-in email and the password.
struct PasswordSignInScreen: View {
    @EnvironmentObject private var model: AppModel
    let server: String
    @Binding var email: String
    let go: (SignInFlow.Step) -> Void
    @State private var password = ""
    @State private var reveal = false
    @State private var busy = false
    @State private var failure: String?
    @FocusState private var focus: Field?

    enum Field { case email, password }

    private var ready: Bool { email.contains("@") && !password.isEmpty && !busy }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(Copy.passwordTitle).font(.largeTitle.bold())
                Text(Copy.passwordHint).font(.subheadline).foregroundStyle(.secondary)
                TextField(Copy.signInEmail, text: $email)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                    .keyboardType(.emailAddress).textContentType(.username)
                    .submitLabel(.next).focused($focus, equals: .email)
                    .onSubmit { focus = .password }
                    .modifier(FieldStyle())
                PasswordField(title: Copy.passwordField, text: $password, reveal: $reveal, content: .password)
                    .submitLabel(.go).focused($focus, equals: .password)
                    .onSubmit(submit)
                if let failure { FailureLine(text: failure) }
                PrimaryButton(title: Copy.passwordSubmit, busy: busy, enabled: ready, action: submit)
                Button(Copy.passwordForgot) { go(.email) }
                    .font(.subheadline).frame(maxWidth: .infinity, minHeight: 44)
            }
            .padding(24)
        }
        .scrollDismissesKeyboard(.interactively)
        .background(Theme.page)
        .onAppear { focus = email.isEmpty ? .email : .password }
        .onChange(of: password) { failure = nil }
    }

    private func submit() {
        guard ready else { return }
        busy = true
        failure = nil
        let address = email.trimmingCharacters(in: .whitespaces)
        Task {
            failure = await model.signIn(server: server, email: address, password: password, device: DeviceName.current)
            busy = false
        }
    }
}

/// A password field with a show / hide eye.
struct PasswordField: View {
    let title: String
    @Binding var text: String
    @Binding var reveal: Bool
    var content: UITextContentType = .password

    var body: some View {
        HStack(spacing: 8) {
            Group {
                if reveal {
                    TextField(title, text: $text)
                } else {
                    SecureField(title, text: $text)
                }
            }
            .textInputAutocapitalization(.never).autocorrectionDisabled()
            .textContentType(content)
            Button { reveal.toggle() } label: {
                Image(systemName: reveal ? "eye.slash" : "eye").foregroundStyle(.secondary)
                    .frame(width: 32, height: 32)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(reveal ? Copy.passwordHide : Copy.passwordShow)
        }
        .modifier(FieldStyle())
    }
}

/// 邀请码: joining for the first time, or signing in with a code an owner made.
struct InviteScreen: View {
    @EnvironmentObject private var model: AppModel
    let server: String
    @State private var code = ""
    @State private var busy = false
    @State private var failure: String?
    @FocusState private var focused: Bool

    private var clean: String { code.filter { $0.isLetter || $0.isNumber } }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(Copy.inviteTitle).font(.largeTitle.bold())
                Text(Copy.inviteHint).font(.subheadline).foregroundStyle(.secondary)
                TextField(Copy.inviteField, text: $code)
                    .textInputAutocapitalization(.characters).autocorrectionDisabled()
                    .font(.title3.monospaced()).focused($focused)
                    .submitLabel(.join).onSubmit(submit)
                    .modifier(FieldStyle())
                if let failure { FailureLine(text: failure) }
                PrimaryButton(title: Copy.inviteSubmit, busy: busy, enabled: clean.count >= 8, action: submit)
            }
            .padding(24)
        }
        .background(Theme.page)
        .onAppear { focused = true }
        .onChange(of: code) { failure = nil }
    }

    private func submit() {
        guard clean.count >= 8, !busy else { return }
        busy = true
        failure = nil
        Task {
            failure = await model.join(server: server, invite: code, device: DeviceName.current)
            busy = false
        }
    }
}

/// LoginScan: the camera reads the code a signed-in computer shows; without a camera (the
/// simulator) the code is pasted or typed.
struct ScanSignInScreen: View {
    @EnvironmentObject private var model: AppModel
    @Binding var server: String
    @State private var typing = !CodeScanner.available
    @State private var typed = ""
    @State private var busy = false
    @State private var failure: String?
    @State private var cameraProblem: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(Copy.scanTitle).font(.largeTitle.bold())
                Text(Copy.scanHint).font(.subheadline).foregroundStyle(.secondary)
                if !typing {
                    ZStack {
                        CodeScanner(onCode: use, onProblem: { cameraProblem = $0 })
                        ScanFrame()
                        if busy {
                            Color.black.opacity(0.4)
                            ProgressView(Copy.scanSigningIn).tint(.white).foregroundStyle(.white)
                        }
                    }
                    .aspectRatio(1, contentMode: .fit)
                    .clipShape(RoundedRectangle(cornerRadius: 24))
                }
                if let cameraProblem { FailureLine(text: cameraProblem) }
                if typing {
                    TextField(Copy.scanCodeField, text: $typed)
                        .textInputAutocapitalization(.characters).autocorrectionDisabled()
                        .font(.title3.monospaced())
                        .submitLabel(.go).onSubmit { use(typed) }
                        .modifier(FieldStyle())
                }
                if let failure { FailureLine(text: failure) }
                if typing {
                    PrimaryButton(title: Copy.scanSubmit, busy: busy, enabled: !typed.isEmpty) { use(typed) }
                }
                HStack(spacing: 16) {
                    Button(Copy.scanPaste, systemImage: "doc.on.clipboard") {
                        if let text = UIPasteboard.general.string { typing = true; typed = text; use(text) }
                    }
                    if !typing {
                        Button(Copy.scanTypeCode, systemImage: "keyboard") { typing = true }
                    }
                }
                .font(.subheadline).frame(maxWidth: .infinity, minHeight: 44)
            }
            .padding(24)
        }
        .background(Theme.page)
        .onChange(of: typed) { failure = nil }
        .onAppear {
            #if DEBUG
                // Screenshots and end-to-end checks hand the code in at launch (`TC_CODE`).
                if let code = ProcessInfo.processInfo.environment["TC_CODE"], !code.isEmpty {
                    typing = true
                    typed = code
                    use(code)
                }
            #endif
        }
    }

    /// Reads a scanned or typed text and signs in with it.
    private func use(_ text: String) {
        guard !busy else { return }
        guard let parsed = parseSignInCode(text: text) else {
            failure = Copy.scanNotACode
            return
        }
        let target = parsed.server ?? server
        guard !target.isEmpty else {
            failure = Copy.failServer
            return
        }
        busy = true
        failure = nil
        Task {
            failure = await model.claim(server: target, code: parsed.code, device: DeviceName.current)
            busy = false
            if failure == nil { server = target }
        }
    }
}

/// The corners of the scan window.
private struct ScanFrame: View {
    var body: some View {
        GeometryReader { geo in
            let side = geo.size.width * 0.62
            let inset = (geo.size.width - side) / 2
            Path { p in
                let l: CGFloat = 28
                for (x, y, dx, dy) in [(inset, inset, 1.0, 1.0), (inset + side, inset, -1.0, 1.0), (inset, inset + side, 1.0, -1.0), (inset + side, inset + side, -1.0, -1.0)] {
                    p.move(to: CGPoint(x: x, y: y + dy * l))
                    p.addLine(to: CGPoint(x: x, y: y))
                    p.addLine(to: CGPoint(x: x + dx * l, y: y))
                }
            }
            .stroke(.white, style: StrokeStyle(lineWidth: 4, lineCap: .round, lineJoin: .round))
        }
        .allowsHitTesting(false)
    }
}

/// 让电脑帮我登录: this phone shows a code; a signed-in computer types it and approves.
struct AskSignInScreen: View {
    @EnvironmentObject private var model: AppModel
    let server: String
    @State private var request: SignInRequest?
    @State private var expired = false
    @State private var failure: String?
    @State private var run = 0

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(Copy.askTitle).font(.largeTitle.bold())
                Text(Copy.askHint).font(.subheadline).foregroundStyle(.secondary)
                VStack(spacing: 14) {
                    if let request {
                        Text(request.code)
                            .font(.system(size: 40, weight: .semibold, design: .monospaced))
                            .foregroundStyle(expired ? .tertiary : .primary)
                            .textSelection(.enabled)
                            .contextMenu { Button(Copy.askCopy, systemImage: "doc.on.doc") { UIPasteboard.general.string = request.code } }
                        if expired {
                            Text(Copy.askExpired).font(.subheadline).foregroundStyle(.secondary)
                            Button(Copy.askRenew) { start() }.font(.subheadline.weight(.medium))
                        } else {
                            Label(Copy.askWaiting, systemImage: "hourglass").font(.subheadline).foregroundStyle(.secondary)
                        }
                    } else if failure == nil {
                        ProgressView().frame(height: 48)
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 160)
                .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
                if let failure {
                    FailureLine(text: failure)
                    Button(Copy.retry) { start() }.font(.subheadline)
                }
            }
            .padding(24)
        }
        .background(Theme.page)
        .task(id: run) { await poll() }
        .onAppear { if request == nil { start() } }
        .onDisappear { run += 1; request = nil }
    }

    private func start() {
        expired = false
        failure = nil
        request = nil
        Task {
            switch await model.askSignIn(server: server, device: DeviceName.current) {
            case .success(let r):
                request = r
                run += 1
            case .failure(let e):
                failure = AppModel.message(for: e)
            }
        }
    }

    /// Every 2 seconds while the code shows.
    private func poll() async {
        guard let request else { return }
        while !Task.isCancelled && !expired {
            try? await Task.sleep(for: .seconds(2))
            if Task.isCancelled { return }
            switch await model.pollAsk(server: server, request: request) {
            case .expired: expired = true
            case .approved: return
            default: continue
            }
        }
    }
}
