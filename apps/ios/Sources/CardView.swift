import SwiftUI

// MARK: - The card, decoded

/// The parts of a card (Card v2 JSON, or v1) the phone draws. Unknown blocks keep their
/// `fallbackText`; the message's text version stands in when one has none.
struct CardDoc {
    struct Option: Hashable { let id: String; let label: String; let detail: String }
    struct Ref: Hashable { let label: String; let url: URL?; let topic: (group: String, topic: String)?
        static func == (a: Ref, b: Ref) -> Bool { a.label == b.label && a.url == b.url }
        func hash(into h: inout Hasher) { h.combine(label); h.combine(url) }
    }
    struct Confirm { let title: String; let body: String }
    struct Irreversible { let verb: String; let target: String; let consequence: String; let typeToConfirm: String? }

    indirect enum Block {
        case text(String)
        case fields([(String, String)])
        case list([(String, String?)])
        case progress(Double, String?)
        case code(String)
        case divider
        case link(Ref)
        case evidence(label: String, state: String?, detail: String?)
        case metric(label: String, value: String, delta: Double?, better: String?)
        case decision(id: String, options: [Option], multiple: Bool, defaults: [String])
        case input(id: String, kind: String, label: String, placeholder: String, required: Bool, options: [Option], initial: Any?)
        case draft(id: String, label: String, text: String, editable: Bool)
        case confirm(id: String, label: String, yes: String, no: String)
        case group(label: String?, blocks: [Block])
        case unknown(String?)
    }

    struct Action: Identifiable {
        let id: String
        let label: String
        /// `primary`, `danger` or `default`.
        let style: String
        /// `submit`, `cancel`, `reject`, `link`, `press`.
        let kind: String
        let confirm: Confirm?
        let role: String?
        let target: Ref?
        var declines: Bool { kind == "cancel" || kind == "reject" }
    }

    let version: Int
    let is21: Bool
    let rev: UInt32
    /// `ask`, `done`, `working`, `info`; v1 cards read as `info` (or `ask` with buttons).
    let intent: String
    let failed: Bool
    let title: String
    let blocks: [Block]
    let actions: [Action]
    let irreversible: Irreversible?

    init?(json: String) {
        guard let data = json.data(using: .utf8),
            let o = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        else { return nil }
        version = (o["v"] as? Int) ?? 1
        is21 = (o["catalog"] as? String) == "tc.card/2.1"
        rev = UInt32((o["rev"] as? Int) ?? 1)
        title = (o["title"] as? String) ?? ""
        failed = (o["status"] as? String) == "failed"
        let rawBlocks = (o["blocks"] as? [[String: Any]]) ?? []
        // 2.1 containers name their children; draw each child once, inside its container.
        var byId: [String: [String: Any]] = [:]
        var named = Set<String>()
        for b in rawBlocks {
            if let id = b["id"] as? String { byId[id] = b }
            for c in (b["children"] as? [String]) ?? [] { named.insert(c) }
        }
        blocks = rawBlocks.filter { !named.contains(($0["id"] as? String) ?? "") }.map { CardDoc.block($0, byId: byId) }
        actions = ((o["actions"] as? [[String: Any]]) ?? []).map(CardDoc.action)
        let ask = o["ask"] as? [String: Any]
        if let i = ask?["irreversible"] as? [String: Any] {
            irreversible = Irreversible(
                verb: (i["verb"] as? String) ?? "", target: (i["target"] as? String) ?? "",
                consequence: (i["consequence"] as? String) ?? "", typeToConfirm: i["typeToConfirm"] as? String)
        } else {
            irreversible = nil
        }
        if version == 1 {
            intent = actions.isEmpty ? "info" : "ask"
        } else {
            intent = (o["intent"] as? String) ?? "info"
        }
    }

    private static func options(_ v: Any?) -> [Option] {
        ((v as? [[String: Any]]) ?? []).compactMap {
            guard let id = $0["id"] as? String else { return nil }
            return Option(id: id, label: ($0["label"] as? String) ?? id, detail: ($0["detail"] as? String) ?? "")
        }
    }

