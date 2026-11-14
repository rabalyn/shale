//! GnuPG as programs (ADR-0011 §1, MAIL-170, MAIL-171): `gpg` for OpenPGP, `gpgsm` for S/MIME.
//!
//! Every call is `std::process::Command` with an argument list (never a shell), the data on stdin and the
//! result on stdout. What happened is read from the `[GNUPG:]` status lines (`--status-fd 2`) and from nothing
//! else: not from GnuPG's human text, not from anything in a mail. Passphrases are never handled here: `gpg-agent`
//! starts the pinentry, which is why the calls can take minutes, why they have time-outs and a cancel flag
//! that kills the child, and why callers run them in jobs and never on the UI thread.
//!
//! Nothing is ever looked up on the network (`--no-auto-key-retrieve --no-auto-key-locate`).

use crate::config::CryptoStandard;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A call that cannot open a dialog (a key list, a verification) gets this long.
pub const SHORT: Duration = Duration::from_secs(30);
/// A call that may open the pinentry dialog (decrypt, sign) gets this long.
pub const DIALOG: Duration = Duration::from_secs(300);
/// More than this much output from one call is refused.
const MAX_OUTPUT: usize = 256 << 20;
const MAX_STDERR: usize = 4 << 20;

/// Where and how GnuPG is called.
#[derive(Clone, Debug)]
pub struct Gnupg {
    /// `GNUPGHOME` for the child (None: the environment's, the default `~/.gnupg`).
    pub home: Option<PathBuf>,
    pub gpg: String,
    pub gpgsm: String,
}

impl Default for Gnupg {
    fn default() -> Self {
        Gnupg { home: None, gpg: "gpg".into(), gpgsm: "gpgsm".into() }
    }
}

/// Why a GnuPG call did not produce a result. The words are for the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GpgError {
    /// The program is not installed (or not executable).
    Missing(CryptoStandard),
    Timeout,
    Cancelled,
    TooBig,
    /// Anything else: starting the child or talking to it failed.
    Io(String),
    /// An argument that must not reach the program (checked before it starts).
    BadArgument(String),
}

impl std::fmt::Display for GpgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpgError::Missing(s) => write!(f, "{} is not installed (the program “{}” was not found), so {} cannot be used.", s.program(), s.program(), s.label()),
            GpgError::Timeout => write!(f, "GnuPG did not answer in time (a passphrase dialog may have been left open)."),
            GpgError::Cancelled => write!(f, "Cancelled."),
            GpgError::TooBig => write!(f, "GnuPG produced more output than Shale accepts."),
            GpgError::Io(e) => write!(f, "GnuPG could not be run: {e}"),
            GpgError::BadArgument(a) => write!(f, "Not passed to GnuPG: {a}"),
        }
    }
}

impl std::error::Error for GpgError {}

/// One `[GNUPG:] KEYWORD args` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub keyword: String,
    pub args: String,
}

impl Status {
    /// The arguments split at spaces (the user-id of `GOODSIG` and the like is the rest of the line: use `args`).
    pub fn fields(&self) -> Vec<&str> {
        self.args.split(' ').filter(|f| !f.is_empty()).collect()
    }
}

/// What a call gave: stdout, the status lines, the exit code.
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub stdout: Vec<u8>,
    pub status: Vec<Status>,
    pub code: i32,
}

impl Run {
    pub fn has(&self, keyword: &str) -> bool {
        self.status.iter().any(|s| s.keyword == keyword)
    }
}

/// The status lines of GnuPG's stderr: only lines that begin with `[GNUPG:] `.
pub fn parse_status(stderr: &[u8]) -> Vec<Status> {
    let mut out = Vec::new();
    for line in stderr.split(|b| *b == b'\n') {
        let Some(rest) = line.strip_prefix(b"[GNUPG:] ") else { continue };
        let text = String::from_utf8_lossy(rest);
        let text = text.trim_end_matches('\r');
        let (kw, args) = text.split_once(' ').unwrap_or((text, ""));
        if !kw.is_empty() && kw.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
            out.push(Status { keyword: kw.to_string(), args: args.to_string() });
        }
    }
    out
}

