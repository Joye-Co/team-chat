//! Machine health and CLI discovery.

use crate::config::{Config, RuntimeConfig};
use crate::proto::{CliInfo, Health};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The program file for `cmd`: a path as given, or a name looked up in `PATH`. On Windows a
/// name without an extension gets the extensions of `PATHEXT` (`claude` is `claude.cmd` or
/// `claude.exe`).
pub fn which(cmd: &str) -> Option<PathBuf> {
    which_in(cmd, std::env::var_os("PATH").as_deref())
}

/// [`which`] with the `PATH` to search (a run's CLI is looked up in the run's `PATH`).
pub fn which_in(cmd: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if cmd.is_empty() {
        return None;
    }
    let names: Vec<String> = if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_default();
        windows_names(cmd, &pathext)
    } else {
        vec![cmd.to_string()]
    };
    let is_path = cmd.contains('/') || (cfg!(windows) && (cmd.contains('\\') || cmd.contains(':')));
    if is_path {
        return names.iter().map(PathBuf::from).find(|p| is_executable(p));
    }
    std::env::split_paths(path?)
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    crate::sys::is_executable(p)
}

/// The extensions Windows can start directly (`.bat` and `.cmd` through `cmd.exe`). `PATHEXT`
/// also lists script types (`.JS`, `.VBS`) that need a host program: those are not CLIs.
const WINDOWS_EXTS: &[&str] = &[".com", ".exe", ".bat", ".cmd"];

/// The file names to try for `cmd` on Windows: `cmd` itself when it already ends in a
/// startable extension, else `cmd` with each startable extension in the order of `pathext`.
/// Never the bare name: npm puts a shell script `claude` next to `claude.cmd`.
pub fn windows_names(cmd: &str, pathext: &str) -> Vec<String> {
    let lower = cmd.to_ascii_lowercase();
    if WINDOWS_EXTS.iter().any(|e| lower.ends_with(e)) {
        return vec![cmd.to_string()];
    }
    let mut exts: Vec<String> = pathext
        .split(';')
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| WINDOWS_EXTS.contains(&e.as_str()))
        .collect();
    for e in WINDOWS_EXTS {
        if !exts.iter().any(|x| x == e) {
            exts.push(e.to_string());
        }
    }
    exts.iter().map(|e| format!("{cmd}{e}")).collect()
}

/// How to start a CLI's program file: the program and the arguments that go before the
/// caller's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// How to start `program`. On Unix the file itself. On Windows an npm-installed CLI is a
/// `.cmd` file that starts `node <script>` or an `.exe` of the package: that target is started
/// directly, so a prompt never goes through `cmd.exe` and its quoting rules. Any other `.cmd`
/// or `.bat` is started as it is; the standard library then quotes the arguments for
/// `cmd.exe` and refuses the ones it cannot quote safely.
pub fn launch(program: &Path) -> Launch {
    let direct = Launch {
        program: program.to_path_buf(),
        args: vec![],
    };
    if !cfg!(windows) {
        return direct;
    }
    let ext = program
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext != "cmd" && ext != "bat" {
        return direct;
    }
    let Some(dir) = program.parent() else {
        return direct;
    };
    let text = match std::fs::read(program) {
        Ok(b) if b.len() <= 64 * 1024 => String::from_utf8_lossy(&b).into_owned(),
        _ => return direct,
    };
    let Some(target) = shim_target(&text) else {
        return direct;
    };
    let target = dir.join(target.replace('\\', std::path::MAIN_SEPARATOR_STR));
    if !target.is_file() {
        return direct;
    }
    let ext = target
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "exe" => Launch {
            program: target,
            args: vec![],
        },
        "js" | "cjs" | "mjs" => {
            // Like the shim: the node.exe next to it, else the one on PATH.
            let beside = dir.join("node.exe");
            let node = if beside.is_file() {
                Some(beside)
            } else {
                which("node")
            };
            match node {
                Some(node) => Launch {
                    program: node,
                    args: vec![target.to_string_lossy().into_owned()],
                },
                None => direct,
            }
        }
        _ => direct,
    }
}

