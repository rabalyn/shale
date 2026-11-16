//! How a keyring entry (app, server, user) is named in the Keychain and the Credential Manager.
//! The names are unambiguous: every part that may contain any character is preceded by its
//! length, so no two different triples give the same name and apps never see each other's entries.

/// The Keychain "service" of an app: all Shale entries share the prefix, so they sort together.
#[cfg(any(target_os = "macos", test))]
pub fn service(app: &str) -> String {
    format!("Shale {app}")
}

/// The Keychain "account" (and the tail of the Credential Manager target) of a server + user.
pub fn account(server: &str, user: &str) -> String {
    format!("{}:{server}:{user}", server.len())
}

/// The Credential Manager target name of (app, server, user).
#[cfg(any(windows, test))]
pub fn target(app: &str, server: &str, user: &str) -> String {
    format!("shale:{}:{app}:{}", app.len(), account(server, user))
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: SEC-024
    #[test]
    fn keyring_item_names_never_collide() {
        let triples = [
            ("shale-caldav", "https://a:443", "bob"),
            ("shale-caldav", "https://a", "443:bob"),
            ("shale-mail", "https://a:443", "bob"),
            ("shale-caldav", "https://a:443:bob", ""),
            ("shale-caldav", "", "https://a:443:bob"),
            ("shale-cal", "dav:1:x", "y"),
            ("shale-caldav", "1:x", "y"),
            ("shale-caldav", "ü:ö", "日本:x"),
        ];
        let mut targets = std::collections::HashSet::new();
        let mut accounts = std::collections::HashSet::new();
        for (a, s, u) in triples {
            assert!(targets.insert(target(a, s, u)), "target of {a:?} {s:?} {u:?} is not unique");
            assert!(accounts.insert((service(a), account(s, u))), "service + account of {a:?} {s:?} {u:?} is not unique");
        }
    }
}
