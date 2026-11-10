//! The few things the daemon asks of the operating system that Unix and Windows spell
//! differently: private files, the file owner, the host name, disk space and load.
//!
//! Windows has no file modes. The daemon's files (config, machine token, key store, run
//! folders) live in the user's profile, whose ACL lets only that user, SYSTEM and
//! administrators in, so the mode helpers do nothing there and the owner checks pass.

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

/// `chmod`.
pub fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// True when the group or everyone may read, write or run the file (mode bits `077`).
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

/// The effective user id of the daemon (0 on Windows, like [`owner`]).
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

/// The 1, 5 and 15 minute load averages. Windows has none: zeroes.
pub fn load_avg() -> [f64; 3] {
    #[cfg(unix)]
    {
        let mut l = [0f64; 3];
        // SAFETY: l has room for 3 doubles.
        let n = unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
        if n < 3 { [0.0; 3] } else { l }
    }
    #[cfg(not(unix))]
    [0.0; 3]
}
