//! Windows: who may open the daemon's private files.
//!
//! Unix answers this with the mode bits `077`. Windows answers it with the file's owner and
//! its access list (DACL). A file is private when its owner is the daemon's user (or SYSTEM
//! or the Administrators group) and every entry that allows access names one of those three.
//! [`protect`] writes exactly such a list, cut off from what the parent folder would hand
//! down, so a run folder under `D:\runs` is as closed as one in the user's profile.
//!
//! A volume without access lists (FAT, exFAT, most network shares) cannot be made private:
//! [`protect`] or [`check`] fails there and the daemon refuses to put a run on it.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid,
    GetAce, GetSecurityDescriptorDacl, GetTokenInformation, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    TOKEN_QUERY, TokenOwner, TokenUser, WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid,
    WinCreatorOwnerSid, WinLocalSystemSid,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const ACE_ALLOWED: u8 = 0;
const ACE_DENIED: u8 = 1;
/// The rights a read-only checkout takes from the user: write and append data (in a folder:
/// add files and folders), write extended attributes and attributes, delete children, delete.
const WRITE_RIGHTS: &str = "0x10156";

/// A SID in a buffer of its own (`SECURITY_MAX_SID_SIZE` is 68 bytes), aligned for the struct.
type SidBuf = [u64; 9];

/// The SIDs that may own and open a private file.
struct Trusted {
    /// `TOKEN_USER` and `TOKEN_OWNER` of the daemon's process: each starts with a pointer to a
    /// SID that lies in the same buffer.
    user: Vec<u64>,
    owner: Vec<u64>,
    /// The user's SID as text (`S-1-5-21-...`), for the access lists [`protect`] writes.
    user_text: String,
    system: SidBuf,
    admins: SidBuf,
    creator_owner: SidBuf,
    owner_rights: SidBuf,
}

impl Trusted {
    fn user(&self) -> PSID {
        // SAFETY: the buffer holds a TOKEN_USER, whose first field is the SID pointer.
        unsafe { *self.user.as_ptr().cast::<PSID>() }
    }
    fn owner(&self) -> PSID {
        // SAFETY: the buffer holds a TOKEN_OWNER, whose only field is the SID pointer.
        unsafe { *self.owner.as_ptr().cast::<PSID>() }
    }
    fn sid(buf: &SidBuf) -> PSID {
        buf.as_ptr().cast_mut().cast()
    }
    /// May own a private file: the user, the owner new files of this process get (the
    /// Administrators group for an elevated process), SYSTEM, Administrators.
    fn may_own(&self, sid: PSID) -> bool {
        [
            self.user(),
            self.owner(),
            Self::sid(&self.system),
            Self::sid(&self.admins),
        ]
        .into_iter()
        // SAFETY: all are valid SIDs.
        .any(|s| unsafe { EqualSid(s, sid) } != 0)
    }
    /// May be allowed access: the owners above, and the two placeholders that stand for the
    /// file's own owner (`CREATOR OWNER`, `OWNER RIGHTS`).
    fn may_open(&self, sid: PSID) -> bool {
        self.may_own(sid)
            || [
                Self::sid(&self.creator_owner),
                Self::sid(&self.owner_rights),
            ]
            .into_iter()
            // SAFETY: all are valid SIDs.
            .any(|s| unsafe { EqualSid(s, sid) } != 0)
    }
}

// SAFETY: the pointers lead into the struct's own heap buffers, which never change.
unsafe impl Send for Trusted {}
// SAFETY: as above; nothing is mutated after construction.
unsafe impl Sync for Trusted {}

fn os_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

fn token_info(token: HANDLE, class: i32) -> std::io::Result<Vec<u64>> {
    let mut len = 0u32;
    // SAFETY: a size query with a null buffer.
    unsafe {
        GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut len);
    }
    let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
    // SAFETY: the buffer holds at least `len` bytes.
    let ok = unsafe {
        GetTokenInformation(
            token,
            class,
            buf.as_mut_ptr().cast::<c_void>(),
            (buf.len() * 8) as u32,
            &mut len,
        )
    };
    if ok == 0 { Err(os_error()) } else { Ok(buf) }
}

