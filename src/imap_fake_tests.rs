//! A pretend IMAP server for tests: real TCP and real TLS (a throw-away certificate), enough of
//! IMAP4rev1 for the sync (`CAPABILITY`, `STARTTLS`, `LOGIN`, `AUTHENTICATE PLAIN`, `LIST`,
//! `EXAMINE`, `UID SEARCH`, `UID FETCH`, optionally `CONDSTORE`). It records every command it
//! receives, so tests can check that the client never sends anything that changes a mailbox.
//! Commands it should never see (`SELECT`, `STORE` …) are answered `NO`.

use crate::cal_time::{civil_from_days, days_from_civil};
use crate::config::MailSecurity;
use crate::imap_client::{ConnectSettings, Limits};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct FakeMessage {
    pub uid: u32,
    pub flags: Vec<String>,
    /// UTC seconds.
    pub internal_date: i64,
    pub raw: Vec<u8>,
    pub modseq: u64,
}

#[derive(Clone, Debug)]
pub struct FakeFolder {
    pub name: String,
    pub attrs: Vec<String>,
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub highestmodseq: u64,
    pub msgs: Vec<FakeMessage>,
}

pub struct FakeState {
    pub folders: Vec<FakeFolder>,
    /// Every command line received, in order (tag included).
    pub commands: Vec<String>,
    pub condstore: bool,
    /// Offer `SASL-IR` (initial response in the AUTHENTICATE line).
    pub sasl_ir: bool,
    pub user: String,
    pub password: String,
    /// Wait this long before answering a fetch (to make connections overlap).
    pub fetch_delay: Duration,
    pub connections: usize,
    pub open_connections: usize,
    pub max_open_connections: usize,
    /// Folders on which a fetch answers a message with a huge literal of this many bytes.
    pub pad_message: Option<(u32, usize)>,
    /// Folders that answer EXAMINE with NO.
    pub refuse_examine: Vec<String>,
}

pub struct FakeServer {
    pub port: u16,
    pub security: MailSecurity,
    pub state: Arc<Mutex<FakeState>>,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

fn certified() -> &'static (CertificateDer<'static>, Vec<u8>) {
    static C: OnceLock<(CertificateDer<'static>, Vec<u8>)> = OnceLock::new();
    C.get_or_init(|| {
        let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("certificate");
        (ck.cert.der().clone(), ck.signing_key.serialize_der())
    })
}

/// The certificate that the pretend server presents (what a test client must be told to trust).
pub fn server_cert() -> CertificateDer<'static> {
    certified().0.clone()
}

fn server_config() -> Arc<ServerConfig> {
    let (cert, key) = certified();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(vec![cert.clone()], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.clone()))).unwrap();
    Arc::new(cfg)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn internal_date_text(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    format!("{d:02}-{}-{y} {:02}:{:02}:{:02} +0000", MONTHS[m as usize - 1], s / 3600, s % 3600 / 60, s % 60)
}

fn parse_search_date(s: &str) -> Option<i64> {
    let mut p = s.split('-');
    let d: u32 = p.next()?.parse().ok()?;
    let mon = p.next()?;
    let m = MONTHS.iter().position(|x| x.eq_ignore_ascii_case(mon))? as u32 + 1;
    let y: i32 = p.next()?.parse().ok()?;
    Some(days_from_civil(y, m, d) * 86_400)
}

impl FakeState {
    pub fn folder_mut(&mut self, name: &str) -> &mut FakeFolder {
        self.folders.iter_mut().find(|f| f.name == name).expect("no such folder in the pretend server")
    }

    pub fn add_folder(&mut self, name: &str, attrs: &[&str]) {
        self.folders.push(FakeFolder { name: name.into(), attrs: attrs.iter().map(|a| a.to_string()).collect(), uidvalidity: 1000, uidnext: 1, highestmodseq: 1, msgs: vec![] });
    }

    /// Add a message; returns its UID.
    pub fn add_message(&mut self, folder: &str, raw: Vec<u8>, internal_date: i64) -> u32 {
        let f = self.folder_mut(folder);
        let uid = f.uidnext;
        f.uidnext += 1;
        f.highestmodseq += 1;
        let modseq = f.highestmodseq;
        f.msgs.push(FakeMessage { uid, flags: vec![], internal_date, raw, modseq });
        uid
    }

