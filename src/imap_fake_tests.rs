//! A pretend IMAP server for tests: real TCP and real TLS (a throw-away certificate), enough of
//! IMAP4rev1 for the sync (`CAPABILITY`, `STARTTLS`, `LOGIN`, `AUTHENTICATE PLAIN`, `LIST`,
//! `EXAMINE`, `UID SEARCH`, `UID FETCH`, optionally `CONDSTORE`) and, for phase 2, the writes:
//! `SELECT`, `UID STORE` (with `UNCHANGEDSINCE`), `UID COPY` (`COPYUID`), `UID MOVE`, `UID EXPUNGE`,
//! with switches for a server without `MOVE` or without `UIDPLUS`. It records every command it
//! receives, so tests can check that the client never sends anything that changes a mailbox
//! where it must not. A plain `EXPUNGE` is answered `NO` (and shows in the log: it must never come).
//! `APPEND` (phase 3, the Sent copy) stores the message and answers with `APPENDUID`. Other things it
//! should never see (`DELETE` …) are answered `NO`.

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
    /// OAuth2 sign-in: the access tokens that are good. None: the server has no OAuth2.
    pub valid_tokens: Option<Arc<Mutex<std::collections::HashSet<String>>>>,
    /// The OAuth2 mechanisms offered (`XOAUTH2`, `OAUTHBEARER`).
    pub oauth_mechs: Vec<&'static str>,
    /// What the client answered to an OAuth2 error challenge (decoded), per refused sign-in.
    pub oauth_error_answers: Vec<Vec<u8>>,
    /// The mechanism of every `AUTHENTICATE` received.
    pub auth_mechs: Vec<String>,
    /// Offer `MOVE` (RFC 6851).
    pub move_cap: bool,
    /// Offer `UIDPLUS` (`UID EXPUNGE`, `COPYUID`).
    pub uidplus: bool,
    /// The next write command (STORE, COPY, MOVE, EXPUNGE) is answered `NO` with this text.
    pub fail_next_write: Option<String>,
    /// Every write command with this verb (`COPY`, `STORE`, `EXPUNGE`, `MOVE`) is answered `NO`, until it is reset.
    pub fail_every: Option<&'static str>,
    /// The connection is closed, without an answer and without doing anything, when a command with
    /// this verb (`UID MOVE`, `UID EXPUNGE` …) arrives.
    pub hang_up_on: Option<String>,
    /// After a `UID COPY` or `UID MOVE` the server forgets to say `COPYUID` (a server without UIDPLUS does this anyway).
    pub no_copyuid: bool,
    /// Every `APPEND` that was accepted: (folder, bytes).
    pub appended: Vec<(String, Vec<u8>)>,
    /// After storing an appended message the connection is closed without the answer (the message *is* there).
    pub hang_up_after_append: bool,
    /// The server does not say `APPENDUID` (also the case without `UIDPLUS`).
    pub no_appenduid: bool,
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

