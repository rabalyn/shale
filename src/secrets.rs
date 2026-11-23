//! CalDAV and mail account secrets in the system keyring instead of the settings file.
//!
//! On Linux the Secret Service (GNOME Keyring, KWallet, KeePassXC …) is used through the
//! `secret-tool` program; the password goes to it on standard input, never on the command line.
//! On macOS the Keychain (`keychain.rs`) and on Windows the Credential Manager (`wincred.rs`) hold them.
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
//!
//! Every secret handed out here is a [`Secret`] (locked, zeroed on drop, SEC-016). The bytes that
//! go to `secret-tool` and come back from it are zeroed after use (SEC-017); the pipes and the
//! keyring process itself are out of reach (SEC-020).

use crate::calendar::dav;
use crate::calendar::store::{CalendarSource, SourceKind};
use crate::secret::Secret;
use eframe::egui;
use std::collections::HashMap;
use std::sync::Mutex;

/// The account a calendar belongs to: (server, user name). None without a user name.
fn account_of(s: &CalendarSource) -> Option<(String, String)> {
    let SourceKind::Caldav { url, username, .. } = &s.kind else { return None };
    if username.is_empty() {
        return None;
    }
    let o = dav::Origin::parse(url).ok()?;
    Some((format!("{}://{}:{}", o.scheme, o.host, o.port), username.clone()))
}

/// Keyring application name of CalDAV passwords.
const CALDAV: &str = "shale-caldav";
/// Keyring application name of mail account secrets.
const MAIL: &str = "shale-mail";
/// Keyring application name of social account secrets: calendars, mail and social accounts never see each other's entries.
const SOCIAL: &str = "shale-social";
/// Keyring application name of git tokens (the sync wizard, SYNC-053): their own entries too.
const GIT: &str = "shale-git";

