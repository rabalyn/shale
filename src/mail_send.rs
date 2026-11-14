//! Sending what is in the outbox (ADR-0010, MAIL-132 to MAIL-142): the job that takes a queued mail
//! whose undo time has passed, sends it over SMTP, and records the Sent copy.
//!
//! The order is what keeps a mail from being sent twice (the outbox module has the state machine):
//!
//! 1. `begin_send` writes the state `Sending` to disk, from `Queued` only, before the first byte goes out.
//! 2. The SMTP conversation. Its failures say in which phase they happened ([`SmtpError`]): everything
//!    before the closing `.` of the data is a definite "not sent" (`Failed`), a missing answer after it is
//!    `Unsure`. Neither is ever retried by this module; only the person does that.
//! 3. `Sent` is written, then the exact bytes are stored as a message of the Sent folder together with the
//!    `append` action for the server (one transaction, [`mail_ops::record_sent_copy`]), then the entry is
//!    marked as copied. A crash between 3's steps leaves a `Sent` mail without the mark: the copy is made
//!    at the next run, once (the Message-ID tells). The append is a queued action like the ones of phase 2:
//!    its failure can never cause a resend, it is not even connected to the sending.

use crate::compose::{BuildOptions, Draft};
use crate::config::MailAccount;
use crate::imap_client::{Authenticator, ConnectSettings, Limits};
use crate::imap_sync::AuthFactory;
use crate::mail_ops;
use crate::mail_store::MailStore;
use crate::outbox::{Item, Outbox, Outcome, State};
use crate::smtp_client::{Smtp, SmtpError};
use std::sync::Arc;
use std::time::Duration;

/// Makes SMTP connections for an account (tests plug in the pretend server).
pub trait SmtpConnector: Send + Sync {
    fn connect(&self) -> Result<Smtp, SmtpError>;
}

/// SMTP with the operating system's certificate check and the sign-in of the account.
pub struct RealSmtp {
    pub settings: ConnectSettings,
    pub auth: Arc<dyn AuthFactory>,
}

/// Connect with an authenticator from `auth`; if the server refuses the sign-in and the factory can get a
/// new token, once more (like IMAP, MAIL-083).
pub fn connect_with_retry(auth: &dyn AuthFactory, mut connect: impl FnMut(&mut dyn Authenticator) -> Result<Smtp, SmtpError>) -> Result<Smtp, SmtpError> {
    let mut a = auth.authenticator().map_err(SmtpError::Auth)?;
    match connect(a.as_mut()) {
        Err(SmtpError::Auth(_)) if auth.token_rejected() => {
            let mut a = auth.authenticator().map_err(SmtpError::Auth)?;
            connect(a.as_mut()).map_err(|e| match e {
                SmtpError::Auth(t) => SmtpError::Auth(format!("{t} The server refused a fresh token as well. {}", crate::oauth::SIGN_IN_AGAIN)),
                other => other,
            })
        }
        r => r,
    }
}

impl SmtpConnector for RealSmtp {
    fn connect(&self) -> Result<Smtp, SmtpError> {
        connect_with_retry(self.auth.as_ref(), |a| Smtp::connect(&self.settings, a))
    }
}

/// The server settings of an account's SMTP (MAIL-120).
pub fn smtp_settings(a: &MailAccount) -> ConnectSettings {
    ConnectSettings { host: a.smtp_host.trim().to_string(), port: a.smtp_port, security: a.smtp_security, limits: Limits { io_timeout: Duration::from_secs(120), ..Limits::default() } }
}

/// The connector of a configured account; the password comes from the keyring (or `session`, typed for this run).
pub fn connector_for_account(a: &MailAccount, session: Option<String>) -> Result<Arc<dyn SmtpConnector>, String> {
    if a.smtp_host.trim().is_empty() {
        return Err("This account has no SMTP server yet (account settings: Sending).".into());
    }
    let settings = smtp_settings(a);
    let auth: Arc<dyn AuthFactory> = match (&a.auth, session.clone()) {
        (crate::config::MailAuth::Password, Some(pw)) => Arc::new(crate::imap_sync::PasswordAuth { user: a.user.clone(), password: pw }),
        _ => match crate::imap_sync::auth_factory(a, session, &settings.host, settings.port) {
            Ok(f) => f,
            Err(e) if e.contains("no password") => return Err(crate::imap_sync::NO_PASSWORD.into()),
            Err(e) => return Err(e),
        },
    };
    Ok(Arc::new(RealSmtp { settings, auth }))
}

