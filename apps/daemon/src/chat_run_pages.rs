//! What `chat-run` knows about two-way 产出 pages without the daemon: the page's contract
//! (`artifact guide`), a starter page (`artifact example`), and the checks of events.json and of
//! the preview before anything is uploaded. The limits are the server's and the page host's; the
//! tests read them from their sources, so the guide cannot say another number.

use serde_json::{Value, json};
use std::path::Path;

/// Most events a version declares (protocol `MAX_ARTIFACT_EVENTS`).
pub const MAX_EVENTS: usize = 20;
/// Longest declared event name after its first letter (protocol `ARTIFACT_EVENT_NAME_RE`).
const MAX_EVENT_NAME_TAIL: usize = 63;
/// Longest event label (protocol `ArtifactEventDecl.label`).
const MAX_EVENT_LABEL: usize = 200;
/// The keys of an event's payload schema (protocol `TC_SCHEMA_KEYS`).
pub const SCHEMA_KEYS: &[&str] = &[
    "type",
    "properties",
    "required",
    "enum",
    "items",
    "maxItems",
    "minimum",
    "maximum",
    "maxLength",
    "pattern",
    "description",
];
/// Deepest payload schema (protocol `tcSchemaProblem`).
const MAX_SCHEMA_DEPTH: usize = 4;
/// Longest `pattern` of a payload schema.
const MAX_SCHEMA_PATTERN: usize = 200;
/// KiB of a payload of `emit` and `ask` (protocol `MAX_ARTIFACT_EVENT_PAYLOAD_BYTES`).
const PAYLOAD_KIB: usize = 16;
/// KiB of a reply (protocol `MAX_ARTIFACT_REPLY_BYTES`).
const REPLY_KIB: usize = 64;
/// KiB of a data snapshot (protocol `MAX_ARTIFACT_DATA_BYTES`).
const DATA_KIB: usize = 256;
/// Seconds `ask()` waits by default and at most (artifact-view `ASK_TIMEOUT_*_MS`).
const ASK_DEFAULT_S: u64 = 30;
const ASK_MAX_S: u64 = 120;
/// Asks one page may have open (artifact-view `MAX_OPEN_ASKS`).
const OPEN_ASKS: usize = 4;
/// Seconds after the viewer's input in which a page may send (artifact-view
/// `USER_ACTION_WINDOW`).
const USER_ACTION_S: u64 = 2;
/// Longest `Error.message` a page gets (artifact-view `MAX_ERROR_MESSAGE_CHARS`).
const ERROR_MESSAGE_CHARS: usize = 300;
/// Data pushes a second per version (server `DATA_RATE`).
const DATA_PER_S: u64 = 2;
/// Seconds in which an ask takes its reply (server `REPLY_WINDOW_MS`).
const REPLY_WINDOW_S: u64 = 120;
/// Minutes an event can be named as `--cause` (server `CAUSE_WINDOW_MS`).
const CAUSE_MIN: u64 = 30;
/// Preview limits (protocol `ArtifactPreview`, `ArtifactMetric`, `MAX_ARTIFACT_METRICS`).
pub const MAX_SUMMARY: usize = 500;
pub const MAX_METRICS: usize = 6;
pub const MAX_METRIC_TEXT: usize = 40;

