//! The provider of the thread and sidebar state scenes (COMPLETENESS.md §4,
//! §2.14, §2.13): the sample data with one part reshaped, every other call
//! forwarded unchanged.
//!
//! - [`Shape::NoMessages`]: every 话题 has no messages yet (the empty thread).
//! - [`Shape::OlderLoading`]: the thread has older messages and their page is
//!   loading (the skeleton rows above the first message).
//! - [`Shape::FirstSync`]: signed in, connecting, nothing synced yet: no 群
//!   in the sidebar (the sidebar's first-sync skeleton).

use std::{path::PathBuf, sync::Arc};

use futures::channel::mpsc::UnboundedReceiver;

use crate::data::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    NoMessages,
    OlderLoading,
    FirstSync,
}

pub struct Shaped {
    inner: Arc<dyn ChatProvider>,
    shape: Shape,
}

impl Shaped {
    pub fn new(inner: Arc<dyn ChatProvider>, shape: Shape) -> Arc<Self> {
        Arc::new(Self { inner, shape })
    }
}

impl ChatProvider for Shaped {
    fn connection(&self) -> ConnectionState {
        match self.shape {
            Shape::FirstSync => ConnectionState::Connecting,
            _ => self.inner.connection(),
        }
    }
    fn groups(&self) -> Vec<GroupSummary> {
        match self.shape {
            Shape::FirstSync => Vec::new(),
            _ => self.inner.groups(),
        }
    }
    fn topics(&self, group_id: &str) -> Vec<Topic> {
        match self.shape {
            Shape::FirstSync => Vec::new(),
            _ => self.inner.topics(group_id),
        }
    }
    fn todo(&self) -> Vec<TodoItem> {
        match self.shape {
            Shape::FirstSync => Vec::new(),
            _ => self.inner.todo(),
        }
    }
    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        match self.shape {
            Shape::NoMessages | Shape::FirstSync => MessageWindow::default(),
            Shape::OlderLoading => MessageWindow {
                has_older: true,
                loading_older: true,
                ..self.inner.messages(group_id, topic_id, limit)
            },
        }
    }
    fn session(&self) -> SessionState {
        self.inner.session()
    }
    fn login(&self, request: LoginRequest) {
        self.inner.login(request)
    }
    fn logout(&self) {
        self.inner.logout()
    }
    fn sign_out(&self, wipe_cache: bool) {
        self.inner.sign_out(wipe_cache)
    }
    fn reconnect_now(&self) {
        self.inner.reconnect_now()
    }
    fn members(&self) -> Vec<Member> {
        self.inner.members()
    }
    fn member(&self, id: &str) -> Option<Member> {
        self.inner.member(id)
    }
    fn topic(&self, group_id: &str, topic_id: &str) -> Option<Topic> {
        self.inner.topic(group_id, topic_id)
    }
    fn load_older(&self, group_id: &str, topic_id: &str) {
        self.inner.load_older(group_id, topic_id)
    }
    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId> {
        self.inner.typing(group_id, topic_id)
    }
    fn artifacts(&self, group_id: &str, topic_id: &str) -> Vec<ArtifactSummary> {
        self.inner.artifacts(group_id, topic_id)
    }
    fn send(&self, request: SendRequest) -> ClientMsgId {
        self.inner.send(request)
    }
    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        self.inner.start_topic(request)
    }
    fn retry_send(&self, group_id: &str, client_msg_id: &str) {
        self.inner.retry_send(group_id, client_msg_id)
    }
    fn edit(&self, group_id: &str, message_id: &str, body: String) {
        self.inner.edit(group_id, message_id, body)
    }
    fn delete(&self, group_id: &str, message_id: &str) {
        self.inner.delete(group_id, message_id)
    }
    fn react(&self, group_id: &str, topic_id: &str, message_id: &str, emoji: &str, on: bool) {
        self.inner.react(group_id, topic_id, message_id, emoji, on)
    }
    fn set_topic_status(
        &self,
        group_id: &str,
        topic_id: &str,
        status: TopicStatus,
        waiting_on: Vec<MemberId>,
    ) {
        self.inner
            .set_topic_status(group_id, topic_id, status, waiting_on)
    }
    fn quoted_message(
        &self,
        group_id: &str,
        quoting_id: &str,
        quoted_id: &str,
    ) -> Option<QuotedMessage> {
        self.inner.quoted_message(group_id, quoting_id, quoted_id)
    }
    fn rename_topic(&self, group_id: &str, topic_id: &str, title: String) {
        self.inner.rename_topic(group_id, topic_id, title)
    }
    fn mark_read(&self, group_id: &str, topic_id: &str, seq: u64) {
        self.inner.mark_read(group_id, topic_id, seq)
    }
    fn set_typing(&self, group_id: &str, topic_id: &str) {
        self.inner.set_typing(group_id, topic_id)
    }
    fn discard_send(&self, group_id: &str, client_msg_id: &str) {
        self.inner.discard_send(group_id, client_msg_id)
    }
    fn draft(&self, key: &DraftKey) -> Draft {
        self.inner.draft(key)
    }
    fn save_draft_text(&self, key: &DraftKey, body: String, mentions: Vec<MemberId>) {
        self.inner.save_draft_text(key, body, mentions)
    }
    fn set_quote(&self, key: &DraftKey, quote_of: Option<MessageId>) {
        self.inner.set_quote(key, quote_of)
    }
    fn attach_files(&self, key: &DraftKey, files: Vec<NewFile>) {
        self.inner.attach_files(key, files)
    }
    fn remove_file(&self, key: &DraftKey, file_id: &str) {
        self.inner.remove_file(key, file_id)
    }
    fn clear_draft(&self, key: &DraftKey) {
        self.inner.clear_draft(key)
    }
    fn send_with_files(
        &self,
        request: SendRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
    ) -> ClientMsgId {
        self.inner.send_with_files(request, key, files)
    }
    fn start_topic_with_files(
        &self,
        request: StartTopicRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
    ) -> ClientMsgId {
        self.inner.start_topic_with_files(request, key, files)
    }
    fn take_rejected_files(&self, key: &DraftKey) -> Vec<RejectedFile> {
        self.inner.take_rejected_files(key)
    }
    fn file_transfer(&self, key: &str) -> Option<TransferState> {
        self.inner.file_transfer(key)
    }
    fn cached_file(&self, file: &FileAttachment) -> Option<PathBuf> {
        self.inner.cached_file(file)
    }
    fn thumbnail(&self, group_id: &str, topic_id: &str, file: &FileAttachment) -> Option<PathBuf> {
        self.inner.thumbnail(group_id, topic_id, file)
    }
    fn fetch_file(&self, group_id: &str, topic_id: &str, file: &FileAttachment) -> Reply<PathBuf> {
        self.inner.fetch_file(group_id, topic_id, file)
    }
    fn cancel_download(&self, file: &FileAttachment) {
        self.inner.cancel_download(file)
    }
    fn topic_files(&self, group_id: &str, topic_id: &str) -> Vec<TopicFile> {
        self.inner.topic_files(group_id, topic_id)
    }
    fn refresh_artifacts(&self, group_id: &str, topic_id: &str) {
        self.inner.refresh_artifacts(group_id, topic_id)
    }
    fn artifact(&self, artifact_id: &str) -> Option<ArtifactDetail> {
        self.inner.artifact(artifact_id)
    }
    fn request_artifact(&self, group_id: &str, topic_id: &str, artifact_id: &str) {
        self.inner.request_artifact(group_id, topic_id, artifact_id)
    }
    fn open_artifact_version(
        &self,
        artifact_id: &str,
        version: Option<u32>,
    ) -> Reply<ArtifactFolder> {
        self.inner.open_artifact_version(artifact_id, version)
    }
    fn create_artifact(&self, request: NewArtifactRequest) -> Reply<CreatedArtifact> {
        self.inner.create_artifact(request)
    }
    fn create_group(&self, request: CreateGroupRequest) -> AdminReply<GroupId> {
        self.inner.create_group(request)
    }
    fn open_dm(&self, member_id: &str) -> AdminReply<GroupId> {
        self.inner.open_dm(member_id)
    }
    fn add_group_members(&self, group_id: &str, member_ids: Vec<MemberId>) -> AdminReply<()> {
        self.inner.add_group_members(group_id, member_ids)
    }
    fn rename_group(&self, group_id: &str, name: String) -> AdminReply<()> {
        self.inner.rename_group(group_id, name)
    }
    fn leave_group(&self, group_id: &str) {
        self.inner.leave_group(group_id)
    }
    fn group_muted(&self, group_id: &str) -> bool {
        self.inner.group_muted(group_id)
    }
    fn set_group_muted(&self, group_id: &str, muted: bool) {
        self.inner.set_group_muted(group_id, muted)
    }
    fn saved_navigation(&self) -> Option<SavedNavigation> {
        self.inner.saved_navigation()
    }
    fn save_navigation(&self, navigation: SavedNavigation) {
        self.inner.save_navigation(navigation)
    }
    fn topic_read_seq(&self, group_id: &str, topic_id: &str) -> Option<u64> {
        self.inner.topic_read_seq(group_id, topic_id)
    }
    fn topic_following(&self, group_id: &str, topic_id: &str) -> bool {
        self.inner.topic_following(group_id, topic_id)
    }
    fn set_topic_following(&self, group_id: &str, topic_id: &str, follow: bool) {
        self.inner.set_topic_following(group_id, topic_id, follow)
    }
    fn topic_muted(&self, group_id: &str, topic_id: &str) -> bool {
        self.inner.topic_muted(group_id, topic_id)
    }
    fn set_topic_muted(&self, group_id: &str, topic_id: &str, muted: bool) {
        self.inner.set_topic_muted(group_id, topic_id, muted)
    }
    fn set_topic_owner(&self, group_id: &str, topic_id: &str, owner: Option<MemberId>) {
        self.inner.set_topic_owner(group_id, topic_id, owner)
    }
    fn search(&self, query: SearchQuery) -> SearchReply {
        self.inner.search(query)
    }
    fn runs(&self, group_id: &str, topic_id: &str) -> Vec<BotRunSummary> {
        self.inner.runs(group_id, topic_id)
    }
    fn run_process(&self, run_id: &str) -> RunProcess {
        self.inner.run_process(run_id)
    }
    fn follow_run(&self, run_id: &str) {
        self.inner.follow_run(run_id)
    }
    fn cancel_run(&self, group_id: &str, topic_id: &str, run_id: &str) -> AdminReply<()> {
        self.inner.cancel_run(group_id, topic_id, run_id)
    }
    fn run_stopping(&self, run_id: &str) -> bool {
        self.inner.run_stopping(run_id)
    }
    fn download_run_session(&self, run_id: &str, dir: PathBuf) -> SessionDownload {
        self.inner.download_run_session(run_id, dir)
    }
    fn update_profile(&self, name: String, role: Option<String>) -> AdminReply<Member> {
        self.inner.update_profile(name, role)
    }
    fn upload_avatar(&self, path: PathBuf) -> AdminReply<Member> {
        self.inner.upload_avatar(path)
    }
    fn create_invite(&self, request: InviteRequest) -> AdminReply<InviteCreated> {
        self.inner.create_invite(request)
    }
    fn set_member_level(&self, member_id: &str, level: MemberLevel) -> AdminReply<Member> {
        self.inner.set_member_level(member_id, level)
    }
    fn set_member_deactivated(&self, member_id: &str, deactivated: bool) -> AdminReply<Member> {
        self.inner.set_member_deactivated(member_id, deactivated)
    }
    fn create_bot_member(
        &self,
        name: String,
        role: Option<String>,
        character: Option<CharacterId>,
        runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        self.inner.create_bot_member(name, role, character, runner)
    }
    fn bot_runner(&self, member_id: &str) -> AdminReply<Option<RunnerSetup>> {
        self.inner.bot_runner(member_id)
    }
    fn update_bot_member(&self, member_id: &str, edit: BotMemberEdit) -> AdminReply<()> {
        self.inner.update_bot_member(member_id, edit)
    }
    fn set_member_character(
        &self,
        member_id: &str,
        character: Option<CharacterId>,
    ) -> AdminReply<Member> {
        self.inner.set_member_character(member_id, character)
    }
    fn set_member_plan(&self, member_id: &str, plan: pages::MemberPlan) -> AdminReply<Member> {
        self.inner.set_member_plan(member_id, plan)
    }
    fn machines(&self) -> AdminReply<Vec<MachineInfo>> {
        self.inner.machines()
    }
    fn create_machine(&self, name: String, labels: Vec<String>) -> AdminReply<MachineCreated> {
        self.inner.create_machine(name, labels)
    }
    fn group_prefs(&self, group_id: &str) -> GroupPrefs {
        self.inner.group_prefs(group_id)
    }
    fn set_group_prefs(
        &self,
        group_id: &str,
        muted: Option<bool>,
        notify: Option<NotifyLevel>,
    ) -> AdminReply<()> {
        self.inner.set_group_prefs(group_id, muted, notify)
    }
    fn followed_topics(&self) -> Vec<FollowedTopic> {
        self.inner.followed_topics()
    }
    fn send_diagnostics(&self, description: String) -> AdminReply<String> {
        self.inner.send_diagnostics(description)
    }
    fn card(&self, group_id: &str, message_id: &str) -> Option<Arc<card::Card>> {
        self.inner.card(group_id, message_id)
    }
    fn card_presses(&self, group_id: &str, message_id: &str) -> Vec<CardPress> {
        self.inner.card_presses(group_id, message_id)
    }
    fn card_press_state(&self, group_id: &str, message_id: &str) -> Option<CardPressState> {
        self.inner.card_press_state(group_id, message_id)
    }
    fn press_card_action(&self, group_id: &str, topic_id: &str, message_id: &str, action_id: &str) {
        self.inner
            .press_card_action(group_id, topic_id, message_id, action_id)
    }
    fn card_image(&self, group_id: &str, topic_id: &str, file_id: &str) -> Option<PathBuf> {
        self.inner.card_image(group_id, topic_id, file_id)
    }
    fn desk_note(&self) -> Option<v2::DeskNote> {
        self.inner.desk_note()
    }
    fn asks_for_me(&self) -> Vec<v2::AskItem> {
        self.inner.asks_for_me()
    }
    fn pinned_ask(&self, group_id: &str, topic_id: &str) -> Option<v2::AskItem> {
        self.inner.pinned_ask(group_id, topic_id)
    }
    fn done_today(&self) -> Vec<v2::DoneItem> {
        self.inner.done_today()
    }
    fn topic_result_proof(&self, group_id: &str, topic_id: &str) -> Vec<v2::Proof> {
        self.inner.topic_result_proof(group_id, topic_id)
    }
    fn active_runs(&self) -> Vec<BotRunSummary> {
        self.inner.active_runs()
    }
    fn queue(&self) -> Vec<v2::QueuedItem> {
        self.inner.queue()
    }
    fn topic_changes(&self, group_id: &str, topic_id: &str) -> Option<v2::TopicChanges> {
        self.inner.topic_changes(group_id, topic_id)
    }
    fn run_usage(&self, run_id: &str) -> Option<v2::RunUsage> {
        self.inner.run_usage(run_id)
    }
    fn topic_cost(&self, group_id: &str, topic_id: &str) -> Option<v2::TopicCost> {
        self.inner.topic_cost(group_id, topic_id)
    }
    fn topic_decisions(&self, group_id: &str, topic_id: &str) -> Vec<v2::Decision> {
        self.inner.topic_decisions(group_id, topic_id)
    }
    fn group_decisions(&self, group_id: &str) -> Vec<v2::Decision> {
        self.inner.group_decisions(group_id)
    }
    fn group_purpose(&self, group_id: &str) -> Option<String> {
        self.inner.group_purpose(group_id)
    }
    fn set_group_purpose(&self, group_id: &str, purpose: String) -> AdminReply<()> {
        self.inner.set_group_purpose(group_id, purpose)
    }
    fn side_chat_default_member(&self) -> Option<MemberId> {
        self.inner.side_chat_default_member()
    }
    fn side_chat(&self, group_id: &str, topic_id: &str, member_id: &str) -> v2::SideChatThread {
        self.inner.side_chat(group_id, topic_id, member_id)
    }
    fn send_side_chat(
        &self,
        group_id: &str,
        topic_id: &str,
        member_id: &str,
        body: String,
        refs: Vec<DraftRef>,
    ) {
        self.inner
            .send_side_chat(group_id, topic_id, member_id, body, refs)
    }
    fn post_side_chat_suggestion(
        &self,
        group_id: &str,
        topic_id: &str,
        member_id: &str,
        message_id: &str,
    ) {
        self.inner
            .post_side_chat_suggestion(group_id, topic_id, member_id, message_id)
    }
    fn machine_screen(&self, run_id: &str) -> Option<v2::MachineScreen> {
        self.inner.machine_screen(run_id)
    }
    fn acceptance_criteria(&self, run_id: &str) -> Vec<v2::AcceptanceItem> {
        self.inner.acceptance_criteria(run_id)
    }
    fn run_step_times(&self, run_id: &str) -> Vec<Millis> {
        self.inner.run_step_times(run_id)
    }
    fn take_over_run(&self, group_id: &str, topic_id: &str, run_id: &str) -> AdminReply<()> {
        self.inner.take_over_run(group_id, topic_id, run_id)
    }
    fn hand_back_run(&self, group_id: &str, topic_id: &str, run_id: &str) -> AdminReply<()> {
        self.inner.hand_back_run(group_id, topic_id, run_id)
    }
    fn member_stats(&self, member_id: &str) -> Option<v2::MemberStats> {
        self.inner.member_stats(member_id)
    }
    fn all_artifacts(&self) -> Vec<(GroupId, ArtifactSummary)> {
        self.inner.all_artifacts()
    }
    fn artifact_story(&self, artifact_id: &str) -> Option<v2::ArtifactStory> {
        self.inner.artifact_story(artifact_id)
    }
    fn card_v2(&self, group_id: &str, message_id: &str) -> Option<cards_v2::CardV2Message> {
        self.inner.card_v2(group_id, message_id)
    }
    fn card_v2_answers(&self, group_id: &str, message_id: &str) -> Vec<cards_v2::CardV2Answer> {
        self.inner.card_v2_answers(group_id, message_id)
    }
    fn card_v2_pending(&self, group_id: &str, message_id: &str) -> Option<cards_v2::CardV2Pending> {
        self.inner.card_v2_pending(group_id, message_id)
    }
    fn answer_card_v2(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        answer: cards_v2::CardV2AnswerRequest,
    ) {
        self.inner
            .answer_card_v2(group_id, topic_id, message_id, answer)
    }
    fn withdraw_card_v2_answer(&self, group_id: &str, topic_id: &str, message_id: &str) {
        self.inner
            .withdraw_card_v2_answer(group_id, topic_id, message_id)
    }
    fn send_card_event(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        request: cards_v2::CardEventRequest,
    ) -> Option<String> {
        self.inner
            .send_card_event(group_id, topic_id, message_id, request)
    }
    fn card_v2_events(&self, group_id: &str, message_id: &str) -> Vec<cards_v2::MyCardEvent> {
        self.inner.card_v2_events(group_id, message_id)
    }
    fn drop_card_event(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        client_event_id: &str,
    ) {
        self.inner
            .drop_card_event(group_id, topic_id, message_id, client_event_id)
    }
    fn retry_card_event(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        client_event_id: &str,
    ) -> bool {
        self.inner
            .retry_card_event(group_id, topic_id, message_id, client_event_id)
    }
    fn message_proof(&self, group_id: &str, message_id: &str) -> Vec<v2::Proof> {
        self.inner.message_proof(group_id, message_id)
    }
    fn media_duration(&self, file: &FileAttachment) -> Option<u64> {
        self.inner.media_duration(file)
    }
    fn artifact_preview(&self, artifact_id: &str, version: u32) -> Option<v2::ArtifactPreview> {
        self.inner.artifact_preview(artifact_id, version)
    }
    fn changes_approval(&self, group_id: &str, topic_id: &str) -> Option<(MemberId, Millis)> {
        self.inner.changes_approval(group_id, topic_id)
    }
    fn approve_changes(&self, group_id: &str, topic_id: &str) -> AdminReply<()> {
        self.inner.approve_changes(group_id, topic_id)
    }
    fn withdraw_changes_approval(&self, group_id: &str, topic_id: &str) -> AdminReply<()> {
        self.inner.withdraw_changes_approval(group_id, topic_id)
    }
    fn changes_approval_queued(&self, group_id: &str, topic_id: &str) -> bool {
        self.inner.changes_approval_queued(group_id, topic_id)
    }
    fn clock_now(&self) -> Millis {
        self.inner.clock_now()
    }
    fn topic_glance(&self, group_id: &str, topic_id: &str) -> pages::TopicGlance {
        self.inner.topic_glance(group_id, topic_id)
    }
    fn group_recent_done(&self, group_id: &str) -> Vec<pages::RecentDone> {
        self.inner.group_recent_done(group_id)
    }
    fn group_artifacts(&self, group_id: &str) -> Vec<ArtifactSummary> {
        self.inner.group_artifacts(group_id)
    }
    fn artifact_thumb(&self, artifact_id: &str) -> Option<pages::ArtifactThumb> {
        self.inner.artifact_thumb(artifact_id)
    }
    fn can_record_decision(&self, group_id: &str) -> bool {
        self.inner.can_record_decision(group_id)
    }
    fn record_decision(&self, group_id: &str, text: String) -> AdminReply<()> {
        self.inner.record_decision(group_id, text)
    }
    fn member_extras(&self, member_id: &str) -> pages::MemberExtras {
        self.inner.member_extras(member_id)
    }
    fn retry_member_problem(&self, member_id: &str, run_id: &str) -> AdminReply<()> {
        self.inner.retry_member_problem(member_id, run_id)
    }
    fn artifact_responses(&self, artifact_id: &str) -> Vec<pages::ArtifactResponse> {
        self.inner.artifact_responses(artifact_id)
    }
    fn artifact_responses_load(&self, artifact_id: &str) -> pages::ResponsesLoad {
        self.inner.artifact_responses_load(artifact_id)
    }
    fn reload_artifact_responses(&self, artifact_id: &str) {
        self.inner.reload_artifact_responses(artifact_id)
    }
    fn artifact_page_events(&self, artifact_id: &str, version: u32) -> Vec<String> {
        self.inner.artifact_page_events(artifact_id, version)
    }
    fn send_artifact_event(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        payload: Option<serde_json::Value>,
    ) {
        self.inner
            .send_artifact_event(artifact_id, version, name, payload)
    }
    fn side_chat_topics(&self) -> std::collections::HashSet<(GroupId, TopicId)> {
        self.inner.side_chat_topics()
    }
    fn page_bridge(&self, artifact_id: &str, version: u32) -> Option<page_host::PageBridge> {
        self.inner.page_bridge(artifact_id, version)
    }
    fn send_page_event(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        kind: page_host::PageEventKind,
        payload: Option<serde_json::Value>,
    ) -> Result<String, page_host::PageSendError> {
        self.inner
            .send_page_event(artifact_id, version, name, kind, payload)
    }
    fn fetch_page_data(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> futures::channel::oneshot::Receiver<Option<(serde_json::Value, u64)>> {
        self.inner.fetch_page_data(artifact_id, version)
    }
    fn page_signals(&self) -> Option<UnboundedReceiver<page_host::PageSignal>> {
        self.inner.page_signals()
    }
    fn file_media(&self, file: &FileAttachment) -> Option<chat_core::proto::MediaInfo> {
        self.inner.file_media(file)
    }
    fn artifact_media(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Option<chat_core::proto::MediaInfo> {
        self.inner.artifact_media(artifact_id, version)
    }
    fn file_poster(
        &self,
        group_id: &str,
        topic_id: &str,
        file: &FileAttachment,
    ) -> Option<PathBuf> {
        self.inner.file_poster(group_id, topic_id, file)
    }
    fn upload_generating(&self, key: &str) -> bool {
        self.inner.upload_generating(key)
    }
    fn artifact_poster(&self, artifact_id: &str, version: u32) -> Option<PathBuf> {
        self.inner.artifact_poster(artifact_id, version)
    }
    fn refs_supported(&self) -> bool {
        self.inner.refs_supported()
    }
    fn add_draft_ref(&self, key: &DraftKey, r: DraftRef) -> AddRef {
        self.inner.add_draft_ref(key, r)
    }
    fn remove_draft_ref(&self, key: &DraftKey, index: usize) {
        self.inner.remove_draft_ref(key, index)
    }
    fn message_block_refs(&self, message: &Message) -> Vec<chat_core::proto::BlockRef> {
        self.inner.message_block_refs(message)
    }
    fn send_with_refs(
        &self,
        request: SendRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
        refs: Vec<chat_core::proto::BlockRefInput>,
    ) -> ClientMsgId {
        self.inner.send_with_refs(request, key, files, refs)
    }
    fn notify_preferences(&self) -> AdminReply<NotifyPreferences> {
        self.inner.notify_preferences()
    }
    fn set_notify_preferences(&self, prefs: NotifyPreferences) -> AdminReply<NotifyPreferences> {
        self.inner.set_notify_preferences(prefs)
    }
    fn approval_defaults(&self) -> AdminReply<ApprovalDefaults> {
        self.inner.approval_defaults()
    }
    fn set_approval_defaults(&self, defaults: ApprovalDefaults) -> AdminReply<ApprovalDefaults> {
        self.inner.set_approval_defaults(defaults)
    }
    fn devices(&self) -> AdminReply<Vec<DeviceInfo>> {
        self.inner.devices()
    }
    fn sign_out_device(&self, device_id: &str) -> AdminReply<()> {
        self.inner.sign_out_device(device_id)
    }
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        self.inner.subscribe()
    }
}
