//! 接管 on the daemon (design-takeover 3.4, 3.8): while a person holds a run its CLI is stopped
//! and their commands run one at a time in the run's work folder. Each is reported as
//! `hand.command`, `hand.output` and `hand.result`; at hand-back the CLI gets a note on what
//! they did ([`crate::handback`]) and the server a `hand.summary`.

use crate::config::Config;
use crate::executor::{Executor, HandChild, HandReq};
use crate::handback::{self, End, Ran};
use crate::proto::RunEvent;
use chat_protocol::takeover::{MAX_HAND_OUTPUT_BYTES, MAX_HANDBACK_FILES, RunHandback};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// A person's command ends after this long.
pub const LIMIT: Duration = Duration::from_secs(600);
/// Output of one command sent as `hand.output`; past it only the last
/// [`MAX_HAND_OUTPUT_BYTES`] come, at the end.
pub const MAX_OUTPUT: usize = 1 << 20;
/// How long output is still read after the command ended (a process it left in the background
/// can keep its pipe open).
const DRAIN: Duration = Duration::from_millis(300);

/// The environment of a person's command on this machine: the daemon's own, without the
/// run's injected values (`chat-run`, the GitHub helper, the gate shims, the app's API keys,
/// the runtime's env), without `gated_env` and `env_passthrough`, and with the daemon's own
/// `PATH` (no run bin folder, so nothing is stopped at a gate).
pub fn host_env(cfg: &Config, run_env: &[(String, String)]) -> Vec<(String, String)> {
    let same = |a: &str, b: &str| {
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let own = |k: &str| {
        crate::run::BASE_ENV
            .iter()
            .chain(crate::sys::OS_ENV)
            .any(|n| same(n, k))
    };
    let injected: Vec<&str> = run_env
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| !own(k))
        .collect();
    std::env::vars()
        .filter(|(k, _)| {
            !injected.iter().any(|n| same(n, k))
                && !cfg.gated_env.iter().any(|n| same(n, k))
                && !cfg.env_passthrough.iter().any(|n| same(n, k))
                && !k.starts_with("CHAT_")
        })
        .collect()
}

/// Output of one command as `hand.output` texts: at most [`MAX_HAND_OUTPUT_BYTES`] each, cut
/// on a character boundary, at most `cap` bytes in all; after that only the last
/// [`MAX_HAND_OUTPUT_BYTES`] are kept, for the end. Also keeps the last lines for the
/// hand-back note.
#[derive(Debug)]
pub struct OutCap {
    cap: usize,
    pending: Vec<u8>,
    sent: usize,
    dropped: u64,
    tail: Vec<u8>,
    last: Vec<u8>,
}

impl OutCap {
    pub fn new(cap: usize) -> OutCap {
        OutCap {
            cap,
            pending: vec![],
            sent: 0,
            dropped: 0,
            tail: vec![],
            last: vec![],
        }
    }

    fn keep_end(buf: &mut Vec<u8>, max: usize) {
        if buf.len() > max {
            buf.drain(..buf.len() - max);
        }
    }

    /// Takes `b`; returns the texts ready to send (whole chunks).
    pub fn push(&mut self, b: &[u8]) -> Vec<String> {
        self.last.extend_from_slice(b);
        Self::keep_end(&mut self.last, 4096);
        let room = self.cap.saturating_sub(self.sent + self.pending.len());
        let (now, later) = b.split_at(room.min(b.len()));
        self.pending.extend_from_slice(now);
        if !later.is_empty() {
            self.dropped += later.len() as u64;
            self.tail.extend_from_slice(later);
            Self::keep_end(&mut self.tail, MAX_HAND_OUTPUT_BYTES);
        }
        let mut out = vec![];
        while self.pending.len() >= MAX_HAND_OUTPUT_BYTES {
            let mut cut = MAX_HAND_OUTPUT_BYTES;
            // Back off from a cut inside a character (a cut at the end is never inside one).
            while cut < self.pending.len()
                && cut > MAX_HAND_OUTPUT_BYTES - 4
                && (self.pending[cut] & 0xC0) == 0x80
            {
                cut -= 1;
            }
            out.push(self.take(cut));
        }
        out
    }

