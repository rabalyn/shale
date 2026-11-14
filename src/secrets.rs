//! CalDAV and mail account secrets in the system keyring instead of the settings file.
//!
//! On Linux the Secret Service (GNOME Keyring, KWallet, KeePassXC …) is used through the
//! `secret-tool` program; the password goes to it on standard input, never on the command line.
//! One entry per account (server + user name), shared by all calendars of that account.
//! Where no keyring is available the password stays in the settings file, which only its
//! owner can read.
//!
//! Mail accounts (`mail_secret`, `set_mail_secret`, `clear_mail_secret`) keep one entry per
//! account id and kind (password, later an OAuth2 refresh token). They never go to the settings
//! file: without a keyring the caller has to ask again.
//!
//! In memory, `SourceKind::Caldav::password` always holds the real password: `load_passwords`
//! fills it in after reading the settings, `for_file` takes it out again before writing them.

use crate::cal_dav;
use crate::cal_store::{CalendarSource, SourceKind};
use std::collections::HashMap;
use std::sync::Mutex;

/// The account a calendar belongs to: (server, user name). None without a user name.
fn account_of(s: &CalendarSource) -> Option<(String, String)> {
    let SourceKind::Caldav { url, username, .. } = &s.kind else { return None };
    if username.is_empty() {
        return None;
    }
    let o = cal_dav::Origin::parse(url).ok()?;
    Some((format!("{}://{}:{}", o.scheme, o.host, o.port), username.clone()))
}

#[allow(dead_code)] // wired up by the mail sync and account settings (phase 1b)
/// Keyring application name of CalDAV passwords.
const CALDAV: &str = "shale-caldav";
/// Keyring application name of mail account secrets.
const MAIL: &str = "shale-mail";

#[allow(dead_code)] // wired up by the mail sync and account settings (phase 1b)
/// What a mail account secret is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MailSecret {
    Password,
    /// For the OAuth2 sign-in (phase 1c).
    RefreshToken,
}

#[allow(dead_code)] // wired up by the mail sync and account settings (phase 1b)
impl MailSecret {
    fn name(self) -> &'static str {
        match self {
            MailSecret::Password => "password",
            MailSecret::RefreshToken => "refresh-token",
        }
    }
}

#[allow(dead_code)] // wired up by the mail sync and account settings (phase 1b)
/// The secret of a mail account, if the keyring has it.
pub fn mail_secret(account_id: &str, kind: MailSecret) -> Option<String> {
    backend::lookup(MAIL, account_id, kind.name())
}

#[allow(dead_code)] // wired up by the mail sync and account settings (phase 1b)
/// Put a mail account secret into the keyring. False if that did not work (no keyring, or the
/// unlock was refused): the secret is then not saved anywhere.
pub fn set_mail_secret(account_id: &str, kind: MailSecret, secret: &str) -> bool {
    backend::store(MAIL, account_id, kind.name(), secret)
}

#[allow(dead_code)] // wired up by the mail sync and account settings (phase 1b)
/// Remove a mail account secret (the account was deleted).
pub fn clear_mail_secret(account_id: &str, kind: MailSecret) {
    backend::clear(MAIL, account_id, kind.name());
}

/// Passwords this run knows to be in the keyring (read from or written to it).
static IN_KEYRING: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);
/// Accounts whose password the keyring refused this run (e.g. the unlock prompt was cancelled):
/// not asked again until the password changes, so saving the settings doesn't keep prompting.
static REFUSED: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);

fn known() -> std::sync::MutexGuard<'static, Option<HashMap<(String, String), String>>> {
    IN_KEYRING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Fill in the passwords of calendars whose settings entry has none (they are in the keyring).
