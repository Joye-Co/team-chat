//! 登录时启动 (设置 › 通用): the app's own login item through
//! `SMAppService.mainAppService` (macOS 13+). The system owns the truth: the
//! person can also turn it off in 系统设置 › 通用 › 登录项, so the switch reads
//! the service's status, not only the saved choice.
//!
//! Without an app bundle (`cargo run`) there is no service: the switch keeps
//! the saved choice and nothing is registered.
//!
//! On Windows the login item is a value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` named by the app id, holding the
//! quoted path of this exe. The person can turn it off in 任务管理器 › 启动应用 (or 设置 ›
//! 应用 › 启动), which Windows records under `Explorer\StartupApproved\Run`; the switch
//! reads both, and turning it on clears that record.

/// `SMAppServiceStatus`: what the system says about the login item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NotRegistered,
    Enabled,
    /// Registered, but the person has to allow it in 登录项.
    RequiresApproval,
    NotFound,
}

impl Status {
    fn from_raw(raw: isize) -> Self {
        match raw {
            1 => Self::Enabled,
            2 => Self::RequiresApproval,
            3 => Self::NotFound,
            _ => Self::NotRegistered,
        }
    }

    /// The switch is on while the item is registered.
    pub fn is_on(self) -> bool {
        matches!(self, Self::Enabled | Self::RequiresApproval)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::{
        msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject},
    };
    use objc2_foundation::{NSBundle, NSError};

    use super::Status;

    #[link(name = "ServiceManagement", kind = "framework")]
    unsafe extern "C" {}

    fn main_app() -> Option<Retained<AnyObject>> {
        // A process without a bundle id has no login item of its own.
        NSBundle::mainBundle().bundleIdentifier()?;
        let class = AnyClass::get(c"SMAppService")?;
        // SAFETY: `+[SMAppService mainAppService]` returns the app's service.
        unsafe { msg_send![class, mainAppService] }
    }

    pub fn status() -> Option<Status> {
        let service = main_app()?;
        // SAFETY: `status` is an NSInteger property.
        let raw: isize = unsafe { msg_send![&service, status] };
        Some(Status::from_raw(raw))
    }

    pub fn set(on: bool) -> Result<Status, String> {
        let service = main_app().ok_or("no app bundle")?;
        // SAFETY: both methods take an NSError out-parameter and return BOOL.
        let done: Result<(), Retained<NSError>> = unsafe {
            if on {
                msg_send![&service, registerAndReturnError: _]
            } else {
                msg_send![&service, unregisterAndReturnError: _]
            }
        };
        done.map_err(|e| e.localizedDescription().to_string())?;
        let now = status().unwrap_or(Status::NotRegistered);
        if now == Status::RequiresApproval {
            // The person has to allow it: show them where.
            let class = AnyClass::get(c"SMAppService").ok_or("no SMAppService")?;
            // SAFETY: a class method without arguments.
            let _: () = unsafe { msg_send![class, openSystemSettingsLoginItems] };
        }
        Ok(now)
    }
}

/// The Run value for an exe: its path in quotes, so a path with spaces is one argument.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn run_command(exe: &std::path::Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Whether a Run value starts this exe (quoted or not, any case, with or without
/// arguments after it).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn runs_exe(value: &str, exe: &std::path::Path) -> bool {
    let exe = exe.display().to_string().to_lowercase();
    let value = value.trim().to_lowercase();
    match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next() == Some(exe.as_str()),
        None => value == exe || value.starts_with(&format!("{exe} ")),
    }
}

/// Whether a `StartupApproved\Run` record leaves the item on. Windows writes 12 bytes: a
/// flag and the time it was turned off. An even flag (2, 6) is on, an odd one (3, 7) is off;
/// no record is on.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn approved(record: Option<&[u8]>) -> bool {
    record
        .and_then(|bytes| bytes.first())
        .is_none_or(|flag| flag & 1 == 0)
}