/// What a mail account secret is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MailSecret {
    Password,
    /// The OAuth2 refresh token (`mail/oauth.rs`).
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
pub fn mail_secret(account_id: &str, kind: MailSecret) -> Option<Secret> {
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
static IN_KEYRING: Mutex<Option<HashMap<(String, String), Secret>>> = Mutex::new(None);
/// Accounts whose password the keyring refused this run (e.g. the unlock prompt was cancelled):
/// not asked again until the password changes, so saving the settings doesn't keep prompting.
static REFUSED: Mutex<Option<HashMap<(String, String), Secret>>> = Mutex::new(None);

/// Passwords that came out of the settings file of an earlier version and are still only there.
/// If the keyring refuses them they must stay in the file: it is the only copy.
static IN_FILE: Mutex<Option<HashMap<(String, String), Secret>>> = Mutex::new(None);
/// Messages for the person, shown by the app as toasts (taken with `take_notices`).
static NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Secrets of social accounts the keyring refused: kept for this run only.
static SOCIAL_SESSION: Mutex<Option<HashMap<(String, String), Secret>>> = Mutex::new(None);

/// Tests: every notice ever given, which app frames never take (headless app tests running at
/// the same time empty `NOTICES` with `take_notices`, raba/shale#80).
#[cfg(test)]
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn notice(msg: String) {
    #[cfg(test)]
    SEEN.lock().unwrap_or_else(|e| e.into_inner()).push(msg.clone());
    NOTICES.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
}

/// What the person has to be told about secrets that could not be saved (each only once).
pub fn take_notices() -> Vec<String> {
    std::mem::take(&mut *NOTICES.lock().unwrap_or_else(|e| e.into_inner()))
}

fn known() -> std::sync::MutexGuard<'static, Option<HashMap<(String, String), Secret>>> {
    IN_KEYRING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Fill in the passwords of calendars whose settings entry has none (they are in the keyring).
pub fn load_passwords(sources: &mut [CalendarSource]) {
    let mut cache: HashMap<(String, String), Option<Secret>> = HashMap::new();
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
                let ok = backend::store(CALDAV, &acct.0, &acct.1, password.expose());
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
pub fn social_lookup(server: &str, handle: &str) -> Option<Secret> {
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
pub fn social_store_or_keep(server: &str, handle: &str, secret: &Secret) -> Secret {
    if social_store(server, handle, secret.expose()) {
        return Secret::default();
    }
    if !backend::available() {
        return secret.clone();
    }
    SOCIAL_SESSION.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert((server.to_string(), handle.to_string()), secret.clone());
    notice(format!("The sign-in of “{handle}” could not be saved in the system keyring, so it was not saved at all. You will have to sign in again the next time Shale starts."));
    Secret::default()
}

/// Move a secret that an earlier version left in the settings file to the keyring. It leaves the
/// file only when the keyring has it (read back); otherwise it stays, as it is the only copy.
pub fn social_move_to_keyring(server: &str, handle: &str, in_file: &mut Secret) {
    if !in_file.is_empty() && backend::available() && social_store(server, handle, in_file.expose()) {
        in_file.clear();
    }
}

/// The secret of a social account: from the keyring, else the one kept for this run, else the one
/// in the settings file.
pub fn social_secret(server: &str, handle: &str, from_file: &Secret) -> Secret {
    social_lookup(server, handle).or_else(|| SOCIAL_SESSION.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&(server.to_string(), handle.to_string())).cloned())).unwrap_or_else(|| from_file.clone())
}

// ---------------------------------------------------------------- git tokens (the sync wizard)

/// The personal access token for a git server and user name, from the keyring (SYNC-053). `server`
/// is `https://host[:port]` (`gitsync::credentials::server_key`). Without a keyring: None.
pub fn git_token(server: &str, user: &str) -> Option<Secret> {
    backend::lookup(GIT, server, user)
}

/// Put a git token into the keyring and read it back. False when no keyring works: the token is then
/// not kept anywhere (no settings file, no `.git/config`, SYNC-053).
pub fn git_token_store(server: &str, user: &str, token: &str) -> bool {
    backend::store(GIT, server, user, token)
}

// ---------------------------------------------------------------- password fields and the key recording

thread_local! {
    /// Frames left in which a password field counts as being on screen (SEC-009). The window's
    /// drawing and its input hook run on the same thread.
    static SECRET_ON_SCREEN: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}
/// A field that was on screen a moment ago still counts: the last keys can arrive after it is gone.
const SECRET_FRAMES: u8 = 3;

/// The one way to show a password field. The text box hides what is typed and edits the locked
/// [`Secret`] buffer directly, the key recording (`ShaleApp::log_input`) writes nothing while any
/// such field is on screen, and the field's undo history is emptied every frame: egui keeps plain
/// text copies of the field there (SEC-015). A test refuses `.password(true)` anywhere else, so a
/// new password field cannot forget this. `hint` is the grey text of an empty field (may be empty).
pub fn password_edit(ui: &mut egui::Ui, secret: &mut Secret, hint: &str, width: f32) -> egui::Response {
    SECRET_ON_SCREEN.with(|c| c.set(SECRET_FRAMES));
    let mut edit = egui::TextEdit::singleline(secret).desired_width(width);
    if !hint.is_empty() {
        edit = edit.hint_text(hint);
    }
    let response = ui.add(edit.password(true));
    if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), response.id) {
        state.clear_undoer();
        state.store(ui.ctx(), response.id);
    }
    response
}

/// Called once per frame by the key recording: is (or was just) a password field on screen?
pub fn secret_field_shown() -> bool {
    SECRET_ON_SCREEN.with(|c| {
        let n = c.get();
        c.set(n.saturating_sub(1));
        n > 0
    })
}

/// Where passwords end up, for the settings dialog. Says so when secrets could not be locked
/// into memory (they are then only wiped when dropped, SEC-018).
pub fn where_stored() -> String {
    let base = if backend::available() { format!("Passwords are kept in {}.", store_name()) } else { no_keyring_text().to_string() };
    if crate::secret::lock_failures() > 0 {
        format!("{base} The system would not lock typed passwords into memory ({}), so they could end up in swap; use encrypted swap.", crate::secret::LOCK_LIMIT)
    } else {
        base
    }
}

/// What the settings dialog calls the store of this system (SEC-003).
fn store_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "the macOS Keychain"
    } else if cfg!(windows) {
        "the Windows Credential Manager"
    } else {
        "the system keyring"
    }
}

/// What the settings dialog says when the system's store cannot be used (SEC-003, SEC-021).
fn no_keyring_text() -> &'static str {
    if cfg!(target_os = "linux") {
        "No system keyring found (install secret-tool / libsecret to use one): passwords are stored in the settings file, readable only by you."
    } else if cfg!(target_os = "macos") {
        "The macOS Keychain cannot be reached: passwords are stored in the settings file, readable only by you."
    } else if cfg!(windows) {
        "The Windows Credential Manager cannot be used: passwords are stored in the settings file, readable only by you."
    } else {
        "This system has no keyring support in Shale: passwords are stored in the settings file, readable only by you."
    }
}

