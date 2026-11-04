//! The provider of the page state scenes (`*-loading`, `*-failed`): the v2
//! sample team's 群 list with nothing synced yet (no 话题, members, asks,
//! runs or 产出 in the cache) and a fixed connection state. Connecting
//! draws the pages' skeletons; Reconnecting draws their failure Alert.

use std::sync::Arc;

use futures::channel::mpsc::{UnboundedReceiver, unbounded};

use crate::data::*;

pub struct Unsynced {
    inner: Arc<dyn ChatProvider>,
    state: ConnectionState,
}

impl Unsynced {
    pub fn new(inner: Arc<dyn ChatProvider>, state: ConnectionState) -> Arc<Self> {
        Arc::new(Self { inner, state })
    }
}

impl ChatProvider for Unsynced {
    fn session(&self) -> SessionState {
        self.inner.session()
    }
    fn login(&self, request: LoginRequest) {
        self.inner.login(request)
    }
    fn logout(&self) {
        self.inner.logout()
    }
    fn connection(&self) -> ConnectionState {
        self.state.clone()
    }
    fn members(&self) -> Vec<Member> {
        match self.inner.session() {
            SessionState::SignedIn { me, .. } => vec![me],
            _ => Vec::new(),
        }
    }
    fn groups(&self) -> Vec<GroupSummary> {
        self.inner.groups()
    }
    fn topics(&self, _group_id: &str) -> Vec<Topic> {
        Vec::new()
    }
    fn todo(&self) -> Vec<TodoItem> {
        Vec::new()
    }
    fn messages(&self, _group_id: &str, _topic_id: &str, _limit: usize) -> MessageWindow {
        MessageWindow::default()
    }
    fn load_older(&self, _group_id: &str, _topic_id: &str) {}
    fn typing(&self, _group_id: &str, _topic_id: &str) -> Vec<MemberId> {
        Vec::new()
    }
    fn artifacts(&self, _group_id: &str, _topic_id: &str) -> Vec<ArtifactSummary> {
        Vec::new()
    }
    fn send(&self, request: SendRequest) -> ClientMsgId {
        request.body.len().to_string()
    }
    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        request.body.len().to_string()
    }
    fn edit(&self, _group_id: &str, _message_id: &str, _body: String) {}
    fn delete(&self, _group_id: &str, _message_id: &str) {}
    fn set_topic_status(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _status: TopicStatus,
        _waiting_on: Vec<MemberId>,
    ) {
    }
    fn mark_read(&self, _group_id: &str, _topic_id: &str, _seq: u64) {}
    fn set_typing(&self, _group_id: &str, _topic_id: &str) {}
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        unbounded().1
    }
    fn reconnect_now(&self) {}
}
