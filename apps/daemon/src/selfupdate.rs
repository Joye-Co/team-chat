//! Restart on a new version: when the daemon's own program file is replaced (the app updated
//! itself, or someone copied a new binary over it), the running process still runs the old
//! code. The daemon notices the new file and, once no run is active, exits so that the service
//! manager (launchd `KeepAlive`, systemd `Restart=always`) starts the new version. Nobody has to restart anything by hand.
//!
//! Windows: a running program file cannot be written or deleted, but it can be renamed. The
//! replace protocol every updater follows there (the app's updater, the PowerShell install
//! command, a person doing it by hand):
//!
//! 1. write the new program completely under another name in the same folder
//!    (`<name>.exe.new`), and check it (SHA-256);
//! 2. rename the running `<name>.exe` to `<name>.exe.old-<anything unique>` in the same folder;
//! 3. rename `<name>.exe.new` to `<name>.exe`. If that fails, rename the old file back.
//!
//! The updater never stops the daemon or its scheduled task (that would cancel the runs). The
//! daemon does the rest: it sees another file (file index, size, write time)
//! at its own path, waits until the stamp is the same at two checks and no run is active, and
//! exits; the scheduled task starts the new file within a minute ([`crate::service`]). When a
//! daemon starts it deletes the `<name>.exe.old-*` files next to it ([`clean_stale`]); one
//! that is still running (the old daemon has not exited yet) stays until the next start.

use std::path::{Path, PathBuf};

/// What identifies one version of the program file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: i64,
    mtime_nsec: i64,
}

impl Stamp {
    #[cfg(unix)]
    pub fn of(path: &Path) -> Option<Stamp> {
        use std::os::unix::fs::MetadataExt as _;
        let m = std::fs::metadata(path).ok()?;
        (m.is_file() && m.mode() & 0o111 != 0).then(|| Stamp {
            dev: m.dev(),
            ino: m.ino(),
            size: m.size(),
            mtime: m.mtime(),
            mtime_nsec: m.mtime_nsec(),
        })
    }

    /// Windows: the volume serial number and the file index are what the device and inode
    /// are on Unix. The times alone would not do: a file renamed into a name that was just
    /// freed gets the creation time of the file that had the name (NTFS tunneling), and a
    /// copied file keeps its write time.
    #[cfg(windows)]
    pub fn of(path: &Path) -> Option<Stamp> {
        use std::os::windows::fs::MetadataExt as _;
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let file = std::fs::File::open(path).ok()?;
        let m = file.metadata().ok()?;
        if !m.is_file() {
            return None;
        }
        // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is a valid value to be filled in.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is an open file and `info` is a valid place to write.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
            return None;
        }
        Some(Stamp {
            dev: u64::from(info.dwVolumeSerialNumber),
            ino: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            size: m.file_size(),
            mtime: m.last_write_time() as i64,
            mtime_nsec: 0,
        })
    }
}

/// Watches the program file. [`Watch::check`] says when a new version is ready: the file
/// changed and stayed the same between two checks (an update may still be writing it).
#[derive(Debug)]
pub struct Watch {
    path: PathBuf,
    started: Option<Stamp>,
    pending: Option<Stamp>,
}

impl Watch {
    pub fn new(path: PathBuf) -> Watch {
        let started = Stamp::of(&path);
        Watch {
            path,
            started,
            pending: None,
        }
    }

    /// The running program's file. On Windows this is also when the program files an update
    /// moved aside are deleted.
    pub fn current() -> Option<Watch> {
        let exe = std::env::current_exe().ok()?;
        #[cfg(windows)]
        clean_stale(&exe);
        Some(Watch::new(exe))
    }

    /// True when a new program file has been in place since the previous check.
    pub fn check(&mut self) -> bool {
        let Some(started) = &self.started else {
            return false;
        };
        let now = Stamp::of(&self.path);
        match now {
            Some(s) if &s != started => {
                let ready = self.pending.as_ref() == Some(&s);
                self.pending = Some(s);
                ready
            }
            _ => {
                self.pending = None;
                false
            }
        }
    }
}

/// What an updater puts after the program's file name when it moves the running file aside
/// (`daemon.exe` becomes `daemon.exe.old-<anything>`).
pub const STALE_MARK: &str = ".old-";

