//! `chat-run` files and 产出 (docs/machine-protocol.md, "Files and 产出"): the daemon itself
//! uploads the files a CLI names and makes the 产出 or its new version over HTTP with the
//! machine token, as the run's runner member, with the run's id. No run event carries them:
//! files can be larger than a machine frame.
//!
//! Only files inside the run's working folder are read (a CLI cannot send `~/.ssh` this way);
//! symbolic links are followed only when they stay inside it. Text files are redacted like run
//! events before they leave the machine.

use crate::redact::Redactor;
use crate::upload::same_origin;
use crate::workspace::hex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// `chat-run` ops the daemon answers itself.
pub const FILE_UPLOAD: &str = "file.upload";
pub const ARTIFACT_CREATE: &str = "artifact.create";
pub const ARTIFACT_VERSION: &str = "artifact.version";
pub const OPS: &[&str] = &[FILE_UPLOAD, ARTIFACT_CREATE, ARTIFACT_VERSION];

/// The protocol's limits (`MAX_FILE_BYTES`, `MAX_ARTIFACT_FILES`).
const MAX_FILE_BYTES: u64 = 100_000_000;
const MAX_FILES: usize = 500;
/// How long `chat-run` waits for these ops (uploads take longer than an output).
pub const TIMEOUT: Duration = Duration::from_secs(300);

/// What the daemon needs to answer these ops for one run.
pub struct Local {
    pub server_url: String,
    pub token: Arc<String>,
    pub run_id: String,
    /// The run's working folder; only files inside it are sent.
    pub root: PathBuf,
    pub redactor: Arc<Redactor>,
}

pub(crate) fn err(code: &str, reason: Option<&str>, message: impl Into<String>) -> Value {
    let mut e = json!({ "code": code, "message": message.into() });
    if let Some(r) = reason {
        e["reason"] = json!(r);
    }
    json!({ "ok": false, "error": e })
}

/// A path inside the run's working folder, resolved (symbolic links included).
pub(crate) fn inside(root: &Path, path: &str) -> Result<PathBuf, Value> {
    // Windows: the spellings that reach outside the file system are refused before anything
    // is opened (a network path would sign the daemon's user in to a server).
    let path = if cfg!(windows) {
        crate::sys::windows_plain_path(path)
    } else {
        path
    };
    if cfg!(windows)
        && let Some(why) = crate::sys::windows_path_problem(path)
    {
        return Err(err(
            "forbidden",
            Some("file.outside_workspace"),
            format!("{path} is {why}, not a file in the run's working folder"),
        ));
    }
    let root = std::fs::canonicalize(root).map_err(|e| {
        err(
            "unavailable",
            None,
            format!("the working folder is gone: {e}"),
        )
    })?;
    let p = Path::new(path);
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    };
    let real = std::fs::canonicalize(&p).map_err(|e| {
        err(
            "not_found",
            None,
            format!("cannot read {}: {e}", p.display()),
        )
    })?;
    if !real.starts_with(&root) {
        return Err(err(
            "forbidden",
            Some("file.outside_workspace"),
            format!(
                "{} is outside the run's working folder {}",
                p.display(),
                root.display()
            ),
        ));
    }
    Ok(real)
}

/// The type of a file from its name; the server guesses again for `application/octet-stream`.
pub fn mime_of(name: &str) -> &'static str {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

pub(crate) fn client(timeout: Duration) -> Result<reqwest::Client, Value> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| err("unavailable", None, e.to_string()))
}

pub(crate) fn url(local: &Local, path: &str) -> Result<String, Value> {
    let u = format!("{}{path}", local.server_url.trim_end_matches('/'));
    // The machine token goes only to the configured server.
    if !same_origin(&u, &local.server_url) {
        return Err(err("invalid", None, "bad server url"));
    }
    Ok(u)
}

