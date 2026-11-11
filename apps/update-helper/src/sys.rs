//! The system side of the helper: [`Os`] implements [`Sys`] on Windows. On other systems
//! it does the same with plain processes, which is enough to try the helper by hand; the
//! app uses the helper only on Windows.

use std::{
    ffi::OsString,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

use crate::{Running, Signer, Sys};

/// The real system.
pub struct Os;

struct Started(Child);

impl Running for Started {
    fn exited(&mut self) -> bool {
        !matches!(self.0.try_wait(), Ok(None))
    }

    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The Windows system folder (`C:\Windows\System32`). The helper starts system tools from
/// there by full path: a bare name would be looked up in the helper's own folder first,
/// which the user (and so any program of the user) can write to.
#[cfg(windows)]
fn system_dir() -> std::path::PathBuf {
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buf = [0u16; 512];
    // SAFETY: the buffer is ours and its length is passed.
    let len = unsafe { GetSystemDirectoryW(Some(&mut buf)) } as usize;
    if len == 0 || len > buf.len() {
        return std::path::PathBuf::from(r"C:\Windows\System32");
    }
    std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len]))
}

#[cfg(not(windows))]
fn system_dir() -> std::path::PathBuf {
    std::path::PathBuf::from("/usr/bin")
}

/// A system tool by its full path, started without a console window (the helper has none
/// itself).
fn system_tool(relative: &str) -> Command {
    let mut cmd = Command::new(system_dir().join(relative));
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

const POWERSHELL: &str = r"WindowsPowerShell\v1.0\powershell.exe";
const REG: &str = "reg.exe";
/// Where Windows keeps the commands it runs once at the user's next sign-in.
const RUN_ONCE: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce";

/// The `DisplayVersion` line of `reg query <key> /v DisplayVersion`.
pub(crate) fn parse_reg_value(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        if parts.next()? != name || !parts.next()?.starts_with("REG_") {
            return None;
        }
        let rest: Vec<&str> = parts.collect();
        (!rest.is_empty()).then(|| rest.join(" "))
    })
}

/// Reads the lines the signature query prints: the status, then the certificate's subject
/// and thumbprint when there is one. No output at all is a failed query, not "unsigned".
pub(crate) fn parse_signer(output: &str) -> Signer {
    let mut lines = output.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(status) = lines.next() else {
        return Signer::Unknown("the signature query printed nothing".into());
    };
    match (lines.next(), lines.next()) {
        (Some(subject), Some(thumbprint)) => Signer::Signed {
            status: status.to_string(),
            subject: subject.to_string(),
            thumbprint: thumbprint.to_string(),
        },
        (None, _) if status == "NotSigned" => Signer::Unsigned,
        _ => Signer::Unknown(format!("status {status} without a certificate")),
    }
}

