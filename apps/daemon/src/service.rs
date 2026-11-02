//! launchd user agent: plist template, install and uninstall.

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

fn uid() -> u32 {
    // SAFETY: getuid has no preconditions.
    unsafe { libc::getuid() }
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
