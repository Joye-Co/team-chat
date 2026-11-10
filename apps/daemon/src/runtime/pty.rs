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

/// Windows: the program runs in a pseudoconsole ([`crate::winpty`]) inside a Job Object. The
/// screen it draws arrives as text with escape codes, which are removed like on Unix; where a
/// program moves the cursor instead of writing a line break, the text of two lines runs
/// together. Cancel types Ctrl+C, then ends the job after 3 s.
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
    if ext == "cmd" || ext == "bat" {
        // A batch file runs in cmd.exe, which would read the prompt as commands.
        return Outcome::failed(format!(
            "cannot start {command}: {} is a batch file; the pty kind needs the program itself",
            launch.program.display()
        ));
    }
    let mut all_args = launch.args.clone();
    all_args.extend(args);
    let mut env = spec.env.clone();
    env.retain(|(k, _)| !k.eq_ignore_ascii_case("TERM"));
    env.push(("TERM".into(), "dumb".into()));
    let (mut pty, mut writer, mut reader) =
        match crate::winpty::spawn(&launch.program, &all_args, &spec.cwd, &env, (200, 50)) {
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
    let waiter = pty.waiter();
    let mut exited = tokio::task::spawn_blocking(move || waiter.wait());
    let mut out = String::new();
    let mut pending = Vec::<u8>::new();
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
        let clean = strip_ansi(&text);
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
    }
}