pub fn load_passwords(sources: &mut [CalendarSource]) {
    let mut cache: HashMap<(String, String), Option<String>> = HashMap::new();
    for s in sources.iter_mut() {
        let Some(acct) = account_of(s) else { continue };
        let SourceKind::Caldav { password, .. } = &mut s.kind else { continue };
        if !password.is_empty() {
            continue;
        }
        let found = cache.entry(acct.clone()).or_insert_with(|| backend::lookup(CALDAV, &acct.0, &acct.1)).clone();
        if let Some(p) = found {
            known().get_or_insert_with(HashMap::new).insert(acct, p.clone());
            *password = p;
        }
    }
}

/// The calendars as they go into the settings file: passwords moved to the keyring where that
/// works (otherwise kept in the file). Keyring entries of accounts that are no longer used are
/// removed.
pub fn for_file(sources: &[CalendarSource]) -> Vec<CalendarSource> {
    let mut out = sources.to_vec();
    let mut used = std::collections::HashSet::new();
    let mut k = known();
    let map = k.get_or_insert_with(HashMap::new);
    for s in out.iter_mut() {
        let Some(acct) = account_of(s) else { continue };
        used.insert(acct.clone());
        let SourceKind::Caldav { password, .. } = &mut s.kind else { continue };
        if password.is_empty() {
            continue;
        }
        let mut refused = REFUSED.lock().unwrap_or_else(|e| e.into_inner());
        let refused = refused.get_or_insert_with(HashMap::new);
        let stored = map.get(&acct) == Some(password)
            || (refused.get(&acct) != Some(password) && {
                let ok = backend::store(CALDAV, &acct.0, &acct.1, password);
                if ok {
                    map.insert(acct.clone(), password.clone());
                    refused.remove(&acct);
                } else {
                    refused.insert(acct.clone(), password.clone());
                }
                ok
            });
        if stored {
            password.clear();
        }
    }
    let unused: Vec<(String, String)> = map.keys().filter(|a| !used.contains(*a)).cloned().collect();
    for a in unused {
        backend::clear(CALDAV, &a.0, &a.1);
        map.remove(&a);
    }
    out
}

/// Where passwords end up, for the settings dialog.
pub fn where_stored() -> &'static str {
    if backend::available() {
        "Passwords are kept in the system keyring."
    } else {
        "No system keyring found (install secret-tool / libsecret to use one): passwords are stored in the settings file, readable only by you."
    }
}

#[cfg(all(target_os = "linux", not(test)))]
mod backend {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn attrs<'a>(app: &'a str, server: &'a str, user: &'a str) -> [&'a str; 6] {
        ["application", app, "server", server, "user", user]
    }

    /// Run secret-tool with `input` on stdin. Gives up after a while (an unlock prompt left open).
    fn run(args: &[&str], input: Option<&str>) -> Option<(bool, String)> {
        let mut child = Command::new("secret-tool").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        {
            let mut stdin = child.stdin.take()?;
            if let Some(i) = input {
                let _ = stdin.write_all(i.as_bytes());
            }
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        };
        let mut out = String::new();
        let _ = child.stdout.take()?.read_to_string(&mut out);
        Some((status.success(), out))
    }

    pub fn available() -> bool {
        static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            // a search that finds nothing still succeeds, but only with a running Secret Service
            Command::new("secret-tool").args(["search", "application", "shale-caldav-probe"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
        })
    }

    pub fn lookup(app: &str, server: &str, user: &str) -> Option<String> {
        let mut args = vec!["lookup"];
        args.extend(attrs(app, server, user));
        match run(&args, None) {
            Some((true, p)) if !p.is_empty() => Some(p),
            _ => None,
        }
    }

    pub fn store(app: &str, server: &str, user: &str, password: &str) -> bool {
        if !available() {
            return false;
        }
        let label = format!("{}: {user} on {server}", if app == "shale-caldav" { "Shale CalDAV" } else { "Shale mail" });
        let mut args = vec!["store", "--label", label.as_str()];
        args.extend(attrs(app, server, user));
        // the stored password must read back exactly (no trailing newline is added)
        matches!(run(&args, Some(password)), Some((true, _))) && lookup(app, server, user).as_deref() == Some(password)
    }

    pub fn clear(app: &str, server: &str, user: &str) {
        let mut args = vec!["clear"];
        args.extend(attrs(app, server, user));
        let _ = run(&args, None);
    }
}

