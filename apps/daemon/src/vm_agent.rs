//! `vm-agent`: PID 1 of a run's microVM (the daemon binary started by that name from the tools
//! disk; `vm-init` in the base image execs it).
//!
//! It waits for the daemon on vsock port [`AGENT_PORT`], mounts the run's NFS shares at their
//! host paths, starts the CLI as the run's user and relays its stdin and stdout as frames (see
//! [`crate::executor::vmwire`]). The CLI's stderr goes straight to the run's `stderr.log` on
//! its share. When the daemon closes the stream the agent kills what is left and powers off.

/// The name the agent is started by.
pub const NAME: &str = "vm-agent";

/// 接管 (design-takeover 3.4): the person's command runs as the run's uid + 1, in the run's
/// group, so it cannot read the CLI's environment (`/proc/<pid>/environ`) or its files that
/// are not open to the group.
pub fn hand_uid(run_uid: u32) -> u32 {
    run_uid.saturating_add(1)
}

/// Where a person's command sees the shares: each `open` path (the work folder, the run's git
/// dir) is kept, every other share (the member's home, the run folder) is unmounted in the
/// command's own mount namespace. Returns the shares to unmount, innermost first, and for
/// each open path the folders to make (outermost first) before it is mounted again, since
/// unmounting a share can take away the folder an open path was in.
pub fn hand_mounts(shares: &[String], open: &[String]) -> (Vec<String>, Vec<Vec<String>>) {
    let mut hide: Vec<String> = shares.to_vec();
    hide.sort_by_key(|s| std::cmp::Reverse(s.len()));
    hide.dedup();
    let dirs = open
        .iter()
        .map(|p| {
            let mut acc = String::new();
            p.split('/')
                .filter(|c| !c.is_empty())
                .map(|c| {
                    acc.push('/');
                    acc.push_str(c);
                    acc.clone()
                })
                .collect()
        })
        .collect();
    (hide, dirs)
}

/// The person's temporary home in the VM: made at the first command of a take-over, removed
/// at hand-back, so a sign-in there does not outlive it.
pub const HAND_HOME: &str = "/tmp/tc-hand-home";

