//! UI tests of the mail view and the account settings (MAIL-040…MAIL-071): the real app is run
//! frame by frame with egui, real controls are clicked and real keys are pressed, and the mail
//! server is the pretend one of `imap_fake_tests.rs` (real TCP and TLS). The keyring is the
//! pretend one of `secrets.rs`.

use crate::app::{Modal, Options, ShaleApp, View};
use crate::config::{MailAccount, MailSecurity};
use crate::imap_client::{Client, ConnectSettings, ImapError, Limits, PlainAuth};
use crate::imap_fake_tests::{server_cert, FakeServer};
use crate::imap_sync::{Connector, ConnectorFactory, SyncOptions, NO_PASSWORD};
use crate::mail_db::{STATE_FULL, STATE_HEADER, STATE_TOO_LARGE};
use crate::mail_ui::{Body, ListingState, Pane, TestState};
use crate::secrets::{self, MailSecret};
use eframe::egui::{self, Color32, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, Vec2};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 2026-11-14 12:00 UTC: the sync window is counted from here.
const NOW: i64 = 1_794_657_600;
const PASSWORD: &str = "secret1";

struct TestConnector {
    settings: ConnectSettings,
    user: String,
    password: String,
    /// While set, the server cannot be reached (the network is down).
    offline: Arc<std::sync::atomic::AtomicBool>,
}

impl Connector for TestConnector {
    fn connect(&self) -> Result<Client, ImapError> {
        if self.offline.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(ImapError::Connection("the network is down".into()));
        }
        let mut a = PlainAuth::new(&self.user, &self.password);
        Client::connect_test(&self.settings, server_cert(), &mut a)
    }
}

struct OAuthTestConnector {
    settings: ConnectSettings,
    auth: Arc<crate::oauth::OAuthFactory>,
}

impl Connector for OAuthTestConnector {
    fn connect(&self) -> Result<Client, ImapError> {
        crate::imap_sync::connect_with_retry(self.auth.as_ref(), |a| Client::connect_test(&self.settings, server_cert(), a))
    }
}

/// A connector factory like the real one (the password is the session's or the keyring's), but
/// for the pretend server's certificate.
fn factory(offline: Arc<std::sync::atomic::AtomicBool>) -> ConnectorFactory {
    Arc::new(move |acct: &MailAccount, session: Option<crate::secret::Secret>| {
        if acct.auth == crate::config::MailAuth::OAuth2 {
            // like `connector_for_account`, but trusting the pretend server's certificate
            let cfg = crate::oauth::OAuthConfig::for_account(acct, session.or_else(|| secrets::mail_secret(&acct.id, MailSecret::ClientSecret))).map_err(|e| e.to_string())?;
            let source = crate::oauth::shared_source(&acct.id, cfg);
            let auth = Arc::new(crate::oauth::OAuthFactory { user: acct.user.clone(), host: acct.host.clone(), port: acct.port, source });
            let settings = ConnectSettings { host: acct.host.clone(), port: acct.port, security: acct.security, limits: Limits { io_timeout: Duration::from_secs(15), ..Limits::default() } };
            return Ok(Arc::new(OAuthTestConnector { settings, auth }) as Arc<dyn Connector>);
        }
        let password = session.or_else(|| secrets::mail_secret(&acct.id, MailSecret::Password)).ok_or_else(|| NO_PASSWORD.to_string())?;
        let settings = ConnectSettings { host: acct.host.clone(), port: acct.port, security: acct.security, limits: Limits { io_timeout: Duration::from_secs(15), max_message: 20_000, ..Limits::default() } };
        Ok(Arc::new(TestConnector { settings, user: acct.user.clone(), password: password.expose().to_string(), offline: offline.clone() }) as Arc<dyn Connector>)
    })
}

fn dated(subject: &str, from: &str, body: &str, day: u32) -> Vec<u8> {
    format!("From: {from}\r\nTo: ada@example.com\r\nSubject: {subject}\r\nDate: {day:02} Nov 2026 10:22:33 +0100\r\nMessage-ID: <{day}-{}@fake>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n", subject.replace(' ', "-"))
        .into_bytes()
}

fn with_attachment(day: u32) -> Vec<u8> {
    format!("From: Carol <carol@x.de>\r\nTo: ada@example.com\r\nSubject: Report attached\r\nDate: {day:02} Nov 2026 09:00:00 +0100\r\nMessage-ID: <report@fake>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSee the attached report.\r\n--b1\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\nContent-Disposition: attachment; filename=\"report.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8gUERG\r\n--b1--\r\n").into_bytes()
}

fn seed(server: &FakeServer) {
    let mut s = server.state();
    s.password = PASSWORD.into();
    s.add_folder("INBOX", &[]);
    s.add_folder("Sent", &["\\Sent"]);
    s.add_folder("Trash", &["\\Trash"]);
    s.add_folder("[Gmail]/All Mail", &["\\All"]);
    s.add_message("INBOX", dated("Invoice 2026", "Bob <bob@x.de>", "Please pay the invoice for the lecture hall.", 10), NOW - 4 * 86_400);
    s.add_message("INBOX", dated("Lunch plans", "Dora <dora@x.de>", "Pizza at noon, projector later.", 11), NOW - 3 * 86_400);
    s.add_message("INBOX", with_attachment(12), NOW - 2 * 86_400);
    s.add_message("Sent", dated("Re: Invoice 2026", "ada@example.com", "Paid, thanks.", 13), NOW - 86_400);
    s.add_message("Trash", dated("Old newsletter", "News <n@x.de>", "Unsubscribe me.", 5), NOW - 9 * 86_400);
    s.add_message("Trash", dated("Spam offer", "Spam <s@x.de>", "Buy now, a lot of text to be loaded.", 6), NOW - 8 * 86_400);
    s.add_message("[Gmail]/All Mail", dated("Invoice 2026", "Bob <bob@x.de>", "copy", 10), NOW - 4 * 86_400);
}

struct W {
    app: ShaleApp,
    ctx: egui::Context,
    t: f64,
    server: FakeServer,
    dir: PathBuf,
    account: MailAccount,
    /// Set it and the pretend server cannot be reached.
    offline: Arc<std::sync::atomic::AtomicBool>,
    /// What the last frame painted: text and its colour.
    texts: Vec<(String, Color32)>,
    /// What the last frame filled: rectangles and their colour.
    fills: Vec<(Rect, Color32)>,
}

impl W {
    /// An app on a small vault with the mail view open. `with_account`: the account of the
    /// pretend server is set up with its password in the keyring.
    fn new(tag: &str, with_account: bool) -> W {
        let dir = std::env::temp_dir().join(format!("shale-mailui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Welcome.md"), "# Welcome\n").unwrap();
        let server = FakeServer::start(MailSecurity::ImplicitTls);
        seed(&server);
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.mail_sync_minutes = 0;
        // marking read when a message is opened is tested on its own; elsewhere the timer must not write
        app.cfg.mail_mark_read = false;
        let offline = Arc::new(std::sync::atomic::AtomicBool::new(false));
        app.mail.factory = factory(offline.clone());
        // no test may reach a real SMTP server: the sending tests plug in the pretend one
        app.mail.smtp_factory = Arc::new(|_, _| Err("no SMTP server in this test".to_string()));
        app.mail.opts = SyncOptions { now: Some(NOW), batch_size: 2, ..SyncOptions::default() };
        let account = MailAccount { id: format!("t-{tag}"), name: "Test mail".into(), host: "localhost".into(), port: server.port, user: "ada".into(), ..MailAccount::default() };
        if with_account {
            app.cfg.mail_accounts = vec![account.clone()];
            assert!(secrets::set_mail_secret(&account.id, MailSecret::Password, PASSWORD));
        }
        app.show_view(View::Mail, false);
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut w = W { app, ctx, t: 0.0, server, dir, account, offline, texts: vec![], fills: vec![] };
        w.until("the mail store opens", |a| a.mail.ready_store().is_some());
        w
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500.0, 900.0))), time: Some(self.t), events, ..Default::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
        out.textures_delta.clear();
        let mut texts = vec![];
        let mut fills = vec![];
        fn walk(s: &Shape, out: &mut Vec<(String, Color32)>, fills: &mut Vec<(Rect, Color32)>) {
            match s {
                Shape::Vec(v) => v.iter().for_each(|s| walk(s, out, fills)),
                Shape::Text(t) => out.push((t.galley.job.text.clone(), t.galley.job.sections.first().map(|s| s.format.color).unwrap_or(Color32::WHITE))),
                Shape::Rect(r) => fills.push((r.rect, r.fill)),
                _ => {}
            }
        }
        for c in &out.shapes {
            walk(&c.shape, &mut texts, &mut fills);
        }
        self.texts = texts;
        self.fills = fills;
    }

    fn until(&mut self, what: &str, f: impl Fn(&ShaleApp) -> bool) {
        let start = Instant::now();
        loop {
            self.frame(vec![]);
            if f(&self.app) {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(40), "timed out waiting for: {what}");
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    /// Let a dialog that just opened finish its first (invisible) sizing frames.
    fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }

    fn shows(&self, part: &str) -> bool {
        self.texts.iter().any(|(t, _)| t.contains(part))
    }

    fn rect(&self, key: &str) -> Rect {
        let found = match &self.app.modal {
            Some(Modal::MailAccounts(d)) => d.rects.iter().find(|r| r.0 == key).map(|r| r.1),
            Some(Modal::Import(d)) => d.rects.iter().find(|r| r.0 == key).map(|r| r.1),
            _ => None,
        };
        found.or_else(|| self.app.mail.rects.iter().rev().find(|r| r.0 == key).map(|r| r.1)).unwrap_or_else(|| {
            let have: Vec<&String> = self.app.mail.rects.iter().map(|r| &r.0).collect();
            let dlg: Vec<String> = match &self.app.modal {
                Some(Modal::MailAccounts(d)) => d.rects.iter().map(|r| r.0.clone()).collect(),
                _ => vec![],
            };
            panic!("no control called {key:?} on screen; mail controls: {have:?}; dialog: {dlg:?}; modal open: {}", self.app.modal.is_some())
        })
    }

    fn has(&self, key: &str) -> bool {
        self.app.mail.rects.iter().any(|r| r.0 == key) || matches!(&self.app.modal, Some(Modal::MailAccounts(d)) if d.rects.iter().any(|r| r.0 == key))
    }

    fn click_at(&mut self, p: Pos2, modifiers: Modifiers) {
        let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers };
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![b(true)]);
        self.frame(vec![b(false)]);
        self.frame(vec![]);
    }

    fn click(&mut self, key: &str) {
        let mut r = self.rect(key);
        if self.app.modal.is_some() {
            // a dialog is centred and may still be changing its size: wait until the control
            // stays where it is
            let mut same = 0;
            for _ in 0..40 {
                self.frame(vec![]);
                let now = self.rect(key);
                same = if now == r { same + 1 } else { 0 };
                r = now;
                if same >= 3 {
                    break;
                }
            }
        }
        self.click_at(r.center(), Modifiers::NONE);
    }

    /// Click into a field, select what is in it and type.
    fn typ(&mut self, key: &str, text: &str) {
        self.click(key);
        self.frame(vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
        self.frame(vec![Event::Text(text.into())]);
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        let text = match key {
            Key::J => Some("j"),
            Key::K => Some("k"),
            Key::I => Some("i"),
            Key::Slash => Some("/"),
            Key::Space => Some(" "),
            Key::U => Some("u"),
            Key::S => Some("s"),
            Key::F => Some("f"),
            Key::D => Some("d"),
            Key::M => Some("m"),
            Key::T => Some("t"),
            Key::A => Some("a"),
            _ => None,
        };
        let mut ev = vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }];
        if let (Some(t), false) = (text, modifiers.command) {
            ev.push(Event::Text(t.into()));
        }
        self.frame(ev);
        self.frame(vec![]);
    }

    fn type_text(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.into())]);
    }

    fn sync_and_wait(&mut self) {
        let id = self.account.id.clone();
        self.until("the sync button", |a| a.mail.rects.iter().any(|r| r.0 == "mail:sync"));
        self.click("mail:sync");
        self.until("the sync ends", move |a| a.mail.syncs.get(&id).is_some_and(|r| r.finished.is_some()));
    }

    /// Wait until every action of the person has been sent (or given up).
    fn settle_ops(&mut self) {
        self.until("the actions were sent", |a| a.mail.pending_total == 0 && !a.mail.busy_with_ops());
    }

    fn pending_is_empty(&self) -> bool {
        self.app.mail.pending_total == 0
    }

    /// The flags of a mail on the server, sorted.
    fn server_flags(&self, folder: &str, uid: u32) -> Vec<String> {
        let mut v = self.server.state().messages(folder).into_iter().find(|m| m.0 == uid).map(|m| m.1).unwrap_or_default();
        v.sort();
        v
    }

    fn server_uids(&self, folder: &str) -> Vec<u32> {
        self.server.state().messages(folder).iter().map(|m| m.0).collect()
    }

    fn folder_id(&self, name: &str) -> i64 {
        self.app.mail.folders.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no folder {name}")).id
    }

    fn open_folder(&mut self, name: &str) {
        let id = self.folder_id(name);
        self.until("the folder is listed", move |a| a.mail.rects.iter().any(|r| r.0 == format!("mail:folder:{id}")));
        self.click(&format!("mail:folder:{id}"));
    }

    fn subjects(&mut self) -> Vec<String> {
        let n = self.app.mail.list_len();
        self.app.mail.ensure_rows(0..n);
        (0..n).filter_map(|i| self.app.mail.row(i).map(|m| m.subject.clone())).collect()
    }

    fn row_of(&mut self, subject: &str) -> usize {
        self.subjects().iter().position(|s| s == subject).unwrap_or_else(|| panic!("no mail {subject:?} in the list"))
    }

    fn open_row(&mut self, i: usize) {
        self.until("the row is on screen", move |a| a.mail.rects.iter().any(|r| r.0 == format!("mail:row:{i}")));
        self.click(&format!("mail:row:{i}"));
    }

    fn open_mail(&mut self, subject: &str) {
        let i = self.row_of(subject);
        self.open_row(i);
        self.until("the message is read", |a| matches!(a.mail.open.as_ref().map(|o| &o.body), Some(Body::Ready(_))));
    }

    fn vault_files(&self) -> Vec<String> {
        fn walk(d: &Path, root: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                    continue;
                }
                if p.is_dir() {
                    walk(&p, root, out);
                } else {
                    out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
                }
            }
        }
        let mut v = vec![];
        walk(&self.dir, &self.dir, &mut v);
        v.sort();
        v
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_dir_all(&self.app.mail.root);
        secrets::clear_mail_secret(&self.account.id, MailSecret::Password);
    }
}

// spec: MAIL-040, MAIL-041, MAIL-042, MAIL-043
#[test]
fn an_account_is_added_in_the_dialog_tested_and_saved_with_its_password_in_the_keyring() {
    let mut w = W::new("dialog", false);
    w.until("the empty view", |a| a.mail.rects.iter().any(|r| r.0 == "mail:add-account"));
    w.click("mail:add-account");
    assert!(matches!(w.app.modal, Some(Modal::MailAccounts(_))), "the button opens the account settings");
    w.settle();
    w.click("md:add");
    w.typ("md:name", "Work mail");
    w.typ("md:host", "localhost");
    w.typ("md:user", "ada");
    w.typ("md:password", PASSWORD);
    // the port follows the security mode while it is still the usual one; the mode cannot be plain text
    let port = |w: &W| match &w.app.modal {
        Some(Modal::MailAccounts(d)) => d.ports[0].clone(),
        _ => panic!("no dialog"),
    };
    assert_eq!(port(&w), "993");
    w.click("md:sec:StartTls");
    assert_eq!(port(&w), "143");
    w.click("md:sec:ImplicitTls");
    assert_eq!(port(&w), "993");
    assert!(!w.shows("Plain"), "there is no plain-text choice");
    w.typ("md:port", &w.server.port.to_string());
    if let Some(Modal::MailAccounts(d)) = &w.app.modal {
        assert_eq!(d.accounts[0].auth, crate::config::MailAuth::Password, "a new account signs in with a password until OAuth2 is chosen");
    }
    w.frame(vec![]);
    assert!(w.shows("OAuth2 (in the browser)") && w.has("md:auth-oauth2"), "OAuth2 is offered");

    // "Test connection" runs as a job, says what it found and records the folders
    w.click("md:test");
    let id = match &w.app.modal {
        Some(Modal::MailAccounts(d)) => d.accounts[0].id.clone(),
        _ => unreachable!(),
    };
    let id2 = id.clone();
    w.until("the test ends", move |a| matches!(a.mail.tests.get(&id2), Some(TestState::Ok(_)) | Some(TestState::Failed(_))));
    let t = w.app.mail.tests.get(&id).unwrap();
    assert!(
        matches!(t, TestState::Ok(m) if m.contains("4 folders")),
        "{}",
        match t {
            TestState::Failed(e) => e.clone(),
            _ => "?".into(),
        }
    );
    w.frame(vec![]);
    assert!(w.shows("never synced automatically; loaded when you open it"), "Trash is explained");
    let by_name = |w: &W, n: &str| w.app.mail.folders.iter().find(|f| f.name == n).unwrap().clone();
    assert!(by_name(&w, "INBOX").sync_enabled && !by_name(&w, "Trash").sync_enabled && !by_name(&w, "[Gmail]/All Mail").sync_enabled, "Trash and All Mail start unticked");
    // untick Sent: saved in the index at once
    let sent = by_name(&w, "Sent").id;
    w.click(&format!("md:folder:{sent}"));
    assert!(!by_name(&w, "Sent").sync_enabled);
    assert!(!w.app.mail.reader_for_tests().unwrap().folders(None).unwrap().iter().find(|f| f.id == sent).unwrap().sync_enabled, "the index has it");
    w.click(&format!("md:folder:{sent}"));
    assert!(by_name(&w, "Sent").sync_enabled);

    w.click("md:done");
    assert!(w.app.modal.is_none());
    assert_eq!(w.app.cfg.mail_accounts.len(), 1);
    let a = &w.app.cfg.mail_accounts[0];
    assert_eq!((a.name.as_str(), a.host.as_str(), a.user.as_str(), a.port), ("Work mail", "localhost", "ada", w.server.port));
    let id3 = id.clone();
    w.until("the keyring has the password", move |_| secrets::mail_secret(&id3, MailSecret::Password).is_some());
    assert_eq!(secrets::mail_secret(&id, MailSecret::Password).as_ref().map(|s| s.expose()), Some(PASSWORD));
    let json = serde_json::to_string(&w.app.cfg).unwrap();
    assert!(!json.contains(PASSWORD), "the password is never in the settings file");
    // the new account is synced right away
    let id4 = id.clone();
    w.until("the first sync ends", move |a| a.mail.syncs.get(&id4).is_some_and(|r| r.finished.is_some()));
    assert!(w.app.mail.syncs[&id].errors.is_empty(), "{:?}", w.app.mail.syncs[&id].errors);
    secrets::clear_mail_secret(&id, MailSecret::Password);
}

// spec: MAIL-041
#[test]
fn without_a_keyring_the_password_stays_in_memory_and_the_dialog_says_so() {
    let mut w = W::new("nokeyring", false);
    let mut acct = w.account.clone();
    acct.id = secrets::NO_KEYRING_ACCOUNT.into();
    w.app.modal = Some(Modal::MailAccounts(Box::new(crate::mail_dialog::MailDialog::new(vec![acct.clone()], 0, false))));
    w.settle();
    assert!(w.shows("No system keyring was found"), "the dialog says there is no keyring");
    assert!(w.app.input_is_secret(), "the key recording writes nothing while the dialog is open");
    w.typ("md:password", PASSWORD);
    w.click("md:done");
    let id = acct.id.clone();
    w.until("the password is in memory", move |a| a.mail.session_pw.contains_key(&id));
    w.until("the person is told", |a| a.toasts.iter().any(|(t, _)| t.contains("No keyring")));
    assert_eq!(secrets::mail_secret(&acct.id, MailSecret::Password), None, "nothing in the keyring");
    assert!(!serde_json::to_string(&w.app.cfg).unwrap().contains(PASSWORD), "nothing in the settings file");
    // it works for this session
    let id = acct.id.clone();
    w.until("the sync ends", move |a| a.mail.syncs.get(&id).is_some_and(|r| r.finished.is_some()));
    assert!(w.app.mail.syncs[&acct.id].errors.is_empty(), "{:?}", w.app.mail.syncs[&acct.id].errors);
}

