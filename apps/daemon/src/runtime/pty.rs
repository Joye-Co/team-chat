//! Plain PTY fallback for CLIs without a structured protocol.
//!
//! `{prompt}` in the configured args is replaced by the prompt; without it, the prompt is
//! typed into the terminal followed by a newline. Output (ANSI codes removed) streams as one
//! message item `out`. Follow-up input is typed into the terminal. The exit code decides the
//! status; the result text is the end of the output.

use super::*;
use crate::proto::{RunEvent, truncate};
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub struct PtyRuntime;

impl Runtime for PtyRuntime {
    fn start(
        &self,
        spec: RunSpec,
        emit: Emit,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Outcome> {
        Box::pin(run(spec, emit, control))
    }
}

/// Remove ANSI escape sequences and carriage returns.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\x1b' => match it.peek() {
                Some('[') => {
                    it.next();
                    while let Some(&n) = it.peek() {
                        it.next();
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    it.next();
                    while let Some(n) = it.next() {
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            it.next();
                            break;
                        }
                    }
                }
                _ => {
                    it.next();
                }
            },
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(unix)]
fn open_pty() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut master = -1;
    let mut slave = -1;
    let mut ws = libc::winsize {
        ws_row: 50,
        ws_col: 200,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: openpty writes two fds; null name and termios are allowed.
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            // A raw pointer: Linux takes `*const winsize`, macOS `*mut winsize`.
            &raw mut ws,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: both fds are fresh and owned by us.
    Ok(unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) })
}

const KEEP: usize = 256 * 1024;

/// What a Windows pseudoconsole sends, turned back into the lines the program wrote.
///
/// A Unix pty passes the program's bytes on: a program that prints lines sends lines. A
/// Windows pseudoconsole keeps a screen and sends what changed on it: a line break arrives
/// as a line feed, or as "move the cursor to row 7"; a run of spaces as "move the cursor 12
/// to the right". With the escape codes merely removed, the words of two lines run together.
/// This follows the cursor: a move to another row is a line break, a move to the right is
/// spaces, everything else that is not text is dropped. A program that prints lines then
/// gives the same text on Windows as on Unix. What cannot be the same: a program that draws
/// (moves up and writes over what it wrote) gives its drawing steps as lines, where a Unix
/// pty gives the same steps without the line breaks; neither is readable text.
///
/// The state is kept between chunks: an escape code can be cut anywhere.
#[derive(Debug)]
pub struct ConsoleText {
    /// The cursor's row, from 1; `rows` at most (below it the screen scrolls).
    row: u32,
    rows: u32,
    /// How wide the screen is, and how many characters the current line of the output has:
    /// a move to the right never goes past the screen's edge.
    cols: u32,
    col: u32,
    state: ConsoleState,
    /// Whether anything was written on the current line of the output.
    line_open: bool,
}

#[derive(Debug)]
enum ConsoleState {
    Text,
    /// After ESC.
    Escape,
    /// After ESC and a character that takes one more (`ESC ( B`).
    EscapeOne,
    /// After `ESC [`: the parameters so far.
    Csi(String),
    /// After `ESC ]`, until BEL or `ESC \`.
    Osc,
    OscEscape,
}

impl ConsoleText {
    pub fn new(cols: u32, rows: u32) -> ConsoleText {
        ConsoleText {
            row: 1,
            rows: rows.max(1),
            cols: cols.max(1),
            col: 0,
            state: ConsoleState::Text,
            line_open: false,
        }
    }

    fn line_break(&mut self, out: &mut String) {
        out.push('\n');
        self.line_open = false;
        self.col = 0;
    }

    /// The cursor goes to `row`: on another row than the one the text is on, what comes next
    /// is another line.
    fn go_to_row(&mut self, row: u32, out: &mut String) {
        let row = row.clamp(1, self.rows);
        if row != self.row && self.line_open {
            self.line_break(out);
        }
        self.row = row;
    }

