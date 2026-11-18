//! What `chat-run` knows about cards without the daemon: the schema in pieces (`card schema`),
//! working cards (`card example`), a check before posting (`card check`), and what `card post`
//! fills in. The server stays the authority: nothing here refuses a card it would take.

use chat_protocol::card2::{CardV2, CardV2Block, MAX_PATCH_OPS, PatchOp};
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

/// The JSON Schema of a Card v2 document (`protocol/schema/CardV2.schema.json`, made from the
/// protocol package's Zod with `bun run gen:protocol`): the shape; the server checks the write
/// rules too. Each block type is `$defs.<type>`; `$defs.block` is their union.
pub const CARD_SCHEMA: &str = include_str!("../../../protocol/schema/CardV2.schema.json");
/// The JSON Schema of `ops.json` (`protocol/schema/PatchOp.schema.json`). It names the parts it
/// shares with the card as `CardV2.schema.json#/...`.
pub const OPS_SCHEMA: &str = include_str!("../../../protocol/schema/PatchOp.schema.json");

fn schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| serde_json::from_str(CARD_SCHEMA).unwrap_or_default())
}

/// The block types of [`CARD_SCHEMA`], in catalog order.
pub fn block_types() -> &'static [String] {
    static TYPES: OnceLock<Vec<String>> = OnceLock::new();
    TYPES.get_or_init(|| {
        schema()["$defs"]["block"]["oneOf"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|b| b["$ref"].as_str()?.strip_prefix("#/$defs/"))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// The block types by what they are for: the list the run note prints. Every type of
/// [`block_types`] is in exactly one group (a test holds this).
pub const BLOCK_GROUPS: &[(&str, &[&str])] = &[
    (
        "ask",
        &[
            "decision",
            "confirm",
            "input",
            "questionnaire",
            "poll",
            "rank",
            "rating",
            "scale",
            "slots",
            "matrix",
            "imageChoice",
            "compare",
            "draft",
        ],
    ),
    (
        "show",
        &[
            "text", "metric", "chart", "table", "fields", "diff", "evidence", "plan", "progress",
            "list", "code", "image", "diagram", "embed", "link", "ref", "divider",
        ],
    ),
    ("act", &["buttons"]),
    ("lay out", &["group", "stack", "row", "tabs"]),
];

/// `Ask: decision, confirm, ...; show: ...`: [`BLOCK_GROUPS`] as one sentence.
pub fn block_groups() -> String {
    BLOCK_GROUPS
        .iter()
        .map(|(what, types)| format!("{what}: {}", types.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The live events each block type can send (server `eventsOf`, `server/src/cards2/inputs.ts`;
/// a test reads that file). An `input` of kind text or number also sends `input.submit`.
pub const BLOCK_EVENTS: &[(&str, &[&str])] = &[
    ("decision", &["choice.change", "choice.submit"]),
    ("imageChoice", &["choice.change", "choice.submit"]),
    ("slots", &["choice.change", "choice.submit"]),
    ("poll", &["choice.change", "choice.submit"]),
    ("confirm", &["choice.change", "choice.submit"]),
    ("rank", &["input.change"]),
    ("matrix", &["input.change"]),
    ("rating", &["input.change"]),
    ("scale", &["input.change"]),
    ("input", &["input.change"]),
    ("chart", &["chart.select", "chart.clear"]),
    ("metric", &["metric.press"]),
    ("table", &["table.rowSelect", "table.sort"]),
    ("buttons", &["button.press"]),
    ("draft", &["draft.edit"]),
    ("compare", &["compare.pick"]),
    ("tabs", &["tab.open"]),
    ("questionnaire", &["action.press"]),
];
/// The extra event of an `input` block of kind text or number.
pub const INPUT_SUBMIT: &str = "input.submit";

/// The events a block (its JSON) can send.
fn events_of(block: &Value) -> Vec<&'static str> {
    let t = block["type"].as_str().unwrap_or_default();
    let mut out: Vec<&'static str> = BLOCK_EVENTS
        .iter()
        .find(|(name, _)| *name == t)
        .map(|(_, e)| e.to_vec())
        .unwrap_or_default();
    if t == "input" && matches!(block["kind"].as_str(), Some("text" | "number")) {
        out.push(INPUT_SUBMIT);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// card schema
// ---------------------------------------------------------------------------------------------

/// `v` with every `$ref` to a shared part written out. Block types and `json` (any JSON value,
/// which names itself) stay references.
fn inline(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            if let Some(name) = m.get("$ref").and_then(Value::as_str)
                && let Some(name) = name.strip_prefix("#/$defs/")
                && name != "json"
                && name != "block"
                && !block_types().iter().any(|t| t == name)
                && let Some(def) = schema()["$defs"].get(name)
            {
                return inline(def);
            }
            Value::Object(m.iter().map(|(k, x)| (k.clone(), inline(x))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(inline).collect()),
        _ => v.clone(),
    }
}

/// A few words for the values a field takes.
fn brief(s: &Value) -> String {
    let s = inline(s);
    if let Some(c) = s.get("const") {
        return c.to_string();
    }
    if let Some(e) = s["enum"].as_array() {
        return e
            .iter()
            .map(|x| x.as_str().map(str::to_string).unwrap_or(x.to_string()))
            .collect::<Vec<_>>()
            .join("|");
    }
    if s["$ref"] == "#/$defs/block" {
        return "block".into();
    }
    if s["$ref"] == "#/$defs/json" {
        return "any JSON".into();
    }
    match s["type"].as_str() {
        Some("array") => format!("list of {}", brief(&s["items"])),
        Some(t) => t.to_string(),
        None => "object".into(),
    }
}

/// The fields of the card in reading order (a test holds it against the schema).
const CARD_FIELDS: &[&str] = &[
    "v", "catalog", "rev", "intent", "status", "title", "summary", "fallback", "ask", "blocks",
    "actions", "data",
];
/// The fields every block has.
const BASE_FIELDS: &[&str] = &[
    "id",
    "emphasis",
    "events",
    "deliver",
    "emitBy",
    "fallbackText",
];

/// `chat-run card schema`: the card's fields and one line per block type.
pub fn schema_index() -> String {
    let s = schema();
    let required = |of: &Value, k: &str| {
        of["required"]
            .as_array()
            .is_some_and(|r| r.iter().any(|x| x == k))
    };
    let mut out = String::from("A card is one JSON object (* = required):\n");
    for k in CARD_FIELDS {
        let f = &s["properties"][*k];
        let star = if required(s, k) { "*" } else { "" };
        let about = f["description"]
            .as_str()
            .map(|d| format!(". {d}"))
            .unwrap_or_default();
        out.push_str(&format!("  {k}{star}: {}{about}\n", brief(f)));
    }
    out.push_str(
        "\nBlocks (every block has type* and id*; it may have emphasis, events, deliver, emitBy, \
fallbackText). After the type: its required fields.\n",
    );
    for t in block_types() {
        let def = &s["$defs"][t];
        let needs: Vec<&str> = def["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|k| !matches!(*k, "type" | "id"))
            .collect();
        let about = def["description"].as_str().unwrap_or_default();
        let needs = if needs.is_empty() {
            String::new()
        } else {
            format!(" ({})", needs.join(", "))
        };
        out.push_str(&format!("  {t}{needs}: {about}\n"));
    }
    out.push_str(
        "\n`chat-run card schema --block <type>` prints one block's JSON Schema, `--ops` the patch \
ops of ops.json, `--full` the whole JSON Schema. `chat-run card example` lists working cards; \
`chat-run card check --file card.json` checks yours before you post it.",
    );
    out
}

/// `chat-run card schema --block <type>`: that block's JSON Schema, complete in itself.
pub fn block_schema(t: &str) -> Result<String, String> {
    if !block_types().iter().any(|x| x == t) {
        return Err(format!(
            "unknown block type {t}; block types: {}",
            block_types().join(", ")
        ));
    }
    let def = inline(&schema()["$defs"][t]);
    serde_json::to_string_pretty(&def).map_err(|e| e.to_string())
}

/// `chat-run card schema --full`: the whole schema on one line.
pub fn full_schema() -> String {
    schema().to_string()
}

// ---------------------------------------------------------------------------------------------
// card example
// ---------------------------------------------------------------------------------------------

/// One working card (or ops.json): its kind, what it shows and the JSON text. The files are
/// fixtures of the protocol too (`packages/protocol/scripts/fixtures-examples.ts`).
pub struct Example {
    pub kind: &'static str,
    pub about: &'static str,
    pub text: &'static str,
}

macro_rules! example {
    ($kind:literal, $about:literal) => {
        Example {
            kind: $kind,
            about: $about,
            text: include_str!(concat!("../examples/cards/", $kind, ".json")),
        }
    };
}

pub const EXAMPLES: &[Example] = &[
    example!(
        "ask",
        "a choice for the person you work for, with the option you would pick"
    ),
    example!("form", "values to fill in, sent with one submit action"),
    example!(
        "plan",
        "a plan people agree to before you start (intent ask, a plan block)"
    ),
    example!(
        "review",
        "a change to approve: a diff, its proof, approve and reject"
    ),
    example!("data", "numbers to read: a metric, a chart and a table"),
    example!(
        "evidence",
        "what was checked and what shows it: evidence lines and a reference"
    ),
    example!(
        "progress",
        "long work people follow: a working card you patch"
    ),
    example!(
        "live",
        "a card people act on: a chart and buttons that send you events"
    ),
    example!(
        "patch",
        "ops.json for `card patch`: the next state of the `progress` card"
    ),
];

/// `chat-run card example [kind]`: the list of kinds, or one example's JSON.
pub fn example(kind: Option<&str>) -> Result<String, String> {
    let Some(kind) = kind else {
        let mut out = String::from("chat-run card example <kind> prints a working card:\n");
        for e in EXAMPLES {
            out.push_str(&format!("  {}: {}\n", e.kind, e.about));
        }
        out.push_str(
            "Save one, change it, check it and post it: `chat-run card example ask > card.json`, \
`chat-run card check --file card.json`, `chat-run card post --file card.json`.",
        );
        return Ok(out);
    };
    EXAMPLES
        .iter()
        .find(|e| e.kind == kind)
        .map(|e| e.text.trim_end().to_string())
        .ok_or_else(|| {
            format!(
                "unknown example {kind}; examples: {}",
                EXAMPLES
                    .iter()
                    .map(|e| e.kind)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

// ---------------------------------------------------------------------------------------------
// What `card post` fills in
// ---------------------------------------------------------------------------------------------

/// Longest `fallback` (protocol `Text(4000)`).
const MAX_FALLBACK_CHARS: usize = 4000;

/// Fills in what a card file may leave out, and says what it did that the author should know:
/// `rev` (the server sets 1 on post anyway), `fallback` (the title, the summary and the first
/// text block) and, on an `ask` card without `ask`, `ask: {"from": ["requester"]}` (without it
/// nobody is asked and the topic does not wait).
pub fn complete(card: &mut Value) -> Vec<String> {
    let mut said = vec![];
    let Some(m) = card.as_object_mut() else {
        return said;
    };
    if !m.contains_key("rev") {
        m.insert("rev".into(), json!(1));
    }
    if !m.contains_key("fallback") {
        let first_text = m
            .get("blocks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|b| b["type"] == "text")
            .and_then(|b| b["text"].as_str());
        let parts: Vec<&str> = [
            m.get("title").and_then(Value::as_str),
            m.get("summary").and_then(Value::as_str),
            first_text,
        ]
        .into_iter()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .collect();
        let text: String = parts.join("\n").chars().take(MAX_FALLBACK_CHARS).collect();
        m.insert("fallback".into(), json!(text));
    }
    if m.get("intent").and_then(Value::as_str) == Some("ask")
        && m.get("ask").is_none_or(Value::is_null)
    {
        m.insert("ask".into(), json!({ "from": ["requester"] }));
        said.push(
            "the card had intent \"ask\" and no `ask`: set ask {\"from\": [\"requester\"]} (the \
person you work for is asked; list member ids or \"owners\" to ask others)"
                .into(),
        );
    }
    said
}

// ---------------------------------------------------------------------------------------------
// card check
// ---------------------------------------------------------------------------------------------

/// What a check found: `problems` the server refuses, `warnings` it takes but that do not do
/// what they seem to, and an outline of what will show.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Check {
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    pub outline: Vec<String>,
}

impl Check {
    /// The report `chat-run card check` prints.
    pub fn report(&self, what: &str) -> String {
        let count = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        let mut out = format!(
            "{what}: {}, {}\n",
            count(self.problems.len(), "problem"),
            count(self.warnings.len(), "warning")
        );
        for p in &self.problems {
            out.push_str(&format!("problem: {p}\n"));
        }
        for w in &self.warnings {
            out.push_str(&format!("warning: {w}\n"));
        }
        if !self.outline.is_empty() {
            out.push_str("what it shows:\n");
            for l in &self.outline {
                out.push_str(&format!("  {l}\n"));
            }
        }
        if self.problems.is_empty() {
            out.push_str("ok: the local check found nothing the server refuses.");
        } else {
            out.push_str("The server would refuse this. Fix the problems and check again.");
        }
        out
    }

    /// `v` (the JSON `chat-run` prints) with what the check found, when it found something.
    pub fn onto(&self, mut v: Value) -> Value {
        if let Some(m) = v.as_object_mut() {
            if !self.problems.is_empty() {
                m.insert("problems".into(), json!(self.problems));
            }
            if !self.warnings.is_empty() {
                m.insert("warnings".into(), json!(self.warnings));
            }
        }
        v
    }
}

/// `blocks.2 (rating conf)`: how a problem names a block.
fn block_name(path: &str, b: &Value) -> String {
    let t = b["type"].as_str().unwrap_or("?");
    match b["id"].as_str() {
        Some(id) => format!("{path} ({t} {id})"),
        None => format!("{path} ({t})"),
    }
}

/// What to do about a parse error of a block.
fn block_hint(b: &Value, e: &str) -> String {
    let t = b["type"].as_str().unwrap_or_default();
    if !block_types().iter().any(|x| x == t) {
        return format!(
            "unknown block type \"{t}\"; block types: {}",
            block_types().join(", ")
        );
    }
    let hint = if e.contains("missing field `id`") {
        "; every block needs a unique id (1..64 of A-Za-z0-9_.:-)".to_string()
    } else {
        format!("; `chat-run card schema --block {t}` prints its fields")
    };
    format!("{e}{hint}")
}

/// The schema of `v` among alternatives: the one whose constants (`type`, `kind`, `op`) match.
fn alternative<'a>(s: &'a Value, v: &Map<String, Value>) -> Option<&'a Value> {
    let options = s["oneOf"].as_array().or_else(|| s["anyOf"].as_array())?;
    options.iter().map(deref).find(|o| {
        o["properties"].as_object().is_some_and(|p| {
            p.iter()
                .all(|(k, f)| f.get("const").is_none_or(|c| v.get(k) == Some(c)))
        })
    })
}

/// A schema with its `$ref` followed (into the card schema, also from the ops schema).
fn deref(s: &Value) -> &Value {
    let Some(r) = s["$ref"].as_str() else {
        return s;
    };
    let pointer = r.rsplit_once('#').map_or(r, |(_, p)| p);
    schema().pointer(pointer).map_or(s, deref)
}

/// The fields of `v` that the schema `s` does not have, with what it has instead. Unknown
/// fields are dropped by the server without a word, so a wrong name posts an empty block.
fn unknown_fields(v: &Value, s: &Value, path: &str, out: &mut Vec<String>) {
    let s = deref(s);
    match v {
        Value::Object(m) => {
            let s = if s.get("properties").is_some() {
                s
            } else if let Some(a) = alternative(s, m) {
                a
            } else {
                return;
            };
            let Some(props) = s["properties"].as_object() else {
                return;
            };
            for (k, x) in m {
                let at = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                match props.get(k) {
                    Some(f) => unknown_fields(x, f, &at, out),
                    None => {
                        let has: Vec<&str> = props
                            .keys()
                            .map(String::as_str)
                            .filter(|k| *k != "type" && !BASE_FIELDS.contains(k))
                            .collect();
                        let of = m
                            .get("type")
                            .or(m.get("op"))
                            .and_then(Value::as_str)
                            .unwrap_or("it");
                        out.push(format!(
                            "{at}: unknown field, the server drops it ({of} has: {})",
                            has.join(", ")
                        ));
                    }
                }
            }
        }
        Value::Array(a) => {
            if let Some(items) = s.get("items") {
                for (i, x) in a.iter().enumerate() {
                    unknown_fields(x, items, &format!("{path}.{i}"), out);
                }
            }
        }
        _ => {}
    }
}

/// A card around one block, to ask the protocol crate about that block alone.
fn probe(block: &Value) -> Option<CardV2> {
    serde_json::from_value(json!({
        "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "info", "title": "t",
        "fallback": "", "blocks": [block],
    }))
    .ok()
}

/// What a block declares that does nothing, or shows nothing.
fn block_warnings(b: &Value, name: &str, out: &mut Vec<String>) {
    let t = b["type"].as_str().unwrap_or_default();
    let can = events_of(b);
    for e in b["events"].as_array().into_iter().flatten() {
        let e = e.as_str().unwrap_or_default();
        if !can.contains(&e) {
            let sends = if can.is_empty() {
                format!("a {t} block sends no events")
            } else {
                format!("a {t} block sends: {}", can.join(", "))
            };
            out.push(format!(
                "{name}: events lists \"{e}\", which never comes ({sends})"
            ));
        }
    }
    let listed = |e: &str| {
        b["events"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == e))
    };
    match t {
        "buttons" if !listed("button.press") => out.push(format!(
            "{name}: the buttons send nothing: add \"events\": [\"button.press\"]"
        )),
        "plan" if b["items"].as_array().is_none_or(Vec::is_empty) && b["runId"].is_null() => out
            .push(format!(
                "{name}: the plan has no items, so it shows nothing (items: [{{\"text\", \
\"state\": \"done\"|\"current\"|\"waiting\"|\"pending\"}}])"
            )),
        "input"
            if matches!(b["kind"].as_str(), Some("select" | "chips"))
                && b["options"].as_array().is_none_or(Vec::is_empty) =>
        {
            out.push(format!(
                "{name}: a {} input without options has nothing to choose",
                b["kind"].as_str().unwrap_or_default()
            ))
        }
        "chart" => {
            if let Some(x) = b["x"].as_array() {
                for s in b["series"].as_array().into_iter().flatten() {
                    let n = s["values"].as_array().map_or(0, Vec::len);
                    if n != x.len() {
                        out.push(format!(
                            "{name}: series \"{}\" has {n} values for {} labels of x",
                            s["name"].as_str().unwrap_or_default(),
                            x.len()
                        ));
                    }
                }
            }
        }
        _ => {}
    }
}

fn cut(s: &str, max: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        format!("{}…", one_line.chars().take(max).collect::<String>())
    }
}

/// One line of the outline for a block: its type, id and the text people see first.
fn block_line(b: &Value) -> String {
    let t = b["type"].as_str().unwrap_or("?");
    let id = b["id"].as_str().unwrap_or("?");
    let words = ["label", "title", "question", "text", "summary"]
        .iter()
        .find_map(|k| b[*k].as_str().filter(|s| !s.trim().is_empty()));
    let extra = match t {
        "decision" | "poll" | "imageChoice" | "compare" => b["options"]
            .as_array()
            .map(|o| format!(" [{} options]", o.len())),
        "table" => b["rows"].as_array().map(|r| format!(" [{} rows]", r.len())),
        "plan" => b["items"]
            .as_array()
            .map(|r| format!(" [{} steps]", r.len())),
        "progress" => b["value"]
            .as_f64()
            .map(|v| format!(" [{:.0} %]", v * 100.0)),
        "metric" => b["value"].as_f64().map(|v| {
            format!(
                " [{v}{}]",
                b["unit"]
                    .as_str()
                    .map(|u| format!(" {u}"))
                    .unwrap_or_default()
            )
        }),
        "chart" => b["kind"].as_str().map(|k| format!(" [{k}]")),
        "input" => b["kind"].as_str().map(|k| format!(" [{k}]")),
        _ => None,
    }
    .unwrap_or_default();
    match words {
        Some(w) => format!("{t} {id}: {}{extra}", cut(w, 60)),
        None => format!("{t} {id}{extra}"),
    }
}

/// Checks a card (the document, not the `{cardV2}` wrapper) as `card post` would send it.
pub fn check_card(card: &Value) -> Check {
    let mut c = Check::default();
    let mut card = card.clone();
    if !card.is_object() {
        c.problems
            .push("the card file must hold a JSON object".into());
        return c;
    }
    c.warnings.extend(complete(&mut card));
    unknown_fields(&card, schema(), "", &mut c.warnings);
    let blocks: Vec<Value> = card["blocks"].as_array().cloned().unwrap_or_default();
    if !card["blocks"].is_array() {
        c.problems
            .push("blocks is required: a list of blocks (it may be empty)".into());
    }
    // Each block by itself, so one wrong block does not hide the next.
    for (i, b) in blocks.iter().enumerate() {
        let name = block_name(&format!("blocks.{i}"), b);
        match serde_json::from_value::<CardV2Block>(b.clone()) {
            Err(e) => c
                .problems
                .push(format!("{name}: {}", block_hint(b, &e.to_string()))),
            Ok(CardV2Block::Unknown) => c.problems.push(format!("{name}: {}", block_hint(b, ""))),
            Ok(parsed) => {
                if !parsed.is_container()
                    && let Some(p) = probe(b).and_then(|x| x.problem())
                {
                    c.problems
                        .push(format!("{name}: {}: {}", p.reason, p.message));
                }
                block_warnings(b, &name, &mut c.warnings);
            }
        }
    }
    // The card as a whole: the first write rule it breaks (what the server would name).
    if c.problems.is_empty() {
        match serde_json::from_value::<CardV2>(card.clone()) {
            Err(e) => c.problems.push(format!(
                "{e}; a card needs v: 2, intent, title and blocks (`chat-run card schema`)"
            )),
            Ok(parsed) => {
                if let Some(p) = parsed.problem() {
                    c.problems.push(format!("{}: {}", p.reason, p.message));
                }
                card_warnings(&card, &parsed, &mut c.warnings);
            }
        }
    }
    // What will show.
    let intent = card["intent"].as_str().unwrap_or("?");
    c.outline.push(format!(
        "{intent} card \"{}\"",
        cut(card["title"].as_str().unwrap_or_default(), 60)
    ));
    if let Some(from) = card["ask"]["from"].as_array() {
        let who: Vec<&str> = from.iter().filter_map(Value::as_str).collect();
        c.outline.push(format!("asks: {}", who.join(", ")));
    }
    for b in &blocks {
        c.outline.push(block_line(b));
    }
    let actions: Vec<String> = card["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| {
            let kind = a["kind"].as_str().unwrap_or("submit");
            let role = a["role"]
                .as_str()
                .map(|r| format!(", role {r}"))
                .unwrap_or_default();
            format!("{} ({kind}{role})", a["label"].as_str().unwrap_or_default())
        })
        .collect();
    if !actions.is_empty() {
        c.outline.push(format!("actions: {}", actions.join(", ")));
    }
    c
}

/// What the card as a whole declares that does not do what it seems to.
fn card_warnings(raw: &Value, card: &CardV2, out: &mut Vec<String>) {
    // Input blocks of a question that no answer carries.
    if card.ask.is_some() {
        let sent: Vec<&str> = card
            .actions()
            .iter()
            .flat_map(|a| a.inputs.iter().flatten())
            .map(String::as_str)
            .collect();
        let in_questionnaire: Vec<&str> = card
            .blocks
            .iter()
            .filter(|b| matches!(b, CardV2Block::Questionnaire { .. }))
            .flat_map(|b| b.child_ids())
            .collect();
        let loose: Vec<&str> = card
            .all_blocks()
            .into_iter()
            .filter(|b| b.takes_input())
            .filter(|b| {
                !matches!(
                    b,
                    CardV2Block::Confirm { .. }
                        | CardV2Block::Compare { .. }
                        | CardV2Block::Poll { .. }
                        | CardV2Block::Questionnaire { .. }
                ) && !b.base().is_some_and(|x| x.is_immediate())
            })
            .filter_map(CardV2Block::id)
            .filter(|id| !sent.contains(id) && !in_questionnaire.contains(id))
            .collect();
        if !loose.is_empty() {
            out.push(format!(
                "no answer carries the value of {}: name them in a submit action's `inputs`, or \
give a single choice \"deliver\": \"immediate\"",
                loose.join(", ")
            ));
        }
    }
    if raw["intent"] == "done" {
        out.push(
            "a done card you post is the run's result: your last message is not posted again, \
and the pull requests, 产出 and acceptance checks of the automatic result card are not added. For \
numbers post an info card and let the last message be the result"
                .into(),
        );
    }
}

/// Checks the ops of a `card patch` (a list, or `{"ops": [...]}`).
pub fn check_ops(ops: &Value) -> Check {
    let mut c = Check::default();
    let ops = ops.get("ops").unwrap_or(ops);
    let Some(list) = ops.as_array().filter(|a| !a.is_empty()) else {
        c.problems
            .push("the ops file must hold a non-empty list of patch ops".into());
        return c;
    };
    if list.len() > MAX_PATCH_OPS {
        c.problems.push(format!(
            "{} ops: a patch takes at most {MAX_PATCH_OPS}",
            list.len()
        ));
    }
    let ops_schema: Value = serde_json::from_str(OPS_SCHEMA).unwrap_or_default();
    unknown_fields(ops, &ops_schema, "ops", &mut c.warnings);
    for (i, op) in list.iter().enumerate() {
        let name = format!("ops.{i} ({})", op["op"].as_str().unwrap_or("?"));
        match serde_json::from_value::<PatchOp>(op.clone()) {
            Err(e) => c.problems.push(format!(
                "{name}: {e}; ops are set {{id, props}}, replace {{id, block}}, insert {{block, \
after|before|index|parent}}, remove {{id}}, data {{path, value}} and card {{props}} (`chat-run \
card schema --ops`)"
            )),
            Ok(parsed) => {
                if let Err(e) = parsed.validate() {
                    c.problems.push(format!("{name}: {e}"));
                }
                if let Some(b) = op.get("block") {
                    let block = block_name("block", b);
                    if matches!(
                        serde_json::from_value::<CardV2Block>(b.clone()),
                        Ok(CardV2Block::Unknown)
                    ) {
                        c.problems
                            .push(format!("{name}: {block}: {}", block_hint(b, "")));
                    } else if let Some(p) = probe(b).and_then(|x| x.problem()) {
                        c.problems
                            .push(format!("{name}: {block}: {}: {}", p.reason, p.message));
                    }
                    block_warnings(b, &format!("{name}: {block}"), &mut c.warnings);
                }
            }
        }
        // Only an op that parses is something the patch does.
        if serde_json::from_value::<PatchOp>(op.clone()).is_err() {
            continue;
        }
        c.outline.push(match op["op"].as_str() {
            Some("set") => format!(
                "set {}: {}",
                op["id"].as_str().unwrap_or("?"),
                op["props"]
                    .as_object()
                    .map(|p| p.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            ),
            Some("card") => format!(
                "card: {}",
                op["props"]
                    .as_object()
                    .map(|p| p.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            ),
            Some(o @ ("replace" | "insert")) => format!("{o} {}", block_line(&op["block"])),
            Some("remove") => format!("remove {}", op["id"].as_str().unwrap_or("?")),
            Some("data") => format!("data at \"{}\"", op["path"].as_str().unwrap_or_default()),
            _ => "?".into(),
        });
    }
    c
}

// ---------------------------------------------------------------------------------------------
// The commands that need no daemon
// ---------------------------------------------------------------------------------------------

/// `chat-run card schema|example|check ...`: the exit code and what to print (standard output
/// on 0, the error message on 1), or `None` for another command.
pub fn local(args: &[String]) -> Option<Result<String, String>> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["card", "schema"] => Some(Ok(schema_index())),
        ["card", "schema", "--full"] => Some(Ok(full_schema())),
        ["card", "schema", "--ops"] => Some(Ok(OPS_SCHEMA.trim_end().to_string())),
        ["card", "schema", "--block", t] => Some(block_schema(t)),
        ["card", "schema", flag] if flag.starts_with("--block=") => {
            Some(block_schema(&flag["--block=".len()..]))
        }
        ["card", "schema", ..] => Some(Err(
            "card schema takes one of --block <type>, --ops, --full, or nothing".into(),
        )),
        ["card", "example"] => Some(example(None)),
        ["card", "example", kind] if !kind.starts_with('-') => Some(example(Some(kind))),
        ["card", "example", ..] => Some(Err("card example takes one kind".into())),
        ["card", "check", flag, path] if matches!(*flag, "--file" | "--ops") => {
            Some(check_file(flag, path))
        }
        ["card", "check", flag] if flag.starts_with("--file=") || flag.starts_with("--ops=") => {
            let (flag, path) = flag.split_once('=').unwrap_or((flag, ""));
            Some(check_file(flag, path))
        }
        ["card", "check", ..] => Some(Err(
            "card check takes --file card.json or --ops ops.json".into()
        )),
        _ => None,
    }
}

/// `card check`: Ok with the report when nothing is refused, Err with the report otherwise.
fn check_file(flag: &str, path: &str) -> Result<String, String> {
    let v = crate::chat_run::read_json(path)?;
    let c = if flag == "--ops" {
        check_ops(&v)
    } else {
        check_card(v.get("cardV2").unwrap_or(&v))
    };
    let report = c.report(path);
    if c.problems.is_empty() {
        Ok(report)
    } else {
        Err(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn repo() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// A validator of the ops schema with the card's parts it names put beside it.
    fn ops_validator() -> jsonschema::Validator {
        let text = OPS_SCHEMA.replace("CardV2.schema.json#", "#");
        let mut ops: Value = serde_json::from_str(&text).unwrap();
        let card = schema();
        let mut defs = card["$defs"].clone();
        defs["json"] = ops["$defs"]["json"].clone();
        ops["$defs"] = defs;
        ops["properties"] = card["properties"].clone();
        jsonschema::validator_for(&ops).unwrap()
    }

    #[test]
    fn every_block_type_is_in_one_group_and_has_a_description() {
        let types = block_types();
        assert!(types.len() >= 35, "{types:?}");
        let grouped: Vec<&str> = BLOCK_GROUPS
            .iter()
            .flat_map(|(_, t)| t.iter().copied())
            .collect();
        for t in types {
            assert_eq!(
                grouped.iter().filter(|g| **g == t).count(),
                1,
                "{t} must be in exactly one group of BLOCK_GROUPS"
            );
            assert!(
                schema()["$defs"][t]["description"].is_string(),
                "{t} has no description"
            );
        }
        for g in &grouped {
            assert!(types.iter().any(|t| t == g), "{g} is not a block type");
        }
        // The fields the index prints are the card's fields.
        let mut have: Vec<&str> = schema()["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut want = CARD_FIELDS.to_vec();
        have.sort_unstable();
        want.sort_unstable();
        assert_eq!(have, want);
        for t in types {
            for f in BASE_FIELDS {
                assert!(!schema()["$defs"][t]["properties"][f].is_null(), "{t}.{f}");
            }
        }
    }

    #[test]
    fn the_schema_index_is_short_and_names_every_block() {
        let index = schema_index();
        assert!(index.len() < 9000, "{} bytes", index.len());
        for t in block_types() {
            assert!(index.contains(&format!("\n  {t}")), "{t}");
        }
        assert!(index.contains("intent*: ask|done|working|info."), "{index}");
        assert!(index.contains("  chart (kind, series): "), "{index}");
        // Every way to see more that it names is a command.
        for cmd in [
            "card schema --block",
            "--ops",
            "--full",
            "card example",
            "card check --file",
        ] {
            assert!(index.contains(cmd), "{cmd}");
        }
    }

    #[test]
    fn one_block_schema_is_small_complete_and_valid() {
        for t in block_types() {
            let text = block_schema(t).unwrap();
            assert!(text.len() < 12_000 || t == "group", "{t}: {}", text.len());
            let s: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(s["properties"]["type"]["const"], json!(t));
            // The shared parts are written out; only block types and any-JSON stay references.
            let mut refs = vec![];
            fn walk(v: &Value, out: &mut Vec<String>) {
                match v {
                    Value::Object(m) => {
                        if let Some(r) = m.get("$ref").and_then(Value::as_str) {
                            out.push(r.to_string());
                        }
                        m.values().for_each(|x| walk(x, out));
                    }
                    Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
                    _ => {}
                }
            }
            walk(&s, &mut refs);
            for r in refs {
                let name = r.strip_prefix("#/$defs/").unwrap();
                assert!(
                    name == "json" || block_types().iter().any(|b| b == name),
                    "{t}: {r}"
                );
            }
            assert!(s["properties"]["id"]["description"].is_string(), "{t}");
        }
        assert!(block_schema("chart").unwrap().len() < 4000);
        let e = block_schema("pie").unwrap_err();
        assert!(
            e.contains("unknown block type pie") && e.contains("chart"),
            "{e}"
        );
        // The full schema is valid JSON Schema, far smaller than the 262 KB it was.
        let full = full_schema();
        assert!(full.len() < 60_000, "{}", full.len());
        jsonschema::validator_for(&serde_json::from_str(&full).unwrap()).unwrap();
    }

    #[test]
    fn every_example_is_a_card_the_schema_the_rules_and_the_check_take() {
        let card = jsonschema::validator_for(schema()).unwrap();
        let ops = ops_validator();
        let list = example(None).unwrap();
        for e in EXAMPLES {
            assert!(list.contains(&format!("  {}: ", e.kind)), "{}", e.kind);
            let text = example(Some(e.kind)).unwrap();
            let v: Value = serde_json::from_str(&text).unwrap();
            assert!(
                text.lines().count() <= 45,
                "{}: {} lines",
                e.kind,
                text.lines().count()
            );
            if e.kind == "patch" {
                let errs: Vec<String> = ops.iter_errors(&v).map(|e| e.to_string()).collect();
                assert!(errs.is_empty(), "patch: {errs:?}");
                let c = check_ops(&v);
                assert_eq!((c.problems, c.warnings), (vec![], vec![]), "patch");
                continue;
            }
            let errs: Vec<String> = card.iter_errors(&v).map(|e| e.to_string()).collect();
            assert!(errs.is_empty(), "{}: {errs:?}", e.kind);
            // The write rules of the protocol crate, which the JSON Schema does not hold.
            let parsed: CardV2 = serde_json::from_value(v.clone()).unwrap();
            assert_eq!(parsed.problem(), None, "{}", e.kind);
            // Parse and serialize keep every field: nothing in an example is dropped.
            assert_eq!(serde_json::to_value(&parsed).unwrap(), v, "{}", e.kind);
            let c = check_card(&v);
            assert_eq!((c.problems, c.warnings), (vec![], vec![]), "{}", e.kind);
            assert!(c.outline.len() >= 2, "{}", e.kind);
        }
        // The ops example patches the progress example: its ids are blocks of that card.
        let progress: Value =
            serde_json::from_str(example(Some("progress")).unwrap().as_str()).unwrap();
        let ids: Vec<&str> = progress["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["id"].as_str())
            .collect();
        let patch: Value = serde_json::from_str(example(Some("patch")).unwrap().as_str()).unwrap();
        for op in patch.as_array().unwrap() {
            if let Some(id) = op["id"].as_str() {
                assert!(ids.contains(&id), "{id}");
            }
        }
        assert!(
            example(Some("nope"))
                .unwrap_err()
                .contains("examples: ask, form")
        );
    }

    #[test]
    fn the_ops_schema_takes_every_op_and_refuses_the_common_guesses() {
        let v = ops_validator();
        for ok in [
            json!([{ "op": "set", "id": "p", "props": { "value": 0.8 } }]),
            json!([{ "op": "replace", "id": "t", "block": { "type": "text", "id": "t", "text": "x" } }]),
            json!([{ "op": "insert", "block": { "type": "divider", "id": "d" }, "after": "t" }]),
            json!([{ "op": "remove", "id": "t" }]),
            json!([{ "op": "data", "path": "/a", "value": { "b": [1] } }]),
            json!([{ "op": "card", "props": { "status": null, "ask": null, "title": "x" } }]),
        ] {
            let errs: Vec<String> = v.iter_errors(&ok).map(|e| e.to_string()).collect();
            assert!(errs.is_empty(), "{ok}: {errs:?}");
            assert_eq!(check_ops(&ok).problems, Vec::<String>::new(), "{ok}");
        }
        for bad in [
            json!([{ "op": "replace", "path": "/blocks/0/value", "value": 0.8 }]),
            json!([{ "op": "update", "id": "p", "props": {} }]),
            json!([{ "op": "set", "id": "p", "value": 0.8 }]),
            json!([{ "op": "card", "title": "x" }]),
            json!([]),
        ] {
            assert!(!v.is_valid(&bad), "{bad}");
            let c = check_ops(&bad);
            assert!(!c.problems.is_empty(), "{bad}");
        }
        let c = check_ops(&json!([{ "op": "set", "id": "p", "props": { "id": "q" } }]));
        assert!(
            c.problems[0].contains("set cannot change id"),
            "{:?}",
            c.problems
        );
        let text = OPS_SCHEMA;
        for op in ["set", "replace", "insert", "remove", "data", "card"] {
            assert!(text.contains(&format!("\"const\": \"{op}\"")), "{op}");
        }
        assert!(text.len() < 8000, "{}", text.len());
    }

    #[test]
    fn the_events_table_is_the_servers() {
        // server/src/cards2/inputs.ts `eventsOf`: `case "<type>":` lines, then the `return [..]`
        // they share.
        let src = std::fs::read_to_string(repo().join("server/src/cards2/inputs.ts")).unwrap();
        let body = src.split("export function eventsOf").nth(1).unwrap();
        let mut theirs: Vec<(String, Vec<String>)> = vec![];
        let mut cases: Vec<String> = vec![];
        let quoted = |l: &str| -> Vec<String> {
            l.split('"')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect()
        };
        for line in body.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("case ") {
                cases.extend(quoted(rest));
            } else if l.starts_with("return") && !cases.is_empty() {
                // `input`: the longer list is for text and number; the table holds the shorter.
                let all = quoted(l.rsplit('[').next().unwrap());
                for c in cases.drain(..) {
                    theirs.push((c, all.clone()));
                }
            } else if l.starts_with("default:") {
                break;
            }
        }
        assert!(theirs.len() >= 15, "{theirs:?}");
        let ours: Vec<(String, Vec<String>)> = BLOCK_EVENTS
            .iter()
            .map(|(t, e)| (t.to_string(), e.iter().map(|x| x.to_string()).collect()))
            .collect();
        let sorted = |mut v: Vec<(String, Vec<String>)>| {
            v.sort();
            v
        };
        assert_eq!(sorted(ours), sorted(theirs));
        assert!(body.contains(
            r#"b.kind === "text" || b.kind === "number" ? ["input.change", "input.submit"]"#
        ));
        assert_eq!(
            events_of(&json!({ "type": "input", "kind": "text" })),
            vec!["input.change", "input.submit"]
        );
        assert_eq!(
            events_of(&json!({ "type": "input", "kind": "date" })),
            vec!["input.change"]
        );
        // Every type of the table is a block type, every event is in the catalog.
        for (t, events) in BLOCK_EVENTS {
            assert!(block_types().iter().any(|b| b == t), "{t}");
            for e in events.iter() {
                assert!(chat_protocol::card2::CARD_EVENT_NAMES.contains(e), "{e}");
            }
        }
    }

    #[test]
    fn post_fills_in_rev_fallback_and_who_is_asked() {
        let mut card = json!({
            "v": 2, "intent": "ask", "title": "选一个", "summary": "两个都行",
            "blocks": [
                { "type": "divider", "id": "d" },
                { "type": "text", "id": "t", "text": "先看**这里**" },
            ],
        });
        let said = complete(&mut card);
        assert_eq!(card["rev"], 1);
        assert_eq!(card["fallback"], "选一个\n两个都行\n先看**这里**");
        assert_eq!(card["ask"], json!({ "from": ["requester"] }));
        assert_eq!(said.len(), 1);
        assert!(said[0].contains("requester"), "{said:?}");
        let parsed: CardV2 = serde_json::from_value(card.clone()).unwrap();
        assert_eq!(parsed.problem(), None);
        // What the file says stays.
        let mut card = json!({
            "v": 2, "rev": 7, "intent": "ask", "title": "t", "fallback": "f",
            "ask": { "from": ["owners"] }, "blocks": [],
        });
        let before = card.clone();
        assert!(complete(&mut card).is_empty());
        assert_eq!(card, before);
        // Other intents get no ask; a long text is cut to the limit.
        let mut card = json!({
            "v": 2, "intent": "info", "title": "t",
            "blocks": [{ "type": "text", "id": "t", "text": "字".repeat(5000) }],
        });
        assert!(complete(&mut card).is_empty());
        assert!(card.get("ask").is_none());
        assert_eq!(card["fallback"].as_str().unwrap().chars().count(), 4000);
    }

    #[test]
    fn the_check_lists_every_wrong_block_and_what_would_be_dropped() {
        // The guesses of the audit: each is one line that says how to fix it.
        let card = json!({
            "v": 2, "catalog": "tc.card/2.1", "intent": "info", "title": "t",
            "blocks": [
                { "type": "text", "text": "no id" },
                { "type": "table", "id": "t", "headers": ["a"], "rows": [[1]] },
                { "type": "pie", "id": "p" },
                { "type": "rating", "id": "r", "label": "l", "fallbackText": "f", "max": 10 },
                { "type": "progress", "id": "g", "value": 40 },
            ],
        });
        let c = check_card(&card);
        assert_eq!(c.problems.len(), 5, "{:#?}", c.problems);
        assert!(
            c.problems[0].starts_with("blocks.0 (text): "),
            "{}",
            c.problems[0]
        );
        assert!(
            c.problems[0].contains("every block needs a unique id"),
            "{}",
            c.problems[0]
        );
        assert!(
            c.problems[1].starts_with("blocks.1 (table t): "),
            "{}",
            c.problems[1]
        );
        assert!(
            c.problems[1].contains("card schema --block table"),
            "{}",
            c.problems[1]
        );
        assert!(
            c.problems[2].contains("unknown block type \"pie\"") && c.problems[2].contains("chart")
        );
        assert!(
            c.problems[3].starts_with("blocks.3 (rating r): card.limits"),
            "{}",
            c.problems[3]
        );
        assert!(
            c.problems[4].starts_with("blocks.4 (progress g): card.limits"),
            "{}",
            c.problems[4]
        );
        assert!(
            c.warnings
                .iter()
                .any(|w| w.starts_with("blocks.1.headers: unknown field") && w.contains("columns")),
            "{:?}",
            c.warnings
        );
        assert!(c.report("card.json").starts_with("card.json: 5 problems, "));
        assert!(c.report("card.json").ends_with("check again."));

        // A card the server takes that does not do what it seems to: warnings only.
        let card = json!({
            "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "ask", "title": "问一下", "fallback": "f",
            "ask": { "from": ["requester"], "who": "x" },
            "blocks": [
                { "type": "plan", "id": "p", "steps": [{ "text": "a", "status": "in_progress" }] },
                { "type": "text", "id": "x", "text": "t", "markdown": true, "tone": "warning" },
                { "type": "table", "id": "t", "columns": ["a"], "rows": [["1"]], "events": ["row.select"] },
                { "type": "chart", "id": "c", "kind": "bar", "x": ["a", "b", "c"], "series": [{ "name": "s", "values": [1, 2] }], "events": ["click"] },
                { "type": "input", "id": "i", "kind": "select", "label": "选" },
                { "type": "buttons", "id": "b", "fallbackText": "f", "buttons": [{ "id": "go", "label": "走" }] },
                { "type": "decision", "id": "d", "options": [{ "id": "a", "label": "A", "hint": 1 }, { "id": "b", "label": "B" }] },
            ],
            "actions": [{ "id": "ok", "label": "好", "type": "approve" }],
        });
        let c = check_card(&card);
        assert_eq!(c.problems, Vec::<String>::new());
        let has = |needle: &str| c.warnings.iter().any(|w| w.contains(needle));
        for needle in [
            "ask.who: unknown field",
            "blocks.0.steps: unknown field, the server drops it (plan has: items, runId)",
            "blocks.1.markdown: unknown field",
            "blocks.1.tone: unknown field",
            "blocks.6.options.0.hint: unknown field",
            "actions.0.type: unknown field",
            "blocks.0 (plan p): the plan has no items",
            "blocks.2 (table t): events lists \"row.select\", which never comes (a table block sends: table.rowSelect, table.sort)",
            "blocks.3 (chart c): events lists \"click\"",
            "blocks.3 (chart c): series \"s\" has 2 values for 3 labels of x",
            "blocks.4 (input i): a select input without options",
            "blocks.5 (buttons b): the buttons send nothing",
            "no answer carries the value of i, d",
        ] {
            assert!(has(needle), "{needle}\n{:#?}", c.warnings);
        }
        assert_eq!(c.warnings.len(), 13, "{:#?}", c.warnings);
        let report = c.report("card.json");
        assert!(
            report.contains("ask card \"问一下\"") && report.contains("asks: requester"),
            "{report}"
        );
        assert!(
            report.contains("  decision d [2 options]") && report.contains("actions: 好 (submit)"),
            "{report}"
        );
        assert!(
            report.ends_with("ok: the local check found nothing the server refuses."),
            "{report}"
        );

        // The first write rule over the whole card, with the server's reason.
        let c = check_card(&json!({
            "v": 2, "intent": "info", "title": "t",
            "blocks": [{ "type": "rating", "id": "r", "label": "l", "fallbackText": "f", "max": 5 }],
        }));
        assert_eq!(
            c.problems,
            vec!["card.unknown_block: rating needs catalog tc.card/2.1".to_string()]
        );
        // An ask card without `ask` is filled in, and the check says so; a done card says what it replaces.
        let c = check_card(&json!({ "v": 2, "intent": "ask", "title": "t", "blocks": [] }));
        assert!(
            c.problems.is_empty() && c.warnings[0].contains("no `ask`"),
            "{c:?}"
        );
        let c = check_card(&json!({ "v": 2, "intent": "done", "title": "t", "blocks": [] }));
        assert!(c.warnings[0].contains("is the run's result"), "{c:?}");
        // The warnings travel with the JSON the command prints.
        let v = c.onto(json!({ "messageId": "M" }));
        assert_eq!(v["messageId"], "M");
        assert!(v["warnings"][0].is_string() && v.get("problems").is_none());
    }

    #[test]
    fn the_local_commands_answer_without_a_daemon() {
        let run = |a: &[&str]| local(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(
            run(&["card", "schema"])
                .unwrap()
                .unwrap()
                .starts_with("A card is one JSON object")
        );
        assert!(
            run(&["card", "schema", "--block", "chart"])
                .unwrap()
                .unwrap()
                .contains("\"series\"")
        );
        assert!(run(&["card", "schema", "--block=chart"]).unwrap().is_ok());
        assert!(
            run(&["card", "schema", "--ops"])
                .unwrap()
                .unwrap()
                .contains("Patch ops")
        );
        assert!(
            run(&["card", "schema", "--full"])
                .unwrap()
                .unwrap()
                .starts_with('{')
        );
        assert!(run(&["card", "schema", "--nope"]).unwrap().is_err());
        assert!(
            run(&["card", "example"])
                .unwrap()
                .unwrap()
                .contains("  review: ")
        );
        assert!(
            run(&["card", "example", "ask"])
                .unwrap()
                .unwrap()
                .contains("\"intent\": \"ask\"")
        );
        assert!(run(&["card", "example", "x"]).unwrap().is_err());
        assert!(
            run(&["card", "check"])
                .unwrap()
                .unwrap_err()
                .contains("--file card.json or --ops ops.json")
        );
        assert!(run(&["card", "post", "--file", "x"]).is_none());
        assert!(run(&["card", "get"]).is_none());
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("card.json");
        std::fs::write(&good, example(Some("data")).unwrap()).unwrap();
        let out = run(&["card", "check", "--file", good.to_str().unwrap()])
            .unwrap()
            .unwrap();
        assert!(
            out.contains("0 problems, 0 warnings") && out.contains("chart daily: 每天新增 [bar]"),
            "{out}"
        );
        // The wrapper form of `card post` is checked too.
        let wrapped = dir.path().join("wrapped.json");
        std::fs::write(&wrapped, json!({ "cardV2": { "v": 2, "intent": "info", "title": "t", "blocks": [{ "type": "nope", "id": "n" }] }, "body": "b" }).to_string()).unwrap();
        let out = run(&["card", "check", &format!("--file={}", wrapped.display())])
            .unwrap()
            .unwrap_err();
        assert!(
            out.contains("1 problem, 0 warnings") && out.contains("unknown block type \"nope\""),
            "{out}"
        );
        let ops = dir.path().join("ops.json");
        std::fs::write(&ops, example(Some("patch")).unwrap()).unwrap();
        let out = run(&["card", "check", "--ops", ops.to_str().unwrap()])
            .unwrap()
            .unwrap();
        assert!(
            out.contains("set bar: label, value") && out.contains("card: fallback, summary"),
            "{out}"
        );
    }
}
