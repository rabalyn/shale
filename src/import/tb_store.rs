//! Finding a message in Thunderbird's own storage on this computer, for drags where
//! Thunderbird hands over only a link to the message (`mailbox-message://…#offset`,
//! `imap-message://…#uid`, `imap://…/fetch>UID>/INBOX>uid`) and its file name.
//!
//! Local folders and POP accounts: the link holds the message's position in the folder file,
//! so it is found exactly. IMAP accounts: Thunderbird keeps copies of the messages on this
//! computer (on by default); they are found by the subject from the file name, and when several
//! messages share it, all of them are offered (the newest ticked).

use crate::mail;
use std::cell::Cell;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Clone)]
pub struct MsgRef {
    pub imap: bool,
    pub user: String,
    pub host: String,
    /// folder names from the top, e.g. ["INBOX", "Projects"]
    pub folder: Vec<String>,
    /// local folders: byte position in the folder file; IMAP: the message's UID
    pub key: Option<u64>,
    /// `mailbox:///path?number=` links name the folder file directly
    pub file: Option<PathBuf>,
}

fn pct(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = |c: u8| (c as char).to_digit(16);
            if let (Some(x), Some(y)) = (h(b[i + 1]), h(b[i + 2])) {
                out.push((x * 16 + y) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn digits(s: &str) -> Option<u64> {
    let d: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    d.parse().ok()
}

/// Read a Thunderbird message link.
pub fn parse_link(link: &str) -> Option<MsgRef> {
    let link = link.trim();
    let (scheme, rest) = link.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme == "mailbox" && rest.starts_with('/') {
        // mailbox:///home/u/.thunderbird/x/Mail/Local%20Folders/Inbox?number=1234
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let key = query.split('&').find_map(|kv| kv.strip_prefix("number=")).and_then(digits);
        return Some(MsgRef { imap: false, user: String::new(), host: String::new(), folder: vec![], key, file: Some(PathBuf::from(pct(path))) });
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (user, host) = match authority.rsplit_once('@') {
        Some((u, h)) => (pct(u), pct(h)),
        None => (String::new(), pct(authority)),
    };
    let host = host.split(':').next().unwrap_or("").to_string();
    match scheme.as_str() {
        "mailbox-message" | "mailbox" | "imap-message" => {
            let (folder, key) = match path.split_once('#') {
                Some((f, k)) => (f, digits(k)),
                None => (path.split('?').next().unwrap_or(path), path.split_once("number=").and_then(|(_, k)| digits(k))),
            };
            let folder = folder.split('/').filter(|s| !s.is_empty()).map(pct).collect();
            Some(MsgRef { imap: scheme == "imap-message", user, host, folder, key, file: None })
        }
        "imap" => {
            // imap://user@host:993/fetch>UID>/INBOX/Sub>1234
            let p = pct(path);
            let after = p.split_once("UID>").map(|x| x.1)?;
            let delim = after.chars().next()?;
            let body = &after[delim.len_utf8()..];
            let (folder, uid) = body.rsplit_once('>')?;
            let folder = folder.split(delim).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
            Some(MsgRef { imap: true, user, host, folder, key: digits(uid), file: None })
        }
        _ => None,
    }
}

/// Where Thunderbird keeps its profiles.
pub fn default_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    [".thunderbird", ".var/app/org.mozilla.Thunderbird/.thunderbird", ".var/app/net.thunderbird.Thunderbird/.thunderbird", "snap/thunderbird/common/.thunderbird", ".mozilla-thunderbird"]
        .iter()
        .map(|p| home.join(p))
        .filter(|p| p.is_dir())
        .collect()
}

/// A mail account's folder on disk, from the profile's prefs.js.
#[derive(Debug, Clone)]
struct Server {
    host: String,
    user: String,
    kind: String,
    dir: PathBuf,
}

fn servers(profile: &Path) -> Vec<Server> {
    let Ok(prefs) = crate::fsutil::read_to_string(profile.join("prefs.js")) else { return vec![] };
    let mut map: std::collections::BTreeMap<String, Server> = Default::default();
    for line in prefs.lines() {
        let Some(rest) = line.trim().strip_prefix("user_pref(\"mail.server.") else { continue };
        let Some((key, val)) = rest.split_once("\",") else { continue };
        let val = val.trim().trim_end_matches(");").trim().trim_matches('"').replace("\\\\", "\\");
        let Some((id, field)) = key.split_once('.') else { continue };
        let s = map.entry(id.to_string()).or_insert(Server { host: String::new(), user: String::new(), kind: String::new(), dir: PathBuf::new() });
        match field {
            "hostname" => s.host = val,
            "userName" => s.user = val,
            "type" => s.kind = val,
            "directory-rel" if s.dir.as_os_str().is_empty() || !s.dir.is_dir() => s.dir = profile.join(val.trim_start_matches("[ProfD]")),
            "directory-rel" => {}
            "directory" => s.dir = PathBuf::from(val),
            _ => {}
        }
    }
    map.into_values().filter(|s| !s.host.is_empty()).collect()
}

fn profiles(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for r in roots {
        if r.join("prefs.js").is_file() {
            out.push(r.clone());
        }
        if let Ok(rd) = std::fs::read_dir(r) {
            for e in rd.flatten() {
                if e.path().join("prefs.js").is_file() {
                    out.push(e.path());
                }
            }
        }
    }
    out
}

/// Account folders that may hold the message (best match first).
fn server_dirs(roots: &[PathBuf], r: &MsgRef) -> Vec<PathBuf> {
    let mut exact = Vec::new();
    let mut loose = Vec::new();
    for p in profiles(roots) {
        for s in servers(&p) {
            if !s.host.eq_ignore_ascii_case(&r.host) || (r.imap != (s.kind == "imap")) {
                continue;
            }
            if r.user.is_empty() || s.user.eq_ignore_ascii_case(&r.user) {
                exact.push(s.dir);
            } else {
                loose.push(s.dir);
            }
        }
        // without usable prefs: the usual folder names
        let base = p.join(if r.imap { "ImapMail" } else { "Mail" });
        if let Ok(rd) = std::fs::read_dir(&base) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_lowercase();
                let h = r.host.to_lowercase();
                if n == h || n.strip_prefix(&h).map(|t| t.starts_with('-')).unwrap_or(false) {
                    loose.push(e.path());
                }
            }
        }
    }
    exact.extend(loose);
    exact.dedup();
    exact
}

/// A child of `dir` named like `name` (case does not matter).
fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false))
}

