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

impl Gnupg {
    /// The machine's `gpg` and `gpgsm` against the person's own GnuPG home. Only the app itself
    /// (through `Default`) and the test kit's `TestHome` (with a home of its own) call it.
    pub fn system() -> Self {
        Gnupg { home: None, gpg: "gpg".into(), gpgsm: "gpgsm".into() }
    }
}

impl Default for Gnupg {
    /// The machine's programs; in a test build programs that do not exist, so a test never starts
    /// the real GnuPG against the person's `~/.gnupg` (DEV-017).
    fn default() -> Self {
        #[cfg(test)]
        {
            Gnupg { home: None, gpg: "/nonexistent/gpg".into(), gpgsm: "/nonexistent/gpgsm".into() }
        }
        #[cfg(not(test))]
        {
            Gnupg::system()
        }
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

/// A `GNUPGHOME` value. On Windows with forward slashes: the GnuPG that comes with Git for Windows
/// (an MSYS program) reads a backslash path as a relative one, and Gpg4win's accepts both.
fn home_text(p: &std::path::Path) -> std::ffi::OsString {
    if cfg!(windows) {
        p.to_string_lossy().replace('\\', "/").into()
    } else {
        p.as_os_str().to_owned()
    }
}

/// Where to get GnuPG on this system (BUILD-018): macOS and Windows do not come with it.
fn install_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Install GnuPG first, for example with Homebrew: brew install gnupg"
    } else if cfg!(windows) {
        "Install GnuPG first, for example Gpg4win from https://www.gpg4win.org, and restart Shale."
    } else {
        "Install the gnupg package of your system first."
    }
}

impl std::fmt::Display for GpgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpgError::Missing(s) => write!(f, "{} is not installed (the program “{}” was not found), so {} cannot be used. {}", s.program(), s.program(), s.label(), install_hint()),
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
            cmd.env("GNUPGHOME", home_text(h));
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = crate::program::spawn(&mut cmd).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { GpgError::Missing(s) } else { GpgError::Io(e.to_string()) })?;
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
        let mut cmd = Command::new(self.program(s));
        if let Some(h) = &self.home {
            cmd.env("GNUPGHOME", home_text(h));
        }
        cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        let failed = |e: std::io::Error| if e.kind() == std::io::ErrorKind::NotFound { GpgError::Missing(s) } else { GpgError::Io(e.to_string()) };
        let out = crate::program::spawn(&mut cmd).map_err(failed)?.wait_with_output().map_err(|e| GpgError::Io(e.to_string()))?;
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text.lines().next().unwrap_or("").trim().to_string())
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
    /// Words for the trust level of a key found for a recipient.
    pub fn trust_words(&self) -> &'static str {
        match self.validity {
            'u' => "yours",
            'f' => "trusted",
            'm' => "marginally trusted",
            'e' => "expired",
            'r' => "revoked",
            'd' => "disabled",
            'i' => "invalid",
            'n' => "not trusted",
            _ => "not certified",
        }
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
        let days = crate::calendar::time::days_from_civil(n(0, 4)? as i32, n(4, 6)? as u32, n(6, 8)? as u32);
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
                        if !k.uids.contains(&u) {
                            k.uids.push(u);
                        }
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
                    // gpgsm says a revoked certificate as GOODSIG, then `TRUST_NEVER 94` (GPG_ERR_CERT_REVOKED): the
                    // signature is mathematically good, the certificate no longer counts
                    if st.keyword == "TRUST_NEVER" && last.state == SigState::Good && st.fields().first().and_then(|c| c.parse::<u32>().ok()).is_some_and(|c| c & 0xFFFF == 94) {
                        last.state = SigState::RevokedKey;
                    }
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

/// What an opaque signed message holds: its content and the signatures on it.
type SignedContent = (Vec<u8>, Vec<SigStatus>);

impl Gnupg {
    /// An opaque signed message (S/MIME `signed-data`, where the content is inside the signature): its content and
    /// signatures, from one `--verify --output -` call. That call never uses a secret key and opens no dialog, so a
    /// message that merely *says* it is signed cannot get a decryption started without the person's consent.
    /// None: not a readable signed message.
    pub fn extract_signed(&self, s: CryptoStandard, data: &[u8], cancel: &AtomicBool) -> Result<Option<SignedContent>, GpgError> {
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
    /// `gpgsm` only says that it failed: no certificate of yours fits it, or the data is damaged.
    NoKeyOrDamaged,
    /// The data is not an encrypted message, or is damaged.
    Corrupt,
    Other(String),
}

impl std::fmt::Display for DecryptFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecryptFailure::NoSecretKey(_) => write!(f, "This mail is encrypted for a key that is not in your keyring (no secret key fits). If it is your key, import it with “gpg --import” or “gpgsm --import”."),
            DecryptFailure::Cancelled => write!(f, "The passphrase dialog was cancelled, so the mail was not decrypted. Open it again to retry."),
            DecryptFailure::NoKeyOrDamaged => {
                write!(f, "This mail could not be decrypted: none of your certificates fits it (it may be encrypted for another address), or the data is damaged. If it is yours, import the certificate with “gpgsm --import”.")
            }
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
        Ok(Err(decrypt_failure(&run, s)))
    }
}

/// Why a decryption gave nothing, from its status lines.
fn decrypt_failure(run: &Run, s: CryptoStandard) -> DecryptFailure {
    let missing: Vec<String> = run.status.iter().filter(|s| s.keyword == "NO_SECKEY").filter_map(|s| s.fields().first().map(|f| f.to_string())).collect();
    let code = error_code(&run.status);
    if code == Some(99) || run.status.iter().any(|s| s.keyword == "ERROR" && s.args.contains("cancel")) {
        DecryptFailure::Cancelled
    } else if run.has("BAD_PASSPHRASE") || code == Some(11) {
        DecryptFailure::BadPassphrase
    } else if !missing.is_empty() || code == Some(17) {
        DecryptFailure::NoSecretKey(missing)
    } else if run.has("NODATA") || code == Some(58) {
        DecryptFailure::Corrupt
    } else if s == CryptoStandard::Smime && run.has("DECRYPTION_FAILED") && code.is_none() {
        DecryptFailure::NoKeyOrDamaged
    } else if run.has("DECRYPTION_FAILED") {
        DecryptFailure::Corrupt
    } else {
        DecryptFailure::Other(run.status.iter().find(|s| s.keyword == "ERROR" || s.keyword == "FAILURE").map(|s| format!("{} {}", s.keyword, s.args)).unwrap_or_else(|| format!("exit code {}", run.code)))
    }
}

// ---------------------------------------------------------------- signing and encrypting

/// Why `INV_RECP` refused a recipient (the reason numbers of GnuPG).
fn inv_recp_words(reason: &str) -> &'static str {
    match reason {
        // what gpg says for an expired, revoked or disabled key (it gives no better reason)
        "0" => "the key cannot be used (it has expired, been revoked or been disabled)",
        "1" => "no such key in your keyring",
        "2" => "the name matches several keys",
        "3" => "the key may not be used for encryption",
        "4" => "the key has been revoked",
        "5" => "the key has expired",
        "6" | "7" => "no current revocation list is available for its certificate",
        "8" => "it does not meet the policy",
        "10" => "the key is not trusted",
        "11" | "12" => "the certificate chain is incomplete (a certificate is missing)",
        _ => "GnuPG refused the key",
    }
}

