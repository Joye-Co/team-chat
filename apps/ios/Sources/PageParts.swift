import SwiftUI

/// Rows and headers shared by the pages beyond chat.

/// A section title over a group of rows.
struct SectionTitle: View {
    let text: String
    var trailing: String?

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(text).font(.footnote.weight(.semibold)).foregroundStyle(.secondary)
                .accessibilityAddTraits(.isHeader)
            Spacer()
            if let trailing {
                Text(trailing).font(.footnote).foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 12).padding(.top, 20).padding(.bottom, 6)
    }
}

/// A rounded group of rows on the page, like an inset list.
struct Panel<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        VStack(spacing: 0) { content }
            .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
    }
}

/// An avatar with a small dot when the member is online.
struct PresenceAvatar: View {
    let name: String
    let isBot: Bool
    let online: Bool
    var size: CGFloat = 40

    var body: some View {
        Avatar(name: name, round: !isBot, size: size)
            .overlay(alignment: .bottomTrailing) {
                if online {
                    Circle().fill(Theme.online)
                        .frame(width: size * 0.28, height: size * 0.28)
                        .overlay(Circle().stroke(Theme.page, lineWidth: 2))
                        .offset(x: 2, y: 2)
                }
            }
    }
}

/// One 产出 in a list: what it is, where it came from and who made it.
struct ArtifactRow: View {
    let artifact: ArtifactItem
    var showWhere = true

    var body: some View {
        NavigationLink(value: Route.artifact(id: artifact.id)) {
            HStack(spacing: 12) {
                Image(systemName: artifact.isPage ? "doc.richtext" : "doc")
                    .font(.system(size: 18, weight: .medium))
                    .foregroundStyle(Theme.accent)
                    .frame(width: 44, height: 44)
                    .background(Theme.accent.opacity(0.12), in: RoundedRectangle(cornerRadius: 12))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(artifact.title).font(.body.weight(.semibold)).lineLimit(1)
                    Text(subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 8)
                VStack(alignment: .trailing, spacing: 4) {
                    Text(Format.listTime(artifact.updatedAt)).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
                    Text(Copy.artifactVersion(artifact.currentVersion)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 12).frame(minHeight: 64)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowButtonStyle())
    }

    private var subtitle: String {
        let place = showWhere ? [artifact.groupName, artifact.topicTitle] : []
        return (place + [artifact.createdByName]).filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// One run: the member, what it does and how far it is.
struct RunRow: View {
    let run: RunItem
    var showMember = true

    var body: some View {
        NavigationLink(value: Route.run(id: run.id)) {
            HStack(spacing: 12) {
                if showMember {
                    Avatar(name: run.memberName, round: false, size: 36)
                } else {
                    Image(systemName: run.statusSymbol).foregroundStyle(run.active ? Theme.accent : .secondary)
                        .frame(width: 36)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(showMember ? run.memberName : run.topicTitle).font(.subheadline.weight(.semibold)).lineLimit(1)
                    Text(run.line).font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
                    if run.active, let progress = run.progress {
                        ProgressView(value: progress).tint(Theme.accent).padding(.top, 4)
                    }
                }
                Spacer(minLength: 8)
                VStack(alignment: .trailing, spacing: 4) {
                    StatusChip(word: run.statusWord, live: run.active && !run.queued)
                    Text(Format.listTime(run.updatedAt)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 10).frame(minHeight: 60)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowButtonStyle())
    }
}

/// A status word; live work is in the accent color with a moving dot.
struct StatusChip: View {
    let word: String
    var live = false

    var body: some View {
        HStack(spacing: 4) {
            if live {
                Image(systemName: "circle.fill").font(.system(size: 6))
                    .symbolEffect(.pulse, options: .repeating)
                    .accessibilityHidden(true)
            }
            Text(word).font(.caption.weight(.semibold))
        }
        .padding(.horizontal, 8).frame(minHeight: 22)
        .foregroundStyle(live ? Theme.accent : .secondary)
        .background(live ? Theme.accent.opacity(0.12) : Theme.fill, in: Capsule())
    }
}

/// A member in a list: avatar with presence, name, role.
struct MemberLine: View {
    let member: MemberItem
    var detail: String?

    var body: some View {
        NavigationLink(value: Route.member(id: member.id)) {
            HStack(spacing: 12) {
                PresenceAvatar(name: member.name, isBot: member.isBot, online: member.online, size: 40)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(member.name).font(.body.weight(.semibold)).lineLimit(1)
                        if member.isMe {
                            Text(Copy.you).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    let second = detail ?? (member.role.isEmpty ? (member.isBot ? Copy.aiMember : "") : member.role)
                    if !second.isEmpty {
                        Text(second).font(.subheadline).foregroundStyle(detail != nil ? Theme.accent : .secondary).lineLimit(1)
                    }
                }
                Spacer(minLength: 8)
                Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 12).frame(minHeight: 60)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowButtonStyle())
        .accessibilityElement(children: .combine)
        .accessibilityValue(member.online ? Copy.online : Copy.notOnline)
    }
}

extension Theme {
    static let online = Color(uiColor: UIColor { $0.userInterfaceStyle == .dark ? UIColor(red: 0.19, green: 0.82, blue: 0.35, alpha: 1) : UIColor(red: 0.20, green: 0.70, blue: 0.33, alpha: 1) })
    /// The live view's terminal.
    static let terminal = Color(uiColor: UIColor { $0.userInterfaceStyle == .dark ? UIColor(white: 0.07, alpha: 1) : UIColor(red: 0.11, green: 0.12, blue: 0.14, alpha: 1) })
    static let terminalInk = Color(white: 0.88)
    static let terminalDim = Color(white: 0.55)
}
