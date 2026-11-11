//! The few things the daemon asks of the operating system that Unix and Windows spell
//! differently: private files, the file owner, the host name, disk space and load.
//!
//! Windows has no file modes: who may open a file is its owner and its access list
//! ([`crate::winacl`]). There a mode without bits for the group and others (`0600`, `0700`)
//! means an access list for the daemon's user, SYSTEM and Administrators only, and the checks
//! of the mode bits `077` and of the owner are [`acl_problem`].

use std::path::Path;

/// What separates the folders of `PATH`.
pub const PATH_SEP: char = if cfg!(windows) { ';' } else { ':' };

/// Variables every program on this system needs, which a run's CLI and a sign-in get besides
/// the daemon's own list. Windows: without `SystemRoot` sockets and crypto fail to load, and
/// the CLIs find their logins and caches through the profile folders. None on Unix.
#[cfg(not(windows))]
pub const OS_ENV: &[&str] = &[];
#[cfg(windows)]
pub const OS_ENV: &[&str] = &[
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "USERNAME",
    "USERDOMAIN",
    "COMPUTERNAME",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "TEMP",
    "TMP",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
];

/// Sets the Unix mode a new file is made with.
pub fn mode(opts: &mut std::fs::OpenOptions, mode: u32) -> &mut std::fs::OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    opts
}

/// `chmod`. Windows: a mode that leaves the group and others out gives the file or folder an
/// access list of its own for the daemon's user only ([`protect`]); any other mode does nothing.
pub fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        if mode & 0o077 == 0 {
            protect(path)
        } else {
            Ok(())
        }
    }
}

/// Windows: closes `path` (a file or a folder with all that is and will be in it) to everyone
/// but the daemon's user, SYSTEM and Administrators, whatever the folder above allows. Fails
/// on a volume without access lists. Unix: nothing, the mode of each file does this.
pub fn protect(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        crate::winacl::protect(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}

/// Makes the folder `path` and the folders above it. Windows: the folders above are made the
/// ordinary way; `path` itself is made with its access list (the daemon's user, SYSTEM and
/// Administrators, nothing inherited), so nobody else is ever in it, and a folder of that name
/// that is already there is used only when it is private ([`acl_problem`]). Unix:
/// `create_dir_all`.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::winacl::create_dir(path)
    }
    #[cfg(not(windows))]
    {
        std::fs::create_dir_all(path)
    }
}

/// Makes the new file `path` for writing, for the daemon's user alone: mode 0600 on Unix, on
/// Windows an access list that is part of the creation. Fails when the file is already there.
pub fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(windows)]
    {
        crate::winacl::create_file(path)
    }
    #[cfg(not(windows))]
    {
        mode(
            std::fs::OpenOptions::new().write(true).create_new(true),
            0o600,
        )
        .open(path)
    }
}

/// Windows: why somebody other than the daemon's user (and SYSTEM and Administrators) can get
/// at `path`: its owner or an entry of its access list. None when it is private. Unix: always
/// None; the mode bits ([`open_to_others`]) and the owner ([`owner`]) are checked there.
pub fn acl_problem(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        crate::winacl::check(path).err()
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// True when the group or everyone may read, write or run the file (mode bits `077`). Always
/// false on Windows: ask [`acl_problem`] there.
pub fn open_to_others(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o077 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// True for a regular file that can be started: an executable bit on Unix, any file on Windows
/// (the extension decides there, see [`crate::health::which`]).
pub fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    meta.is_file()
}

/// The effective user id of the daemon (0 on Windows, like [`owner`]: [`acl_problem`] checks
/// the owner there).
pub fn euid() -> u32 {
    #[cfg(unix)]
    // SAFETY: plain syscall without arguments.
    unsafe {
        libc::geteuid()
    }
    #[cfg(not(unix))]
    0
}

/// The user id that owns a file (0 on Windows, like [`euid`]).
pub fn owner(meta: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.uid()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

/// This machine's host name.
pub fn host_name() -> Option<String> {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: buf is valid for writes of buf.len() bytes.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        (rc == 0).then(|| {
            let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
            String::from_utf8_lossy(&buf[..end]).into_owned()
        })
    }
    #[cfg(not(unix))]
    std::env::var("COMPUTERNAME").ok().filter(|n| !n.is_empty())
}

/// Free and total bytes of the volume that holds `path` (an existing folder). Zeroes when the
/// system does not say.
pub fn disk(path: &Path) -> (u64, u64) {
    #[cfg(unix)]
    {
        let Ok(c) = std::ffi::CString::new(path.to_string_lossy().as_bytes()) else {
            return (0, 0);
        };
        // SAFETY: statvfs writes into a zeroed struct; c is a valid NUL-terminated path.
        unsafe {
            let mut s: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c.as_ptr(), &mut s) != 0 {
                return (0, 0);
            }
            let frsize = s.f_frsize as u64;
            (s.f_bavail as u64 * frsize, s.f_blocks as u64 * frsize)
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let (mut free, mut total) = (0u64, 0u64);
        // SAFETY: wide is NUL-terminated; the two out pointers are valid u64s.
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free,
                &mut total,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 { (0, 0) } else { (free, total) }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        (0, 0)
    }
}

/// The 1, 5 and 15 minute load averages. Windows keeps none, so the daemon keeps its own
/// ([`windows_load`]): zeroes until its first sample, five seconds after the first call.
pub fn load_avg() -> [f64; 3] {
    #[cfg(unix)]
    {
        let mut l = [0f64; 3];
        // SAFETY: l has room for 3 doubles.
        let n = unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
        if n < 3 { [0.0; 3] } else { l }
    }
    #[cfg(windows)]
    {
        windows_load::get()
    }
    #[cfg(not(any(unix, windows)))]
    [0.0; 3]
}

/// Windows: the processor time of the whole machine so far as (total, idle), in 100 ns units
/// summed over all processors.
#[cfg(windows)]
pub fn cpu_times() -> Option<(u64, u64)> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut idle, mut kernel, mut user) = (zero, zero, zero);
    // SAFETY: the three out pointers are valid FILETIMEs.
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    let n = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    // The kernel time includes the idle time.
    Some((n(kernel) + n(user), n(idle)))
}