/// GnuPG writes `\xHH` and `\\` into user ids in the colon and status output.
pub fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            if b[i + 1] == b'x' {
                if let Some(v) = s.get(i + 2..i + 4).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 4;
                    continue;
                }
            }
            if b[i + 1] == b'\\' {
                out.push(b'\\');
                i += 2;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A fingerprint or key id: only hexadecimal digits, 8 to 64 of them. What goes on a command line as a key is
/// checked with this, so no argument can ever be taken for an option.
pub fn is_key_id(s: &str) -> bool {
    (8..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// An address that may be given to a key lookup: no option-looking start, no control characters, no space tricks.
pub fn is_lookup_address(s: &str) -> bool {
    !s.is_empty() && s.len() <= 254 && !s.starts_with('-') && s.contains('@') && !s.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\\' | '!' | '*'))
}

impl Gnupg {
    pub fn program(&self, s: CryptoStandard) -> &str {
        match s {
            CryptoStandard::OpenPgp => &self.gpg,
            CryptoStandard::Smime => &self.gpgsm,
        }
    }

    /// Run the program with `args` (after the fixed options), `input` on stdin. Ends when it exits, after `limit`
    /// (the child is killed) or when `cancel` is set (the child is killed).
    pub fn run(&self, s: CryptoStandard, args: &[&str], input: &[u8], limit: Duration, cancel: &AtomicBool) -> Result<Run, GpgError> {
        let mut cmd = Command::new(self.program(s));
        cmd.args(["--batch", "--no-tty", "--status-fd", "2"]);
        if s == CryptoStandard::OpenPgp {
            cmd.args(["--no-auto-key-retrieve", "--no-auto-key-locate", "--no-greeting"]);
        }
        cmd.args(args);
        if let Some(h) = &self.home {
            cmd.env("GNUPGHOME", h);
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = spawn_retrying(&mut cmd).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { GpgError::Missing(s) } else { GpgError::Io(e.to_string()) })?;
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().ok_or_else(|| GpgError::Io("no stdout".into()))?;
        let mut stderr = child.stderr.take().ok_or_else(|| GpgError::Io("no stderr".into()))?;
        let data = input.to_vec();
        let writer = std::thread::spawn(move || {
            if let Some(mut w) = stdin {
                let _ = w.write_all(&data); // the child may stop reading early: that is its answer, not our error
            }
        });
        // the readers report over channels, so a helper process that keeps a pipe open after the child is gone
        // (which `gpg` does not, but a stand-in might) cannot hold this call up
        let (out_tx, out_rx) = std::sync::mpsc::channel();
        let (err_tx, err_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = out_tx.send(read_capped(&mut stdout, MAX_OUTPUT));
        });
        std::thread::spawn(move || {
            let _ = err_tx.send(read_capped(&mut stderr, MAX_STDERR).0);
        });
        let start = Instant::now();
        let mut failure = None;
        let code = loop {
            match child.try_wait() {
                Ok(Some(st)) => break st.code().unwrap_or(-1),
                Ok(None) => {}
                Err(e) => {
                    failure = Some(GpgError::Io(e.to_string()));
                    break -1;
                }
            }
            if cancel.load(Ordering::SeqCst) {
                failure = Some(GpgError::Cancelled);
            } else if start.elapsed() > limit {
                failure = Some(GpgError::Timeout);
            }
            if failure.is_some() {
                let _ = child.kill();
                let _ = child.wait();
                break -1;
            }
            std::thread::sleep(Duration::from_millis(15));
        };
        let _ = writer.join();
        if let Some(f) = failure {
            return Err(f);
        }
        let grace = Duration::from_secs(5);
        let (stdout, too_big) = out_rx.recv_timeout(grace).map_err(|_| GpgError::Io("GnuPG's output did not end".into()))?;
        let stderr = err_rx.recv_timeout(grace).map_err(|_| GpgError::Io("GnuPG's status output did not end".into()))?;
        if too_big {
            return Err(GpgError::TooBig);
        }
        Ok(Run { stdout, status: parse_status(&stderr), code })
    }

    /// Is the program there? Its version line, or why not.
    pub fn version(&self, s: CryptoStandard) -> Result<String, GpgError> {
        let out = Command::new(self.program(s)).arg("--version").stdin(Stdio::null()).output().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { GpgError::Missing(s) } else { GpgError::Io(e.to_string()) })?;
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text.lines().next().unwrap_or("").trim().to_string())
    }
}

/// Start the child. A program that was written a moment ago can be "busy" while another thread's fork still holds its file
/// open (Linux `ETXTBSY`, seen with test stand-ins); that passes within milliseconds, so it is tried again a few times.
fn spawn_retrying(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    let mut tries = 0;
    loop {
        match cmd.spawn() {
            Err(e) if e.raw_os_error() == Some(26) && tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            r => return r,
        }
    }
}

/// Read to the end, keeping at most `cap` bytes; the flag says that there was more.
fn read_capped(r: &mut impl Read, cap: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    let mut over = false;
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if out.len() + n > cap {
                    over = true; // keep reading so the child can finish, but keep nothing more
                } else {
                    out.extend_from_slice(&buf[..n]);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (out, over)
}

// ---------------------------------------------------------------- keys

/// A key or certificate as `--list-keys --with-colons` describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyInfo {
    pub fingerprint: String,
    pub key_id: String,
    /// The user ids (names with addresses) or the certificate's subject and its e-mail addresses.
    pub uids: Vec<String>,
    /// The addresses among the user ids, lower case.
    pub emails: Vec<String>,
    /// GnuPG's validity letter of the primary key: `u` ultimate, `f` full, `m` marginal, `q`/`-` unknown,
    /// `n` never, `e` expired, `r` revoked, `d` disabled, `i` invalid.
    pub validity: char,
    pub can_encrypt: bool,
    pub can_sign: bool,
    pub expires: Option<i64>,
    pub secret: bool,
}

impl KeyInfo {
    /// Can this key be used for a new mail now (not expired, revoked, disabled or invalid)?
    pub fn usable(&self) -> bool {
        !matches!(self.validity, 'e' | 'r' | 'd' | 'i')
    }
    /// The first user id, for lists.
    pub fn label(&self) -> String {
        self.uids.first().cloned().unwrap_or_else(|| self.fingerprint.clone())
    }
}

