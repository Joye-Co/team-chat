//! File transfers: uploads with progress (SHA-256 computed while reading, streamed
//! body) and downloads into a content-addressed cache with a size limit and LRU
//! cleanup. Run these on the core runtime ([`crate::Client::spawn`]); the
//! [`crate::Client`] wrappers in `client_ext` do that.

use crate::http::{ApiError, ApiResult, HttpClient, redact};
use crate::proto::{
    CreateFileRequest, FileMeta, FileRef, FileStatus, MAX_FILE_BYTES, MAX_UPLOAD_BYTES,
    MultipartUpload, UploadPartResponse,
};
use crate::store::Store;
use futures_util::StreamExt;
use reqwest::Method;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Read and write size of one chunk.
const CHUNK: usize = 256 * 1024;
/// Deadline of one whole transfer.
pub(crate) const TRANSFER_TIMEOUT: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Reading the local file to compute its SHA-256.
    Hashing,
    Uploading,
    Downloading,
    /// Checking a finished download.
    Verifying,
}

/// Progress of one transfer. `total` is the expected byte count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
}

/// A progress callback. It is called from the core runtime threads, at most once per
/// 256 KiB chunk; keep it cheap (send to a channel, store in an atomic).
pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

/// A callback that ignores progress.
pub fn no_progress() -> ProgressFn {
    Arc::new(|_| {})
}

