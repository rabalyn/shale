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
use crate::imap_client::{Client, ConnectSettings, ImapError, Limits};
use crate::imap_sync::{connect_with_retry, discover_folders, fetch_folder_listing, sync_account, AuthFactory, Connector, PasswordAuth, SyncOptions};
use crate::mail_db::{FolderRow, MessageSummary, STATE_FULL, STATE_HEADER};
use crate::mail_store::MailStore;
use crate::oauth::{self, OAuthConfig, OAuthFactory};
use crate::oauth_fake_tests::{http_get, http_post_form};
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

fn sync(store: &MailStore, id: &str, connector: &dyn Connector) -> crate::imap_sync::SyncSummary {
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
    // which `oauth.rs` tests cover with its own pretend provider)
    assert!(oauth::needs_sign_in(text) && text.contains("Authentication failed"), "{text}");

    // signing in again works
    sign_in_headless(&acct).expect("signing in again");
    connector.connect().expect("works again").logout();

    secrets::clear_mail_secret(&acct.id, MailSecret::RefreshToken);
    oauth::forget_source(&acct.id);
    let _ = std::fs::remove_dir_all(dir);
}
