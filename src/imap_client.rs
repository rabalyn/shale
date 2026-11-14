//! A small blocking IMAP client (ADR-0007, ADR-0009, MAIL-020 to MAIL-025, MAIL-100 to MAIL-103).
//!
//! It speaks the part of IMAP4rev1 that phase 1 needs: `CAPABILITY`, `STARTTLS`, sign-in,
//! `LIST`, `EXAMINE`, `UID SEARCH` and `UID FETCH`, with `CONDSTORE` where the server has it.
//! Three rules are built into the one place that sends commands:
//!
//! - **Read-only.** [`check_read_only`] looks at every command before it is sent and refuses
//!   anything that could change the server: no `SELECT`, `STORE`, `EXPUNGE`, `COPY`, `MOVE`,
//!   `APPEND`, `DELETE`, and no fetch that would mark a mail as read (`BODY[`, `RFC822`;
//!   only `BODY.PEEK[`).
//! - **Always TLS.** Implicit TLS from the first byte, or `STARTTLS` that must succeed before
//!   anything else is sent; sign-in is refused on a connection that is not encrypted. The
//!   server certificate is checked by the operating system's verifier
//!   (`rustls-platform-verifier`); the production path has no switch to relax that.
//! - **Bounded.** Lines, literals (message bodies) and waiting time have limits. A message over
//!   the limit is skipped without being read into memory, and the connection stays usable.
//!
//! **Writing** (phase 2, ADR-0009) has its own single gate: [`Client::write`] takes a typed
//! [`WriteCmd`], builds the command text from it and sends it only after [`check_write`] accepted
//! its exact shape. Nothing else in the program can send a command that changes a mailbox: the read
//! path (`run`) is bound to [`check_read_only`], and the write path is a private function reached only
//! from `write`. Plain `EXPUNGE` does not exist here: only `UID EXPUNGE` of a given set.
//!
//! Signing in goes through the [`Authenticator`] hook: [`PlainAuth`] for passwords now, XOAUTH2
//! in phase 1c.

use crate::config::MailSecurity;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use rustls_platform_verifier::BuilderVerifierExt;
use std::collections::HashSet;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

/// Limits that keep a broken or hostile server from using up memory or time.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Largest message that is downloaded (bytes). Bigger ones are skipped.
    pub max_message: u64,
    /// Longest line of an answer, such as the result of `UID SEARCH ALL` in a huge folder.
    pub max_line: usize,
    /// Most literal bytes in one answer other than a message download.
    pub max_small_literal: u64,
    pub connect_timeout: Duration,
    /// How long a read or write may stall before the connection is given up.
    pub io_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_message: 200 << 20, max_line: 16 << 20, max_small_literal: 1 << 20, connect_timeout: Duration::from_secs(20), io_timeout: Duration::from_secs(60) }
    }
}

/// Where to connect and how.
#[derive(Clone, Debug)]
pub struct ConnectSettings {
    pub host: String,
    pub port: u16,
    pub security: MailSecurity,
    pub limits: Limits,
}

/// What went wrong with the connection, for a message to the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImapError {
    /// Could not reach the server, or the connection broke.
    Connection(String),
    /// TLS failed, such as a certificate that is not trusted.
    Tls(String),
    /// Wrong user name or password, or the server refused the sign-in.
    Auth(String),
    /// The server said something this client does not understand, or too much of it.
    Protocol(String),
    /// The server answered NO or BAD.
    Server(String),
    /// The client itself refused to send a command because it could change the server.
    Refused(String),
}

impl fmt::Display for ImapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImapError::Connection(s) => write!(f, "connection: {s}"),
            ImapError::Tls(s) => write!(f, "secure connection: {s}"),
            ImapError::Auth(s) => write!(f, "sign-in failed: {s}"),
            ImapError::Protocol(s) => write!(f, "unexpected answer from the server: {s}"),
            ImapError::Server(s) => write!(f, "the server said: {s}"),
            ImapError::Refused(s) => write!(f, "refused to send a command that could change the mailbox: {s}"),
        }
    }
}

impl std::error::Error for ImapError {}

pub type Result<T> = std::result::Result<T, ImapError>;

fn io_error(e: io::Error) -> ImapError {
    if let Some(t) = e.get_ref().and_then(|x| x.downcast_ref::<rustls::Error>()) {
        return ImapError::Tls(t.to_string());
    }
    match e.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => ImapError::Connection("the server did not answer in time".into()),
        io::ErrorKind::UnexpectedEof => ImapError::Connection("the server closed the connection".into()),
        _ => ImapError::Connection(e.to_string()),
    }
}

// ---------------------------------------------------------------- signing in

/// How to sign in. Passwords use [`PlainAuth`]; XOAUTH2 (phase 1c) is another implementation.
pub trait Authenticator: Send {
    /// Called with the server's capabilities before anything is sent: `offered("XOAUTH2")` says
    /// whether the server has `AUTH=XOAUTH2`. An authenticator that knows several mechanisms
    /// picks one here.
    fn select_mechanism(&mut self, _offered: &dyn Fn(&str) -> bool) {}
    /// The SASL mechanism, such as `PLAIN` or `XOAUTH2`.
    fn mechanism(&self) -> &str;
    /// The first message of the exchange.
    fn initial_response(&mut self) -> Vec<u8>;
    /// The answer to a server challenge (empty for most: that is also how XOAUTH2 acknowledges an error).
    fn respond(&mut self, _challenge: &[u8]) -> Vec<u8> {
        Vec::new()
    }
    /// User name and password for the old `LOGIN` command, for servers without `AUTH=PLAIN`.
    fn login_credentials(&self) -> Option<(String, String)> {
        None
    }
}

/// User name and password (`AUTHENTICATE PLAIN`, RFC 4616). Never shown in debug output.
pub struct PlainAuth {
    user: String,
    password: String,
}

impl PlainAuth {
    pub fn new(user: &str, password: &str) -> PlainAuth {
        PlainAuth { user: user.into(), password: password.into() }
    }
}

impl fmt::Debug for PlainAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PlainAuth({}, ***)", self.user)
    }
}

impl Authenticator for PlainAuth {
    fn mechanism(&self) -> &str {
        "PLAIN"
    }
    fn initial_response(&mut self) -> Vec<u8> {
        let mut v = vec![0];
        v.extend_from_slice(self.user.as_bytes());
        v.push(0);
        v.extend_from_slice(self.password.as_bytes());
        v
    }
    fn login_credentials(&self) -> Option<(String, String)> {
        Some((self.user.clone(), self.password.clone()))
    }
}

pub(crate) fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// ---------------------------------------------------------------- read-only guard

