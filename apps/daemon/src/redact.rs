//! Secret redaction for everything a run sends off this machine: run events, `run.done`
//! texts and the uploaded session file. Group members read run events in the process view,
//! so tool output, diffs and messages must not carry the tokens a CLI saw.
//!
//! Rules: the exact values of the `env_passthrough` variables and of the machine token, then
//! patterns for known token formats (the chat-core diagnostics rules, `tm_` machine tokens,
//! GitHub, OpenAI, Anthropic, AWS, Slack keys, private key blocks) and secret-named
//! `key=value` pairs.

use crate::config::Config;
use crate::proto::RunEvent;
use regex::{Captures, Regex};
use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const MASK: &str = "<redacted>";

/// Exact values shorter than this are not masked: they would hit ordinary words.
const MIN_EXACT_LEN: usize = 8;

fn patterns() -> &'static [(Regex, &'static str)] {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        let r = |p: &str| Regex::new(p).expect("redaction regex");
        vec![
            // PEM private keys, the whole block.
            (
                r(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY-----"),
                MASK,
            ),
            // Authorization headers and bearer values.
            (r(r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]{8,}"), "${1}<redacted>"),
            (
                r(r"(?i)((?:authorization|proxy-authorization|x-admin-secret|x-hook-secret|x-api-key|cookie|set-cookie)\s*[:=]\s*)[^\r\n,;]+"),
                "${1}<redacted>",
            ),
            // Chat member tokens, hook secrets and machine tokens.
            (r(r"\b(?:tc|whk|tm)_[A-Za-z0-9_-]{20,}"), MASK),
            // GitHub tokens.
            (r(r"\bgh[pousr]_[A-Za-z0-9]{30,}"), MASK),
            (r(r"\bgithub_pat_[A-Za-z0-9_]{20,}"), MASK),
            // OpenAI (sk-, sk-proj-) and Anthropic (sk-ant-) keys.
            (r(r"\bsk-[A-Za-z0-9_-]{20,}"), MASK),
            // AWS access key ids, Slack tokens, Google API keys.
            (r(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"), MASK),
            (r(r"\bxox[abprs]-[A-Za-z0-9-]{10,}"), MASK),
            (r(r"\bAIza[0-9A-Za-z_-]{35}"), MASK),
            // Secret-bearing query parameters.
            (
                r(r"(?i)([?&](?:code|token|access_token|refresh_token|secret|sig|signature|key|api_key|apikey|password|auth|x-amz-signature|x-amz-credential|x-amz-security-token)=)[^&\s#]+"),
                "${1}<redacted>",
            ),
            // URL user info (https://user:pass@host).
            (r(r"(?i)([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@"), "${1}<redacted>@"),
        ]
    })
}

/// `NAME=value`, `name: value` and `"name": "value"` with a secret-looking name. The value is
/// masked only when it looks like a secret (8+ characters with a digit), so source code such
/// as `token: String` or `password = read_password()` stays readable.
fn pair_rule() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r#"(?i)("?[A-Za-z0-9_.-]*(?:token|secret|password|passwd|api_?key|access_?key|private_?key|invite_?code|client_?secret|credentials?)"?\s*[:=]\s*"?)([^"\s,;&}'`)]+)"#,
        )
        .expect("redaction regex")
    })
}

/// Invite codes (XXXX-XXXX, XXXX-XXXX-XXXX, Crockford alphabet, upper case), not when part of
/// a longer dashed id such as a UUID, and only with both a digit and a letter.
fn invite_rule() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\b[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}(?:-[0-9A-HJKMNP-TV-Z]{4})?\b")
            .expect("redaction regex")
    })
}

fn looks_secret(v: &str) -> bool {
    v.len() >= 8 && v.bytes().any(|b| b.is_ascii_digit()) && !v.starts_with(MASK)
}

/// JSON keys whose string values are ids or enums, never free text.
const ID_KEYS: &[&str] = &[
    "t",
    "type",
    "runId",
    "itemId",
    "turnId",
    "approvalId",
    "inputId",
    "sessionId",
    "status",
    "state",
    "role",
    "kind",
    "runtime",
    "source",
    "sha256",
];

#[derive(Debug, Clone, Default)]
pub struct Redactor {
    exact: Vec<String>,
    /// One-time codes of a device sign-in (`gh auth login` prints `XXXX-XXXX` next to
    /// github.com/login/device): they look like invite codes, but the person must read them,
    /// so once seen next to the sign-in page they are not masked.
    device_codes: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// Values one run learned while it ran (feature "github": every token it fetched); shared
    /// by the clones of that run's redactor ([`Self::for_run`]) and masked like `exact`.
    extra: std::sync::Arc<std::sync::RwLock<Vec<String>>>,
}

/// Text that shows a device sign-in: the codes in it are meant for a person.
fn device_login(s: &str) -> bool {
    s.contains("/login/device") || s.contains("one-time code")
}

const MAX_DEVICE_CODES: usize = 32;

