//! Tests against the real servers of the local container environment (`tests/mail-env/`,
//! MAIL-091): Dovecot with TLS and seeded mailboxes, and a pretend OAuth2 provider. They are
//! ignored unless asked for, because they need the containers:
//!
//! ```text
//! tests/mail-env/mail-env.sh up
//! cargo test -- --ignored mail_env
//! ```
//!
//! Trust: the server's certificate is made by `setup-certs.sh`; here it is trusted as an extra
//! root by test code only (`Client::connect_test`). The production client has no such switch.
//!
//! The tests share one environment, so they take turns (`TURN`); the OAuth2 test ends by
//! restarting the provider (`mail-env.sh revoke`), which kills every token it handed out.

use crate::config::{MailAccount, MailAuth, MailSecurity, OAuthProvider};
use crate::mail::db::{FolderRow, MessageSummary, Op, STATE_FULL, STATE_HEADER};
use crate::mail::imap_client::MailFlag;
use crate::mail::imap_client::{Client, ConnectSettings, ImapError, Limits};
use crate::mail::imap_sync::{connect_with_retry, discover_folders, fetch_folder_listing, sync_account, AuthFactory, Connector, PasswordAuth, SyncOptions};
use crate::mail::oauth::{self, OAuthConfig, OAuthFactory};
use crate::mail::oauth_fake_tests::{http_get, http_post_form};
use crate::mail::ops::{self, Action, Confirmed};
use crate::mail::store::MailStore;
use crate::secrets::{self, MailSecret};
use rustls::pki_types::CertificateDer;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const IMAPS_PORT: u16 = 10993;
const PROVIDER: &str = "http://localhost:18080/default";
const MAILDATA_VOLUME: &str = "shale-mail-env-maildata";
const ALPINE: &str = "docker.io/library/alpine:3.24.2";

static TURN: Mutex<()> = Mutex::new(());

fn env_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("mail-env")
}

/// `podman` unless SHALE_MAIL_ENV_ENGINE says otherwise (`docker`).
fn engine() -> String {
    std::env::var("SHALE_MAIL_ENV_ENGINE").unwrap_or_else(|_| "podman".into())
}

fn run(program: &str, args: &[&str]) -> String {
    let out = Command::new(program).args(args).output().unwrap_or_else(|e| panic!("could not run {program}: {e}"));
    assert!(out.status.success(), "{program} {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The certificate authority that `setup-certs.sh` made, as DER.
fn ca_cert() -> CertificateDer<'static> {
    let path = env_dir().join("certs").join("ca.crt");
    let pem = crate::fsutil::read_to_string(&path).unwrap_or_else(|e| panic!("no {}: {e}. Start the environment first: tests/mail-env/mail-env.sh up", path.display()));
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    CertificateDer::from(crate::mail::base64_decode(body.as_bytes()))
}

fn settings() -> ConnectSettings {
    ConnectSettings { host: "localhost".into(), port: IMAPS_PORT, security: MailSecurity::ImplicitTls, limits: Limits { io_timeout: Duration::from_secs(60), ..Limits::default() } }
}

struct EnvConnector {
    auth: Arc<dyn AuthFactory>,
}

impl Connector for EnvConnector {
    fn connect(&self) -> Result<Client, ImapError> {
        connect_with_retry(self.auth.as_ref(), |a| Client::connect_test(&settings(), ca_cert(), a))
    }
}

/// The message files of all mailboxes (directory, name with its flags): what the server has on
/// disk. Read through a second container that mounts the volume read-only.
fn maildir_files() -> Vec<String> {
    let out = run(&engine(), &["run", "--rm", "-v", &format!("{MAILDATA_VOLUME}:/v:ro"), ALPINE, "sh", "-c", "cd /v && find . \\( -path '*/cur/*' -o -path '*/new/*' \\) -type f | sort"]);
    out.lines().map(String::from).collect()
}

/// `doveadm fetch` of a mailbox inside the Dovecot container: one line per message with its
/// subject and flags (the server's own view, `\Recent` included).
fn dovecot_view(user: &str, mailbox: &str) -> Vec<String> {
    let id = run(&engine(), &["ps", "-q", "--filter", "label=com.docker.compose.service=dovecot", "--filter", "label=com.docker.compose.project=shale-mail-env"]);
    let id = id.trim();
    assert!(!id.is_empty(), "the dovecot container is not running: tests/mail-env/mail-env.sh up");
    let out = run(&engine(), &["exec", id, "/dovecot/bin/doveadm", "-f", "flow", "fetch", "-u", user, "uid hdr.subject flags", "mailbox", mailbox, "all"]);
    out.lines().map(String::from).collect()
}

fn folder<'a>(rows: &'a [FolderRow], name: &str) -> &'a FolderRow {
    rows.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no folder {name}: {:?}", rows.iter().map(|f| &f.name).collect::<Vec<_>>()))
}

fn messages(store: &MailStore, rows: &[FolderRow], name: &str) -> Vec<MessageSummary> {
    store.reader().unwrap().list(folder(rows, name).id, 500, 0).unwrap()
}

fn subjects(store: &MailStore, rows: &[FolderRow], name: &str) -> Vec<String> {
    let mut v: Vec<String> = messages(store, rows, name).into_iter().map(|m| m.subject).collect();
    v.sort();
    v
}