#[cfg(all(target_os = "linux", not(test)))]
mod backend {
    use crate::secret::Secret;
    use std::io::{Read, Write};
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    use zeroize::Zeroizing;

    fn attrs<'a>(app: &'a str, server: &'a str, user: &'a str) -> [&'a str; 6] {
        ["application", app, "server", server, "user", user]
    }

    /// The most a secret-tool answer may hold; more counts as no answer.
    const MAX_ANSWER: usize = 8192;

    /// Run secret-tool with `input` on stdin. Gives up after a while (an unlock prompt left open).
    /// What it prints comes back in a buffer that is zeroed when dropped (it never reallocates).
    fn run(args: &[&str], input: Option<&str>) -> Option<(bool, Zeroizing<Vec<u8>>)> {
        let mut child = crate::program::command("secret-tool").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        {
            let mut stdin = child.stdin.take()?;
            if let Some(i) = input {
                // a borrowed slice of the caller's Secret: no copy of ours; the pipe is the kernel's
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
        let mut out = Zeroizing::new(Vec::with_capacity(MAX_ANSWER));
        let mut chunk = Zeroizing::new([0u8; 1024]);
        let mut stdout = child.stdout.take()?;
        loop {
            let n = stdout.read(&mut chunk[..]).unwrap_or(0);
            if n == 0 {
                break;
            }
            if out.len() + n > MAX_ANSWER {
                return None;
            }
            out.extend_from_slice(&chunk[..n]);
        }
        Some((status.success(), out))
    }

    pub fn available() -> bool {
        static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            // a search that finds nothing still succeeds, but only with a running Secret Service
            crate::program::command("secret-tool").args(["search", "application", "shale-caldav-probe"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
        })
    }

    pub fn lookup(app: &str, server: &str, user: &str) -> Option<Secret> {
        let mut args = vec!["lookup"];
        args.extend(attrs(app, server, user));
        match run(&args, None) {
            Some((true, p)) if !p.is_empty() => std::str::from_utf8(&p).ok().map(Secret::new),
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
            } else if app == super::GIT {
                "Shale git"
            } else {
                "Shale mail"
            }
        );
        let mut args = vec!["store", "--label", label.as_str()];
        args.extend(attrs(app, server, user));
        // the stored password must read back exactly (no trailing newline is added)
        matches!(run(&args, Some(password)), Some((true, _))) && lookup(app, server, user).is_some_and(|s| s.expose() == password)
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
    use crate::secret::Secret;
    use std::collections::HashMap;
    use std::sync::Mutex;

    type Key = (String, String, String);
    pub static FAKE: Mutex<Option<HashMap<Key, Secret>>> = Mutex::new(None);

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
    pub fn lookup(app: &str, server: &str, user: &str) -> Option<Secret> {
        FAKE.lock().unwrap().as_ref()?.get(&key(app, server, user)).cloned()
    }
    pub fn store(app: &str, server: &str, user: &str, password: &str) -> bool {
        if server.contains(NO_KEYRING_ACCOUNT) || !available() {
            return false;
        }
        FAKE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key(app, server, user), Secret::new(password));
        true
    }
    pub fn clear(app: &str, server: &str, user: &str) {
        if let Some(m) = FAKE.lock().unwrap().as_mut() {
            m.remove(&key(app, server, user));
        }
    }
}

/// macOS: the Keychain (`secrets/keychain.rs`).
#[cfg(all(target_os = "macos", not(test)))]
mod backend {
    pub use super::keychain::{available, clear, lookup, store};
}

/// Windows: the Credential Manager (`secrets/wincred.rs`).
#[cfg(all(windows, not(test)))]
mod backend {
    pub use super::wincred::{available, clear, lookup, store};
}

/// Other systems: no keyring support, passwords stay in the owner-only settings file.
#[cfg(all(not(any(target_os = "linux", target_os = "macos", windows)), not(test)))]
mod backend {
    pub fn available() -> bool {
        false
    }
    pub fn lookup(_: &str, _: &str, _: &str) -> Option<crate::secret::Secret> {
        None
    }
    pub fn store(_: &str, _: &str, _: &str, _: &str) -> bool {
        false
    }
    pub fn clear(_: &str, _: &str, _: &str) {}
}

