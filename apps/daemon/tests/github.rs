//! Feature "github" end to end on this machine: git's credential helper and the `gh` proxy
//! (the daemon binary started as `chat-run git-credential` and as `gh`) ask the daemon, the
//! daemon asks a fake server (`GET /v1/machine/runs/:id/github`) on every use.

use chat_protocol::github::{GitIdentity, RunGithub};
use daemon::chat_run::{Outputs, Server, Spool, TIMEOUT, socket_path, spool_dir};
use daemon::proto::RunEvent;
use daemon::redact::Redactor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

const TOKEN: &str = "fake-person-token-0123456789abcdef";

/// What the fake server answers for the token and what it was asked.
#[derive(Default)]
struct Fake {
    status: Mutex<u16>,
    seen: Mutex<Vec<String>>,
}

impl Fake {
    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

/// A server with the three machine routes of feature "github".
async fn fake_server(status: u16) -> (String, Arc<Fake>) {
    let fake = Arc::new(Fake {
        status: Mutex::new(status),
        seen: Mutex::default(),
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let f = fake.clone();
    tokio::spawn(async move {
        loop {
            let Ok((s, _)) = l.accept().await else { return };
            let f = f.clone();
            tokio::spawn(async move {
                let (head, body, _) = read_request(s).await;
                let (mut s, line, headers) = head;
                let auth = headers
                    .iter()
                    .find(|(k, _)| k == "authorization")
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                f.seen
                    .lock()
                    .unwrap()
                    .push(format!("{line} {auth} {}", String::from_utf8_lossy(&body)));
                let status = *f.status.lock().unwrap();
                let (code, json) = if line.starts_with("GET ") {
                    match status {
                        200 => ("200 OK", format!(r#"{{"token":"{TOKEN}","login":"octo"}}"#)),
                        404 => (
                            "404 Not Found",
                            r#"{"error":{"code":"not_found","reason":"github.not_connected"}}"#
                                .into(),
                        ),
                        _ => (
                            "409 Conflict",
                            r#"{"error":{"code":"conflict","reason":"github.invalid"}}"#.into(),
                        ),
                    }
                } else {
                    ("200 OK", r#"{"ok":true}"#.into())
                };
                let _ = s
                    .write_all(
                        format!(
                            "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                            json.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    (url, fake)
}

type Head = (tokio::net::TcpStream, String, Vec<(String, String)>);

/// One HTTP/1.1 request: the request line, lower-case headers and the body (a length or
/// chunked). Returns the stream for the answer.
async fn read_request(s: tokio::net::TcpStream) -> (Head, Vec<u8>, ()) {
    let mut r = tokio::io::BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).await.unwrap();
    let mut headers = vec![];
    loop {
        let mut h = String::new();
        r.read_line(&mut h).await.unwrap();
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let get = |k: &str| {
        headers
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v): &(String, String)| v.clone())
    };
    let mut body = vec![];
    if let Some(n) = get("content-length").and_then(|v| v.parse::<usize>().ok()) {
        body.resize(n, 0);
        r.read_exact(&mut body).await.unwrap();
    } else if get("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        loop {
            let mut size = String::new();
            r.read_line(&mut size).await.unwrap();
            let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
            let mut chunk = vec![0; n + 2];
            r.read_exact(&mut chunk).await.unwrap();
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
    }
    (
        (r.into_inner(), line.trim_end().to_string(), headers),
        body,
        (),
    )
}

fn spec(rewrite: bool) -> RunGithub {
    RunGithub {
        source: "app".into(),
        member_id: "U1".into(),
        login: Some("octo".into()),
        author: Some(GitIdentity {
            name: "Ann Person".into(),
            email: "1+octo@users.noreply.github.com".into(),
        }),
        committer: GitIdentity {
            name: "Robo".into(),
            email: "ai-B1@users.noreply.invalid".into(),
        },
        rewrite_ssh: rewrite.then_some(true),
        asked_by: None,
    }
}

fn linked(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    #[cfg(unix)]
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_daemon"), &p).unwrap();
    #[cfg(not(unix))]
    if std::fs::hard_link(env!("CARGO_BIN_EXE_daemon"), &p).is_err() {
        std::fs::copy(env!("CARGO_BIN_EXE_daemon"), &p).unwrap();
    }
    p
}

/// A run's daemon side: its outputs with the GitHub of `spec`, its socket, its events and
/// redactor.
struct RunSide {
    outputs: Arc<Outputs>,
    events: Arc<Mutex<Vec<RunEvent>>>,
    redactor: Arc<Redactor>,
}

fn run_side(url: &str, host: &str, spec: RunGithub) -> RunSide {
    let redactor = Arc::new(Redactor::new(vec![]).for_run());
    let outputs = Arc::new(Outputs::new("r1"));
    outputs.set_github(Arc::new(
        daemon::github::Run::new(
            url,
            Arc::new("tm_machine_token_for_tests_only".into()),
            "r1",
            spec,
            redactor.clone(),
        )
        .with_host(host),
    ));
    RunSide {
        outputs,
        events: Arc::default(),
        redactor,
    }
}

impl RunSide {
    fn emit(&self) -> impl Fn(RunEvent) + Send + Sync + 'static {
        let seen = self.events.clone();
        move |e| seen.lock().unwrap().push(e)
    }
}

/// The run's environment for git: the run's `bin` first on PATH, no config of this machine
/// but `system` (a file the test writes).
fn git_env(
    bin: &Path,
    system: &Path,
    spec: &RunGithub,
    base: &str,
    transport: (&str, PathBuf),
) -> Vec<(String, String)> {
    let mut env = vec![];
    daemon::github::put_env_at(spec, &mut env, base);
    let path = std::env::join_paths(std::iter::once(bin.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    env.push(("PATH".into(), path.to_string_lossy().into()));
    env.push(("GIT_CONFIG_SYSTEM".into(), system.display().to_string()));
    env.push((
        "GIT_CONFIG_GLOBAL".into(),
        system.with_extension("global").display().to_string(),
    ));
    env.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
    env.push((transport.0.into(), transport.1.display().to_string()));
    env
}

fn git(
    env: &[(String, String)],
    dir: Option<&Path>,
    args: &[&str],
    stdin: &str,
) -> std::process::Output {
    use std::io::Write;
    let mut c = Command::new("git");
    if let Some(d) = dir {
        c.arg("-C").arg(d);
    }
    c.args(args)
        .env_remove("CHAT_RUN_SOCKET")
        .env_remove("CHAT_RUN_DIR")
        .env_remove("SSH_AUTH_SOCK")
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = c.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// A system helper that would sign in as somebody else (Git for Windows' `manager` stands for
/// it): the run's empty helper clears it for github.com.
fn system_config(dir: &Path) -> PathBuf {
    let p = dir.join("system.gitconfig");
    std::fs::write(
        &p,
        "[credential]\n\thelper = \"!f() { echo username=machine; echo password=machine-owner; }; f\"\n",
    )
    .unwrap();
    std::fs::write(p.with_extension("global"), "").unwrap();
    p
}

/// `git credential fill` gets the person's token from the daemon, which asked the server with
/// the machine token, over the socket and over the request folder (Codex); the machine's own
/// helper is not used, no `.res` file stays and the run's redactor has the token.
#[tokio::test(flavor = "multi_thread")]
async fn git_gets_the_persons_token_through_the_daemon() {
    let (url, fake) = fake_server(200).await;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    linked(&bin, "chat-run");
    let system = system_config(tmp.path());
    let side = run_side(&url, "github.com", spec(false));
    let fill = "protocol=https\nhost=github.com\n\n";

    let server = Server::start(
        socket_path(tmp.path()),
        side.outputs.clone(),
        side.emit(),
        TIMEOUT,
    )
    .unwrap();
    let env = git_env(
        &bin,
        &system,
        &spec(false),
        daemon::github::BASE,
        ("CHAT_RUN_SOCKET", server.path().to_path_buf()),
    );
    let out = tokio::task::spawn_blocking(move || git(&env, None, &["credential", "fill"], fill))
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("username=x-access-token"), "{text}");
    assert!(text.contains(&format!("password={TOKEN}")), "{text}");
    assert!(!text.contains("machine-owner"), "{text}");
    drop(server);

    // The request folder (Codex): the answer file is gone after the call.
    let work = tmp.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let spool = Spool::start(
        spool_dir(&work, "r1"),
        side.outputs.clone(),
        side.emit(),
        TIMEOUT,
    )
    .unwrap();
    let dir = spool.path().to_path_buf();
    let env = git_env(
        &bin,
        &system,
        &spec(false),
        daemon::github::BASE,
        ("CHAT_RUN_DIR", dir.clone()),
    );
    let out = tokio::task::spawn_blocking(move || git(&env, None, &["credential", "fill"], fill))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains(TOKEN));
    let left: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".res") || n.ends_with(".req"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    drop(spool);

    // Asked each time, never cached, with the machine token, for this run.
    let seen = fake.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    for s in &seen {
        assert!(
            s.starts_with("GET /v1/machine/runs/r1/github HTTP/1.1 Bearer tm_machine_token"),
            "{s}"
        );
    }
    // One `used` event, never the token; the run masks the token from now on.
    let events = side.events.lock().unwrap().clone();
    assert_eq!(
        events,
        vec![RunEvent::Github {
            state: "used".into(),
            login: Some("octo".into()),
            reason: None
        }]
    );
    assert_eq!(side.redactor.text(&format!("x {TOKEN}")), "x <redacted>");
}

/// Not connected: the helper fails at once with the line the CLI must act on, the server is
/// told once (`/wait`), and the run reports `missing`.
#[tokio::test(flavor = "multi_thread")]
async fn without_a_connection_git_fails_now_and_the_server_is_told_once() {
    let (url, fake) = fake_server(404).await;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    linked(&bin, "chat-run");
    let system = system_config(tmp.path());
    let side = run_side(&url, "github.com", spec(false));
    let server = Server::start(
        socket_path(tmp.path()),
        side.outputs.clone(),
        side.emit(),
        TIMEOUT,
    )
    .unwrap();
    let env = git_env(
        &bin,
        &system,
        &spec(false),
        daemon::github::BASE,
        ("CHAT_RUN_SOCKET", server.path().to_path_buf()),
    );
    for _ in 0..2 {
        let env = env.clone();
        let started = std::time::Instant::now();
        let out = tokio::task::spawn_blocking(move || {
            git(
                &env,
                None,
                &["credential", "fill"],
                "protocol=https\nhost=github.com\n\n",
            )
        })
        .await
        .unwrap();
        assert!(!out.status.success());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("GitHub is not connected for Ann Person (they have been asked)"),
            "{err}"
        );
        assert!(!String::from_utf8_lossy(&out.stdout).contains("machine-owner"));
    }
    let seen = fake.seen();
    let waits: Vec<_> = seen.iter().filter(|s| s.contains("/github/wait")).collect();
    assert_eq!(waits.len(), 1, "{seen:?}");
    assert!(
        waits[0].ends_with(r#"{"reason":"not_connected"}"#),
        "{}",
        waits[0]
    );
    assert_eq!(
        side.events.lock().unwrap().clone(),
        vec![RunEvent::Github {
            state: "missing".into(),
            login: Some("octo".into()),
            reason: None
        }]
    );
}

/// `gh auth login` is stopped; `gh` (`gh.exe` on Windows) is the proxy by its name.
#[test]
fn gh_sign_in_is_stopped() {
    let tmp = tempfile::tempdir().unwrap();
    let gh = linked(tmp.path(), "gh");
    for sub in ["login", "logout", "refresh", "setup-git"] {
        let out = Command::new(&gh)
            .args(["auth", sub])
            .env("CHAT_GITHUB_AS", "Ann Person")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).trim(),
            "GitHub sign-in is handled by the app for Ann Person; do not sign in here."
        );
    }
}

/// The proxy finds the real gh after the run's tools and runs it with the person's token;
/// without a connection it fails and the server is told.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn gh_runs_the_real_gh_with_the_token() {
    use std::os::unix::fs::PermissionsExt;
    let (url, fake) = fake_server(200).await;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let sys = tmp.path().join("sys");
    std::fs::create_dir(&bin).unwrap();
    std::fs::create_dir(&sys).unwrap();
    linked(&bin, "chat-run");
    let proxy = linked(&bin, "gh");
    let real = sys.join("gh");
    std::fs::write(
        &real,
        "#!/bin/sh\necho \"args=$* token=$GH_TOKEN\"\nexit 3\n",
    )
    .unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();
    let side = run_side(&url, "github.com", spec(false));
    let server = Server::start(
        socket_path(tmp.path()),
        side.outputs.clone(),
        side.emit(),
        TIMEOUT,
    )
    .unwrap();
    let path = std::env::join_paths([bin.clone(), sys.clone()]).unwrap();
    let socket = server.path().to_path_buf();
    let run = move |with_person: bool| {
        let mut c = Command::new(&proxy);
        c.args(["pr", "create"])
            .env("PATH", &path)
            .env("CHAT_RUN_SOCKET", &socket)
            .env_remove("CHAT_RUN_DIR")
            .env_remove("GH_TOKEN");
        if with_person {
            c.env("CHAT_GITHUB_AS", "Ann Person");
        } else {
            c.env_remove("CHAT_GITHUB_AS");
        }
        c.output().unwrap()
    };
    let r = run.clone();
    let out = tokio::task::spawn_blocking(move || r(true)).await.unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("args=pr create token={TOKEN}")
    );
    // Machine mode (no person named): the real gh as it is, nothing asked.
    let r = run.clone();
    let out = tokio::task::spawn_blocking(move || r(false)).await.unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "args=pr create token="
    );
    assert_eq!(fake.seen().len(), 1);

    *fake.status.lock().unwrap() = 409;
    let out = tokio::task::spawn_blocking(move || run(true))
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "the real gh did not run");
    assert!(String::from_utf8_lossy(&out.stderr).contains("no longer works"));
    let seen = fake.seen();
    assert!(
        seen.iter()
            .any(|s| s.contains("/github/wait") && s.ends_with(r#"{"reason":"invalid"}"#)),
        "{seen:?}"
    );
}

/// A local GitHub stand-in: git's smart HTTP (`git http-backend`) behind Basic auth that takes
/// only `x-access-token:<TOKEN>`. Returns its base URL and the bare repositories' root.
#[cfg(unix)]
async fn git_host(root: PathBuf) -> String {
    use base64::Engine;
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let want = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{TOKEN}"))
    );
    tokio::spawn(async move {
        loop {
            let Ok((s, _)) = l.accept().await else { return };
            let root = root.clone();
            let want = want.clone();
            tokio::spawn(async move {
                let ((mut s, line, headers), body, _) = read_request(s).await;
                let get = |k: &str| {
                    headers
                        .iter()
                        .find(|(n, _)| n == k)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                };
                if get("authorization") != want {
                    let _ = s
                        .write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"git\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await;
                    return;
                }
                let mut parts = line.split(' ');
                let method = parts.next().unwrap_or_default().to_string();
                let target = parts.next().unwrap_or_default().to_string();
                let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                let mut c = tokio::process::Command::new("git");
                c.arg("http-backend")
                    .env("GIT_PROJECT_ROOT", &root)
                    .env("GIT_HTTP_EXPORT_ALL", "1")
                    .env("REQUEST_METHOD", &method)
                    .env("PATH_INFO", path)
                    .env("QUERY_STRING", query)
                    .env("CONTENT_TYPE", get("content-type"))
                    .env("CONTENT_LENGTH", body.len().to_string())
                    .env("REMOTE_USER", "x-access-token")
                    .env("REMOTE_ADDR", "127.0.0.1")
                    .env("GIT_PROTOCOL", get("git-protocol"))
                    .env("HTTP_CONTENT_ENCODING", get("content-encoding"))
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped());
                let mut child = c.spawn().unwrap();
                let mut stdin = child.stdin.take().unwrap();
                stdin.write_all(&body).await.unwrap();
                drop(stdin);
                let out = child.wait_with_output().await.unwrap();
                let raw = out.stdout;
                let split = raw
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|i| (i, 4))
                    .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2)))
                    .unwrap_or((raw.len(), 0));
                let head = String::from_utf8_lossy(&raw[..split.0]).to_string();
                let rest = &raw[(split.0 + split.1).min(raw.len())..];
                let mut status = "200 OK".to_string();
                let mut answer = String::new();
                for h in head.lines() {
                    if let Some(v) = h.strip_prefix("Status:") {
                        status = v.trim().to_string();
                    } else if !h.trim().is_empty() {
                        answer.push_str(h.trim_end());
                        answer.push_str("\r\n");
                    }
                }
                let _ = s
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\n{answer}Content-Length: {}\r\nConnection: close\r\n\r\n",
                            rest.len()
                        )
                        .as_bytes(),
                    )
                    .await;
                let _ = s.write_all(rest).await;
            });
        }
    });
    base
}

