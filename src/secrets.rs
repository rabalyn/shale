//! CalDAV and mail account secrets in the system keyring instead of the settings file.
//!
//! On Linux the Secret Service (GNOME Keyring, KWallet, KeePassXC …) is used through the
//! `secret-tool` program; the password goes to it on standard input, never on the command line.
//! One entry per account (server + user name), shared by all calendars of that account.
//! Where no keyring is available at all the password stays in the settings file, which only its
//! owner can read. Where there is a keyring that refuses (the unlock prompt was cancelled) the
//! password is never written to the file: it is kept in memory for this run and the person is
//! told once (`take_notices`).
//!
//! Mail accounts (`mail_secret`, `set_mail_secret`, `clear_mail_secret`) keep one entry per
//! account id and kind (password, later an OAuth2 refresh token). They never go to the settings
//! file: without a keyring the caller has to ask again.
//!
//! In memory, `SourceKind::Caldav::password` always holds the real password: `load_passwords`
//! fills it in after reading the settings, `for_file` takes it out again before writing them.

use crate::cal_dav;
use crate::cal_store::{CalendarSource, SourceKind};
use eframe::egui;
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

/// Keyring application name of CalDAV passwords.
const CALDAV: &str = "shale-caldav";
/// Keyring application name of mail account secrets.
const MAIL: &str = "shale-mail";
/// Keyring application name of social account secrets: calendars, mail and social accounts never see each other's entries.
const SOCIAL: &str = "shale-social";

/// What a mail account secret is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MailSecret {
    Password,
    /// The OAuth2 refresh token (`oauth.rs`).
    RefreshToken,
    /// The OAuth2 client secret, for providers that want one from desktop apps.
    ClientSecret,
}

impl MailSecret {
    fn name(self) -> &'static str {
        match self {
            MailSecret::Password => "password",
            MailSecret::RefreshToken => "refresh-token",
            MailSecret::ClientSecret => "client-secret",
        }
    }
}

/// The secret of a mail account, if the keyring has it.
pub fn mail_secret(account_id: &str, kind: MailSecret) -> Option<String> {
    backend::lookup(MAIL, account_id, kind.name())
}

/// Put a mail account secret into the keyring. False if that did not work (no keyring, or the
/// unlock was refused): the secret is then not saved anywhere.
pub fn set_mail_secret(account_id: &str, kind: MailSecret, secret: &str) -> bool {
    backend::store(MAIL, account_id, kind.name(), secret)
}

/// Remove a mail account secret (the account was deleted).
pub fn clear_mail_secret(account_id: &str, kind: MailSecret) {
    backend::clear(MAIL, account_id, kind.name());
}

/// Passwords this run knows to be in the keyring (read from or written to it).
static IN_KEYRING: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);
/// Accounts whose password the keyring refused this run (e.g. the unlock prompt was cancelled):
/// not asked again until the password changes, so saving the settings doesn't keep prompting.
static REFUSED: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);

/// Passwords that came out of the settings file of an earlier version and are still only there.
/// If the keyring refuses them they must stay in the file: it is the only copy.
static IN_FILE: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);
/// Messages for the person, shown by the app as toasts (taken with `take_notices`).
static NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Secrets of social accounts the keyring refused: kept for this run only.
static SOCIAL_SESSION: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);

/// Every notice of this test run, never taken: headless app tests running at the same time take
/// `NOTICES` in their frames, so the secrets tests read this copy.
#[cfg(test)]
static NOTICE_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn notice(msg: String) {
    #[cfg(test)]
    NOTICE_LOG.lock().unwrap_or_else(|e| e.into_inner()).push(msg.clone());
    NOTICES.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
}

/// What the person has to be told about secrets that could not be saved (each only once).
pub fn take_notices() -> Vec<String> {
    std::mem::take(&mut *NOTICES.lock().unwrap_or_else(|e| e.into_inner()))
}

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
            // from the file of an earlier version: it stays there until a keyring has taken it
            IN_FILE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(acct, password.clone());
            continue;
        }
        let found = cache.entry(acct.clone()).or_insert_with(|| backend::lookup(CALDAV, &acct.0, &acct.1)).clone();
        if let Some(p) = found {
            known().get_or_insert_with(HashMap::new).insert(acct, p.clone());
            *password = p;
        }
    }
}

