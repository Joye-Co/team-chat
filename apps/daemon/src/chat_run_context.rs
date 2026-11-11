//! `chat-run` topic context (docs/machine-protocol.md, "Topic context"; feature `context`): the
//! prompt carries only a budget of the topic, so a run reads older messages, whole messages,
//! search hits and files of its group with these ops. Like files and 产出, the daemon answers
//! them itself over HTTP with the machine token, as the run's member (results can be larger
//! than a machine frame). They only read.
//!
//! `file.get` writes the file inside the run's working folder only: `--out` must resolve there
//! (symbolic links included), and the default is `<working folder>/.chat-files/<fileId>/<name>`.

use crate::chat_run_files::{Local, answer, client, encode, err, mime_of, url};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const TOPIC_HISTORY: &str = "topic.history";
pub const TOPIC_MESSAGE: &str = "topic.message";
pub const CONTEXT_SEARCH: &str = "context.search";
pub const FILE_GET: &str = "file.get";
/// Keeps something for later (docs/machine-protocol.md, "Memory notes"): POST, pending until the daily consolidation.
pub const MEMORY_NOTE: &str = "memory.note";
pub const OPS: &[&str] = &[
    TOPIC_HISTORY,
    TOPIC_MESSAGE,
    CONTEXT_SEARCH,
    FILE_GET,
    MEMORY_NOTE,
];
/// Memory kinds a run may write (protocol `MemoryKind`).
pub const MEMORY_KINDS: &[&str] = &[
    "convention",
    "fact",
    "decision",
    "preference",
    "lesson",
    "playbook",
];

/// `topic.history` page size: 1-100.
pub const MAX_HISTORY_LIMIT: u64 = 100;
/// `context.search` hits: 1-50.
pub const MAX_SEARCH_LIMIT: u64 = 50;
/// `file.get` refuses files over 100 MiB.
pub const MAX_GET_BYTES: u64 = 100 * 1024 * 1024;
/// The folder of downloaded files in the working folder (git ignores it).
pub const FILES_DIR: &str = ".chat-files";
/// `file.get` waits like the other file ops.
pub const FILE_TIMEOUT: Duration = crate::chat_run_files::TIMEOUT;
/// The reads answer with one JSON document.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the daemon (and `chat-run`) wait for `op`.
pub fn timeout(op: &str) -> Duration {
    if op == FILE_GET {
        FILE_TIMEOUT
    } else {
        READ_TIMEOUT
    }
}

/// Message and file ids go into the URL path: ULID-like words only.
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// A `u64` field of `body` that, when present, lies in `lo..=hi`.
fn number(body: &Value, k: &str, lo: u64, hi: u64) -> Result<Option<u64>, Value> {
    match &body[k] {
        Value::Null => Ok(None),
        v => match v.as_u64().filter(|n| (lo..=hi).contains(n)) {
            Some(n) => Ok(Some(n)),
            None => Err(err(
                "invalid",
                None,
                format!("{k} must be a number from {lo} to {hi}"),
            )),
        },
    }
}

