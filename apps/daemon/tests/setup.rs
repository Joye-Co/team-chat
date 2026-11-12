//! Protocol fixtures, config, launchd and systemd services, run folders and cleanup.
//!
//! The launchd and systemd cases are Unix only (the Windows scheduled task is tested for real
//! by the release workflow's install and update test, `scripts/test-windows-update.sh`); the
//! rest runs on every system.

use daemon::config::Config;
use daemon::proto::{Down, RepoSpec, Up};
use serde_json::Value;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn fixture_lines(name: &str) -> Vec<Value> {
    let p = format!(
        "{}/tests/fixtures/machine/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(p)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn machine_protocol_fixtures_round_trip() {
    for v in fixture_lines("up.jsonl") {
        let f: Up = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
        assert_eq!(serde_json::to_value(&f).unwrap(), v);
    }
    for v in fixture_lines("down.jsonl") {
        let f: Down = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
        assert_eq!(serde_json::to_value(&f).unwrap(), v);
    }
    // Unknown fields are ignored on read.
    let f: Down = serde_json::from_str(r#"{"t":"ack","runId":"r","seq":3,"future":1}"#).unwrap();
    assert_eq!(
        f,
        Down::Ack {
            run_id: "r".into(),
            seq: 3
        }
    );
}

/// A `pty` runtime whose program every system has.
fn shell_runtime() -> &'static str {
    if cfg!(windows) {
        "[runtimes.sh]\nkind = \"pty\"\ncommand = \"cmd.exe\"\n"
    } else {
        "[runtimes.sh]\nkind = \"pty\"\ncommand = \"/bin/sh\"\n"
    }
}

fn base_config(dir: &std::path::Path) -> String {
    let token = dir.join("token");
    std::fs::write(&token, "tm_x").unwrap();
    // Only this user may read it: mode 600, on Windows an access list of its own.
    daemon::sys::set_mode(&token, 0o600).unwrap();
    format!(
        "server_url = \"https://x.example\"\ntoken_file = '{}'\nwork_root = '{}'\n",
        token.display(),
        dir.join("work").display()
    )
}

#[test]
fn config_defaults_and_problems() {
    let dir = tempfile::tempdir().unwrap();
    let mut text = base_config(dir.path());
    text.push_str(shell_runtime());
    let c = Config::parse(&text).unwrap();
    assert_eq!(c.max_parallel_runs, 2);
    assert_eq!(c.keep_runs_hours, 72);
    assert_eq!(c.ws_url(), "wss://x.example/v1/machine/ws");
    assert!(c.problems().is_empty(), "{:?}", c.problems());

    // A token that other users of the machine can read stops the daemon.
    #[cfg(unix)]
    {
        std::fs::set_permissions(
            dir.path().join("token"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(c.problems().iter().any(|p| p.contains("mode 600")));
    }
    #[cfg(windows)]
    {
        let opened = Command::new("icacls")
            .arg(dir.path().join("token"))
            .args(["/grant", "*S-1-1-0:R"])
            .output()
            .unwrap();
        assert!(opened.status.success(), "{opened:?}");
        assert!(
            c.problems()
                .iter()
                .any(|p| p.contains("private to this user")),
            "{:?}",
            c.problems()
        );
    }
    let bad = text.replace("https://x.example", "http://x.example");
    assert!(
        Config::parse(&bad)
            .unwrap()
            .problems()
            .iter()
            .any(|p| p.contains("https"))
    );
    assert!(Config::parse("server_url = 1").is_err());
    assert!(Config::parse(&format!("{text}\nunknown_key = 1\n")).is_err());
    // The example parses.
    assert!(Config::parse(daemon::config::EXAMPLE).is_ok());
}

#[test]
fn doctor_reports_without_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let mut text = base_config(dir.path());
    text.push_str(shell_runtime());
    text.push_str("[runtimes.gone]\nkind = \"acp\"\ncommand = \"/nonexistent/agent\"\n");
    let c = Config::parse(&text).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (report, ok) = rt.block_on(daemon::doctor::report(&c));
    assert!(!ok);
    assert!(report.contains("NOT FOUND"));
    assert!(report.contains("disk free"));
    assert!(!report.contains("tm_x"), "doctor must not print the token");
}

#[cfg(unix)]
#[test]
fn launchd_plist_install_and_uninstall() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("daemon.toml");
    std::fs::write(&cfg, "").unwrap();
    let agents = dir.path().join("LaunchAgents");
    let file = daemon::service::install(&agents, &cfg, &dir.path().join("logs"), false).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        file.file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".daemon.plist")
    );
    assert!(text.contains("<string>run</string>"));
    assert!(
        text.contains(
            &std::fs::canonicalize(&cfg)
                .unwrap()
                .to_string_lossy()
                .to_string()
        )
    );
    assert!(text.contains("<key>KeepAlive</key>"));
    assert!(text.contains("<key>PATH</key>"));
    check_plist(&text, &std::fs::canonicalize(&cfg).unwrap());
    daemon::service::uninstall(&agents, false).unwrap();
    assert!(!file.exists());
}