/// The calendars as they go into the settings file. Keyring entries of accounts that are no
/// longer used are removed.
///
/// What happens to a password in memory (SEC-011):
///
/// | keyring                | password                  | settings file | memory | the person is told         |
/// |------------------------|---------------------------|---------------|--------|----------------------------|
/// | none on this system    | any                       | has it        | has it | no (documented, owner-only)|
/// | takes it               | any (also from the file)  | empty         | has it | no                         |
/// | refuses (unlock denied)| new or changed this run   | empty         | has it | once: not saved, asked again |
/// | refuses (unlock denied)| from the file, unchanged  | keeps it      | has it | no (nothing was lost)      |
///
/// A password is never dropped when it exists nowhere else: a refused password that was already
/// in the file stays there; one that was never saved stays in memory for this run.
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
        let mut in_file = IN_FILE.lock().unwrap_or_else(|e| e.into_inner());
        let in_file = in_file.get_or_insert_with(HashMap::new);
        let stored = map.get(&acct) == Some(password)
            || (refused.get(&acct) != Some(password) && {
                let ok = backend::store(CALDAV, &acct.0, &acct.1, password);
                if ok {
                    map.insert(acct.clone(), password.clone());
                    refused.remove(&acct);
                } else if backend::available() {
                    refused.insert(acct.clone(), password.clone());
                    if in_file.get(&acct) != Some(password) {
                        notice(format!("The password of the calendar account “{}” could not be saved in the system keyring, so it was not saved at all. You will be asked for it again the next time Shale starts.", acct.1));
                    }
                }
                ok
            });
        if stored {
            in_file.remove(&acct);
            password.clear();
        } else if backend::available() && in_file.get(&acct) != Some(password) {
            // the keyring is there but refused: never write it to the file
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

/// Is there a keyring to keep mail passwords in? (Without one they are asked for per session.)
pub fn keyring_available() -> bool {
    backend::available()
}

// ---------------------------------------------------------------- social accounts (Mastodon / Bluesky)

/// The access token (Mastodon) or app password (Bluesky) of a social account, from the keyring.
/// `server` is the account's server address as stored in `social::Account::server`.
pub fn social_lookup(server: &str, handle: &str) -> Option<String> {
    backend::lookup(SOCIAL, server, handle)
}

/// Put the secret of a social account into the keyring. False when no keyring works (the caller
/// then keeps it in the owner-only settings file, see `social_store_or_keep`).
pub fn social_store(server: &str, handle: &str, secret: &str) -> bool {
    backend::store(SOCIAL, server, handle, secret)
}

/// Remove the secret of a social account from the keyring (when the account is removed).
pub fn social_clear(server: &str, handle: &str) {
    backend::clear(SOCIAL, server, handle)
}

/// What goes into the settings file for the secret: empty when the keyring has it now; the
/// secret itself (the file is readable only by its owner) when there is no keyring at all; empty
/// again when there is a keyring that refused it: the secret is then kept for this run only and
/// the person is told once (SEC-011).
pub fn social_store_or_keep(server: &str, handle: &str, secret: &str) -> String {
    if social_store(server, handle, secret) {
        return String::new();
    }
    if !backend::available() {
        return secret.to_string();
    }
    SOCIAL_SESSION.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert((server.to_string(), handle.to_string()), secret.to_string());
    notice(format!("The sign-in of “{handle}” could not be saved in the system keyring, so it was not saved at all. You will have to sign in again the next time Shale starts."));
    String::new()
}

/// Move a secret that an earlier version left in the settings file to the keyring. It leaves the
/// file only when the keyring has it (read back); otherwise it stays, as it is the only copy.
pub fn social_move_to_keyring(server: &str, handle: &str, in_file: &mut String) {
    if !in_file.is_empty() && backend::available() && social_store(server, handle, in_file) {
        in_file.clear();
    }
}

/// The secret of a social account: from the keyring, else the one kept for this run, else the one
/// in the settings file.
pub fn social_secret(server: &str, handle: &str, from_file: &str) -> String {
    social_lookup(server, handle).or_else(|| SOCIAL_SESSION.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&(server.to_string(), handle.to_string())).cloned())).unwrap_or_else(|| from_file.to_string())
}