/// The path and query of one read op (`file.get` is not one).
pub fn read_path(run: &str, op: &str, body: &Value) -> Result<String, Value> {
    match op {
        TOPIC_HISTORY => {
            let mut q = vec![];
            for (k, lo, hi) in [
                ("before", 0, u64::MAX),
                ("after", 0, u64::MAX),
                ("limit", 1, MAX_HISTORY_LIMIT),
            ] {
                if let Some(n) = number(body, k, lo, hi)? {
                    q.push(format!("{k}={n}"));
                }
            }
            let q = if q.is_empty() {
                String::new()
            } else {
                format!("?{}", q.join("&"))
            };
            Ok(format!("/v1/machine/runs/{run}/topic/messages{q}"))
        }
        TOPIC_MESSAGE => {
            let Some(id) = body["messageId"].as_str().filter(|s| valid_id(s)) else {
                return Err(err("invalid", None, "messageId is required"));
            };
            Ok(format!("/v1/machine/runs/{run}/topic/messages/{id}"))
        }
        CONTEXT_SEARCH => {
            let Some(query) = body["query"].as_str().filter(|s| !s.trim().is_empty()) else {
                return Err(err("invalid", None, "query is required"));
            };
            let scope = match &body["scope"] {
                Value::Null => "topic",
                v => match v.as_str() {
                    Some(s @ ("topic" | "group")) => s,
                    _ => return Err(err("invalid", None, "scope must be topic or group")),
                },
            };
            let mut p = format!(
                "/v1/machine/runs/{run}/search?q={}&scope={scope}",
                encode(query)
            );
            if let Some(n) = number(body, "limit", 1, MAX_SEARCH_LIMIT)? {
                p.push_str(&format!("&limit={n}"));
            }
            Ok(p)
        }
        _ => Err(err("invalid", Some("output.unknown_op"), "unknown op")),
    }
}

/// Decodes `%XX` (the `X-File-Name` header is URI-encoded UTF-8).
pub fn decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The server's file name as one safe path component, else `fallback`.
pub fn safe_name(name: &str, fallback: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    s = s.trim().to_string();
    // A file name is at most 255 bytes.
    while s.len() > 255 {
        s.pop();
    }
    if s.is_empty() || s.chars().all(|c| c == '.') {
        return fallback.to_string();
    }
    s
}

/// Where `file.get` writes: a file path, or a folder the file goes into under its own name.
#[derive(Debug, PartialEq)]
pub enum Target {
    File(PathBuf),
    Dir(PathBuf),
}

/// `out` (absolute, or relative to the working folder) resolved inside the working folder:
/// what exists is resolved with its symbolic links, the rest must be plain names.
pub fn resolve_out(root: &Path, out: &str) -> Result<Target, Value> {
    // Windows: see `chat_run_files::inside`.
    let out = if cfg!(windows) {
        crate::sys::windows_plain_path(out)
    } else {
        out
    };
    if cfg!(windows)
        && let Some(why) = crate::sys::windows_path_problem(out)
    {
        return Err(err(
            "forbidden",
            Some("file.outside_workspace"),
            format!("{out} is {why}, not a place in the run's working folder"),
        ));
    }
    let root = std::fs::canonicalize(root).map_err(|e| {
        err(
            "unavailable",
            None,
            format!("the working folder is gone: {e}"),
        )
    })?;
    let outside = || {
        err(
            "forbidden",
            Some("file.outside_workspace"),
            format!(
                "{out} is outside the run's working folder {}",
                root.display()
            ),
        )
    };
    let p = Path::new(out);
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    };
    let mut base = p.as_path();
    let mut rest = vec![];
    let real = loop {
        if std::fs::symlink_metadata(base).is_ok() {
            break std::fs::canonicalize(base).map_err(|_| outside())?;
        }
        match (base.parent(), base.components().next_back()) {
            (Some(parent), Some(Component::Normal(n))) => {
                rest.push(n.to_owned());
                base = parent;
            }
            (Some(parent), Some(Component::CurDir)) => base = parent,
            _ => return Err(outside()),
        }
    };
    let mut path = real;
    for n in rest.iter().rev() {
        path.push(n);
    }
    if !path.starts_with(&root) {
        return Err(outside());
    }
    if path.is_dir() || (out.ends_with('/') && !path.exists()) {
        Ok(Target::Dir(path))
    } else {
        Ok(Target::File(path))
    }
}

