import SwiftUI

/// 群资料: what the 群 is for, who is in it, what is being done now and the newest 产出.
struct GroupInfoScreen: View {
    @EnvironmentObject private var model: AppModel
    let groupId: String
    @State private var info: GroupInfo?
    @State private var artifacts: [ArtifactItem] = []

    var body: some View {
        ScrollView {
            if let info {
                VStack(spacing: 0) {
                    header(info)
                    if !info.isDm {
                        SectionTitle(text: Copy.purpose)
                        Text(info.purpose.isEmpty ? Copy.purposeEmpty : info.purpose)
                            .font(.body).foregroundStyle(info.purpose.isEmpty ? .secondary : .primary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(14)
                            .background(Theme.fill, in: RoundedRectangle(cornerRadius: Theme.rowRadius))
                    }
                    if !info.activeRuns.isEmpty {
                        SectionTitle(text: Copy.workingNow)
                        Panel { ForEach(info.activeRuns, id: \.id) { RunRow(run: $0) } }
                    }
                    SectionTitle(text: Copy.membersTitle, trailing: Copy.memberCount(UInt32(info.members.count)))
                    Panel { ForEach(info.members, id: \.id) { MemberLine(member: $0) } }
                    if !info.isDm {
                        SectionTitle(text: Copy.topics, trailing: Copy.topicsCount(info.topicCount))
                        Panel {
                            NavigationLink(value: Route.topics(group: groupId)) {
                                HStack {
                                    Label(Copy.allTopics, systemImage: "number")
                                    Spacer()
                                    Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
                                }
                                .padding(.horizontal, 12).frame(minHeight: 50).contentShape(Rectangle())
                            }
                            .buttonStyle(RowButtonStyle())
                        }
                    }
                    if !artifacts.isEmpty {
                        SectionTitle(text: Copy.recentArtifacts, trailing: Copy.artifactsCount(info.artifactCount))
                        Panel { ForEach(artifacts.prefix(5), id: \.id) { ArtifactRow(artifact: $0, showWhere: false) } }
                    }
                }
                .padding(.horizontal, Theme.gutter).padding(.bottom, 24)
            }
        }
        .background(Theme.page)
        .navigationTitle(info?.isDm == true ? Copy.details : Copy.groupInfoTitle)
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            reload()
            model.refreshArtifacts(group: groupId)
        }
        .onChange(of: model.runsTick) { reload() }
        .onChange(of: model.artifactsTick) { reload() }
        .onChange(of: model.groupTicks[groupId]) { reload() }
    }

    private func reload() {
        info = model.groupInfo(groupId)
        artifacts = model.artifacts(group: groupId)
    }

    private func header(_ info: GroupInfo) -> some View {
        VStack(spacing: 8) {
            Avatar(name: info.name, round: false, size: 72)
            Text(info.name).font(.title2.weight(.bold)).multilineTextAlignment(.center)
            if !info.isDm {
                Text([Copy.memberCount(UInt32(info.members.count)), Copy.topicsCount(info.topicCount)].joined(separator: " · "))
                    .font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity).padding(.top, 12)
    }
}
