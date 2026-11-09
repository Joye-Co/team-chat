//! `firecracker`: each run's CLI runs in its own Firecracker microVM (`[vm]` in `daemon.toml`).
//!
//! The VM boots the read-only base image with a per-run scratch disk as its writable layer and
//! the tools disk (this binary as `vm-agent` and `chat-run`). The run folder, the run's git dir
//! and the CLIs' config folders (`[vm].shares`) are NFS shares of this host, mounted in the VM
//! at their host paths, so everything the daemon reads and writes (worktree, session files,
//! 产出, `stderr.log`) stays where it is. Stdin and stdout go over vsock to `vm-agent`
//! ([`super::vmwire`]); `chat-run` reaches the run's socket through vsock as well.
//!
//! Root-only steps (the VM's tap device, NFS exports) go through `[vm].helper` with `sudo -n`
//! (`scripts/runner/host-setup.sh` installs it). The network rules are static: VMs reach the
//! internet, never this host (except NFS), private ranges or each other.

use super::vmwire::{self, AGENT_PORT, CHAT_RUN_PORT, Tag};
use super::{BoxFuture, Executor, Process, Spawned, Stopped};
use crate::config::VmConfig;
use crate::runtime::RunSpec;
use crate::workspace::{self, RunMeta};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

/// `PATH` of the CLI in the VM: the tools disk first (`chat-run`), then the base image.
pub const GUEST_PATH: &str =
    "/opt/runner/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
/// The `chat-run` socket in the VM (relayed to the run's socket on the host).
pub const GUEST_CHAT_RUN_SOCKET: &str = "/run/chat-run.sock";
/// How long a VM may take from start to a connected agent.
const BOOT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct Firecracker {
    cfg: VmConfig,
    slots: Arc<Mutex<Vec<bool>>>,
    tools: Mutex<Option<PathBuf>>,
}

impl Firecracker {
    pub fn new(cfg: VmConfig, slots: u32) -> Firecracker {
        Firecracker {
            cfg,
            slots: Arc::new(Mutex::new(vec![false; slots.max(1) as usize])),
            tools: Mutex::new(None),
        }
    }

    fn take_slot(&self) -> Option<u32> {
        let mut s = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let i = s.iter().position(|used| !used)?;
        s[i] = true;
        Some(i as u32)
    }

    /// The tools disk of this daemon binary: made once per binary, next to the VMs.
    fn tools_disk(&self) -> std::io::Result<PathBuf> {
        let mut cached = self.tools.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = cached.as_ref().filter(|p| p.is_file()) {
            return Ok(p.clone());
        }
        let exe = std::env::current_exe()?;
        let bytes = std::fs::read(&exe)?;
        let h = workspace::hex(&Sha256::digest(&bytes))[..16].to_string();
        std::fs::create_dir_all(&self.cfg.dir)?;
        let disk = self.cfg.dir.join(format!("tools-{h}.ext4"));
        if !disk.is_file() {
            make_tools_disk(&bytes, &disk)?;
        }
        *cached = Some(disk.clone());
        Ok(disk)
    }
}

fn make_tools_disk(exe: &[u8], disk: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let stage = disk.with_extension(format!("stage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(stage.join("bin"))?;
    let agent = stage.join(crate::vm_agent::NAME);
    std::fs::write(&agent, exe)?;
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755))?;
    std::os::unix::fs::symlink(
        format!("/opt/runner/{}", crate::vm_agent::NAME),
        stage.join("bin").join(crate::chat_run::NAME),
    )?;
    let tmp = disk.with_extension("tmp");
    let size_mb = exe.len() / (1 << 20) * 2 + 16;
    let out = std::process::Command::new("mkfs.ext4")
        .args(["-q", "-F", "-L", "runner-tools", "-d"])
        .arg(&stage)
        .arg(&tmp)
        .arg(format!("{size_mb}M"))
        .output();
    let _ = std::fs::remove_dir_all(&stage);
    let out = out?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "mkfs.ext4 for the tools disk: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    std::fs::rename(&tmp, disk)
}