    fn csi(&mut self, params: &str, end: char, out: &mut String) {
        // Private modes (`?25h`) and the like have no part in the text.
        if params.starts_with(['?', '>', '=', '<']) {
            return;
        }
        let mut nums = params.split(';').map(|p| p.parse::<u32>().ok());
        let first = nums.next().flatten();
        let n = first.unwrap_or(1).max(1);
        match end {
            'H' | 'f' | 'd' => self.go_to_row(first.unwrap_or(1), out),
            'B' | 'E' => self.go_to_row(self.row.saturating_add(n), out),
            'A' | 'F' => self.go_to_row(self.row.saturating_sub(n).max(1), out),
            'C' => {
                // To the screen's edge at most: a code of a few bytes must not become
                // thousands of spaces.
                let n = n.min(self.cols.saturating_sub(self.col));
                out.extend(std::iter::repeat_n(' ', n as usize));
                self.col += n;
                self.line_open |= n > 0;
            }
            _ => {}
        }
    }

    /// The text of the next chunk of the console's output.
    pub fn feed(&mut self, chunk: &str) -> String {
        let mut out = String::with_capacity(chunk.len());
        for c in chunk.chars() {
            let state = std::mem::replace(&mut self.state, ConsoleState::Text);
            self.state = match state {
                ConsoleState::Text => match c {
                    '\x1b' => ConsoleState::Escape,
                    '\r' => ConsoleState::Text,
                    '\n' => {
                        self.line_break(&mut out);
                        self.row = (self.row + 1).min(self.rows);
                        ConsoleState::Text
                    }
                    // Bell, backspace and the other controls a screen acts on.
                    c if c.is_control() && c != '\t' => ConsoleState::Text,
                    c => {
                        out.push(c);
                        self.line_open = true;
                        self.col = self.col.saturating_add(1);
                        ConsoleState::Text
                    }
                },
                ConsoleState::Escape => match c {
                    '[' => ConsoleState::Csi(String::new()),
                    ']' => ConsoleState::Osc,
                    '(' | ')' | '*' | '+' | '#' | '%' => ConsoleState::EscapeOne,
                    _ => ConsoleState::Text,
                },
                ConsoleState::EscapeOne => ConsoleState::Text,
                ConsoleState::Csi(mut params) => {
                    if ('@'..='~').contains(&c) {
                        self.csi(&params, c, &mut out);
                        ConsoleState::Text
                    } else if params.len() < 64 {
                        params.push(c);
                        ConsoleState::Csi(params)
                    } else {
                        // Not a code this was written by a console.
                        ConsoleState::Text
                    }
                }
                ConsoleState::Osc => match c {
                    '\x07' => ConsoleState::Text,
                    '\x1b' => ConsoleState::OscEscape,
                    _ => ConsoleState::Osc,
                },
                ConsoleState::OscEscape => ConsoleState::Text,
            };
        }
        out
    }
}

