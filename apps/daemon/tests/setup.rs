//! Protocol fixtures, config, launchd service, run folders and cleanup.

use daemon::config::Config;
use daemon::proto::{Down, RepoSpec, Up};
use serde_json::Value;
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

fn base_config(dir: &std::path::Path) -> String {
    let token = dir.join("token");
    std::fs::write(&token, "tm_x").unwrap();
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    format!(
        "server_url = \"https://x.example\"\ntoken_file = \"{}\"\nwork_root = \"{}\"\n",
        token.display(),
        dir.join("work").display()
    )
}

#[test]
fn config_defaults_and_problems() {
    let dir = tempfile::tempdir().unwrap();
    let mut text = base_config(dir.path());
    text.push_str("[runtimes.sh]\nkind = \"pty\"\ncommand = \"/bin/sh\"\n");
    let c = Config::parse(&text).unwrap();
    assert_eq!(c.max_parallel_runs, 2);
    assert_eq!(c.keep_runs_hours, 72);
    assert_eq!(c.ws_url(), "wss://x.example/v1/machine/ws");
    assert!(c.problems().is_empty(), "{:?}", c.problems());

    std::fs::set_permissions(
        dir.path().join("token"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(c.problems().iter().any(|p| p.contains("mode 600")));
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
    text.push_str("[runtimes.sh]\nkind = \"pty\"\ncommand = \"/bin/sh\"\n[runtimes.gone]\nkind = \"acp\"\ncommand = \"/nonexistent/agent\"\n");
    let c = Config::parse(&text).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (report, ok) = rt.block_on(daemon::doctor::report(&c));
    assert!(!ok);
    assert!(report.contains("NOT FOUND"));
    assert!(report.contains("disk free"));
    assert!(!report.contains("tm_x"), "doctor must not print the token");
}

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
    let lint = Command::new("plutil")
        .arg("-lint")
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        lint.status.success(),
        "{}",
        String::from_utf8_lossy(&lint.stdout)
    );
    daemon::service::uninstall(&agents, false).unwrap();
    assert!(!file.exists());
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
    text.push_str(&format!("repo_roots = [\"{}\"]\nkeep_runs_hours = 0\n[runtimes.sh]\nkind = \"pty\"\ncommand = \"/bin/sh\"\n", dir.path().display()));
    let cfg = Config::parse(&text).unwrap();

    let spec = RepoSpec {
        url: None,
        path: Some(repo.to_string_lossy().into()),
        base_branch: Some("main".into()),
    };
    daemon::workspace::validate_repo(&cfg, &spec).unwrap();
    let outside = RepoSpec {
        url: None,
        path: Some("/tmp".into()),
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
    text.push_str("keep_runs_hours = 0\n[runtimes.sh]\nkind = \"pty\"\ncommand = \"/bin/sh\"\n");
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
