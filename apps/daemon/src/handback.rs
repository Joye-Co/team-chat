//! What a person did while holding a run (接管), told to the CLI when they hand it back
//! (design-takeover 3.5): how long the run was held, which commands ran and how they ended,
//! what changed in the work folder, and the person's words.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

/// Longest hand-back note in characters; command output is cut first.
pub const MAX_NOTE_CHARS: usize = 6000;
/// Lines of a command's output the note keeps.
pub const TAIL_LINES: usize = 5;
/// Files a work folder without git is compared by, at most.
const MAX_SCAN: usize = 20_000;

/// Size and modification time of a file; None when it is not there.
type Stamp = Option<(u64, u128)>;

fn stamp(p: &Path) -> Stamp {
    let m = std::fs::symlink_metadata(p).ok()?;
    let t = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some((m.len(), t))
}

/// The work folder when the hold started.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// The folder is a git work tree.
    git: bool,
    head: Option<String>,
    /// `git status` entries: path → (status code, stamp). Without git: every file → ("", stamp).
    entries: BTreeMap<String, (String, Stamp)>,
}

/// What changed in the work folder while it was held.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub head_from: Option<String>,
    pub head_to: Option<String>,
    /// Changed paths, relative to the work folder, sorted.
    pub files: Vec<String>,
}

async fn git(work: &Path, args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(work)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(30), out)
        .await
        .ok()?
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// `git status --porcelain -z`: path → status code. A rename's old path is listed too.
fn parse_status(z: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut parts = z.split('\0').filter(|s| !s.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = (&entry[..2], &entry[3..]);
        out.insert(path.to_string(), code.to_string());
        if (code.contains('R') || code.contains('C'))
            && let Some(from) = parts.next()
        {
            out.insert(from.to_string(), code.to_string());
        }
    }
    out
}

async fn git_state(work: &Path) -> Option<(Option<String>, BTreeMap<String, (String, Stamp)>)> {
    git(work, &["rev-parse", "--is-inside-work-tree"]).await?;
    let head = git(work, &["rev-parse", "--short", "HEAD"])
        .await
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty());
    let status = git(work, &["status", "--porcelain", "-z"])
        .await
        .unwrap_or_default();
    let entries = parse_status(&status)
        .into_iter()
        .map(|(p, code)| {
            let s = stamp(&work.join(&p));
            (p, (code, s))
        })
        .collect();
    Some((head, entries))
}

/// Every file below `work` (not in `.git`), at most [`MAX_SCAN`].
fn scan(work: &Path) -> BTreeMap<String, (String, Stamp)> {
    let mut out = BTreeMap::new();
    let mut dirs = vec![work.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            if out.len() >= MAX_SCAN {
                return out;
            }
            let p = e.path();
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                if e.file_name() != ".git" {
                    dirs.push(p);
                }
                continue;
            }
            if let Ok(rel) = p.strip_prefix(work) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(rel, (String::new(), stamp(&p)));
            }
        }
    }
    out
}

/// Records the work folder: HEAD and `git status`, or without git each file's modification
/// time.
pub async fn snapshot(work: &Path) -> Snapshot {
    match git_state(work).await {
        Some((head, entries)) => Snapshot {
            git: true,
            head,
            entries,
        },
        None => {
            let w = work.to_path_buf();
            let entries = tokio::task::spawn_blocking(move || scan(&w))
                .await
                .unwrap_or_default();
            Snapshot {
                git: false,
                head: None,
                entries,
            }
        }
    }
}

/// What changed since `before`: paths whose status or file changed, and with git the files of
/// the commits made meanwhile.
pub async fn changes(before: &Snapshot, work: &Path) -> Changes {
    let after = snapshot(work).await;
    let mut files: BTreeSet<String> = BTreeSet::new();
    for (p, v) in &after.entries {
        if before.entries.get(p) != Some(v) {
            files.insert(p.clone());
        }
    }
    for p in before.entries.keys() {
        if !after.entries.contains_key(p) {
            files.insert(p.clone());
        }
    }
    if before.git
        && after.git
        && let (Some(a), Some(b)) = (&before.head, &after.head)
        && a != b
        && let Some(diff) = git(work, &["diff", "--name-only", "-z", a, b]).await
    {
        files.extend(diff.split('\0').filter(|s| !s.is_empty()).map(String::from));
    }
    Changes {
        head_from: before.head.clone(),
        head_to: after.head,
        files: files.into_iter().collect(),
    }
}

/// How one of the person's commands ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    Ok,
    Failed(Option<i64>),
    TimedOut,
    Cancelled,
    /// It did not start.
    Error(String),
}

