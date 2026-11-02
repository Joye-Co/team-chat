//! Platform-independent parts of the `artifact://` scheme: origins, paths, MIME types, CSP
//! and the response for one request.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Error;

/// URL scheme that serves 产出 pages.
pub const SCHEME: &str = "artifact";

/// Largest file the scheme handler serves (the V1 limit for 产出 files).
pub const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Where the files of one 产出 version come from.
#[derive(Clone, Debug)]
pub enum Content {
    /// A folder on disk; the client caches each version in its own folder.
    Folder(PathBuf),
    /// Files in memory, keyed by relative path such as `index.html` or `img/a.png`.
    Memory(Arc<HashMap<String, Arc<[u8]>>>),
}

impl Content {
    /// Content of one in-memory file, served as `entry`.
    pub fn single(entry: &str, bytes: impl Into<Arc<[u8]>>) -> Self {
        let mut map = HashMap::new();
        map.insert(entry.to_string(), bytes.into());
        Content::Memory(Arc::new(map))
    }

    pub fn memory(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        Content::Memory(Arc::new(
            files.into_iter().map(|(k, v)| (k, Arc::from(v))).collect(),
        ))
    }

    /// Canonicalizes a folder root so later containment checks compare real paths.
    pub(crate) fn prepared(self) -> Result<Self, Error> {
        match self {
            Content::Folder(root) => {
                let canon = root
                    .canonicalize()
                    .map_err(|_| Error::FolderMissing(root.clone()))?;
                if !canon.is_dir() {
                    return Err(Error::FolderMissing(root));
                }
                Ok(Content::Folder(canon))
            }
            m => Ok(m),
        }
    }
}

/// The origin host for one artifact version: `<id>-v<version>`.
///
/// URL hosts are case-insensitive, so an id that is not all `[a-z0-9-]` (or that starts with
/// the escape prefix) is hex-encoded behind `x--`. The mapping is injective, so two
/// versions never share an origin.
pub fn origin_host(artifact_id: &str, version: u32) -> Result<String, Error> {
    if artifact_id.is_empty()
        || artifact_id.len() > 64
        || !artifact_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::InvalidArtifactId);
    }
    let plain = artifact_id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !artifact_id.starts_with("x--")
        && !artifact_id.starts_with('-');
    let enc = if plain {
        artifact_id.to_string()
    } else {
        let mut s = String::from("x--");
        for b in artifact_id.bytes() {
            s.push_str(&format!("{b:02x}"));
        }
        s
    };
    Ok(format!("{enc}-v{version}"))
}

/// The Content-Security-Policy for every response of an origin.
pub fn csp(host: &str) -> String {
    let me = format!("'self' {SCHEME}://{host}");
    format!(
        "default-src 'none'; script-src {me} 'unsafe-inline'; style-src {me} 'unsafe-inline'; \
         img-src {me} data:; font-src {me} data:; media-src {me} data:; connect-src 'none'; \
         frame-src 'none'; child-src 'none'; worker-src 'none'; object-src 'none'; \
         manifest-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors 'none'"
    )
}

/// The WKContentRuleList source: blocks every network and file load.
pub const CONTENT_RULES: &str = r#"[
 {"trigger":{"url-filter":"^[hH][tT][tT][pP][sS]?:"},"action":{"type":"block"}},
 {"trigger":{"url-filter":"^[wW][sS][sS]?:"},"action":{"type":"block"}},
 {"trigger":{"url-filter":"^[fF][tT][pP]:"},"action":{"type":"block"}},
 {"trigger":{"url-filter":"^[fF][iI][lL][eE]:"},"action":{"type":"block"}}
]"#;