/// `<working folder>/.chat-files/<fileId>/`, made, with a `.gitignore` of `*` in `.chat-files`.
fn default_dir(root: &Path, file_id: &str) -> Result<PathBuf, Value> {
    let failed = |e: std::io::Error| err("failed", None, format!("cannot make {FILES_DIR}: {e}"));
    let files = root.join(FILES_DIR);
    let dir = files.join(file_id);
    std::fs::create_dir_all(&dir).map_err(failed)?;
    let ignore = files.join(".gitignore");
    if std::fs::symlink_metadata(&ignore).is_err() {
        std::fs::write(&ignore, "*\n").map_err(failed)?;
    }
    // A `.chat-files` link of the repository must not lead out of the working folder.
    match resolve_out(root, &dir.display().to_string())? {
        Target::Dir(d) => Ok(d),
        Target::File(_) => Err(err("failed", None, format!("{FILES_DIR} is not a folder"))),
    }
}

/// Downloads one file of the run's group into the working folder.
async fn get(local: &Local, body: &Value) -> Value {
    let Some(file_id) = body["fileId"].as_str().filter(|s| valid_id(s)) else {
        return err("invalid", None, "fileId is required");
    };
    let target = match body["out"].as_str().filter(|s| !s.is_empty()) {
        Some(out) => resolve_out(&local.root, out),
        None => default_dir(&local.root, file_id).map(Target::Dir),
    };
    let target = match target {
        Ok(t) => t,
        Err(e) => return e,
    };
    let path = format!("/v1/machine/runs/{}/files/{file_id}", local.run_id);
    let (u, c) = match (url(local, &path), client(FILE_TIMEOUT)) {
        (Ok(u), Ok(c)) => (u, c),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let mut res = match c.get(u).bearer_auth(local.token.as_str()).send().await {
        Ok(r) if r.status().is_success() => r,
        other => return answer(other).await,
    };
    let too_large = || err("too_large", None, format!("{file_id} is over 100 MiB"));
    if res.content_length().is_some_and(|n| n > MAX_GET_BYTES) {
        return too_large();
    }
    let header = |k: &str| {
        res.headers()
            .get(k)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let name = header("X-File-Name")
        .map(|n| decode(&n))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| file_id.to_string());
    let file = match target {
        Target::File(p) => p,
        Target::Dir(d) => d.join(safe_name(&name, file_id)),
    };
    let mime = header("Content-Type").unwrap_or_else(|| mime_of(&name).to_string());
    let Some(dir) = file.parent().map(Path::to_path_buf) else {
        return err("invalid", None, "out names no file");
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return err(
            "failed",
            None,
            format!("cannot make {}: {e}", dir.display()),
        );
    }
    // Written beside the target and renamed into place: no half file is left, and a link at the
    // target is replaced, never followed.
    let part = dir.join(format!(".{file_id}.{}.part", std::process::id()));
    let mut out = match std::fs::File::create(&part) {
        Ok(f) => f,
        Err(e) => {
            return err(
                "failed",
                None,
                format!("cannot write {}: {e}", dir.display()),
            );
        }
    };
    let mut size: u64 = 0;
    let fail = |v: Value| {
        let _ = std::fs::remove_file(&part);
        v
    };
    loop {
        match res.chunk().await {
            Ok(Some(chunk)) => {
                size += chunk.len() as u64;
                if size > MAX_GET_BYTES {
                    return fail(too_large());
                }
                if let Err(e) = out.write_all(&chunk) {
                    return fail(err("failed", None, format!("cannot write the file: {e}")));
                }
            }
            Ok(None) => break,
            Err(e) => {
                return fail(err(
                    "unavailable",
                    Some("server.not_connected"),
                    format!("the download broke off: {}", e.without_url()),
                ));
            }
        }
    }
    if let Err(e) = out.flush().and_then(|_| std::fs::rename(&part, &file)) {
        return fail(err(
            "failed",
            None,
            format!("cannot write {}: {e}", file.display()),
        ));
    }
    json!({ "ok": true, "result": {
        "path": file.display().to_string(),
        "name": name,
        "mime": mime,
        "size": size,
    } })
}

/// Answers one of [`OPS`].
/// The `memory.note` body the server takes, or the error for a bad one.
pub fn memory_body(body: &Value) -> Result<Value, Value> {
    let scope = body["scope"].as_str().unwrap_or("");
    if !matches!(scope, "member" | "person" | "group") {
        return Err(err("invalid", None, "scope is member, person or group"));
    }
    let kind = body["kind"].as_str().unwrap_or("");
    if !MEMORY_KINDS.contains(&kind) {
        return Err(err(
            "invalid",
            None,
            format!("kind is one of {}", MEMORY_KINDS.join(", ")),
        ));
    }
    let text = body["text"].as_str().unwrap_or("").trim();
    if text.is_empty() || text.chars().count() > 500 {
        return Err(err("invalid", None, "text is 1..500 characters"));
    }
    let mut out = serde_json::json!({ "scope": scope, "kind": kind, "text": text });
    if let Some(w) = body["when"]
        .as_str()
        .map(str::trim)
        .filter(|w| !w.is_empty())
    {
        if w.chars().count() > 300 {
            return Err(err("invalid", None, "when is at most 300 characters"));
        }
        out["when"] = Value::from(w);
    }
    Ok(out)
}

pub async fn handle(op: &str, body: &Value, local: &Local) -> Value {
    if op == FILE_GET {
        return get(local, body).await;
    }
    if op == MEMORY_NOTE {
        let note = match memory_body(body) {
            Ok(b) => b,
            Err(e) => return e,
        };
        let path = format!("/v1/machine/runs/{}/memories", local.run_id);
        let (u, c) = match (url(local, &path), client(READ_TIMEOUT)) {
            (Ok(u), Ok(c)) => (u, c),
            (Err(e), _) | (_, Err(e)) => return e,
        };
        return answer(
            c.post(u)
                .bearer_auth(local.token.as_str())
                .header("content-type", "application/json")
                .body(note.to_string())
                .send()
                .await,
        )
        .await;
    }
    let path = match read_path(&local.run_id, op, body) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (u, c) = match (url(local, &path), client(READ_TIMEOUT)) {
        (Ok(u), Ok(c)) => (u, c),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    answer(c.get(u).bearer_auth(local.token.as_str()).send().await).await
}

/// Links that lead out of a working folder, for the tests of the path rules here and in
/// `chat_run_files`.
#[cfg(test)]
pub(crate) mod test_links {
    use std::path::Path;

    /// A link `link` to the folder `target`: a symbolic link, on Windows a junction (which a
    /// user without privileges can make, so it is what a repository can really bring).
    pub fn dir_link(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        {
            let out = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(out.status.success(), "mklink /J: {out:?}");
        }
    }

    /// Makes `<root>/away`, a link to the folder `other`, and when this user may make one
    /// `<root>/link`, a link to the file `<other>/secret`. Returns paths, relative to `root`,
    /// that lead out through them.
    pub fn make(root: &Path, other: &Path) -> Vec<String> {
        let _ = std::fs::write(other.join("secret"), "x");
        dir_link(other, &root.join("away"));
        let mut out = vec!["away/secret".to_string()];
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(other.join("secret"), root.join("link")).is_ok();
        // Windows: a symbolic link needs a privilege (or developer mode).
        #[cfg(windows)]
        let linked =
            std::os::windows::fs::symlink_file(other.join("secret"), root.join("link")).is_ok();
        if linked {
            out.push("link".to_string());
        }
        out
    }

    /// Windows: the path of `dir` with its last name in the 8.3 short form, when the volume
    /// keeps one that differs.
    #[cfg(windows)]
    pub fn short_name(dir: &Path) -> Option<String> {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
        let mut buf = vec![0u16; 1024];
        // SAFETY: `wide` is NUL-terminated and `buf` has the length given.
        let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if n == 0 || n as usize >= buf.len() {
            return None;
        }
        let short = std::ffi::OsString::from_wide(&buf[..n as usize])
            .to_string_lossy()
            .into_owned();
        (!short.eq_ignore_ascii_case(&dir.to_string_lossy())).then_some(short)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::Redactor;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn read_ops_build_their_urls_and_check_the_body() {
        let p = |op, b: Value| read_path("R1", op, &b);
        assert_eq!(
            p(TOPIC_HISTORY, json!({})).unwrap(),
            "/v1/machine/runs/R1/topic/messages"
        );
        assert_eq!(
            p(TOPIC_HISTORY, json!({ "before": 40, "limit": 100 })).unwrap(),
            "/v1/machine/runs/R1/topic/messages?before=40&limit=100"
        );
        assert_eq!(
            p(TOPIC_HISTORY, json!({ "after": 7 })).unwrap(),
            "/v1/machine/runs/R1/topic/messages?after=7"
        );
        assert_eq!(
            p(TOPIC_MESSAGE, json!({ "messageId": "01HX" })).unwrap(),
            "/v1/machine/runs/R1/topic/messages/01HX"
        );
        assert_eq!(
            p(
                CONTEXT_SEARCH,
                json!({ "query": "发布 计划&x", "scope": "group", "limit": 5 })
            )
            .unwrap(),
            "/v1/machine/runs/R1/search?q=%E5%8F%91%E5%B8%83%20%E8%AE%A1%E5%88%92%26x&scope=group&limit=5"
        );
        assert_eq!(
            p(CONTEXT_SEARCH, json!({ "query": "x" })).unwrap(),
            "/v1/machine/runs/R1/search?q=x&scope=topic"
        );
        for (op, bad) in [
            (TOPIC_HISTORY, json!({ "limit": 0 })),
            (TOPIC_HISTORY, json!({ "limit": 101 })),
            (TOPIC_HISTORY, json!({ "before": "3" })),
            (TOPIC_MESSAGE, json!({})),
            (TOPIC_MESSAGE, json!({ "messageId": "../files/F" })),
            (CONTEXT_SEARCH, json!({ "query": " " })),
            (CONTEXT_SEARCH, json!({ "query": "x", "limit": 51 })),
            (CONTEXT_SEARCH, json!({ "query": "x", "scope": "all" })),
            ("card.post", json!({})),
        ] {
            let e = p(op, bad.clone()).unwrap_err();
            assert_eq!(e["ok"], false, "{op} {bad}");
            assert!(e["error"]["code"].is_string(), "{op} {bad}");
        }
    }

    #[test]
    fn header_names_become_one_safe_path_component() {
        assert_eq!(decode("%E6%8A%A5%E5%91%8A%201.pdf"), "报告 1.pdf");
        assert_eq!(decode("a%2"), "a%2");
        assert_eq!(decode("%zz"), "%zz");
        assert_eq!(safe_name("报告 1.pdf", "F1"), "报告 1.pdf");
        assert_eq!(safe_name("../../etc/passwd", "F1"), ".._.._etc_passwd");
        assert_eq!(safe_name("a\\b\nc", "F1"), "a_b_c");
        for bad in ["", " ", ".", "..", "..."] {
            assert_eq!(safe_name(bad, "F1"), "F1", "{bad:?}");
        }
        assert!(safe_name(&"长".repeat(200), "F1").len() <= 255);
    }

    #[test]
    fn out_paths_must_stay_in_the_working_folder() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(root.path()).unwrap();
        std::fs::create_dir(root.path().join("docs")).unwrap();
        assert_eq!(
            resolve_out(root.path(), "docs").unwrap(),
            Target::Dir(real.join("docs"))
        );
        assert_eq!(
            resolve_out(root.path(), "new/a.pdf").unwrap(),
            Target::File(real.join("new/a.pdf"))
        );
        assert_eq!(
            resolve_out(root.path(), "./new/dir/").unwrap(),
            Target::Dir(real.join("new/dir"))
        );
        let abs = root.path().join("x.bin");
        assert_eq!(
            resolve_out(root.path(), abs.to_str().unwrap()).unwrap(),
            Target::File(real.join("x.bin"))
        );
        // A link to a folder outside (on Windows a junction).
        test_links::make(root.path(), other.path());
        let mut bad = vec![
            "../x".to_string(),
            "new/../../x".to_string(),
            "away/x".to_string(),
            "away/new/x".to_string(),
            other.path().join("x").display().to_string(),
        ];
        if cfg!(windows) {
            let outside = other.path().join("x").display().to_string();
            bad.extend([
                format!(r"\\?\{outside}"),
                format!(r"\\localhost\{}$\{}", &outside[..1], &outside[3..]),
                format!("{}{}", &outside[..2], &outside[3..]),
                r"\Windows\x".to_string(),
                r"..\..\..\..\..\..\Windows\x".to_string(),
                r"AWAY\x".to_string(),
                "new.txt:stream".to_string(),
                "new.txt::$DATA".to_string(),
                "new.txt.".to_string(),
                "new /a.txt".to_string(),
                "NUL".to_string(),
                r"new\COM1.txt".to_string(),
                r"\\.\pipe\x".to_string(),
            ]);
        }
        for bad in bad {
            assert_eq!(
                resolve_out(root.path(), &bad).unwrap_err()["error"]["reason"],
                "file.outside_workspace",
                "{bad}"
            );
        }
        assert!(!other.path().join("x").exists() && !other.path().join("new").exists());
    }

    #[test]
    fn the_default_folder_is_ignored_by_git_and_stays_inside() {
        let root = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(root.path()).unwrap();
        let d = default_dir(root.path(), "F1").unwrap();
        assert_eq!(d, real.join(".chat-files/F1"));
        assert_eq!(
            std::fs::read_to_string(root.path().join(".chat-files/.gitignore")).unwrap(),
            "*\n"
        );
        // A `.chat-files` link that leads out is refused.
        let other = tempfile::tempdir().unwrap();
        let root2 = tempfile::tempdir().unwrap();
        test_links::dir_link(other.path(), &root2.path().join(".chat-files"));
        assert_eq!(
            default_dir(root2.path(), "F1").unwrap_err()["error"]["reason"],
            "file.outside_workspace"
        );
    }

    /// One canned HTTP answer per connection; records each request's head.
    async fn server(
        status: &'static str,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(vec![]));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let mut head = vec![];
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).await.unwrap_or(0) == 0 {
                        break;
                    }
                    head.push(b[0]);
                }
                seen2
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&head).to_string());
                let mut out = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in &headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                let _ = s.write_all(out.as_bytes()).await;
                let _ = s.write_all(&body).await;
                let _ = s.shutdown().await;
            }
        });
        (url, seen)
    }

    fn local(url: &str, root: &Path) -> Local {
        Local {
            server_url: url.to_string(),
            token: Arc::new("tm_secret".into()),
            run_id: "R1".into(),
            root: root.to_path_buf(),
            redactor: Arc::new(Redactor::new(vec![])),
        }
    }

    fn json_headers() -> Vec<(&'static str, String)> {
        vec![("Content-Type", "application/json".into())]
    }

    #[tokio::test]
    async fn topic_history_prints_the_server_json() {
        let root = tempfile::tempdir().unwrap();
        let page = json!({ "messages": [{ "id": "M1", "seq": 3, "authorId": "U", "authorName": "A", "at": 1, "text": "hi" }], "hasMore": true });
        let (url, seen) = server("200 OK", json_headers(), page.to_string().into_bytes()).await;
        let a = handle(
            TOPIC_HISTORY,
            &json!({ "before": 4, "limit": 10 }),
            &local(&url, root.path()),
        )
        .await;
        assert_eq!(a, json!({ "ok": true, "result": page }));
        let head = seen.lock().unwrap()[0].clone();
        assert!(
            head.starts_with("GET /v1/machine/runs/R1/topic/messages?before=4&limit=10 HTTP/1.1"),
            "{head}"
        );
        assert!(
            head.to_ascii_lowercase()
                .contains("authorization: bearer tm_secret"),
            "{head}"
        );
    }

    #[tokio::test]
    async fn topic_message_maps_server_errors() {
        let root = tempfile::tempdir().unwrap();
        let body = json!({ "error": { "code": "not_found", "message": "no such message" } });
        let (url, seen) = server(
            "404 Not Found",
            json_headers(),
            body.to_string().into_bytes(),
        )
        .await;
        let a = handle(
            TOPIC_MESSAGE,
            &json!({ "messageId": "M9" }),
            &local(&url, root.path()),
        )
        .await;
        assert_eq!(
            a,
            json!({ "ok": false, "error": { "code": "not_found", "message": "no such message" } })
        );
        assert!(seen.lock().unwrap()[0].starts_with("GET /v1/machine/runs/R1/topic/messages/M9 "));
    }

    #[tokio::test]
    async fn search_sends_the_encoded_query_and_scope() {
        let root = tempfile::tempdir().unwrap();
        let hits = json!({ "hits": [] });
        let (url, seen) = server("200 OK", json_headers(), hits.to_string().into_bytes()).await;
        let a = handle(
            CONTEXT_SEARCH,
            &json!({ "query": "上线 日期", "scope": "group" }),
            &local(&url, root.path()),
        )
        .await;
        assert_eq!(a["result"], hits);
        assert!(seen.lock().unwrap()[0].starts_with(
            "GET /v1/machine/runs/R1/search?q=%E4%B8%8A%E7%BA%BF%20%E6%97%A5%E6%9C%9F&scope=group "
        ));
    }

    #[tokio::test]
    async fn file_get_writes_into_the_default_folder_or_out() {
        let root = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(root.path()).unwrap();
        let (url, seen) = server(
            "200 OK",
            vec![
                ("Content-Type", "application/pdf".into()),
                ("X-File-Name", "%E6%8A%A5%E5%91%8A%2F1.pdf".into()),
            ],
            b"%PDF-1".to_vec(),
        )
        .await;
        let l = local(&url, root.path());
        let a = handle(FILE_GET, &json!({ "fileId": "F1" }), &l).await;
        let path = real.join(".chat-files/F1/报告_1.pdf");
        assert_eq!(
            a,
            json!({ "ok": true, "result": {
                "path": path.display().to_string(), "name": "报告/1.pdf",
                "mime": "application/pdf", "size": 6,
            } })
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"%PDF-1");
        assert!(seen.lock().unwrap()[0].starts_with("GET /v1/machine/runs/R1/files/F1 "));
        // No `.part` file is left.
        assert_eq!(
            std::fs::read_dir(real.join(".chat-files/F1"))
                .unwrap()
                .count(),
            1
        );

        let a = handle(FILE_GET, &json!({ "fileId": "F1", "out": "in/a.pdf" }), &l).await;
        assert_eq!(
            a["result"]["path"],
            real.join("in/a.pdf").display().to_string()
        );
        assert_eq!(std::fs::read(real.join("in/a.pdf")).unwrap(), b"%PDF-1");

        let a = handle(FILE_GET, &json!({ "fileId": "F1", "out": "../x.pdf" }), &l).await;
        assert_eq!(a["error"]["reason"], "file.outside_workspace");
        // Refused before any download.
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn file_get_refuses_files_over_100_mib() {
        let root = tempfile::tempdir().unwrap();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_GET_BYTES + 1
            );
            let _ = s.write_all(head.as_bytes()).await;
        });
        let a = handle(
            FILE_GET,
            &json!({ "fileId": "F2" }),
            &local(&url, root.path()),
        )
        .await;
        assert_eq!(a["error"]["code"], "too_large");
        assert!(
            !root.path().join(".chat-files/F2").exists()
                || std::fs::read_dir(root.path().join(".chat-files/F2"))
                    .unwrap()
                    .count()
                    == 0
        );
    }
}
