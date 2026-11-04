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
    MEMBER_ID = "member id: {id}",
    // process view (bot runs)
    RUN_PANEL_TITLE = "Run details",
    RUN_PANEL_COPY = "Copy run details",
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
    // --- v11/pl-sf-symbols: the SF Symbols spike scene (developer scene) ---
    SF_TITLE = "SF Symbols spike (POLISH-SPEC §10.2)",
    SF_COLD = "cold: first lookup {lookup} ms · 40 at 1x {x1} ms · 40 at 2x {x2} ms · warm 80 {warm} ms",
    SF_SET_LUCIDE = "Lucide (today, stroke 1.75)",
    SF_SET_REGULAR = "SF Symbols Regular 13 pt (tiles Semibold)",
    SF_SET_MEDIUM = "SF Symbols Medium 13 pt (tiles Semibold)",
    SF_SAMPLE_GROUP_1 = "产品研发",
    SF_SAMPLE_GROUP_2 = "夜间告警降噪",
    SF_SAMPLE_TOPIC_DONE = "staging 验收",
    SF_SAMPLE_TOPIC_YOU = "发布 1.2 需要拍板",
    SF_PAGE_APPEARANCE = "外观",
    SF_PAGE_SHORTCUTS = "快捷键",
    SF_PAGE_PRIVACY = "隐私与数据",
    // --- end v11/pl-sf-symbols ---
    // --- v12/symbols: the app's own SF Symbols path in the comparison scene ---
    SF_SET_APP = "App icons (Sym via svg(), §6 sizes)",
    SF_SET_BEFORE = "Lucide (before, stroke 1.75)",
    // --- end v12/symbols ---
    // --- v14/t4: the developer rows of Settings › 通用 (labels localized, 2.19) ---
    LABEL_CONNECTION = "连接",
    LABEL_MEMBER_ID = "成员 ID",
    // --- end v14/t4 ---
}
