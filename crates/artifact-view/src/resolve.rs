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
    if is_diagram_host(host) {
        // The diagram renderer (REFS §B6): its own scripts only, no inline script.
        return format!(
            "default-src 'none'; script-src {me}; style-src 'unsafe-inline'; img-src data:; \
             connect-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; \
             object-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors 'none'"
        );
    }
    format!(
        "default-src 'none'; script-src {me} 'unsafe-inline'; style-src {me} 'unsafe-inline'; \
         img-src {me} data:; font-src {me} data:; media-src {me} data:; connect-src 'none'; \
         frame-src 'none'; child-src 'none'; worker-src 'none'; object-src 'none'; \
         manifest-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors 'none'"
    )
}

/// The reserved origin of the diagram renderer (`teamchat-diagram-v<n>`).
pub fn is_diagram_host(host: &str) -> bool {
    host.strip_prefix(crate::diagram::ARTIFACT_ID)
        .and_then(|r| r.strip_prefix("-v"))
        .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
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
        "mov" => "video/quicktime",
        "m4v" => "video/x-m4v",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "vtt" => "text/vtt; charset=utf-8",
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
        416 => b"range not satisfiable",
        _ => b"error",
    };
    response(status, host, "text/plain; charset=utf-8", Arc::from(text))
}

/// Answers one request for `url` when the window currently shows `host`.
pub fn serve(content: &Content, host: &str, entry: &str, method: &str, url: &str) -> Response {
    serve_range(content, host, entry, method, url, None)
}

/// A parsed single `Range: bytes=...` request header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteRange {
    /// `bytes=a-b` or `bytes=a-` (end `None`).
    From { start: u64, end: Option<u64> },
    /// `bytes=-n`: the last `n` bytes.
    Suffix(u64),
}

