//! The process tree of a run. A CLI starts in its own process group, but CLIs start their tool
//! commands in groups or sessions of their own (Claude Code's Bash tool, Codex exec), so a signal
//! to the CLI's group does not reach them. These helpers find the CLI's descendants and signal
//! each of their groups. They never signal the daemon's own group, pid 1, or a process outside
//! the run's tree.

use std::collections::HashSet;

/// What one signal goes to: a process group whose leader is in the run's tree, or one process
/// whose group leader is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    Group(i32),
    Pid(i32),
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
            Target::Group(g)
        } else {
            Target::Pid(pid)
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

pub fn signal(targets: &[Target], sig: i32) {
    for t in targets {
        // SAFETY: plain syscalls; a negative pid addresses a process group.
        unsafe {
            match *t {
                Target::Group(g) if g > 1 => libc::kill(-g, sig),
                Target::Pid(p) if p > 1 => libc::kill(p, sig),
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
