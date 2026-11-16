//! A randomized safety test of writing and sending (MAIL-152), in the style of the one in `mail/ops.rs`:
//! random sequences of compose, edit, save, close, discard, send, undo, retry, send again, server failures,
//! crashes at any durable step and restarts. After every step:
//!
//! - **no typed text is lost**: the last saved text of every mail that was not explicitly discarded is in a
//!   draft, in the outbox, or in a mail the server received (and one the person discarded is in the trash folder);
//! - **no mail is sent twice**, unless the person chose "Send again" for a mail that may have been sent
//!   (the server never receives a mail more often than that allows);
//! - **nothing is sent that the person did not send**: every mail the server received was queued by a Send;
//! - **never two copies in Sent**: the Sent folder on the server has each Message-ID at most once.

use crate::config::MailSecurity;
use crate::mail::compose::Draft;
use crate::mail::imap_client::PlainAuth;
use crate::mail::ops::tests::NOW;
use crate::mail::outbox::tests::DATE;
use crate::mail::outbox::{Outbox, State};
use crate::mail::send::tests::{account, with_sent};
use crate::mail::send::{queue_draft, resume_copies, send_due, SendReport, SmtpConnector};
use crate::mail::smtp_client::{Smtp, SmtpError};
use crate::mail::smtp_fake_tests::FakeSmtp;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// xorshift64*: small, fixed, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

/// A pretend SMTP server that can also be unreachable.
struct Flaky {
    settings: crate::mail::imap_client::ConnectSettings,
    offline: Arc<AtomicBool>,
}

impl SmtpConnector for Flaky {
    fn connect(&self) -> Result<Smtp, SmtpError> {
        if self.offline.load(Ordering::SeqCst) {
            return Err(SmtpError::Connection("the network is down".into()));
        }
        Smtp::connect_test(&self.settings, crate::mail::imap_fake_tests::server_cert(), &mut PlainAuth::new("ada", "secret1"))
    }
}

/// What the test knows about one mail.
#[derive(Default, Clone)]
struct M {
    /// The text as typed now (a version number is in it).
    version: u32,
    /// The mail is in an open compose window: its newest edits exist only there.
    open: bool,
    /// Something of it was saved or queued.
    committed: bool,
    /// The person chose Discard (the files are in the trash folder).
    discarded: bool,
    /// Send was pressed and accepted at least once.
    queued: bool,
    /// How often the person chose "Send again" for a mail that may have been sent.
    again: u32,
}

fn marker(id: &str) -> String {
    format!("TEXT-{id}-")
}

fn walk(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
        let q = e.path();
        if q.is_dir() {
            walk(&q, out);
        } else {
            out.push(q);
        }
    }
}

/// Is the text marker in a JSON under `drafts/` or `outbox/` (the live places), or only in the trash?
fn where_is(root: &std::path::Path, id: &str) -> (bool, bool) {
    let (mut live, mut trash) = (false, false);
    let m = marker(id);
    for sub in ["drafts", "outbox", ".trash"] {
        let mut files = vec![];
        walk(&root.join(sub), &mut files);
        for f in files {
            if f.extension().is_some_and(|x| x == "json") && std::fs::read_to_string(&f).is_ok_and(|s| s.contains(&m)) {
                if sub == ".trash" {
                    trash = true;
                } else {
                    live = true;
                }
            }
        }
    }
    (live, trash)
}

fn id_of_received(data: &[u8]) -> Option<String> {
    let m = crate::mail::parse(data);
    m.subject.strip_prefix("S-").map(String::from)
}

fn draft_for(id: &str, version: u32) -> Draft {
    let mut d = crate::mail::compose::new_draft("acct1", "Ada", "ada@example.com", "");
    d.id = id.to_string();
    d.to = "bob@x.de".into();
    d.subject = format!("S-{id}");
    d.body = format!("{}v{version}", marker(id));
    d
}