/// What went wrong in a signing or encrypting call, in words, from its status lines.
fn op_failure(run: &Run, doing: &str) -> String {
    let code = error_code(&run.status);
    // a signing call whose dialog is cancelled or whose passphrase is wrong says `FAILURE sign <number>`, not `ERROR`
    let failed_with = |c: u32| run.status.iter().filter(|s| s.keyword == "FAILURE").any(|s| s.fields().get(1).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n & 0xFFFF == c));
    if code == Some(99) || failed_with(99) {
        return format!("The passphrase dialog was cancelled, so the mail was not {doing}.");
    }
    if run.has("BAD_PASSPHRASE") || code == Some(11) || failed_with(11) {
        return format!("The passphrase was not accepted, so the mail was not {doing}.");
    }
    let refused: Vec<String> = run
        .status
        .iter()
        .filter(|s| s.keyword == "INV_RECP")
        .map(|s| {
            let (reason, who) = s.args.split_once(' ').unwrap_or((&s.args, ""));
            format!("{}: {}", unescape(who), inv_recp_words(reason))
        })
        .collect();
    if !refused.is_empty() {
        return format!("The mail was not {doing}: {}.", refused.join("; "));
    }
    if run.has("INV_SGNR") || run.has("NO_SECKEY") || code == Some(17) {
        return format!("The mail was not {doing}: the key chosen for this account has no secret key here (or has expired). Choose another in the account settings (Encryption).");
    }
    let detail = run.status.iter().find(|s| s.keyword == "ERROR" || s.keyword == "FAILURE").map(|s| format!(" ({} {})", s.keyword, s.args)).unwrap_or_default();
    format!("GnuPG could not do it, so the mail was not {doing}{detail}.")
}

impl Gnupg {
    /// A detached signature of `data` made by the key `fingerprint` (armored text for OpenPGP, DER for S/MIME). SHA-256 is
    /// asked for, so the `micalg` of the mail can say so. The passphrase dialog, if one is needed, is GnuPG's.
    pub fn sign_detached(&self, s: CryptoStandard, fingerprint: &str, data: &[u8], cancel: &AtomicBool) -> Result<Vec<u8>, String> {
        if !is_key_id(fingerprint) {
            return Err(format!("{fingerprint:?} is not a key fingerprint"));
        }
        let args: Vec<&str> = match s {
            CryptoStandard::OpenPgp => vec!["--armor", "--detach-sign", "--digest-algo", "SHA256", "-u", fingerprint],
            CryptoStandard::Smime => vec!["--detach-sign", "--digest-algo", "sha256", "-u", fingerprint],
        };
        let run = self.run(s, &args, data, DIALOG, cancel).map_err(|e| e.to_string())?;
        if run.has("SIG_CREATED") && run.code == 0 && !run.stdout.is_empty() {
            Ok(run.stdout)
        } else {
            Err(op_failure(&run, "signed"))
        }
    }