impl Redactor {
    /// Masks these exact values (each at least 8 characters) on top of the patterns.
    pub fn new(values: impl IntoIterator<Item = String>) -> Redactor {
        let mut exact: Vec<String> = values
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| v.len() >= MIN_EXACT_LEN)
            .collect();
        // Longest first, so a value that contains another is masked whole.
        exact.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        exact.dedup();
        Redactor {
            exact,
            device_codes: Default::default(),
            extra: Default::default(),
        }
    }

    /// A redactor for one run: the same rules, with its own list of values that can grow while
    /// the run goes on ([`Self::add`]). Its clones share that list; other runs do not see it.
    pub fn for_run(&self) -> Redactor {
        Redactor {
            exact: self.exact.clone(),
            device_codes: self.device_codes.clone(),
            extra: Default::default(),
        }
    }

    /// Masks `value` from now on in this run's text (values shorter than 8 characters are
    /// ignored, as for [`Self::new`]).
    pub fn add(&self, value: &str) {
        let v = value.trim();
        if v.len() < MIN_EXACT_LEN {
            return;
        }
        let mut extra = self.extra.write().unwrap_or_else(|e| e.into_inner());
        if !extra.iter().any(|x| x == v) {
            extra.push(v.to_string());
            extra.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        }
    }

    /// True once [`Self::add`] took a value.
    pub fn has_added(&self) -> bool {
        !self
            .extra
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    /// The machine token and the values of every `env_passthrough` and `gated_env` variable in
    /// this process.
    /// [`Self::for_daemon`] plus the API keys the app gave the machine (feature "accounts").
    pub fn for_daemon_with(cfg: &Config, token: &str, keys: &[String]) -> Redactor {
        let mut r = Self::for_daemon(cfg, token);
        let mut all = std::mem::take(&mut r.exact);
        all.extend(keys.iter().cloned());
        Redactor::new(all)
    }

    pub fn for_daemon(cfg: &Config, token: &str) -> Redactor {
        let mut values: Vec<String> = cfg
            .env_passthrough
            .iter()
            .chain(&cfg.gated_env)
            .filter_map(|k| std::env::var(k).ok())
            .collect();
        values.push(token.to_string());
        Redactor::new(values)
    }

    pub fn text(&self, s: &str) -> String {
        let mut out = s.to_string();
        for v in &self.exact {
            if out.contains(v.as_str()) {
                out = out.replace(v.as_str(), MASK);
            }
        }
        for v in self.extra.read().unwrap_or_else(|e| e.into_inner()).iter() {
            if out.contains(v.as_str()) {
                out = out.replace(v.as_str(), MASK);
            }
        }
        for (re, rep) in patterns() {
            if re.is_match(&out) {
                out = re.replace_all(&out, *rep).into_owned();
            }
        }
        let pair = pair_rule();
        if pair.is_match(&out) {
            out = pair
                .replace_all(&out, |c: &Captures| {
                    if looks_secret(&c[2]) {
                        format!("{}{MASK}", &c[1])
                    } else {
                        c[0].to_string()
                    }
                })
                .into_owned();
        }
        let inv = invite_rule();
        if inv.is_match(&out) {
            let src = out.clone();
            let mut known = self.device_codes.lock().unwrap_or_else(|e| e.into_inner());
            if device_login(&src) {
                for m in inv.find_iter(&src) {
                    if !known.iter().any(|k| k == m.as_str()) {
                        if known.len() >= MAX_DEVICE_CODES {
                            known.remove(0);
                        }
                        known.push(m.as_str().to_string());
                    }
                }
            }
            out = inv
                .replace_all(&src, |c: &Captures| {
                    let m = c.get(0).expect("match");
                    let before = src[..m.start()].chars().next_back();
                    let after = src[m.end()..].chars().next();
                    let dashed =
                        |c: Option<char>| c.is_some_and(|c| c == '-' || c.is_ascii_alphanumeric());
                    let t = m.as_str();
                    let mixed = t.bytes().any(|b| b.is_ascii_digit())
                        && t.bytes().any(|b| b.is_ascii_alphabetic());
                    if mixed && !dashed(before) && !dashed(after) && !known.iter().any(|k| k == t) {
                        MASK.to_string()
                    } else {
                        t.to_string()
                    }
                })
                .into_owned();
        }
        out
    }

    /// Redacts a text file of the run in place (`stderr.log`), when that changes it.
    pub fn file(&self, path: &std::path::Path) {
        let Ok(raw) = std::fs::read(path) else { return };
        let text = String::from_utf8_lossy(&raw);
        let masked = self.text(&text);
        if masked != text {
            let _ = std::fs::write(path, masked);
        }
    }

    /// Redacts every free-text string inside a JSON value (ids and enums are left alone).
    pub fn value(&self, v: &mut Value) {
        match v {
            Value::String(s) => {
                let r = self.text(s);
                if r != *s {
                    *s = r;
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| self.value(x)),
            Value::Object(o) => {
                for (k, x) in o.iter_mut() {
                    if x.is_string() && ID_KEYS.contains(&k.as_str()) {
                        continue;
                    }
                    self.value(x);
                }
            }
            _ => {}
        }
    }

    /// The JSON text of a frame with its free text redacted.
    pub fn frame<T: serde::Serialize>(&self, frame: &T) -> Option<String> {
        let mut v = serde_json::to_value(frame).ok()?;
        self.value(&mut v);
        serde_json::to_string(&v).ok()
    }
}

/// Holds back streamed message text until a line ends, so a secret split over two deltas is
/// still seen whole by the redactor. A final `message` for the item replaces its held text;
/// any other event and the end of the run release it. Text held for [`HOLD`] is released up
/// to its last two words while the stream goes on, and whole once the stream pauses for
/// [`HOLD`] (an interactive prompt has no line end).
#[derive(Debug, Default)]
pub struct DeltaGate {
    held: Vec<(String, Option<String>, String)>,
    /// When the oldest held text was held.
    first: Option<Instant>,
    /// When the last delta came.
    last: Option<Instant>,
}

pub const HOLD: Duration = Duration::from_secs(1);
/// A line longer than this is cut at a space.
const LONG_LINE: usize = 4096;
/// An unterminated private key block is held up to this size.
const MAX_HOLD: usize = 64 * 1024;

fn open_key_block(s: &str) -> bool {
    s.len() < MAX_HOLD
        && s.rfind("-----BEGIN ")
            .is_some_and(|b| !s[b..].contains("-----END "))
}
fn safe_cut(s: &str) -> usize {
    if open_key_block(s) {
        return 0;
    }
    if let Some(i) = s.rfind('\n') {
        return i + 1;
    }
    if s.len() > LONG_LINE {
        let mut lim = s.len() - 512;
        while !s.is_char_boundary(lim) {
            lim -= 1;
        }
        if let Some(i) = s[..lim].rfind(char::is_whitespace) {
            return i + 1;
        }
        return lim;
    }
    0
}

/// Start of the second-to-last word: what may go out while the stream goes on.
fn word_cut(s: &str) -> usize {
    let t = s.trim_end();
    let Some(i) = t.rfind(char::is_whitespace) else {
        return 0;
    };
    match t[..i].trim_end().rfind(char::is_whitespace) {
        Some(j) => j + 1,
        None => 0,
    }
}

impl DeltaGate {
    pub fn holding(&self) -> bool {
        !self.held.is_empty()
    }

    /// When [`DeltaGate::tick`] should run next.
    pub fn deadline(&self) -> Option<Instant> {
        self.first.map(|t| t + HOLD)
    }

    fn release(&mut self, out: &mut Vec<RunEvent>, keep: Option<&str>) {
        let held = std::mem::take(&mut self.held);
        for (item_id, turn_id, text) in held {
            if keep == Some(item_id.as_str()) || text.is_empty() {
                continue;
            }
            out.push(RunEvent::MessageDelta {
                item_id,
                turn_id,
                text,
            });
        }
        self.first = None;
    }

    /// Releases all held text.
    pub fn flush(&mut self) -> Vec<RunEvent> {
        let mut out = vec![];
        self.release(&mut out, None);
        out
    }

    /// Called at [`DeltaGate::deadline`]: releases all text once the stream paused, or all but
    /// the last two words while it goes on.
    pub fn tick(&mut self, now: Instant) -> Vec<RunEvent> {
        if self.last.is_none_or(|l| now.duration_since(l) >= HOLD) {
            return self.flush();
        }
        let mut out = vec![];
        if let Some((item_id, turn_id, text)) = self.held.first_mut() {
            let cut = if open_key_block(text) {
                0
            } else {
                word_cut(text)
            };
            if cut > 0 {
                let rest = text.split_off(cut);
                out.push(RunEvent::MessageDelta {
                    item_id: item_id.clone(),
                    turn_id: turn_id.clone(),
                    text: std::mem::replace(text, rest),
                });
            }
        }
        self.first = Some(now);
        out
    }

    pub fn push(&mut self, ev: RunEvent) -> Vec<RunEvent> {
        let mut out = vec![];
        match ev {
            RunEvent::MessageDelta {
                item_id,
                turn_id,
                text,
            } => {
                // A delta of another item first releases the held one, to keep the order.
                if self.held.iter().any(|(i, _, _)| *i != item_id) {
                    self.release(&mut out, None);
                }
                if self.held.is_empty() {
                    self.held
                        .push((item_id.clone(), turn_id.clone(), String::new()));
                }
                let entry = &mut self.held[0];
                entry.2.push_str(&text);
                let cut = safe_cut(&entry.2);
                if cut > 0 {
                    let rest = entry.2.split_off(cut);
                    let ready = std::mem::replace(&mut entry.2, rest);
                    out.push(RunEvent::MessageDelta {
                        item_id,
                        turn_id,
                        text: ready,
                    });
                }
                let now = Instant::now();
                self.last = Some(now);
                if entry.2.is_empty() {
                    self.held.clear();
                    self.first = None;
                } else if self.first.is_none() {
                    self.first = Some(now);
                }
            }
            RunEvent::Message { ref item_id, .. } => {
                // The full text replaces what is held for the same item.
                let id = item_id.clone();
                self.release(&mut out, Some(&id));
                out.push(ev);
            }
            other => {
                self.release(&mut out, None);
                out.push(other);
            }
        }
        out
    }
}