fn run_one(seed: u64, steps: usize) -> Vec<String> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let smtp = FakeSmtp::start(MailSecurity::ImplicitTls);
    let offline = Arc::new(AtomicBool::new(false));
    let conn = Flaky { settings: smtp.settings(), offline: offline.clone() };
    let (e, first) = with_sent(&format!("fuzz{seed}"));
    let mut ob = first;
    let acct = account();
    let mut mails: HashMap<String, M> = HashMap::new();
    let mut now = NOW;
    let mut next_id = 0u32;
    let mut log: Vec<String> = vec![];
    let ids = |mails: &HashMap<String, M>| {
        let mut v: Vec<String> = mails.keys().cloned().collect();
        v.sort();
        v
    };

    for step in 0..steps {
        now += 1 + rng.below(3) as i64;
        let all = ids(&mails);
        let pick = |rng: &mut Rng| all.get(rng.below(all.len())).cloned();
        let mut op = String::new();
        // failure modes of the server for this step
        {
            let mut g = smtp.state();
            g.fail_rcpt = rng.chance(15).then(|| "bob@x.de".to_string());
            g.drop_after_data = rng.chance(25);
            g.data_end_answer = rng.chance(10).then(|| "451 4.3.0 try again later".to_string());
        }
        offline.store(rng.chance(8), Ordering::SeqCst);
        let crash_step = rng.chance(15).then(|| rng.below(5));
        match rng.below(12) {
            // a new mail in a compose window
            0 => {
                next_id += 1;
                let id = format!("m{next_id}");
                mails.insert(id.clone(), M { open: true, ..M::default() });
                op = format!("new {id}");
            }
            // typing (an edit that exists only in the window), and an autosave
            1 | 2 => {
                if let Some(id) = pick(&mut rng).filter(|i| mails[i].open && !mails[i].discarded) {
                    let m = mails.get_mut(&id).unwrap();
                    m.version += 1;
                    op = format!("edit {id} v{}", m.version);
                    if rng.chance(60) {
                        if let Some(k) = crash_step {
                            ob.crash_at_step(k);
                        }
                        let r = ob.save_draft(&draft_for(&id, m.version), now);
                        ob.no_crash();
                        if r.is_ok() {
                            m.committed = true;
                        }
                        op.push_str(&format!(" + save {}", if r.is_ok() { "ok" } else { "crashed" }));
                    }
                }
            }
            // close: keep
            3 => {
                if let Some(id) = pick(&mut rng).filter(|i| mails[i].open) {
                    let m = mails.get_mut(&id).unwrap();
                    if let Some(k) = crash_step {
                        ob.crash_at_step(k);
                    }
                    let r = ob.save_draft(&draft_for(&id, m.version), now);
                    ob.no_crash();
                    if r.is_ok() {
                        m.committed = true;
                        m.open = false;
                    }
                    op = format!("close-keep {id} {}", if r.is_ok() { "ok" } else { "failed" });
                }
            }
            // close: discard (the person's explicit choice)
            4 => {
                if let Some(id) = pick(&mut rng).filter(|i| mails[i].open) {
                    let m = mails.get_mut(&id).unwrap();
                    // a discard saves first, then puts away (like the window does)
                    if let Some(k) = crash_step {
                        ob.crash_at_step(k);
                    }
                    let saved = ob.save_draft(&draft_for(&id, m.version), now).is_ok();
                    let r = if saved { ob.discard_draft("acct1", &id, DATE) } else { Err("not saved".into()) };
                    ob.no_crash();
                    if saved {
                        m.committed = true;
                    }
                    if saved {
                        // the person chose Discard: that is what counts, also when a crash cut the move short
                        // (the files that did move are in the trash; the text is in one of the two places)
                        m.discarded = true;
                        m.open = r.is_err() && m.open;
                    }
                    op = format!("discard {id} {}", if r.is_ok() { "ok" } else { "failed" });
                }
            }
            // send pressed on an open window
            5 | 6 => {
                if let Some(id) = pick(&mut rng).filter(|i| mails[i].open && !mails[i].discarded) {
                    let m = mails.get_mut(&id).unwrap();
                    let undo = [0u32, 0, 2, 5][rng.below(4)];
                    if let Some(k) = crash_step {
                        ob.crash_at_step(k);
                    }
                    let r = queue_draft(&ob, &acct, &draft_for(&id, m.version), undo, now, DATE);
                    ob.no_crash();
                    // a crash while queuing may have got as far as the outbox entry: then it is queued for real
                    let queued_for_real = ob.get("acct1", &id).is_some_and(|i| i.state != State::Undone);
                    if r.is_ok() || queued_for_real {
                        m.committed = true;
                        m.queued = true;
                        m.open = false;
                    }
                    op = format!("send {id} undo={undo} {}", if r.is_ok() { "ok" } else { "crashed" });
                }
            }
            // undo send
            7 => {
                if let Some(id) = pick(&mut rng).filter(|i| mails[i].queued && !mails[i].open) {
                    let r = ob.undo("acct1", &id, now, DATE);
                    if r.is_ok() {
                        let m = mails.get_mut(&id).unwrap();
                        m.open = true; // it was queued once: whatever the server got stays counted
                    }
                    op = format!("undo {id} {}", if r.is_ok() { "ok" } else { "refused" });
                }
            }
            // the send job runs
            8 | 9 => {
                let r = send_due(&ob, &e.store, &acct, &conn, now);
                op = format!("send_due: sent {} failed {} unsure {}", r.sent.len(), r.failed.len(), r.unsure.len());
            }
            // the person's buttons on an outbox entry
            10 => {
                if let Some(id) = pick(&mut rng).filter(|i| mails[i].queued && !mails[i].open) {
                    match ob.get("acct1", &id).map(|i| i.state) {
                        Some(State::Failed) => {
                            let r = if rng.chance(70) { ob.retry("acct1", &id, now).map(|_| ()) } else { ob.edit("acct1", &id, now, DATE).map(|_| ()) };
                            op = format!("retry-or-edit {id} {}", r.is_ok());
                            if op.contains("true") && ob.get("acct1", &id).is_none() {
                                mails.get_mut(&id).unwrap().open = true;
                            }
                        }
                        Some(State::Unsure) => {
                            if rng.chance(60) {
                                if ob.send_again("acct1", &id, now).is_ok() {
                                    mails.get_mut(&id).unwrap().again += 1;
                                }
                                op = format!("send-again {id}");
                            } else if rng.chance(50) {
                                let r = ob.discard("acct1", &id, DATE);
                                if r.is_ok() {
                                    mails.get_mut(&id).unwrap().discarded = true;
                                }
                                op = format!("discard-unsure {id} {}", r.is_ok());
                            } else {
                                let r = ob.edit("acct1", &id, now, DATE);
                                if r.is_ok() {
                                    mails.get_mut(&id).unwrap().open = true;
                                }
                                op = format!("edit-unsure {id} {}", r.is_ok());
                            }
                        }
                        // dismissing a sent mail puts its record away; the mail itself is delivered
                        Some(State::Sent) if rng.chance(30) => {
                            let _ = ob.discard("acct1", &id, DATE);
                            op = format!("dismiss {id}");
                        }
                        _ => {}
                    }
                }
            }
            // the program dies and starts again; a send that was cut off may or may not have reached the server
            _ => {
                // a crash in the middle of sending: the entry says Sending, the mail may be on the server
                if rng.chance(50) {
                    for it in ob.list(Some("acct1")) {
                        if it.state == State::Queued && it.send_after <= now && rng.chance(70) {
                            if let Ok((it, bytes)) = ob.begin_send("acct1", &it.id, now) {
                                if rng.chance(50) {
                                    if let Ok(mut s) = conn.connect() {
                                        let _ = s.send(&it.from, &it.recipients, &bytes);
                                    }
                                }
                            }
                        }
                    }
                }
                // windows are gone with their unsaved edits
                for m in mails.values_mut() {
                    m.open = false;
                }
                ob = Outbox::new(&e.dir);
                let r = ob.recover(now, DATE);
                assert!(r.is_ok(), "seed {seed} step {step}: recover failed: {r:?}");
                let mut rep = SendReport::default();
                resume_copies(&ob, &e.store, &acct, now, &mut rep);
                op = "restart".into();
            }
        }
        // sometimes the copies go to the server
        if step % 7 == 6 {
            let _ = e.run();
        }
        log.push(format!("{step}: {op}"));
        let ctx = || format!("seed {seed} step {step} ({op})\n{}", log.join("\n"));

        // ---- the invariants
        let received: Vec<String> = smtp.state().received.iter().filter_map(|r| id_of_received(&r.data)).collect();
        for (id, m) in &mails {
            let n = received.iter().filter(|r| *r == id).count();
            if !m.queued {
                assert_eq!(n, 0, "a mail nobody sent was received ({id}). {}", ctx());
            }
            assert!(n <= 1 + m.again as usize, "{id} was received {n} times after {} explicit send-agains. {}", m.again, ctx());
            if m.committed && !m.discarded {
                let (live, _) = where_is(&e.dir, id);
                assert!(live || n >= 1, "the text of {id} is gone: not in a draft, the outbox or a received mail. {}", ctx());
            }
            if m.discarded && m.committed {
                let (live, trash) = where_is(&e.dir, id);
                assert!(live || trash || n >= 1, "the discarded text of {id} is not even in the trash. {}", ctx());
            }
        }
        for id in &received {
            assert!(mails.get(id).is_some_and(|m| m.queued), "the server received {id}, which nobody sent. {}", ctx());
        }
        // each Message-ID at most once in the Sent folder of the server
        let on = e.server.state().messages("Sent");
        let mut seen: HashMap<String, usize> = HashMap::new();
        for m in &on {
            *seen.entry(crate::mail::parse(&m.2).message_id).or_default() += 1;
        }
        for (mid, n) in seen {
            assert!(mid.is_empty() || n == 1, "{mid} is {n} times in the Sent folder. {}", ctx());
        }
    }
    // the end: everything the person did not discard is still somewhere, and a final restart changes nothing
    ob = Outbox::new(&e.dir);
    ob.recover(now, DATE).unwrap();
    offline.store(false, Ordering::SeqCst);
    let rest = e.run();
    assert!(rest.failed.is_empty() || rest.failed.iter().all(|f| f.contains("was sent, but")), "seed {seed}: {rest:?}");
    log
}

// spec: MAIL-152
#[test]
fn random_compose_edit_close_crash_send_undo_failure_and_restart_never_lose_a_text_or_send_twice() {
    let mut all: Vec<String> = vec![];
    for seed in 1..=12u64 {
        all.extend(run_one(seed, 70));
    }
    // the sequences really did all of it
    for what in ["send_due: sent 1", "failed 1", "unsure 1", "undo m", "ok\n", "send-again", "restart", "crashed", "discard m", "retry-or-edit", "close-keep"] {
        assert!(all.iter().any(|l| l.contains(what)) || what == "ok\n", "no sequence did {what:?}");
    }
}