/// A local file to upload into a group (and optionally a topic).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UploadRequest {
    pub group_id: String,
    pub topic_id: Option<String>,
    pub path: PathBuf,
    /// Default: the file name of `path`.
    pub name: Option<String>,
    /// Default: guessed from the name's extension.
    pub mime: Option<String>,
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The MIME type for a file name, from its extension. Unknown: application/octet-stream.
pub fn guess_mime(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "ico" => "image/x-icon",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "txt" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "tar" => "application/x-tar",
        "dmg" => "application/x-apple-diskimage",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// Pixel size of an image file from its header (png, jpeg, gif, webp, heic, bmp, tiff
/// and more), or `None` when it is not an image this reader knows.
pub fn image_size(path: &Path) -> Option<(u32, u32)> {
    let s = imagesize::size(path).ok()?;
    let w = u32::try_from(s.width).ok()?;
    let h = u32::try_from(s.height).ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

/// Pixel size of an image held in memory.
pub fn image_size_of(bytes: &[u8]) -> Option<(u32, u32)> {
    let s = imagesize::blob_size(bytes).ok()?;
    let w = u32::try_from(s.width).ok()?;
    let h = u32::try_from(s.height).ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

/// SHA-256 (lowercase hex) and size of a file, read in chunks.
pub async fn hash_file(path: &Path, progress: &ProgressFn) -> ApiResult<(String, u64)> {
    let mut f = tokio::fs::File::open(path).await?;
    let total = f.metadata().await?.len();
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        progress(Progress {
            phase: Phase::Hashing,
            done,
            total,
        });
    }
    Ok((hex(&h.finalize()), done))
}

/// Uploads a local file: hashes it, creates the record, streams the bytes and returns
/// the `ready` record. Use [`FileMeta::to_file_ref`] for `SendRequest.attachments`.
/// The file is hashed again while it streams; a file that changed in between fails
/// with [`ApiError::Checksum`] before the last byte is sent.
pub async fn upload_file(
    http: &HttpClient,
    req: &UploadRequest,
    progress: &ProgressFn,
) -> ApiResult<FileMeta> {
    let name = match &req.name {
        Some(n) => n.clone(),
        None => req
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .ok_or_else(|| ApiError::Invalid("file has no name".into()))?,
    };
    let mime = req
        .mime
        .clone()
        .unwrap_or_else(|| guess_mime(&name).to_string());
    let size = tokio::fs::metadata(&req.path).await?.len();
    // 1.1: up to 2 GB, refused before anything is hashed or sent. Over 100 MB uploads in parts.
    if size > MAX_UPLOAD_BYTES {
        return Err(ApiError::Invalid(format!(
            "file larger than {MAX_UPLOAD_BYTES} bytes"
        )));
    }
    let (width, height) = if mime.starts_with("image/") {
        let p = req.path.clone();
        tokio::task::spawn_blocking(move || image_size(&p))
            .await
            .ok()
            .flatten()
            .map_or((None, None), |(w, h)| (Some(w), Some(h)))
    } else {
        (None, None)
    };
    let (sha256, hashed) = hash_file(&req.path, progress).await?;
    if hashed != size {
        return Err(ApiError::Checksum);
    }
    let created = http
        .create_file(&CreateFileRequest {
            group_id: req.group_id.clone(),
            topic_id: req.topic_id.clone(),
            name,
            size,
            mime,
            sha256: sha256.clone(),
            width,
            height,
        })
        .await?;
    if size > MAX_FILE_BYTES {
        let Some(mp) = created.multipart.as_ref() else {
            return Err(ApiError::Invalid(format!(
                "file larger than {MAX_FILE_BYTES} bytes"
            )));
        };
        return upload_parts(http, &req.path, size, &sha256, mp, progress).await;
    }
    if !http.same_origin(&created.upload_url) {
        return Err(ApiError::BadUrl);
    }
    let file = tokio::fs::File::open(&req.path).await?;
    let body = reqwest::Body::wrap_stream(upload_stream(file, size, sha256, progress.clone()));
    let put = http
        .authed(Method::PUT, &created.upload_url)?
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::CONTENT_LENGTH, size)
        .timeout(TRANSFER_TIMEOUT)
        .body(body);
    let resp = http.send_checked(put).await?;
    let meta: FileMeta = HttpClient::json(resp).await?;
    if meta.status != FileStatus::Ready {
        return Err(ApiError::Decode("upload did not finish the file".into()));
    }
    Ok(meta)
}

/// Tries per part of a multipart upload before the upload fails.
const PART_TRIES: u32 = 3;

/// Uploads a file over [`MAX_FILE_BYTES`] in parts, in order, each part tried up to
/// [`PART_TRIES`] times, then completes it. The file is hashed again on the way; a file that
/// changed fails with [`ApiError::Checksum`] before the last part is sent.
async fn upload_parts(
    http: &HttpClient,
    path: &Path,
    size: u64,
    sha256: &str,
    mp: &MultipartUpload,
    progress: &ProgressFn,
) -> ApiResult<FileMeta> {
    if !http.same_origin(&mp.part_url) || !http.same_origin(&mp.complete_url) {
        return Err(ApiError::BadUrl);
    }
    let mut file = tokio::fs::File::open(path).await?;
    let mut hash = Sha256::new();
    let mut done = 0u64;
    for n in 1..=mp.parts {
        let len = mp.part_size(size, n);
        let mut buf = vec![0u8; len as usize];
        file.read_exact(&mut buf)
            .await
            .map_err(|_| ApiError::Checksum)?;
        hash.update(&buf);
        if n == mp.parts {
            let mut extra = [0u8; 1];
            if file.read(&mut extra).await? != 0 || hex(&hash.clone().finalize()) != sha256 {
                return Err(ApiError::Checksum);
            }
        }
        let url = format!("{}/{n}", mp.part_url);
        let mut tries = 0;
        loop {
            tries += 1;
            let put = http
                .authed(Method::PUT, &url)?
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .header(reqwest::header::CONTENT_LENGTH, len)
                .timeout(TRANSFER_TIMEOUT)
                .body(buf.clone());
            match http.send_checked(put).await {
                Ok(resp) => {
                    let _: UploadPartResponse = HttpClient::json(resp).await?;
                    break;
                }
                Err(e)
                    if tries < PART_TRIES
                        && matches!(
                            e,
                            ApiError::Network(_)
                                | ApiError::Server { status: 500.., .. }
                                | ApiError::Unexpected { status: 500.. }
                        ) =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        done += len;
        progress(Progress {
            phase: Phase::Uploading,
            done,
            total: size,
        });
    }
    let resp = http
        .send_checked(
            http.authed(Method::POST, &mp.complete_url)?
                .timeout(TRANSFER_TIMEOUT),
        )
        .await?;
    let meta: FileMeta = HttpClient::json(resp).await?;
    if meta.status != FileStatus::Ready {
        return Err(ApiError::Decode("upload did not finish the file".into()));
    }
    Ok(meta)
}

/// The upload body: chunks of the file, hashed on the way. The last chunk is held
/// back until the hash is checked, so a changed file never completes the upload.
fn upload_stream(
    file: tokio::fs::File,
    size: u64,
    sha256: String,
    progress: ProgressFn,
) -> impl futures_util::Stream<Item = std::io::Result<Vec<u8>>> + Send + 'static {
    struct St {
        file: tokio::fs::File,
        hash: Option<Sha256>,
        held: Option<Vec<u8>>,
        done: u64,
    }
    let st = St {
        file,
        hash: Some(Sha256::new()),
        held: None,
        done: 0,
    };
    futures_util::stream::unfold(st, move |mut st| {
        let sha256 = sha256.clone();
        let progress = progress.clone();
        async move {
            st.hash.as_ref()?;
            let mut buf = vec![0u8; CHUNK];
            let n = match st.file.read(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    st.hash = None;
                    return Some((Err(e), st));
                }
            };
            buf.truncate(n);
            if n == 0 {
                // End of file: check the hash before the held chunk goes out.
                let h = st.hash.take()?;
                if st.done != size || hex(&h.finalize()) != sha256 {
                    return Some((Err(std::io::Error::other("file changed during upload")), st));
                }
                let last = st.held.take().unwrap_or_default();
                progress(Progress {
                    phase: Phase::Uploading,
                    done: st.done,
                    total: size,
                });
                return Some((Ok(last), st));
            }
            if let Some(h) = st.hash.as_mut() {
                h.update(&buf);
            }
            st.done += n as u64;
            if st.done > size {
                st.hash = None;
                return Some((Err(std::io::Error::other("file changed during upload")), st));
            }
            let out = st.held.replace(buf).unwrap_or_default();
            progress(Progress {
                phase: Phase::Uploading,
                done: st.done.saturating_sub(n as u64),
                total: size,
            });
            Some((Ok(out), st))
        }
    })
    .filter(|r| std::future::ready(!matches!(r, Ok(v) if v.is_empty())))
}

/// A file name that is safe as one path segment.
pub(crate) fn safe_file_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    while s.starts_with('.') {
        s.remove(0);
    }
    if s.chars().count() > 200 {
        // Keep the extension.
        let ext: String = s
            .rsplit_once('.')
            .map(|(_, e)| e.chars().take(16).collect())
            .unwrap_or_default();
        let stem: String = s.chars().take(180).collect();
        s = if ext.is_empty() {
            stem
        } else {
            format!("{stem}.{ext}")
        };
    }
    if s.trim().is_empty() {
        "file".into()
    } else {
        s
    }
}

