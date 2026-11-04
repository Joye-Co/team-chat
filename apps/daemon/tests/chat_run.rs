//! The daemon binary started as `chat-run` (argv0 dispatch).

use std::process::Command;

fn linked(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_daemon"), &p).unwrap();
    p
}

#[test]
fn the_daemon_binary_named_chat_run_is_the_client() {
    let tmp = tempfile::tempdir().unwrap();
    let chat_run = linked(tmp.path(), "chat-run");

    let help = Command::new(&chat_run).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("chat-run card patch"));

    let out = Command::new(&chat_run)
        .args(["resolve", "--event", "E", "--result", "ignored"])
        .env_remove("CHAT_RUN_SOCKET")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("CHAT_RUN_SOCKET")
    );

    let bad = Command::new(&chat_run)
        .args(["resolve", "--event", "E", "--result", "patched"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));

    // Under its own name it is still the daemon.
    let daemon = Command::new(linked(tmp.path(), "daemon"))
        .arg("resolve")
        .output()
        .unwrap();
    assert_eq!(daemon.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&daemon.stderr).contains("usage: daemon"));
}