    private static func ref(_ label: String, _ target: [String: Any]?) -> Ref {
        let kind = target?["kind"] as? String
        if kind == "topic", let g = target?["groupId"] as? String, let t = target?["topicId"] as? String {
            return Ref(label: label, url: nil, topic: (g, t))
        }
        return Ref(label: label, url: (target?["url"] as? String).flatMap(URL.init(string:)), topic: nil)
    }

    private static func number(_ v: Any?) -> String {
        guard let n = v as? NSNumber else { return "" }
        let d = n.doubleValue
        return d.rounded() == d && abs(d) < 1e15 ? String(Int64(d)) : d.formatted(.number.precision(.fractionLength(0...2)))
    }

    private static func block(_ b: [String: Any], byId: [String: [String: Any]]) -> Block {
        let id = (b["id"] as? String) ?? ""
        let fallback = b["fallbackText"] as? String
        switch b["type"] as? String {
        case "text": return .text((b["text"] as? String) ?? "")
        case "fields":
            let fs = ((b["fields"] as? [[String: Any]]) ?? []).map { (($0["label"] as? String) ?? "", ($0["value"] as? String) ?? "") }
            return .fields(fs)
        case "list":
            return .list(((b["items"] as? [[String: Any]]) ?? []).map { (($0["text"] as? String) ?? "", $0["status"] as? String) })
        case "plan":
            let items = ((b["items"] as? [[String: Any]]) ?? []).map { (($0["text"] as? String) ?? "", $0["state"] as? String) }
            return items.isEmpty ? .unknown(fallback) : .list(items)
        case "progress": return .progress((b["value"] as? Double) ?? 0, b["label"] as? String)
        case "code", "diff": return .code((b["text"] as? String) ?? "")
        case "divider": return .divider
        case "link", "ref":
            return .link(ref((b["label"] as? String) ?? "", b["target"] as? [String: Any]))
        case "evidence":
            return .evidence(label: (b["label"] as? String) ?? "", state: b["state"] as? String, detail: b["detail"] as? String)
        case "metric":
            let unit = (b["unit"] as? String).map { " \($0)" } ?? ""
            return .metric(label: (b["label"] as? String) ?? "", value: number(b["value"]) + unit,
                           delta: (b["delta"] as? NSNumber)?.doubleValue, better: b["better"] as? String)
        case "decision":
            return .decision(id: id, options: options(b["options"]), multiple: (b["multiple"] as? Bool) ?? false,
                             defaults: (b["default"] as? [String]) ?? [])
        case "input":
            return .input(id: id, kind: (b["kind"] as? String) ?? "text", label: (b["label"] as? String) ?? "",
                          placeholder: (b["placeholder"] as? String) ?? "", required: (b["required"] as? Bool) ?? false,
                          options: options(b["options"]), initial: b["default"])
        case "draft":
            return .draft(id: id, label: (b["label"] as? String) ?? "", text: (b["text"] as? String) ?? "",
                          editable: (b["editable"] as? Bool) ?? false)
        case "confirm":
            return .confirm(id: id, label: (b["label"] as? String) ?? "", yes: (b["yes"] as? String) ?? Copy.cardYes,
                            no: (b["no"] as? String) ?? Copy.cardNo)
        case "group", "stack":
            let inline = (b["blocks"] as? [[String: Any]]) ?? []
            let children = ((b["children"] as? [String]) ?? []).compactMap { byId[$0] }
            let label = (b["label"] as? String) ?? (b["title"] as? String)
            return .group(label: label, blocks: (inline + children).map { block($0, byId: byId) })
        default:
            return .unknown(fallback)
        }
    }

    private static func action(_ a: [String: Any]) -> Action {
        let confirm = (a["confirm"] as? [String: Any]).map { Confirm(title: ($0["title"] as? String) ?? "", body: ($0["body"] as? String) ?? "") }
            ?? (a["confirm"] as? String).map { Confirm(title: $0, body: "") }
        return Action(
            id: (a["id"] as? String) ?? "", label: (a["label"] as? String) ?? "",
            style: (a["style"] as? String) ?? "default", kind: (a["kind"] as? String) ?? "submit",
            confirm: confirm, role: a["role"] as? String,
            target: (a["target"] as? [String: Any]).map { ref((a["label"] as? String) ?? "", $0) })
    }

