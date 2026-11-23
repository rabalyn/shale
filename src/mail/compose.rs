//! Writing a mail (ADR-0010, MAIL-124 to MAIL-129, MAIL-135): the fields of a draft, the checks
//! on what a person typed, replies and forwards, and building the message that is sent.
//!
//! The message is built by `lettre`'s builder (header encoding, addresses, multipart,
//! transfer encodings); this module decides *what* goes in. The final message is made by one
//! pipeline, [`finalize`]: build, then a hook, then bytes. The hook is the identity today and is
//! where phase 4 signs and encrypts.

use crate::mail::{Attachment as MailAttachment, Mail};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::{Address, Message};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// All attachments of one mail together may not be bigger than this (the send would fail on most servers anyway).
pub const MAX_ATTACHMENTS_TOTAL: usize = 25 << 20;

/// What kind of mail a draft is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    New,
    Reply,
    ReplyAll,
    Forward,
}

/// One attachment. The bytes are stored beside the draft, not in its JSON.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Att {
    pub name: String,
    pub mime: String,
    #[serde(skip)]
    pub data: Arc<Vec<u8>>,
    /// The name of the file that holds the bytes beside a saved draft or mail (set by the outbox).
    pub file: String,
}

/// A mail as the person is writing it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Draft {
    pub id: String,
    pub account: String,
    pub kind: Kind,
    pub from_name: String,
    pub from_addr: String,
    /// The address fields as typed: names and addresses separated by commas.
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<Att>,
    /// The Message-ID of the mail this answers, and its chain (`References`).
    pub in_reply_to: String,
    pub references: Vec<String>,
    /// Sign and/or encrypt with this standard when sending (ADR-0011, MAIL-192).
    pub sign: bool,
    pub encrypt: bool,
    pub standard: crate::config::CryptoStandard,
}

impl Draft {
    pub fn attachment_bytes(&self) -> usize {
        self.attachments.iter().map(|a| a.data.len()).sum()
    }
}

/// Why a draft cannot be built or sent: words for the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposeError(pub String);

impl std::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ComposeError {}

fn bad<T>(s: impl Into<String>) -> Result<T, ComposeError> {
    Err(ComposeError(s.into()))
}

// ---------------------------------------------------------------- what a person types

/// A header value typed by the person: no CR, LF or NUL (MAIL-125).
pub fn check_header_text(field: &str, value: &str) -> Result<(), ComposeError> {
    if value.contains(['\r', '\n', '\0']) {
        return bad(format!("{field} may not contain a line break"));
    }
    Ok(())
}

/// The parts of an address list typed by a person: split at commas and semicolons that are not
/// inside quotes or `<>`.
pub fn split_list(list: &str) -> Vec<String> {
    let mut parts = vec![];
    let mut cur = String::new();
    let (mut quoted, mut angle) = (false, false);
    for c in list.chars() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    parts.push(cur);
    parts.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// The addresses of a list as mailboxes; an entry that is not an address is an error that names it.
pub fn parse_list(field: &str, list: &str) -> Result<Vec<Mailbox>, ComposeError> {
    check_header_text(field, list)?;
    split_list(list)
        .into_iter()
        .map(|p| match p.parse::<Mailbox>() {
            Ok(m) => Ok(m),
            Err(_) => bad(format!("{field}: “{p}” is not an e-mail address")),
        })
        .collect()
}

/// The bare address (lower case) of one entry of a list, if it has one.
pub fn bare_address(entry: &str) -> Option<String> {
    entry.trim().parse::<Mailbox>().ok().map(|m| m.email.to_string().to_lowercase())
}

/// Name and address of one entry of a list typed or read from a header (the name may be empty).
pub fn name_and_address(entry: &str) -> Option<(String, String)> {
    let m = entry.trim().parse::<Mailbox>().ok()?;
    Some((m.name.unwrap_or_default(), m.email.to_string()))
}

/// The sender's mailbox from the two fields of the account.
pub fn sender(name: &str, addr: &str) -> Result<Mailbox, ComposeError> {
    check_header_text("From name", name)?;
    check_header_text("From address", addr)?;
    let a = match addr.trim().parse::<Address>() {
        Ok(a) => a,
        Err(_) => return bad("The From address of the account is not an e-mail address (see the account settings)"),
    };
    let n = name.trim();
    Ok(Mailbox::new(if n.is_empty() { None } else { Some(n.to_string()) }, a))
}

// ---------------------------------------------------------------- ids

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn random_hex(n: usize) -> String {
    match crate::mail::oauth::random_bytes(n) {
        Ok(b) => hex(&b),
        // the system has no random numbers: time and a counter keep ids apart (they are not secret)
        Err(_) => {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            format!("{:0width$x}", t ^ ((N.fetch_add(1, std::sync::atomic::Ordering::SeqCst) as u128) << 100), width = n * 2).chars().rev().take(n * 2).collect()
        }
    }
}

/// An id for a draft or an outbox entry: sortable by time, unique, safe as a file name.
pub fn new_id() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("{ms:013}-{}", random_hex(4))
}

/// `<96 random bits@domain>` (MAIL-124). The domain is the sender's; a sender without one gets `shale.invalid`.
pub fn message_id(from_addr: &str) -> String {
    let domain = from_addr.trim().rsplit_once('@').map(|(_, d)| d.trim()).filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')).unwrap_or("shale.invalid");
    format!("<{}@{domain}>", random_hex(12))
}