// spec: MAIL-043
#[test]
fn removing_an_account_clears_its_password_and_keeps_its_mail() {
    let mut w = W::new("remove", true);
    w.sync_and_wait();
    let root = w.app.mail.root.clone();
    let emls = |root: &Path| -> usize {
        fn count(d: &Path) -> usize {
            std::fs::read_dir(d).map(|rd| rd.flatten().map(|e| if e.path().is_dir() { count(&e.path()) } else { (e.path().extension().is_some_and(|x| x == "eml")) as usize }).sum()).unwrap_or(0)
        }
        count(root)
    };
    let before = emls(&root);
    assert!(before >= 4, "{before} mails were copied");
    w.app.open_mail_settings();
    w.settle();
    w.click("md:remove");
    w.click("md:done");
    assert!(w.app.cfg.mail_accounts.is_empty());
    let id = w.account.id.clone();
    w.until("the secret is cleared", move |_| secrets::mail_secret(&id, MailSecret::Password).is_none());
    assert_eq!(emls(&root), before, "the local copy stays");
    assert!(w.app.toasts.iter().any(|(t, _)| t.contains("stays in")), "{:?}", w.app.toasts);
}

// spec: MAIL-050, MAIL-051, MAIL-052, MAIL-053, MAIL-054, MAIL-058
#[test]
fn sync_fills_the_folders_the_list_and_the_reading_pane_and_attachments_are_saved_to_the_vault() {
    let mut w = W::new("read", true);
    w.sync_and_wait();
    assert!(w.app.mail.syncs[&w.account.id].errors.is_empty());
    // the folder tree shows the account and the unread count of the Inbox
    w.frame(vec![]);
    assert!(w.shows("INBOX  (3)"), "{:?}", w.texts.iter().map(|t| &t.0).collect::<Vec<_>>());
    assert!(w.shows("Test mail"));
    // the Inbox is open by itself; newest first
    assert_eq!(w.subjects(), vec!["Report attached", "Lunch plans", "Invoice 2026"]);
    // open a message: headers, plain text
    w.open_mail("Invoice 2026");
    w.frame(vec![]);
    assert!(w.shows("Please pay the invoice for the lecture hall."));
    assert!(w.shows("Bob <bob@x.de>") && w.shows("Invoice 2026"));
    // a message with an attachment: listed, saved into the vault on request
    w.open_mail("Report attached");
    w.frame(vec![]);
    assert!(w.shows("report.pdf") && w.shows("1 attachment"));
    let before = w.vault_files();
    w.click("mail:att-save:0");
    w.frame(vec![]);
    let saved: Vec<String> = w.vault_files().into_iter().filter(|f| !before.contains(f)).collect();
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert!(saved[0].ends_with("report.pdf"), "{saved:?}");
    assert_eq!(std::fs::read(w.dir.join(&saved[0])).unwrap(), b"Hello PDF");
    // the sync only used read-only commands
    let verbs = w.server.state().verbs();
    assert!(verbs.iter().all(|v| !v.starts_with("SELECT") && !v.starts_with("STORE") && !v.starts_with("EXPUNGE") && !v.starts_with("COPY")), "{verbs:?}");
}

// spec: MAIL-053
#[test]
fn html_mail_is_shown_as_plain_text_and_never_loads_anything() {
    let mut w = W::new("html", true);
    {
        let mut s = w.server.state();
        let html = "From: Eve <eve@x.de>\r\nTo: ada@example.com\r\nSubject: Offer\r\nDate: 14 Nov 2026 08:00:00 +0100\r\nMessage-ID: <html@fake>\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Hello <b>there</b></p><img src=\"http://tracker.example/x.png\"><script>alert(1)</script>\r\n";
        s.add_message("INBOX", html.as_bytes().to_vec(), NOW);
    }
    w.sync_and_wait();
    w.open_mail("Offer");
    w.frame(vec![]);
    assert!(w.shows("Hello"));
    let shown = match &w.app.mail.open.as_ref().unwrap().body {
        Body::Ready(l) => l.shown.clone(),
        _ => unreachable!(),
    };
    assert!(!shown.contains('<') && !shown.contains("tracker") && !shown.contains("alert"), "{shown:?}");
    // …and nothing but the mail server was contacted (the pretend server saw only IMAP commands)
    assert!(w.server.state().verbs().iter().all(|v| !v.contains("http")));
}

// spec: MAIL-053
#[test]
fn a_very_long_text_is_cut_with_a_note() {
    let long = "x".repeat(crate::mail_ui::TEXT_CAP + 10);
    let (shown, cut) = crate::mail_ui::cut_text(&long);
    assert!(cut && shown.chars().count() == crate::mail_ui::TEXT_CAP);
    let (short, cut) = crate::mail_ui::cut_text("ä short");
    assert!(!cut && short == "ä short");
    // never in the middle of a character
    let umlauts = "ä".repeat(crate::mail_ui::TEXT_CAP + 5);
    let (shown, cut) = crate::mail_ui::cut_text(&umlauts);
    assert!(cut && shown.chars().count() == crate::mail_ui::TEXT_CAP);
}

// spec: MAIL-056, MAIL-055
#[test]
fn opening_trash_lists_it_on_demand_and_a_message_is_loaded_with_its_button() {
    let mut w = W::new("trash", true);
    w.sync_and_wait();
    let trash = w.folder_id("Trash");
    assert!(w.subjects().len() == 3, "the Inbox is open and Trash was not synced");
    assert_eq!(w.app.mail.counts.get(&trash), None, "Trash has no mail yet");
    let listed_before = w.server.state().verbs().iter().filter(|v| v.starts_with("EXAMINE \"Trash\"")).count();
    assert_eq!(listed_before, 0, "the sync did not look into Trash");
    w.open_folder("Trash");
    assert!(matches!(w.app.mail.listings.get(&trash), Some(ListingState::Loading) | Some(ListingState::Done(_))), "opening Trash starts the listing");
    w.until("the listing arrives", move |a| matches!(a.mail.listings.get(&trash), Some(ListingState::Done(_))) && a.mail.total == 2);
    assert_eq!(w.subjects(), vec!["Spam offer", "Old newsletter"]);
    w.frame(vec![]);
    assert!(w.shows("not downloaded"), "rows say so");
    // opening a row shows the button, not the text
    w.open_row(0);
    assert!(matches!(w.app.mail.open.as_ref().map(|o| &o.body), Some(Body::NeedsDownload)));
    w.frame(vec![]);
    assert!(!w.shows("Buy now"));
    w.click("mail:load");
    w.until("the message arrives", |a| matches!(a.mail.open.as_ref().map(|o| &o.body), Some(Body::Ready(_))));
    w.frame(vec![]);
    assert!(w.shows("Buy now, a lot of text to be loaded."));
    // the listing is not asked for again by opening the folder a second time (once per run)
    let examines = |w: &W| w.server.state().verbs().iter().filter(|v| v.starts_with("EXAMINE \"Trash\"")).count();
    let n = examines(&w);
    w.open_folder("INBOX");
    w.open_folder("Trash");
    w.frame(vec![]);
    assert_eq!(examines(&w), n);
    // and still nothing that changes a mailbox
    assert!(w.server.state().verbs().iter().all(|v| !v.starts_with("SELECT") && !v.starts_with("STORE")));
}

// spec: MAIL-055
#[test]
fn a_message_over_the_size_limit_has_a_load_button() {
    let mut w = W::new("big", true);
    {
        let mut s = w.server.state();
        s.add_message("INBOX", dated("Huge scan", "Bob <bob@x.de>", &"line of a very long mail\r\n".repeat(2_000), 14), NOW);
    }
    w.sync_and_wait();
    let i = w.row_of("(too large to download)");
    assert_eq!(w.app.mail.row(i).unwrap().state, STATE_TOO_LARGE);
    w.open_row(i);
    w.frame(vec![]);
    assert!(matches!(w.app.mail.open.as_ref().map(|o| &o.body), Some(Body::TooLarge)));
    assert!(w.shows("over the limit"), "the reason is given");
    // the person asks for it: with a bigger limit it comes
    w.app.mail.opts.limits.max_message = 10_000_000;
    // (the factory above sets its own limit, so make it the bigger one too)
    w.app.mail.factory = Arc::new(|acct: &MailAccount, session: Option<crate::secret::Secret>| {
        let password = session.or_else(|| secrets::mail_secret(&acct.id, MailSecret::Password)).ok_or_else(|| NO_PASSWORD.to_string())?;
        let settings = ConnectSettings { host: acct.host.clone(), port: acct.port, security: acct.security, limits: Limits { io_timeout: Duration::from_secs(15), max_message: 10_000_000, ..Limits::default() } };
        Ok(Arc::new(TestConnector { settings, user: acct.user.clone(), password: password.expose().to_string(), offline: Arc::new(std::sync::atomic::AtomicBool::new(false)) }) as Arc<dyn Connector>)
    });
    w.click("mail:load");
    w.until("the message arrives", |a| matches!(a.mail.open.as_ref().map(|o| &o.body), Some(Body::Ready(_))));
    w.frame(vec![]);
    assert!(w.shows("line of a very long mail"));
    let id = w.app.mail.open.as_ref().unwrap().msg.id;
    assert_eq!(w.app.mail.open.as_ref().unwrap().msg.id, id);
    let i = w.row_of("Huge scan");
    assert_eq!(w.app.mail.row(i).unwrap().state, STATE_FULL, "the row was updated");
}

// spec: MAIL-057
#[test]
fn a_mail_deleted_on_the_server_is_dimmed_and_says_so() {
    let mut w = W::new("gone", true);
    w.sync_and_wait();
    {
        let mut s = w.server.state();
        let uid = s.folder_mut("INBOX").msgs.iter().find(|m| String::from_utf8_lossy(&m.raw).contains("Lunch plans")).unwrap().uid;
        s.delete_message("INBOX", uid);
    }
    w.sync_and_wait();
    w.until("the list shows it", |a| a.mail.total == 3);
    w.frame(vec![]);
    let dim = |w: &W, text: &str| w.texts.iter().find(|(t, _)| t == text).map(|(_, c)| c.a());
    assert!(dim(&w, "Lunch plans").is_some_and(|a| a < 200), "the deleted mail is drawn dimmed: {:?}", dim(&w, "Lunch plans"));
    assert!(dim(&w, "Invoice 2026").is_some_and(|a| a == 255), "the others are not");
    w.open_mail("Lunch plans");
    w.frame(vec![]);
    assert!(w.shows("deleted on the server"));
    assert!(w.shows("Pizza at noon"), "its text is still readable");
    assert!(w.app.mail.open.as_ref().unwrap().msg.gone_on_server);
}

// spec: MAIL-058
#[test]
fn a_sync_always_ends_with_a_report_even_after_an_error_and_can_be_cancelled() {
    // wrong password: the run ends, the view says what went wrong
    let mut w = W::new("badpw", true);
    secrets::set_mail_secret(&w.account.id, MailSecret::Password, "not the password");
    w.sync_and_wait();
    let run = &w.app.mail.syncs[&w.account.id];
    let s = run.finished.as_ref().unwrap();
    assert!(!s.errors.is_empty() && !run.running(), "{s:?}");
    w.frame(vec![]);
    assert!(w.shows("Sync problem"), "the problem is on screen");
    assert!(w.app.mail.status(&w.app.cfg).is_none(), "nothing runs any more");

    // a slow server: progress in the status bar, Cancel works
    secrets::set_mail_secret(&w.account.id, MailSecret::Password, PASSWORD);
    w.server.state().fetch_delay = Duration::from_millis(300);
    w.click("mail:sync");
    w.until("progress shows", |a| a.mail.status(&a.cfg).is_some());
    w.frame(vec![]);
    assert!(w.shows("Mail Test mail"), "the status bar names the account");
    assert!(w.has("mail:cancel"));
    // two syncs of one account never run at once
    let cfg = w.app.cfg.clone();
    w.app.mail.start_sync(&cfg, &w.account.id);
    w.click("mail:cancel");
    let id = w.account.id.clone();
    w.until("the run ends", move |a| a.mail.syncs.get(&id).is_some_and(|r| r.finished.is_some()));
    assert!(w.app.mail.syncs[&w.account.id].finished.as_ref().unwrap().cancelled);
    assert!(w.server.state().max_open_connections <= 4);
}

// spec: MAIL-058
#[test]
fn mail_is_fetched_by_itself_on_the_timer_and_never_twice_at_once() {
    let mut w = W::new("timer", true);
    w.app.cfg.mail_sync_minutes = 10;
    let id = w.account.id.clone();
    w.until("the automatic sync ran", move |a| a.mail.syncs.get(&id).is_some_and(|r| r.finished.is_some()));
    let n = w.server.state().connections;
    assert!(n >= 1);
    for _ in 0..30 {
        w.frame(vec![]);
    }
    assert_eq!(w.server.state().connections, n, "the next one is ten minutes away");
    // switched off: nothing starts by itself
    let w2 = W::new("timer-off", true);
    let mut w2 = w2;
    for _ in 0..30 {
        w2.frame(vec![]);
    }
    assert_eq!(w2.server.state().connections, 0);
}

// spec: MAIL-059
#[test]
fn an_account_without_a_password_asks_for_it_and_syncs_with_it() {
    let mut w = W::new("askpw", true);
    secrets::clear_mail_secret(&w.account.id, MailSecret::Password);
    w.sync_and_wait();
    w.until("the question appears", |a| a.mail.prompt.is_some());
    w.frame(vec![]);
    assert!(w.shows("Password for Test mail"));
    w.typ("mail:password", PASSWORD);
    w.click("mail:password-ok");
    assert!(w.app.mail.prompt.is_none());
    assert_eq!(w.app.mail.session_pw.get(&w.account.id).map(|s| s.expose()), Some(PASSWORD));
    let id = w.account.id.clone();
    w.until("the sync after it ends well", move |a| a.mail.syncs.get(&id).is_some_and(|r| r.finished.is_some() && r.errors.is_empty()));
    assert!(w.app.mail.total >= 3);
    assert_eq!(secrets::mail_secret(&w.account.id, MailSecret::Password), None, "it was not put into the keyring");
}

// spec: MAIL-060, MAIL-061
#[test]
fn search_waits_for_a_pause_runs_in_a_job_and_the_keys_work_without_the_mouse() {
    let mut w = W::new("search", true);
    w.sync_and_wait();
    w.until("the Inbox is open", |a| a.mail.total == 3);
    w.frame(vec![]);
    // `/` goes into the search field and the slash is not typed
    w.press(Key::Slash, Modifiers::NONE);
    assert!(w.ctx.memory(|m| m.has_focus(egui::Id::unique("mail-search"))), "/ focuses the search field");
    assert_eq!(w.app.mail.query, "");
    w.type_text("invoice");
    assert!(w.app.mail.results.is_none(), "not searched on every key: it waits for a pause");
    w.until("the results arrive", |a| a.mail.results.is_some());
    // all folders: Inbox, Sent and the All Mail copy of the invoice (newest first)
    let hits: Vec<String> = w.app.mail.results.as_ref().unwrap().iter().map(|m| format!("{}:{}", m.folder, m.subject)).collect();
    assert_eq!(hits.len(), 1 + 1, "{hits:?}"); // All Mail is not synced and Trash is not either
    assert!(hits.iter().any(|h| h.starts_with("INBOX:")) && hits.iter().any(|h| h.starts_with("Sent:")), "{hits:?}");
    // the syntax of the index: field prefixes and phrases
    w.press(Key::Escape, Modifiers::NONE);
    w.click("mail:search");
    w.frame(vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
    w.type_text("from:dora \"projector later\"");
    w.until("the phrase search arrives", |a| a.mail.results.as_ref().is_some_and(|r| r.len() == 1));
    assert_eq!(w.app.mail.results.as_ref().unwrap()[0].subject, "Lunch plans");
    // this folder only
    w.click("mail:scope-folder");
    w.until("the scoped search", |a| !a.mail.search_all && !a.mail.searching && a.mail.results.is_some());
    // ↓ goes into the results, Enter opens, Esc steps back out
    w.press(Key::ArrowDown, Modifiers::NONE);
    assert!(!w.ctx.memory(|m| m.focused().is_some()), "the field let go of the keyboard");
    assert_eq!(w.app.mail.cursor, Some(0));
    w.press(Key::Enter, Modifiers::NONE);
    w.until("the hit is read", |a| matches!(a.mail.open.as_ref().map(|o| &o.body), Some(Body::Ready(_))));
    assert_eq!(w.app.mail.open.as_ref().unwrap().msg.subject, "Lunch plans");
    w.press(Key::Escape, Modifiers::NONE);
    assert!(w.app.mail.open.is_none(), "Esc closes the message first");
    w.press(Key::Escape, Modifiers::NONE);
    assert!(w.app.mail.results.is_none() && w.app.mail.query.is_empty(), "then it clears the search");
    // j / k / Enter / Space in the list, Ctrl+J / Ctrl+K between folders
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(0));
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(2));
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(2), "the end of the list stops it");
    w.press(Key::K, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(1));
    w.press(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(0));
    w.press(Key::Space, Modifiers::NONE);
    assert_eq!(w.app.mail.marked.len(), 1, "Space marks");
    w.press(Key::Space, Modifiers::NONE);
    assert!(w.app.mail.marked.is_empty(), "and unmarks");
    let inbox = w.folder_id("INBOX");
    assert_eq!(w.app.mail.folder, Some(inbox));
    w.press(Key::J, Modifiers::COMMAND);
    assert_ne!(w.app.mail.folder, Some(inbox), "Ctrl+J: next folder");
    w.press(Key::K, Modifiers::COMMAND);
    assert_eq!(w.app.mail.folder, Some(inbox), "Ctrl+K: back");
    // while a text field has the keyboard, j and k are letters
    w.click("mail:search");
    let c = w.app.mail.cursor;
    w.type_text("j");
    w.frame(vec![Event::Key { key: Key::J, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    assert_eq!(w.app.mail.cursor, c);
    assert_eq!(w.app.mail.query, "j");
}

// spec: MAIL-070, MAIL-071
#[test]
fn import_hands_the_stored_mail_to_the_import_dialog_and_writes_nothing_until_import_is_pressed() {
    let mut w = W::new("import", true);
    w.sync_and_wait();
    w.until("the Inbox is open", |a| a.mail.total == 3);
    let vault_before = w.vault_files();
    w.open_mail("Invoice 2026");
    let stored = {
        let store = w.app.mail.ready_store().unwrap();
        store.read_raw(&w.app.mail.open.as_ref().unwrap().msg).unwrap()
    };
    w.click("mail:import");
    w.until("the dialog opens", |a| matches!(a.modal, Some(Modal::Import(_))));
    w.settle();
    {
        let Some(Modal::Import(d)) = &w.app.modal else { unreachable!() };
        assert_eq!(d.items.len(), 1);
        let crate::import::Source::Email { mail, original } = &d.items[0].source else { panic!("not an e-mail item") };
        assert_eq!(original.1, stored, "the very bytes of the stored .eml");
        assert!(original.0.ends_with(".eml"));
        assert_eq!(mail.subject, "Invoice 2026");
        assert!(d.items[0].already.is_none());
    }
    assert_eq!(w.vault_files(), vault_before, "nothing is written before Import is pressed");
    // pressing Import makes the note, with the original kept next to it (like a dropped .eml)
    w.click("import");
    w.settle();
    let after = w.vault_files();
    assert!(after.iter().any(|f| f.ends_with("Invoice 2026.md")), "{after:?}");
    assert!(after.iter().any(|f| f.ends_with(".eml")), "{after:?}");
    // the same mail again is recognised as imported (IMP-005)
    // the import opened the new note; go back to the mail
    assert!(!matches!(w.app.tabs[w.app.active].view, View::Mail), "the new note is shown");
    w.app.show_view(View::Mail, false);
    w.settle();
    w.press(Key::I, Modifiers::NONE);
    w.until("the dialog opens again", |a| matches!(a.modal, Some(Modal::Import(_))));
    w.settle();
    {
        let Some(Modal::Import(d)) = &w.app.modal else { unreachable!() };
        assert!(d.items[0].already.is_some(), "a duplicate is marked");
    }
    w.click("cancel");
    w.settle();

    // several marked mails, one of them not downloaded: the others go, the dialog says why
    w.open_folder("Trash");
    let trash = w.folder_id("Trash");
    w.until("Trash is listed", move |a| matches!(a.mail.listings.get(&trash), Some(ListingState::Done(_))) && a.mail.total == 2);
    w.open_folder("INBOX");
    w.frame(vec![]);
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::Space, Modifiers::NONE);
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::Space, Modifiers::NONE);
    assert_eq!(w.app.mail.marked.len(), 2);
    w.frame(vec![]);
    w.click("mail:import-marked");
    w.until("the dialog opens", |a| matches!(a.modal, Some(Modal::Import(_))));
    w.settle();
    {
        let Some(Modal::Import(d)) = &w.app.modal else { unreachable!() };
        assert_eq!(d.items.len(), 2);
        assert!(d.notes.is_empty(), "{:?}", d.notes);
    }
    w.click("cancel");
    w.settle();
    // a headers-only message in the selection is left out and said so
    w.app.mail.marked.clear();
    w.open_folder("Trash");
    w.until("Trash rows", |a| a.mail.total == 2);
    let n = w.app.mail.list_len();
    w.app.mail.ensure_rows(0..n);
    let hdr = w.app.mail.row(0).unwrap().clone();
    assert_eq!(hdr.state, STATE_HEADER);
    w.app.mail.marked.insert(hdr.id, hdr);
    w.press(Key::I, Modifiers::NONE);
    w.until("the dialog opens", |a| matches!(a.modal, Some(Modal::Import(_))));
    let Some(Modal::Import(d)) = &w.app.modal else { unreachable!() };
    assert!(d.items.is_empty() && d.notes.iter().any(|n| n.contains("not downloaded")), "{:?}", d.notes);
}

