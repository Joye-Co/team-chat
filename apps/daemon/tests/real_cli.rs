//! The real Claude Code CLI, as installed on this machine, driven by the daemon's own runtime
//! against a fake model (`tests/fake/model.rs`): no login, no model call, and everything
//! between the daemon and the CLI is the real thing. This is how what depends on the CLI's
//! own behavior is seen on a system nobody can sit at (Windows CI): how it starts the approval
//! hook, what a hook that cannot answer does, which shell tools it has, where it keeps the
//! session file.
//!
//! Each case is skipped (with a line on standard error) when `claude` is not on PATH, and
//! fails instead when `TC_REQUIRE_REAL_CLI` is set (CI installs the CLI and sets it).

#[path = "fake/model.rs"]
mod model;

use daemon::config::RuntimeConfig;
use daemon::proto::RunEvent;
use daemon::runtime::{self, Outcome, RunSpec, Status};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

/// The real `claude`, or None (skip) when this machine has none.
fn claude() -> Option<PathBuf> {
    let found = daemon::health::which("claude");
    if found.is_none() {
        assert!(
            std::env::var_os("TC_REQUIRE_REAL_CLI").is_none(),
            "TC_REQUIRE_REAL_CLI is set and claude is not on PATH"
        );
        eprintln!("skipped: claude is not on PATH");
    }
    found
}

fn spec(dir: &Path, claude: &Path, model: &model::Model, prompt: &str) -> RunSpec {
    let cwd = dir.join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let mut env: Vec<(String, String)> = vec![
        ("PATH".into(), std::env::var("PATH").unwrap()),
        ("HOME".into(), dir.to_string_lossy().into()),
        (
            "CLAUDE_CONFIG_DIR".into(),
            dir.join("claude").to_string_lossy().into(),
        ),
    ];
    for k in daemon::sys::OS_ENV {
        if let Ok(v) = std::env::var(k) {
            env.push((k.to_string(), v));
        }
    }
    env.extend(model.env().into_iter().map(|(k, v)| (k.to_string(), v)));
    RunSpec {
        run_id: "r1".into(),
        runtime_name: "claude".into(),
        cfg: RuntimeConfig {
            kind: "claude".into(),
            command: Some(claude.to_string_lossy().into()),
            args: vec![],
            env: BTreeMap::new(),
            model: None,
            models: None,
            efforts: None,
            approval_policy: None,
            sandbox: None,
            permission_mode: None,
            allowed_tools: None,
        },
        prompt: prompt.into(),
        model: None,
        effort: None,
        cwd,
        run_dir: dir.to_path_buf(),
        env,
        aside: false,
        note: None,
        resume_session_id: None,
        gate_hook: None,
        gate_alive: None,
        exec: daemon::executor::local(),
    }
}

async fn drive(spec: RunSpec) -> (Outcome, Vec<RunEvent>) {
    let rt = runtime::for_kind("claude").unwrap();
    let (etx, mut erx) = mpsc::unbounded_channel();
    let (_ctx, crx) = mpsc::unbounded_channel();
    let run_dir = spec.run_dir.clone();
    let fut = rt.start(spec, etx, crx);
    tokio::pin!(fut);
    let mut evs = vec![];
    let outcome = tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            tokio::select! {
                o = &mut fut => break o,
                Some(e) = erx.recv() => evs.push(e),
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "the CLI did not finish; its stderr: {}",
            std::fs::read_to_string(run_dir.join("stderr.log")).unwrap_or_default()
        )
    });
    while let Ok(e) = erx.try_recv() {
        evs.push(e);
    }
    (outcome, evs)
}

fn stderr_of(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("stderr.log")).unwrap_or_default()
}