/// One step of a load average: `prev` decays over `window` seconds and takes in `now`, the
/// load of the last `step` seconds. The same exponential average Unix kernels keep.
#[cfg(any(windows, test))]
fn load_step(prev: f64, now: f64, step: f64, window: f64) -> f64 {
    let keep = (-step / window).exp();
    prev * keep + now * (1.0 - keep)
}

/// The load averages Windows does not keep. Unix counts the processes that run or wait for a
/// processor; the nearest thing Windows gives without a performance counter session is how
/// many processors were busy: the busy share of the last five seconds times the number of
/// processors. Below full load the two say the same (2.0 is two busy processors); above it
/// this one stops at the processor count where Unix keeps counting the queue. The server uses
/// the number only to prefer the less busy of two machines.
#[cfg(windows)]
mod windows_load {
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    const STEP: Duration = Duration::from_secs(5);
    static LOAD: OnceLock<&'static Mutex<[f64; 3]>> = OnceLock::new();

    pub fn get() -> [f64; 3] {
        let load = LOAD.get_or_init(|| {
            let load: &'static Mutex<[f64; 3]> = Box::leak(Box::new(Mutex::new([0.0; 3])));
            let _ = std::thread::Builder::new()
                .name("load".into())
                .spawn(move || sample(load));
            load
        });
        *load.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn sample(load: &'static Mutex<[f64; 3]>) {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get() as f64)
            .unwrap_or(1.0);
        let mut prev = super::cpu_times();
        loop {
            std::thread::sleep(STEP);
            let now = super::cpu_times();
            if let (Some((t0, i0)), Some((t1, i1))) = (prev, now)
                && t1 > t0
                && i1 >= i0
            {
                let busy = 1.0 - ((i1 - i0) as f64 / (t1 - t0) as f64).clamp(0.0, 1.0);
                let mut l = load.lock().unwrap_or_else(|e| e.into_inner());
                for (v, window) in l.iter_mut().zip([60.0, 300.0, 900.0]) {
                    *v = super::load_step(*v, busy * cpus, STEP.as_secs_f64(), window);
                }
            }
            prev = now;
        }
    }
}

/// Folders under the user's profile and the system's program folders where coding CLIs and
/// the tools they need are usually installed, in the order they are added to `PATH`. Not all
/// of them are on a Windows user's `PATH`: the Claude Code installer puts `claude.exe` in
/// `.local\bin` and only says so. `var` reads an environment variable.
#[cfg(any(windows, test))]
fn usual_program_folders(
    var: impl Fn(&str) -> Option<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    let mut add = |base: &str, rest: &[&str]| {
        if let Some(mut p) = var(base) {
            p.extend(rest);
            out.push(p);
        }
    };
    add("USERPROFILE", &[".local", "bin"]);
    add("USERPROFILE", &[".claude", "local"]);
    add("APPDATA", &["npm"]);
    add("USERPROFILE", &[".bun", "bin"]);
    add("USERPROFILE", &[".cargo", "bin"]);
    add("ProgramFiles", &["nodejs"]);
    add("ProgramFiles", &["Git", "cmd"]);
    add("LOCALAPPDATA", &["Programs", "Git", "cmd"]);
    out
}

/// `path` with each of `more` that is not in it yet added at the end (what is on `PATH`
/// already wins), compared the way Windows compares paths: without case, and a trailing
/// separator does not count.
#[cfg(any(windows, test))]
fn path_with(path: &str, more: &[std::path::PathBuf], sep: char) -> String {
    let key = |p: &str| p.trim().trim_end_matches(['\\', '/']).to_ascii_lowercase();
    let mut parts: Vec<String> = path
        .split(sep)
        .filter(|p| !p.trim().is_empty())
        .map(str::to_string)
        .collect();
    for m in more {
        let m = m.to_string_lossy().into_owned();
        if !parts.iter().any(|p| key(p) == key(&m)) {
            parts.push(m);
        }
    }
    parts.join(&sep.to_string())
}

/// Windows: adds the usual program folders that exist to this process's `PATH`, after what
/// is there. On a Mac the app does this when it installs the service (the service keeps the
/// login shell's `PATH` plus the usual folders); a scheduled task gets the user's `PATH`
/// fresh at every start, so the daemon adds them itself, each time it starts. Call it before
/// any thread is started. Unix: nothing.
pub fn add_usual_program_folders() {
    #[cfg(windows)]
    {
        let folders: Vec<_> = usual_program_folders(|k| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(Into::into)
        })
        .into_iter()
        .filter(|p| p.is_dir())
        .collect();
        let path = std::env::var("PATH").unwrap_or_default();
        let next = path_with(&path, &folders, PATH_SEP);
        if next != path {
            // SAFETY: called at the start of `main`, before any other thread exists.
            unsafe { std::env::set_var("PATH", next) };
        }
    }
}

