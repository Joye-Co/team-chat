//! The daemon binary started as `chat-run` (argv0 dispatch).

use std::process::Command;

fn linked(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let p = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    #[cfg(unix)]
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_daemon"), &p).unwrap();
    // Windows: a hard link, like the run's `bin\\chat-run.exe` (a copy across volumes).
    #[cfg(not(unix))]
    if std::fs::hard_link(env!("CARGO_BIN_EXE_daemon"), &p).is_err() {
        std::fs::copy(env!("CARGO_BIN_EXE_daemon"), &p).unwrap();
    }
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
        .env_remove("CHAT_RUN_DIR")
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

/// The client binary reaches the daemon over both transports of a run: the socket (a named
/// pipe on Windows) and the request folder. A gated secret is asked for, never just given.
#[tokio::test(flavor = "multi_thread")]
async fn the_client_asks_the_daemon_over_the_socket_and_the_request_folder() {
    use daemon::chat_run::{Outputs, Server, Spool, TIMEOUT, socket_path, spool_dir};
    use daemon::proto::RunEvent;
    use std::sync::{Arc, Mutex};

    let tmp = tempfile::tempdir().unwrap();
    let chat_run = linked(tmp.path(), "chat-run");
    let outputs = Arc::new(Outputs::new("r1"));
    outputs.set_gate(Arc::new(daemon::secrets::Gate::new(vec!["CF_PROD".into()])));
    let seen = Arc::new(Mutex::new(Vec::<RunEvent>::new()));
    let emit = |seen: &Arc<Mutex<Vec<RunEvent>>>| {
        let seen = seen.clone();
        move |e: RunEvent| seen.lock().unwrap().push(e)
    };
    let ask = |var: &'static str, value: std::path::PathBuf| {
        let chat_run = chat_run.clone();
        tokio::task::spawn_blocking(move || {
            Command::new(&chat_run)
                .args(["secret", "CF_PROD"])
                .env_remove("CHAT_RUN_SOCKET")
                .env_remove("CHAT_RUN_DIR")
                .env(var, value)
                .output()
                .unwrap()
        })
    };

    let server = Server::start(
        socket_path(tmp.path()),
        outputs.clone(),
        emit(&seen),
        TIMEOUT,
    )
    .unwrap();
    let out = ask("CHAT_RUN_SOCKET", server.path().to_path_buf())
        .await
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("pending"), "{err}");
    assert!(out.stdout.is_empty(), "no value without an approval");
    assert!(
        matches!(
            seen.lock().unwrap().as_slice(),
            [RunEvent::ApprovalRequest { .. }]
        ),
        "a person is asked"
    );
    drop(server);

    let work = tmp.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let spool = Spool::start(
        spool_dir(&work, "r1"),
        outputs.clone(),
        emit(&seen),
        TIMEOUT,
    )
    .unwrap();
    let out = ask("CHAT_RUN_DIR", spool.path().to_path_buf())
        .await
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("pending"), "{err}");
    drop(spool);
    assert!(!work.join(".chat-run-r1").exists());
}

/// `daemon doctor` on this system: it finds git and a program on PATH by name (on Windows
/// through PATHEXT), reads the disk, accepts a private token file and says OK.
#[test]
fn doctor_reports_this_machine() {
    let tmp = tempfile::tempdir().unwrap();
    let token = tmp.path().join("token");
    std::fs::write(&token, "tm_test\n").unwrap();
    daemon::sys::set_mode(&token, 0o600).unwrap();
    let config = tmp.path().join("daemon.toml");
    // TOML literal strings: a Windows path has backslashes.
    std::fs::write(
        &config,
        format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = '{}'\nwork_root = '{}'\n\
             [runtimes.shell]\nkind = \"pty\"\ncommand = \"{}\"\n",
            token.display(),
            tmp.path().join("runs").display(),
            if cfg!(windows) { "cmd" } else { "sh" },
        ),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .arg("doctor")
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{text}");
    assert!(text.contains("git:          found"), "{text}");
    assert!(text.contains("shell") && text.contains("found"), "{text}");
    assert!(!text.contains("disk free:    0.0 GiB of 0.0 GiB"), "{text}");
    assert!(text.trim_end().ends_with("OK"), "{text}");
}