/// The command line that starts the batch file `script` with `args` in `cmd.exe` (`cmd` is
/// its path), each argument arriving in the script as it was given. A batch file's line is
/// read by `cmd.exe`, which has its own rules: an argument is put in quotes unless it is made
/// of characters known to be plain, a quote inside is doubled, and a `%` is written so that no
/// variable is put in its place. An argument with a line break cannot be written (the line
/// would end there): that is an error. The same rules as Rust's standard library, which the
/// other runtimes start their batch files with.
pub fn batch_line(cmd: &str, script: &str, args: &[String]) -> Result<String, String> {
    if script.contains('"') || script.ends_with('\\') {
        return Err(format!(
            "{script} cannot be started: a quote or a trailing backslash in its name"
        ));
    }
    // The whole command is in one more pair of quotes, closed at the end. `/e:ON` turns on
    // what the `%` spelling below needs, `/v:OFF` keeps `!name!` as text, `/d` skips AutoRun.
    let mut line = format!("\"{cmd}\" /e:ON /v:OFF /d /c \"\"{script}\"");
    for arg in args {
        if arg.contains(['\r', '\n', '\0']) {
            return Err(
                "an argument with a line break cannot be given to a batch file: put the \
program the batch file starts in the runtime's `command`"
                    .into(),
            );
        }
        line.push(' ');
        const PLAIN: &str = r"#$*+-./:?@\_";
        let quote = arg.is_empty()
            || arg.ends_with('\\')
            || arg.chars().any(|c| {
                (c.is_ascii() && !(c.is_ascii_alphanumeric() || PLAIN.contains(c)))
                    || c.is_control()
            });
        if quote {
            line.push('"');
        }
        let mut backslashes = 0;
        for c in arg.chars() {
            if c == '\\' {
                backslashes += 1;
            } else {
                if c == '"' {
                    // The backslashes before a quote are doubled, and so is the quote.
                    line.extend(std::iter::repeat_n('\\', backslashes));
                    line.push('"');
                } else if c == '%' {
                    // `%%cd:~,%` is nothing followed by `%`: no `%name%` is ever read.
                    line.push_str("%%cd:~,");
                }
                backslashes = 0;
            }
            line.push(c);
        }
        if quote {
            line.extend(std::iter::repeat_n('\\', backslashes));
            line.push('"');
        }
    }
    line.push('"');
    Ok(line)
}

