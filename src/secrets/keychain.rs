//! The macOS Keychain: one generic password per (app, server, user) in the default keychain.
//! The Keychain's "service" is `Shale <app>`, its "account" `<len>:<server>:<user>` (`names.rs`).
//! A cancelled or refused unlock prompt, or a locked keychain without a screen: `store` is false
//! and `lookup` finds nothing (SEC-011 keeps the secret for this run).

use super::names;
use crate::secret::Secret;
use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
use security_framework_sys::base::errSecItemNotFound;
use zeroize::Zeroizing;

/// Is the default keychain reachable? Asks for an item that does not exist: "not found" means the
/// store works, anything else (no default keychain, no user interaction possible) means it does not.
pub fn available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| match get_generic_password(&names::service("probe"), "probe") {
        Ok(found) => {
            // nobody put it there, but never leave a copy around
            drop(Zeroizing::new(found));
            true
        }
        Err(e) => e.code() == errSecItemNotFound,
    })
}

pub fn lookup(app: &str, server: &str, user: &str) -> Option<Secret> {
    // the Vec the framework hands out is zeroed when dropped; the Secret is made straight from it
    let bytes = Zeroizing::new(get_generic_password(&names::service(app), &names::account(server, user)).ok()?);
    if bytes.is_empty() {
        return None;
    }
    std::str::from_utf8(&bytes).ok().map(Secret::new)
}

pub fn store(app: &str, server: &str, user: &str, password: &str) -> bool {
    if !available() {
        return false;
    }
    // creates the item or updates it; an Err covers a cancelled unlock prompt
    set_generic_password(&names::service(app), &names::account(server, user), password.as_bytes()).is_ok() && lookup(app, server, user).is_some_and(|s| s.expose() == password)
}

pub fn clear(app: &str, server: &str, user: &str) {
    let _ = delete_generic_password(&names::service(app), &names::account(server, user));
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

    /// True when the real Keychain can be used. On CI (`CI` set) it must be there; elsewhere the
    /// test is skipped with a note.
    fn usable() -> bool {
        if available() {
            return true;
        }
        assert!(std::env::var_os("CI").is_none(), "the Keychain is not reachable on CI");
        eprintln!("skipping: the default keychain is not reachable");
        false
    }

    // spec: SEC-024
    #[test]
    fn keychain_stores_overwrites_looks_up_and_clears() {
        if !usable() {
            return;
        }
        let app = test_app("roundtrip");
        let _g = Cleanup::new(&app, &[("https://c.example:443", "ann")]);
        assert!(lookup(&app, "https://c.example:443", "ann").is_none());
        assert!(store(&app, "https://c.example:443", "ann", "first"));
        assert_eq!(lookup(&app, "https://c.example:443", "ann").unwrap().expose(), "first");
        assert!(store(&app, "https://c.example:443", "ann", "second"));
        assert_eq!(lookup(&app, "https://c.example:443", "ann").unwrap().expose(), "second");
        clear(&app, "https://c.example:443", "ann");
        assert!(lookup(&app, "https://c.example:443", "ann").is_none());
    }

    // spec: SEC-024
    #[test]
    fn keychain_entries_of_different_app_server_user_do_not_mix() {
        if !usable() {
            return;
        }
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

    // spec: SEC-024
    #[test]
    fn keychain_round_trips_colons_and_unicode() {
        if !usable() {
            return;
        }
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
}