/// The one gate every command passes before it is sent (MAIL-023). Allowed: sign-in,
/// `CAPABILITY`, `STARTTLS`, `LIST`, `EXAMINE`, `NOOP`, `LOGOUT`, `UID SEARCH`, and `UID FETCH`
/// that does not ask for anything that marks a mail as read.
pub fn check_read_only(cmd: &str) -> Result<()> {
    let refuse = |why: &str| Err(ImapError::Refused(format!("{why}: {}", cmd.split_whitespace().take(2).collect::<Vec<_>>().join(" "))));
    if cmd.contains(['\r', '\n', '\0']) {
        return refuse("line break in a command");
    }
    let up = cmd.to_ascii_uppercase();
    let mut words = up.split_whitespace();
    match words.next() {
        Some("CAPABILITY" | "LOGIN" | "AUTHENTICATE" | "STARTTLS" | "LOGOUT" | "NOOP" | "LIST" | "EXAMINE") => Ok(()),
        Some("UID") => match words.next() {
            Some("SEARCH") => Ok(()),
            Some("FETCH") => {
                // what is left after taking out the harmless forms must not mention a body or message part
                let rest = up.replace("BODY.PEEK[", "").replace("RFC822.SIZE", "").replace("BINARY.PEEK[", "").replace("BINARY.SIZE[", "");
                if rest.contains("BODY[") || rest.contains("RFC822") || rest.contains("BINARY[") {
                    refuse("a fetch that would mark the mail as read")
                } else {
                    Ok(())
                }
            }
            _ => refuse("not a read-only command"),
        },
        _ => refuse("not a read-only command"),
    }
}

// ---------------------------------------------------------------- the write gate

/// The flags a person can set or clear: read and flagged (MAIL-102).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailFlag {
    Seen,
    Flagged,
}

impl MailFlag {
    pub fn as_str(self) -> &'static str {
        match self {
            MailFlag::Seen => "\\Seen",
            MailFlag::Flagged => "\\Flagged",
        }
    }
}

/// Everything that can change a mailbox, as data. The command text is made from this and from
/// nothing else (MAIL-101). Sets of UIDs are sorted and never contain `*`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteCmd {
    /// Open a folder for writing (`SELECT`); reading uses `EXAMINE`.
    Select {
        folder: String,
    },
    StoreFlags {
        uids: Vec<u32>,
        add: bool,
        flags: Vec<MailFlag>,
        unchanged_since: Option<u64>,
    },
    /// `UID MOVE` (needs the `MOVE` capability).
    Move {
        uids: Vec<u32>,
        to: String,
    },
    /// `UID COPY`: the first step of the fallback for a server without `MOVE`.
    Copy {
        uids: Vec<u32>,
        to: String,
    },
    /// Set (`set: true`) or clear `\Deleted`: only the fallback of a move and "delete forever".
    MarkDeleted {
        uids: Vec<u32>,
        set: bool,
    },
    /// `UID EXPUNGE`: removes exactly these UIDs, and only if they carry `\Deleted` (needs `UIDPLUS`).
    ExpungeUids {
        uids: Vec<u32>,
    },
}

fn checked_set(uids: &[u32]) -> Result<String> {
    if uids.is_empty() {
        return Err(ImapError::Refused("an empty set of messages".into()));
    }
    if uids.contains(&0) {
        return Err(ImapError::Refused("a message number of 0".into()));
    }
    Ok(uid_set(uids))
}

impl WriteCmd {
    /// The command line (without tag). Errors for an empty set, UID 0, an empty name or no flags.
    pub fn to_command(&self) -> Result<String> {
        let name = |n: &str| if n.is_empty() { Err(ImapError::Refused("an empty folder name".into())) } else { quote(n) };
        Ok(match self {
            WriteCmd::Select { folder } => format!("SELECT {}", name(folder)?),
            WriteCmd::StoreFlags { uids, add, flags, unchanged_since } => {
                if flags.is_empty() {
                    return Err(ImapError::Refused("no flags".into()));
                }
                let cond = unchanged_since.map(|m| format!("(UNCHANGEDSINCE {m}) ")).unwrap_or_default();
                let fl: Vec<&str> = flags.iter().map(|f| f.as_str()).collect();
                format!("UID STORE {} {cond}{}FLAGS ({})", checked_set(uids)?, if *add { '+' } else { '-' }, fl.join(" "))
            }
            WriteCmd::Move { uids, to } => format!("UID MOVE {} {}", checked_set(uids)?, name(to)?),
            WriteCmd::Copy { uids, to } => format!("UID COPY {} {}", checked_set(uids)?, name(to)?),
            WriteCmd::MarkDeleted { uids, set } => format!("UID STORE {} {}FLAGS (\\Deleted)", checked_set(uids)?, if *set { '+' } else { '-' }),
            WriteCmd::ExpungeUids { uids } => format!("UID EXPUNGE {}", checked_set(uids)?),
        })
    }
}

/// A quoted string at the start of `s` (with `\` escapes) and what follows it.
fn quoted_prefix(s: &str) -> Option<(String, &str)> {
    let mut chars = s.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut out = String::new();
    let mut esc = false;
    for (i, c) in chars {
        if esc {
            out.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '"' {
            return Some((out, &s[i + 1..]));
        } else {
            out.push(c);
        }
    }
    None
}

/// A set that is only numbers, `,` and `:` (never `*`), each part a UID or a range of two.
fn plain_uid_set(s: &str) -> bool {
    !s.is_empty()
        && s.split(',').all(|part| {
            let nums: Vec<&str> = part.split(':').collect();
            (nums.len() == 1 || nums.len() == 2) && nums.iter().all(|n| !n.is_empty() && n.len() <= 10 && n.bytes().all(|b| b.is_ascii_digit()) && n.parse::<u64>().is_ok_and(|v| v > 0))
        })
}

/// The gate of the write path (MAIL-101): accepts exactly the commands [`WriteCmd::to_command`] builds
/// and refuses everything else, plain `EXPUNGE` and `*` in a set included.
pub fn check_write(cmd: &str) -> Result<()> {
    let refuse = |why: &str| Err(ImapError::Refused(format!("{why}: {}", cmd.split_whitespace().take(2).collect::<Vec<_>>().join(" "))));
    if cmd.contains(['\r', '\n', '\0']) {
        return refuse("line break in a command");
    }
    if let Some(rest) = cmd.strip_prefix("SELECT ") {
        return match quoted_prefix(rest) {
            Some((n, "")) if !n.is_empty() => Ok(()),
            _ => refuse("a malformed SELECT"),
        };
    }
    let Some(rest) = cmd.strip_prefix("UID ") else { return refuse("not a command the write path sends") };
    if let Some(args) = rest.strip_prefix("EXPUNGE ") {
        return if plain_uid_set(args) { Ok(()) } else { refuse("UID EXPUNGE needs a plain set of UIDs") };
    }
    if let Some(args) = rest.strip_prefix("MOVE ").or_else(|| rest.strip_prefix("COPY ")) {
        let Some((set, folder)) = args.split_once(' ') else { return refuse("a malformed move or copy") };
        return match quoted_prefix(folder) {
            Some((n, "")) if plain_uid_set(set) && !n.is_empty() => Ok(()),
            _ => refuse("a malformed move or copy"),
        };
    }
    if let Some(args) = rest.strip_prefix("STORE ") {
        let Some((set, mut tail)) = args.split_once(' ') else { return refuse("a malformed STORE") };
        if !plain_uid_set(set) {
            return refuse("STORE needs a plain set of UIDs");
        }
        if let Some(t) = tail.strip_prefix("(UNCHANGEDSINCE ") {
            let Some((n, after)) = t.split_once(") ") else { return refuse("a malformed STORE") };
            if n.is_empty() || n.len() > 20 || !n.bytes().all(|b| b.is_ascii_digit()) {
                return refuse("a malformed STORE");
            }
            tail = after;
        }
        let Some(list) = tail.strip_prefix("+FLAGS (").or_else(|| tail.strip_prefix("-FLAGS (")).and_then(|t| t.strip_suffix(')')) else { return refuse("STORE only adds or removes flags") };
        let flags: Vec<&str> = list.split(' ').collect();
        return if !flags.is_empty() && flags.iter().all(|f| matches!(*f, "\\Seen" | "\\Flagged" | "\\Deleted")) { Ok(()) } else { refuse("a flag that is not read, flagged or deleted") };
    }
    refuse("not a command the write path sends")
}

/// What a write told back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteOutcome {
    /// `Select`: what the server said about the folder.
    pub selected: Option<Selected>,
    /// `COPYUID` (`Move`, `Copy`): the UIDs the messages got in the target folder.
    pub copyuid: Option<CopyUid>,
    /// `STORE` with `UNCHANGEDSINCE`: the UIDs that were not changed because another client changed them first.
    pub modified: Vec<u32>,
    /// The flags the server reported (`STORE` answers).
    pub flags: Vec<Fetched>,
}

