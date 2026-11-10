//! Windows: who may change a program file. The scheduled task starts the daemon's program as
//! the user every time it is not running, so a program file (or a folder above it) that
//! another user of the computer can write is a way to run code as this user. `install-service`
//! refuses such a place ([`check_program`]).
//!
//! The rules are plain functions over security ids and access masks, tested on every system;
//! reading the owner and the access list is Windows code. When `sys.rs` gets access-list
//! helpers, this reading belongs there.

/// SYSTEM, the Administrators group and TrustedInstaller (the owner of Windows' own files).
/// Whoever is one of these can do anything on the computer already.
const ADMIN_SIDS: &[&str] = &[
    "S-1-5-18",
    "S-1-5-32-544",
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
];

/// CREATOR OWNER and OWNER RIGHTS stand for the owner, who is checked by itself.
const OWNER_SIDS: &[&str] = &["S-1-3-0", "S-1-3-4"];

const FILE_WRITE_DATA: u32 = 0x2;
const FILE_APPEND_DATA: u32 = 0x4;
const FILE_DELETE_CHILD: u32 = 0x40;
const DELETE: u32 = 0x1_0000;
const WRITE_DAC: u32 = 0x4_0000;
const WRITE_OWNER: u32 = 0x8_0000;
const GENERIC_ALL: u32 = 0x1000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

/// What lets someone change the program file, or put another file in its place in its folder.
pub const CHANGE: u32 = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_DELETE_CHILD
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_ALL
    | GENERIC_WRITE;

/// What lets someone move a folder above the program away and put another one there. Making
/// new files or folders in it (what every user may do in `C:\`) is not that.
pub const MOVE_AWAY: u32 = FILE_DELETE_CHILD | DELETE | WRITE_DAC | WRITE_OWNER | GENERIC_ALL;

/// The same for a drive's root, which nobody can move or delete.
pub const MOVE_CHILD: u32 = FILE_DELETE_CHILD | WRITE_DAC | WRITE_OWNER | GENERIC_ALL;

/// True for this user and for those who administer the computer.
pub fn trusted(sid: &str, me: &str) -> bool {
    sid.eq_ignore_ascii_case(me) || ADMIN_SIDS.iter().any(|a| sid.eq_ignore_ascii_case(a))
}

/// One "allow" entry of an access list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allow {
    pub sid: String,
    pub mask: u32,
    /// The entry is only a template for what is made inside the folder.
    pub inherit_only: bool,
}

/// The first entry that lets someone other than this user and the administrators do what
/// `forbidden` names.
pub fn untrusted_writer<'a>(entries: &'a [Allow], forbidden: u32, me: &str) -> Option<&'a Allow> {
    entries.iter().find(|e| {
        !e.inherit_only
            && e.mask & forbidden != 0
            && !trusted(&e.sid, me)
            && !OWNER_SIDS.iter().any(|o| e.sid.eq_ignore_ascii_case(o))
    })
}

/// The security id (`S-1-5-21-…`) of the user this process runs as.
#[cfg(windows)]
pub fn current_user_sid() -> Result<String, String> {
    win::current_user_sid()
}

/// Refuses a program file that someone other than this user and the administrators could
/// replace: the file, its folder, and every folder above it are checked.
#[cfg(windows)]
pub fn check_program(exe: &std::path::Path) -> Result<(), String> {
    let me = current_user_sid()?;
    let refuse = |path: &std::path::Path, why: String| {
        format!(
            "{} is not a safe place for the daemon: {why}. Put it in a folder only you or administrators can change (for example under %LOCALAPPDATA%\\Programs or Program Files).",
            path.display()
        )
    };
    let mut path = Some(exe);
    let mut depth = 0;
    while let Some(p) = path {
        let parent = p.parent().filter(|x| !x.as_os_str().is_empty());
        let forbidden = match (depth, parent) {
            (0 | 1, _) => CHANGE,
            (_, Some(_)) => MOVE_AWAY,
            (_, None) => MOVE_CHILD,
        };
        let (owner, entries) = win::owner_and_access(p)
            .map_err(|e| refuse(p, format!("its access list cannot be read ({e})")))?;
        if !trusted(&owner, &me) {
            return Err(refuse(p, format!("it belongs to another user ({owner})")));
        }
        let Some(entries) = entries else {
            return Err(refuse(
                p,
                "it has no access list: everyone may change it".into(),
            ));
        };
        if let Some(e) = untrusted_writer(&entries, forbidden, &me) {
            return Err(refuse(p, format!("{} may change it", e.sid)));
        }
        path = parent;
        depth += 1;
    }
    Ok(())
}

#[cfg(windows)]
mod win {
    use super::Allow;
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GetAce, GetTokenInformation, OWNER_SECURITY_INFORMATION,
        PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_ALLOWED_CALLBACK_ACE_TYPE: u8 = 9;
    const INHERIT_ONLY_ACE: u8 = 0x8;

    /// The start of every "allow" entry: header (type, flags, size), mask, then the id.
    #[repr(C)]
    struct AllowAce {
        ace_type: u8,
        ace_flags: u8,
        ace_size: u16,
        mask: u32,
        sid_start: u32,
    }