pub fn main() -> i32 {
    #[cfg(target_os = "linux")]
    {
        linux::main()
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("{NAME} runs only inside a Linux microVM");
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_persons_command_sees_only_the_open_shares() {
        let shares = vec![
            "/srv/runs/r1".to_string(),
            "/home/runner".to_string(),
            "/srv/lines/k/work".to_string(),
            "/srv/mirrors/m.git".to_string(),
        ];
        let open = vec![
            "/srv/runs/r1/work".to_string(),
            "/srv/mirrors/m.git".to_string(),
        ];
        let (hide, dirs) = hand_mounts(&shares, &open);
        // Innermost first, so a share inside another goes before it.
        assert_eq!(hide[0], "/srv/mirrors/m.git");
        assert_eq!(hide.last().unwrap(), "/home/runner");
        assert_eq!(hide.len(), 4);
        assert_eq!(
            dirs[0],
            vec!["/srv", "/srv/runs", "/srv/runs/r1", "/srv/runs/r1/work"]
        );
        assert_eq!(dirs[1].last().unwrap(), "/srv/mirrors/m.git");
        assert_eq!(hand_uid(1000), 1001);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use crate::executor::vmwire::{
        AGENT_PORT, CHAT_RUN_PORT, EXEC_OUT_CAP, EXEC_OUT_CHUNK, Exec, ExecCancel, ExecExit, Exit,
        Hold, Mount, Start, Tag, exec_out, read_frame, write_frame,
    };
    use std::collections::{HashMap, HashSet};
    use std::ffi::CString;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const VMADDR_CID_ANY: u32 = u32::MAX;
    const VMADDR_CID_HOST: u32 = 2;

    macro_rules! log {
        ($($a:tt)*) => { eprintln!("vm-agent: {}", format!($($a)*)) };
    }

    /// PID 1: a child process serves the run while this one reaps every orphan. When the
    /// server ends, the VM powers off (Firecracker exits on the reboot, kernel `reboot=k`).
    pub fn main() -> i32 {
        // SAFETY: plain fork before any thread exists.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            let code = match serve() {
                Ok(()) => 0,
                Err(e) => {
                    log!("{e}");
                    1
                }
            };
            // SAFETY: ends the server child without running the parent's destructors.
            unsafe { libc::_exit(code) };
        }
        if pid < 0 {
            log!("fork failed");
        }
        loop {
            let mut status = 0;
            // SAFETY: waits for any child; status is a valid out pointer.
            let p = unsafe { libc::waitpid(-1, &mut status, 0) };
            if p == pid
                || (p < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD))
            {
                break;
            }
        }
        // SAFETY: flush the shares, then the reboot that ends the VM.
        unsafe {
            libc::sync();
            libc::reboot(libc::RB_AUTOBOOT);
        }
        0
    }

    fn vsock(port: u32, cid: u32, listen: bool) -> std::io::Result<File> {
        // SAFETY: socket(2) with constant arguments.
        let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: fd is a new socket this File now owns.
        let sock = unsafe { File::from_raw_fd(fd) };
        // SAFETY: an all-zero sockaddr_vm is valid; the fields are set below.
        let mut addr: libc::sockaddr_vm = unsafe { std::mem::zeroed() };
        addr.svm_family = libc::AF_VSOCK as libc::sa_family_t;
        addr.svm_port = port;
        addr.svm_cid = cid;
        let len = std::mem::size_of::<libc::sockaddr_vm>() as libc::socklen_t;
        let ptr = &addr as *const libc::sockaddr_vm as *const libc::sockaddr;
        // SAFETY: ptr and len describe addr, which lives across the calls.
        let rc = unsafe {
            if listen {
                let r = libc::bind(fd, ptr, len);
                if r == 0 { libc::listen(fd, 4) } else { r }
            } else {
                libc::connect(fd, ptr, len)
            }
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(sock)
    }

    fn accept(listener: &File) -> std::io::Result<File> {
        use std::os::fd::AsRawFd;
        // SAFETY: accept on a listening socket; no address is asked for.
        let fd = unsafe {
            libc::accept4(
                listener.as_raw_fd(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                libc::SOCK_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: fd is the accepted connection, owned by this File.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn cstr(s: &str) -> std::io::Result<CString> {
        CString::new(s).map_err(|_| std::io::Error::other("nul in a path"))
    }

    fn mount_share(host_ip: &str, guest_ip: &str, m: &Mount) -> std::io::Result<()> {
        std::fs::create_dir_all(m.target())?;
        let source = cstr(&format!("{host_ip}:{}", m.path))?;
        let target = cstr(m.target())?;
        let fstype = cstr("nfs4")?;
        let data = cstr(&format!(
            "vers=4.2,addr={host_ip},clientaddr={guest_ip},hard,proto=tcp,timeo=600,retrans=2"
        ))?;
        let flags =
            if m.read_only { libc::MS_RDONLY } else { 0 } | libc::MS_NOSUID | libc::MS_NODEV;
        // SAFETY: every pointer is a valid C string that lives across the call.
        let rc = unsafe {
            libc::mount(
                source.as_ptr(),
                target.as_ptr(),
                fstype.as_ptr(),
                flags,
                data.as_ptr() as *const libc::c_void,
            )
        };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            return Err(std::io::Error::other(format!("mount {}: {e}", m.target())));
        }
        Ok(())
    }

    fn chown(path: &str, uid: u32, gid: u32) {
        if let Ok(p) = cstr(path) {
            // SAFETY: valid C string.
            unsafe { libc::chown(p.as_ptr(), uid, gid) };
        }
    }

    /// Relays connections to `path` in the VM to the host's `chat-run` port.
    fn relay_chat_run(path: &str, uid: u32, gid: u32) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::remove_file(path);
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir)?;
        }
        let listener = std::os::unix::net::UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        chown(path, uid, gid);
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let up = match vsock(CHAT_RUN_PORT, VMADDR_CID_HOST, false) {
                        Ok(s) => s,
                        Err(e) => {
                            log!("chat-run relay: {e}");
                            return;
                        }
                    };
                    let (Ok(mut c_r), Ok(mut u_w)) = (conn.try_clone(), up.try_clone()) else {
                        return;
                    };
                    let t = std::thread::spawn(move || {
                        let _ = std::io::copy(&mut c_r, &mut u_w);
                        // SAFETY: half-close the host side so it sees the request end.
                        unsafe {
                            libc::shutdown(std::os::fd::AsRawFd::as_raw_fd(&u_w), libc::SHUT_WR)
                        };
                    });
                    let (mut u_r, mut c_w) = (up, conn);
                    let _ = std::io::copy(&mut u_r, &mut c_w);
                    let _ = t.join();
                });
            }
        });
        Ok(())
    }

    fn serve() -> std::io::Result<()> {
        let listener = vsock(AGENT_PORT, VMADDR_CID_ANY, true)?;
        let conn = accept(&listener)?;
        drop(listener);
        let writer = Arc::new(Mutex::new(conn.try_clone()?));
        let send = {
            let w = writer.clone();
            move |tag: Tag, p: &[u8]| -> std::io::Result<()> {
                let mut w = w.lock().unwrap_or_else(|e| e.into_inner());
                write_frame(&mut *w, tag, p)
            }
        };
        let mut reader = conn;
        let start: Start = match read_frame(&mut reader)? {
            Some((Tag::Start, p)) => serde_json::from_slice(&p).map_err(std::io::Error::other)?,
            _ => return Err(std::io::Error::other("the first frame must be start")),
        };
        let pgid = match start_cli(&start, &send) {
            Ok(p) => p,
            Err(e) => {
                let _ = send(Tag::Error, e.to_string().as_bytes());
                let _ = send(Tag::StdoutEnd, b"");
                let exit = Exit {
                    code: None,
                    signal: None,
                };
                let _ = send(Tag::Exit, &serde_json::to_vec(&exit).unwrap_or_default());
                // Keep the stream until the daemon is done reading.
                while let Ok(Some(_)) = read_frame(&mut reader) {}
                return Err(e);
            }
        };
        let (child_pid, stdin) = pgid;
        let mut stdin = stdin;
        let mut hand = Hand::new(&start, child_pid);
        loop {
            match read_frame(&mut reader) {
                Ok(Some((Tag::Stdin, p))) => {
                    if let Some(s) = stdin.as_mut()
                        && s.write_all(&p).and_then(|_| s.flush()).is_err()
                    {
                        stdin = None;
                    }
                }
                Ok(Some((Tag::StdinEnd, _))) => stdin = None,
                Ok(Some((Tag::Signal, p))) => {
                    if let Ok(sig) = std::str::from_utf8(&p)
                        .unwrap_or_default()
                        .trim()
                        .parse::<i32>()
                    {
                        // SAFETY: a negative pid addresses the CLI's process group.
                        unsafe { libc::kill(-child_pid, sig) };
                    }
                }
                Ok(Some((Tag::Sync, _))) => {
                    // SAFETY: plain syscall.
                    unsafe { libc::sync() };
                    let _ = send(Tag::Synced, b"");
                }
                Ok(Some((Tag::Hold, p))) => {
                    if let Ok(h) = serde_json::from_slice::<Hold>(&p) {
                        hand.hold(h.stop);
                    }
                }
                Ok(Some((Tag::Exec, p))) => {
                    if let Ok(e) = serde_json::from_slice::<Exec>(&p) {
                        hand.exec(e, &send);
                    }
                }
                Ok(Some((Tag::ExecCancel, p))) => {
                    if let Ok(c) = serde_json::from_slice::<ExecCancel>(&p) {
                        hand.cancel(&c.id);
                    }
                }
                Ok(Some((Tag::Restore, _))) => hand.restore(),
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
        // The daemon is gone: nothing may keep running.
        hand.hold(false);
        // SAFETY: a negative pid addresses the CLI's process group.
        unsafe { libc::kill(-child_pid, libc::SIGKILL) };
        Ok(())
    }

    /// 接管 in the VM: the CLI's group and its tools are stopped while the VM runs on, and the
    /// person's commands run as another user (design-takeover 3.4, 3.8).
    struct Hand {
        cli: i32,
        run_dir: PathBuf,
        uid: u32,
        gid: u32,
        /// Where the shares are mounted in the VM.
        shares: Vec<String>,
        stopped: Vec<crate::proctree::Target>,
        held: bool,
        /// Work folders opened to the run's group.
        opened: HashSet<String>,
        home: Option<PathBuf>,
        /// The person's running commands: id → pid (also their process group).
        running: Arc<Mutex<HashMap<String, i32>>>,
    }

    impl Hand {
        fn new(start: &Start, cli: i32) -> Hand {
            Hand {
                cli,
                run_dir: Path::new(&start.stderr)
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default(),
                uid: start.uid,
                gid: start.gid,
                shares: start
                    .mounts
                    .iter()
                    .map(|m| m.target().to_string())
                    .collect(),
                stopped: vec![],
                held: false,
                opened: HashSet::new(),
                home: None,
                running: Arc::default(),
            }
        }

        /// SIGSTOP / SIGCONT the CLI's group and the tool groups below it.
        fn hold(&mut self, stop: bool) {
            if stop && !self.held {
                crate::proctree::stop(self.cli, &self.run_dir, &mut self.stopped);
                self.held = true;
            } else if !stop && self.held {
                crate::proctree::resume(self.cli, &self.stopped);
                self.stopped.clear();
                self.held = false;
            }
        }

        fn cancel(&self, id: &str) {
            let pid = self
                .running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .copied();
            if let Some(pid) = pid.filter(|p| *p > 1) {
                // SAFETY: a negative pid addresses the command's own group (it called setsid).
                unsafe { libc::kill(-pid, libc::SIGKILL) };
            }
        }

        /// The person's files in the open folders go back to the run's user; the temporary
        /// home goes.
        fn restore(&mut self) {
            let person = super::hand_uid(self.uid);
            for dir in &self.opened {
                chown_tree(Path::new(dir), person, self.uid, self.gid);
            }
            if let Some(h) = self.home.take() {
                let _ = std::fs::remove_dir_all(h);
            }
        }

        /// Opens `cwd` to the run's group (as the run's user, who owns it): read, write and
        /// enter, new files in the group. The member's home stays closed.
        fn open(&mut self, cwd: &str) -> bool {
            if self.opened.contains(cwd) {
                return true;
            }
            let ok = Command::new("/bin/sh")
                .args([
                    "-c",
                    "chmod -R g+rwX -- \"$1\" && find \"$1\" -type d -exec chmod g+s {} +",
                    "sh",
                    cwd,
                ])
                .uid(self.uid)
                .gid(self.gid)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                self.opened.insert(cwd.to_string());
            }
            ok
        }

        fn make_home(&mut self) -> std::io::Result<PathBuf> {
            use std::os::unix::fs::PermissionsExt;
            if let Some(h) = &self.home {
                return Ok(h.clone());
            }
            let h = PathBuf::from(super::HAND_HOME);
            let _ = std::fs::remove_dir_all(&h);
            std::fs::create_dir_all(&h)?;
            std::os::unix::fs::chown(&h, Some(super::hand_uid(self.uid)), Some(self.gid))?;
            std::fs::set_permissions(&h, std::fs::Permissions::from_mode(0o700))?;
            self.home = Some(h.clone());
            Ok(h)
        }

        fn exec<S>(&mut self, e: Exec, send: &S)
        where
            S: Fn(Tag, &[u8]) -> std::io::Result<()> + Clone + Send + 'static,
        {
            let started = Instant::now();
            let exit = |code: Option<i64>, error: Option<&str>, tail: String| {
                let x = ExecExit {
                    id: e.id.clone(),
                    code,
                    ms: started.elapsed().as_millis() as u64,
                    dropped: 0,
                    tail,
                    error: error.map(str::to_string),
                };
                let _ = send(Tag::ExecExit, &serde_json::to_vec(&x).unwrap_or_default());
            };
            if e.argv.is_empty() {
                return exit(None, None, "no command\n".into());
            }
            if !self.open(&e.cwd) {
                return exit(None, Some("unsupported"), String::new());
            }
            let home = match self.make_home() {
                Ok(h) => h,
                Err(err) => return exit(None, None, format!("no home folder: {err}\n")),
            };
            match spawn_hand(&e, &home, &self.shares, super::hand_uid(self.uid), self.gid) {
                Ok((child, r)) => {
                    let pid = child.id() as i32;
                    self.running
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(e.id.clone(), pid);
                    relay_hand(
                        e.id.clone(),
                        child,
                        r,
                        started,
                        self.running.clone(),
                        send.clone(),
                    );
                }
                Err(err) => exit(None, None, format!("cannot start the command: {err}\n")),
            }
        }
    }

    /// `chown` back to the run's user what `person` owns below `dir` (not following links).
    fn chown_tree(dir: &Path, person: u32, uid: u32, gid: u32) {
        use std::os::unix::fs::MetadataExt;
        let mut dirs = vec![dir.to_path_buf()];
        while let Some(d) = dirs.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for entry in rd.flatten() {
                let p = entry.path();
                let Ok(m) = std::fs::symlink_metadata(&p) else {
                    continue;
                };
                if m.uid() == person {
                    let _ = std::os::unix::fs::lchown(&p, Some(uid), Some(gid));
                }
                if m.is_dir() {
                    dirs.push(p);
                }
            }
        }
    }

    /// Starts the person's command: a session of its own, a mount namespace in which only the
    /// open shares are left, then the person's user. Returns the child and its output pipe.
    fn spawn_hand(
        e: &Exec,
        home: &Path,
        shares: &[String],
        uid: u32,
        gid: u32,
    ) -> std::io::Result<(std::process::Child, std::io::PipeReader)> {
        let (hide, dirs) = super::hand_mounts(shares, &e.open);
        // Every C string is made here: between fork and exec nothing may allocate.
        let stage_root = Path::new("/run/tc-hand");
        let mut stages = vec![];
        for i in 0..e.open.len() {
            let s = stage_root.join(i.to_string());
            std::fs::create_dir_all(&s)?;
            stages.push(cstr(&s.to_string_lossy())?);
        }
        let open: Vec<CString> = e.open.iter().map(|p| cstr(p)).collect::<Result<_, _>>()?;
        let hide: Vec<CString> = hide.iter().map(|p| cstr(p)).collect::<Result<_, _>>()?;
        let dirs: Vec<Vec<CString>> = dirs
            .iter()
            .map(|d| d.iter().map(|p| cstr(p)).collect::<Result<_, _>>())
            .collect::<Result<_, _>>()?;
        let cwd = cstr(&e.cwd)?;
        let root = cstr("/")?;
        let (r, w) = std::io::pipe()?;
        let w2 = w.try_clone()?;
        let mut cmd = Command::new(&e.argv[0]);
        cmd.args(&e.argv[1..])
            .env_clear()
            .envs(e.env.iter().map(|(k, v)| (k, v)))
            .env("HOME", home)
            .stdin(Stdio::null())
            .stdout(w)
            .stderr(w2);
        // SAFETY: only syscalls on the strings made above; no allocation.
        unsafe {
            cmd.pre_exec(move || {
                let err = || Err(std::io::Error::last_os_error());
                let null = std::ptr::null();
                if libc::setsid() < 0 || libc::unshare(libc::CLONE_NEWNS) != 0 {
                    return err();
                }
                if libc::mount(
                    null,
                    root.as_ptr(),
                    null,
                    libc::MS_REC | libc::MS_PRIVATE,
                    std::ptr::null(),
                ) != 0
                {
                    return err();
                }
                for (o, s) in open.iter().zip(&stages) {
                    if libc::mount(
                        o.as_ptr(),
                        s.as_ptr(),
                        null,
                        libc::MS_BIND,
                        std::ptr::null(),
                    ) != 0
                    {
                        return err();
                    }
                }
                for h in &hide {
                    libc::umount2(h.as_ptr(), libc::MNT_DETACH);
                }
                for ((o, s), d) in open.iter().zip(&stages).zip(&dirs) {
                    for p in d {
                        libc::mkdir(p.as_ptr(), 0o755);
                    }
                    if libc::mount(
                        s.as_ptr(),
                        o.as_ptr(),
                        null,
                        libc::MS_BIND,
                        std::ptr::null(),
                    ) != 0
                    {
                        return err();
                    }
                    libc::umount2(s.as_ptr(), libc::MNT_DETACH);
                }
                if libc::setgroups(0, std::ptr::null()) != 0
                    || libc::setgid(gid) != 0
                    || libc::setuid(uid) != 0
                    || libc::chdir(cwd.as_ptr()) != 0
                {
                    return err();
                }
                Ok(())
            });
        }
        let child = cmd.spawn();
        // The pipe's write ends live in `cmd` until it is dropped.
        drop(cmd);
        Ok((child?, r))
    }

    /// Relays a command's output as [`Tag::ExecOut`] (up to [`EXEC_OUT_CAP`], then only the
    /// last bytes, in [`Tag::ExecExit`]) and its end.
    fn relay_hand<S>(
        id: String,
        mut child: std::process::Child,
        mut r: std::io::PipeReader,
        started: Instant,
        running: Arc<Mutex<HashMap<String, i32>>>,
        send: S,
    ) where
        S: Fn(Tag, &[u8]) -> std::io::Result<()> + Clone + Send + 'static,
    {
        #[derive(Default)]
        struct Out {
            sent: usize,
            dropped: u64,
            tail: Vec<u8>,
            closed: bool,
        }
        let out = Arc::new(Mutex::new(Out::default()));
        let (eof_tx, eof_rx) = std::sync::mpsc::channel::<()>();
        {
            let (out, send, id) = (out.clone(), send.clone(), id.clone());
            std::thread::spawn(move || {
                let mut buf = vec![0u8; EXEC_OUT_CHUNK];
                loop {
                    let n = match r.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
                    if o.closed {
                        break;
                    }
                    let room = EXEC_OUT_CAP.saturating_sub(o.sent).min(n);
                    if room > 0 {
                        let _ = send(Tag::ExecOut, &exec_out(&id, &buf[..room]));
                        o.sent += room;
                    }
                    if room < n {
                        o.dropped += (n - room) as u64;
                        o.tail.extend_from_slice(&buf[room..n]);
                        let extra = o.tail.len().saturating_sub(EXEC_OUT_CHUNK);
                        o.tail.drain(..extra);
                    }
                }
                let _ = eof_tx.send(());
            });
        }
        std::thread::spawn(move || {
            let status = child.wait();
            running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            // A process left in the background can hold the pipe: wait only a little.
            let _ = eof_rx.recv_timeout(Duration::from_millis(500));
            let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
            o.closed = true;
            let x = ExecExit {
                id,
                code: status.ok().and_then(|s| s.code().map(i64::from)),
                ms: started.elapsed().as_millis() as u64,
                dropped: o.dropped,
                tail: String::from_utf8_lossy(&o.tail).into_owned(),
                error: None,
            };
            let _ = send(Tag::ExecExit, &serde_json::to_vec(&x).unwrap_or_default());
        });
    }

    /// Mounts the shares, starts the CLI and its stdout and exit relays. Returns the CLI's pid
    /// (also its process group) and its stdin.
    fn start_cli(
        start: &Start,
        send: &(impl Fn(Tag, &[u8]) -> std::io::Result<()> + Clone + Send + 'static),
    ) -> std::io::Result<(i32, Option<std::process::ChildStdin>)> {
        let mut mounts = start.mounts.clone();
        // Outer folders first: the CLIs' folders go on top of the bot's home folder.
        mounts.sort_by_key(|m| m.target().len());
        for m in &mounts {
            mount_share(&start.host_ip, &start.guest_ip, m)?;
        }
        if let Some((_, home)) = start.env.iter().find(|(k, _)| k == "HOME")
            && std::fs::create_dir_all(home).is_ok()
        {
            chown(home, start.uid, start.gid);
        }
        if let Some(sock) = &start.chat_run_socket {
            relay_chat_run(sock, start.uid, start.gid)?;
        }
        let program =
            if start.program.contains('/') && !std::path::Path::new(&start.program).exists() {
                std::path::Path::new(&start.program)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                start.program.clone()
            };
        let stderr = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&start.stderr)?;
        let mut child = Command::new(&program)
            .args(&start.args)
            .current_dir(&start.cwd)
            .env_clear()
            .envs(start.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .uid(start.uid)
            .gid(start.gid)
            .process_group(0)
            .spawn()
            .map_err(|e| std::io::Error::other(format!("cannot start {program}: {e}")))?;
        let pid = child.id() as i32;
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().expect("piped stdout");
        let out = send.clone();
        let reader = std::thread::spawn(move || {
            let mut buf = vec![0u8; 64 << 10];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if out(Tag::Stdout, &buf[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = out(Tag::StdoutEnd, b"");
        });
        let done = send.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            // Helpers the CLI left in its group hold stdout open: they go with the CLI, as on
            // the host.
            // SAFETY: a negative pid addresses the CLI's process group.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            let _ = reader.join();
            let exit = match status {
                Ok(s) => Exit {
                    code: s.code().map(i64::from),
                    signal: s.signal(),
                },
                Err(_) => Exit {
                    code: None,
                    signal: None,
                },
            };
            let _ = done(Tag::Exit, &serde_json::to_vec(&exit).unwrap_or_default());
        });
        Ok((pid, stdin))
    }
}