/// Tests: a pretend keyring in memory (never the real one).
#[cfg(test)]
mod backend {
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub static FAKE: Mutex<Option<HashMap<(String, String, String), String>>> = Mutex::new(None);

    fn key(app: &str, server: &str, user: &str) -> (String, String, String) {
        (app.to_string(), server.to_string(), user.to_string())
    }

    pub fn available() -> bool {
        true
    }
    pub fn lookup(app: &str, server: &str, user: &str) -> Option<String> {
        FAKE.lock().unwrap().as_ref()?.get(&key(app, server, user)).cloned()
    }
    pub fn store(app: &str, server: &str, user: &str, password: &str) -> bool {
        FAKE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key(app, server, user), password.into());
        true
    }
    pub fn clear(app: &str, server: &str, user: &str) {
        if let Some(m) = FAKE.lock().unwrap().as_mut() {
            m.remove(&key(app, server, user));
        }
    }
}

/// Other systems: no keyring support yet, passwords stay in the owner-only settings file.
#[cfg(all(not(target_os = "linux"), not(test)))]
mod backend {
    pub fn available() -> bool {
        false
    }
    pub fn lookup(_: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    pub fn store(_: &str, _: &str, _: &str, _: &str) -> bool {
        false
    }
    pub fn clear(_: &str, _: &str, _: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dav(id: &str, url: &str, user: &str, pw: &str) -> CalendarSource {
        CalendarSource { id: id.into(), name: id.into(), color: "#000000".into(), enabled: true, sync_minutes: 15, kind: SourceKind::Caldav { url: url.into(), username: user.into(), password: pw.into() } }
    }

    // spec: SEC-003
    #[test]
    fn passwords_go_to_the_keyring_and_come_back() {
        let sources = vec![dav("a", "https://dav.example.com/cal/a/", "ada", "s3cret"), dav("b", "https://DAV.example.com:443/cal/b/", "ada", "s3cret"), dav("anon", "https://pub.example.com/c/", "", "")];
        let file = for_file(&sources);
        let json = serde_json::to_string(&file).unwrap();
        assert!(!json.contains("s3cret"), "no password in the settings file: {json}");
        assert_eq!(backend::lookup(CALDAV, "https://dav.example.com:443", "ada").as_deref(), Some("s3cret"), "one entry for the account");
        // reading the settings back
        let mut loaded: Vec<CalendarSource> = serde_json::from_str(&json).unwrap();
        load_passwords(&mut loaded);
        assert_eq!(loaded, sources);
        // the account is removed: its keyring entry goes too
        let _ = for_file(&sources[2..]);
        assert_eq!(backend::lookup(CALDAV, "https://dav.example.com:443", "ada"), None);
    }

    // spec: MAIL-021
    #[test]
    fn mail_secrets_have_their_own_entries() {
        assert!(set_mail_secret("acct-1", MailSecret::Password, "pw-1"));
        assert!(set_mail_secret("acct-1", MailSecret::RefreshToken, "tok-1"));
        assert_eq!(mail_secret("acct-1", MailSecret::Password).as_deref(), Some("pw-1"));
        assert_eq!(mail_secret("acct-1", MailSecret::RefreshToken).as_deref(), Some("tok-1"));
        assert_eq!(mail_secret("acct-2", MailSecret::Password), None);
        // a CalDAV entry of the same names is a different entry
        assert_eq!(backend::lookup(CALDAV, "acct-1", "password"), None);
        clear_mail_secret("acct-1", MailSecret::Password);
        assert_eq!(mail_secret("acct-1", MailSecret::Password), None);
        assert_eq!(mail_secret("acct-1", MailSecret::RefreshToken).as_deref(), Some("tok-1"));
        clear_mail_secret("acct-1", MailSecret::RefreshToken);
    }
}