// The real macOS and Windows stores are compiled in test builds too: the tests of this file use
// the pretend keyring above, `keychain` and `wincred` have tests of their own against the real one.
#[cfg(target_os = "macos")]
mod keychain;
#[cfg(any(target_os = "macos", windows, test))]
mod names;
#[cfg(windows)]
mod wincred;

#[cfg(test)]
mod tests {
    /// The calendar passwords the settings file knows (`for_file` drops the keyring entries of
    /// accounts that are not in the list it gets) and the pretend keyring are shared by the whole
    /// process, so the tests of this module take turns.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }
    use super::*;

    // spec: SOC-003
    // spec: SEC-003
    #[test]
    fn social_secrets_have_their_own_keyring_entries() {
        let _serial = serial();
        assert!(social_store("https://social.test", "ada@social.test", "tok-1"));
        assert_eq!(social_lookup("https://social.test", "ada@social.test").as_ref().map(|s| s.expose()), Some("tok-1"));
        assert_eq!(backend::lookup(CALDAV, "https://social.test", "ada@social.test"), None, "not mixed up with calendars");
        assert_eq!(social_store_or_keep("https://social.test", "ada@social.test", &Secret::new("tok-2")), "");
        assert_eq!(social_secret("https://social.test", "ada@social.test", &Secret::new("file")), "tok-2");
        social_clear("https://social.test", "ada@social.test");
        assert_eq!(social_secret("https://social.test", "ada@social.test", &Secret::new("file")), "file");
    }

    // spec: SYNC-053
    #[test]
    fn git_tokens_have_their_own_keyring_entries_and_without_a_keyring_nothing_is_kept() {
        let _serial = serial();
        assert!(git_token_store("https://git.test", "ada", "tok-g"));
        assert_eq!(git_token("https://git.test", "ada").as_ref().map(|s| s.expose()), Some("tok-g"));
        assert_eq!(backend::lookup(SOCIAL, "https://git.test", "ada"), None, "not mixed up with social accounts");
        assert_eq!(git_token("https://git.test", "bob").map(|s| s.expose().to_string()), None);
        backend::clear(GIT, "https://git.test", "ada");
        assert!(git_token("https://git.test", "ada").is_none());
        pretend_no_keyring(true);
        assert!(!git_token_store("https://git.test", "ada", "tok-g"), "no keyring: not kept");
        assert!(git_token("https://git.test", "ada").is_none());
        pretend_no_keyring(false);
    }

    fn dav(id: &str, url: &str, user: &str, pw: &str) -> CalendarSource {
        CalendarSource { id: id.into(), name: id.into(), color: "#000000".into(), enabled: true, sync_minutes: 15, kind: SourceKind::Caldav { url: url.into(), username: user.into(), password: pw.into() } }
    }

    // spec: SEC-003
    #[test]
    // its own host: other tests (the calendar dialog) save "ada" at dav.example.com at the same time
    fn passwords_go_to_the_keyring_and_come_back() {
        let _serial = serial();
        let sources = vec![dav("a", "https://sec003.example.com/cal/a/", "ada", "s3cret"), dav("b", "https://SEC003.example.com:443/cal/b/", "ada", "s3cret"), dav("anon", "https://pub.example.com/c/", "", "")];
        let file = for_file(&sources);
        let json = serde_json::to_string(&file).unwrap();
        assert!(!json.contains("s3cret"), "no password in the settings file: {json}");
        assert_eq!(backend::lookup(CALDAV, "https://sec003.example.com:443", "ada").as_ref().map(|s| s.expose()), Some("s3cret"), "one entry for the account");
        // reading the settings back
        let mut loaded: Vec<CalendarSource> = serde_json::from_str(&json).unwrap();
        load_passwords(&mut loaded);
        assert_eq!(loaded, sources);
        // the account is removed: its keyring entry goes too
        let _ = for_file(&sources[2..]);
        assert_eq!(backend::lookup(CALDAV, "https://sec003.example.com:443", "ada"), None);
    }