/// A turn of the real CLI ends well, and its session file is where the daemon computes it
/// (`projects/<the working folder's name>/<session id>.jsonl`), not merely somewhere under
/// the config folder: a follow-up run resumes from that place.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_real_cli_runs_a_turn_and_keeps_its_session_where_the_daemon_looks() {
    let Some(claude) = claude() else { return };
    let dir = tempfile::tempdir().unwrap();
    let model = model::Model::start().await;
    let s = spec(dir.path(), &claude, &model, "say ok");
    let cwd = s.cwd.clone();
    let (o, evs) = drive(s).await;
    assert_eq!(
        o.status,
        Status::Succeeded,
        "{o:?} {}",
        stderr_of(dir.path())
    );
    assert_eq!(o.result_text.as_deref(), Some("ok"), "{o:?}");
    assert!(
        evs.iter().any(|e| matches!(e, RunEvent::Session { .. })),
        "{evs:?}"
    );
    assert!(!model.requests().is_empty(), "the model was never asked");
    let sid = o.session_id.clone().expect("a session id");
    let direct = dir
        .path()
        .join("claude")
        .join("projects")
        .join(daemon::runtime::claude::project_dir_name(&cwd))
        .join(format!("{sid}.jsonl"));
    let projects: Vec<String> = std::fs::read_dir(dir.path().join("claude").join("projects"))
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        direct.is_file(),
        "no session file at {}; the CLI made {projects:?}",
        direct.display()
    );
}

/// What the model was told about the one tool call of a run with the approval hook installed
/// as `hook`: (is_error, text).
async fn tool_result_with_hook(
    claude: &Path,
    hook: &Path,
    tool: &str,
    command: &str,
    extra_env: &[(&str, &str)],
) -> Option<(bool, String)> {
    let dir = tempfile::tempdir().unwrap();
    let model = model::Model::start().await;
    let mut s = spec(
        dir.path(),
        claude,
        &model,
        &format!("Run the tool.\nTOOL {tool} {command}"),
    );
    s.gate_hook = Some(hook.to_path_buf());
    for (k, v) in extra_env {
        s.env.push((k.to_string(), v.to_string()));
    }
    let (o, _) = drive(s).await;
    if !model.tools().iter().any(|t| t == tool) {
        eprintln!(
            "the CLI has no {tool} tool here; it offered {:?}",
            model.tools()
        );
        return None;
    }
    assert_eq!(
        o.status,
        Status::Succeeded,
        "{o:?} {}",
        stderr_of(dir.path())
    );
    let results = model.tool_results();
    assert_eq!(
        results.len(),
        1,
        "{results:?} {} {}",
        stderr_of(dir.path()),
        model
            .requests()
            .iter()
            .map(|r| format!(
                "\n[tools {} stream {}] {}",
                r["tools"].as_array().map_or(0, Vec::len),
                r["stream"],
                r["messages"]
                    .to_string()
                    .chars()
                    .take(1500)
                    .collect::<String>()
            ))
            .collect::<String>()
    );
    results.into_iter().next()
}

/// The real CLI starts the approval hook of [`daemon::gates::claude_settings`] before a shell
/// tool runs, and a hook that cannot ask the daemon does what this system says: Unix lets the
/// command run (as it always has), Windows refuses it. The hook here is the real `chat-run`
/// with no daemon behind it (no `CHAT_RUN_SOCKET`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_real_cli_starts_the_hook_and_a_hook_without_a_daemon_decides_by_system() {
    let Some(claude) = claude() else { return };
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let links = tempfile::tempdir().unwrap();
    let hook = daemon::chat_run::install_link(links.path())
        .unwrap()
        .join(daemon::chat_run::link_name());

    // The hook's own words in the result prove that the CLI started it; the command's output
    // proves that the command ran.
    let (is_error, text) =
        tool_result_with_hook(&claude, &hook, "Bash", "echo gated-marker-bash", &[])
            .await
            .expect("the Bash tool");
    if cfg!(windows) {
        assert!(is_error, "{text}");
        assert!(
            text.contains("the daemon could not be asked"),
            "the hook's refusal: {text}"
        );
        assert!(
            !text.contains("gated-marker-bash"),
            "the command ran: {text}"
        );
    } else {
        assert!(!is_error && text.contains("gated-marker-bash"), "{text}");
    }
}

/// A hook that cannot even be started (its program is not there): Windows blocks the command
/// (`onFailure: "block"`), so no failure of the hook lets a gated command through there. Unix
/// keeps what it has always done: the command runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hook_that_cannot_start_blocks_the_command_on_windows() {
    let Some(claude) = claude() else { return };
    let nowhere = tempfile::tempdir().unwrap();
    let hook = nowhere
        .path()
        .join("bin")
        .join(daemon::chat_run::link_name());
    let (is_error, text) =
        tool_result_with_hook(&claude, &hook, "Bash", "echo gated-marker-missing", &[])
            .await
            .expect("the Bash tool");
    if cfg!(windows) {
        assert!(is_error, "{text}");
        assert!(
            !text.contains("gated-marker-missing"),
            "the command ran: {text}"
        );
    } else {
        assert!(!is_error && text.contains("gated-marker-missing"), "{text}");
    }
}