// spec: MAIL-052
#[test]
fn fifty_thousand_messages_draw_quickly_and_a_steady_frame_runs_no_query() {
    let root = std::env::temp_dir().join(format!("shale-mail-perf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    {
        let store = crate::mail_store::MailStore::open(&root).unwrap();
        let folder = store.ensure_folder("perf", "Perf", "INBOX", Some("/"), None, true).unwrap();
        store.commit(vec![crate::mail_db::Op::FolderState { folder: folder.id, uidvalidity: Some(7), uidnext: Some(50_001), highestmodseq: None, last_sync: 1 }]).unwrap();
        let folder = crate::mail_db::FolderRow { uidvalidity: Some(7), ..folder };
        let start = Instant::now();
        for chunk in 0..10u32 {
            let ops: Vec<_> = (0..5000u32)
                .map(|i| {
                    let uid = chunk * 5000 + i + 1;
                    let headers = format!("From: Sender {} <s{}@x.de>\r\nSubject: Message number {uid}\r\n\r\n", uid % 97, uid % 97);
                    crate::mail_db::Op::message(store.header_only(&folder, 7, uid, headers.as_bytes(), if uid % 3 == 0 { "\\Seen" } else { "" }, Some(1_700_000_000 + i64::from(uid) * 60), 1000))
                })
                .collect();
            store.commit(ops).unwrap();
        }
        eprintln!("seeded 50,000 rows in {:?}", start.elapsed());
    }
    let dir = std::env::temp_dir().join(format!("shale-mailui-perf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Welcome.md"), "# Welcome\n").unwrap();
    let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
    app.cfg.mail_sync_minutes = 0;
    app.cfg.mail_accounts = vec![MailAccount { id: "perf".into(), name: "Perf".into(), host: "localhost".into(), user: "u".into(), ..MailAccount::default() }];
    app.mail.root = root.clone();
    app.show_view(View::Mail, false);
    let server = FakeServer::start(MailSecurity::ImplicitTls);
    let mut w = W {
        app,
        ctx: egui::Context::default(),
        t: 0.0,
        server,
        dir: dir.clone(),
        account: MailAccount { id: "perf".into(), ..MailAccount::default() },
        offline: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        texts: vec![],
        fills: vec![],
    };
    crate::theme::apply(&w.ctx);
    w.until("the store and the first page", |a| a.mail.total == 50_000);
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert!(w.shows("50000 messages"), "{:?}", w.texts.iter().map(|t| &t.0).take(30).collect::<Vec<_>>());
    // steady frames: no query at all, and fast
    let q = w.app.mail.queries;
    let start = Instant::now();
    for _ in 0..40 {
        w.frame(vec![]);
    }
    let per_frame = start.elapsed() / 40;
    eprintln!("steady frame with 50,000 messages: {per_frame:?}");
    assert_eq!(w.app.mail.queries, q, "a steady frame reads nothing from the index");
    assert!(per_frame < Duration::from_millis(60), "a frame took {per_frame:?}");
    // scrolling with the keys reads at most one page per page crossed
    for _ in 0..25 {
        w.press(Key::J, Modifiers::NONE);
    }
    assert_eq!(w.app.mail.cursor, Some(24));
    assert!(w.app.mail.queries - q <= 2, "{} queries for 25 steps", w.app.mail.queries - q);
    // a jump to the end of the list reads the last page only
    let q = w.app.mail.queries;
    w.app.mail.cursor = Some(49_990);
    w.app.mail.scroll_to_cursor = true;
    let start = Instant::now();
    for _ in 0..4 {
        w.frame(vec![]);
    }
    eprintln!("jump to the end: {:?}", start.elapsed());
    assert!(w.app.mail.visible.end > 49_900, "{:?}", w.app.mail.visible);
    assert!(w.app.mail.queries - q <= 3, "{}", w.app.mail.queries - q);
    let m = w.app.mail.cursor_message().unwrap();
    assert_eq!(m.subject, "Message number 10");
    // the newest first: row 0 is the highest date
    w.app.mail.cursor = Some(0);
    assert_eq!(w.app.mail.cursor_message().unwrap().subject, "Message number 50000");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: MAIL-051
#[test]
fn folders_are_ordered_with_the_inbox_first_and_sub_folders_under_their_parent() {
    let mk = |name: &str, su: Option<&str>| crate::mail_db::FolderRow {
        id: 0,
        account: "a".into(),
        name: name.into(),
        delimiter: Some("/".into()),
        special_use: su.map(String::from),
        uidvalidity: None,
        uidnext: None,
        highestmodseq: None,
        sync_enabled: true,
        last_sync: None,
    };
    let mut v = crate::mail_ui::MailView::new(std::env::temp_dir().join("shale-mail-order-unused"));
    v.folders = vec![mk("Trash", Some("\\Trash")), mk("Work/Plans", None), mk("Work", None), mk("Sent", Some("\\Sent")), mk("INBOX", None), mk("Archive", None), mk("Spam", None)];
    let names: Vec<String> = v.account_folders("a").iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, vec!["INBOX", "Sent", "Archive", "Work", "Work/Plans", "Spam", "Trash"]);
    assert_eq!(crate::mail_ui::folder_depth(&mk("Work/Plans/2026", None)), 2);
    assert_eq!(crate::mail_ui::folder_label(&mk("Work/Plans", None)), "Plans");
    assert_eq!(crate::mail_ui::folder_label(&mk("Gesendet &AOQ-", None)), "Gesendet ä");
    // a folder whose parent is not a folder shows its whole path; one with a parent shows its own name
    v.folders.push(mk("Projects/Hall B", None));
    let of = |v: &crate::mail_ui::MailView, n: &str| v.tree_entry(v.folders.iter().find(|f| f.name == n).unwrap());
    assert_eq!(of(&v, "Projects/Hall B"), (0, "Projects/Hall B".to_string()));
    assert_eq!(of(&v, "Work/Plans"), (1, "Plans".to_string()));
    assert_eq!(of(&v, "INBOX"), (0, "INBOX".to_string()));
}

// spec: MAIL-052
#[test]
fn dates_in_the_list_are_short() {
    let now = NOW;
    // times are local; stay away from midnight edges by using the same instant
    assert_eq!(crate::mail_ui::list_date(None, now), "");
    assert!(crate::mail_ui::list_date(Some(now), now).contains(':'), "today shows the time");
    assert!(crate::mail_ui::list_date(Some(now - 100 * 86_400), now).chars().any(|c| c.is_alphabetic()), "this year shows day and month");
    assert!(crate::mail_ui::list_date(Some(now - 800 * 86_400), now).starts_with("202"), "older shows the date");
}

// spec: MAIL-083, MAIL-084, MAIL-086
#[test]
fn an_oauth2_account_signs_in_through_the_dialog_syncs_renews_its_token_and_asks_to_sign_in_again_when_revoked() {
    use crate::oauth_fake_tests::{http_get, FakeOAuth};
    use std::sync::atomic::{AtomicBool, Ordering};
    if std::process::Command::new("curl").arg("--version").output().is_err() {
        return;
    }
    let provider = FakeOAuth::start();
    let mut w = W::new("oauth", false);
    {
        let mut s = w.server.state();
        s.valid_tokens = Some(provider.valid_access());
    }
    // the pretend browser: waits to be let go, then asks the provider and follows its redirect
    let go = Arc::new(AtomicBool::new(false));
    let opened = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (go2, opened2) = (go.clone(), opened.clone());
    let _browser = crate::opener::install_test_browser(Box::new(move |url| {
        opened2.lock().unwrap().push(url.to_string());
        let (url, go) = (url.to_string(), go2.clone());
        std::thread::spawn(move || {
            while !go.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            let (_, head, _) = http_get(&url);
            if let Some(loc) = head.lines().find_map(|l| l.strip_prefix("Location: ")) {
                let _ = http_get(loc.trim());
            }
        });
    }));

    w.until("the empty view", |a| a.mail.rects.iter().any(|r| r.0 == "mail:add-account"));
    w.click("mail:add-account");
    w.settle();
    w.click("md:add");
    w.typ("md:name", "OAuth mail");
    w.typ("md:host", "localhost");
    w.typ("md:port", &w.server.port.to_string());
    w.typ("md:user", "ada");
    w.click("md:auth-oauth2");
    // the Google preset also fills the SMTP server; this test is about reading, and must never reach a real server
    if let Some(Modal::MailAccounts(d)) = &mut w.app.modal {
        assert_eq!(d.accounts[0].smtp_host, "smtp.gmail.com", "the preset fills the SMTP server (MAIL-120)");
        assert!(d.accounts[0].server_stores_sent);
        d.accounts[0].smtp_host.clear();
    }
    w.click("md:provider:Custom");
    assert!(!w.has("md:password"), "an OAuth2 account has no password field");
    // nothing typed yet: the sign-in is refused with a clear message
    w.click("md:signin");
    let id = match &w.app.modal {
        Some(Modal::MailAccounts(d)) => d.accounts[0].id.clone(),
        _ => unreachable!(),
    };
    let id1 = id.clone();
    w.until("the first attempt fails", move |a| matches!(a.mail.signins.get(&id1), Some(crate::mail_ui::SignInState::Failed(_))));
    w.frame(vec![]);
    assert!(w.shows("client ID"), "the missing client ID is said in words");
    w.typ("md:oauth-auth-url", &format!("http://127.0.0.1:{}/authorize", provider.port));
    w.typ("md:oauth-token-url", &format!("http://127.0.0.1:{}/token", provider.port));
    w.typ("md:oauth-scope", "mail");
    w.typ("md:oauth-client-id", "shale-test");

    // sign in: waiting state with Cancel, then cancel it
    w.click("md:signin");
    w.frame(vec![]);
    assert!(w.shows("Waiting for the browser") && w.has("md:signin-cancel"));
    let o = opened.clone();
    w.until("the browser got the address", move |_| !o.lock().unwrap().is_empty());
    w.until("the address is offered", |a| a.mail.signins.values().any(|s| matches!(s, crate::mail_ui::SignInState::Waiting { url: Some(_), .. })));
    w.click("md:signin-cancel");
    let id2 = id.clone();
    w.until("the cancelled sign-in ends", move |a| matches!(a.mail.signins.get(&id2), Some(crate::mail_ui::SignInState::Failed(_))));
    w.frame(vec![]);
    assert!(w.shows("The sign-in was cancelled."));
    assert!(secrets::mail_secret(&id, MailSecret::RefreshToken).is_none(), "a cancelled sign-in keeps nothing");
    go.store(true, Ordering::SeqCst); // the old pretend browser answers late: nobody listens any more

    // sign in for real
    w.click("md:signin");
    let id3 = id.clone();
    w.until("the sign-in ends", move |a| matches!(a.mail.signins.get(&id3), Some(crate::mail_ui::SignInState::Done { .. })));
    w.frame(vec![]);
    assert!(w.shows("Signed in as ada@example.com"), "who signed in is shown");
    assert!(secrets::mail_secret(&id, MailSecret::RefreshToken).is_some(), "the refresh token is in the keyring");
    {
        let urls = opened.lock().unwrap();
        let last = urls.last().unwrap();
        assert!(last.starts_with(&format!("http://127.0.0.1:{}/authorize?", provider.port)) && last.contains("code_challenge_method=S256") && last.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A"), "{last}");
    }

    // "Test connection" works with OAuth2 (OAUTHBEARER is preferred: the pretend server offers both)
    w.click("md:test");
    let id4 = id.clone();
    w.until("the test ends", move |a| matches!(a.mail.tests.get(&id4), Some(TestState::Ok(_)) | Some(TestState::Failed(_))));
    assert!(
        matches!(w.app.mail.tests.get(&id), Some(TestState::Ok(m)) if m.contains("4 folders")),
        "{}",
        match w.app.mail.tests.get(&id) {
            Some(TestState::Failed(e)) => e.clone(),
            _ => "?".into(),
        }
    );
    assert_eq!(w.server.state().auth_mechs.last().map(String::as_str), Some("OAUTHBEARER"));
    w.click("md:done");
    assert!(w.app.modal.is_none());
    let a = &w.app.cfg.mail_accounts[0];
    assert_eq!((a.auth, a.oauth_client_id.as_str()), (crate::config::MailAuth::OAuth2, "shale-test"));
    let json = serde_json::to_string(&w.app.cfg).unwrap();
    assert!(!json.contains("rt-") && !json.contains("at-"), "no token in the settings file");
    let id5 = id.clone();
    w.until("the first sync ends", move |a| a.mail.syncs.get(&id5).is_some_and(|r| r.finished.is_some()));
    assert!(w.app.mail.syncs[&id].errors.is_empty(), "{:?}", w.app.mail.syncs[&id].errors);

    // the server stops accepting the access token (it expired): the sync renews it by itself
    let before = provider.state.lock().unwrap().token_requests.len();
    provider.expire_access_tokens();
    w.app.mail.syncs.remove(&id);
    w.account = w.app.cfg.mail_accounts[0].clone();
    w.sync_and_wait();
    assert!(w.app.mail.syncs[&id].errors.is_empty(), "{:?}", w.app.mail.syncs[&id].errors);
    assert!(provider.state.lock().unwrap().token_requests.len() > before, "a new access token was fetched");

    // the provider revokes the sign-in: a clear message and a way to sign in again
    {
        let mut st = provider.state.lock().unwrap();
        st.revoked = true;
    }
    provider.expire_access_tokens();
    w.sync_and_wait();
    let errs = w.app.mail.syncs[&id].errors.join(" ");
    let fin = w.app.mail.syncs[&id].finished.as_ref().map(|s| s.errors.join(" ")).unwrap_or_default();
    assert!(crate::oauth::needs_sign_in(&errs) || crate::oauth::needs_sign_in(&fin), "{errs} / {fin}");
    assert!(crate::oauth::needs_sign_in(&format!("{errs}{fin}")) && format!("{errs}{fin}").contains("revoked"));
    assert_eq!(w.app.mail.signed_in.get(&id), Some(&false));
    assert!(secrets::mail_secret(&id, MailSecret::RefreshToken).is_none(), "a dead refresh token is dropped");
    w.frame(vec![]);
    assert!(w.shows("sign-in expired"), "the mail view says so");
    w.app.open_mail_settings();
    w.settle();
    w.frame(vec![]);
    assert!(w.has("md:signin"));
    // sign in again
    provider.state.lock().unwrap().revoked = false;
    go.store(true, Ordering::SeqCst);
    w.click("md:signin");
    let id6 = id.clone();
    w.until("signed in again", move |a| matches!(a.mail.signins.get(&id6), Some(crate::mail_ui::SignInState::Done { .. })));
    w.click("md:done");
    w.app.mail.syncs.remove(&id);
    w.sync_and_wait();
    assert!(w.app.mail.syncs[&id].errors.is_empty(), "{:?}", w.app.mail.syncs[&id].errors);
    secrets::clear_mail_secret(&id, MailSecret::RefreshToken);
    secrets::clear_mail_secret(&id, MailSecret::ClientSecret);
    crate::oauth::forget_source(&id);
}

// ------------------------------------------------------------ phase 2: flags, move, delete

/// A synced window with the Inbox open, the first row under the cursor.
fn inbox_with_cursor(tag: &str) -> W {
    let mut w = W::new(tag, true);
    w.sync_and_wait();
    w.until("the Inbox is open", |a| a.mail.total == 3);
    w.frame(vec![]);
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(0));
    w
}

// spec: MAIL-102, MAIL-110, MAIL-111
#[test]
fn keys_mark_read_flag_delete_and_move_work_on_the_cursor_and_on_the_marked_mails_without_the_mouse() {
    let mut w = inbox_with_cursor("keys2");
    // u: read (the list is bold and dotted for unread, plain for read); the server is told
    assert!(w.has("mail:unread:0"));
    w.press(Key::U, Modifiers::NONE);
    w.frame(vec![]);
    assert!(!w.has("mail:unread:0"), "read mail is not marked as unread");
    assert!(w.has("mail:pending-mark:0"), "while the server has not answered, the row says so");
    w.settle_ops();
    w.frame(vec![]);
    assert_eq!(w.server_flags("INBOX", 3), vec!["\\Seen"]);
    assert!(w.shows("INBOX  (2)"), "the unread count follows");
    assert!(!w.has("mail:pending-mark:0"));
    w.press(Key::U, Modifiers::NONE);
    w.settle_ops();
    w.frame(vec![]);
    assert!(w.server_flags("INBOX", 3).is_empty(), "u again: unread");
    assert!(w.has("mail:unread:0"));
    // s and f: the flag
    w.press(Key::S, Modifiers::NONE);
    w.settle_ops();
    w.frame(vec![]);
    assert_eq!(w.server_flags("INBOX", 3), vec!["\\Flagged"]);
    assert!(w.has("mail:flag-mark:0"), "a flagged mail has a flag in the list");
    w.press(Key::F, Modifiers::NONE);
    w.settle_ops();
    w.frame(vec![]);
    assert!(w.server_flags("INBOX", 3).is_empty());
    assert!(!w.has("mail:flag-mark:0"));
    // a key with Ctrl, or while a text field has the keyboard, does nothing
    w.press(Key::D, Modifiers::COMMAND);
    w.click("mail:search");
    w.type_text("d");
    w.frame(vec![Event::Key { key: Key::D, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    w.frame(vec![Event::Key { key: Key::Delete, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    w.settle_ops();
    assert_eq!(w.server_uids("INBOX"), vec![1, 2, 3], "nothing was deleted");
    w.press(Key::Escape, Modifiers::NONE);
    w.app.mail.query.clear();
    // Space marks two mails, d moves both to Trash (never STORE \Deleted + EXPUNGE)
    w.frame(vec![Event::PointerMoved(Pos2::new(5.0, 5.0))]);
    w.app.mail.cursor = Some(0);
    w.press(Key::Space, Modifiers::NONE);
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::Space, Modifiers::NONE);
    assert_eq!(w.app.mail.marked.len(), 2);
    w.press(Key::D, Modifiers::NONE);
    assert!(w.app.mail.marked.is_empty(), "the marks are used up");
    w.settle_ops();
    w.frame(vec![]);
    assert_eq!(w.server_uids("INBOX"), vec![1]);
    assert_eq!(w.server_uids("Trash").len(), 4, "the two that were there and the two moved");
    assert_eq!(w.subjects(), vec!["Invoice 2026"]);
    let verbs = w.server.state().verbs();
    assert!(verbs.iter().all(|v| !v.contains("Deleted") && !v.starts_with("EXPUNGE") && !v.starts_with("UID EXPUNGE")), "{verbs:?}");
    // m: the folder picker, typing filters, Enter moves
    w.app.mail.cursor = Some(0);
    w.press(Key::M, Modifiers::NONE);
    assert!(w.app.mail.mover.is_some(), "m opens the picker");
    w.frame(vec![]);
    assert!(w.has("mail:mover-field"));
    assert_eq!(w.app.mail.mover.as_ref().unwrap().query, "", "the m that opened it is not typed into it");
    w.type_text("sen");
    w.frame(vec![]);
    let names: Vec<String> = w.app.mail.mover_candidates().iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, vec!["Sent"], "typing filters the folders");
    w.press(Key::Enter, Modifiers::NONE);
    assert!(w.app.mail.mover.is_none());
    w.settle_ops();
    assert_eq!(w.server_uids("Sent").len(), 2);
    assert!(w.server_uids("INBOX").is_empty());
    // Esc leaves the picker without moving anything
    w.frame(vec![]);
    w.app.mail.cursor = None;
    w.open_folder("Sent");
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::M, Modifiers::NONE);
    w.press(Key::Escape, Modifiers::NONE);
    assert!(w.app.mail.mover.is_none());
    assert_eq!(w.server_uids("Sent").len(), 2);
}

// spec: MAIL-111
#[test]
fn the_reading_pane_buttons_the_marked_bar_and_the_row_menu_do_the_same_things() {
    let mut w = inbox_with_cursor("buttons2");
    w.open_mail("Lunch plans"); // uid 2
    w.frame(vec![]);
    w.click("mail:read-toggle");
    w.settle_ops();
    w.frame(vec![]);
    assert_eq!(w.server_flags("INBOX", 2), vec!["\\Seen"]);
    assert!(w.shows("Mark unread"), "the button says what it does next");
    w.click("mail:flag-toggle");
    w.settle_ops();
    w.frame(vec![]);
    assert_eq!(w.server_flags("INBOX", 2), vec!["\\Flagged", "\\Seen"]);
    assert!(w.shows("Unflag") && w.shows("Flagged"));
    // Move…: the list of folders, a click on one
    w.click("mail:move");
    w.frame(vec![]);
    assert!(w.has("mail:mover-item:0"));
    let first = w.app.mail.mover_candidates()[0].name.clone();
    w.click("mail:mover-item:0");
    w.settle_ops();
    assert_eq!(w.server_uids("INBOX"), vec![1, 3], "mail 2 left the Inbox");
    assert_eq!(w.server_uids(&first).len(), if first == "Trash" { 3 } else { w.server.state().messages(&first).len() });
    assert!(!w.server.state().messages(&first).is_empty());
    // the reading pane no longer shows mail that left
    w.frame(vec![]);
    assert!(w.app.mail.open.is_none());
    // Delete in the reading pane
    w.open_mail("Invoice 2026");
    w.click("mail:delete");
    w.settle_ops();
    assert_eq!(w.server_uids("INBOX"), vec![3]);
    // the marked bar
    w.frame(vec![]);
    w.app.mail.cursor = Some(0);
    w.app.mail.pane = Pane::List; // the clicks into the reading pane gave it the keyboard
    w.press(Key::Space, Modifiers::NONE);
    w.frame(vec![]);
    w.click("mail:marked-flag");
    w.settle_ops();
    assert_eq!(w.server_flags("INBOX", 3), vec!["\\Flagged"]);
    w.click("mail:marked-delete");
    w.settle_ops();
    assert!(w.server_uids("INBOX").is_empty());
    // the menu of a row (a right click)
    w.open_folder("Sent");
    w.frame(vec![]);
    let row = w.rect("mail:row:0");
    let p = row.center();
    let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE };
    w.frame(vec![Event::PointerMoved(p)]);
    w.frame(vec![b(true)]);
    w.frame(vec![b(false)]);
    w.frame(vec![]);
    assert!(w.has("mail:ctx-read") && w.has("mail:ctx-flag") && w.has("mail:ctx-move") && w.has("mail:ctx-delete"), "right click opens the menu");
    w.click("mail:ctx-flag");
    w.settle_ops();
    assert_eq!(w.server_flags("Sent", 1), vec!["\\Flagged"]);
}

// spec: MAIL-105
#[test]
fn delete_in_trash_asks_first_and_names_the_count_and_only_the_confirmation_deletes() {
    let mut w = W::new("forever2", true);
    w.sync_and_wait();
    w.open_folder("Trash");
    let trash = w.folder_id("Trash");
    w.until("the listing arrives", move |a| matches!(a.mail.listings.get(&trash), Some(ListingState::Done(_))) && a.mail.total == 2);
    w.frame(vec![]);
    // another program flagged mail 2 as deleted: it must survive "delete forever" of mail 1
    w.server.state().set_flags("Trash", 2, &["\\Deleted"]);
    let i = w.row_of("Old newsletter");
    w.open_row(i);
    w.frame(vec![]);
    assert!(w.shows("Delete forever…"), "in Trash the button says what it does");
    w.press(Key::D, Modifiers::NONE);
    w.frame(vec![]);
    assert!(w.app.mail.forever.is_some());
    assert!(w.shows("Delete 1 message forever?"), "the dialog names the count");
    assert_eq!(w.server_uids("Trash"), vec![1, 2], "nothing happened yet");
    // Esc and Enter do not delete; Cancel does not either
    w.press(Key::Enter, Modifiers::NONE);
    assert!(w.app.mail.forever.is_some() && w.server_uids("Trash") == vec![1, 2]);
    w.press(Key::Escape, Modifiers::NONE);
    assert!(w.app.mail.forever.is_none());
    w.press(Key::D, Modifiers::NONE);
    w.frame(vec![]);
    w.click("mail:forever-cancel");
    assert!(w.app.mail.forever.is_none());
    assert_eq!(w.server_uids("Trash"), vec![1, 2]);
    assert!(w.pending_is_empty());
    // the button asks the same question, and only "Delete 1 forever" deletes
    w.click("mail:delete");
    w.frame(vec![]);
    w.click("mail:forever-confirm");
    w.settle_ops();
    assert_eq!(w.server_uids("Trash"), vec![2], "only the chosen mail is gone; the other program's \\Deleted mail stays");
    let cmds = w.server.state().verbs();
    assert!(cmds.iter().any(|c| c == "UID EXPUNGE 1") && cmds.iter().all(|c| c != "EXPUNGE"), "{cmds:?}");
}

// spec: MAIL-107, MAIL-111
#[test]
fn changes_made_offline_show_as_waiting_survive_and_go_out_when_the_network_returns() {
    let mut w = inbox_with_cursor("offline2");
    w.app.mail.ops_retry = Duration::from_millis(100);
    w.offline.store(true, std::sync::atomic::Ordering::SeqCst);
    w.press(Key::U, Modifiers::NONE);
    w.press(Key::S, Modifiers::NONE);
    w.frame(vec![]);
    w.until("the attempt fails", |a| a.mail.offline.values().next().is_some());
    w.frame(vec![]);
    assert!(w.shows("2 changes are waiting for the server"), "{:?}", w.texts.iter().map(|t| &t.0).collect::<Vec<_>>());
    assert!(w.has("mail:pending-mark:0") && w.has("mail:flag-mark:0"), "shown as done, marked as waiting");
    assert!(w.server_flags("INBOX", 3).is_empty(), "the server was not touched");
    // the network is back: the next retry sends it
    w.offline.store(false, std::sync::atomic::Ordering::SeqCst);
    w.settle_ops();
    w.frame(vec![]);
    assert_eq!(w.server_flags("INBOX", 3), vec!["\\Flagged", "\\Seen"]);
    assert!(!w.has("mail:pending-mark:0") && !w.shows("waiting for the server"));
}

// spec: MAIL-107, MAIL-111
#[test]
fn a_refused_action_is_undone_in_the_list_and_the_person_is_told_why() {
    let mut w = inbox_with_cursor("refused2");
    w.server.state().fail_next_write = Some("[NOPERM] this folder is read-only for you".into());
    w.press(Key::U, Modifiers::NONE);
    w.frame(vec![]);
    assert!(!w.has("mail:unread:0"), "shown as read at first");
    w.until("the refusal arrives", |a| !a.mail.problems.is_empty());
    w.settle_ops();
    w.frame(vec![]);
    assert!(w.has("mail:unread:0"), "and unread again when the server said no");
    assert!(w.shows("Not done:") && w.shows("read-only for you") || w.app.mail.problems.iter().any(|p| p.contains("read-only for you")));
    assert!(w.server_flags("INBOX", 3).is_empty());
    assert!(w.pending_is_empty());
    w.click("mail:problem-dismiss");
    w.frame(vec![]);
    assert!(w.app.mail.problems.is_empty() && !w.has("mail:problem-dismiss"));
}

// spec: MAIL-109
#[test]
fn opening_a_message_marks_it_read_after_the_delay_only_if_the_setting_is_on_and_fetching_stays_peek() {
    // off: nothing is written however long it stays open
    let mut w = W::new("markread-off", true);
    w.app.cfg.mail_mark_read = false;
    w.app.cfg.mail_mark_read_secs = 0;
    w.sync_and_wait();
    w.until("the Inbox is open", |a| a.mail.total == 3);
    w.open_mail("Invoice 2026");
    for _ in 0..30 {
        w.frame(vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(w.server_flags("INBOX", 1).is_empty());
    drop(w);
    // on, with a delay: not at once, then yes
    let mut w = W::new("markread-on", true);
    w.app.cfg.mail_mark_read = true;
    w.app.cfg.mail_mark_read_secs = 1;
    w.sync_and_wait();
    w.until("the Inbox is open", |a| a.mail.total == 3);
    w.open_mail("Invoice 2026");
    let opened = Instant::now();
    for _ in 0..8 {
        w.frame(vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(opened.elapsed() < Duration::from_millis(900) && w.server_flags("INBOX", 1).is_empty(), "not before the delay");
    w.until("it is marked read", |a| a.mail.pending_total == 0 && a.mail.open.as_ref().is_some_and(|o| o.msg.is_seen()));
    w.settle_ops();
    assert_eq!(w.server_flags("INBOX", 1), vec!["\\Seen"]);
    assert!(w.server.state().verbs().iter().all(|v| !v.contains("BODY[") || v.contains("BODY.PEEK[")), "bodies are always fetched with PEEK");
    // closing the message before the delay: it stays unread
    w.open_mail("Lunch plans");
    w.press(Key::Escape, Modifiers::NONE);
    std::thread::sleep(Duration::from_millis(1100));
    for _ in 0..5 {
        w.frame(vec![]);
    }
    assert!(w.server_flags("INBOX", 2).is_empty(), "a message closed before the delay was not read");
}

// spec: MAIL-109
#[test]
fn the_account_settings_have_the_mark_as_read_setting_and_its_delay() {
    let mut w = W::new("readsetting", true);
    w.app.cfg.mail_mark_read = true;
    w.app.cfg.mail_mark_read_secs = 2;
    w.app.open_mail_settings();
    w.settle();
    assert!(w.has("md:mark-read") && w.has("md:mark-read-secs"));
    w.typ("md:mark-read-secs", "x");
    w.click("md:done");
    assert!(matches!(w.app.modal, Some(Modal::MailAccounts(_))), "a bad number keeps the dialog open");
    w.typ("md:mark-read-secs", "5");
    w.click("md:mark-read");
    w.click("md:done");
    assert!(w.app.modal.is_none());
    assert!(!w.app.cfg.mail_mark_read);
    assert_eq!(w.app.cfg.mail_mark_read_secs, 5);
}

// spec: MAIL-202
#[test]
fn queued_actions_of_an_account_that_is_not_configured_any_more_are_never_sent_and_stay_queued() {
    let mut w = inbox_with_cursor("orphan-ops");
    w.app.mail.ops_retry = Duration::from_millis(50);
    w.offline.store(true, std::sync::atomic::Ordering::SeqCst);
    w.press(Key::S, Modifiers::NONE);
    w.until("the attempt fails", |a| a.mail.offline.values().next().is_some());
    let queued = |w: &W| w.app.mail.ready_store().unwrap().reader().unwrap().pending_ops(None).unwrap().len();
    assert_eq!(queued(&w), 1);
    // the account is gone from the settings; a retry started before that (every 50 ms) ends offline first
    w.app.cfg.mail_accounts.clear();
    w.until("no attempt of the old settings is running", |a| !a.mail.busy_with_ops());
    // the network is back
    w.offline.store(false, std::sync::atomic::Ordering::SeqCst);
    let flags = w.server_flags("INBOX", 3);
    let id = w.account.id.clone();
    let cfg = w.app.cfg.clone();
    w.app.mail.start_ops(&cfg, &id);
    assert!(!w.app.mail.busy_with_ops(), "no job is started for an account that is not configured");
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(400) {
        w.frame(vec![]);
        assert!(!w.app.mail.busy_with_ops());
        std::thread::sleep(Duration::from_millis(8));
    }
    assert_eq!(w.server_flags("INBOX", 3), flags, "nothing reached the server");
    assert_eq!(queued(&w), 1, "and nothing was deleted from the queue");
}

/// A world after a sync, with `n` local sent copies whose queued append is gone (as after a restore from a backup).
fn world_with_unsent(tag: &str, n: u32) -> W {
    let mut w = W::new(tag, true);
    w.sync_and_wait();
    let store = w.app.mail.ready_store().unwrap();
    for i in 0..n {
        let raw = dated(&format!("Lost copy {i}"), "ada@example.com", "only here", 14 + i);
        crate::mail_ops::record_sent_copy(&store, &w.account.id, &raw, NOW + i64::from(i)).unwrap().unwrap();
    }
    let ids: Vec<i64> = store.reader().unwrap().pending_ops(None).unwrap().iter().map(|p| p.id).collect();
    store.commit(ids.into_iter().map(|id| crate::mail_db::Op::PendingDone { id }).collect()).unwrap();
    w.app.mail.refresh();
    w.frame(vec![]);
    w
}

// spec: MAIL-214
#[test]
fn sent_copies_that_only_exist_here_are_announced_and_uploaded_again_once_even_when_clicked_twice() {
    let mut w = world_with_unsent("unsent-up", 2);
    assert!(w.shows("2 sent mails are only on this computer"), "{:?}", w.texts);
    assert_eq!(w.server_uids("Sent").len(), 1);
    w.click("mail:unsent-open");
    w.settle();
    assert!(w.shows("Lost copy 0") && w.shows("Lost copy 1"));
    assert!(w.shows("Test mail"), "the account is named");
    // two quick clicks on the same row: the second works on the stale list and queues nothing
    let stale = w.app.mail.unsent.clone();
    let cfg = w.app.cfg.clone();
    assert_eq!(w.app.mail.unsent_upload(&cfg, &stale[..1]), 1);
    assert_eq!(w.app.mail.unsent_upload(&cfg, &stale[..1]), 1);
    assert_eq!(w.app.mail.ready_store().unwrap().reader().unwrap().pending_ops(None).unwrap().len(), 1, "one append queued");
    w.settle_ops();
    assert_eq!(w.server_uids("Sent").len(), 2, "one copy arrived");
    // the other one through the button of the list
    w.click("mail:unsent-upload-all");
    w.settle_ops();
    assert_eq!(w.server_uids("Sent").len(), 3);
    w.frame(vec![]);
    assert!(w.app.mail.unsent.is_empty());
    assert!(!w.shows("only on this computer"));
}

// spec: MAIL-212
#[test]
fn dismissing_in_the_list_keeps_the_mail_and_stops_the_notice() {
    let mut w = world_with_unsent("unsent-dismiss", 2);
    w.click("mail:unsent-open");
    w.settle();
    w.click("mail:unsent-dismiss:0");
    w.frame(vec![]);
    assert_eq!(w.app.mail.unsent.len(), 1);
    assert!(w.shows("1 sent mail is only on this computer"), "{:?}", w.texts);
    w.click("mail:unsent-dismiss-all");
    w.frame(vec![]);
    assert!(w.app.mail.unsent.is_empty() && !w.has("mail:unsent-notice"));
    assert_eq!(w.server_uids("Sent").len(), 1, "nothing was uploaded");
    let r = w.app.mail.ready_store().unwrap().reader().unwrap();
    let sent = w.folder_id("Sent");
    assert_eq!(r.list(sent, 10, 0).unwrap().iter().filter(|m| m.uidvalidity == 0).count(), 2, "both stay in the local Sent folder");
}

// spec: MAIL-213
#[test]
fn a_copy_of_an_account_that_is_not_set_up_any_more_can_only_be_dismissed() {
    let mut w = world_with_unsent("unsent-gone", 1);
    // the account of the copy was removed; another one is set up
    w.app.cfg.mail_accounts = vec![MailAccount { id: "other".into(), name: "Other".into(), host: "localhost".into(), ..MailAccount::default() }];
    w.frame(vec![]);
    w.click("mail:unsent-open");
    w.settle();
    assert!(w.shows("not set up any more"));
    assert!(!w.has("mail:unsent-upload:0") && !w.has("mail:unsent-upload-all"));
    assert!(w.has("mail:unsent-dismiss:0"));
    // even asked directly, nothing is queued for it
    let stale = w.app.mail.unsent.clone();
    let cfg = w.app.cfg.clone();
    assert_eq!(w.app.mail.unsent_upload(&cfg, &stale), 0);
    assert!(w.app.mail.ready_store().unwrap().reader().unwrap().pending_ops(None).unwrap().is_empty());
}

// spec: MAIL-201
#[test]
fn an_open_draft_is_saved_when_the_app_closes_without_waiting_for_the_autosave() {
    let (mut w, _smtp) = compose_world("exit-draft", 10);
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:to", "bob@x.de");
    w.typ("mail:compose:subject", "Typed just before closing");
    w.write_body("these words must survive");
    assert!(w.drafts_on_disk().is_empty(), "the autosave has not run yet");
    // the question dialog pauses the autosave; closing the app must save anyway
    w.app.mail.compose.as_mut().unwrap().ask = Some(crate::mail_compose::Ask::Close);
    eframe::App::on_exit(&mut w.app);
    let ob = crate::outbox::Outbox::new(&w.app.mail.root);
    let drafts = w.drafts_on_disk();
    assert_eq!(drafts.len(), 1);
    let (d, problems) = ob.load_draft(&drafts[0].account, &drafts[0].id).unwrap();
    assert!(problems.is_empty());
    assert_eq!((d.subject.as_str(), d.to.as_str()), ("Typed just before closing", "bob@x.de"));
    assert!(d.body.contains("these words must survive"), "{}", d.body);
}

// spec: MAIL-201
#[test]
fn when_the_draft_cannot_be_saved_at_exit_its_text_goes_to_the_recovery_folder() {
    let (mut w, _smtp) = compose_world("exit-rescue", 10);
    w.app.recovery_root = w.dir.join("recovery");
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:to", "bob@x.de");
    w.typ("mail:compose:subject", "Rescue me");
    w.write_body("unsaved words");
    // a file where the drafts folder should be: the save fails
    let drafts = w.app.mail.root.join("drafts");
    let _ = std::fs::remove_dir_all(&drafts);
    std::fs::write(&drafts, b"in the way").unwrap();
    assert!(w.app.mail.flush_draft().is_err());
    eframe::App::on_exit(&mut w.app);
    let mut found = vec![];
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                walk(&e.path(), out);
            } else {
                out.push(e.path());
            }
        }
    }
    walk(&w.dir.join("recovery"), &mut found);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].to_string_lossy().contains("mail-drafts"));
    let text = std::fs::read_to_string(&found[0]).unwrap();
    assert!(text.contains("bob@x.de") && text.contains("Rescue me") && text.contains("unsaved words"), "{text}");
}

// ---------------------------------------------------------------- phase 3: writing and sending (MAIL-120…MAIL-151)

use crate::mail_compose::LocalView;
use crate::outbox::State as OutState;
use crate::smtp_fake_tests::FakeSmtp;

/// An app with the pretend IMAP server synced and a pretend SMTP server wired in; the undo time is `undo` seconds.
fn compose_world(tag: &str, undo: u32) -> (W, FakeSmtp) {
    let mut w = W::new(tag, true);
    let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
    {
        let a = &mut w.app.cfg.mail_accounts[0];
        a.smtp_host = "localhost".into();
        a.smtp_port = smtp.port;
        a.from_name = "Ada Lovelace".into();
        a.from_address = "ada@example.com".into();
        a.signature = "Ada".into();
    }
    w.app.cfg.mail_undo_secs = undo;
    let (settings, user, password) = {
        let g = smtp.state();
        (smtp.settings(), g.user.clone(), g.password.clone())
    };
    w.app.mail.smtp_factory = Arc::new(move |_, _| {
        Ok(Arc::new(crate::mail_send::tests::TestSmtp { settings: settings.clone(), user: user.clone(), password: password.clone(), connects: std::sync::atomic::AtomicUsize::new(0) }) as Arc<dyn crate::mail_send::SmtpConnector>)
    });
    w.sync_and_wait();
    w.until("the Inbox is open", |a| a.mail.total == 3);
    w.until("the start-up check is done", |a| a.mail.outbox.is_some() && a.mail.outbox_ready());
    w.frame(vec![]);
    (w, smtp)
}

impl W {
    /// A key with the text the keyboard would type with it.
    fn press_text(&mut self, key: Key, modifiers: Modifiers, text: &str) {
        self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }, Event::Text(text.into())]);
        self.frame(vec![]);
    }

    fn compose_field(&self, f: &str) -> String {
        let c = self.app.mail.compose.as_ref().expect("a compose window is open");
        match f {
            "to" => c.draft.to.clone(),
            "cc" => c.draft.cc.clone(),
            "bcc" => c.draft.bcc.clone(),
            "subject" => c.draft.subject.clone(),
            _ => c.draft.body.clone(),
        }
    }

    /// Type into the body, above the signature.
    fn write_body(&mut self, text: &str) {
        self.click("mail:compose:body");
        self.press(Key::ArrowUp, Modifiers::COMMAND);
        self.type_text(text);
    }

    fn compose_open(&self) -> bool {
        self.app.mail.compose.is_some()
    }

    fn ctrl_enter(&mut self) {
        self.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
        self.frame(vec![]);
    }

    fn esc(&mut self) {
        self.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        self.frame(vec![]);
    }

    fn items(&self) -> Vec<crate::outbox::Item> {
        crate::outbox::Outbox::new(&self.app.mail.root).list(None)
    }

    fn drafts_on_disk(&self) -> Vec<crate::outbox::DraftInfo> {
        crate::outbox::Outbox::new(&self.app.mail.root).list_drafts(None)
    }
}