/// The host and guest addresses of a slot: a /30 per VM in 10.200.0.0/16.
pub fn addrs(slot: u32) -> (String, String) {
    let base = slot * 4;
    let (hi, lo) = (base >> 8, base & 0xff);
    (
        format!("10.200.{hi}.{}", lo + 1),
        format!("10.200.{hi}.{}", lo + 2),
    )
}

pub fn tap_name(slot: u32) -> String {
    format!("rvm{slot}")
}

/// The CLI's environment in the VM: its own `PATH`, the relayed `chat-run` socket, and no
/// host-only sockets or temp folder. Returns the host's `chat-run` socket when the run has one.
pub fn guest_env(env: &[(String, String)]) -> (Vec<(String, String)>, Option<String>) {
    let mut host_socket = None;
    let mut out = vec![];
    for (k, v) in env {
        match k.as_str() {
            "PATH" | "TMPDIR" | "SSH_AUTH_SOCK" => {}
            k if k == crate::chat_run::ENV_SOCKET => host_socket = Some(v.clone()),
            _ => out.push((k.clone(), v.clone())),
        }
    }
    out.push(("PATH".into(), GUEST_PATH.into()));
    if host_socket.is_some() {
        out.push((
            crate::chat_run::ENV_SOCKET.into(),
            GUEST_CHAT_RUN_SOCKET.into(),
        ));
    }
    (out, host_socket)
}