/// A push over HTTPS-style auth to a local smart-HTTP remote: git takes the token from the
/// helper, the commit says the person authored it and the AI member committed it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn git_pushes_as_the_person_with_the_ai_member_as_committer() {
    let (url, fake) = fake_server(200).await;
    let tmp = tempfile::tempdir().unwrap();
    let repos = tmp.path().join("repos");
    std::fs::create_dir(&repos).unwrap();
    let bare = repos.join("repo.git");
    assert!(
        Command::new("git")
            .args(["init", "--bare", "--quiet", "-b", "main"])
            .arg(&bare)
            .status()
            .unwrap()
            .success()
    );
    let base = git_host(repos.clone()).await;
    let host = base.trim_start_matches("http://").to_string();
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    linked(&bin, "chat-run");
    let system = system_config(tmp.path());
    let side = run_side(&url, &host, spec(false));
    let server = Server::start(
        socket_path(tmp.path()),
        side.outputs.clone(),
        side.emit(),
        TIMEOUT,
    )
    .unwrap();
    let env = git_env(
        &bin,
        &system,
        &spec(false),
        &base,
        ("CHAT_RUN_SOCKET", server.path().to_path_buf()),
    );
    let work = tmp.path().join("work");
    let remote = format!("{base}/repo.git");
    let out = tokio::task::spawn_blocking(move || {
        let ok = |o: std::process::Output| {
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            o
        };
        ok(git(
            &env,
            None,
            &["init", "--quiet", "-b", "main", &work.display().to_string()],
            "",
        ));
        std::fs::write(work.join("a.txt"), "hi\n").unwrap();
        ok(git(&env, Some(&work), &["add", "a.txt"], ""));
        ok(git(
            &env,
            Some(&work),
            &["commit", "--quiet", "-m", "first"],
            "",
        ));
        ok(git(
            &env,
            Some(&work),
            &["push", "--quiet", &remote, "main"],
            "",
        ));
        ok(git(
            &env,
            Some(&bare),
            &["log", "-1", "--format=%an <%ae>|%cn <%ce>", "main"],
            "",
        ))
    })
    .await
    .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "Ann Person <1+octo@users.noreply.github.com>|Robo <ai-B1@users.noreply.invalid>"
    );
    assert!(!fake.seen().is_empty());
}

