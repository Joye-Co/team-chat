//! Shims for gated actions (docs/machine-protocol.md "Gated actions", "Shims"; audit rows 盯
//! 2.1 and 2.2 of docs/plans/usable/harness-supervise.zh.md).
//!
//! The runtime's own gate (Claude Code's hook, Codex's and ACP's approval requests) sees the
//! command line as the CLI wrote it. A line the patterns do not know (`git -C . push origin
//! main`, a plain `git push` while `main` is checked out, `B=main; git push origin HEAD:$B`, a
//! deploy inside `bash deploy.sh` or a package script) would get through. So a run with gates
//! gets a folder of shims first on its `PATH`: links to this binary named like the programs the
//! kinds are about ([`NAMES`]). A shim asks the daemon (`gate.check {via: "shim"}`) about this
//! one invocation and then starts the real program, or refuses. What it sends is decided on the
//! parsed program and arguments and on the facts, not on the text of a shell line: the
//! arguments as the program got them (the shell already put in variables), plus for `git push`
//! the remote branches it would really update, and for `npm`/`pnpm`/`yarn`/`bun run` the
//! package script it runs ([`check_text`]). The same patterns then decide.
//!
//! This guards against mistakes, not against a member set on getting around it: a program
//! started by its full path, or a login shell whose start-up files the run cannot reach (`sh
//! -l`; a login shell inside the VM image) is not seen. A login zsh gets [`zsh_dotdir`], every
//! bash (a login bash, Git Bash on Windows) [`bash_env_file`]. The floor is the repository's
//! branch protection and the production environment's own approval.

use serde_json::{Value, json};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The programs that get a shim: the ones the default kinds name.
pub const NAMES: &[&str] = &[
    "git", "gh", "wrangler", "npm", "npx", "pnpm", "yarn", "bun", "bunx", "cargo", "vercel",
    "netlify", "fly", "flyctl", "twine", "psql", "mysql", "sqlite3",
];

/// Set to `1` in a run that has gates: only then does a shim ask the daemon. In a VM the shims
/// are always on the tools disk; a run without gates has nothing to ask.
pub const ENV_GATES: &str = "CHAT_RUN_GATES";

/// Claude Code reads this file before each Bash command (`CLAUDE_ENV_FILE`).
pub const CLAUDE_ENV_FILE: &str = "CLAUDE_ENV_FILE";

/// The shim `argv0` names, if any.
pub fn started_as(argv0: &OsStr) -> Option<&'static str> {
    NAMES
        .iter()
        .copied()
        .find(|n| crate::chat_run::started_as(argv0, n))
}

/// True for a folder of a run's tools (where `chat-run` is, or the VM's tools folder).
fn tools_dir(dir: &Path) -> bool {
    dir.as_os_str().is_empty()
        || dir == Path::new(crate::github::GUEST_BIN)
        || dir.join(crate::chat_run::link_name()).exists()
}

/// The real `name` on `path`: not in a folder of the run's tools and not this program. On
/// Windows a name gets the extensions of `PATHEXT`.
pub fn real_program(name: &str, path: &OsStr) -> Option<PathBuf> {
    let me = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(p).ok());
    let names = if cfg!(windows) {
        crate::health::windows_names(name, &std::env::var("PATHEXT").unwrap_or_default())
    } else {
        vec![name.to_string()]
    };
    std::env::split_paths(path)
        .filter(|d| !tools_dir(d))
        .flat_map(|d| names.iter().map(move |n| d.join(n)).collect::<Vec<_>>())
        .find(|c| {
            crate::sys::is_executable(c) && (me.is_none() || std::fs::canonicalize(c).ok() != me)
        })
}

/// Links `bin/<name>` to this binary (a hard link or a copy on Windows).
pub fn link(bin: &Path, name: &str) -> std::io::Result<()> {
    let link = bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let _ = std::fs::remove_file(&link);
    let exe = crate::chat_run::exe()?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(exe, &link)?;
    #[cfg(not(unix))]
    if std::fs::hard_link(&exe, &link).is_err() {
        std::fs::copy(&exe, &link)?;
    }
    Ok(())
}

/// Puts a shim into the run's tools folder `bin` for each of [`NAMES`] that is on `path` (a
/// program the machine does not have stays missing, so `command -v vercel` still says so).
/// Returns the names linked.
pub fn install(bin: &Path, path: &OsStr) -> std::io::Result<Vec<&'static str>> {
    let mut done = vec![];
    for name in NAMES {
        if real_program(name, path).is_some() {
            link(bin, name)?;
            done.push(*name);
        }
    }
    Ok(done)
}

/// `bin` as a shell started for the run sees it on `PATH`: Git Bash on Windows turns `PATH`
/// into MSYS form ([`msys_path`]).
fn shell_dir(bin: &str) -> String {
    if cfg!(windows) {
        msys_path(bin)
    } else {
        bin.to_string()
    }
}

/// A Windows path in the form MSYS (Git Bash) uses on its `PATH`: `C:\r\bin` is `/c/r/bin`.
pub fn msys_path(p: &str) -> String {
    let p = p.strip_prefix(r"\\?\").unwrap_or(p).replace('\\', "/");
    let b = p.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        format!("/{}{}", (b[0] as char).to_ascii_lowercase(), &p[2..])
    } else {
        p
    }
}

/// A start-up file's path as the value of the variable a shell reads it from: on Windows with
/// `/`, which Git Bash and Windows programs both open.
fn env_file_value(file: &Path) -> String {
    let s = file.display().to_string();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s
    }
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Writes `<run folder>/.claude-env.sh`, which puts `bin` first on `PATH` again: Claude Code
/// runs a Bash command with the `PATH` of the user's shell, whose profile may put other
/// folders first (on Windows Git Bash, whose `/etc/profile` puts `/mingw64/bin` and `/usr/bin`
/// first). Returns the value for [`CLAUDE_ENV_FILE`].
pub fn claude_env_file(run_dir: &Path, bin: &str) -> std::io::Result<String> {
    let file = run_dir.join(".claude-env.sh");
    let quoted = sh_quote(&shell_dir(bin));
    std::fs::write(&file, format!("export PATH={quoted}:\"$PATH\"\n"))?;
    Ok(env_file_value(&file))
}