fn well_known(kind: i32) -> std::io::Result<SidBuf> {
    let mut buf: SidBuf = [0; 9];
    let mut len = std::mem::size_of::<SidBuf>() as u32;
    // SAFETY: the buffer is large enough for any SID.
    let ok = unsafe {
        CreateWellKnownSid(
            kind,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if ok == 0 { Err(os_error()) } else { Ok(buf) }
}

fn sid_text(sid: PSID) -> String {
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: `sid` is a valid SID; the call allocates the string.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return "an unknown account".into();
    }
    // SAFETY: a NUL-terminated string from the call above, freed below.
    let s = unsafe {
        let len = (0..).take_while(|i| *text.add(*i) != 0).count();
        String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
    };
    // SAFETY: allocated by ConvertSidToStringSidW.
    unsafe {
        LocalFree(text.cast());
    }
    s
}

fn trusted() -> std::io::Result<&'static Trusted> {
    static T: OnceLock<Result<Trusted, String>> = OnceLock::new();
    T.get_or_init(|| {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: the pseudo handle of this process; `token` receives a new handle.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(os_error().to_string());
        }
        let made = (|| {
            let user = token_info(token, TokenUser)?;
            let owner = token_info(token, TokenOwner)?;
            let mut t = Trusted {
                user,
                owner,
                user_text: String::new(),
                system: well_known(WinLocalSystemSid)?,
                admins: well_known(WinBuiltinAdministratorsSid)?,
                creator_owner: well_known(WinCreatorOwnerSid)?,
                owner_rights: well_known(WinCreatorOwnerRightsSid)?,
            };
            t.user_text = sid_text(t.user());
            Ok::<_, std::io::Error>(t)
        })();
        // SAFETY: the token handle is ours.
        unsafe {
            CloseHandle(token);
        }
        made.map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|e| std::io::Error::other(e.clone()))
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// A security descriptor made from SDDL text; freed when dropped.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn parse(sddl: &str) -> std::io::Result<Descriptor> {
        let text: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `text` is NUL-terminated; `sd` receives an allocation freed in Drop.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(os_error());
        }
        Ok(Descriptor(sd))
    }

    fn dacl(&self) -> std::io::Result<*mut ACL> {
        let (mut present, mut defaulted) = (0, 0);
        let mut dacl: *mut ACL = std::ptr::null_mut();
        // SAFETY: the descriptor is valid; the out pointers are valid.
        let ok =
            unsafe { GetSecurityDescriptorDacl(self.0, &mut present, &mut dacl, &mut defaulted) };
        if ok == 0 || present == 0 || dacl.is_null() {
            return Err(std::io::Error::other("the access list was not built"));
        }
        Ok(dacl)
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn set_dacl(path: &Path, sddl: &str) -> std::io::Result<()> {
    let sd = Descriptor::parse(sddl)?;
    let dacl = sd.dacl()?;
    // SAFETY: the path is NUL-terminated and the access list lives until `sd` drops.
    // PROTECTED: nothing is inherited from the parent folder.
    let err = unsafe {
        SetNamedSecurityInfoW(
            wide(path).as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dacl,
            std::ptr::null(),
        )
    };
    if err != 0 {
        return Err(std::io::Error::from_raw_os_error(err as i32));
    }
    Ok(())
}

/// The entries that give the user, SYSTEM and Administrators full access. `inherit`: also to
/// everything made in the folder later.
fn full_access(user: &str, inherit: bool) -> String {
    let flags = if inherit { "OICI" } else { "" };
    format!("(A;{flags};FA;;;{user})(A;{flags};FA;;;SY)(A;{flags};FA;;;BA)")
}

/// Gives `path` (a file or a folder) an access list of its own: the daemon's user, SYSTEM and
/// Administrators, nobody else, nothing inherited from above. What is made in a folder later
/// inherits the same list.
pub fn protect(path: &Path) -> std::io::Result<()> {
    let t = trusted()?;
    let is_dir = std::fs::metadata(path)?.is_dir();
    set_dacl(path, &format!("D:P{}", full_access(&t.user_text, is_dir)))
}

/// Makes the folder `dir` and everything in it read-only for the daemon's user (an entry that
/// denies the write rights, inherited by all that is in it and all that would be made there),
/// or gives the write access back. Like the Unix mode bits, the user could undo it by hand;
/// a CLI that only writes files cannot. A process that holds the backup and restore
/// privileges (an elevated administrator's) is not held back from making folders or deleting,
/// as root is not held back by mode bits.
pub fn set_read_only(dir: &Path, read_only: bool) -> std::io::Result<()> {
    let t = trusted()?;
    let deny = if read_only {
        format!("(D;OICI;{WRITE_RIGHTS};;;{})", t.user_text)
    } else {
        String::new()
    };
    set_dacl(
        dir,
        &format!("D:P{deny}{}", full_access(&t.user_text, true)),
    )
}

/// Why `path` is not private to the daemon's user. `Ok` when its owner is that user (or SYSTEM
/// or Administrators) and nobody else is allowed any access.
pub fn check(path: &Path) -> Result<(), String> {
    let t = trusted().map_err(|e| format!("cannot read this user's identity: {e}"))?;
    let mut owner: PSID = std::ptr::null_mut();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: the path is NUL-terminated; the out pointers are valid; `sd` is freed below.
    let err = unsafe {
        GetNamedSecurityInfoW(
            wide(path).as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut sd,
        )
    };
    if err != 0 {
        return Err(format!(
            "cannot read its access list: {}",
            std::io::Error::from_raw_os_error(err as i32)
        ));
    }
    let verdict = (|| {
        if owner.is_null() || !t.may_own(owner) {
            return Err(format!(
                "it belongs to {}, not to this user",
                if owner.is_null() {
                    "nobody".into()
                } else {
                    sid_text(owner)
                }
            ));
        }
        if dacl.is_null() {
            return Err("it has no access list: everyone may open it".into());
        }
        // SAFETY: a valid ACL returned by the call above.
        let count = unsafe { (*dacl).AceCount } as u32;
        for i in 0..count {
            let mut ace: *mut c_void = std::ptr::null_mut();
            // SAFETY: `i` is below the entry count.
            if unsafe { GetAce(dacl, i, &mut ace) } == 0 || ace.is_null() {
                return Err("its access list cannot be read".into());
            }
            // SAFETY: every entry starts with an ACE_HEADER.
            let kind = unsafe { (*ace.cast::<ACE_HEADER>()).AceType };
            match kind {
                ACE_DENIED => {}
                ACE_ALLOWED => {
                    let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
                    // SAFETY: an allowed entry is an ACCESS_ALLOWED_ACE; its SID starts at
                    // the SidStart field.
                    let (mask, sid) =
                        unsafe { ((*allowed).Mask, (&raw mut (*allowed).SidStart).cast()) };
                    if mask != 0 && !t.may_open(sid) {
                        return Err(format!("{} may open it", sid_text(sid)));
                    }
                }
                other => {
                    return Err(format!(
                        "its access list has an entry of a kind ({other}) that is not checked"
                    ));
                }
            }
        }
        Ok(())
    })();
    // SAFETY: allocated by GetNamedSecurityInfoW.
    unsafe {
        LocalFree(sd);
    }
    verdict
}

/// Security attributes for a kernel object (a named pipe) that only the daemon's user may
/// open. Keep the value alive while the attributes are in use.
pub struct OnlyMe {
    _sd: Descriptor,
    attrs: SECURITY_ATTRIBUTES,
}

impl OnlyMe {
    pub fn new() -> std::io::Result<OnlyMe> {
        let t = trusted()?;
        let sd = Descriptor::parse(&format!("D:P(A;;GA;;;{})", t.user_text))?;
        let attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        Ok(OnlyMe { _sd: sd, attrs })
    }

    pub fn as_ptr(&mut self) -> *mut c_void {
        (&raw mut self.attrs).cast()
    }
}

// SAFETY: the descriptor is an allocation of its own that is only read.
unsafe impl Send for OnlyMe {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lets everyone in, the way a folder under `D:\` is by default.
    fn open_to_everyone(path: &Path) {
        let is_dir = path.is_dir();
        let flags = if is_dir { "OICI" } else { "" };
        set_dacl(path, &format!("D:P(A;{flags};FA;;;WD)")).unwrap();
    }

    #[test]
    fn a_protected_folder_is_private_and_so_is_what_is_made_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path().join("run");
        std::fs::create_dir(&run).unwrap();
        open_to_everyone(&run);
        let why = check(&run).unwrap_err();
        assert!(why.contains("S-1-1-0"), "{why}");
        // A file made there inherits the open list.
        std::fs::write(run.join("old.req"), "x").unwrap();
        assert!(check(&run.join("old.req")).is_err());

        protect(&run).unwrap();
        check(&run).unwrap();
        // The file that was there, and one made now, and a folder with a file in it.
        check(&run.join("old.req")).unwrap();
        std::fs::write(run.join("new.req"), "x").unwrap();
        check(&run.join("new.req")).unwrap();
        std::fs::create_dir(run.join("work")).unwrap();
        std::fs::write(run.join("work").join("a"), "x").unwrap();
        check(&run.join("work").join("a")).unwrap();
        // Still usable by this user.
        assert_eq!(std::fs::read_to_string(run.join("new.req")).unwrap(), "x");
    }

    #[test]
    fn a_file_others_may_open_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("token");
        std::fs::write(&token, "t").unwrap();
        open_to_everyone(&token);
        assert!(check(&token).is_err());
        protect(&token).unwrap();
        check(&token).unwrap();
        // Authenticated Users with read access only is still somebody else.
        set_dacl(
            &token,
            &format!(
                "D:P(A;;FR;;;AU){}",
                full_access(&trusted().unwrap().user_text, false)
            ),
        )
        .unwrap();
        let why = check(&token).unwrap_err();
        assert!(why.contains("S-1-5-11"), "{why}");
        assert!(check(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn a_read_only_folder_refuses_writes_until_it_is_writable_again() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(work.join("src")).unwrap();
        std::fs::write(work.join("src").join("a.txt"), "old").unwrap();
        set_read_only(&work, true).unwrap();
        assert!(std::fs::write(work.join("src").join("a.txt"), "new").is_err());
        assert!(std::fs::write(work.join("new.txt"), "x").is_err());
        assert!(std::fs::write(work.join("src").join("new.txt"), "x").is_err());
        // Not asserted: making a folder and removing a file. Windows does both with "backup
        // intent", which skips the access list for a process that holds the restore privilege
        // (an elevated one, like the CI runner). A CLI of an ordinary user is refused.
        assert_eq!(
            std::fs::read_to_string(work.join("src").join("a.txt")).unwrap(),
            "old"
        );
        // Still private while read-only.
        check(&work).unwrap();
        set_read_only(&work, false).unwrap();
        std::fs::write(work.join("src").join("a.txt"), "new").unwrap();
        std::fs::write(work.join("new.txt"), "x").unwrap();
        std::fs::remove_dir_all(&work).unwrap();
    }
}
