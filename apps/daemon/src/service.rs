//! The daemon as a service of the logged-in user: a launchd agent on macOS, a systemd user
//! unit on Linux, a scheduled task on Windows. Templates, install and uninstall.

use std::path::{Path, PathBuf};

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The plist. PATH is captured at install time so the CLIs found by `doctor` are found by
/// the service too (launchd starts agents with a minimal PATH).
pub fn plist(label: &str, exe: &Path, config: &Path, path_env: &str, log_dir: &Path) -> String {
    let e = |p: &Path| xml_escape(&p.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>run</string>
    <string>--config</string>
    <string>{config}</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>{path}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>ProcessType</key>
  <string>Standard</string>
  <key>StandardOutPath</key>
  <string>{log}/daemon.log</string>
  <key>StandardErrorPath</key>
  <string>{log}/daemon.log</string>
</dict>
</plist>
"#,
        label = xml_escape(label),
        exe = e(exe),
        config = e(config),
        path = xml_escape(path_env),
        log = e(log_dir),
    )
}

pub fn agents_dir() -> PathBuf {
    crate::config::home().join("Library/LaunchAgents")
}

pub fn plist_path(dir: &Path) -> PathBuf {
    dir.join(format!("{}.plist", crate::product::service_label()))
}

#[cfg(unix)]
fn uid() -> u32 {
    // SAFETY: getuid has no preconditions.
    unsafe { libc::getuid() }
}

/// Windows has no user ids; launchd is not there either (see [`install_task`]).
#[cfg(not(unix))]
fn uid() -> u32 {
    0
}