/// The PowerShell tool (the shell tool of a Windows machine without Git Bash, turned on here
/// with the CLI's own switch) goes through the same hook. Skipped where the CLI does not
/// offer the tool (no `pwsh` on a Unix machine).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_powershell_tool_goes_through_the_hook_too() {
    let Some(claude) = claude() else { return };
    if !cfg!(windows) && daemon::health::which("pwsh").is_none() {
        eprintln!("skipped: no pwsh on this machine");
        return;
    }
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let links = tempfile::tempdir().unwrap();
    let hook = daemon::chat_run::install_link(links.path())
        .unwrap()
        .join(daemon::chat_run::link_name());
    let Some((is_error, text)) = tool_result_with_hook(
        &claude,
        &hook,
        "PowerShell",
        "Write-Output gated-marker-pwsh",
        &[("CLAUDE_CODE_USE_POWERSHELL_TOOL", "1")],
    )
    .await
    else {
        if cfg!(windows) {
            panic!("Claude Code on Windows must offer the PowerShell tool when it is turned on");
        }
        return;
    };
    if cfg!(windows) {
        assert!(is_error, "{text}");
        assert!(
            text.contains("the daemon could not be asked"),
            "the hook's refusal: {text}"
        );
        assert!(
            !text.contains("gated-marker-pwsh"),
            "the command ran: {text}"
        );
    } else {
        assert!(!is_error && text.contains("gated-marker-pwsh"), "{text}");
    }
}

/// The Windows form of the hook (started without a shell, `onFailure: "block"`) before the
/// real CLI of this system, whatever the system: a hook program that is not there blocks the
/// command, and one that is there and says nothing lets it run. The CLI's hook runner is the
/// same code on every system, so this is seen on the Mac and Linux too, not only on Windows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_windows_form_of_the_hook_blocks_when_it_cannot_start() {
    let Some(claude) = claude() else { return };
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let links = tempfile::tempdir().unwrap();
    let real = daemon::chat_run::install_link(links.path())
        .unwrap()
        .join(daemon::chat_run::link_name());
    let missing = links.path().join("nowhere").join("chat-run");
    for (hook, marker, runs) in [
        (&missing, "gated-marker-exec-missing", false),
        // The real hook without a daemon: refuses on Windows, says nothing on Unix.
        (&real, "gated-marker-exec-real", cfg!(unix)),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let model = model::Model::start().await;
        let settings = dir.path().join("settings.json");
        std::fs::write(
            &settings,
            daemon::gates::claude_settings_as(hook, true).to_string(),
        )
        .unwrap();
        let mut s = spec(
            dir.path(),
            &claude,
            &model,
            &format!("Run the tool.\nTOOL Bash echo {marker}"),
        );
        s.cfg.args = vec!["--settings".into(), settings.to_string_lossy().into()];
        let (o, _) = drive(s).await;
        assert_eq!(
            o.status,
            Status::Succeeded,
            "{o:?} {}",
            stderr_of(dir.path())
        );
        let results = model.tool_results();
        assert_eq!(results.len(), 1, "{results:?} {}", stderr_of(dir.path()));
        let (is_error, text) = &results[0];
        assert_eq!(text.contains(marker), runs, "{marker}: {text}");
        assert_eq!(*is_error, !runs, "{marker}: {text}");
    }
}

