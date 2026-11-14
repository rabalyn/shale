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
use crate::mail_ui::{Body, ListingState, TestState};
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
}

impl Connector for TestConnector {
    fn connect(&self) -> Result<Client, ImapError> {
        let mut a = PlainAuth::new(&self.user, &self.password);
        Client::connect_test(&self.settings, server_cert(), &mut a)
    }
}

/// A connector factory like the real one (the password is the session's or the keyring's), but
/// for the pretend server's certificate.
fn factory() -> ConnectorFactory {
    Arc::new(|acct: &MailAccount, session: Option<String>| {
        let password = session.or_else(|| secrets::mail_secret(&acct.id, MailSecret::Password)).ok_or_else(|| NO_PASSWORD.to_string())?;
        let settings = ConnectSettings { host: acct.host.clone(), port: acct.port, security: acct.security, limits: Limits { io_timeout: Duration::from_secs(15), max_message: 20_000, ..Limits::default() } };
        Ok(Arc::new(TestConnector { settings, user: acct.user.clone(), password }) as Arc<dyn Connector>)
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
    /// What the last frame painted: text and its colour.
    texts: Vec<(String, Color32)>,
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
        app.mail.factory = factory();
        app.mail.opts = SyncOptions { now: Some(NOW), batch_size: 2, ..SyncOptions::default() };
        let account = MailAccount { id: format!("t-{tag}"), name: "Test mail".into(), host: "localhost".into(), port: server.port, user: "ada".into(), ..MailAccount::default() };
        if with_account {
            app.cfg.mail_accounts = vec![account.clone()];
            assert!(secrets::set_mail_secret(&account.id, MailSecret::Password, PASSWORD));
        }
        app.show_view(View::Mail, false);
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut w = W { app, ctx, t: 0.0, server, dir, account, texts: vec![] };
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
        fn walk(s: &Shape, out: &mut Vec<(String, Color32)>) {
            match s {
                Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                Shape::Text(t) => out.push((t.galley.job.text.clone(), t.galley.job.sections.first().map(|s| s.format.color).unwrap_or(Color32::WHITE))),
                _ => {}
            }
        }
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        self.texts = texts;
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
        assert_eq!(d.accounts[0].auth, crate::config::MailAuth::Password, "OAuth2 is offered disabled, never chosen");
    }
    w.frame(vec![]);
    assert!(w.shows("coming in a later step"));

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
    assert_eq!(secrets::mail_secret(&id, MailSecret::Password).as_deref(), Some(PASSWORD));
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
    w.app.mail.factory = Arc::new(|acct: &MailAccount, session: Option<String>| {
        let password = session.or_else(|| secrets::mail_secret(&acct.id, MailSecret::Password)).ok_or_else(|| NO_PASSWORD.to_string())?;
        let settings = ConnectSettings { host: acct.host.clone(), port: acct.port, security: acct.security, limits: Limits { io_timeout: Duration::from_secs(15), max_message: 10_000_000, ..Limits::default() } };
        Ok(Arc::new(TestConnector { settings, user: acct.user.clone(), password }) as Arc<dyn Connector>)
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
    assert_eq!(w.app.mail.session_pw.get(&w.account.id).map(String::as_str), Some(PASSWORD));
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
    let mut w = W { app, ctx: egui::Context::default(), t: 0.0, server, dir: dir.clone(), account: MailAccount { id: "perf".into(), ..MailAccount::default() }, texts: vec![] };
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

#[test]
fn zz_debug() {
    let ctx = egui::Context::default();
    crate::theme::apply(&ctx);
    let mut t = 0.0;
    let mail = crate::mail_ui::MailView::new(std::env::temp_dir().join("zz-unused"));
    let mut d = crate::mail_dialog::MailDialog::new(vec![], 10, true);
    let mut frame = |events: Vec<Event>, d: &mut crate::mail_dialog::MailDialog| {
        t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500.0, 900.0))), time: Some(t), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |_| {});
            egui::Modal::new(egui::Id::unique("m")).show(ui.ctx(), |ui| {
                crate::mail_dialog::show(ui, d, &mail);
            });
        });
        out.textures_delta.clear();
    };
    for _ in 0..4 {
        frame(vec![], &mut d);
    }
    let at = d.rects.iter().find(|r| r.0 == "md:add").unwrap().1;
    let p = at.center();
    let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    for ev in [vec![Event::PointerMoved(p)], vec![b(true)], vec![b(false)], vec![]] {
        eprintln!("--- frame");
        frame(ev, &mut d);
        eprintln!("accounts {}", d.accounts.len());
    }
}
