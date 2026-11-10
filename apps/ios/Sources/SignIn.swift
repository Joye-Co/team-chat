import SwiftUI

/// Sign-in: the email, then the 6-digit code. A build without a server address asks for it.
struct SignInFlow: View {
    @EnvironmentObject private var model: AppModel
    @State private var server = Session.defaultServer ?? ""
    @State private var email = ""
    @State private var path: [Step] = []

    enum Step: Hashable {
        case code(email: String)
        case credential
    }

    var body: some View {
        NavigationStack(path: $path) {
            EmailScreen(server: $server, email: $email) { path.append($0) }
                .navigationDestination(for: Step.self) { step in
                    switch step {
                    case .code(let email): CodeScreen(server: server, email: email)
                    case .credential: CredentialScreen(server: $server)
                    }
                }
        }
    }
}

private struct EmailScreen: View {
    @EnvironmentObject private var model: AppModel
    @Binding var server: String
    @Binding var email: String
    let go: (SignInFlow.Step) -> Void
    @State private var busy = false
    @State private var failure: String?
    @FocusState private var focused: Bool

    private var asksServer: Bool { Session.defaultServer == nil }
    private var ready: Bool { email.contains("@") && !server.isEmpty && !busy }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(Copy.signInTitle).font(.largeTitle.bold())
                Text(Copy.signInEmailHint).font(.subheadline).foregroundStyle(.secondary)
                if asksServer {
                    TextField(Copy.signInServer, text: $server)
                        .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                        .modifier(FieldStyle())
                }
                TextField(Copy.signInEmail, text: $email)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                    .keyboardType(.emailAddress).textContentType(.emailAddress)
                    .submitLabel(.continue).focused($focused)
                    .onSubmit(send)
                    .modifier(FieldStyle())
                if let failure { FailureLine(text: failure) }
                Button(action: send) {
                    ZStack {
                        Text(Copy.signInSendCode).opacity(busy ? 0 : 1)
                        if busy { ProgressView().tint(Theme.onAccent) }
                    }
                    .font(.headline).frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
                .disabled(!ready)
                Button(Copy.signInOtherWay) { go(.credential) }
                    .font(.subheadline).frame(maxWidth: .infinity, minHeight: 44)
            }
            .padding(24)
        }
        .scrollDismissesKeyboard(.interactively)
        .background(Theme.page)
        .onAppear { focused = !asksServer }
    }

    private func send() {
        guard ready else { return }
        busy = true
        failure = nil
        let address = email.trimmingCharacters(in: .whitespaces)
        Task {
            failure = await model.requestCode(server: server, email: address)
            busy = false
            if failure == nil { go(.code(email: address)) }
        }
    }
}

private struct CodeScreen: View {
    @EnvironmentObject private var model: AppModel
    let server: String
    let email: String
    @State private var code = ""
    @State private var busy = false
    @State private var failure: String?
    @State private var resendAt = Date.now.addingTimeInterval(60)
    @FocusState private var focused: Bool

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(Copy.codeTitle).font(.largeTitle.bold())
                Text(Copy.codeSentTo(email)).font(.subheadline).foregroundStyle(.secondary)
                digits
                if let failure { FailureLine(text: failure) }
                Button(action: submit) {
                    ZStack {
                        Text(Copy.codeSubmit).opacity(busy ? 0 : 1)
                        if busy { ProgressView().tint(Theme.onAccent) }
                    }
                    .font(.headline).frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
                .disabled(code.count != 6 || busy)
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    let left = Int(resendAt.timeIntervalSince(context.date).rounded(.up))
                    Group {
                        if left > 0 {
                            Text(Copy.codeResendIn(left)).foregroundStyle(.secondary)
                        } else {
                            Button(Copy.codeResend, action: resend)
                        }
                    }
                    .font(.subheadline.monospacedDigit()).frame(maxWidth: .infinity, minHeight: 44)
                }
            }
            .padding(24)
        }
        .scrollDismissesKeyboard(.interactively)
        .background(Theme.page)
        .onAppear { focused = true }
    }

    /// Six boxes drawn over one hidden field, so paste and the keyboard's code suggestion work.
    private var digits: some View {
        ZStack {
            TextField(Copy.codeField, text: $code)
                .keyboardType(.numberPad).textContentType(.oneTimeCode)
                .focused($focused)
                .opacity(0.02)
                .onChange(of: code) { _, now in
                    let clean = String(now.filter(\.isNumber).prefix(6))
                    if clean != now { code = clean }
                    if !clean.isEmpty { failure = nil }
                    if clean.count == 6 { submit() }
                }
            HStack(spacing: 8) {
                ForEach(0..<6, id: \.self) { index in
                    let digit = index < code.count ? String(Array(code)[index]) : ""
                    Text(digit).font(.title2.weight(.semibold).monospacedDigit())
                        .frame(maxWidth: .infinity, minHeight: 56)
                        .background(Theme.fill, in: RoundedRectangle(cornerRadius: 14))
                        .overlay {
                            if focused && index == min(code.count, 5) {
                                RoundedRectangle(cornerRadius: 14).strokeBorder(Theme.accent, lineWidth: 2)
                            }
                        }
                }
            }
            .allowsHitTesting(false)
            .accessibilityHidden(true)
        }
        .contentShape(Rectangle())
        .onTapGesture { focused = true }
    }

    private func submit() {
        guard code.count == 6, !busy else { return }
        busy = true
        failure = nil
        Task {
            failure = await model.signIn(server: server, email: email, code: code, device: UIDevice.current.name)
            busy = false
            if failure != nil { code = "" }
        }
    }

    private func resend() {
        resendAt = .now.addingTimeInterval(60)
        Task { failure = await model.requestCode(server: server, email: email) }
    }
}

/// Development sign-in with a server address and a credential.
private struct CredentialScreen: View {
    @EnvironmentObject private var model: AppModel
    @Binding var server: String
    @State private var credential = ""
    @State private var failure: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(Copy.signInTitle).font(.largeTitle.bold())
                Text(Copy.signInDevHint).font(.subheadline).foregroundStyle(.secondary)
                TextField(Copy.signInServer, text: $server)
                    .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                    .modifier(FieldStyle())
                SecureField(Copy.signInCredential, text: $credential).modifier(FieldStyle())
                if let failure { FailureLine(text: failure) }
                Button {
                    failure = model.signIn(server: server, token: credential)
                } label: {
                    Text(Copy.signInContinue).font(.headline).frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
                .disabled(server.isEmpty || credential.isEmpty)
            }
            .padding(24)
        }
        .background(Theme.page)
    }
}

private struct FieldStyle: ViewModifier {
    func body(content: Content) -> some View {
        content.padding(.horizontal, 16).frame(minHeight: 52)
            .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
    }
}

private struct FailureLine: View {
    let text: String

    var body: some View {
        Label(text, systemImage: "exclamationmark.circle.fill")
            .font(.footnote).foregroundStyle(Theme.bad)
            .transition(.opacity)
    }
}
