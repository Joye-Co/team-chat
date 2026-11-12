//! `firecracker`: each run's CLI runs in its own Firecracker microVM (`[vm]` in `daemon.toml`).
//!
//! The VM boots the read-only base image with a per-run scratch disk as its writable layer and
//! the tools disk (this binary as `vm-agent`, `chat-run` and the `gh` proxy of feature
//! "github", which finds the image's real `gh` after `/opt/runner/bin` on `GUEST_PATH`). The
//! run folder, the working folder, the run's git dir and the run's `vm_shares` (its CLIs'
//! folders, [`crate::vm_login`]) are NFS shares of this host, mounted in the VM at their host
//! paths, so everything the daemon reads and writes (worktree, session files, 产出,
//! `stderr.log`) stays where it is. The VM's `$HOME` is on its scratch disk and goes with it.
//! This host's CLI sign-in folders are never shared: the run has a copy of the sign-in it
//! cannot use to change them ([`crate::vm_login`]). Stdin and stdout go over vsock to `vm-agent`
//! ([`super::vmwire`]); `chat-run` reaches the run's socket through vsock as well.
//!
//! Root-only steps (the VM's tap device, NFS exports) go through
//! `[vm].helper` with `sudo -n` (`scripts/runner/host-setup.sh` installs it). The network rules
//! are static: VMs reach the internet, never this host (except NFS on their own tap's address),
//! private ranges or each other.

use super::vmwire::{self, AGENT_PORT, CHAT_RUN_PORT, Tag};
use super::{BoxFuture, Executor, HandChild, HandEnd, HandReq, Process, Spawned, Stopped};
use crate::config::VmConfig;
use crate::runtime::RunSpec;
use crate::workspace::{self, RunMeta};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

/// `PATH` of the CLI in the VM: the tools disk first (`chat-run`), then the base image, its
/// rustup proxies last. The repository's cache folders go right after the tools disk
/// ([`crate::vm_cache::ENV_PATH_EXTRA`]).
pub const GUEST_PATH: &str = "/opt/runner/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/opt/rust/cargo/bin";
/// The `chat-run` socket in the VM (relayed to the run's socket on the host).
pub const GUEST_CHAT_RUN_SOCKET: &str = "/run/chat-run.sock";
/// How long a VM may take from start to a connected agent.
const BOOT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct Firecracker {
    cfg: VmConfig,
    slots: Arc<Mutex<Vec<bool>>>,
    tools: Mutex<Option<PathBuf>>,
    /// Each running VM's control channel by run folder (接管 reaches the VM by it).
    procs: Procs,
    /// Ids of the person's commands on the wire.
    execs: std::sync::atomic::AtomicU64,
}

/// Weak: the run's [`VmProcess`] alone keeps a VM's control channel open, and when it is
/// dropped the VM goes.
type Procs = Arc<Mutex<HashMap<PathBuf, mpsc::WeakUnboundedSender<Ctl>>>>;