/// The answer of a server call as `{ok, result}` / `{ok: false, error}`.
pub(crate) async fn answer(res: Result<reqwest::Response, reqwest::Error>) -> Value {
    match res {
        Err(e) => err(
            "unavailable",
            Some("server.not_connected"),
            format!("cannot reach the server: {}", e.without_url()),
        ),
        Ok(r) => {
            let status = r.status();
            let body: Value = r
                .bytes()
                .await
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or(Value::Null);
            if status.is_success() {
                return json!({ "ok": true, "result": body });
            }
            let e = &body["error"];
            let code = e["code"].as_str().unwrap_or("failed");
            let msg = e["message"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
            err(code, e["reason"].as_str(), msg)
        }
    }
}

/// Uploads one file of the working folder; returns the server's `FileMeta` (or the error answer).
async fn upload(
    local: &Local,
    path: &Path,
    name: &str,
    mime: Option<&str>,
) -> Result<Value, Value> {
    let meta = std::fs::metadata(path).map_err(|e| err("not_found", None, e.to_string()))?;
    if !meta.is_file() {
        return Err(err(
            "invalid",
            None,
            format!("{} is not a file", path.display()),
        ));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(err("too_large", None, format!("{name} is over 100 MB")));
    }
    let raw = tokio::fs::read(path)
        .await
        .map_err(|e| err("not_found", None, format!("cannot read {name}: {e}")))?;
    // Text leaves the machine redacted, like run events.
    let bytes = match String::from_utf8(raw) {
        Ok(text) => local.redactor.text(&text).into_bytes(),
        Err(e) => e.into_bytes(),
    };
    let sha = hex(&Sha256::digest(&bytes));
    let mime = mime.unwrap_or_else(|| mime_of(name));
    let target = url(local, &format!("/v1/machine/runs/{}/files", local.run_id))?;
    let res = client(TIMEOUT)?
        .put(target)
        .bearer_auth(local.token.as_str())
        .header("Content-Type", mime)
        .header("X-File-Name", encode(name))
        .header("X-File-Sha256", sha)
        .body(bytes)
        .send()
        .await;
    let a = answer(res).await;
    if a["ok"] == json!(true) {
        Ok(a["result"].clone())
    } else {
        Err(a)
    }
}

/// Percent-encodes a file name for the `X-File-Name` header.
pub(crate) fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The files of a folder (recursively, regular files only, not hidden ones), as paths relative
/// to it with `/`, sorted.
fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), Value> {
    let entries = std::fs::read_dir(dir).map_err(|e| err("not_found", None, e.to_string()))?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let ft = e
            .file_type()
            .map_err(|e| err("failed", None, e.to_string()))?;
        if ft.is_dir() {
            walk(&e.path(), root, out)?;
        } else if ft.is_file() {
            let rel = e
                .path()
                .strip_prefix(root)
                .map(|p| {
                    p.components()
                        .map(|c| c.as_os_str().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .unwrap_or(name);
            out.push((rel, e.path()));
            if out.len() > MAX_FILES {
                return Err(err(
                    "too_large",
                    None,
                    format!("more than {MAX_FILES} files"),
                ));
            }
        }
    }
    Ok(())
}

/// `files` of a version from `dir` (a folder) or `file` (one file), uploaded; and the default
/// entry (`index.html`, else the only file).
async fn version_files(local: &Local, body: &Value) -> Result<(Vec<Value>, Option<String>), Value> {
    let mut list: Vec<(String, PathBuf)> = vec![];
    if let Some(dir) = body["dir"].as_str() {
        let dir = inside(&local.root, dir)?;
        walk(&dir, &dir, &mut list)?;
        list.sort();
    } else if let Some(file) = body["file"].as_str() {
        let file = inside(&local.root, file)?;
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        list.push((name, file));
    } else {
        return Ok((vec![], None));
    }
    if list.is_empty() {
        return Err(err("invalid", None, "no files to send"));
    }
    let entry = if list.iter().any(|(p, _)| p == "index.html") {
        Some("index.html".to_string())
    } else if list.len() == 1 {
        Some(list[0].0.clone())
    } else {
        None
    };
    let mut files = vec![];
    for (rel, path) in &list {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let meta = upload(local, path, name, None).await?;
        files.push(json!({ "path": rel, "fileId": meta["id"] }));
    }
    Ok((files, entry))
}

/// The JSON body of a 产出 request from a `chat-run` body: uploads `dir`/`file` into `files`.
async fn artifact_body(local: &Local, body: &Value, create: bool) -> Result<Value, Value> {
    let (files, entry) = version_files(local, body).await?;
    let mut out = serde_json::Map::new();
    for k in [
        "title",
        "kind",
        "entry",
        "files",
        "note",
        "events",
        "media",
        "data",
        "parentVersion",
    ] {
        if !body[k].is_null() {
            out.insert(k.to_string(), body[k].clone());
        }
    }
    if !files.is_empty() {
        out.insert("files".into(), json!(files));
    }
    if !out.contains_key("entry")
        && let Some(e) = entry
    {
        out.insert("entry".into(), json!(e));
    }
    if create && !out.contains_key("kind") {
        let html = out["entry"]
            .as_str()
            .is_some_and(|e| e.ends_with(".html") || e.ends_with(".htm"));
        out.insert("kind".into(), json!(if html { "html" } else { "file" }));
    }
    if !out.contains_key("files") {
        return Err(err("invalid", None, "give --dir or --file"));
    }
    if !out.contains_key("entry") {
        return Err(err(
            "invalid",
            None,
            "--entry is required for a folder without index.html",
        ));
    }
    Ok(Value::Object(out))
}

/// Answers one of [`OPS`].
pub async fn handle(op: &str, body: &Value, local: &Local) -> Value {
    let run = &local.run_id;
    match op {
        FILE_UPLOAD => {
            let Some(path) = body["path"].as_str() else {
                return err("invalid", None, "path is required");
            };
            let real = match inside(&local.root, path) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let name = body["name"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    real.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "file".into())
                });
            match upload(local, &real, &name, body["mime"].as_str()).await {
                Ok(meta) => json!({ "ok": true, "result": { "file": meta } }),
                Err(e) => e,
            }
        }
        ARTIFACT_CREATE | ARTIFACT_VERSION => {
            let create = op == ARTIFACT_CREATE;
            let path = if create {
                format!("/v1/machine/runs/{run}/artifacts")
            } else {
                let Some(id) = body["artifactId"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()))
                else {
                    return err("invalid", None, "artifactId is required");
                };
                format!("/v1/machine/runs/{run}/artifacts/{id}/versions")
            };
            let req = match artifact_body(local, body, create).await {
                Ok(b) => b,
                Err(e) => return e,
            };
            let target = match url(local, &path) {
                Ok(u) => u,
                Err(e) => return e,
            };
            let c = match client(TIMEOUT) {
                Ok(c) => c,
                Err(e) => return e,
            };
            answer(
                c.post(target)
                    .bearer_auth(local.token.as_str())
                    .header("Content-Type", "application/json")
                    .body(req.to_string())
                    .send()
                    .await,
            )
            .await
        }
        _ => err("invalid", Some("output.unknown_op"), "unknown op"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_outside_the_working_folder_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.html"), "x").unwrap();
        std::fs::create_dir(root.path().join("docs")).unwrap();
        std::fs::write(root.path().join("docs").join("b.txt"), "x").unwrap();
        std::fs::write(other.path().join("secret"), "x").unwrap();
        assert!(inside(root.path(), "a.html").is_ok());
        assert!(inside(root.path(), "docs/b.txt").is_ok());
        assert!(inside(root.path(), root.path().join("a.html").to_str().unwrap()).is_ok());
        let out = inside(root.path(), other.path().join("secret").to_str().unwrap()).unwrap_err();
        assert_eq!(out["error"]["reason"], "file.outside_workspace");
        let out = inside(root.path(), "../../etc/passwd").unwrap_err();
        assert_ne!(out["ok"], true);
        // A link that points out of the folder is refused too: a link to a file, and a link
        // to a folder (on Windows a junction, which needs no privilege) with a file below it.
        let links = crate::chat_run_context::test_links::make(root.path(), other.path());
        for link in &links {
            assert_eq!(
                inside(root.path(), link).unwrap_err()["error"]["reason"],
                "file.outside_workspace",
                "{link}"
            );
        }
        assert!(links.iter().any(|l| l.starts_with("away")), "{links:?}");
    }

    /// Windows: every spelling that names something else than a file under the working
    /// folder is refused, most of them before the file system is asked.
    #[cfg(windows)]
    #[test]
    fn windows_spellings_do_not_leave_the_working_folder() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.html"), "x").unwrap();
        std::fs::write(other.path().join("secret.txt"), "x").unwrap();
        let outside = other.path().join("secret.txt");
        let outside = outside.to_str().unwrap();
        let drive = &outside[..2];
        // The 8.3 short name of the other folder, when the volume keeps short names.
        let short = crate::chat_run_context::test_links::short_name(other.path());
        let mut bad: Vec<String> = vec![
            outside.to_string(),
            outside.to_uppercase(),
            outside.to_lowercase(),
            outside.replace('\\', "/"),
            format!(r"\\?\{outside}"),
            format!(r"\\?\UNC\localhost\{}$\{}", &drive[..1], &outside[3..]),
            format!(r"\\.\{outside}"),
            format!(r"\\localhost\{}$\{}", &drive[..1], &outside[3..]),
            format!(r"\\127.0.0.1\{}$\{}", &drive[..1], &outside[3..]),
            format!(
                "//localhost/{}$/{}",
                &drive[..1],
                outside[3..].replace('\\', "/")
            ),
            format!("{drive}{}", &outside[3..]),
            format!("{outside}."),
            format!("{outside} "),
            format!("{outside}::$DATA"),
            format!("{outside}:stream"),
            r"..\..\..\..\..\..\Windows\win.ini".to_string(),
            r"C:\Windows\win.ini".to_string(),
            r"\Windows\win.ini".to_string(),
            "/Windows/win.ini".to_string(),
            "a.html::$DATA".to_string(),
            "a.html:other".to_string(),
            "a.html.".to_string(),
            "a.html ".to_string(),
            "NUL".to_string(),
            "CON".to_string(),
            r"\\.\pipe\chat-run-test".to_string(),
        ];
        if let Some(short) = short {
            bad.push(format!(r"{short}\secret.txt"));
        }
        for path in &bad {
            let out = inside(root.path(), path);
            assert!(out.is_err(), "{path} was taken as {out:?}");
            assert_ne!(out.unwrap_err()["ok"], true, "{path}");
        }
        // The same file in the working folder under other spellings is the file itself.
        let real = std::fs::canonicalize(root.path().join("a.html")).unwrap();
        std::fs::create_dir_all(root.path().join("docs")).unwrap();
        // Also the resolved form the `chat-run` client sends (`\\?\C:\…`).
        let resolved = real.to_string_lossy().into_owned();
        assert!(resolved.starts_with(r"\\?\"), "{resolved}");
        for same in ["A.HTML", r".\a.html", r"docs\..\a.html", resolved.as_str()] {
            assert_eq!(inside(root.path(), same).unwrap(), real, "{same}");
        }
    }

    #[test]
    fn folders_list_visible_files_with_slash_paths() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("assets")).unwrap();
        std::fs::write(root.path().join("index.html"), "x").unwrap();
        std::fs::write(root.path().join("assets/app.js"), "x").unwrap();
        std::fs::write(root.path().join(".hidden"), "x").unwrap();
        let mut out = vec![];
        walk(root.path(), root.path(), &mut out).unwrap();
        out.sort();
        let paths: Vec<_> = out.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["assets/app.js", "index.html"]);
        assert_eq!(mime_of("index.HTML"), "text/html");
        assert_eq!(encode("报告 1.html"), "%E6%8A%A5%E5%91%8A%201.html");
    }

    #[tokio::test]
    async fn a_version_body_carries_its_parent_version() {
        let root = tempfile::tempdir().unwrap();
        let local = Local {
            server_url: "http://127.0.0.1:9".into(),
            token: Arc::new("tm_test".into()),
            run_id: "R1".into(),
            root: root.path().to_path_buf(),
            redactor: Arc::new(Redactor::new(vec![])),
        };
        let files = json!([{ "path": "index.html", "fileId": "F1" }]);
        let body =
            json!({ "artifactId": "A", "entry": "index.html", "files": files, "parentVersion": 2 });
        let out = artifact_body(&local, &body, false).await.unwrap();
        assert_eq!(out["parentVersion"], 2);
        assert!(out.get("artifactId").is_none());
        let body = json!({ "artifactId": "A", "entry": "index.html", "files": files });
        let out = artifact_body(&local, &body, false).await.unwrap();
        assert!(out.get("parentVersion").is_none());
    }
}