/// One command the person ran, with the last lines of its output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub command: String,
    pub end: End,
    pub tail: String,
}

fn held_words(held: Duration) -> String {
    let min = held.as_secs() / 60;
    if min == 0 {
        "under a minute".to_string()
    } else {
        format!("{min} min")
    }
}

fn end_words(e: &End) -> String {
    match e {
        End::Ok => "ok".into(),
        End::Failed(Some(c)) => format!("failed (exit {c})"),
        End::Failed(None) => "failed".into(),
        End::TimedOut => "stopped after 10 min".into(),
        End::Cancelled => "stopped by the person".into(),
        End::Error(e) => format!("did not start ({e})"),
    }
}

fn cut_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The last `n` lines of `text`, each indented.
fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|l| format!("   {}", cut_chars(l, 300)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_arguments)]
fn compose(
    by: &str,
    held: Duration,
    work: &Path,
    ran: &[Ran],
    ch: &Changes,
    words: Option<&str>,
    lines: usize,
    command_chars: usize,
) -> String {
    let who = if by.is_empty() {
        "A person".to_string()
    } else {
        format!("Member {by}")
    };
    let mut s = format!(
        "[team-chat] A person held your computer. {who} paused you for {} and ran {} in {}",
        held_words(held),
        match ran.len() {
            0 => "no commands".to_string(),
            1 => "1 command".to_string(),
            n => format!("{n} commands"),
        },
        work.display()
    );
    s.push_str(if ran.is_empty() { ".\n" } else { ":\n" });
    for (i, r) in ran.iter().enumerate() {
        s.push_str(&format!(
            "{}. `{}` → {}\n",
            i + 1,
            cut_chars(&r.command, command_chars),
            end_words(&r.end)
        ));
        if lines > 0 && r.end != End::Ok {
            let t = tail_lines(&r.tail, lines);
            if !t.is_empty() {
                s.push_str(&t);
                s.push('\n');
            }
        }
    }
    let mut what = vec![];
    if let (Some(a), Some(b)) = (&ch.head_from, &ch.head_to)
        && a != b
    {
        what.push(format!("HEAD {a} → {b}"));
    }
    if !ch.files.is_empty() {
        let shown: Vec<&str> = ch.files.iter().take(20).map(String::as_str).collect();
        let more = ch.files.len() - shown.len();
        let mut list = shown.join(", ");
        if more > 0 {
            list.push_str(&format!(" and {more} more"));
        }
        what.push(format!("changed: {list}"));
    }
    s.push_str(&format!(
        "Work folder changes: {}\n",
        if what.is_empty() {
            "none".to_string()
        } else {
            what.join("; ")
        }
    ));
    if let Some(w) = words.map(str::trim).filter(|w| !w.is_empty()) {
        let name = if by.is_empty() {
            "The person".to_string()
        } else {
            format!("Member {by}")
        };
        s.push_str(&format!("{name}'s note: {w}\n"));
    }
    s.push_str("Check these changes before you continue; do not redo what the person did.");
    s
}