/// Splits `artifact://host/path?query#frag` into lowercase host and raw path.
pub fn split_url(url: &str) -> Option<(String, &str)> {
    let rest = url
        .get(..SCHEME.len() + 3)
        .filter(|p| p.eq_ignore_ascii_case(&format!("{SCHEME}://")))
        .map(|_| &url[SCHEME.len() + 3..])?;
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let rest = &rest[..end];
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if host.is_empty() || host.contains(['@', ':']) {
        return None;
    }
    Some((host.to_ascii_lowercase(), path))
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Turns a URL path into a safe relative path. Rejects `.` and `..` segments (also when
/// percent-encoded), backslashes, NUL and empty segments. `/` and `dir/` map to `index.html`.
pub fn relative_path(url_path: &str, entry: &str) -> Option<String> {
    let decoded = percent_decode(url_path)?;
    let trimmed = decoded.strip_prefix('/').unwrap_or(&decoded);
    let mut rel = if trimmed.is_empty() {
        entry.to_string()
    } else {
        trimmed.to_string()
    };
    if rel.ends_with('/') {
        rel.push_str("index.html");
    }
    validate_rel(&rel).then_some(rel)
}

pub(crate) fn validate_rel(rel: &str) -> bool {
    !rel.is_empty()
        && rel.len() <= 1024
        && !rel.contains(['\\', '\0'])
        && rel
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

/// Percent-encodes a validated relative path for use in a URL.
pub(crate) fn encode_path(rel: &str) -> String {
    let mut s = String::new();
    for b in rel.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// MIME type by file extension. Text types carry `charset=utf-8`.
pub fn mime_for(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "xhtml" | "xht" => "application/xhtml+xml; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "txt" | "log" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// One response of the scheme handler.
pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Arc<[u8]>,
}

fn response(status: u16, host: &str, mime: &str, body: Arc<[u8]>) -> Response {
    Response {
        status,
        headers: vec![
            ("Content-Type", mime.to_string()),
            ("Content-Length", body.len().to_string()),
            ("Content-Security-Policy", csp(host)),
            ("X-Content-Type-Options", "nosniff".into()),
            ("Cache-Control", "no-store".into()),
            ("Referrer-Policy", "no-referrer".into()),
            ("X-DNS-Prefetch-Control", "off".into()),
            ("Cross-Origin-Resource-Policy", "same-origin".into()),
        ],
        body,
    }
}

fn error(status: u16, host: &str) -> Response {
    let text: &[u8] = match status {
        404 => b"not found",
        405 => b"method not allowed",
        413 => b"too large",
        _ => b"error",
    };
    response(status, host, "text/plain; charset=utf-8", Arc::from(text))
}

/// Answers one request for `url` when the window currently shows `host`.
pub fn serve(content: &Content, host: &str, entry: &str, method: &str, url: &str) -> Response {
    if !method.eq_ignore_ascii_case("GET") {
        return error(405, host);
    }
    let Some((req_host, path)) = split_url(url) else {
        return error(404, host);
    };
    if req_host != host {
        return error(404, host);
    }
    let Some(rel) = relative_path(path, entry) else {
        return error(404, host);
    };
    let mime = mime_for(&rel);
    match content {
        Content::Memory(files) => match files.get(&rel) {
            Some(bytes) => response(200, host, mime, bytes.clone()),
            None => error(404, host),
        },
        Content::Folder(root) => match read_contained(root, &rel) {
            Ok(bytes) => response(200, host, mime, bytes),
            Err(status) => error(status, host),
        },
    }
}

/// Reads `root/rel` only when its real path (after symlinks) stays inside `root`.
/// `root` must already be canonical.
fn read_contained(root: &Path, rel: &str) -> Result<Arc<[u8]>, u16> {
    let real = root.join(rel).canonicalize().map_err(|_| 404u16)?;
    if !real.starts_with(root) {
        return Err(404);
    }
    let meta = std::fs::metadata(&real).map_err(|_| 404u16)?;
    if !meta.is_file() {
        return Err(404);
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(413);
    }
    std::fs::read(&real).map(Arc::from).map_err(|_| 404)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_injective_and_lowercase() {
        assert_eq!(origin_host("abc-1", 3).unwrap(), "abc-1-v3");
        assert_eq!(origin_host("ABC", 1).unwrap(), "x--414243-v1");
        assert_ne!(
            origin_host("ABC", 1).unwrap(),
            origin_host("abc", 1).unwrap()
        );
        assert_eq!(origin_host("x--41", 1).unwrap(), "x--782d2d3431-v1");
        assert!(origin_host("", 1).is_err());
        assert!(origin_host("a/b", 1).is_err());
        assert!(origin_host("a.b", 1).is_err());
        assert!(origin_host(&"a".repeat(65), 1).is_err());
    }

    #[test]
    fn paths_reject_traversal() {
        assert_eq!(
            relative_path("/", "index.html").as_deref(),
            Some("index.html")
        );
        assert_eq!(
            relative_path("/a/b.js", "index.html").as_deref(),
            Some("a/b.js")
        );
        assert_eq!(
            relative_path("/docs/", "index.html").as_deref(),
            Some("docs/index.html")
        );
        assert_eq!(relative_path("/a%20b.png", "i").as_deref(), Some("a b.png"));
        for bad in [
            "/../x",
            "/a/../../x",
            "/%2e%2e/x",
            "/%2E%2E%2Fsecret",
            "/a/./b",
            "/a//b",
            "/..%5cx",
            "/a%00b",
            "/%zz",
            "/%ff%fe",
        ] {
            assert_eq!(relative_path(bad, "index.html"), None, "{bad}");
        }
    }

    #[test]
    fn split_url_checks_scheme_and_host() {
        assert_eq!(
            split_url("artifact://AbC-v1/x.js?q#f"),
            Some(("abc-v1".into(), "/x.js"))
        );
        assert_eq!(split_url("artifact://h-v1"), Some(("h-v1".into(), "/")));
        assert_eq!(split_url("https://h-v1/x"), None);
        assert_eq!(split_url("artifact://user@h/x"), None);
        assert_eq!(split_url("artifact:///x"), None);
    }

    #[test]
    fn serve_memory_and_folder() {
        let c = Content::memory([("index.html".into(), b"<p>hi</p>".to_vec())]);
        let r = serve(&c, "a-v1", "index.html", "GET", "artifact://a-v1/");
        assert_eq!(r.status, 200);
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| *k == "Content-Type" && v.starts_with("text/html"))
        );
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| *k == "Content-Security-Policy" && v.contains("connect-src 'none'"))
        );
        assert_eq!(
            serve(&c, "a-v1", "index.html", "GET", "artifact://a-v2/").status,
            404
        );
        assert_eq!(
            serve(&c, "a-v1", "index.html", "POST", "artifact://a-v1/").status,
            405
        );

        let dir = std::env::temp_dir().join(format!("artifact-view-test-{}", std::process::id()));
        let root = dir.join("v1");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.css"), b"p{}").unwrap();
        std::fs::write(dir.join("secret.txt"), b"secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("secret.txt"), root.join("link.txt")).unwrap();
        let c = Content::Folder(root.clone()).prepared().unwrap();
        let r = serve(&c, "a-v1", "index.html", "GET", "artifact://a-v1/sub/a.css");
        assert_eq!(r.status, 200);
        assert_eq!(&*r.body, b"p{}");
        assert_eq!(
            serve(
                &c,
                "a-v1",
                "index.html",
                "GET",
                "artifact://a-v1/../secret.txt"
            )
            .status,
            404
        );
        assert_eq!(
            serve(
                &c,
                "a-v1",
                "index.html",
                "GET",
                "artifact://a-v1/%2e%2e/secret.txt"
            )
            .status,
            404
        );
        assert_eq!(
            serve(&c, "a-v1", "index.html", "GET", "artifact://a-v1/link.txt").status,
            404
        );
        assert_eq!(
            serve(&c, "a-v1", "index.html", "GET", "artifact://a-v1/sub").status,
            404
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mime_types() {
        assert_eq!(mime_for("a.JS"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("a.svg"), "image/svg+xml");
        assert_eq!(mime_for("a.XHTML"), "application/xhtml+xml; charset=utf-8");
        assert_eq!(mime_for("noext"), "application/octet-stream");
    }
}