fn temp_store(tag: &str) -> (PathBuf, MailStore) {
    let dir = std::env::temp_dir().join(format!("shale-mailenv-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = MailStore::open(&dir).expect("the mail store");
    (dir, store)
}

fn sync(store: &MailStore, id: &str, connector: &dyn Connector) -> crate::mail::imap_sync::SyncSummary {
    let (tx, _rx) = channel();
    sync_account(store, id, id, connector, &SyncOptions::default(), &tx, &AtomicBool::new(false))
}

fn is_unseen(line: &str) -> bool {
    !line.contains("\\Seen")
}

// spec: MAIL-091, MAIL-092
#[test]
#[ignore = "needs the containers: tests/mail-env/mail-env.sh up"]
fn mail_env_password_account_syncs_end_to_end_and_changes_nothing_on_the_server() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, store) = temp_store("pw");
    let auth: Arc<dyn AuthFactory> = Arc::new(PasswordAuth { user: "alice@shale.test".into(), password: "shale-test-pw".into() });
    let connector = EnvConnector { auth };

    // a wrong password is an authentication error, not a crash
    let bad = EnvConnector { auth: Arc::new(PasswordAuth { user: "alice@shale.test".into(), password: "nope".into() }) };
    assert!(matches!(bad.connect().err(), Some(ImapError::Auth(_))));
    // the certificate is only trusted by the test: the production path (operating system store) refuses it
    let mut p = PasswordAuth { user: "alice@shale.test".into(), password: "shale-test-pw".into() }.authenticator().unwrap();
    let err = Client::connect(&settings(), p.as_mut()).err().expect("the system does not trust the test CA");
    assert!(matches!(err, ImapError::Tls(_)), "{err}");

    let files_before = maildir_files();
    let view_before = dovecot_view("alice@shale.test", "INBOX");
    assert!(view_before.iter().filter(|l| is_unseen(l)).count() >= 3, "the seed has unseen mail: {view_before:?}");

    // folders, with special-use flags
    let found = discover_folders(&store, &connector, "alice", "Alice").expect("folders");
    let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
    for want in ["INBOX", "Work", "Work/Projects", "Sent", "Drafts", "Trash", "Junk"] {
        assert!(names.contains(&want), "{want} missing in {names:?}");
    }
    assert_eq!(folder(&found, "Trash").special_use.as_deref(), Some("\\Trash"));

    let summary = sync(&store, "alice", &connector);
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!(summary.folders_failed, 0);
    let rows = store.reader().unwrap().folders(Some("alice")).unwrap();

    // the 3-year window: "Ancient history" (1200 days) and "Nested old" are not fetched, "Two years ago" is
    let inbox = subjects(&store, &rows, "INBOX");
    assert_eq!(inbox, ["Big attachment", "Grüße aus Köln", "HTML only newsletter", "Invoice 2026", "Lunch plans", "Meeting notes", "Two years ago"], "INBOX");
    assert_eq!(subjects(&store, &rows, "Work"), ["Work item"]);
    assert_eq!(subjects(&store, &rows, "Work/Projects"), ["Project plan"], "a nested folder, with the old mail left out");
    assert_eq!(subjects(&store, &rows, "Sent"), ["Re: Invoice 2026"]);
    // Trash and Junk are never synced by themselves
    for never in ["Trash", "Junk"] {
        assert!(messages(&store, &rows, never).is_empty(), "{never} was synced");
        assert!(!folder(&rows, never).sync_enabled, "{never} is switched on");
    }
    assert_eq!(store.reader().unwrap().count_messages().unwrap(), 10);

    // a large mail with an attachment, fully downloaded
    let inbox_msgs = messages(&store, &rows, "INBOX");
    let big = inbox_msgs.iter().find(|m| m.subject == "Big attachment").unwrap();
    assert!(big.state == STATE_FULL && big.has_attachments && big.size > 6_000_000, "{big:?}");
    let mail = store.read_mail(big).unwrap();
    assert_eq!(mail.attachments.len(), 1);
    assert_eq!((mail.attachments[0].name.as_str(), mail.attachments[0].data.len()), ("big.bin", 4_500_000));
    // an HTML-only mail is shown as text, with nothing to load
    let html = inbox_msgs.iter().find(|m| m.subject == "HTML only newsletter").unwrap();
    let text = store.read_mail(html).unwrap().text;
    assert!(text.contains("Spring news") && text.contains("reader") && !text.contains("<h1>") && !text.contains("<body"), "{text}");
    // headers and a body in ISO-8859-1
    let latin = inbox_msgs.iter().find(|m| m.subject.starts_with("Gr")).unwrap();
    assert!(latin.from.contains("Jürgen Müller"), "{}", latin.from);
    assert!(store.read_mail(latin).unwrap().text.contains("schöne Grüße"));
    // flags come along as the server has them
    let notes = inbox_msgs.iter().find(|m| m.subject == "Meeting notes").unwrap();
    assert!(notes.flags.contains("\\Flagged") && notes.is_seen(), "{}", notes.flags);
    assert!(!inbox_msgs.iter().find(|m| m.subject == "Invoice 2026").unwrap().is_seen());

    // a second sync finds nothing new and does not fail
    let again = sync(&store, "alice", &connector);
    assert!(again.errors.is_empty() && again.messages_added == 0, "{again:?}");
    // Trash is listed on request: headers only
    let listed = fetch_folder_listing(&store, &connector, "alice", "Trash", 100, &SyncOptions::default()).unwrap();
    assert_eq!(listed, 2);
    let rows = store.reader().unwrap().folders(Some("alice")).unwrap();
    assert!(messages(&store, &rows, "Trash").iter().all(|m| m.state == STATE_HEADER));

    // read-only: not a file moved or renamed, no flag changed, unseen mail still unseen and
    // never taken out of new/ (what SELECT or a non-PEEK fetch would do)
    assert_eq!(maildir_files(), files_before, "the server's message files changed");
    let view_after = dovecot_view("alice@shale.test", "INBOX");
    assert_eq!(view_after, view_before, "the server's view of INBOX changed");
    assert!(view_after.iter().filter(|l| is_unseen(l)).count() >= 3);
    let _ = std::fs::remove_dir_all(dir);
}

fn oauth_account() -> MailAccount {
    MailAccount {
        id: "mailenv-oauth".into(),
        name: "OAuth user".into(),
        host: "localhost".into(),
        port: IMAPS_PORT,
        user: "oauth@shale.test".into(),
        auth: MailAuth::OAuth2,
        oauth_provider: OAuthProvider::Custom,
        oauth_client_id: "shale-mail-env".into(),
        oauth_auth_url: format!("{PROVIDER}/authorize"),
        oauth_token_url: format!("{PROVIDER}/token"),
        oauth_scope: "openid mail offline_access".into(),
        ..MailAccount::default()
    }
}