/// The file an npm, pnpm or yarn `.cmd` shim starts, relative to the shim's folder (with
/// backslashes, as written). The shim's last command line ends in `"<%dp0%>\<target>" %*`.
/// None when the text is not such a shim.
pub fn shim_target(text: &str) -> Option<String> {
    let line = text.lines().rev().find(|l| l.contains("%*"))?;
    let line = &line[..line.rfind("%*")?];
    // The last quoted word before `%*`.
    let end = line.rfind('"')?;
    let start = line[..end].rfind('"')?;
    let word = &line[start + 1..end];
    let rest = ["%dp0%\\", "%~dp0\\", "%~dp0"]
        .iter()
        .find_map(|p| word.strip_prefix(p))?;
    let plain = !rest.is_empty()
        && !rest.starts_with('\\')
        && !rest.contains(['%', '"', '^', '&', '|', '<', '>', '!', ':']);
    plain.then(|| rest.to_string())
}

/// A command that starts the program file `program` (see [`launch`]).
pub fn command(program: &Path) -> tokio::process::Command {
    let l = launch(program);
    let mut c = tokio::process::Command::new(&l.program);
    c.args(&l.args);
    c
}

/// First line of `<cmd> --version`, with a 10 s limit.
pub async fn cli_version(cmd: &str) -> Option<String> {
    let mut c = command(Path::new(cmd));
    c.arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(10), c.output())
        .await
        .ok()?
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// Every configured runtime: whether its CLI is installed, its version and what it can run
/// ([`crate::models`]), asked of all CLIs at once.
pub async fn discover_clis(cfg: &Config) -> Vec<CliInfo> {
    futures_util::future::join_all(cfg.runtimes.iter().map(|(name, rt)| discover_cli(name, rt)))
        .await
}

async fn discover_cli(name: &str, rt: &RuntimeConfig) -> CliInfo {
    let command = rt.command();
    let found = which(&command);
    let version = match &found {
        Some(p) if rt.kind != "pty" => cli_version(&p.to_string_lossy()).await,
        _ => None,
    };
    let can_run = crate::models::discover(rt, found.as_deref()).await;
    CliInfo {
        runtime: name.to_string(),
        kind: rt.kind.clone(),
        command,
        version,
        available: found.is_some(),
        models: can_run.models,
        efforts: can_run.efforts,
        // Only Claude Code runs stop at gated commands (a PreToolUse hook); Codex cannot yet.
        gates: crate::gates::enforced_by(&rt.kind).then_some(true),
    }
}

pub fn disk(path: &Path) -> (u64, u64) {
    let mut p = path.to_path_buf();
    while !p.exists() {
        if !p.pop() {
            break;
        }
    }
    crate::sys::disk(&p)
}