// ---------------------------------------------------------------- password fields and the key recording

thread_local! {
    /// Frames left in which a password field counts as being on screen (SEC-009). The window's
    /// drawing and its input hook run on the same thread.
    static SECRET_ON_SCREEN: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}
/// A field that was on screen a moment ago still counts: the last keys can arrive after it is gone.
const SECRET_FRAMES: u8 = 3;

/// The one way to show a password field. The text box hides what is typed, and the key recording
/// (`ShaleApp::log_input`) writes nothing while any such field is on screen. A test refuses
/// `.password(true)` anywhere else, so a new password field cannot forget this.
pub fn password_edit(ui: &mut egui::Ui, edit: egui::TextEdit<'_>) -> egui::Response {
    SECRET_ON_SCREEN.with(|c| c.set(SECRET_FRAMES));
    ui.add(edit.password(true))
}

/// Called once per frame by the key recording: is (or was just) a password field on screen?
pub fn secret_field_shown() -> bool {
    SECRET_ON_SCREEN.with(|c| {
        let n = c.get();
        c.set(n.saturating_sub(1));
        n > 0
    })
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
        let label = format!(
            "{}: {user} on {server}",
            if app == super::CALDAV {
                "Shale CalDAV"
            } else if app == super::SOCIAL {
                "Shale social"
            } else {
                "Shale mail"
            }
        );
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

/// A mail account with this id has no keyring in tests: saving its password fails.
#[cfg(test)]
pub const NO_KEYRING_ACCOUNT: &str = "no-keyring";

/// Tests: pretend that there is no keyring at all on this thread (`false` undoes it).
#[cfg(test)]
pub fn pretend_no_keyring(on: bool) {
    backend::NONE.with(|n| n.set(on));
}

/// Tests: a pretend keyring in memory (never the real one).
#[cfg(test)]
mod backend {
    use super::NO_KEYRING_ACCOUNT;
    use std::collections::HashMap;
    use std::sync::Mutex;

    type Key = (String, String, String);
    pub static FAKE: Mutex<Option<HashMap<Key, String>>> = Mutex::new(None);

    fn key(app: &str, server: &str, user: &str) -> Key {
        (app.to_string(), server.to_string(), user.to_string())
    }

    thread_local! {
        /// Pretend that this system has no keyring at all (only for the test thread that sets it).
        pub static NONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    pub fn available() -> bool {
        !NONE.with(|n| n.get())
    }
    pub fn lookup(app: &str, server: &str, user: &str) -> Option<String> {
        FAKE.lock().unwrap().as_ref()?.get(&key(app, server, user)).cloned()
    }
    pub fn store(app: &str, server: &str, user: &str, password: &str) -> bool {
        if server.contains(NO_KEYRING_ACCOUNT) || !available() {
            return false;
        }
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

    // spec: SEC-003
    #[test]
    fn social_secrets_have_their_own_keyring_entries() {
        assert!(social_store("https://social.test", "ada@social.test", "tok-1"));
        assert_eq!(social_lookup("https://social.test", "ada@social.test").as_deref(), Some("tok-1"));
        assert_eq!(backend::lookup(CALDAV, "https://social.test", "ada@social.test"), None, "not mixed up with calendars");
        assert_eq!(social_store_or_keep("https://social.test", "ada@social.test", "tok-2"), "");
        assert_eq!(social_secret("https://social.test", "ada@social.test", "file"), "tok-2");
        social_clear("https://social.test", "ada@social.test");
        assert_eq!(social_secret("https://social.test", "ada@social.test", "file"), "file");
    }

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

    /// The notices so far, also those an app frame of another test has taken already.
    fn notices_now() -> Vec<String> {
        NOTICE_LOG.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    // spec: SEC-011
    #[test]
    fn a_refused_keyring_never_puts_a_new_password_into_the_settings_file() {
        // the pretend keyring refuses every address that contains NO_KEYRING_ACCOUNT
        let sources = vec![dav("r", "https://no-keyring/cal/", "ada-refused", "typed-this-run")];
        let file = for_file(&sources);
        let json = serde_json::to_string(&file).unwrap();
        assert!(!json.contains("typed-this-run"), "nothing in the file: {json}");
        assert!(matches!(&sources[0].kind, SourceKind::Caldav { password, .. } if password == "typed-this-run"), "still in memory for this run");
        let told: Vec<_> = notices_now().into_iter().filter(|t| t.contains("ada-refused")).collect();
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].contains("could not be saved") && told[0].contains("again"), "{told:?}");
        // saving again does not repeat the message
        let _ = for_file(&sources);
        assert_eq!(notices_now().iter().filter(|t| t.contains("ada-refused")).count(), 1);
    }

    // spec: SEC-011
    #[test]
    fn a_password_from_the_file_stays_there_while_the_keyring_refuses_and_moves_when_it_takes() {
        let mut loaded = vec![dav("f", "https://no-keyring/cal/", "ada-old", "old-file-pw")];
        load_passwords(&mut loaded);
        let file = for_file(&loaded);
        assert!(serde_json::to_string(&file).unwrap().contains("old-file-pw"), "the only copy is not dropped");
        assert!(!notices_now().iter().any(|t| t.contains("ada-old")), "nothing was lost, nothing new to say");
        // a keyring that takes it: it leaves the file
        let mut loaded = vec![dav("g", "https://dav.example.com/cal/g/", "ada-old2", "old-file-pw2")];
        load_passwords(&mut loaded);
        let file = for_file(&loaded);
        assert!(!serde_json::to_string(&file).unwrap().contains("old-file-pw2"));
        assert_eq!(backend::lookup(CALDAV, "https://dav.example.com:443", "ada-old2").as_deref(), Some("old-file-pw2"));
    }

    // spec: SEC-011
    #[test]
    fn without_any_keyring_the_password_stays_in_the_owner_only_file_as_documented() {
        pretend_no_keyring(true);
        let sources = vec![dav("n", "https://nokr.example.com/cal/n/", "ada-nokr", "plain-pw")];
        let file = for_file(&sources);
        let kept = serde_json::to_string(&file).unwrap().contains("plain-pw");
        let social = social_store_or_keep("https://nokr.test", "ada@nokr.test", "tok");
        pretend_no_keyring(false);
        assert!(kept, "no keyring at all: kept in the file");
        assert!(!notices_now().iter().any(|t| t.contains("ada-nokr") || t.contains("ada@nokr.test")));
        assert_eq!(social, "tok");
    }

    // spec: SEC-011
    #[test]
    fn a_social_secret_the_keyring_refuses_is_kept_for_this_run_and_never_goes_to_the_file() {
        let kept = social_store_or_keep("https://no-keyring.test", "ada@refuse.test", "app-pw");
        assert_eq!(kept, "", "not for the settings file");
        assert_eq!(social_secret("https://no-keyring.test", "ada@refuse.test", ""), "app-pw", "works for this run");
        assert!(notices_now().iter().any(|t| t.contains("ada@refuse.test") && t.contains("sign in again")));
        // an old secret in the file is moved when the keyring takes it, kept when it refuses
        let mut a = String::from("old-token");
        social_move_to_keyring("https://ok.test", "ada@ok.test", &mut a);
        assert_eq!(a, "");
        assert_eq!(social_lookup("https://ok.test", "ada@ok.test").as_deref(), Some("old-token"));
        let mut b = String::from("old-token-2");
        social_move_to_keyring("https://no-keyring.test", "ada@refuse2.test", &mut b);
        assert_eq!(b, "old-token-2");
    }

    // spec: SEC-009
    #[test]
    fn every_password_field_goes_through_password_edit() {
        let mut bad = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if !name.ends_with(".rs") || name == "secrets.rs" {
                    continue;
                }
                let text = std::fs::read_to_string(&p).unwrap();
                if text.contains(".password(true)") {
                    bad.push(name);
                }
            }
        }
        assert!(bad.is_empty(), "use secrets::password_edit for password fields (the key recording must know about them): {bad:?}");
    }
}
