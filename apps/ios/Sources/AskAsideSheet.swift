import SwiftUI
import UIKit

/// The 单独问 item of a message's menu. It shows only where an AI member can be asked.
struct AskAsideMenuItem: View {
    @EnvironmentObject private var model: AppModel
    let messageId: String

    var body: some View {
        if let context = model.messageContext(messageId), !AskAsideSheet.candidates(model, context).isEmpty {
            Button(Copy.askAside, systemImage: "bubble.left.and.text.bubble.right") {
                AskAsideSheet.present(context: context, model: model)
            }
        }
    }
}

/// 单独问: a private question to an AI member about the 话题, read up to the message. The
/// question and the answer stay in my 私聊 with the member; my earlier ones are listed here.
struct AskAsideSheet: View {
    @EnvironmentObject private var model: AppModel
    let context: MessageContext
    let close: () -> Void

    @State private var member: String = ""
    @State private var question = ""
    @State private var sending = false
    @State private var sentTo: String?
    @State private var failed = false
    @State private var asides: [TopicItem] = []
    @FocusState private var focused: Bool

    /// AI members of the message's 群, the message's author first when it is one.
    static func candidates(_ model: AppModel, _ context: MessageContext) -> [MemberItem] {
        let inGroup = model.groupInfo(context.groupId)?.members ?? []
        return inGroup.filter { $0.isBot && !$0.isMe }
            .sorted { a, _ in a.id == context.authorId }
    }

    static func present(context: MessageContext, model: AppModel) {
        guard let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first(where: { $0.activationState == .foregroundActive }),
              var top = scene.keyWindow?.rootViewController else { return }
        while let next = top.presentedViewController { top = next }
        weak var host: UIViewController?
        let sheet = UIHostingController(rootView: AskAsideSheet(context: context, close: { host?.dismiss(animated: true) }).environmentObject(model))
        host = sheet
        sheet.sheetPresentationController?.detents = [.medium(), .large()]
        sheet.sheetPresentationController?.prefersGrabberVisible = true
        top.present(sheet, animated: true)
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    quote
                    let members = AskAsideSheet.candidates(model, context)
                    if members.isEmpty {
                        Text(Copy.askAsideNoMember).font(.subheadline).foregroundStyle(.secondary).padding(.top, 16)
                    } else {
                        if members.count > 1 {
                            SectionTitle(text: Copy.askAsideWho)
                            ScrollView(.horizontal, showsIndicators: false) {
                                HStack(spacing: 8) {
                                    ForEach(members, id: \.id) { m in memberChip(m) }
                                }
                                .padding(.horizontal, 2)
                            }
                        }
                        composer.padding(.top, 16)
                        if let sentTo {
                            Label(Copy.askAsideSent(sentTo), systemImage: "checkmark.circle.fill")
                                .font(.footnote).foregroundStyle(Theme.online).padding(.top, 10)
                        }
                        Text(Copy.askAsideHint).font(.footnote).foregroundStyle(.secondary).padding(.top, 10)
                    }
                    if !asides.isEmpty {
                        SectionTitle(text: Copy.askAsideEarlier)
                        Panel { ForEach(asides, id: \.id) { AsideRow(topic: $0) } }
                    }
                }
                .padding(.horizontal, Theme.gutter).padding(.bottom, 24)
            }
            .background(Theme.page)
            .navigationTitle(Copy.askAside)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(Copy.close, action: close) }
            }
            .navigationDestination(for: Route.self) { RouteView(route: $0) }
            .alert(Copy.askAsideFailed, isPresented: $failed) { Button(Copy.close, role: .cancel) {} }
        }
        .onAppear {
            member = AskAsideSheet.candidates(model, context).first?.id ?? ""
            reload()
            focused = true
        }
        .onChange(of: model.groupTicks) { reload() }
        .onChange(of: model.topicTicks) { reload() }
    }

    private func reload() {
        asides = model.asides(group: context.groupId, topic: context.topicId)
    }

    private var quote: some View {
        HStack(spacing: 10) {
            RoundedRectangle(cornerRadius: 2).fill(Theme.accent).frame(width: 3)
            VStack(alignment: .leading, spacing: 2) {
                Text(model.member(context.authorId)?.name ?? "").font(.footnote.weight(.semibold))
                Text(context.excerpt).font(.footnote).foregroundStyle(.secondary).lineLimit(2)
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(.top, 8)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Copy.about)
    }

    private func memberChip(_ m: MemberItem) -> some View {
        Button { member = m.id } label: {
            HStack(spacing: 6) {
                Avatar(name: m.name, round: false, size: 24)
                Text(m.name).font(.subheadline.weight(.semibold))
            }
            .padding(.leading, 4).padding(.trailing, 12).frame(minHeight: 36)
            .foregroundStyle(member == m.id ? Theme.accent : .primary)
            .background(member == m.id ? Theme.accent.opacity(0.14) : Theme.fill, in: Capsule())
            .overlay(Capsule().stroke(member == m.id ? Theme.accent : .clear, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(member == m.id ? .isSelected : [])
    }

    private var composer: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField(Copy.askAsidePlaceholder, text: $question, axis: .vertical)
                .lineLimit(1...6)
                .focused($focused)
                .padding(.horizontal, 14).padding(.vertical, 11)
                .background(Theme.fill, in: RoundedRectangle(cornerRadius: 20))
            Button { Task { await send() } } label: {
                Image(systemName: "arrow.up").font(.body.weight(.bold))
                    .frame(width: 40, height: 40)
                    .foregroundStyle(Theme.onAccent)
                    .background(canSend ? Theme.accent : Theme.accent.opacity(0.35), in: Circle())
            }
            .disabled(!canSend)
            .accessibilityLabel(Copy.askAsideSend)
        }
    }

    private var canSend: Bool {
        !sending && !member.isEmpty && !question.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func send() async {
        sending = true
        defer { sending = false }
        let text = question.trimmingCharacters(in: .whitespacesAndNewlines)
        if await model.askAside(member: member, about: context, question: text) != nil {
            sentTo = model.member(member)?.name
            question = ""
            reload()
        } else {
            failed = true
        }
    }
}
