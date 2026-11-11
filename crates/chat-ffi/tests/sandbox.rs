//! The bridge against a real local server (the sandbox of `scripts/sandbox/up.ts`). Ignored by
//! default; run with
//!   TC_SANDBOX_DIR=.tmp/sandbox cargo test -p chat-ffi --test sandbox -- --ignored
//! It reads the server address and the owner's token from that folder and prints neither.

use std::time::{Duration, Instant};

use chat_ffi::ChatClient;

fn sandbox() -> Option<(String, String)> {
    let dir = std::path::PathBuf::from(std::env::var("TC_SANDBOX_DIR").ok()?);
    let read = |f: &str| {
        std::fs::read_to_string(dir.join(f))
            .ok()
            .map(|s| s.trim().to_string())
    };
    Some((read("server-url")?, read("owner-token")?))
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "needs a running sandbox"]
fn sends_an_image_and_a_text_and_both_arrive() {
    let Some((url, token)) = sandbox() else {
        panic!("set TC_SANDBOX_DIR to the sandbox folder");
    };
    let dir = tempfile::tempdir().unwrap();
    let client = ChatClient::new(url, token, dir.path().to_string_lossy().into()).unwrap();
    client.start();
    until("the groups", || {
        client.groups().map(|g| !g.is_empty()).unwrap_or(false)
    });
    let group = client
        .groups()
        .unwrap()
        .into_iter()
        .find(|g| !g.is_dm)
        .unwrap();
    let topic = group.main_topic_id.clone();

    // A tiny PNG made here.
    let png = dir.path().join("pixel.png");
    std::fs::write(
        &png,
        [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xDE, 0, 0, 0, 0x0C, 0x49,
            0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0, 0, 0x03, 0x01, 0x01, 0, 0x18,
            0xDD, 0x8D, 0xB0, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ],
    )
    .unwrap();
    let caption = format!("bridge test {}", std::process::id());
    client
        .send_file(
            group.id.clone(),
            topic.clone(),
            png.to_string_lossy().into(),
            None,
            caption.clone(),
            None,
        )
        .unwrap();
    until("the image message", || {
        client.messages(topic.clone(), 30).unwrap().iter().any(|m| {
            m.body == caption && m.files.iter().any(|f| f.is_image && f.name == "pixel.png")
        })
    });
    let m = client
        .messages(topic.clone(), 30)
        .unwrap()
        .into_iter()
        .find(|m| m.body == caption)
        .unwrap();
    assert!(m.is_mine);
    // The file comes back from the server and matches.
    let path = client.file_path(m.files[0].ref_json.clone()).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), std::fs::read(&png).unwrap());
    client.stop();
}
