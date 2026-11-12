//! The process tree of a run. A CLI starts in its own process group, but CLIs start their tool
//! commands in groups or sessions of their own (Claude Code's Bash tool, Codex exec), so a signal
//! to the CLI's group does not reach them. These helpers find the CLI's descendants and signal
//! each of their groups. They never signal the daemon's own group, pid 1, or a process outside
//! the run's tree.

use std::collections::HashSet;
use std::path::Path;

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

/// CPU time (user and system, nanoseconds) process `pid` has used; None when it is gone.
// libc marks the Mach timebase as deprecated in favour of the `mach2` crate; one plain call does
// not need another dependency.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
pub fn cpu_ns(pid: i32) -> Option<u64> {
    if pid <= 0 {
        return None;
    }
    // SAFETY: a plain C struct; zeroes are a valid value.
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: the buffer holds `size` bytes; libproc writes at most that many.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    // The totals are in Mach time units (nanoseconds on Intel, ticks on Apple silicon).
    let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
    // SAFETY: writes the two fields of `tb`.
    unsafe {
        libc::mach_timebase_info(&mut tb);
    }
    let ticks = info.pti_total_user.saturating_add(info.pti_total_system) as u128;
    let (numer, denom) = if tb.denom == 0 {
        (1, 1)
    } else {
        (tb.numer as u128, tb.denom as u128)
    };
    Some((ticks * numer / denom) as u64)
}

