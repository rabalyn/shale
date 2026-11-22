//! Owner-only access lists on Windows (DATA-014): what `chmod 0600` and `0700` are on Linux.
//!
//! [`restrict_to_owner`] gives a file or folder a *protected* access list (it takes nothing from
//! its parent folder) with two entries: full control for the current user, and for `SYSTEM`.
//! `SYSTEM` stays because Windows' own services run as it for the person's benefit (the virus
//! scanner, backup and file history, the removal of a profile) and fail on a file they cannot
//! open; it is no other person, and it can take over the computer anyway. Nobody else gets an
//! entry: not the administrators group, not `Users`, not `Everyone`.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW, SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

/// The text of a SID (`S-1-5-21-…`).
///
/// # Safety
/// `sid` must point to a valid SID.
unsafe fn sid_string(sid: PSID) -> io::Result<String> {
    let mut text: PWSTR = null_mut();
    // SAFETY: `sid` is valid (the caller); the text the call allocates is read up to its end and freed
    unsafe {
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(s)
    }
}

fn query_user_sid() -> io::Result<String> {
    let mut token: HANDLE = null_mut();
    // SAFETY: the calls get valid pointers; the token is closed again and the buffer outlives the
    // `TOKEN_USER` read from it (a `u64` buffer is aligned for it)
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let result = if GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) == 0 { Err(io::Error::last_os_error()) } else { sid_string((*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid) };
        CloseHandle(token);
        result
    }
}

/// The SID of the user this process runs as (the one of the token, so also for an elevated
/// process of an administrator, whose files are owned by the group instead).
pub fn user_sid() -> io::Result<&'static str> {
    static SID: OnceLock<Option<String>> = OnceLock::new();
    SID.get_or_init(|| query_user_sid().ok()).as_deref().ok_or_else(|| io::Error::other("cannot read the SID of the current user"))
}

/// Has the access list of `path` the "protected" flag (no entries taken from the parent folder)?
fn is_protected(path: &Path) -> bool {
    let name = wide(path);
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: valid pointers; the descriptor the call allocates is freed
    unsafe {
        if GetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, null_mut(), null_mut(), null_mut(), null_mut(), &mut sd) != ERROR_SUCCESS {
            return false;
        }
        let (mut control, mut revision) = (0u16, 0u32);
        let protected = GetSecurityDescriptorControl(sd, &mut control, &mut revision) != 0 && control & SE_DACL_PROTECTED != 0;
        LocalFree(sd);
        protected
    }
}

/// Make the access list of `path` owner-only (see the module comment). A folder (`container`)
/// passes the list on to what is made in it. One that has the list already is left alone, and so
/// is one that has something in it: setting a list rewrites the list of everything inside (the
/// installed TeX Live in Shale's data folder has a hundred thousand files), and such a folder is
/// in the user's profile, which only the user, `SYSTEM` and the administrators can enter. A file
/// system without access lists (FAT) has nothing to protect, which is not an error.
pub fn restrict_to_owner(path: &Path, container: bool) -> io::Result<()> {
    if container && (is_protected(path) || std::fs::read_dir(path)?.next().is_some()) {
        return Ok(());
    }
    let inherit = if container { "OICI" } else { "" };
    let sddl = wide(format!("D:P(A;{inherit};FA;;;{})(A;{inherit};FA;;;SY)", user_sid()?));
    let name = wide(path);
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: valid pointers; the descriptor made from the text is freed, and the list read from it
    // is only used while it lives
    let code = unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, null_mut()) == 0 {
            return Err(io::Error::last_os_error());
        }
        let (mut present, mut defaulted, mut dacl) = (0, 0, null_mut());
        if GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) == 0 || present == 0 {
            LocalFree(sd);
            return Err(io::Error::other("the access list of owner-only files could not be made"));
        }
        let code = SetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION, null_mut(), null_mut(), dacl, null());
        LocalFree(sd);
        code
    };
    match code {
        ERROR_SUCCESS | ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION => Ok(()),
        e => Err(io::Error::from_raw_os_error(e as i32)),
    }
}