/// Windows: the Git Bash that Claude Code would use for its Bash tool and for shell hooks,
/// found the way the CLI finds it: `CLAUDE_CODE_GIT_BASH_PATH`, Git's usual install folders,
/// then next to the `git` on `PATH`. None: the CLI uses PowerShell as its shell tool.
#[cfg(windows)]
pub fn git_bash() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let mut tries: Vec<PathBuf> = var("CLAUDE_CODE_GIT_BASH_PATH").into_iter().collect();
    for base in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(p) = var(base) {
            tries.push(p.join("Git").join("bin").join("bash.exe"));
        }
    }
    if let Some(git) = crate::health::which("git")
        && let Some(root) = git.parent().and_then(Path::parent)
    {
        tries.push(root.join("bin").join("bash.exe"));
    }
    tries.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A steady load of two busy processors is 2.0 after a few windows, on each average at
    /// its own pace, and falls back to nothing when the machine goes idle.
    #[test]
    fn the_windows_load_average_follows_the_busy_processors() {
        let mut l = [0.0f64; 3];
        for _ in 0..(3600 / 5) {
            for (v, w) in l.iter_mut().zip([60.0, 300.0, 900.0]) {
                *v = load_step(*v, 2.0, 5.0, w);
            }
        }
        assert!(
            (l[0] - 2.0).abs() < 0.01 && (l[2] - 2.0).abs() < 0.05,
            "{l:?}"
        );
        let one_minute = load_step(0.0, 2.0, 5.0, 60.0);
        let fifteen = load_step(0.0, 2.0, 5.0, 900.0);
        assert!(one_minute > fifteen && fifteen > 0.0);
        for _ in 0..(3600 / 5) {
            l[0] = load_step(l[0], 0.0, 5.0, 60.0);
        }
        assert!(l[0] < 0.01);
    }

    /// The usual program folders come from the profile and program variables that are set,
    /// and are added to a PATH after what is there, once, compared as Windows compares paths.
    #[test]
    fn usual_program_folders_are_added_after_the_path_once() {
        let var = |k: &str| match k {
            "USERPROFILE" => Some(std::path::PathBuf::from(r"C:\Users\me")),
            "APPDATA" => Some(std::path::PathBuf::from(r"C:\Users\me\AppData\Roaming")),
            _ => None,
        };
        let folders = usual_program_folders(var);
        let names: Vec<String> = folders
            .iter()
            .map(|p| p.to_string_lossy().replace('/', "\\"))
            .collect();
        assert_eq!(
            names,
            [
                r"C:\Users\me\.local\bin",
                r"C:\Users\me\.claude\local",
                r"C:\Users\me\AppData\Roaming\npm",
                r"C:\Users\me\.bun\bin",
                r"C:\Users\me\.cargo\bin",
            ]
        );
        let more = [
            std::path::PathBuf::from(r"C:\Users\me\.local\bin"),
            std::path::PathBuf::from(r"C:\Users\Me\AppData\Roaming\NPM"),
        ];
        assert_eq!(
            path_with(r"C:\Windows;;c:\users\me\appdata\roaming\npm\", &more, ';'),
            r"C:\Windows;c:\users\me\appdata\roaming\npm\;C:\Users\me\.local\bin"
        );
        assert_eq!(path_with("", &more[..1], ';'), r"C:\Users\me\.local\bin");
        let same = r"C:\Windows;C:\Users\me\.local\bin";
        assert_eq!(path_with(same, &more[..1], ';'), same);
    }

    /// Windows: the machine's processor time moves forward and the idle part is within it.
    #[cfg(windows)]
    #[test]
    fn windows_cpu_times_move_forward() {
        let (t0, i0) = cpu_times().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let (t1, i1) = cpu_times().unwrap();
        assert!(t1 > t0 && i1 >= i0 && i1 - i0 <= t1 - t0);
        assert!(load_avg().iter().all(|v| *v >= 0.0));
    }
}