/// Streams `resp` into `dest_part` while hashing, then checks size and SHA-256.
pub(crate) async fn stream_to_file(
    resp: reqwest::Response,
    dest_part: &Path,
    expect_size: u64,
    expect_sha256: Option<&str>,
    progress: &ProgressFn,
) -> ApiResult<()> {
    let mut out = tokio::fs::File::create(dest_part).await?;
    let mut h = Sha256::new();
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(redact)?;
        done += chunk.len() as u64;
        if done > expect_size {
            return Err(ApiError::Checksum);
        }
        h.update(&chunk);
        out.write_all(&chunk).await?;
        progress(Progress {
            phase: Phase::Downloading,
            done,
            total: expect_size,
        });
    }
    out.flush().await?;
    out.sync_all().await?;
    drop(out);
    progress(Progress {
        phase: Phase::Verifying,
        done,
        total: expect_size,
    });
    if done != expect_size {
        return Err(ApiError::Checksum);
    }
    if let Some(sha) = expect_sha256
        && hex(&h.finalize()) != sha
    {
        return Err(ApiError::Checksum);
    }
    Ok(())
}

/// The downloaded-file cache: `<dir>/<sha256>/<name>`, indexed in the store.
#[derive(Clone)]
pub struct FileCache {
    pub dir: PathBuf,
    pub max_bytes: u64,
    store: Store,
}

impl FileCache {
    pub fn new(store: Store, dir: PathBuf, max_bytes: u64) -> FileCache {
        FileCache {
            dir,
            max_bytes,
            store,
        }
    }

    /// The cached copy of a file under its own name (`<dir>/<sha256>/<name>`), when the
    /// content is on disk. The same bytes sent under another name get a hard link (or a copy)
    /// with this name, so the path that opens always carries the name the member clicked:
    /// a safe name never opens a copy whose first name was dangerous (`.terminal`, `.app`).
    pub fn get_named(&self, sha256: &str, name: &str) -> Option<PathBuf> {
        let any = self.get(sha256)?;
        let want = self.dir.join(sha256).join(safe_file_name(name));
        if any == want || want.is_file() {
            return Some(want);
        }
        if std::fs::hard_link(&any, &want).is_err() {
            let part = self
                .dir
                .join(sha256)
                .join(format!(".part-{}", crate::new_id()));
            let guard = PartFile(Some(part.clone()));
            std::fs::copy(&any, &part).ok()?;
            quarantine(&part);
            std::fs::rename(&part, &want).ok()?;
            guard.keep();
        }
        Some(want)
    }

    /// A cached path of the content (under any name it was stored with). Marks it used.
    /// To open a file, use [`FileCache::get_named`].
    pub fn get(&self, sha256: &str) -> Option<PathBuf> {
        if !is_sha256(sha256) {
            return None;
        }
        let p = PathBuf::from(self.store.file_cache_get(sha256).ok()??);
        if p.is_file() && p.starts_with(&self.dir) {
            Some(p)
        } else {
            let _ = self.store.file_cache_remove(sha256);
            None
        }
    }