/// The daemon's own clone of a private repository (before the CLI starts) goes with the
/// person's token as a header; without one it goes without any credentials and fails.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_daemons_own_clone_uses_the_persons_token() {
    let tmp = tempfile::tempdir().unwrap();
    let repos = tmp.path().join("repos");
    std::fs::create_dir(&repos).unwrap();
    let bare = repos.join("repo.git");
    let seed = tmp.path().join("seed");
    let sh = |args: &[&str]| {
        let o = Command::new("git")
            .args(args)
            .env("GIT_AUTHOR_NAME", "A")
            .env("GIT_AUTHOR_EMAIL", "a@example.invalid")
            .env("GIT_COMMITTER_NAME", "A")
            .env("GIT_COMMITTER_EMAIL", "a@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    let seed_s = seed.display().to_string();
    sh(&["init", "--quiet", "-b", "main", &seed_s]);
    std::fs::write(seed.join("a.txt"), "hi\n").unwrap();
    sh(&["-C", &seed_s, "add", "a.txt"]);
    sh(&["-C", &seed_s, "commit", "--quiet", "-m", "first"]);
    sh(&[
        "clone",
        "--bare",
        "--quiet",
        &seed_s,
        &bare.display().to_string(),
    ]);
    let base = git_host(repos).await;
    let cfg = daemon::config::Config::parse(&format!(
        "server_url = \"http://127.0.0.1:1\"\ntoken_file = '{}'\nwork_root = '{}'\n",
        tmp.path().join("token").display(),
        tmp.path().join("root").display()
    ))
    .unwrap();
    let repo = daemon::proto::RepoSpec {
        url: Some(format!("{base}/repo.git")),
        path: None,
        base_branch: None,
    };
    let without = daemon::workspace::with_git_env(
        daemon::github::fetch_env_at(&spec(false), None, &base),
        daemon::workspace::prepare(&cfg, "r1", Some(&repo)),
    )
    .await;
    assert!(without.is_err());
    let (work, _) = daemon::workspace::with_git_env(
        daemon::github::fetch_env_at(&spec(false), Some(TOKEN), &base),
        daemon::workspace::prepare(&cfg, "r2", Some(&repo)),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(work.join("a.txt")).unwrap(), "hi\n");
    // The token is in no file of the mirror.
    let mirror_config = std::fs::read_dir(tmp.path().join("root").join("repos"))
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path().join("config")).unwrap_or_default())
        .collect::<String>();
    assert!(!mirror_config.contains("Basic") && !mirror_config.contains(TOKEN));
}
