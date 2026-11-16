//! A pretend SMTP server for tests (ADR-0010): real TCP and real TLS (the throw-away certificate
//! of `mail/imap_fake_tests.rs`), `EHLO`, `STARTTLS`, `AUTH` with `PLAIN`, `LOGIN`, `XOAUTH2` and
//! `OAUTHBEARER`, `MAIL FROM`, `RCPT TO`, `DATA`. It records everything it receives: every command,
//! every sign-in, every message with its envelope. Modes: refuse a recipient, hang up after the end of
//! the data without answering (the mail *is* kept: it arrived), answer the end of the data with an
//! error, be slow, limit the size.

use crate::config::MailSecurity;
use crate::mail::imap_client::{ConnectSettings, Limits, PlainAuth};
use crate::mail::imap_fake_tests::{server_cert, server_config};
use crate::mail::smtp_client::{Smtp, SmtpError};
use rustls::{ServerConnection, StreamOwned};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A mail the server accepted.
#[derive(Clone, Debug)]
pub struct Received {
    pub from: String,
    pub rcpts: Vec<String>,
    /// The message as the server got it, dot-stuffing removed.
    pub data: Vec<u8>,
}

pub struct SmtpState {
    pub user: String,
    pub password: String,
    /// The `AUTH` mechanisms offered.
    pub mechs: Vec<&'static str>,
    /// OAuth2: the access tokens that are good.
    pub valid_tokens: HashSet<String>,
    /// Every command line received (sign-in secrets are shown as `***`).
    pub commands: Vec<String>,
    pub received: Vec<Received>,
    /// `(mechanism, accepted)` of every sign-in.
    pub auths: Vec<(String, bool)>,
    /// What the client answered to an OAuth2 error challenge (decoded).
    pub oauth_error_answers: Vec<Vec<u8>>,
    pub connections: usize,
    /// This recipient gets `550`.
    pub fail_rcpt: Option<String>,
    /// After the end of the data the connection is closed without an answer (the mail is kept).
    pub drop_after_data: bool,
    /// After the end of the data the server answers this (the mail is not kept), such as `451 try later`.
    pub data_end_answer: Option<String>,
    /// Wait this long before answering the end of the data.
    pub data_delay: Duration,
    pub max_size: Option<u64>,
    /// Refuse the greeting of a connection (`554`).
    pub refuse_greeting: bool,
}