/// `[COPYUID <uidvalidity of the target> <source uids> <new uids>]`, the two lists in the same order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyUid {
    pub uidvalidity: u32,
    pub from: Vec<u32>,
    pub to: Vec<u32>,
}

/// `1:3,7` → `[1, 2, 3, 7]` (at most 100,000 entries; None if it is not a plain set).
pub fn parse_uid_set(s: &str) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let (a, b) = match part.split_once(':') {
            Some((a, b)) => (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?),
            None => {
                let n = part.parse::<u32>().ok()?;
                (n, n)
            }
        };
        let (lo, hi) = (a.min(b), a.max(b));
        if out.len() as u64 + (hi - lo) as u64 + 1 > 100_000 {
            return None;
        }
        out.extend(lo..=hi);
    }
    Some(out)
}

fn parse_copyuid(arg: &str) -> Option<CopyUid> {
    let mut p = arg.split_whitespace();
    let uidvalidity = p.next()?.parse().ok()?;
    let from = parse_uid_set(p.next()?)?;
    let to = parse_uid_set(p.next()?)?;
    (from.len() == to.len()).then_some(CopyUid { uidvalidity, from, to })
}

/// The text inside `[CODE ...]` of a status line, if it has that code.
fn status_code<'a>(text: &'a str, code: &str) -> Option<&'a str> {
    let start = text.find(&format!("[{code} "))? + code.len() + 2;
    let end = start + text[start..].find(']')?;
    Some(&text[start..end])
}

// ---------------------------------------------------------------- reading answers

/// A literal inside an answer: the bytes, or just its size if it was over the limit.
#[derive(Debug, PartialEq, Eq)]
enum Literal {
    Data(Vec<u8>),
    Skipped(u64),
}

/// One answer with its literals taken out: `text` has a 0 byte where each literal was.
#[derive(Debug, Default)]
struct RawResponse {
    text: Vec<u8>,
    lits: Vec<Literal>,
}

/// The connection: plain only until STARTTLS has been done.
enum Io {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
    /// While the connection is being upgraded.
    Gone,
}

impl Read for Io {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Io::Plain(s) => s.read(buf),
            Io::Tls(s) => s.read(buf),
            Io::Gone => Err(io::Error::other("no connection")),
        }
    }
}

impl Write for Io {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Io::Plain(s) => s.write(buf),
            Io::Tls(s) => s.write(buf),
            Io::Gone => Err(io::Error::other("no connection")),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Io::Plain(s) => s.flush(),
            Io::Tls(s) => s.flush(),
            Io::Gone => Ok(()),
        }
    }
}

/// Buffered reading of lines and literals from the connection.
struct Wire {
    io: Io,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
    max_line: usize,
}

impl Wire {
    fn new(io: Io, max_line: usize) -> Wire {
        Wire { io, buf: vec![0; 16 * 1024], pos: 0, len: 0, max_line }
    }

    fn fill(&mut self) -> Result<()> {
        if self.pos == self.len {
            self.pos = 0;
            self.len = 0;
        }
        if self.len == self.buf.len() {
            if self.pos > 0 {
                self.buf.copy_within(self.pos..self.len, 0);
                self.len -= self.pos;
                self.pos = 0;
            } else {
                let n = self.buf.len() * 2;
                self.buf.resize(n, 0);
            }
        }
        let n = self.io.read(&mut self.buf[self.len..]).map_err(io_error)?;
        if n == 0 {
            return Err(ImapError::Connection("the server closed the connection".into()));
        }
        self.len += n;
        Ok(())
    }

    /// One line without its CRLF. Longer than the limit is an error (and the connection is not used again).
    fn read_line(&mut self) -> Result<Vec<u8>> {
        let mut scanned = 0;
        loop {
            let avail = &self.buf[self.pos..self.len];
            if let Some(i) = avail[scanned.min(avail.len().saturating_sub(1))..].windows(2).position(|w| w == b"\r\n") {
                let end = scanned.min(avail.len().saturating_sub(1)) + i;
                let line = avail[..end].to_vec();
                self.pos += end + 2;
                return Ok(line);
            }
            if avail.len() > self.max_line {
                return Err(ImapError::Protocol("a line of the answer is too long".into()));
            }
            scanned = avail.len();
            self.fill()?;
        }
    }

    fn take(&mut self, n: u64, keep: bool) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut left = n;
        while left > 0 {
            if self.pos == self.len {
                self.fill()?;
            }
            let k = ((self.len - self.pos) as u64).min(left) as usize;
            if keep {
                out.extend_from_slice(&self.buf[self.pos..self.pos + k]);
            }
            self.pos += k;
            left -= k as u64;
        }
        Ok(out)
    }

    /// One whole answer: lines, with the literals they announce read in between. Literals
    /// beyond `limit` bytes (in total) are read and thrown away.
    fn read_response(&mut self, limit: u64) -> Result<RawResponse> {
        let mut r = RawResponse::default();
        let mut budget = limit;
        loop {
            let line = self.read_line()?;
            if line.contains(&0) {
                return Err(ImapError::Protocol("a NUL byte in an answer".into()));
            }
            match literal_at_end(&line) {
                Some((start, n)) => {
                    r.text.extend_from_slice(&line[..start]);
                    r.text.push(0);
                    if n <= budget {
                        budget -= n;
                        r.lits.push(Literal::Data(self.take(n, true)?));
                    } else {
                        self.take(n, false)?;
                        r.lits.push(Literal::Skipped(n));
                    }
                }
                None => {
                    r.text.extend_from_slice(&line);
                    return Ok(r);
                }
            }
        }
    }
}

