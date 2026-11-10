//! The daemon as a service of the logged-in user: a launchd agent on macOS, a systemd user
//! unit on Linux. Templates, install and uninstall.

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

/// Windows has no user ids; launchd is not there either (see [`windows_unsupported`]).
#[cfg(not(unix))]
fn uid() -> u32 {
    0
}

/// The error of `install-service` and `uninstall-service` on Windows, where the service will
/// be a scheduled task of the logged-in user.
pub fn windows_unsupported() -> String {
    "install-service is not supported on Windows yet: start `daemon run` yourself (a scheduled \
task that does it at logon is planned)"
        .into()
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

#[cfg(test)]
mod tests {
    use super::systemd_quote as q;

    #[test]
    fn systemd_quoting() {
        assert_eq!(q("/usr/bin/x"), "\"/usr/bin/x\"");
        assert_eq!(q("/a b/c"), "\"/a b/c\"");
        assert_eq!(q("50%$HOME\"\\"), "\"50%%$$HOME\\\"\\\\\"");
    }
}