impl Firecracker {
    pub fn new(cfg: VmConfig, slots: u32) -> Firecracker {
        Firecracker {
            cfg,
            slots: Arc::new(Mutex::new(vec![false; slots.max(1) as usize])),
            tools: Mutex::new(None),
            procs: Arc::default(),
            execs: std::sync::atomic::AtomicU64::new(1),
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

/// The files of the tools disk (mounted at `/opt/runner`): the binary as `vm-agent`, and in
/// `bin/` the names it is started by in a run.
fn stage_tools(exe: &[u8], stage: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::remove_dir_all(stage);
    std::fs::create_dir_all(stage.join("bin"))?;
    let agent = stage.join(crate::vm_agent::NAME);
    std::fs::write(&agent, exe)?;
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755))?;
    // `gh` is the proxy of feature "github" and a shim; the others are shims of gated
    // actions (crate::shim), each finding the image's real program after the tools folder.
    let names = std::iter::once(crate::chat_run::NAME).chain(crate::shim::NAMES.iter().copied());
    for name in names {
        std::os::unix::fs::symlink(
            format!("/opt/runner/{}", crate::vm_agent::NAME),
            stage.join("bin").join(name),
        )?;
    }
    Ok(())
}

fn make_tools_disk(exe: &[u8], disk: &Path) -> std::io::Result<()> {
    let stage = disk.with_extension(format!("stage-{}", std::process::id()));
    stage_tools(exe, &stage)?;
    let tmp = disk.with_extension("tmp");
    let size_mb = exe.len() / (1 << 20) * 2 + 16;
    let out = std::process::Command::new(mkfs_ext4())
        .args(["-q", "-F", "-L", "runner-tools", "-d"])
        .arg(&stage)
        .arg(&tmp)
        .arg(format!("{size_mb}M"))
        .output();
    let _ = std::fs::remove_dir_all(&stage);
    let out =
        out.map_err(|e| std::io::Error::other(format!("mkfs.ext4 for the tools disk: {e}")))?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "mkfs.ext4 for the tools disk: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    std::fs::rename(&tmp, disk)
}

/// `mkfs.ext4`: on PATH, else in the system folders a service PATH often lacks.
pub fn mkfs_ext4() -> PathBuf {
    crate::health::which("mkfs.ext4")
        .or_else(|| {
            ["/usr/sbin/mkfs.ext4", "/sbin/mkfs.ext4"]
                .iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("mkfs.ext4"))
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
    let mut path_extra = None;
    let mut out = vec![];
    for (k, v) in env {
        match k.as_str() {
            "PATH" | "TMPDIR" | "SSH_AUTH_SOCK" => {}
            k if k == crate::chat_run::ENV_SOCKET => host_socket = Some(v.clone()),
            k if k == crate::vm_cache::ENV_PATH_EXTRA => path_extra = Some(v.clone()),
            _ => out.push((k.clone(), v.clone())),
        }
    }
    let path = match path_extra.filter(|x| !x.is_empty()) {
        Some(x) => match GUEST_PATH.split_once(':') {
            Some((tools, rest)) => format!("{tools}:{x}:{rest}"),
            None => format!("{GUEST_PATH}:{x}"),
        },
        None => GUEST_PATH.to_string(),
    };
    out.push(("PATH".into(), path));
    if host_socket.is_some() {
        out.push((
            crate::chat_run::ENV_SOCKET.into(),
            GUEST_CHAT_RUN_SOCKET.into(),
        ));
    }
    (out, host_socket)
}

/// Runs `[vm].helper <args>` as root (sudo without a password prompt).
pub(crate) async fn helper(cfg: &VmConfig, args: &[&str]) -> Result<(), String> {
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

/// `PATH` of a person's command in the VM: the base image only, without the tools disk
/// (`chat-run`, the `gh` proxy and the gate shims are the AI member's).
pub const HAND_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// The [`Tag::Exec`] of a person's command: `bash -c` in the work folder with only `PATH`,
/// `LANG` and `TERM=dumb` (the agent adds `HOME`); the work folder and the run's git dir are
/// the only shares it sees.
pub fn hand_exec(id: &str, req: &HandReq, git_dir: Option<&Path>) -> vmwire::Exec {
    let lang = req
        .env
        .iter()
        .find(|(k, _)| k == "LANG")
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty() && v.len() < 64)
        .unwrap_or_else(|| "C.UTF-8".into());
    let cwd = req.cwd.to_string_lossy().to_string();
    vmwire::Exec {
        id: id.to_string(),
        argv: vec!["/bin/bash".into(), "-c".into(), req.command.clone()],
        cwd: cwd.clone(),
        env: vec![
            ("PATH".into(), HAND_PATH.into()),
            ("LANG".into(), lang),
            ("TERM".into(), "dumb".into()),
        ],
        open: std::iter::once(cwd)
            .chain(git_dir.map(|g| g.to_string_lossy().to_string()))
            .collect(),
    }
}

/// The end of a person's command in the VM: the last bytes the agent kept past its cap go on
/// `out` as plain output, and the bytes it dropped before them are counted once, in
/// [`HandEnd::skipped`], so [`crate::hand::OutCap`] writes the one "not shown" line.
pub fn exec_end(e: vmwire::ExecExit, out: &tokio::sync::mpsc::UnboundedSender<Vec<u8>>) -> HandEnd {
    let mut skipped = 0;
    if e.dropped > 0 {
        skipped = e.dropped.saturating_sub(e.tail.len() as u64);
        if !e.tail.is_empty() {
            let _ = out.send(e.tail.into_bytes());
        }
    }
    let unsupported = e.error.as_deref() == Some("unsupported");
    HandEnd {
        code: e.code,
        unsupported,
        error: e.error.filter(|_| !unsupported),
        skipped,
    }
}

impl Firecracker {
    fn ctl(&self, run_dir: &Path) -> Option<mpsc::WeakUnboundedSender<Ctl>> {
        let procs = self.procs.lock().unwrap_or_else(|e| e.into_inner());
        procs.get(run_dir).cloned()
    }

    fn send(&self, run_dir: &Path, c: Ctl) -> bool {
        self.ctl(run_dir)
            .and_then(|w| w.upgrade())
            .is_some_and(|s| s.send(c).is_ok())
    }
}

impl Executor for Firecracker {
    fn spawn(&self, spec: &RunSpec, program: &str, args: &[String]) -> std::io::Result<Spawned> {
        let tools = self
            .tools_disk()
            .map_err(|e| std::io::Error::other(format!("VM tools disk: {e}")))?;
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
            vm_shares: spec.vm_shares.clone(),
            program: program.to_string(),
            args: args.to_vec(),
            procs: self.procs.clone(),
        };
        self.procs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(spec.run_dir.clone(), ctl_tx.downgrade());
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

    /// 接管: the VM keeps running and vm-agent stops only the CLI's process group and its
    /// tools, so a person's commands can run beside it (design-takeover 3.4). Asked before
    /// the VM is up, the agent stops the CLI as soon as it has started it.
    fn stop(&self, run_dir: &Path) -> Option<Box<dyn Stopped>> {
        let ctl = self.ctl(run_dir)?;
        ctl.upgrade()?.send(Ctl::Hold(true)).ok()?;
        Some(Box::new(VmStopped { ctl }))
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

    fn takeover(&self) -> Option<&'static str> {
        Some(super::TAKEOVER_ISOLATED)
    }

    /// vm-agent runs the command as another user than the run's, in a mount namespace that
    /// sees only the work folder and the git dir of the run's shares.
    fn hand(&self, req: &HandReq) -> std::io::Result<HandChild> {
        let ctl = self
            .ctl(&req.run_dir)
            .ok_or_else(|| std::io::Error::other("the run's VM is not running"))?;
        let id = self
            .execs
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .to_string();
        let git_dir = workspace::read_meta(&req.run_dir).and_then(|m| m.git_dir);
        let (out, output) = mpsc::unbounded_channel();
        let (done_tx, done) = tokio::sync::oneshot::channel();
        ctl.upgrade()
            .ok_or_else(|| std::io::Error::other("the run's VM is not running"))?
            .send(Ctl::Exec(
                hand_exec(&id, req, git_dir.as_deref()),
                out,
                done_tx,
            ))
            .map_err(|_| std::io::Error::other("the run's VM is not running"))?;
        Ok(HandChild {
            output,
            done,
            cancel: Box::new(move || {
                if let Some(s) = ctl.upgrade() {
                    let _ = s.send(Ctl::ExecCancel(id.clone()));
                }
            }),
        })
    }

    fn hand_back(&self, run_dir: &Path) {
        self.send(run_dir, Ctl::Restore);
    }
}

struct VmStopped {
    ctl: mpsc::WeakUnboundedSender<Ctl>,
}

impl Stopped for VmStopped {
    fn resume(&self) {
        if let Some(s) = self.ctl.upgrade() {
            let _ = s.send(Ctl::Hold(false));
        }
    }
}

enum Ctl {
    Signal(i32),
    Destroy,
    /// 接管: stop (true) or continue the CLI.
    Hold(bool),
    /// 接管: a person's command, where its output and its end go.
    Exec(
        vmwire::Exec,
        mpsc::UnboundedSender<Vec<u8>>,
        tokio::sync::oneshot::Sender<HandEnd>,
    ),
    ExecCancel(String),
    Restore,
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
    /// `RunSpec.vm_shares`: the run's CLI folders and its repository's cache.
    vm_shares: Vec<PathBuf>,
    program: String,
    args: Vec<String>,
    /// The executor's control channels: this run's goes at teardown.
    procs: Procs,
}

/// A folder never shared with a VM: the daemon user's home itself, or anything in one of its
/// dot folders (the CLIs' sign-ins, the daemon's config and machine token, SSH keys). The
/// root helper refuses these too.
fn private_share(home: &Path, p: &Path) -> bool {
    let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    match p.strip_prefix(&home) {
        Ok(rest) => rest
            .components()
            .next()
            .is_none_or(|c| c.as_os_str().to_string_lossy().starts_with('.')),
        Err(_) => false,
    }
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

    /// The run's id: the name of its run folder.
    fn run_id(&self) -> String {
        self.run_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    }

    fn console_tail(&self) -> String {
        let s = std::fs::read(self.vm_dir.join("console.log")).unwrap_or_default();
        let start = s.len().saturating_sub(1500);
        String::from_utf8_lossy(&s[start..]).trim().to_string()
    }

    /// The paths shared with the VM: the run folder, the working folder (a line's folder, kept
    /// on this host for the member's next run in the topic), the run's git dir, the run's
    /// `vm_shares` (its CLIs' folders), and `[vm].shares`.
    fn shares(&self) -> Vec<PathBuf> {
        let mut out = vec![self.run_dir.clone(), self.cwd.clone()];
        if let Some(gd) = workspace::read_meta(&self.run_dir).and_then(|m| m.git_dir) {
            out.push(gd);
        }
        out.extend(self.vm_shares.iter().cloned());
        let home = crate::config::home();
        for s in &self.cfg.shares {
            if s.is_dir() && !private_share(&home, s) {
                out.push(s.clone());
            }
        }
        out.iter()
            .filter_map(|p| std::fs::canonicalize(p).ok())
            .filter(|p| !private_share(&home, p))
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
        crate::metrics::vm_booting(&self.run_id(), scratch.clone());
        std::fs::File::create(&scratch)
            .and_then(|f| f.set_len(u64::from(self.cfg.scratch_gb) << 30))
            .map_err(|e| format!("scratch disk: {e}"))?;
        let out = tokio::process::Command::new(mkfs_ext4())
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
        crate::metrics::vm_process(&self.run_id(), pid);

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
                    at: None,
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
        crate::metrics::vm_booted(&self.run_id(), booted.elapsed().as_millis() as u64);
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
        // The person's commands that run: where their output and their end go.
        type Exec = (
            mpsc::UnboundedSender<Vec<u8>>,
            tokio::sync::oneshot::Sender<HandEnd>,
        );
        let mut execs: HashMap<String, Exec> = HashMap::new();
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
                    Some((Tag::ExecOut, p)) => {
                        if let Some((id, bytes)) = vmwire::parse_exec_out(&p)
                            && let Some((out, _)) = execs.get(&id)
                        {
                            let _ = out.send(bytes.to_vec());
                        }
                    }
                    Some((Tag::ExecExit, p)) => {
                        if let Ok(e) = serde_json::from_slice::<vmwire::ExecExit>(&p)
                            && let Some((out, done)) = execs.remove(&e.id)
                        {
                            let _ = done.send(exec_end(e, &out));
                        }
                    }
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
                    Some(Ctl::Hold(stop)) => {
                        let p = serde_json::to_vec(&vmwire::Hold { stop }).unwrap_or_default();
                        let _ = vmwire::write_frame_async(&mut wr, Tag::Hold, &p).await;
                    }
                    Some(Ctl::Exec(e, out, done)) => {
                        let p = serde_json::to_vec(&e).unwrap_or_default();
                        if vmwire::write_frame_async(&mut wr, Tag::Exec, &p).await.is_ok() {
                            execs.insert(e.id, (out, done));
                        }
                    }
                    Some(Ctl::ExecCancel(id)) => {
                        let p = serde_json::to_vec(&vmwire::ExecCancel { id }).unwrap_or_default();
                        let _ = vmwire::write_frame_async(&mut wr, Tag::ExecCancel, &p).await;
                    }
                    Some(Ctl::Restore) => {
                        let _ = vmwire::write_frame_async(&mut wr, Tag::Restore, b"").await;
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
        self.procs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.run_dir);
        if let Some(mut c) = child {
            let _ = c.start_kill();
            let _ = c.wait().await;
        }
        crate::metrics::vm_gone(&self.run_id());
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

    /// Feature "github": in the VM, `gh` on `GUEST_PATH` is the tools disk's proxy, and the
    /// proxy skips the tools folder when it looks for the real one.
    #[test]
    fn the_guest_path_finds_the_gh_proxy_first() {
        let tmp = tempfile::tempdir().unwrap();
        let stage = tmp.path().join("tools");
        stage_tools(b"agent", &stage).unwrap();
        let agent = format!("/opt/runner/{}", crate::vm_agent::NAME);
        for name in [crate::chat_run::NAME, crate::github::GH] {
            let link = std::fs::read_link(stage.join("bin").join(name)).unwrap();
            assert_eq!(link, Path::new(&agent), "{name}");
        }
        assert_eq!(GUEST_PATH.split(':').next(), Some(crate::github::GUEST_BIN));
        let (env, _) = guest_env(&[("CHAT_GITHUB_AS".to_string(), "Ann".to_string())]);
        assert!(env.contains(&("CHAT_GITHUB_AS".to_string(), "Ann".to_string())));
    }

    #[test]
    fn the_daemon_users_home_and_its_dot_folders_are_never_shared() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        for d in [".claude", ".config/team-chat", "team-chat-runs/r1"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        assert!(private_share(&home, &home));
        assert!(private_share(&home, &home.join(".claude")));
        assert!(private_share(&home, &home.join(".config/team-chat")));
        assert!(!private_share(&home, &home.join("team-chat-runs/r1")));
        assert!(!private_share(&home, tmp.path()));
    }

    #[test]
    fn a_vm_shares_its_cli_root_and_no_members_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let p = |s: &str| {
            let d = tmp.path().join(s);
            std::fs::create_dir_all(&d).unwrap();
            d
        };
        let (run_dir, cwd, homes) = (p("runs/r1"), p("lines/l1/work"), p("homes"));
        let cli_root = p("lines/l1/cli");
        // A member folder of an older daemon is on disk: it is not shared any more.
        p("homes/01BOT");
        let mut cfg = crate::config::Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n[runtimes.claude]\nkind = \"claude\"\n[vm]\nkernel = \"/k\"\nrootfs = \"/r\"\nhomes = \"{}\"\n",
            homes.display()
        ))
        .unwrap()
        .vm
        .take()
        .unwrap();
        // An old config that still names the CLIs' sign-in folders: they are skipped.
        cfg.shares = vec![crate::config::home().join(".claude")];
        let env = vec![
            ("HOME".to_string(), "/home/runner".to_string()),
            ("CHAT_BOT_MEMBER_ID".to_string(), "01BOT".to_string()),
        ];
        let job = Job {
            cfg: cfg.clone(),
            slots: Arc::default(),
            slot: 0,
            tools: PathBuf::new(),
            vm_dir: p("vms/r1"),
            run_dir: run_dir.clone(),
            cwd: cwd.clone(),
            env,
            vm_shares: vec![cli_root.clone()],
            program: "claude".into(),
            args: vec![],
            procs: Arc::default(),
        };
        let shares = job.shares();
        let canon = |d: &Path| std::fs::canonicalize(d).unwrap();
        assert!(shares.contains(&canon(&cli_root)));
        assert!(!shares.iter().any(|s| s.starts_with(canon(&homes))));
        assert!(shares.contains(&canon(&run_dir)) && shares.contains(&canon(&cwd)));
        assert_eq!(shares.len(), 3, "{shares:?}");
    }

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

    /// The repository's cache folders come right after the tools disk (its shims stay first)
    /// and before the image's own; the variable itself does not reach the CLI.
    #[test]
    fn guest_env_puts_the_cache_on_path_after_the_shims() {
        let c = Path::new("/w/caches/k");
        let mut env = vec![("PATH".to_string(), "/host/bin".to_string())];
        env.extend(crate::vm_cache::guest_vars(c));
        let (g, _) = guest_env(&env);
        let get = |k: &str| g.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get(crate::vm_cache::ENV_PATH_EXTRA), None);
        assert_eq!(get("CARGO_HOME"), Some("/w/caches/k/cargo"));
        let path: Vec<&str> = get("PATH").unwrap().split(':').collect();
        assert_eq!(path[0], crate::github::GUEST_BIN);
        assert_eq!(&path[1..3], &["/w/caches/k/bin", "/w/caches/k/cargo/bin"]);
        assert_eq!(path[7], "/usr/local/sbin");
        assert_eq!(path.last(), Some(&"/opt/rust/cargo/bin"));
        assert!(!path.contains(&"/host/bin"));
    }
}
