//! Reading e-mail messages (.eml, as Thunderbird saves or drags them out): headers with
//! encoded words, multipart bodies, base64 / quoted-printable, charsets, attachment names
//! (also RFC 2231), and turning HTML-only mails into plain text.

use crate::calendar::time as t;

pub(crate) mod agent_cache;
pub(crate) mod compose;
pub(crate) mod compose_ui;
pub(crate) mod crypto;
pub(crate) mod db;
pub(crate) mod demo;
pub(crate) mod dialog;
#[cfg(test)]
mod env_tests;
pub(crate) mod gpg;
pub(crate) mod imap_client;
#[cfg(test)]
mod imap_fake_tests;
pub(crate) mod imap_sync;
pub(crate) mod keys;
pub(crate) mod oauth;
#[cfg(test)]
pub(crate) mod oauth_fake_tests;
pub(crate) mod ops;
pub(crate) mod outbox;
pub(crate) mod send;
#[cfg(test)]
mod send_fuzz_tests;
pub(crate) mod signature;
#[cfg(test)]
mod signature_tests;
pub(crate) mod smtp_client;
#[cfg(test)]
mod smtp_fake_tests;
pub(crate) mod store;
pub(crate) mod ui;
#[cfg(test)]
mod ui_tests;
pub(crate) mod unsent;
pub(crate) mod view;
pub(crate) mod watch;
#[cfg(test)]
mod watch_tests;

#[derive(Clone, Debug, Default)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
    /// shown inside the message (pictures in signatures and the like)
    pub inline: bool,
    /// the `Content-ID` without the angle brackets, as `cid:` links in the HTML name it
    pub cid: String,
    /// a part of the signature, or content (IMP-059)
    pub kind: AttKind,
}

/// What an attachment is for (IMP-059).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AttKind {
    #[default]
    Content,
    /// the `smime.p7s` or `signature.asc` part of a signed message
    CryptoSignature,
    /// a logo, icon or banner of the signature block
    SignatureImage,
}

#[derive(Clone, Debug, Default)]
pub struct Mail {
    pub subject: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub date_raw: String,
    /// when it was sent (UTC seconds), if the date could be read
    pub date: Option<i64>,
    pub message_id: String,
    /// `Reply-To` and `References` as written (replies, MAIL-126)
    pub reply_to: String,
    pub references: String,
    /// the message text (plain text; HTML-only mails are converted)
    pub text: String,
    pub attachments: Vec<Attachment>,
}

// ---------------------------------------------------------------- small decoders

pub fn base64_decode(s: &[u8]) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in s {
        if c == b'=' {
            break;
        }
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|v| v as u8)
}

/// Quoted-printable; `header` also turns '_' into spaces (encoded words).
pub fn qp_decode(s: &[u8], header: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'=' {
            // soft line break
            if s.get(i + 1) == Some(&b'\r') && s.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if s.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if let (Some(a), Some(b)) = (s.get(i + 1).and_then(|c| hex(*c)), s.get(i + 2).and_then(|c| hex(*c))) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(if header && c == b'_' { b' ' } else { c });
        i += 1;
    }
    out
}

/// Bytes in a charset → text. UTF-8, US-ASCII, Latin-1/-15 and Windows-1252 are read exactly;
/// anything else is read as UTF-8 where possible.
pub fn decode_charset(data: &[u8], charset: &str) -> String {
    let cs = charset.trim().trim_matches('"').to_ascii_lowercase();
    let latin = |b: u8| b as char;
    match cs.as_str() {
        "iso-8859-1" | "latin1" | "latin-1" | "iso8859-1" | "l1" => data.iter().map(|&b| latin(b)).collect(),
        "iso-8859-15" | "latin9" | "iso8859-15" => data
            .iter()
            .map(|&b| match b {
                0xA4 => '€',
                0xA6 => 'Š',
                0xA8 => 'š',
                0xB4 => 'Ž',
                0xB8 => 'ž',
                0xBC => 'Œ',
                0xBD => 'œ',
                0xBE => 'Ÿ',
                _ => latin(b),
            })
            .collect(),
        "windows-1252" | "cp1252" | "x-cp1252" => {
            const HIGH: [char; 32] = ['€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ'];
            data.iter().map(|&b| if (0x80..0xA0).contains(&b) { HIGH[(b - 0x80) as usize] } else { latin(b) }).collect()
        }
        _ => match std::str::from_utf8(data) {
            Ok(s) => s.to_string(),
            // not UTF-8 after all: most mails that lie about it are Windows-1252
            Err(_) if cs.is_empty() || cs == "utf-8" || cs == "us-ascii" => decode_charset(data, "windows-1252"),
            Err(_) => String::from_utf8_lossy(data).into_owned(),
        },
    }
}