/// Sign in the way a person would, with the pretend browser typing the user name into the
/// provider's login page.
fn sign_in_headless(acct: &MailAccount) -> Result<oauth::SignedIn, oauth::OAuthError> {
    let _browser = crate::opener::install_test_browser(Box::new(|url| {
        let url = url.to_string();
        std::thread::spawn(move || {
            let (status, head, _) = http_post_form(&url, "username=oauth%40shale.test&claims=");
            assert_eq!(status, 302, "the login page redirects back: {head}");
            let loc = head.lines().find_map(|l| l.strip_prefix("location: ").or_else(|| l.strip_prefix("Location: "))).expect("a Location header").trim().to_string();
            let _ = http_get(&loc);
        });
    }));
    oauth::sign_in(acct, None, &AtomicBool::new(false), Duration::from_secs(60), &|_| {})
}

// spec: MAIL-091
#[test]
#[ignore = "needs the containers: tests/mail-env/mail-env.sh up"]
fn mail_env_oauth_account_signs_in_syncs_renews_its_token_and_a_dead_sign_in_is_said_in_words() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let acct = oauth_account();
    secrets::clear_mail_secret(&acct.id, MailSecret::RefreshToken);
    oauth::forget_source(&acct.id);
    let (dir, store) = temp_store("oauth");
    let files_before = maildir_files();

    // not signed in yet
    let cfg = OAuthConfig::for_account(&acct, None).unwrap();
    let source = oauth::shared_source(&acct.id, cfg.clone());
    let factory = Arc::new(OAuthFactory { user: acct.user.clone(), host: acct.host.clone(), port: acct.port, source: source.clone() });
    let connector = EnvConnector { auth: factory.clone() };
    assert!(matches!(connector.connect().err(), Some(ImapError::Auth(t)) if oauth::needs_sign_in(&t)));

    // the browser sign-in, with PKCE, against the provider's real login page
    let who = sign_in_headless(&acct).expect("the sign-in");
    assert_eq!(who.user.as_deref(), Some("oauth@shale.test"));
    assert!(secrets::mail_secret(&acct.id, MailSecret::RefreshToken).is_some(), "the refresh token is in the keyring");

    // Dovecot accepts the token (OAUTHBEARER or XOAUTH2) and the sync works
    let summary = sync(&store, &acct.id, &connector);
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    let rows = store.reader().unwrap().folders(Some(&acct.id)).unwrap();
    assert_eq!(subjects(&store, &rows, "INBOX"), ["Hello from OAuth land", "Second mail"]);
    assert_eq!(subjects(&store, &rows, "Sent"), ["Sent via OAuth"]);
    assert!(messages(&store, &rows, "Trash").is_empty());
    assert_eq!(maildir_files(), files_before, "the server's message files changed");
    assert!(dovecot_view("oauth@shale.test", "INBOX").iter().any(|l| l.contains("Hello from OAuth land") && is_unseen(l)), "the unseen mail is still unseen");

    // the access token is dropped (as if the server had refused it): a new one is fetched by itself
    let before = source.refresh_count();
    source.reject();
    connector.connect().expect("a new access token").logout();
    assert_eq!(source.refresh_count(), before + 1);

    // a token for someone else's name is refused
    let other = EnvConnector { auth: Arc::new(OAuthFactory { user: "alice@shale.test".into(), host: acct.host.clone(), port: acct.port, source: source.clone() }) };
    assert!(matches!(other.connect().err(), Some(ImapError::Auth(_))));

    // the provider forgets everything it handed out (restart): the old tokens are dead
    run("sh", &[env_dir().join("mail-env.sh").to_str().unwrap(), "revoke"]);
    let err = connector.connect().err().expect("the sign-in is dead");
    let ImapError::Auth(text) = &err else { panic!("{err}") };
    // (this pretend provider hands out a token for a stranger on any refresh token, so what the
    // person sees is the server's refusal of a fresh token; a real provider says invalid_grant,
    // which `mail/oauth.rs` tests cover with its own pretend provider)
    assert!(oauth::needs_sign_in(text) && text.contains("Authentication failed"), "{text}");

    // signing in again works
    sign_in_headless(&acct).expect("signing in again");
    connector.connect().expect("works again").logout();

    secrets::clear_mail_secret(&acct.id, MailSecret::RefreshToken);
    oauth::forget_source(&acct.id);
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------------------ phase 2 (MAIL-113)

fn container_id() -> String {
    let id = run(&engine(), &["ps", "-q", "--filter", "label=com.docker.compose.service=dovecot", "--filter", "label=com.docker.compose.project=shale-mail-env"]);
    let id = id.trim().to_string();
    assert!(!id.is_empty(), "the dovecot container is not running: tests/mail-env/mail-env.sh up");
    id
}