    /// Returns the cached copy or downloads the file (`GET /v1/files/:id/content`),
    /// checks its size and SHA-256, stores it and trims the cache to `max_bytes`.
    pub async fn fetch(
        &self,
        http: &HttpClient,
        file: &FileRef,
        progress: &ProgressFn,
    ) -> ApiResult<PathBuf> {
        if !is_sha256(&file.sha256) {
            return Err(ApiError::Invalid("bad sha256".into()));
        }
        if let Some(p) = self.get_named(&file.sha256, &file.name) {
            return Ok(p);
        }
        let url = http.url(&format!(
            "/v1/files/{}/content",
            crate::http::enc(&file.file_id)
        ));
        let resp = http
            .send_checked(http.authed(Method::GET, &url)?.timeout(TRANSFER_TIMEOUT))
            .await?;
        self.store_response(resp, &file.sha256, file.size, &file.name, progress)
            .await?;
        self.get_named(&file.sha256, &file.name)
            .ok_or_else(|| ApiError::Io("cached file is gone".into()))
    }

    /// Adds a local file (for example one I just uploaded) to the cache by copying it.
    pub async fn insert_copy(&self, src: &Path, sha256: &str, name: &str) -> ApiResult<PathBuf> {
        if !is_sha256(sha256) {
            return Err(ApiError::Invalid("bad sha256".into()));
        }
        if let Some(p) = self.get(sha256) {
            return Ok(p);
        }
        let dir = self.dir.join(sha256);
        tokio::fs::create_dir_all(&dir).await?;
        let part = dir.join(format!(".part-{}", crate::new_id()));
        let size = tokio::fs::copy(src, &part).await?;
        self.finish(part, sha256, size, name).await
    }

    async fn store_response(
        &self,
        resp: reqwest::Response,
        sha256: &str,
        size: u64,
        name: &str,
        progress: &ProgressFn,
    ) -> ApiResult<PathBuf> {
        let dir = self.dir.join(sha256);
        tokio::fs::create_dir_all(&dir).await?;
        let part = dir.join(format!(".part-{}", crate::new_id()));
        // Removes the partial file when the download fails or is cancelled
        // (the task aborted, so this future is dropped mid-stream).
        let guard = PartFile(Some(part.clone()));
        stream_to_file(resp, &part, size, Some(sha256), progress).await?;
        guard.keep();
        self.finish(part, sha256, size, name).await
    }

    async fn finish(
        &self,
        part: PathBuf,
        sha256: &str,
        size: u64,
        name: &str,
    ) -> ApiResult<PathBuf> {
        let dir = self.dir.join(sha256);
        let dest = dir.join(safe_file_name(name));
        // Downloaded content: Gatekeeper still checks it when it opens (an app, a script, a
        // Terminal settings file), as for a file from a browser.
        quarantine(&part);
        // Same content under another name: keep the first file; `get_named` links the name.
        let existing = first_complete_file(&dir).await;
        let dest = match existing {
            Some(p) => {
                let _ = tokio::fs::remove_file(&part).await;
                p
            }
            None => {
                tokio::fs::rename(&part, &dest).await?;
                dest
            }
        };
        self.store
            .file_cache_put(sha256, &dest.to_string_lossy(), size)
            .map_err(|e| ApiError::Io(e.to_string()))?;
        self.trim(sha256).await;
        Ok(dest)
    }

    /// Removes least recently used files until the cache fits `max_bytes`.
    async fn trim(&self, keep: &str) {
        let Ok(removed) = self.store.file_cache_evict(self.max_bytes, keep) else {
            return;
        };
        for (sha, _) in removed {
            if is_sha256(&sha) {
                let _ = tokio::fs::remove_dir_all(self.dir.join(&sha)).await;
            }
        }
    }
}

/// Sets `com.apple.quarantine` on a downloaded file, so macOS treats it like a file from a
/// browser. Errors are ignored (the file system may not support extended attributes).
fn quarantine(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(p) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return;
        };
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let value = format!("0081;{secs:08x};;");
        // SAFETY: both pointers are valid NUL-terminated strings or a buffer of `len` bytes
        // for the duration of the call.
        unsafe {
            libc::setxattr(
                p.as_ptr(),
                c"com.apple.quarantine".as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

/// A partial download that is removed on drop unless kept.
struct PartFile(Option<PathBuf>);

impl PartFile {
    fn keep(mut self) {
        self.0 = None;
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

async fn first_complete_file(dir: &Path) -> Option<PathBuf> {
    let mut rd = tokio::fs::read_dir(dir).await.ok()?;
    while let Ok(Some(e)) = rd.next_entry().await {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if !n.starts_with(".part-") && e.file_type().await.ok()?.is_file() {
            return Some(e.path());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::PartFile;

    #[test]
    fn a_dropped_partial_download_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let (gone, kept) = (dir.path().join(".part-a"), dir.path().join(".part-b"));
        std::fs::write(&gone, b"half").unwrap();
        std::fs::write(&kept, b"all").unwrap();
        drop(PartFile(Some(gone.clone())));
        PartFile(Some(kept.clone())).keep();
        assert!(!gone.exists());
        assert!(kept.exists());
    }
}
