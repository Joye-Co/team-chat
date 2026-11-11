import SwiftUI

/// AllowNotify: why notifications help, before the system asks. Shown once, when something
/// first waits on me or after my first message, never on first launch.
struct AllowNotifyScreen: View {
    @ObservedObject var push = PushCenter.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            Image(systemName: "bell.badge")
                .font(.system(size: 44, weight: .regular))
                .foregroundStyle(Theme.accent)
                .accessibilityHidden(true)
                .padding(.top, 32)
            VStack(alignment: .leading, spacing: 8) {
                Text(Copy.allowTitle).font(.title2.weight(.bold))
                Text(Copy.allowBody).font(.body).foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 18) {
                reason("hand.raised", Copy.allowWaiting, Copy.allowWaitingHint, tint: Theme.waiting)
                reason("at", Copy.allowMention, Copy.allowMentionHint, tint: Theme.accent)
                reason("checkmark.circle", Copy.allowResult, Copy.allowResultHint, tint: .green)
            }
            Spacer(minLength: 0)
            Text(Copy.allowFootnote).font(.footnote).foregroundStyle(.secondary)
            VStack(spacing: 8) {
                Button {
                    Task { await push.requestPermission() }
                } label: {
                    Text(Copy.allowTurnOn).font(.headline).frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.borderedProminent)
                .buttonBorderShape(.roundedRectangle(radius: 14))
                Button(Copy.allowLater) { push.putOff() }
                    .frame(maxWidth: .infinity, minHeight: 44)
            }
        }
        .padding(.horizontal, 24).padding(.bottom, 12)
        .background(Theme.page)
        .interactiveDismissDisabled()
    }

    private func reason(_ symbol: String, _ title: String, _ hint: String, tint: Color) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Image(systemName: symbol)
                .font(.system(size: 18, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 36, height: 36)
                .background(tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body.weight(.semibold))
                Text(hint).font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}