/// `BASH_ENV` of a gated run. Bash reads it in every non-interactive shell (`bash -c`, `bash
/// script.sh`), and a login bash (`bash -lc`: Codex with bash as the user's shell, Git Bash)
/// reads it after `/etc/profile` and the user's profile, which put other folders first.
pub const BASH_ENV: &str = "BASH_ENV";
/// The user's own `BASH_ENV`, which the run's file reads first.
pub const ENV_USER_BASH_ENV: &str = "CHAT_RUN_USER_BASH_ENV";

/// Set (exported) once a bash of the run read `~/.bashrc` for [`bash_env_file`], so the
/// shells it starts do not read it again.
pub const ENV_BASHRC_READ: &str = "CHAT_RUN_BASHRC_READ";

/// Writes `<run folder>/.bash-env.sh`, for [`BASH_ENV`]: reads the user's own `BASH_ENV`
/// ([`ENV_USER_BASH_ENV`], expanded as bash expands `BASH_ENV`), then puts `bin` first on
/// `PATH` again, once. Returns the value for [`BASH_ENV`].
///
/// With `read_bashrc` (Codex, an ACP agent): Codex reads `~/.bashrc` when it captures the
/// user's bash only while `BASH_ENV` is empty, which the run's `BASH_ENV` never is. So a login,
/// non-interactive bash of the run with no own `BASH_ENV` reads `~/.bashrc` itself, once per
/// process tree ([`ENV_BASHRC_READ`]).
pub fn bash_env_file(run_dir: &Path, bin: &str, read_bashrc: bool) -> std::io::Result<String> {
    let file = run_dir.join(".bash-env.sh");
    let quoted = sh_quote(&shell_dir(bin));
    let user = ENV_USER_BASH_ENV;
    let guard = ENV_BASHRC_READ;
    let bashrc = if read_bashrc {
        format!(
            "elif [ -z \"${{{guard}:-}}\" ] && shopt -q login_shell && [[ $- != *i* ]] \
&& [ -n \"${{HOME-}}\" ] && [ -r \"$HOME/.bashrc\" ]; then\n\
  export {guard}=1\n\
  . \"$HOME/.bashrc\"\n"
        )
    } else {
        String::new()
    };
    std::fs::write(
        &file,
        format!(
            "# Written for a run: reads your own BASH_ENV, then puts the run's tools first on PATH.\n\
if [ -n \"${{{user}:-}}\" ]; then\n\
  eval \"__chat_run_user=\\\"${{{user}}}\\\"\"\n\
  if [ -r \"$__chat_run_user\" ]; then . \"$__chat_run_user\"; fi\n\
  unset __chat_run_user\n\
{bashrc}fi\n\
__chat_run_bin={quoted}\n\
__chat_run_path=\":$PATH:\"\n\
while [[ \"$__chat_run_path\" == *\":$__chat_run_bin:\"* ]]; do\n\
  __chat_run_path=\"${{__chat_run_path//\":$__chat_run_bin:\"/:}}\"\n\
done\n\
__chat_run_path=\"${{__chat_run_path#:}}\"\n\
__chat_run_path=\"${{__chat_run_path%:}}\"\n\
export PATH=\"$__chat_run_bin${{__chat_run_path:+:$__chat_run_path}}\"\n\
unset __chat_run_bin __chat_run_path\n"
        ),
    )?;
    Ok(env_file_value(&file))
}

/// Writes the run's [`bash_env_file`] and points [`BASH_ENV`] of `env` at it; the user's own
/// `BASH_ENV` (from `env`, else the daemon's) moves to [`ENV_USER_BASH_ENV`].
pub fn set_bash_env(
    env: &mut Vec<(String, String)>,
    run_dir: &Path,
    bin: &str,
    read_bashrc: bool,
) -> std::io::Result<()> {
    let file = bash_env_file(run_dir, bin, read_bashrc)?;
    let own = env
        .iter()
        .find(|(k, _)| k == BASH_ENV)
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var(BASH_ENV).ok())
        .filter(|v| !v.is_empty() && *v != file);
    env.retain(|(k, _)| k != BASH_ENV && k != ENV_USER_BASH_ENV && k != ENV_BASHRC_READ);
    if let Some(own) = own {
        env.push((ENV_USER_BASH_ENV.into(), own));
    }
    env.push((BASH_ENV.into(), file));
    Ok(())
}

/// `ZDOTDIR` of a run whose CLI runs commands in the user's login zsh (Codex: `zsh -lc`).
pub const ZDOTDIR: &str = "ZDOTDIR";
/// The user's own `ZDOTDIR` (empty: `$HOME`), which the run's zsh files read from.
pub const ENV_USER_ZDOTDIR: &str = "CHAT_RUN_USER_ZDOTDIR";