/// A sign-in with the real CLI gets as far as it can without a person: the CLI starts, prints
/// the link the app shows (no browser is opened on the machine: `BROWSER` names a program
/// that does nothing, or on Windows does not exist, and the CLI has no other way), and a
/// cancel ends it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sign_in_with_the_real_cli_reports_the_link_and_can_be_cancelled() {
    use daemon::accounts::{LoginCtl, run_login};
    use daemon::proto::login_phase;
    let Some(claude) = claude() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mut env = BTreeMap::new();
    env.insert(
        "CLAUDE_CONFIG_DIR".to_string(),
        dir.path().join("claude").to_string_lossy().to_string(),
    );
    let rt = RuntimeConfig {
        kind: "claude".into(),
        command: Some(claude.to_string_lossy().into()),
        args: vec![],
        env,
        model: None,
        models: None,
        efforts: None,
        approval_policy: None,
        sandbox: None,
        permission_mode: None,
        allowed_tools: None,
    };
    let (ctl_tx, ctl_rx) = mpsc::unbounded_channel();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let login = tokio::spawn(run_login("l1".into(), "claude".into(), rt, ctl_rx, tx));
    let mut states = vec![];
    let url = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let s = rx.recv().await.expect("the sign-in ended without a link");
            states.push(format!("{} {:?}", s.state, s.error));
            if s.state == login_phase::OPEN_URL {
                break s.url.expect("a link");
            }
            assert_ne!(s.state, login_phase::FAILED, "{states:?}");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no link in time: {states:?}"));
    assert!(url.starts_with("https://"), "{url}");
    ctl_tx.send(LoginCtl::Cancel).unwrap();
    let last = tokio::time::timeout(Duration::from_secs(30), async {
        let mut last = None;
        while let Some(s) = rx.recv().await {
            last = Some(s.state);
        }
        last
    })
    .await
    .expect("the sign-in did not end after the cancel");
    assert_eq!(last.as_deref(), Some(login_phase::CANCELLED));
    login.await.unwrap();
}

/// The real CLIs are found by name the way a run finds them, say their version, and `doctor`
/// reports them. Windows: npm's `claude.cmd` and `codex.cmd` are resolved to what they start
/// (the package's `.exe`, or `node` with the package's script), so no prompt goes through
/// `cmd.exe`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_real_clis_are_found_by_name_and_say_their_version() {
    let Some(claude) = claude() else { return };
    let dir = tempfile::tempdir().unwrap();
    let token = dir.path().join("token");
    std::fs::write(&token, "tm_x").unwrap();
    daemon::sys::set_mode(&token, 0o600).unwrap();
    let mut text = format!(
        "server_url = \"https://x.example\"\ntoken_file = '{}'\nwork_root = '{}'\n\
         [runtimes.claude]\nkind = \"claude\"\n",
        token.display(),
        dir.path().join("work").display()
    );
    let codex = daemon::health::which("codex");
    if codex.is_some() {
        text.push_str("[runtimes.codex]\nkind = \"codex\"\n");
    } else {
        assert!(
            std::env::var_os("TC_REQUIRE_REAL_CODEX").is_none(),
            "TC_REQUIRE_REAL_CODEX is set and codex is not on PATH"
        );
    }
    let cfg = daemon::config::Config::parse(&text).unwrap();
    assert!(cfg.problems().is_empty(), "{:?}", cfg.problems());
    let clis = daemon::health::discover_clis(&cfg).await;
    for c in &clis {
        assert!(c.available, "{c:?}");
        let version = c.version.as_deref().unwrap_or_default();
        assert!(
            version.chars().any(|ch| ch.is_ascii_digit()),
            "{}: no version in {version:?}",
            c.runtime
        );
    }
    let real = clis.iter().find(|c| c.kind == "claude").unwrap();
    assert!(
        real.version.as_deref().unwrap().contains("Claude Code"),
        "{real:?}"
    );
    assert_eq!(real.gates, Some(true), "{real:?}");
    assert!(
        real.models.as_ref().is_some_and(|m| !m.is_empty()),
        "{real:?}"
    );
    assert_eq!(
        clis.iter()
            .find(|c| c.kind == "codex")
            .and_then(|c| c.gates),
        None
    );
    let (report, _) = daemon::doctor::report(&cfg).await;
    assert!(!report.contains("NOT FOUND"), "{report}");
    assert!(report.contains("Claude Code"), "{report}");
    // Windows: the owner is told what a side chat of Codex can still do.
    #[cfg(windows)]
    if cfg.runtimes.values().any(|rt| rt.kind == "codex") {
        assert!(
            report.contains(daemon::doctor::CODEX_SIDE_CHAT_ON_WINDOWS),
            "{report}"
        );
    }
    eprintln!("{report}");

    // What a run starts.
    for found in [Some(claude), codex].into_iter().flatten() {
        let launch = daemon::health::launch(&found);
        eprintln!("{} starts {launch:?}", found.display());
        assert!(launch.program.is_file(), "{launch:?}");
        if cfg!(windows) {
            let ext = launch
                .program
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            assert_eq!(
                ext.as_deref(),
                Some("exe"),
                "a wrapper is started through cmd.exe: {launch:?}"
            );
        }
    }
}
