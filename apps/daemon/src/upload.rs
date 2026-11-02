//! Raw session upload: gzip + HTTP PUT.

use crate::proto::SessionUpload;
use crate::workspace::hex;
use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

pub const MAX_GZIP_BYTES: usize = 100 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Uploaded {
    pub bytes: u64,
    pub gzip_bytes: u64,
    pub sha256: String,
}

fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    Some(format!(
        "{}://{}",
        scheme.to_ascii_lowercase(),
        host.to_ascii_lowercase()
    ))
}

/// The machine token is sent only to the configured server's own origin.
pub fn same_origin(a: &str, b: &str) -> bool {
    matches!((origin(a), origin(b)), (Some(x), Some(y)) if x == y)
}

pub async fn upload(
    file: &Path,
    target: &SessionUpload,
    server_url: &str,
    token: &str,
) -> Result<Uploaded, String> {
    let raw = tokio::fs::read(file)
        .await
        .map_err(|e| format!("cannot read session file: {e}"))?;
    let sha = hex(&Sha256::digest(&raw));
    let raw_len = raw.len() as u64;
    let gz = tokio::task::spawn_blocking(move || {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw)?;
        enc.finish()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("gzip: {e}"))?;
    if gz.len() > MAX_GZIP_BYTES {
        return Err(format!("session is too large ({} bytes gzip)", gz.len()));
    }
    let gz_len = gz.len() as u64;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let mut last_err = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2u64 << (2 * attempt))).await;
        }
        let mut req = client
            .put(&target.url)
            .header("Content-Type", "application/gzip")
            .header("X-Session-Sha256", &sha)
            .header("X-Session-Bytes", raw_len.to_string());
        for (k, v) in &target.headers {
            if !k.eq_ignore_ascii_case("authorization") {
                req = req.header(k, v);
            }
        }
        if same_origin(&target.url, server_url) {
            req = req.bearer_auth(token);
        }
        match req.body(gz.clone()).send().await {
            Ok(r) if r.status().is_success() => {
                return Ok(Uploaded {
                    bytes: raw_len,
                    gzip_bytes: gz_len,
                    sha256: sha,
                });
            }
            Ok(r) if r.status().is_client_error() => {
                return Err(format!("upload rejected: HTTP {}", r.status().as_u16()));
            }
            Ok(r) => last_err = format!("upload failed: HTTP {}", r.status().as_u16()),
            Err(e) => last_err = format!("upload failed: {}", e.without_url()),
        }
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::same_origin;

    #[test]
    fn token_only_to_same_origin() {
        assert!(same_origin("https://a.example/v1/x", "https://A.example"));
        assert!(!same_origin(
            "https://a.example.evil/v1",
            "https://a.example"
        ));
        assert!(!same_origin("http://a.example/v1", "https://a.example"));
        assert!(same_origin("http://127.0.0.1:9/x", "http://127.0.0.1:9"));
        assert!(!same_origin("http://127.0.0.1:8/x", "http://127.0.0.1:9"));
    }
}