/// Writes the zsh start-up files of `<run folder>/zsh`, for [`ZDOTDIR`]: each reads the user's
/// own file of that name, then puts `bin` first on `PATH` again. A login zsh would otherwise
/// leave the shims last: on macOS `/etc/zprofile` runs `path_helper`, which puts the system
/// folders before the inherited `PATH`, and a profile may put its own folders first. Returns
/// the folder.
pub fn zsh_dotdir(run_dir: &Path, bin: &str) -> std::io::Result<PathBuf> {
    let dir = run_dir.join("zsh");
    std::fs::create_dir_all(&dir)?;
    let quoted = format!("'{}'", bin.replace('\'', r"'\''"));
    for name in [".zshenv", ".zprofile", ".zshrc", ".zlogin"] {
        // The user's .zshenv may move ZDOTDIR: the later files are read from there.
        let keep = if name == ".zshenv" {
            format!("export {ENV_USER_ZDOTDIR}=\"$ZDOTDIR\"\n")
        } else {
            String::new()
        };
        std::fs::write(
            dir.join(name),
            format!(
                "# Written for a run: reads your own {name}, then puts the run's tools first on PATH.\n\
__chat_run_zd=\"$ZDOTDIR\"\n\
ZDOTDIR=\"${{{ENV_USER_ZDOTDIR}:-$HOME}}\"\n\
[[ -r \"$ZDOTDIR/{name}\" ]] && source \"$ZDOTDIR/{name}\"\n\
{keep}\
ZDOTDIR=\"$__chat_run_zd\"\n\
unset __chat_run_zd\n\
path=({quoted} ${{path:#{quoted}}})\n\
export PATH\n"
            ),
        )?;
    }
    Ok(dir)
}

// --- What a shim asks about. ------------------------------------------------------------------

/// One argument as a word of the line the patterns match: the separators that split a shell
/// line into commands become spaces, so one invocation stays one command.
fn word(a: &str) -> String {
    a.chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | ';' | '|' | '&') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