/// `doveadm` inside the Dovecot container, optionally with a message on its standard input.
/// Errors are returned, not fatal (cleaning up must not fail a test that already failed).
fn doveadm(args: &[&str], stdin: Option<&[u8]>) -> Result<String, String> {
    use std::io::Write;
    let id = container_id();
    let mut cmd = Command::new(engine());
    cmd.args(["exec", "-i", &id, "/dovecot/bin/doveadm"]).args(args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    if let Some(mut si) = child.stdin.take() {
        if let Some(data) = stdin {
            si.write_all(data).map_err(|e| e.to_string())?;
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

const USER: &str = "alice@shale.test";

/// The server's own view of a mailbox: "uid=1 hdr.subject=… flags=…", without `\Recent` (which only says
/// that no session has looked at the message yet).
fn server_view(mailbox: &str) -> Vec<String> {
    let out = doveadm(&["-f", "flow", "fetch", "-u", USER, "uid hdr.subject flags", "mailbox", mailbox, "all"], None).unwrap_or_default();
    out.lines().map(|l| l.replace("\\Recent", "").split_whitespace().collect::<Vec<_>>().join(" ")).collect()
}

fn line_of<'a>(view: &'a [String], subject: &str) -> Option<&'a String> {
    view.iter().find(|l| l.contains(&format!("hdr.subject={subject}")))
}

fn put_message(mailbox: &str, subject: &str) {
    let raw = format!(
        "From: Sender <sender@shale.test>\r\nTo: {USER}\r\nSubject: {subject}\r\nMessage-ID: <{}@scratch.shale.test>\r\nDate: Thu, 12 Nov 2026 10:00:00 +0000\r\n\r\nA test message for {subject}.\r\n",
        subject.replace(' ', "-").to_lowercase()
    );
    doveadm(&["save", "-u", USER, "-m", mailbox], Some(raw.as_bytes())).unwrap_or_else(|e| panic!("could not put {subject:?} into {mailbox}: {e}"));
}

/// Remove everything this test makes (also what an earlier, failed run left behind).
fn clean_scratch() {
    for mb in ["Scratch", "Scratch2"] {
        let _ = doveadm(&["mailbox", "delete", "-u", USER, mb], None);
    }
    for subject in ["Scratch four", "Scratch five"] {
        let _ = doveadm(&["expunge", "-u", USER, "mailbox", "Trash", "header", "Subject", subject], None);
    }
}

fn run_ops(store: &MailStore, connector: &dyn Connector) -> ops::OpsReport {
    let r = ops::run_pending(store, connector, "alice");
    assert!(r.failed.is_empty() && r.offline.is_none(), "{r:?}");
    assert_eq!(r.remaining, 0, "{r:?}");
    r
}

fn only_scratch_is_synced(store: &MailStore, connector: &dyn Connector) {
    let rows = discover_folders(store, connector, "alice", "Alice").expect("folders");
    for f in &rows {
        let on = f.name == "Scratch" || f.name == "Scratch2";
        store.commit(vec![Op::SyncEnabled { folder: f.id, enabled: on }]).unwrap();
    }
}

// spec: MAIL-113
#[test]
#[ignore = "needs the containers: tests/mail-env/mail-env.sh up"]
fn mail_env_flag_unflag_move_delete_and_delete_forever_change_dovecot_as_asked_and_nothing_else() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    clean_scratch();
    let (dir, store) = temp_store("ops");
    let connector = EnvConnector { auth: Arc::new(PasswordAuth { user: USER.into(), password: "shale-test-pw".into() }) };
    // what must not change: every seeded mailbox
    let watched = ["INBOX", "Work", "Work/Projects", "Sent", "Drafts", "Junk", "Trash"];
    let before: Vec<Vec<String>> = watched.iter().map(|m| server_view(m)).collect();
    assert!(!before[0].is_empty() && !before[6].is_empty(), "the seed is there: {before:?}");

    for mb in ["Scratch", "Scratch2"] {
        doveadm(&["mailbox", "create", "-u", USER, mb], None).unwrap();
    }
    for s in ["Scratch one", "Scratch two", "Scratch three", "Scratch four"] {
        put_message("Scratch", s);
    }
    only_scratch_is_synced(&store, &connector);
    let sum = sync(&store, "alice", &connector);
    assert!(sum.errors.is_empty() && sum.messages_added == 4, "{sum:?}");
    let rows = store.reader().unwrap().folders(Some("alice")).unwrap();
    let by_subject = |name: &str, subject: &str| -> MessageSummary { messages(&store, &rows_now(&store), name).into_iter().find(|m| m.subject == subject).unwrap_or_else(|| panic!("{subject} not in {name}")) };
    fn rows_now(store: &MailStore) -> Vec<FolderRow> {
        store.reader().unwrap().folders(Some("alice")).unwrap()
    }
    let _ = rows;
    let now = crate::calendar::time::now();
    let flags = |add: &[MailFlag], remove: &[MailFlag]| Action::SetFlags { add: add.to_vec(), remove: remove.to_vec() };

    // read, flagged, and back
    ops::enqueue(&store, &[by_subject("Scratch", "Scratch one")], &flags(&[MailFlag::Seen], &[]), now).unwrap();
    ops::enqueue(&store, &[by_subject("Scratch", "Scratch two")], &flags(&[MailFlag::Flagged], &[]), now).unwrap();
    assert_eq!(run_ops(&store, &connector).done, 2);
    let v = server_view("Scratch");
    assert!(line_of(&v, "Scratch one").unwrap().contains("\\Seen") && !line_of(&v, "Scratch one").unwrap().contains("\\Flagged"), "{v:?}");
    assert!(line_of(&v, "Scratch two").unwrap().contains("\\Flagged") && !line_of(&v, "Scratch two").unwrap().contains("\\Seen"), "{v:?}");
    assert!(!line_of(&v, "Scratch three").unwrap().contains("\\Seen"), "an untouched mail stays unseen: {v:?}");
    // the local copy took the server's answer
    assert!(by_subject("Scratch", "Scratch one").is_seen());
    assert!(by_subject("Scratch", "Scratch two").is_flagged());
    ops::enqueue(&store, &[by_subject("Scratch", "Scratch one")], &flags(&[], &[MailFlag::Seen]), now).unwrap();
    ops::enqueue(&store, &[by_subject("Scratch", "Scratch two")], &flags(&[], &[MailFlag::Flagged]), now).unwrap();
    run_ops(&store, &connector);
    let v = server_view("Scratch");
    assert!(!line_of(&v, "Scratch one").unwrap().contains("\\Seen") && !line_of(&v, "Scratch two").unwrap().contains("\\Flagged"), "{v:?}");

    // move: the server has it in the other folder and not in this one; the row and file followed
    let three = by_subject("Scratch", "Scratch three");
    let old_file = store.abs_path(&three.path);
    assert!(old_file.is_file());
    ops::enqueue(&store, &[three], &Action::MoveTo { folder: "Scratch2".into() }, now).unwrap();
    assert_eq!(run_ops(&store, &connector).done, 1);
    assert!(line_of(&server_view("Scratch"), "Scratch three").is_none());
    assert!(line_of(&server_view("Scratch2"), "Scratch three").is_some());
    let moved = by_subject("Scratch2", "Scratch three");
    assert!(moved.state == STATE_FULL && store.abs_path(&moved.path).is_file(), "the row in the new folder has a file");
    assert!(!old_file.exists() && !store.read_raw(&moved).unwrap().is_empty());

    // delete: Dovecot has it in Trash (a move, no \Deleted)
    let trash_before = server_view("Trash");
    ops::enqueue(&store, &[by_subject("Scratch", "Scratch four")], &Action::Delete, now).unwrap();
    assert_eq!(run_ops(&store, &connector).done, 1);
    assert!(line_of(&server_view("Scratch"), "Scratch four").is_none());
    let trash_now = server_view("Trash");
    let four = line_of(&trash_now, "Scratch four").expect("in Trash");
    assert!(!four.contains("\\Deleted"), "a delete is a move, not a flag: {four}");
    assert_eq!(trash_now.len(), trash_before.len() + 1);

    // delete forever: only the chosen mail; another program's \Deleted mail in Trash survives
    put_message("Trash", "Scratch five");
    doveadm(&["flags", "add", "-u", USER, "\\Deleted", "mailbox", "Trash", "header", "Subject", "Scratch five"], None).unwrap();
    fetch_folder_listing(&store, &connector, "alice", "Trash", 100, &SyncOptions::default()).unwrap();
    let rows = rows_now(&store);
    let trash_row = messages(&store, &rows, "Trash").into_iter().find(|m| m.subject == "Scratch four").expect("listed");
    ops::enqueue(&store, &[trash_row], &Action::DeleteForever(Confirmed::after_dialog(1)), now).unwrap();
    assert_eq!(run_ops(&store, &connector).done, 1);
    let trash_after = server_view("Trash");
    assert!(line_of(&trash_after, "Scratch four").is_none(), "{trash_after:?}");
    let five = line_of(&trash_after, "Scratch five").expect("the other program's \\Deleted mail survived");
    assert!(five.contains("\\Deleted"), "{five}");
    assert_eq!(trash_after.len(), trash_before.len() + 1, "the seeded Trash mails and the survivor");
    // nothing local was deleted: the file of every handled mail is still in the store (in .trash for those that left)
    let kept: Vec<String> = std::fs::read_dir(dir.join(".trash")).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    assert!(!kept.is_empty(), "the local trash has the old files");

    // a sync afterwards agrees with the server
    let sum = sync(&store, "alice", &connector);
    assert!(sum.errors.is_empty(), "{sum:?}");
    let rows = rows_now(&store);
    assert_eq!(subjects(&store, &rows, "Scratch"), ["Scratch one", "Scratch two"]);
    assert_eq!(subjects(&store, &rows, "Scratch2"), ["Scratch three"]);

    // nothing else changed anywhere
    clean_scratch();
    for (i, m) in watched.iter().enumerate() {
        assert_eq!(server_view(m), before[i], "{m} was changed");
    }
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------------------ phase 3: sending (MAIL-160)

const SUBMISSIONS_PORT: u16 = 10465;
const SUBMISSION_PORT: u16 = 10587;
const MAILPIT: &str = "http://127.0.0.1:18025/api/v1";

/// SMTP to Dovecot's submission service, trusting the test CA (test code only).
struct EnvSmtp {
    port: u16,
    security: MailSecurity,
    auth: Arc<dyn AuthFactory>,
}

impl crate::mail::send::SmtpConnector for EnvSmtp {
    fn connect(&self) -> Result<crate::mail::smtp_client::Smtp, crate::mail::smtp_client::SmtpError> {
        let settings = ConnectSettings { host: "localhost".into(), port: self.port, security: self.security, limits: Limits { io_timeout: Duration::from_secs(60), ..Limits::default() } };
        crate::mail::send::connect_with_retry(self.auth.as_ref(), |a| crate::mail::smtp_client::Smtp::connect_test(&settings, ca_cert(), a))
    }
}

fn curl(args: &[&str]) -> Vec<u8> {
    let out = Command::new("curl").arg("-s").args(args).output().expect("curl");
    assert!(out.status.success(), "curl {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

/// What Mailpit (the sink behind the submission service) holds: its JSON listing.
fn sink_messages() -> Vec<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(&curl(&[&format!("{MAILPIT}/messages?limit=200")])).expect("Mailpit answers JSON");
    v["messages"].as_array().cloned().unwrap_or_default()
}

fn clear_sink() {
    curl(&["-X", "DELETE", &format!("{MAILPIT}/messages")]);
}

/// Take the test mails out of a user's Sent folder again (the seeded mailbox must stay as it was).
fn clean_sent(user: &str, tag: &str) {
    let _ = doveadm(&["expunge", "-u", user, "mailbox", "Sent", "header", "Subject", tag], None);
}

/// One whole send against the real servers: compose, outbox, SMTP through Dovecot's submission service into the
/// sink, the Sent copy by APPEND, and what arrived checked byte for byte.
fn send_end_to_end(acct: &MailAccount, smtp: EnvSmtp, imap: EnvConnector, tag: &str) {
    let (dir, store) = temp_store(&format!("send-{tag}"));
    let now = crate::calendar::time::now();
    let subject = format!("Shale send test ✓ Grüße {tag}");
    clean_sent(&acct.user, "Shale send test");
    clear_sink();

    // the folders (Sent is the special-use one) and their numbering are known before sending
    discover_folders(&store, &imap, &acct.id, &acct.name).expect("folders");
    let sum = sync(&store, &acct.id, &imap);
    assert!(sum.errors.is_empty(), "{:?}", sum.errors);
    let sent_before = dovecot_view(&acct.user, "Sent");

    // a mail with a non-ASCII subject and body, a hidden recipient and an attachment of binary bytes
    let bin: Vec<u8> = (0..20_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    let mut d = crate::mail::send::new_draft(acct);
    d.to = "Bob Müller <bob@example.org>".into();
    d.cc = "carol@example.org".into();
    d.bcc = "hidden@example.org".into();
    d.subject = subject.clone();
    d.body = "Hallo Bob,\n\nanbei der Plan. Schöne Grüße aus Köln — ÄÖÜ ß € 日本語\n".into();
    d.attachments.push(crate::mail::compose::Att { name: "Plan Größe.pdf".into(), mime: "application/pdf".into(), data: Arc::new(bin.clone()), ..Default::default() });
    let ob = crate::mail::outbox::Outbox::new(&dir);
    let item = crate::mail::send::queue_draft(&ob, acct, &d, 0, now, &crate::mail::store::today_string()).expect("queued");
    let eml = ob.read_eml(&acct.id, &item.id).unwrap();

    // sent exactly once
    let r = crate::mail::send::send_due(&ob, &store, acct, &smtp, now);
    assert_eq!((r.sent.len(), r.failed.len(), r.unsure.len()), (1, 0, 0), "{r:?}");
    let again = crate::mail::send::send_due(&ob, &store, acct, &smtp, now + 5);
    assert_eq!(again, crate::mail::send::SendReport::default());

    // what arrived in the sink (Mailpit gets it from Dovecot's submission service)
    let mut got = vec![];
    for _ in 0..40 {
        got = sink_messages();
        if !got.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_eq!(got.len(), 1, "exactly one mail arrived: {got:?}");
    let m = &got[0];
    let addrs = |k: &str| -> Vec<String> { m[k].as_array().map(|a| a.iter().filter_map(|x| x["Address"].as_str().map(String::from)).collect()).unwrap_or_default() };
    assert_eq!((addrs("To"), addrs("Cc"), addrs("Bcc")), (vec!["bob@example.org".to_string()], vec!["carol@example.org".to_string()], vec!["hidden@example.org".to_string()]), "the envelope: Bcc got the mail");
    assert_eq!(m["Subject"].as_str(), Some(subject.as_str()), "the non-ASCII subject arrived intact");
    let raw = curl(&[&format!("{MAILPIT}/message/{}/raw", m["ID"].as_str().expect("an id"))]);
    let our_head_end = eml.windows(4).position(|w| w == b"\r\n\r\n").expect("headers end") + 2;
    let our_head = &eml[..our_head_end];
    assert!(!String::from_utf8_lossy(our_head).to_lowercase().contains("bcc"), "Shale's own headers have no Bcc");
    // the headers Shale built arrive byte for byte. The sink (Mailpit) and Dovecot add their own lines in front
    // (Return-Path, Received, and Mailpit shows the envelope's Bcc recipients as a Bcc line): none of that is Shale's.
    let at = raw.windows(our_head.len()).position(|w| w == our_head).expect("Shale's header block arrived unchanged");
    let before = String::from_utf8_lossy(&raw[..at]).to_lowercase();
    let after = String::from_utf8_lossy(&raw[at..]).to_lowercase();
    assert!(!after.contains("bcc:") && !after.contains("hidden@example.org"), "no Bcc in the mail itself:\n{after}");
    assert!(
        before.contains("received:") && before.lines().all(|l| l.starts_with(' ') || l.starts_with('\t') || l.is_empty() || ["return-path:", "received:", "bcc:", "by ", "for "].iter().any(|p| l.starts_with(p))),
        "only the servers' own lines come before: {before}"
    );
    let headers = String::from_utf8_lossy(our_head).to_lowercase();
    assert!(headers.contains(&format!("message-id: {}", item.message_id.to_lowercase())), "our Message-ID survived: {headers}");
    assert!(!headers.contains("user-agent") && !headers.contains("disposition-notification"), "{headers}");
    let parsed = crate::mail::parse(&raw);
    assert_eq!(parsed.subject, subject);
    assert!(parsed.from.contains(&acct.from_address) && parsed.to.contains("Müller") && parsed.to.contains("bob@example.org") && parsed.cc.contains("carol@example.org"), "{parsed:?}");
    assert!(parsed.text.contains("Schöne Grüße aus Köln — ÄÖÜ ß € 日本語"), "{}", parsed.text);
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!((parsed.attachments[0].name.as_str(), parsed.attachments[0].mime.as_str()), ("Plan Größe.pdf", "application/pdf"));
    assert_eq!(parsed.attachments[0].data, bin, "the attachment is intact");
    // the part of the message Shale built is what arrived (the server only adds a Received header)
    let body_of = |b: &[u8]| b[b.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4..].to_vec();
    assert_eq!(body_of(&raw), body_of(&eml), "the body is byte for byte what was in the outbox");

    // the Sent copy: appended once, flagged read, the same bytes; the local row has the server's UID
    let r = ops::run_pending(&store, &imap, &acct.id);
    assert_eq!((r.done, r.failed.len()), (1, 0), "{r:?}");
    let sent_now = dovecot_view(&acct.user, "Sent");
    let ours: Vec<&String> = sent_now.iter().filter(|l| l.contains("Shale send test")).collect();
    assert_eq!(ours.len(), 1, "exactly one copy in Sent: {sent_now:?}");
    assert!(ours[0].contains("\\Seen"), "{}", ours[0]);
    assert_eq!(sent_now.len(), sent_before.len() + 1, "nothing else changed in Sent");
    assert_eq!(ops::run_pending(&store, &imap, &acct.id), ops::OpsReport::default(), "nothing is appended a second time");
    let sum = sync(&store, &acct.id, &imap);
    assert!(sum.errors.is_empty(), "{:?}", sum.errors);
    let rows = store.reader().unwrap().folders(Some(&acct.id)).unwrap();
    let mine: Vec<MessageSummary> = messages(&store, &rows, "Sent").into_iter().filter(|m| m.subject == subject).collect();
    assert_eq!(mine.len(), 1, "one row in the list after a sync: {mine:?}");
    assert_eq!(store.read_raw(&mine[0]).unwrap(), eml, "the stored copy is the exact sent message");
    assert!(sink_messages().len() == 1, "and still only one mail was sent");

    clean_sent(&acct.user, "Shale send test");
    clear_sink();
    let _ = std::fs::remove_dir_all(dir);
}

// spec: MAIL-160
#[test]
#[ignore = "needs the containers: tests/mail-env/mail-env.sh up"]
fn mail_env_password_and_oauth_accounts_send_with_an_attachment_and_dovecot_has_one_sent_copy_each() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    // the password account sends over STARTTLS (587): nothing, AUTH included, goes before the upgrade
    let pw = MailAccount {
        id: "mailenv-send-pw".into(),
        name: "Alice".into(),
        host: "localhost".into(),
        port: IMAPS_PORT,
        user: "alice@shale.test".into(),
        smtp_host: "localhost".into(),
        smtp_port: SUBMISSION_PORT,
        smtp_security: MailSecurity::StartTls,
        from_name: "Alice Ünïcode".into(),
        from_address: "alice@shale.test".into(),
        ..MailAccount::default()
    };
    let auth: Arc<dyn AuthFactory> = Arc::new(PasswordAuth { user: pw.user.clone(), password: "shale-test-pw".into() });
    send_end_to_end(&pw, EnvSmtp { port: SUBMISSION_PORT, security: MailSecurity::StartTls, auth: auth.clone() }, EnvConnector { auth: auth.clone() }, &format!("pw{}", std::process::id()));
    // a wrong password is an authentication error from the real submission service
    let bad = EnvSmtp { port: SUBMISSIONS_PORT, security: MailSecurity::ImplicitTls, auth: Arc::new(PasswordAuth { user: pw.user.clone(), password: "nope".into() }) };
    assert!(matches!(crate::mail::send::SmtpConnector::connect(&bad).err(), Some(crate::mail::smtp_client::SmtpError::Auth(_))));
    // the production certificate check refuses the test CA, as for IMAP
    let mut a = auth.authenticator().unwrap();
    let settings = ConnectSettings { host: "localhost".into(), port: SUBMISSIONS_PORT, security: MailSecurity::ImplicitTls, limits: Limits::default() };
    assert!(matches!(crate::mail::smtp_client::Smtp::connect(&settings, a.as_mut()).err(), Some(crate::mail::smtp_client::SmtpError::Tls(_))));

    // the OAuth2 account sends over implicit TLS (465) with the token of its sign-in
    let mut oa = oauth_account();
    oa.id = "mailenv-send-oauth".into();
    oa.smtp_host = "localhost".into();
    oa.smtp_port = SUBMISSIONS_PORT;
    oa.from_address = "oauth@shale.test".into();
    secrets::clear_mail_secret(&oa.id, MailSecret::RefreshToken);
    oauth::forget_source(&oa.id);
    sign_in_headless(&oa).expect("the sign-in");
    let source = oauth::shared_source(&oa.id, OAuthConfig::for_account(&oa, None).unwrap());
    let factory: Arc<dyn AuthFactory> = Arc::new(OAuthFactory { user: oa.user.clone(), host: "localhost".into(), port: SUBMISSIONS_PORT, source: source.clone() });
    let imap_factory: Arc<dyn AuthFactory> = Arc::new(OAuthFactory { user: oa.user.clone(), host: "localhost".into(), port: IMAPS_PORT, source });
    send_end_to_end(&oa, EnvSmtp { port: SUBMISSIONS_PORT, security: MailSecurity::ImplicitTls, auth: factory }, EnvConnector { auth: imap_factory }, &format!("oauth{}", std::process::id()));
    secrets::clear_mail_secret(&oa.id, MailSecret::RefreshToken);
    oauth::forget_source(&oa.id);
}

// ------------------------------------------------------------ phase 4: signed and encrypted mail (MAIL-198)

/// One signed and encrypted mail of one standard through the real submission service into the sink, and the copy in Dovecot's Sent.
fn crypto_end_to_end(std: crate::config::CryptoStandard, tag: &str) {
    use crate::config::CryptoStandard;
    use crate::mail::crypto::{process, Encryption};
    use crate::mail::gpg::testkit::{no_cancel, TestHome};
    let (alice, bob, carol) = (TestHome::new(), TestHome::new(), TestHome::new());
    let (a_key, b_key, c_key) = match std {
        CryptoStandard::OpenPgp => {
            let (a, b, c) = (alice.gen_pgp("Alice Shale", "alice@shale.test", "never", None), bob.gen_pgp("Bob", "bob@example.org", "never", None), carol.gen_pgp("Carol", "carol@example.org", "never", None));
            alice.share_pgp_with(&bob, &a);
            alice.share_pgp_with(&carol, &a);
            bob.share_pgp_with(&alice, &b);
            carol.share_pgp_with(&alice, &c);
            (a, b, c)
        }
        CryptoStandard::Smime => {
            let (a, b) = (alice.gen_smime("Alice Shale", "alice@shale.test"), bob.gen_smime("Bob", "bob@example.org"));
            alice.share_smime_with(&bob, &a);
            bob.share_smime_with(&alice, &b);
            (a, b, String::new())
        }
    };
    let mut acct = MailAccount {
        id: format!("mailenv-crypto-{tag}"),
        name: "Alice".into(),
        host: "localhost".into(),
        port: IMAPS_PORT,
        user: "alice@shale.test".into(),
        smtp_host: "localhost".into(),
        smtp_port: SUBMISSION_PORT,
        smtp_security: MailSecurity::StartTls,
        from_name: "Alice Shale".into(),
        from_address: "alice@shale.test".into(),
        ..MailAccount::default()
    };
    match std {
        CryptoStandard::OpenPgp => acct.pgp_key = a_key.clone(),
        CryptoStandard::Smime => acct.smime_key = a_key.clone(),
    }
    let auth: Arc<dyn AuthFactory> = Arc::new(PasswordAuth { user: acct.user.clone(), password: "shale-test-pw".into() });
    let smtp = EnvSmtp { port: SUBMISSION_PORT, security: MailSecurity::StartTls, auth: auth.clone() };
    let imap = EnvConnector { auth: auth.clone() };
    let (dir, store) = temp_store(&format!("crypto-{tag}"));
    let now = crate::calendar::time::now();
    let subject = format!("Shale crypto test ✓ {tag}");
    clean_sent(&acct.user, "Shale crypto test");
    clear_sink();
    discover_folders(&store, &imap, &acct.id, &acct.name).expect("folders");
    let sum = sync(&store, &acct.id, &imap);
    assert!(sum.errors.is_empty(), "{:?}", sum.errors);
    let sent_before = dovecot_view(&acct.user, "Sent");

    // signed and encrypted for Bob (To), and with OpenPGP for Carol on Bcc (a hidden recipient)
    let mut d = crate::mail::send::new_draft(&acct);
    d.to = "Bob <bob@example.org>".into();
    if std == CryptoStandard::OpenPgp {
        d.bcc = "carol@example.org".into();
    }
    d.subject = subject.clone();
    let marker = format!("zq{}x{}", std::process::id(), if std == CryptoStandard::OpenPgp { "pgp" } else { "smime" });
    let secret = format!("the secret plan {marker} — Grüße aus Köln");
    d.body = format!("{secret}\n");
    (d.sign, d.encrypt, d.standard) = (true, true, std);
    let ob = crate::mail::outbox::Outbox::new(&dir);
    let item = crate::mail::send::queue_with(&ob, &alice.g, &acct, &d, crate::mail::send::SendTiming { undo_secs: 0, now, date: &crate::mail::store::today_string() }, &no_cancel()).expect("signed and encrypted");
    let eml = ob.read_eml(&acct.id, &item.id).unwrap();
    assert!(!String::from_utf8_lossy(&eml).contains(&marker), "nothing readable in the outbox");
    let r = crate::mail::send::send_due(&ob, &store, &acct, &smtp, now);
    assert_eq!((r.sent.len(), r.failed.len(), r.unsure.len()), (1, 0, 0), "{r:?}");

    // what the sink got from Dovecot's submission service: our message, byte for byte after the servers' own lines
    let mut got = vec![];
    for _ in 0..40 {
        got = sink_messages();
        if !got.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_eq!(got.len(), 1, "exactly one mail arrived: {got:?}");
    let raw = curl(&[&format!("{MAILPIT}/message/{}/raw", got[0]["ID"].as_str().expect("an id"))]);
    let at = raw.windows(8).position(|w| w == &eml[..8]).expect("our message starts somewhere in what arrived");
    let arrived = &raw[at..];
    assert_eq!(arrived, &eml[..], "the signed and encrypted message arrived unchanged: a relay that touched a byte would break it");

    // every recipient reads it and the signature is Alice's
    let mut readers: Vec<(&str, &TestHome)> = vec![("bob", &bob)];
    if std == CryptoStandard::OpenPgp {
        readers.push(("carol (Bcc)", &carol));
    }
    for (name, h) in readers {
        let o = process(&h.g, arrived, true, &no_cancel(), now);
        assert_eq!(o.security.encryption, Encryption::Decrypted(std), "{name}: {:?}", o.security);
        assert_eq!(o.security.lines.len(), 1, "{name}: {:?}", o.security);
        assert_eq!((o.security.lines[0].sig.state, o.security.lines[0].from_ok), (crate::mail::gpg::SigState::Good, Some(true)), "{name}: {:?}", o.security);
        assert_eq!(crate::mail::parse(o.display.as_ref().unwrap()).text, secret, "{name}");
    }
    let _ = (b_key, c_key);

    // the copy in Dovecot's Sent folder: appended once, and Alice can read it (the mail was also encrypted to her)
    let r = ops::run_pending(&store, &imap, &acct.id);
    assert_eq!((r.done, r.failed.len()), (1, 0), "{r:?}");
    let sent_now = dovecot_view(&acct.user, "Sent");
    assert_eq!(sent_now.iter().filter(|l| l.contains("Shale crypto test")).count(), 1, "{sent_now:?}");
    assert_eq!(sent_now.len(), sent_before.len() + 1);
    let sum = sync(&store, &acct.id, &imap);
    assert!(sum.errors.is_empty(), "{:?}", sum.errors);
    let rows = store.reader().unwrap().folders(Some(&acct.id)).unwrap();
    let mine: Vec<MessageSummary> = messages(&store, &rows, "Sent").into_iter().filter(|m| m.subject == subject).collect();
    assert_eq!(mine.len(), 1, "{mine:?}");
    let stored = store.read_raw(&mine[0]).unwrap();
    assert_eq!(stored, eml, "the copy from Dovecot is the exact sent message");
    let o = process(&alice.g, &stored, true, &no_cancel(), now);
    assert_eq!(o.security.encryption, Encryption::Decrypted(std), "{:?}", o.security);
    assert_eq!(crate::mail::parse(o.display.as_ref().expect("the Sent copy decrypts")).text, secret);
    // and storing it: the decrypted copy lands beside the original, searchable
    store.save_decrypted(&mine[0], o.display.as_ref().unwrap(), &o.security, &crate::mail::store::today_string()).unwrap();
    assert_eq!(store.reader().unwrap().search(&marker, None, 5, 0).unwrap().len(), 1, "the text of the encrypted Sent mail is found by search");

    clean_sent(&acct.user, "Shale crypto test");
    clear_sink();
    let _ = std::fs::remove_dir_all(dir);
}

// spec: MAIL-198
#[test]
#[ignore = "needs the containers: tests/mail-env/mail-env.sh up"]
fn mail_env_signed_and_encrypted_openpgp_and_smime_mail_arrives_intact_verifies_decrypts_and_the_sent_copy_decrypts() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    if !crate::mail::gpg::testkit::have_gnupg() {
        panic!("gpg, gpgsm and gpgconf are needed for this test");
    }
    crypto_end_to_end(crate::config::CryptoStandard::OpenPgp, &format!("pgp{}", std::process::id()));
    crypto_end_to_end(crate::config::CryptoStandard::Smime, &format!("smime{}", std::process::id()));
}
