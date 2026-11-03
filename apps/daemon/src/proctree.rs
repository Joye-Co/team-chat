//! The process tree of a run. A CLI starts in its own process group, but CLIs start their tool
//! commands in groups or sessions of their own (Claude Code's Bash tool, Codex exec), so a signal
//! to the CLI's group does not reach them. These helpers find the CLI's descendants and signal
//! each of their groups. They never signal the daemon's own group, pid 1, or a process outside
//! the run's tree.

use std::collections::HashSet;

/// What one signal goes to: a process group whose leader is in the run's tree, or one process
/// whose group leader is not. Each keeps the start time of its process (the group's leader),
/// so a later signal skips a pid that now belongs to another process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    Group(i32, Option<u64>),
    Pid(i32, u64),
}

/// Start time of process `pid` (microseconds since the epoch), from libproc.
#[cfg(target_os = "macos")]
pub fn start_time(pid: i32) -> Option<u64> {
    if pid <= 0 {
        return None;
    }
    // SAFETY: a plain C struct; zeroes are a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer holds `size` bytes; libproc writes at most that many.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (n == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

/// Start time of process `pid` (clock ticks since boot), from `/proc/<pid>/stat` (the 20th
/// field after the command).
#[cfg(target_os = "linux")]
pub fn start_time(pid: i32) -> Option<u64> {
    if pid <= 0 {
        return None;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn start_time(_pid: i32) -> Option<u64> {
    None
}

impl Target {
    /// Whether the target still is the process (group) it was when it was found. A group
    /// whose leader is gone stays a target: its pid cannot be reused while the group has
    /// members, and without members the signal reaches nobody.
    pub fn is_current(&self) -> bool {
        match *self {
            Target::Pid(p, start) => start_time(p) == Some(start),
            Target::Group(g, start) => match start_time(g) {
                None => true,
                now => start.is_some() && now == start,
            },
        }
    }
}

/// Direct children of `pid`.
#[cfg(target_os = "macos")]
fn children(pid: i32) -> Vec<i32> {
    let mut buf = vec![0i32; 256];
    loop {
        let bytes = (buf.len() * std::mem::size_of::<i32>()) as libc::c_int;
        // SAFETY: the buffer holds `bytes` bytes; libproc writes at most that many.
        let n = unsafe { libc::proc_listchildpids(pid, buf.as_mut_ptr().cast(), bytes) };
        if n <= 0 {
            return vec![];
        }
        let n = n as usize;
        if n < buf.len() {
            buf.truncate(n);
            buf.retain(|p| *p > 0);
            return buf;
        }
        buf.resize(buf.len() * 2, 0);
    }
}

/// Parent of every process, from `/proc/<pid>/stat` (the field after the command).
#[cfg(target_os = "linux")]
fn parents() -> Vec<(i32, i32)> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return vec![];
    };
    dir.flatten()
        .filter_map(|e| {
            let pid: i32 = e.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
            let rest = &stat[stat.rfind(')')? + 1..];
            let ppid: i32 = rest.split_whitespace().nth(1)?.parse().ok()?;
            Some((pid, ppid))
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn children(_pid: i32) -> Vec<i32> {
    vec![]
}

/// Every descendant of `root` (not `root` itself).
pub fn descendants(root: i32) -> Vec<i32> {
    #[cfg(target_os = "linux")]
    let all = parents();
    #[cfg(target_os = "linux")]
    let children = |pid: i32| -> Vec<i32> {
        all.iter()
            .filter(|(_, pp)| *pp == pid)
            .map(|(p, _)| *p)
            .collect()
    };
    let mut seen = HashSet::from([root]);
    let mut out = vec![];
    let mut todo = vec![root];
    while let Some(pid) = todo.pop() {
        for c in children(pid) {
            if seen.insert(c) {
                out.push(c);
                todo.push(c);
            }
        }
    }
    out
}

/// The groups and processes below the CLI `root`, which leads its own group. `root`'s group
/// itself is not in the list. Empty when `root` does not lead a group (it is gone, or the pid
/// now belongs to another process).
pub fn targets(root: i32) -> Vec<Target> {
    if root <= 1 || pgid(root) != Some(root) {
        return vec![];
    }
    // SAFETY: plain syscalls without arguments.
    let (own_group, own_pid) = unsafe { (libc::getpgrp(), libc::getpid()) };
    let desc = descendants(root);
    let tree: HashSet<i32> = desc.iter().copied().chain([root]).collect();
    let mut out: Vec<Target> = vec![];
    for pid in desc {
        if pid <= 1 || pid == own_pid {
            continue;
        }
        let Some(g) = pgid(pid) else { continue };
        if g == root {
            continue;
        }
        let t = if g > 1 && g != own_group && tree.contains(&g) {
            Target::Group(g, start_time(g))
        } else {
            let Some(start) = start_time(pid) else {
                continue;
            };
            Target::Pid(pid, start)
        };
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

fn pgid(pid: i32) -> Option<i32> {
    // SAFETY: plain syscall.
    let g = unsafe { libc::getpgid(pid) };
    (g > 0).then_some(g)
}

/// Sends `sig` to each target that [`Target::is_current`]; skips a reused pid.
pub fn signal(targets: &[Target], sig: i32) {
    for t in targets {
        if !t.is_current() {
            continue;
        }
        // SAFETY: plain syscalls; a negative pid addresses a process group.
        unsafe {
            match *t {
                Target::Group(g, _) if g > 1 => libc::kill(-g, sig),
                Target::Pid(p, _) if p > 1 => libc::kill(p, sig),
                _ => 0,
            };
        }
    }
}

/// SIGSTOP the CLI's group `root`, then every group below it. Scans again until a scan finds
/// nothing new, so a child started during the scan stops too. Adds what it stopped to `stopped`.
pub fn stop(root: i32, stopped: &mut Vec<Target>) {
    if root <= 1 {
        return;
    }
    // SAFETY: plain syscall addressing the CLI's process group.
    unsafe {
        libc::kill(-root, libc::SIGSTOP);
    }
    for _ in 0..50 {
        let new: Vec<Target> = targets(root)
            .into_iter()
            .filter(|t| !stopped.contains(t))
            .collect();
        if new.is_empty() {
            return;
        }
        signal(&new, libc::SIGSTOP);
        stopped.extend(new);
    }
}

/// SIGCONT what [`stop`] stopped, then the CLI's group `root`.
pub fn resume(root: i32, stopped: &[Target]) {
    signal(stopped, libc::SIGCONT);
    if root > 1 {
        // SAFETY: plain syscall addressing the CLI's process group.
        unsafe {
            libc::kill(-root, libc::SIGCONT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn signal_skips_a_pid_that_is_another_process_now() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id() as i32;
        let start = start_time(pid).expect("start time of a live child");
        assert_eq!(start_time(pid), Some(start), "start time is stable");
        assert!(Target::Pid(pid, start).is_current());

        // A target saved for an earlier process with this pid: the signal must not reach the
        // process that has the pid now.
        let old = Target::Pid(pid, start - 1);
        assert!(!old.is_current());
        signal(&[old], libc::SIGTERM);
        assert!(
            child.try_wait().unwrap().is_none(),
            "reused pid got the signal"
        );
        assert!(alive(pid));
        // The same for a group whose leader is another process now.
        assert!(!Target::Group(pid, Some(start - 1)).is_current());
        assert!(!Target::Group(pid, None).is_current());

        signal(&[Target::Pid(pid, start)], libc::SIGTERM);
        child.wait().unwrap();
        assert!(
            !Target::Pid(pid, start).is_current(),
            "a reaped pid is not current"
        );
        // A group whose leader is gone stays a target (its pid is not reused while the group
        // has members).
        assert!(Target::Group(pid, Some(start)).is_current());
    }
}