    fn take(&mut self, n: usize) -> String {
        let bytes: Vec<u8> = self.pending.drain(..n).collect();
        self.sent += bytes.len();
        bounded(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// `n` more bytes past the cap that never came to [`OutCap::push`] (an executor that caps
    /// on its own): counted in the "not shown" line, not kept.
    pub fn skip(&mut self, n: u64) {
        if n > 0 {
            self.dropped += n;
        }
    }

    /// What is pending, except a character still incomplete at its end.
    pub fn flush(&mut self) -> Option<String> {
        let n = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => self.pending.len(),
        };
        (n > 0).then(|| self.take(n))
    }

    /// Everything left, at the end: what is pending, then the last bytes past the cap.
    pub fn finish(&mut self) -> Vec<String> {
        let mut out = vec![];
        if !self.pending.is_empty() {
            let n = self.pending.len();
            out.push(self.take(n));
        }
        if self.dropped > 0 {
            let mut tail = std::mem::take(&mut self.tail);
            // The count line and the last bytes fit one text, so its end is never cut off;
            // the tail starts on a character.
            let room = MAX_HAND_OUTPUT_BYTES - 64;
            if tail.len() > room {
                tail.drain(..tail.len() - room);
            }
            let lead = tail
                .iter()
                .take(3)
                .take_while(|b| (*b & 0xC0) == 0x80)
                .count();
            tail.drain(..lead);
            let skipped = self.dropped - tail.len() as u64;
            out.push(bounded(format!(
                "\n[... {skipped} bytes not shown ...]\n{}",
                String::from_utf8_lossy(&tail)
            )));
        }
        out
    }

    /// The last lines of the output, for the hand-back note.
    pub fn last_text(&self) -> String {
        String::from_utf8_lossy(&self.last).into_owned()
    }
}

/// At most [`MAX_HAND_OUTPUT_BYTES`]: invalid bytes became U+FFFD and may have grown the text.
fn bounded(mut s: String) -> String {
    if s.len() > MAX_HAND_OUTPUT_BYTES {
        let mut end = MAX_HAND_OUTPUT_BYTES;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

/// What a running command sends to its run.
#[derive(Debug)]
pub enum HandMsg {
    Event(RunEvent),
    Ended { id: String, ran: Ran },
}

/// The command that runs now.
pub struct Running {
    pub id: String,
    cancel: Arc<dyn Fn() + Send + Sync>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl Running {
    /// The person stops it (or hands back, or the run ends): its process group ends.
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        (self.cancel)();
    }
}

/// Runs the started `child` to its end: output as `hand.output`, then `hand.result` and
/// [`HandMsg::Ended`]. Ends it after `limit`.
pub fn watch(
    child: HandChild,
    id: String,
    command: String,
    limit: Duration,
    tx: mpsc::UnboundedSender<HandMsg>,
) -> Running {
    let HandChild {
        mut output,
        done,
        cancel,
    } = child;
    let cancel: Arc<dyn Fn() + Send + Sync> = Arc::from(cancel);
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let running = Running {
        id: id.clone(),
        cancel: cancel.clone(),
        cancelled: cancelled.clone(),
    };
    tokio::spawn(async move {
        let started = Instant::now();
        let mut cap = OutCap::new(MAX_OUTPUT);
        let send_out = |texts: Vec<String>| {
            for text in texts.into_iter().filter(|t| !t.is_empty()) {
                let _ = tx.send(HandMsg::Event(RunEvent::HandOutput {
                    command_id: id.clone(),
                    text,
                }));
            }
        };
        let limit_at = tokio::time::sleep(limit);
        tokio::pin!(limit_at);
        let mut flush = tokio::time::interval(Duration::from_millis(100));
        let mut timed_out = false;
        let mut open = true;
        tokio::pin!(done);
        let end = loop {
            tokio::select! {
                b = output.recv(), if open => match b {
                    Some(b) => send_out(cap.push(&b)),
                    None => open = false,
                },
                e = &mut done => break e.unwrap_or_default(),
                _ = &mut limit_at, if !timed_out => {
                    timed_out = true;
                    cancel();
                }
                _ = flush.tick() => send_out(cap.flush().into_iter().collect()),
            }
        };
        if open {
            let drain = tokio::time::sleep(DRAIN);
            tokio::pin!(drain);
            loop {
                tokio::select! {
                    b = output.recv() => match b {
                        Some(b) => send_out(cap.push(&b)),
                        None => break,
                    },
                    _ = &mut drain => break,
                }
            }
        }
        cap.skip(end.skipped);
        if let Some(e) = &end.error {
            cap.push(format!("{e}\n").as_bytes());
        }
        send_out(cap.finish());
        let was_cancelled = cancelled.load(std::sync::atomic::Ordering::SeqCst);
        let ok = end.code == Some(0) && !timed_out && !was_cancelled && end.error.is_none();
        let _ = tx.send(HandMsg::Event(RunEvent::HandResult {
            command_id: id.clone(),
            ok,
            exit_code: end.code,
            ms: started.elapsed().as_millis() as u64,
            timed_out: timed_out.then_some(true),
            error: end.unsupported.then(|| "unsupported".to_string()),
        }));
        let end = if timed_out {
            End::TimedOut
        } else if was_cancelled {
            End::Cancelled
        } else if let Some(e) = end.error {
            End::Error(e)
        } else if end.unsupported {
            End::Error("this machine cannot run commands for this run".into())
        } else if ok {
            End::Ok
        } else {
            End::Failed(end.code)
        };
        let _ = tx.send(HandMsg::Ended {
            id,
            ran: Ran {
                command,
                end,
                tail: cap.last_text(),
            },
        });
    });
    running
}

/// A take-over of a run: who holds it, since when, the work folder then, and the commands.
pub struct Takeover {
    pub by: String,
    since: Instant,
    before: handback::Snapshot,
    ran: Vec<Ran>,
    pub running: Option<Running>,
}

/// A refused command: `hand.result` with `ok: false` and the reason.
pub fn refused(id: &str, error: &str) -> RunEvent {
    RunEvent::HandResult {
        command_id: id.to_string(),
        ok: false,
        exit_code: None,
        ms: 0,
        timed_out: None,
        error: Some(error.to_string()),
    }
}

impl Takeover {
    /// Starts a take-over: records the work folder for the hand-back.
    pub async fn start(by: Option<String>, work: &Path) -> Takeover {
        Takeover {
            by: by.unwrap_or_default(),
            since: Instant::now(),
            before: handback::snapshot(work).await,
            ran: vec![],
            running: None,
        }
    }

    /// `run.command`: only from the person holding the run, one at a time.
    #[allow(clippy::too_many_arguments)]
    pub fn command(
        &mut self,
        exec: &dyn Executor,
        req: HandReq,
        id: String,
        by: &str,
        limit: Duration,
        tx: &mpsc::UnboundedSender<HandMsg>,
        event: &impl Fn(RunEvent),
    ) {
        if self.by.is_empty() || by != self.by {
            return event(refused(&id, "not_held"));
        }
        if self.running.is_some() {
            return event(refused(&id, "busy"));
        }
        let command = req.command.clone();
        match exec.hand(&req) {
            Ok(child) => {
                event(RunEvent::HandCommand {
                    command_id: id.clone(),
                    command: command.clone(),
                    by: by.to_string(),
                });
                self.running = Some(watch(child, id, command, limit, tx.clone()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
                event(refused(&id, "unsupported"))
            }
            Err(e) => {
                event(RunEvent::HandCommand {
                    command_id: id.clone(),
                    command: command.clone(),
                    by: by.to_string(),
                });
                event(RunEvent::HandOutput {
                    command_id: id.clone(),
                    text: format!("cannot start the command: {e}\n"),
                });
                event(RunEvent::HandResult {
                    command_id: id,
                    ok: false,
                    exit_code: None,
                    ms: 0,
                    timed_out: None,
                    error: None,
                });
                self.ran.push(Ran {
                    command,
                    end: End::Error(e.to_string()),
                    tail: String::new(),
                });
            }
        }
    }

    /// `run.command.cancel` of the running command.
    pub fn cancel(&self, id: &str) {
        if let Some(r) = self.running.as_ref().filter(|r| r.id == id) {
            r.cancel();
        }
    }

    /// A message of the running command: its events go on, its end is recorded.
    pub fn on_msg(&mut self, m: HandMsg, event: &impl Fn(RunEvent)) {
        match m {
            HandMsg::Event(ev) => event(ev),
            HandMsg::Ended { id, ran } => {
                if self.running.as_ref().is_some_and(|r| r.id == id) {
                    self.running = None;
                }
                self.ran.push(ran);
            }
        }
    }

    /// Hand-back: ends the running command (waiting a little for its result), then returns
    /// the note for the CLI and the `hand.summary`.
    pub async fn hand_back(
        mut self,
        work: &Path,
        words: Option<&str>,
        rx: &mut mpsc::UnboundedReceiver<HandMsg>,
        event: &impl Fn(RunEvent),
    ) -> (String, RunHandback) {
        if let Some(r) = &self.running {
            r.cancel();
            let wait = tokio::time::sleep(Duration::from_secs(5));
            tokio::pin!(wait);
            while self.running.is_some() {
                tokio::select! {
                    m = rx.recv() => match m {
                        Some(m) => self.on_msg(m, event),
                        None => break,
                    },
                    _ = &mut wait => break,
                }
            }
        }
        let held = self.since.elapsed();
        let ch = handback::changes(&self.before, work).await;
        let note = handback::note(&self.by, held, work, &self.ran, &ch, words);
        let summary = RunHandback {
            by: self.by.clone(),
            held_ms: held.as_millis() as u64,
            commands: self.ran.len() as u64,
            files: ch
                .files
                .iter()
                .filter(|f| f.len() <= 1024)
                .take(MAX_HANDBACK_FILES)
                .cloned()
                .collect(),
            head_from: ch.head_from,
            head_to: ch.head_to,
        };
        (note, summary)
    }
}

/// The run ended while the person's command `id` ran (it was cancelled with the take-over):
/// its last events still go up, waiting up to `wait` for them, and a `hand.result` with
/// `ok: false` goes up when its own does not come in time. Without `id`, only what is already
/// there goes up.
pub async fn finish_with_run(
    id: Option<String>,
    rx: &mut mpsc::UnboundedReceiver<HandMsg>,
    wait: Duration,
    event: &impl Fn(RunEvent),
) {
    if let Some(id) = id {
        let mut result = false;
        let until = tokio::time::sleep(wait);
        tokio::pin!(until);
        loop {
            tokio::select! {
                m = rx.recv() => match m {
                    Some(HandMsg::Event(ev)) => {
                        if matches!(&ev, RunEvent::HandResult { command_id, .. } if *command_id == id) {
                            result = true;
                        }
                        event(ev);
                    }
                    Some(HandMsg::Ended { id: ended, .. }) if ended == id => break,
                    Some(HandMsg::Ended { .. }) => {}
                    None => break,
                },
                _ = &mut until => break,
            }
        }
        if !result {
            event(RunEvent::HandResult {
                command_id: id,
                ok: false,
                exit_code: None,
                ms: 0,
                timed_out: None,
                error: None,
            });
        }
    }
    while let Ok(m) = rx.try_recv() {
        if let HandMsg::Event(ev) = m {
            event(ev);
        }
    }
}

impl Drop for Takeover {
    /// A run that ends while held: the person's command ends with it.
    fn drop(&mut self) {
        if let Some(r) = &self.running {
            r.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_cut_on_characters_and_capped_with_the_tail_at_the_end() {
        let mut c = OutCap::new(40_000);
        // 16 383 ASCII bytes then a 3-byte character across the 16 KiB line.
        let mut b = vec![b'a'; MAX_HAND_OUTPUT_BYTES - 1];
        b.extend_from_slice("€".as_bytes());
        b.extend_from_slice(b"tail\n");
        let out = c.push(&b);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), MAX_HAND_OUTPUT_BYTES - 1);
        assert_eq!(c.flush().as_deref(), Some("€tail\n"));
        // Exactly one text's worth goes at once.
        let mut whole = OutCap::new(40_000);
        let out = whole.push(&vec![b'a'; MAX_HAND_OUTPUT_BYTES]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), MAX_HAND_OUTPUT_BYTES);
        // An incomplete character waits for its last byte.
        c.push(&"é".as_bytes()[..1]);
        assert_eq!(c.flush(), None);
        c.push(&"é".as_bytes()[1..]);
        assert_eq!(c.flush().as_deref(), Some("é"));
        // Past the cap: nothing more now, the last bytes at the end with a count.
        let big = vec![b'z'; 50_000];
        let sent: usize = c.push(&big).iter().map(String::len).sum();
        let pending = c.flush().map(|s| s.len()).unwrap_or(0);
        assert!(MAX_HAND_OUTPUT_BYTES + 8 + sent + pending <= 40_000);
        let end = c.finish();
        let last = end.last().unwrap();
        assert!(last.contains("bytes not shown"), "{last}");
        assert!(last.len() <= MAX_HAND_OUTPUT_BYTES);
        assert!(c.last_text().ends_with("zzz"));
    }

    /// Runs `command` with the local executor and `limit`; cancels it after `cancel_after`.
    async fn run_one(
        command: &str,
        limit: Duration,
        cancel_after: Option<Duration>,
    ) -> (Vec<RunEvent>, Ran, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let exec = crate::executor::local();
        let req = HandReq {
            run_dir: tmp.path().join("run"),
            cwd: tmp.path().to_path_buf(),
            command: command.into(),
            env: std::env::vars().collect(),
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let child = exec.hand(&req).unwrap();
        let running = watch(child, "c1".into(), command.into(), limit, tx);
        if let Some(d) = cancel_after {
            tokio::time::sleep(d).await;
            running.cancel();
        }
        let mut evs = vec![];
        let ran = loop {
            match tokio::time::timeout(Duration::from_secs(20), rx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                HandMsg::Event(e) => evs.push(e),
                HandMsg::Ended { ran, .. } => break ran,
            }
        };
        exec.hand_end(&req.run_dir);
        (evs, ran, tmp)
    }

    fn output(evs: &[RunEvent]) -> String {
        evs.iter()
            .filter_map(|e| match e {
                RunEvent::HandOutput { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn result(evs: &[RunEvent]) -> (bool, Option<i64>, Option<bool>) {
        evs.iter()
            .find_map(|e| match e {
                RunEvent::HandResult {
                    ok,
                    exit_code,
                    timed_out,
                    ..
                } => Some((*ok, *exit_code, *timed_out)),
                _ => None,
            })
            .expect("hand.result")
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_runs_in_the_work_folder_with_its_output_and_exit_code() {
        let (evs, ran, tmp) = run_one(
            "echo out; echo err >&2; pwd -P; printf x > made.txt; exit 3",
            LIMIT,
            None,
        )
        .await;
        let out = output(&evs);
        assert!(out.contains("out\n") && out.contains("err\n"), "{out}");
        let here = std::fs::canonicalize(tmp.path()).unwrap();
        assert!(out.contains(&here.display().to_string()), "{out}");
        assert!(tmp.path().join("made.txt").is_file());
        assert_eq!(result(&evs), (false, Some(3), None));
        assert_eq!(ran.end, End::Failed(Some(3)));
        assert!(ran.tail.contains("err"));
    }

    /// The 10-minute limit (here 300 ms) ends the command's group; a cancel does too, and what
    /// it left in the background does not hold up its result.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_limit_and_a_cancel_end_the_command() {
        let started = Instant::now();
        let (evs, ran, _t) = run_one("sleep 30", Duration::from_millis(300), None).await;
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(result(&evs), (false, None, Some(true)));
        assert_eq!(ran.end, End::TimedOut);

        let started = Instant::now();
        let (evs, ran, _t) = run_one("sleep 30", LIMIT, Some(Duration::from_millis(200))).await;
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(!result(&evs).0);
        assert_eq!(ran.end, End::Cancelled);

        // A background process keeps the output pipe open; the result comes anyway, and the
        // run's end takes the process with it.
        let started = Instant::now();
        let (evs, ran, tmp) = run_one("sleep 30 & echo $! > bg.pid; echo done", LIMIT, None).await;
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(result(&evs), (true, Some(0), None));
        assert_eq!(ran.end, End::Ok);
        let pid: i32 = std::fs::read_to_string(tmp.path().join("bg.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // `run_one` ended the run (hand_end): the background sleep is gone.
        let mut gone = false;
        for _ in 0..50 {
            // SAFETY: probing a pid with signal 0.
            if unsafe { libc::kill(pid, 0) } != 0 {
                gone = true;
                break;
            }
            // Reap it if it is our zombie: it is not ours (the shell's child), so just wait.
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(gone, "background process {pid} outlived the run");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_powershell_command_runs_in_the_work_folder() {
        let (evs, ran, tmp) = run_one(
            "Write-Output out; [Console]::Error.WriteLine('err'); Set-Content -Path made.txt -Value x; exit 3",
            LIMIT,
            None,
        )
        .await;
        let out = output(&evs);
        assert!(out.contains("out") && out.contains("err"), "{out}");
        assert!(tmp.path().join("made.txt").is_file());
        assert_eq!(result(&evs), (false, Some(3), None));
        assert_eq!(ran.end, End::Failed(Some(3)));
        let (evs, ran, _t) =
            run_one("Start-Sleep 600", LIMIT, Some(Duration::from_millis(500))).await;
        assert!(!result(&evs).0);
        assert_eq!(ran.end, End::Cancelled);
    }

    /// A VM's command past the agent's cap: its 1 MiB, then the agent's last bytes at the
    /// end; the "not shown" line counts what the agent dropped, once.
    #[cfg(unix)]
    #[tokio::test]
    async fn output_capped_in_the_vm_is_counted_once() {
        use crate::executor::vmwire::{EXEC_OUT_CAP, EXEC_OUT_CHUNK, ExecExit};
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (done_tx, done) = tokio::sync::oneshot::channel();
        let child = HandChild {
            output: out_rx,
            done,
            cancel: Box::new(|| {}),
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _running = watch(child, "c1".into(), "yes".into(), LIMIT, tx);
        for _ in 0..EXEC_OUT_CAP / EXEC_OUT_CHUNK {
            out_tx.send(vec![b'y'; EXEC_OUT_CHUNK]).unwrap();
        }
        let dropped = 5_000_000u64;
        let tail = "z".repeat(EXEC_OUT_CHUNK);
        let end = crate::executor::firecracker::exec_end(
            ExecExit {
                id: "c1".into(),
                code: Some(0),
                ms: 1,
                dropped,
                tail,
                error: None,
            },
            &out_tx,
        );
        assert_eq!(end.skipped, dropped - EXEC_OUT_CHUNK as u64);
        drop(out_tx);
        done_tx.send(end).unwrap();
        let mut evs = vec![];
        while let HandMsg::Event(e) = tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .unwrap()
            .unwrap()
        {
            evs.push(e);
        }
        let out = output(&evs);
        assert_eq!(out.matches("bytes not shown").count(), 1);
        // The agent's 1 MiB, then one count line, then the last bytes up to the end.
        assert_eq!(
            out[..EXEC_OUT_CAP].bytes().filter(|b| *b == b'y').count(),
            EXEC_OUT_CAP
        );
        let rest = &out[EXEC_OUT_CAP..];
        let (head, shown) = rest
            .strip_prefix("\n[... ")
            .and_then(|r| r.split_once(" bytes not shown ...]\n"))
            .unwrap_or_else(|| panic!("{}", rest.get(..80).unwrap_or(rest)));
        assert!(!shown.is_empty() && shown.bytes().all(|b| b == b'z'));
        // What was shown and what was not add up to what the agent dropped.
        assert_eq!(head.parse::<u64>().unwrap() + shown.len() as u64, dropped);
        assert!(evs.iter().all(|e| match e {
            RunEvent::HandOutput { text, .. } => text.len() <= MAX_HAND_OUTPUT_BYTES,
            _ => true,
        }));
        assert_eq!(result(&evs), (true, Some(0), None));
    }

    /// The run ends while a person's command runs: the command still gets its `hand.result`.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_running_when_the_run_ends_gets_its_result() {
        let tmp = tempfile::tempdir().unwrap();
        let exec = crate::executor::local();
        let req = HandReq {
            run_dir: tmp.path().join("run"),
            cwd: tmp.path().to_path_buf(),
            command: "echo started; sleep 600".into(),
            env: std::env::vars().collect(),
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let running = watch(
            exec.hand(&req).unwrap(),
            "c1".into(),
            req.command.clone(),
            LIMIT,
            tx,
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        running.cancel();
        let evs = std::sync::Mutex::new(vec![]);
        finish_with_run(Some("c1".into()), &mut rx, Duration::from_secs(5), &|e| {
            evs.lock().unwrap().push(e)
        })
        .await;
        exec.hand_end(&req.run_dir);
        let evs = evs.into_inner().unwrap();
        assert_eq!(result(&evs), (false, None, None));
        assert_eq!(
            evs.iter()
                .filter(|e| matches!(e, RunEvent::HandResult { .. }))
                .count(),
            1
        );
        // A command whose end does not come in time gets one with `ok: false`.
        let (_tx, mut rx) = mpsc::unbounded_channel();
        let evs = std::sync::Mutex::new(vec![]);
        finish_with_run(
            Some("c2".into()),
            &mut rx,
            Duration::from_millis(50),
            &|e| evs.lock().unwrap().push(e),
        )
        .await;
        let evs = evs.into_inner().unwrap();
        assert!(matches!(
            &evs[..],
            [RunEvent::HandResult { command_id, ok: false, .. }] if command_id == "c2"
        ));
    }

    #[test]
    fn the_persons_env_has_none_of_the_runs_secrets() {
        let cfg = Config::parse(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\ngated_env = [\"TC_TEST_GATED\"]\nenv_passthrough = [\"TC_TEST_PASS\"]\n",
        )
        .unwrap();
        // SAFETY: test-only variables no other test reads.
        unsafe {
            std::env::set_var("TC_TEST_GATED", "g");
            std::env::set_var("TC_TEST_PASS", "p");
            std::env::set_var("TC_TEST_KEY", "from the daemon");
            std::env::set_var("TC_TEST_PLAIN", "x");
        }
        let run_env = vec![
            ("PATH".to_string(), "/runs/r1/bin:/usr/bin".to_string()),
            ("TC_TEST_KEY".to_string(), "sk-app-key".to_string()),
            (
                "CHAT_RUN_SOCKET".to_string(),
                "/runs/r1/chat-run.sock".to_string(),
            ),
        ];
        let env = host_env(&cfg, &run_env);
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(get("TC_TEST_GATED"), None);
        assert_eq!(get("TC_TEST_PASS"), None);
        assert_eq!(get("TC_TEST_KEY"), None);
        assert_eq!(get("CHAT_RUN_SOCKET"), None);
        assert_eq!(get("TC_TEST_PLAIN").as_deref(), Some("x"));
        assert_eq!(get("PATH"), std::env::var("PATH").ok());
    }
}