    pub fn delete_message(&mut self, folder: &str, uid: u32) {
        let f = self.folder_mut(folder);
        f.msgs.retain(|m| m.uid != uid);
        f.highestmodseq += 1;
    }

    pub fn set_flags(&mut self, folder: &str, uid: u32, flags: &[&str]) {
        let f = self.folder_mut(folder);
        f.highestmodseq += 1;
        let ms = f.highestmodseq;
        if let Some(m) = f.msgs.iter_mut().find(|m| m.uid == uid) {
            m.flags = flags.iter().map(|s| s.to_string()).collect();
            m.modseq = ms;
        }
    }

    /// The server lost its numbering: new UIDVALIDITY, messages renumbered from 1.
    pub fn renumber(&mut self, folder: &str, new_validity: u32) {
        let f = self.folder_mut(folder);
        f.uidvalidity = new_validity;
        for (i, m) in f.msgs.iter_mut().enumerate() {
            m.uid = i as u32 + 1;
        }
        f.uidnext = f.msgs.len() as u32 + 1;
    }

    /// Commands received, without their tag.
    pub fn verbs(&self) -> Vec<String> {
        self.commands.iter().map(|c| c.split_once(' ').map(|x| x.1).unwrap_or("").to_string()).collect()
    }
}

impl FakeServer {
    pub fn start(security: MailSecurity) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(FakeState {
            folders: vec![],
            commands: vec![],
            condstore: true,
            sasl_ir: true,
            user: "ada".into(),
            password: "pässword \"quoted\"".into(),
            fetch_delay: Duration::ZERO,
            connections: 0,
            open_connections: 0,
            max_open_connections: 0,
            pad_message: None,
            refuse_examine: vec![],
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        let join = std::thread::spawn(move || {
            let cfg = server_config();
            while !sp.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((s, _)) => {
                        let (st, cfg) = (st.clone(), cfg.clone());
                        std::thread::spawn(move || {
                            let _ = s.set_nonblocking(false);
                            let _ = s.set_read_timeout(Some(Duration::from_secs(20)));
                            {
                                let mut g = st.lock().unwrap();
                                g.connections += 1;
                                g.open_connections += 1;
                                g.max_open_connections = g.max_open_connections.max(g.open_connections);
                            }
                            let _ = connection(s, &st, cfg, security);
                            st.lock().unwrap().open_connections -= 1;
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(3)),
                }
            }
        });
        FakeServer { port, security, state, stop, join: Some(join) }
    }

    pub fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap()
    }