fn launchctl(args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("launchctl")
        .args(args)
        .output()
        .map_err(|e| format!("launchctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Writes the plist into `dir` and, when `load`, (re)loads it with launchctl.
pub fn log_dir() -> PathBuf {
    crate::config::home()
        .join("Library/Logs")
        .join(format!("{}-daemon", crate::product::slug()))
}

pub fn install(dir: &Path, config: &Path, log_dir: &Path, load: bool) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let config = std::fs::canonicalize(config)
        .map_err(|_| format!("config {} does not exist", config.display()))?;
    std::fs::create_dir_all(log_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into());
    let label = crate::product::service_label();
    let file = plist_path(dir);
    std::fs::write(&file, plist(&label, &exe, &config, &path_env, log_dir))
        .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    if load {
        let target = format!("gui/{}", uid());
        let _ = launchctl(&["bootout", &format!("{target}/{label}")]);
        launchctl(&["bootstrap", &target, &file.to_string_lossy()])?;
    }
    Ok(file)
}

pub fn uninstall(dir: &Path, unload: bool) -> Result<PathBuf, String> {
    let label = crate::product::service_label();
    let file = plist_path(dir);
    if unload {
        let _ = launchctl(&["bootout", &format!("gui/{}/{label}", uid())]);
    }
    match std::fs::remove_file(&file) {
        Ok(()) => Ok(file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(file),
        Err(e) => Err(format!("cannot remove {}: {e}", file.display())),
    }
}

/// systemd quoting for a value inside double quotes: backslash, quote, `%` specifiers and `$`
/// variables are taken literally.
fn systemd_quote(s: &str) -> String {
    let mut out = String::from('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '%' => out.push_str("%%"),
            '$' => out.push_str("$$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The systemd user unit. PATH is captured at install time, as for launchd. `Restart=always`
/// also starts a new version after [`crate::selfupdate`] exits.
pub fn unit(exe: &Path, config: &Path, path_env: &str, log_dir: &Path) -> Result<String, String> {
    let log = log_dir.join("daemon.log").to_string_lossy().to_string();
    if log.chars().any(char::is_whitespace) {
        return Err(format!(
            "log folder {} must not contain spaces",
            log_dir.display()
        ));
    }
    let log = log.replace('%', "%%");
    Ok(format!(
        "[Unit]\nDescription={name} machine daemon\n\n[Service]\nExecStart={exe} run --config {config}\nEnvironment={path}\nRestart=always\nRestartSec=10\nStandardOutput=append:{log}\nStandardError=append:{log}\n\n[Install]\nWantedBy=default.target\n",
        name = crate::product::name().replace('%', "%%"),
        exe = systemd_quote(&exe.to_string_lossy()),
        config = systemd_quote(&config.to_string_lossy()),
        path = systemd_quote(&format!("PATH={path_env}")),
    ))
}

pub fn units_dir() -> PathBuf {
    crate::config::home().join(".config/systemd/user")
}

pub fn unit_path(dir: &Path) -> PathBuf {
    dir.join(crate::product::systemd_unit())
}

pub fn systemd_log_dir() -> PathBuf {
    crate::config::home()
        .join(".local/state")
        .join(format!("{}-daemon", crate::product::slug()))
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|e| format!("systemctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Writes the unit into `dir` and, when `load`, enables and (re)starts it. Also asks for
/// lingering, so the service runs from boot without anyone logged in.
pub fn install_systemd(
    dir: &Path,
    config: &Path,
    log_dir: &Path,
    load: bool,
) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let config = std::fs::canonicalize(config)
        .map_err(|_| format!("config {} does not exist", config.display()))?;
    std::fs::create_dir_all(log_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into());
    let file = unit_path(dir);
    std::fs::write(&file, unit(&exe, &config, &path_env, log_dir)?)
        .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    if load {
        let name = crate::product::systemd_unit();
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", &name])?;
        systemctl(&["restart", &name])?;
        let linger = std::process::Command::new("loginctl")
            .arg("enable-linger")
            .status();
        if !linger.is_ok_and(|s| s.success()) {
            eprintln!(
                "could not turn on lingering: the service starts only when you log in (run `sudo loginctl enable-linger $USER`)"
            );
        }
    }
    Ok(file)
}

pub fn uninstall_systemd(dir: &Path, unload: bool) -> Result<PathBuf, String> {
    let file = unit_path(dir);
    if unload {
        let _ = systemctl(&["disable", "--now", &crate::product::systemd_unit()]);
    }
    let removed = match std::fs::remove_file(&file) {
        Ok(()) => Ok(file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(file),
        Err(e) => Err(format!("cannot remove {}: {e}", file.display())),
    };
    if unload {
        let _ = systemctl(&["daemon-reload"]);
    }
    removed
}

// ---------------------------------------------------------------------------------------
// Windows: a scheduled task of the logged-in user.
//
// Not a Windows service: a service runs in another session and cannot see the user's CLI
// logins. The task is a Task Scheduler XML definition registered with `schtasks /Create /XML`
// rather than through the Task Scheduler COM API: the definition is plain text that is tested
// on every system, `schtasks` is part of every Windows, and the daemon needs no COM code.
// Nothing is read back from `schtasks` (its output is in the system's language and code page):
// only its exit code, and the definition file the daemon keeps next to its log.
//
// What the definition says:
// - it starts when this user logs on, and only in this user's logon session (no stored
//   password, the user's own token, least privilege), so the CLIs see the user's logins;
// - Task Scheduler runs one instance only, and a second trigger asks for a start every minute:
//   while the daemon runs that does nothing, and after the daemon ended for any reason (a
//   crash, a kill, its own exit on a new version) it starts it again within a minute. This is
//   what `KeepAlive` is for launchd. `RestartOnFailure` alone would not do: it does not start
//   a program that exited with 0;
// - no time limit, not stopped on battery or when the computer stops being idle;
// - the program is `conhost.exe --headless <daemon> run …`: the daemon is a console program,
//   and started directly it would show a console window for as long as it runs. A headless
//   console host gives it a real console without a window, which the CLIs of the runs inherit
//   (no windows for them either) and which `CTRL_BREAK_EVENT`, the polite cancel, needs;
// - the daemon writes its own log (`--log-file`): nobody reads the output of a headless
//   console. `PATH` is not captured as on Unix: the task gets the user's current environment.
// ---------------------------------------------------------------------------------------

/// Name of the scheduled task.
pub fn task_name() -> String {
    crate::product::service_label()
}

/// `%LOCALAPPDATA%\<slug>-daemon`: the task definition (`task.xml`) and the log (`daemon.log`).
pub fn task_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join("AppData").join("Local"))
        .join(format!("{}-daemon", crate::product::slug()))
}

pub fn task_file(dir: &Path) -> PathBuf {
    dir.join("task.xml")
}

pub fn task_log(dir: &Path) -> PathBuf {
    dir.join("daemon.log")
}

fn system32(program: &str) -> PathBuf {
    std::env::var_os("SystemRoot")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join(program)
}

/// What a task definition is made of.
pub struct Task<'a> {
    /// `DOMAIN\user`: who the task belongs to and runs as.
    pub user: &'a str,
    /// The console host that hides the daemon's console.
    pub conhost: &'a Path,
    pub exe: &'a Path,
    pub config: &'a Path,
    pub log: &'a Path,
    /// False leaves the logon trigger out (see [`install_task`]).
    pub at_logon: bool,
}

/// The Task Scheduler definition (schema 1.2, Windows Vista and later).
pub fn task_xml(t: &Task) -> String {
    let e = |p: &Path| xml_escape(&p.to_string_lossy());
    let user = xml_escape(t.user);
    let logon = if t.at_logon {
        format!(
            "    <LogonTrigger>\n      <Enabled>true</Enabled>\n      <UserId>{user}</UserId>\n    </LogonTrigger>\n"
        )
    } else {
        String::new()
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>{name} machine daemon</Description>
  </RegistrationInfo>
  <Triggers>
{logon}    <TimeTrigger>
      <Repetition>
        <Interval>PT1M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
      <StartBoundary>2020-01-01T00:00:00</StartBoundary>
      <Enabled>true</Enabled>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>999</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{conhost}</Command>
      <Arguments>--headless &quot;{exe}&quot; run --config &quot;{config}&quot; --log-file &quot;{log}&quot;</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        name = xml_escape(crate::product::name()),
        conhost = e(t.conhost),
        exe = e(t.exe),
        config = e(t.config),
        log = e(t.log),
    )
}

/// `schtasks /Create /XML` reads UTF-16 with a byte order mark.
fn utf16_file(text: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

/// A system program without a console window of its own (the app starts the daemon, which
/// starts these, with no console at all).
fn system_command(program: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(system32(program));
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    cmd
}

fn schtasks(args: &[&str]) -> Result<(), String> {
    let out = system_command("schtasks.exe")
        .args(args)
        .output()
        .map_err(|e| format!("schtasks: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "schtasks {} failed ({}): {}",
            args.first().copied().unwrap_or_default(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// `DOMAIN\user` of the user this process runs as.
fn current_user() -> Result<String, String> {
    let from_env = || {
        let user = std::env::var("USERNAME").ok().filter(|u| !u.is_empty())?;
        Some(match std::env::var("USERDOMAIN") {
            Ok(d) if !d.is_empty() => format!("{d}\\{user}"),
            _ => user,
        })
    };
    let who = system_command("whoami.exe")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // whoami prints in the console code page: a name it could not print as UTF-8 comes from
    // the environment instead.
    who.or_else(from_env)
        .ok_or_else(|| "cannot tell which user this is (USERNAME is not set)".into())
}

/// True when the task is registered.
pub fn task_exists(name: &str) -> bool {
    schtasks(&["/Query", "/TN", name]).is_ok()
}

/// Writes the definition of the task `name` that runs `exe run --config <config>` into `dir`
/// and, when `load`, registers it and starts it now (a running one is ended first, so the new
/// definition and the program file now on disk are what runs).
///
/// A standard user may not be allowed to register a logon trigger on a managed computer. The
/// task is then registered without it: the every-minute trigger starts the daemon within a
/// minute of the logon.
pub fn install_task(
    name: &str,
    exe: &Path,
    config: &Path,
    dir: &Path,
    load: bool,
) -> Result<PathBuf, String> {
    let exe = std::path::absolute(exe).map_err(|e| e.to_string())?;
    if !config.is_file() {
        return Err(format!("config {} does not exist", config.display()));
    }
    let config = std::path::absolute(config).map_err(|e| e.to_string())?;
    for p in [&exe, &config, &dir.to_path_buf()] {
        if p.to_string_lossy().contains('"') {
            return Err(format!("{} must not contain a quote", p.display()));
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let user = current_user()?;
    let conhost = system32("conhost.exe");
    let log = task_log(dir);
    let file = task_file(dir);
    let mut task = Task {
        user: &user,
        conhost: &conhost,
        exe: &exe,
        config: &config,
        log: &log,
        at_logon: true,
    };
    let write = |task: &Task| -> Result<(), String> {
        std::fs::write(&file, utf16_file(&task_xml(task)))
            .map_err(|e| format!("cannot write {}: {e}", file.display()))
    };
    if !load {
        write(&task)?;
        return Ok(file);
    }
    let register = |task: &Task| -> Result<(), String> {
        write(task)?;
        let xml = file.to_string_lossy();
        schtasks(&["/Create", "/TN", name, "/XML", &xml, "/F"])
    };
    if let Err(first) = register(&task) {
        task.at_logon = false;
        register(&task).map_err(|_| first)?;
        eprintln!(
            "the task could not be registered with a logon trigger; it starts within a minute of your logon instead"
        );
    }
    restart_task(name, dir)?;
    Ok(file)
}

/// Ends the running task and its daemon and starts the task again: the daemon file now on
/// disk is what runs.
pub fn restart_task(name: &str, dir: &Path) -> Result<(), String> {
    let _ = schtasks(&["/End", "/TN", name]);
    stop_daemon(dir);
    schtasks(&["/Run", "/TN", name])
}

/// Ends and removes the task `name`, stops its daemon and removes the definition file in `dir`.
pub fn uninstall_task(name: &str, dir: &Path, unload: bool) -> Result<PathBuf, String> {
    let file = task_file(dir);
    if unload {
        if task_exists(name) {
            let _ = schtasks(&["/End", "/TN", name]);
            schtasks(&["/Delete", "/TN", name, "/F"])?;
        }
        stop_daemon(dir);
    }
    match std::fs::remove_file(&file) {
        Ok(()) => Ok(file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(file),
        Err(e) => Err(format!("cannot remove {}: {e}", file.display())),
    }
}

/// Logs larger than this are moved to `daemon.log.1` when the daemon starts.
const LOG_KEEP_BYTES: u64 = 10 * 1024 * 1024;

/// Where the task's daemon says which process it is (see [`stop_daemon`]).
pub fn task_pid_file(dir: &Path) -> PathBuf {
    dir.join("daemon.pid")
}

// Ending the task ends only the console host: the daemon behind it keeps running. So the
// daemon of a task makes a named event (`Local\<slug>-daemon-stop-<pid>`, this logon session
// only) and writes its pid next to its log; whoever stops it sets the event, the daemon shuts
// down the way it does on Ctrl+C (its runs are cancelled and reported), and only a daemon
// that does not end in time is killed. The event also tells that the pid still is a daemon.

#[cfg(windows)]
static STOP_EVENT: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(windows)]
fn stop_event_name(pid: u32) -> Vec<u16> {
    format!("Local\\{}-daemon-stop-{pid}\0", crate::product::slug())
        .encode_utf16()
        .collect()
}

/// `run --log-file`, what the scheduled task starts: sends everything the daemon prints to
/// `path` (appending; the task's headless console shows nothing) and makes this daemon one
/// that [`stop_daemon`] can stop.
#[cfg(windows)]
pub fn log_to(path: &Path) -> Result<(), String> {
    use std::os::windows::io::IntoRawHandle as _;
    use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
    use windows_sys::Win32::System::Threading::CreateEventW;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    if std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_KEEP_BYTES) {
        let mut old = path.as_os_str().to_owned();
        old.push(".1");
        let _ = std::fs::rename(path, old);
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    // The handle stays open for the life of the process: it is the process's output now.
    let handle = file.into_raw_handle();
    // SAFETY: the handle is a valid open file that is never closed.
    unsafe {
        SetStdHandle(STD_OUTPUT_HANDLE, handle as _);
        SetStdHandle(STD_ERROR_HANDLE, handle as _);
    }
    let name = stop_event_name(std::process::id());
    // SAFETY: the name is a NUL-terminated UTF-16 string; a manual-reset event, not set.
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) };
    if event.is_null() {
        return Err(format!(
            "cannot make the stop event: {}",
            std::io::Error::last_os_error()
        ));
    }
    STOP_EVENT.store(event as isize, std::sync::atomic::Ordering::SeqCst);
    let pid_file = task_pid_file(dir);
    std::fs::write(&pid_file, std::process::id().to_string())
        .map_err(|e| format!("cannot write {}: {e}", pid_file.display()))
}

/// Unix service managers write the log themselves (launchd `StandardOutPath`, systemd
/// `StandardOutput`) and stop the daemon with SIGTERM.
#[cfg(not(windows))]
pub fn log_to(_path: &Path) -> Result<(), String> {
    let _ = LOG_KEEP_BYTES;
    Err("--log-file is for the Windows scheduled task: redirect the output yourself".into())
}

/// Blocks until someone asks this daemon to stop ([`stop_daemon`]). Never returns for a
/// daemon that was not started with `--log-file`. Run it on a thread of its own.
pub fn wait_stop() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
        let event = STOP_EVENT.load(std::sync::atomic::Ordering::SeqCst);
        if event != 0 {
            // SAFETY: the event handle is ours and is never closed.
            unsafe { WaitForSingleObject(event as _, INFINITE) };
            return;
        }
    }
    loop {
        std::thread::park();
    }
}

/// How long a daemon that was asked to stop may take (it gives its runs ten seconds).
#[cfg(windows)]
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Stops the daemon that wrote its pid into `dir`: asks, waits, then kills. True when one was
/// running. Its runs end with it: each is a Job Object that closes with the daemon.
#[cfg(windows)]
pub fn stop_daemon(dir: &Path) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};
    let pid_file = task_pid_file(dir);
    let pid = std::fs::read_to_string(&pid_file)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok());
    let _ = std::fs::remove_file(&pid_file);
    let Some(pid) = pid else {
        return false;
    };
    let name = stop_event_name(pid);
    // SAFETY: the name is a NUL-terminated UTF-16 string.
    let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
    if event.is_null() {
        // No such event: that process is gone, or the pid is another program's now.
        return false;
    }
    // SAFETY: the handle is an open event, closed here.
    unsafe {
        SetEvent(event);
        CloseHandle(event);
    }
    let start = std::time::Instant::now();
    while crate::winjob::pid_alive(pid) {
        if start.elapsed() > STOP_WAIT {
            let _ = system_command("taskkill.exe")
                .args(["/F", "/PID", &pid.to_string()])
                .output();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    true
}

#[cfg(not(windows))]
pub fn stop_daemon(_dir: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::systemd_quote as q;
    use super::*;

    fn sample(at_logon: bool) -> String {
        task_xml(&Task {
            user: r"PC\li & co",
            conhost: Path::new(r"C:\Windows\System32\conhost.exe"),
            exe: Path::new(r"C:\Users\li & co\AppData\Local\Programs\x\x-daemon.exe"),
            config: Path::new(r"C:\Users\li & co\.config\x\daemon.toml"),
            log: Path::new(r"C:\Users\li & co\AppData\Local\x-daemon\daemon.log"),
            at_logon,
        })
    }

    #[test]
    fn the_task_runs_the_daemon_for_this_user_and_keeps_it_running() {
        let xml = sample(true);
        let doc = roxmltree::Document::parse(&xml).expect("well-formed XML");
        let ns = "http://schemas.microsoft.com/windows/2004/02/mit/task";
        assert_eq!(doc.root_element().tag_name().namespace(), Some(ns));
        let get = |path: &[&str]| {
            let full: Vec<(&str, &str)> = path.iter().map(|n| (ns, *n)).collect();
            let mut node = doc.root_element();
            for name in &full {
                node = node
                    .children()
                    .find(|c| c.has_tag_name(*name))
                    .unwrap_or_else(|| panic!("no {path:?}"));
            }
            node.text().unwrap_or_default().to_string()
        };
        // At logon of this user, in this user's own logon session, without more rights.
        assert_eq!(get(&["Triggers", "LogonTrigger", "UserId"]), r"PC\li & co");
        assert_eq!(get(&["Principals", "Principal", "UserId"]), r"PC\li & co");
        assert_eq!(
            get(&["Principals", "Principal", "LogonType"]),
            "InteractiveToken"
        );
        assert_eq!(
            get(&["Principals", "Principal", "RunLevel"]),
            "LeastPrivilege"
        );
        // One instance, asked for every minute for ever: started again after it ends.
        assert_eq!(get(&["Settings", "MultipleInstancesPolicy"]), "IgnoreNew");
        assert_eq!(
            get(&["Triggers", "TimeTrigger", "Repetition", "Interval"]),
            "PT1M"
        );
        assert_eq!(
            get(&["Triggers", "TimeTrigger", "Repetition", "StopAtDurationEnd"]),
            "false"
        );
        assert!(!xml.contains("<Duration>"), "repeats without an end");
        assert_eq!(get(&["Settings", "RestartOnFailure", "Interval"]), "PT1M");
        // No time limit; battery and idle never stop it.
        assert_eq!(get(&["Settings", "ExecutionTimeLimit"]), "PT0S");
        assert_eq!(get(&["Settings", "DisallowStartIfOnBatteries"]), "false");
        assert_eq!(get(&["Settings", "StopIfGoingOnBatteries"]), "false");
        assert_eq!(get(&["Settings", "IdleSettings", "StopOnIdleEnd"]), "false");
        assert_eq!(get(&["Settings", "RunOnlyIfIdle"]), "false");
        assert_eq!(get(&["Settings", "RunOnlyIfNetworkAvailable"]), "false");
        assert_eq!(get(&["Settings", "Enabled"]), "true");
        // No console window: a headless console host starts the daemon, which logs to a file.
        assert_eq!(
            get(&["Actions", "Exec", "Command"]),
            r"C:\Windows\System32\conhost.exe"
        );
        assert_eq!(
            get(&["Actions", "Exec", "Arguments"]),
            r#"--headless "C:\Users\li & co\AppData\Local\Programs\x\x-daemon.exe" run --config "C:\Users\li & co\.config\x\daemon.toml" --log-file "C:\Users\li & co\AppData\Local\x-daemon\daemon.log""#
        );
    }

    #[test]
    fn the_task_without_a_logon_trigger_still_starts_every_minute() {
        let xml = sample(false);
        roxmltree::Document::parse(&xml).expect("well-formed XML");
        assert!(!xml.contains("LogonTrigger"));
        assert!(xml.contains("<TimeTrigger>"));
    }

    #[test]
    fn the_task_file_is_utf16_with_a_byte_order_mark() {
        let bytes = utf16_file("<a>机</a>");
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "<a>机</a>");
    }

    #[test]
    fn systemd_quoting() {
        assert_eq!(q("/usr/bin/x"), "\"/usr/bin/x\"");
        assert_eq!(q("/a b/c"), "\"/a b/c\"");
        assert_eq!(q("50%$HOME\"\\"), "\"50%%$$HOME\\\"\\\\\"");
    }
}