/// The folder's file (mbox) or directory (maildir): "A/B" is stored as "A.sbd/B".
fn folder_path(server: &Path, folder: &[String]) -> Option<PathBuf> {
    let (last, parents) = folder.split_last()?;
    let mut p = server.to_path_buf();
    for f in parents {
        p = child(&p, &format!("{f}.sbd"))?;
    }
    child(&p, last)
}

/// Undo the mbox "From " line and its >From escaping.
fn clean_message(mut raw: Vec<u8>) -> Vec<u8> {
    if raw.starts_with(b"From ") {
        let cut = raw.iter().position(|&c| c == b'\n').map(|p| p + 1).unwrap_or(raw.len());
        raw.drain(..cut);
    }
    let mut out = Vec::with_capacity(raw.len());
    for line in raw.split_inclusive(|&c| c == b'\n') {
        out.extend_from_slice(if line.starts_with(b">From ") { &line[1..] } else { line });
    }
    out
}

/// A header line: a field name (printable ASCII except the colon, RFC 5322) right before a colon.
fn is_header_line(line: &[u8]) -> bool {
    let name = line.iter().position(|&c| c == b':').unwrap_or(0);
    name > 0 && line[..name].iter().all(|c| (33..=126).contains(c))
}

/// Thunderbird marked the message deleted (flag 0x0008 of `X-Mozilla-Status`) but has not compacted
/// the folder yet, so its text is still in the file.
fn is_deleted(headers: &str) -> bool {
    mail::header_value(headers, "x-mozilla-status").and_then(|v| u32::from_str_radix(v.trim(), 16).ok()).map(|f| f & 0x0008 != 0).unwrap_or(false)
}