/// Whether the access list of `path` is protected, and each of its entries as (SID, ACE flags):
/// what the tests compare with the user and `SYSTEM`.
#[cfg(test)]
pub(super) fn entries(path: &Path) -> (bool, Vec<(String, u8)>) {
    use windows_sys::Win32::Security::{AclSizeInformation, GetAce, GetAclInformation, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION};
    let name = wide(path);
    // SAFETY: valid pointers; the entries are read while the descriptor lives
    unsafe {
        let (mut dacl, mut sd): (*mut ACL, PSECURITY_DESCRIPTOR) = (null_mut(), null_mut());
        assert_eq!(GetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, null_mut(), null_mut(), &mut dacl, null_mut(), &mut sd), ERROR_SUCCESS);
        let (mut control, mut revision) = (0u16, 0u32);
        assert_ne!(GetSecurityDescriptorControl(sd, &mut control, &mut revision), 0);
        let mut info: ACL_SIZE_INFORMATION = std::mem::zeroed();
        assert_ne!(GetAclInformation(dacl, (&mut info as *mut ACL_SIZE_INFORMATION).cast(), std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32, AclSizeInformation), 0);
        let mut list = vec![];
        for i in 0..info.AceCount {
            let mut ace = null_mut();
            assert_ne!(GetAce(dacl, i, &mut ace), 0);
            let header = &*ace.cast::<ACE_HEADER>();
            assert_eq!(header.AceType, 0, "only entries that allow");
            let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
            list.push((sid_string((&raw mut (*allowed).SidStart).cast()).unwrap(), header.AceFlags));
        }
        LocalFree(sd);
        (control & SE_DACL_PROTECTED != 0, list)
    }
}

/// Check that `path` has the user and `SYSTEM` as its only entries; with `own` its list is
/// protected (set by [`restrict_to_owner`]), else it is inherited from a folder that has one. Gives
/// the ACE flags of the entries.
#[cfg(test)]
pub(super) fn assert_only_user_and_system(path: &Path, own: bool) -> Vec<u8> {
    const INHERITED: u32 = windows_sys::Win32::Security::INHERITED_ACE;
    let (protected, list) = entries(path);
    assert_eq!(protected, own, "{}: protected access list", path.display());
    let mut sids: Vec<&str> = list.iter().map(|(s, _)| s.as_str()).collect();
    sids.sort_unstable();
    let mut want = vec![user_sid().unwrap(), "S-1-5-18"];
    want.sort_unstable();
    assert_eq!(sids, want, "{}: the user and SYSTEM, nobody else", path.display());
    assert!(list.iter().all(|(_, flags)| (u32::from(*flags) & INHERITED != 0) != own), "{}: entries inherited or not", path.display());
    list.iter().map(|(_, f)| *f).collect()
}

/// A file or folder that [`restrict_to_owner`] was applied to.
#[cfg(test)]
pub(super) fn assert_owner_only(path: &Path) -> Vec<u8> {
    assert_only_user_and_system(path, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::{CONTAINER_INHERIT_ACE, OBJECT_INHERIT_ACE};

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("shale-winacl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: DATA-014
    #[test]
    fn data_014_a_file_gets_a_protected_list_for_the_user_and_system_without_inheritance_flags() {
        let d = dir("file");
        let f = d.join("settings.json");
        std::fs::write(&f, "x").unwrap();
        restrict_to_owner(&f, false).unwrap();
        assert!(assert_owner_only(&f).iter().all(|flags| *flags == 0));
        assert_eq!(std::fs::read(&f).unwrap(), b"x", "the owner still reads and writes it");
        std::fs::write(&f, "y").unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: DATA-014
    #[test]
    fn data_014_a_folder_passes_the_list_on_and_a_file_made_in_it_has_only_the_two_entries() {
        let d = dir("folder");
        let sub = d.join("private");
        std::fs::create_dir(&sub).unwrap();
        restrict_to_owner(&sub, true).unwrap();
        assert!(assert_owner_only(&sub).iter().all(|flags| u32::from(*flags) & (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) == OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE));
        assert!(is_protected(&sub));
        restrict_to_owner(&sub, true).unwrap();
        std::fs::write(sub.join("inner.txt"), "x").unwrap();
        assert_only_user_and_system(&sub.join("inner.txt"), false);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: DATA-014
    #[test]
    fn data_014_a_folder_with_something_in_it_keeps_its_list_so_nothing_is_walked_over() {
        let d = dir("full");
        std::fs::write(d.join("a.txt"), "x").unwrap();
        restrict_to_owner(&d, true).unwrap();
        assert!(!is_protected(&d), "a folder that has files is not rewritten");
        assert!(!is_protected(&d.join("a.txt")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: DATA-014
    #[test]
    fn data_014_a_folder_that_was_open_to_others_is_closed_and_the_user_sid_is_known() {
        let d = dir("open");
        let (protected, before) = entries(&d);
        // the temp folder of the user: a list of its own place, passed on, that is not protected
        assert!(!protected && !before.is_empty());
        restrict_to_owner(&d, true).unwrap();
        assert_owner_only(&d);
        assert!(user_sid().unwrap().starts_with("S-1-"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