/// Windows: the program runs in a pseudoconsole ([`crate::winpty`]) inside a Job Object. What
/// the console sends is turned back into the lines the program wrote ([`ConsoleText`]).
/// Cancel types Ctrl+C, then ends the job after 3 s.
#[cfg(windows)]
async fn run(spec: RunSpec, emit: Emit, mut control: mpsc::UnboundedReceiver<Control>) -> Outcome {
    use std::io::{Read, Write};
    let command = spec.cfg.command();
    let has_placeholder = spec.cfg.args.iter().any(|a| a.contains("{prompt}"));
    let args: Vec<String> = spec
        .cfg
        .args
        .iter()
        .map(|a| a.replace("{prompt}", &spec.prompt))
        .collect();
    let path = spec
        .env
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
        .map(|(_, v)| std::ffi::OsString::from(v));
    let found = crate::health::which_in(&command, path.as_deref())
        .or_else(|| crate::health::which(&command))
        .unwrap_or_else(|| PathBuf::from(&command));
    let launch = crate::health::launch(&found);
    let ext = launch
        .program
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mut all_args = launch.args.clone();
    all_args.extend(args);
    // A batch file that is not an npm wrapper (those are resolved to what they start, see
    // `health::launch`) runs in `cmd.exe`, with its arguments written the way `cmd.exe` reads
    // them. What is typed into the console goes to the programs the batch file starts.
    let line = if ext == "cmd" || ext == "bat" {
        let cmd = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("System32")
            .join("cmd.exe");
        match batch_line(
            &cmd.to_string_lossy(),
            &launch.program.to_string_lossy(),
            &all_args,
        ) {
            Ok(line) => line,
            Err(e) => return Outcome::failed(format!("cannot start {command}: {e}")),
        }
    } else {
        crate::winpty::command_line(&launch.program, &all_args)
    };
    let mut env = spec.env.clone();
    env.retain(|(k, _)| !k.eq_ignore_ascii_case("TERM"));
    env.push(("TERM".into(), "dumb".into()));
    const COLS: i16 = 200;
    const ROWS: i16 = 50;
    let (mut pty, mut writer, mut reader) =
        match crate::winpty::spawn_line(&line, &spec.cwd, &env, (COLS, ROWS)) {
            Ok(p) => p,
            Err(e) => return Outcome::failed(format!("cannot start {command}: {e}")),
        };
    crate::workspace::record_process(&spec.run_dir, pty.pid as i32, &command);
    // 接管 (pause) finds the job by the run folder.
    let _tracked = crate::executor::local::track(&spec.run_dir, pty.job.clone());
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let raw_path = spec.run_dir.join("pty.log");
    std::thread::spawn(move || {
        let mut raw = std::fs::File::create(raw_path).ok();
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Some(r) = raw.as_mut() {
                        let _ = r.write_all(&buf[..n]);
                    }
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    // Enter is a carriage return on a console.
    if !has_placeholder {
        let _ = writer.write_all(format!("{}\r", spec.prompt).as_bytes());
    }
    let _ = emit.send(RunEvent::TurnStarted {
        turn_id: "t1".into(),
    });
    let waiter = match pty.waiter() {
        Ok(w) => w,
        Err(e) => return Outcome::failed(format!("cannot watch {command}: {e}")),
    };
    let mut exited = tokio::task::spawn_blocking(move || waiter.wait());
    let mut out = String::new();
    let mut pending = Vec::<u8>::new();
    let mut console = ConsoleText::new(COLS as u32, ROWS as u32);
    let mut take = |b: &[u8], out: &mut String| {
        pending.extend_from_slice(b);
        // Keep an incomplete UTF-8 tail for the next chunk.
        let cut = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => pending.len(),
        };
        let text = String::from_utf8_lossy(&pending[..cut]).into_owned();
        pending.drain(..cut);
        let clean = console.feed(&text);
        if !clean.is_empty() {
            out.push_str(&clean);
            if out.len() > KEEP * 2 {
                let mut start = out.len() - KEEP;
                while !out.is_char_boundary(start) {
                    start += 1;
                }
                out.drain(..start);
            }
            let _ = emit.send(RunEvent::MessageDelta {
                item_id: "out".into(),
                turn_id: Some("t1".into()),
                text: clean,
            });
        }
    };
    let mut control_open = true;
    let mut cancelled = false;
    let code: Option<i64> = loop {
        tokio::select! {
            Some(b) = rx.recv() => take(&b, &mut out),
            code = &mut exited => break code.ok().flatten(),
            c = control.recv(), if control_open => match c {
                None => control_open = false,
                Some(Control::Input { input_id, text }) => {
                    let _ = emit.send(RunEvent::InputAccepted { input_id });
                    let _ = writer.write_all(format!("{text}\r").as_bytes());
                }
                Some(Control::Approval { .. }) => {}
                Some(Control::Cancel) => {
                    cancelled = true;
                    // Ctrl+C, typed.
                    let _ = writer.write_all(b"\x03");
                    let code = match tokio::time::timeout(Duration::from_secs(3), &mut exited).await {
                        Ok(code) => code.ok().flatten(),
                        Err(_) => {
                            pty.job.terminate();
                            (&mut exited).await.ok().flatten()
                        }
                    };
                    break code;
                }
            },
        }
    };
    // What the program left running goes; then the console closes, which ends the output
    // stream once the reader thread has taken the rest of it.
    pty.job.terminate();
    drop(writer);
    let closing = tokio::task::spawn_blocking(move || {
        pty.close_console();
        pty
    });
    let rest = async {
        while let Some(b) = rx.recv().await {
            take(&b, &mut out);
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(5), rest).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), closing).await;
    conclude(&spec, &emit, &out, code, cancelled)
}

