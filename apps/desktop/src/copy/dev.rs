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
    // process view (bot runs)
    RUN_ID = "run id: {id}",
    RUN_BOT = "bot: {id}",
    RUN_MACHINE = "machine: {id}",
    RUN_STATUS = "status: {status}",
    RUN_TIMES = "created {created} · started {started} · ended {ended} · updated {updated}",
    RUN_SUMMARY = "summary: {text}",
    RUN_SESSION = "CLI session: {id} ({runtime})",
    RUN_LAST_SEQ = "log lastSeq: {seq}",
    RUN_RAW = "Raw run events ({n}, newest last)",
    RUN_TOOL_INPUT = "input: {json}",
    RUN_DOWNLOAD_SESSION = "Download raw session (.jsonl.gz)",
    RUN_DOWNLOADING = "Downloading session…",
    RUN_DOWNLOADED = "Saved to {path}",
    RUN_DOWNLOAD_FAILED = "Session download failed: {error}",
    // search
    SEARCH_HIT = "message {id} · seq {seq}",
}