/// `20261114T221602` or epoch seconds → epoch seconds.
pub fn parse_gpg_time(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || s == "0" {
        return None;
    }
    if s.len() == 15 && s.as_bytes()[8] == b'T' {
        let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
        let days = crate::cal_time::days_from_civil(n(0, 4)? as i32, n(4, 6)? as u32, n(6, 8)? as u32);
        return Some(days * 86_400 + n(9, 11)? * 3600 + n(11, 13)? * 60 + n(13, 15)?);
    }
    s.parse().ok()
}

/// Parse `--with-colons` listing output of `gpg` or `gpgsm`.
pub fn parse_keys(out: &[u8], secret: bool) -> Vec<KeyInfo> {
    let text = String::from_utf8_lossy(out);
    let mut keys: Vec<KeyInfo> = Vec::new();
    // which record's `fpr` line comes next: only the one after the primary counts
    let mut want_fpr = false;
    for line in text.lines() {
        let f: Vec<&str> = line.split(':').collect();
        let get = |i: usize| f.get(i).copied().unwrap_or("");
        match get(0) {
            "pub" | "sec" | "crt" | "crs" => {
                let caps = get(11);
                keys.push(KeyInfo {
                    fingerprint: String::new(),
                    key_id: get(4).to_string(),
                    uids: vec![],
                    emails: vec![],
                    validity: get(1).chars().next().unwrap_or('-'),
                    can_encrypt: caps.contains(['e', 'E']),
                    can_sign: caps.contains(['s', 'S']),
                    expires: parse_gpg_time(get(6)),
                    secret: secret || matches!(get(0), "sec" | "crs"),
                });
                // gpgsm puts the subject in the record itself
                if matches!(get(0), "crt" | "crs") && !get(9).is_empty() {
                    if let Some(k) = keys.last_mut() {
                        k.uids.push(unescape(get(9)));
                    }
                }
                want_fpr = true;
            }
            "sub" | "ssb" => {
                if let Some(k) = keys.last_mut() {
                    let caps = get(11);
                    let ok = !matches!(get(1).chars().next(), Some('e' | 'r' | 'd' | 'i'));
                    if ok && caps.contains(['e', 'E']) {
                        k.can_encrypt = true;
                    }
                    if ok && caps.contains(['s', 'S']) {
                        k.can_sign = true;
                    }
                }
                want_fpr = false;
            }
            "fpr" if want_fpr => {
                if let Some(k) = keys.last_mut() {
                    k.fingerprint = get(9).to_string();
                }
                want_fpr = false;
            }
            "uid" => {
                if let Some(k) = keys.last_mut() {
                    let u = unescape(get(9));
                    if !u.is_empty() && !matches!(get(1), "r" | "e") {
                        if let Some(addr) = address_in(&u) {
                            if !k.emails.contains(&addr) {
                                k.emails.push(addr);
                            }
                        }
                        k.uids.push(u);
                    }
                }
            }
            _ => {}
        }
    }
    keys.retain(|k| is_key_id(&k.fingerprint));
    keys
}

/// The e-mail address in a user id: `Name <a@b.c>` or `<a@b.c>`, lower case.
pub fn address_in(uid: &str) -> Option<String> {
    let a = match (uid.rfind('<'), uid.rfind('>')) {
        (Some(s), Some(e)) if s < e => &uid[s + 1..e],
        _ => uid,
    };
    let a = a.trim();
    (a.contains('@') && !a.contains(char::is_whitespace)).then(|| a.to_ascii_lowercase())
}

impl Gnupg {
    /// The keys (or, with `secret`, the secret keys) of a standard; `query` limits them to one address. An address
    /// that no key has is an empty list, not an error.
    pub fn list_keys(&self, s: CryptoStandard, secret: bool, query: Option<&str>, cancel: &AtomicBool) -> Result<Vec<KeyInfo>, GpgError> {
        let mut args = vec!["--with-colons", if secret { "--list-secret-keys" } else { "--list-keys" }];
        let q;
        if let Some(a) = query {
            if !is_lookup_address(a) {
                return Err(GpgError::BadArgument(a.to_string()));
            }
            q = format!("<{a}>");
            args.extend(["--", &q]);
        }
        let run = self.run(s, &args, b"", SHORT, cancel)?;
        Ok(parse_keys(&run.stdout, secret))
    }

    /// The user ids and addresses of one key, by its fingerprint.
    pub fn key_info(&self, s: CryptoStandard, fingerprint: &str, cancel: &AtomicBool) -> Result<Option<KeyInfo>, GpgError> {
        if !is_key_id(fingerprint) {
            return Err(GpgError::BadArgument(fingerprint.to_string()));
        }
        let run = self.run(s, &["--with-colons", "--list-keys", "--", fingerprint], b"", SHORT, cancel)?;
        Ok(parse_keys(&run.stdout, false).into_iter().next())
    }
}

// ---------------------------------------------------------------- signatures

/// What `gpg` says about one signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SigState {
    /// Cryptographically valid, key and signature in date.
    Good,
    /// The signature does not match the data.
    Bad,
    /// The key that made it is not in the keyring.
    UnknownKey,
    /// Valid, but the key has expired (since).
    ExpiredKey,
    /// Valid, but the signature itself has expired.
    ExpiredSig,
    /// Valid, but the key has been revoked.
    RevokedKey,
    /// Could not be checked for another reason (unsupported algorithm, broken packet).
    Error,
}

/// The level of trust GnuPG puts on the signer's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Trust {
    Unknown,
    Never,
    Undefined,
    Marginal,
    Full,
    Ultimate,
}