#[cfg(unix)]
async fn run(spec: RunSpec, emit: Emit, mut control: mpsc::UnboundedReceiver<Control>) -> Outcome {
    let program = spec.cfg.command();
    let has_placeholder = spec.cfg.args.iter().any(|a| a.contains("{prompt}"));
    let args: Vec<String> = spec
        .cfg
        .args
        .iter()
        .map(|a| a.replace("{prompt}", &spec.prompt))
        .collect();
    let (master, slave) = match open_pty() {
        Ok(p) => p,
        Err(e) => return Outcome::failed(format!("openpty: {e}")),
    };
    let child = {
        let mut cmd = tokio::process::Command::new(&program);
        let dup = |fd: &OwnedFd| fd.try_clone().map(std::process::Stdio::from);
        let (Ok(i), Ok(o), Ok(e)) = (dup(&slave), dup(&slave), dup(&slave)) else {
            return Outcome::failed("cannot duplicate pty fd");
        };
        cmd.args(&args)
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .env("TERM", "dumb")
            .stdin(i)
            .stdout(o)
            .stderr(e)
            .kill_on_drop(true);
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                Ok(())
            });
        }
        cmd.spawn()
    };
    drop(slave);
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Outcome::failed(format!("cannot start {program}: {e}")),
    };
    let pgid = child.id().map(|p| p as i32).unwrap_or(0);
    crate::workspace::record_process(&spec.run_dir, pgid, &program);
    let kill = |sig: i32| {
        if pgid > 0 {
            // SAFETY: plain syscall addressing the child's session/process group.
            unsafe {
                libc::kill(-pgid, sig);
            }
        }
    };
    let mut writer = match master.try_clone() {
        Ok(fd) => std::fs::File::from(fd),
        Err(e) => return Outcome::failed(format!("pty: {e}")),
    };
    let reader_fd = master;
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let raw_path = spec.run_dir.join("pty.log");
    std::thread::spawn(move || {
        let mut f = std::fs::File::from(reader_fd);
        let mut raw = std::fs::File::create(raw_path).ok();
        let mut buf = [0u8; 8192];
        loop {
            match f.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Some(r) = raw.as_mut() {
                        let _ = r.write_all(&buf[..n]);
                    }
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    if !has_placeholder {
        let _ = writer.write_all(format!("{}\n", spec.prompt).as_bytes());
    }
    let _ = emit.send(RunEvent::TurnStarted {
        turn_id: "t1".into(),
    });
    let mut out = String::new();
    let mut pending = Vec::<u8>::new();
    let mut control_open = true;
    let mut cancelled = false;
    let mut exit: Option<std::process::ExitStatus> = None;
    let mut reader_open = true;
    while reader_open || exit.is_none() {
        tokio::select! {
            chunk = rx.recv(), if reader_open => match chunk {
                Some(b) => {
                    pending.extend_from_slice(&b);
                    // Keep an incomplete UTF-8 tail for the next chunk.
                    let cut = match std::str::from_utf8(&pending) {
                        Ok(_) => pending.len(),
                        Err(e) if e.error_len().is_none() => e.valid_up_to(),
                        Err(_) => pending.len(),
                    };
                    let text = String::from_utf8_lossy(&pending[..cut]).into_owned();
                    pending.drain(..cut);
                    let clean = strip_ansi(&text);
                    if !clean.is_empty() {
                        out.push_str(&clean);
                        if out.len() > KEEP * 2 {
                            let mut start = out.len() - KEEP;
                            while !out.is_char_boundary(start) { start += 1; }
                            out.drain(..start);
                        }
                        let _ = emit.send(RunEvent::MessageDelta { item_id: "out".into(), turn_id: Some("t1".into()), text: clean });
                    }
                }
                None => reader_open = false,
            },
            st = child.wait(), if exit.is_none() => {
                exit = st.ok();
                if exit.is_none() { break; }
                // The reader ends when the last slave fd closes; do not wait on
                // background children that keep it open.
                if reader_open {
                    let deadline = tokio::time::sleep(Duration::from_secs(2));
                    tokio::pin!(deadline);
                    loop {
                        tokio::select! {
                            c = rx.recv() => match c {
                                Some(b) => { let t = strip_ansi(&String::from_utf8_lossy(&b)); out.push_str(&t);
                                    if !t.is_empty() { let _ = emit.send(RunEvent::MessageDelta { item_id: "out".into(), turn_id: Some("t1".into()), text: t }); } }
                                None => break,
                            },
                            _ = &mut deadline => break,
                        }
                    }
                    reader_open = false;
                }
            },
            c = control.recv(), if control_open => match c {
                None => control_open = false,
                Some(Control::Input { input_id, text }) => {
                    let _ = emit.send(RunEvent::InputAccepted { input_id });
                    let _ = writer.write_all(format!("{text}\n").as_bytes());
                }
                Some(Control::Approval { .. }) => {}
                Some(Control::Cancel) => {
                    cancelled = true;
                    // Jobs the shell started in groups of their own; only while the shell is
                    // not reaped (its pid is still ours).
                    let tools = if exit.is_none() { crate::proctree::run_targets(pgid, &spec.run_dir) } else { crate::proctree::orphans(&spec.run_dir) };
                    kill(libc::SIGTERM);
                    crate::proctree::signal(&tools, libc::SIGTERM);
                    crate::proctree::signal(&tools, libc::SIGCONT);
                    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                        Ok(Ok(s)) => exit = Some(s),
                        _ => { kill(libc::SIGKILL); exit = child.wait().await.ok(); }
                    }
                    crate::proctree::signal(&tools, libc::SIGKILL);
                    break;
                }
            },
        }
    }
    kill(libc::SIGKILL);
    let code = exit.and_then(|s| s.code()).map(i64::from);
    let _ = writer.as_raw_fd();
    conclude(&spec, &emit, &out, code, cancelled)
}

/// The end of a run: the whole output as the message, the turn's status and the outcome.
fn conclude(spec: &RunSpec, emit: &Emit, out: &str, code: Option<i64>, cancelled: bool) -> Outcome {
    let text = out.trim().to_string();
    let tail = {
        let mut start = text.len().saturating_sub(4096);
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text[start..].to_string()
    };
    let _ = emit.send(RunEvent::Message {
        item_id: "out".into(),
        turn_id: Some("t1".into()),
        role: "assistant".into(),
        text: truncate(&text, 64 * 1024),
    });
    let status = if cancelled {
        Status::Cancelled
    } else if code == Some(0) {
        Status::Succeeded
    } else {
        Status::Failed
    };
    let _ = emit.send(RunEvent::TurnCompleted {
        turn_id: "t1".into(),
        status: match status {
            Status::Succeeded => "completed",
            Status::Cancelled => "interrupted",
            Status::Failed => "failed",
        }
        .into(),
        error: None,
    });
    Outcome {
        status,
        exit_code: code,
        result_text: (!tail.is_empty()).then_some(tail),
        error: (status == Status::Failed).then(|| format!("exit code {code:?}")),
        session_id: None,
        session_file: Some(spec.run_dir.join("pty.log")),
        usage: None,
        failure: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(chunks: &[&str]) -> String {
        let mut c = ConsoleText::new(200, 50);
        chunks.iter().map(|x| c.feed(x)).collect()
    }

    /// What a pseudoconsole sends for a program that prints lines is those lines: its line
    /// feeds, its cursor moves to the next row, its "move right" for spaces; the screen
    /// set-up, colors, the window title and the cursor's show and hide are not text.
    #[test]
    fn console_output_becomes_the_lines_the_program_wrote() {
        // The start of every session, then two lines with a plain line feed.
        assert_eq!(
            lines(&[
                "\x1b[?25l\x1b[2J\x1b[m\x1b[Hstart:hi\r\ngot:abc\r\n\x1b]0;C:\\Windows\\cmd.exe\x07\x1b[?25h"
            ]),
            "start:hi\ngot:abc\n"
        );
        // A line break as a move to the next row, with and without a column.
        assert_eq!(lines(&["one\x1b[2;1Htwo\x1b[3Hthree"]), "one\ntwo\nthree");
        assert_eq!(lines(&["one\x1b[Etwo\x1b[2Bthree"]), "one\ntwo\nthree");
        // A move within the row is not a line break; a move right is spaces.
        assert_eq!(lines(&["a\x1b[1;5Hb\x1b[3Cc\x1b[Cd"]), "ab   c d");
        // A move to a row after a line feed already made the break: one break, not two.
        assert_eq!(lines(&["one\r\n\x1b[2;1Htwo"]), "one\ntwo");
        // Colors, erasing, modes and a title in the middle of a line leave the text whole.
        assert_eq!(
            lines(&["\x1b[32mgreen\x1b[0m \x1b[Kword\x1b[?1004h\x1b]2;title\x1b\\!\x1b(B"]),
            "green word!"
        );
        // At the bottom the screen scrolls: the cursor stays on the last row, each line feed
        // is still one line.
        let mut c = ConsoleText::new(200, 3);
        let text: String = ["1\r\n", "2\r\n", "3\r\n", "4\r\n", "\x1b[3;1H5\r\n", "6"]
            .iter()
            .map(|x| c.feed(x))
            .collect();
        assert_eq!(text, "1\n2\n3\n4\n5\n6");
        // A move to the right stops at the screen's edge, however far the code says and
        // however often it comes: a line never grows past the width by moves.
        let far = lines(&["ab\x1b[999C\x1b[999C\x1b[5Cc\r\nd\x1b[3Ce"]);
        assert_eq!(far, format!("ab{}c\nd   e", " ".repeat(198)));
        let flood: String = "\x1b[999C".repeat(10_000);
        assert!(lines(&[&flood]).len() <= 200);
        // Tabs stay, other controls go.
        assert_eq!(lines(&["a\tb\x07\x08c"]), "a\tbc");
    }

    /// An escape code cut anywhere between two chunks is still one code.
    #[test]
    fn console_codes_may_be_cut_between_chunks() {
        let whole =
            "\x1b[?25l\x1b[2J\x1b[Hfirst line\x1b[2;1Hsecond\x1b[5Cline\x1b]0;a title\x07 end\r\n";
        let want = lines(&[whole]);
        assert_eq!(want, "first line\nsecond     line end\n");
        for cut in 1..whole.len() {
            if whole.is_char_boundary(cut) {
                assert_eq!(lines(&[&whole[..cut], &whole[cut..]]), want, "cut at {cut}");
            }
        }
        let each: Vec<String> = whole.chars().map(String::from).collect();
        let each: Vec<&str> = each.iter().map(String::as_str).collect();
        assert_eq!(lines(&each), want);
    }

    /// A batch file's command line: the arguments in the spelling `cmd.exe` reads back as
    /// they were given, and none that could end the line or name a variable.
    #[test]
    fn a_batch_file_gets_its_arguments_in_cmd_spelling() {
        let args = |a: &[&str]| -> Vec<String> { a.iter().map(|x| x.to_string()).collect() };
        let cmd = r"C:\Windows\System32\cmd.exe";
        let script = r"C:\Users\li & co\tools\agent.cmd";
        assert_eq!(
            batch_line(cmd, script, &args(&["run", "--fast"])).unwrap(),
            r#""C:\Windows\System32\cmd.exe" /e:ON /v:OFF /d /c ""C:\Users\li & co\tools\agent.cmd" run --fast""#
        );
        assert_eq!(
            batch_line(
                cmd,
                script,
                &args(&[
                    "a b",
                    "",
                    r"C:\dir\",
                    "x&calc",
                    "100%",
                    "say \"hi\"",
                    "%PATH%",
                    "^|<>"
                ])
            )
            .unwrap(),
            concat!(
                r#""C:\Windows\System32\cmd.exe" /e:ON /v:OFF /d /c ""C:\Users\li & co\tools\agent.cmd""#,
                r#" "a b" "" "C:\dir\\" "x&calc" "100%%cd:~,%" "say ""hi""" "%%cd:~,%PATH%%cd:~,%" "^|<>"""#
            )
        );
        for bad in ["two\nlines", "a\rb"] {
            assert!(batch_line(cmd, script, &args(&[bad])).is_err(), "{bad:?}");
        }
        assert!(batch_line(cmd, "bad\"name.cmd", &[]).is_err());
    }
}