#[cfg(unix)]
#[test]
fn systemd_unit_install_and_uninstall() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("daemon.toml");
    std::fs::write(&cfg, "").unwrap();
    let units = dir.path().join("systemd/user");
    let file =
        daemon::service::install_systemd(&units, &cfg, &dir.path().join("logs"), false).unwrap();
    assert!(
        file.file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-daemon.service")
    );
    let text = std::fs::read_to_string(&file).unwrap();
    let cfg = std::fs::canonicalize(&cfg).unwrap();
    let exec = text
        .lines()
        .find(|l| l.starts_with("ExecStart="))
        .expect("ExecStart line");
    assert!(exec.ends_with(&format!(" run --config \"{}\"", cfg.display())));
    assert!(text.contains("\nEnvironment=\"PATH="));
    assert!(text.contains("\nRestart=always\n"));
    assert!(text.contains("\nWantedBy=default.target\n"));
    assert!(text.contains(&format!(
        "StandardOutput=append:{}",
        dir.path().join("logs/daemon.log").display()
    )));
    daemon::service::uninstall_systemd(&units, false).unwrap();
    assert!(!file.exists());
}

/// Parses the LaunchAgent plist as XML on every platform and checks its structure: a
/// `plist` 1.0 root with one `dict` of key/value pairs, the program arguments and the keys
/// launchd needs.
#[cfg(unix)]
fn elems<'a, 'i>(n: roxmltree::Node<'a, 'i>) -> Vec<roxmltree::Node<'a, 'i>> {
    n.children().filter(|c| c.is_element()).collect()
}

#[cfg(unix)]
fn check_plist(text: &str, config: &std::path::Path) {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc =
        roxmltree::Document::parse_with_options(text, opts).expect("plist is well-formed XML");
    let root = doc.root_element();
    assert_eq!(root.tag_name().name(), "plist");
    assert_eq!(root.attribute("version"), Some("1.0"));
    let top = elems(root);
    assert_eq!(top.len(), 1, "one top-level dict");
    let dict = elems(top[0]);
    assert_eq!(top[0].tag_name().name(), "dict");
    assert_eq!(dict.len() % 2, 0, "dict holds key/value pairs");
    let mut map = std::collections::BTreeMap::new();
    for pair in dict.chunks(2) {
        assert_eq!(pair[0].tag_name().name(), "key");
        let key = pair[0].text().unwrap_or_default().to_string();
        assert!(
            map.insert(key.clone(), pair[1]).is_none(),
            "duplicate key {key}"
        );
    }
    for key in [
        "Label",
        "ProgramArguments",
        "EnvironmentVariables",
        "RunAtLoad",
        "KeepAlive",
        "StandardOutPath",
        "StandardErrorPath",
    ] {
        assert!(map.contains_key(key), "missing {key}");
    }
    assert_eq!(map["KeepAlive"].tag_name().name(), "true");
    assert_eq!(map["RunAtLoad"].tag_name().name(), "true");
    assert!(!map["Label"].text().unwrap_or_default().is_empty());
    let args: Vec<String> = elems(map["ProgramArguments"])
        .iter()
        .map(|a| {
            assert_eq!(a.tag_name().name(), "string");
            a.text().unwrap_or_default().to_string()
        })
        .collect();
    assert_eq!(args.len(), 4);
    assert_eq!(&args[1..3], ["run", "--config"]);
    assert_eq!(args[3], config.to_string_lossy());
    let env = elems(map["EnvironmentVariables"]);
    assert_eq!(env.first().and_then(|k| k.text()), Some("PATH"));
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let s = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        s.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&s.stderr)
    );
}