/// The message starting at `offset` in an mbox file.
fn read_at(file: &Path, offset: u64) -> Option<Vec<u8>> {
    let mut f = crate::fsutil::open_read(file).ok()?;
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut r = BufReader::new(f.take(200 << 20));
    let mut out = Vec::new();
    let mut line = Vec::new();
    let mut first = true;
    let mut prev_blank = true;
    loop {
        line.clear();
        if r.read_until(b'\n', &mut line).ok()? == 0 {
            break;
        }
        if !first && prev_blank && line.starts_with(b"From ") {
            break;
        }
        if first && !(line.starts_with(b"From ") || is_header_line(&line)) {
            return None; // not the start of a message (the folder was compacted?)
        }
        first = false;
        prev_blank = line.iter().all(|c| c.is_ascii_whitespace());
        out.extend_from_slice(&line);
    }
    (!out.is_empty()).then(|| clean_message(out))
}

/// Letters and digits only, lower case: "Re_ Hörsaal B – Pläne.eml" ≈ "Re: Hörsaal B – Pläne".
fn fold(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn subject_matches(subject: &str, wanted: &str) -> bool {
    let (a, b) = (fold(subject), fold(wanted));
    !b.is_empty() && (a == b || (b.chars().count() >= 12 && a.starts_with(&b)))
}

/// Messages in an mbox file whose headers pass `pick`.
fn scan_mbox(file: &Path, pick: &dyn Fn(&str) -> bool) -> Vec<Vec<u8>> {
    let Ok(f) = crate::fsutil::open_read(file) else { return vec![] };
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut found = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut in_headers = false;
    let mut header_text = String::new();
    let mut keep = false;
    let mut prev_blank = true;
    let mut line = Vec::new();
    let finish = |cur: &mut Vec<u8>, keep: bool, found: &mut Vec<Vec<u8>>| {
        if keep && !cur.is_empty() {
            found.push(clean_message(std::mem::take(cur)));
        }
        cur.clear();
    };
    loop {
        line.clear();
        match r.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if prev_blank && line.starts_with(b"From ") {
            finish(&mut cur, keep, &mut found);
            keep = false;
            in_headers = true;
            header_text.clear();
        }
        let blank = line.iter().all(|c| c.is_ascii_whitespace());
        if in_headers {
            if blank {
                in_headers = false;
                keep = pick(&header_text);
            } else {
                header_text.push_str(&String::from_utf8_lossy(&line));
            }
        }
        // only keep the text of messages that may be the one
        if in_headers || keep {
            cur.extend_from_slice(&line);
        } else if !cur.is_empty() && !keep {
            cur.clear();
        }
        prev_blank = blank;
    }
    finish(&mut cur, keep, &mut found);
    found
}

/// Messages in a maildir folder whose headers pass `pick`.
fn scan_maildir(dir: &Path, pick: &dyn Fn(&str) -> bool) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for sub in ["cur", "new"] {
        let Ok(rd) = std::fs::read_dir(dir.join(sub)) else { continue };
        for e in rd.flatten() {
            let Ok(mut f) = crate::fsutil::open_read(e.path()) else { continue };
            let mut head = vec![0u8; 64 * 1024];
            let n = f.read(&mut head).unwrap_or(0);
            head.truncate(n);
            let text = String::from_utf8_lossy(&head).replace("\r\n", "\n");
            let headers = text.split("\n\n").next().unwrap_or("").to_string();
            if pick(&headers) {
                if let Ok(all) = crate::fsutil::read(e.path()) {
                    out.push(clean_message(all));
                }
            }
        }
    }
    out
}