/// The note the CLI gets at hand-back, at most [`MAX_NOTE_CHARS`]: output lines go first, then
/// long commands are cut, then the list itself.
pub fn note(
    by: &str,
    held: Duration,
    work: &Path,
    ran: &[Ran],
    ch: &Changes,
    words: Option<&str>,
) -> String {
    for (lines, chars) in [(TAIL_LINES, 500), (2, 500), (0, 500), (0, 120), (0, 40)] {
        let s = compose(by, held, work, ran, ch, words, lines, chars);
        if s.chars().count() <= MAX_NOTE_CHARS {
            return s;
        }
    }
    // Still too long (very many commands): keep the head and the closing instruction.
    let s = compose(by, held, work, ran, ch, words, 0, 40);
    let last = "\nCheck these changes before you continue; do not redo what the person did.";
    let mut head: String = s
        .chars()
        .take(MAX_NOTE_CHARS - last.chars().count() - 1)
        .collect();
    head.push('…');
    head.push_str(last);
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(cmd: &str, end: End, tail: &str) -> Ran {
        Ran {
            command: cmd.into(),
            end,
            tail: tail.into(),
        }
    }

    #[test]
    fn the_note_has_the_fixed_form() {
        let ch = Changes {
            head_from: Some("a1b2c3".into()),
            head_to: Some("d4e5f6".into()),
            files: vec![".env.example".into()],
        };
        let n = note(
            "U1",
            Duration::from_secs(12 * 60 + 5),
            Path::new("/w/lines/k/work"),
            &[
                ran("npm test", End::Failed(Some(1)), "a\nb\nc\nd\ne\nf\ng\n"),
                ran("git commit -am \"fix env\"", End::Ok, "1 file changed"),
                ran("sleep 900", End::TimedOut, ""),
            ],
            &ch,
            Some("I fixed .env"),
        );
        let want = "[team-chat] A person held your computer. Member U1 paused you for 12 min and \
ran 3 commands in /w/lines/k/work:
1. `npm test` → failed (exit 1)
   c
   d
   e
   f
   g
2. `git commit -am \"fix env\"` → ok
3. `sleep 900` → stopped after 10 min
Work folder changes: HEAD a1b2c3 → d4e5f6; changed: .env.example
Member U1's note: I fixed .env
Check these changes before you continue; do not redo what the person did.";
        assert_eq!(n, want);
    }

    #[test]
    fn a_hold_without_commands_or_changes_says_so() {
        let n = note(
            "",
            Duration::from_secs(20),
            Path::new("/w"),
            &[],
            &Changes::default(),
            None,
        );
        assert!(n.contains("paused you for under a minute and ran no commands in /w."));
        assert!(n.contains("Work folder changes: none\n"));
        assert!(!n.contains("note:"));
    }

    #[test]
    fn a_long_note_cuts_output_first_and_stays_under_the_limit() {
        let long = "x".repeat(400);
        let tail = (0..10).map(|_| long.clone()).collect::<Vec<_>>().join("\n");
        let many: Vec<Ran> = (0..20)
            .map(|i| ran(&format!("make t{i}"), End::Failed(Some(2)), &tail))
            .collect();
        let n = note(
            "U1",
            Duration::from_secs(60),
            Path::new("/w"),
            &many,
            &Changes::default(),
            Some("hi"),
        );
        assert!(n.chars().count() <= MAX_NOTE_CHARS, "{}", n.chars().count());
        // Every command is still listed, and the person's words are kept.
        assert!(n.contains("20. `make t19`"));
        assert!(n.contains("Member U1's note: hi"));
        let huge: Vec<Ran> = (0..400)
            .map(|i| ran(&format!("echo {i} {}", "y".repeat(100)), End::Ok, ""))
            .collect();
        let n = note(
            "U1",
            Duration::from_secs(60),
            Path::new("/w"),
            &huge,
            &Changes::default(),
            None,
        );
        assert!(n.chars().count() <= MAX_NOTE_CHARS);
        assert!(n.ends_with("do not redo what the person did."));
    }

    #[test]
    fn status_parses_renames_and_spaces() {
        let m = parse_status(" M a b.txt\0R  new.rs\0old.rs\0?? x/\0");
        assert_eq!(m.get("a b.txt").map(String::as_str), Some(" M"));
        assert_eq!(m.get("new.rs").map(String::as_str), Some("R "));
        assert_eq!(m.get("old.rs").map(String::as_str), Some("R "));
        assert_eq!(m.get("x/").map(String::as_str), Some("??"));
    }

    #[tokio::test]
    async fn changes_are_found_with_and_without_git() {
        // Without git: a new file and a changed file.
        let tmp = tempfile::tempdir().unwrap();
        let w = tmp.path();
        std::fs::write(w.join("keep.txt"), "a").unwrap();
        std::fs::write(w.join("edit.txt"), "a").unwrap();
        let before = snapshot(w).await;
        std::fs::write(w.join("edit.txt"), "bb").unwrap();
        std::fs::write(w.join("new.txt"), "c").unwrap();
        let ch = changes(&before, w).await;
        assert_eq!(
            ch.files,
            vec!["edit.txt".to_string(), "new.txt".to_string()]
        );
        assert_eq!(ch.head_from, None);

        // With git: an edit to a file the AI member had already changed, and a commit.
        let tmp = tempfile::tempdir().unwrap();
        let w = tmp.path();
        let run = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(w)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "{args:?}");
        };
        run(&["init", "-q"]);
        std::fs::write(w.join("a.txt"), "1").unwrap();
        std::fs::write(w.join("b.txt"), "1").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "one"]);
        std::fs::write(w.join("a.txt"), "2").unwrap();
        let before = snapshot(w).await;
        assert!(before.head.is_some());
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(w.join("a.txt"), "33").unwrap();
        std::fs::write(w.join("b.txt"), "2").unwrap();
        run(&["commit", "-qam", "two"]);
        let ch = changes(&before, w).await;
        assert_ne!(ch.head_from, ch.head_to);
        assert_eq!(ch.files, vec!["a.txt".to_string(), "b.txt".to_string()]);
    }
}