    /// Some block could not be drawn and says nothing in its place.
    var hasSilentUnknown: Bool {
        func scan(_ bs: [Block]) -> Bool {
            bs.contains {
                switch $0 {
                case .unknown(let text): (text ?? "").isEmpty
                case .group(_, let inner): scan(inner)
                default: false
                }
            }
        }
        return scan(blocks)
    }

    /// An approval: sending it changes something for real, so it is confirmed and Face ID'd.
    func guarded(_ action: Action) -> Bool {
        guard !action.declines, action.kind != "link" else { return false }
        return irreversible != nil || action.style == "danger" || action.role == "approve" || action.id == "approve"
    }
}

// MARK: - The view

/// A message's card: its blocks, then its question (choices, fields, buttons) while it is open
/// for me, or the record of the answer once answered. Two taps answer a choice: the option,
/// then the button. Approvals ask once more and want Face ID.
struct CardView: View {
    @EnvironmentObject private var model: AppModel
    let message: MessageItem
    let json: String

    @State private var record: CardRecord?
    @State private var choices: [String: [String]] = [:]
    @State private var texts: [String: String] = [:]
    @State private var switches: [String: Bool] = [:]
    @State private var drafts: [String: String] = [:]
    @State private var editingDraft: String?
    @State private var missing: String?
    @State private var approving: CardDoc.Action?
    @State private var authFailed = false
    @State private var seeded = false

    private var doc: CardDoc? { CardDoc(json: json) }

    var body: some View {
        if let doc {
            content(doc)
                .onAppear { reload(); seed(doc) }
                .onChange(of: model.topicTicks) { reload() }
                .onChange(of: model.ticks["card:\(message.id)"]) { reload() }
                .sheet(item: $approving) { action in
                    ApproveSheet(doc: doc, action: action) { send(action, doc: doc, approved: true) }
                        .presentationDetents([.medium, .large])
                }
        } else {
            Text(message.body).fixedSize(horizontal: false, vertical: true)
        }
    }

    private var answering: Bool { record?.canAnswer ?? false }