/// Parses a `Range` header. Returns `None` for a header this server ignores (other units,
/// several ranges, bad syntax): the full file is then sent with status 200.
pub fn parse_range(header: &str) -> Option<ByteRange> {
    let spec = header.trim();
    let rest = spec
        .get(..6)
        .filter(|p| p.eq_ignore_ascii_case("bytes="))
        .map(|_| spec[6..].trim())?;
    if rest.contains(',') {
        return None;
    }
    let (a, b) = rest.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let num = |s: &str| -> Option<u64> {
        (!s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    if a.is_empty() {
        return num(b).map(ByteRange::Suffix);
    }
    let start = num(a)?;
    let end = if b.is_empty() { None } else { Some(num(b)?) };
    if end.is_some_and(|e| e < start) {
        return None;
    }
    Some(ByteRange::From { start, end })
}

/// The inclusive byte span of `range` in a file of `len` bytes, or `None` when the range
/// cannot be satisfied (status 416).
pub fn resolve_range(range: ByteRange, len: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    match range {
        ByteRange::From { start, end } => {
            if start >= len {
                return None;
            }
            Some((start, end.unwrap_or(len - 1).min(len - 1)))
        }
        ByteRange::Suffix(0) => None,
        ByteRange::Suffix(n) => Some((len.saturating_sub(n), len - 1)),
    }
}

/// Like [`serve`], with the request's `Range` header. Media elements (`<video>`, `<audio>`)
/// read their files in ranges; a satisfiable single range gets status 206 with only those
/// bytes, so a large video is never read whole into memory.
pub fn serve_range(
    content: &Content,
    host: &str,
    entry: &str,
    method: &str,
    url: &str,
    range: Option<&str>,
) -> Response {
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
    let range = range.and_then(parse_range);
    match content {
        Content::Memory(files) => match files.get(&rel) {
            Some(bytes) => {
                let len = bytes.len() as u64;
                match range {
                    None => full(host, mime, bytes.clone()),
                    Some(r) => match resolve_range(r, len) {
                        Some((a, b)) => partial(
                            host,
                            mime,
                            Arc::from(&bytes[a as usize..=b as usize]),
                            a,
                            b,
                            len,
                        ),
                        None => unsatisfiable(host, len),
                    },
                }
            }
            None => error(404, host),
        },
        Content::Folder(root) => {
            let real = match open_contained(root, &rel) {
                Ok(v) => v,
                Err(status) => return error(status, host),
            };
            let (real, len) = real;
            match range {
                None => {
                    if len > MAX_FILE_BYTES {
                        return error(413, host);
                    }
                    match std::fs::read(&real) {
                        Ok(bytes) => full(host, mime, Arc::from(bytes)),
                        Err(_) => error(404, host),
                    }
                }
                Some(r) => match resolve_range(r, len) {
                    Some((a, b)) => {
                        // One reply carries at most MAX_RANGE_BYTES; the media element asks
                        // again for the rest.
                        let b = b.min(a + MAX_RANGE_BYTES - 1);
                        match read_span(&real, a, b) {
                            Ok(bytes) => partial(host, mime, bytes, a, b, len),
                            Err(_) => error(404, host),
                        }
                    }
                    None => unsatisfiable(host, len),
                },
            }
        }
    }
}

/// Largest body of one 206 reply from a folder.
pub const MAX_RANGE_BYTES: u64 = 8 * 1024 * 1024;

fn full(host: &str, mime: &str, body: Arc<[u8]>) -> Response {
    let mut r = response(200, host, mime, body);
    r.headers.push(("Accept-Ranges", "bytes".into()));
    r
}

fn partial(host: &str, mime: &str, body: Arc<[u8]>, a: u64, b: u64, len: u64) -> Response {
    let mut r = response(206, host, mime, body);
    r.headers.push(("Accept-Ranges", "bytes".into()));
    r.headers
        .push(("Content-Range", format!("bytes {a}-{b}/{len}")));
    r
}

fn unsatisfiable(host: &str, len: u64) -> Response {
    let mut r = error(416, host);
    r.headers.push(("Content-Range", format!("bytes */{len}")));
    r
}

/// The real path and length of `root/rel`, only when its real path (after symlinks) stays
/// inside `root`. `root` must already be canonical.
fn open_contained(root: &Path, rel: &str) -> Result<(PathBuf, u64), u16> {
    let real = root.join(rel).canonicalize().map_err(|_| 404u16)?;
    if !real.starts_with(root) {
        return Err(404);
    }
    let meta = std::fs::metadata(&real).map_err(|_| 404u16)?;
    if !meta.is_file() {
        return Err(404);
    }
    Ok((real, meta.len()))
}

fn read_span(path: &Path, a: u64, b: u64) -> std::io::Result<Arc<[u8]>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(a))?;
    let mut buf = vec![0u8; (b - a + 1) as usize];
    f.read_exact(&mut buf)?;
    Ok(Arc::from(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_origin_has_the_strict_csp() {
        assert!(is_diagram_host("teamchat-diagram-v1"));
        assert!(!is_diagram_host("teamchat-diagram-vx"));
        assert!(!is_diagram_host("a1-v1"));
        let c = csp("teamchat-diagram-v1");
        assert!(c.contains("script-src 'self' artifact://teamchat-diagram-v1;"));
        assert!(!c.contains("script-src 'self' artifact://teamchat-diagram-v1 'unsafe-inline'"));
        assert!(c.contains("connect-src 'none'"));
        assert!(csp("a1-v1").contains("'unsafe-inline'"));
    }

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
    fn ranges() {
        assert_eq!(
            parse_range("bytes=0-1"),
            Some(ByteRange::From {
                start: 0,
                end: Some(1)
            })
        );
        assert_eq!(
            parse_range("BYTES= 5-"),
            Some(ByteRange::From {
                start: 5,
                end: None
            })
        );
        assert_eq!(parse_range("bytes=-10"), Some(ByteRange::Suffix(10)));
        for bad in [
            "bytes=0-1,4-5",
            "items=0-1",
            "bytes=5-2",
            "bytes=a-b",
            "bytes=-",
            "bytes=+1-2",
        ] {
            assert_eq!(parse_range(bad), None, "{bad}");
        }
        assert_eq!(
            resolve_range(
                ByteRange::From {
                    start: 2,
                    end: Some(99)
                },
                10
            ),
            Some((2, 9))
        );
        assert_eq!(
            resolve_range(
                ByteRange::From {
                    start: 10,
                    end: None
                },
                10
            ),
            None
        );
        assert_eq!(resolve_range(ByteRange::Suffix(4), 10), Some((6, 9)));
        assert_eq!(resolve_range(ByteRange::Suffix(40), 10), Some((0, 9)));
        assert_eq!(resolve_range(ByteRange::Suffix(0), 10), None);

        let header = |r: &Response, k: &str| {
            r.headers
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.clone())
        };
        let c = Content::memory([("clip.mp4".into(), (0u8..10).collect::<Vec<_>>())]);
        let u = "artifact://a-v1/clip.mp4";
        let r = serve_range(&c, "a-v1", "index.html", "GET", u, Some("bytes=0-1"));
        assert_eq!((r.status, &*r.body), (206, &[0u8, 1][..]));
        assert_eq!(header(&r, "Content-Range").as_deref(), Some("bytes 0-1/10"));
        assert_eq!(header(&r, "Content-Length").as_deref(), Some("2"));
        assert_eq!(header(&r, "Content-Type").as_deref(), Some("video/mp4"));
        let r = serve_range(&c, "a-v1", "index.html", "GET", u, Some("bytes=20-"));
        assert_eq!(r.status, 416);
        assert_eq!(header(&r, "Content-Range").as_deref(), Some("bytes */10"));
        let r = serve_range(&c, "a-v1", "index.html", "GET", u, None);
        assert_eq!((r.status, r.body.len()), (200, 10));
        assert_eq!(header(&r, "Accept-Ranges").as_deref(), Some("bytes"));
        // A range never widens access: other origins and traversal still fail.
        let r = serve_range(
            &c,
            "a-v1",
            "index.html",
            "GET",
            "artifact://a-v2/clip.mp4",
            Some("bytes=0-1"),
        );
        assert_eq!(r.status, 404);

        let dir = std::env::temp_dir().join(format!("artifact-view-range-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.wav"), (0u8..100).collect::<Vec<_>>()).unwrap();
        let c = Content::Folder(dir.clone()).prepared().unwrap();
        let r = serve_range(
            &c,
            "a-v1",
            "index.html",
            "GET",
            "artifact://a-v1/a.wav",
            Some("bytes=-3"),
        );
        assert_eq!((r.status, &*r.body), (206, &[97u8, 98, 99][..]));
        assert_eq!(
            header(&r, "Content-Range").as_deref(),
            Some("bytes 97-99/100")
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