/// "Test connection" for SMTP: connect, secure, `EHLO`, sign in; never sends a mail (MAIL-121).
pub fn test_connection(c: &dyn SmtpConnector) -> Result<String, String> {
    let s = c.connect().map_err(|e| e.to_string())?;
    s.quit();
    Ok("signed in".into())
}

/// The sender of an account: its From name and address (the user name, if that is an address, when none is set).
pub fn sender_of(a: &MailAccount) -> (String, String) {
    let addr = if a.from_address.trim().is_empty() && a.user.contains('@') { a.user.trim().to_string() } else { a.from_address.trim().to_string() };
    (a.from_name.trim().to_string(), addr)
}

/// A new, empty draft for an account, with its sender and signature.
pub fn new_draft(a: &MailAccount) -> Draft {
    let (name, addr) = sender_of(a);
    crate::compose::new_draft(&a.id, &name, &addr, &a.signature)
}

/// Send pressed (MAIL-132): the mail goes into the outbox, to be sent when `undo_secs` have passed. Nothing goes
/// to the network here.
pub fn queue_draft(ob: &Outbox, a: &MailAccount, draft: &Draft, undo_secs: u32, now: i64, date: &str) -> Result<Item, String> {
    let built = crate::compose::finalize_plain(draft, &BuildOptions { now, user_agent: a.user_agent_header, ..BuildOptions::default() }).map_err(|e| e.0)?;
    ob.queue(draft, &built, now, now + i64::from(undo_secs), date)
}

/// What a run of [`send_due`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SendReport {
    /// Ids of the mails that went out.
    pub sent: Vec<String>,
    /// Ids and the server's words for the mails that were definitely not sent.
    pub failed: Vec<(String, String)>,
    /// Ids of the mails that may or may not have been sent.
    pub unsure: Vec<String>,
    /// Other things worth telling (a missing Sent folder, a copy that could not be recorded).
    pub notes: Vec<String>,
}

fn connect_failed(e: &SmtpError) -> String {
    e.to_string()
}

/// Send the queued mails of an account whose time has come, one after the other, each on its own
/// connection. Blocks: run it as a job. A mail is sent at most once per call and never again by this function
/// once it is anything but `Queued`.
pub fn send_due(ob: &Outbox, store: &MailStore, a: &MailAccount, connector: &dyn SmtpConnector, now: i64) -> SendReport {
    let mut report = SendReport::default();
    for item in ob.list(Some(&a.id)) {
        if item.state != State::Queued || item.send_after > now {
            continue;
        }
        let (item, bytes) = match ob.begin_send(&a.id, &item.id, now) {
            Ok(x) => x,
            Err(_) => continue, // taken back, or another run has it
        };
        // from here on the entry says `Sending`; whatever happens, it ends in a state (or the program dies and the next start says Unsure)
        let result = crate::jobs::catch(|| -> Result<String, SmtpError> {
            let mut smtp = connector.connect()?;
            let r = smtp.send(&item.from, &item.recipients, &bytes);
            if r.is_ok() {
                smtp.quit();
            }
            r
        })
        .unwrap_or_else(|p| Err(SmtpError::Unsure(format!("an internal error while sending: {p}"))));
        match result {
            Ok(_) => match ob.finish(&a.id, &item.id, Outcome::Sent, now) {
                Ok(done) => {
                    report.sent.push(done.id.clone());
                    copy_to_sent(ob, store, a, &done, &bytes, now, &mut report);
                }
                Err(e) => report.notes.push(format!("the mail went out, but its state could not be written: {e}")),
            },
            Err(SmtpError::Unsure(t)) => {
                let _ = ob.finish(&a.id, &item.id, Outcome::Unsure(t), now);
                report.unsure.push(item.id.clone());
            }
            Err(e) => {
                let _ = ob.finish(&a.id, &item.id, Outcome::Failed(connect_failed(&e)), now);
                report.failed.push((item.id.clone(), e.to_string()));
            }
        }
    }
    resume_copies(ob, store, a, now, &mut report);
    report
}