/// The program a Run value starts (quoted, or the first word).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn run_value_exe(value: &str) -> Option<std::path::PathBuf> {
    let value = value.trim();
    let program = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => value.split_whitespace().next()?,
    };
    (!program.is_empty()).then(|| program.into())
}

/// What the registry says: on when the Run value starts this app and the person has not
/// turned it off. `is_ours` says whether a program is this app's exe: the same path, or
/// another name of the same file (after the product was renamed the installer keeps the
/// old exe name as a hard link, and a value written before still names it).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn registry_status(
    run: Option<&str>,
    record: Option<&[u8]>,
    is_ours: impl Fn(&std::path::Path) -> bool,
) -> Status {
    match run.and_then(run_value_exe) {
        Some(program) if is_ours(&program) && approved(record) => Status::Enabled,
        _ => Status::NotRegistered,
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use windows_registry::{CURRENT_USER, Type};

    use super::Status;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    /// The value's name: the app id, so a debug build and the installed app do not share it.
    fn name() -> &'static str {
        crate::provider::session::app_id()
    }

    /// Whether `program` is `exe`: the same path, or the same file under another name.
    fn is_exe(program: &std::path::Path, exe: &std::path::Path) -> bool {
        super::runs_exe(&super::run_command(program), exe)
            || same_file::is_same_file(program, exe).unwrap_or(false)
    }

    pub(super) fn status_of(name: &str) -> Option<Status> {
        Some(status_for(name, &std::env::current_exe().ok()?))
    }

    pub(super) fn status_for(name: &str, exe: &std::path::Path) -> Status {
        let run = CURRENT_USER
            .open(RUN)
            .and_then(|key| key.get_string(name))
            .ok();
        let record = CURRENT_USER
            .open(APPROVED)
            .and_then(|key| key.get_value(name))
            .ok();
        super::registry_status(run.as_deref(), record.as_deref(), |program| {
            is_exe(program, exe)
        })
    }

    /// Points the Run value at `exe` when it names another path of the same file (the
    /// exe's former name): the next rename or clean-up of old names then cannot turn the
    /// login item off. Leaves a value of another program, and the person's on/off record,
    /// alone. True when it rewrote the value.
    pub(super) fn adopt_for(name: &str, exe: &std::path::Path) -> bool {
        let Ok(key) = CURRENT_USER.create(RUN) else {
            return false;
        };
        let Some(program) = key
            .get_string(name)
            .ok()
            .and_then(|value| super::run_value_exe(&value))
        else {
            return false;
        };
        let same_path = super::runs_exe(&super::run_command(&program), exe);
        if same_path || !same_file::is_same_file(&program, exe).unwrap_or(false) {
            return false;
        }
        key.set_string(name, super::run_command(exe)).is_ok()
    }

    pub fn adopt() -> bool {
        std::env::current_exe().is_ok_and(|exe| adopt_for(name(), &exe))
    }

    pub(super) fn set_of(name: &str, on: bool) -> Result<Status, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let key = CURRENT_USER.create(RUN).map_err(|e| e.to_string())?;
        if on {
            key.set_string(name, super::run_command(&exe))
                .map_err(|e| e.to_string())?;
            // Turned off in 任务管理器 before: the switch turns it back on.
            if let Ok(approved) = CURRENT_USER.create(APPROVED) {
                let _ =
                    approved.set_bytes(name, Type::Bytes, &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            }
        } else {
            match key.remove_value(name) {
                Ok(()) => {}
                // Not there: already off.
                Err(_) if key.get_string(name).is_err() => {}
                Err(e) => return Err(e.to_string()),
            }
            if let Ok(approved) = CURRENT_USER.create(APPROVED) {
                let _ = approved.remove_value(name);
            }
        }
        Ok(status_of(name).unwrap_or(Status::NotRegistered))
    }

    pub fn status() -> Option<Status> {
        status_of(name())
    }

    pub fn set(on: bool) -> Result<Status, String> {
        set_of(name(), on)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use super::Status;
    pub fn status() -> Option<Status> {
        None
    }
    pub fn set(_on: bool) -> Result<Status, String> {
        Err("not macOS".into())
    }
}

thread_local! {
    /// What the system said last; `None` until it is asked. Asking is a
    /// synchronous XPC call to the system (about 1.5 ms), too slow for every
    /// frame of the 通用 pane, so the pane asks again only when it is shown,
    /// when its window comes to the front and after the switch changes it.
    static LAST: std::cell::Cell<Option<Option<Status>>> = const { std::cell::Cell::new(None) };
}

/// Asks the system again (the person may have changed it in 系统设置 ›
/// 登录项), and keeps the answer for [`status`].
pub fn refresh() -> Option<Status> {
    let now = if super::prefs::saving() {
        imp::status()
    } else {
        None
    };
    LAST.with(|l| l.set(Some(now)));
    now
}

/// What the system said last, or `None` when this process has no login item
/// (no bundle, self-tests).
pub fn status() -> Option<Status> {
    LAST.with(|l| l.get()).unwrap_or_else(refresh)
}

/// At app start on Windows: a login item written under the exe's former name now names
/// the exe that is running (see `imp::adopt_for`).
pub fn adopt_current_exe() {
    #[cfg(target_os = "windows")]
    if super::prefs::saving() && imp::adopt() {
        tracing::info!("the login item now names the current exe");
        refresh();
    }
}

/// Whether the switch shows on: the system's state, else the saved choice.
pub fn shown_on(saved: bool) -> bool {
    status().map_or(saved, Status::is_on)
}

/// Turns the login item on or off and saves what the system now says.
pub fn set(on: bool) {
    if !super::prefs::saving() {
        return;
    }
    let result = match imp::status() {
        Some(_) => imp::set(on).map(Status::is_on),
        None => Ok(on),
    };
    let now = match result {
        Ok(now) => now,
        Err(error) => {
            tracing::warn!(%error, on, "login item");
            imp::status().is_some_and(Status::is_on)
        }
    };
    refresh();
    super::prefs::update(|p| p.launch_at_login = now);
}

#[cfg(test)]
mod tests {
    use super::Status;

    #[test]
    fn a_run_value_is_ours_when_it_starts_this_exe() {
        use std::path::Path;
        let exe = Path::new(r"C:\Program Files\App\app.exe");
        let command = super::run_command(exe);
        assert_eq!(command, r#""C:\Program Files\App\app.exe""#);
        assert!(super::runs_exe(&command, exe));
        assert!(super::runs_exe(
            r#""c:\program files\app\APP.EXE" --hidden"#,
            exe
        ));
        assert!(!super::runs_exe(r#""C:\Other\app.exe""#, exe));
        assert!(!super::runs_exe("", exe));
        let plain = Path::new(r"C:\App\app.exe");
        assert!(super::runs_exe(r"C:\App\app.exe", plain));
        assert!(super::runs_exe(r"C:\App\app.exe --hidden", plain));
        assert!(!super::runs_exe(r"C:\App\app.exe.old", plain));
    }

    #[test]
    fn the_task_manager_record_turns_the_item_off() {
        use std::path::Path;
        let exe = Path::new(r"C:\App\app.exe");
        let run = super::run_command(exe);
        let on = [2u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let off = [3u8, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];
        let status = |run: Option<&str>, record: Option<&[u8]>| {
            super::registry_status(run, record, |program| {
                super::runs_exe(&super::run_command(program), exe)
            })
            .is_on()
        };
        assert!(status(Some(&run), None));
        assert!(status(Some(&run), Some(&on)));
        assert!(status(Some(&run), Some(&[6])));
        assert!(!status(Some(&run), Some(&off)));
        assert!(!status(Some(&run), Some(&[7])));
        assert!(!status(None, Some(&on)));
        assert!(!status(Some(r#""C:\Other\app.exe""#), None));
    }

    /// Writes a value of its own under the real Run key of the signed-in user (Windows CI)
    /// and takes it out again.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_run_key_round_trip() {
        let name = format!("desktop-test-{}", chat_core::new_id());
        assert_eq!(super::imp::status_of(&name), Some(Status::NotRegistered));
        assert_eq!(super::imp::set_of(&name, true), Ok(Status::Enabled));
        assert_eq!(super::imp::status_of(&name), Some(Status::Enabled));
        // Turning it on again is fine; off twice too.
        assert_eq!(super::imp::set_of(&name, true), Ok(Status::Enabled));
        assert_eq!(super::imp::set_of(&name, false), Ok(Status::NotRegistered));
        assert_eq!(super::imp::set_of(&name, false), Ok(Status::NotRegistered));
        assert_eq!(super::imp::status_of(&name), Some(Status::NotRegistered));
    }

    #[test]
    fn a_run_value_names_its_program() {
        use std::path::Path;
        let exe = |value| super::run_value_exe(value);
        assert_eq!(
            exe(r#""C:\Program Files\App\app.exe" --hidden"#).as_deref(),
            Some(Path::new(r"C:\Program Files\App\app.exe"))
        );
        assert_eq!(
            exe(r"C:\App\app.exe --hidden").as_deref(),
            Some(Path::new(r"C:\App\app.exe"))
        );
        assert_eq!(exe(""), None);
        assert_eq!(exe("\"\""), None);
    }

    /// After a rename the install keeps the exe under its old name as a hard link, and a
    /// login item written before names that one. Started under the new name the switch
    /// still reads on, and the app rewrites the value to the exe that runs; a value of
    /// some other program is neither ours nor touched.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_login_item_under_the_former_exe_name_is_still_on_and_is_adopted() {
        use windows_registry::CURRENT_USER;
        const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
        let dir = tempfile::tempdir().unwrap();
        let (old, new, other) = (
            dir.path().join("OldName.exe"),
            dir.path().join("New Name.exe"),
            dir.path().join("Other.exe"),
        );
        std::fs::write(&old, b"MZ the app").unwrap();
        std::fs::hard_link(&old, &new).unwrap();
        std::fs::write(&other, b"MZ another program").unwrap();
        let name = format!("desktop-test-{}", chat_core::new_id());
        let key = CURRENT_USER.create(RUN).unwrap();
        let value = || CURRENT_USER.open(RUN).unwrap().get_string(&name).unwrap();

        // Written while the exe had its old name; the app now runs under the new one.
        key.set_string(&name, super::run_command(&old)).unwrap();
        assert_eq!(super::imp::status_for(&name, &new), Status::Enabled);
        assert!(super::imp::adopt_for(&name, &new), "rewritten");
        assert_eq!(value(), super::run_command(&new));
        assert_eq!(super::imp::status_for(&name, &new), Status::Enabled);
        assert!(
            !super::imp::adopt_for(&name, &new),
            "nothing to do the second time"
        );

        // Another program under this name is not this app.
        key.set_string(&name, super::run_command(&other)).unwrap();
        assert_eq!(super::imp::status_for(&name, &new), Status::NotRegistered);
        assert!(!super::imp::adopt_for(&name, &new));
        assert_eq!(value(), super::run_command(&other), "left alone");

        // A value that names a file that is gone is off, and left alone.
        key.set_string(&name, super::run_command(&dir.path().join("Gone.exe")))
            .unwrap();
        assert_eq!(super::imp::status_for(&name, &new), Status::NotRegistered);
        assert!(!super::imp::adopt_for(&name, &new));
        key.remove_value(&name).unwrap();
    }

    #[test]
    fn raw_status_maps_to_the_switch() {
        assert!(!Status::from_raw(0).is_on());
        assert!(Status::from_raw(1).is_on());
        assert!(Status::from_raw(2).is_on());
        assert!(!Status::from_raw(3).is_on());
    }
}