/// Is this a Message-ID we would put in a header (`<…>`, no space, no control character)?
fn clean_id(id: &str) -> Option<String> {
    let t = id.trim();
    (t.len() > 2 && t.starts_with('<') && t.ends_with('>') && !t.contains(|c: char| c.is_whitespace() || c.is_control())).then(|| t.to_string())
}

// ---------------------------------------------------------------- building

/// What a send needs besides the draft.
#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    /// Seconds since 1970 for the `Date` header.
    pub now: i64,
    /// Use this Message-ID (a mail that is built again after an edit keeps its own); else a new one.
    pub message_id: Option<String>,
    /// Add `User-Agent: Shale <version>` (MAIL-120).
    pub user_agent: bool,
    /// Drafts may be saved incomplete: no recipient needed, no sender check.
    pub lenient: bool,
    /// The mail is going to be signed or encrypted (ADR-0011): its text uses a transfer encoding that no server or relay can
    /// change without changing the bytes (quoted-printable when there are trailing blanks or long lines).
    pub safe_transfer: bool,
}

/// A built message: the bytes to send and what the envelope and the outbox need to know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Built {
    pub bytes: Vec<u8>,
    pub message_id: String,
    pub from: String,
    /// Everyone who gets the mail, Bcc included (the envelope; never in the headers).
    pub recipients: Vec<String>,
}

fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

/// The steps from a draft to the final bytes (MAIL-129): build, the hook, bytes. Phase 4 passes a
/// hook that signs or encrypts; [`identity`] is today's.
pub fn finalize(draft: &Draft, opts: &BuildOptions, hook: &dyn Fn(Built) -> Result<Built, ComposeError>) -> Result<Built, ComposeError> {
    hook(build(draft, opts)?)
}

/// The hook of phase 3: nothing changes.
pub fn identity(b: Built) -> Result<Built, ComposeError> {
    Ok(b)
}

/// The sendable form of a draft with no hook.
pub fn finalize_plain(draft: &Draft, opts: &BuildOptions) -> Result<Built, ComposeError> {
    finalize(draft, opts, &identity)
}

/// Build the message of a draft (MAIL-124, MAIL-125).
pub fn build(draft: &Draft, opts: &BuildOptions) -> Result<Built, ComposeError> {
    // a draft is saved whatever is in it (never lose a text): what cannot be a header is left out of its message file
    let mut draft = draft.clone();
    if opts.lenient {
        let clean = |s: &str| s.replace(['\r', '\n', '\0'], " ");
        draft.subject = clean(&draft.subject);
        draft.in_reply_to = clean(&draft.in_reply_to);
        draft.from_name = clean(&draft.from_name);
        for a in draft.attachments.iter_mut() {
            a.name = clean(&a.name);
        }
        let ok = |list: &str| split_list(&clean(list)).into_iter().filter(|p| p.parse::<Mailbox>().is_ok()).collect::<Vec<_>>().join(", ");
        (draft.to, draft.cc, draft.bcc) = (ok(&draft.to), ok(&draft.cc), ok(&draft.bcc));
    }
    let draft = &draft;
    check_header_text("Subject", &draft.subject)?;
    check_header_text("In-Reply-To", &draft.in_reply_to)?;
    let to = parse_list("To", &draft.to)?;
    let cc = parse_list("Cc", &draft.cc)?;
    let bcc = parse_list("Bcc", &draft.bcc)?;
    if !opts.lenient && to.len() + cc.len() + bcc.len() == 0 {
        return bad("Add at least one recipient");
    }
    let from = match sender(&draft.from_name, &draft.from_addr) {
        Ok(f) => f,
        Err(e) if opts.lenient => {
            let _ = e;
            Mailbox::new(None, "unknown@shale.invalid".parse().map_err(|_| ComposeError("internal".into()))?)
        }
        Err(e) => return Err(e),
    };
    if draft.attachment_bytes() > MAX_ATTACHMENTS_TOTAL {
        return bad(format!("The attachments are bigger than {} MB together", MAX_ATTACHMENTS_TOTAL >> 20));
    }
    let id = opts.message_id.clone().and_then(|i| clean_id(&i)).unwrap_or_else(|| message_id(from.email.as_ref()));
    let mut b = Message::builder().from(from.clone()).message_id(Some(id.clone())).date(std::time::UNIX_EPOCH + std::time::Duration::from_secs(opts.now.max(0) as u64)).subject(draft.subject.clone());
    for m in &to {
        b = b.to(m.clone());
    }
    for m in &cc {
        b = b.cc(m.clone());
    }
    for m in &bcc {
        b = b.bcc(m.clone());
    }
    if let Some(r) = clean_id(&draft.in_reply_to) {
        b = b.in_reply_to(r);
    }
    let refs: Vec<String> = draft.references.iter().filter_map(|r| clean_id(r)).collect();
    if !refs.is_empty() {
        b = b.references(refs.join(" "));
    }
    if opts.lenient && to.len() + cc.len() + bcc.len() == 0 {
        // a draft without recipients: the envelope (not a header) needs somebody, the message gets no To
        let placeholder: Address = "draft@shale.invalid".parse().map_err(|_| ComposeError("internal".into()))?;
        b = b.envelope(lettre::address::Envelope::new(Some(from.email.clone()), vec![placeholder]).map_err(|e| ComposeError(e.to_string()))?);
    }
    if opts.user_agent {
        b = b.user_agent(format!("Shale {}", env!("CARGO_PKG_VERSION")));
    }
    let text = {
        let body = crlf(&draft.body);
        // a signature covers the exact bytes: trailing blanks and long lines are the first things a relay "fixes"
        let fragile = body.split("\r\n").any(|l| l.ends_with([' ', '\t']) || l.len() > 76);
        if opts.safe_transfer && fragile {
            SinglePart::builder().header(ContentType::TEXT_PLAIN).header(lettre::message::header::ContentTransferEncoding::QuotedPrintable).body(body)
        } else {
            SinglePart::plain(body)
        }
    };
    let msg = if draft.attachments.is_empty() {
        b.singlepart(text)
    } else {
        let mut mp = MultiPart::mixed().singlepart(text);
        for a in &draft.attachments {
            check_header_text("Attachment name", &a.name)?;
            let ct = ContentType::parse(&a.mime).unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("a valid type"));
            mp = mp.singlepart(Attachment::new(if a.name.is_empty() { "attachment".to_string() } else { a.name.clone() }).body(a.data.as_ref().clone(), ct));
        }
        b.multipart(mp)
    }
    .map_err(|e| ComposeError(format!("The mail could not be put together: {e}")))?;
    let recipients: Vec<String> = msg.envelope().to().iter().map(|a| a.to_string()).collect();
    Ok(Built { bytes: msg.formatted(), message_id: id, from: from.email.to_string(), recipients })
}