    pub fn settings(&self) -> ConnectSettings {
        ConnectSettings { host: "localhost".into(), port: self.port, security: self.security, limits: Limits { io_timeout: Duration::from_secs(15), ..Limits::default() } }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

enum Next {
    Done,
    Upgrade,
}

fn connection(tcp: TcpStream, st: &Arc<Mutex<FakeState>>, cfg: Arc<ServerConfig>, security: MailSecurity) -> std::io::Result<()> {
    match security {
        MailSecurity::ImplicitTls => {
            let tls = StreamOwned::new(ServerConnection::new(cfg).unwrap(), tcp);
            let mut r = BufReader::new(tls);
            r.get_mut().write_all(b"* OK pretend IMAP ready\r\n")?;
            session(&mut r, st, true).map(|_| ())
        }
        MailSecurity::StartTls => {
            let mut r = BufReader::new(tcp);
            r.get_mut().write_all(b"* OK pretend IMAP ready\r\n")?;
            if let Next::Upgrade = session(&mut r, st, false)? {
                let tcp = r.into_inner();
                let tls = StreamOwned::new(ServerConnection::new(cfg).unwrap(), tcp);
                let mut r = BufReader::new(tls);
                session(&mut r, st, true)?;
            }
            Ok(())
        }
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    match s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        Some(inner) => {
            let mut out = String::new();
            let mut it = inner.chars();
            while let Some(c) = it.next() {
                if c == '\\' {
                    if let Some(n) = it.next() {
                        out.push(n);
                    }
                } else {
                    out.push(c);
                }
            }
            out
        }
        None => s.to_string(),
    }
}

/// UIDs in an IMAP set such as `1:3,7,9:*` given the UIDs that exist.
fn in_set(set: &str, uid: u32, max: u32) -> bool {
    set.split(',').any(|part| {
        let num = |x: &str| if x == "*" { max } else { x.parse().unwrap_or(0) };
        match part.split_once(':') {
            Some((a, b)) => {
                let (a, b) = (num(a), num(b));
                uid >= a.min(b) && uid <= a.max(b)
            }
            None => uid == num(part),
        }
    })
}

fn session<S: Read + Write>(r: &mut BufReader<S>, st: &Arc<Mutex<FakeState>>, tls: bool) -> std::io::Result<Next> {
    let mut selected: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(Next::Done);
        }
        let cmd_line = line.trim_end_matches(['\r', '\n']).to_string();
        let (tag, rest) = cmd_line.split_once(' ').unwrap_or((&cmd_line, ""));
        st.lock().unwrap().commands.push(cmd_line.clone());
        let upper = rest.to_ascii_uppercase();
        let mut words = upper.split_whitespace();
        let verb = words.next().unwrap_or("").to_string();
        let out = r.get_mut();
        match verb.as_str() {
            "CAPABILITY" => {
                let g = st.lock().unwrap();
                let mut caps = vec!["IMAP4rev1", "AUTH=PLAIN", "SPECIAL-USE"];
                if g.sasl_ir {
                    caps.push("SASL-IR");
                }
                if g.condstore {
                    caps.push("CONDSTORE");
                }
                if !tls {
                    caps.push("STARTTLS");
                    caps.push("LOGINDISABLED");
                }
                write!(out, "* CAPABILITY {}\r\n{tag} OK done\r\n", caps.join(" "))?;
            }
            "STARTTLS" if !tls => {
                write!(out, "{tag} OK begin TLS\r\n")?;
                out.flush()?;
                return Ok(Next::Upgrade);
            }
            "NOOP" => write!(out, "{tag} OK\r\n")?,
            "LOGOUT" => {
                write!(out, "* BYE bye\r\n{tag} OK logged out\r\n")?;
                return Ok(Next::Done);
            }
            "LOGIN" if tls => {
                let args = rest.split_once(' ').map(|x| x.1).unwrap_or("");
                let (u, p) = split_two_quoted(args);
                let g = st.lock().unwrap();
                if u == g.user && p == g.password {
                    write!(out, "{tag} OK logged in\r\n")?;
                } else {
                    write!(out, "{tag} NO [AUTHENTICATIONFAILED] wrong user or password\r\n")?;
                }
            }
            "AUTHENTICATE" if tls => {
                let b64 = match rest.split_whitespace().nth(2) {
                    Some(x) => x.to_string(),
                    None => {
                        write!(out, "+ \r\n")?;
                        out.flush()?;
                        let mut l = String::new();
                        r.read_line(&mut l)?;
                        l.trim().to_string()
                    }
                };
                let out = r.get_mut();
                let data = crate::mail::base64_decode(b64.as_bytes());
                let parts: Vec<&[u8]> = data.split(|&b| b == 0).collect();
                let g = st.lock().unwrap();
                if parts.len() == 3 && parts[1] == g.user.as_bytes() && parts[2] == g.password.as_bytes() {
                    write!(out, "{tag} OK authenticated\r\n")?;
                } else {
                    write!(out, "{tag} NO [AUTHENTICATIONFAILED] wrong user or password\r\n")?;
                }
            }
            "LIST" => {
                let g = st.lock().unwrap();
                for f in &g.folders {
                    write!(out, "* LIST (\\HasNoChildren{}) \"/\" \"{}\"\r\n", f.attrs.iter().map(|a| format!(" {a}")).collect::<String>(), f.name)?;
                }
                write!(out, "{tag} OK list done\r\n")?;
            }
            "EXAMINE" => {
                let name = unquote(rest.trim_start_matches(|c: char| !c.is_whitespace()).trim().split(" (").next().unwrap_or(""));
                let g = st.lock().unwrap();
                match g.folders.iter().find(|f| f.name == name && !g.refuse_examine.contains(&name)) {
                    Some(f) => {
                        selected = Some(name);
                        write!(out, "* {} EXISTS\r\n* OK [UIDVALIDITY {}] ok\r\n* OK [UIDNEXT {}] ok\r\n", f.msgs.len(), f.uidvalidity, f.uidnext)?;
                        if g.condstore {
                            write!(out, "* OK [HIGHESTMODSEQ {}] ok\r\n", f.highestmodseq)?;
                        }
                        write!(out, "{tag} OK [READ-ONLY] examined\r\n")?;
                    }
                    None => write!(out, "{tag} NO no such folder\r\n")?,
                }
            }
            "UID" => {
                let sub = words.next().unwrap_or("");
                let g = st.lock().unwrap();
                let folder = selected.as_ref().and_then(|n| g.folders.iter().find(|f| &f.name == n));
                match (sub, folder) {
                    ("SEARCH", Some(f)) => {
                        let args: Vec<&str> = rest.split_whitespace().skip(2).collect();
                        let max = f.msgs.iter().map(|m| m.uid).max().unwrap_or(0);
                        let hits: Vec<u32> = match args.first().map(|a| a.to_ascii_uppercase()).as_deref() {
                            Some("ALL") => f.msgs.iter().map(|m| m.uid).collect(),
                            Some("SINCE") => {
                                let from = parse_search_date(args.get(1).unwrap_or(&"")).unwrap_or(0);
                                f.msgs.iter().filter(|m| m.internal_date >= from).map(|m| m.uid).collect()
                            }
                            Some("UID") => {
                                let from: u32 = args.get(1).and_then(|a| a.split(':').next()).and_then(|a| a.parse().ok()).unwrap_or(1);
                                let mut v: Vec<u32> = f.msgs.iter().filter(|m| m.uid >= from).map(|m| m.uid).collect();
                                if v.is_empty() && max > 0 {
                                    v.push(max); // `n:*` always includes the last message
                                }
                                v
                            }
                            _ => vec![],
                        };
                        write!(out, "* SEARCH{}\r\n{tag} OK search done\r\n", hits.iter().map(|u| format!(" {u}")).collect::<String>())?;
                    }
                    ("FETCH", Some(f)) => {
                        let (delay, pad) = (g.fetch_delay, g.pad_message);
                        let condstore = g.condstore;
                        let args = rest.splitn(3, ' ').nth(2).unwrap_or("");
                        let set = args.split_whitespace().next().unwrap_or("");
                        let items = args[set.len()..].to_ascii_uppercase();
                        let since: Option<u64> = items.split("CHANGEDSINCE").nth(1).and_then(|x| x.trim().trim_end_matches(')').trim().parse().ok());
                        let max = f.msgs.iter().map(|m| m.uid).max().unwrap_or(0);
                        let msgs: Vec<FakeMessage> = f.msgs.iter().filter(|m| in_set(set, m.uid, max)).filter(|m| since.map(|s| m.modseq > s).unwrap_or(true)).cloned().collect();
                        let ids: Vec<u32> = f.msgs.iter().map(|m| m.uid).collect();
                        drop(g);
                        if !delay.is_zero() {
                            std::thread::sleep(delay);
                        }
                        for m in msgs {
                            let seq = ids.iter().position(|&u| u == m.uid).unwrap_or(0) + 1;
                            let mut resp = format!("* {seq} FETCH (UID {}", m.uid).into_bytes();
                            if items.contains("FLAGS") {
                                resp.extend_from_slice(format!(" FLAGS ({})", m.flags.join(" ")).as_bytes());
                            }
                            if items.contains("INTERNALDATE") {
                                resp.extend_from_slice(format!(" INTERNALDATE \"{}\"", internal_date_text(m.internal_date)).as_bytes());
                            }
                            if items.contains("RFC822.SIZE") {
                                resp.extend_from_slice(format!(" RFC822.SIZE {}", m.raw.len()).as_bytes());
                            }
                            if (since.is_some() || items.contains("MODSEQ")) && condstore {
                                resp.extend_from_slice(format!(" MODSEQ ({})", m.modseq).as_bytes());
                            }
                            let body: Option<(&str, Vec<u8>)> = if items.contains("BODY.PEEK[]") || items.contains("BODY[]") {
                                let mut raw = m.raw.clone();
                                if let Some((u, n)) = pad {
                                    if u == m.uid {
                                        raw.extend(std::iter::repeat_n(b'x', n));
                                    }
                                }
                                Some(("BODY[]", raw))
                            } else if items.contains("BODY.PEEK[HEADER]") {
                                let end = m.raw.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).unwrap_or(m.raw.len());
                                Some(("BODY[HEADER]", m.raw[..end].to_vec()))
                            } else {
                                None
                            };
                            if let Some((name, data)) = body {
                                resp.extend_from_slice(format!(" {name} {{{}}}\r\n", data.len()).as_bytes());
                                resp.extend_from_slice(&data);
                            }
                            resp.extend_from_slice(b")\r\n");
                            let out = r.get_mut();
                            out.write_all(&resp)?;
                        }
                        write!(r.get_mut(), "{tag} OK fetch done\r\n")?;
                    }
                    _ => write!(out, "{tag} NO nothing selected or not supported\r\n")?,
                }
            }
            // everything that could change a mailbox: the client must never get here
            "SELECT" | "STORE" | "EXPUNGE" | "COPY" | "MOVE" | "APPEND" | "DELETE" | "CREATE" | "RENAME" | "CLOSE" => write!(out, "{tag} NO the pretend server is read-only\r\n")?,
            _ => write!(out, "{tag} BAD unknown command\r\n")?,
        }
        r.get_mut().flush()?;
    }
}