pub fn health(cfg: &Config, active_runs: u32) -> Health {
    let (free, total) = disk(&cfg.work_root);
    let l = crate::sys::load_avg();
    let round = |x: f64| (x * 100.0).round() / 100.0;
    Health {
        disk_free_bytes: free,
        disk_total_bytes: total,
        load1: round(l[0]),
        load5: round(l[1]),
        load15: round(l[2]),
        cpu_count: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1),
        active_runs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_names_follow_pathext_and_never_try_the_bare_name() {
        assert_eq!(
            windows_names("claude", ".COM;.EXE;.BAT;.CMD;.VBS;.JS"),
            ["claude.com", "claude.exe", "claude.bat", "claude.cmd"]
        );
        // The order of PATHEXT wins; what it leaves out is still tried, last.
        assert_eq!(
            windows_names("codex", ".CMD;.EXE"),
            ["codex.cmd", "codex.exe", "codex.com", "codex.bat"]
        );
        assert_eq!(windows_names("git.EXE", ".COM;.EXE"), ["git.EXE"]);
        assert_eq!(
            windows_names("C:\\bin\\claude.cmd", ""),
            ["C:\\bin\\claude.cmd"]
        );
        // A dot in the name is not an extension Windows can start.
        assert_eq!(windows_names("tool.v2", ".EXE")[0], "tool.v2.exe");
    }

    #[test]
    fn shim_target_reads_npm_pnpm_and_old_shims() {
        let npm = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*\r\n";
        assert_eq!(
            shim_target(npm).as_deref(),
            Some("node_modules\\@anthropic-ai\\claude-code\\cli.js")
        );
        let native = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*\r\n";
        assert_eq!(
            shim_target(native).as_deref(),
            Some("node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe")
        );
        let old = "@IF EXIST \"%~dp0\\node.exe\" (\r\n  \"%~dp0\\node.exe\"  \"%~dp0\\node_modules\\x\\cli.js\" %*\r\n) ELSE (\r\n  @SETLOCAL\r\n  @SET PATHEXT=%PATHEXT:;.JS;=;%\r\n  node  \"%~dp0\\node_modules\\x\\cli.js\" %*\r\n)\r\n";
        assert_eq!(shim_target(old).as_deref(), Some("node_modules\\x\\cli.js"));
        let pnpm = "@SETLOCAL\r\n@IF EXIST \"%~dp0\\node.exe\" (\r\n  \"%~dp0\\node.exe\"  \"%~dp0\\..\\pkg\\bin\\cli.mjs\" %*\r\n) ELSE (\r\n  node  \"%~dp0\\..\\pkg\\bin\\cli.mjs\" %*\r\n)\r\n";
        assert_eq!(shim_target(pnpm).as_deref(), Some("..\\pkg\\bin\\cli.mjs"));
    }

    #[test]
    fn shim_target_refuses_what_is_not_a_plain_shim() {
        assert_eq!(shim_target("@echo off\r\necho hi\r\n"), None);
        assert_eq!(shim_target("@python \"%~dp0\\tool.py\" %1\r\n"), None);
        // A target that names another variable or leaves the shim's folder by an absolute path.
        assert_eq!(shim_target("\"%dp0%\\%NAME%.exe\" %*"), None);
        assert_eq!(shim_target("\"C:\\tools\\x.exe\" %*"), None);
        assert_eq!(shim_target("\"%dp0%\\\" %*"), None);
        assert_eq!(shim_target("node %*"), None);
    }

    #[test]
    fn launch_is_the_file_itself_for_a_program_that_is_not_a_shim() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("tool");
        std::fs::write(&exe, "x").unwrap();
        assert_eq!(
            launch(&exe),
            Launch {
                program: exe,
                args: vec![]
            }
        );
    }

    /// The shim's `.exe` target is started directly, and a `.js` target with the `node.exe`
    /// next to the shim.
    #[cfg(windows)]
    #[test]
    fn launch_resolves_an_npm_shim() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("node_modules").join("x");
        std::fs::create_dir_all(pkg.join("bin")).unwrap();
        std::fs::write(pkg.join("bin").join("x.exe"), "x").unwrap();
        std::fs::write(pkg.join("cli.js"), "x").unwrap();
        std::fs::write(dir.path().join("node.exe"), "x").unwrap();
        let native = dir.path().join("native.cmd");
        std::fs::write(&native, "\"%dp0%\\node_modules\\x\\bin\\x.exe\"   %*\r\n").unwrap();
        assert_eq!(
            launch(&native),
            Launch {
                program: pkg.join("bin").join("x.exe"),
                args: vec![]
            }
        );
        let js = dir.path().join("js.cmd");
        std::fs::write(
            &js,
            "\"%_prog%\"  \"%dp0%\\node_modules\\x\\cli.js\" %*\r\n",
        )
        .unwrap();
        assert_eq!(
            launch(&js),
            Launch {
                program: dir.path().join("node.exe"),
                args: vec![pkg.join("cli.js").to_string_lossy().into_owned()]
            }
        );
        // Found by name through PATHEXT, in the given PATH.
        let found = which_in("js", Some(dir.path().as_os_str())).unwrap();
        assert_eq!(
            found.file_name().unwrap().to_string_lossy().to_lowercase(),
            "js.cmd"
        );
        assert!(which_in("native.cmd", Some(dir.path().as_os_str())).is_some());
        assert!(which_in("missing", Some(dir.path().as_os_str())).is_none());
    }
}