    // spec: CAL-032
    #[test]
    fn an_edited_calendar_account_replaces_its_keyring_entry_and_the_old_user_name_entry_goes() {
        let _serial = serial();
        let before = vec![dav("e1", "https://cal032.example.com/cal/a/", "ada-edit", "old-pw"), dav("e2", "https://cal032.example.com/cal/b/", "ada-edit", "old-pw")];
        let _ = for_file(&before);
        let acct = "https://cal032.example.com:443";
        assert_eq!(backend::lookup(CALDAV, acct, "ada-edit").as_ref().map(|s| s.expose()), Some("old-pw"));
        // a new password for the same user: the entry is replaced
        let pw = vec![dav("e1", "https://cal032.example.com/cal/a/", "ada-edit", "new-pw"), dav("e2", "https://cal032.example.com/cal/b/", "ada-edit", "new-pw")];
        let file = for_file(&pw);
        assert!(!serde_json::to_string(&file).unwrap().contains("new-pw"));
        assert_eq!(backend::lookup(CALDAV, acct, "ada-edit").as_ref().map(|s| s.expose()), Some("new-pw"));
        // a new user name: a new entry, the old one is cleared
        let user = vec![dav("e1", "https://cal032.example.com/cal/a/", "ada-edit2", "new-pw"), dav("e2", "https://cal032.example.com/cal/b/", "ada-edit2", "new-pw")];
        let _ = for_file(&user);
        assert_eq!(backend::lookup(CALDAV, acct, "ada-edit2").as_ref().map(|s| s.expose()), Some("new-pw"));
        assert_eq!(backend::lookup(CALDAV, acct, "ada-edit"), None);
        let _ = for_file(&[]);
    }

    // spec: MAIL-021
    #[test]
    fn mail_secrets_have_their_own_entries() {
        let _serial = serial();
        assert!(set_mail_secret("acct-1", MailSecret::Password, "pw-1"));
        assert!(set_mail_secret("acct-1", MailSecret::RefreshToken, "tok-1"));
        assert_eq!(mail_secret("acct-1", MailSecret::Password).as_ref().map(|s| s.expose()), Some("pw-1"));
        assert_eq!(mail_secret("acct-1", MailSecret::RefreshToken).as_ref().map(|s| s.expose()), Some("tok-1"));
        assert_eq!(mail_secret("acct-2", MailSecret::Password), None);
        // a CalDAV entry of the same names is a different entry
        assert_eq!(backend::lookup(CALDAV, "acct-1", "password"), None);
        clear_mail_secret("acct-1", MailSecret::Password);
        assert_eq!(mail_secret("acct-1", MailSecret::Password), None);
        assert_eq!(mail_secret("acct-1", MailSecret::RefreshToken).as_ref().map(|s| s.expose()), Some("tok-1"));
        clear_mail_secret("acct-1", MailSecret::RefreshToken);
    }

    /// Every notice so far, also those an app frame of another test already took.
    fn notices_now() -> Vec<String> {
        SEEN.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    // spec: SEC-011
    #[test]
    fn a_refused_keyring_never_puts_a_new_password_into_the_settings_file() {
        let _serial = serial();
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
        let _serial = serial();
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
        assert_eq!(backend::lookup(CALDAV, "https://dav.example.com:443", "ada-old2").as_ref().map(|s| s.expose()), Some("old-file-pw2"));
    }

    // spec: SEC-011
    #[test]
    fn without_any_keyring_the_password_stays_in_the_owner_only_file_as_documented() {
        let _serial = serial();
        pretend_no_keyring(true);
        let sources = vec![dav("n", "https://nokr.example.com/cal/n/", "ada-nokr", "plain-pw")];
        let file = for_file(&sources);
        let kept = serde_json::to_string(&file).unwrap().contains("plain-pw");
        let social = social_store_or_keep("https://nokr.test", "ada@nokr.test", &Secret::new("tok"));
        pretend_no_keyring(false);
        assert!(kept, "no keyring at all: kept in the file");
        assert!(!notices_now().iter().any(|t| t.contains("ada-nokr") || t.contains("ada@nokr.test")));
        assert_eq!(social, "tok");
    }

    // spec: SEC-011
    #[test]
    fn a_social_secret_the_keyring_refuses_is_kept_for_this_run_and_never_goes_to_the_file() {
        let _serial = serial();
        let kept = social_store_or_keep("https://no-keyring.test", "ada@refuse.test", &Secret::new("app-pw"));
        assert_eq!(kept, "", "not for the settings file");
        assert_eq!(social_secret("https://no-keyring.test", "ada@refuse.test", &Secret::default()), "app-pw", "works for this run");
        assert!(notices_now().iter().any(|t| t.contains("ada@refuse.test") && t.contains("sign in again")));
        // an old secret in the file is moved when the keyring takes it, kept when it refuses
        let mut a = Secret::new("old-token");
        social_move_to_keyring("https://ok.test", "ada@ok.test", &mut a);
        assert_eq!(a, "");
        assert_eq!(social_lookup("https://ok.test", "ada@ok.test").as_ref().map(|s| s.expose()), Some("old-token"));
        let mut b = Secret::new("old-token-2");
        social_move_to_keyring("https://no-keyring.test", "ada@refuse2.test", &mut b);
        assert_eq!(b, "old-token-2");
    }

    // spec: SEC-009
    #[test]
    fn every_password_field_goes_through_password_edit() {
        let _serial = serial();
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

    /// One frame of a window with `add` in it.
    fn frame(ctx: &egui::Context, t: f64, events: Vec<egui::Event>, mut add: impl FnMut(&mut egui::Ui)) {
        let raw = egui::RawInput { time: Some(t), events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| add(ui));
        });
        // no renderer here: drop the font texture upload on purpose (debug builds assert)
        out.textures_delta.clear();
    }