/// The files next to `exe` that an update moved aside.
pub fn stale_files(exe: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (exe.parent(), exe.file_name()) else {
        return vec![];
    };
    let prefix = format!("{}{STALE_MARK}", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .collect()
}

/// Deletes the program files an update moved aside; returns how many are gone. One that
/// cannot be deleted (it is still running) is left for the next start.
pub fn clean_stale(exe: &Path) -> usize {
    stale_files(exe)
        .iter()
        .filter(|p| std::fs::remove_file(p).is_ok())
        .count()
}

/// Puts the program file back after an update that was interrupted between its two renames
/// (a power loss): when `exe` is missing and files moved aside are there, the newest of them
/// is renamed to `exe`. Returns the file it renamed. The scheduled task runs this before it
/// starts the daemon ([`crate::service`]), from a copy of the program that updates never touch.
pub fn repair(exe: &Path) -> Option<PathBuf> {
    if exe.exists() {
        return None;
    }
    let newest = stale_files(exe).into_iter().max_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH)
    })?;
    std::fs::rename(&newest, exe).ok().map(|()| newest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_comes_back_from_the_file_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("x-daemon.exe");
        let aside = dir.path().join("x-daemon.exe.old-7");
        // Nothing to repair: the program is there, or nothing was moved aside.
        assert_eq!(repair(&exe), None);
        std::fs::write(&exe, "v2").unwrap();
        std::fs::write(&aside, "v1").unwrap();
        assert_eq!(repair(&exe), None);
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "v2");
        // The update stopped after its first rename.
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(repair(&exe), Some(aside.clone()));
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "v1");
        assert!(!aside.exists());
    }

    #[test]
    fn only_the_files_an_update_moved_aside_are_stale() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("x-daemon.exe");
        for name in [
            "x-daemon.exe",
            "x-daemon.exe.old-1",
            "x-daemon.exe.old-20261110",
            "x-daemon.exe.new",
            "x-daemon.exe.older",
            "y-daemon.exe.old-1",
            "x-daemon.toml",
        ] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        std::fs::create_dir(dir.path().join("x-daemon.exe.old-dir")).unwrap();
        assert_eq!(stale_files(&exe).len(), 2);
        assert_eq!(clean_stale(&exe), 2);
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "x-daemon.exe",
                "x-daemon.exe.new",
                "x-daemon.exe.old-dir",
                "x-daemon.exe.older",
                "x-daemon.toml",
                "y-daemon.exe.old-1",
            ]
        );
        assert_eq!(clean_stale(&exe), 0);
    }

    fn write_exe(path: &Path, body: &str) {
        let tmp = path.with_extension("new");
        std::fs::write(&tmp, body).unwrap();
        crate::sys::set_mode(&tmp, 0o755).unwrap();
        // Replaced the way an update replaces it: a new file moved over the old one.
        std::fs::rename(&tmp, path).unwrap();
    }

    #[test]
    fn a_replaced_program_is_ready_after_it_stays_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("daemon");
        write_exe(&exe, "v1");
        let mut w = Watch::new(exe.clone());
        assert!(!w.check(), "unchanged");
        write_exe(&exe, "v2 longer");
        assert!(!w.check(), "just changed: it may still be written");
        assert!(w.check(), "the same new file one check later");
        assert!(w.check(), "stays ready until the daemon restarts");
        write_exe(&exe, "v3 even longer");
        assert!(!w.check(), "changed again: wait once more");
        assert!(w.check());
    }

    #[test]
    fn a_missing_or_not_executable_file_is_not_a_new_version() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("daemon");
        write_exe(&exe, "v1");
        let mut w = Watch::new(exe.clone());
        std::fs::remove_file(&exe).unwrap();
        assert!(!w.check());
        assert!(!w.check());
        // Not executable (a mode Windows does not have: any file there is a version).
        #[cfg(unix)]
        {
            std::fs::write(&exe, "half written").unwrap();
            assert!(!w.check());
            assert!(!w.check());
        }
        // No file at start: nothing to watch.
        let mut none = Watch::new(dir.path().join("missing"));
        assert!(!none.check());
    }
}
