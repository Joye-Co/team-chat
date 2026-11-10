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

/// A command that opens no console window (the helper has none itself).
fn quiet(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

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

/// Reads the two lines the signature query prints: the status, then the subject.
pub(crate) fn parse_signer(output: &str) -> Signer {
    let mut lines = output.lines().map(str::trim).filter(|l| !l.is_empty());
    match (lines.next(), lines.next()) {
        (Some("Valid"), Some(subject)) => Signer::Valid(subject.to_string()),
        (Some("NotSigned"), _) | (None, _) => Signer::Unsigned,
        (Some(status), _) => Signer::Invalid(status.to_string()),
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
        let out = quiet("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(
                "$s = Get-AuthenticodeSignature -LiteralPath $env:UPDATE_SIGNED_FILE; \
                 [Console]::Out.WriteLine($s.Status); \
                 if ($s.SignerCertificate) { [Console]::Out.WriteLine($s.SignerCertificate.Subject) }",
            )
            .env("UPDATE_SIGNED_FILE", file)
            .output();
        match out {
            Ok(out) if out.status.success() => parse_signer(&String::from_utf8_lossy(&out.stdout)),
            _ => Signer::Unsigned,
        }
    }

    fn shown_version(&self, uninstall_key: &str) -> Option<String> {
        if !cfg!(windows) {
            return None;
        }
        let out = quiet("reg.exe")
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
        let _ = quiet("reg.exe")
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
}

/// `--shortcut <lnk> --target <exe> --app-id <id>`: writes a shortcut to the app that
/// carries its AppUserModelID.
pub fn shortcut_from_args(args: &[OsString]) -> Result<(), String> {
    let (mut lnk, mut target, mut app_id) = (None, None, None);
    let mut it = args.iter();
    if let Some(first) = it.next() {
        lnk = Some(first.clone());
    }
    while let Some(arg) = it.next() {
        match arg.to_string_lossy().as_ref() {
            "--target" => target = it.next().cloned(),
            "--app-id" => app_id = it.next().cloned(),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let (Some(lnk), Some(target), Some(app_id)) = (lnk, target, app_id) else {
        return Err("usage: --shortcut <lnk> --target <exe> --app-id <id>".into());
    };
    write_shortcut(
        Path::new(&lnk),
        Path::new(&target),
        &app_id.to_string_lossy(),
    )
}

#[cfg(windows)]
fn write_shortcut(lnk: &Path, target: &Path, app_id: &str) -> Result<(), String> {
    use windows::{
        Win32::{
            Storage::EnhancedStorage::PKEY_AppUserModel_ID,
            System::{
                Com::{
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
                    CoInitializeEx, IPersistFile, StructuredStorage::PROPVARIANT,
                },
                Variant::VT_LPWSTR,
            },
            UI::Shell::{IShellLinkW, PropertiesSystem::IPropertyStore, SHStrDupW, ShellLink},
        },
        core::{HSTRING, Interface as _},
    };
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
            store.Commit()?;
            let file: IPersistFile = link.cast()?;
            file.Save(&HSTRING::from(lnk.as_os_str()), true)?;
        }
        Ok(())
    };
    run().map_err(|e| format!("cannot write the shortcut: {e}"))
}

#[cfg(not(windows))]
fn write_shortcut(_lnk: &Path, _target: &Path, _app_id: &str) -> Result<(), String> {
    Err("shortcuts exist only on Windows".into())
}