/// Header text with `=?charset?B|Q?…?=` encoded words decoded.
pub fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, after) = rest.split_at(start);
        let parsed = (|| {
            let body = &after[2..];
            let q1 = body.find('?')?;
            let charset = &body[..q1];
            let enc = body.get(q1 + 1..q1 + 2)?;
            if body.get(q1 + 2..q1 + 3)? != "?" {
                return None;
            }
            let text_start = q1 + 3;
            let end = body[text_start..].find("?=")? + text_start;
            let text = &body[text_start..end];
            let bytes = match enc {
                "B" | "b" => base64_decode(text.as_bytes()),
                "Q" | "q" => qp_decode(text.as_bytes(), true),
                _ => return None,
            };
            let charset = charset.split('*').next().unwrap_or(charset); // RFC 2231 language
            Some((decode_charset(&bytes, charset), 2 + end + 2))
        })();
        match parsed {
            Some((text, used)) => {
                // whitespace between two encoded words is dropped
                if !(last_was_word && before.trim().is_empty()) {
                    out.push_str(before);
                }
                out.push_str(&text);
                rest = &after[used..];
                last_was_word = true;
            }
            None => {
                out.push_str(before);
                out.push_str("=?");
                rest = &after[2..];
                last_was_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

fn pct_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let (Some(x), Some(y)) = (b.get(i + 1).and_then(|c| hex(*c)), b.get(i + 2).and_then(|c| hex(*c))) {
                out.push(x * 16 + y);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------- headers

/// Split into (headers, body). Headers: (lower-case name, unfolded raw value).
pub(crate) fn split_message(raw: &[u8]) -> (Vec<(String, String)>, &[u8]) {
    let mut i = 0;
    let mut lines: Vec<String> = Vec::new();
    // an mbox "From " line before the headers
    if raw.starts_with(b"From ") {
        i = raw.iter().position(|&c| c == b'\n').map(|p| p + 1).unwrap_or(raw.len());
    }
    let body_start;
    loop {
        if i >= raw.len() {
            body_start = raw.len();
            break;
        }
        let end = raw[i..].iter().position(|&c| c == b'\n').map(|p| i + p).unwrap_or(raw.len());
        let line = &raw[i..end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        i = (end + 1).min(raw.len() + 1);
        if line.is_empty() {
            body_start = i.min(raw.len());
            break;
        }
        let text = decode_charset(line, "utf-8");
        if (line[0] == b' ' || line[0] == b'\t') && !lines.is_empty() {
            let l = lines.last_mut().unwrap();
            l.push(' ');
            l.push_str(text.trim());
        } else {
            lines.push(text);
        }
    }
    let headers = lines
        .into_iter()
        .filter_map(|l| {
            let (n, v) = l.split_once(':')?;
            Some((n.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect();
    (headers, &raw[body_start.min(raw.len())..])
}

/// One header's (unfolded, still encoded) value from a block of header lines (for the Thunderbird
/// lookup of dropped messages, which only Wayland delivers).
#[cfg(target_os = "linux")]
pub fn header_value(headers: &str, name: &str) -> Option<String> {
    let block = format!("{}\n\n", headers.trim_end());
    let (h, _) = split_message(block.as_bytes());
    header(&h, &name.to_ascii_lowercase()).map(|s| s.to_string())
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// One piece of an RFC 2231 parameter: its number, its text, and whether it is percent-encoded.
type ParamPiece = (u32, String, bool);

/// `text/plain; charset="utf-8"; name*0*=…` → ("text/plain", params). RFC 2231 parameters
/// (charset'' and continuations) are put together and decoded.
pub(crate) fn parse_params(v: &str) -> (String, Vec<(String, String)>) {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in v.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            ';' if !quoted => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    let main = parts.first().map(|s| s.trim().to_ascii_lowercase()).unwrap_or_default();
    // name → (pieces by number, was encoded)
    let mut acc: Vec<(String, Vec<ParamPiece>)> = Vec::new();
    for p in parts.iter().skip(1) {
        let Some((k, val)) = p.split_once('=') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let val = val.trim();
        let val = if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 { val[1..val.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\") } else { val.to_string() };
        let (base, num, enc) = {
            let enc = k.ends_with('*');
            let k2 = k.trim_end_matches('*');
            match k2.split_once('*') {
                Some((b, n)) => (b.to_string(), n.parse().unwrap_or(0), enc),
                None => (k2.to_string(), 0, enc),
            }
        };
        match acc.iter_mut().find(|(n, _)| *n == base) {
            Some((_, v)) => v.push((num, val, enc)),
            None => acc.push((base, vec![(num, val, enc)])),
        }
    }
    let params = acc
        .into_iter()
        .map(|(name, mut pieces)| {
            pieces.sort_by_key(|p| p.0);
            let mut charset = String::from("utf-8");
            let mut bytes = Vec::new();
            for (i, (_, val, enc)) in pieces.iter().enumerate() {
                if *enc {
                    let mut v = val.as_str();
                    if i == 0 {
                        if let Some((cs, rest)) = v.split_once('\'') {
                            charset = cs.to_string();
                            v = rest.split_once('\'').map(|r| r.1).unwrap_or(rest);
                        }
                    }
                    bytes.extend(pct_decode(v));
                } else {
                    bytes.extend(val.as_bytes());
                }
            }
            let text = decode_charset(&bytes, &charset);
            (name, decode_words(&text))
        })
        .collect();
    (main, params)
}

pub(crate) fn param<'a>(p: &'a [(String, String)], name: &str) -> Option<&'a str> {
    p.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

// ---------------------------------------------------------------- dates

/// "Thu, 13 Nov 2026 10:22:33 +0100 (CET)" → UTC seconds.
pub fn parse_date(s: &str) -> Option<i64> {
    let s = s.split('(').next().unwrap_or(s);
    let mut tok: Vec<&str> = s.split(|c: char| c.is_whitespace() || c == ',').filter(|x| !x.is_empty()).collect();
    if tok.first().map(|w| w.chars().all(|c| c.is_alphabetic())).unwrap_or(false) && tok.len() > 4 {
        tok.remove(0); // weekday
    }
    let day: u32 = tok.first()?.parse().ok()?;
    let mon = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|m| tok.get(1).map(|t| t.to_ascii_lowercase().starts_with(m)).unwrap_or(false))? as u32 + 1;
    let mut year: i32 = tok.get(2)?.parse().ok()?;
    if year < 100 {
        year += if year < 50 { 2000 } else { 1900 };
    }
    let mut hms = tok.get(3)?.split(':').map(|x| x.parse::<i64>().unwrap_or(0));
    let (h, m, sec) = (hms.next()?, hms.next().unwrap_or(0), hms.next().unwrap_or(0));
    let zone = tok.get(4).copied().unwrap_or("+0000");
    let off = if let Some(z) = zone.strip_prefix('+').or_else(|| zone.strip_prefix('-')) {
        let v: i64 = z.get(..4).and_then(|z| z.parse().ok()).unwrap_or(0);
        let o = (v / 100) * 3600 + (v % 100) * 60;
        if zone.starts_with('-') {
            -o
        } else {
            o
        }
    } else {
        match zone.to_ascii_uppercase().as_str() {
            "EST" => -5 * 3600,
            "EDT" => -4 * 3600,
            "CST" => -6 * 3600,
            "CDT" => -5 * 3600,
            "MST" => -7 * 3600,
            "MDT" => -6 * 3600,
            "PST" => -8 * 3600,
            "PDT" => -7 * 3600,
            "CET" => 3600,
            "CEST" => 7200,
            _ => 0,
        }
    };
    if !(1..=31).contains(&day) || h > 23 || m > 59 {
        return None;
    }
    Some(t::days_from_civil(year, mon, day) * 86_400 + h * 3600 + m * 60 + sec - off)
}

// ---------------------------------------------------------------- HTML → text

fn entity(name: &str) -> Option<String> {
    if let Some(n) = name.strip_prefix('#') {
        let v = if let Some(h) = n.strip_prefix('x').or_else(|| n.strip_prefix('X')) { u32::from_str_radix(h, 16).ok()? } else { n.parse().ok()? };
        return char::from_u32(v).map(|c| c.to_string());
    }
    Some(
        match name {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => " ",
            "auml" => "ä",
            "ouml" => "ö",
            "uuml" => "ü",
            "Auml" => "Ä",
            "Ouml" => "Ö",
            "Uuml" => "Ü",
            "szlig" => "ß",
            "euro" => "€",
            "eacute" => "é",
            "egrave" => "è",
            "agrave" => "à",
            "ndash" => "–",
            "mdash" => "—",
            "hellip" => "…",
            "laquo" => "«",
            "raquo" => "»",
            "bdquo" => "„",
            "ldquo" => "“",
            "rdquo" => "”",
            "lsquo" => "‘",
            "rsquo" => "’",
            "copy" => "©",
            "reg" => "®",
            "middot" => "·",
            "bull" => "•",
            "deg" => "°",
            _ => return None,
        }
        .to_string(),
    )
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        let after = &rest[p + 1..];
        match after.find(';').filter(|e| *e <= 10).and_then(|e| entity(&after[..e]).map(|t| (t, e))) {
            Some((t, e)) => {
                out.push_str(&t);
                rest = &after[e + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A readable plain-text version of an HTML mail (paragraphs, line breaks, lists, links as
/// markdown links; styles, scripts and the head left out).
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let lower = html.to_ascii_lowercase();
    let b = html.as_bytes();
    let mut link: Option<(String, usize)> = None; // href, where its text starts in out
    let push_nl = |out: &mut String, n: usize| {
        let have = out.chars().rev().take_while(|c| *c == '\n').count();
        if !out.is_empty() && have < n {
            for _ in have..n {
                out.push('\n');
            }
        }
    };
    while i < b.len() {
        if b[i] == b'<' {
            let end = match html[i..].find('>') {
                Some(e) => i + e,
                None => break,
            };
            let tag = &lower[i + 1..end];
            let raw_tag = &html[i + 1..end];
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            let closing = tag.starts_with('/');
            if tag.starts_with("!--") {
                i = lower[i..].find("-->").map(|e| i + e + 3).unwrap_or(b.len());
                continue;
            }
            if !closing && matches!(name.as_str(), "style" | "script" | "head" | "title") {
                i = lower[end..].find(&format!("</{name}")).map(|e| end + e).and_then(|e| lower[e..].find('>').map(|g| e + g + 1)).unwrap_or(b.len());
                continue;
            }
            match (name.as_str(), closing) {
                ("br", _) => out.push('\n'),
                ("p" | "div" | "table" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol", _) => push_nl(&mut out, if name == "div" { 1 } else { 2 }),
                ("tr", true) => push_nl(&mut out, 1),
                ("td" | "th", true) => out.push_str("  "),
                ("li", false) => {
                    push_nl(&mut out, 1);
                    out.push_str("- ");
                }
                ("hr", _) => {
                    push_nl(&mut out, 1);
                    out.push_str("———\n");
                }
                ("a", false) => {
                    let href = tag.find("href=").map(|p| {
                        // `tag` is the lower-case copy: same byte positions as `raw_tag`
                        let r = raw_tag[p + 5..].trim_start();
                        let q = r.chars().next().unwrap_or(' ');
                        if q == '"' || q == '\'' {
                            r[1..].split(q).next().unwrap_or("").to_string()
                        } else {
                            r.split_whitespace().next().unwrap_or("").to_string()
                        }
                    });
                    link = href.map(|h| (decode_entities(&h), out.len()));
                }
                ("a", true) => {
                    if let Some((href, start)) = link.take() {
                        let text = out[start..].to_string();
                        if (href.starts_with("http") || href.starts_with("mailto:")) && text.trim() != href.trim_start_matches("mailto:") && !text.trim().is_empty() {
                            out.truncate(start);
                            out.push_str(&format!("[{}]({href})", text.trim()));
                        }
                    }
                }
                _ => {}
            }
            i = end + 1;
        } else {
            let next = html[i..].find('<').map(|p| i + p).unwrap_or(b.len());
            let chunk = decode_entities(&html[i..next]);
            // whitespace in HTML is just a space
            let mut last_space = out.ends_with(' ') || out.ends_with('\n') || out.is_empty();
            for c in chunk.chars() {
                if c.is_whitespace() && c != '\u{a0}' {
                    if !last_space {
                        out.push(' ');
                        last_space = true;
                    }
                } else {
                    out.push(if c == '\u{a0}' { ' ' } else { c });
                    last_space = false;
                }
            }
            i = next;
        }
    }
    // tidy: trim line ends, at most one empty line in a row
    let mut tidy = String::new();
    let mut blank = 0;
    for l in out.lines() {
        let l = l.trim();
        if l.is_empty() {
            blank += 1;
            if blank > 1 || tidy.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        tidy.push_str(l);
        tidy.push('\n');
    }
    tidy.trim_end().to_string()
}

// ---------------------------------------------------------------- the message tree

pub(crate) fn transfer_decode(body: &[u8], enc: &str) -> Vec<u8> {
    match enc.trim().to_ascii_lowercase().as_str() {
        "base64" => base64_decode(body),
        "quoted-printable" => qp_decode(body, false),
        _ => body.to_vec(),
    }
}

pub(crate) fn split_multipart<'a>(body: &'a [u8], boundary: &str) -> Vec<&'a [u8]> {
    let delim = format!("--{boundary}");
    let mut parts = Vec::new();
    let mut start: Option<usize> = None;
    let mut i = 0;
    while i < body.len() {
        let end = body[i..].iter().position(|&c| c == b'\n').map(|p| i + p).unwrap_or(body.len());
        let line = &body[i..end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(delim.as_bytes()) {
            let tail = &line[delim.len()..];
            let is_end = tail.starts_with(b"--");
            if tail.iter().all(|c| c.is_ascii_whitespace()) || is_end {
                if let Some(s) = start {
                    // the line break before the boundary belongs to it
                    let mut e = i;
                    if e > s && body[e - 1] == b'\n' {
                        e -= 1;
                    }
                    if e > s && body[e - 1] == b'\r' {
                        e -= 1;
                    }
                    parts.push(&body[s..e.max(s)]);
                }
                if is_end {
                    return parts;
                }
                start = Some((end + 1).min(body.len()));
            }
        }
        i = end + 1;
    }
    if let Some(s) = start {
        parts.push(&body[s..]);
    }
    parts
}

#[derive(Default)]
struct Walk {
    plain: Option<String>,
    html: Option<String>,
    attachments: Vec<Attachment>,
}

fn clean_name(n: &str) -> String {
    let n: String = n.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '\0'..='\x1f') { '_' } else { c }).collect();
    let n = n.trim().trim_start_matches('.').to_string();
    if n.is_empty() {
        "attachment".into()
    } else {
        n
    }
}

fn ext_for(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "application/pdf" => "pdf",
        "text/calendar" => "ics",
        "text/plain" => "txt",
        "text/html" => "html",
        "message/rfc822" => "eml",
        _ => "bin",
    }
}

fn walk(part: &[u8], w: &mut Walk, depth: usize) {
    let (h, body) = split_message(part);
    let (ctype, cparams) = parse_params(header(&h, "content-type").unwrap_or("text/plain"));
    let ctype = if ctype.is_empty() { "text/plain".to_string() } else { ctype };
    let (disp, dparams) = parse_params(header(&h, "content-disposition").unwrap_or(""));
    let enc = header(&h, "content-transfer-encoding").unwrap_or("7bit");
    if ctype.starts_with("multipart/") && depth < 20 {
        if let Some(b) = param(&cparams, "boundary") {
            for p in split_multipart(body, b) {
                walk(p, w, depth + 1);
            }
            return;
        }
    }
    let filename = param(&dparams, "filename").or_else(|| param(&cparams, "name")).map(clean_name);
    let is_attachment = disp == "attachment" || filename.is_some() || !(ctype.starts_with("text/plain") || ctype.starts_with("text/html"));
    let data = transfer_decode(body, enc);
    if !is_attachment {
        let text = decode_charset(&data, param(&cparams, "charset").unwrap_or("utf-8"));
        if ctype == "text/plain" && w.plain.is_none() {
            w.plain = Some(text);
            return;
        }
        if ctype == "text/html" && w.html.is_none() {
            w.html = Some(text);
            return;
        }
    }
    if data.is_empty() {
        return;
    }
    let name = filename.unwrap_or_else(|| {
        if ctype == "message/rfc822" {
            let (h2, _) = split_message(&data);
            let s = header(&h2, "subject").map(decode_words).unwrap_or_default();
            format!("{}.eml", clean_name(if s.is_empty() { "message" } else { &s }))
        } else {
            format!("{}-{}.{}", if disp == "inline" || header(&h, "content-id").is_some() { "inline" } else { "attachment" }, w.attachments.len() + 1, ext_for(&ctype))
        }
    });
    let inline = disp == "inline" || (disp.is_empty() && header(&h, "content-id").is_some());
    let cid = header(&h, "content-id").unwrap_or("").trim().trim_start_matches('<').trim_end_matches('>').trim().to_string();
    w.attachments.push(Attachment { name, mime: ctype, data, inline, cid, kind: AttKind::Content });
}

/// Read a whole message.
pub fn parse(raw: &[u8]) -> Mail {
    let (h, _) = split_message(raw);
    let get = |n: &str| header(&h, n).map(decode_words).unwrap_or_default();
    let mut w = Walk::default();
    walk(raw, &mut w, 0);
    signature::classify(&mut w.attachments, w.html.as_deref());
    let text = match (w.plain, w.html) {
        (Some(p), _) if !p.trim().is_empty() => p.replace("\r\n", "\n"),
        (_, Some(h)) => html_to_text(&h),
        (p, _) => p.unwrap_or_default().replace("\r\n", "\n"),
    };
    let date_raw = get("date");
    Mail {
        subject: get("subject"),
        from: get("from"),
        to: get("to"),
        cc: get("cc"),
        date: parse_date(&date_raw),
        date_raw,
        message_id: header(&h, "message-id").unwrap_or("").trim().to_string(),
        reply_to: get("reply-to"),
        references: header(&h, "references").unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" "),
        text: text.trim_end().to_string(),
        attachments: w.attachments,
    }
}

/// Only the name part of an address list: `"Müller, Hans" <h@x.de>` → `Müller, Hans`.
pub fn display_name(addr: &str) -> String {
    let a = addr.trim();
    match a.find('<') {
        Some(p) if p > 0 => a[..p].trim().trim_matches('"').trim().to_string(),
        _ => a.trim_matches(|c| c == '<' || c == '>').to_string(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // spec: IMP-010
    #[test]
    fn decoders() {
        assert_eq!(base64_decode(b"SGFsbG8g\r\nV2VsdA=="), b"Hallo Welt");
        assert_eq!(qp_decode(b"Gr=C3=BC=C3=9Fe=\r\n an alle", false), "Grüße an alle".as_bytes());
        assert_eq!(decode_words("=?utf-8?Q?Beamer_im_H=C3=B6rsaal?= =?UTF-8?B?IOKAkyBTdGF0dXM=?="), "Beamer im Hörsaal – Status");
        assert_eq!(decode_words("Re: =?iso-8859-1?q?Pl=E4ne?= Bau B"), "Re: Pläne Bau B");
        assert_eq!(decode_words("plain =? not encoded"), "plain =? not encoded");
        assert_eq!(decode_charset(&[0x80, 0x20, 0x93, 0x61, 0x94], "windows-1252"), "€ “a”");
        assert_eq!(decode_charset(&[0xA4], "iso-8859-15"), "€");
        assert_eq!(decode_charset(&[0xFC], "utf-8"), "ü", "broken UTF-8 is read as Windows-1252");
    }

    // spec: IMP-010
    #[test]
    fn params_and_dates() {
        let (m, p) = parse_params("attachment; filename*0*=UTF-8''Grundri%C3%9F%20EG; filename*1*=%2Epdf");
        assert_eq!(m, "attachment");
        assert_eq!(param(&p, "filename"), Some("Grundriß EG.pdf"));
        let (m, p) = parse_params("text/plain; charset=\"ISO-8859-1\"; format=flowed");
        assert_eq!(m, "text/plain");
        assert_eq!(param(&p, "charset"), Some("ISO-8859-1"));
        let (_, p) = parse_params("application/pdf; name=\"=?UTF-8?B?UGzDpG5lLnBkZg==?=\"");
        assert_eq!(param(&p, "name"), Some("Pläne.pdf"));
        let d = parse_date("Thu, 13 Nov 2026 10:22:33 +0100 (CET)").unwrap();
        assert_eq!(d, t::days_from_civil(2026, 11, 13) * 86_400 + 9 * 3600 + 22 * 60 + 33);
        assert_eq!(parse_date("13 Nov 26 10:22 GMT"), Some(t::days_from_civil(2026, 11, 13) * 86_400 + 10 * 3600 + 22 * 60));
        assert_eq!(parse_date("nonsense"), None);
    }

    // spec: IMP-010
    #[test]
    fn html_mails_become_readable_text() {
        let t = html_to_text("<html><head><style>p{color:red}</style></head><body><p>Hallo&nbsp;Team,</p><p>die <b>Pläne</b> sind\n  da:<br>Bau&nbsp;B</p><ul><li>Beamer</li><li>Audio</li></ul><a href=\"https://example.org/x\">Link</a><!-- x --></body></html>");
        assert_eq!(t, "Hallo Team,\n\ndie Pläne sind da:\nBau B\n\n- Beamer\n- Audio\n\n[Link](https://example.org/x)");
    }

    // spec: IMP-010
    #[test]
    fn html_entities_are_decoded_once_and_what_is_not_an_entity_stays() {
        assert_eq!(decode_entities("Tom &amp; Jerry &lt;b&gt; &quot;x&quot; &apos;y&apos;"), "Tom & Jerry <b> \"x\" 'y'");
        assert_eq!(
            decode_entities("&auml;&ouml;&uuml;&Auml;&Ouml;&Uuml;&szlig; &euro; &eacute;&egrave;&agrave; &ndash;&mdash;&hellip; &laquo;&raquo;&bdquo;&ldquo;&rdquo;&lsquo;&rsquo; &copy;&reg;&middot;&bull;&deg;"),
            "äöüÄÖÜß € éèà –—… «»„“”‘’ ©®·•°"
        );
        assert_eq!(decode_entities("&#65;&#x42;&#X43; &#x1F600; &#228;"), "ABC 😀 ä");
        // decoded once: the result is not looked at again
        assert_eq!(decode_entities("&amp;lt; &amp;amp;"), "&lt; &amp;");
        // not entities: unknown names, a missing semicolon, a far too long name, no digits, a surrogate, past the last character
        for t in ["&foo;", "&AMP;", "AT&T", "a & b", "&amp", "&;", "&#;", "&#x;", "&#xD800;", "&#x110000;", "&#-1;", "&#12ab;", "&averyverylongname;", "&", "&&", "trailing &"] {
            assert_eq!(decode_entities(t), t, "{t}");
        }
        assert_eq!(decode_entities("a &foo; &amp; b"), "a &foo; & b", "one unknown entity does not stop the next");
        assert_eq!(decode_entities("&&amp;"), "&&");
        assert_eq!(decode_entities(""), "");
        assert_eq!(entity("nbsp").as_deref(), Some(" "));
        assert_eq!(entity("#0").as_deref(), Some("\0"));
        assert_eq!(entity("nope"), None);
    }

    // spec: IMP-010
    #[test]
    fn dates_in_the_many_shapes_mail_programs_write_them() {
        let at = |y: i32, m: u32, d: u32, h: i64, mi: i64, s: i64| t::days_from_civil(y, m, d) * 86_400 + h * 3600 + mi * 60 + s;
        let cases = [
            ("Thu, 13 Nov 2026 10:22:33 +0100 (CET)", Some(at(2026, 11, 13, 9, 22, 33))),
            ("13 Nov 2026 10:22 -0500", Some(at(2026, 11, 13, 15, 22, 0))),
            ("13 Nov 2026 10:22:33 -0930", Some(at(2026, 11, 13, 19, 52, 33))),
            ("13 Nov 2026 10:22:33 +0530", Some(at(2026, 11, 13, 4, 52, 33))),
            ("Thu, 13 Nov 2026 10:22:33", Some(at(2026, 11, 13, 10, 22, 33))),
            ("13 Nov 2026 10 +0000", Some(at(2026, 11, 13, 10, 0, 0))),
            ("13 November 2026 10:00 +0000", Some(at(2026, 11, 13, 10, 0, 0))),
            ("13 NOV 2026 10:00 +0000", Some(at(2026, 11, 13, 10, 0, 0))),
            ("Fri, 14 Nov 2025 08:00:00 EST", Some(at(2025, 11, 14, 13, 0, 0))),
            ("14 Nov 2025 08:00 EDT", Some(at(2025, 11, 14, 12, 0, 0))),
            ("14 Nov 2025 08:00 CST", Some(at(2025, 11, 14, 14, 0, 0))),
            ("14 Nov 2025 08:00 CDT", Some(at(2025, 11, 14, 13, 0, 0))),
            ("14 Nov 2025 08:00 MST", Some(at(2025, 11, 14, 15, 0, 0))),
            ("14 Nov 2025 08:00 MDT", Some(at(2025, 11, 14, 14, 0, 0))),
            ("14 Nov 2025 08:00 PST", Some(at(2025, 11, 14, 16, 0, 0))),
            ("14 Nov 2025 08:00 PDT", Some(at(2025, 11, 14, 15, 0, 0))),
            ("14 Nov 2025 08:00 cet", Some(at(2025, 11, 14, 7, 0, 0))),
            ("14 Aug 2025 08:00 CEST", Some(at(2025, 8, 14, 6, 0, 0))),
            ("14 Nov 2025 08:00 XYZ", Some(at(2025, 11, 14, 8, 0, 0))),
            ("14 Nov 2025 08:00 +05", Some(at(2025, 11, 14, 8, 0, 0))),
            ("1 Jan 99 00:00 GMT", Some(at(1999, 1, 1, 0, 0, 0))),
            ("1 Jan 49 00:00 GMT", Some(at(2049, 1, 1, 0, 0, 0))),
            ("1 Jan 50 00:00 GMT", Some(at(1950, 1, 1, 0, 0, 0))),
            ("  Sun,   2 Nov   2026   23:59:59   +0000  ", Some(at(2026, 11, 2, 23, 59, 59))),
            ("29 Feb 2028 12:00 +0000", Some(at(2028, 2, 29, 12, 0, 0))),
            ("", None),
            ("nonsense", None),
            ("13 Nov", None),
            ("13 Nov 2026", None),
            ("13 Foo 2026 10:00", None),
            ("x Nov 2026 10:00", None),
            ("13 Nov year 10:00", None),
            ("0 Nov 2026 10:00", None),
            ("32 Nov 2026 10:00", None),
            ("13 Nov 2026 24:00", None),
            ("13 Nov 2026 10:60", None),
            ("(comment only)", None),
        ];
        for (s, want) in cases {
            assert_eq!(parse_date(s), want, "{s:?}");
        }
    }

    // spec: IMP-010
    #[test]
    fn charsets_are_read_by_name_in_any_spelling_and_a_lying_label_is_forgiven() {
        for name in ["iso-8859-1", "ISO-8859-1", "latin1", "Latin-1", "iso8859-1", "l1", " \"iso-8859-1\" "] {
            assert_eq!(decode_charset(&[0x48, 0xE4, 0xFC, 0xDF, 0xA4], name), "Häüß¤", "{name}");
        }
        assert_eq!(decode_charset(&[0xA4, 0xA6, 0xA8, 0xB4, 0xB8, 0xBC, 0xBD, 0xBE, 0xE4], "iso-8859-15"), "€ŠšŽžŒœŸä");
        assert_eq!(decode_charset(&[0xA4], "latin9"), "€");
        assert_eq!(decode_charset(&[0xA4], "iso8859-15"), "€");
        let all: Vec<u8> = (0x80..0xA0).collect();
        let s = decode_charset(&all, "cp1252");
        assert_eq!(s.chars().count(), 32);
        assert!(s.starts_with('€') && s.ends_with('Ÿ'));
        assert_eq!(decode_charset(&[0x81, 0x8d, 0x8f, 0x90, 0x9d], "x-cp1252"), "\u{81}\u{8d}\u{8f}\u{90}\u{9d}", "the holes of Windows-1252 stay control characters");
        assert_eq!(decode_charset(&[0xe4, 0x80], "Windows-1252"), "ä€");
        // UTF-8, US-ASCII and no label: read as UTF-8, else (the label lied) as Windows-1252
        for cs in ["utf-8", "UTF-8", "us-ascii", "", "\"utf-8\""] {
            assert_eq!(decode_charset("Grüße €".as_bytes(), cs), "Grüße €", "{cs:?}");
            assert_eq!(decode_charset(&[b'a', 0xfc, 0x80], cs), "aü€", "{cs:?} that is not UTF-8");
        }
        // a charset we do not know: UTF-8 if it is, else the bytes with replacement characters
        assert_eq!(decode_charset("Grüße".as_bytes(), "koi8-r"), "Grüße");
        assert_eq!(decode_charset(&[b'a', 0xff, b'b'], "koi8-r"), "a\u{fffd}b");
        assert_eq!(decode_charset(&[], "iso-8859-2"), "");
        assert_eq!(decode_charset(&[], "iso-8859-1"), "");
    }

    // spec: IMP-010
    #[test]
    fn html_to_text_handles_links_comments_scripts_odd_markup_and_cut_off_input() {
        let h = html_to_text;
        // links: markdown unless the text is the address, there is no usable address or no text
        assert_eq!(h("<a href=\"https://x.org/a?b=1&amp;c=2\">the page</a>"), "[the page](https://x.org/a?b=1&c=2)");
        assert_eq!(h("<A HREF='https://x.org'>single</A>"), "[single](https://x.org)");
        assert_eq!(h("<a href=https://x.org>bare</a>"), "[bare](https://x.org)");
        assert_eq!(h("<a href=\"https://x.org\">https://x.org</a>"), "https://x.org");
        assert_eq!(h("<a href=\"mailto:ada@x.org\">ada@x.org</a> and <a href=\"mailto:ada@x.org\">write to Ada</a>"), "ada@x.org and [write to Ada](mailto:ada@x.org)");
        assert_eq!(h("<a href=\"javascript:alert(1)\">click</a>"), "click", "no script links");
        assert_eq!(h("<a href=\"\">empty</a><a>none</a><a href=\"https://x.org\"></a>"), "emptynone");
        assert_eq!(h("<a href=\"https://x.org\">never closed"), "never closed");
        // what is not text: scripts, styles, the head, title, comments (also one that never ends)
        assert_eq!(h("<title>T</title><script>if (a<b) x()</script><STYLE>a{}</STYLE>text"), "text");
        assert_eq!(h("<head><meta charset=utf-8></head><body>x</body>"), "x");
        assert_eq!(h("a<!-- hidden <b>x</b> -->b"), "ab");
        assert_eq!(h("a<!-- never ends"), "a");
        assert_eq!(h("a<style>never ends"), "a");
        // structure
        assert_eq!(h("one<br/>two<BR>three"), "one\ntwo\nthree");
        assert_eq!(h("<div>a</div><div>b</div>"), "a\nb");
        assert_eq!(h("<h1>T</h1><p>x</p><hr>y"), "T\n\nx\n\n———\ny");
        assert_eq!(h("<table><tr><td>a</td><td>b</td></tr><tr><th>c</th><td>d</td></tr></table>"), "a  b\nc  d");
        assert_eq!(h("<ol><li>x<li>y</ol>"), "- x\n- y");
        assert_eq!(h("<blockquote>q</blockquote>after"), "q\n\nafter");
        // white space: runs collapse, no-break spaces are spaces, blank lines are at most one, ends are trimmed
        assert_eq!(h("  a \n\t b&nbsp;&nbsp;c  "), "a b c");
        assert_eq!(h("<p>a</p><p></p><p></p><p>b</p>"), "a\n\nb");
        assert_eq!(h("<p>   </p>"), "");
        // odd and cut-off markup never panics: what follows an unfinished tag is dropped
        for t in [
            "",
            "<",
            "<>",
            "</>",
            "x <",
            "a < b",
            "<a",
            "<a href=",
            "<a href=\"",
            "<a href=\"x\"",
            "<p",
            "</p",
            "<!--",
            "<!-",
            "<!",
            "<![CDATA[x]]>y",
            "&",
            "&#",
            "<?xml version=\"1.0\"?>x",
            "<a href=\"é\">é</a>é",
            "\u{a0}",
            "<b><i>x</b></i>",
        ] {
            let _ = h(t);
        }
        assert_eq!(h("x <"), "x");
        assert_eq!(h("é<b>ü</b>"), "éü");
        assert_eq!(h("<![CDATA[x]]>y"), "y");
    }

    pub(crate) fn sample_eml() -> Vec<u8> {
        let pdf = "JVBERi0xLjQKJcOkw7zDtsOfCg=="; // a few bytes standing in for a PDF
        let png = "iVBORw0KGgo=";
        format!(
            "From - Thu Nov 13 10:30:00 2026\r\n\
X-Mozilla-Status: 0001\r\n\
Message-ID: <abc123@uni.example>\r\n\
Date: Thu, 13 Nov 2026 10:22:33 +0100\r\n\
From: \"=?UTF-8?Q?M=C3=BCller=2C_Hans?=\" <h.mueller@arch.example>\r\n\
To: Medientechnik <mt@uni.example>\r\n\
Cc: Bauamt <bau@uni.example>\r\n\
Subject: =?UTF-8?Q?Neubau_H=C3=B6rsaal_B_=E2=80=93_Pl=C3=A4ne?=\r\n\
\x20Stand 3\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"outer\"\r\n\
\r\n\
This is a multi-part message in MIME format.\r\n\
--outer\r\n\
Content-Type: multipart/alternative; boundary=\"alt\"\r\n\
\r\n\
--alt\r\n\
Content-Type: text/plain; charset=UTF-8\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
Hallo zusammen,=0D=0A=0D=0Aanbei die Pl=C3=A4ne f=C3=BCr den H=C3=B6rsaal B.=\r\n\
\x20Gr=C3=BC=C3=9Fe\r\n\
--alt\r\n\
Content-Type: multipart/related; boundary=\"rel\"\r\n\
\r\n\
--rel\r\n\
Content-Type: text/html; charset=UTF-8\r\n\
\r\n\
<p>Hallo zusammen,</p><img src=\"cid:logo\">\r\n\
--rel\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo>\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
{png}\r\n\
--rel--\r\n\
--alt--\r\n\
--outer\r\n\
Content-Type: application/pdf; name=\"plan.pdf\"\r\n\
Content-Disposition: attachment;\r\n\
\x20filename*=UTF-8''Grundri%C3%9F%20EG.pdf\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
{pdf}\r\n\
--outer\r\n\
Content-Type: message/rfc822\r\n\
\r\n\
Subject: Alte Mail\r\n\
\r\n\
Text\r\n\
--outer--\r\n"
        )
        .into_bytes()
    }

    // spec: IMP-010
    #[test]
    fn a_thunderbird_message_with_attachments() {
        let m = parse(&sample_eml());
        assert_eq!(m.subject, "Neubau Hörsaal B – Pläne Stand 3");
        assert_eq!(m.from, "\"Müller, Hans\" <h.mueller@arch.example>");
        assert_eq!(display_name(&m.from), "Müller, Hans");
        assert_eq!(m.to, "Medientechnik <mt@uni.example>");
        assert_eq!(m.cc, "Bauamt <bau@uni.example>");
        assert_eq!(m.message_id, "<abc123@uni.example>");
        assert_eq!(m.date, parse_date("Thu, 13 Nov 2026 10:22:33 +0100"));
        assert_eq!(m.text, "Hallo zusammen,\r\n\r\nanbei die Pläne für den Hörsaal B. Grüße".replace("\r\n", "\n"));
        let names: Vec<(&str, bool)> = m.attachments.iter().map(|a| (a.name.as_str(), a.inline)).collect();
        assert_eq!(names, vec![("inline-1.png", true), ("Grundriß EG.pdf", false), ("Alte Mail.eml", false)]);
        assert!(m.attachments[1].data.starts_with(b"%PDF-1.4"));
        assert_eq!(m.attachments[0].data, b"\x89PNG\r\n\x1a\n");
    }

    // spec: IMP-010
    #[test]
    fn html_only_and_single_part_messages() {
        let m = parse(b"Subject: Hi\nContent-Type: text/html; charset=iso-8859-1\nContent-Transfer-Encoding: quoted-printable\n\n<div>Gr=FC=DFe</div>");
        assert_eq!(m.text, "Grüße");
        let m = parse(b"Subject: plain\n\nJust text\n");
        assert_eq!((m.subject.as_str(), m.text.as_str(), m.attachments.len()), ("plain", "Just text", 0));
        let m = parse(b"");
        assert!(m.subject.is_empty() && m.text.is_empty());
    }
}