    /// `data` encrypted to the keys `to` (fingerprint and whether it is a hidden recipient: OpenPGP puts no key id for it
    /// in the message), and signed in the same operation by `sign_with` when given (OpenPGP only; for S/MIME sign first).
    /// Armored text for OpenPGP, DER for S/MIME. Only keys of the person's own keyring are used: they chose them.
    pub fn encrypt(&self, s: CryptoStandard, to: &[(String, bool)], sign_with: Option<&str>, data: &[u8], cancel: &AtomicBool) -> Result<Vec<u8>, String> {
        if to.is_empty() {
            return Err("There is nobody to encrypt for.".into());
        }
        if let Some(bad) = to.iter().map(|t| t.0.as_str()).chain(sign_with).find(|f| !is_key_id(f)) {
            return Err(format!("{bad:?} is not a key fingerprint"));
        }
        let mut args: Vec<&str> = match s {
            CryptoStandard::OpenPgp => vec!["--armor", "--trust-model", "always", "--encrypt"],
            CryptoStandard::Smime => vec!["--encrypt"],
        };
        for (fpr, hidden) in to {
            args.push(if *hidden && s == CryptoStandard::OpenPgp { "-R" } else { "-r" });
            args.push(fpr);
        }
        if let (Some(k), CryptoStandard::OpenPgp) = (sign_with, s) {
            args.extend(["--sign", "--digest-algo", "SHA256", "-u", k]);
        }
        let run = self.run(s, &args, data, DIALOG, cancel).map_err(|e| e.to_string())?;
        // gpgsm says nothing at the end of an encryption (gpg says END_ENCRYPTION): its success is the exit code, output and no refusal
        let finished = s == CryptoStandard::Smime || run.has("END_ENCRYPTION");
        if finished && !run.has("INV_RECP") && !run.has("ERROR") && !run.has("FAILURE") && run.code == 0 && !run.stdout.is_empty() {
            Ok(run.stdout)
        } else {
            Err(op_failure(&run, "encrypted"))
        }
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
        let root = crate::fsutil::private_temp_root("shale-gpg").map_err(|e| GpgError::Io(e.to_string()))?;
        let path = root.join(format!("{}-{n}-{nanos}", std::process::id()));
        crate::fsutil::create_private_dir_new(&path).map_err(|e| GpgError::Io(e.to_string()))?;
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
    //! `trustlist.txt` as trusted roots: `gpgsm` cannot issue certificates. A CA, its certificates and its CRL (for revoked ones) come from `openssl` ([`TestCa`]), so no PKCS#12 file is ever imported.

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

    /// Is `openssl` here? Tests that need a CA skip themselves with a note when it is not.
    pub fn have_openssl() -> bool {
        let have = Command::new("openssl").arg("version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if !have {
            eprintln!("skipping: openssl is not installed here");
        }
        have
    }

    /// A throw-away certificate authority made with `openssl`, for S/MIME certificates that chain to a root and can be
    /// revoked. Only public certificates and the CRL go into a home (`gpgsm` imports no private key from it), and
    /// signatures are made with `openssl cms`, so nothing here depends on a PKCS#12 import (which `gpgsm` cannot do
    /// from OpenSSL 4 files). Never reaches the network: the home asks `dirmngr` for the CRL it was given.
    pub struct TestCa {
        dir: PathBuf,
    }

    /// A certificate issued by a [`TestCa`], with its key in the CA's folder; imported into a home as a public certificate.
    pub struct CaCert {
        /// The fingerprint `gpgsm` knows it by.
        pub fpr: String,
        crt: PathBuf,
        key: PathBuf,
    }

    impl TestCa {
        fn openssl(&self, args: &[&str]) -> Vec<u8> {
            let out = Command::new("openssl").args(args).current_dir(&self.dir).stdin(Stdio::null()).output().expect("openssl runs");
            assert!(out.status.success(), "openssl {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            out.stdout
        }

        /// A new CA called `cn`, imported into `home` as a trusted root. The home is set to check CRLs, by the issuer
        /// (the test certificates have no distribution point) and offline; the CA's (still empty) CRL is loaded.
        pub fn new(home: &TestHome, cn: &str) -> TestCa {
            let dir = home.dir.join(format!("ca-{}", COUNT.fetch_add(1, Ordering::SeqCst)));
            std::fs::create_dir(&dir).unwrap();
            let conf = "[ca]\ndefault_ca=c\n[c]\ndatabase=index.txt\nnew_certs_dir=.\nserial=serial\ncrlnumber=crlnumber\ndefault_md=sha256\ndefault_days=3650\ndefault_crl_days=3650\npolicy=p\nunique_subject=no\n[p]\ncommonName=supplied\norganizationName=optional\n[req]\ndistinguished_name=d\nprompt=no\n[d]\nCN=x\n[v3_ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,cRLSign\nsubjectKeyIdentifier=hash\n";
            for (name, text) in [("ca.cnf", conf), ("index.txt", ""), ("serial", "1000\n"), ("crlnumber", "01\n")] {
                std::fs::write(dir.join(name), text).unwrap();
            }
            let ca = TestCa { dir };
            ca.openssl(&["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.crt", "-config", "ca.cnf", "-extensions", "v3_ca", "-days", "3650", "-subj", &format!("/CN={cn}/O=Shale Test")]);
            // gpgsm.conf of a test home says `disable-crl-checks`; a CA needs the CRL looked at, and only the cached one
            std::fs::write(home.dir.join("gpgsm.conf"), "enable-issuer-based-crl-check\n").unwrap();
            std::fs::write(home.dir.join("dirmngr.conf"), "disable-http\ndisable-ldap\n").unwrap();
            let crt = std::fs::read(ca.dir.join("ca.crt")).unwrap();
            home.import_smime(&crt);
            let fpr = home.g.list_keys(CryptoStandard::Smime, false, None, &no_cancel()).unwrap().into_iter().find(|k| k.uids.iter().any(|u| u.contains(cn))).expect("the CA certificate").fingerprint;
            home.trust_smime(&fpr);
            ca.publish_crl(home);
            ca
        }

        /// A certificate for `CN=cn` with `email`, signed by this CA, imported into `home` (the public part only).
        pub fn issue(&self, home: &TestHome, cn: &str, email: &str) -> CaCert {
            let n = COUNT.fetch_add(1, Ordering::SeqCst);
            let (key, csr, crt) = (format!("u{n}.key"), format!("u{n}.csr"), format!("u{n}.crt"));
            self.openssl(&["req", "-newkey", "rsa:2048", "-nodes", "-keyout", &key, "-out", &csr, "-subj", &format!("/CN={cn}/O=Shale Test/emailAddress={email}")]);
            let ext = format!("basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nsubjectAltName=email:{email}\nauthorityKeyIdentifier=keyid\n");
            std::fs::write(self.dir.join(format!("u{n}.ext")), ext).unwrap();
            self.openssl(&["ca", "-batch", "-config", "ca.cnf", "-cert", "ca.crt", "-keyfile", "ca.key", "-in", &csr, "-out", &crt, "-extfile", &format!("u{n}.ext"), "-notext"]);
            home.import_smime(&std::fs::read(self.dir.join(&crt)).unwrap());
            let fpr = home.g.list_keys(CryptoStandard::Smime, false, Some(email), &no_cancel()).unwrap().into_iter().next().expect("the issued certificate").fingerprint;
            CaCert { fpr, crt: self.dir.join(crt), key: self.dir.join(key) }
        }

        /// Revoke `cert` and give `home` the new CRL.
        pub fn revoke(&self, home: &TestHome, cert: &CaCert) {
            let crt = cert.crt.file_name().unwrap().to_string_lossy().into_owned();
            self.openssl(&["ca", "-config", "ca.cnf", "-cert", "ca.crt", "-keyfile", "ca.key", "-revoke", &crt]);
            self.publish_crl(home);
        }

        /// Make a CRL of what is revoked so far and load it into the `dirmngr` of `home`.
        fn publish_crl(&self, home: &TestHome) {
            self.openssl(&["ca", "-config", "ca.cnf", "-cert", "ca.crt", "-keyfile", "ca.key", "-gencrl", "-out", "ca.crl"]);
            self.openssl(&["crl", "-in", "ca.crl", "-outform", "DER", "-out", "ca.crl.der"]);
            let out = home.cmd("gpgsm").args(["--batch", "--call-dirmngr", "loadcrl"]).arg(self.dir.join("ca.crl.der")).output().expect("gpgsm runs");
            assert!(out.status.success(), "gpgsm --call-dirmngr loadcrl: {}", String::from_utf8_lossy(&out.stderr));
        }
    }

    impl CaCert {
        /// A detached S/MIME signature (DER) over `data` by this certificate, made by `openssl cms`; the CA's
        /// certificate goes along, as a mail client would send it.
        pub fn sign(&self, ca: &TestCa, data: &[u8]) -> Vec<u8> {
            std::fs::write(ca.dir.join("data.bin"), data).unwrap();
            let (crt, key) = (self.crt.to_string_lossy().into_owned(), self.key.to_string_lossy().into_owned());
            ca.openssl(&["cms", "-sign", "-binary", "-in", "data.bin", "-signer", &crt, "-inkey", &key, "-certfile", "ca.crt", "-outform", "DER", "-md", "sha256"])
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = Command::new("gpgconf").arg("--homedir").arg(home_text(&self.dir)).args(["--kill", "gpg-agent"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            let _ = Command::new("gpgconf").arg("--homedir").arg(home_text(&self.dir)).args(["--kill", "dirmngr"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            let _ = Command::new("gpgconf").arg("--homedir").arg(home_text(&self.dir)).arg("--remove-socketdir").stdout(Stdio::null()).stderr(Stdio::null()).status();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Are `gpg`, `gpgsm` and `gpgconf` here? Tests that need them skip themselves with a note when they are not (the
    /// Linux CI image has them; macOS and Windows usually do not, BUILD-018).
    pub fn have_gnupg() -> bool {
        // asked with a home that is not the person's, so even a version line does not look at `~/.gnupg` (DEV-017)
        let home = std::env::temp_dir().join("shale-gnupg-probe-home");
        let have = ["gpg", "gpgsm", "gpgconf"].iter().all(|p| Command::new(p).env("GNUPGHOME", home_text(&home)).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok());
        if !have {
            eprintln!("skipping: gpg, gpgsm and gpgconf are not installed here");
            return false;
        }
        // The GnuPG that comes with Git for Windows is an MSYS program: it reads `C:/…` as a relative path,
        // so a throw-away home cannot be handed to it. Gpg4win's native GnuPG prints its home as a Windows path.
        if cfg!(windows) {
            let out = Command::new("gpg").arg("--version").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
            if out.lines().any(|l| l.starts_with("Home: /")) {
                eprintln!("skipping: the gpg on the path is an MSYS build (Git for Windows); the tests need Gpg4win's native GnuPG");
                return false;
            }
        }
        true
    }

    impl TestHome {
        pub fn new() -> TestHome {
            // short, so that the agent's socket path fits when it has to live in the home
            let base = if cfg!(unix) { std::path::PathBuf::from("/tmp") } else { std::env::temp_dir() };
            let dir = base.join(format!("shg{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&dir);
            crate::fsutil::ensure_private_dir(&dir).expect("temp home");
            std::fs::write(dir.join("gpg-agent.conf"), "allow-loopback-pinentry\n").expect("agent conf");
            std::fs::write(dir.join("gpgsm.conf"), "disable-crl-checks\n").expect("gpgsm conf");
            TestHome { g: Gnupg { home: Some(dir.clone()), ..Gnupg::system() }, dir }
        }

        fn cmd(&self, program: &str) -> Command {
            let mut c = Command::new(program);
            c.env("GNUPGHOME", home_text(&self.dir)).stdin(Stdio::null());
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
            self.gen_smime_valid(cn, email, None, "2031-01-01")
        }

        /// Like [`gen_smime`](Self::gen_smime), but valid from `created` (`YYYY-MM-DD`, default: now) until `not_after`:
        /// gpgsm signs the certificate with whatever dates it is given, so a certificate that expired in the past
        /// (`created` 2020-01-01, `not_after` 2020-01-10) needs no other tool.
        pub fn gen_smime_valid(&self, cn: &str, email: &str, created: Option<&str>, not_after: &str) -> String {
            let from = created.map(|c| format!("Creation-Date: {c}\n")).unwrap_or_default();
            let params = format!("Key-Type: RSA\nKey-Length: 2048\nKey-Usage: sign, encrypt\nSerial: random\nName-DN: CN={cn},O=Shale Test\nName-Email: {email}\n{from}Not-After: {not_after}\n");
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

        /// A new OpenPGP key that is protected by `passphrase`; its fingerprint. A call that needs the key asks the
        /// agent's pinentry, which in the tests is the stub of [`use_pinentry_stub`](Self::use_pinentry_stub).
        #[cfg(unix)]
        pub fn gen_pgp_protected(&self, name: &str, email: &str, passphrase: &str) -> String {
            let uid = format!("{name} <{email}>");
            let out = self.cmd("gpg").args(["--batch", "--pinentry-mode", "loopback", "--passphrase", passphrase, "--quick-gen-key", &uid, "default", "default", "never"]).output().expect("gpg runs");
            assert!(out.status.success(), "gpg --quick-gen-key: {}", String::from_utf8_lossy(&out.stderr));
            self.g.list_keys(CryptoStandard::OpenPgp, true, Some(email), &no_cancel()).unwrap().into_iter().next().expect("the new key").fingerprint
        }

        /// Make the agent of this home use a pinentry stub (a shell script) instead of a dialog, and keep no passphrase
        /// in its cache, so every call asks. What the stub answers is set with [`pinentry_says`](Self::pinentry_says).
        #[cfg(unix)]
        pub fn use_pinentry_stub(&self) {
            use std::os::unix::fs::PermissionsExt;
            let stub = self.dir.join("pinentry-stub");
            let script = "#!/bin/sh\n\
                answer() { cat \"$(dirname \"$0\")/pinentry-answer\"; }\n\
                echo 'OK Pleased to meet you'\n\
                while read -r cmd rest; do\n\
                  case \"$cmd\" in\n\
                    GETPIN)\n\
                      a=$(answer)\n\
                      case \"$a\" in\n\
                        cancel) echo 'ERR 83886179 Operation cancelled <Pinentry>' ;;\n\
                        hang) sleep 120 ;;\n\
                        *) echo \"D $a\"; echo OK ;;\n\
                      esac ;;\n\
                    BYE) echo 'OK closing connection'; exit 0 ;;\n\
                    *) echo OK ;;\n\
                  esac\n\
                done\n";
            std::fs::write(&stub, script).unwrap();
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut conf = std::fs::read_to_string(self.dir.join("gpg-agent.conf")).unwrap_or_default();
            conf.push_str(&format!("pinentry-program {}\ndefault-cache-ttl 0\nmax-cache-ttl 0\n", stub.display()));
            std::fs::write(self.dir.join("gpg-agent.conf"), conf).unwrap();
            self.pinentry_says("cancel");
            let _ = self.cmd("gpgconf").args(["--reload", "gpg-agent"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }

        /// What the pinentry stub does at the next passphrase question: `cancel`, `hang` (never answers), or any other
        /// text, which is typed as the passphrase.
        #[cfg(unix)]
        pub fn pinentry_says(&self, answer: &str) {
            std::fs::write(self.dir.join("pinentry-answer"), answer).unwrap();
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

        /// Like [`pgp_sign`](Self::pgp_sign), made at `faked_time` and valid for `expire` (gpg's `--default-sig-expire`).
        pub fn pgp_sign_expiring(&self, fpr: &str, data: &[u8], faked_time: &str, expire: &str) -> Vec<u8> {
            self.pipe("gpg", &["--armor", "--detach-sign", "-u", fpr, "--faked-system-time", faked_time, "--default-sig-expire", expire], data)
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

        /// `gpg --list-packets` of some data: which keys a message is encrypted to.
        pub fn list_packets(&self, data: &[u8]) -> String {
            let mut c = self.cmd("gpg").args(["--batch", "--list-packets"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("gpg runs");
            c.stdin.take().unwrap().write_all(data).unwrap();
            String::from_utf8_lossy(&c.wait_with_output().unwrap().stdout).into_owned()
        }

        /// Revoke an OpenPGP key of this home with the certificate gpg stored when it was made.
        pub fn revoke_pgp(&self, fpr: &str) {
            let cert = std::fs::read_to_string(self.dir.join("openpgp-revocs.d").join(format!("{fpr}.rev"))).expect("revocation certificate");
            let cleaned: Vec<&str> = cert.lines().map(|l| l.strip_prefix(':').unwrap_or(l)).collect();
            self.import_pgp(cleaned.join("\n").as_bytes());
        }

        /// A detached S/MIME signature (DER) over `data`.
        /// A detached S/MIME signature made at `faked_time` (`YYYYMMDDTHHMMSS`), when the certificate was still valid.
        pub fn smime_sign_at(&self, fpr: &str, data: &[u8], faked_time: &str) -> Vec<u8> {
            self.pipe("gpgsm", &["--detach-sign", "-u", fpr, "--faked-system-time", faked_time], data)
        }

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

    // spec: SEC-013
    #[test]
    fn the_temp_folder_is_private_and_a_made_up_path_is_never_adopted() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        assert_ne!(a.path, b.path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&a.path).unwrap().permissions().mode() & 0o777, 0o700);
            assert_ne!(a.path.parent(), Some(std::env::temp_dir().as_path()), "not directly in the shared temp folder");
        }
        // a path that exists already (a folder or a link someone made) is an error
        assert!(crate::fsutil::create_private_dir_new(&a.path).is_err());
        #[cfg(unix)]
        {
            let link = a.path.join("link");
            std::os::unix::fs::symlink(&b.path, &link).unwrap();
            assert!(crate::fsutil::create_private_dir_new(&link).is_err());
        }
    }

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

    // spec: DEV-017
    #[test]
    fn dev_017_the_default_gnupg_of_a_test_build_names_programs_that_do_not_exist() {
        let g = Gnupg::default();
        assert!(g.home.is_none());
        for s in [CryptoStandard::OpenPgp, CryptoStandard::Smime] {
            assert!(!std::path::Path::new(g.program(s)).exists(), "{}", g.program(s));
            assert_eq!(g.version(s), Err(GpgError::Missing(s)));
            assert_eq!(g.list_keys(s, true, None, &no_cancel()), Err(GpgError::Missing(s)));
        }
        // the machine's programs have to be asked for by name
        let system = Gnupg::system();
        assert_eq!((system.gpg.as_str(), system.gpgsm.as_str(), system.home), ("gpg", "gpgsm", None));
    }

    // spec: MAIL-170
    #[test]
    fn a_missing_program_is_reported_in_words() {
        let g = Gnupg { gpg: "/nonexistent/gpg-for-shale-tests".into(), gpgsm: "/nonexistent/gpgsm-for-shale-tests".into(), home: None };
        let e = g.list_keys(CryptoStandard::OpenPgp, false, None, &no_cancel()).unwrap_err();
        assert_eq!(e, GpgError::Missing(CryptoStandard::OpenPgp));
        assert!(e.to_string().contains("not installed"), "{e}");
        assert!(e.to_string().contains("Install "), "it says where to get it: {e}");
        assert!(matches!(g.version(CryptoStandard::Smime), Err(GpgError::Missing(CryptoStandard::Smime))));
    }

    // spec: MAIL-170
    #[cfg(unix)]
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

    // spec: DEV-017
    #[cfg(unix)]
    #[test]
    fn dev_017_the_version_is_asked_with_the_home_of_the_gnupg_too() {
        let dir = TempDir::new().unwrap();
        // a stand-in that says which home it was given
        let script = dir.write("fake-gpg", b"#!/bin/sh\necho \"home:${GNUPGHOME:-UNSET}\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let program = script.to_string_lossy().into_owned();
        let g = Gnupg { gpg: program.clone(), gpgsm: program, home: Some(dir.path.clone()) };
        for s in [CryptoStandard::OpenPgp, CryptoStandard::Smime] {
            assert_eq!(g.version(s).unwrap(), format!("home:{}", dir.path.display()));
        }
    }

    // spec: MAIL-194, MAIL-197
    #[test]
    fn signing_and_encrypting_round_trip_for_openpgp_and_a_hidden_recipient_has_no_key_id_in_the_message() {
        if !have_gnupg() {
            return;
        }
        let (alice, bob, carol) = (TestHome::new(), TestHome::new(), TestHome::new());
        let a = alice.gen_pgp("Alice", "alice@example.org", "never", None);
        let b = bob.gen_pgp("Bob", "bob@example.org", "never", None);
        let c = carol.gen_pgp("Carol", "carol@example.org", "never", None);
        for (h, f) in [(&bob, &b), (&carol, &c)] {
            h.share_pgp_with(&alice, f);
        }
        alice.share_pgp_with(&bob, &a);
        alice.share_pgp_with(&carol, &a);
        let data = b"Content-Type: text/plain\r\n\r\nhello \xc3\xa4\r\n";
        // a detached signature: the programs' own check agrees
        let sig = alice.g.sign_detached(CryptoStandard::OpenPgp, &a, data, &no_cancel()).unwrap();
        assert!(sig.starts_with(b"-----BEGIN PGP SIGNATURE-----"));
        let v = bob.g.verify_detached(CryptoStandard::OpenPgp, &sig, data, &no_cancel()).unwrap();
        assert_eq!((v.len(), v[0].state), (1, SigState::Good));
        let mut tampered = data.to_vec();
        tampered[30] ^= 1;
        assert_eq!(bob.g.verify_detached(CryptoStandard::OpenPgp, &sig, &tampered, &no_cancel()).unwrap()[0].state, SigState::Bad);
        // signed and encrypted in one message, to Bob, Carol (hidden) and Alice herself
        let cipher = alice.g.encrypt(CryptoStandard::OpenPgp, &[(b.clone(), false), (c.clone(), true), (a.clone(), false)], Some(&a), data, &no_cancel()).unwrap();
        for h in [&alice, &bob, &carol] {
            let d = h.g.decrypt(CryptoStandard::OpenPgp, &cipher, &no_cancel()).unwrap().unwrap();
            assert_eq!(d.plain, data);
            assert_eq!(d.signatures.len(), 1, "signed in the same operation");
            assert_eq!(d.signatures[0].state, SigState::Good);
        }
        let packets = alice.list_packets(&cipher);
        let c_id = &c[c.len() - 16..];
        assert!(!packets.to_ascii_uppercase().contains(c_id), "the hidden recipient's key id is not in the message:\n{packets}");
        assert!(packets.to_ascii_uppercase().contains(&b[b.len() - 16..]) || packets.contains("pubkey enc packet"), "{packets}");
        assert!(packets.contains("0000000000000000"), "a hidden recipient shows as the wildcard key id");
        // a stranger cannot read it
        let eve = TestHome::new();
        eve.gen_pgp("Eve", "eve@example.org", "never", None);
        assert!(matches!(eve.g.decrypt(CryptoStandard::OpenPgp, &cipher, &no_cancel()).unwrap(), Err(DecryptFailure::NoSecretKey(_))));
        // a key that is not there, and bad arguments, are refused in words
        let e = alice.g.encrypt(CryptoStandard::OpenPgp, &[("0123456789ABCDEF0123456789ABCDEF01234567".into(), false)], None, data, &no_cancel()).unwrap_err();
        assert!(e.contains("not encrypted"), "{e}");
        assert!(alice.g.encrypt(CryptoStandard::OpenPgp, &[("--help".into(), false)], None, data, &no_cancel()).is_err());
        assert!(alice.g.encrypt(CryptoStandard::OpenPgp, &[], None, data, &no_cancel()).is_err());
        assert!(alice.g.sign_detached(CryptoStandard::OpenPgp, "not a key", data, &no_cancel()).is_err());
        // a key that was not made here has no secret key: signing says so
        let e = alice.g.sign_detached(CryptoStandard::OpenPgp, &b, data, &no_cancel()).unwrap_err();
        assert!(e.contains("not signed") && e.contains("secret key"), "{e}");
    }

    // spec: MAIL-194, MAIL-197
    #[test]
    fn signing_and_encrypting_round_trip_for_smime() {
        if !have_gnupg() {
            return;
        }
        let (alice, bob) = (TestHome::new(), TestHome::new());
        let a = alice.gen_smime("Alice S/MIME", "alice@example.org");
        let b = bob.gen_smime("Bob S/MIME", "bob@example.org");
        bob.share_smime_with(&alice, &b);
        alice.share_smime_with(&bob, &a);
        let data = b"Content-Type: text/plain\r\n\r\nhello\r\n";
        let sig = alice.g.sign_detached(CryptoStandard::Smime, &a, data, &no_cancel()).unwrap();
        let v = bob.g.verify_detached(CryptoStandard::Smime, &sig, data, &no_cancel()).unwrap();
        assert_eq!((v.len(), v[0].state), (1, SigState::Good), "{v:?}");
        let cipher = alice.g.encrypt(CryptoStandard::Smime, &[(b.clone(), false), (a.clone(), false)], None, data, &no_cancel()).unwrap();
        for h in [&alice, &bob] {
            assert_eq!(h.g.decrypt(CryptoStandard::Smime, &cipher, &no_cancel()).unwrap().unwrap().plain, data);
        }
        let eve = TestHome::new();
        eve.gen_smime("Eve S/MIME", "eve@example.org");
        assert!(matches!(eve.g.decrypt(CryptoStandard::Smime, &cipher, &no_cancel()).unwrap(), Err(DecryptFailure::NoSecretKey(_) | DecryptFailure::NoKeyOrDamaged)), "{:?}", eve.g.decrypt(CryptoStandard::Smime, &cipher, &no_cancel()));
    }

    // spec: MAIL-171
    #[cfg(unix)]
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

    fn key(validity: char) -> KeyInfo {
        KeyInfo { fingerprint: "0123456789ABCDEF0123456789ABCDEF01234567".into(), key_id: "89ABCDEF01234567".into(), uids: Vec::new(), emails: Vec::new(), validity, can_encrypt: true, can_sign: true, expires: None, secret: false }
    }

    fn st(keyword: &str, args: &str) -> Status {
        Status { keyword: keyword.into(), args: args.into() }
    }

    fn run_of(status: &[(&str, &str)], code: i32) -> Run {
        Run { stdout: Vec::new(), status: status.iter().map(|(k, a)| st(k, a)).collect(), code }
    }

    // spec: MAIL-174, MAIL-193
    #[test]
    fn each_trust_level_of_a_key_has_its_words_and_expired_revoked_disabled_and_invalid_keys_are_not_usable() {
        let words: Vec<(char, &str, bool)> = vec![
            ('u', "yours", true),
            ('f', "trusted", true),
            ('m', "marginally trusted", true),
            ('e', "expired", false),
            ('r', "revoked", false),
            ('d', "disabled", false),
            ('i', "invalid", false),
            ('n', "not trusted", true),
            ('-', "not certified", true),
            ('q', "not certified", true),
        ];
        for (letter, text, usable) in words {
            let k = key(letter);
            assert_eq!((k.trust_words(), k.usable()), (text, usable), "validity {letter}");
        }
        // the label is the first user id, or the fingerprint when there is none
        let mut k = key('u');
        assert_eq!(k.label(), k.fingerprint);
        k.uids = vec!["Ada <ada@x.org>".into(), "Other".into()];
        assert_eq!(k.label(), "Ada <ada@x.org>");
    }

    // spec: MAIL-193
    #[test]
    fn a_refused_recipient_is_explained_in_words_for_every_reason_gnupg_gives() {
        let reasons = [
            ("0", "has expired, been revoked or been disabled"),
            ("1", "no such key"),
            ("2", "several keys"),
            ("3", "may not be used for encryption"),
            ("4", "has been revoked"),
            ("5", "has expired"),
            ("6", "revocation list"),
            ("7", "revocation list"),
            ("8", "policy"),
            ("10", "not trusted"),
            ("11", "chain is incomplete"),
            ("12", "chain is incomplete"),
            ("99", "refused the key"),
        ];
        for (n, text) in reasons {
            assert!(inv_recp_words(n).contains(text), "reason {n}: {}", inv_recp_words(n));
        }
        // the words name the address (unescaped) and every refused recipient
        let r = run_of(&[("INV_RECP", "5 bob@example.org"), ("INV_RECP", "4 Eve\\x20Ex <eve@example.org>")], 2);
        assert_eq!(op_failure(&r, "encrypted"), "The mail was not encrypted: bob@example.org: the key has expired; Eve Ex <eve@example.org>: the key has been revoked.");
    }

    // spec: MAIL-195
    #[test]
    fn a_failed_signing_or_encrypting_says_why_in_words() {
        let w = |status: &[(&str, &str)]| op_failure(&run_of(status, 2), "signed");
        assert!(w(&[("ERROR", "sign 83886179")]).contains("dialog was cancelled"), "{}", w(&[("ERROR", "sign 83886179")]));
        assert!(w(&[("BAD_PASSPHRASE", "ABCD")]).contains("passphrase was not accepted"));
        assert!(w(&[("ERROR", "sign 83886091")]).contains("passphrase was not accepted"), "error code 11");
        for s in [&[("INV_SGNR", "9 Ada")][..], &[("NO_SECKEY", "ABCD")][..], &[("ERROR", "sign 67108881")][..]] {
            let t = w(s);
            assert!(t.contains("no secret key here (or has expired)") && t.contains("not signed"), "{t}");
        }
        assert!(w(&[("FAILURE", "sign 83886179")]).contains("dialog was cancelled"), "a cancelled signing dialog is a FAILURE line");
        assert!(w(&[("FAILURE", "sign 67108875")]).contains("passphrase was not accepted"), "a wrong passphrase when signing is a FAILURE line too");
        assert_eq!(w(&[("FAILURE", "sign 33554433")]), "GnuPG could not do it, so the mail was not signed (FAILURE sign 33554433).");
        assert_eq!(w(&[]), "GnuPG could not do it, so the mail was not signed.");
    }

    // spec: MAIL-182
    #[test]
    fn a_failed_decryption_is_told_apart_by_its_status_lines() {
        let f = |s: CryptoStandard, status: &[(&str, &str)]| decrypt_failure(&run_of(status, 2), s);
        let pgp = CryptoStandard::OpenPgp;
        assert_eq!(f(pgp, &[("ERROR", "pkdecrypt_failed 83886179")]), DecryptFailure::Cancelled);
        assert_eq!(f(pgp, &[("ERROR", "pkdecrypt_failed cancelled by user")]), DecryptFailure::Cancelled);
        assert_eq!(f(pgp, &[("BAD_PASSPHRASE", "AA")]), DecryptFailure::BadPassphrase);
        assert_eq!(f(pgp, &[("ERROR", "x 83886091")]), DecryptFailure::BadPassphrase);
        assert_eq!(f(pgp, &[("NO_SECKEY", "AA"), ("NO_SECKEY", "BB")]), DecryptFailure::NoSecretKey(vec!["AA".into(), "BB".into()]));
        assert_eq!(f(pgp, &[("ERROR", "x 67108881")]), DecryptFailure::NoSecretKey(Vec::new()));
        assert_eq!(f(pgp, &[("NODATA", "1")]), DecryptFailure::Corrupt);
        assert_eq!(f(pgp, &[("ERROR", "x 67108922")]), DecryptFailure::Corrupt);
        assert_eq!(f(pgp, &[("DECRYPTION_FAILED", "")]), DecryptFailure::Corrupt);
        // gpgsm only says DECRYPTION_FAILED, without an error code
        assert_eq!(f(CryptoStandard::Smime, &[("DECRYPTION_FAILED", "")]), DecryptFailure::NoKeyOrDamaged);
        assert_eq!(f(pgp, &[("FAILURE", "decrypt 5")]), DecryptFailure::Other("FAILURE decrypt 5".into()));
        assert_eq!(f(pgp, &[]), DecryptFailure::Other("exit code 2".into()));
        // every failure has words for the person
        let words = |d: DecryptFailure| d.to_string();
        assert!(words(DecryptFailure::NoSecretKey(Vec::new())).contains("not in your keyring"));
        assert!(words(DecryptFailure::Cancelled).contains("cancelled"));
        assert!(words(DecryptFailure::BadPassphrase).contains("not accepted"));
        assert!(words(DecryptFailure::NoKeyOrDamaged).contains("none of your certificates fits"));
        assert!(words(DecryptFailure::Corrupt).contains("damaged"));
        assert!(words(DecryptFailure::Other("boom".into())).ends_with("boom"));
    }

    // spec: MAIL-174
    #[test]
    fn signature_verdicts_are_read_from_status_lines_with_signer_fingerprint_trust_and_time() {
        let sigs = |status: &[(&str, &str)]| signatures(&status.iter().map(|(k, a)| st(k, a)).collect::<Vec<_>>());
        let good = sigs(&[
            ("NEWSIG", ""),
            ("GOODSIG", "AAAA1111 Ada\\x20L <ada@x.org>"),
            ("VALIDSIG", "FFFF0000FFFF0000FFFF0000FFFF0000FFFF0000 2026-11-14 1794650000 0 4 0 22 8 00 0123456789ABCDEF0123456789ABCDEF01234567"),
            ("TRUST_FULLY", "0 pgp"),
        ]);
        assert_eq!(good.len(), 1);
        assert_eq!((good[0].state, good[0].trust, good[0].signer.as_str(), good[0].key_id.as_str()), (SigState::Good, Trust::Full, "Ada L <ada@x.org>", "AAAA1111"));
        assert_eq!((good[0].fingerprint.as_str(), good[0].time), ("0123456789ABCDEF0123456789ABCDEF01234567", Some(1_794_650_000)), "the primary key's fingerprint wins");
        for (line, trust) in [("TRUST_NEVER", Trust::Never), ("TRUST_UNDEFINED", Trust::Undefined), ("TRUST_MARGINAL", Trust::Marginal), ("TRUST_ULTIMATE", Trust::Ultimate)] {
            assert_eq!(sigs(&[("GOODSIG", "A x"), (line, "0 pgp")])[0].trust, trust, "{line}");
        }
        assert_eq!(sigs(&[("GOODSIG", "A x")])[0].trust, Trust::Unknown);
        for (kw, state) in [("BADSIG", SigState::Bad), ("EXPKEYSIG", SigState::ExpiredKey), ("EXPSIG", SigState::ExpiredSig), ("REVKEYSIG", SigState::RevokedKey)] {
            let s = sigs(&[(kw, "BBBB2222 Bob <bob@x.org>")]);
            assert_eq!((s[0].state, s[0].fingerprint.as_str(), s[0].signer.as_str()), (state, "BBBB2222", "Bob <bob@x.org>"), "{kw}");
        }
        // gpgsm: a revoked certificate is a good signature, then TRUST_NEVER with the code of "certificate revoked"
        let rv = sigs(&[("GOODSIG", "A x"), ("TRUST_NEVER", "94")]);
        assert_eq!((rv[0].state, rv[0].trust), (SigState::RevokedKey, Trust::Never));
        assert_eq!(sigs(&[("GOODSIG", "A x"), ("TRUST_NEVER", "0 pgp")])[0].state, SigState::Good, "another reason for no trust is no revocation");
        // a status line that belongs to no signature changes nothing
        assert!(sigs(&[("VALIDSIG", "AA"), ("TRUST_FULLY", "0 pgp"), ("NEWSIG", "")]).is_empty());
        // ERRSIG: rc 9 is a missing key, anything else is an error
        let miss = sigs(&[("ERRSIG", "CCCC3333 1 8 00 1794650000 9 -")]);
        assert_eq!((miss[0].state, miss[0].key_id.as_str()), (SigState::UnknownKey, "CCCC3333"));
        assert_eq!(sigs(&[("ERRSIG", "CCCC3333 1 8 00 1794650000 4 -")])[0].state, SigState::Error);
        // gpgsm names no signer for a certificate it does not have
        let sm = sigs(&[("ERROR", "verify.findkey 50331657")]);
        assert_eq!((sm.len(), sm[0].state), (1, SigState::UnknownKey));
        assert!(sigs(&[("ERROR", "verify.findkey 50331658")]).is_empty(), "another code is not a missing key");
        assert!(sigs(&[]).is_empty());
    }

    // spec: MAIL-174, MAIL-197
    #[test]
    fn real_signatures_are_good_bad_unknown_expired_by_key_expired_by_date_or_revoked() {
        if !have_gnupg() {
            return;
        }
        let h = TestHome::new();
        let fpr = h.gen_pgp("Ada Test", "ada@example.org", "never", None);
        let data = b"the signed text\r\n";
        let sig = h.pgp_sign(&fpr, data, None);
        let verdict = |g: &Gnupg, sig: &[u8], data: &[u8]| g.verify_detached(CryptoStandard::OpenPgp, sig, data, &no_cancel()).unwrap();
        let good = verdict(&h.g, &sig, data);
        assert_eq!((good[0].state, good[0].fingerprint.as_str(), good[0].signer.as_str()), (SigState::Good, fpr.as_str(), "Ada Test <ada@example.org>"), "{good:?}");
        assert!(good[0].time.is_some());
        // one changed byte: bad
        let bad = verdict(&h.g, &sig, b"the signed Text\r\n");
        assert_eq!((bad.len(), bad[0].state), (1, SigState::Bad), "{bad:?}");
        // a keyring without the key: unknown signer, with the key id of the signature
        let other = TestHome::new();
        let unknown = verdict(&other.g, &sig, data);
        assert_eq!(unknown[0].state, SigState::UnknownKey, "{unknown:?}");
        assert!(fpr.ends_with(&unknown[0].key_id), "{unknown:?}");
        // a signature with an end date that has passed (made in 2020 with a key that already existed then)
        let dated = TestHome::new();
        let d_fpr = dated.gen_pgp("Dated Key", "dated@example.org", "never", Some("20200101T000000"));
        let old = dated.pgp_sign_expiring(&d_fpr, data, "20200110T000000", "1d");
        let exp = verdict(&dated.g, &old, data);
        assert_eq!(exp[0].state, SigState::ExpiredSig, "{exp:?}");
        // a key that has expired since
        let short = TestHome::new();
        let s_fpr = short.gen_pgp("Short Key", "short@example.org", "30d", Some("20200101T000000"));
        let s_sig = short.pgp_sign(&s_fpr, data, Some("20200110T000000"));
        let ek = verdict(&short.g, &s_sig, data);
        assert_eq!(ek[0].state, SigState::ExpiredKey, "{ek:?}");
        // the key list says the same in words
        let k = short.g.key_info(CryptoStandard::OpenPgp, &s_fpr, &no_cancel()).unwrap().unwrap();
        assert_eq!((k.trust_words(), k.usable()), ("expired", false), "{k:?}");
        assert!(k.expires.is_some());
        // revoked after signing
        h.revoke_pgp(&fpr);
        let rk = verdict(&h.g, &sig, data);
        assert_eq!(rk[0].state, SigState::RevokedKey, "{rk:?}");
        let k = h.g.key_info(CryptoStandard::OpenPgp, &fpr, &no_cancel()).unwrap().unwrap();
        assert_eq!((k.trust_words(), k.usable()), ("revoked", false), "{k:?}");
    }

    // spec: MAIL-193, MAIL-195
    #[test]
    fn encrypting_to_an_expired_or_revoked_key_and_signing_with_one_are_refused_in_words() {
        if !have_gnupg() {
            return;
        }
        let me = TestHome::new();
        let my = me.gen_pgp("Me", "me@example.org", "never", None);
        let old = TestHome::new();
        let o_fpr = old.gen_pgp("Old Key", "old@example.org", "30d", Some("20200101T000000"));
        old.share_pgp_with(&me, &o_fpr);
        let rev = TestHome::new();
        let r_fpr = rev.gen_pgp("Rev Key", "rev@example.org", "never", None);
        rev.revoke_pgp(&r_fpr);
        rev.share_pgp_with(&me, &r_fpr);
        let to = |f: &str| vec![(f.to_string(), false)];
        let e = me.g.encrypt(CryptoStandard::OpenPgp, &to(&o_fpr), None, b"x", &no_cancel()).unwrap_err();
        assert!(e.starts_with("The mail was not encrypted:") && e.contains("has expired, been revoked or been disabled"), "{e}");
        let e = me.g.encrypt(CryptoStandard::OpenPgp, &to(&r_fpr), None, b"x", &no_cancel()).unwrap_err();
        assert!(e.contains("has expired, been revoked or been disabled"), "{e}");
        // a key that is not in the keyring
        let e = me.g.encrypt(CryptoStandard::OpenPgp, &to("0123456789ABCDEF0123456789ABCDEF01234567"), None, b"x", &no_cancel()).unwrap_err();
        assert!(e.contains("no such key in your keyring"), "{e}");
        // the good key still works
        assert!(me.g.encrypt(CryptoStandard::OpenPgp, &to(&my), Some(&my), b"x", &no_cancel()).is_ok());
        // signing with a key that is not here, and with one that has expired
        let e = me.g.sign_detached(CryptoStandard::OpenPgp, "0123456789ABCDEF0123456789ABCDEF01234567", b"x", &no_cancel()).unwrap_err();
        assert!(e.starts_with("The mail was not signed") && e.contains("no secret key here"), "{e}");
        let e = old.g.sign_detached(CryptoStandard::OpenPgp, &o_fpr, b"x", &no_cancel()).unwrap_err();
        assert!(e.starts_with("The mail was not signed"), "{e}");
        assert!(me.g.sign_detached(CryptoStandard::OpenPgp, "not a key", b"x", &no_cancel()).unwrap_err().contains("not a key fingerprint"));
    }

    // spec: MAIL-171, MAIL-182
    #[cfg(unix)]
    #[test]
    fn a_protected_key_with_a_pinentry_stub_gives_cancelled_a_wrong_passphrase_a_time_out_and_the_right_one() {
        if !have_gnupg() {
            return;
        }
        let h = TestHome::new();
        let fpr = h.gen_pgp_protected("Pat Protected", "pat@example.org", "correct horse");
        h.use_pinentry_stub();
        let data = b"protected text\r\n";
        let cipher = h.pgp_encrypt(&[&fpr], None, data);
        let decrypt = |h: &TestHome| h.g.decrypt(CryptoStandard::OpenPgp, &cipher, &no_cancel()).unwrap();
        // the dialog is cancelled
        h.pinentry_says("cancel");
        assert_eq!(decrypt(&h).unwrap_err(), DecryptFailure::Cancelled);
        // a wrong passphrase is typed
        h.pinentry_says("wrong passphrase");
        assert_eq!(decrypt(&h).unwrap_err(), DecryptFailure::BadPassphrase);
        // the right one opens it (so the stub does reach gpg)
        h.pinentry_says("correct horse");
        assert_eq!(decrypt(&h).unwrap().plain, data);
        // signing with the protected key asks the same way and says so in words
        h.pinentry_says("cancel");
        let e = h.g.sign_detached(CryptoStandard::OpenPgp, &fpr, b"x", &no_cancel()).unwrap_err();
        assert!(e.contains("dialog was cancelled") && e.contains("not signed"), "{e}");
        h.pinentry_says("wrong passphrase");
        let e = h.g.sign_detached(CryptoStandard::OpenPgp, &fpr, b"x", &no_cancel()).unwrap_err();
        assert!(e.contains("passphrase was not accepted"), "{e}");
        // a dialog nobody answers: the call is killed at its time limit, and by cancel
        h.pinentry_says("hang");
        let t = Instant::now();
        let r = h.g.run(CryptoStandard::OpenPgp, &["--decrypt"], &cipher, Duration::from_millis(1500), &no_cancel());
        assert_eq!(r.unwrap_err(), GpgError::Timeout);
        assert!(t.elapsed() < Duration::from_secs(20));
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(800));
            c2.store(true, Ordering::SeqCst);
        });
        let r = h.g.run(CryptoStandard::OpenPgp, &["--decrypt"], &cipher, DIALOG, &cancel);
        assert_eq!(r.unwrap_err(), GpgError::Cancelled);
        // (the agent still waits for the stub, so nothing may ask it after this; the home's drop kills it)
    }

    // spec: MAIL-174, MAIL-193
    #[test]
    fn an_s_mime_certificate_that_has_expired_is_shown_as_expired_and_is_refused_for_encrypting() {
        if !have_gnupg() {
            return;
        }
        let me = TestHome::new();
        let my = me.gen_smime("Me S/MIME", "me@example.org");
        let old = TestHome::new();
        let fpr = old.gen_smime_valid("Old S/MIME", "old@example.org", Some("2020-01-01"), "2020-01-10");
        let data = b"signed while it was valid\r\n";
        let sig = old.smime_sign_at(&fpr, data, "20200105T000000");
        let v = old.g.verify_detached(CryptoStandard::Smime, &sig, data, &no_cancel()).unwrap();
        assert_eq!(v[0].state, SigState::ExpiredKey, "{v:?}");
        let k = old.g.key_info(CryptoStandard::Smime, &fpr, &no_cancel()).unwrap().unwrap();
        assert_eq!((k.trust_words(), k.usable()), ("expired", false), "{k:?}");
        // encrypting to it is refused, signing with it too
        old.share_smime_with(&me, &fpr);
        let to = |f: &str| vec![(f.to_string(), false)];
        let e = me.g.encrypt(CryptoStandard::Smime, &to(&fpr), None, b"x", &no_cancel()).unwrap_err();
        assert!(e.starts_with("The mail was not encrypted:"), "{e}");
        let e = old.g.sign_detached(CryptoStandard::Smime, &fpr, b"x", &no_cancel()).unwrap_err();
        assert!(e.starts_with("The mail was not signed"), "{e}");
        assert!(me.g.encrypt(CryptoStandard::Smime, &to(&my), None, b"x", &no_cancel()).is_ok());
    }

    // spec: MAIL-174, MAIL-193, MAIL-197
    #[test]
    fn an_s_mime_certificate_that_a_ca_has_revoked_is_never_shown_as_valid_and_is_refused_for_encrypting() {
        if !have_gnupg() || !have_openssl() {
            return;
        }
        let h = TestHome::new();
        let ca = TestCa::new(&h, "Shale Test CA");
        let good = ca.issue(&h, "Good Person", "good@example.org");
        let bad = ca.issue(&h, "Revoked Person", "revoked@example.org");
        let data = b"signed before the revocation\r\n";
        let (good_sig, bad_sig) = (good.sign(&ca, data), bad.sign(&ca, data));
        let verdict = |sig: &[u8]| h.g.verify_detached(CryptoStandard::Smime, sig, data, &no_cancel()).unwrap();
        // control: the chain through the CA and its CRL is built, and a certificate that is not revoked is valid
        let v = verdict(&good_sig);
        assert_eq!((v.len(), v[0].state, v[0].fingerprint.as_str(), v[0].trust), (1, SigState::Good, good.fpr.as_str(), Trust::Full), "{v:?}");
        let v = verdict(&bad_sig);
        assert_eq!((v[0].state, v[0].trust), (SigState::Good, Trust::Full), "not revoked yet: {v:?}");
        let to = |f: &str| vec![(f.to_string(), false)];
        assert!(h.g.encrypt(CryptoStandard::Smime, &to(&bad.fpr), None, b"x", &no_cancel()).is_ok());
        // the CA revokes one certificate and publishes a new CRL
        ca.revoke(&h, &bad);
        let v = verdict(&bad_sig);
        assert_eq!((v.len(), v[0].state, v[0].fingerprint.as_str()), (1, SigState::RevokedKey, bad.fpr.as_str()), "{v:?}");
        assert_eq!(verdict(&good_sig)[0].state, SigState::Good, "the other certificate of the CA is still valid");
        // one changed byte of a revoked certificate's mail is bad, not revoked
        assert_eq!(h.g.verify_detached(CryptoStandard::Smime, &bad_sig, b"changed\r\n", &no_cancel()).unwrap()[0].state, SigState::Bad);
        // encrypting to it is refused in words, to the other one is not
        let e = h.g.encrypt(CryptoStandard::Smime, &to(&bad.fpr), None, b"x", &no_cancel()).unwrap_err();
        assert!(e.starts_with("The mail was not encrypted:"), "{e}");
        assert!(h.g.encrypt(CryptoStandard::Smime, &to(&good.fpr), None, b"x", &no_cancel()).is_ok());
    }
}