/// One signature, from the status lines.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigStatus {
    pub state: SigState,
    pub key_id: String,
    /// The fingerprint of the signing key (the primary key's, when GnuPG says it).
    pub fingerprint: String,
    /// The user id or the certificate's subject, as the status line has it.
    pub signer: String,
    pub trust: Trust,
    pub time: Option<i64>,
}

/// Read the signatures out of status lines, in order.
pub fn signatures(status: &[Status]) -> Vec<SigStatus> {
    let mut out: Vec<SigStatus> = Vec::new();
    for st in status {
        let (state, key_id, signer) = match st.keyword.as_str() {
            "GOODSIG" | "EXPKEYSIG" | "EXPSIG" | "REVKEYSIG" | "BADSIG" => {
                let (id, who) = st.args.split_once(' ').unwrap_or((&st.args, ""));
                let state = match st.keyword.as_str() {
                    "GOODSIG" => SigState::Good,
                    "EXPKEYSIG" => SigState::ExpiredKey,
                    "EXPSIG" => SigState::ExpiredSig,
                    "REVKEYSIG" => SigState::RevokedKey,
                    _ => SigState::Bad,
                };
                (state, id.to_string(), unescape(who))
            }
            "ERRSIG" => {
                let f = st.fields();
                // keyid pkalgo hashalgo class time rc fpr; rc 9 = the key is missing
                let missing = f.get(5) == Some(&"9");
                (if missing { SigState::UnknownKey } else { SigState::Error }, f.first().unwrap_or(&"").to_string(), String::new())
            }
            "VALIDSIG" => {
                if let Some(last) = out.last_mut() {
                    let f = st.fields();
                    // fpr date timestamp expire version reserved pkalgo hashalgo class [primary-fpr]
                    if let Some(fp) = f.get(9).or(f.first()) {
                        last.fingerprint = fp.to_string();
                    }
                    last.time = f.get(2).and_then(|t| parse_gpg_time(t)).or_else(|| f.get(1).and_then(|t| parse_gpg_time(t)));
                }
                continue;
            }
            "TRUST_UNDEFINED" | "TRUST_NEVER" | "TRUST_MARGINAL" | "TRUST_FULLY" | "TRUST_ULTIMATE" => {
                if let Some(last) = out.last_mut() {
                    last.trust = match st.keyword.as_str() {
                        "TRUST_NEVER" => Trust::Never,
                        "TRUST_UNDEFINED" => Trust::Undefined,
                        "TRUST_MARGINAL" => Trust::Marginal,
                        "TRUST_FULLY" => Trust::Full,
                        _ => Trust::Ultimate,
                    };
                }
                continue;
            }
            _ => continue,
        };
        out.push(SigStatus { state, fingerprint: key_id.clone(), key_id, signer, trust: Trust::Unknown, time: None });
    }
    // gpgsm does not name a signer whose certificate it does not have: `ERROR verify.findkey <code 9>`
    if out.is_empty() && status.iter().any(|s| s.keyword == "ERROR" && s.args.starts_with("verify.findkey") && s.fields().get(1).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n & 0xFFFF == 9)) {
        out.push(SigStatus { state: SigState::UnknownKey, key_id: String::new(), fingerprint: String::new(), signer: String::new(), trust: Trust::Unknown, time: None });
    }
    out
}

impl Gnupg {
    /// Verify a detached signature over `signed` (the exact bytes). The signature goes through a private temp file,
    /// the data through stdin.
    pub fn verify_detached(&self, s: CryptoStandard, signature: &[u8], signed: &[u8], cancel: &AtomicBool) -> Result<Vec<SigStatus>, GpgError> {
        let dir = TempDir::new()?;
        let sig = dir.write("signature", signature)?;
        let sig = sig.to_string_lossy().into_owned();
        let run = self.run(s, &["--verify", &sig, "-"], signed, SHORT, cancel)?;
        Ok(signatures(&run.status))
    }

    /// Verify a whole signed message (a clear-signed block, an opaque OpenPGP signature): the signatures it carries.
    pub fn verify_inline(&self, s: CryptoStandard, message: &[u8], cancel: &AtomicBool) -> Result<Vec<SigStatus>, GpgError> {
        let run = self.run(s, &["--verify"], message, SHORT, cancel)?;
        Ok(signatures(&run.status))
    }
}

impl Gnupg {
    /// An opaque signed message (S/MIME `signed-data`, where the content is inside the signature): its content and
    /// signatures, from one `--verify --output -` call. That call never uses a secret key and opens no dialog, so a
    /// message that merely *says* it is signed cannot get a decryption started without the person's consent.
    /// None: not a readable signed message.
    pub fn extract_signed(&self, s: CryptoStandard, data: &[u8], cancel: &AtomicBool) -> Result<Option<(Vec<u8>, Vec<SigStatus>)>, GpgError> {
        let run = self.run(s, &["--verify", "--output", "-"], data, SHORT, cancel)?;
        let sigs = signatures(&run.status);
        if sigs.is_empty() || run.has("DECRYPTION_FAILED") || run.has("BEGIN_DECRYPTION") {
            return Ok(None);
        }
        Ok(Some((run.stdout, sigs)))
    }
}

// ---------------------------------------------------------------- decrypting

