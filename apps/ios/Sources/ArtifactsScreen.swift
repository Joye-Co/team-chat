import QuickLook
import SwiftUI
import WebKit

/// 产出 tab: every 产出 of my 群, newest change first, narrowed to one 群 from the title menu.
struct ArtifactsScreen: View {
    @EnvironmentObject private var model: AppModel
    @State private var group: String?
    @State private var items: [ArtifactItem] = []

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(spacing: 2) {
                    ForEach(items, id: \.id) { ArtifactRow(artifact: $0, showWhere: true) }
                }
                .padding(.horizontal, Theme.gutter).padding(.vertical, 8)
            }
            .background(Theme.page)
            .overlay {
                if items.isEmpty {
                    ContentUnavailableView(Copy.artifactsEmpty, systemImage: "square.stack", description: Text(Copy.artifactsEmptyHint))
                }
            }
            .refreshable { model.refreshArtifacts(group: group) }
            .navigationTitle(title)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { groupMenu }
            }
            .navigationDestination(for: Route.self) { RouteView(route: $0) }
        }
        .onAppear {
            reload()
            model.refreshArtifacts()
        }
        .onChange(of: model.artifactsTick) { reload() }
        .onChange(of: group) { reload() }
    }

    private var title: String {
        group.flatMap { model.group($0)?.name } ?? Copy.artifactsTitle
    }

    private var groupMenu: some View {
        Menu {
            Picker(Copy.allGroups, selection: $group) {
                Text(Copy.allGroups).tag(String?.none)
                ForEach(model.groups.filter { !$0.isDm }, id: \.id) { g in
                    Text(g.name).tag(Optional(g.id))
                }
            }
        } label: {
            Image(systemName: group == nil ? "line.3.horizontal.decrease.circle" : "line.3.horizontal.decrease.circle.fill")
                .accessibilityLabel(Copy.allGroups)
        }
    }

    private func reload() {
        items = model.artifacts(group: group)
    }
}

/// One 产出: a page in an isolated web view, or a file in the system preview. The version
/// menu opens older versions; 来龙去脉 says where it came from.
struct ArtifactScreen: View {
    @EnvironmentObject private var model: AppModel
    let artifactId: String

    @State private var detail: ArtifactDetail?
    @State private var version: UInt32?
    @State private var opened: OpenedVersion?
    @State private var failed = false
    @State private var preview: URL?
    @State private var showOrigin = false

    var body: some View {
        content
            .background(Theme.page)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .principal) {
                    VStack(spacing: 0) {
                        Text(detail?.artifact.title ?? "").font(.headline).lineLimit(1)
                        if let shown {
                            Text(Copy.artifactVersionLong(shown) + (shown == detail?.artifact.currentVersion ? " · " + Copy.artifactCurrent : ""))
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        if let versions = detail?.versions, versions.count > 1 {
                            Picker(Copy.artifactVersions, selection: $version) {
                                ForEach(versions, id: \.version) { v in
                                    Text(Copy.artifactVersionLong(v.version)).tag(Optional(v.version))
                                }
                            }
                        }
                        Button(Copy.artifactOrigin, systemImage: "info.circle") { showOrigin = true }
                        if let detail {
                            NavigationLink(value: Route.chat(group: detail.artifact.groupId, topic: detail.artifact.topicId)) {
                                Label(Copy.openTopic, systemImage: "number")
                            }
                            NavigationLink(value: Route.topicInfo(group: detail.artifact.groupId, topic: detail.artifact.topicId)) {
                                Label(Copy.details, systemImage: "info.circle")
                            }
                        }
                        if let opened, !opened.isPage {
                            Button(Copy.artifactOpenFile, systemImage: "eye") { preview = URL(fileURLWithPath: opened.entry) }
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle").accessibilityLabel(Copy.more)
                    }
                }
            }
            .quickLookPreview($preview)
            .sheet(isPresented: $showOrigin) {
                if let detail { ArtifactOrigin(detail: detail).presentationDetents([.medium, .large]) }
            }
            .task { await load() }
            .onChange(of: version) { Task { await open() } }
            .onChange(of: model.ticks["artifacts:\(detail?.artifact.topicId ?? "")"]) {
                detail = model.artifact(artifactId) ?? detail
            }
    }

    private var shown: UInt32? { opened?.version ?? version ?? detail?.artifact.currentVersion }