/// `... {123}` or `... {123+}` at the end of a line: where it starts and how long the literal is.
fn literal_at_end(line: &[u8]) -> Option<(usize, u64)> {
    let inner = line.strip_suffix(b"}")?;
    let open = inner.iter().rposition(|&b| b == b'{')?;
    let digits = &inner[open + 1..];
    let digits = digits.strip_suffix(b"+").unwrap_or(digits);
    if digits.is_empty() || digits.len() > 15 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some((open, std::str::from_utf8(digits).ok()?.parse().ok()?))
}

// ---------------------------------------------------------------- understanding answers

#[derive(Debug, PartialEq)]
enum Tok {
    Atom(String),
    Str(Vec<u8>),
    /// A literal over the limit: only its size.
    Skipped(u64),
    Open,
    Close,
    End,
}

struct Toks<'a> {
    t: &'a [u8],
    i: usize,
    lits: &'a [Literal],
    li: usize,
}

impl<'a> Toks<'a> {
    fn new(r: &'a RawResponse) -> Toks<'a> {
        Toks { t: &r.text, i: 0, lits: &r.lits, li: 0 }
    }

    fn skip_space(&mut self) {
        while self.i < self.t.len() && self.t[self.i] == b' ' {
            self.i += 1;
        }
    }

    fn next(&mut self) -> Tok {
        self.skip_space();
        let Some(&c) = self.t.get(self.i) else { return Tok::End };
        match c {
            b'(' => {
                self.i += 1;
                Tok::Open
            }
            b')' => {
                self.i += 1;
                Tok::Close
            }
            0 => {
                self.i += 1;
                let l = self.lits.get(self.li);
                self.li += 1;
                match l {
                    Some(Literal::Data(d)) => Tok::Str(d.clone()),
                    Some(Literal::Skipped(n)) => Tok::Skipped(*n),
                    None => Tok::End,
                }
            }
            b'"' => {
                self.i += 1;
                let mut s = Vec::new();
                while self.i < self.t.len() && self.t[self.i] != b'"' {
                    if self.t[self.i] == b'\\' && self.i + 1 < self.t.len() {
                        self.i += 1;
                    }
                    s.push(self.t[self.i]);
                    self.i += 1;
                }
                self.i += 1;
                Tok::Str(s)
            }
            _ => {
                let start = self.i;
                while self.i < self.t.len() && !matches!(self.t[self.i], b' ' | b'(' | b')' | 0) {
                    if self.t[self.i] == b'[' {
                        // BODY[HEADER.FIELDS (A B)]: brackets can hold spaces and parentheses
                        while self.i < self.t.len() && self.t[self.i] != b']' {
                            self.i += 1;
                        }
                    }
                    self.i += 1;
                }
                let end = self.i.min(self.t.len());
                Tok::Atom(String::from_utf8_lossy(&self.t[start..end]).into_owned())
            }
        }
    }

    /// Skip one value (an atom, string, literal or a whole parenthesised list).
    fn skip_value(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.next() {
                Tok::Open => depth += 1,
                Tok::Close => depth = depth.saturating_sub(1),
                Tok::End => return,
                _ => {}
            }
            if depth == 0 {
                return;
            }
        }
    }

    /// The rest of the line as text (for the human-readable part of a status line).
    fn rest(&mut self) -> String {
        self.skip_space();
        String::from_utf8_lossy(&self.t[self.i.min(self.t.len())..]).replace('\0', "").trim().to_string()
    }
}

fn text_of(t: Tok) -> Option<Vec<u8>> {
    match t {
        Tok::Atom(a) if a.eq_ignore_ascii_case("NIL") => None,
        Tok::Atom(a) => Some(a.into_bytes()),
        Tok::Str(s) => Some(s),
        _ => None,
    }
}

/// A folder as `LIST` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderEntry {
    /// The name as the server writes it (modified UTF-7); what `EXAMINE` gets.
    pub name: String,
    pub delimiter: Option<String>,
    /// Attributes such as `\HasNoChildren`, `\Noselect`, `\Trash`.
    pub attributes: Vec<String>,
}

impl FolderEntry {
    pub fn has_attr(&self, a: &str) -> bool {
        self.attributes.iter().any(|x| x.eq_ignore_ascii_case(a))
    }
    /// The name as people read it.
    #[cfg(test)]
    pub fn display_name(&self) -> String {
        decode_mutf7(&self.name)
    }
}

/// What `EXAMINE` told about a folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selected {
    pub exists: u32,
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub highestmodseq: Option<u64>,
}

/// A body from a fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Data(Vec<u8>),
    /// Over the size limit: skipped, this many bytes.
    TooLarge(u64),
}

/// One `FETCH` answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetched {
    pub uid: u32,
    /// Flags, sorted, separated by spaces.
    pub flags: Option<String>,
    pub internal_date: Option<i64>,
    pub size: Option<u64>,
    pub body: Option<Body>,
    pub modseq: Option<u64>,
}

#[derive(Debug)]
enum Untagged {
    Capability(Vec<String>),
    List(FolderEntry),
    Exists(u32),
    Status { kind: String, code: Option<(String, String)>, text: String },
    Search(Vec<u32>),
    Fetch(Fetched),
    Other,
}

fn parse_untagged(r: &RawResponse) -> Result<Untagged> {
    let bad = |what: &str| ImapError::Protocol(what.to_string());
    let mut t = Toks::new(r);
    let Tok::Atom(first) = t.next() else { return Err(bad("an answer that does not start with a word")) };
    if let Ok(n) = first.parse::<u32>() {
        let Tok::Atom(w) = t.next() else { return Ok(Untagged::Other) };
        return Ok(match w.to_ascii_uppercase().as_str() {
            "EXISTS" => Untagged::Exists(n),
            "FETCH" => Untagged::Fetch(parse_fetch(&mut t)?),
            _ => Untagged::Other,
        });
    }
    Ok(match first.to_ascii_uppercase().as_str() {
        "CAPABILITY" => {
            let mut caps = Vec::new();
            while let Tok::Atom(a) = t.next() {
                caps.push(a.to_ascii_uppercase());
            }
            Untagged::Capability(caps)
        }
        "LIST" | "LSUB" => {
            if t.next() != Tok::Open {
                return Err(bad("a folder entry without attributes"));
            }
            let mut attributes = Vec::new();
            loop {
                match t.next() {
                    Tok::Atom(a) => attributes.push(a),
                    Tok::Close => break,
                    _ => return Err(bad("a folder entry with broken attributes")),
                }
            }
            let delimiter = text_of(t.next()).map(|d| String::from_utf8_lossy(&d).into_owned());
            let name = match t.next() {
                Tok::Skipped(_) => return Err(bad("a folder name that is far too long")),
                other => text_of(other).ok_or_else(|| bad("a folder entry without a name"))?,
            };
            Untagged::List(FolderEntry { name: String::from_utf8_lossy(&name).into_owned(), delimiter, attributes })
        }
        "SEARCH" => {
            let mut v = Vec::new();
            while let Tok::Atom(a) = t.next() {
                match a.parse::<u32>() {
                    Ok(n) => v.push(n),
                    Err(_) => break, // (MODSEQ n) of CONDSTORE servers
                }
            }
            Untagged::Search(v)
        }
        k @ ("OK" | "NO" | "BAD" | "BYE" | "PREAUTH") => {
            let rest = t.rest();
            let (code, text) = match rest.strip_prefix('[').and_then(|s| s.split_once(']')) {
                Some((c, text)) => {
                    let (name, arg) = c.split_once(' ').unwrap_or((c, ""));
                    (Some((name.to_ascii_uppercase(), arg.trim().to_string())), text.trim().to_string())
                }
                None => (None, rest),
            };
            Untagged::Status { kind: k.to_string(), code, text }
        }
        _ => Untagged::Other,
    })
}