/// Why a decryption gave nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecryptFailure {
    /// None of the secret keys fits (key ids that the message is encrypted to, when known).
    NoSecretKey(Vec<String>),
    /// The passphrase or PIN dialog was cancelled.
    Cancelled,
    BadPassphrase,
    /// The data is not an encrypted message, or is damaged.
    Corrupt,
    Other(String),
}

impl std::fmt::Display for DecryptFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecryptFailure::NoSecretKey(_) => write!(f, "This mail is encrypted for a key that is not in your keyring (no secret key fits). If it is your key, import it with “gpg --import” or “gpgsm --import”."),
            DecryptFailure::Cancelled => write!(f, "The passphrase dialog was cancelled, so the mail was not decrypted. Open it again to retry."),
            DecryptFailure::BadPassphrase => write!(f, "The passphrase was not accepted, so the mail was not decrypted. Open it again to retry."),
            DecryptFailure::Corrupt => write!(f, "The encrypted part is damaged or is not an encrypted message, so it could not be decrypted."),
            DecryptFailure::Other(e) => write!(f, "The mail could not be decrypted: {e}"),
        }
    }
}

/// The plain data and the signatures that a decryption reported.
#[derive(Clone, Debug)]
pub struct Decrypted {
    pub plain: Vec<u8>,
    pub signatures: Vec<SigStatus>,
}

/// The GnuPG error code (the low 16 bits of the number in `ERROR <where> <number>`).
fn error_code(status: &[Status]) -> Option<u32> {
    status.iter().filter(|s| s.keyword == "ERROR").filter_map(|s| s.fields().get(1).and_then(|n| n.parse::<u32>().ok())).map(|n| n & 0xFFFF).next_back()
}

impl Gnupg {
    /// Decrypt. Plain data is returned only if GnuPG said `DECRYPTION_OKAY` (integrity checked) and not
    /// `DECRYPTION_FAILED`; everything else is a [`DecryptFailure`].
    pub fn decrypt(&self, s: CryptoStandard, data: &[u8], cancel: &AtomicBool) -> Result<Result<Decrypted, DecryptFailure>, GpgError> {
        let run = self.run(s, &["--decrypt"], data, DIALOG, cancel)?;
        if run.has("DECRYPTION_OKAY") && !run.has("DECRYPTION_FAILED") {
            return Ok(Ok(Decrypted { plain: run.stdout, signatures: signatures(&run.status) }));
        }
        let missing: Vec<String> = run.status.iter().filter(|s| s.keyword == "NO_SECKEY").filter_map(|s| s.fields().first().map(|f| f.to_string())).collect();
        let code = error_code(&run.status);
        let fail = if code == Some(99) || run.status.iter().any(|s| s.keyword == "ERROR" && s.args.contains("cancel")) {
            DecryptFailure::Cancelled
        } else if run.has("BAD_PASSPHRASE") || code == Some(11) {
            DecryptFailure::BadPassphrase
        } else if !missing.is_empty() || code == Some(17) {
            DecryptFailure::NoSecretKey(missing)
        } else if run.has("NODATA") || code == Some(58) || run.has("DECRYPTION_FAILED") {
            DecryptFailure::Corrupt
        } else {
            DecryptFailure::Other(run.status.iter().find(|s| s.keyword == "ERROR" || s.keyword == "FAILURE").map(|s| format!("{} {}", s.keyword, s.args)).unwrap_or_else(|| format!("exit code {}", run.code)))
        };
        Ok(Err(fail))
    }
}

// ---------------------------------------------------------------- a private temp folder