/// Messages in a folder whose headers pass `pick`; copies marked deleted are skipped, but a
/// skipped copy that `pick` would have taken sets `saw_deleted` (IMP-030).
fn scan(place: &Path, pick: &dyn Fn(&str) -> bool, saw_deleted: &Cell<bool>) -> Vec<Vec<u8>> {
    let live = |h: &str| {
        if !pick(h) {
            return false;
        }
        if is_deleted(h) {
            saw_deleted.set(true);
            return false;
        }
        true
    };
    if place.is_dir() {
        scan_maildir(place, &live)
    } else {
        scan_mbox(place, &live)
    }
}

fn bare_id(s: &str) -> String {
    s.trim().trim_start_matches('<').trim_end_matches('>').trim().to_string()
}

fn has_id(headers: &str, id: &str) -> bool {
    mail::header_value(headers, "message-id").map(|m| bare_id(&m) == bare_id(id)).unwrap_or(false)
}

fn has_subject(headers: &str, wanted: &str) -> bool {
    mail::header_value(headers, "subject").map(|s| subject_matches(&mail::decode_words(&s), wanted)).unwrap_or(false)
}

// ---------------------------------------------------------------- the folder index (.msf)

/// Read a Mork value up to its closing ')': `\` escapes the next character (or a line break),
/// `$xx` is a byte. Returns the value and the position after the ')'.
fn mork_literal(b: &[u8], mut i: usize) -> (String, usize) {
    let mut out = Vec::new();
    while i < b.len() {
        match b[i] {
            b')' => return (String::from_utf8_lossy(&out).into_owned(), i + 1),
            b'\\' if i + 1 < b.len() => {
                if b[i + 1] == b'\r' || b[i + 1] == b'\n' {
                    i += 2;
                    while i < b.len() && (b[i] == b'\n' || b[i] == b'\r') {
                        i += 1;
                    }
                    continue;
                }
                out.push(b[i + 1]);
                i += 2;
            }
            b'$' if i + 2 < b.len() => match u8::from_str_radix(&String::from_utf8_lossy(&b[i + 1..i + 3]), 16) {
                Ok(v) => {
                    out.push(v);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'$');
                    i += 1;
                }
            },
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    (String::from_utf8_lossy(&out).into_owned(), i)
}

fn mork_token(b: &[u8], mut i: usize, stop: &[u8]) -> (String, usize) {
    let start = i;
    while i < b.len() && !stop.contains(&b[i]) {
        i += 1;
    }
    (String::from_utf8_lossy(&b[start..i]).trim().to_uppercase(), i)
}

type Rows = std::collections::HashMap<u64, std::collections::HashMap<String, String>>;

/// The rows of a Thunderbird folder index (row id = the message's key, for IMAP its UID):
/// column name → value (e.g. "message-id", "subject", "storeToken" = where its text is).
pub fn mork_rows(b: &[u8]) -> Rows {
    use std::collections::HashMap;
    let mut cols: HashMap<String, String> = HashMap::new();
    let mut vals: HashMap<String, String> = HashMap::new();
    let mut rows: Rows = HashMap::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'@' if b[i..].starts_with(b"@$$") => {
                i += 3;
                while i < b.len() && b[i] != b'@' {
                    i += 1;
                }
                i += 1;
            }
            b'<' => {
                // a dictionary: (id=value) entries; "<(a=c)>" marks the column names
                i += 1;
                let mut is_cols = false;
                while i < b.len() && b[i] != b'>' {
                    match b[i] {
                        b'<' => {
                            let end = b[i..].iter().position(|&c| c == b'>').map(|p| i + p).unwrap_or(b.len());
                            if String::from_utf8_lossy(&b[i..end]).replace(' ', "").contains("a=c") {
                                is_cols = true;
                            }
                            i = end + 1;
                        }
                        b'/' if b.get(i + 1) == Some(&b'/') => {
                            while i < b.len() && b[i] != b'\n' {
                                i += 1;
                            }
                        }
                        b'(' => {
                            let (id, j) = mork_token(b, i + 1, b"=)");
                            if j < b.len() && b[j] == b'=' {
                                let (v, k) = mork_literal(b, j + 1);
                                if is_cols {
                                    cols.insert(id, v)
                                } else {
                                    vals.insert(id, v)
                                };
                                i = k;
                            } else {
                                i = j + 1;
                            }
                        }
                        _ => i += 1,
                    }
                }
                i += 1;
            }
            b'[' => {
                // a row: [id(^col^val)(^col=literal)(name=literal)…]
                let (tok, mut j) = mork_token(b, i + 1, b"([] \t\r\n");
                let cut = tok.starts_with('-');
                let id = u64::from_str_radix(tok.trim_start_matches('-').split(':').next().unwrap_or(""), 16).ok();
                let mut cells: Vec<(String, String)> = Vec::new();
                while j < b.len() && b[j] != b']' {
                    if b[j] == b'(' {
                        let by_ref = b.get(j + 1) == Some(&b'^');
                        let (c, k) = mork_token(b, j + 1 + by_ref as usize, b"^=)");
                        let col = if by_ref { cols.get(&c).cloned().unwrap_or(c) } else { c.to_lowercase() };
                        match b.get(k) {
                            Some(b'^') => {
                                let (v, m) = mork_token(b, k + 1, b")");
                                cells.push((col, vals.get(&v).cloned().unwrap_or_default()));
                                j = m + 1;
                            }
                            Some(b'=') => {
                                let (v, m) = mork_literal(b, k + 1);
                                cells.push((col, v));
                                j = m;
                            }
                            _ => j = k + 1,
                        }
                    } else {
                        j += 1;
                    }
                }
                if let Some(id) = id {
                    if cut {
                        rows.remove(&id);
                    }
                    let row = rows.entry(id).or_default();
                    for (c, v) in cells {
                        row.insert(c, v);
                    }
                }
                i = j + 1;
            }
            b'(' => {
                // a stray cell (table settings): skip it
                let end = b[i..].iter().position(|&c| c == b')').map(|p| i + p).unwrap_or(b.len());
                i = end + 1;
            }
            _ => i += 1,
        }
    }
    rows
}