/// Runs `[vm].helper <args>` as root (sudo without a password prompt).
async fn helper(cfg: &VmConfig, args: &[&str]) -> Result<(), String> {
    let out = tokio::process::Command::new("sudo")
        .arg("-n")
        .arg(&cfg.helper)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("{}: {e}", cfg.helper))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} {}: {}",
            cfg.helper,
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn helper_blocking(cfg: &VmConfig, args: &[&str]) {
    let _ = std::process::Command::new("sudo")
        .arg("-n")
        .arg(&cfg.helper)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// One request to the Firecracker API socket.
fn api(sock: &Path, method: &str, path: &str, body: &str) -> Result<(), String> {
    let mut s = std::os::unix::net::UnixStream::connect(sock).map_err(|e| e.to_string())?;
    let t = Some(Duration::from_secs(3));
    let _ = s.set_read_timeout(t);
    let _ = s.set_write_timeout(t);
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    let mut head = [0u8; 12];
    s.read_exact(&mut head).map_err(|e| e.to_string())?;
    let status = String::from_utf8_lossy(&head[9..12]).to_string();
    if status.starts_with('2') {
        Ok(())
    } else {
        Err(format!("firecracker api {method} {path}: {status}"))
    }
}

fn vm_state(sock: &Path, state: &str) -> Result<(), String> {
    api(sock, "PATCH", "/vm", &format!(r#"{{"state":"{state}"}}"#))
}

impl Executor for Firecracker {
    fn spawn(&self, spec: &RunSpec, program: &str, args: &[String]) -> std::io::Result<Spawned> {
        let tools = self.tools_disk()?;
        let slot = self
            .take_slot()
            .ok_or_else(|| std::io::Error::other("no free VM slot"))?;
        std::fs::File::create(spec.run_dir.join("stderr.log"))?;
        let (stdin_host, stdin_vm) = tokio::io::duplex(1 << 16);
        let (stdout_vm, stdout_host) = tokio::io::duplex(1 << 16);
        let (ctl_tx, ctl_rx) = mpsc::unbounded_channel();
        let (exit_tx, exit_rx) = watch::channel(None);
        let job = Job {
            cfg: self.cfg.clone(),
            slots: self.slots.clone(),
            slot,
            tools,
            vm_dir: self.cfg.dir.join(&spec.run_id),
            run_dir: spec.run_dir.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            program: program.to_string(),
            args: args.to_vec(),
        };
        tokio::spawn(job.run(stdin_vm, stdout_vm, ctl_rx, exit_tx));
        Ok(Spawned {
            stdin: Some(Box::new(stdin_host)),
            stdout: Box::new(stdout_host),
            process: Box::new(VmProcess {
                ctl: ctl_tx,
                exit: exit_rx,
            }),
        })
    }

    fn stop(&self, run_dir: &Path) -> Option<Box<dyn Stopped>> {
        let sock = self.cfg.dir.join(run_dir.file_name()?).join("api.sock");
        if !sock.exists() {
            return None;
        }
        match vm_state(&sock, "Paused") {
            Ok(()) => Some(Box::new(VmStopped { sock })),
            Err(e) => {
                eprintln!("{} pause: {e}", crate::proto::now_ms());
                None
            }
        }
    }

    fn kill_leftover(&self, meta: &RunMeta) {
        super::local::Local.kill_leftover(meta);
        let dir = self.cfg.dir.join(&meta.run_id);
        if let Some(slot) = std::fs::read_to_string(dir.join("slot"))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
        {
            helper_blocking(&self.cfg, &["down", &slot.to_string()]);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn isolated(&self) -> bool {
        true
    }
}

struct VmStopped {
    sock: PathBuf,
}

impl Stopped for VmStopped {
    fn resume(&self) {
        if let Err(e) = vm_state(&self.sock, "Resumed") {
            eprintln!("{} resume: {e}", crate::proto::now_ms());
        }
    }
}

enum Ctl {
    Signal(i32),
    Destroy,
}

/// The handle the runtime holds. `exit` is None while the CLI runs, then its exit code.
struct VmProcess {
    ctl: mpsc::UnboundedSender<Ctl>,
    exit: watch::Receiver<Option<Option<i64>>>,
}

impl VmProcess {
    async fn wait_exit(&mut self, limit: Duration) -> Option<Option<i64>> {
        let r = tokio::time::timeout(limit, self.exit.wait_for(|v| v.is_some())).await;
        match r {
            Ok(Ok(v)) => *v,
            // The job ended without an exit (cannot happen: it always sets one) or timed out.
            Ok(Err(_)) => Some(None),
            Err(_) => None,
        }
    }

    async fn kill_vm(&mut self, grace: Duration) -> Option<i64> {
        let _ = self.ctl.send(Ctl::Signal(libc::SIGTERM));
        if let Some(code) = self.wait_exit(grace).await {
            return code;
        }
        let _ = self.ctl.send(Ctl::Destroy);
        self.wait_exit(Duration::from_secs(10)).await.flatten()
    }
}

impl Process for VmProcess {
    fn signal(&self, sig: i32) {
        let _ = self.ctl.send(Ctl::Signal(sig));
    }

    fn kill(&mut self, grace: Duration) -> BoxFuture<'_, Option<i64>> {
        Box::pin(self.kill_vm(grace))
    }

    fn finish(&mut self, limit: Duration) -> BoxFuture<'_, Option<i64>> {
        Box::pin(async move {
            match self.wait_exit(limit).await {
                Some(code) => code,
                None => self.kill_vm(Duration::from_secs(3)).await,
            }
        })
    }

    fn kill_now(&mut self) {
        let _ = self.ctl.send(Ctl::Destroy);
    }
}

/// One VM from boot to teardown.
struct Job {
    cfg: VmConfig,
    slots: Arc<Mutex<Vec<bool>>>,
    slot: u32,
    tools: PathBuf,
    vm_dir: PathBuf,
    run_dir: PathBuf,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    program: String,
    args: Vec<String>,
}

impl Job {
    fn note(&self, msg: &str) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(self.run_dir.join("stderr.log"))
        {
            let _ = writeln!(f, "[vm] {msg}");
        }
    }

    fn console_tail(&self) -> String {
        let s = std::fs::read(self.vm_dir.join("console.log")).unwrap_or_default();
        let start = s.len().saturating_sub(1500);
        String::from_utf8_lossy(&s[start..]).trim().to_string()
    }

    /// The paths shared with the VM: the run folder, the run's git dir, the CLIs' folders.
    fn shares(&self) -> Vec<PathBuf> {
        let mut out = vec![self.run_dir.clone()];
        if let Some(gd) = workspace::read_meta(&self.run_dir).and_then(|m| m.git_dir) {
            out.push(gd);
        }
        for s in &self.cfg.shares {
            if s.is_dir() {
                out.push(s.clone());
            }
        }
        out.iter()
            .filter_map(|p| std::fs::canonicalize(p).ok())
            .fold(vec![], |mut acc: Vec<PathBuf>, p| {
                if !acc.iter().any(|a| p.starts_with(a)) {
                    acc.retain(|a| !a.starts_with(&p));
                    acc.push(p);
                }
                acc
            })
    }

    async fn boot(
        &self,
    ) -> Result<(tokio::process::Child, tokio::net::UnixStream, Vec<PathBuf>), String> {
        let slot = self.slot.to_string();
        // A crash can leave the slot's tap and exports behind.
        let _ = helper(&self.cfg, &["down", &slot]).await;
        helper(&self.cfg, &["up", &slot]).await?;
        let shares = self.shares();
        for s in &shares {
            helper(&self.cfg, &["export", &slot, &s.to_string_lossy(), "rw"]).await?;
        }
        let _ = std::fs::remove_dir_all(&self.vm_dir);
        std::fs::create_dir_all(&self.vm_dir).map_err(|e| format!("vm folder: {e}"))?;
        std::fs::write(self.vm_dir.join("slot"), &slot).map_err(|e| e.to_string())?;
        let scratch = self.vm_dir.join("scratch.ext4");
        std::fs::File::create(&scratch)
            .and_then(|f| f.set_len(u64::from(self.cfg.scratch_gb) << 30))
            .map_err(|e| format!("scratch disk: {e}"))?;
        let out = tokio::process::Command::new("mkfs.ext4")
            .args([
                "-q",
                "-F",
                "-L",
                "runner-scratch",
                "-E",
                "lazy_itable_init=1,lazy_journal_init=1",
            ])
            .arg(&scratch)
            .output()
            .await
            .map_err(|e| format!("mkfs.ext4: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "mkfs.ext4: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let vsock = self.vm_dir.join("v.sock");
        if let Some(host) = guest_env(&self.env).1 {
            let link = PathBuf::from(format!("{}_{CHAT_RUN_PORT}", vsock.display()));
            std::os::unix::fs::symlink(host, link).map_err(|e| e.to_string())?;
        }
        let rootfs = std::fs::canonicalize(&self.cfg.rootfs).map_err(|e| format!("rootfs: {e}"))?;
        let (host_ip, guest_ip) = addrs(self.slot);
        let ip: Vec<u8> = guest_ip.split('.').filter_map(|p| p.parse().ok()).collect();
        let mac = format!(
            "06:00:{:02x}:{:02x}:{:02x}:{:02x}",
            ip[0], ip[1], ip[2], ip[3]
        );
        let config = serde_json::json!({
            "boot-source": {
                "kernel_image_path": self.cfg.kernel,
                "boot_args": format!(
                    "console=ttyS0 reboot=k panic=1 pci=off ro init=/sbin/vm-init ip={guest_ip}::{host_ip}:255.255.255.252::eth0:off"
                ),
            },
            "drives": [
                { "drive_id": "root", "path_on_host": rootfs, "is_root_device": true, "is_read_only": true },
                { "drive_id": "tools", "path_on_host": self.tools, "is_root_device": false, "is_read_only": true },
                { "drive_id": "scratch", "path_on_host": scratch, "is_root_device": false, "is_read_only": false },
            ],
            "network-interfaces": [
                { "iface_id": "eth0", "host_dev_name": tap_name(self.slot), "guest_mac": mac },
            ],
            "vsock": { "guest_cid": 3, "uds_path": vsock },
            "machine-config": { "vcpu_count": self.cfg.vcpus, "mem_size_mib": self.cfg.mem_mib },
        });
        let config_path = self.vm_dir.join("vm.json");
        std::fs::write(&config_path, config.to_string()).map_err(|e| e.to_string())?;
        let console =
            std::fs::File::create(self.vm_dir.join("console.log")).map_err(|e| e.to_string())?;
        let console2 = console.try_clone().map_err(|e| e.to_string())?;
        let mut child = tokio::process::Command::new(&self.cfg.firecracker)
            .arg("--api-sock")
            .arg(self.vm_dir.join("api.sock"))
            .arg("--config-file")
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(console))
            .stderr(Stdio::from(console2))
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("{}: {e}", self.cfg.firecracker))?;
        let pid = child.id().map(|p| p as i32).unwrap_or(0);
        workspace::record_process(&self.run_dir, pid, &self.cfg.firecracker);

        let started = tokio::time::Instant::now();
        loop {
            if let Ok(Some(st)) = child.try_wait() {
                return Err(format!(
                    "the VM stopped while booting ({st}): {}",
                    self.console_tail()
                ));
            }
            if started.elapsed() > BOOT_TIMEOUT {
                let _ = child.start_kill();
                return Err(format!(
                    "the VM did not start in 30 s: {}",
                    self.console_tail()
                ));
            }
            if let Ok(s) = connect_agent(&vsock).await {
                return Ok((child, s, shares));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn run(
        self,
        mut stdin_vm: tokio::io::DuplexStream,
        mut stdout_vm: tokio::io::DuplexStream,
        mut ctl: mpsc::UnboundedReceiver<Ctl>,
        exit: watch::Sender<Option<Option<i64>>>,
    ) {
        let booted = tokio::time::Instant::now();
        let (mut child, stream, shares) = match self.boot().await {
            Ok(b) => b,
            Err(e) => {
                self.note(&e);
                drop(stdout_vm);
                let _ = exit.send(Some(None));
                self.teardown(None).await;
                return;
            }
        };
        let (mut rd, mut wr) = stream.into_split();
        // Reading a frame is not cancel-safe: one task reads, the loop takes whole frames.
        let (frames_tx, mut frames) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok(Some(f)) = vmwire::read_frame_async(&mut rd).await {
                if frames_tx.send(f).is_err() {
                    break;
                }
            }
        });
        let (env, host_socket) = guest_env(&self.env);
        let (host_ip, guest_ip) = addrs(self.slot);
        let start = vmwire::Start {
            program: self.program.clone(),
            args: self.args.clone(),
            env,
            cwd: self.cwd.to_string_lossy().to_string(),
            stderr: self
                .run_dir
                .join("stderr.log")
                .to_string_lossy()
                .to_string(),
            // SAFETY: plain getters.
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            host_ip,
            guest_ip,
            mounts: shares
                .iter()
                .map(|p| vmwire::Mount {
                    path: p.to_string_lossy().to_string(),
                    read_only: false,
                })
                .collect(),
            chat_run_socket: host_socket.map(|_| GUEST_CHAT_RUN_SOCKET.to_string()),
        };
        let payload = serde_json::to_vec(&start).unwrap_or_default();
        if let Err(e) = vmwire::write_frame_async(&mut wr, Tag::Start, &payload).await {
            self.note(&format!("cannot reach vm-agent: {e}"));
            drop(stdout_vm);
            let _ = exit.send(Some(None));
            self.teardown(Some(child)).await;
            return;
        }
        eprintln!(
            "{} run {}: VM {} up in {} ms",
            crate::proto::now_ms(),
            self.run_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            self.slot,
            booted.elapsed().as_millis()
        );

        let mut stdout_open = true;
        let mut stdin_open = true;
        let mut pending_exit: Option<Option<i64>> = None;
        let mut sync_deadline: Option<tokio::time::Instant> = None;
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let sync_wait = async {
                match sync_deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                f = frames.recv() => match f {
                    Some((Tag::Stdout, p)) => {
                        if stdout_open && stdout_vm.write_all(&p).await.is_err() {
                            stdout_open = false;
                        }
                    }
                    Some((Tag::StdoutEnd, _)) => {
                        let _ = stdout_vm.shutdown().await;
                        stdout_open = false;
                    }
                    Some((Tag::Exit, p)) => {
                        let e: Option<vmwire::Exit> = serde_json::from_slice(&p).ok();
                        pending_exit = Some(e.and_then(|e| e.code));
                        // Shared files the CLI wrote reach the host before the run reads them.
                        let _ = vmwire::write_frame_async(&mut wr, Tag::Sync, b"").await;
                        sync_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(5));
                    }
                    Some((Tag::Synced, _)) => {
                        if let Some(code) = pending_exit.take() {
                            sync_deadline = None;
                            let _ = exit.send(Some(code));
                        }
                    }
                    Some((Tag::Error, p)) => self.note(&String::from_utf8_lossy(&p)),
                    Some(_) => {}
                    None => break,
                },
                _ = sync_wait => {
                    sync_deadline = None;
                    if let Some(code) = pending_exit.take() {
                        let _ = exit.send(Some(code));
                    }
                }
                n = stdin_vm.read(&mut buf), if stdin_open => match n {
                    Ok(0) | Err(_) => {
                        stdin_open = false;
                        let _ = vmwire::write_frame_async(&mut wr, Tag::StdinEnd, b"").await;
                    }
                    Ok(n) => {
                        let _ = vmwire::write_frame_async(&mut wr, Tag::Stdin, &buf[..n]).await;
                    }
                },
                c = ctl.recv() => match c {
                    Some(Ctl::Signal(sig)) => {
                        let _ = vmwire::write_frame_async(&mut wr, Tag::Signal, sig.to_string().as_bytes()).await;
                    }
                    Some(Ctl::Destroy) | None => break,
                },
                _ = child.wait() => {
                    self.note(&format!("the VM stopped: {}", self.console_tail()));
                    break;
                }
            }
        }
        drop(stdout_vm);
        if exit.borrow().is_none() {
            let _ = exit.send(Some(pending_exit.unwrap_or(None)));
        }
        self.teardown(Some(child)).await;
    }

    async fn teardown(&self, child: Option<tokio::process::Child>) {
        if let Some(mut c) = child {
            let _ = c.start_kill();
            let _ = c.wait().await;
        }
        if let Err(e) = helper(&self.cfg, &["down", &self.slot.to_string()]).await {
            eprintln!("{} vm slot {}: {e}", crate::proto::now_ms(), self.slot);
        }
        let _ = tokio::fs::remove_dir_all(&self.vm_dir).await;
        let mut s = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(used) = s.get_mut(self.slot as usize) {
            *used = false;
        }
    }
}

/// Firecracker's host side of vsock: connect to the unix socket, ask for the guest port.
async fn connect_agent(vsock: &Path) -> std::io::Result<tokio::net::UnixStream> {
    let mut s = tokio::net::UnixStream::connect(vsock).await?;
    s.write_all(format!("CONNECT {AGENT_PORT}\n").as_bytes())
        .await?;
    let mut line = vec![];
    let read = async {
        let mut b = [0u8; 1];
        loop {
            s.read_exact(&mut b).await?;
            if b[0] == b'\n' || line.len() > 64 {
                break;
            }
            line.push(b[0]);
        }
        std::io::Result::Ok(())
    };
    tokio::time::timeout(Duration::from_secs(2), read)
        .await
        .map_err(|_| std::io::Error::other("no answer"))??;
    if line.starts_with(b"OK") {
        Ok(s)
    } else {
        Err(std::io::Error::other("agent not listening"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_get_their_own_slash_30() {
        assert_eq!(addrs(0), ("10.200.0.1".into(), "10.200.0.2".into()));
        assert_eq!(addrs(1), ("10.200.0.5".into(), "10.200.0.6".into()));
        assert_eq!(addrs(64), ("10.200.1.1".into(), "10.200.1.2".into()));
        assert_eq!(tap_name(7), "rvm7");
    }

    #[test]
    fn guest_env_replaces_path_and_relays_chat_run() {
        let env = vec![
            ("PATH".to_string(), "/run/bin:/usr/bin".to_string()),
            ("HOME".to_string(), "/home/runner".to_string()),
            ("TMPDIR".to_string(), "/var/folders/x".to_string()),
            (
                crate::chat_run::ENV_SOCKET.to_string(),
                "/srv/r/chat-run.sock".to_string(),
            ),
        ];
        let (g, host) = guest_env(&env);
        assert_eq!(host.as_deref(), Some("/srv/r/chat-run.sock"));
        let get = |k: &str| g.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("PATH"), Some(GUEST_PATH));
        assert_eq!(get("HOME"), Some("/home/runner"));
        assert_eq!(get("TMPDIR"), None);
        assert_eq!(
            get(crate::chat_run::ENV_SOCKET),
            Some(GUEST_CHAT_RUN_SOCKET)
        );
    }
}
