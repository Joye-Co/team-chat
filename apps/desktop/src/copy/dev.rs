//! The developer view (开发者视图). Only this section may use developer words
//! (session, token, bot, thread, ...). The catalog test skips it.

crate::copy! {
    TITLE = "Developer view",
    CONNECTION = "connection: {state}",
    SESSION = "session: {state}",
    SERVER = "server: {url}",
    GROUP_ID = "group id: {id}",
    TOPIC_ID = "topic id: {id}",
    LAST_SEQ = "lastSeq: {seq}",
    MEMBER_KIND_BOT = "bot",
    MEMBER_KIND_HUMAN = "human",
    PROVIDER = "provider: {name}",
    STATE_OFFLINE = "offline",
    STATE_CONNECTING = "connecting",
    STATE_ONLINE = "online",
    STATE_RECONNECTING = "reconnecting (attempt {attempt}, retry in {ms} ms)",
}