/// CPU time (user and system, nanoseconds) process `pid` has used, from `/proc/<pid>/stat`
/// (`utime` and `stime`, the 12th and 13th fields after the state); None when it is gone.
#[cfg(target_os = "linux")]
pub fn cpu_ns(pid: i32) -> Option<u64> {
    if pid <= 0 {
        return None;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut f = rest.split_whitespace().skip(11);
    let utime: u64 = f.next()?.parse().ok()?;
    let stime: u64 = f.next()?.parse().ok()?;
    // SAFETY: plain syscall without pointers.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let hz = if hz > 0 { hz as u64 } else { 100 };
    Some((utime + stime).saturating_mul(1_000_000_000 / hz))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn cpu_ns(_pid: i32) -> Option<u64> {
    None
}

/// 盯 1.4: the CPU time each process below the CLI `root` (its tools: a build, a test run) has
/// used, by pid. None when `root` is gone.
pub fn tools_cpu(root: i32) -> Option<std::collections::HashMap<i32, u64>> {
    if root <= 1 || start_time(root).is_none() {
        return None;
    }
    Some(
        descendants(root)
            .into_iter()
            .filter_map(|p| cpu_ns(p).map(|ns| (p, ns)))
            .collect(),
    )
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

/// The variable that marks every process of a run: the daemon gives the CLI
/// `CHAT_RUN_FOLDER=<run folder>` (unique on the machine, unlike the run id across work roots),
/// and its tools inherit it.
pub const ENV_FOLDER: &str = "CHAT_RUN_FOLDER";

fn marker(run_dir: &Path) -> Vec<u8> {
    let mut m = format!("{ENV_FOLDER}=").into_bytes();
    m.extend_from_slice(run_dir.as_os_str().as_encoded_bytes());
    m
}

/// Processes of the run in `run_dir` found by their environment, also the ones that left the
/// CLI's tree (a tool that daemonized, or whose parent exited, is re-parented to pid 1 or to a
/// subreaper): processes of the daemon's own user whose environment holds
/// `CHAT_RUN_FOLDER=<run_dir>`. Each is the group it leads, or the process.
/// macOS does not show the environment of its own platform binaries (`/bin/sleep`), so such an
/// orphan is not found there; tools a CLI starts from other paths (node, python, cargo) are.
pub fn orphans(run_dir: &Path) -> Vec<Target> {
    if run_dir.as_os_str().is_empty() {
        return vec![];
    }
    let want = marker(run_dir);
    // SAFETY: plain syscalls without arguments.
    let (own_group, own_pid) = unsafe { (libc::getpgrp(), libc::getpid()) };
    let mut out = vec![];
    for pid in own_pids() {
        if pid <= 1 || pid == own_pid || !environ(pid).contains(&want) {
            continue;
        }
        let t = match pgid(pid) {
            Some(g) if g == pid && g != own_group => Target::Group(g, start_time(g)),
            _ => match start_time(pid) {
                Some(start) => Target::Pid(pid, start),
                None => continue,
            },
        };
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// The tool targets of a run: [`targets`] below the CLI `root` and the run's [`orphans`].
pub fn run_targets(root: i32, run_dir: &Path) -> Vec<Target> {
    let mut out = targets(root);
    for t in orphans(run_dir) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Pids of the daemon's own user.
#[cfg(target_os = "macos")]
fn own_pids() -> Vec<i32> {
    let mut buf = vec![0i32; 4096];
    let pids = loop {
        let bytes = (buf.len() * std::mem::size_of::<i32>()) as libc::c_int;
        // SAFETY: the buffer holds `bytes` bytes; libproc writes at most that many.
        let n = unsafe { libc::proc_listallpids(buf.as_mut_ptr().cast(), bytes) };
        if n <= 0 {
            return vec![];
        }
        let n = n as usize;
        if n < buf.len() {
            buf.truncate(n);
            break buf;
        }
        buf.resize(buf.len() * 2, 0);
    };
    // SAFETY: plain syscall without arguments.
    let me = unsafe { libc::geteuid() };
    pids.into_iter()
        .filter(|&pid| pid > 1)
        .filter(|&pid| {
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
            n == size && info.pbi_uid == me
        })
        .collect()
}

/// Pids of the daemon's own user, from `/proc`.
#[cfg(target_os = "linux")]
fn own_pids() -> Vec<i32> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: plain syscall without arguments.
    let me = unsafe { libc::geteuid() };
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return vec![];
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&pid| {
            pid > 1 && std::fs::metadata(format!("/proc/{pid}")).is_ok_and(|m| m.uid() == me)
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn own_pids() -> Vec<i32> {
    vec![]
}

/// The environment of `pid` (each `NAME=value` entry), from `KERN_PROCARGS2`: argc, the exec
/// path, padding, argc arguments, then the environment, each ending with NUL. Empty when the
/// process is gone or belongs to another user.
#[cfg(target_os = "macos")]
fn environ(pid: i32) -> Vec<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: libc::size_t = 0;
    // SAFETY: a size query (null buffer) of a three-element MIB.
    let ok = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 || size < 4 {
        return vec![];
    }
    let mut buf = vec![0u8; size];
    // SAFETY: the buffer holds `size` bytes; the kernel writes at most that many.
    let ok = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 || size < 4 {
        return vec![];
    }
    buf.truncate(size);
    let argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]).max(0) as usize;
    let mut parts = buf[4..].split(|b| *b == 0);
    // The exec path, then NUL padding up to the arguments.
    let _ = parts.next();
    let mut rest: Vec<&[u8]> = parts.skip_while(|p| p.is_empty()).collect();
    if rest.len() < argc {
        return vec![];
    }
    rest.drain(..argc);
    rest.into_iter()
        .take_while(|p| !p.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

#[cfg(target_os = "linux")]
fn environ(pid: i32) -> Vec<Vec<u8>> {
    std::fs::read(format!("/proc/{pid}/environ"))
        .map(|b| {
            b.split(|c| *c == 0)
                .filter(|p| !p.is_empty())
                .map(<[u8]>::to_vec)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn environ(_pid: i32) -> Vec<Vec<u8>> {
    vec![]
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

/// SIGSTOP the CLI's group `root`, then every group below it and the run's orphans. Scans
/// again until a scan finds nothing new, so a child started during the scan stops too. Adds
/// what it stopped to `stopped`.
pub fn stop(root: i32, run_dir: &Path, stopped: &mut Vec<Target>) {
    if root <= 1 {
        return;
    }
    // SAFETY: plain syscall addressing the CLI's process group.
    unsafe {
        libc::kill(-root, libc::SIGSTOP);
    }
    for _ in 0..50 {
        let new: Vec<Target> = run_targets(root, run_dir)
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

    /// 盯 1.4: the CPU the tools below a CLI use is seen, and a gone CLI has no tools.
    #[test]
    fn tools_cpu_sees_a_busy_tool_and_not_a_sleeping_one() {
        // The "CLI" waits for a busy tool and a sleeping one.
        let mut cli = Command::new("/bin/sh")
            .arg("-c")
            .arg("(while :; do :; done) & sleep 30 & wait")
            .spawn()
            .unwrap();
        let root = cli.id() as i32;
        let total = |m: &std::collections::HashMap<i32, u64>| m.values().sum::<u64>();
        // Up to 10 s for both tools to start and the busy one to burn some CPU.
        let mut first = std::collections::HashMap::new();
        for _ in 0..500 {
            first = tools_cpu(root).unwrap();
            if first.len() >= 2 && total(&first) > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(first.len() >= 2, "{first:?}");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let second = tools_cpu(root).unwrap();
        assert!(total(&second) > total(&first), "{first:?} {second:?}");
        for p in second.keys() {
            // SAFETY: plain syscall; the pids are this test's own tools.
            unsafe {
                libc::kill(*p, libc::SIGKILL);
            }
        }
        let _ = cli.kill();
        let _ = cli.wait();
        assert!(tools_cpu(root).is_none());
        assert!(cpu_ns(std::process::id() as i32).is_some());
    }

    const STAND_IN: &str = "TC_PROCTREE_STAND_IN";

    /// The tool of `orphans_are_found_by_the_run_marker`: with `TC_PROCTREE_STAND_IN` set it
    /// runs for 30 s (the test kills it sooner); in a plain test run it returns at once.
    #[test]
    fn stand_in_tool() {
        if std::env::var_os(STAND_IN).is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    /// A tool that daemonized (its parent exited, so pid 1 is its parent) is found by the run's
    /// folder in its environment, and only for its own run.
    #[test]
    fn orphans_are_found_by_the_run_marker() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("orphan.pid");
        // The tool is this test program running `stand_in_tool`: a program built here, like the
        // tools a CLI starts (node, python, cargo), so macOS shows its environment. (An ad-hoc
        // re-signed copy of /bin/sleep was killed at launch on the macOS 15 runners.)
        let exe = std::env::current_exe().unwrap();
        // sh starts the tool in the background and exits: the tool is re-parented to pid 1.
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "'{}' --exact proctree::tests::stand_in_tool -q >/dev/null 2>&1 & echo $! > '{}'",
                exe.display(),
                pidfile.display()
            ))
            .env(ENV_FOLDER, dir.path().join("runs/r1"))
            .env(STAND_IN, "1")
            .status()
            .unwrap();
        assert!(status.success());
        let pid: i32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let _ = std::fs::remove_file(&pidfile);
        let hit = |t: &Target| matches!(*t, Target::Pid(p, _) | Target::Group(p, _) if p == pid);
        // Up to 15 s for the tool to start and to be re-parented on a loaded runner.
        let mut found = vec![];
        for _ in 0..750 {
            found = orphans(&dir.path().join("runs/r1"));
            if found.iter().any(hit) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(found.iter().any(hit), "alive: {}, {found:?}", alive(pid));
        assert!(!orphans(&dir.path().join("runs/r2")).iter().any(hit));
        assert!(
            !orphans(&dir.path().join("runs/r")).iter().any(hit),
            "a prefix is not the run"
        );
        signal(&found, libc::SIGKILL);
        for _ in 0..100 {
            if !alive(pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!alive(pid));
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