#[tokio::test]
async fn worktree_scratch_and_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(
        &repo,
        &[
            "-c",
            "user.email=a@b",
            "-c",
            "user.name=a",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
    let mut text = base_config(dir.path());
    text.push_str(&format!(
        "repo_roots = ['{}']\nkeep_runs_hours = 0\n{}",
        dir.path().display(),
        shell_runtime()
    ));
    let cfg = Config::parse(&text).unwrap();

    let spec = RepoSpec {
        url: None,
        path: Some(repo.to_string_lossy().into()),
        base_branch: Some("main".into()),
    };
    daemon::workspace::validate_repo(&cfg, &spec).unwrap();
    let outside = RepoSpec {
        url: None,
        path: Some(if cfg!(windows) { r"C:\Windows" } else { "/tmp" }.into()),
        base_branch: None,
    };
    assert!(daemon::workspace::validate_repo(&cfg, &outside).is_err());
    let evil = RepoSpec {
        url: Some("--upload-pack=x".into()),
        path: None,
        base_branch: None,
    };
    assert!(daemon::workspace::validate_repo(&cfg, &evil).is_err());
    let bad_branch = RepoSpec {
        url: None,
        path: Some(repo.to_string_lossy().into()),
        base_branch: Some("-x".into()),
    };
    assert!(daemon::workspace::validate_repo(&cfg, &bad_branch).is_err());

    let (work, meta) = daemon::workspace::prepare(&cfg, "rw1", Some(&spec))
        .await
        .unwrap();
    assert!(work.join(".git").exists(), "worktree created");
    assert_eq!(meta.branch.as_deref(), Some("bot/rw1"));
    let (scratch, meta2) = daemon::workspace::prepare(&cfg, "rs1", None).await.unwrap();
    assert!(scratch.is_dir() && meta2.git_dir.is_none());

    // An active run is kept; finished ones past keep_runs_hours go, with their worktree.
    let removed = daemon::workspace::cleanup(&cfg, &["rs1".to_string()]).await;
    assert_eq!(removed, 1);
    assert!(!work.exists());
    assert!(scratch.exists());
    let list = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["worktree", "list"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&list.stdout).lines().count(),
        1,
        "worktree pruned"
    );
    let br = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["branch", "--list", "bot/*"])
        .output()
        .unwrap();
    assert!(br.stdout.is_empty(), "run branch deleted");
}

fn git_out(dir: &std::path::Path, args: &[&str]) -> String {
    let s = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        s.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&s.stderr)
    );
    String::from_utf8_lossy(&s.stdout).trim().to_string()
}

fn commit(dir: &std::path::Path, msg: &str) {
    git(
        dir,
        &[
            "-c",
            "user.email=a@b",
            "-c",
            "user.name=a",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            msg,
        ],
    );
}

