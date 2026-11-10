//! Restart on a new version: when the daemon's own program file is replaced (the app updated
//! itself, or someone copied a new binary over it), the running process still runs the old
//! code. The daemon notices the new file and, once no run is active, exits so that the service
//! manager (launchd `KeepAlive`, systemd `Restart=always`) starts the new version. Nobody has to restart anything by hand.
//!
//! Windows: a running program file cannot be written, but it can be renamed, so an update
//! moves it aside and puts the new file in its place; the stamp there is the file's size,
//! write time and creation time.

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

    /// Windows keeps no inode number in the standard metadata: the creation time stands in
    /// for it (a new file moved into place has a new one).
    #[cfg(windows)]
    pub fn of(path: &Path) -> Option<Stamp> {
        use std::os::windows::fs::MetadataExt as _;
        let m = std::fs::metadata(path).ok()?;
        m.is_file().then(|| Stamp {
            dev: 0,
            ino: m.creation_time(),
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

    /// The running program's file.
    pub fn current() -> Option<Watch> {
        std::env::current_exe().ok().map(Watch::new)
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

#[cfg(test)]
mod tests {
    use super::*;

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