// spec: MAIL-150, MAIL-151, MAIL-132, MAIL-141
#[test]
fn compose_with_c_complete_an_address_send_with_ctrl_enter_and_the_mail_appears_in_sent() {
    let (mut w, smtp) = compose_world("send1", 1);
    w.press_text(Key::C, Modifiers::NONE, "c");
    assert!(w.compose_open(), "c opens a new mail");
    assert_eq!(w.compose_field("to"), "", "the letter c was not typed into the window");
    assert!(w.shows("Ada Lovelace") && w.shows("ada@example.com"), "the sender is shown");
    assert!(w.compose_field("body").contains("-- \nAda"), "the signature is below the cursor");
    // completion: "bo" offers Bob, from the addresses in the mail index; a click takes it
    w.click("mail:compose:to");
    w.type_text("bo");
    w.frame(vec![]);
    assert!(w.has("mail:suggest:0"), "a suggestion is offered for what is typed");
    w.click("mail:suggest:0");
    assert!(w.compose_field("to").contains("bob@x.de"), "{}", w.compose_field("to"));
    w.typ("mail:compose:subject", "Plans für Köln");
    w.write_body("Hello Bob, the plans are ready.");
    w.ctrl_enter();
    assert!(!w.compose_open(), "Ctrl+Enter sends: the window closes");
    assert_eq!(w.items().len(), 1);
    assert!(w.has("mail:undo-send") || w.items()[0].state != OutState::Queued, "the mail waits for the undo time");
    // sent after the undo time, exactly once
    w.until("the mail was sent", |_| !smtp.state().received.is_empty());
    w.until("the outbox says sent and the copy is made", |a| a.mail.pending_total == 0 && !a.mail.busy_with_ops() && crate::outbox::Outbox::new(&a.mail.root).list(None).iter().all(|i| i.state == OutState::Sent && i.copy_done));
    assert_eq!(smtp.state().received.len(), 1);
    let got = crate::mail::parse(&smtp.state().received[0].data);
    assert_eq!(got.subject, "Plans für Köln");
    assert!(got.to.contains("bob@x.de") && got.text.contains("Hello Bob, the plans are ready."), "{got:?}");
    assert_eq!(smtp.state().received[0].rcpts, vec!["bob@x.de"]);
    w.settle_ops();
    // the queued append may not be counted yet when the wait above ends: wait for the copy on the server itself
    let t0 = Instant::now();
    while w.server.state().messages("Sent").len() < 2 {
        assert!(t0.elapsed() < Duration::from_secs(20), "the copy never reached the Sent folder on the server");
        w.frame(vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    // the Sent folder on the server has it, flagged read, once; and the list shows it
    let on = w.server.state().messages("Sent");
    assert_eq!(on.len(), 2, "the seeded one and ours");
    assert_eq!(on.iter().filter(|m| crate::mail::parse(&m.2).subject == "Plans für Köln").count(), 1);
    w.open_folder("Sent");
    w.until("the Sent list has it", |a| a.mail.total == 2);
    assert!(w.subjects().contains(&"Plans für Köln".to_string()));
}

// spec: MAIL-126, MAIL-150
#[test]
fn reply_reply_all_and_forward_start_with_the_right_fields_and_quote_the_original() {
    let (mut w, _smtp) = compose_world("answer1", 10);
    w.open_mail("Invoice 2026"); // from Bob, to ada
    w.click("mail:reply");
    assert!(
        w.compose_open() || {
            w.until("the answer opens", |a| a.mail.compose.is_some());
            true
        }
    );
    assert_eq!(w.compose_field("to"), "Bob <bob@x.de>");
    assert_eq!(w.compose_field("subject"), "Re: Invoice 2026");
    let body = w.compose_field("body");
    assert!(body.contains(" wrote:\n> Please pay the invoice for the lecture hall."), "{body}");
    let d = w.app.mail.compose.as_ref().unwrap().draft.clone();
    assert_eq!(d.in_reply_to, "<10-Invoice-2026@fake>");
    assert_eq!(d.references, vec!["<10-Invoice-2026@fake>"]);
    // Esc on an untouched answer closes without asking and leaves no draft
    w.esc();
    assert!(!w.compose_open(), "a mail the person has not typed into closes without a question");
    assert!(w.drafts_on_disk().is_empty());
    // Shift+R: reply all (the mail went to ada only: the same as a reply); Shift+F: forward with the attachment of another mail
    w.press_text(Key::R, Modifiers::SHIFT, "R");
    w.until("reply all opens", |a| a.mail.compose.is_some());
    assert_eq!(w.compose_field("to"), "Bob <bob@x.de>");
    w.esc();
    w.open_mail("Report attached");
    w.press_text(Key::F, Modifiers::SHIFT, "F");
    w.until("forward opens", |a| a.mail.compose.is_some());
    assert_eq!(w.compose_field("subject"), "Fwd: Report attached");
    assert_eq!(w.compose_field("to"), "");
    let atts = w.app.mail.compose.as_ref().unwrap().draft.attachments.clone();
    assert_eq!((atts.len(), atts[0].name.as_str(), atts[0].data.as_slice()), (1, "report.pdf", &b"Hello PDF"[..]), "the original's attachment comes along");
    assert!(w.shows("report.pdf"));
    w.esc();
    // f is still the flag key
    assert!(!w.compose_open());
    w.open_mail("Lunch plans");
    w.press_text(Key::F, Modifiers::NONE, "f");
    w.settle_ops();
    assert!(w.server_flags("INBOX", 2).contains(&"\\Flagged".to_string()), "plain f flags, Shift+F forwards");
}

// spec: MAIL-134
#[test]
fn undo_send_pulls_the_mail_back_into_the_editor_and_nothing_goes_out() {
    let (mut w, smtp) = compose_world("undo1", 30);
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:to", "bob@x.de");
    w.typ("mail:compose:subject", "Take me back");
    w.write_body("precious words");
    w.click("mail:compose:send");
    assert!(!w.compose_open(), "{:?} {:?}", w.app.mail.compose.as_ref().map(|c| c.error.clone()), w.app.mail.compose.as_ref().map(|c| (c.draft.to.clone(), c.draft.subject.clone(), c.ask.clone())));
    w.until("the strip with Undo shows", |a| a.mail.rects.iter().any(|r| r.0 == "mail:undo-send"));
    assert_eq!(w.items()[0].state, OutState::Queued);
    w.click("mail:undo-send");
    w.until("the editor is back", |a| a.mail.compose.is_some());
    assert_eq!((w.compose_field("subject").as_str(), w.compose_field("to").as_str()), ("Take me back", "bob@x.de"));
    assert!(w.compose_field("body").contains("precious words"));
    assert!(w.items().is_empty(), "the outbox is empty again");
    for _ in 0..30 {
        w.frame(vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(smtp.state().received.is_empty() && smtp.state().commands.is_empty(), "nothing was sent, not even a connection");
    // 0 seconds: sent at once (no strip needed)
    w.app.cfg.mail_undo_secs = 0;
    w.click("mail:compose:send");
    w.until("sent at once", |_| smtp.state().received.len() == 1);
}

// spec: MAIL-135
#[test]
fn a_forgotten_attachment_an_empty_subject_and_an_empty_body_each_ask_before_sending() {
    let (mut w, smtp) = compose_world("confirm1", 0);
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:to", "bob@x.de");
    w.typ("mail:compose:subject", "Plans");
    w.write_body("Please see the attached plan.");
    w.click("mail:compose:send");
    assert!(w.has("mail:compose:confirm-yes"), "the text mentions an attachment, nothing is attached");
    w.settle();
    assert!(w.shows("mentions an attachment"));
    assert!(w.compose_open() && w.items().is_empty() && smtp.state().commands.is_empty());
    w.click("mail:compose:confirm-no");
    assert!(w.compose_open() && !w.has("mail:compose:confirm-yes"), "back to the mail");
    // no subject and attachment words: two questions, one after the other
    w.click("mail:compose:subject");
    w.press(Key::A, Modifiers::COMMAND);
    w.press(Key::Delete, Modifiers::NONE);
    assert_eq!(w.compose_field("subject"), "");
    w.ctrl_enter();
    w.settle();
    assert!(w.shows("no subject"));
    w.click("mail:compose:confirm-yes");
    w.settle();
    assert!(w.shows("mentions an attachment"), "the next question");
    w.click("mail:compose:confirm-yes");
    assert!(!w.compose_open(), "now it goes");
    w.until("sent", |_| smtp.state().received.len() == 1);
    // an empty body
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:to", "bob@x.de");
    w.typ("mail:compose:subject", "Only a subject");
    w.click("mail:compose:body");
    w.press(Key::A, Modifiers::COMMAND);
    w.press(Key::Delete, Modifiers::NONE);
    w.click("mail:compose:send");
    w.settle();
    assert!(w.shows("has no text"), "an empty body asks");
    w.click("mail:compose:confirm-no");
    // an attachment from the vault answers the reminder
    w.type_text("see attached");
    w.typ("mail:compose:att-path", &w.dir.join("Welcome.md").to_string_lossy());
    w.click("mail:compose:att-add");
    assert_eq!(w.app.mail.compose.as_ref().unwrap().draft.attachments.len(), 1);
    w.click("mail:compose:send");
    assert!(!w.has("mail:compose:confirm-yes"), "nothing to ask: the attachment is there");
    w.until("the second mail was sent", |_| smtp.state().received.len() == 2);
    let m = crate::mail::parse(&smtp.state().received[1].data);
    assert_eq!((m.attachments.len(), m.attachments[0].name.as_str()), (1, "Welcome.md"));
}

// spec: MAIL-130, MAIL-131
#[test]
fn closing_with_changes_asks_keep_or_discard_and_nothing_is_lost_either_way() {
    let (mut w, _smtp) = compose_world("close1", 10);
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.esc();
    assert!(!w.compose_open(), "nothing typed: closes without a question");
    assert!(w.drafts_on_disk().is_empty());
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:subject", "Unsent thoughts");
    w.esc();
    assert!(w.compose_open() && w.has("mail:compose:keep") && w.has("mail:compose:discard"), "asked: keep draft / discard");
    // Esc again (or Keep writing) goes back to the mail
    w.esc();
    assert!(w.compose_open() && !w.has("mail:compose:keep"));
    // Discard: the draft goes to the trash folder, not away
    w.esc();
    w.click("mail:compose:discard");
    assert!(!w.compose_open());
    assert!(w.drafts_on_disk().is_empty());
    let trashed: Vec<_> = std::fs::read_dir(w.app.mail.root.join(".trash")).unwrap().flatten().collect();
    assert!(!trashed.is_empty(), "the discarded draft is in .trash");
    // Keep draft: it is in Drafts, and a click opens it again with everything in it
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:subject", "Kept for later");
    w.write_body("half a sentence");
    w.click("mail:compose:close");
    w.click("mail:compose:keep");
    assert!(!w.compose_open());
    let acct = w.account.id.clone();
    assert_eq!(w.drafts_on_disk().len(), 1);
    w.until("the folder list has Drafts (1)", |a| a.mail.rects.iter().any(|r| r.0 == format!("mail:drafts:{}", acct)));
    w.click(&format!("mail:drafts:{acct}"));
    w.until("the draft is listed", |a| a.mail.rects.iter().any(|r| r.0 == "mail:draft:0"));
    assert!(w.shows("Kept for later"));
    w.click("mail:draft:0");
    w.until("the draft opens", |a| a.mail.compose.is_some());
    assert_eq!(w.compose_field("subject"), "Kept for later");
    assert!(w.compose_field("body").contains("half a sentence"));
    // closing it again without a change asks nothing and keeps it
    w.esc();
    assert!(!w.compose_open());
    assert_eq!(w.drafts_on_disk().len(), 1);
    assert!(matches!(w.app.mail.local_view, Some(LocalView::Drafts(_))));
}

// spec: MAIL-130
#[test]
fn a_draft_is_saved_by_itself_after_a_pause_and_a_crash_leaves_it_in_drafts() {
    let (mut w, _smtp) = compose_world("autosave1", 10);
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:subject", "Autosaved");
    w.write_body("typed before the crash");
    assert!(w.drafts_on_disk().is_empty(), "not yet: the pause has not passed");
    w.until("the draft was saved by itself", |a| crate::outbox::Outbox::new(&a.mail.root).list_drafts(None).len() == 1);
    // the program dies here: a new run finds it
    let root = w.app.mail.root.clone();
    let ob = crate::outbox::Outbox::new(&root);
    let r = ob.recover(NOW, "2026-11-14").unwrap();
    assert_eq!(r, crate::outbox::Recovery::default());
    let drafts = ob.list_drafts(None);
    assert_eq!(drafts.len(), 1);
    let (d, problems) = ob.load_draft(&drafts[0].account, &drafts[0].id).unwrap();
    assert!(problems.is_empty());
    assert_eq!(d.subject, "Autosaved");
    assert!(d.body.contains("typed before the crash"));
    // a new mail view announces what it finds
    let mut view = crate::mail_ui::MailView::new(root);
    view.open_store();
    let cfg = w.app.cfg.clone();
    let start = Instant::now();
    while view.notices.iter().all(|n| !n.contains("unsent draft")) {
        view.tick(&cfg, true);
        assert!(start.elapsed() < Duration::from_secs(20), "the restored draft is announced: {:?}", view.notices);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(view.local_drafts.len(), 1);
}

// spec: MAIL-133, MAIL-136
#[test]
fn a_mail_that_may_have_been_sent_waits_for_the_person_and_a_failed_one_can_be_retried() {
    let (mut w, smtp) = compose_world("unsure1", 0);
    smtp.state().drop_after_data = true;
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:to", "bob@x.de");
    w.typ("mail:compose:subject", "Did it go");
    w.write_body("hello");
    w.ctrl_enter();
    w.until("the mail is unsure", |a| a.mail.local_items.iter().any(|i| i.state == OutState::Unsure));
    assert_eq!(smtp.state().received.len(), 1, "it did arrive: nobody could have known");
    assert!(w.app.mail.problems.iter().any(|p| p.contains("may have been sent")));
    let acct = w.account.id.clone();
    w.click(&format!("mail:outbox:{acct}"));
    w.until("the entry is listed", |a| a.mail.rects.iter().any(|r| r.0 == "mail:outbox:0"));
    w.click("mail:outbox:0");
    w.frame(vec![]);
    assert!(w.shows("may have been sent") && w.shows("Check your Sent folder"), "the entry says what to do");
    // nothing resends it by itself
    for _ in 0..40 {
        w.frame(vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(smtp.state().received.len(), 1);
    // Send again is the person's choice
    smtp.state().drop_after_data = false;
    w.click("mail:outbox:send-again");
    w.until("sent again", |_| smtp.state().received.len() == 2);
    // a refused recipient: failed, with the server's words, retry sends
    smtp.state().fail_rcpt = Some("bob@x.de".into());
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:to", "bob@x.de");
    w.typ("mail:compose:subject", "Refused");
    w.write_body("hello");
    w.ctrl_enter();
    // the entry is Failed on disk before the job's answer (with the server's words) arrives: wait for both
    w.until("the mail failed and the server's words are shown", |a| a.mail.local_items.iter().any(|i| i.subject == "Refused" && i.state == OutState::Failed) && a.mail.problems.iter().any(|p| p.contains("550")));
    w.frame(vec![]);
    w.until("both entries listed", |a| a.mail.local_items.iter().filter(|i| i.state != OutState::Sent).count() >= 1);
    let i = w.app.mail.local_items.iter().filter(|i| i.account == acct).position(|i| i.subject == "Refused").unwrap();
    w.click(&format!("mail:outbox:{i}"));
    w.frame(vec![]);
    assert!(w.shows("was not sent") && w.shows("550"));
    smtp.state().fail_rcpt = None;
    w.click("mail:outbox:retry");
    w.until("the retry went out", |_| smtp.state().received.len() == 3);
}

// spec: MAIL-120, MAIL-121
#[test]
fn the_account_settings_have_the_sending_section_with_presets_and_test_connection_signs_in_to_smtp() {
    let mut w = W::new("smtp-dialog", false);
    let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
    let (settings, user, password) = {
        let g = smtp.state();
        (smtp.settings(), g.user.clone(), g.password.clone())
    };
    w.app.mail.smtp_factory = Arc::new(move |_, _| {
        Ok(Arc::new(crate::mail_send::tests::TestSmtp { settings: settings.clone(), user: user.clone(), password: password.clone(), connects: std::sync::atomic::AtomicUsize::new(0) }) as Arc<dyn crate::mail_send::SmtpConnector>)
    });
    w.app.open_mail_settings();
    w.settle();
    w.click("md:add");
    w.typ("md:host", "localhost");
    w.typ("md:port", &w.server.port.to_string());
    w.typ("md:user", "ada");
    w.typ("md:password", PASSWORD);
    // the Sending section is folded; opening it shows the fields
    assert!(!w.has("md:smtp-host"));
    w.click("md:smtp-header");
    w.settle();
    w.typ("md:smtp-host", "localhost");
    w.typ("md:smtp-port", &smtp.port.to_string());
    w.typ("md:from-name", "Ada Lovelace");
    w.typ("md:from-addr", "ada@example.com");
    w.click("md:test");
    w.until("the test ends", |a| a.mail.tests.values().any(|t| matches!(t, TestState::Ok(_) | TestState::Failed(_))));
    let t = w.app.mail.tests.values().next().unwrap();
    assert!(
        matches!(t, TestState::Ok(m) if m.contains("Sending works")),
        "{}",
        match t {
            TestState::Failed(e) => e.clone(),
            TestState::Ok(m) => m.clone(),
            _ => "?".into(),
        }
    );
    assert!(smtp.state().received.is_empty(), "testing never sends a mail");
    assert!(smtp.commands().iter().all(|c| !c.starts_with("MAIL FROM") && !c.starts_with("DATA")), "{:?}", smtp.commands());
    assert!(smtp.commands().iter().any(|c| c.starts_with("AUTH")));
    // a wrong password for SMTP is told in words
    smtp.state().password = "other".into();
    w.click("md:test");
    w.until("the second test ends", |a| a.mail.tests.values().any(|t| matches!(t, TestState::Failed(_))));
    let TestState::Failed(e) = w.app.mail.tests.values().next().unwrap() else { unreachable!() };
    assert!(e.contains("Reading works") && e.contains("535"), "{e}");
    // Done saves the sending settings
    w.click("md:done");
    let a = &w.app.cfg.mail_accounts[0];
    assert_eq!((a.smtp_host.as_str(), a.smtp_port, a.from_name.as_str(), a.from_address.as_str()), ("localhost", smtp.port, "Ada Lovelace", "ada@example.com"));
}

// spec: MAIL-127, MAIL-128
#[test]
fn attachments_come_from_the_vault_and_notes_and_addresses_from_the_people_notes() {
    let (mut w, smtp) = compose_world("attach2", 0);
    std::fs::create_dir_all(w.dir.join("Adressbuch")).unwrap();
    std::fs::write(w.dir.join("Adressbuch/Dora Dobler.md"), "# Dora Dobler\n- E-mail: dora.dobler@people.example\n").unwrap();
    std::fs::write(w.dir.join("Plan.md"), "# The plan\n\n- Größe äöü\n").unwrap();
    std::fs::create_dir_all(w.dir.join("z_media")).unwrap();
    std::fs::write(w.dir.join("z_media/picture.png"), [0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
    w.app.actions.push(crate::app::Action::Rescan);
    w.until("the vault is known to the mail view", |a| a.mail.vault.as_ref().is_some_and(|v| v.files.iter().any(|f| f.0 == "z_media/picture.png") && v.people.iter().any(|p| p.1 == "dora.dobler@people.example")));
    w.press_text(Key::C, Modifiers::NONE, "c");
    // a name from a people note is completed
    w.click("mail:compose:to");
    w.type_text("dobl");
    w.frame(vec![]);
    assert!(w.has("mail:suggest:0") && w.shows("dora.dobler@people.example"), "completed from the people notes");
    w.click("mail:suggest:0");
    assert!(w.compose_field("to").contains("Dora Dobler <dora.dobler@people.example>"), "{}", w.compose_field("to"));
    w.typ("mail:compose:subject", "Files");
    w.write_body("Hello");
    // a file of the vault, from its media folder
    w.click("mail:compose:att-vault");
    w.settle();
    w.type_text("picture");
    w.frame(vec![]);
    w.click("mail:picker:0");
    // a note, as markdown
    w.click("mail:compose:att-note");
    w.settle();
    w.type_text("plan");
    w.frame(vec![]);
    w.click("mail:picker:0");
    let atts = w.app.mail.compose.as_ref().unwrap().draft.attachments.clone();
    assert_eq!(atts.iter().map(|a| (a.name.as_str(), a.mime.as_str())).collect::<Vec<_>>(), vec![("picture.png", "image/png"), ("Plan.md", "text/markdown")]);
    assert_eq!(atts[0].data.as_slice(), &[0x89, b'P', b'N', b'G', 1, 2, 3]);
    assert_eq!(atts[1].data.as_slice(), "# The plan\n\n- Größe äöü\n".as_bytes());
    w.click("mail:compose:att-remove:0");
    assert_eq!(w.app.mail.compose.as_ref().unwrap().draft.attachments.len(), 1);
    w.click("mail:compose:send");
    w.until("sent", |_| smtp.state().received.len() == 1);
    let m = crate::mail::parse(&smtp.state().received[0].data);
    assert_eq!((m.attachments.len(), m.attachments[0].name.as_str(), m.attachments[0].mime.as_str()), (1, "Plan.md", "text/markdown"));
    assert!(m.to.contains("dora.dobler@people.example"));
    // a file that is not there is told, not attached
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:att-path", "/nonexistent/file.pdf");
    w.click("mail:compose:att-add");
    assert!(w.app.mail.compose.as_ref().unwrap().error.as_deref().is_some_and(|e| e.contains("nonexistent")));
    assert!(w.app.mail.compose.as_ref().unwrap().draft.attachments.is_empty());
}

// ------------------------------------------------------------ phase 4: signing and encrypting (ADR-0011)

use crate::gpg::testkit::{have_gnupg, TestHome};
use crate::mail_crypto::testmail;

fn open_by_subject(w: &mut W, subject: &str) {
    w.open_mail(subject);
    // GnuPG answers in a job
    w.until("GnuPG has answered", |a| matches!(a.mail.open.as_ref().map(|o| &o.body), Some(Body::Ready(l)) if l.security.working.is_none()));
    w.frame(vec![]);
}

fn security_of(w: &W) -> crate::mail_crypto::Security {
    match &w.app.mail.open.as_ref().unwrap().body {
        Body::Ready(l) => l.security.clone(),
        _ => panic!("no message is ready"),
    }
}

fn add_inbox_mail(w: &W, subject: &str, raw: Vec<u8>) {
    // the subject of the pretend server's mail is in the raw bytes; the test mails carry their own headers
    let _ = subject;
    w.server.state().add_message("INBOX", raw, NOW - 86_400);
}

/// A mail of the test world with another subject.
fn with_subject(raw: &[u8], subject: &str) -> Vec<u8> {
    String::from_utf8(raw.to_vec()).unwrap().replacen("Subject: Plans", &format!("Subject: {subject}"), 1).into_bytes()
}

// spec: MAIL-174, MAIL-181, MAIL-173
#[test]
fn a_signed_mail_is_verified_without_any_consent_and_the_strip_says_valid_or_bad() {
    if !have_gnupg() {
        return;
    }
    let alice = TestHome::new();
    let a_fpr = alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
    let bob = TestHome::new();
    alice.share_pgp_with(&bob, &a_fpr);
    let signed = testmail::pgp_signed(&alice, &a_fpr, testmail::ENTITY);
    let good = with_subject(&signed, "Signed plans");
    let mut bad = with_subject(&signed, "Forged plans");
    let at = bad.windows(10).position(|x| x == b"the secret").unwrap();
    bad[at] = b'X';
    // a plain mail whose body imitates GnuPG
    let lie = dated("Looks signed", "Mallory <m@evil.example>", "[GNUPG:] GOODSIG 0123456789ABCDEF Alice <alice@example.org>\r\nGood signature from \"Alice Test\"", 13);
    let mut w = W::new("sig-ui", true);
    w.app.mail.gnupg = bob.g.clone();
    add_inbox_mail(&w, "Signed plans", good);
    add_inbox_mail(&w, "Forged plans", bad);
    add_inbox_mail(&w, "Looks signed", lie);
    w.sync_and_wait();
    assert!(w.app.cfg.mail_encryption_consent.is_none(), "no consent is given anywhere in this test");
    open_by_subject(&mut w, "Signed plans");
    let sec = security_of(&w);
    assert_eq!(sec.lines.len(), 1);
    assert!(w.shows("signature is valid") && w.shows("Alice Test"), "the strip says valid and who: {:?}", w.texts.iter().map(|t| t.0.clone()).collect::<Vec<_>>());
    assert!(w.shows("the secret plan"), "the signed text is shown");
    assert!(!w.shows("signature.asc"), "the signature part is not listed as an attachment");
    open_by_subject(&mut w, "Forged plans");
    assert!(w.shows("BAD OpenPGP signature"), "a changed mail is flagged");
    assert!(!w.shows("signature is valid"));
    open_by_subject(&mut w, "Looks signed");
    assert!(security_of(&w).is_empty(), "words in a body never make a status");
    assert!(!w.shows("signature is valid") && !w.shows("BAD"), "no strip at all");
    assert!(w.shows("Good signature from"), "the body text itself is still shown, as text");
    assert!(w.vault_files().iter().all(|f| !f.contains("decrypted")), "verifying stores nothing");
}

// spec: MAIL-180, MAIL-181, MAIL-182, MAIL-183, MAIL-184
#[test]
fn encrypted_mail_needs_the_typed_consent_then_is_decrypted_stored_and_searchable_and_withdrawing_stops_it() {
    if !have_gnupg() {
        return;
    }
    let alice = TestHome::new();
    alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
    let bob = TestHome::new();
    let b_fpr = bob.gen_pgp("Bob", "bob@example.org", "never", None);
    bob.share_pgp_with(&alice, &b_fpr);
    let enc = |subject: &str, text: &str| {
        let entity = format!("Content-Type: text/plain; charset=utf-8\r\n\r\n{text}\r\n");
        with_subject(&testmail::pgp_encrypted(&alice.pgp_encrypt(&[&b_fpr], None, entity.as_bytes())), subject)
    };
    let mut w = W::new("enc-ui", true);
    w.app.mail.gnupg = bob.g.clone();
    add_inbox_mail(&w, "Zebra plans", enc("Zebra plans", "the zebrafish plan"));
    add_inbox_mail(&w, "Okapi plans", enc("Okapi plans", "the okapi plan"));
    w.sync_and_wait();
    // 1. no consent: the strip says so, nothing is decrypted, nothing is stored
    w.open_mail("Zebra plans");
    w.frame(vec![]);
    assert_eq!(security_of(&w).encryption, crate::mail_crypto::Encryption::NeedsConsent);
    assert!(w.shows("Encrypted — enable encryption in the settings"));
    assert!(!w.shows("zebrafish"));
    let store_files = |w: &W| -> Vec<String> {
        fn walk(d: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.push(p.to_string_lossy().into_owned());
                }
            }
        }
        let mut v = vec![];
        walk(&w.app.mail.root, &mut v);
        v
    };
    assert!(store_files(&w).iter().all(|f| !f.contains(".decrypted.")), "{:?}", store_files(&w));
    // 2. the button leads to the settings; the red banner and the field are there
    w.click("mail:sec:settings");
    assert!(matches!(w.app.modal, Some(Modal::MailAccounts(_))));
    w.settle();
    w.click("md:enc-header");
    w.settle();
    assert!(w.has("md:enc-banner") && w.shows("ENCRYPTED MAILS WILL ALWAYS BE STORED DECRYPTED LOCALLY"));
    // GnuPG is detected (in a job), its secret keys are listed, and the way to bring keys over from Thunderbird is explained (MAIL-191)
    w.until("GnuPG was looked at", |a| a.mail.gnupg_info.is_some());
    w.frame(vec![]);
    assert!(w.shows("OpenPGP (gpg): gpg (GnuPG)") && w.shows("S/MIME (gpgsm): gpgsm (GnuPG)"), "{:?}", w.texts.iter().map(|t| t.0.clone()).collect::<Vec<_>>());
    assert!(w.shows("Bob <bob@example.org>"), "the secret key of the test home is listed");
    assert!(w.shows("gpg --import") && w.shows("Thunderbird") && w.shows("gpgsm --import") && w.shows("gpg --locate-keys"));
    let consent = |w: &W| match &w.app.modal {
        Some(Modal::MailAccounts(d)) => d.consent.clone(),
        _ => panic!("no dialog"),
    };
    // 3. anything but the exact sentence leaves the button dead
    for wrong in ["i agree to decrypted local storage", "I agree to decrypted local storage ", " I agree to decrypted local storage", "I agree to decrypted local storage.", "I agree"] {
        w.typ("md:enc-phrase", wrong);
        w.click("md:enc-enable");
        assert_eq!(consent(&w), None, "{wrong:?} must not switch encryption on");
    }
    w.typ("md:enc-phrase", crate::config::CONSENT_PHRASE);
    w.click("md:enc-enable");
    assert!(consent(&w).is_some(), "the exact sentence switches it on");
    w.click("md:done");
    assert!(w.app.modal.is_none());
    assert!(w.app.cfg.mail_encryption_consent.is_some(), "the consent and its date are in the settings");
    // 4. opening the mail now decrypts it, in a job, and stores the copy
    open_by_subject(&mut w, "Zebra plans");
    assert_eq!(security_of(&w).encryption, crate::mail_crypto::Encryption::Decrypted(crate::config::CryptoStandard::OpenPgp), "{:?}", security_of(&w));
    assert!(w.shows("the zebrafish plan") && w.shows("Decrypted"));
    let files = store_files(&w);
    assert!(files.iter().any(|f| f.ends_with(".decrypted.eml")) && files.iter().any(|f| f.ends_with(".decrypted.json")), "{files:?}");
    // the original is still the ciphertext
    let msg = w.app.mail.open.as_ref().unwrap().msg.clone();
    let raw = w.app.mail.ready_store().unwrap().read_raw(&msg).unwrap();
    assert!(String::from_utf8_lossy(&raw).contains("BEGIN PGP MESSAGE") && !String::from_utf8_lossy(&raw).contains("zebrafish"));
    // 5. the decrypted text is searched
    w.app.mail.query = "zebrafish".into();
    w.app.mail.search_now();
    w.until("the search ends", |a| a.mail.results.is_some() && !a.mail.searching);
    assert_eq!(w.app.mail.results.as_ref().unwrap().len(), 1, "search finds text that was only in the encrypted part");
    w.app.mail.clear_search();
    // reopening shows the stored copy without asking GnuPG again (no GnuPG home at all)
    w.app.mail.gnupg = crate::gpg::Gnupg { gpg: "/nonexistent/gpg".into(), gpgsm: "/nonexistent/gpgsm".into(), home: None };
    open_by_subject(&mut w, "Okapi plans");
    assert!(matches!(security_of(&w).encryption, crate::mail_crypto::Encryption::Failed(_)), "a mail that was not decrypted before needs GnuPG");
    open_by_subject(&mut w, "Zebra plans");
    assert!(matches!(security_of(&w).encryption, crate::mail_crypto::Encryption::DecryptedEarlier(..)), "{:?}", security_of(&w));
    assert!(w.shows("the zebrafish plan"));
    w.app.mail.gnupg = bob.g.clone();
    // 6. withdrawing: no further decryption, the existing copy stays and is still shown
    w.app.open_mail_settings();
    w.settle();
    if !w.has("md:enc-withdraw") {
        // the section opens by itself while the consent is given (and remembers being open)
        w.click("md:enc-header");
        w.settle();
    }
    assert!(w.shows("deletes nothing"), "the settings say what withdrawing does not do");
    w.click("md:enc-withdraw");
    w.click("md:done");
    assert!(w.app.cfg.mail_encryption_consent.is_none());
    open_by_subject(&mut w, "Okapi plans");
    assert_eq!(security_of(&w).encryption, crate::mail_crypto::Encryption::NeedsConsent);
    assert!(!w.shows("okapi plan"));
    assert!(store_files(&w).iter().filter(|f| f.ends_with(".decrypted.eml")).count() == 1, "only the first mail has a decrypted copy");
    open_by_subject(&mut w, "Zebra plans");
    assert!(w.shows("the zebrafish plan"), "the copy that exists is still shown");
    w.app.mail.query = "zebrafish".into();
    w.app.mail.search_now();
    w.until("the search ends", |a| a.mail.results.is_some() && !a.mail.searching);
    assert_eq!(w.app.mail.results.as_ref().unwrap().len(), 1, "and still searched");
}

// spec: MAIL-190, MAIL-191
#[test]
fn the_account_settings_pick_a_key_and_a_certificate_sign_by_default_and_the_standard_and_new_mail_starts_that_way() {
    if !have_gnupg() {
        return;
    }
    let alice = TestHome::new();
    let a_pgp = alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
    let a_sm = alice.gen_smime("Alice S/MIME", "alice@example.org");
    let (mut w, _smtp) = compose_world("keys-ui", 1);
    w.app.mail.gnupg = alice.g.clone();
    w.app.open_mail_settings();
    w.settle();
    w.until("GnuPG was looked at", |a| a.mail.gnupg_info.is_some());
    // the Sending section is open (this account sends): fold it, so that the next one is in view
    w.click("md:smtp-header");
    w.settle();
    w.click("md:keys-header");
    w.settle();
    // pick the OpenPGP key and the S/MIME certificate from the lists of GnuPG's secret keys
    w.click("md:pgp-key");
    w.frame(vec![]);
    w.click(&format!("md:pgp-key:{a_pgp}"));
    w.settle();
    assert!(!w.has(&format!("md:pgp-key:{a_pgp}")), "the list closes after a choice");
    w.click("md:smime-key");
    w.settle();
    assert!(w.has(&format!("md:smime-key:{a_sm}")), "the list of certificates opens");
    w.click(&format!("md:smime-key:{a_sm}"));
    w.click("md:sign-default");
    w.click("md:std:Smime");
    let acct = |w: &W| match &w.app.modal {
        Some(Modal::MailAccounts(d)) => d.accounts[0].clone(),
        _ => panic!("no dialog"),
    };
    let a = acct(&w);
    assert_eq!((a.pgp_key.as_str(), a.smime_key.as_str(), a.sign_by_default, a.preferred_standard), (a_pgp.as_str(), a_sm.as_str(), true, crate::config::CryptoStandard::Smime));
    assert!(w.shows("Alice Test") && w.shows("CN=Alice S/MIME"), "the keys are shown by name");
    w.click("md:done");
    assert!(w.app.modal.is_none());
    let saved = &w.app.cfg.mail_accounts[0];
    assert_eq!((saved.pgp_key.as_str(), saved.smime_key.as_str(), saved.sign_by_default), (a_pgp.as_str(), a_sm.as_str(), true));
    // a new mail starts signed, with the preferred standard
    w.press_text(Key::C, Modifiers::NONE, "c");
    assert!(w.compose_open());
    let d = w.app.mail.compose.as_ref().unwrap().draft.clone();
    assert_eq!((d.sign, d.encrypt, d.standard), (true, false, crate::config::CryptoStandard::Smime), "Encrypt is never on by itself");
    w.frame(vec![]);
    assert!(w.has("mail:compose:std:OpenPgp") && w.has("mail:compose:std:Smime"), "with both keys, both standards are offered");
    // an account without any key gets no toggles, only the way to get them
    w.esc();
    w.app.cfg.mail_accounts[0].pgp_key.clear();
    w.app.cfg.mail_accounts[0].smime_key.clear();
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.frame(vec![]);
    assert!(!w.has("mail:compose:sign") && w.shows("choose a key in the account settings"));
    assert!(!w.app.mail.compose.as_ref().unwrap().draft.sign);
}

// spec: MAIL-192, MAIL-193, MAIL-195
#[test]
fn compose_toggles_show_the_key_of_every_recipient_refuse_a_missing_one_and_send_signed_and_encrypted_in_a_job() {
    if !have_gnupg() {
        return;
    }
    let (alice, bob) = (TestHome::new(), TestHome::new());
    let a = alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
    let b = bob.gen_pgp("Bob", "bob@example.org", "never", None);
    bob.share_pgp_with(&alice, &b);
    alice.share_pgp_with(&bob, &a);
    let (mut w, smtp) = compose_world("crypt-ui", 1);
    w.app.mail.gnupg = alice.g.clone();
    w.app.cfg.mail_accounts[0].pgp_key = a.clone();
    w.app.cfg.mail_accounts[0].from_address = "alice@example.org".into();
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.frame(vec![]);
    assert!(w.has("mail:compose:sign") && w.has("mail:compose:encrypt"));
    // Encrypt needs the consent: without it the box does nothing
    w.click("mail:compose:encrypt");
    assert!(!w.app.mail.compose.as_ref().unwrap().draft.encrypt, "Encrypt is off until encryption is switched on in the settings");
    w.app.cfg.mail_encryption_consent = Some("2026-11-14 12:00".into());
    w.typ("mail:compose:to", "bob@example.org, dave@example.org");
    w.typ("mail:compose:subject", "Secret plans");
    w.write_body("the secret plan");
    w.click("mail:compose:sign");
    w.click("mail:compose:encrypt");
    let d = w.app.mail.compose.as_ref().unwrap().draft.clone();
    assert!((d.sign, d.encrypt) == (true, true));
    // the keys are looked up in the local keyring, in a job, after a short pause
    w.until("the keys were looked up", |a| a.mail.compose.as_ref().is_some_and(|c| c.keys_for.is_some() && c.keys_for == Some(c.keys_signature())));
    w.frame(vec![]);
    assert!(w.shows("bob@example.org: key found") && w.shows("dave@example.org: no key"), "{:?}", w.texts.iter().filter(|t| t.0.contains("@example.org")).map(|t| t.0.clone()).collect::<Vec<_>>());
    assert!(w.shows("Also encrypted for you"), "the note about the sender's own copy and the visible subject");
    // the toggles are part of the draft and survive saving
    w.click("mail:compose:save");
    let (acct, id) = {
        let c = w.app.mail.compose.as_ref().unwrap();
        (c.draft.account.clone(), c.draft.id.clone())
    };
    let (saved, _) = crate::outbox::Outbox::new(&w.app.mail.root).load_draft(&acct, &id).unwrap();
    assert_eq!((saved.sign, saved.encrypt), (true, true));
    // Send with a recipient that has no key: refused in words, the window and the typed text stay, nothing is in the outbox
    w.ctrl_enter();
    w.until("the answer", |a| a.mail.compose.as_ref().is_some_and(|c| c.crypting.is_none() && c.error.is_some()));
    let e = w.app.mail.compose.as_ref().unwrap().error.clone().unwrap();
    assert!(e.contains("dave@example.org") && e.contains("not sent"), "{e}");
    assert!(w.items().is_empty() && w.compose_field("body").contains("the secret plan"));
    // the person takes dave out; Send now runs GnuPG in a job and puts the encrypted mail into the outbox
    w.typ("mail:compose:to", "bob@example.org");
    w.until("the keys were looked up again", |a| a.mail.compose.as_ref().is_some_and(|c| c.keys_for.is_some() && c.keys_for == Some(c.keys_signature())));
    w.ctrl_enter();
    let root = w.app.mail.root.clone();
    // the job writes the outbox file before the window hears back, so wait for both (a slow runner sees the gap)
    w.until("the mail is in the outbox and the window closed", move |a| a.mail.compose.is_none() && !crate::outbox::Outbox::new(&root).list(None).is_empty());
    let item = w.items().remove(0);
    let eml = crate::outbox::Outbox::new(&w.app.mail.root).read_eml(&item.account, &item.id).unwrap();
    assert!(!String::from_utf8_lossy(&eml).contains("the secret plan"), "the outbox holds the encrypted bytes");
    assert!(!w.compose_open(), "the window closed when the mail was queued");
    w.until("the mail was sent", |_| !smtp.state().received.is_empty());
    let got = smtp.state().received[0].data.clone();
    assert_eq!(got, eml);
    for (name, h) in [("bob", &bob), ("alice", &alice)] {
        let o = crate::mail_crypto::process(&h.g, &got, true, &std::sync::atomic::AtomicBool::new(false), NOW);
        assert_eq!(o.security.lines.len(), 1, "{name}: {:?}", o.security);
        assert!(crate::mail::parse(o.display.as_ref().unwrap()).text.starts_with("the secret plan"), "{name}");
    }
}

// spec: MAIL-171
#[cfg(unix)]
#[test]
fn a_gnupg_call_that_waits_for_a_dialog_never_blocks_the_window_and_cancel_stops_it() {
    use std::os::unix::fs::PermissionsExt;
    // a stand-in for gpg that never answers, like one waiting for a passphrase dialog nobody sees
    let dir = std::env::temp_dir().join(format!("shale-slowgpg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("slow-gpg");
    std::fs::write(&script, "#!/bin/sh\nexec sleep 60\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut w = W::new("slow-gpg", true);
    w.app.mail.gnupg = crate::gpg::Gnupg { gpg: script.to_string_lossy().into_owned(), gpgsm: script.to_string_lossy().into_owned(), home: None };
    w.app.cfg.mail_encryption_consent = Some("2026-11-14 12:00".into());
    let alice = TestHome::new();
    let signed = testmail::pgp_signed(&alice, &alice.gen_pgp("Alice Test", "alice@example.org", "never", None), testmail::ENTITY);
    add_inbox_mail(&w, "Plans", signed);
    w.sync_and_wait();
    w.open_mail("Plans");
    // GnuPG is "waiting": the window keeps drawing frames, says so, and offers Cancel
    for _ in 0..10 {
        w.frame(vec![]);
    }
    assert!(matches!(w.app.mail.open.as_ref().map(|o| &o.body), Some(Body::Ready(l)) if l.security.working.is_some()));
    assert!(w.shows("Checking the signature") && w.has("mail:sec:cancel"));
    let t = Instant::now();
    w.click("mail:sec:cancel");
    w.until("the call ends after Cancel", |a| matches!(a.mail.open.as_ref().map(|o| &o.body), Some(Body::Ready(l)) if l.security.working.is_none()));
    assert!(t.elapsed() < Duration::from_secs(10), "the child was killed, not waited for");
    w.frame(vec![]);
    assert!(w.shows("Cancelled."), "the strip says what happened");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: MAIL-207, MAIL-208
#[test]
fn queued_actions_of_an_account_that_is_not_configured_are_not_counted_as_waiting_but_noted_and_kept() {
    let mut w = inbox_with_cursor("orphan-count");
    w.offline.store(true, std::sync::atomic::Ordering::SeqCst);
    w.press(Key::U, Modifiers::NONE);
    w.press(Key::S, Modifiers::NONE);
    w.frame(vec![]);
    w.until("the attempt fails", |a| a.mail.offline.values().next().is_some());
    w.frame(vec![]);
    assert!(w.shows("2 changes are waiting for the server") && !w.shows("removed accounts"));
    // the account gets a new id (its server or user was changed): its old id is not configured any more
    w.app.cfg.mail_accounts[0].id = "someone-else".into();
    w.app.mail.refresh();
    w.frame(vec![]);
    assert!(!w.shows("waiting for the server"), "not counted: they will never be sent");
    assert!(w.shows("2 queued changes belong to removed accounts (not sent)"), "{:?}", w.texts.iter().map(|t| &t.0).collect::<Vec<_>>());
    assert_eq!(w.app.mail.pending_split(&w.app.cfg), (0, 2));
    assert_eq!(w.app.mail.pending_total, 2, "they stay queued");
}

/// The inbox of the pretend server with one very long mail on top, synced, the list has the keyboard.
fn inbox_with_a_long_mail(tag: &str) -> W {
    let mut w = W::new(tag, true);
    let body = (1..=150).map(|i| format!("line {i} of a long text")).collect::<Vec<_>>().join("\r\n");
    w.server.state().add_message("INBOX", dated("Long text", "Bob <bob@x.de>", &body, 14), NOW);
    w.sync_and_wait();
    w.until("the Inbox is open", |a| a.mail.total == 4);
    w.frame(vec![]);
    w
}

// spec: MAIL-062, MAIL-063
#[test]
fn ctrl_h_and_ctrl_l_move_the_keyboard_across_the_panes_and_the_marker_follows() {
    let mut w = inbox_with_a_long_mail("panes");
    let marked = |w: &W, pane: &str| w.has("mail:focus") && w.rect("mail:focus") == w.rect(&format!("mail:pane:{pane}"));
    // the list has the keyboard when the view opens, and it is marked
    assert_eq!(w.app.mail.pane, Pane::List);
    assert!(marked(&w, "list"));
    // Ctrl+L: list → message, and at the message pane it stays
    w.press(Key::L, Modifiers::COMMAND);
    assert_eq!(w.app.mail.pane, Pane::Message);
    assert!(marked(&w, "message"));
    w.press(Key::L, Modifiers::COMMAND);
    assert_eq!(w.app.mail.pane, Pane::Message, "nothing to the right of the message");
    assert!(marked(&w, "message"));
    // Ctrl+H: message → list → folders
    w.press(Key::H, Modifiers::COMMAND);
    assert_eq!(w.app.mail.pane, Pane::List);
    assert!(marked(&w, "list"));
    w.press(Key::H, Modifiers::COMMAND);
    assert_eq!(w.app.mail.pane, Pane::Folders);
    assert!(marked(&w, "folders"));
    // Ctrl+H at the folders goes on into the left sidebar (NAV-011); the mail view lets go
    w.press(Key::H, Modifiers::COMMAND);
    assert!(w.app.tree_sel.is_some(), "the file list has the keyboard");
    assert!(!w.has("mail:focus"), "no pane is marked while the keyboard is in the sidebar");
    let cursor = w.app.mail.cursor;
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, cursor, "j belongs to the file list now");
    // Ctrl+L from the sidebar comes back into the folder pane
    w.press(Key::L, Modifiers::COMMAND);
    assert!(w.app.tree_sel.is_none());
    assert_eq!(w.app.mail.pane, Pane::Folders);
    assert!(marked(&w, "folders"));
    // the search field belongs to the list pane; Ctrl+H leaves it and moves on
    w.press(Key::Slash, Modifiers::NONE);
    assert!(w.ctx.memory(|m| m.has_focus(egui::Id::unique("mail-search"))));
    w.frame(vec![]);
    assert_eq!(w.app.mail.pane, Pane::List, "the field is part of the list pane");
    w.press(Key::H, Modifiers::COMMAND);
    assert!(!w.ctx.memory(|m| m.focused().is_some()), "the field was left");
    assert_eq!(w.app.mail.pane, Pane::Folders);
    // a click gives a pane the keyboard; choosing a folder hands it to the list
    let r = w.rect("mail:pane:message");
    w.click_at(Pos2::new(r.center().x, r.max.y - 20.0), Modifiers::NONE);
    assert_eq!(w.app.mail.pane, Pane::Message);
    assert!(marked(&w, "message"));
    let inbox = w.folder_id("INBOX");
    w.click(&format!("mail:folder:{inbox}"));
    assert_eq!(w.app.mail.pane, Pane::List);
    assert!(marked(&w, "list"));
}

// spec: MAIL-064
#[test]
fn j_and_k_act_in_the_pane_that_has_the_keyboard_only() {
    let mut w = inbox_with_a_long_mail("jk");
    let inbox = w.folder_id("INBOX");
    // folder pane: j / k select folders, the list stays
    w.press(Key::H, Modifiers::COMMAND);
    assert_eq!(w.app.mail.pane, Pane::Folders);
    w.press(Key::J, Modifiers::NONE);
    assert_ne!(w.app.mail.folder, Some(inbox), "j: the next folder");
    assert_eq!(w.app.mail.cursor, None);
    w.press(Key::K, Modifiers::NONE);
    assert_eq!(w.app.mail.folder, Some(inbox), "k: back to the Inbox");
    // keys that work on messages do nothing here: a key meant for a folder never deletes a mail
    w.app.mail.cursor = Some(0);
    w.press(Key::D, Modifiers::NONE);
    w.press(Key::U, Modifiers::NONE);
    w.press(Key::Space, Modifiers::NONE);
    w.frame(vec![]);
    assert!(w.pending_is_empty() && w.app.mail.marked.is_empty() && w.app.mail.problems.is_empty());
    assert_eq!(w.server_uids("INBOX").len(), 4);
    w.app.mail.cursor = None;
    // Enter in the folder pane goes on into the list
    w.press(Key::Enter, Modifiers::NONE);
    assert_eq!(w.app.mail.pane, Pane::List);
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(0));
    assert_eq!(w.app.mail.folder, Some(inbox), "in the list j does not change the folder");
    // message pane: j / k scroll the text and leave the cursor alone
    w.press(Key::Enter, Modifiers::NONE);
    w.until("the long mail is read", |a| matches!(a.mail.open.as_ref().map(|o| &o.body), Some(Body::Ready(_))));
    assert_eq!(w.app.mail.open.as_ref().unwrap().msg.subject, "Long text");
    w.press(Key::L, Modifiers::COMMAND);
    assert_eq!(w.app.mail.pane, Pane::Message);
    w.frame(vec![]);
    assert_eq!(w.app.mail.read_offset, 0.0);
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::J, Modifiers::NONE);
    w.until("the message scrolled", |a| a.mail.read_offset > 0.0);
    let down = w.app.mail.read_offset;
    assert_eq!(w.app.mail.cursor, Some(0), "the list did not move");
    w.press(Key::K, Modifiers::NONE);
    w.until("the message scrolled back", move |a| a.mail.read_offset < down);
    assert_eq!(w.app.mail.cursor, Some(0));
    // back in the list the cursor moves again
    w.press(Key::H, Modifiers::COMMAND);
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(1));
}

// spec: MAIL-061
#[test]
fn a_rebound_mail_key_works_and_the_old_one_does_not() {
    use crate::keys::{Action as KeyAction, Binding};
    let mut w = inbox_with_a_long_mail("rebind");
    // the keymap refuses what clashes: a Global key, another key of the same scope, a fixed key
    let b = |s: &str| Binding::parse(s).unwrap();
    assert!(w.app.keymap.check(KeyAction::MailRead, &b("Ctrl+H")).unwrap_err().contains("already used"));
    assert!(w.app.keymap.check(KeyAction::MailRead, &b("D")).unwrap_err().contains("already used"));
    assert!(w.app.keymap.check(KeyAction::MailRead, &b("Enter")).unwrap_err().contains("fixed"));
    w.app.keymap.set(KeyAction::MailRead, b("G")).unwrap();
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(0));
    // the old key does nothing now
    w.press(Key::U, Modifiers::NONE);
    w.frame(vec![]);
    assert!(w.pending_is_empty() && w.has("mail:unread:0"), "u no longer marks read");
    // the new key does
    w.press(Key::G, Modifiers::NONE);
    w.settle_ops();
    w.frame(vec![]);
    assert!(!w.has("mail:unread:0"), "g marks read");
    assert_eq!(w.server_flags("INBOX", 4), vec!["\\Seen"]);
}

// spec: MAIL-065, MAIL-066
#[test]
fn sync_load_attachments_search_scope_and_the_password_field_work_from_the_keyboard() {
    let mut w = W::new("kbd", true);
    secrets::clear_mail_secret(&w.account.id, MailSecret::Password);
    w.frame(vec![]);
    // Ctrl+R syncs; with no password the question appears with the keyboard in its field, so the
    // letters of the password are typed and trigger no mail key (s, e, c, r, t)
    w.press(Key::R, Modifiers::COMMAND);
    w.until("the question appears", |a| a.mail.prompt.is_some());
    w.frame(vec![]);
    assert!(w.ctx.memory(|m| m.focused().is_some()), "the password field has the keyboard");
    w.type_text(PASSWORD);
    assert_eq!(w.app.mail.prompt.as_ref().map(|p| p.text.expose()), Some(PASSWORD));
    assert!(w.app.mail.compose.is_none(), "the letters typed no mail keys");
    w.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    let id = w.account.id.clone();
    w.until("the sync ends well", move |a| a.mail.syncs.get(&id).is_some_and(|r| r.finished.is_some() && r.errors.is_empty()) && a.mail.total == 3);
    w.frame(vec![]);
    // `t` switches the search between all folders and this one
    assert!(w.app.mail.search_all);
    w.press(Key::T, Modifiers::NONE);
    assert!(!w.app.mail.search_all);
    w.press(Key::T, Modifiers::NONE);
    assert!(w.app.mail.search_all);
    // `]` selects the attachment, Shift+O saves it into the vault
    w.open_mail("Report attached");
    w.frame(vec![]);
    w.press(Key::CloseBracket, Modifiers::NONE);
    assert_eq!(w.app.mail.att_sel, 0);
    assert!(w.shows("▸ report.pdf"), "the selected attachment is marked");
    let before = w.vault_files();
    w.press(Key::O, Modifiers::SHIFT);
    let saved: Vec<String> = w.vault_files().into_iter().filter(|f| !before.contains(f)).collect();
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert_eq!(std::fs::read(w.dir.join(&saved[0])).unwrap(), b"Hello PDF");
    // Shift+L loads a message that is not here
    w.open_folder("Trash");
    let trash = w.folder_id("Trash");
    w.until("the listing arrives", move |a| matches!(a.mail.listings.get(&trash), Some(ListingState::Done(_))) && a.mail.total == 2);
    w.open_row(0);
    assert!(matches!(w.app.mail.open.as_ref().map(|o| &o.body), Some(Body::NeedsDownload)));
    w.press(Key::L, Modifiers::SHIFT);
    w.until("the message arrives", |a| matches!(a.mail.open.as_ref().map(|o| &o.body), Some(Body::Ready(_))));
    // Esc closes the message, and at last dismisses the note of a failed action
    w.app.mail.problems.push("the server said no".into());
    w.press(Key::Escape, Modifiers::NONE);
    assert!(w.app.mail.open.is_none());
    assert_eq!(w.app.mail.problems.len(), 1);
    w.press(Key::Escape, Modifiers::NONE);
    assert!(w.app.mail.problems.is_empty());
    // `a` opens the account settings
    w.press(Key::A, Modifiers::NONE);
    w.frame(vec![]);
    assert!(matches!(w.app.modal, Some(Modal::MailAccounts(_))));
}

// ------------------------------------------------------------ the keyboard of drafts, outbox, the menu and the unsent window

use crate::outbox::{Outbox, Outcome};

impl W {
    /// Is the cursor colour painted over this control?
    fn cursor_painted_on(&self, key: &str) -> bool {
        let r = self.rect(key);
        let c = crate::theme::ACCENT.gamma_multiply(0.25);
        self.fills.iter().any(|(f, col)| *col == c && f.intersects(r))
    }

    /// Put mails into the outbox files: `kind` 0 waits (for an hour), 1 failed, 2 may have been sent.
    fn put_in_outbox(&mut self, subject: &str, kind: u8) {
        let ob = Outbox::new(&self.app.mail.root);
        let real = crate::cal_time::now();
        let d = crate::outbox::tests::draft(&self.account.id, subject, "the text");
        let b = crate::compose::build(&d, &crate::compose::BuildOptions { now: real, ..Default::default() }).unwrap();
        ob.queue(&d, &b, real, real + 3600, "2026-11-14").unwrap();
        if kind > 0 {
            ob.begin_send(&d.account, &d.id, real + 4000).unwrap();
            let out = if kind == 1 { Outcome::Failed("550 no such user".into()) } else { Outcome::Unsure("the connection ended".into()) };
            ob.finish(&d.account, &d.id, out, real).unwrap();
        }
        self.app.mail.refresh_local();
        self.frame(vec![]);
    }

    /// Move the cursor of the outbox list to the mail with this subject, with the keys.
    fn outbox_go(&mut self, subject: &str) {
        let acct = self.account.id.clone();
        let at = self.app.mail.local_items.iter().filter(|i| i.account == acct).position(|i| i.subject == subject).unwrap_or_else(|| panic!("no {subject:?} in the outbox"));
        for _ in 0..8 {
            self.press(Key::K, Modifiers::NONE);
        }
        for _ in 0..at {
            self.press(Key::J, Modifiers::NONE);
        }
        assert_eq!(self.app.mail.outbox_sel.as_ref().map(|k| k.1.clone()), self.app.mail.local_items.iter().find(|i| i.subject == subject).map(|i| i.id.clone()), "the mail under the cursor is the selected one");
    }
}

// spec: MAIL-067
#[test]
fn drafts_and_outbox_lists_work_from_the_keyboard() {
    let (mut w, smtp) = compose_world("kbd-local", 30);
    let acct = w.account.id.clone();
    let ob = Outbox::new(&w.app.mail.root);
    for (i, subject) in ["First draft", "Second draft"].iter().enumerate() {
        ob.save_draft(&crate::outbox::tests::draft(&acct, subject, "words"), NOW + i as i64).unwrap();
    }
    for (subject, kind) in [("Waiting", 0), ("Failed A", 1), ("Unsure", 2), ("Failed B", 1), ("Failed C", 1)] {
        w.put_in_outbox(subject, kind);
    }
    // Drafts: j / k move a visible cursor; a message key does not touch the Inbox
    w.click(&format!("mail:drafts:{acct}"));
    w.frame(vec![]);
    assert_eq!(w.app.mail.pane, Pane::List);
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(0));
    assert!(w.cursor_painted_on("mail:draft:0"), "the row under the cursor is painted");
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(1));
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(1), "the last row stops the cursor");
    w.press(Key::K, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(0));
    w.press(Key::U, Modifiers::NONE);
    w.press(Key::S, Modifiers::NONE);
    w.settle_ops();
    assert!(w.server_flags("INBOX", 3).is_empty(), "u and s did not act on a message of the Inbox");
    // e (and Enter) open the draft under the cursor
    let first = w.app.mail.local_drafts[0].subject.clone();
    w.press_text(Key::E, Modifiers::NONE, "e");
    w.until("the draft opens", |a| a.mail.compose.is_some());
    assert_eq!(w.compose_field("subject"), first);
    w.app.mail.compose_close();
    w.until("the editor is closed", |a| a.mail.compose.is_none());
    w.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    w.until("Enter opens it too", |a| a.mail.compose.is_some());
    w.app.mail.compose_close();
    w.until("the editor is closed again", |a| a.mail.compose.is_none());
    // d discards the draft; the cursor stays in the list
    w.press(Key::D, Modifiers::NONE);
    assert_eq!(w.drafts_on_disk().len(), 1);
    assert_eq!(w.app.mail.cursor, Some(0));
    w.press(Key::Delete, Modifiers::NONE);
    assert!(w.drafts_on_disk().is_empty());
    assert_eq!(w.app.mail.cursor, None, "no row is left to stand on");

    // Outbox: the cursor selects the mail, the detail shows it
    w.click(&format!("mail:outbox:{acct}"));
    w.frame(vec![]);
    w.outbox_go("Failed A");
    assert!(w.cursor_painted_on(&format!("mail:outbox:{}", w.app.mail.cursor.unwrap())));
    w.frame(vec![]);
    assert!(w.shows("was not sent") && w.shows("550"), "the detail shows what happened");
    // Shift+S retries a failed mail
    w.press_text(Key::S, Modifiers::SHIFT, "S");
    w.until("the failed mail went out", |_| smtp.state().received.len() == 1);
    // and sends one that may have been sent again
    w.outbox_go("Unsure");
    w.press_text(Key::S, Modifiers::SHIFT, "S");
    w.until("the unsure mail went out again", |_| smtp.state().received.len() == 2);
    // e takes a failed mail back into the editor
    w.outbox_go("Failed B");
    w.press_text(Key::E, Modifiers::NONE, "e");
    w.until("the editor opens", |a| a.mail.compose.is_some());
    assert_eq!(w.compose_field("subject"), "Failed B");
    w.app.mail.compose_close();
    w.until("the editor is closed", |a| a.mail.compose.is_none());
    // e does nothing for a mail that waits; z takes it back into the editor
    w.outbox_go("Waiting");
    w.press_text(Key::E, Modifiers::NONE, "e");
    assert!(w.app.mail.compose.is_none(), "a waiting mail is taken back with z");
    w.press_text(Key::Z, Modifiers::NONE, "z");
    w.until("the waiting mail is back in the editor", |a| a.mail.compose.is_some());
    assert_eq!(w.compose_field("subject"), "Waiting");
    w.app.mail.compose_close();
    w.until("the editor is closed once more", |a| a.mail.compose.is_none());
    // d discards the record of a failed mail
    w.click(&format!("mail:outbox:{acct}"));
    w.frame(vec![]);
    w.outbox_go("Failed C");
    let before = w.items().len();
    w.press(Key::D, Modifiers::NONE);
    assert_eq!(w.items().len(), before - 1);
    assert!(w.items().iter().all(|i| i.subject != "Failed C"));
}

// spec: MAIL-068
#[test]
fn the_menu_of_a_row_opens_by_key_and_is_driven_by_the_arrows_enter_and_esc() {
    let (mut w, _smtp) = compose_world("menukey", 30);
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.app.mail.cursor, Some(0));
    w.press(Key::Space, Modifiers::NONE);
    assert_eq!(w.app.mail.marked.len(), 1);
    // Shift+F10 opens the menu at the row, the first entry is marked
    w.press(Key::F10, Modifiers::SHIFT);
    assert!(w.has("mail:ctx-read") && w.has("mail:ctx-flag") && w.has("mail:ctx-move") && w.has("mail:ctx-delete"));
    assert_eq!(w.rect("mail:menu-sel"), w.rect("mail:ctx-read"));
    // no other key acts meanwhile: d would delete
    w.press(Key::D, Modifiers::NONE);
    w.settle_ops();
    assert_eq!(w.server_uids("INBOX").len(), 3, "d did not delete");
    assert!(w.has("mail:ctx-read"), "the menu is still open");
    // ↓ marks the next entry
    w.press(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(w.rect("mail:menu-sel"), w.rect("mail:ctx-flag"));
    w.press(Key::J, Modifiers::NONE);
    assert_eq!(w.rect("mail:menu-sel"), w.rect("mail:ctx-move"));
    w.press(Key::K, Modifiers::NONE);
    w.press(Key::ArrowUp, Modifiers::NONE);
    w.press(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(w.rect("mail:menu-sel"), w.rect("mail:ctx-read"), "the mark stops at the first entry");
    // Esc closes only the menu: the mark of the message stays
    w.press(Key::Escape, Modifiers::NONE);
    assert!(!w.has("mail:ctx-read"));
    assert_eq!(w.app.mail.marked.len(), 1, "Esc closed the menu and nothing else");
    // the Menu key opens it again; ↓ Enter flags the marked mail
    w.press(Key::Menu, Modifiers::NONE);
    assert!(w.has("mail:ctx-flag"));
    w.press(Key::ArrowDown, Modifiers::NONE);
    w.press(Key::Enter, Modifiers::NONE);
    assert!(!w.has("mail:ctx-flag"), "Enter ran the entry and closed the menu");
    w.settle_ops();
    assert_eq!(w.server_flags("INBOX", 3), vec!["\\Flagged"]);
    // a click outside closes it
    w.press(Key::Menu, Modifiers::NONE);
    assert!(w.has("mail:ctx-read"));
    w.click_at(Pos2::new(1400.0, 800.0), Modifiers::NONE);
    assert!(!w.has("mail:ctx-read"));
    // a draft has a menu too
    let acct = w.account.id.clone();
    Outbox::new(&w.app.mail.root).save_draft(&crate::outbox::tests::draft(&acct, "Menu draft", "words"), NOW).unwrap();
    w.app.mail.refresh_local();
    w.click(&format!("mail:drafts:{acct}"));
    w.frame(vec![]);
    w.press(Key::Menu, Modifiers::NONE);
    assert!(!w.has("mail:draft-discard:0"), "no cursor, no menu");
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::Menu, Modifiers::NONE);
    assert!(w.has("mail:draft-discard:0"));
    w.press(Key::Enter, Modifiers::NONE);
    assert!(w.drafts_on_disk().is_empty(), "the menu entry discarded the draft");
    assert!(!w.has("mail:draft-discard:0"));
    // the Outbox has no row menu
    w.click(&format!("mail:outbox:{acct}"));
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::Menu, Modifiers::NONE);
    assert!(w.app.mail.kbd.menu.is_none());
}

// spec: MAIL-069
#[test]
fn the_unsent_window_is_driven_from_the_keyboard() {
    let mut w = world_with_unsent("unsent-kbd", 3);
    assert!(!w.app.mail.unsent_open);
    w.press_text(Key::U, Modifiers::SHIFT, "U");
    w.settle();
    assert!(w.app.mail.unsent_open, "Shift+U opens the window");
    let first = w.rect("mail:unsent-sel");
    // the window has the keyboard: c does not start a mail, j moves the mark
    w.press_text(Key::C, Modifiers::NONE, "c");
    assert!(w.app.mail.compose.is_none());
    w.press(Key::J, Modifiers::NONE);
    w.frame(vec![]);
    assert!(w.rect("mail:unsent-sel").min.y > first.min.y, "the mark moved down");
    let marked = w.app.mail.unsent[1].subject.clone();
    // Enter uploads the marked mail again, once
    w.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    w.until("the copy was queued and is not listed any more", |a| a.mail.unsent.len() == 2);
    assert!(w.app.mail.unsent.iter().all(|c| c.subject != marked));
    w.settle_ops();
    let server = w.server.state.clone();
    w.until("it reached the server", |_| server.lock().unwrap().messages("Sent").len() == 2);
    // d dismisses the marked one
    let next = w.app.mail.unsent[1.min(w.app.mail.unsent.len() - 1)].subject.clone();
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::D, Modifiers::NONE);
    assert_eq!(w.app.mail.unsent.len(), 1);
    assert!(w.app.mail.unsent.iter().all(|c| c.subject != next));
    // Esc closes the window, Shift+U opens and closes it
    w.press(Key::Escape, Modifiers::NONE);
    assert!(!w.app.mail.unsent_open);
    w.press_text(Key::U, Modifiers::SHIFT, "U");
    assert!(w.app.mail.unsent_open);
    w.press_text(Key::U, Modifiers::SHIFT, "U");
    assert!(!w.app.mail.unsent_open);
    // the last one dismissed: the window goes, and Shift+U has nothing to show
    w.press_text(Key::U, Modifiers::SHIFT, "U");
    assert!(w.app.mail.unsent_open);
    w.press(Key::Delete, Modifiers::NONE);
    w.frame(vec![]);
    assert!(w.app.mail.unsent.is_empty() && !w.app.mail.unsent_open);
    w.press_text(Key::U, Modifiers::SHIFT, "U");
    assert!(!w.app.mail.unsent_open);
}

fn two_attachments(day: u32) -> Vec<u8> {
    format!("From: Carol <carol@x.de>\r\nTo: ada@example.com\r\nSubject: Two files\r\nDate: {day:02} Nov 2026 09:30:00 +0100\r\nMessage-ID: <two@fake>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b2\"\r\n\r\n--b2\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nBoth files are attached.\r\n--b2\r\nContent-Type: text/plain; name=\"a.txt\"\r\nContent-Disposition: attachment; filename=\"a.txt\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8gQQ==\r\n--b2\r\nContent-Type: text/plain; name=\"b.txt\"\r\nContent-Disposition: attachment; filename=\"b.txt\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8gQg==\r\n--b2--\r\n").into_bytes()
}

// spec: MAIL-065
#[test]
fn o_opens_the_selected_attachment_through_the_opener_and_the_bracket_key_cycles_through_several() {
    let mut w = W::new("att-two", true);
    w.server.state().add_message("INBOX", two_attachments(15), NOW - 86_400);
    w.sync_and_wait();
    w.until("the Inbox is open", |a| a.mail.total == 4);
    // a pretend program: nothing starts, the opener tells what it was given
    let opened: Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>> = Arc::default();
    let seen = opened.clone();
    let _program = crate::opener::install_test_browser(Box::new(move |target| {
        let bytes = std::fs::read(target).unwrap_or_default();
        seen.lock().unwrap().push((target.to_string(), bytes));
    }));
    w.open_mail("Two files");
    w.frame(vec![]);
    assert_eq!(w.app.mail.att_sel, 0);
    w.press(Key::CloseBracket, Modifiers::NONE);
    assert_eq!(w.app.mail.att_sel, 1);
    assert!(w.shows("▸ b.txt") && !w.shows("▸ a.txt"), "the second attachment is marked");
    w.press_text(Key::O, Modifiers::NONE, "o");
    w.press(Key::CloseBracket, Modifiers::NONE);
    assert_eq!(w.app.mail.att_sel, 0, "after the last one it goes round to the first");
    assert!(w.shows("▸ a.txt"));
    w.press_text(Key::O, Modifiers::NONE, "o");
    w.press(Key::CloseBracket, Modifiers::NONE);
    assert_eq!(w.app.mail.att_sel, 1);
    let got = opened.lock().unwrap().clone();
    assert_eq!(got.len(), 2, "o opened exactly twice: {got:?}");
    assert!(got[0].0.ends_with("b.txt") && got[0].1 == b"Hello B", "{got:?}");
    assert!(got[1].0.ends_with("a.txt") && got[1].1 == b"Hello A", "{got:?}");
    // in the folder pane o opens nothing (MAIL-064)
    w.app.mail.pane = Pane::Folders;
    w.press_text(Key::O, Modifiers::NONE, "o");
    assert_eq!(opened.lock().unwrap().len(), 2);
}

// spec: MAIL-065, MAIL-134
#[test]
fn z_takes_back_the_waiting_mail() {
    let (mut w, smtp) = compose_world("zkey", 30);
    w.press_text(Key::C, Modifiers::NONE, "c");
    w.typ("mail:compose:to", "bob@x.de");
    w.typ("mail:compose:subject", "Back with z");
    w.write_body("precious words");
    w.click("mail:compose:send");
    assert!(!w.compose_open());
    w.until("the strip with Undo shows", |a| a.mail.rects.iter().any(|r| r.0 == "mail:undo-send"));
    assert_eq!(w.items()[0].state, OutState::Queued);
    w.press_text(Key::Z, Modifiers::NONE, "z");
    w.until("the editor is back", |a| a.mail.compose.is_some());
    assert_eq!(w.compose_field("subject"), "Back with z");
    assert!(w.items().is_empty());
    for _ in 0..20 {
        w.frame(vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(smtp.state().received.is_empty() && smtp.state().commands.is_empty(), "nothing was sent");
}

impl W {
    /// Is a text drawn that is exactly this (a key hint is a text of its own)?
    fn shows_exactly(&self, text: &str) -> bool {
        self.texts.iter().any(|(t, _)| t == text)
    }

    /// Rest the pointer on a control long enough for its tooltip to show.
    fn hover(&mut self, key: &str) {
        let p = self.rect(key).center();
        self.frame(vec![Event::PointerMoved(p)]);
        for _ in 0..16 {
            self.frame(vec![]);
        }
    }

    /// Move the pointer away, so the tooltip goes.
    fn unhover(&mut self) {
        self.frame(vec![Event::PointerMoved(Pos2::new(1400.0, 880.0))]);
        for _ in 0..3 {
            self.frame(vec![]);
        }
    }
}

// spec: MAIL-072
#[test]
fn key_hints_on_menu_entries_and_tooltips_follow_the_registry() {
    use crate::keys::{Action as KeyAction, Binding};
    let (mut w, _smtp) = compose_world("hints", 30);
    let acct = w.account.id.clone();
    // the row menu shows the keys that are set now (the keymap belongs to this app only)
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::Menu, Modifiers::NONE);
    assert!(w.has("mail:ctx-read"));
    for hint in ["u", "s / f", "m", "d / Delete"] {
        assert!(w.shows_exactly(hint), "the menu shows {hint:?}");
    }
    w.press(Key::Escape, Modifiers::NONE);
    // a rebound key shows the new key, an unbound action none
    w.app.keymap.set(KeyAction::MailRead, Binding::parse("G").unwrap()).unwrap();
    w.app.keymap.clear(KeyAction::MailFlag);
    w.press(Key::Menu, Modifiers::NONE);
    assert!(w.has("mail:ctx-flag"), "the entry stays");
    assert!(w.shows_exactly("g") && !w.shows_exactly("u"), "the rebound key replaces the old one");
    assert!(!w.shows_exactly("s / f") && !w.shows_exactly("s") && !w.shows_exactly("f"), "an unbound action shows no hint");
    assert!(w.shows_exactly("m") && w.shows_exactly("d / Delete"));
    w.press(Key::Escape, Modifiers::NONE);
    // the menu of a draft
    Outbox::new(&w.app.mail.root).save_draft(&crate::outbox::tests::draft(&acct, "Hint draft", "words"), NOW).unwrap();
    w.app.mail.refresh_local();
    w.click(&format!("mail:drafts:{acct}"));
    w.press(Key::J, Modifiers::NONE);
    w.press(Key::Menu, Modifiers::NONE);
    assert!(w.has("mail:draft-discard:0") && w.shows_exactly("d / Delete"));
    w.press(Key::Escape, Modifiers::NONE);

    // a button of the outbox shows its key in its tooltip
    w.app.keymap.reset(KeyAction::MailFlag);
    w.put_in_outbox("Failed hint", 1);
    w.click(&format!("mail:outbox:{acct}"));
    w.frame(vec![]);
    w.outbox_go("Failed hint");
    w.frame(vec![]);
    // the buttons of the detail also paint the key on the button itself (no hover needed)
    assert!(w.shows_exactly("Shift+S") && w.shows_exactly("e") && w.shows_exactly("d / Delete"), "painted keys: {:?}", w.texts);
    assert!(w.shows_exactly("Retry") && w.shows_exactly("Edit") && w.shows_exactly("Discard"), "the labels stay");
    w.hover("mail:outbox:retry");
    assert!(w.shows("Send the mail again (Shift+S)"), "the default key");
    w.hover("mail:outbox:edit");
    assert!(w.shows("(e)"), "Edit shows e");
    w.unhover();
    w.app.keymap.set(KeyAction::MailRetry, Binding::parse("X").unwrap()).unwrap();
    w.frame(vec![]);
    assert!(w.shows_exactly("x") && !w.shows_exactly("Shift+S"), "the rebound key is painted");
    w.hover("mail:outbox:retry");
    assert!(w.shows("Send the mail again (x)") && !w.shows("Shift+S"), "the rebound key");
    w.unhover();
    w.app.keymap.clear(KeyAction::MailRetry);
    w.frame(vec![]);
    assert!(!w.shows_exactly("x") && !w.shows_exactly("Shift+S") && w.shows_exactly("Retry"), "unbound: no key on the button");
    w.hover("mail:outbox:retry");
    assert!(w.shows_exactly("Send the mail again"), "no key, no hint: {:?}", w.texts);
}

// spec: MAIL-073
#[test]
fn tab_focus_is_drawn_on_the_rows_of_drafts_and_outbox() {
    let (mut w, _smtp) = compose_world("tabfocus", 30);
    let acct = w.account.id.clone();
    Outbox::new(&w.app.mail.root).save_draft(&crate::outbox::tests::draft(&acct, "Focus draft", "words"), NOW).unwrap();
    w.put_in_outbox("Focus failed", 1);
    for (folder, mark) in [(format!("mail:drafts:{acct}"), "mail:draft-focus:0"), (format!("mail:outbox:{acct}"), "mail:outbox-focus:0")] {
        w.click(&folder);
        w.frame(vec![]);
        assert!(!w.has(mark), "{mark} is not drawn before Tab moves there");
        let mut found = false;
        for _ in 0..80 {
            w.frame(vec![Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
            w.frame(vec![]);
            if w.has(mark) {
                found = true;
                break;
            }
        }
        assert!(found, "Tab reaches the row and {mark} is drawn");
        assert!(w.rect(mark).width() > 100.0, "it frames the whole row");
    }
}
