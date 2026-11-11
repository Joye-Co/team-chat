//! `vm-agent`: PID 1 of a run's microVM (the daemon binary started by that name from the tools
//! disk; `vm-init` in the base image execs it).
//!
//! It waits for the daemon on vsock port [`AGENT_PORT`], mounts the run's NFS shares at their
//! host paths, starts the CLI as the run's user and relays its stdin and stdout as frames (see
//! [`crate::executor::vmwire`]). The CLI's stderr goes straight to the run's `stderr.log` on
//! its share. When the daemon closes the stream the agent kills what is left and powers off.

/// The name the agent is started by.
pub const NAME: &str = "vm-agent";

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

#[cfg(target_os = "linux")]
mod linux {
    use crate::executor::vmwire::{
        AGENT_PORT, CHAT_RUN_PORT, Exit, Mount, Start, Tag, read_frame, write_frame,
    };
    use std::ffi::CString;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};

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
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
        // The daemon is gone: nothing may keep running.
        // SAFETY: a negative pid addresses the CLI's process group.
        unsafe { libc::kill(-child_pid, libc::SIGKILL) };
        Ok(())
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
