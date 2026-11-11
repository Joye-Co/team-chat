import SwiftUI

/// SetPassword: set or change my password, with a strength meter. The current password is asked
/// only when one is set (the server lets a device that just signed in skip it).
struct SetPasswordScreen: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    /// A password is set already: this changes it.
    let changing: Bool
    let saved: () -> Void
    @State private var current = ""
    @State private var password = ""
    @State private var again = ""
    @State private var reveal = false
    @State private var busy = false
    @State private var failure: String?
    @State private var askRemove = false
    @FocusState private var focus: Field?

    enum Field { case current, new, again }

    private var strength: PasswordStrength { PasswordStrength(password) }
    private var ready: Bool { password.count >= 8 && again == password && !busy }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(Copy.setPasswordHint).font(.subheadline).foregroundStyle(.secondary)
                    if changing {
                        PasswordField(title: Copy.currentPassword, text: $current, reveal: $reveal, content: .password)
                            .focused($focus, equals: .current)
                            .submitLabel(.next).onSubmit { focus = .new }
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        PasswordField(title: Copy.newPassword, text: $password, reveal: $reveal, content: .newPassword)
                            .focused($focus, equals: .new)
                            .submitLabel(.next).onSubmit { focus = .again }
                        if !password.isEmpty { StrengthMeter(strength: strength) }
                    }
                    PasswordField(title: Copy.repeatPassword, text: $again, reveal: $reveal, content: .newPassword)
                        .focused($focus, equals: .again)
                        .submitLabel(.done).onSubmit(save)
                    if let failure { FailureLine(text: failure) }
                    PrimaryButton(title: Copy.save, busy: busy, enabled: ready, action: save)
                    if changing {
                        Button(Copy.removePassword, role: .destructive) { askRemove = true }
                            .font(.subheadline).frame(maxWidth: .infinity, minHeight: 44)
                    }
                }
                .padding(24)
            }
            .scrollDismissesKeyboard(.interactively)
            .background(Theme.page)
            .navigationTitle(changing ? Copy.changePasswordTitle : Copy.setPasswordTitle)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(Copy.cancel) { dismiss() } }
            }
            .confirmationDialog(Copy.removePasswordConfirm, isPresented: $askRemove, titleVisibility: .visible) {
                Button(Copy.removePassword, role: .destructive, action: remove)
            }
            .onAppear { focus = changing ? .current : .new }
            .onChange(of: password) { failure = nil }
            .onChange(of: again) { failure = nil }
        }
    }

    private func save() {
        guard !busy else { return }
        if password.count < 8 { failure = Copy.passwordTooShort; return }
        if again != password { failure = Copy.passwordsDiffer; return }
        busy = true
        failure = nil
        Task {
            failure = await model.setPassword(password, current: changing && !current.isEmpty ? current : nil)
            busy = false
            if failure == nil { saved(); dismiss() }
        }
    }

    private func remove() {
        busy = true
        Task {
            failure = await model.removePassword(current: current.isEmpty ? nil : current)
            busy = false
            if failure == nil { saved(); dismiss() }
        }
    }
}

/// A rough strength from length and the kinds of characters; never a rule, only a hint.
struct PasswordStrength: Equatable {
    /// 0 太简单 … 3 很好.
    let level: Int

    init(_ password: String) {
        let count = password.count
        var kinds = 0
        if password.contains(where: \.isLowercase) { kinds += 1 }
        if password.contains(where: \.isUppercase) { kinds += 1 }
        if password.contains(where: \.isNumber) { kinds += 1 }
        if password.contains(where: { !$0.isLetter && !$0.isNumber }) || password.unicodeScalars.contains(where: { $0.value > 0x2E7F }) { kinds += 1 }
        let distinct = Set(password).count
        var score = 0
        if count >= 8 { score += 1 }
        if count >= 12 { score += 1 }
        if count >= 16 { score += 1 }
        if kinds >= 2 { score += 1 }
        if kinds >= 3 { score += 1 }
        if distinct <= 3 || count < 8 { score = 0 }
        level = min(3, max(0, score - 1))
    }

    var word: String {
        [Copy.strengthWeak, Copy.strengthFair, Copy.strengthGood, Copy.strengthStrong][level]
    }

    var color: Color {
        [Theme.bad, Theme.waiting, Theme.accent, Color.green][level]
    }
}

private struct StrengthMeter: View {
    let strength: PasswordStrength

    var body: some View {
        HStack(spacing: 10) {
            HStack(spacing: 4) {
                ForEach(0..<4, id: \.self) { i in
                    Capsule().fill(i <= strength.level ? strength.color : Theme.fill).frame(height: 4)
                }
            }
            Text(strength.word).font(.footnote.weight(.medium)).foregroundStyle(strength.color)
                .frame(minWidth: 44, alignment: .trailing)
        }
        .animation(.easeOut(duration: 0.2), value: strength)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(strength.word)
    }
}