/// After a send: the local Sent copy and the append (MAIL-141). Accounts whose server keeps sent mail itself skip it.
fn copy_to_sent(ob: &Outbox, store: &MailStore, a: &MailAccount, item: &Item, bytes: &[u8], now: i64, report: &mut SendReport) {
    if !a.server_stores_sent {
        match mail_ops::record_sent_copy(store, &a.id, bytes, now) {
            Ok(Some(_)) => {}
            Ok(None) => report.notes.push("The mail was sent. This account has no Sent folder that Shale knows of, so no copy was saved (sync the account or use “Test connection” so that its folders are known).".into()),
            Err(e) => {
                // not marked: the next run tries again (the Message-ID prevents a second copy)
                report.notes.push(format!("The mail was sent, but its copy for the Sent folder could not be recorded yet: {e}"));
                return;
            }
        }
    }
    if let Err(e) = ob.mark_copy_done(&a.id, &item.id) {
        report.notes.push(format!("the Sent copy was recorded, but the outbox could not say so: {e}"));
    }
}

/// A mail that was sent but whose copy was not recorded (a crash in between): record it now.
pub fn resume_copies(ob: &Outbox, store: &MailStore, a: &MailAccount, now: i64, report: &mut SendReport) {
    for item in ob.list(Some(&a.id)) {
        if item.state == State::Sent && !item.copy_done {
            if let Ok(bytes) = ob.read_eml(&a.id, &item.id) {
                copy_to_sent(ob, store, a, &item, &bytes, now, report);
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::MailSecurity;
    use crate::imap_fake_tests::server_cert;
    use crate::mail_ops::tests::{env, Env, NOW};
    use crate::outbox::tests::{draft, temp, DATE};
    use crate::smtp_fake_tests::FakeSmtp;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct TestSmtp {
        pub settings: ConnectSettings,
        pub user: String,
        pub password: String,
        pub connects: AtomicUsize,
    }

    impl SmtpConnector for TestSmtp {
        fn connect(&self) -> Result<Smtp, SmtpError> {
            self.connects.fetch_add(1, Ordering::SeqCst);
            Smtp::connect_test(&self.settings, server_cert(), &mut crate::imap_client::PlainAuth::new(&self.user, &self.password))
        }
    }

    pub fn connector(s: &FakeSmtp) -> TestSmtp {
        let g = s.state();
        TestSmtp { settings: s.settings(), user: g.user.clone(), password: g.password.clone(), connects: AtomicUsize::new(0) }
    }

    pub fn account() -> MailAccount {
        MailAccount { id: "acct1".into(), user: "ada".into(), from_name: "Ada Lovelace".into(), from_address: "ada@example.com".into(), smtp_host: "localhost".into(), ..MailAccount::default() }
    }

    /// An IMAP environment (INBOX, Trash, Archive) with a Sent folder that the index knows, and an outbox in the same store.
    pub fn with_sent(tag: &str) -> (Env, Outbox) {
        let e = env(&format!("send-{tag}"));
        e.server.state().add_folder("Sent", &["\\Sent"]);
        crate::imap_sync::discover_folders(&e.store, &e.conn, "acct1", "Test").unwrap();
        // the first sync of the new folder gives it its UIDVALIDITY
        let (tx, _rx) = std::sync::mpsc::channel();
        let sum = crate::imap_sync::sync_account(&e.store, "acct1", "Test", &e.conn, &crate::imap_sync::SyncOptions { now: Some(NOW), ..Default::default() }, &tx, &std::sync::atomic::AtomicBool::new(false));
        assert!(sum.errors.is_empty(), "{:?}", sum.errors);
        let ob = Outbox::new(&e.dir);
        (e, ob)
    }

    fn queue(ob: &Outbox, subject: &str, body: &str) -> Item {
        let d = draft("acct1", subject, body);
        queue_draft(ob, &account(), &d, 0, NOW, DATE).unwrap()
    }

    fn sent_list(e: &Env) -> Vec<crate::mail_db::MessageSummary> {
        e.list("Sent")
    }

    // spec: MAIL-132, MAIL-141
    #[test]
    fn a_queued_mail_is_sent_once_with_the_exact_bytes_and_the_sent_copy_is_recorded_and_appended() {
        let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
        let c = connector(&smtp);
        let (e, ob) = with_sent("t1");
        let item = queue(&ob, "Plans für Köln", "Hallo Bob");
        let eml = ob.read_eml("acct1", &item.id).unwrap();
        let r = send_due(&ob, &e.store, &account(), &c, NOW);
        assert_eq!((r.sent.len(), r.failed.len(), r.unsure.len()), (1, 0, 0), "{r:?}");
        assert_eq!(smtp.state().received.len(), 1);
        assert_eq!(smtp.state().received[0].data, eml, "the bytes that arrived are the bytes in the outbox");
        assert_eq!(smtp.state().received[0].rcpts, vec!["bob@x.de"]);
        let it = ob.get("acct1", &item.id).unwrap();
        assert_eq!((it.state, it.copy_done), (State::Sent, true));
        // the Sent copy shows at once (before any append), is read, and waits for the server
        let sent = sent_list(&e);
        assert_eq!(sent.len(), 1);
        assert_eq!((sent[0].subject.as_str(), sent[0].pending.as_deref(), sent[0].is_seen()), ("Plans für Köln", Some("append"), true));
        assert!(sent[0].uid >= mail_ops::SYNTHETIC_UID_BASE);
        assert_eq!(e.store.read_raw(&sent[0]).unwrap(), eml);
        assert_eq!(e.pending().len(), 1);
        assert!(e.server.state().messages("Sent").is_empty(), "nothing was appended yet");
        // a second run sends nothing again
        let r = send_due(&ob, &e.store, &account(), &c, NOW + 5);
        assert_eq!(r, SendReport::default());
        assert_eq!(smtp.state().received.len(), 1);
        // the append: exactly one copy on the server, and the local row now has the server's UID
        let rep = e.run();
        assert_eq!((rep.done, rep.failed.len()), (1, 0), "{rep:?}");
        let on = e.server.state().messages("Sent");
        assert_eq!(on.len(), 1);
        assert_eq!((on[0].2.clone(), on[0].1.clone()), (eml.clone(), vec!["\\Seen".to_string()]));
        let sent = sent_list(&e);
        assert_eq!(sent.len(), 1, "the synthetic row is replaced, not doubled");
        assert_eq!((sent[0].uid, sent[0].pending.clone()), (on[0].0, None));
        assert_eq!(e.store.read_raw(&sent[0]).unwrap(), eml);
        assert!(e.pending().is_empty());
        // the local trash holds the synthetic file; nothing was deleted
        assert!(!e.dir.join(format!("acct1/Sent/1000/{}.eml", mail_ops::SYNTHETIC_UID_BASE)).exists());
        // and a sync afterwards agrees: one message, no duplicate
        let (tx, _rx) = std::sync::mpsc::channel();
        crate::imap_sync::sync_account(&e.store, "acct1", "Test", &e.conn, &crate::imap_sync::SyncOptions { now: Some(NOW), ..Default::default() }, &tx, &std::sync::atomic::AtomicBool::new(false));
        assert_eq!(sent_list(&e).len(), 1);
    }

    // spec: MAIL-141
    #[test]
    fn the_sync_leaves_a_local_sent_copy_alone_while_its_append_waits() {
        let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
        let (e, ob) = with_sent("t2");
        queue(&ob, "Waiting", "x");
        send_due(&ob, &e.store, &account(), &connector(&smtp), NOW);
        let (tx, _rx) = std::sync::mpsc::channel();
        let sum = crate::imap_sync::sync_account(&e.store, "acct1", "Test", &e.conn, &crate::imap_sync::SyncOptions { now: Some(NOW), ..Default::default() }, &tx, &std::sync::atomic::AtomicBool::new(false));
        assert!(sum.errors.is_empty());
        let sent = sent_list(&e);
        assert_eq!(sent.len(), 1);
        assert!(!sent[0].gone_on_server, "not marked as deleted on the server");
        // a flag action on it is refused with words while the copy is on its way
        let r = mail_ops::enqueue(&e.store, &sent, &mail_ops::Action::SetFlags { add: vec![crate::imap_client::MailFlag::Flagged], remove: vec![] }, NOW).unwrap();
        assert_eq!(r.queued, 0);
        assert!(r.notes[0].contains("being saved"), "{:?}", r.notes);
    }

    // spec: MAIL-142
    #[test]
    fn a_failing_append_never_resends_the_mail_it_waits_and_is_retried_once_the_server_is_back() {
        let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
        let c = connector(&smtp);
        let (e, ob) = with_sent("t3");
        let item = queue(&ob, "Append later", "text");
        send_due(&ob, &e.store, &account(), &c, NOW);
        // the network goes away: the append is offline and stays
        e.conn.offline.store(true, Ordering::SeqCst);
        let rep = e.run();
        assert!(rep.offline.is_some(), "{rep:?}");
        assert_eq!(e.pending().len(), 1);
        for t in 0..3 {
            let r = send_due(&ob, &e.store, &account(), &c, NOW + 10 + t);
            assert_eq!(r, SendReport::default(), "nothing is sent again because an append failed");
        }
        assert_eq!(smtp.state().received.len(), 1);
        assert_eq!(ob.get("acct1", &item.id).unwrap().state, State::Sent);
        e.conn.offline.store(false, Ordering::SeqCst);
        let rep = e.run();
        assert_eq!(rep.done, 1, "{rep:?}");
        assert_eq!(e.server.state().messages("Sent").len(), 1);
        assert_eq!(smtp.state().received.len(), 1);
    }

    // spec: MAIL-142
    #[test]
    fn an_append_the_server_stored_but_never_answered_is_not_appended_twice_and_a_refusal_is_told() {
        let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
        let c = connector(&smtp);
        let (e, ob) = with_sent("t4");
        queue(&ob, "Stored but unanswered", "text");
        send_due(&ob, &e.store, &account(), &c, NOW);
        e.server.state().hang_up_after_append = true;
        let rep = e.run();
        assert!(rep.offline.is_some(), "a broken connection is a retry: {rep:?}");
        assert_eq!(e.server.state().messages("Sent").len(), 1, "the server has it");
        e.server.state().hang_up_after_append = false;
        let rep = e.run();
        assert_eq!(rep.done, 1, "{rep:?}");
        assert_eq!(e.server.state().messages("Sent").len(), 1, "found by Message-ID, not appended again");
        assert_eq!(sent_list(&e).len(), 1);
        // a server that refuses: told in words, the mail stays sent
        queue(&ob, "Refused copy", "text");
        send_due(&ob, &e.store, &account(), &c, NOW + 1);
        e.server.state().fail_next_write = Some("[OVERQUOTA] mailbox full".into());
        let rep = e.run();
        assert_eq!(rep.failed.len(), 1, "{rep:?}");
        assert!(rep.failed[0].contains("was sent, but the copy") && rep.failed[0].contains("OVERQUOTA"), "{:?}", rep.failed);
        assert_eq!(smtp.state().received.len(), 2);
        assert!(e.pending().is_empty());
    }

    // spec: MAIL-141
    #[test]
    fn accounts_that_store_sent_mail_themselves_and_accounts_without_a_sent_folder_get_no_copy() {
        let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
        let c = connector(&smtp);
        let (e, ob) = with_sent("t5");
        queue(&ob, "Gmail style", "text");
        let mut a = account();
        a.server_stores_sent = true;
        let r = send_due(&ob, &e.store, &a, &c, NOW);
        assert_eq!(r.sent.len(), 1);
        assert!(sent_list(&e).is_empty() && e.pending().is_empty(), "the server keeps its own copy");
        assert!(ob.list(None).iter().all(|i| i.copy_done));
        // an account whose index knows no Sent folder: sent, told, nothing else
        let root = temp("nosent");
        let store = MailStore::open(&root).unwrap();
        let ob2 = Outbox::new(&root);
        queue_draft(&ob2, &account(), &draft("acct1", "No folder", "x"), 0, NOW, DATE).unwrap();
        let r = send_due(&ob2, &store, &account(), &c, NOW);
        assert_eq!(r.sent.len(), 1);
        assert!(r.notes.iter().any(|n| n.contains("no Sent folder")), "{:?}", r.notes);
        assert!(ob2.list(None)[0].copy_done);
        drop(store);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-133, MAIL-136
    #[test]
    fn a_refused_recipient_fails_the_mail_with_the_servers_words_and_nothing_retries_it() {
        let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
        smtp.state().fail_rcpt = Some("bob@x.de".into());
        let c = connector(&smtp);
        let (e, ob) = with_sent("t6");
        let item = queue(&ob, "Refused", "x");
        let r = send_due(&ob, &e.store, &account(), &c, NOW);
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].1.contains("550"), "{r:?}");
        let it = ob.get("acct1", &item.id).unwrap();
        assert_eq!(it.state, State::Failed);
        assert!(it.error.unwrap().contains("bob@x.de"));
        for t in 1..4 {
            assert_eq!(send_due(&ob, &e.store, &account(), &c, NOW + t), SendReport::default());
        }
        assert_eq!(c.connects.load(Ordering::SeqCst), 1, "not even a connection is made for it again");
        assert!(sent_list(&e).is_empty() && e.pending().is_empty(), "no Sent copy of a mail that was not sent");
        // the person's retry sends it
        smtp.state().fail_rcpt = None;
        ob.retry("acct1", &item.id, NOW + 10).unwrap();
        assert_eq!(send_due(&ob, &e.store, &account(), &c, NOW + 10).sent.len(), 1);
    }

    // spec: MAIL-133
    #[test]
    fn a_connection_lost_after_the_data_makes_the_mail_unsure_with_no_sent_copy_and_no_retry() {
        let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
        smtp.state().drop_after_data = true;
        let c = connector(&smtp);
        let (e, ob) = with_sent("t7");
        let item = queue(&ob, "Unsure", "x");
        let r = send_due(&ob, &e.store, &account(), &c, NOW);
        assert_eq!(r.unsure, vec![item.id.clone()]);
        assert_eq!(ob.get("acct1", &item.id).unwrap().state, State::Unsure);
        assert_eq!(smtp.state().received.len(), 1, "it did arrive: nobody could have known");
        for t in 1..4 {
            send_due(&ob, &e.store, &account(), &c, NOW + t);
        }
        assert_eq!(smtp.state().received.len(), 1, "and it is not sent again by itself");
        assert!(sent_list(&e).is_empty());
        // the person looked in Sent and sends it again on purpose
        smtp.state().drop_after_data = false;
        ob.send_again("acct1", &item.id, NOW + 9).unwrap();
        assert_eq!(send_due(&ob, &e.store, &account(), &c, NOW + 9).sent.len(), 1);
        assert_eq!(smtp.state().received.len(), 2);
    }

    // spec: MAIL-134
    #[test]
    fn nothing_is_sent_before_the_undo_time_and_undo_takes_it_back() {
        let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
        let c = connector(&smtp);
        let (e, ob) = with_sent("t8");
        let d = draft("acct1", "Undo me", "x");
        let item = queue_draft(&ob, &account(), &d, 10, NOW, DATE).unwrap();
        assert_eq!(item.send_after, NOW + 10);
        assert_eq!(send_due(&ob, &e.store, &account(), &c, NOW + 9), SendReport::default());
        assert_eq!(c.connects.load(Ordering::SeqCst), 0);
        let back = ob.undo("acct1", &item.id, NOW + 9, DATE).unwrap();
        assert_eq!(back.subject, "Undo me");
        assert_eq!(send_due(&ob, &e.store, &account(), &c, NOW + 99), SendReport::default());
        assert!(smtp.state().received.is_empty());
    }

    // spec: MAIL-133
    #[test]
    fn a_crash_after_sent_before_the_copy_is_recorded_makes_the_copy_at_the_next_run_once() {
        let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
        let c = connector(&smtp);
        let (e, ob) = with_sent("t9");
        let item = queue(&ob, "Crash copy", "x");
        // the same steps as send_due, cut off after Sent was written
        let (it, bytes) = ob.begin_send("acct1", &item.id, NOW).unwrap();
        Smtp::connect_test(&c.settings, server_cert(), &mut crate::imap_client::PlainAuth::new("ada", "secret1")).unwrap().send(&it.from, &it.recipients, &bytes).unwrap();
        ob.finish("acct1", &item.id, Outcome::Sent, NOW).unwrap();
        // restart: recover, then the resume
        let ob = Outbox::new(&e.dir);
        ob.recover(NOW + 1, DATE).unwrap();
        let mut r = SendReport::default();
        resume_copies(&ob, &e.store, &account(), NOW + 2, &mut r);
        resume_copies(&ob, &e.store, &account(), NOW + 3, &mut r);
        assert_eq!(sent_list(&e).len(), 1, "once, however often it is resumed");
        assert_eq!(e.pending().len(), 1);
        assert!(ob.get("acct1", &item.id).unwrap().copy_done);
        assert_eq!(smtp.state().received.len(), 1);
    }
}