    @ViewBuilder
    private func content(_ doc: CardDoc) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            header(doc)
            ForEach(Array(doc.blocks.enumerated()), id: \.offset) { _, block in
                blockView(block, doc: doc)
            }
            if doc.hasSilentUnknown, !message.body.isEmpty, message.body != doc.title {
                VStack(alignment: .leading, spacing: 4) {
                    Text(Copy.cardOtherwiseText).font(.footnote).foregroundStyle(.secondary)
                    Text(message.body).font(.subheadline).fixedSize(horizontal: false, vertical: true)
                }
            }
            if let missing {
                Text(Copy.cardMissing(missing)).font(.footnote).foregroundStyle(Theme.bad)
            }
            footer(doc)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
        .animation(.snappy(duration: 0.25), value: record)
    }

    @ViewBuilder
    private func header(_ doc: CardDoc) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if let badge = badge(doc) {
                Label(badge.word, systemImage: badge.symbol)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(badge.tint)
            }
            if !doc.title.isEmpty {
                Text(doc.title).font(.headline).fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func badge(_ doc: CardDoc) -> (word: String, symbol: String, tint: Color)? {
        if doc.failed { return (Copy.cardFailed, "xmark.octagon", Theme.bad) }
        switch doc.intent {
        case "ask" where answering || record?.refused == true: return (Copy.cardAsk, "hand.raised", Theme.waiting)
        case "done": return (Copy.cardDone, "checkmark.seal", .green)
        case "working": return (Copy.cardWorking, "circle.dotted", Theme.accent)
        default: return nil
        }
    }

    // MARK: Blocks

    private func blockView(_ block: CardDoc.Block, doc: CardDoc) -> AnyView {
        switch block {
        case .text(let text):
            return AnyView(Text(markdown(text)).font(.subheadline).fixedSize(horizontal: false, vertical: true))
        case .fields(let fields):
            return AnyView(VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(fields.enumerated()), id: \.offset) { _, f in
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        Text(f.0).foregroundStyle(.secondary).frame(width: 96, alignment: .leading)
                        Text(f.1).frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .font(.subheadline)
                }
            })
        case .list(let items):
            return AnyView(VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    Label {
                        Text(item.0).font(.subheadline).fixedSize(horizontal: false, vertical: true)
                    } icon: {
                        Image(systemName: listSymbol(item.1)).foregroundStyle(listTint(item.1))
                    }
                }
            })
        case .progress(let value, let label):
            return AnyView(ProgressView(value: min(max(value, 0), 1)) {
                if let label { Text(label).font(.footnote).foregroundStyle(.secondary) }
            }.tint(Theme.accent))
        case .code(let text):
            return AnyView(ScrollView(.horizontal, showsIndicators: false) {
                Text(text).font(.caption.monospaced()).padding(10)
            }
            .background(Theme.page, in: RoundedRectangle(cornerRadius: 10)))
        case .divider:
            return AnyView(Divider())
        case .link(let ref):
            return AnyView(refView(ref))
        case .evidence(let label, let state, let detail):
            return AnyView(Label {
                VStack(alignment: .leading, spacing: 2) {
                    Text(label).font(.subheadline)
                    if let detail, !detail.isEmpty { Text(detail).font(.footnote).foregroundStyle(.secondary) }
                }
            } icon: {
                Image(systemName: state == "fail" ? "xmark.circle.fill" : state == "pass" ? "checkmark.circle.fill" : "info.circle")
                    .foregroundStyle(state == "fail" ? Theme.bad : state == "pass" ? .green : .secondary)
                    .accessibilityLabel(state == "fail" ? Copy.evidenceFail : state == "pass" ? Copy.evidencePass : "")
            })
        case .metric(let label, let value, let delta, let better):
            return AnyView(HStack(alignment: .firstTextBaseline) {
                Text(label).font(.subheadline).foregroundStyle(.secondary)
                Spacer()
                Text(value).font(.title3.weight(.semibold).monospacedDigit())
                if let delta, delta != 0 {
                    let good = better == nil ? nil : (delta > 0) == (better == "up")
                    Text((delta > 0 ? "+" : "") + delta.formatted(.number.precision(.fractionLength(0...1))) + "%")
                        .font(.footnote.monospacedDigit())
                        .foregroundStyle(good == nil ? Color.secondary : good! ? Color.green : Theme.bad)
                }
            })
        case .decision(let id, let options, let multiple, _):
            return AnyView(VStack(spacing: 8) {
                ForEach(options, id: \.self) { option in
                    optionRow(option, chosen: (choices[id] ?? []).contains(option.id), multiple: multiple) {
                        choose(option.id, in: id, multiple: multiple)
                    }
                }
            }
            .disabled(!answering))
        case .input(let id, let kind, let label, let placeholder, let required, let options, _):
            return AnyView(inputView(id: id, kind: kind, label: label, placeholder: placeholder, required: required, options: options))
        case .draft(let id, let label, let text, let editable):
            return AnyView(draftView(id: id, label: label, text: text, editable: editable))
        case .confirm(let id, let label, let yes, let no):
            return AnyView(VStack(alignment: .leading, spacing: 8) {
                Text(label).font(.subheadline)
                if answering {
                    HStack(spacing: 8) {
                        button(yes, style: "primary") { commit(id, value: true, doc: doc) }
                        button(no, style: "default") { commit(id, value: false, doc: doc) }
                    }
                }
            })
        case .group(let label, let inner):
            return AnyView(VStack(alignment: .leading, spacing: 10) {
                if let label, !label.isEmpty { Text(label).font(.subheadline.weight(.semibold)) }
                ForEach(Array(inner.enumerated()), id: \.offset) { _, b in blockView(b, doc: doc) }
            })
        case .unknown(let text):
            if let text, !text.isEmpty {
                return AnyView(Text(text).font(.subheadline).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true))
            }
            return AnyView(EmptyView())
        }
    }

    private func markdown(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
    }

    private func listSymbol(_ status: String?) -> String {
        switch status {
        case "done": "checkmark.circle.fill"
        case "in_progress", "current": "circle.dotted"
        case "failed": "xmark.circle.fill"
        case "skipped": "minus.circle"
        case "waiting": "hourglass"
        case nil: "circle.fill"
        default: "circle"
        }
    }

    private func listTint(_ status: String?) -> Color {
        switch status {
        case "done": .green
        case "in_progress", "current": Theme.accent
        case "failed": Theme.bad
        case "waiting": Theme.waiting
        default: .secondary
        }
    }

    @ViewBuilder
    private func refView(_ ref: CardDoc.Ref) -> some View {
        if let topic = ref.topic {
            NavigationLink(value: Route.chat(group: topic.group, topic: topic.topic)) { refLabel(ref.label) }
                .buttonStyle(.plain)
        } else if let url = ref.url {
            Link(destination: url) { refLabel(ref.label) }
        } else {
            refLabel(ref.label).foregroundStyle(.secondary)
        }
    }

    private func refLabel(_ label: String) -> some View {
        HStack(spacing: 6) {
            Image(systemName: "link")
            Text(label).lineLimit(2)
            Spacer(minLength: 0)
            Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(.tertiary)
        }
        .font(.subheadline)
        .foregroundStyle(Theme.accent)
        .padding(.horizontal, 12).padding(.vertical, 10)
        .background(Theme.page, in: RoundedRectangle(cornerRadius: 12))
    }

    private func optionRow(_ option: CardDoc.Option, chosen: Bool, multiple: Bool, tap: @escaping () -> Void) -> some View {
        Button(action: tap) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: chosen ? (multiple ? "checkmark.square.fill" : "largecircle.fill.circle") : (multiple ? "square" : "circle"))
                    .foregroundStyle(chosen ? Theme.accent : Color.secondary)
                VStack(alignment: .leading, spacing: 2) {
                    Text(option.label).font(.subheadline).foregroundStyle(.primary)
                    if !option.detail.isEmpty { Text(option.detail).font(.footnote).foregroundStyle(.secondary) }
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 12).padding(.vertical, 11)
            .background(Theme.page, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(chosen ? Theme.accent : .clear, lineWidth: 1.5))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .sensoryFeedback(.selection, trigger: chosen)
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }

    @ViewBuilder
    private func inputView(id: String, kind: String, label: String, placeholder: String, required: Bool, options: [CardDoc.Option]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 4) {
                Text(label).font(.subheadline.weight(.medium))
                if required { Text(Copy.cardRequired).font(.caption).foregroundStyle(.secondary) }
            }
            switch kind {
            case "switch":
                Toggle(isOn: Binding(get: { switches[id] ?? false }, set: { switches[id] = $0 })) { EmptyView() }
                    .labelsHidden().tint(Theme.accent)
            case "select":
                Menu {
                    ForEach(options, id: \.self) { o in Button(o.label) { choices[id] = [o.id] } }
                } label: {
                    HStack {
                        Text(options.first { $0.id == choices[id]?.first }?.label ?? Copy.cardChoose)
                            .foregroundStyle(choices[id]?.isEmpty == false ? Color.primary : Color.secondary)
                        Spacer()
                        Image(systemName: "chevron.up.chevron.down").foregroundStyle(.secondary)
                    }
                    .font(.subheadline)
                    .padding(.horizontal, 12).padding(.vertical, 10)
                    .background(Theme.page, in: RoundedRectangle(cornerRadius: 12))
                }
            case "chips":
                FlowChips(options: options, chosen: Set(choices[id] ?? [])) { choose($0, in: id, multiple: true) }
            default:
                TextField(placeholder.isEmpty ? Copy.cardFill : placeholder,
                          text: Binding(get: { texts[id] ?? "" }, set: { texts[id] = $0 }),
                          axis: kind == "textarea" ? .vertical : .horizontal)
                    .lineLimit(kind == "textarea" ? 3...8 : 1...1)
                    .keyboardType(kind == "number" ? .decimalPad : .default)
                    .font(.subheadline)
                    .padding(.horizontal, 12).padding(.vertical, 10)
                    .background(Theme.page, in: RoundedRectangle(cornerRadius: 12))
            }
        }
        .disabled(!answering)
    }

    @ViewBuilder
    private func draftView(id: String, label: String, text: String, editable: Bool) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                if !label.isEmpty { Text(label).font(.subheadline.weight(.medium)) }
                Spacer()
                if editable && answering {
                    Button(editingDraft == id ? Copy.cardEditDone : Copy.cardEdit) {
                        if editingDraft == id { editingDraft = nil } else {
                            if drafts[id] == nil { drafts[id] = text }
                            editingDraft = id
                        }
                    }
                    .font(.footnote.weight(.semibold)).tint(Theme.accent)
                }
            }
            if editingDraft == id {
                TextField("", text: Binding(get: { drafts[id] ?? text }, set: { drafts[id] = $0 }), axis: .vertical)
                    .lineLimit(3...12)
                    .font(.subheadline)
                    .padding(12)
                    .background(Theme.page, in: RoundedRectangle(cornerRadius: 12))
            } else {
                Text(markdown(drafts[id] ?? text)).font(.subheadline)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(12).frame(maxWidth: .infinity, alignment: .leading)
                    .background(Theme.page, in: RoundedRectangle(cornerRadius: 12))
            }
        }
    }

    // MARK: Footer: buttons, or the record

    @ViewBuilder
    private func footer(_ doc: CardDoc) -> some View {
        if let record {
            if record.refused {
                Text(Copy.cardRefused(model.cardRefusals[message.id] ?? "")).font(.footnote).foregroundStyle(Theme.bad)
            }
            if authFailed {
                Text(Copy.approveFailed).font(.footnote).foregroundStyle(Theme.bad)
            }
            if record.sending {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text(record.actionLabel.isEmpty ? Copy.cardSending : "\(record.actionLabel) · \(Copy.cardSending)")
                }
                .font(.subheadline).foregroundStyle(.secondary)
            } else if record.canAnswer {
                buttons(doc)
            } else if record.state != "open" && record.state != "none" {
                recordLine(record)
            } else if record.state == "open" && doc.intent == "ask" && !doc.actions.isEmpty && !message.isMine {
                Label(Copy.cardNotForMe, systemImage: "hourglass").font(.footnote).foregroundStyle(.secondary)
            }
        }
        let links = doc.actions.filter { $0.kind == "link" && $0.target != nil }
        if !links.isEmpty {
            ForEach(links) { a in if let t = a.target { refView(t) } }
        }
    }

    private func buttons(_ doc: CardDoc) -> some View {
        let actions = doc.actions.filter { $0.kind != "link" && $0.kind != "press" }
        return ViewThatFits(in: .horizontal) {
            HStack(spacing: 8) { ForEach(actions) { a in button(a.label, style: look(a, doc)) { press(a, doc: doc) } } }
            VStack(spacing: 8) { ForEach(actions) { a in button(a.label, style: look(a, doc)) { press(a, doc: doc) } } }
        }
    }

    /// Solid red only for the step that cannot be undone; any other "danger" action (a plain
    /// 拒绝) is a quiet button with red text, so the options of a choice look alike.
    private func look(_ a: CardDoc.Action, _ doc: CardDoc) -> String {
        if a.declines { return "default" }
        if a.style == "danger" { return doc.irreversible != nil && !a.declines ? "danger" : "dangerText" }
        return a.style
    }

    private func button(_ label: String, style: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label).font(.subheadline.weight(.semibold)).lineLimit(1)
                .frame(maxWidth: .infinity, minHeight: 44)
                .foregroundStyle(style == "default" ? Color.primary : style == "dangerText" ? Theme.bad : Theme.onAccent)
                .background(style == "danger" ? Theme.bad : style == "primary" ? Theme.accent : Theme.page, in: Capsule())
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
    }

    private func recordLine(_ r: CardRecord) -> some View {
        let what = r.details.isEmpty ? r.actionLabel : r.details.joined(separator: Copy.nameSeparator)
        let line: String
        switch r.state {
        case "answered":
            line = r.answeredByMe ? Copy.cardAnsweredByMe(what)
                : r.answeredBy.isEmpty ? Copy.cardAnsweredSomeone : Copy.cardAnsweredBy(r.answeredBy, what)
        case "expired": line = Copy.cardExpired
        case "superseded": line = Copy.cardSuperseded
        default: line = Copy.cardClosed
        }
        let symbol = r.state != "answered" ? "clock" : r.declined ? "xmark.circle.fill" : "checkmark.circle.fill"
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: symbol).foregroundStyle(r.state != "answered" ? Color.secondary : r.declined ? Color.secondary : Color.green)
            VStack(alignment: .leading, spacing: 2) {
                Text(line).font(.subheadline.weight(.medium))
                let extra = [r.edited ? Copy.cardEdited : "", r.answeredAt > 0 ? Format.listTime(r.answeredAt) : ""].filter { !$0.isEmpty }
                if !extra.isEmpty { Text(extra.joined(separator: " · ")).font(.footnote).foregroundStyle(.secondary) }
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.page, in: RoundedRectangle(cornerRadius: 12))
        .transition(.opacity.combined(with: .scale(scale: 0.98)))
    }

    // MARK: Answering

    private func reload() {
        let next = model.cardRecord(message.id)
        if next != record { record = next }
    }

    /// The card's defaults (chosen options, input values), once.
    private func seed(_ doc: CardDoc) {
        guard !seeded else { return }
        seeded = true
        func visit(_ bs: [CardDoc.Block]) {
            for b in bs {
                switch b {
                case .decision(let id, _, _, let defaults) where !defaults.isEmpty: choices[id] = defaults
                case .input(let id, let kind, _, _, _, _, let initial):
                    switch (kind, initial) {
                    case ("switch", let v as Bool): switches[id] = v
                    case ("select", let v as String): choices[id] = [v]
                    case ("chips", let v as [String]): choices[id] = v
                    case (_, let v as String): texts[id] = v
                    case (_, let v as NSNumber): texts[id] = v.stringValue
                    default: break
                    }
                case .group(_, let inner): visit(inner)
                default: break
                }
            }
        }
        visit(doc.blocks)
    }

    private func choose(_ option: String, in block: String, multiple: Bool) {
        missing = nil
        if multiple {
            var set = choices[block] ?? []
            if let i = set.firstIndex(of: option) { set.remove(at: i) } else { set.append(option) }
            choices[block] = set
        } else {
            choices[block] = [option]
        }
    }

    private func press(_ action: CardDoc.Action, doc: CardDoc) {
        authFailed = false
        if doc.version == 1 {
            if action.confirm != nil || doc.guarded(action) { approving = action } else { model.pressCard(message.id, action: action.id); reload() }
            return
        }
        if !action.declines, let gap = firstMissing(doc) {
            missing = gap
            return
        }
        if doc.guarded(action) || action.confirm != nil {
            approving = action
        } else {
            send(action, doc: doc, approved: false)
        }
    }

    /// Sends after the confirmation sheet; an approval also wants Face ID first.
    private func send(_ action: CardDoc.Action, doc: CardDoc, approved: Bool) {
        let guarded = approved && doc.guarded(action)
        Task { @MainActor in
            if guarded {
                guard await model.confirmOwner(reason: Copy.approveFaceReason) else {
                    authFailed = true
                    return
                }
            }
            if doc.version == 1 {
                model.pressCard(message.id, action: action.id)
            } else {
                let (inputs, edits) = action.declines ? ([:], [:]) : answer(doc)
                model.answerCard(message.id, rev: doc.rev, action: action.id, inputs: inputs, edits: edits)
            }
            editingDraft = nil
            reload()
        }
    }

    private func commit(_ block: String, value: Bool, doc: CardDoc) {
        model.commitCardBlock(message.id, rev: doc.rev, block: block, event: "choice.submit", value: ["value": value])
        reload()
    }

    /// The label of the first required field that is still empty.
    private func firstMissing(_ doc: CardDoc) -> String? {
        func scan(_ bs: [CardDoc.Block]) -> String? {
            for b in bs {
                switch b {
                case .input(let id, let kind, let label, _, true, _, _):
                    let empty = switch kind {
                    case "switch": false
                    case "select", "chips": (choices[id] ?? []).isEmpty
                    default: (texts[id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    }
                    if empty { return label }
                case .decision(let id, _, _, _) where (choices[id] ?? []).isEmpty:
                    return Copy.cardChoose
                case .group(_, let inner): if let m = scan(inner) { return m }
                default: break
                }
            }
            return nil
        }
        return scan(doc.blocks)
    }

    /// Inputs and edited drafts in the card's catalog shapes: bare values (2.0) or
    /// `{value}` / `{values}` (2.1).
    private func answer(_ doc: CardDoc) -> ([String: Any], [String: String]) {
        var inputs: [String: Any] = [:]
        var edits: [String: String] = [:]
        func one(_ v: Any) -> Any { doc.is21 ? ["value": v] : v }
        func many(_ v: [String]) -> Any { doc.is21 ? ["values": v] : v }
        func visit(_ bs: [CardDoc.Block]) {
            for b in bs {
                switch b {
                case .decision(let id, _, let multiple, _):
                    guard let c = choices[id], !c.isEmpty else { continue }
                    inputs[id] = multiple ? many(c) : one(c[0])
                case .input(let id, let kind, _, _, _, _, _):
                    switch kind {
                    case "switch": inputs[id] = one(switches[id] ?? false)
                    case "select": if let c = choices[id]?.first { inputs[id] = one(c) }
                    case "chips": inputs[id] = many(choices[id] ?? [])
                    case "number":
                        if let t = texts[id], let n = Double(t.replacingOccurrences(of: ",", with: ".")) { inputs[id] = one(n) }
                    default:
                        if let t = texts[id]?.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty { inputs[id] = one(t) }
                    }
                case .draft(let id, _, let text, _):
                    if let d = drafts[id], d != text { edits[id] = d }
                case .group(_, let inner): visit(inner)
                default: break
                }
            }
        }
        visit(doc.blocks)
        return (inputs, edits)
    }
}

/// Options as wrapping chips.
private struct FlowChips: View {
    let options: [CardDoc.Option]
    let chosen: Set<String>
    let tap: (String) -> Void

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 6) { chips }
            VStack(alignment: .leading, spacing: 6) { chips }
        }
    }

    private var chips: some View {
        ForEach(options, id: \.self) { o in
            Button { tap(o.id) } label: {
                Text(o.label).font(.subheadline)
                    .padding(.horizontal, 12).padding(.vertical, 7)
                    .foregroundStyle(chosen.contains(o.id) ? Theme.onAccent : Color.primary)
                    .background(chosen.contains(o.id) ? Theme.accent : Theme.page, in: Capsule())
            }
            .buttonStyle(.plain)
        }
    }
}

