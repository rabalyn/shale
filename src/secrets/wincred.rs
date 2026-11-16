//! The Windows Credential Manager: one generic credential per (app, server, user), kept for this
//! user on this computer. The target name is `shale:<len>:<app>:<len>:<server>:<user>`
//! (`names.rs`), the credential's user name field holds the user. The password is the credential
//! blob (UTF-8). Windows allows at most `CRED_MAX_CREDENTIAL_BLOB_SIZE` (2560) bytes: a longer
//! secret makes `store` false (SEC-011 keeps it for this run).

use super::names;
use crate::secret::Secret;
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::Security::Credentials::{CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};

/// Always there on Windows.
pub fn available() -> bool {
    true
}

/// `s` as a NUL-terminated UTF-16 string; None when it holds a NUL itself (Windows would cut it short).
fn wide(s: &str) -> Option<Vec<u16>> {
    if s.contains('\0') {
        return None;
    }
    Some(s.encode_utf16().chain(std::iter::once(0)).collect())
}

pub fn lookup(app: &str, server: &str, user: &str) -> Option<Secret> {
    let target = wide(&names::target(app, server, user))?;
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `target` is a NUL-terminated UTF-16 string that lives through the call; `cred` is a
    // valid out pointer; on success Windows fills it with a buffer that CredFree releases below.
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) } == 0 || cred.is_null() {
        return None;
    }
    // SAFETY: CredReadW succeeded, so `cred` points to a valid CREDENTIALW until CredFree. A blob
    // of `CredentialBlobSize` bytes starts at `CredentialBlob` (or the size is 0). We hold the only
    // reference to this buffer, and `cred` is not used after CredFree.
    unsafe {
        let c = &*cred;
        let blob: &mut [u8] = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 { &mut [] } else { std::slice::from_raw_parts_mut(c.CredentialBlob, c.CredentialBlobSize as usize) };
        let secret = if blob.is_empty() { None } else { std::str::from_utf8(blob).ok().map(Secret::new) };
        // the credential buffer is Windows' own: wipe the password before giving it back
        zeroize::Zeroize::zeroize(blob);
        CredFree(cred as *const _);
        secret
    }
}

pub fn store(app: &str, server: &str, user: &str, password: &str) -> bool {
    let (Some(mut target), Some(mut user_name)) = (wide(&names::target(app, server, user)), wide(user)) else { return false };
    let Ok(size) = u32::try_from(password.len()) else { return false };
    let cred = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        Comment: std::ptr::null_mut(),
        LastWritten: FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 },
        CredentialBlobSize: size,
        // CredWriteW only reads the blob: the borrowed bytes of the caller's Secret are used as they are
        CredentialBlob: password.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: std::ptr::null_mut(),
        UserName: user_name.as_mut_ptr(),
    };
    // SAFETY: `cred` points to live, NUL-terminated strings (`target`, `user_name`) and a blob of
    // `size` bytes (`password`), all valid for the call; CredWriteW copies them and does not write to them.
    let written = unsafe { CredWriteW(&cred, 0) } != 0;
    written && lookup(app, server, user).is_some_and(|s| s.expose() == password)
}

pub fn clear(app: &str, server: &str, user: &str) {
    let Some(target) = wide(&names::target(app, server, user)) else { return };
    // SAFETY: `target` is a NUL-terminated UTF-16 string that lives through the call. A missing entry is not an error here.
    unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique app name for this test run, so it never touches a real Shale entry.
    fn test_app(tag: &str) -> String {
        format!("shale-test-{tag}-{}", std::process::id())
    }

    /// Removes the entries of a test even when an assert fails.
    struct Cleanup(Vec<(String, String, String)>);
    impl Cleanup {
        fn new(app: &str, items: &[(&str, &str)]) -> Self {
            Cleanup(items.iter().map(|(s, u)| (app.to_string(), s.to_string(), u.to_string())).collect())
        }
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for (a, s, u) in &self.0 {
                clear(a, s, u);
            }
        }
    }

    // spec: SEC-025
    #[test]
    fn credential_manager_stores_overwrites_looks_up_and_clears() {
        let app = test_app("roundtrip");
        let _g = Cleanup::new(&app, &[("https://c.example:443", "ann")]);
        assert!(available());
        assert!(lookup(&app, "https://c.example:443", "ann").is_none());
        assert!(store(&app, "https://c.example:443", "ann", "first"));
        assert_eq!(lookup(&app, "https://c.example:443", "ann").unwrap().expose(), "first");
        assert!(store(&app, "https://c.example:443", "ann", "second"));
        assert_eq!(lookup(&app, "https://c.example:443", "ann").unwrap().expose(), "second");
        clear(&app, "https://c.example:443", "ann");
        assert!(lookup(&app, "https://c.example:443", "ann").is_none());
    }

    // spec: SEC-025
    #[test]
    fn credential_manager_entries_of_different_app_server_user_do_not_mix() {
        let (a, b) = (test_app("a"), test_app("b"));
        let _ga = Cleanup::new(&a, &[("s1", "u"), ("s2", "u"), ("s1", "v")]);
        let _gb = Cleanup::new(&b, &[("s1", "u")]);
        assert!(store(&a, "s1", "u", "one"));
        assert!(lookup(&a, "s2", "u").is_none());
        assert!(lookup(&a, "s1", "v").is_none());
        assert!(lookup(&b, "s1", "u").is_none());
        assert!(store(&b, "s1", "u", "other"));
        assert!(store(&a, "s2", "u", "two"));
        assert_eq!(lookup(&a, "s1", "u").unwrap().expose(), "one");
        assert_eq!(lookup(&a, "s2", "u").unwrap().expose(), "two");
        assert_eq!(lookup(&b, "s1", "u").unwrap().expose(), "other");
    }

    // spec: SEC-025
    #[test]
    fn credential_manager_round_trips_colons_and_unicode() {
        let app = test_app("unicode");
        let items = [("https://ü.example:8443", "名前:with:colons"), ("https://ü.example", "8443:名前:with:colons")];
        let _g = Cleanup::new(&app, &items);
        for (i, (s, u)) in items.iter().enumerate() {
            assert!(store(&app, s, u, &format!("pässwörd:日本:{i}")));
        }
        for (i, (s, u)) in items.iter().enumerate() {
            assert_eq!(lookup(&app, s, u).unwrap().expose(), format!("pässwörd:日本:{i}"));
        }
    }

    // spec: SEC-025
    #[test]
    fn credential_manager_refuses_what_it_cannot_hold() {
        let app = test_app("limits");
        let _g = Cleanup::new(&app, &[("s", "u")]);
        assert!(!store(&app, "s", "u", &"x".repeat(3000)), "more than CRED_MAX_CREDENTIAL_BLOB_SIZE bytes");
        assert!(!store(&app, "s\0", "u", "x"));
        assert!(lookup(&app, "s", "u").is_none());
    }
}