// ---------------------------------------------------------------- the checks before sending

/// What to ask about before the mail goes into the outbox (MAIL-135).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    NoSubject,
    NoBody,
    ForgottenAttachment,
    /// S/MIME encryption with a Bcc: every recipient's certificate is listed in the message (ADR-0011 §4).
    BccVisibleInSmime,
}

impl Confirm {
    pub fn question(self) -> &'static str {
        match self {
            Confirm::NoSubject => "This mail has no subject. Send it anyway?",
            Confirm::NoBody => "This mail has no text. Send it anyway?",
            Confirm::ForgottenAttachment => "The text mentions an attachment, but nothing is attached. Send it anyway?",
            Confirm::BccVisibleInSmime => "S/MIME lists the certificate of every recipient in the encrypted message, so the people who get it can tell who was on Bcc (OpenPGP hides them). Send it anyway?",
        }
    }
}

const ATTACH_WORDS: [&str; 8] = ["attached", "attachment", "attaching", "attach", "angehängt", "angehaengt", "anbei", "anhang"];

/// Does the person's own text (not what they quote) mention an attachment?
pub fn mentions_attachment(body: &str) -> bool {
    body.lines().filter(|l| !l.trim_start().starts_with('>')).any(|l| {
        let low = l.to_lowercase();
        ATTACH_WORDS.iter().any(|w| low.contains(w))
    })
}

/// The questions to ask, in order. Quoted lines, the signature and attribution lines do not count as text.
pub fn confirmations(d: &Draft) -> Vec<Confirm> {
    let mut v = vec![];
    if d.subject.trim().is_empty() {
        v.push(Confirm::NoSubject);
    }
    let typed: Vec<&str> = d.body.lines().filter(|l| !l.trim_start().starts_with('>')).collect();
    let before_sig: Vec<&str> = typed.iter().copied().take_while(|l| l.trim_end() != "--").collect();
    let has_text = before_sig.iter().any(|l| !(l.trim().is_empty() || l.starts_with("On ") && l.trim_end().ends_with("wrote:")));
    if !has_text && d.attachments.is_empty() {
        v.push(Confirm::NoBody);
    }
    if d.attachments.is_empty() && mentions_attachment(&before_sig.join("\n")) {
        v.push(Confirm::ForgottenAttachment);
    }
    if d.encrypt && d.standard == crate::config::CryptoStandard::Smime && !d.bcc.trim().is_empty() {
        v.push(Confirm::BccVisibleInSmime);
    }
    v
}

// ---------------------------------------------------------------- replies and forwards

/// `Re: x` once: a subject that already has a reply prefix (also `AW:`) is left alone.
pub fn reply_subject(s: &str) -> String {
    let t = s.trim();
    let low = t.to_lowercase();
    if ["re:", "aw:", "re :"].iter().any(|p| low.starts_with(p)) {
        t.to_string()
    } else {
        format!("Re: {t}")
    }
}

pub fn forward_subject(s: &str) -> String {
    let t = s.trim();
    let low = t.to_lowercase();
    if ["fwd:", "fw:", "wg:"].iter().any(|p| low.starts_with(p)) {
        t.to_string()
    } else {
        format!("Fwd: {t}")
    }
}