// MARK: - Approval sheet

/// One more look before an approval or an action that asks to be confirmed: what happens,
/// to what, and what follows. Some ask to type a word first.
private struct ApproveSheet: View {
    let doc: CardDoc
    let action: CardDoc.Action
    let confirmed: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var typed = ""

    private var word: String? { doc.irreversible?.typeToConfirm }
    private var ready: Bool { word.map { typed.trimmingCharacters(in: .whitespaces) == $0 } ?? true }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(action.confirm?.title.isEmpty == false ? action.confirm!.title : Copy.approveSheetTitle)
                .font(.title3.weight(.semibold))
            if let body = action.confirm?.body, !body.isEmpty {
                Text(body).font(.subheadline).foregroundStyle(.secondary)
            }
            if let i = doc.irreversible {
                VStack(alignment: .leading, spacing: 10) {
                    Text(Copy.approveVerb(i.verb, i.target)).font(.subheadline.weight(.medium))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(Copy.approveConsequence).font(.footnote).foregroundStyle(.secondary)
                        Text(i.consequence).font(.subheadline)
                    }
                }
                .padding(14).frame(maxWidth: .infinity, alignment: .leading)
                .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
            }
            if let word {
                TextField(Copy.approveTypeToConfirm(word), text: $typed)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                    .padding(12).background(Theme.fill, in: RoundedRectangle(cornerRadius: 12))
            }
            Spacer(minLength: 0)
            VStack(spacing: 8) {
                Button {
                    dismiss()
                    confirmed()
                } label: {
                    Label(action.label, systemImage: doc.guarded(action) ? "faceid" : "checkmark")
                        .font(.body.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 50)
                        .foregroundStyle(Theme.onAccent)
                        .background(action.style == "danger" || doc.irreversible != nil ? Theme.bad : Theme.accent, in: Capsule())
                }
                .buttonStyle(.plain)
                .disabled(!ready).opacity(ready ? 1 : 0.4)
                Button(Copy.approveCancel) { dismiss() }
                    .font(.body).frame(maxWidth: .infinity, minHeight: 44)
                    .tint(.secondary)
            }
        }
        .padding(20)
        .background(Theme.page)
    }
}