fn line(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().map(|a| word(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a shim sends as `command` for `name args` run in `cwd`: the invocation as plain words,
/// then the facts, one line each. `git` is the real git (for the facts of `git push`).
pub fn check_text(name: &str, args: &[String], cwd: &Path, git: Option<&Path>) -> String {
    let mut lines = vec![line(name, args)];
    match name {
        "git" => {
            if let Some(bin) = git {
                lines.extend(git_lines(bin, cwd, args));
            }
        }
        "npm" | "pnpm" | "yarn" | "bun" => lines.extend(script_lines(name, args, cwd)),
        _ => {}
    }
    let mut seen = std::collections::HashSet::new();
    lines.retain(|l| seen.insert(l.clone()));
    lines.join("\n")
}

/// Git's options before the subcommand that take the next argument as their value.
const GIT_VALUE_OPTS: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--super-prefix",
    "--config-env",
];

/// Git's own commands: an alias cannot have their names, so these are never looked up.
const GIT_BUILTINS: &[&str] = &[
    "add",
    "am",
    "apply",
    "bisect",
    "blame",
    "branch",
    "cat-file",
    "checkout",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "config",
    "describe",
    "diff",
    "fetch",
    "for-each-ref",
    "format-patch",
    "fsck",
    "gc",
    "grep",
    "hash-object",
    "help",
    "init",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "merge",
    "merge-base",
    "mv",
    "notes",
    "pull",
    "push",
    "rebase",
    "reflog",
    "remote",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "send-pack",
    "shortlog",
    "show",
    "stash",
    "status",
    "submodule",
    "switch",
    "symbolic-ref",
    "tag",
    "update-index",
    "update-ref",
    "version",
    "worktree",
];

/// The real git with the invocation's own options before the subcommand (`-C`, `-c`,
/// `--git-dir`), so it reads the same repository and config.
struct Git<'a> {
    bin: &'a Path,
    cwd: &'a Path,
    globals: Vec<String>,
}

impl Git<'_> {
    fn out(&self, args: &[&str]) -> Option<String> {
        let out = Command::new(self.bin)
            .current_dir(self.cwd)
            .args(&self.globals)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn config(&self, key: &str) -> Option<String> {
        self.out(&["config", "--get", key])
            .filter(|v| !v.is_empty())
    }

    fn config_all(&self, key: &str) -> Vec<String> {
        self.out(&["config", "--get-all", key])
            .map(|v| v.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// The checked-out branch, None when HEAD is detached.
    fn branch(&self) -> Option<String> {
        self.out(&["symbolic-ref", "-q", "HEAD"])?
            .strip_prefix("refs/heads/")
            .map(str::to_string)
    }

    fn branches(&self) -> Vec<String> {
        self.out(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])
            .map(|v| v.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }
}

/// Splits git's arguments into the options before the subcommand, the subcommand and the rest.
fn split_git(args: &[String]) -> (Vec<String>, Option<String>, Vec<String>) {
    let mut globals = vec![];
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if !a.starts_with('-') || a == "--" {
            break;
        }
        globals.push(a.clone());
        if GIT_VALUE_OPTS.contains(&a.as_str())
            && let Some(v) = args.get(i + 1)
        {
            globals.push(v.clone());
            i += 1;
        }
        i += 1;
    }
    (
        globals,
        args.get(i).cloned(),
        args.get(i + 1..).unwrap_or_default().to_vec(),
    )
}

/// The facts lines of a git invocation: the command without its options before the
/// subcommand and with aliases put in (`git push …`), and for a push one line per remote
/// branch it updates, `git push <remote> <source>:refs/heads/<branch>` (a deletion has no
/// source).
fn git_lines(bin: &Path, cwd: &Path, args: &[String]) -> Vec<String> {
    let (globals, sub, mut rest) = split_git(args);
    let Some(mut sub) = sub else {
        return vec![];
    };
    let git = Git { bin, cwd, globals };
    // Aliases (`git p` with `alias.p = push`); one that runs a shell (`!…`) starts git again,
    // and that git is a shim too.
    for _ in 0..8 {
        if GIT_BUILTINS.contains(&sub.as_str()) {
            break;
        }
        let Some(words) = git
            .config(&format!("alias.{sub}"))
            .filter(|v| !v.starts_with('!'))
            .and_then(|v| crate::gates::shell_words(&v))
            .filter(|w| !w.is_empty())
        else {
            break;
        };
        sub = words[0].clone();
        rest = words[1..].iter().cloned().chain(rest).collect();
    }
    let mut out = vec![line(
        "git",
        &std::iter::once(sub.clone())
            .chain(rest.clone())
            .collect::<Vec<_>>(),
    )];
    if sub == "push" {
        out.extend(push_lines(&git, &rest));
    }
    out
}

/// `git push`'s options that take the next argument as their value (when not given with `=`).
const PUSH_VALUE_OPTS: &[&str] = &["-o", "--push-option", "--repo", "--receive-pack", "--exec"];

/// The remote branches `git push <args>` would update, as facts lines.
fn push_lines(git: &Git, args: &[String]) -> Vec<String> {
    let (mut all, mut delete, mut tags) = (false, false, false);
    let mut repo: Option<String> = None;
    let mut positional = vec![];
    let mut i = 0;
    let mut options = true;
    while let Some(a) = args.get(i) {
        i += 1;
        if !options || !a.starts_with('-') || a == "-" {
            positional.push(a.clone());
            continue;
        }
        if a == "--" {
            options = false;
            continue;
        }
        match a.as_str() {
            "--all" | "--branches" | "--mirror" => all = true,
            "--delete" => delete = true,
            "--tags" => tags = true,
            _ if a.starts_with("--repo=") => repo = Some(a["--repo=".len()..].to_string()),
            _ if PUSH_VALUE_OPTS.contains(&a.as_str()) => {
                if a == "--repo" {
                    repo = args.get(i).cloned();
                }
                i += 1;
            }
            // Short flags together (`-fu`, `-d`); `-o` takes the rest of the word.
            _ if !a.starts_with("--") => {
                let flags = &a[1..];
                let flags = flags.split('o').next().unwrap_or_default();
                if flags.contains('d') {
                    delete = true;
                }
            }
            _ => {}
        }
    }
    let mut positional = positional.into_iter();
    let branch = git.branch();
    let remote = positional
        .next()
        .or(repo)
        .or_else(|| {
            let b = branch.as_deref()?;
            git.config(&format!("branch.{b}.pushRemote"))
                .or_else(|| git.config("remote.pushDefault"))
                .or_else(|| git.config(&format!("branch.{b}.remote")))
        })
        .unwrap_or_else(|| "origin".into());
    let mut specs: Vec<String> = positional.collect();
    // (source, remote ref) pairs; an empty source deletes.
    let mut targets: Vec<(String, String)> = vec![];
    let every_branch = |targets: &mut Vec<(String, String)>| {
        for b in git.branches() {
            targets.push((b.clone(), qualify(&b)));
        }
    };
    if all {
        every_branch(&mut targets);
    } else if specs.is_empty() && !tags {
        // No refspec: the remote's configured ones, else `push.default`.
        let configured = if remote.contains('/') || remote.contains(':') {
            vec![]
        } else {
            git.config_all(&format!("remote.{remote}.push"))
        };
        if !configured.is_empty() {
            specs = configured;
        } else if let Some(b) = branch.as_deref() {
            match git.config("push.default").as_deref().unwrap_or("simple") {
                "nothing" => {}
                "matching" => every_branch(&mut targets),
                "upstream" | "tracking" => {
                    let up = git
                        .config(&format!("branch.{b}.merge"))
                        .unwrap_or_else(|| b.to_string());
                    targets.push(("HEAD".into(), qualify(&up)));
                }
                _ => targets.push(("HEAD".into(), qualify(b))),
            }
        }
    }
    for spec in specs {
        let spec = spec.strip_prefix('+').unwrap_or(&spec).to_string();
        if spec.contains('*') {
            every_branch(&mut targets);
            continue;
        }
        if delete {
            targets.push((String::new(), qualify(&spec)));
            continue;
        }
        match spec.split_once(':') {
            Some((src, "")) => {
                // `src:` pushes to the same name.
                targets.push((src.to_string(), same_name(src, branch.as_deref())));
            }
            Some((src, dst)) => targets.push((src.to_string(), qualify(dst))),
            None => targets.push((spec.clone(), same_name(&spec, branch.as_deref()))),
        }
    }
    targets
        .into_iter()
        .filter(|(_, dst)| !dst.is_empty())
        .map(|(src, dst)| format!("git push {} {}:{dst}", word(&remote), word(&src)))
        .collect()
}

/// The remote ref a refspec without a destination updates: `HEAD` is the checked-out branch.
fn same_name(src: &str, branch: Option<&str>) -> String {
    match src {
        "HEAD" | "@" => branch.map(qualify).unwrap_or_default(),
        _ => qualify(src),
    }
}

/// A destination as a full ref: `main` and `heads/main` are `refs/heads/main`.
fn qualify(dst: &str) -> String {
    if dst.starts_with("refs/") {
        dst.to_string()
    } else if let Some(rest) = dst.strip_prefix("heads/") {
        format!("refs/heads/{rest}")
    } else if dst.is_empty() {
        String::new()
    } else {
        format!("refs/heads/{dst}")
    }
}

/// The package scripts `npm run x` (`pnpm x`, `yarn x`, `bun run x`) runs, from the
/// `package.json` of the folder it runs in: a script's program usually comes from
/// `node_modules/.bin`, where no shim is.
fn script_lines(name: &str, args: &[String], cwd: &Path) -> Vec<String> {
    let dir_opts: &[&str] = match name {
        "npm" => &["--prefix"],
        "pnpm" => &["-C", "--dir"],
        _ => &["--cwd"],
    };
    let mut dir = cwd.to_path_buf();
    let mut positional = vec![];
    let mut i = 0;
    while let Some(a) = args.get(i) {
        i += 1;
        if a == "--" {
            break;
        }
        if let Some((k, v)) = a.split_once('=')
            && dir_opts.contains(&k)
        {
            dir = cwd.join(v);
        } else if dir_opts.contains(&a.as_str()) {
            if let Some(v) = args.get(i) {
                dir = cwd.join(v);
            }
            i += 1;
        } else if !a.starts_with('-') {
            positional.push(a.as_str());
        }
    }
    let script = match positional
        .iter()
        .position(|p| matches!(*p, "run" | "run-script"))
    {
        Some(at) => positional.get(at + 1).copied(),
        None if name == "npm" => positional
            .first()
            .copied()
            .filter(|p| matches!(*p, "start" | "stop" | "restart" | "test")),
        None => positional.first().copied(),
    };
    let Some(script) = script else {
        return vec![];
    };
    let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else {
        return vec![];
    };
    let Ok(pkg) = serde_json::from_str::<Value>(&text) else {
        return vec![];
    };
    [
        format!("pre{script}"),
        script.to_string(),
        format!("post{script}"),
    ]
    .iter()
    .filter_map(|s| pkg["scripts"][s].as_str())
    .map(str::to_string)
    .collect()
}

// --- Running a shim. --------------------------------------------------------------------------

/// Asks the daemon whether `name args` may run. Ok: run it. Err: the exit code, after the
/// reason went to standard error. Outside a gated run nothing is asked.
pub fn gate(name: &str, args: &[String]) -> Result<(), i32> {
    if std::env::var(ENV_GATES).ok().as_deref() != Some("1") {
        return Ok(());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    let git = (name == "git")
        .then(|| real_program("git", &path))
        .flatten();
    let command = check_text(name, args, &cwd, git.as_deref());
    let answer = ask(json!({ "tool": "shim", "via": crate::gates::VIA_SHIM, "command": command }));
    match decision(answer) {
        Ok(()) => Ok(()),
        Err(why) => {
            eprintln!("{name}: {why}");
            Err(1)
        }
    }
}

fn ask(body: Value) -> Result<Value, String> {
    use crate::chat_run::{ENV_DIR, ENV_SOCKET, call, call_files};
    let var = |k: &str| std::env::var_os(k).filter(|s| !s.is_empty());
    let (op, wait) = (crate::gates::OP, crate::gates::SHIM_CALL_WAIT);
    match (var(ENV_SOCKET), var(ENV_DIR)) {
        (Some(s), _) => call(Path::new(&s), op, body, wait),
        (None, Some(d)) => call_files(Path::new(&d), op, body, wait),
        (None, None) => Err(format!("{ENV_SOCKET} is not set")),
    }
}

/// The daemon's answer as run (Ok) or refused with the reason the CLI reads. When the daemon
/// could not be asked the command is refused too, as the hook does
/// ([`crate::gates::hook_failure`]).
fn decision(answer: Result<Value, String>) -> Result<(), String> {
    let why = match answer {
        Ok(v) if v["ok"] == json!(true) && v["result"]["decision"] == "allow" => return Ok(()),
        Ok(v) if v["ok"] == json!(true) => {
            return Err(v["result"]["reason"]
                .as_str()
                .unwrap_or("a person declined this command")
                .to_string());
        }
        Ok(v) => v["error"]["message"]
            .as_str()
            .unwrap_or("the daemon refused")
            .to_string(),
        Err(e) => e,
    };
    Err(format!(
        "this command did not run: the machine could not check whether it needs a person's \
approval ({why}). Wait a few seconds, then run the same command again."
    ))
}

/// A shim's `main`: asks ([`gate`]), then runs the real `name` with `args` (on Unix in place of
/// this process). Exits like the real program.
pub fn main(name: &str, args: Vec<String>) -> i32 {
    if let Err(code) = gate(name, &args) {
        return code;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(real) = real_program(name, &path) else {
        eprintln!("{name}: command not found");
        return 127;
    };
    run(&real, &args)
}

/// Runs `program args` and returns its exit code (on Unix it replaces this process).
pub fn run(program: &Path, args: &[String]) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = Command::new(program).args(args).exec();
        eprintln!("{}: {e}", program.display());
        126
    }
    #[cfg(not(unix))]
    {
        let launch = crate::health::launch(program);
        match Command::new(&launch.program)
            .args(&launch.args)
            .args(args)
            .status()
        {
            Ok(s) => s.code().unwrap_or(1),
            Err(e) => {
                eprintln!("{}: {e}", program.display());
                126
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::Gates;
    use crate::proto::RunGate;

    fn s(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn git_bin() -> PathBuf {
        crate::health::which("git").expect("git on PATH for the tests")
    }

    /// A throwaway repository with no remote, on branch `branch`, with one commit.
    fn repo(branch: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let g = |args: &[&str]| {
            let ok = Command::new(git_bin())
                .current_dir(tmp.path())
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        g(&["init", "-q", "-b", branch]);
        // The machine's own push.default does not decide the tests.
        g(&["config", "push.default", "simple"]);
        g(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ]);
        tmp
    }

    fn gates() -> Gates {
        Gates::new(&[
            RunGate {
                kind: "mergeMain".into(),
                patterns: s(&[
                    "\\bgh pr merge\\b",
                    "\\bgit push\\b.*[\\s:](refs/heads/)?(main|master)(\\s|$)",
                ]),
            },
            RunGate {
                kind: "deployProd".into(),
                patterns: s(&[
                    "\\bwrangler (deploy|publish)\\b.*(--env|-e)[ =](prod|production)\\b",
                ]),
            },
        ])
    }

    fn kind(name: &str, args: &[&str], cwd: &Path) -> Option<String> {
        let text = check_text(name, &s(args), cwd, Some(&git_bin()));
        gates().matches(&text).map(str::to_string)
    }

    /// The bypasses of 盯 2.2 with real git in a repository with no remote: each one is seen
    /// as the push to main it is.
    #[test]
    fn git_push_is_decided_on_the_branch_it_updates() {
        let on_main = repo("main");
        let d = on_main.path();
        // Verification log problem 3, and the spellings the line patterns miss.
        for args in [
            &["-C", ".", "push", "origin", "main"][..],
            &["push"],
            &["push", "origin"],
            &["push", "-u", "origin", "HEAD"],
            &["push", "origin", "HEAD"],
            &["push", "--force", "origin", "@"],
            &["-c", "push.default=current", "push"],
            &["push", "origin", "HEAD:main"],
            &["push", "origin", "feature:heads/main"],
            &["push", "origin", "--delete", "main"],
            &["push", "origin", ":main"],
            &["push", "--all", "origin"],
            &["--git-dir=.git", "push"],
        ] {
            assert_eq!(
                kind("git", args, d).as_deref(),
                Some("mergeMain"),
                "{args:?}"
            );
        }
        // `B=main; git push origin HEAD:$B`: the shell gives the shim the value.
        let b = "main";
        assert_eq!(
            kind("git", &["push", "origin", &format!("HEAD:{b}")], d).as_deref(),
            Some("mergeMain")
        );
        // An alias for push.
        Command::new(git_bin())
            .current_dir(d)
            .args(["config", "alias.p", "push"])
            .status()
            .unwrap();
        assert_eq!(kind("git", &["p"], d).as_deref(), Some("mergeMain"));
        // Pushes that leave main alone.
        for args in [
            &["push", "origin", "HEAD:claude/x"][..],
            &["push", "origin", "main:backup-main"],
            &["push", "--tags"],
            &["status"],
            &["log", "--oneline", "main"],
            &["-c", "push.default=nothing", "push"],
        ] {
            assert_eq!(kind("git", args, d), None, "{args:?}");
        }

        // On a work branch a plain push goes to that branch; naming main still counts.
        let on_work = repo("claude/m4");
        let w = on_work.path();
        assert_eq!(kind("git", &["push"], w), None);
        assert_eq!(kind("git", &["push", "-u", "origin", "HEAD"], w), None);
        assert_eq!(
            kind("git", &["push", "origin", "HEAD:main"], w).as_deref(),
            Some("mergeMain")
        );
        // `git -C <main checkout> push` from anywhere.
        let main_dir = d.display().to_string();
        assert_eq!(
            kind("git", &["-C", &main_dir, "push"], w).as_deref(),
            Some("mergeMain")
        );
        // `branch.<b>.merge` with push.default upstream.
        Command::new(git_bin())
            .current_dir(w)
            .args(["config", "branch.claude/m4.merge", "refs/heads/main"])
            .status()
            .unwrap();
        assert_eq!(
            kind("git", &["-c", "push.default=upstream", "push"], w).as_deref(),
            Some("mergeMain")
        );
        // A configured remote push refspec.
        Command::new(git_bin())
            .current_dir(w)
            .args(["config", "remote.origin.push", "HEAD:refs/heads/master"])
            .status()
            .unwrap();
        assert_eq!(kind("git", &["push"], w).as_deref(), Some("mergeMain"));
    }

    #[test]
    fn package_scripts_and_wrapped_programs_are_seen() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts":{"ship":"bun run build && wrangler deploy --env production","build":"tsc"}}"#,
        )
        .unwrap();
        let d = tmp.path();
        assert_eq!(
            kind("npm", &["run", "ship"], d).as_deref(),
            Some("deployProd")
        );
        assert_eq!(
            kind("bun", &["run", "ship"], d).as_deref(),
            Some("deployProd")
        );
        assert_eq!(kind("pnpm", &["ship"], d).as_deref(), Some("deployProd"));
        assert_eq!(
            kind("yarn", &["--silent", "ship"], d).as_deref(),
            Some("deployProd")
        );
        let parent = d.parent().unwrap();
        let name = d.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            kind("npm", &["--prefix", name, "run", "ship"], parent).as_deref(),
            Some("deployProd")
        );
        assert_eq!(kind("npm", &["run", "build"], d), None);
        assert_eq!(kind("npm", &["install"], d), None);
        // `npx wrangler …` and `bunx wrangler …`: the plain line has it.
        assert_eq!(
            kind("bunx", &["wrangler", "deploy", "--env", "prod"], d).as_deref(),
            Some("deployProd")
        );
        assert_eq!(
            kind("gh", &["pr", "merge", "5"], d).as_deref(),
            Some("mergeMain")
        );
        // A separator inside an argument does not make a second command.
        assert_eq!(
            check_text("gh", &s(&["issue", "comment", "-b", "a; b\nc"]), d, None),
            "gh issue comment -b a  b c"
        );
    }

    #[test]
    fn the_answer_decides_and_no_answer_refuses() {
        assert!(decision(Ok(json!({ "ok": true, "result": { "decision": "allow" } }))).is_ok());
        let no = decision(Ok(
            json!({ "ok": true, "result": { "decision": "deny", "reason": "a person declined it" } }),
        ));
        assert_eq!(no.unwrap_err(), "a person declined it");
        for failed in [
            Err("cannot reach the daemon".to_string()),
            Ok(json!({ "ok": false, "error": { "message": "x" } })),
        ] {
            assert!(
                decision(failed)
                    .unwrap_err()
                    .contains("run the same command again")
            );
        }
    }

    #[test]
    fn shims_go_only_where_the_program_is_and_skip_the_tools_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let tools = tmp.path().join("bin");
        let real = tmp.path().join("real");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        crate::chat_run::install_link(tmp.path()).unwrap();
        let exe = format!("wrangler{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(real.join(&exe), "#!/bin/sh\n").unwrap();
        crate::sys::set_mode(&real.join(&exe), 0o755).unwrap();
        let path = std::env::join_paths([tools.clone(), real.clone()]).unwrap();
        let linked = install(&tools, &path).unwrap();
        assert_eq!(linked, ["wrangler"]);
        assert!(tools.join(&exe).exists());
        assert!(
            !tools
                .join(format!("vercel{}", std::env::consts::EXE_SUFFIX))
                .exists()
        );
        // The shim is never its own real program.
        assert_eq!(real_program("wrangler", &path), Some(real.join(&exe)));
        assert_eq!(started_as(OsStr::new("/x/bin/wrangler")), Some("wrangler"));
        assert_eq!(started_as(OsStr::new("/x/bin/chat-run")), None);
    }

    /// Codex runs each command as `zsh -lc`. On macOS `/etc/zprofile` (path_helper) and many
    /// profiles put their folders first; the run's zsh files put the shims first again, after
    /// reading the user's own files.
    #[cfg(unix)]
    #[test]
    fn a_login_zsh_keeps_the_shims_first() {
        if Command::new("zsh").arg("-c").arg("true").status().is_err() {
            eprintln!("no zsh here: skipped");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let own = tmp.path().join("own zdot");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&own).unwrap();
        // The user's .zshenv moves ZDOTDIR; their .zprofile puts a folder first.
        std::fs::write(
            home.join(".zshenv"),
            format!("export ZDOTDIR='{}'\nSEEN=env\n", own.display()),
        )
        .unwrap();
        std::fs::write(
            own.join(".zprofile"),
            "export PATH=/opt/first:$PATH\nSEEN=$SEEN,profile\n",
        )
        .unwrap();
        let bin = tmp.path().join("run").join("bin");
        let zdot = zsh_dotdir(&tmp.path().join("run"), &bin.display().to_string()).unwrap();
        let run = |login: bool| {
            let out = Command::new("zsh")
                .arg(if login { "-lc" } else { "-c" })
                .arg(r#"printf '%s|%s|%s' "$PATH" "$SEEN" "$ZDOTDIR""#)
                .env_clear()
                .env("HOME", &home)
                .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                .env(ZDOTDIR, &zdot)
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).to_string()
        };
        let out = run(true);
        let parts: Vec<&str> = out.split('|').collect();
        let path: Vec<&str> = parts[0].split(':').collect();
        assert_eq!(path[0], bin.display().to_string(), "{out}");
        assert_eq!(path.iter().filter(|p| **p == path[0]).count(), 1, "{out}");
        assert!(path.contains(&"/opt/first"), "{out}");
        assert_eq!(parts[1], "env,profile", "{out}");
        assert_eq!(parts[2], zdot.display().to_string(), "{out}");
        // A plain `zsh -c` reads only .zshenv: the shims stay first too.
        let out = run(false);
        assert!(out.starts_with(&format!("{}:", bin.display())), "{out}");
    }

    /// The bash programs here: the system one and any other (Homebrew's bash 5 next to macOS's
    /// 3.2).
    #[cfg(unix)]
    fn bashes() -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = vec![];
        let path = std::env::var_os("PATH").unwrap_or_default();
        let fixed = ["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"].map(PathBuf::from);
        for dir in fixed.into_iter().chain(std::env::split_paths(&path)) {
            let b = dir.join("bash");
            let Ok(real) = std::fs::canonicalize(&b) else {
                continue;
            };
            if crate::sys::is_executable(&b)
                && !found
                    .iter()
                    .any(|f| std::fs::canonicalize(f).ok().as_ref() == Some(&real))
            {
                found.push(b);
            }
        }
        found
    }

    /// Codex runs each command as `<user's shell> -lc`; with bash that reads `/etc/profile`
    /// (macOS: path_helper) and the user's `.bash_profile`, which put their folders first (Git
    /// Bash's `/etc/profile` puts `/mingw64/bin` first the same way). The run's `BASH_ENV` comes
    /// after them and puts the shims first again, once, after reading the user's own
    /// `BASH_ENV`. A plain `bash -c`, a script and a `bash -l` script get it too.
    #[cfg(unix)]
    #[test]
    fn every_bash_keeps_the_shims_first() {
        let bashes = bashes();
        if bashes.is_empty() {
            eprintln!("no bash here: skipped");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".bash_profile"),
            "export PATH=/mingw64/bin:/opt/first:$PATH\nSEEN=profile\n",
        )
        .unwrap();
        let own = tmp.path().join("own env.sh");
        std::fs::write(&own, "SEEN=${SEEN:-none},own\n").unwrap();
        let script = tmp.path().join("deploy.sh");
        std::fs::write(&script, "printf '%s|%s' \"$PATH\" \"$SEEN\"\n").unwrap();
        let run_dir = tmp.path().join("run [1]");
        std::fs::create_dir_all(&run_dir).unwrap();
        let bin = run_dir.join("it's bin").display().to_string();
        let mut env = vec![(BASH_ENV.to_string(), own.display().to_string())];
        set_bash_env(&mut env, &run_dir, &bin, false).unwrap();
        assert_eq!(
            env.iter().filter(|(k, _)| k == BASH_ENV).count(),
            1,
            "{env:?}"
        );
        assert!(env.contains(&(ENV_USER_BASH_ENV.into(), own.display().to_string())));
        for bash in &bashes {
            let start = |args: &[&str]| {
                let out = Command::new(bash)
                    .args(args)
                    .env_clear()
                    .env("HOME", &home)
                    .env("PATH", format!("{bin}:/usr/bin:/bin:{bin}"))
                    .envs(env.iter().map(|(k, v)| (k, v)))
                    .output()
                    .unwrap();
                assert!(out.status.success(), "{bash:?} {args:?}: {out:?}");
                String::from_utf8_lossy(&out.stdout).to_string()
            };
            let line = r#"printf '%s|%s' "$PATH" "$SEEN""#;
            let script = script.display().to_string();
            for (args, seen) in [
                (vec!["-lc", line], "profile,own"),
                (vec!["-c", line], "none,own"),
                (vec![&script[..]], "none,own"),
                (vec!["-l", &script[..]], "profile,own"),
            ] {
                let out = start(&args);
                let (path, got) = out.split_once('|').unwrap();
                let path: Vec<&str> = path.split(':').collect();
                assert_eq!(path[0], bin, "{bash:?} {args:?}: {out}");
                assert_eq!(
                    path.iter().filter(|p| **p == bin).count(),
                    1,
                    "{bash:?} {args:?}: {out}"
                );
                if args[0] == "-l" || args[0] == "-lc" {
                    assert!(path.contains(&"/mingw64/bin"), "{bash:?} {args:?}: {out}");
                }
                assert_eq!(got, seen, "{bash:?} {args:?}: {out}");
            }
        }
    }

    /// Codex captures the user's bash with `bash -lc '<capture>'`, whose capture reads
    /// `~/.bashrc` only while `BASH_ENV` is empty (codex-cli 0.159.0). The run's file reads it
    /// in its place, once per process tree, and only with no own `BASH_ENV`; a Claude Code run
    /// (no `read_bashrc`) leaves it alone as before.
    #[cfg(unix)]
    #[test]
    fn a_codex_bash_capture_still_reads_the_bashrc() {
        let bashes = bashes();
        if bashes.is_empty() {
            eprintln!("no bash here: skipped");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".bash_profile"), "export PATH=/opt/first:$PATH\n").unwrap();
        std::fs::write(
            home.join(".bashrc"),
            "export READS=\"${READS:-}rc\"\nexport PATH=/opt/rc:$PATH\nalias ll='ls -l'\n",
        )
        .unwrap();
        let bin = tmp.path().join("bin").display().to_string();
        // Codex's capture: reads the bashrc itself only with no BASH_ENV, then prints the
        // shell's state (here: what the commands it runs later would see).
        let capture = r#"if [ -z "${BASH_ENV-}" ] && [ -n "${HOME-}" ] && [ -r "$HOME/.bashrc" ]; then . "$HOME/.bashrc"; fi
printf '%s|%s|%s|' "$PATH" "$READS" "$(alias ll 2>/dev/null)"
bash -lc 'printf "%s|%s" "$PATH" "$READS"'"#;
        for bash in &bashes {
            for codex in [true, false] {
                let run_dir = tmp.path().join(format!("run-{codex}"));
                std::fs::create_dir_all(&run_dir).unwrap();
                let mut env = vec![];
                set_bash_env(&mut env, &run_dir, &bin, codex).unwrap();
                let out = Command::new(bash)
                    .args(["-lc", capture])
                    .env_clear()
                    .env("HOME", &home)
                    .env(
                        "PATH",
                        format!("{}:/usr/bin:/bin", bash.parent().unwrap().display()),
                    )
                    .envs(env.iter().map(|(k, v)| (k, v)))
                    .output()
                    .unwrap();
                assert!(out.status.success(), "{bash:?}: {out:?}");
                let out = String::from_utf8_lossy(&out.stdout).to_string();
                let parts: Vec<&str> = out.split('|').collect();
                assert_eq!(parts.len(), 5, "{bash:?} {codex}: {out}");
                for path in [parts[0], parts[3]] {
                    assert!(
                        path.starts_with(&format!("{bin}:")),
                        "{bash:?} {codex}: {out}"
                    );
                }
                if codex {
                    assert!(parts[0].contains("/opt/rc"), "{bash:?}: {out}");
                    assert_eq!(parts[1], "rc", "{bash:?}: {out}");
                    assert!(parts[2].contains("ls -l"), "{bash:?}: {out}");
                    // The bash the capture starts inherits what the bashrc exported and does
                    // not read it a second time.
                    assert_eq!(parts[4], "rc", "{bash:?}: {out}");
                } else {
                    assert_eq!(parts[1], "", "{bash:?}: {out}");
                    assert_eq!(parts[4], "", "{bash:?}: {out}");
                }
            }
        }
    }

    /// Bash expands `BASH_ENV` before it reads it, so the user's own `$HOME/.bashenv` still
    /// counts once the run's file reads it.
    #[cfg(unix)]
    #[test]
    fn the_users_own_bash_env_is_expanded_as_bash_does() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("my home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".bashenv"), "OWN=yes\n").unwrap();
        let run_dir = tmp.path().join("run");
        std::fs::create_dir_all(&run_dir).unwrap();
        let bin = run_dir.join("bin").display().to_string();
        let mut env = vec![(BASH_ENV.to_string(), "$HOME/.bashenv".to_string())];
        set_bash_env(&mut env, &run_dir, &bin, true).unwrap();
        let out = Command::new("bash")
            .args(["-c", r#"printf '%s|%s' "${OWN:-no}" "${PATH%%:*}""#])
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .envs(env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("yes|{bin}"),
            "{out:?}"
        );
    }

    #[test]
    fn windows_paths_take_the_msys_form_of_git_bash() {
        assert_eq!(msys_path(r"C:\Users\a b\run\bin"), "/c/Users/a b/run/bin");
        assert_eq!(msys_path(r"\\?\D:\r\bin"), "/d/r/bin");
        assert_eq!(msys_path(r"\\host\share\bin"), "//host/share/bin");
        assert_eq!(msys_path("/already/posix"), "/already/posix");
    }

    #[cfg(unix)]
    #[test]
    fn the_claude_env_file_puts_the_shims_first_again() {
        let tmp = tempfile::tempdir().unwrap();
        let f = claude_env_file(tmp.path(), "/r/it's/bin").unwrap();
        let out = Command::new("sh")
            .arg("-c")
            .arg(format!(". '{f}'; printf %s \"$PATH\""))
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "/r/it's/bin:/usr/bin:/bin"
        );
    }
}