/// `"user" "password"` (quoted, with `\` escapes) → the two strings.
fn split_two_quoted(s: &str) -> (String, String) {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            cur.push(c);
            esc = false;
        } else if c == '\\' && in_q {
            esc = true;
        } else if c == '"' {
            if in_q {
                parts.push(std::mem::take(&mut cur));
            }
            in_q = !in_q;
        } else if in_q {
            cur.push(c);
        }
    }
    (parts.first().cloned().unwrap_or_default(), parts.get(1).cloned().unwrap_or_default())
}

/// A small plain message for tests.
pub fn message(subject: &str, from: &str, body: &str) -> Vec<u8> {
    format!("From: {from}\r\nTo: ada@example.com\r\nSubject: {subject}\r\nDate: Thu, 13 Nov 2026 10:22:33 +0100\r\nMessage-ID: <{subject}@fake>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imap_client::*;

    fn account(server: &FakeServer) -> PlainAuth {
        let s = server.state();
        PlainAuth::new(&s.user, &s.password)
    }

    fn seeded(security: MailSecurity) -> FakeServer {
        let server = FakeServer::start(security);
        {
            let mut s = server.state();
            s.add_folder("INBOX", &[]);
            s.add_folder("Sent", &["\\Sent"]);
            s.add_folder("Gesendet &AOQ-", &[]);
            for i in 0..3 {
                s.add_message("INBOX", message(&format!("m{i}"), "Bob <bob@x.de>", "hello"), 1_700_000_000 + i * 86_400);
            }
        }
        server
    }

    fn connect(server: &FakeServer) -> Result<Client> {
        let mut auth = account(server);
        Client::connect_test(&server.settings(), server_cert(), &mut auth)
    }

    // spec: MAIL-020, MAIL-022
    #[test]
    fn implicit_tls_signs_in_lists_examines_searches_and_fetches() {
        let server = seeded(MailSecurity::ImplicitTls);
        let mut c = connect(&server).unwrap();
        assert!(c.has_capability("CONDSTORE") && c.has_capability("SPECIAL-USE"));
        let folders = c.list_folders().unwrap();
        assert_eq!(folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["INBOX", "Sent", "Gesendet &AOQ-"]);
        assert!(folders[1].has_attr("\\Sent"));
        assert_eq!(folders[2].display_name(), "Gesendet ä");
        let sel = c.examine("INBOX").unwrap();
        assert_eq!((sel.exists, sel.uidvalidity, sel.uidnext), (3, 1000, 4));
        assert_eq!(sel.highestmodseq, Some(4));
        assert_eq!(c.uid_search(SearchKey::All).unwrap(), vec![1, 2, 3]);
        assert_eq!(c.uid_search(SearchKey::UidFrom(3)).unwrap(), vec![3]);
        let mut got = vec![];
        c.uid_fetch_messages(&[1, 2], Part::Full, |f| {
            got.push(f);
            Ok(())
        })
        .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].uid, 1);
        assert_eq!(got[0].internal_date, Some(1_700_000_000));
        let Some(Body::Data(d)) = &got[0].body else { panic!("no body") };
        assert!(d.starts_with(b"From: Bob"));
        assert_eq!(got[0].size, Some(d.len() as u64));
        c.logout();
    }

    // spec: MAIL-020
    #[test]
    fn starttls_is_done_before_anything_secret_is_sent() {
        let server = seeded(MailSecurity::StartTls);
        let mut c = connect(&server).unwrap();
        assert_eq!(c.uid_search(SearchKey::All).unwrap_or_default().len(), 0, "nothing selected yet is an error, not a crash");
        let verbs = server.state().verbs();
        let tls_at = verbs.iter().position(|v| v == "STARTTLS").expect("STARTTLS was sent");
        let auth_at = verbs.iter().position(|v| v.starts_with("AUTHENTICATE")).expect("signed in");
        assert!(tls_at < auth_at);
        assert!(!verbs[..tls_at].iter().any(|v| v.starts_with("LOGIN") || v.starts_with("AUTHENTICATE")), "no credentials before TLS");
        assert!(c.list_folders().unwrap().len() == 3);
    }

    // spec: MAIL-020
    #[test]
    fn a_server_without_starttls_is_refused_not_used_in_plain_text() {
        // a plain-text server: starts a STARTTLS session but the client is told it must have TLS first
        let server = FakeServer::start(MailSecurity::StartTls);
        server.state().password = "x".into();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(b"* OK hello\r\n").unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut l = String::new();
            let mut seen = vec![];
            while r.read_line(&mut l).unwrap_or(0) > 0 {
                seen.push(l.trim().to_string());
                let tag = l.split(' ').next().unwrap().to_string();
                if l.contains("CAPABILITY") {
                    write!(s, "* CAPABILITY IMAP4rev1 AUTH=PLAIN\r\n{tag} OK\r\n").unwrap();
                } else {
                    write!(s, "{tag} BAD\r\n").unwrap();
                }
                l.clear();
            }
            seen
        });
        let mut settings = server.settings();
        settings.port = port;
        let mut auth = PlainAuth::new("ada", "secret");
        let err = Client::connect_test(&settings, server_cert(), &mut auth).err().expect("must fail");
        assert!(matches!(err, ImapError::Tls(_)), "{err}");
        let seen = t.join().unwrap();
        assert!(seen.iter().all(|l| !l.contains("LOGIN") && !l.contains("AUTHENTICATE") && !l.contains("secret")), "{seen:?}");
    }

    // spec: MAIL-022
    #[test]
    fn a_certificate_the_system_does_not_trust_is_refused() {
        let server = seeded(MailSecurity::ImplicitTls);
        let mut auth = account(&server);
        // the production path: only the operating system's trust store, which does not know our throw-away certificate
        let err = Client::connect(&server.settings(), &mut auth).err().expect("must fail");
        assert!(matches!(err, ImapError::Tls(_)), "{err}");
        assert!(server.state().verbs().iter().all(|v| !v.starts_with("LOGIN") && !v.starts_with("AUTHENTICATE")), "no password was sent");
    }

    // spec: MAIL-025
    #[test]
    fn signing_in_works_with_and_without_an_initial_response_and_a_wrong_password_is_an_auth_error() {
        let server = seeded(MailSecurity::ImplicitTls);
        server.state().sasl_ir = false;
        connect(&server).unwrap().logout();
        server.state().sasl_ir = true;
        connect(&server).unwrap().logout();
        let mut bad = PlainAuth::new("ada", "wrong");
        let err = Client::connect_test(&server.settings(), server_cert(), &mut bad).err().expect("must fail");
        assert!(matches!(err, ImapError::Auth(_)), "{err}");
        assert_eq!(format!("{bad:?}"), "PlainAuth(ada, ***)");
    }

    // spec: MAIL-023
    #[test]
    fn the_client_never_sends_anything_that_changes_the_mailbox() {
        let server = seeded(MailSecurity::ImplicitTls);
        let mut c = connect(&server).unwrap();
        c.list_folders().unwrap();
        c.examine("INBOX").unwrap();
        c.uid_search(SearchKey::Since(2020, 1, 1)).unwrap();
        c.uid_fetch_messages(&[1, 2, 3], Part::Full, |_| Ok(())).unwrap();
        c.uid_fetch_messages(&[1], Part::Headers, |_| Ok(())).unwrap();
        c.uid_fetch_flags(Some(1), |_| Ok(())).unwrap();
        c.logout();
        let verbs = server.state().verbs();
        let allowed = ["CAPABILITY", "AUTHENTICATE", "LOGIN", "LIST", "EXAMINE", "UID SEARCH", "UID FETCH", "LOGOUT"];
        for v in &verbs {
            assert!(allowed.iter().any(|a| v.to_ascii_uppercase().starts_with(a)), "unexpected command {v}");
            let up = v.to_ascii_uppercase();
            for banned in ["SELECT", "STORE", "EXPUNGE", "COPY", "MOVE", "APPEND", "DELETE", "CREATE", "RENAME", "CLOSE"] {
                assert!(!up.split_whitespace().any(|w| w == banned), "{v}");
            }
            let stripped = up.replace("BODY.PEEK[", "").replace("RFC822.SIZE", "");
            assert!(!stripped.contains("BODY[") && !stripped.contains("RFC822"), "a fetch that marks mail as read: {v}");
        }
        assert!(verbs.iter().any(|v| v.starts_with("EXAMINE")));
    }

    // spec: MAIL-023
    #[test]
    fn the_guard_refuses_what_could_change_a_mailbox() {
        for ok in ["CAPABILITY", "LIST \"\" \"*\"", "EXAMINE \"INBOX\"", "UID SEARCH ALL", "UID FETCH 1:* (UID FLAGS)", "UID FETCH 5 (UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])", "uid fetch 5 (BODY.PEEK[HEADER])", "LOGOUT"] {
            assert!(check_read_only(ok).is_ok(), "{ok}");
        }
        for bad in [
            "SELECT \"INBOX\"",
            "UID STORE 1 +FLAGS (\\Deleted)",
            "STORE 1 FLAGS ()",
            "EXPUNGE",
            "UID EXPUNGE 1",
            "COPY 1 Trash",
            "UID COPY 1 Trash",
            "UID MOVE 1 Trash",
            "MOVE 1 Trash",
            "APPEND INBOX {10}",
            "DELETE Foo",
            "CREATE Foo",
            "RENAME a b",
            "CLOSE",
            "UNSELECT",
            "IDLE",
            "UID FETCH 1 (BODY[])",
            "UID FETCH 1 (BODY[HEADER])",
            "UID FETCH 1 (RFC822)",
            "UID FETCH 1 (RFC822.TEXT)",
            "UID FETCH 1 (RFC822.HEADER)",
            "UID FETCH 1 (BINARY[])",
            "UID FETCH 1 (BODY.PEEK[] BODY[])",
            "UID",
            "",
            "NOOP\r\nSTORE 1 FLAGS ()",
            "UID SEARCH ALL\nDELETE x",
            "XYZZY",
        ] {
            assert!(matches!(check_read_only(bad), Err(ImapError::Refused(_))), "{bad:?} must be refused");
        }
    }

    // spec: MAIL-024
    #[test]
    fn a_message_over_the_size_limit_is_skipped_without_being_read_and_the_connection_goes_on() {
        let server = seeded(MailSecurity::ImplicitTls);
        server.state().pad_message = Some((2, 3_000_000));
        let mut settings = server.settings();
        settings.limits.max_message = 1_000_000;
        let mut auth = account(&server);
        let mut c = Client::connect_test(&settings, server_cert(), &mut auth).unwrap();
        c.examine("INBOX").unwrap();
        let mut got = vec![];
        c.uid_fetch_messages(&[1, 2, 3], Part::Full, |f| {
            got.push(f);
            Ok(())
        })
        .unwrap();
        assert_eq!(got.len(), 3);
        assert!(matches!(got[0].body, Some(Body::Data(_))));
        assert!(matches!(got[1].body, Some(Body::TooLarge(n)) if n > 3_000_000));
        assert!(matches!(got[2].body, Some(Body::Data(_))), "the answers after it were still understood");
        assert_eq!(c.uid_search(SearchKey::All).unwrap().len(), 3, "and the connection is in sync");
    }

    // spec: MAIL-024
    #[test]
    fn a_silent_server_is_given_up_on_after_the_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let _t = std::thread::spawn(move || {
            let (_s, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(3));
        });
        let mut settings = ConnectSettings { host: "localhost".into(), port, security: MailSecurity::ImplicitTls, limits: Limits::default() };
        settings.limits.io_timeout = Duration::from_millis(300);
        let mut auth = PlainAuth::new("a", "b");
        let started = std::time::Instant::now();
        let err = Client::connect_test(&settings, server_cert(), &mut auth).err().expect("must fail");
        assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
        assert!(matches!(err, ImapError::Connection(_) | ImapError::Tls(_)), "{err}");
    }
}
