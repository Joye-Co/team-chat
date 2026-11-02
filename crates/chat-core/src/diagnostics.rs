//! Diagnostics: collect recent logs, remove secrets, gzip, upload
//! (`POST /v1/diagnostics`).

use crate::http::{ApiError, ApiResult, HttpClient};
use crate::proto::{DiagnosticsResponse, MAX_DIAGNOSTICS_BYTES};
use regex::Regex;
use reqwest::Method;
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

/// Uncompressed text read from the logs at most (newest lines kept).
pub const MAX_COLLECT_BYTES: usize = 32 * 1024 * 1024;

const R: &str = "<redacted>";

struct Rules {
    list: Vec<(Regex, &'static str)>,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        let r = |p: &str| Regex::new(p).expect("redaction regex");
        Rules {
            list: vec![
                // Authorization headers and bearer values.
                (r(r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]+"), "${1}<redacted>"),
                (
                    r(r"(?i)((?:authorization|x-admin-secret|x-hook-secret|x-hub-signature-256|cookie|set-cookie)\s*[:=]\s*)[^\r\n,;]+"),
                    "${1}<redacted>",
                ),
                // Member tokens (tc_ + 43 base64url) and hook secrets (whk_ + 43).
                (r(r"\b(?:tc|whk)_[A-Za-z0-9_-]{20,}"), R),
                // Secret-bearing query parameters.
                (
                    r(r"(?i)([?&](?:code|token|access_token|refresh_token|secret|sig|signature|key|api_key|apikey|password|auth|x-amz-signature|x-amz-credential|x-amz-security-token)=)[^&\s#]*"),
                    "${1}<redacted>",
                ),
                // key=value or "key": "value" pairs with secret names.
                (
                    r(r#"(?i)("?(?:token|secret|password|passwd|invite_?code|inviteCode|api_?key|admin_?secret|private_?key)"?\s*[:=]\s*"?)[^"\s,;&}]+"#),
                    "${1}<redacted>",
                ),
                // Invite codes: XXXX-XXXX-XXXX in the Crockford alphabet.
                (
                    r(r"(?i)\b[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}\b"),
                    R,
                ),
                // URL user info (https://user:pass@host).
                (r(r"(?i)(https?://)[^/\s:@]+:[^/\s@]+@"), "${1}<redacted>@"),
            ],
        }
    })
}

/// Removes tokens, hook secrets, invite codes, secret headers and secret-bearing URL
/// parameters from text.
pub fn redact(text: &str) -> String {
    let mut s = std::borrow::Cow::Borrowed(text);
    for (re, rep) in &rules().list {
        if let std::borrow::Cow::Owned(o) = re.replace_all(&s, *rep) {
            s = std::borrow::Cow::Owned(o);
        }
    }
    s.into_owned()
}

/// Reads the newest `*.log` files of `<data_dir>/logs`, oldest first, up to
/// `max_bytes` (older lines dropped), each under a `==> name <==` header.
pub fn collect_logs(data_dir: &Path, max_bytes: usize) -> std::io::Result<String> {
    let dir = data_dir.join("logs");
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = match std::fs::read_dir(&dir)
    {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter(|e| e.file_name().to_string_lossy().contains(".log"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    // Newest first, so the budget goes to recent logs.
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut parts: Vec<String> = Vec::new();
    let mut left = max_bytes;
    for (_, p) in files {
        if left == 0 {
            break;
        }
        let bytes = std::fs::read(&p)?;
        let text = String::from_utf8_lossy(&bytes);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let body = tail(&text, left);
        left = left.saturating_sub(body.len());
        parts.push(format!("==> {name} <==\n{body}\n"));
    }
    parts.reverse();
    Ok(parts.concat())
}

/// The last at most `max` bytes of `s`, starting at a line start.
fn tail(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    match s[start..].find('\n') {
        Some(i) => &s[start + i + 1..],
        None => &s[start..],
    }
}

fn gzip(text: &str) -> std::io::Result<Vec<u8>> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(text.as_bytes())?;
    enc.finish()
}

/// Redacts and gzips `text`, keeping the newest lines so the result fits the
/// server limit (5 MiB compressed).
pub fn build_report(text: &str) -> std::io::Result<Vec<u8>> {
    let clean = redact(text);
    let mut budget = clean.len();
    loop {
        let body = tail(&clean, budget);
        let gz = gzip(body)?;
        if gz.len() as u64 <= MAX_DIAGNOSTICS_BYTES {
            return Ok(gz);
        }
        budget /= 2;
        if budget < 1024 {
            return gzip("");
        }
    }
}

/// Collects the logs of `data_dir`, puts `header` (app version, OS, state; it is
/// redacted too) on top, and uploads the report. Returns the report id.
pub async fn send(http: &HttpClient, data_dir: &Path, header: &str) -> ApiResult<String> {
    let dir = data_dir.to_path_buf();
    let header = header.to_string();
    let gz = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let logs = collect_logs(&dir, MAX_COLLECT_BYTES)?;
        build_report(&format!("{header}\n\n{logs}"))
    })
    .await
    .map_err(|e| ApiError::Io(e.to_string()))??;
    let req = http
        .authed(Method::POST, &http.url("/v1/diagnostics"))?
        .header(reqwest::header::CONTENT_TYPE, "application/gzip")
        .timeout(std::time::Duration::from_secs(120))
        .body(gz);
    let resp = http.send_checked(req).await?;
    let r: DiagnosticsResponse = HttpClient::json(resp).await?;
    Ok(r.id)
}