/// Two runs on one `repo.url` share a bare mirror. The second run's fetch must not delete or
/// refuse to update the first run's branch, and cleanup keeps a branch with unpushed commits.
#[tokio::test]
async fn mirror_runs_keep_their_branches() {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "-b", "main"]);
    commit(&origin, "init");
    let mut text = base_config(dir.path());
    text.push_str("keep_runs_hours = 0\n");
    text.push_str(shell_runtime());
    let cfg = Config::parse(&text).unwrap();
    let url = origin.to_string_lossy().to_string();

    // Run A starts from the default branch, commits, pushes, then commits again: the remote
    // bot/ra and the local bot/ra now differ.
    let a_spec = RepoSpec {
        url: Some(url.clone()),
        path: None,
        base_branch: None,
    };
    let (work_a, meta_a) = daemon::workspace::prepare(&cfg, "ra", Some(&a_spec))
        .await
        .unwrap();
    commit(&work_a, "a1");
    git(&work_a, &["push", "-q", "origin", "bot/ra"]);
    commit(&work_a, "a2");
    let a_head = git_out(&work_a, &["rev-parse", "HEAD"]);

    // Upstream moves on; run B (same url) must start from the new tip.
    commit(&origin, "upstream");
    let upstream = git_out(&origin, &["rev-parse", "main"]);
    let b_spec = RepoSpec {
        url: Some(url.clone()),
        path: None,
        base_branch: Some("main".into()),
    };
    let (work_b, _) = daemon::workspace::prepare(&cfg, "rb", Some(&b_spec))
        .await
        .expect("second run on the same repo.url prepares");
    assert_eq!(git_out(&work_b, &["rev-parse", "HEAD"]), upstream);
    // Run A's branch and worktree are intact.
    assert_eq!(git_out(&work_a, &["rev-parse", "HEAD"]), a_head);
    let mirror = meta_a.git_dir.clone().unwrap();
    assert_eq!(
        git_out(&mirror, &["rev-parse", "refs/heads/bot/ra"]),
        a_head
    );
    // A third run with no base branch also starts from the fetched default branch.
    let (work_c, _) = daemon::workspace::prepare(&cfg, "rc", Some(&a_spec))
        .await
        .unwrap();
    assert_eq!(git_out(&work_c, &["rev-parse", "HEAD"]), upstream);

    // Cleanup (keep_runs_hours = 0): A has an unpushed commit, so its branch stays; C has none.
    let removed = daemon::workspace::cleanup(&cfg, &["rb".to_string()]).await;
    assert_eq!(removed, 2);
    assert!(!work_a.exists() && !work_c.exists() && work_b.exists());
    assert_eq!(
        git_out(&mirror, &["rev-parse", "refs/heads/bot/ra"]),
        a_head
    );
    assert_eq!(
        git_out(&mirror, &["branch", "--list", "bot/rc"]),
        "",
        "branch without own commits deleted"
    );
}

/// A daemon crash leaves `work/.chat-run-<run id>` (the Codex request folder) behind: the next
/// start removes the transport's files and the folder of every run whose CLI is not running,
/// and keeps other files, other folders and the folders of active runs.
#[test]
fn crash_leftover_request_folders_are_removed_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config::parse(&base_config(dir.path())).unwrap();
    let runs = daemon::workspace::runs_dir(&cfg);
    let spool = |run: &str| {
        let s = daemon::chat_run::spool_dir(&runs.join(run).join("work"), run);
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join(".gitignore"), "*\n").unwrap();
        std::fs::write(s.join("a1-1.req"), "{}").unwrap();
        std::fs::write(s.join("a1-1.res.tmp"), "{}").unwrap();
        s
    };
    let left = spool("dead1");
    let kept_files = spool("dead2");
    std::fs::write(kept_files.join("notes.md"), "not ours").unwrap();
    let live = spool("live1");
    // A folder of the repository with the same prefix but another run id stays.
    let repo_dir = runs.join("dead1/work/.chat-run-other");
    std::fs::create_dir_all(&repo_dir).unwrap();

    let removed = daemon::workspace::sweep_spools(&cfg, &["live1".to_string()]);
    assert_eq!(removed, 1);
    assert!(!left.exists());
    assert_eq!(
        std::fs::read_dir(&kept_files)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>(),
        ["notes.md"]
    );
    assert!(live.join("a1-1.req").exists());
    assert!(repo_dir.exists());
}

/// A side chat run's checkout is read-only while it runs; a crash that leaves it so does not
/// stop cleanup from removing the run.
#[tokio::test]
async fn read_only_checkout_and_its_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let mut text = base_config(dir.path());
    text.push_str("keep_runs_hours = 0\n");
    let cfg = Config::parse(&text).unwrap();
    let (work, _) = daemon::workspace::prepare(&cfg, "ra1", None).await.unwrap();
    std::fs::create_dir_all(work.join("src")).unwrap();
    std::fs::write(work.join("src/a.rs"), "x").unwrap();
    daemon::workspace::set_read_only(&work, true);
    for p in [work.join("new.txt"), work.join("src/b.rs")] {
        let e = std::fs::write(&p, "y").unwrap_err();
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::PermissionDenied,
            "{}",
            p.display()
        );
    }
    assert!(std::fs::write(work.join("src/a.rs"), "y").is_err());
    daemon::workspace::set_read_only(&work, false);
    std::fs::write(work.join("src/a.rs"), "y").unwrap();
    daemon::workspace::set_read_only(&work, true);
    // Left read-only (a crash): cleanup still removes the run.
    assert_eq!(daemon::workspace::cleanup(&cfg, &[]).await, 1);
    assert!(!work.exists());
}