/// The index next to a folder: "INBOX" → "INBOX.msf".
fn index_of(place: &Path) -> PathBuf {
    let mut n = place.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    n.push(".msf");
    place.with_file_name(n)
}

/// The text of a message the index knows about.
fn by_index(place: &Path, row: &std::collections::HashMap<String, String>, saw_deleted: &Cell<bool>) -> Option<Vec<u8>> {
    let id = row.get("message-id").cloned();
    let ok = |raw: &[u8]| -> bool {
        let text = String::from_utf8_lossy(&raw[..raw.len().min(64 * 1024)]).replace("\r\n", "\n");
        let headers = text.split("\n\n").next().unwrap_or("");
        id.as_ref().map(|m| has_id(headers, m)).unwrap_or(true)
    };
    if let Some(token) = row.get("storetoken").or_else(|| row.get("storeToken")) {
        if place.is_file() {
            if let Some(raw) = token.trim().parse::<u64>().ok().and_then(|o| read_at(place, o)) {
                if ok(&raw) {
                    return Some(raw);
                }
            }
        } else {
            for sub in ["cur", "new"] {
                if let Ok(raw) = crate::fsutil::read(place.join(sub).join(token.trim())) {
                    let raw = clean_message(raw);
                    if ok(&raw) {
                        return Some(raw);
                    }
                }
            }
        }
    }
    // the position is out of date (folder compacted): look for its Message-ID
    let id = id?;
    scan(place, &|h| has_id(h, &id), saw_deleted).into_iter().next()
}