    fn sid_string(sid: PSID) -> Result<String, String> {
        let mut text: *mut u16 = std::ptr::null_mut();
        // SAFETY: `sid` is a valid security id; on success `text` is a NUL-terminated string
        // that LocalFree releases.
        unsafe {
            if ConvertSidToStringSidW(sid, &mut text) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let out = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text as _);
            Ok(out)
        }
    }

    pub fn current_user_sid() -> Result<String, String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: the pseudo handle of this process is always valid.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(format!(
                "cannot tell which user this is: {}",
                std::io::Error::last_os_error()
            ));
        }
        // A TOKEN_USER and the id it points to: far below this size. u64 keeps it aligned.
        let mut buf = [0u64; 32];
        let mut len = 0u32;
        // SAFETY: the buffer is writable for the size passed and aligned for TOKEN_USER.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buf.as_mut_ptr() as _,
                std::mem::size_of_val(&buf) as u32,
                &mut len,
            )
        };
        let res = if ok == 0 {
            Err(format!(
                "cannot tell which user this is: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            // SAFETY: the call filled the buffer with a TOKEN_USER whose id lies in it.
            sid_string(unsafe { (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid })
        };
        // SAFETY: the token handle is ours.
        unsafe { CloseHandle(token) };
        res
    }

    /// The owner of a file or folder and the "allow" entries of its access list (`None`: it
    /// has no list, which lets everyone do everything).
    pub fn owner_and_access(path: &Path) -> Result<(String, Option<Vec<Allow>>), String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut owner: PSID = std::ptr::null_mut();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: the name is NUL-terminated; the out pointers are valid; on success they
        // point into `descriptor`, which LocalFree releases below.
        let err = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if err != 0 {
            return Err(std::io::Error::from_raw_os_error(err as i32).to_string());
        }
        let read = || -> Result<(String, Option<Vec<Allow>>), String> {
            if owner.is_null() {
                return Err("no owner".into());
            }
            let owner = sid_string(owner)?;
            if dacl.is_null() {
                return Ok((owner, None));
            }
            let mut entries = vec![];
            // SAFETY: `dacl` points to a valid access list inside `descriptor`.
            let count = unsafe { (*dacl).AceCount };
            for i in 0..u32::from(count) {
                let mut ace = std::ptr::null_mut();
                // SAFETY: the index is below the count; `ace` receives a pointer into the list.
                if unsafe { GetAce(dacl, i, &mut ace) } == 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                let ace = ace as *const AllowAce;
                // SAFETY: every entry starts with the header; "allow" entries have the mask
                // and the id right after it.
                unsafe {
                    let kind = (*ace).ace_type;
                    if kind != ACCESS_ALLOWED_ACE_TYPE && kind != ACCESS_ALLOWED_CALLBACK_ACE_TYPE {
                        continue;
                    }
                    entries.push(Allow {
                        sid: sid_string(std::ptr::addr_of!((*ace).sid_start) as PSID)?,
                        mask: (*ace).mask,
                        inherit_only: (*ace).ace_flags & INHERIT_ONLY_ACE != 0,
                    });
                }
            }
            Ok((owner, Some(entries)))
        };
        let res = read();
        // SAFETY: the descriptor was allocated by GetNamedSecurityInfoW.
        unsafe { LocalFree(descriptor as _) };
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "S-1-5-21-1-2-3-1001";
    const OTHER: &str = "S-1-5-21-1-2-3-1002";
    const USERS: &str = "S-1-5-32-545";
    const AUTHENTICATED: &str = "S-1-5-11";

    fn allow(sid: &str, mask: u32, inherit_only: bool) -> Allow {
        Allow {
            sid: sid.into(),
            mask,
            inherit_only,
        }
    }

    #[test]
    fn this_user_and_administrators_are_trusted() {
        assert!(trusted(ME, ME));
        assert!(trusted("S-1-5-18", ME));
        assert!(trusted("S-1-5-32-544", ME));
        assert!(!trusted(OTHER, ME));
        assert!(!trusted(USERS, ME));
    }

    #[test]
    fn a_folder_others_may_write_is_refused() {
        const READ_EXECUTE: u32 = 0x1200A9;
        const MODIFY: u32 = 0x1301BF;
        const FULL: u32 = 0x1F01FF;
        // A per-user install folder, and Program Files: others read, nobody else writes.
        let private = [allow(ME, FULL, false), allow("S-1-5-18", FULL, false)];
        assert_eq!(untrusted_writer(&private, CHANGE, ME), None);
        let program_files = [
            allow("S-1-5-32-544", FULL, false),
            allow(USERS, READ_EXECUTE, false),
            allow("S-1-3-0", GENERIC_ALL, true),
        ];
        assert_eq!(untrusted_writer(&program_files, CHANGE, ME), None);
        // D:\Apps\X: every signed-in user may modify.
        let shared = [allow(ME, FULL, false), allow(AUTHENTICATED, MODIFY, false)];
        assert_eq!(
            untrusted_writer(&shared, CHANGE, ME).map(|e| e.sid.as_str()),
            Some(AUTHENTICATED)
        );
        // Another user alone is enough.
        let other = [allow(OTHER, FILE_WRITE_DATA, false)];
        assert!(untrusted_writer(&other, CHANGE, ME).is_some());
    }

    #[test]
    fn making_folders_in_a_drive_root_is_not_moving_the_folders_in_it() {
        // C:\: signed-in users may make folders there, and get "modify" in the ones they
        // make (an inherit-only entry).
        let root = [
            allow("S-1-5-32-544", 0x1F01FF, false),
            allow(AUTHENTICATED, FILE_APPEND_DATA, false),
            allow(AUTHENTICATED, 0x1301BF, true),
        ];
        assert_eq!(untrusted_writer(&root, MOVE_CHILD, ME), None);
        assert!(untrusted_writer(&root, CHANGE, ME).is_some());
        // A second drive's root: "modify" for signed-in users, which has no "delete what is
        // in it" for the root itself.
        let d = [allow(AUTHENTICATED, 0x1301BF, false)];
        assert_eq!(untrusted_writer(&d, MOVE_CHILD, ME), None);
        assert!(untrusted_writer(&d, MOVE_AWAY, ME).is_some());
    }
}