/// `chat-run artifact guide`: what the author of a two-way page must know.
pub fn guide() -> String {
    format!(
        "A two-way page: people act on it, the events reach you, you push the state back to \
every open copy.

Make it
  chat-run artifact example --out <folder>     writes a working page and its manifests
  chat-run artifact create --title <title> --dir <folder>/page --events <folder>/events.json \
--data <folder>/data.json --summary <one sentence>
  Keep events.json and data.json outside --dir: every file in --dir is a file of the page.

The manifests
  events.json   [{{\"name\": \"item.toggle\", \"kind\": \"emit\", \"label\": \"…\", \"schema\": \
{{…}}}}], at most {MAX_EVENTS}. name: a lower-case letter, then at most {MAX_EVENT_NAME_TAIL} of \
A-Za-z0-9_.- ; kind: \"emit\" (default, tells you something) or \"ask\" (the page waits for your \
reply); schema (optional): the payload's shape, checked on every event, from {keys} (at most \
{MAX_SCHEMA_DEPTH} levels). The page can send only what is declared here.
  data.json     any JSON, at most {DATA_KIB} KiB: the state the page draws.
  reply.json    any JSON, at most {REPLY_KIB} KiB: what the page's ask() resolves to.

In the page: window.teamchat (there before your first script runs; absent in a normal browser)
  viewer                         {{memberId, name}} or null
  can                            {{emit, ask, events: [{{name, kind}}]}}; emit and ask are false \
for a viewer who may only read
  onData(cb)                     cb(data, {{rev, cause?}}) runs with the current data (null \
before the first push) and again on every push; returns a function that unsubscribes
  getData()                      a Promise of the current data, or null
  emit(name, payload?)           a Promise of {{eventId}}
  ask(name, payload?, {{timeoutMs}})   a Promise of your reply; it waits {ASK_DEFAULT_S} s unless \
timeoutMs says otherwise (at most {ASK_MAX_S} s: your reply takes you a turn, so pass \
{{timeoutMs: {ASK_MAX_S}000}}); {OPEN_ASKS} asks may be open at once
  A payload is JSON of at most {PAYLOAD_KIB} KiB. A call that fails rejects with an Error whose \
.code is one of: not_allowed (the event is not declared, or the viewer may only read), \
no_user_action (sent later than {USER_ACTION_S} s after the viewer's own input), bad_payload (the \
payload does not fit the event's schema; .message says where), bad_request, too_large, \
rate_limited (sent too fast, or you cannot take work now), offline (nothing was sent), timeout, \
declined (you declined the ask; .message is your text), failed, reloaded (a new version replaced \
the page), unknown_method. .message has at most {ERROR_MESSAGE_CHARS} characters.

Rules a page must follow
  - emit and ask work only within {USER_ACTION_S} s of the viewer's own click, drag or key press. \
Nothing can be sent on load or on a timer: for fresh data, push it yourself.
  - The page runs in a sandbox with no network: put every script, style, font and picture in \
the folder and name it by a relative path (or a data: URI; a picture or media the page makes \
itself may use a blob: URL). No CDN, no web fonts, no fetch, XHR \
or WebSocket, no iframe, no worker, no <form> submit: handle clicks in script. A link to a web \
page asks the person before it opens in their browser.
  - alert, confirm and prompt show nothing: they warn in the console and return undefined, false \
and null. Draw your own dialog in the page.
  - Do not set fonts or colors for plain content: the app gives every page its font (14/22), \
its text color, table, code, button and field styles in both themes, and a transparent \
background, in the pane and in the 产出 window alike. <button data-tc=\"primary\"> is the main \
action (button[data-tc=primary]). Style your own parts with var(--tc-fg), var(--tc-muted), \
var(--tc-border), var(--tc-fill), var(--tc-bg), var(--tc-primary), var(--tc-danger), \
var(--tc-radius), var(--tc-gap), var(--tc-font-mono) and var(--tc-pad); there is no --tc-ok or \
--tc-warn. Never hard-code black or white and set no body background.
  - It opens in a pane about 400 pt wide: nothing may be cut off at that width.
  - Guard it: `const tc = window.teamchat; if (!tc || !tc.can.emit) {{ /* read only */ }}`. When \
emit rejects, put the control back and say why.
  - Keep the state in the page data, not in script variables or localStorage: a new version \
reloads every open copy, and the page's own storage may be gone the next time it opens.

Your side
  An event reaches you as a <{scheme}-event> block of kind artifact.emit or artifact.ask with \
artifactId, version, eventId, event, value and actor.
  chat-run artifact data --artifact <id> --version <n>    reads {{data, dataRev}}
  chat-run artifact data --artifact <id> --version <n> --file data.json --base-data-rev <dataRev> \
[--cause <eventId>]
      replaces the whole snapshot in every open copy, at most {DATA_PER_S} a second. With \
--base-data-rev a push over a newer state fails (artifact.stale_data): read again and retry. \
--cause names an event of the last {CAUSE_MIN} minutes.
  chat-run artifact reply --event <eventId> --file reply.json   (or --decline <text>)   answers \
an ask, within {REPLY_WINDOW_S} s.
  - Data belongs to one version and only the member that made the version can push it. \
`chat-run artifact version` starts without data: pass --keep-data to carry over the state of the \
version it is made from (one you made; otherwise it starts empty) or --data with a new state.
  - Emits of the same name sent in a burst are merged: the latest is delivered and the earlier \
ones reject with rate_limited. Send the full value ({{\"id\": \"b\", \"done\": true}}), never \
\"toggle\".",
        keys = SCHEMA_KEYS.join(", "),
        scheme = crate::product::url_scheme(),
    )
}

/// The starter page: a shared checklist with one `emit` and one `ask`.
pub const EXAMPLE_PAGE: &str = include_str!("../examples/page/index.html");
pub const EXAMPLE_EVENTS: &str = include_str!("../examples/page/events.json");
pub const EXAMPLE_DATA: &str = include_str!("../examples/page/data.json");
pub const EXAMPLE_REPLY: &str = include_str!("../examples/page/reply.json");

/// The files of the starter, by the path `--out` writes them at. Only `page/` is the 产出.
const EXAMPLE_FILES: &[(&str, &str)] = &[
    ("page/index.html", EXAMPLE_PAGE),
    ("events.json", EXAMPLE_EVENTS),
    ("data.json", EXAMPLE_DATA),
    ("reply.json", EXAMPLE_REPLY),
];

/// The commands that publish the starter written to `dir` and answer its events.
fn example_next(dir: &str) -> String {
    format!(
        "Publish it:\n  chat-run artifact create --title \"发布前清单\" --dir {dir}/page --events \
{dir}/events.json --data {dir}/data.json --summary \"发布前要做完的三件事\"\nWhen item.toggle \
arrives: change the item in data.json and push it with\n  chat-run artifact data --artifact <id> \
--version <n> --file {dir}/data.json --cause <eventId>\nWhen item.add (an ask) arrives: push the \
new list the same way, then answer with\n  chat-run artifact reply --event <eventId> --file \
{dir}/reply.json\n`chat-run artifact guide` has the page API and the rules."
    )
}

/// `chat-run artifact example [--out <folder>]`: prints the starter's files, or writes them.
pub fn example(out: Option<&str>) -> Result<String, String> {
    let Some(dir) = out else {
        let mut text = String::new();
        for (path, body) in EXAMPLE_FILES {
            text.push_str(&format!("===== {path} =====\n{}\n\n", body.trim_end()));
        }
        text.push_str(
            "`chat-run artifact example --out <folder>` writes these files (page/index.html is \
the page; the manifests stay beside the page folder, not in it).",
        );
        return Ok(text);
    };
    if dir.is_empty() {
        return Err("--out needs a folder".into());
    }
    let root = Path::new(dir);
    for (path, _) in EXAMPLE_FILES {
        if root.join(path).exists() {
            return Err(format!(
                "{} is already there: give a new folder",
                root.join(path).display()
            ));
        }
    }
    let mut wrote = vec![];
    for (path, body) in EXAMPLE_FILES {
        let file = root.join(path);
        let parent = file.parent().unwrap_or(root);
        std::fs::create_dir_all(parent)
            .and_then(|()| std::fs::write(&file, body))
            .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        wrote.push(format!("  {}", file.display()));
    }
    let dir = dir.trim_end_matches('/');
    Ok(format!(
        "wrote:\n{}\n{}",
        wrote.join("\n"),
        example_next(dir)
    ))
}

/// `chat-run artifact guide|example ...`: what to print (standard output, exit 0) or the error
/// (exit 1); `None` for another command.
pub fn local(args: &[String]) -> Option<Result<String, String>> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["artifact", "guide"] => Some(Ok(guide())),
        ["artifact", "guide", ..] => Some(Err("artifact guide takes no flags".into())),
        ["artifact", "example"] => Some(example(None)),
        ["artifact", "example", "--out", dir] => Some(example(Some(dir))),
        ["artifact", "example", flag] if flag.starts_with("--out=") => {
            Some(example(Some(&flag["--out=".len()..])))
        }
        ["artifact", "example", ..] => Some(Err("artifact example takes --out <folder>".into())),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// events.json
// ---------------------------------------------------------------------------------------------

fn event_name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name.len() <= 1 + MAX_EVENT_NAME_TAIL
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Why a payload schema is not one the server takes (protocol `tcSchemaProblem`).
fn schema_problem(schema: &Value, depth: usize) -> Option<String> {
    if depth > MAX_SCHEMA_DEPTH {
        return Some(format!("schema is nested deeper than {MAX_SCHEMA_DEPTH}"));
    }
    let Some(m) = schema.as_object() else {
        return Some("schema is an object".into());
    };
    for (k, v) in m {
        if !SCHEMA_KEYS.contains(&k.as_str()) {
            return Some(format!(
                "schema key {k} is not supported; a schema takes {}",
                SCHEMA_KEYS.join(", ")
            ));
        }
        match k.as_str() {
            "pattern"
                if v.as_str()
                    .is_none_or(|p| p.chars().count() > MAX_SCHEMA_PATTERN) =>
            {
                return Some(format!(
                    "pattern is a string of at most {MAX_SCHEMA_PATTERN} characters"
                ));
            }
            "properties" => {
                let Some(props) = v.as_object() else {
                    return Some("properties is an object".into());
                };
                if let Some(e) = props.values().find_map(|p| schema_problem(p, depth + 1)) {
                    return Some(e);
                }
            }
            "items" => {
                if let Some(e) = schema_problem(v, depth + 1) {
                    return Some(e);
                }
            }
            _ => {}
        }
    }
    None
}

/// The declarations of an events.json (a list, or `{"events": [...]}`), checked before the
/// upload. The server drops a key it does not know without a word, so `"type": "ask"` would
/// declare an `emit` and the page's `ask()` would fail; here it is an error that names the key.
pub fn events(v: Value) -> Result<Value, String> {
    let list = match v {
        Value::Object(mut m) if m.contains_key("events") => m.remove("events").unwrap_or_default(),
        v => v,
    };
    let Some(items) = list.as_array() else {
        return Err(
            "events.json must hold a list: [{\"name\": \"item.toggle\", \"kind\": \"emit\"}] (or \
{\"events\": [...]})"
                .into(),
        );
    };
    if items.len() > MAX_EVENTS {
        return Err(format!(
            "events.json declares {} events: at most {MAX_EVENTS}",
            items.len()
        ));
    }
    let mut names: Vec<&str> = vec![];
    for (i, e) in items.iter().enumerate() {
        let Some(m) = e.as_object() else {
            return Err(format!(
                "event {i}: must be an object {{name, kind, label, schema}}"
            ));
        };
        let name = m.get("name").and_then(Value::as_str).unwrap_or_default();
        let at = if name.is_empty() {
            format!("event {i}")
        } else {
            format!("event {name}")
        };
        if let Some(k) = m
            .keys()
            .find(|k| !matches!(k.as_str(), "name" | "kind" | "label" | "schema"))
        {
            let hint = match k.as_str() {
                "type" => "; say what the event is with \"kind\": \"emit\" or \"ask\"",
                "payload" | "payloadSchema" | "params" => {
                    "; the payload's shape goes in \"schema\""
                }
                "title" | "description" => "; the words people see go in \"label\"",
                _ => "",
            };
            return Err(format!(
                "{at}: unknown key {k} (an event has name, kind, label, schema){hint}"
            ));
        }
        if !event_name_ok(name) {
            return Err(format!(
                "{at}: name must be a lower-case letter, then at most {MAX_EVENT_NAME_TAIL} of \
A-Za-z0-9_.-"
            ));
        }
        if names.contains(&name) {
            return Err(format!("{at}: declared twice"));
        }
        names.push(name);
        match m.get("kind") {
            None => {}
            Some(k) if k == "emit" || k == "ask" => {}
            Some(k) => return Err(format!("{at}: kind must be \"emit\" or \"ask\", not {k}")),
        }
        match m.get("label") {
            None => {}
            Some(Value::String(l)) if l.chars().count() <= MAX_EVENT_LABEL => {}
            Some(_) => {
                return Err(format!(
                    "{at}: label is text of at most {MAX_EVENT_LABEL} characters"
                ));
            }
        }
        if let Some(e) = m.get("schema").and_then(|s| schema_problem(s, 1)) {
            return Err(format!("{at}: {e}"));
        }
    }
    Ok(list)
}

// ---------------------------------------------------------------------------------------------
// The preview (`--summary`, `--metrics`)
// ---------------------------------------------------------------------------------------------

/// The `preview` of a version from `--summary` and the metrics.json of `--metrics` (a list, or
/// `{"metrics": [...]}`): what the 产出's card in the conversation shows without opening the
/// page. `None` when neither is given.
pub fn preview(summary: Option<&str>, metrics: Option<Value>) -> Result<Option<Value>, String> {
    let mut out = serde_json::Map::new();
    if let Some(s) = summary {
        let s = s.trim();
        if s.is_empty() || s.chars().count() > MAX_SUMMARY {
            return Err(format!("--summary needs 1-{MAX_SUMMARY} characters"));
        }
        out.insert("summary".into(), json!(s));
    }
    if let Some(v) = metrics {
        let list = match v {
            Value::Object(mut m) if m.contains_key("metrics") => {
                m.remove("metrics").unwrap_or_default()
            }
            v => v,
        };
        let shape = "metrics.json must hold a list: [{\"label\": \"报名\", \"value\": \"12 位\", \
\"delta\": \"+3\"}]";
        let Some(items) = list.as_array() else {
            return Err(shape.into());
        };
        if items.len() > MAX_METRICS {
            return Err(format!(
                "metrics.json has {} figures: at most {MAX_METRICS}",
                items.len()
            ));
        }
        for (i, item) in items.iter().enumerate() {
            let Some(m) = item.as_object() else {
                return Err(format!("metric {i}: {shape}"));
            };
            if let Some(k) = m
                .keys()
                .find(|k| !matches!(k.as_str(), "label" | "value" | "delta"))
            {
                return Err(format!(
                    "metric {i}: unknown key {k} (a metric has label, value, delta)"
                ));
            }
            for (k, required) in [("label", true), ("value", true), ("delta", false)] {
                let ok = match m.get(k) {
                    None => !required,
                    Some(Value::String(s)) => !s.is_empty() && s.chars().count() <= MAX_METRIC_TEXT,
                    Some(_) => false,
                };
                if !ok {
                    return Err(format!(
                        "metric {i}: {k} is text of 1-{MAX_METRIC_TEXT} characters (a number is \
written as text: \"12\")"
                    ));
                }
            }
        }
        out.insert("metrics".into(), list);
    }
    Ok((!out.is_empty()).then_some(Value::Object(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(path: &str) -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn the_guide_says_the_numbers_of_the_code() {
        // Each limit the guide prints is the constant of its source.
        let files = source("packages/protocol/src/files.ts");
        let bridge = source("crates/artifact-view/src/bridge.rs");
        let gate = source("crates/artifact-view/src/gate.rs");
        let server = source("server/src/files/bridge.ts");
        for (src, needle) in [
            (&files, format!("MAX_ARTIFACT_EVENTS = {MAX_EVENTS};")),
            (
                &files,
                format!(
                    "ARTIFACT_EVENT_NAME_RE = /^[a-z][A-Za-z0-9_.-]{{0,{MAX_EVENT_NAME_TAIL}}}$/;"
                ),
            ),
            (
                &files,
                format!(
                    "MAX_ARTIFACT_EVENT_PAYLOAD_BYTES = {};",
                    fmt(PAYLOAD_KIB * 1024)
                ),
            ),
            (
                &files,
                format!("MAX_ARTIFACT_REPLY_BYTES = {};", fmt(REPLY_KIB * 1024)),
            ),
            (
                &files,
                format!("MAX_ARTIFACT_DATA_BYTES = {};", fmt(DATA_KIB * 1024)),
            ),
            (
                &files,
                format!("label: z.string().max({MAX_EVENT_LABEL}).optional()"),
            ),
            (&files, format!("if (depth > {MAX_SCHEMA_DEPTH}) return")),
            (&files, format!("v.length > {MAX_SCHEMA_PATTERN})")),
            (
                &files,
                format!("summary: z.string().min(1).max({MAX_SUMMARY}).optional()"),
            ),
            (&files, format!("MAX_ARTIFACT_METRICS = {MAX_METRICS};")),
            (
                &files,
                format!("label: z.string().min(1).max({MAX_METRIC_TEXT}),"),
            ),
            (
                &files,
                format!("value: z.string().min(1).max({MAX_METRIC_TEXT}),"),
            ),
            (
                &files,
                format!("delta: z.string().min(1).max({MAX_METRIC_TEXT}).optional()"),
            ),
            (
                &bridge,
                format!("MAX_PAYLOAD_BYTES: usize = {PAYLOAD_KIB} * 1024;"),
            ),
            (&bridge, format!("MAX_OPEN_ASKS: usize = {OPEN_ASKS};")),
            (
                &bridge,
                format!("MAX_ERROR_MESSAGE_CHARS: usize = {ERROR_MESSAGE_CHARS};"),
            ),
            (
                &bridge,
                format!("ASK_TIMEOUT_DEFAULT_MS: u64 = {ASK_DEFAULT_S}_000;"),
            ),
            (
                &bridge,
                format!("ASK_TIMEOUT_MAX_MS: u64 = {ASK_MAX_S}_000;"),
            ),
            (
                &gate,
                format!("USER_ACTION_WINDOW: Duration = Duration::from_secs({USER_ACTION_S});"),
            ),
            (&server, format!("DATA_RATE = {{ rate: {DATA_PER_S},")),
            (&server, format!("REPLY_WINDOW_MS = {REPLY_WINDOW_S}_000;")),
            (&server, format!("CAUSE_WINDOW_MS = {CAUSE_MIN} * 60_000;")),
        ] {
            assert!(src.contains(&needle), "the source no longer says: {needle}");
        }
        let keys = files
            .split("const TC_SCHEMA_KEYS = new Set([")
            .nth(1)
            .and_then(|s| s.split("]);").next())
            .unwrap();
        let theirs: Vec<&str> = keys.split('"').skip(1).step_by(2).collect();
        assert_eq!(theirs, SCHEMA_KEYS);
        // The error codes of the page API, the methods and the theme variables.
        let g = guide();
        let codes = bridge
            .split("pub fn code(self)")
            .nth(1)
            .and_then(|s| s.split("\n    }\n").next())
            .unwrap();
        for line in codes
            .lines()
            .filter(|l| l.trim_start().starts_with("Reject::") && l.contains("=> \""))
        {
            let code = line.split('"').nth(1).unwrap();
            assert!(g.contains(code), "the guide does not name the code {code}");
        }
        let api = bridge
            .split("Object.defineProperty(window, 'teamchat'")
            .nth(1)
            .unwrap();
        for member in ["viewer", "can", "getData", "onData", "emit", "ask"] {
            assert!(
                api.contains(&format!("{member}:")) && g.contains(member),
                "{member}"
            );
        }
        let css = source("apps/desktop/src/ui/web_embed.rs");
        for var in [
            "--tc-fg",
            "--tc-muted",
            "--tc-border",
            "--tc-fill",
            "--tc-bg",
            "--tc-primary",
            "--tc-danger",
            "--tc-radius",
            "--tc-gap",
            "--tc-font-mono",
            "--tc-pad",
        ] {
            assert!(
                css.contains(&format!("{var}:")) && g.contains(&format!("var({var})")),
                "{var}"
            );
        }
        // The variables a page might guess are not there, and the guide says so.
        for var in ["--tc-ok", "--tc-warn"] {
            assert!(!css.contains(var) && g.contains(var), "{var}");
        }
        assert!(css.contains("button[data-tc=primary]") && g.contains("button[data-tc=primary]"));
        // The dialogs are stubs that return exactly what the guide says.
        assert!(bridge.contains("[['alert', undefined], ['confirm', false], ['prompt', null]]"));
        assert!(g.contains("return undefined, false and null"));
        // The sandbox it describes is the content policy of the page host.
        let policy = source("crates/artifact-view/src/resolve.rs");
        for rule in [
            "connect-src 'none'",
            "frame-src 'none'",
            "worker-src 'none'",
            "form-action 'none'",
            "default-src 'none'",
            "img-src {me} data: blob:",
            "media-src {me} data: blob:",
        ] {
            assert!(policy.contains(rule), "{rule}");
        }
        assert!(g.contains("blob: URL"));
        assert!(source("crates/artifact-view/src/mac.rs").contains("nonPersistentDataStore"));
        // Every command it names is in the usage.
        for cmd in [
            "chat-run artifact example",
            "chat-run artifact create",
            "chat-run artifact data",
            "chat-run artifact reply",
            "chat-run artifact version",
        ] {
            assert!(
                g.contains(cmd) && crate::chat_run::USAGE.contains(cmd),
                "{cmd}"
            );
        }
        for flag in [
            "--events",
            "--data",
            "--keep-data",
            "--summary",
            "--base-data-rev",
            "--cause",
            "--decline",
        ] {
            assert!(
                g.contains(flag) && crate::chat_run::USAGE.contains(flag),
                "{flag}"
            );
        }
        assert!(g.lines().count() <= 70, "{} lines", g.lines().count());
    }

    fn fmt(n: usize) -> String {
        // 16384 as the TypeScript sources write it: 16_384.
        let s = n.to_string();
        let (head, tail) = s.split_at(s.len() - 3);
        format!("{head}_{tail}")
    }

    #[test]
    fn the_starter_page_is_what_the_guide_asks_for() {
        // Its manifests pass the checks of the CLI.
        let decls = events(serde_json::from_str(EXAMPLE_EVENTS).unwrap()).unwrap();
        let decls = decls.as_array().unwrap();
        assert_eq!(decls.len(), 2);
        let data: Value = serde_json::from_str(EXAMPLE_DATA).unwrap();
        assert!(data["items"].is_array());
        serde_json::from_str::<Value>(EXAMPLE_REPLY).unwrap();
        // The page sends exactly the declared events, of the declared kind.
        for d in decls {
            let call = format!(
                "tc.{}('{}'",
                d["kind"].as_str().unwrap(),
                d["name"].as_str().unwrap()
            );
            assert!(EXAMPLE_PAGE.contains(&call), "{call}");
        }
        assert_eq!(
            EXAMPLE_PAGE.matches("tc.emit(").count() + EXAMPLE_PAGE.matches("tc.ask(").count(),
            2
        );
        // It follows the rules: no network, no fixed colors, a guard, state from onData.
        let lower = EXAMPLE_PAGE.to_lowercase();
        for banned in [
            "http://",
            "https://",
            "fetch(",
            "xmlhttprequest",
            "websocket",
            "<iframe",
            "localstorage",
            "#000",
            "#fff",
            "black",
            "white",
            "font-family",
            "action=",
        ] {
            assert!(!lower.contains(banned), "{banned}");
        }
        for needed in [
            "window.teamchat",
            "tc.can.emit",
            "tc.can.ask",
            "tc.onData(",
            ".catch(",
            "var(--tc-border",
            "var(--tc-muted",
            "preventDefault()",
            "width=device-width",
            // A viewer who may only read does not see the form: `hidden` must win over the
            // page's own display rule.
            "form:not([hidden])",
            // The ask waits as long as the member's turn may take.
            "{ timeoutMs: 120000 }",
        ] {
            assert!(EXAMPLE_PAGE.contains(needed), "{needed}");
        }
        assert!(
            EXAMPLE_PAGE.lines().count() <= 70,
            "{}",
            EXAMPLE_PAGE.lines().count()
        );
    }

    #[test]
    fn artifact_example_prints_or_writes_the_starter() {
        let run = |a: &[&str]| local(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let printed = run(&["artifact", "example"]).unwrap().unwrap();
        for (path, _) in EXAMPLE_FILES {
            assert!(printed.contains(&format!("===== {path} =====")), "{path}");
        }
        assert!(
            run(&["artifact", "guide"])
                .unwrap()
                .unwrap()
                .contains("window.teamchat")
        );
        assert!(run(&["artifact", "guide", "--x"]).unwrap().is_err());
        assert!(run(&["artifact", "example", "--nope"]).unwrap().is_err());
        assert!(run(&["artifact", "create"]).is_none());
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("starter");
        let said = run(&["artifact", "example", "--out", out.to_str().unwrap()])
            .unwrap()
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("page/index.html")).unwrap(),
            EXAMPLE_PAGE
        );
        assert_eq!(
            std::fs::read_to_string(out.join("events.json")).unwrap(),
            EXAMPLE_EVENTS
        );
        // Only the page is in the folder that becomes the 产出.
        assert_eq!(std::fs::read_dir(out.join("page")).unwrap().count(), 1);
        let create = format!(
            "chat-run artifact create --title \"发布前清单\" --dir {0}/page --events {0}/events.json --data {0}/data.json",
            out.display()
        );
        assert!(said.contains(&create), "{said}");
        // The command it prints is one the CLI takes.
        let line = said
            .lines()
            .find(|l| l.contains("artifact create"))
            .unwrap()
            .trim();
        let words: Vec<String> = line
            .split_whitespace()
            .skip(1)
            .map(|w| w.trim_matches('"').to_string())
            .collect();
        let (op, body) = crate::chat_run::request(&words).unwrap();
        assert_eq!(op, crate::chat_run_files::ARTIFACT_CREATE);
        assert_eq!(body["events"][1]["kind"], "ask");
        assert_eq!(body["data"]["items"][0]["id"], "a");
        assert_eq!(body["preview"]["summary"], "发布前要做完的三件事");
        // A second run does not overwrite.
        let again = run(&["artifact", "example", &format!("--out={}", out.display())])
            .unwrap()
            .unwrap_err();
        assert!(again.contains("already there"), "{again}");
    }

    #[test]
    fn events_json_is_checked_before_the_upload() {
        let ok = json!([{ "name": "toggle", "kind": "emit" }, { "name": "q.ask", "kind": "ask", "label": "问", "schema": { "type": "object", "properties": { "a": { "type": "string", "maxLength": 3 } }, "required": ["a"] } }]);
        assert_eq!(events(ok.clone()).unwrap(), ok);
        // The wrapper of card.json and ops.json works here too.
        assert_eq!(events(json!({ "events": ok.clone() })).unwrap(), ok);
        let err = |v: Value| events(v).unwrap_err();
        let e = err(json!([{ "name": "toggle", "type": "ask" }]));
        assert_eq!(
            e,
            "event toggle: unknown key type (an event has name, kind, label, schema); say what the event is with \"kind\": \"emit\" or \"ask\""
        );
        assert!(err(json!([{ "name": "Toggle" }])).contains("lower-case letter"));
        assert!(err(json!([{ "kind": "emit" }])).starts_with("event 0: name must be"));
        assert!(err(json!([{ "name": "a" }, { "name": "a" }])).contains("event a: declared twice"));
        assert!(
            err(json!([{ "name": "a", "kind": "request" }]))
                .contains("kind must be \"emit\" or \"ask\"")
        );
        assert!(err(json!([{ "name": "a", "label": 3 }])).contains("label is text"));
        let e = err(
            json!([{ "name": "a", "schema": { "type": "object", "additionalProperties": false } }]),
        );
        assert!(e.contains("schema key additionalProperties is not supported; a schema takes type, properties, required, enum"), "{e}");
        let deep = json!({ "properties": { "a": { "properties": { "b": { "properties": { "c": { "properties": { "d": {} } } } } } } } });
        assert!(err(json!([{ "name": "a", "schema": deep }])).contains("nested deeper than 4"));
        assert!(err(json!({ "name": "a" })).contains("must hold a list"));
        assert!(err(json!(["a"])).contains("event 0: must be an object"));
        let many: Vec<Value> = (0..21)
            .map(|i| json!({ "name": format!("e{i}") }))
            .collect();
        assert!(err(json!(many)).contains("at most 20"));
    }

    #[test]
    fn the_preview_is_checked_and_built() {
        assert_eq!(preview(None, None).unwrap(), None);
        let metrics = json!([{ "label": "报名", "value": "12 位", "delta": "+3" }, { "label": "满意度", "value": "4.6" }]);
        assert_eq!(
            preview(Some(" 报名情况 "), Some(metrics.clone())).unwrap(),
            Some(json!({ "summary": "报名情况", "metrics": metrics }))
        );
        assert_eq!(
            preview(None, Some(json!({ "metrics": [] }))).unwrap(),
            Some(json!({ "metrics": [] }))
        );
        let err = |s: Option<&str>, m: Option<Value>| preview(s, m).unwrap_err();
        assert!(err(Some(""), None).contains("1-500"));
        assert!(err(Some(&"字".repeat(501)), None).contains("1-500"));
        assert!(err(None, Some(json!("x"))).contains("must hold a list"));
        assert!(
            err(None, Some(json!([{ "label": "a", "value": 12 }])))
                .contains("metric 0: value is text of 1-40")
        );
        assert!(err(None, Some(json!([{ "label": "a" }]))).contains("metric 0: value"));
        assert!(
            err(
                None,
                Some(json!([{ "label": "a", "value": "1", "unit": "x" }]))
            )
            .contains("unknown key unit")
        );
        assert!(
            err(
                None,
                Some(json!([{ "label": "字".repeat(41), "value": "1" }]))
            )
            .contains("metric 0: label")
        );
        let seven: Vec<Value> = (0..7)
            .map(|_| json!({ "label": "a", "value": "1" }))
            .collect();
        assert!(err(None, Some(json!(seven))).contains("at most 6"));
    }
}