/// Find the dragged message(s): `links` as handed over, `file_name` the suggested
/// "<subject>.eml". Returns the raw messages, or why they could not be found.
pub fn find_in(roots: &[PathBuf], links: &[String], file_name: Option<&str>) -> Result<Vec<Vec<u8>>, String> {
    let refs: Vec<MsgRef> = links.iter().filter_map(|l| parse_link(l)).collect();
    if refs.is_empty() {
        return Err(format!("the link to the message could not be read ({})", links.join(" ")));
    }
    if roots.is_empty() {
        return Err("Thunderbird's profile folder was not found".into());
    }
    let subject = file_name.map(|f| f.trim().trim_end_matches(".eml").trim_end_matches(".EML").to_string()).filter(|s| !s.is_empty());
    let mut why: Vec<String> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut not_offline = false;
    let saw_deleted = Cell::new(false);
    // the dataflow of the Thunderbird store (DEV-030): its only caller is the drop from another program
    let _flow = tracing::info_span!("flow.import_thunderbird", peer = "import").entered();
    for r in &refs {
        let mut places: Vec<PathBuf> = r.file.iter().cloned().collect();
        let find_folder = tracing::info_span!("flow.import_thunderbird.find_folder", peer = "profile").entered();
        for d in server_dirs(roots, r) {
            match folder_path(&d, &r.folder) {
                Some(p) => places.push(p),
                None => why.push(format!("no folder “{}” in {}", r.folder.join("/"), d.display())),
            }
        }
        drop(find_folder);
        for p in places {
            if seen.contains(&p) {
                continue;
            }
            seen.push(p.clone());
            // local folders: the link holds the message's position
            if !r.imap && p.is_file() {
                if let Some(raw) = r.key.and_then(|k| tracing::info_span!("flow.import_thunderbird.read_at_position", peer = "profile").in_scope(|| read_at(&p, k))) {
                    return Ok(vec![raw]);
                }
            }
            // the folder's index knows each message by its key (IMAP: UID)
            let mut index_subject = None;
            if let Some(k) = r.key {
                let _index = tracing::info_span!("flow.import_thunderbird.read_index", peer = "profile").entered();
                let idx = index_of(&p);
                match crate::fsutil::read(&idx) {
                    Ok(bytes) => match mork_rows(&bytes).get(&k) {
                        Some(row) => {
                            if let Some(raw) = by_index(&p, row, &saw_deleted) {
                                return Ok(vec![raw]);
                            }
                            index_subject = row.get("subject").cloned();
                            not_offline = true;
                            why.push(format!("{}: message {k} is in Thunderbird's list, but its text is not stored on this computer", p.display()));
                        }
                        None => why.push(format!("{}: message {k} is not in Thunderbird's index", p.display())),
                    },
                    Err(_) => why.push(format!("{}: no index file", p.display())),
                }
            }
            // last resort: the subject
            for s in subject.iter().chain(index_subject.iter()) {
                let found = tracing::info_span!("flow.import_thunderbird.scan_folder", peer = "profile").in_scope(|| scan(&p, &|h| has_subject(h, s), &saw_deleted));
                if !found.is_empty() {
                    return Ok(found);
                }
            }
            if !p.exists() {
                why.push(format!("{} does not exist", p.display()));
            }
        }
    }
    if saw_deleted.get() {
        return Err("the message was deleted in Thunderbird (Thunderbird has not compacted the folder yet)".into());
    }
    if seen.is_empty() && why.is_empty() {
        why.push(format!("no Thunderbird folder for “{}” on this computer", refs[0].folder.join("/")));
    }
    let hint = if not_offline || refs.iter().any(|r| r.imap) {
        " For IMAP accounts, Thunderbird keeps the text only with “Keep messages for this account on this computer” (Account settings → Synchronization & Storage; also per folder under Folder properties → Synchronization)."
    } else {
        ""
    };
    Err(format!("the message was not found: {}.{hint} (Link: {}; file name: {})", why.join("; "), links.join(" "), file_name.unwrap_or("none")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: IMP-008
    #[test]
    fn reads_thunderbird_folder_indexes() {
        let msf = b"// <!-- <mdb:mork:z v=\"1.4\"/> -->\n< <(a=c)> // (f=iso-8859-1)\n  (80=ns:msg:db:row:scope:msgs:all)(81=subject)(82=message-id)\n  (83=storeToken)>\n<(91=Pl$C3$A4ne \\) Stand\\\n 4)(92=abc@x)>\n{1:^80 {(k^84:c)(s=9)}\n  [1F(^81^91)(^82^92)(^83=1234)]\n  [20:^80(^81=Zweite)(^83=99)]}\n";
        let rows = mork_rows(msf);
        assert_eq!(rows[&0x1f]["subject"], "Pläne ) Stand 4");
        assert_eq!(rows[&0x1f]["message-id"], "abc@x");
        assert_eq!(rows[&0x1f]["storeToken"], "1234");
        assert_eq!(rows[&0x20]["subject"], "Zweite");
    }

    // spec: IMP-008
    #[test]
    fn reads_thunderbird_links() {
        let r = parse_link("mailbox-message://nobody@Local%20Folders/Inbox/Projekte#4711").unwrap();
        assert_eq!((r.imap, r.host.as_str(), r.folder.clone(), r.key), (false, "Local Folders", vec!["Inbox".to_string(), "Projekte".into()], Some(4711)));
        let r = parse_link("imap-message://ada%40uni.example@imap.uni.example/INBOX/Bau#123").unwrap();
        assert_eq!((r.imap, r.user.as_str(), r.host.as_str(), r.folder.clone(), r.key), (true, "ada@uni.example", "imap.uni.example", vec!["INBOX".to_string(), "Bau".into()], Some(123)));
        let r = parse_link("imap://ada@imap.uni.example:993/fetch%3EUID%3E.INBOX.Bau%3E123").unwrap();
        assert_eq!((r.host.as_str(), r.folder.clone(), r.key), ("imap.uni.example", vec!["INBOX".to_string(), "Bau".into()], Some(123)));
        let r = parse_link("mailbox:///home/ada/.thunderbird/p/Mail/Local%20Folders/Inbox?number=99&part=1.2").unwrap();
        assert_eq!((r.file.unwrap(), r.key), (PathBuf::from("/home/ada/.thunderbird/p/Mail/Local Folders/Inbox"), Some(99)));
        assert!(parse_link("https://example.org").is_none());
    }

    fn msg(subject: &str, body: &str) -> String {
        format!("From - Thu Nov 12 10:00:00 2026\nX-Mozilla-Status: 0001\nSubject: {subject}\nMessage-ID: <{}@x>\n\n{body}\n\n", fold(subject).len())
    }

    // spec: IMP-008
    #[test]
    fn finds_messages_in_a_profile() {
        let root = std::env::temp_dir().join(format!("shale-tb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let prof = root.join("abc.default-release");
        std::fs::create_dir_all(prof.join("Mail/Local Folders/Inbox.sbd")).unwrap();
        std::fs::create_dir_all(prof.join("ImapMail/imap.uni.example/INBOX.sbd")).unwrap();
        std::fs::write(prof.join("prefs.js"), "user_pref(\"mail.server.server1.hostname\", \"Local Folders\");\nuser_pref(\"mail.server.server1.type\", \"none\");\nuser_pref(\"mail.server.server1.userName\", \"nobody\");\nuser_pref(\"mail.server.server1.directory-rel\", \"[ProfD]Mail/Local Folders\");\nuser_pref(\"mail.server.server2.hostname\", \"imap.uni.example\");\nuser_pref(\"mail.server.server2.type\", \"imap\");\nuser_pref(\"mail.server.server2.userName\", \"ada\");\nuser_pref(\"mail.server.server2.directory-rel\", \"[ProfD]ImapMail/imap.uni.example\");\n").unwrap();
        // a local folder: found by position
        let first = msg("Erste", "eins");
        let second = msg("Hörsaal B – Pläne", ">From the architects\n>From here on");
        std::fs::write(prof.join("Mail/Local Folders/Inbox.sbd/Projekte"), format!("{first}{second}")).unwrap();
        let found = find_in(std::slice::from_ref(&root), &[format!("mailbox-message://nobody@Local%20Folders/Inbox/Projekte#{}", first.len())], None).unwrap();
        assert_eq!(found.len(), 1);
        let text = String::from_utf8_lossy(&found[0]);
        assert!(text.starts_with("X-Mozilla-Status") && text.contains("Subject: Hörsaal B – Pläne") && text.contains("\nFrom the architects\nFrom here on") && !text.contains("eins"), "{text}");
        // IMAP: by the subject in the file name (UTF-8 encoded subject in the folder file)
        let a = msg("=?UTF-8?Q?Bau_H=C3=B6rsaal_B=3A_Stand_4?=", "newest");
        let b = msg("Something else", "no");
        let c = msg("Bau Hörsaal B: Stand 4", "older");
        std::fs::write(prof.join("ImapMail/imap.uni.example/INBOX.sbd/Bau"), format!("{c}{b}{a}")).unwrap();
        let found = find_in(std::slice::from_ref(&root), &["imap-message://ada@imap.uni.example/INBOX/Bau#77".to_string()], Some("Bau Hörsaal B_ Stand 4.eml")).unwrap();
        assert_eq!(found.len(), 2, "both messages with that subject");
        assert!(String::from_utf8_lossy(&found[1]).contains("newest"));
        // not there: says where it looked and what to turn on
        let err = find_in(std::slice::from_ref(&root), &["imap-message://ada@imap.uni.example/INBOX/Bau#77".to_string()], Some("Missing.eml")).unwrap_err();
        assert!(err.contains("Keep messages") && err.contains("Bau") && err.contains("no index file") && err.contains("Missing.eml"), "{err}");
        // with Thunderbird's index: found by its UID, whatever the file name says
        let off = (c.len() + b.len()) as u64;
        let msf = format!("// <!-- <mdb:mork:z v=\"1.4\"/> -->\n< <(a=c)> // (f=iso-8859-1)\n  (B8=subject)(B9=message-id)(BA=storeToken)>\n<(A0=whatever)(A1={}@x)>\n{{1:^80 {{(k^BE:c)(s=9)}}\n  [4D(^B8^A0)(^B9^A1)(^BA=12)]\n  [4E(^B8=Other)(^BA=0)]}}\n@$${{2{{@\n[-4D(^B8^A0)(^B9^A1)(^BA={off})]\n@$$}}2}}@\n", fold("=?UTF-8?Q?Bau_H=C3=B6rsaal_B=3A_Stand_4?=").len());
        std::fs::write(prof.join("ImapMail/imap.uni.example/INBOX.sbd/Bau.msf"), msf).unwrap();
        let found = find_in(std::slice::from_ref(&root), &["imap-message://ada@imap.uni.example/INBOX/Bau#77".to_string()], Some("Something.eml")).unwrap();
        assert_eq!(found.len(), 1);
        assert!(String::from_utf8_lossy(&found[0]).contains("newest"), "the later row (position {off}) counts");
        // the position is out of date: found by its Message-ID
        let msf2 = std::fs::read_to_string(prof.join("ImapMail/imap.uni.example/INBOX.sbd/Bau.msf")).unwrap().replace(&format!("(^BA={off})"), "(^BA=3)");
        std::fs::write(prof.join("ImapMail/imap.uni.example/INBOX.sbd/Bau.msf"), msf2).unwrap();
        let found = find_in(std::slice::from_ref(&root), &["imap-message://ada@imap.uni.example/INBOX/Bau#77".to_string()], None).unwrap();
        assert!(String::from_utf8_lossy(&found[0]).contains("newest"));
        // a message the index doesn't know
        let err = find_in(std::slice::from_ref(&root), &["imap-message://ada@imap.uni.example/INBOX/Bau#79".to_string()], None).unwrap_err();
        assert!(err.contains("not in Thunderbird's index"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod dataflow_tests;
#[cfg(test)]
mod fixture_tests;