fn parse_fetch(t: &mut Toks<'_>) -> Result<Fetched> {
    let bad = |what: &str| ImapError::Protocol(what.to_string());
    if t.next() != Tok::Open {
        return Err(bad("a fetch answer without a list"));
    }
    let mut f = Fetched::default();
    let mut have_uid = false;
    loop {
        let name = match t.next() {
            Tok::Atom(a) => a.to_ascii_uppercase(),
            Tok::Close | Tok::End => break,
            _ => return Err(bad("a fetch answer with a broken list")),
        };
        match name.as_str() {
            "UID" => {
                let Tok::Atom(n) = t.next() else { return Err(bad("a fetch answer with a broken UID")) };
                f.uid = n.parse().map_err(|_| bad("a fetch answer with a broken UID"))?;
                have_uid = true;
            }
            "FLAGS" => {
                if t.next() != Tok::Open {
                    return Err(bad("flags without a list"));
                }
                let mut flags = Vec::new();
                loop {
                    match t.next() {
                        Tok::Atom(a) => flags.push(a),
                        Tok::Close | Tok::End => break,
                        _ => {}
                    }
                }
                flags.sort_by_key(|a| a.to_ascii_lowercase());
                f.flags = Some(flags.join(" "));
            }
            "INTERNALDATE" => {
                if let Some(d) = text_of(t.next()) {
                    f.internal_date = parse_internal_date(&String::from_utf8_lossy(&d));
                }
            }
            "RFC822.SIZE" => {
                if let Tok::Atom(n) = t.next() {
                    f.size = n.parse().ok();
                }
            }
            "MODSEQ" => {
                if t.next() == Tok::Open {
                    if let Tok::Atom(n) = t.next() {
                        f.modseq = n.parse().ok();
                    }
                    t.skip_value_until_close();
                }
            }
            n if n.starts_with("BODY[") => {
                f.body = match t.next() {
                    Tok::Str(d) => Some(Body::Data(d)),
                    Tok::Skipped(n) => Some(Body::TooLarge(n)),
                    Tok::Atom(a) if a.eq_ignore_ascii_case("NIL") => None,
                    _ => return Err(bad("a message body that is not a string")),
                }
            }
            _ => t.skip_value(),
        }
    }
    if !have_uid {
        return Err(bad("a fetch answer without a UID"));
    }
    Ok(f)
}

impl Toks<'_> {
    /// After the first value inside a list: skip to its closing parenthesis.
    fn skip_value_until_close(&mut self) {
        loop {
            match self.next() {
                Tok::Close | Tok::End => return,
                Tok::Open => self.skip_value_open(),
                _ => {}
            }
        }
    }
    fn skip_value_open(&mut self) {
        let mut depth = 1;
        while depth > 0 {
            match self.next() {
                Tok::Open => depth += 1,
                Tok::Close => depth -= 1,
                Tok::End => return,
                _ => {}
            }
        }
    }
}

/// `17-Jul-1996 02:44:25 -0700` → UTC seconds.
pub fn parse_internal_date(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once(' ')?;
    crate::mail::parse_date(&format!("{} {rest}", date.replace('-', " ")))
}

/// IMAP's modified UTF-7 (`Gesendet &AOQ-`) as readable text.
pub fn decode_mutf7(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let code = &after[..end];
        if code.is_empty() {
            out.push('&');
        } else {
            let bytes = crate::mail::base64_decode(code.replace(',', "/").as_bytes());
            let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            out.extend(char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')));
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn absorb_select(sel: &mut Selected, have_validity: &mut bool, u: &Untagged) {
    match u {
        Untagged::Exists(n) => sel.exists = *n,
        Untagged::Status { code: Some((c, arg)), .. } => match c.as_str() {
            "UIDVALIDITY" => {
                sel.uidvalidity = arg.parse().unwrap_or(0);
                *have_validity = true;
            }
            "UIDNEXT" => sel.uidnext = arg.parse().unwrap_or(0),
            "HIGHESTMODSEQ" => sel.highestmodseq = arg.parse().ok(),
            _ => {}
        },
        _ => {}
    }
}

/// A list of UIDs as an IMAP set: `[1,2,3,7,9,10]` → `1:3,7,9:10` (sorted, no duplicates).
pub fn uid_set(uids: &[u32]) -> String {
    let mut v = uids.to_vec();
    v.sort_unstable();
    v.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let mut j = i;
        while j + 1 < v.len() && v[j + 1] == v[j] + 1 {
            j += 1;
        }
        parts.push(if j > i { format!("{}:{}", v[i], v[j]) } else { v[i].to_string() });
        i = j + 1;
    }
    parts.join(",")
}

/// What `UID SEARCH` looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchKey {
    All,
    /// Received on or after this day (year, month, day).
    Since(i32, u32, u32),
    /// This UID and everything after it.
    UidFrom(u32),
}

impl SearchKey {
    fn to_command(self) -> String {
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        match self {
            SearchKey::All => "ALL".into(),
            SearchKey::Since(y, m, d) => format!("SINCE {d}-{}-{y}", MONTHS[(m.clamp(1, 12) - 1) as usize]),
            SearchKey::UidFrom(u) => format!("UID {u}:*"),
        }
    }
}

/// What to download of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The whole message (`BODY.PEEK[]`).
    Full,
    /// Only the headers (`BODY.PEEK[HEADER]`).
    Headers,
}

// ---------------------------------------------------------------- TLS