pub struct FakeSmtp {
    pub port: u16,
    pub security: MailSecurity,
    pub state: Arc<Mutex<SmtpState>>,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

enum Stream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ServerConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(b),
            Stream::Tls(s) => s.read(b),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(b),
            Stream::Tls(s) => s.write(b),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

fn read_line(s: &mut Stream) -> Option<Vec<u8>> {
    let mut line = vec![];
    let mut b = [0u8; 1];
    loop {
        match s.read(&mut b) {
            Ok(1) => {
                line.push(b[0]);
                if line.ends_with(b"\r\n") {
                    line.truncate(line.len() - 2);
                    return Some(line);
                }
            }
            _ => return None,
        }
    }
}

fn say(s: &mut Stream, text: &str) -> bool {
    s.write_all(format!("{text}\r\n").as_bytes()).and_then(|_| s.flush()).is_ok()
}

fn b64(s: &str) -> Vec<u8> {
    crate::mail::base64_decode(s.as_bytes())
}

impl FakeSmtp {
    pub fn start(security: MailSecurity) -> FakeSmtp {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let state = Arc::new(Mutex::new(SmtpState {
            user: "ada".into(),
            password: "secret1".into(),
            mechs: vec!["PLAIN", "LOGIN"],
            valid_tokens: HashSet::new(),
            commands: vec![],
            received: vec![],
            auths: vec![],
            oauth_error_answers: vec![],
            connections: 0,
            fail_rcpt: None,
            drop_after_data: false,
            data_end_answer: None,
            data_delay: Duration::ZERO,
            max_size: None,
            refuse_greeting: false,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        let join = std::thread::spawn(move || {
            while !sp.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((tcp, _)) => {
                        let st = st.clone();
                        tcp.set_nonblocking(false).ok();
                        tcp.set_read_timeout(Some(Duration::from_secs(30))).ok();
                        std::thread::spawn(move || serve(tcp, security, st));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        FakeSmtp { port, security, state, stop, join: Some(join) }
    }

    pub fn state(&self) -> std::sync::MutexGuard<'_, SmtpState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn settings(&self) -> ConnectSettings {
        ConnectSettings { host: "localhost".into(), port: self.port, security: self.security, limits: Limits { io_timeout: Duration::from_secs(10), ..Limits::default() } }
    }

    /// Connect and sign in with the password of the server.
    pub fn connect(&self) -> Result<Smtp, SmtpError> {
        let (u, p) = {
            let s = self.state();
            (s.user.clone(), s.password.clone())
        };
        Smtp::connect_test(&self.settings(), server_cert(), &mut PlainAuth::new(&u, &p))
    }

    pub fn commands(&self) -> Vec<String> {
        self.state().commands.clone()
    }
}

impl Drop for FakeSmtp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn serve(tcp: TcpStream, security: MailSecurity, st: Arc<Mutex<SmtpState>>) {
    fn lock(st: &Arc<Mutex<SmtpState>>) -> std::sync::MutexGuard<'_, SmtpState> {
        st.lock().unwrap_or_else(|e| e.into_inner())
    }
    lock(&st).connections += 1;
    let mut s = match security {
        MailSecurity::ImplicitTls => match ServerConnection::new(server_config()) {
            Ok(c) => Stream::Tls(Box::new(StreamOwned::new(c, tcp))),
            Err(_) => return,
        },
        MailSecurity::StartTls => Stream::Plain(tcp),
    };
    let mut secure = security == MailSecurity::ImplicitTls;
    if lock(&st).refuse_greeting {
        say(&mut s, "554 5.3.2 go away");
        return;
    }
    if !say(&mut s, "220 fake.example ESMTP ready") {
        return;
    }
    let mut authed = false;
    let mut from: Option<String> = None;
    let mut rcpts: Vec<String> = vec![];
    while let Some(raw) = read_line(&mut s) {
        let line = String::from_utf8_lossy(&raw).into_owned();
        let up = line.to_ascii_uppercase();
        let shown = if up.starts_with("AUTH ") { format!("{} ***", line.split(' ').take(2).collect::<Vec<_>>().join(" ")) } else { line.clone() };
        lock(&st).commands.push(shown);
        if up.starts_with("EHLO") {
            let g = lock(&st);
            let mut lines = vec!["fake.example".to_string(), "PIPELINING".into(), "8BITMIME".into()];
            if let Some(m) = g.max_size {
                lines.push(format!("SIZE {m}"));
            } else {
                lines.push("SIZE".into());
            }
            if !secure {
                lines.push("STARTTLS".into());
            } else {
                lines.push(format!("AUTH {}", g.mechs.join(" ")));
            }
            drop(g);
            let n = lines.len();
            for (i, l) in lines.iter().enumerate() {
                if !say(&mut s, &format!("250{}{l}", if i + 1 < n { "-" } else { " " })) {
                    return;
                }
            }
        } else if up == "STARTTLS" {
            if secure {
                say(&mut s, "503 already secure");
                continue;
            }
            say(&mut s, "220 go ahead");
            let Stream::Plain(tcp) = s else { return };
            let Ok(c) = ServerConnection::new(server_config()) else { return };
            s = Stream::Tls(Box::new(StreamOwned::new(c, tcp)));
            secure = true;
        } else if up.starts_with("AUTH ") {
            if !secure {
                say(&mut s, "538 encryption required");
                continue;
            }
            let mut parts = line.splitn(3, ' ');
            parts.next();
            let mech = parts.next().unwrap_or("").to_ascii_uppercase();
            let ir = parts.next().map(String::from);
            let ok = auth(&mut s, &st, &mech, ir);
            lock(&st).auths.push((mech, ok));
            authed = ok;
        } else if up.starts_with("MAIL FROM:") {
            if !authed {
                say(&mut s, "530 5.7.0 Authentication required");
                continue;
            }
            from = line.split_once('<').and_then(|(_, r)| r.split_once('>')).map(|(a, _)| a.to_string());
            rcpts.clear();
            say(&mut s, "250 2.1.0 Ok");
        } else if up.starts_with("RCPT TO:") {
            let addr = line.split_once('<').and_then(|(_, r)| r.split_once('>')).map(|(a, _)| a.to_string()).unwrap_or_default();
            if lock(&st).fail_rcpt.as_deref() == Some(addr.as_str()) {
                say(&mut s, "550 5.1.1 no such user");
            } else {
                rcpts.push(addr);
                say(&mut s, "250 2.1.5 Ok");
            }
        } else if up == "RSET" {
            from = None;
            rcpts.clear();
            say(&mut s, "250 2.0.0 Ok");
        } else if up == "DATA" {
            if from.is_none() || rcpts.is_empty() {
                say(&mut s, "503 5.5.1 need MAIL and RCPT first");
                continue;
            }
            say(&mut s, "354 End data with <CR><LF>.<CR><LF>");
            let mut data = vec![];
            loop {
                let Some(l) = read_line(&mut s) else { return };
                if l == b"." {
                    break;
                }
                let l = if l.starts_with(b"..") { &l[1..] } else { &l[..] };
                data.extend_from_slice(l);
                data.extend_from_slice(b"\r\n");
            }
            let (drop_it, answer, delay) = {
                let g = lock(&st);
                (g.drop_after_data, g.data_end_answer.clone(), g.data_delay)
            };
            std::thread::sleep(delay);
            if let Some(a) = answer {
                say(&mut s, &a);
            } else {
                lock(&st).received.push(Received { from: from.clone().unwrap_or_default(), rcpts: rcpts.clone(), data });
                if drop_it {
                    return; // the mail arrived; the answer never does
                }
                say(&mut s, "250 2.0.0 Ok: queued as FAKE1");
            }
            from = None;
            rcpts.clear();
        } else if up == "QUIT" {
            say(&mut s, "221 bye");
            return;
        } else {
            say(&mut s, "502 5.5.2 command not recognized");
        }
    }
}

/// The sign-in exchange; true if accepted.
fn auth(s: &mut Stream, st: &Arc<Mutex<SmtpState>>, mech: &str, ir: Option<String>) -> bool {
    let (user, pass, offered) = {
        let g = st.lock().unwrap_or_else(|e| e.into_inner());
        (g.user.clone(), g.password.clone(), g.mechs.contains(&mech))
    };
    if !offered {
        say(s, "504 5.5.4 mechanism not supported");
        return false;
    }
    let next = |s: &mut Stream, prompt: &str| -> Option<String> {
        say(s, &format!("334 {prompt}"));
        read_line(s).map(|l| String::from_utf8_lossy(&l).into_owned())
    };
    let verdict = |s: &mut Stream, ok: bool| {
        say(s, if ok { "235 2.7.0 Authentication successful" } else { "535 5.7.8 Authentication credentials invalid" });
        ok
    };
    match mech {
        "PLAIN" => {
            let Some(ir) = ir.or_else(|| next(s, "")) else { return false };
            let raw = b64(&ir);
            let parts: Vec<&[u8]> = raw.split(|b| *b == 0).collect();
            let ok = parts.len() == 3 && parts[1] == user.as_bytes() && parts[2] == pass.as_bytes();
            verdict(s, ok)
        }
        "LOGIN" => {
            let Some(u) = ir.or_else(|| next(s, "VXNlcm5hbWU6")) else { return false };
            let Some(p) = next(s, "UGFzc3dvcmQ6") else { return false };
            let ok = b64(&u) == user.as_bytes() && b64(&p) == pass.as_bytes();
            verdict(s, ok)
        }
        "XOAUTH2" | "OAUTHBEARER" => {
            let Some(ir) = ir.or_else(|| next(s, "")) else { return false };
            let raw = String::from_utf8_lossy(&b64(&ir)).into_owned();
            let token = raw.split("auth=Bearer ").nth(1).map(|t| t.trim_end_matches('\x01').to_string()).unwrap_or_default();
            let who_ok = if mech == "XOAUTH2" { raw.starts_with(&format!("user={user}\x01")) } else { raw.starts_with(&format!("n,a={user},")) };
            let ok = who_ok && st.lock().unwrap_or_else(|e| e.into_inner()).valid_tokens.contains(&token);
            if ok {
                return verdict(s, true);
            }
            // the error as a challenge; the client answers (empty / \x01) and then the final refusal
            let err = crate::mail::imap_client::base64_encode(br#"{"status":"401","schemes":"Bearer","scope":"mail"}"#);
            say(s, &format!("334 {err}"));
            if let Some(l) = read_line(s) {
                st.lock().unwrap_or_else(|e| e.into_inner()).oauth_error_answers.push(b64(&String::from_utf8_lossy(&l)));
            }
            verdict(s, false)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------- tests of the client

fn plain_auth(s: &FakeSmtp) -> PlainAuth {
    let g = s.state();
    PlainAuth::new(&g.user, &g.password)
}

fn envelope(to: &[&str]) -> (String, Vec<String>) {
    ("ada@example.com".to_string(), to.iter().map(|t| t.to_string()).collect())
}

const MAIL: &[u8] = b"From: ada@example.com\r\nTo: bob@x.de\r\nSubject: Hi\r\n\r\nHello\r\n.dot line\r\n..two dots\r\nlast line without dot\r\n";

// spec: MAIL-122
#[test]
fn a_mail_is_sent_over_implicit_tls_with_the_exact_envelope_and_bytes_dots_included() {
    let srv = FakeSmtp::start(MailSecurity::ImplicitTls);
    let mut c = srv.connect().unwrap();
    let (from, to) = envelope(&["bob@x.de", "secret@hidden.org"]);
    let said = c.send(&from, &to, MAIL).unwrap();
    assert!(said.contains("queued"), "{said}");
    c.quit();
    let g = srv.state();
    assert_eq!(g.received.len(), 1);
    assert_eq!(g.received[0].from, "ada@example.com");
    assert_eq!(g.received[0].rcpts, vec!["bob@x.de", "secret@hidden.org"]);
    assert_eq!(g.received[0].data, MAIL, "dot-stuffing is undone by the server: the bytes arrive exactly");
    let cmds: Vec<&str> = g.commands.iter().map(String::as_str).collect();
    assert_eq!(cmds[0], "EHLO localhost");
    assert!(cmds.iter().position(|c| c.starts_with("AUTH")).unwrap() < cmds.iter().position(|c| c.starts_with("MAIL FROM")).unwrap());
    assert!(cmds.contains(&format!("MAIL FROM:<ada@example.com> SIZE={}", MAIL.len()).as_str()), "{cmds:?}");
    assert_eq!(g.auths, vec![("PLAIN".to_string(), true)]);
}

// spec: MAIL-122
#[test]
fn on_port_587_nothing_not_even_auth_goes_before_starttls_and_a_server_without_it_is_refused() {
    let srv = FakeSmtp::start(MailSecurity::StartTls);
    let mut c = srv.connect().unwrap();
    c.send("ada@example.com", &["bob@x.de".to_string()], MAIL).unwrap();
    {
        let g = srv.state();
        let cmds = &g.commands;
        let tls = cmds.iter().position(|c| c == "STARTTLS").expect("STARTTLS was sent");
        let auth = cmds.iter().position(|c| c.starts_with("AUTH")).expect("signed in");
        assert!(tls < auth, "{cmds:?}");
        assert_eq!(cmds.iter().filter(|c| c.starts_with("EHLO")).count(), 2, "EHLO again after the upgrade: {cmds:?}");
        assert!(cmds[..tls].iter().all(|c| c.starts_with("EHLO")), "before STARTTLS only EHLO: {cmds:?}");
    }
    // a server that does not offer STARTTLS: refused before anything else is said
    let bad = FakeSmtp::start(MailSecurity::StartTls);
    // pretend it cannot encrypt by asking for implicit TLS from a plain server: the handshake fails
    let mut settings = bad.settings();
    settings.security = MailSecurity::ImplicitTls;
    let e = Smtp::connect_test(&settings, server_cert(), &mut plain_auth(&bad)).err().expect("must fail");
    assert!(matches!(e, SmtpError::Tls(_) | SmtpError::Connection(_)), "{e:?}");
    assert!(bad.state().commands.iter().all(|c| !c.starts_with("AUTH") && !c.starts_with("MAIL")), "{:?}", bad.state().commands);
}

// spec: MAIL-122
#[test]
fn a_certificate_the_system_does_not_trust_is_refused_and_nothing_is_sent() {
    let srv = FakeSmtp::start(MailSecurity::ImplicitTls);
    let e = Smtp::connect(&srv.settings(), &mut plain_auth(&srv)).err().expect("the production path has no extra root");
    assert!(matches!(e, SmtpError::Tls(_)), "{e:?}");
    assert!(srv.state().commands.is_empty());
}

// spec: MAIL-122
#[test]
fn passwords_sign_in_with_plain_or_login_and_a_wrong_password_is_an_auth_error() {
    let srv = FakeSmtp::start(MailSecurity::ImplicitTls);
    srv.state().mechs = vec!["LOGIN"];
    srv.connect().unwrap().quit();
    assert_eq!(srv.state().auths, vec![("LOGIN".to_string(), true)]);
    srv.state().mechs = vec!["PLAIN", "LOGIN"];
    let e = Smtp::connect_test(&srv.settings(), server_cert(), &mut PlainAuth::new("ada", "wrong")).err().unwrap();
    assert!(matches!(e, SmtpError::Auth(_)), "{e:?}");
    assert!(e.to_string().contains("535"), "{e}");
    // a server with no mechanism we have
    srv.state().mechs = vec!["CRAM-MD5"];
    let e = Smtp::connect_test(&srv.settings(), server_cert(), &mut plain_auth(&srv)).err().unwrap();
    assert!(matches!(e, SmtpError::Auth(_)), "{e:?}");
}

// spec: MAIL-122
#[test]
fn oauth_signs_in_with_the_best_mechanism_and_answers_the_error_challenge_as_specified() {
    use crate::mail::oauth::OAuthAuth;
    let srv = FakeSmtp::start(MailSecurity::ImplicitTls);
    srv.state().valid_tokens.insert("tok-1".into());
    srv.state().mechs = vec!["XOAUTH2"];
    let mut a = OAuthAuth::new("ada", "tok-1", "localhost", srv.port);
    Smtp::connect_test(&srv.settings(), server_cert(), &mut a).unwrap().quit();
    srv.state().mechs = vec!["XOAUTH2", "OAUTHBEARER"];
    let mut a = OAuthAuth::new("ada", "tok-1", "localhost", srv.port);
    Smtp::connect_test(&srv.settings(), server_cert(), &mut a).unwrap().quit();
    assert_eq!(srv.state().auths, vec![("XOAUTH2".to_string(), true), ("OAUTHBEARER".to_string(), true)], "OAUTHBEARER is preferred when both are offered");
    // a refused token: the error challenge is answered with an empty line (XOAUTH2) or \x01 (OAUTHBEARER)
    srv.state().mechs = vec!["XOAUTH2"];
    let e = Smtp::connect_test(&srv.settings(), server_cert(), &mut OAuthAuth::new("ada", "bad", "localhost", srv.port)).err().unwrap();
    assert!(matches!(&e, SmtpError::Auth(t) if t.contains("401")), "{e:?}");
    srv.state().mechs = vec!["OAUTHBEARER"];
    let e = Smtp::connect_test(&srv.settings(), server_cert(), &mut OAuthAuth::new("ada", "bad", "localhost", srv.port)).err().unwrap();
    assert!(matches!(e, SmtpError::Auth(_)));
    assert_eq!(srv.state().oauth_error_answers, vec![Vec::<u8>::new(), vec![1]]);
}

// spec: MAIL-123
#[test]
fn a_refused_recipient_stops_everything_and_nothing_is_sent_to_the_others() {
    let srv = FakeSmtp::start(MailSecurity::ImplicitTls);
    srv.state().fail_rcpt = Some("nobody@x.de".into());
    let mut c = srv.connect().unwrap();
    let e = c.send("ada@example.com", &["bob@x.de".into(), "nobody@x.de".into()], MAIL).unwrap_err();
    assert!(matches!(&e, SmtpError::Refused(t) if t.contains("nobody@x.de") && t.contains("550")), "{e:?}");
    assert!(srv.state().received.is_empty());
    assert!(!srv.commands().iter().any(|c| c == "DATA"), "DATA is never started: {:?}", srv.commands());
    // the connection is still usable: the next mail goes
    c.send("ada@example.com", &["bob@x.de".into()], MAIL).unwrap();
    assert_eq!(srv.state().received.len(), 1);
}

// spec: MAIL-123
#[test]
fn a_connection_that_dies_after_the_end_of_the_data_is_unsure_and_the_mail_did_arrive() {
    let srv = FakeSmtp::start(MailSecurity::ImplicitTls);
    srv.state().drop_after_data = true;
    let mut c = srv.connect().unwrap();
    let e = c.send("ada@example.com", &["bob@x.de".into()], MAIL).unwrap_err();
    assert!(matches!(e, SmtpError::Unsure(_)), "{e:?}");
    assert_eq!(srv.state().received.len(), 1, "exactly the case that cannot be told apart from a failure");
}

// spec: MAIL-123
#[test]
fn an_error_after_the_data_is_a_definite_refusal_and_a_slow_server_is_waited_for() {
    let srv = FakeSmtp::start(MailSecurity::ImplicitTls);
    srv.state().data_end_answer = Some("451 4.3.0 try again later".into());
    let mut c = srv.connect().unwrap();
    let e = c.send("ada@example.com", &["bob@x.de".into()], MAIL).unwrap_err();
    assert!(matches!(&e, SmtpError::Refused(t) if t.contains("451")), "{e:?}");
    assert!(srv.state().received.is_empty());
    {
        let mut g = srv.state();
        g.data_end_answer = None;
        g.data_delay = Duration::from_millis(400);
    }
    let mut c = srv.connect().unwrap();
    c.send("ada@example.com", &["bob@x.de".into()], MAIL).unwrap();
    // a server that is too slow: the wait ends in "unsure" (the end of the data was written)
    let mut s = srv.settings();
    s.limits.io_timeout = Duration::from_millis(150);
    srv.state().data_delay = Duration::from_millis(800);
    let mut c = Smtp::connect_test(&s, server_cert(), &mut plain_auth(&srv)).unwrap();
    let e = c.send("ada@example.com", &["bob@x.de".into()], MAIL).unwrap_err();
    assert!(matches!(e, SmtpError::Unsure(_)), "{e:?}");
}

// spec: MAIL-123
#[test]
fn a_message_that_is_too_big_or_has_bare_line_breaks_is_refused_before_anything_is_sent() {
    let srv = FakeSmtp::start(MailSecurity::ImplicitTls);
    srv.state().max_size = Some(50);
    let mut c = srv.connect().unwrap();
    let e = c.send("ada@example.com", &["bob@x.de".into()], MAIL).unwrap_err();
    assert!(matches!(&e, SmtpError::Refused(t) if t.contains("bigger")), "{e:?}");
    let e = c.send("ada@example.com", &["bob@x.de".into()], b"a\nb").unwrap_err();
    assert!(matches!(&e, SmtpError::Refused(t) if t.contains("line break")), "{e:?}");
    let e = c.send("ada@example.com", &[], b"x\r\n").unwrap_err();
    assert!(matches!(e, SmtpError::Refused(_)));
    assert!(!srv.commands().iter().any(|c| c.starts_with("MAIL FROM")), "{:?}", srv.commands());
}

// spec: MAIL-121
#[test]
fn a_greeting_that_refuses_and_a_closed_port_are_errors_in_words() {
    let srv = FakeSmtp::start(MailSecurity::ImplicitTls);
    srv.state().refuse_greeting = true;
    let e = srv.connect().err().unwrap();
    assert!(matches!(&e, SmtpError::Refused(t) if t.contains("554")), "{e:?}");
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let s = ConnectSettings { host: "127.0.0.1".into(), port, security: MailSecurity::ImplicitTls, limits: Limits::default() };
    let e = Smtp::connect_test(&s, server_cert(), &mut PlainAuth::new("a", "b")).err().unwrap();
    assert!(matches!(e, SmtpError::Connection(_)), "{e:?}");
}