/// Each line with `> ` in front (a line that is quoted already gets just `>`).
pub fn quote(text: &str) -> String {
    text.replace("\r\n", "\n")
        .lines()
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else if l.starts_with('>') {
                format!(">{l}")
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `On 2026-11-14 12:00, Bob wrote:` (the date in the reader's time zone).
pub fn attribution(date: Option<i64>, from: &str) -> String {
    let who = crate::mail::display_name(from);
    let who = if who.is_empty() { from.trim().to_string() } else { who };
    match date {
        Some(d) => format!("On {}, {who} wrote:", crate::mail::ui::full_date(d)),
        None => format!("{who} wrote:"),
    }
}

/// The signature block: `-- ` on a line of its own, then the text (empty if there is none).
pub fn signature_block(sig: &str) -> String {
    let s = sig.trim_end();
    if s.trim().is_empty() {
        String::new()
    } else {
        format!("-- \n{s}")
    }
}

/// Who is addressed: the entries of the lists, without our own addresses and without repeats.
fn merge_recipients(lists: &[&str], own: &[String]) -> String {
    let mut seen: Vec<String> = own.iter().map(|o| o.to_lowercase()).collect();
    let mut out = vec![];
    for l in lists {
        for p in split_list(l) {
            match bare_address(&p) {
                Some(a) if seen.contains(&a) => {}
                Some(a) => {
                    seen.push(a);
                    out.push(p);
                }
                None => {}
            }
        }
    }
    out.join(", ")
}

/// The References chain of an answer: the original's chain plus the original's id, the newest 20 (RFC 5322 advice).
fn chain(orig: &Mail) -> Vec<String> {
    let mut v: Vec<String> = orig.references.split_whitespace().filter_map(clean_id).collect();
    if let Some(id) = clean_id(&orig.message_id) {
        if v.last() != Some(&id) {
            v.push(id);
        }
    }
    let n = v.len();
    if n > 20 {
        v.drain(..n - 20);
    }
    v
}

/// A new draft for `account` with the sender and the signature filled in (MAIL-120).
pub fn new_draft(account: &str, from_name: &str, from_addr: &str, signature: &str) -> Draft {
    let sig = signature_block(signature);
    Draft { id: new_id(), account: account.into(), from_name: from_name.into(), from_addr: from_addr.into(), body: if sig.is_empty() { String::new() } else { format!("\n\n{sig}") }, ..Draft::default() }
}

/// An answer or forward of `orig` (MAIL-126).
pub fn answer(kind: Kind, orig: &Mail, account: &str, from_name: &str, from_addr: &str, signature: &str) -> Draft {
    let mut d = new_draft(account, from_name, from_addr, signature);
    d.kind = kind;
    let sig = signature_block(signature);
    let quoted = format!("{}\n{}", attribution(orig.date, &orig.from), quote(&orig.text));
    d.body = if sig.is_empty() { format!("{quoted}\n\n") } else { format!("{quoted}\n\n\n{sig}") };
    let own = vec![from_addr.to_string()];
    match kind {
        Kind::Reply | Kind::ReplyAll => {
            let to_whom = if orig.reply_to.trim().is_empty() { orig.from.as_str() } else { orig.reply_to.as_str() };
            d.subject = reply_subject(&orig.subject);
            d.in_reply_to = clean_id(&orig.message_id).unwrap_or_default();
            d.references = chain(orig);
            if kind == Kind::Reply {
                d.to = merge_recipients(&[to_whom], &own);
                // an answer to a mail we wrote ourselves goes to the same people again
                if d.to.is_empty() {
                    d.to = merge_recipients(&[&orig.to], &own);
                }
            } else {
                d.to = merge_recipients(&[to_whom, &orig.to], &own);
                d.cc = merge_recipients(&[&orig.cc], &[own.clone(), split_list(&d.to).iter().filter_map(|p| bare_address(p)).collect()].concat());
                if d.to.is_empty() {
                    d.to = merge_recipients(&[&orig.to], &own);
                }
            }
        }
        Kind::Forward => {
            d.subject = forward_subject(&orig.subject);
            d.attachments = orig.attachments.iter().filter(|a| !a.data.is_empty()).map(att_of).collect();
        }
        Kind::New => {}
    }
    d
}

fn att_of(a: &MailAttachment) -> Att {
    Att { name: if a.name.is_empty() { "attachment".into() } else { a.name.clone() }, mime: if a.mime.is_empty() { "application/octet-stream".into() } else { a.mime.clone() }, data: Arc::new(a.data.clone()), ..Att::default() }
}

/// A file name and a type for an attachment from a path.
pub fn att_from_file(name: &str, data: Vec<u8>) -> Att {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    let mime = match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "zip" => "application/zip",
        "json" => "application/json",
        "ics" => "text/calendar",
        "mp4" => "video/mp4",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        // every sound of the one list of media extensions, from the one table (MAIL-241)
        other => crate::media::audio_mime_for_ext(other).unwrap_or("application/octet-stream"),
    };
    Att { name: name.rsplit('/').next().unwrap_or(name).to_string(), mime: mime.into(), data: Arc::new(data), ..Att::default() }
}

/// "Attach note": the note's markdown as `text/markdown` (MAIL-127).
pub fn att_from_note(file_name: &str, markdown: &str) -> Att {
    let name = if file_name.to_lowercase().ends_with(".md") { file_name.to_string() } else { format!("{file_name}.md") };
    Att { name, mime: "text/markdown".into(), data: Arc::new(markdown.as_bytes().to_vec()), ..Att::default() }
}

/// The name of an attachment as a safe file name (for the files beside a draft).
pub fn safe_file_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).take(80).collect();
    if n.is_empty() || n.starts_with('.') {
        format!("_{n}")
    } else {
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail;

    fn draft() -> Draft {
        Draft { id: "d1".into(), account: "a".into(), from_name: "Ada Lovelace".into(), from_addr: "ada@example.com".into(), to: "Bob <bob@x.de>".into(), subject: "Hi".into(), body: "Hello Bob\nSecond line".into(), ..Draft::default() }
    }

    fn opts() -> BuildOptions {
        BuildOptions { now: 1_794_657_600, ..BuildOptions::default() }
    }

    fn header_block(bytes: &[u8]) -> String {
        let s = String::from_utf8_lossy(bytes).into_owned();
        s.split("\r\n\r\n").next().unwrap_or("").to_string()
    }

    // spec: MAIL-125
    #[test]
    fn a_header_value_with_a_line_break_is_refused_in_every_field() {
        for (field, evil) in [("subject", "Hi\r\nBcc: x@y.z"), ("subject", "Hi\nX: y"), ("to", "a@b.c\r\nBcc: evil@x.y"), ("cc", "a@b.c\n"), ("bcc", "a@b.c\0"), ("irt", "<a@b>\r\nX: y")] {
            let mut d = draft();
            match field {
                "subject" => d.subject = evil.into(),
                "to" => d.to = evil.into(),
                "cc" => d.cc = evil.into(),
                "bcc" => d.bcc = evil.into(),
                _ => d.in_reply_to = evil.into(),
            }
            let e = build(&d, &opts()).unwrap_err();
            assert!(e.0.contains("line break"), "{field}: {e}");
        }
        let mut d = draft();
        d.from_name = "Ada\r\nBcc: x@y.z".into();
        assert!(build(&d, &opts()).is_err());
        let mut d = draft();
        d.attachments.push(Att { name: "a\r\nb.txt".into(), mime: "text/plain".into(), data: Arc::new(vec![1]), ..Att::default() });
        assert!(build(&d, &opts()).is_err());
    }

    // spec: MAIL-125
    #[test]
    fn a_display_name_with_a_cr_or_lf_is_an_error_in_words_and_never_a_panic() {
        for brk in ["\r", "\n", "\r\n"] {
            let name = format!("\"Ada{brk}Lovelace\" <ada@x.de>");
            for field in ["to", "cc", "bcc", "from_name"] {
                let mut d = draft();
                match field {
                    "to" => d.to = name.clone(),
                    "cc" => d.cc = name.clone(),
                    "bcc" => d.bcc = name.clone(),
                    _ => d.from_name = format!("Ada{brk}Lovelace"),
                }
                let e = build(&d, &opts()).unwrap_err();
                assert!(e.0.contains("may not contain a line break"), "{field} {brk:?}: {e}");
                let named = if field == "from_name" { "from" } else { field };
                assert!(e.0.to_lowercase().contains(named), "{field} {brk:?}: {e}");
                // a draft saved incomplete builds without a panic and without a bare CR or LF in a header line
                let lenient = BuildOptions { lenient: true, ..opts() };
                let b = build(&d, &lenient).unwrap_or_else(|e| panic!("{field} {brk:?}: {e}"));
                let h = header_block(&b.bytes);
                for line in h.split("\r\n") {
                    assert!(!line.contains(['\r', '\n']), "{field} {brk:?}: {h:?}");
                }
            }
        }
        // lettre 0.11.24 writes a line break in a display name as a space (it panicked before)
        let addr: lettre::Address = "ada@x.de".parse().unwrap();
        let shown = lettre::message::Mailbox::new(Some("Ada\r\nBcc: x@y.z".into()), addr).to_string();
        let m = lettre::Message::builder().from("a@x.de".parse().unwrap()).to(shown.parse().unwrap()).subject("s").body(String::from("b")).unwrap();
        let text = String::from_utf8_lossy(&m.formatted()).into_owned();
        assert!(!text.lines().any(|l| l.to_lowercase().starts_with("bcc:")), "{text}");
    }

    // spec: MAIL-125
    #[test]
    fn a_quoted_local_part_is_never_sent_to_a_different_address() {
        // lettre 0.11.24 keeps a quoted local part as written; it used to cut `"d "@example.com` down to `d@example.com`
        for (to, quoted) in [("\"d \"@example.com", "\"d \"@example.com"), ("\"d e\"@example.com", "\"d e\"@example.com")] {
            let mut d = draft();
            d.to = to.into();
            let b = build(&d, &opts()).unwrap_or_else(|e| panic!("{to}: {e}"));
            assert_eq!(b.recipients, vec![quoted.to_string()], "{to}");
            assert!(header_block(&b.bytes).contains(&format!("To: {quoted}")), "{to}");
        }
    }

    // spec: MAIL-125
    #[test]
    fn addresses_must_parse_and_a_recipient_is_needed() {
        let mut d = draft();
        d.to = "bob@x.de, not an address".into();
        assert!(build(&d, &opts()).unwrap_err().0.contains("not an address"));
        d.to = String::new();
        assert!(build(&d, &opts()).unwrap_err().0.contains("recipient"));
        d.bcc = "secret@x.de".into();
        assert!(build(&d, &opts()).is_ok(), "a Bcc alone is a recipient");
        d.from_addr = "not-an-address".into();
        assert!(build(&d, &opts()).unwrap_err().0.contains("From address"));
        // a draft may be saved incomplete
        let lenient = BuildOptions { lenient: true, ..opts() };
        d.to = String::new();
        d.bcc = String::new();
        assert!(build(&d, &lenient).is_ok());
    }

    // spec: MAIL-124
    #[test]
    fn the_message_has_the_headers_and_round_trips_through_our_own_parser() {
        let mut d = draft();
        d.to = "\"Müller, Hans\" <h@x.de>, bob@x.de".into();
        d.cc = "Cäcilie <c@y.de>".into();
        d.subject = "Grüße aus Köln – ünïcode".into();
        d.body = "Hallo Hans,\nschöne Grüße\n\nÄÖÜ ß € 日本語".into();
        let b = build(&d, &opts()).unwrap();
        let h = header_block(&b.bytes);
        assert!(h.contains("MIME-Version: 1.0"), "{h}");
        assert!(h.contains("Date: Sat, 14 Nov 2026 12:00:00 +0000"), "{h}");
        assert!(h.contains(&format!("Message-ID: {}", b.message_id)), "{h}");
        assert!(h.is_ascii(), "headers are encoded: {h}");
        assert!(!h.contains("User-Agent") && !h.contains("Disposition-Notification") && !h.to_lowercase().contains("return-receipt"), "{h}");
        let m = mail::parse(&b.bytes);
        assert_eq!(m.subject, d.subject);
        assert!(m.from.contains("Ada Lovelace") && m.from.contains("<ada@example.com>"), "{}", m.from);
        assert!(m.to.contains("Müller, Hans") && m.to.contains("h@x.de") && m.to.contains("bob@x.de"), "{}", m.to);
        assert!(m.cc.contains("Cäcilie"), "{}", m.cc);
        assert_eq!(m.text, d.body);
        assert_eq!(m.date, Some(1_794_657_600));
        assert_eq!(b.recipients, vec!["h@x.de", "bob@x.de", "c@y.de"]);
        assert_eq!(b.from, "ada@example.com");
    }

    // spec: MAIL-124
    #[test]
    fn message_ids_are_random_and_use_the_senders_domain_and_user_agent_is_a_setting() {
        let a = message_id("ada@example.com");
        let b = message_id("ada@example.com");
        assert_ne!(a, b);
        assert!(a.starts_with('<') && a.ends_with("@example.com>"), "{a}");
        let local = a.trim_start_matches('<').split('@').next().unwrap();
        assert_eq!(local.len(), 24, "96 bits as hex: {a}");
        assert!(local.bytes().all(|c| c.is_ascii_hexdigit()));
        assert!(message_id("nobody").ends_with("@shale.invalid>"));
    }

    // spec: MAIL-124
    #[test]
    fn user_agent_only_when_asked_and_a_given_message_id_is_kept() {
        let d = draft();
        let h = header_block(&build(&d, &opts()).unwrap().bytes);
        assert!(!h.contains("User-Agent"));
        let h = header_block(&build(&d, &BuildOptions { user_agent: true, message_id: Some("<keep@me>".into()), ..opts() }).unwrap().bytes);
        assert!(h.contains(&format!("User-Agent: Shale {}", env!("CARGO_PKG_VERSION"))), "{h}");
        assert!(h.contains("Message-ID: <keep@me>"), "{h}");
        // a malformed id is not used
        let b = build(&d, &BuildOptions { message_id: Some("no brackets".into()), ..opts() }).unwrap();
        assert!(b.message_id.starts_with('<') && b.message_id.ends_with("@example.com>"));
    }

    // spec: MAIL-124
    #[test]
    fn bcc_goes_to_the_envelope_and_never_into_the_headers() {
        let mut d = draft();
        d.bcc = "Secret <secret@hidden.org>".into();
        let b = build(&d, &opts()).unwrap();
        let all = String::from_utf8_lossy(&b.bytes).to_lowercase();
        // the random boundary and Message-ID are hex and may contain "bcc", so look at header names
        assert!(!all.lines().any(|l| l.starts_with("bcc:")), "no Bcc header");
        assert!(!all.contains("secret@hidden.org") && !all.contains("hidden.org"), "the hidden address is nowhere in the message");
        assert!(b.recipients.contains(&"secret@hidden.org".to_string()));
        assert_eq!(b.recipients.len(), 2);
    }

    // spec: MAIL-127
    #[test]
    fn attachments_round_trip_with_names_types_and_bytes() {
        let mut d = draft();
        let bin: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        d.attachments = vec![
            Att { name: "Grundriß EG.pdf".into(), mime: "application/pdf".into(), data: Arc::new(bin.clone()), ..Att::default() },
            att_from_note("Meeting notes", "# Notes\n\n- one\n- zwei äöü\n"),
            att_from_file("/home/x/picture.png", vec![0x89, b'P', b'N', b'G', 0, 1, 2]),
        ];
        let b = build(&d, &opts()).unwrap();
        let m = mail::parse(&b.bytes);
        assert_eq!(m.text, d.body);
        assert_eq!(m.attachments.len(), 3);
        assert_eq!(m.attachments[0].name, "Grundriß EG.pdf");
        assert_eq!(m.attachments[0].mime, "application/pdf");
        assert_eq!(m.attachments[0].data, bin);
        assert_eq!(m.attachments[1].name, "Meeting notes.md");
        assert_eq!(m.attachments[1].mime, "text/markdown");
        assert_eq!(m.attachments[1].data, "# Notes\n\n- one\n- zwei äöü\n".as_bytes());
        assert_eq!(m.attachments[2].name, "picture.png");
        assert_eq!(m.attachments[2].mime, "image/png");
        assert_eq!(m.attachments[2].data, vec![0x89, b'P', b'N', b'G', 0, 1, 2]);
        // too much is refused with words
        d.attachments = vec![Att { name: "big".into(), mime: "x/y".into(), data: Arc::new(vec![0; MAX_ATTACHMENTS_TOTAL + 1]), ..Att::default() }];
        assert!(build(&d, &opts()).unwrap_err().0.contains("bigger than"));
    }

    // spec: MAIL-241
    #[test]
    fn mail_241_every_audio_extension_goes_out_with_its_audio_type_from_the_shared_table() {
        let mut d = draft();
        for ext in crate::media::AUDIO_EXTS {
            let a = att_from_file(&format!("/home/x/Voice memo.{}", ext.to_ascii_uppercase()), vec![1, 2, 3]);
            assert_eq!(Some(a.mime.as_str()), crate::media::audio_mime_for_ext(ext), ".{ext}");
            assert!(a.mime.starts_with("audio/"), "{ext}: {}", a.mime);
            // what a receiver does with the type gives the extension back (oga and ogg share a type)
            assert!(crate::media::ext_for_mime(&a.mime).is_some_and(|e| e == ext || (ext, e) == ("oga", "ogg")), "{ext}: {}", a.mime);
            d.attachments.push(a);
        }
        // …and the message carries that type, which a reader reads back
        let m = mail::parse(&build(&d, &opts()).unwrap().bytes);
        assert_eq!(m.attachments.len(), crate::media::AUDIO_EXTS.len());
        for (a, ext) in m.attachments.iter().zip(crate::media::AUDIO_EXTS) {
            assert_eq!(Some(a.mime.as_str()), crate::media::audio_mime_for_ext(ext), ".{ext}");
            assert_eq!(a.data, vec![1, 2, 3]);
        }
        // a name that is no sound stays what it was
        assert_eq!(att_from_file("notes.xyz", vec![]).mime, "application/octet-stream");
        assert_eq!(att_from_file("clip.mp4", vec![]).mime, "video/mp4");
    }

    // spec: MAIL-129
    #[test]
    fn the_pipeline_runs_build_then_the_hook_and_the_hook_decides_what_is_sent() {
        let d = draft();
        let plain = finalize_plain(&d, &opts()).unwrap();
        assert_eq!(plain, build(&d, &BuildOptions { message_id: Some(plain.message_id.clone()), ..opts() }).unwrap());
        let signed = finalize(&d, &opts(), &|mut b: Built| {
            b.bytes.extend_from_slice(b"X-Hooked: yes\r\n");
            Ok(b)
        })
        .unwrap();
        assert!(signed.bytes.ends_with(b"X-Hooked: yes\r\n"));
        // a hook that fails stops everything
        assert!(finalize(&d, &opts(), &|_| Err(ComposeError("no key".into()))).is_err());
    }

    fn original() -> Mail {
        mail::parse(
            b"From: Bob Builder <bob@x.de>\r\nTo: ada@example.com, Carol <carol@x.de>\r\nCc: Dave <dave@x.de>, ADA@example.com\r\nSubject: Plans\r\nDate: Fri, 13 Nov 2026 10:22:33 +0000\r\nMessage-ID: <orig@x.de>\r\nReferences: <first@x.de> <second@x.de>\r\nReply-To: Bob Replies <replies@x.de>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nLine one\r\n> already quoted\r\n\r\nLast\r\n--b1\r\nContent-Type: application/pdf; name=\"plan.pdf\"\r\nContent-Disposition: attachment; filename=\"plan.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8gUERG\r\n--b1--\r\n",
        )
    }

    // spec: MAIL-126
    #[test]
    fn a_reply_answers_the_reply_to_address_quotes_and_chains_the_references() {
        let o = original();
        let d = answer(Kind::Reply, &o, "a", "Ada", "ada@example.com", "Ada\nhttps://ada.example");
        assert_eq!(d.subject, "Re: Plans");
        assert_eq!(d.to, "Bob Replies <replies@x.de>");
        assert_eq!(d.cc, "");
        assert_eq!(d.in_reply_to, "<orig@x.de>");
        assert_eq!(d.references, vec!["<first@x.de>", "<second@x.de>", "<orig@x.de>"]);
        assert!(d.attachments.is_empty());
        let date = crate::mail::ui::full_date(o.date.unwrap());
        assert!(d.body.starts_with(&format!("On {date}, Bob Builder wrote:\n> Line one\n>> already quoted\n>\n> Last\n")), "{}", d.body);
        assert!(d.body.ends_with("\n-- \nAda\nhttps://ada.example"), "{}", d.body);
        // the headers of the built answer carry the chain
        let mut d = d;
        d.body = "OK".into();
        let h = header_block(&build(&d, &opts()).unwrap().bytes);
        assert!(h.contains("In-Reply-To: <orig@x.de>"), "{h}");
        assert!(h.contains("References: <first@x.de> <second@x.de> <orig@x.de>"), "{h}");
    }

    // spec: MAIL-126
    #[test]
    fn reply_all_adds_the_others_without_ourselves_or_repeats_and_forward_keeps_attachments() {
        let o = original();
        let d = answer(Kind::ReplyAll, &o, "a", "Ada", "ada@example.com", "");
        assert_eq!(d.to, "Bob Replies <replies@x.de>, Carol <carol@x.de>");
        assert_eq!(d.cc, "Dave <dave@x.de>", "our own address is out, in any case");
        assert!(d.body.ends_with("> Last\n\n"), "{:?}", d.body);
        let f = answer(Kind::Forward, &o, "a", "Ada", "ada@example.com", "");
        assert_eq!(f.subject, "Fwd: Plans");
        assert_eq!((f.to.as_str(), f.cc.as_str(), f.in_reply_to.as_str()), ("", "", ""));
        assert_eq!(f.attachments.len(), 1);
        assert_eq!(f.attachments[0].name, "plan.pdf");
        assert_eq!(f.attachments[0].data.as_slice(), b"Hello PDF");
        assert!(f.body.contains("wrote:\n> Line one"));
        // the prefixes are added once
        assert_eq!(reply_subject("Re: Plans"), "Re: Plans");
        assert_eq!(reply_subject("AW: Plans"), "AW: Plans");
        assert_eq!(forward_subject("Fwd: Plans"), "Fwd: Plans");
        assert_eq!(forward_subject("WG: Plans"), "WG: Plans");
        assert_eq!(forward_subject("Plans"), "Fwd: Plans");
    }

    // spec: MAIL-126
    #[test]
    fn a_long_references_chain_is_cut_to_the_newest_and_an_answer_to_ourselves_goes_to_the_same_people() {
        let refs: Vec<String> = (0..30).map(|i| format!("<r{i}@x>")).collect();
        let o = Mail { message_id: "<now@x>".into(), references: refs.join(" "), from: "ada@example.com".into(), to: "Bob <bob@x.de>".into(), subject: "S".into(), ..Mail::default() };
        let d = answer(Kind::Reply, &o, "a", "Ada", "ada@example.com", "");
        assert_eq!(d.references.len(), 20);
        assert_eq!(d.references.last().unwrap(), "<now@x>");
        assert_eq!(d.references[0], "<r11@x>");
        assert_eq!(d.to, "Bob <bob@x.de>");
        // a mail without an id gives no In-Reply-To
        let d = answer(Kind::Reply, &Mail { from: "bob@x.de".into(), ..Mail::default() }, "a", "", "ada@example.com", "");
        assert_eq!((d.in_reply_to.as_str(), d.references.len()), ("", 0));
    }

    // spec: MAIL-135
    #[test]
    fn the_confirmations_are_asked_for_an_empty_subject_an_empty_body_and_a_forgotten_attachment() {
        let mut d = draft();
        assert!(confirmations(&d).is_empty());
        d.subject = "  ".into();
        assert_eq!(confirmations(&d), vec![Confirm::NoSubject]);
        d.subject = "x".into();
        d.body = "\n\n".into();
        assert_eq!(confirmations(&d), vec![Confirm::NoBody]);
        // an untouched reply (attribution and quote only) has no text of its own, a signature alone neither
        d.body = "On 2026-11-13 10:22, Bob wrote:\n> hello\n\n-- \nAda".into();
        assert_eq!(confirmations(&d), vec![Confirm::NoBody]);
        d.body = "\n\n-- \nAda".into();
        assert_eq!(confirmations(&d), vec![Confirm::NoBody]);
        for word in ["see attached", "Attachment follows", "Anbei der Plan", "im Anhang", "ist angehängt", "I am attaching it"] {
            d.body = format!("Hi\n{word}\n");
            assert_eq!(confirmations(&d), vec![Confirm::ForgottenAttachment], "{word}");
        }
        // quoted text and the signature do not count
        d.body = "Hi, thanks\n> see attached\n\n-- \nAnhang folgt".into();
        assert!(confirmations(&d).is_empty());
        // with an attachment there is nothing to forget, and an attachment alone is a body
        d.body = "see attached".into();
        d.attachments.push(Att { name: "a".into(), mime: "x/y".into(), data: Arc::new(vec![1]), ..Att::default() });
        assert!(confirmations(&d).is_empty());
        d.body = String::new();
        assert!(confirmations(&d).is_empty());
    }

    #[test]
    fn address_lists_split_at_commas_outside_quotes_and_names_stay_as_typed() {
        assert_eq!(split_list("\"Müller, Hans\" <h@x.de>, info@y.de; Team <t@z.de>,"), vec!["\"Müller, Hans\" <h@x.de>", "info@y.de", "Team <t@z.de>"]);
        assert_eq!(bare_address("Bob <BOB@X.de>"), Some("bob@x.de".into()));
        assert_eq!(bare_address("nonsense"), None);
        assert_eq!(safe_file_name("a/b c.pdf"), "a_b_c.pdf");
        assert_eq!(safe_file_name("..hidden"), "_..hidden");
        assert_eq!(safe_file_name(""), "_");
        assert_eq!(quote("a\n\nb\r\n>c"), "> a\n>\n> b\n>>c");
    }

    #[test]
    fn ids_sort_by_time_and_are_unique() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 13 + 1 + 8);
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'));
    }
}