pub(crate) fn server_config() -> Arc<ServerConfig> {
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

    /// Another client moves a message: it leaves `from` and arrives in `to` with a new UID. Returns the new UID.
    pub fn move_message(&mut self, from: &str, to: &str, uid: u32) -> Option<u32> {
        let f = self.folder_mut(from);
        let pos = f.msgs.iter().position(|m| m.uid == uid)?;
        let m = f.msgs.remove(pos);
        f.highestmodseq += 1;
        let (flags, date, raw) = (m.flags, m.internal_date, m.raw);
        let new = self.add_message(to, raw, date);
        self.set_flags(to, new, &flags.iter().map(String::as_str).collect::<Vec<_>>());
        Some(new)
    }

    /// The messages of a folder: (uid, flags, raw).
    pub fn messages(&self, folder: &str) -> Vec<(u32, Vec<String>, Vec<u8>)> {
        self.folders.iter().find(|f| f.name == folder).map(|f| f.msgs.iter().map(|m| (m.uid, m.flags.clone(), m.raw.clone())).collect()).unwrap_or_default()
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
            valid_tokens: None,
            oauth_mechs: vec!["XOAUTH2", "OAUTHBEARER"],
            oauth_error_answers: vec![],
            auth_mechs: vec![],
            move_cap: true,
            uidplus: true,
            fail_next_write: None,
            fail_every: None,
            hang_up_on: None,
            no_copyuid: false,
            appended: vec![],
            hang_up_after_append: false,
            no_appenduid: false,
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
    let mut rw = false;
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
        {
            let sub = if verb == "UID" { words.clone().next().unwrap_or("") } else { "" };
            let full = if sub.is_empty() { verb.clone() } else { format!("{verb} {sub}") };
            let mut g = st.lock().unwrap();
            if g.hang_up_on.as_deref() == Some(full.as_str()) {
                g.hang_up_on = None;
                return Ok(Next::Done);
            }
        }
        let out = r.get_mut();
        match verb.as_str() {
            "CAPABILITY" => {
                let g = st.lock().unwrap();
                let mut caps = vec!["IMAP4rev1", "AUTH=PLAIN", "SPECIAL-USE"];
                if g.sasl_ir {
                    caps.push("SASL-IR");
                }
                let oauth: Vec<String> = if g.valid_tokens.is_some() { g.oauth_mechs.iter().map(|m| format!("AUTH={m}")).collect() } else { vec![] };
                caps.extend(oauth.iter().map(String::as_str));
                if g.condstore {
                    caps.push("CONDSTORE");
                }
                if g.move_cap {
                    caps.push("MOVE");
                }
                if g.uidplus {
                    caps.push("UIDPLUS");
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
                let mech = rest.split_whitespace().nth(1).unwrap_or("").to_ascii_uppercase();
                st.lock().unwrap().auth_mechs.push(mech.clone());
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
                if mech == "XOAUTH2" || mech == "OAUTHBEARER" {
                    // the token is the text after "auth=Bearer " up to the next ^A
                    let text = String::from_utf8_lossy(&data).into_owned();
                    let token = text.split("auth=Bearer ").nth(1).and_then(|t| t.split('\x01').next()).unwrap_or("").to_string();
                    let user_ok = if mech == "XOAUTH2" { text.starts_with(&format!("user={}\x01", st.lock().unwrap().user)) } else { text.starts_with(&format!("n,a={},\x01", st.lock().unwrap().user)) };
                    let good = {
                        let g = st.lock().unwrap();
                        user_ok && g.oauth_mechs.contains(&mech.as_str()) && g.valid_tokens.as_ref().is_some_and(|v| v.lock().unwrap().contains(&token))
                    };
                    if good {
                        write!(out, "{tag} OK authenticated\r\n")?;
                    } else {
                        // the specifications: the reason as a challenge, the client answers, then NO
                        write!(out, "+ {}\r\n", crate::imap_client::base64_encode(b"{\"status\":\"401\",\"schemes\":\"Bearer\",\"scope\":\"mail\"}"))?;
                        out.flush()?;
                        let mut l = String::new();
                        r.read_line(&mut l)?;
                        st.lock().unwrap().oauth_error_answers.push(crate::mail::base64_decode(l.trim().as_bytes()));
                        let out = r.get_mut();
                        write!(out, "{tag} NO [AUTHENTICATIONFAILED] invalid token\r\n")?;
                    }
                    continue;
                }
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
            "EXAMINE" | "SELECT" => {
                let name = unquote(rest.trim_start_matches(|c: char| !c.is_whitespace()).trim().split(" (").next().unwrap_or(""));
                let g = st.lock().unwrap();
                match g.folders.iter().find(|f| f.name == name && !g.refuse_examine.contains(&name)) {
                    Some(f) => {
                        selected = Some(name);
                        rw = verb == "SELECT";
                        write!(out, "* {} EXISTS\r\n* OK [UIDVALIDITY {}] ok\r\n* OK [UIDNEXT {}] ok\r\n", f.msgs.len(), f.uidvalidity, f.uidnext)?;
                        if g.condstore {
                            write!(out, "* OK [HIGHESTMODSEQ {}] ok\r\n", f.highestmodseq)?;
                        }
                        write!(out, "{tag} OK [{}] done\r\n", if rw { "READ-WRITE" } else { "READ-ONLY" })?;
                    }
                    None => write!(out, "{tag} NO no such folder\r\n")?,
                }
            }
            "UID" if matches!(words.clone().next(), Some("STORE" | "COPY" | "MOVE" | "EXPUNGE")) => {
                let sub = words.next().unwrap_or("").to_string();
                write_command(r.get_mut(), st, &selected, rw, tag, rest, &sub)?;
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
                                let set = args.get(1).copied().unwrap_or("");
                                let mut v: Vec<u32> = f.msgs.iter().filter(|m| in_set(set, m.uid, max)).map(|m| m.uid).collect();
                                if v.is_empty() && max > 0 && set.ends_with(":*") {
                                    v.push(max); // `n:*` always includes the last message
                                }
                                v
                            }
                            Some("HEADER") => {
                                let want = unquote(&args[2.min(args.len())..].join(" ")).to_ascii_lowercase();
                                f.msgs.iter().filter(|m| String::from_utf8_lossy(&m.raw).to_ascii_lowercase().lines().any(|l| l.starts_with("message-id:") && l.contains(&want))).map(|m| m.uid).collect()
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
            "APPEND" => {
                // APPEND "folder" [(flags)] {n}: answer `+`, take the n bytes, store, answer with APPENDUID
                let args = rest.split_once(' ').map(|x| x.1).unwrap_or("");
                let name = unquote(args.split(" (").next().unwrap_or("").split(" {").next().unwrap_or("").trim());
                let flags: Vec<String> = args.split_once('(').and_then(|x| x.1.split_once(')')).map(|x| x.0.split_whitespace().map(String::from).collect()).unwrap_or_default();
                let n: usize = args.rsplit_once('{').and_then(|x| x.1.trim_end_matches('}').trim_end_matches('+').parse().ok()).unwrap_or(0);
                {
                    let mut g = st.lock().unwrap();
                    if let Some(why) = g.fail_next_write.take() {
                        write!(r.get_mut(), "{tag} NO {why}\r\n")?;
                        continue;
                    }
                    if !g.folders.iter().any(|f| f.name == name) {
                        write!(r.get_mut(), "{tag} NO [TRYCREATE] no such folder\r\n")?;
                        continue;
                    }
                }
                write!(r.get_mut(), "+ Ready for literal data\r\n")?;
                r.get_mut().flush()?;
                let mut buf = vec![0u8; n];
                r.read_exact(&mut buf)?;
                let mut rest_of_line = String::new();
                r.read_line(&mut rest_of_line)?;
                let (answer, hang) = {
                    let mut g = st.lock().unwrap();
                    let uv = g.folders.iter().find(|f| f.name == name).map(|f| f.uidvalidity).unwrap_or(0);
                    let uid = g.add_message(&name, buf.clone(), 1_794_657_600);
                    if !flags.is_empty() {
                        let fl: Vec<&str> = flags.iter().map(String::as_str).collect();
                        g.set_flags(&name, uid, &fl);
                    }
                    g.appended.push((name.clone(), buf));
                    (if g.uidplus && !g.no_appenduid { format!("{tag} OK [APPENDUID {uv} {uid}] append done\r\n") } else { format!("{tag} OK append done\r\n") }, g.hang_up_after_append)
                };
                if hang {
                    return Ok(Next::Done);
                }
                write!(r.get_mut(), "{answer}")?;
            }
            "STORE" | "EXPUNGE" | "COPY" | "MOVE" | "DELETE" | "CREATE" | "RENAME" | "CLOSE" => write!(out, "{tag} NO the pretend server is read-only\r\n")?,
            _ => write!(out, "{tag} BAD unknown command\r\n")?,
        }
        r.get_mut().flush()?;
    }
}

/// `UID STORE`, `UID COPY`, `UID MOVE` and `UID EXPUNGE`.
fn write_command<W: Write>(out: &mut W, st: &Arc<Mutex<FakeState>>, selected: &Option<String>, rw: bool, tag: &str, rest: &str, sub: &str) -> std::io::Result<()> {
    let mut g = st.lock().unwrap();
    if let Some(why) = g.fail_next_write.take() {
        return write!(out, "{tag} NO {why}\r\n");
    }
    if g.fail_every == Some(sub) {
        return write!(out, "{tag} NO [SERVERBUG] the pretend server fails every {sub}\r\n");
    }
    let Some(name) = selected.clone() else { return write!(out, "{tag} NO no folder selected\r\n") };
    if !rw {
        return write!(out, "{tag} NO [READONLY] the folder was opened read-only\r\n");
    }
    if sub == "MOVE" && !g.move_cap || sub == "EXPUNGE" && !g.uidplus {
        return write!(out, "{tag} BAD unknown command\r\n");
    }
    let (condstore, uidplus, no_copyuid) = (g.condstore, g.uidplus, g.no_copyuid);
    let args = rest.splitn(3, ' ').nth(2).unwrap_or("");
    let set = args.split_whitespace().next().unwrap_or("").to_string();
    let Some(fi) = g.folders.iter().position(|f| f.name == name) else { return write!(out, "{tag} NO no such folder\r\n") };
    let max = g.folders[fi].msgs.iter().map(|m| m.uid).max().unwrap_or(0);
    match sub {
        "STORE" => {
            let tail = args[set.len()..].trim();
            let (unchanged, tail) = match tail.strip_prefix("(UNCHANGEDSINCE ") {
                Some(t) => {
                    let (n, after) = t.split_once(')').unwrap_or(("0", ""));
                    (n.trim().parse::<u64>().ok(), after.trim())
                }
                None => (None, tail),
            };
            let add = tail.starts_with('+');
            let list: Vec<String> = tail.split_once('(').map(|x| x.1.trim_end_matches(')').split_whitespace().map(String::from).collect()).unwrap_or_default();
            let mut modified = vec![];
            let mut resp = String::new();
            let f = &mut g.folders[fi];
            let ids: Vec<u32> = f.msgs.iter().map(|m| m.uid).collect();
            let mut bump = 0;
            let mut changed: Vec<(usize, u32)> = vec![];
            for (i, m) in f.msgs.iter_mut().enumerate() {
                if !in_set(&set, m.uid, max) {
                    continue;
                }
                if unchanged.is_some_and(|u| m.modseq > u) {
                    modified.push(m.uid);
                    continue;
                }
                for fl in &list {
                    let has = m.flags.iter().any(|x| x.eq_ignore_ascii_case(fl));
                    if add && !has {
                        m.flags.push(fl.clone());
                    } else if !add && has {
                        m.flags.retain(|x| !x.eq_ignore_ascii_case(fl));
                    }
                }
                bump += 1;
                changed.push((i, m.uid));
            }
            let base = f.highestmodseq;
            for (n, (i, uid)) in changed.iter().enumerate() {
                let ms = base + 1 + n as u64;
                f.msgs[*i].modseq = ms;
                let seq = ids.iter().position(|u| u == uid).unwrap_or(0) + 1;
                resp.push_str(&format!("* {seq} FETCH (UID {uid} FLAGS ({}){})\r\n", f.msgs[*i].flags.join(" "), if condstore { format!(" MODSEQ ({ms})") } else { String::new() }));
            }
            f.highestmodseq += bump;
            write!(out, "{resp}")?;
            if modified.is_empty() {
                write!(out, "{tag} OK stored\r\n")
            } else {
                write!(out, "{tag} OK [MODIFIED {}] conditional store failed\r\n", modified.iter().map(u32::to_string).collect::<Vec<_>>().join(","))
            }
        }
        "COPY" | "MOVE" => {
            let dest = unquote(args[set.len()..].trim());
            let Some(di) = g.folders.iter().position(|f| f.name == dest) else { return write!(out, "{tag} NO [TRYCREATE] no such folder\r\n") };
            if di == fi {
                return write!(out, "{tag} NO cannot copy into the same folder\r\n");
            }
            let picked: Vec<FakeMessage> = g.folders[fi].msgs.iter().filter(|m| in_set(&set, m.uid, max)).cloned().collect();
            let (mut from, mut to) = (vec![], vec![]);
            for m in &picked {
                let d = &mut g.folders[di];
                let uid = d.uidnext;
                d.uidnext += 1;
                d.highestmodseq += 1;
                let ms = d.highestmodseq;
                d.msgs.push(FakeMessage { uid, flags: m.flags.clone(), internal_date: m.internal_date, raw: m.raw.clone(), modseq: ms });
                from.push(m.uid);
                to.push(uid);
            }
            let uv = g.folders[di].uidvalidity;
            let codes = if uidplus && !no_copyuid && !from.is_empty() { Some(format!("[COPYUID {uv} {} {}]", crate::imap_client::uid_set(&from), crate::imap_client::uid_set(&to))) } else { None };
            if sub == "MOVE" {
                let before: Vec<u32> = g.folders[fi].msgs.iter().map(|m| m.uid).collect();
                g.folders[fi].msgs.retain(|m| !from.contains(&m.uid));
                g.folders[fi].highestmodseq += 1;
                if let Some(c) = &codes {
                    write!(out, "* OK {c} moved\r\n")?;
                }
                for (seq, u) in before.iter().enumerate().rev() {
                    if from.contains(u) {
                        write!(out, "* {} EXPUNGE\r\n", seq + 1)?;
                    }
                }
                write!(out, "{tag} OK moved\r\n")
            } else {
                match codes {
                    Some(c) => write!(out, "{tag} OK {c} copied\r\n"),
                    None => write!(out, "{tag} OK copied\r\n"),
                }
            }
        }
        "EXPUNGE" => {
            let f = &mut g.folders[fi];
            let before: Vec<(u32, bool)> = f.msgs.iter().map(|m| (m.uid, in_set(&set, m.uid, max) && m.flags.iter().any(|x| x.eq_ignore_ascii_case("\\Deleted")))).collect();
            f.msgs.retain(|m| !(in_set(&set, m.uid, max) && m.flags.iter().any(|x| x.eq_ignore_ascii_case("\\Deleted"))));
            f.highestmodseq += 1;
            for (seq, (_, gone)) in before.iter().enumerate().rev() {
                if *gone {
                    write!(out, "* {} EXPUNGE\r\n", seq + 1)?;
                }
            }
            write!(out, "{tag} OK expunged\r\n")
        }
        _ => write!(out, "{tag} BAD unknown command\r\n"),
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

    fn oauth_server(mechs: &[&'static str]) -> (FakeServer, Arc<Mutex<std::collections::HashSet<String>>>) {
        let server = seeded(MailSecurity::ImplicitTls);
        let tokens = Arc::new(Mutex::new(std::collections::HashSet::new()));
        {
            let mut s = server.state();
            s.valid_tokens = Some(tokens.clone());
            s.oauth_mechs = mechs.to_vec();
        }
        (server, tokens)
    }

    fn oauth_connect(server: &FakeServer, token: &str) -> Result<Client> {
        let mut auth = crate::oauth::OAuthAuth::new("ada", token, "localhost", server.port);
        Client::connect_test(&server.settings(), server_cert(), &mut auth)
    }

    // spec: MAIL-084
    #[test]
    fn oauth_signs_in_with_xoauth2_or_oauthbearer_whichever_the_server_offers_and_prefers_oauthbearer() {
        let (server, tokens) = oauth_server(&["XOAUTH2"]);
        tokens.lock().unwrap().insert("good-token".into());
        oauth_connect(&server, "good-token").unwrap().logout();
        assert_eq!(server.state().auth_mechs, vec!["XOAUTH2"]);
        server.state().oauth_mechs = vec!["XOAUTH2", "OAUTHBEARER"];
        oauth_connect(&server, "good-token").unwrap().logout();
        assert_eq!(server.state().auth_mechs, vec!["XOAUTH2", "OAUTHBEARER"], "OAUTHBEARER is preferred when offered");
        server.state().oauth_mechs = vec!["OAUTHBEARER"];
        oauth_connect(&server, "good-token").unwrap().logout();
        // a server with only passwords is not asked for a token
        let plain = seeded(MailSecurity::ImplicitTls);
        let err = oauth_connect(&plain, "good-token").err().expect("must fail");
        assert!(matches!(&err, ImapError::Auth(t) if t.contains("does not offer XOAUTH2")), "{err}");
        assert!(plain.state().auth_mechs.is_empty());
    }

    // spec: MAIL-084
    #[test]
    fn a_refused_token_gets_the_specified_answer_to_the_error_challenge_and_the_servers_reason() {
        let (server, tokens) = oauth_server(&["XOAUTH2"]);
        tokens.lock().unwrap().insert("good-token".into());
        let err = oauth_connect(&server, "expired-token").err().expect("must fail");
        let ImapError::Auth(text) = &err else { panic!("{err}") };
        assert!(text.contains("invalid token") && text.contains("the server said") && text.contains("401"), "{text}");
        assert!(!text.contains("expired-token"), "the token is not repeated in errors");
        assert_eq!(server.state().oauth_error_answers, vec![Vec::<u8>::new()], "XOAUTH2: an empty line");
        server.state().oauth_mechs = vec!["OAUTHBEARER", "XOAUTH2"];
        let _ = oauth_connect(&server, "expired-token");
        assert_eq!(server.state().oauth_error_answers.last().unwrap(), &vec![1u8], "OAUTHBEARER: a single ^A");
        // the connection was not left half open: the next sign-in works
        oauth_connect(&server, "good-token").unwrap().logout();
    }

    // spec: MAIL-083
    #[test]
    fn a_server_that_refuses_the_token_gets_one_retry_with_a_new_one_and_a_revoked_login_says_so() {
        use crate::imap_sync::{connect_with_retry, AuthFactory};
        use crate::oauth::{OAuthConfig, OAuthFactory, TokenSource};
        use crate::oauth_fake_tests::FakeOAuth;
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let provider = FakeOAuth::start();
        let (server, _) = oauth_server(&["XOAUTH2"]);
        server.state().valid_tokens = Some(provider.valid_access());
        let acct = crate::config::MailAccount {
            id: "t-retry".into(),
            user: "ada".into(),
            oauth_provider: crate::config::OAuthProvider::Custom,
            oauth_client_id: "shale-test".into(),
            oauth_auth_url: format!("http://127.0.0.1:{}/authorize", provider.port),
            oauth_token_url: format!("http://127.0.0.1:{}/token", provider.port),
            oauth_scope: "mail".into(),
            ..Default::default()
        };
        struct Mem(Mutex<Option<String>>);
        impl crate::oauth::RefreshStore for Mem {
            fn load(&self) -> Option<String> {
                self.0.lock().unwrap().clone()
            }
            fn save(&self, t: &str) -> bool {
                *self.0.lock().unwrap() = Some(t.into());
                true
            }
            fn clear(&self) {
                *self.0.lock().unwrap() = None;
            }
        }
        let rt = provider.new_refresh_token();
        let source = Arc::new(TokenSource::new(OAuthConfig::for_account(&acct, None).unwrap(), Arc::new(Mem(Mutex::new(Some(rt))))));
        let factory = OAuthFactory { user: "ada".into(), host: "localhost".into(), port: server.port, source: source.clone() };
        let go = |f: &dyn AuthFactory| connect_with_retry(f, |a| Client::connect_test(&server.settings(), server_cert(), a));
        go(&factory).unwrap().logout();
        assert_eq!(source.refresh_count(), 1);
        // the server forgets every token (it expired early): refused once, then a new one works
        provider.expire_access_tokens();
        let before = server.state().auth_mechs.len();
        go(&factory).unwrap().logout();
        assert_eq!(server.state().auth_mechs.len() - before, 2, "one refused, one accepted");
        assert_eq!(source.refresh_count(), 2);
        // a provider that keeps refusing is not asked more than once more
        provider.state.lock().unwrap().revoked = true;
        provider.expire_access_tokens();
        let err = go(&factory).err().expect("must fail");
        assert!(matches!(&err, ImapError::Auth(t) if crate::oauth::needs_sign_in(t)), "{err}");
        // a password factory is never retried
        struct Pw;
        impl AuthFactory for Pw {
            fn authenticator(&self) -> std::result::Result<Box<dyn Authenticator>, String> {
                Ok(Box::new(PlainAuth::new("ada", "wrong")))
            }
        }
        let n = server.state().connections;
        assert!(matches!(go(&Pw), Err(ImapError::Auth(_))));
        assert_eq!(server.state().connections - n, 1);
    }

    // ------------------------------------------------------------ phase 2: the write gate

    fn seeded_for_writes(security: MailSecurity) -> FakeServer {
        let server = FakeServer::start(security);
        {
            let mut s = server.state();
            s.add_folder("INBOX", &[]);
            s.add_folder("Trash", &["\\Trash"]);
            for i in 0..4 {
                s.add_message("INBOX", message(&format!("w{i}"), "Bob <bob@x.de>", "hello"), 1_700_000_000 + i * 86_400);
            }
        }
        server
    }

    fn written(server: &FakeServer) -> Vec<String> {
        server.state().verbs().into_iter().filter(|v| ["SELECT", "UID STORE", "UID MOVE", "UID COPY", "UID EXPUNGE", "EXPUNGE", "STORE"].iter().any(|w| v.starts_with(w))).collect()
    }

    // spec: MAIL-101
    #[test]
    fn typed_commands_become_exact_text_and_the_write_gate_takes_only_those_shapes() {
        let cmds = [
            (WriteCmd::Select { folder: "INBOX".into() }, "SELECT \"INBOX\""),
            (WriteCmd::StoreFlags { uids: vec![3, 1, 2, 7], add: true, flags: vec![MailFlag::Seen], unchanged_since: None }, "UID STORE 1:3,7 +FLAGS (\\Seen)"),
            (WriteCmd::StoreFlags { uids: vec![5], add: false, flags: vec![MailFlag::Seen, MailFlag::Flagged], unchanged_since: Some(42) }, "UID STORE 5 (UNCHANGEDSINCE 42) -FLAGS (\\Seen \\Flagged)"),
            (WriteCmd::Move { uids: vec![9], to: "Tra\"sh\\x".into() }, "UID MOVE 9 \"Tra\\\"sh\\\\x\""),
            (WriteCmd::Copy { uids: vec![9, 10], to: "Trash".into() }, "UID COPY 9:10 \"Trash\""),
            (WriteCmd::MarkDeleted { uids: vec![4], set: true }, "UID STORE 4 +FLAGS (\\Deleted)"),
            (WriteCmd::MarkDeleted { uids: vec![4], set: false }, "UID STORE 4 -FLAGS (\\Deleted)"),
            (WriteCmd::ExpungeUids { uids: vec![4, 5] }, "UID EXPUNGE 4:5"),
            (WriteCmd::Append { folder: "Sent".into(), flags: vec![MailFlag::Seen], bytes: b"Subject: x\r\n\r\nhi\r\n".to_vec() }, "APPEND \"Sent\" (\\Seen) {18}"),
            (WriteCmd::Append { folder: "Sent Items".into(), flags: vec![], bytes: vec![b'x'; 7] }, "APPEND \"Sent Items\" {7}"),
            (WriteCmd::Append { folder: "a\"b".into(), flags: vec![MailFlag::Seen, MailFlag::Flagged], bytes: vec![b'x'; 1] }, "APPEND \"a\\\"b\" (\\Seen \\Flagged) {1}"),
        ];
        for (c, text) in cmds {
            let built = c.to_command().unwrap();
            assert_eq!(built, text);
            check_write(&built).unwrap_or_else(|e| panic!("{built}: {e}"));
        }
        // the typed constructors refuse nonsense before anything is built
        for bad in [
            WriteCmd::ExpungeUids { uids: vec![] },
            WriteCmd::ExpungeUids { uids: vec![0] },
            WriteCmd::Move { uids: vec![1], to: String::new() },
            WriteCmd::Select { folder: "a\r\nb".into() },
            WriteCmd::StoreFlags { uids: vec![1], add: true, flags: vec![], unchanged_since: None },
            WriteCmd::StoreFlags { uids: vec![], add: true, flags: vec![MailFlag::Seen], unchanged_since: None },
            WriteCmd::Append { folder: "Sent".into(), flags: vec![], bytes: vec![] },
            WriteCmd::Append { folder: "Sent".into(), flags: vec![], bytes: b"a\0b".to_vec() },
            WriteCmd::Append { folder: String::new(), flags: vec![], bytes: b"x".to_vec() },
            WriteCmd::Append { folder: "a\nb".into(), flags: vec![], bytes: b"x".to_vec() },
            WriteCmd::Append { folder: "Sent".into(), flags: vec![], bytes: vec![b'x'; crate::imap_client::MAX_APPEND + 1] },
        ] {
            assert!(matches!(bad.to_command(), Err(ImapError::Refused(_))), "{bad:?}");
        }
        // and the gate itself refuses everything else, plain EXPUNGE and `*` first
        for bad in [
            "EXPUNGE",
            "expunge",
            "UID EXPUNGE 1:*",
            "UID EXPUNGE *",
            "UID EXPUNGE ",
            "UID EXPUNGE 1,",
            "UID EXPUNGE 0",
            "UID STORE 1:* +FLAGS (\\Seen)",
            "UID STORE 1 FLAGS (\\Seen)",
            "UID STORE 1 FLAGS.SILENT (\\Seen)",
            "UID STORE 1 +FLAGS (\\Recent)",
            "UID STORE 1 +FLAGS (\\Seen \\Answered)",
            "UID STORE 1 +FLAGS ()",
            "UID STORE 1 +FLAGS (\\Seen)\r\nEXPUNGE",
            "UID STORE 1 (UNCHANGEDSINCE x) +FLAGS (\\Seen)",
            "STORE 1 +FLAGS (\\Seen)",
            "UID MOVE 1:* \"Trash\"",
            "UID MOVE 1 Trash",
            "UID MOVE 1 \"Trash\" extra",
            "UID MOVE 1 \"\"",
            "UID COPY 1 \"Trash\"\r\nDELETE x",
            "SELECT INBOX",
            "SELECT \"INBOX\" extra",
            "EXAMINE \"INBOX\"",
            "DELETE \"INBOX\"",
            "APPEND \"INBOX\" {5+}",
            "APPEND INBOX {5}",
            "APPEND \"INBOX\" (\\Deleted) {5}",
            "APPEND \"INBOX\" (\\Seen \\Draft) {5}",
            "APPEND \"INBOX\" () {5}",
            "APPEND \"INBOX\" {0}",
            "APPEND \"INBOX\" {104857601}",
            "APPEND \"INBOX\" {1234567890}",
            "APPEND \"INBOX\" {5}\r\nDELETE \"INBOX\"",
            "APPEND \"INBOX\" \"14-Nov-2026 12:00:00 +0000\" {5}",
            "APPEND \"\" {5}",
            "APPEND \"INBOX\"",
            "APPEND \"INBOX\" {5} extra",
            "CLOSE",
            "UID FETCH 1 (BODY[])",
            "",
        ] {
            assert!(matches!(check_write(bad), Err(ImapError::Refused(_))), "{bad:?} must be refused");
        }
    }

    // spec: MAIL-101
    #[test]
    fn no_text_of_a_write_command_exists_outside_the_typed_enum() {
        fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    files(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut all = vec![];
        files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut all);
        assert!(all.len() > 20);
        for p in all {
            let src = std::fs::read_to_string(&p).unwrap();
            let code = src.split("#[cfg(test)]\nmod tests").next().unwrap().split("#[cfg(test)]\npub(crate) mod tests").next().unwrap();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with("_tests.rs") {
                continue; // test-only files (the pretend server answers these commands)
            }
            if name == "imap_client.rs" {
                // inside the client: the write gate is passed in exactly one place
                assert_eq!(code.matches("Gate::Write").count(), 3, "Gate::Write: the check in `send`, the call in `write` and the one in `append` (called only from `write`)");
                assert_eq!(code.matches("self.append(").count(), 1, "`append` is reached from `write` only");
                assert_eq!(code.matches("fn run_gated").count(), 1);
                assert!(!code.contains("\"EXPUNGE\"") && !code.contains("format!(\"EXPUNGE"), "plain EXPUNGE is not a thing here");
                continue;
            }
            for (i, l) in code.lines().enumerate() {
                let t = l.trim_start();
                if t.starts_with("//") {
                    continue;
                }
                for banned in ["\"UID STORE", "\"UID MOVE", "\"UID COPY", "\"UID EXPUNGE", "\"EXPUNGE", "\"STORE "] {
                    assert!(!l.contains(banned), "{name}:{}: a write command typed by hand: {l}", i + 1);
                }
            }
        }
    }

    // spec: MAIL-101, MAIL-102
    #[test]
    fn select_is_used_only_for_a_write_and_flags_are_stored_with_unchangedsince() {
        let server = seeded_for_writes(MailSecurity::ImplicitTls);
        let mut c = connect(&server).unwrap();
        // writing before a SELECT is refused on the client and nothing reaches the server
        let err = c.write(&WriteCmd::StoreFlags { uids: vec![1], add: true, flags: vec![MailFlag::Seen], unchanged_since: None }).err().unwrap();
        assert!(matches!(err, ImapError::Refused(_)), "{err}");
        c.examine("INBOX").unwrap();
        let err = c.write(&WriteCmd::StoreFlags { uids: vec![1], add: true, flags: vec![MailFlag::Seen], unchanged_since: None }).err().unwrap();
        assert!(matches!(err, ImapError::Refused(_)), "an EXAMINEd folder is not writable: {err}");
        assert!(written(&server).is_empty());

        let sel = c.write(&WriteCmd::Select { folder: "INBOX".into() }).unwrap().selected.unwrap();
        assert_eq!((sel.uidvalidity, sel.uidnext), (1000, 5));
        let out = c.write(&WriteCmd::StoreFlags { uids: vec![1, 2], add: true, flags: vec![MailFlag::Seen, MailFlag::Flagged], unchanged_since: sel.highestmodseq }).unwrap();
        assert!(out.modified.is_empty());
        assert_eq!(out.flags.len(), 2);
        let on_server = server.state().messages("INBOX");
        assert_eq!(on_server[0].1, vec!["\\Seen", "\\Flagged"]);
        assert_eq!(on_server[2].1, Vec::<String>::new(), "others are untouched");
        // the other client changes message 2; our conditional store of it is refused, message 3 goes through
        server.state().set_flags("INBOX", 2, &["\\Answered"]);
        let out = c.write(&WriteCmd::StoreFlags { uids: vec![2, 3], add: false, flags: vec![MailFlag::Seen], unchanged_since: sel.highestmodseq }).unwrap();
        assert_eq!(out.modified, vec![2]);
        assert_eq!(server.state().messages("INBOX")[1].1, vec!["\\Answered"], "message 2 was not changed by our store");
        // flags that were read back include the modification sequence, for a second try
        let again = c.uid_fetch_flags_of(&[2]).unwrap();
        assert_eq!(again.len(), 1);
        let ms = again[0].modseq.expect("CONDSTORE gives MODSEQ");
        let out = c.write(&WriteCmd::StoreFlags { uids: vec![2], add: true, flags: vec![MailFlag::Seen], unchanged_since: Some(ms) }).unwrap();
        assert!(out.modified.is_empty());
        // examining again drops write mode
        c.examine("INBOX").unwrap();
        assert!(matches!(c.write(&WriteCmd::MarkDeleted { uids: vec![1], set: true }), Err(ImapError::Refused(_))));
        let verbs = written(&server);
        assert_eq!(verbs.iter().filter(|v| v.starts_with("SELECT")).count(), 1, "{verbs:?}");
        assert!(verbs.iter().all(|v| !v.starts_with("EXPUNGE") && !v.starts_with("STORE ")), "{verbs:?}");
    }

    // spec: MAIL-103
    #[test]
    fn move_and_copy_say_the_new_uids_and_a_server_without_move_refuses_move_before_sending_it() {
        let server = seeded_for_writes(MailSecurity::ImplicitTls);
        let mut c = connect(&server).unwrap();
        c.write(&WriteCmd::Select { folder: "INBOX".into() }).unwrap();
        let out = c.write(&WriteCmd::Copy { uids: vec![1, 2], to: "Trash".into() }).unwrap();
        let cu = out.copyuid.expect("COPYUID");
        assert_eq!((cu.uidvalidity, cu.from.clone(), cu.to.clone()), (1000, vec![1, 2], vec![1, 2]));
        assert_eq!(server.state().messages("INBOX").len(), 4, "a copy leaves the original");
        let out = c.write(&WriteCmd::Move { uids: vec![3], to: "Trash".into() }).unwrap();
        assert_eq!(out.copyuid.unwrap().to, vec![3]);
        assert_eq!(server.state().messages("INBOX").iter().map(|m| m.0).collect::<Vec<_>>(), vec![1, 2, 4]);
        // no such target: the server says NO and nothing moved
        let err = c.write(&WriteCmd::Move { uids: vec![4], to: "Nowhere".into() }).err().unwrap();
        assert!(matches!(err, ImapError::Server(_)), "{err}");
        c.logout();

        server.state().move_cap = false;
        let mut c = connect(&server).unwrap();
        c.write(&WriteCmd::Select { folder: "INBOX".into() }).unwrap();
        assert!(!c.can_move());
        let before = written(&server).len();
        assert!(matches!(c.write(&WriteCmd::Move { uids: vec![1], to: "Trash".into() }), Err(ImapError::Refused(_))));
        assert_eq!(written(&server).len(), before, "the refused MOVE never reached the server");
    }

    // spec: MAIL-105
    #[test]
    fn uid_expunge_removes_only_the_uids_asked_for_and_never_without_uidplus() {
        let server = seeded_for_writes(MailSecurity::ImplicitTls);
        // another program has flagged message 3 as deleted and not expunged it yet
        server.state().set_flags("INBOX", 3, &["\\Deleted"]);
        let mut c = connect(&server).unwrap();
        c.write(&WriteCmd::Select { folder: "INBOX".into() }).unwrap();
        c.write(&WriteCmd::MarkDeleted { uids: vec![1], set: true }).unwrap();
        c.write(&WriteCmd::ExpungeUids { uids: vec![1] }).unwrap();
        let left: Vec<u32> = server.state().messages("INBOX").iter().map(|m| m.0).collect();
        assert_eq!(left, vec![2, 3, 4], "only UID 1 is gone; the other client's \\Deleted mail 3 survives");
        // a UID that is not flagged is not removed by UID EXPUNGE either
        c.write(&WriteCmd::ExpungeUids { uids: vec![2] }).unwrap();
        assert_eq!(server.state().messages("INBOX").len(), 3);
        assert!(written(&server).iter().all(|v| v != "EXPUNGE"));
        c.logout();

        server.state().uidplus = false;
        let mut c = connect(&server).unwrap();
        c.write(&WriteCmd::Select { folder: "INBOX".into() }).unwrap();
        assert!(!c.can_expunge_uids());
        let before = written(&server).len();
        let err = c.write(&WriteCmd::ExpungeUids { uids: vec![3] }).err().unwrap();
        assert!(matches!(err, ImapError::Refused(_)), "{err}");
        assert_eq!(written(&server).len(), before);
        assert_eq!(server.state().messages("INBOX").len(), 3, "nothing was expunged");
    }

    // spec: MAIL-107
    #[test]
    fn a_server_that_says_no_or_hangs_up_during_a_write_is_an_error_and_changes_nothing() {
        let server = seeded_for_writes(MailSecurity::ImplicitTls);
        let mut c = connect(&server).unwrap();
        c.write(&WriteCmd::Select { folder: "INBOX".into() }).unwrap();
        server.state().fail_next_write = Some("[UNAVAILABLE] try later".into());
        let err = c.write(&WriteCmd::Move { uids: vec![1], to: "Trash".into() }).err().unwrap();
        assert!(matches!(&err, ImapError::Server(t) if t.contains("UNAVAILABLE")), "{err}");
        server.state().hang_up_on = Some("UID MOVE".into());
        let err = c.write(&WriteCmd::Move { uids: vec![1], to: "Trash".into() }).err().unwrap();
        assert!(matches!(err, ImapError::Connection(_)), "{err}");
        assert_eq!(server.state().messages("INBOX").len(), 4);
        assert_eq!(server.state().messages("Trash").len(), 0);
    }

    // spec: MAIL-140
    #[test]
    fn append_needs_no_select_sends_the_literal_after_the_plus_and_says_the_new_uid() {
        let server = seeded_for_writes(MailSecurity::ImplicitTls);
        server.state().add_folder("Sent", &["\\Sent"]);
        let mut c = connect(&server).unwrap();
        let mail = b"From: ada@example.com\r\nSubject: Hello\r\nMessage-ID: <a@x>\r\n\r\nBody with a literal } and {5} in it\r\n".to_vec();
        let out = c.write(&WriteCmd::Append { folder: "Sent".into(), flags: vec![MailFlag::Seen], bytes: mail.clone() }).unwrap();
        assert_eq!(out.appenduid, Some((1000, 1)));
        let on = server.state().messages("Sent");
        assert_eq!(on.len(), 1);
        assert_eq!((on[0].1.clone(), on[0].2.clone()), (vec!["\\Seen".to_string()], mail.clone()), "the exact bytes, flagged read");
        let verbs = server.state().verbs();
        assert!(verbs.iter().any(|v| v == &format!("APPEND \"Sent\" (\\Seen) {{{}}}", mail.len())), "{verbs:?}");
        assert!(!verbs.iter().any(|v| v.starts_with("SELECT")), "no folder is opened for an append: {verbs:?}");
        // a second append works on the same connection, and without UIDPLUS there is no APPENDUID
        server.state().no_appenduid = true;
        let out = c.write(&WriteCmd::Append { folder: "Sent".into(), flags: vec![], bytes: mail }).unwrap();
        assert_eq!(out.appenduid, None);
        assert_eq!(server.state().messages("Sent").len(), 2);
        assert_eq!(server.state().appended.len(), 2);
    }

    // spec: MAIL-140
    #[test]
    fn an_append_the_server_refuses_before_the_literal_sends_nothing_and_a_hang_up_is_a_connection_error() {
        let server = seeded_for_writes(MailSecurity::ImplicitTls);
        server.state().add_folder("Sent", &["\\Sent"]);
        let mut c = connect(&server).unwrap();
        let w = |c: &mut Client, f: &str| c.write(&WriteCmd::Append { folder: f.into(), flags: vec![MailFlag::Seen], bytes: b"Subject: x\r\n\r\nhi\r\n".to_vec() });
        server.state().fail_next_write = Some("[OVERQUOTA] full".into());
        let e = w(&mut c, "Sent").err().unwrap();
        assert!(matches!(&e, ImapError::Server(t) if t.contains("OVERQUOTA")), "{e}");
        let e = w(&mut c, "Nowhere").err().unwrap();
        assert!(matches!(&e, ImapError::Server(t) if t.contains("TRYCREATE")), "{e}");
        assert!(server.state().appended.is_empty());
        // the connection is still in step: the next append works
        w(&mut c, "Sent").unwrap();
        // the server stores the message and dies before the answer: an error for the client, the message is there
        server.state().hang_up_after_append = true;
        let e = w(&mut c, "Sent").err().unwrap();
        assert!(matches!(e, ImapError::Connection(_)), "{e}");
        assert_eq!(server.state().messages("Sent").len(), 2);
    }
}