    /// Type "hunter2" into a field made by `add` (it returns the field's response) and say whether
    /// egui's undo history of the field is in use afterwards.
    fn types_into(mut add: impl FnMut(&mut egui::Ui) -> egui::Response) -> bool {
        let ctx = egui::Context::default();
        let mut id = egui::Id::NULL;
        let mut in_flux = false;
        for (t, events) in [(0.0, vec![]), (0.1, vec![egui::Event::Text("hunter2".into())]), (0.2, vec![]), (0.3, vec![])] {
            frame(&ctx, t, events, |ui| {
                let r = add(ui);
                r.request_focus();
                id = r.id;
            });
            in_flux |= egui::text_edit::TextEditState::load(&ctx, id).is_some_and(|s| s.undoer().is_in_flux());
        }
        in_flux
    }

    // spec: SEC-015
    #[test]
    fn a_password_field_keeps_no_undo_history_and_edits_the_locked_buffer() {
        let _serial = serial();
        let mut secret = Secret::default();
        let kept = types_into(|ui| password_edit(ui, &mut secret, "hint", 100.0));
        assert_eq!(secret.expose(), "hunter2", "typed into the locked buffer");
        assert!(!kept, "no undo state is kept for the password");
        assert!(secret_field_shown(), "the key recording is told");
        // the control: a plain password field does record its text for undo
        let mut plain = String::new();
        assert!(types_into(|ui| ui.add(egui::TextEdit::singleline(&mut plain).password(true))), "the check can see an undo history when there is one");
        assert_eq!(plain, "hunter2");
    }

    // spec: SEC-018
    #[test]
    fn the_settings_dialog_says_when_secrets_could_not_be_locked() {
        let _serial = serial();
        crate::secret::fail_locks_on_this_thread(true);
        let _unlocked = Secret::new("x");
        crate::secret::fail_locks_on_this_thread(false);
        let text = where_stored();
        assert!(text.contains(crate::secret::LOCK_LIMIT) && text.contains("encrypted swap"), "{text}");
        // the store of this system is still named (the Keychain and the Credential Manager are not called "keyring")
        assert!(text.contains(store_name()), "{text}");
    }

    // spec: SEC-021
    #[test]
    fn without_a_keyring_the_dialog_names_the_settings_file_and_only_linux_points_to_secret_tool() {
        let _serial = serial();
        pretend_no_keyring(true);
        let text = where_stored();
        pretend_no_keyring(false);
        assert!(text.contains("settings file") && text.contains("readable only by you"), "{text}");
        assert_eq!(text.contains("secret-tool"), cfg!(target_os = "linux"), "{text}");
    }

    // spec: SEC-003
    #[test]
    fn the_dialog_names_the_store_of_this_system() {
        let _serial = serial();
        let text = where_stored();
        let name = if cfg!(target_os = "macos") {
            "macOS Keychain"
        } else if cfg!(windows) {
            "Windows Credential Manager"
        } else {
            "system keyring"
        };
        assert!(text.contains(name), "{text}");
    }
}