impl Sys for Os {
    #[cfg(windows)]
    fn wait_exit(&self, pid: u32, timeout: Duration) -> bool {
        use windows::Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        };
        // SAFETY: the handle is used only here and closed before returning.
        unsafe {
            // No such process any more: it has exited.
            let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
                return true;
            };
            let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
            let gone = WaitForSingleObject(handle, ms) == WAIT_OBJECT_0;
            let _ = CloseHandle(handle);
            gone
        }
    }

    #[cfg(not(windows))]
    fn wait_exit(&self, pid: u32, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let alive = Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if !alive {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn run_installer(&self, installer: &Path, install_dir: &Path) -> Result<(), String> {
        let mut cmd = Command::new(installer);
        cmd.stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            // NSIS reads /D= to the end of the command line and takes no quotes, so the
            // folder goes last and unquoted even when it has spaces.
            cmd.raw_arg(format!("/S /UPDATE /D={}", install_dir.display()));
        }
        #[cfg(not(windows))]
        {
            let mut dir = OsString::from("/D=");
            dir.push(install_dir);
            cmd.args([OsString::from("/S"), OsString::from("/UPDATE"), dir]);
        }
        let status = cmd
            .status()
            .map_err(|e| format!("cannot run the installer: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("the installer failed ({status})"))
        }
    }

    fn launch(&self, exe: &Path, args: &[OsString]) -> Result<Box<dyn Running>, String> {
        let mut cmd = Command::new(exe);
        cmd.args(args).stdin(Stdio::null());
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        Ok(Box::new(Started(child)))
    }

    fn signer(&self, file: &Path) -> Signer {
        if !cfg!(windows) {
            return Signer::Unsigned;
        }
        // The path travels in the environment, so no quoting rule applies to it.
        let out = system_tool(POWERSHELL)
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(
                "$ErrorActionPreference = 'Stop'; \
                 $s = Get-AuthenticodeSignature -LiteralPath $env:UPDATE_SIGNED_FILE; \
                 [Console]::Out.WriteLine($s.Status); \
                 if ($s.SignerCertificate) { \
                 [Console]::Out.WriteLine($s.SignerCertificate.Subject); \
                 [Console]::Out.WriteLine($s.SignerCertificate.Thumbprint) }",
            )
            .env("UPDATE_SIGNED_FILE", file)
            .output();
        match out {
            Ok(out) if out.status.success() => parse_signer(&String::from_utf8_lossy(&out.stdout)),
            Ok(out) => Signer::Unknown(format!("the signature query failed ({})", out.status)),
            Err(e) => Signer::Unknown(format!("cannot run the signature query: {e}")),
        }
    }

    fn shown_version(&self, uninstall_key: &str) -> Option<String> {
        if !cfg!(windows) {
            return None;
        }
        let out = system_tool(REG)
            .args([
                "query",
                &format!("HKCU\\{uninstall_key}"),
                "/v",
                "DisplayVersion",
            ])
            .output()
            .ok()?;
        parse_reg_value(&String::from_utf8_lossy(&out.stdout), "DisplayVersion")
    }

    fn set_shown_version(&self, uninstall_key: &str, version: &str) {
        if !cfg!(windows) {
            return;
        }
        let _ = system_tool(REG)
            .args([
                "add",
                &format!("HKCU\\{uninstall_key}"),
                "/v",
                "DisplayVersion",
            ])
            .args(["/t", "REG_SZ", "/d", version, "/f"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    fn register_recovery(&self, name: &str, journal: &Path) -> Result<(), String> {
        if !cfg!(windows) {
            return Ok(());
        }
        let helper = std::env::current_exe().map_err(|e| e.to_string())?;
        // Windows runs this line at the next sign-in and removes the entry first.
        let command = format!(
            "\"{}\" --recover \"{}\"",
            helper.display(),
            journal.display()
        );
        let status = system_tool(REG)
            .args(["add", RUN_ONCE, "/v", name, "/t", "REG_SZ", "/d"])
            .arg(command)
            .arg("/f")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("cannot register the recovery: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("cannot register the recovery ({status})"))
        }
    }

    fn clear_recovery(&self, name: &str) {
        if !cfg!(windows) {
            return;
        }
        let _ = system_tool(REG)
            .args(["delete", RUN_ONCE, "/v", name, "/f"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    /// A test on a real machine can hold the helper at a step (to kill it there):
    /// `UPDATE_HELPER_TEST_PAUSE_AT=<step>`. Only a helper built with the `test-pause`
    /// feature reads it (scripts/test-windows-update.sh builds one); the helper in the
    /// installer is built without the feature and has no such switch.
    #[cfg(feature = "test-pause")]
    fn checkpoint(&self, step: &str) {
        if std::env::var("UPDATE_HELPER_TEST_PAUSE_AT").is_ok_and(|at| at == step) {
            std::thread::sleep(Duration::from_secs(600));
        }
    }
}

/// `--shortcut <lnk> --target <exe> --app-id <id> [--toast-activator <clsid>]`: writes a
/// shortcut to the app that carries its AppUserModelID and, when given, the class id Windows
/// starts for a click on one of the app's notifications after the app has exited.
pub fn shortcut_from_args(args: &[OsString]) -> Result<(), String> {
    let (mut lnk, mut target, mut app_id) = (None, None, None);
    let mut activator = None;
    let mut it = args.iter();
    if let Some(first) = it.next() {
        lnk = Some(first.clone());
    }
    while let Some(arg) = it.next() {
        match arg.to_string_lossy().as_ref() {
            "--target" => target = it.next().cloned(),
            "--app-id" => app_id = it.next().cloned(),
            "--toast-activator" => activator = it.next().cloned(),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let (Some(lnk), Some(target), Some(app_id)) = (lnk, target, app_id) else {
        return Err(
            "usage: --shortcut <lnk> --target <exe> --app-id <id> [--toast-activator <clsid>]"
                .into(),
        );
    };
    let activator = activator
        .map(|a| class_id(&a.to_string_lossy()))
        .transpose()?;
    write_shortcut(
        Path::new(&lnk),
        Path::new(&target),
        &app_id.to_string_lossy(),
        activator.as_deref(),
    )
}

/// A class id as `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` (lower case), from that form with or
/// without braces.
pub fn class_id(text: &str) -> Result<String, String> {
    let inner = text.trim();
    let inner = inner
        .strip_prefix('{')
        .and_then(|t| t.strip_suffix('}'))
        .unwrap_or(inner);
    let groups: Vec<&str> = inner.split('-').collect();
    let well_formed = groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|g| g.bytes().all(|b| b.is_ascii_hexdigit()));
    if well_formed {
        Ok(inner.to_ascii_lowercase())
    } else {
        Err(format!("not a class id: {text}"))
    }
}

#[cfg(windows)]
fn write_shortcut(
    lnk: &Path,
    target: &Path,
    app_id: &str,
    activator: Option<&str>,
) -> Result<(), String> {
    use windows::{
        Win32::{
            Storage::EnhancedStorage::{
                PKEY_AppUserModel_ID, PKEY_AppUserModel_ToastActivatorCLSID,
            },
            System::{
                Com::{
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
                    CoInitializeEx, CoTaskMemAlloc, IPersistFile, StructuredStorage::PROPVARIANT,
                },
                Variant::{VT_CLSID, VT_LPWSTR},
            },
            UI::Shell::{IShellLinkW, PropertiesSystem::IPropertyStore, SHStrDupW, ShellLink},
        },
        core::{GUID, HSTRING, Interface as _},
    };
    let activator = activator
        .map(|a| GUID::try_from(a).map_err(|e| format!("not a class id: {a} ({e})")))
        .transpose()?;
    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let run = || -> windows::core::Result<()> {
        // SAFETY: COM calls on objects created here, on this thread.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(&HSTRING::from(target.as_os_str()))?;
            if let Some(dir) = target.parent() {
                link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
            }
            link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
            let store: IPropertyStore = link.cast()?;
            // The id is a VT_LPWSTR (the crate's From<&str> makes a VT_BSTR). The string is
            // allocated by COM, and dropping the PROPVARIANT frees it (PropVariantClear).
            let mut value = PROPVARIANT::default();
            let text = SHStrDupW(&HSTRING::from(app_id))?;
            {
                let inner = &mut *value.Anonymous.Anonymous;
                inner.vt = VT_LPWSTR;
                inner.Anonymous.pwszVal = text;
            }
            store.SetValue(&PKEY_AppUserModel_ID, &value)?;
            if let Some(class) = activator {
                // A VT_CLSID holds a pointer to a GUID that COM allocated; dropping the
                // PROPVARIANT frees it.
                let mut value = PROPVARIANT::default();
                let guid = CoTaskMemAlloc(std::mem::size_of::<GUID>()).cast::<GUID>();
                if guid.is_null() {
                    return Err(windows::core::Error::from(
                        windows::Win32::Foundation::E_OUTOFMEMORY,
                    ));
                }
                guid.write(class);
                {
                    let inner = &mut *value.Anonymous.Anonymous;
                    inner.vt = VT_CLSID;
                    inner.Anonymous.puuid = guid;
                }
                store.SetValue(&PKEY_AppUserModel_ToastActivatorCLSID, &value)?;
            }
            store.Commit()?;
            let file: IPersistFile = link.cast()?;
            file.Save(&HSTRING::from(lnk.as_os_str()), true)?;
        }
        Ok(())
    };
    run().map_err(|e| format!("cannot write the shortcut: {e}"))
}

#[cfg(not(windows))]
fn write_shortcut(
    _lnk: &Path,
    _target: &Path,
    _app_id: &str,
    _activator: Option<&str>,
) -> Result<(), String> {
    Err("shortcuts exist only on Windows".into())
}