    @ViewBuilder
    private var content: some View {
        if let opened, opened.isPage {
            ArtifactWebView(entry: URL(fileURLWithPath: opened.entry), folder: URL(fileURLWithPath: opened.dir, isDirectory: true))
                .ignoresSafeArea(edges: .bottom)
        } else if let opened {
            FileCard(name: (opened.entry as NSString).lastPathComponent, version: currentVersionItem) {
                preview = URL(fileURLWithPath: opened.entry)
            }
        } else if failed {
            ContentUnavailableView {
                Label(Copy.artifactFailed, systemImage: "exclamationmark.triangle")
            } description: {
                Text(Copy.artifactFailedHint)
            } actions: {
                Button(Copy.tryAgain) { Task { await load() } }.buttonStyle(.bordered)
            }
        } else {
            ProgressView(Copy.artifactOpening).frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var currentVersionItem: ArtifactVersionItem? {
        detail?.versions.first { $0.version == shown }
    }

    private func load() async {
        failed = false
        detail = model.artifact(artifactId)
        if let fresh = await model.loadArtifact(artifactId) { detail = fresh }
        await open()
    }

    private func open() async {
        failed = false
        if let v = await model.openVersion(artifactId, version: version) {
            opened = v
            if version == nil { version = v.version }
        } else {
            opened = nil
            failed = true
        }
    }
}

/// A file 产出: its name and size, and a button to preview it.
private struct FileCard: View {
    let name: String
    let version: ArtifactVersionItem?
    let open: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "doc.text").font(.system(size: 44, weight: .light)).foregroundStyle(Theme.accent)
                .frame(width: 96, height: 96)
                .background(Theme.accent.opacity(0.12), in: RoundedRectangle(cornerRadius: 24))
            VStack(spacing: 4) {
                Text(name).font(.headline).multilineTextAlignment(.center)
                if let version {
                    Text([Format.size(version.size), version.fileCount > 1 ? Copy.fileCount(version.fileCount) : ""]
                        .filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.subheadline).foregroundStyle(.secondary)
                }
            }
            Button(action: open) {
                Text(Copy.artifactOpenFile).font(.body.weight(.semibold)).frame(maxWidth: 240).frame(minHeight: 44)
            }
            .buttonStyle(.borderedProminent).tint(Theme.accent)
        }
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// 来龙去脉: the 话题, who made it and the versions.
private struct ArtifactOrigin: View {
    let detail: ArtifactDetail
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    LabeledContent(Copy.tabGroups, value: detail.artifact.groupName)
                    if !detail.artifact.topicTitle.isEmpty {
                        LabeledContent(Copy.topics, value: detail.artifact.topicTitle)
                    }
                    LabeledContent(Copy.membersTitle, value: Copy.artifactMadeBy(detail.artifact.createdByName))
                    LabeledContent(detail.artifact.isPage ? Copy.artifactPage : Copy.artifactFile, value: Format.day(detail.artifact.updatedAt))
                }
                if !detail.versions.isEmpty {
                    Section(Copy.artifactVersions) {
                        ForEach(detail.versions, id: \.version) { v in
                            VStack(alignment: .leading, spacing: 2) {
                                HStack {
                                    Text(Copy.artifactVersionLong(v.version)).font(.subheadline.weight(.semibold))
                                    Spacer()
                                    Text(Format.listTime(v.createdAt)).font(.footnote.monospacedDigit()).foregroundStyle(.secondary)
                                }
                                Text([v.createdByName, v.note].filter { !$0.isEmpty }.joined(separator: " · "))
                                    .font(.footnote).foregroundStyle(.secondary).lineLimit(3)
                            }
                        }
                    }
                }
            }
            .navigationTitle(Copy.artifactOrigin)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button(Copy.close) { dismiss() } }
            }
        }
    }
}

/// A 产出 page, isolated: no shared storage, no network, reads only its own folder. Links to
/// other pages open in the browser only when tapped.
struct ArtifactWebView: UIViewRepresentable {
    let entry: URL
    let folder: URL

    /// Blocks every load that is not a file; file loads are limited to the folder by WebKit.
    private static let rules = """
    [{"trigger":{"url-filter":".*"},"action":{"type":"block"}},
     {"trigger":{"url-filter":"^file:"},"action":{"type":"ignore-previous-rules"}}]
    """

    func makeCoordinator() -> Coordinator { Coordinator(folder: folder) }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.defaultWebpagePreferences.allowsContentJavaScript = true
        config.preferences.javaScriptCanOpenWindowsAutomatically = false
        config.dataDetectorTypes = []
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        view.isOpaque = false
        view.backgroundColor = .systemBackground
        view.allowsLinkPreview = false
        WKContentRuleListStore.default().compileContentRuleList(forIdentifier: "artifact-offline", encodedContentRuleList: Self.rules) { list, _ in
            if let list { view.configuration.userContentController.add(list) }
            view.loadFileURL(entry, allowingReadAccessTo: folder)
            context.coordinator.loaded = entry
        }
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {
        guard context.coordinator.loaded != nil, context.coordinator.loaded != entry else { return }
        context.coordinator.folder = folder
        context.coordinator.loaded = entry
        view.loadFileURL(entry, allowingReadAccessTo: folder)
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        var folder: URL
        var loaded: URL?

        init(folder: URL) { self.folder = folder }

        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
            guard let url = action.request.url else { return decisionHandler(.cancel) }
            if url.isFileURL, url.standardizedFileURL.path.hasPrefix(folder.standardizedFileURL.path) {
                return decisionHandler(.allow)
            }
            if action.navigationType == .linkActivated, ["http", "https", "mailto"].contains(url.scheme ?? "") {
                UIApplication.shared.open(url)
            }
            decisionHandler(.cancel)
        }
    }
}