fn crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The TLS settings of every real connection: certificates are checked by the operating system.
pub fn platform_tls_config() -> Result<Arc<ClientConfig>> {
    let tls = |e: rustls::Error| ImapError::Tls(e.to_string());
    let cfg = ClientConfig::builder_with_provider(crypto_provider()).with_safe_default_protocol_versions().map_err(tls)?.with_platform_verifier().map_err(tls)?.with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// Tests only: the same verifier, but one more certificate is trusted (the pretend server's).
#[cfg(test)]
pub fn test_tls_config(extra_root: rustls::pki_types::CertificateDer<'static>) -> Arc<ClientConfig> {
    let provider = crypto_provider();
    let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots([extra_root], provider.clone()).expect("verifier");
    let cfg = ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().expect("versions").dangerous().with_custom_certificate_verifier(Arc::new(verifier)).with_no_client_auth();
    Arc::new(cfg)
}

fn tcp_connect(s: &ConnectSettings) -> Result<TcpStream> {
    let addrs = (s.host.as_str(), s.port).to_socket_addrs().map_err(|e| ImapError::Connection(format!("cannot find {}: {e}", s.host)))?;
    let mut last = None;
    for a in addrs {
        match TcpStream::connect_timeout(&a, s.limits.connect_timeout) {
            Ok(t) => {
                let _ = t.set_nodelay(true);
                t.set_read_timeout(Some(s.limits.io_timeout)).map_err(io_error)?;
                t.set_write_timeout(Some(s.limits.io_timeout)).map_err(io_error)?;
                return Ok(t);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.map(io_error).unwrap_or_else(|| ImapError::Connection(format!("no address for {}", s.host))))
}

fn tls_stream(cfg: Arc<ClientConfig>, host: &str, tcp: TcpStream) -> Result<Io> {
    let name = ServerName::try_from(host.to_string()).map_err(|_| ImapError::Tls(format!("{host} is not a valid server name")))?;
    let conn = ClientConnection::new(cfg, name).map_err(|e| ImapError::Tls(e.to_string()))?;
    Ok(Io::Tls(Box::new(StreamOwned::new(conn, tcp))))
}

// ---------------------------------------------------------------- the client

/// One IMAP connection. Everything it can do is read-only.
pub struct Client {
    wire: Wire,
    caps: HashSet<String>,
    secure: bool,
    tag: u32,
    limits: Limits,
    /// The folder opened with `SELECT` for writing; cleared by `EXAMINE`.
    writable: Option<String>,
}

/// Which gate a command passes before it is sent.
#[derive(Clone, Copy)]
enum Gate {
    ReadOnly,
    Write,
}

/// How a command ended.
struct Completion {
    ok: bool,
    text: String,
}

/// Quote a string for a command (a folder name): `"` and `\` escaped, line breaks refused.
fn quote(s: &str) -> Result<String> {
    if s.contains(['\r', '\n', '\0']) {
        return Err(ImapError::Refused("a line break in a name".into()));
    }
    Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
}

impl Client {
    /// Connect, secure the connection and sign in. The server certificate is checked by the
    /// operating system's verifier.
    pub fn connect(settings: &ConnectSettings, auth: &mut dyn Authenticator) -> Result<Client> {
        Client::connect_with(settings, platform_tls_config()?, auth)
    }

    /// Tests only: trust the pretend server's certificate in addition.
    #[cfg(test)]
    pub fn connect_test(settings: &ConnectSettings, root: rustls::pki_types::CertificateDer<'static>, auth: &mut dyn Authenticator) -> Result<Client> {
        Client::connect_with(settings, test_tls_config(root), auth)
    }

    fn connect_with(settings: &ConnectSettings, tls: Arc<ClientConfig>, auth: &mut dyn Authenticator) -> Result<Client> {
        let tcp = tcp_connect(settings)?;
        let mut c = match settings.security {
            MailSecurity::ImplicitTls => {
                let mut c = Client::new(tls_stream(tls, &settings.host, tcp)?, true, &settings.limits);
                c.read_greeting()?;
                c
            }
            MailSecurity::StartTls => {
                let mut c = Client::new(Io::Plain(tcp), false, &settings.limits);
                c.read_greeting()?;
                c.start_tls(tls, &settings.host)?;
                c
            }
        };
        c.refresh_capabilities()?;
        c.authenticate(auth)?;
        Ok(c)
    }

    fn new(io: Io, secure: bool, limits: &Limits) -> Client {
        Client { wire: Wire::new(io, limits.max_line), caps: HashSet::new(), secure, tag: 0, limits: limits.clone(), writable: None }
    }

    fn read_greeting(&mut self) -> Result<()> {
        let mut r = self.wire.read_response(self.limits.max_small_literal)?;
        let Some(rest) = r.text.strip_prefix(b"* ") else { return Err(ImapError::Protocol("no greeting".into())) };
        r.text = rest.to_vec();
        match parse_untagged(&r)? {
            Untagged::Status { kind, .. } if kind == "OK" => Ok(()),
            Untagged::Status { kind, text, .. } => Err(ImapError::Server(format!("{kind} {text}"))),
            _ => Err(ImapError::Protocol("no greeting".into())),
        }
    }

    /// STARTTLS: ask, and only on `OK` upgrade. Without TLS nothing else is ever sent.
    fn start_tls(&mut self, tls: Arc<ClientConfig>, host: &str) -> Result<()> {
        self.refresh_capabilities()?;
        if !self.caps.contains("STARTTLS") {
            return Err(ImapError::Tls("the server does not offer STARTTLS, and a connection without encryption is never used".into()));
        }
        self.run_ok("STARTTLS", 0, |_| Ok(()))?;
        if self.wire.pos != self.wire.len {
            return Err(ImapError::Tls("data arrived before the encryption started".into()));
        }
        let Io::Plain(tcp) = std::mem::replace(&mut self.wire.io, Io::Gone) else { return Err(ImapError::Tls("already encrypted".into())) };
        self.wire.io = tls_stream(tls, host, tcp)?;
        self.secure = true;
        self.caps.clear(); // what the server offered before the encryption does not count
        Ok(())
    }

    pub fn has_capability(&self, name: &str) -> bool {
        self.caps.contains(&name.to_ascii_uppercase())
    }

    fn refresh_capabilities(&mut self) -> Result<()> {
        let mut caps = HashSet::new();
        self.run_ok("CAPABILITY", self.limits.max_small_literal, |u| {
            if let Untagged::Capability(c) = u {
                caps.extend(c);
            }
            Ok(())
        })?;
        self.caps = caps;
        Ok(())
    }

    fn next_tag(&mut self) -> String {
        self.tag += 1;
        format!("S{}", self.tag)
    }

    fn send(&mut self, tag: &str, cmd: &str, gate: Gate) -> Result<()> {
        match gate {
            Gate::ReadOnly => check_read_only(cmd)?,
            Gate::Write => check_write(cmd)?,
        }
        let line = format!("{tag} {cmd}\r\n");
        self.wire.io.write_all(line.as_bytes()).and_then(|_| self.wire.io.flush()).map_err(io_error)
    }

    /// Send a command and hand every untagged answer to `on` until the tagged one comes.
    fn run(&mut self, cmd: &str, literal_limit: u64, on: impl FnMut(Untagged) -> Result<()>) -> Result<Completion> {
        self.run_gated(cmd, literal_limit, Gate::ReadOnly, on)
    }

    fn run_gated(&mut self, cmd: &str, literal_limit: u64, gate: Gate, mut on: impl FnMut(Untagged) -> Result<()>) -> Result<Completion> {
        let tag = self.next_tag();
        self.send(&tag, cmd, gate)?;
        loop {
            let r = self.wire.read_response(literal_limit)?;
            if let Some(rest) = r.text.strip_prefix(tag.as_bytes()).and_then(|x| x.strip_prefix(b" ")) {
                let text = String::from_utf8_lossy(rest).into_owned();
                let ok = text.get(..2).map(|w| w.eq_ignore_ascii_case("OK")).unwrap_or(false);
                return Ok(Completion { ok, text });
            }
            if !r.text.starts_with(b"* ") {
                return Err(ImapError::Protocol("an answer that is neither untagged nor for this command".into()));
            }
            let body = RawResponse { text: r.text[2..].to_vec(), lits: r.lits };
            if body.text.is_empty() {
                continue;
            }
            let u = parse_untagged(&body)?;
            if let Untagged::Status { kind, text, .. } = &u {
                if kind == "BYE" {
                    return Err(ImapError::Connection(format!("the server ended the connection: {text}")));
                }
            }
            on(u)?;
        }
    }

    fn run_ok(&mut self, cmd: &str, literal_limit: u64, on: impl FnMut(Untagged) -> Result<()>) -> Result<String> {
        let c = self.run(cmd, literal_limit, on)?;
        if c.ok {
            Ok(c.text)
        } else {
            Err(ImapError::Server(c.text))
        }
    }

    /// Sign in with the hook. Refused on a connection that is not encrypted.
    fn authenticate(&mut self, auth: &mut dyn Authenticator) -> Result<()> {
        if !self.secure {
            return Err(ImapError::Tls("signing in on a connection without encryption is never done".into()));
        }
        {
            let caps = self.caps.clone();
            auth.select_mechanism(&|m| caps.contains(&format!("AUTH={}", m.to_ascii_uppercase())));
        }
        let mech = auth.mechanism().to_ascii_uppercase();
        if self.caps.contains(&format!("AUTH={mech}")) {
            self.authenticate_sasl(&mech, auth)?;
        } else if let (true, Some((user, pass))) = (mech == "PLAIN" && !self.caps.contains("LOGINDISABLED"), auth.login_credentials()) {
            let cmd = format!("LOGIN {} {}", quote(&user)?, quote(&pass)?);
            let c = self.run(&cmd, self.limits.max_small_literal, |_| Ok(()))?;
            if !c.ok {
                return Err(ImapError::Auth(c.text));
            }
        } else {
            return Err(ImapError::Auth(format!("the server does not offer {mech} sign-in")));
        }
        self.refresh_capabilities()
    }

    fn authenticate_sasl(&mut self, mech: &str, auth: &mut dyn Authenticator) -> Result<()> {
        let tag = self.next_tag();
        let initial = auth.initial_response();
        if self.caps.contains("SASL-IR") {
            self.send(&tag, &format!("AUTHENTICATE {mech} {}", base64_encode(&initial)), Gate::ReadOnly)?;
        } else {
            self.send(&tag, &format!("AUTHENTICATE {mech}"), Gate::ReadOnly)?;
        }
        let mut sent_initial = self.caps.contains("SASL-IR");
        // what the server said when it refused the token (XOAUTH2 / OAUTHBEARER send the reason
        // as a challenge after the first message, and the final NO has no details)
        let mut reason: Option<String> = None;
        loop {
            let r = self.wire.read_response(self.limits.max_small_literal)?;
            if let Some(rest) = r.text.strip_prefix(b"+") {
                let challenge = crate::mail::base64_decode(String::from_utf8_lossy(rest).trim().as_bytes());
                if sent_initial && !challenge.is_empty() {
                    reason = Some(String::from_utf8_lossy(&challenge).chars().filter(|c| !c.is_control()).take(300).collect());
                }
                let answer = if sent_initial { auth.respond(&challenge) } else { initial.clone() };
                sent_initial = true;
                let line = format!("{}\r\n", base64_encode(&answer));
                self.wire.io.write_all(line.as_bytes()).and_then(|_| self.wire.io.flush()).map_err(io_error)?;
            } else if let Some(rest) = r.text.strip_prefix(tag.as_bytes()).and_then(|x| x.strip_prefix(b" ")) {
                let text = String::from_utf8_lossy(rest).into_owned();
                return if text.get(..2).map(|w| w.eq_ignore_ascii_case("OK")).unwrap_or(false) {
                    Ok(())
                } else {
                    Err(ImapError::Auth(match reason {
                        Some(r) => format!("{text} (the server said: {r})"),
                        None => text,
                    }))
                };
            }
            // untagged answers during sign-in carry nothing we need
        }
    }

    /// The folders of the account, with their attributes (special-use ones included).
    pub fn list_folders(&mut self) -> Result<Vec<FolderEntry>> {
        let cmd = if self.has_capability("SPECIAL-USE") { "LIST \"\" \"*\" RETURN (SPECIAL-USE)" } else { "LIST \"\" \"*\"" };
        let mut v = Vec::new();
        self.run_ok(cmd, self.limits.max_small_literal, |u| {
            if let Untagged::List(f) = u {
                v.push(f);
            }
            Ok(())
        })?;
        Ok(v)
    }

    /// Open a folder read-only (`EXAMINE`, never `SELECT`).
    pub fn examine(&mut self, name: &str) -> Result<Selected> {
        let opt = if self.has_capability("CONDSTORE") { " (CONDSTORE)" } else { "" };
        let mut sel = Selected::default();
        let mut have_validity = false;
        self.writable = None; // a folder opened with EXAMINE is read-only, and replaces the selected one
        self.run_ok(&format!("EXAMINE {}{opt}", quote(name)?), self.limits.max_small_literal, |u| {
            absorb_select(&mut sel, &mut have_validity, &u);
            Ok(())
        })?;
        if !have_validity {
            return Err(ImapError::Protocol("the server did not say the folder's UIDVALIDITY".into()));
        }
        Ok(sel)
    }

    /// `UID SEARCH` in the folder that was examined last.
    pub fn uid_search(&mut self, key: SearchKey) -> Result<Vec<u32>> {
        let mut v = Vec::new();
        self.run_ok(&format!("UID SEARCH {}", key.to_command()), self.limits.max_small_literal, |u| {
            if let Untagged::Search(n) = u {
                v.extend(n);
            }
            Ok(())
        })?;
        Ok(v)
    }

    /// Download messages (without marking them read). `on` gets each one as it arrives, so
    /// only one message is in memory at a time. A message over the size limit arrives as
    /// `Body::TooLarge` and is not read into memory.
    pub fn uid_fetch_messages(&mut self, uids: &[u32], part: Part, mut on: impl FnMut(Fetched) -> Result<()>) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let item = match part {
            Part::Full => "BODY.PEEK[]",
            Part::Headers => "BODY.PEEK[HEADER]",
        };
        let limit = match part {
            Part::Full => self.limits.max_message,
            Part::Headers => self.limits.max_small_literal,
        };
        self.run_ok(&format!("UID FETCH {} (UID FLAGS INTERNALDATE RFC822.SIZE {item})", uid_set(uids)), limit, |u| match u {
            Untagged::Fetch(f) => on(f),
            _ => Ok(()),
        })?;
        Ok(())
    }

    /// The flags of every message (or, with `changed_since`, of those changed after that
    /// `HIGHESTMODSEQ`, which needs `CONDSTORE`).
    pub fn uid_fetch_flags(&mut self, changed_since: Option<u64>, mut on: impl FnMut(Fetched) -> Result<()>) -> Result<()> {
        let tail = match changed_since {
            Some(m) if self.has_capability("CONDSTORE") => format!(" (CHANGEDSINCE {m})"),
            _ => String::new(),
        };
        self.run_ok(&format!("UID FETCH 1:* (UID FLAGS){tail}"), self.limits.max_small_literal, |u| match u {
            Untagged::Fetch(f) => on(f),
            _ => Ok(()),
        })?;
        Ok(())
    }

    /// Which of these UIDs exist in the folder that is open (read-only `UID SEARCH`).
    pub fn uid_exists(&mut self, uids: &[u32]) -> Result<Vec<u32>> {
        if uids.is_empty() {
            return Ok(vec![]);
        }
        let mut v = Vec::new();
        self.run_ok(&format!("UID SEARCH UID {}", uid_set(uids)), self.limits.max_small_literal, |u| {
            if let Untagged::Search(n) = u {
                v.extend(n);
            }
            Ok(())
        })?;
        v.retain(|u| uids.contains(u));
        Ok(v)
    }

    /// The flags (and, with CONDSTORE, the modification sequence) of these UIDs.
    pub fn uid_fetch_flags_of(&mut self, uids: &[u32]) -> Result<Vec<Fetched>> {
        if uids.is_empty() {
            return Ok(vec![]);
        }
        let item = if self.has_capability("CONDSTORE") { "UID FLAGS MODSEQ" } else { "UID FLAGS" };
        let mut out = Vec::new();
        self.run_ok(&format!("UID FETCH {} ({item})", uid_set(uids)), self.limits.max_small_literal, |u| {
            if let Untagged::Fetch(f) = u {
                if uids.contains(&f.uid) {
                    out.push(f);
                }
            }
            Ok(())
        })?;
        Ok(out)
    }

    /// The UIDs of messages with this `Message-ID` in the folder that is open (to verify a copy).
    pub fn uid_search_message_id(&mut self, message_id: &str) -> Result<Vec<u32>> {
        let mut v = Vec::new();
        self.run_ok(&format!("UID SEARCH HEADER Message-ID {}", quote(message_id)?), self.limits.max_small_literal, |u| {
            if let Untagged::Search(n) = u {
                v.extend(n);
            }
            Ok(())
        })?;
        Ok(v)
    }

    /// The one way to change a mailbox (MAIL-101). Refused here, before anything is sent: a write
    /// to a folder that was not opened with `Select`, `UID EXPUNGE` without `UIDPLUS`, `UID MOVE`
    /// without `MOVE`; and by [`check_write`]: anything but the exact shapes of [`WriteCmd`].
    pub fn write(&mut self, cmd: &WriteCmd) -> Result<WriteOutcome> {
        let text = cmd.to_command()?;
        match cmd {
            WriteCmd::Select { .. } => {}
            _ if self.writable.is_none() => return Err(ImapError::Refused("no folder is open for writing".into())),
            WriteCmd::ExpungeUids { .. } if !self.has_capability("UIDPLUS") => return Err(ImapError::Refused("the server has no UIDPLUS, so single messages cannot be removed safely".into())),
            WriteCmd::Move { .. } if !self.has_capability("MOVE") => return Err(ImapError::Refused("the server has no MOVE".into())),
            _ => {}
        }
        let mut out = WriteOutcome::default();
        let mut sel = Selected::default();
        let mut have_validity = false;
        let is_select = matches!(cmd, WriteCmd::Select { .. });
        if is_select {
            self.writable = None;
        }
        let c = self.run_gated(&text, self.limits.max_small_literal, Gate::Write, |u| {
            match &u {
                Untagged::Status { code: Some((c, arg)), .. } if c == "COPYUID" => out.copyuid = parse_copyuid(arg),
                Untagged::Fetch(f) => out.flags.push(f.clone()),
                _ => {}
            }
            absorb_select(&mut sel, &mut have_validity, &u);
            Ok(())
        })?;
        if !c.ok {
            return Err(ImapError::Server(c.text));
        }
        if let Some(arg) = status_code(&c.text, "COPYUID") {
            out.copyuid = parse_copyuid(arg);
        }
        if let Some(arg) = status_code(&c.text, "MODIFIED") {
            out.modified = parse_uid_set(arg.trim()).unwrap_or_default();
        }
        if let WriteCmd::Select { folder } = cmd {
            if !have_validity {
                return Err(ImapError::Protocol("the server did not say the folder's UIDVALIDITY".into()));
            }
            if c.text.contains("[READ-ONLY]") {
                return Err(ImapError::Server(format!("{} is read-only on this server", folder)));
            }
            self.writable = Some(folder.clone());
            out.selected = Some(sel);
        }
        Ok(out)
    }

    pub fn can_move(&self) -> bool {
        self.has_capability("MOVE")
    }

    pub fn can_expunge_uids(&self) -> bool {
        self.has_capability("UIDPLUS")
    }

    pub fn has_condstore(&self) -> bool {
        self.has_capability("CONDSTORE")
    }

    /// Say goodbye (errors do not matter any more).
    pub fn logout(mut self) {
        let _ = self.run("LOGOUT", 0, |_| Ok(()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_sets_are_compact_and_sorted() {
        assert_eq!(uid_set(&[9, 1, 2, 3, 7, 10, 3]), "1:3,7,9:10");
        assert_eq!(uid_set(&[]), "");
        assert_eq!(uid_set(&[5]), "5");
    }

    #[test]
    fn folder_names_and_dates_decode() {
        assert_eq!(decode_mutf7("Entw&APw-rfe"), "Entwürfe");
        assert_eq!(decode_mutf7("A&-B"), "A&B");
        assert_eq!(decode_mutf7("plain"), "plain");
        assert_eq!(parse_internal_date("17-Jul-1996 02:44:25 -0700"), Some(837_596_665));
        assert_eq!(parse_internal_date("garbage"), None);
    }

    #[test]
    fn literals_are_found_at_the_end_of_a_line() {
        assert_eq!(literal_at_end(b"* 1 FETCH (BODY[] {123}"), Some((18, 123)));
        assert_eq!(literal_at_end(b"x {5+}"), Some((2, 5)));
        assert_eq!(literal_at_end(b"x {a}"), None);
        assert_eq!(literal_at_end(b"x {}"), None);
        assert_eq!(literal_at_end(b"plain"), None);
    }

    #[test]
    fn answers_with_odd_shapes_do_not_panic() {
        for t in ["", "1", "1 FETCH", "1 FETCH (", "1 FETCH (UID", "1 FETCH (UID x)", "LIST", "LIST (", "LIST (\\a) NIL", "SEARCH a b", "OK [", "1 FETCH (UID 1 BODY[] )", "1 FETCH (UID 1 FLAGS", "1 FETCH (UID 1 MODSEQ (", "\"unterminated"]
        {
            let r = RawResponse { text: t.as_bytes().to_vec(), lits: vec![] };
            let _ = parse_untagged(&r);
        }
    }
}