/// A folder only the owner can enter, removed when dropped (for the signature file of a verification).
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Result<TempDir, GpgError> {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("shale-gpg-{}-{n}-{nanos}", std::process::id()));
        crate::fsutil::ensure_private_dir(&path).map_err(|e| GpgError::Io(e.to_string()))?;
        Ok(TempDir { path })
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, GpgError> {
        let p = self.path.join(name);
        crate::fsutil::write_atomic_private(&p, bytes).map_err(|e| GpgError::Io(e.to_string()))?;
        Ok(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
pub(crate) mod testkit {
    //! A throw-away `GNUPGHOME` with generated keys, for tests. The keys have no passphrase and are made with the
    //! loopback pinentry, which only the tests use (ADR-0011 §5). S/MIME certificates are self-signed and listed in
    //! `trustlist.txt` as trusted roots: `gpgsm` cannot issue certificates.

    use super::*;
    use std::sync::atomic::AtomicUsize;

    static COUNT: AtomicUsize = AtomicUsize::new(0);

    pub struct TestHome {
        pub dir: PathBuf,
        pub g: Gnupg,
    }

    pub fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = Command::new("gpgconf").arg("--homedir").arg(&self.dir).args(["--kill", "gpg-agent"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            let _ = Command::new("gpgconf").arg("--homedir").arg(&self.dir).arg("--remove-socketdir").stdout(Stdio::null()).stderr(Stdio::null()).status();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Are `gpg`, `gpgsm` and `gpgconf` here? Tests that need them say so and pass when they are not (the CI image has them).
    pub fn have_gnupg() -> bool {
        ["gpg", "gpgsm", "gpgconf"].iter().all(|p| Command::new(p).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok())
    }

    impl TestHome {
        pub fn new() -> TestHome {
            // short, so that the agent's socket path fits when it has to live in the home
            let dir = std::path::PathBuf::from("/tmp").join(format!("shg{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&dir);
            crate::fsutil::ensure_private_dir(&dir).expect("temp home");
            std::fs::write(dir.join("gpg-agent.conf"), "allow-loopback-pinentry\n").expect("agent conf");
            std::fs::write(dir.join("gpgsm.conf"), "disable-crl-checks\n").expect("gpgsm conf");
            TestHome { g: Gnupg { home: Some(dir.clone()), ..Gnupg::default() }, dir }
        }

        fn cmd(&self, program: &str) -> Command {
            let mut c = Command::new(program);
            c.env("GNUPGHOME", &self.dir).stdin(Stdio::null());
            c
        }

        /// A new unprotected OpenPGP key (ed25519 + cv25519) for `Name <email>`; its fingerprint. `expire` is gpg's
        /// expiration (`never`, `1d`, …) and `faked_time` the time gpg thinks it is while it makes the key.
        pub fn gen_pgp(&self, name: &str, email: &str, expire: &str, faked_time: Option<&str>) -> String {
            let uid = format!("{name} <{email}>");
            let mut c = self.cmd("gpg");
            c.args(["--batch", "--pinentry-mode", "loopback", "--passphrase", ""]);
            if let Some(t) = faked_time {
                c.args(["--faked-system-time", t]);
            }
            c.args(["--quick-gen-key", &uid, "default", "default", expire]);
            let out = c.output().expect("gpg runs");
            assert!(out.status.success(), "gpg --quick-gen-key: {}", String::from_utf8_lossy(&out.stderr));
            self.g.list_keys(CryptoStandard::OpenPgp, true, Some(email), &no_cancel()).unwrap().into_iter().next().expect("the new key").fingerprint
        }

        /// A new unprotected, self-signed S/MIME certificate with its key (RSA 2048), trusted as a root; its fingerprint.
        pub fn gen_smime(&self, cn: &str, email: &str) -> String {
            let params = format!("Key-Type: RSA\nKey-Length: 2048\nKey-Usage: sign, encrypt\nSerial: random\nName-DN: CN={cn},O=Shale Test\nName-Email: {email}\nNot-After: 2031-01-01\n");
            let pf = self.dir.join(format!("gen-{}.txt", COUNT.fetch_add(1, Ordering::SeqCst)));
            std::fs::write(&pf, params).unwrap();
            let out = self.cmd("gpgsm").args(["--batch", "--pinentry-mode", "loopback", "--passphrase", "", "--gen-key", "--armor", "-o"]).arg(self.dir.join("new.crt")).arg(&pf).output().expect("gpgsm runs");
            assert!(out.status.success(), "gpgsm --gen-key: {}", String::from_utf8_lossy(&out.stderr));
            let cert = std::fs::read(self.dir.join("new.crt")).unwrap();
            self.import_smime(&cert);
            let fpr = self.g.list_keys(CryptoStandard::Smime, true, Some(email), &no_cancel()).unwrap().into_iter().next().expect("the new certificate").fingerprint;
            self.trust_smime(&fpr);
            fpr
        }

        pub fn import_smime(&self, cert: &[u8]) {
            let f = self.dir.join("in.crt");
            std::fs::write(&f, cert).unwrap();
            let out = self.cmd("gpgsm").args(["--batch", "--import"]).arg(&f).output().expect("gpgsm runs");
            assert!(out.status.success(), "gpgsm --import: {}", String::from_utf8_lossy(&out.stderr));
        }

        /// Make a (self-signed) certificate a trusted root.
        pub fn trust_smime(&self, fpr: &str) {
            let mut t = std::fs::read_to_string(self.dir.join("trustlist.txt")).unwrap_or_default();
            t.push_str(&format!("{fpr} S relax\n"));
            std::fs::write(self.dir.join("trustlist.txt"), t).unwrap();
        }

        pub fn export_smime(&self, fpr: &str) -> Vec<u8> {
            let out = self.cmd("gpgsm").args(["--batch", "--armor", "--export", fpr]).output().expect("gpgsm runs");
            assert!(out.status.success());
            out.stdout
        }

        pub fn export_pgp(&self, fpr: &str) -> Vec<u8> {
            let out = self.cmd("gpg").args(["--batch", "--armor", "--export", fpr]).output().expect("gpg runs");
            assert!(out.status.success());
            out.stdout
        }

        /// Run a program of this home with `input` on stdin; its stdout. (Test producers use the programs directly, not
        /// Shale's own code, so what Shale reads is made by somebody else.)
        fn pipe(&self, program: &str, args: &[&str], input: &[u8]) -> Vec<u8> {
            let mut c = self.cmd(program).arg("--batch").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("program runs");
            let mut stdin = c.stdin.take().unwrap();
            let data = input.to_vec();
            let w = std::thread::spawn(move || {
                let _ = stdin.write_all(&data);
            });
            let out = c.wait_with_output().unwrap();
            let _ = w.join();
            assert!(out.status.success(), "{program} {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            out.stdout
        }

        /// An armored detached OpenPGP signature over `data`, made at `faked_time` if given.
        pub fn pgp_sign(&self, fpr: &str, data: &[u8], faked_time: Option<&str>) -> Vec<u8> {
            let mut args = vec!["--armor", "--detach-sign", "-u", fpr];
            if let Some(t) = faked_time {
                args.extend(["--faked-system-time", t]);
            }
            self.pipe("gpg", &args, data)
        }

        /// An armored OpenPGP message for `to`, signed by `sign_with` in the same operation if given.
        pub fn pgp_encrypt(&self, to: &[&str], sign_with: Option<&str>, data: &[u8]) -> Vec<u8> {
            let mut args = vec!["--armor", "--trust-model", "always", "--encrypt"];
            for r in to {
                args.extend(["-r", r]);
            }
            if let Some(k) = sign_with {
                args.extend(["--sign", "-u", k]);
            }
            self.pipe("gpg", &args, data)
        }

        /// A clear-signed OpenPGP text.
        pub fn pgp_clearsign(&self, fpr: &str, text: &str) -> Vec<u8> {
            self.pipe("gpg", &["--armor", "--clearsign", "-u", fpr], text.as_bytes())
        }

        /// Revoke an OpenPGP key of this home with the certificate gpg stored when it was made.
        pub fn revoke_pgp(&self, fpr: &str) {
            let cert = std::fs::read_to_string(self.dir.join("openpgp-revocs.d").join(format!("{fpr}.rev"))).expect("revocation certificate");
            let cleaned: Vec<&str> = cert.lines().map(|l| l.strip_prefix(':').unwrap_or(l)).collect();
            self.import_pgp(cleaned.join("\n").as_bytes());
        }

        /// A detached S/MIME signature (DER) over `data`.
        pub fn smime_sign(&self, fpr: &str, data: &[u8]) -> Vec<u8> {
            self.pipe("gpgsm", &["--detach-sign", "-u", fpr], data)
        }

        /// An opaque S/MIME signed-data (DER): the content is inside.
        pub fn smime_sign_opaque(&self, fpr: &str, data: &[u8]) -> Vec<u8> {
            self.pipe("gpgsm", &["--sign", "-u", fpr], data)
        }

        /// S/MIME enveloped-data (DER) for `to`.
        pub fn smime_encrypt(&self, to: &[&str], data: &[u8]) -> Vec<u8> {
            let mut args = vec!["--encrypt"];
            for r in to {
                args.extend(["-r", r]);
            }
            self.pipe("gpgsm", &args, data)
        }

        pub fn import_pgp(&self, key: &[u8]) {
            let mut c = self.cmd("gpg").args(["--batch", "--import"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("gpg runs");
            c.stdin.take().unwrap().write_all(key).unwrap();
            assert!(c.wait().unwrap().success());
        }

        /// Give another home the public OpenPGP key `fpr` of this one.
        pub fn share_pgp_with(&self, other: &TestHome, fpr: &str) {
            other.import_pgp(&self.export_pgp(fpr));
        }

        /// Give another home the public certificate `fpr` of this one, trusted.
        pub fn share_smime_with(&self, other: &TestHome, fpr: &str) {
            other.import_smime(&self.export_smime(fpr));
            other.trust_smime(fpr);
        }
    }

    use std::io::Write;
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    // spec: MAIL-170
    #[test]
    fn status_lines_are_taken_from_gnupg_lines_only_and_user_ids_are_unescaped() {
        let err = b"gpg: Good signature from \"Mallory <m@x>\"\n[GNUPG:] NEWSIG\n[GNUPG:] GOODSIG 0123456789ABCDEF Ada \\x3cada@x.org\\x3e\n  [GNUPG:] GOODSIG fake fake\n[GNUPG:] VALIDSIG AAAA 2026-11-14 1794657600 0 4 0 22 8 00 BBBB\n[GNUPG:] TRUST_FULLY 0 pgp\nnot [GNUPG:] BADSIG x y\n[GNUPG:] lowercase nonsense\n";
        let st = parse_status(err);
        let keys: Vec<&str> = st.iter().map(|s| s.keyword.as_str()).collect();
        assert_eq!(keys, vec!["NEWSIG", "GOODSIG", "VALIDSIG", "TRUST_FULLY"], "only real status lines at the start of a line");
        let sigs = signatures(&st);
        assert_eq!(sigs.len(), 1);
        assert_eq!((sigs[0].state, sigs[0].signer.as_str(), sigs[0].fingerprint.as_str(), sigs[0].trust, sigs[0].time), (SigState::Good, "Ada <ada@x.org>", "BBBB", Trust::Full, Some(1_794_657_600)));
        assert_eq!(unescape("a\\x2fb\\\\c\\x"), "a/b\\c\\x");
    }

    // spec: MAIL-170
    #[test]
    fn only_hex_fingerprints_and_plain_addresses_reach_the_command_line() {
        assert!(is_key_id("0123456789abcdefABCDEF0123456789abcdef01"));
        for bad in ["", "--version", "zz123456", "1234567", "abc def12", "0123456789abcdef\n"] {
            assert!(!is_key_id(bad), "{bad:?}");
        }
        assert!(is_lookup_address("ada@example.org"));
        for bad in ["--export-secret-keys@x", "a b@x", "a@x\n", "<a@x>", "noat", "*@x", ""] {
            assert!(!is_lookup_address(bad), "{bad:?}");
        }
        let g = Gnupg::default();
        assert!(matches!(g.list_keys(CryptoStandard::OpenPgp, false, Some("--no-options@x"), &no_cancel()), Err(GpgError::BadArgument(_))));
        assert!(matches!(g.key_info(CryptoStandard::OpenPgp, "--help", &no_cancel()), Err(GpgError::BadArgument(_))));
    }

    // spec: MAIL-170
    #[test]
    fn a_missing_program_is_reported_in_words() {
        let g = Gnupg { gpg: "/nonexistent/gpg-for-shale-tests".into(), gpgsm: "/nonexistent/gpgsm-for-shale-tests".into(), home: None };
        let e = g.list_keys(CryptoStandard::OpenPgp, false, None, &no_cancel()).unwrap_err();
        assert_eq!(e, GpgError::Missing(CryptoStandard::OpenPgp));
        assert!(e.to_string().contains("not installed"), "{e}");
        assert!(matches!(g.version(CryptoStandard::Smime), Err(GpgError::Missing(CryptoStandard::Smime))));
    }

    // spec: MAIL-170
    #[test]
    fn a_call_never_asks_for_a_network_lookup_and_passes_its_arguments_as_a_list() {
        let dir = TempDir::new().unwrap();
        // a stand-in for gpg that records how it was called
        let log = dir.write("args.txt", b"").unwrap();
        let script = dir.write("fake-gpg", format!("#!/bin/sh\nfor a in \"$@\"; do echo \"$a\" >> '{}'; done\necho \"stdin:$(cat)\" >> '{}'\n", log.display(), log.display()).as_bytes()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let g = Gnupg { gpg: script.to_string_lossy().into_owned(), home: Some(dir.path.clone()), ..Gnupg::default() };
        g.run(CryptoStandard::OpenPgp, &["--decrypt"], b"cipher; rm -rf / $(evil)", SHORT, &no_cancel()).unwrap();
        let seen = std::fs::read_to_string(&log).unwrap();
        let args: Vec<&str> = seen.lines().collect();
        for must in ["--batch", "--status-fd", "2", "--no-auto-key-retrieve", "--no-auto-key-locate", "--decrypt"] {
            assert!(args.contains(&must), "{must} missing in {args:?}");
        }
        assert!(args.iter().all(|a| !a.contains("keyserver") && !a.contains("auto-key-locate=")), "{args:?}");
        assert!(seen.contains("stdin:cipher; rm -rf / $(evil)"), "data goes on stdin, never into a shell: {seen}");
    }

    // spec: MAIL-171
    #[test]
    fn a_call_that_hangs_is_killed_at_its_time_limit_and_by_cancel_without_blocking_forever() {
        let dir = TempDir::new().unwrap();
        // a stand-in for gpg waiting for a passphrase dialog
        let script = dir.write("slow-gpg", b"#!/bin/sh\nsleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let g = Gnupg { gpg: script.to_string_lossy().into_owned(), ..Gnupg::default() };
        let t = Instant::now();
        let r = g.run(CryptoStandard::OpenPgp, &["--decrypt"], b"data", Duration::from_millis(300), &no_cancel());
        assert_eq!(r.unwrap_err(), GpgError::Timeout);
        assert!(t.elapsed() < Duration::from_secs(5));
        // cancel from another thread
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c2.store(true, Ordering::SeqCst);
        });
        let t = Instant::now();
        let r = g.run(CryptoStandard::OpenPgp, &["--decrypt"], b"data", DIALOG, &cancel);
        assert_eq!(r.unwrap_err(), GpgError::Cancelled);
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    // spec: MAIL-170
    #[test]
    fn keys_are_listed_from_the_colon_output_for_both_standards() {
        if !have_gnupg() {
            return;
        }
        let h = TestHome::new();
        let pgp = h.gen_pgp("Alice Test", "alice@example.org", "never", None);
        let sm = h.gen_smime("Alice S/MIME", "alice@example.org");
        let secret = h.g.list_keys(CryptoStandard::OpenPgp, true, None, &no_cancel()).unwrap();
        assert_eq!(secret.len(), 1);
        assert_eq!((secret[0].fingerprint.as_str(), secret[0].secret, secret[0].can_encrypt, secret[0].can_sign, secret[0].usable()), (pgp.as_str(), true, true, true, true));
        assert_eq!(secret[0].emails, vec!["alice@example.org"]);
        assert!(secret[0].uids[0].starts_with("Alice Test"));
        let certs = h.g.list_keys(CryptoStandard::Smime, true, None, &no_cancel()).unwrap();
        assert_eq!(certs.len(), 1);
        assert_eq!(certs[0].fingerprint, sm);
        assert!(certs[0].can_encrypt && certs[0].emails.contains(&"alice@example.org".to_string()), "{:?}", certs[0]);
        // a lookup by address: found, and not found is an empty list
        assert_eq!(h.g.list_keys(CryptoStandard::OpenPgp, false, Some("ALICE@example.org"), &no_cancel()).unwrap().len(), 1);
        assert!(h.g.list_keys(CryptoStandard::OpenPgp, false, Some("nobody@example.org"), &no_cancel()).unwrap().is_empty());
        assert!(h.g.list_keys(CryptoStandard::Smime, false, Some("nobody@example.org"), &no_cancel()).unwrap().is_empty());
        assert_eq!(h.g.key_info(CryptoStandard::OpenPgp, &pgp, &no_cancel()).unwrap().unwrap().fingerprint, pgp);
        assert!(h.g.version(CryptoStandard::OpenPgp).unwrap().contains("gpg"));
    }
}
