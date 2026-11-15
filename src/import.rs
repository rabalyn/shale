//! Importing things into the vault as tagged notes: e-mails (with their attachments and,
//! optionally, the original message), calendar events, any other files, and dropped text.
//! Each e-mail / event becomes one note named "<date> <subject>", in a chosen folder, with
//! the tags in the note's properties so the tag list finds it.

use crate::cal_ics::{self, Event};
use crate::cal_store::Occ;
use crate::cal_time as t;
use crate::mail::{self, Mail};
use crate::media;
use crate::vault::{rel_to_string, Vault};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub enum Source {
    /// `original`: file name and bytes of the message (kept when asked to)
    Email {
        mail: Mail,
        original: (String, Vec<u8>),
    },
    Event {
        event: Box<Event>,
        calendar: String,
        original: Option<(String, Vec<u8>)>,
    },
    Files(Vec<PathBuf>),
    Text(String),
    /// a Mastodon / Bluesky post (or thread) with its pictures; `network` is "Mastodon" / "Bluesky"
    Social {
        saved: crate::social::note::SavedPost,
        network: &'static str,
        replies: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Item {
    pub source: Source,
    pub include: bool,
    /// the new note's name
    pub title: String,
    /// e-mail: which attachments to bring along
    pub att_on: Vec<bool>,
    /// imported before, as this note
    pub already: Option<PathBuf>,
    /// the people of an e-mail or event (linked to their contact notes)
    pub people: Vec<crate::people::Person>,
}

impl Item {
    pub fn kind(&self) -> std::borrow::Cow<'static, str> {
        match &self.source {
            Source::Email { .. } => "E-mail".into(),
            Source::Event { .. } => "Event".into(),
            Source::Files(_) => "Files".into(),
            Source::Text(_) => "Text".into(),
            Source::Social { saved, .. } if saved.thread.len() > 1 => format!("Thread ({} posts)", saved.thread.len()).into(),
            Source::Social { .. } => "Post".into(),
        }
    }

    /// The text to look for tags in: subject, message, attachment names; event title, place,
    /// notes; file names; the text.
    pub fn tag_text(&self) -> String {
        match &self.source {
            Source::Email { mail, .. } => format!("{}\n{}\n{}", mail.subject, mail.text, mail.attachments.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(" ")),
            Source::Event { event, .. } => format!("{}\n{}\n{}", event.summary, event.location, event.description),
            Source::Files(p) => p.iter().map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).collect::<Vec<_>>().join(" "),
            Source::Text(t) => t.clone(),
            Source::Social { saved, .. } => {
                saved.thread.iter().map(|p| format!("{}\n{}", crate::social::markup::plain_text(&p.text_markdown), p.media.iter().map(|m| m.alt.as_str()).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join("\n")
            }
        }
    }

    /// One line describing it (sender, date, attachments…).
    pub fn detail(&self) -> String {
        match &self.source {
            Source::Email { mail, .. } => {
                let n = mail.attachments.len();
                let when = mail.date.map(|d| local_stamp(d, false)).unwrap_or_else(|| mail.date_raw.clone());
                format!("from {} · {when}{}", mail::display_name(&mail.from), if n > 0 { format!(" · {n} attachment{}", if n == 1 { "" } else { "s" }) } else { String::new() })
            }
            Source::Event { event, calendar, .. } => {
                format!("{} · {calendar}", crate::cal_ui::when_text(&occ_of(event)))
            }
            Source::Files(p) => p.iter().map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect::<Vec<_>>().join(", "),
            Source::Text(s) => format!("{} characters of text", s.chars().count()),
            Source::Social { saved, network, .. } => {
                let first = &saved.thread[0];
                let n = saved.media.len();
                let when = if first.created_at == 0 { String::new() } else { format!(" · {}", local_stamp(first.created_at, false)) };
                format!("{} (@{}) on {network}{when}{}", first.author.name(), first.author.handle, if n > 0 { format!(" · {n} file{}", if n == 1 { "" } else { "s" }) } else { String::new() })
            }
        }
    }

    /// Something that went wrong while the post was fetched (pictures, the thread), if anything.
    pub fn warning(&self) -> Option<String> {
        let Source::Social { saved, .. } = &self.source else { return None };
        let mut parts = vec![];
        if let Some(e) = &saved.thread_error {
            parts.push(format!("The rest of the thread could not be fetched ({e}); only this post is saved."));
        }
        if !saved.failed_media.is_empty() {
            let n = saved.failed_media.len();
            parts.push(format!("{n} picture{} or video{} could not be downloaded; the note links to {} instead.", if n == 1 { "" } else { "s" }, if n == 1 { "" } else { "s" }, if n == 1 { "it" } else { "them" }));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }

    /// The line that marks a note as made from this item (to spot a second import).
    fn marker(&self) -> Option<String> {
        match &self.source {
            Source::Email { mail, .. } if !mail.message_id.is_empty() => Some(format!("message-id: {}", yaml(&mail.message_id))),
            Source::Event { event, .. } if !event.uid.is_empty() => Some(format!("event-uid: {}", yaml(&event_key(event)))),
            _ => None,
        }
    }
}

/// Which event (and, for a repeating one, which date) a note was made from.
fn event_key(e: &Event) -> String {
    match e.recurrence_id {
        Some(r) => format!("{}#{r}", e.uid),
        None => e.uid.clone(),
    }
}

fn occ_of(e: &Event) -> Occ {
    Occ { source: 0, event: 0, start: e.start, end: e.end, all_day: e.all_day }
}

fn today_ymd() -> (i32, u32, u32) {
    media::local_date()
}

fn ymd_text((y, m, d): (i32, u32, u32)) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

/// Local date of a UTC time.
fn local_ymd(utc: i64) -> (i32, u32, u32) {
    t::civil_from_days(t::utc_to_local(utc).0)
}

/// "2026-11-13 10:22" (local time), or just the date.
pub(crate) fn local_stamp(utc: i64, date_only: bool) -> String {
    let (day, secs) = t::utc_to_local(utc);
    let d = ymd_text(t::civil_from_days(day));
    if date_only {
        d
    } else {
        format!("{d} {}", t::hhmm(secs))
    }
}

fn event_ymd(e: &Event) -> (i32, u32, u32) {
    if e.all_day {
        t::civil_from_days(e.start.div_euclid(86_400))
    } else {
        local_ymd(e.start)
    }
}

pub(crate) fn safe_title(s: &str) -> String {
    let s: String = s.chars().map(|c| if c == '/' || c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = s.chars().take(90).collect();
    crate::markdown::link_safe_name(s.trim())
}

pub fn email_item(mail: Mail, original: (String, Vec<u8>)) -> Item {
    let date = mail.date.map(local_ymd).unwrap_or_else(today_ymd);
    let subject = if mail.subject.trim().is_empty() { "(no subject)".to_string() } else { mail.subject.clone() };
    let title = safe_title(&format!("{} {subject}", ymd_text(date)));
    // small pictures inside the text are mostly logos in signatures
    let att_on = mail.attachments.iter().map(|a| !(a.inline && a.data.len() < 30_000)).collect();
    Item { source: Source::Email { mail, original }, include: true, title, att_on, already: None, people: vec![] }
}

/// An item for a stored message, like a dropped `.eml` file: the original is kept under
/// `file_name` (default: the subject).
pub fn eml_item(raw: Vec<u8>, file_name: Option<&str>) -> Item {
    let m = mail::parse(&raw);
    let name = file_name.map(String::from).unwrap_or_else(|| format!("{}.eml", if m.subject.trim().is_empty() { "message".to_string() } else { safe_title(&m.subject) }));
    email_item(m, (name, raw))
}

/// An item for a saved post or thread (see `social::note::fetch_saved`). All its files are ticked.
/// `replies`: include the replies of other people (the dialog's tick box; off by default).
pub fn social_item(saved: crate::social::note::SavedPost, network: &'static str, replies: bool) -> Item {
    let title = crate::social::note::post_title(&saved.thread[0]);
    let att_on = vec![true; saved.media.len()];
    Item { source: Source::Social { replies: replies && !saved.replies.is_empty(), saved, network }, include: true, title, att_on, already: None, people: vec![] }
}

pub fn event_item(event: Event, calendar: String, original: Option<(String, Vec<u8>)>) -> Item {
    let title = safe_title(&format!("{} {}", ymd_text(event_ymd(&event)), if event.summary.trim().is_empty() { "Event" } else { event.summary.trim() }));
    Item { source: Source::Event { event: Box::new(event), calendar, original }, include: true, title, att_on: vec![], already: None, people: vec![] }
}

fn files_item(paths: Vec<PathBuf>) -> Item {
    let title = if paths.len() == 1 { paths[0].file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "File".into()) } else { format!("{} Files", ymd_text(today_ymd())) };
    Item { source: Source::Files(paths), include: true, title: safe_title(&title), att_on: vec![], already: None, people: vec![] }
}

fn text_item(text: String) -> Item {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("Text");
    let title = safe_title(&format!("{} {}", ymd_text(today_ymd()), first.chars().take(60).collect::<String>()));
    Item { source: Source::Text(text), include: true, title, att_on: vec![], already: None, people: vec![] }
}

fn calendar_items(ics: &[u8], calendar: &str, file_name: &str) -> Vec<Item> {
    let text = String::from_utf8_lossy(ics);
    let events = cal_ics::parse_ics(&text);
    let single = events.iter().filter(|e| e.recurrence_id.is_none()).count() <= 1;
    events
        .into_iter()
        .filter(|e| e.recurrence_id.is_none())
        .map(|e| {
            // the calendar file is kept with the note only when it holds just this event
            let original = if single { Some((file_name.to_string(), ics.to_vec())) } else { Some((format!("{}.ics", safe_title(&e.summary)), cal_ics::to_ics("", std::slice::from_ref(&e)).into_bytes())) };
            event_item(e, calendar.to_string(), original)
        })
        .collect()
}

/// Items for dropped / chosen files. Returns the items and messages about what was skipped.
pub fn items_from_paths(paths: &[PathBuf]) -> (Vec<Item>, Vec<String>) {
    let mut items = Vec::new();
    let mut notes = Vec::new();
    let mut plain = Vec::new();
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if p.is_dir() {
            notes.push(format!("“{name}” is a folder; drop the files inside it instead."));
            continue;
        }
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "eml" => match crate::fsutil::read(p) {
                Ok(raw) => items.push(email_item(mail::parse(&raw), (name, raw))),
                Err(e) => notes.push(format!("Could not read “{name}”: {e}")),
            },
            "ics" => match crate::fsutil::read(p) {
                Ok(raw) => {
                    let found = calendar_items(&raw, &name, &name);
                    if found.is_empty() {
                        notes.push(format!("“{name}” holds no events."));
                    }
                    items.extend(found);
                }
                Err(e) => notes.push(format!("Could not read “{name}”: {e}")),
            },
            _ if p.is_file() => plain.push(p.clone()),
            _ => notes.push(format!("“{name}” was not found.")),
        }
    }
    if !plain.is_empty() {
        items.push(files_item(plain));
    }
    (items, notes)
}

/// Text from a drop: UTF-8, or UTF-16 (which some programs send).
pub fn text_of(data: &[u8]) -> String {
    let utf16 = data.starts_with(&[0xFF, 0xFE]) || (data.len() >= 4 && data.len().is_multiple_of(2) && data[1] == 0 && data[3] == 0);
    if utf16 {
        let units: Vec<u16> = data.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units).trim_start_matches('\u{feff}').trim_end_matches('\0').to_string()
    } else {
        String::from_utf8_lossy(data).into_owned()
    }
}

fn file_uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.trim().strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let mut bytes = Vec::new();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = |c: u8| (c as char).to_digit(16);
            if let (Some(x), Some(y)) = (h(b[i + 1]), h(b[i + 2])) {
                bytes.push((x * 16 + y) as u8);
                i += 3;
                continue;
            }
        }
        bytes.push(b[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Links of a mail program to a message inside it (not a file we can read).
fn is_mail_link(s: &str) -> bool {
    ["mailbox:", "imap:", "imap-message:", "mailbox-message:", "news:", "ews:", "owl:"].iter().any(|p| s.trim_start().starts_with(p))
}

const MAIL_LINK_HELP: &str = "The mail program handed over only a link to the message, not the message itself. Drag the message to the desktop or a folder first (or use “Save As…”), then drop the .eml file here.";

fn norm(m: &str) -> String {
    m.to_ascii_lowercase().replace(' ', "")
}

fn preview(d: &[u8]) -> String {
    let t = text_of(d);
    let t = t.trim();
    if t.is_empty() {
        return "(empty)".into();
    }
    let one: String = t.chars().map(|c| if c.is_control() { ' ' } else { c }).take(140).collect();
    format!("“{one}”")
}

/// Items for a drop from another program: `parts` are the kinds it offered with what it sent
/// for each. Uses the best one that holds something: files, a whole message, calendar data, a
/// link to a message in Thunderbird (looked up in Thunderbird's copy on this computer), text.
pub fn items_from_parts(parts: &[(String, Vec<u8>)]) -> (Vec<Item>, Vec<String>) {
    items_from_parts_in(parts, &crate::tb_store::default_roots())
}

pub fn items_from_parts_in(parts: &[(String, Vec<u8>)], tb_roots: &[PathBuf]) -> (Vec<Item>, Vec<String>) {
    let get = |want: &str| parts.iter().find(|(m, d)| norm(m) == want && !text_of(d).trim().is_empty()).map(|(_, d)| d.as_slice());
    let mut notes: Vec<String> = Vec::new();
    // real files, a whole message, calendar data
    for k in ["text/uri-list", "message/rfc822", "text/calendar"] {
        if let Some(d) = get(k) {
            let (items, n) = items_from_drop(k, d);
            if !items.is_empty() {
                return (items, n);
            }
            notes.extend(n.into_iter().filter(|x| x != MAIL_LINK_HELP));
        }
    }
    // a link to a message inside the mail program
    let mut links: Vec<String> = Vec::new();
    for k in ["text/x-moz-url", "application/x-moz-file-promise-url", "text/uri-list", "text/plain;charset=utf-8", "utf8_string", "text/plain", "string"] {
        if let Some(d) = get(k) {
            for l in text_of(d).lines().map(str::trim) {
                if is_mail_link(l) && !links.iter().any(|x| x == l) {
                    links.push(l.to_string());
                }
            }
        }
    }
    let file_name = get("application/x-moz-file-promise-dest-filename").map(|d| text_of(d).trim().trim_end_matches('\0').to_string());
    if !links.is_empty() {
        return match crate::tb_store::find_in(tb_roots, &links, file_name.as_deref()) {
            Ok(found) => {
                let n = found.len();
                let mut items: Vec<Item> = found
                    .into_iter()
                    .map(|raw| {
                        let m = mail::parse(&raw);
                        let name = file_name.clone().filter(|f| f.to_lowercase().ends_with(".eml")).unwrap_or_else(|| format!("{}.eml", if m.subject.trim().is_empty() { "message".to_string() } else { safe_title(&m.subject) }));
                        email_item(m, (name, raw))
                    })
                    .collect();
                if n > 1 {
                    // the newest is most likely the dragged one
                    let newest = items
                        .iter()
                        .enumerate()
                        .max_by_key(|(_, it)| match &it.source {
                            Source::Email { mail, .. } => mail.date.unwrap_or(i64::MIN),
                            _ => i64::MIN,
                        })
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    for (i, it) in items.iter_mut().enumerate() {
                        it.include = i == newest;
                    }
                    notes.push(format!("{n} messages with this subject were found in Thunderbird; the newest is ticked."));
                }
                (items, notes)
            }
            Err(why) => (vec![], vec![format!("Thunderbird handed over only a link to the message, and {why}. You can also drag the message to a folder first (or use “Save As…”) and drop the .eml file.")]),
        };
    }
    // plain text
    for k in ["text/plain;charset=utf-8", "utf8_string", "text/plain", "string"] {
        if let Some(d) = get(k) {
            let (items, n) = items_from_drop("text/plain", d);
            if !items.is_empty() {
                return (items, n);
            }
        }
    }
    notes.push("Nothing usable came with the drop. What the other program sent:".into());
    for (m, d) in parts {
        notes.push(format!("  {m}: {}", preview(d)));
    }
    (vec![], notes)
}

/// Items for data dropped from another program on Wayland (`mime` is the kind we asked for).
pub fn items_from_drop(mime: &str, data: &[u8]) -> (Vec<Item>, Vec<String>) {
    let text = text_of(data);
    let uris = |text: &str| -> (Vec<Item>, Vec<String>) {
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
        let mut paths = Vec::new();
        let mut notes = Vec::new();
        for l in lines {
            match file_uri_path(l) {
                Some(p) => paths.push(p),
                None if is_mail_link(l) => notes.push(MAIL_LINK_HELP.to_string()),
                None => notes.push(format!("“{l}” is not a file on this computer.")),
            }
        }
        notes.dedup();
        let (items, more) = items_from_paths(&paths);
        notes.extend(more);
        (items, notes)
    };
    match mime {
        "text/uri-list" => uris(&text),
        // a link and its title on two lines
        "text/x-moz-url" => uris(text.lines().next().unwrap_or("")),
        "message/rfc822" => {
            let m = mail::parse(data);
            let name = format!("{}.eml", if m.subject.trim().is_empty() { "message".to_string() } else { safe_title(&m.subject) });
            (vec![email_item(m, (name, data.to_vec()))], vec![])
        }
        "text/calendar" => {
            let items = calendar_items(text.as_bytes(), "a calendar program", "event.ics");
            let notes = if items.is_empty() { vec!["The calendar data holds no events.".to_string()] } else { vec![] };
            (items, notes)
        }
        _ => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                (vec![], vec!["Nothing came with the drop.".into()])
            } else if trimmed.starts_with("BEGIN:VCALENDAR") {
                items_from_drop("text/calendar", trimmed.as_bytes())
            } else if trimmed.lines().all(|l| l.trim().starts_with("file://")) {
                uris(trimmed)
            } else if trimmed.lines().all(is_mail_link) {
                (vec![], vec![MAIL_LINK_HELP.to_string()])
            } else if looks_like_mail(trimmed) {
                items_from_drop("message/rfc822", trimmed.as_bytes())
            } else {
                (vec![text_item(trimmed.to_string())], vec![])
            }
        }
    }
}

fn looks_like_mail(s: &str) -> bool {
    let head: Vec<String> = s.lines().take_while(|l| !l.trim().is_empty()).take(60).map(|l| l.to_ascii_lowercase()).collect();
    head.iter().any(|l| l.starts_with("from:")) && head.iter().any(|l| l.starts_with("subject:") || l.starts_with("message-id:"))
}

/// Find the people of e-mails and events and their contact notes in the people folder `dir`.
pub fn find_people(vault: &Vault, dir: &str, items: &mut [Item]) {
    let contacts = crate::people::Contacts::index(vault, dir);
    for it in items.iter_mut() {
        if !it.people.is_empty() {
            continue;
        }
        it.people = match &it.source {
            Source::Email { mail, .. } => crate::people::of_mail(mail, &contacts),
            Source::Event { event, .. } => crate::people::of_event(event, &contacts),
            _ => vec![],
        };
    }
}

/// Mark items that were imported before (same message / event) and leave them out by default.
pub fn mark_duplicates(vault: &Vault, items: &mut [Item]) {
    for it in items.iter_mut() {
        // the same note every time (first path in order), whatever order the index has
        let found = if let Source::Social { saved, .. } = &it.source {
            // any post of the thread, also when it was saved as part of another thread
            vault.notes.iter().filter(|(_, n)| saved.thread.iter().any(|p| crate::social::note::note_has_post(&n.text, &p.id))).map(|(p, _)| p.clone()).min()
        } else {
            let Some(m) = it.marker() else { continue };
            vault.notes.iter().filter(|(_, n)| n.text.lines().take(40).any(|l| l.trim() == m)).map(|(p, _)| p.clone()).min()
        };
        if let Some(p) = found {
            it.already = Some(p);
            it.include = false;
        }
    }
}

/// "building-b, Beamer #status" → ["building-b", "Beamer", "status"] (as tags can be written).
pub fn parse_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in s.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let tag: String = raw.trim().trim_start_matches('#').chars().filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '/')).collect();
        let tag = tag.trim_matches('/').to_string();
        if !tag.is_empty() && !tag.chars().all(|c| c.is_ascii_digit()) && !out.iter().any(|o| o.eq_ignore_ascii_case(&tag)) {
            out.push(tag);
        }
    }
    out
}

/// A folder typed in the dialog → a safe vault-relative folder.
pub fn clean_folder(s: &str) -> Result<PathBuf, String> {
    // the one check for typed vault paths (VAULT-018)
    crate::vault_path::VaultPath::parse(s).map(|p| p.into_path_buf()).map_err(|e| e.to_string())
}

pub(crate) fn yaml(s: &str) -> String {
    let one_line: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    format!("\"{}\"", one_line.replace('\\', "\\\\").replace('"', "\\\""))
}

pub(crate) fn front(lines: &[(&str, String)], tags: &[String]) -> String {
    let mut s = String::from("---\n");
    for (k, v) in lines {
        if !v.is_empty() {
            s.push_str(&format!("{k}: {v}\n"));
        }
    }
    if !tags.is_empty() {
        s.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    s.push_str("---\n");
    s
}

pub struct Options {
    /// vault-relative folder for the notes
    pub folder: PathBuf,
    pub tags: Vec<String>,
    /// keep the original .eml / .ics next to the attachments
    pub keep_original: bool,
    /// where contact notes are (and new ones are made)
    pub people_dir: String,
}

fn link_line(rel: &Path) -> String {
    format!("- {}", media::link_for(rel))
}

/// The note for an e-mail (attachment links already made).
pub fn email_note(mail: &Mail, tags: &[String], attachments: &[PathBuf], original: Option<&Path>, people: &[String]) -> String {
    let subject = if mail.subject.trim().is_empty() { "(no subject)" } else { mail.subject.trim() };
    let date = mail.date.map(|d| local_stamp(d, false)).unwrap_or_default();
    let mut s = front(
        &[
            ("type", "email".into()),
            ("subject", yaml(subject)),
            ("from", yaml(&mail.from)),
            ("to", if mail.to.is_empty() { String::new() } else { yaml(&mail.to) }),
            ("cc", if mail.cc.is_empty() { String::new() } else { yaml(&mail.cc) }),
            ("date", date.clone()),
            ("message-id", if mail.message_id.is_empty() { String::new() } else { yaml(&mail.message_id) }),
        ],
        tags,
    );
    s.push_str(&format!("\n# {}\n\n", safe_title(subject)));
    s.push_str(&format!("- **From:** {}\n", mail.from));
    for (k, v) in [("To", &mail.to), ("Cc", &mail.cc)] {
        if !v.is_empty() {
            s.push_str(&format!("- **{k}:** {v}\n"));
        }
    }
    s.push_str(&format!("- **Date:** {}\n", if date.is_empty() { mail.date_raw.clone() } else { date }));
    if !people.is_empty() {
        s.push_str(&format!("- **People:** {}\n", people.join(", ")));
    }
    if !attachments.is_empty() || original.is_some() {
        s.push_str("\n## Attachments\n\n");
        for a in attachments {
            s.push_str(&link_line(a));
            s.push('\n');
        }
        if let Some(o) = original {
            s.push_str(&format!("- Original message: [[{}]]\n", rel_to_string(o)));
        }
    }
    s.push_str("\n## Message\n\n");
    s.push_str(mail.text.trim());
    s.push('\n');
    s
}

pub fn event_note(e: &Event, calendar: &str, tags: &[String], original: Option<&Path>, people: &[String]) -> String {
    let stamp = |v: i64| if e.all_day { ymd_text(t::civil_from_days(v.div_euclid(86_400))) } else { local_stamp(v, false) };
    let end = if e.all_day { e.end - 86_400 } else { e.end };
    let mut s = front(
        &[
            ("type", "event".into()),
            ("title", yaml(&e.summary)),
            ("start", stamp(e.start)),
            ("end", stamp(end.max(e.start))),
            ("all-day", if e.all_day { "true".into() } else { String::new() }),
            ("location", if e.location.is_empty() { String::new() } else { yaml(&e.location) }),
            ("calendar", yaml(calendar)),
            ("event-uid", if e.uid.is_empty() { String::new() } else { yaml(&event_key(e)) }),
        ],
        tags,
    );
    s.push_str(&format!("\n# {}\n\n", safe_title(if e.summary.is_empty() { "Event" } else { &e.summary })));
    s.push_str(&format!("- **When:** {}\n", crate::cal_ui::when_text(&occ_of(e))));
    if !people.is_empty() {
        s.push_str(&format!("- **People:** {}\n", people.join(", ")));
    }
    if let Some(r) = &e.rrule {
        s.push_str(&format!("- **Repeats:** {}\n", crate::cal_ui::rrule_text(r)));
    }
    for (k, v) in [("Where", &e.location), ("Calendar", &calendar.to_string()), ("Organizer", &e.organizer), ("Status", &e.status.to_lowercase())] {
        if !v.is_empty() {
            s.push_str(&format!("- **{k}:** {v}\n"));
        }
    }
    if !e.attendees.is_empty() {
        s.push_str(&format!("- **Attendees:** {}\n", e.attendees.join(", ")));
    }
    if !e.url.is_empty() {
        s.push_str(&format!("- **Link:** {}\n", e.url));
    }
    if let Some(o) = original {
        s.push_str(&format!("- Calendar file: [[{}]]\n", rel_to_string(o)));
    }
    if !e.description.trim().is_empty() {
        s.push_str("\n## Notes\n\n");
        s.push_str(e.description.trim());
        s.push('\n');
    }
    s
}

/// Make the notes (and copy attachments / files into the media folder). Returns the new notes
/// and messages about anything that failed.
pub fn import(vault: &mut Vault, items: &[Item], o: &Options) -> (Vec<PathBuf>, Vec<String>) {
    let mut made = Vec::new();
    let mut errors = Vec::new();
    let root = vault.root.clone();
    for it in items.iter().filter(|i| i.include) {
        let title = if safe_title(&it.title).is_empty() { "Imported".to_string() } else { safe_title(&it.title) };
        // contact notes (made when needed) and links to them
        let mut people: Vec<String> = Vec::new();
        for p in it.people.iter().filter(|p| p.on) {
            match crate::people::ensure_note(vault, &o.people_dir, p) {
                Ok(rel) => {
                    let link = crate::people::link_to(vault, &rel);
                    if !people.contains(&link) {
                        people.push(link);
                    }
                }
                Err(e) => errors.push(e),
            }
        }
        let result: Result<String, String> = (|| {
            Ok(match &it.source {
                Source::Email { mail, original } => {
                    let date = mail.date.map(local_ymd).unwrap_or_else(today_ymd);
                    let mut atts = Vec::new();
                    for (a, on) in mail.attachments.iter().zip(&it.att_on) {
                        if *on {
                            atts.push(media::save_bytes(&root, &a.name, &a.data, date).map_err(|e| format!("could not save “{}”: {e}", a.name))?);
                        }
                    }
                    let orig = if o.keep_original { Some(media::save_bytes(&root, &original.0, &original.1, date).map_err(|e| format!("could not save the message file: {e}"))?) } else { None };
                    email_note(mail, &o.tags, &atts, orig.as_deref(), &people)
                }
                Source::Event { event, calendar, original } => {
                    let orig = match (o.keep_original, original) {
                        (true, Some((name, data))) => Some(media::save_bytes(&root, name, data, event_ymd(event)).map_err(|e| format!("could not save the calendar file: {e}"))?),
                        _ => None,
                    };
                    event_note(event, calendar, &o.tags, orig.as_deref(), &people)
                }
                Source::Files(paths) => {
                    let date = today_ymd();
                    let mut s = front(&[("type", "files".into()), ("date", ymd_text(date))], &o.tags);
                    s.push_str(&format!("\n# {title}\n\n"));
                    for p in paths {
                        let rel = media::import_file(&root, p, date).map_err(|e| format!("could not copy “{}”: {e}", p.display()))?;
                        s.push_str(&link_line(&rel));
                        s.push('\n');
                    }
                    s
                }
                Source::Text(text) => {
                    let mut s = front(&[("type", "text".into()), ("date", ymd_text(today_ymd()))], &o.tags);
                    s.push_str(&format!("\n# {title}\n\n{}\n", text.trim()));
                    s
                }
                Source::Social { saved, replies, .. } => {
                    // only the ticked files are saved; the others stay links to their web address
                    let date = crate::social::note::media_date(saved);
                    let mut chosen = crate::social::note::SavedPost { media: vec![], failed_media: vec![], thread_error: None, replies: if *replies { saved.replies.clone() } else { vec![] }, ..saved.clone() };
                    let mut paths = vec![];
                    for (m, on) in saved.media.iter().zip(&it.att_on) {
                        if *on {
                            paths.push(media::save_bytes(&root, &m.file_name, &m.data, date).map_err(|e| format!("could not save “{}”: {e}", m.file_name))?);
                            chosen.media.push(m.clone());
                        }
                    }
                    crate::social::note::post_note(&chosen, &paths, &o.tags, &people)
                }
            })
        })();
        match result {
            Ok(text) => {
                let rel = vault.new_note_path(&o.folder, &title);
                match vault.create_file(&rel, &text) {
                    Ok(r) => made.push(r),
                    Err(e) => errors.push(format!("“{title}”: could not write the note: {e}")),
                }
            }
            Err(e) => errors.push(format!("“{title}”: {e}")),
        }
    }
    (made, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault(tag: &str) -> Vault {
        let d = std::env::temp_dir().join(format!("shale-import-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Vault::open(&d).unwrap()
    }

    // spec: IMP-006
    #[test]
    fn tags_and_folders() {
        assert_eq!(parse_tags("bau-b, #Beamer  status;Bau-B 2026 projekt/hörsaal"), vec!["bau-b", "Beamer", "status", "projekt/hörsaal"]);
        assert_eq!(clean_folder("/Projekte/ Neubau B /").unwrap(), PathBuf::from("Projekte/Neubau B"));
        assert!(clean_folder("../x").is_err());
    }

    // spec: IMP-003, IMP-004, IMP-005, TAG-001
    #[test]
    fn an_email_becomes_a_tagged_note_with_its_attachments() {
        let mut v = temp_vault("mail");
        let raw = crate::mail::tests::sample_eml();
        let dir = v.root.join("..").join(format!("shale-import-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let eml = dir.join("Neubau Hörsaal B.eml");
        std::fs::write(&eml, &raw).unwrap();
        let (mut items, notes) = items_from_paths(std::slice::from_ref(&eml));
        assert!(notes.is_empty());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "2026-11-13 Neubau Hörsaal B – Pläne Stand 3");
        assert_eq!(items[0].att_on, vec![false, true, true], "the tiny inline logo is left out");
        let o = Options { folder: PathBuf::from("Projekte/Neubau B"), tags: parse_tags("neubau-b, plaene"), keep_original: true, people_dir: "Adressbuch".into() };
        let (made, errs) = import(&mut v, &items, &o);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(made, vec![PathBuf::from("Projekte/Neubau B/2026-11-13 Neubau Hörsaal B – Pläne Stand 3.md")]);
        let text = std::fs::read_to_string(v.abs(&made[0])).unwrap();
        assert!(text.starts_with("---\ntype: email\nsubject: \"Neubau Hörsaal B – Pläne Stand 3\"\n"), "{text}");
        assert!(text.contains("message-id: \"<abc123@uni.example>\"\n"));
        assert!(text.contains("tags: [neubau-b, plaene]\n"));
        assert!(text.contains("- [[z_media/2026/11/261113_Grundriß EG.pdf]]\n"), "{text}");
        assert!(text.contains("- [[z_media/2026/11/261113_Alte Mail.eml]]\n"));
        assert!(text.contains("- Original message: [[z_media/2026/11/261113_Neubau Hörsaal B.eml]]\n"));
        assert!(!text.contains("inline-1.png"));
        assert!(text.contains("## Message\n\nHallo zusammen,"));
        assert!(v.root.join("z_media/2026/11/261113_Grundriß EG.pdf").exists());
        assert!(!text.contains("**People:**"), "nobody ticked yet (people are found by find_people)");
        // with people: the sender gets a contact note and is linked
        let mut again = items.clone();
        again[0].title = "second".into();
        find_people(&v, "Adressbuch", &mut again);
        assert_eq!(again[0].people.iter().map(|p| (p.name.as_str(), p.on)).collect::<Vec<_>>(), vec![("Hans Müller", true), ("Medientechnik", false), ("Bauamt", false)]);
        let (made2, errs) = import(&mut v, &again, &o);
        assert!(errs.is_empty(), "{errs:?}");
        let text2 = std::fs::read_to_string(v.abs(&made2[0])).unwrap();
        assert!(text2.contains("- **People:** [[Hans Müller]]\n"), "{text2}");
        assert!(v.abs(std::path::Path::new("Adressbuch/Hans Müller.md")).exists());
        // the tags count in the vault (from the note's properties)
        v.rescan();
        let tags: Vec<String> = v.all_tags().into_iter().map(|t| t.0).collect();
        assert!(tags.contains(&"neubau-b".to_string()) && tags.contains(&"plaene".to_string()), "{tags:?}");
        // a second import of the same message is spotted
        mark_duplicates(&v, &mut items);
        assert_eq!(items[0].already.as_deref(), Some(made[0].as_path()));
        assert!(!items[0].include);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: IMP-003
    #[test]
    fn events_files_and_text() {
        let mut v = temp_vault("ev");
        let day = t::days_from_civil(2026, 12, 2);
        let e = Event {
            uid: "ev-1@x".into(),
            summary: "Baubesprechung".into(),
            location: "Raum 1.12".into(),
            description: "Agenda: Medientechnik".into(),
            start: t::local_to_utc(day, 10 * 3600),
            end: t::local_to_utc(day, 11 * 3600),
            attendees: vec!["A".into(), "B".into()],
            ..Default::default()
        };
        let mut items = vec![event_item(e.clone(), "Work".into(), Some(("x.ics".into(), cal_ics::to_ics("", std::slice::from_ref(&e)).into_bytes())))];
        assert_eq!(items[0].title, "2026-12-02 Baubesprechung");
        // a file next to it
        let src = v.root.join("..").join(format!("shale-import-plan-{}.pdf", std::process::id()));
        std::fs::write(&src, b"%PDF-1.4").unwrap();
        let (more, _) = items_from_paths(std::slice::from_ref(&src));
        items.extend(more);
        let (snips, _) = items_from_drop("text/plain", "Beamer tauschen\nim Hörsaal".as_bytes());
        items.extend(snips);
        let o = Options { folder: PathBuf::from("Projekte"), tags: vec!["status".into()], keep_original: true, people_dir: "Adressbuch".into() };
        let (made, errs) = import(&mut v, &items, &o);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(made.len(), 3);
        let ev = std::fs::read_to_string(v.abs(&made[0])).unwrap();
        assert!(ev.contains("type: event\n") && ev.contains("start: 2026-12-02 10:00\n") && ev.contains("event-uid: \"ev-1@x\"\n"), "{ev}");
        assert!(ev.contains("- **Where:** Raum 1.12\n") && ev.contains("- **Attendees:** A, B\n") && ev.contains("## Notes\n\nAgenda: Medientechnik"));
        assert!(ev.contains("- Calendar file: [[z_media/2026/12/261202_x.ics]]"), "{ev}");
        let f = std::fs::read_to_string(v.abs(&made[1])).unwrap();
        assert!(f.contains("type: files") && f.contains(".pdf]]"), "{f}");
        let s = std::fs::read_to_string(v.abs(&made[2])).unwrap();
        assert!(s.contains("Beamer tauschen\nim Hörsaal"));
        assert!(made[2].to_string_lossy().ends_with("Beamer tauschen.md"));
        let _ = std::fs::remove_file(&src);
    }

    // spec: SOC-011, SOC-012
    #[test]
    fn a_saved_post_becomes_a_note_with_pictures_and_a_second_import_is_spotted() {
        use crate::social::demo;
        let mut v = temp_vault("social");
        let posts = demo::posts();
        let mut items = vec![social_item(demo::saved(&posts[4]), "Mastodon", false), social_item(demo::saved(&posts[3]), "Mastodon", false)];
        assert_eq!((items[0].kind().as_ref(), items[1].kind().as_ref()), ("Thread (3 posts)", "Post"));
        assert_eq!((items[0].att_on.clone(), items[1].att_on.clone()), (vec![true], vec![true, true]));
        assert!(items[0].detail().contains("Grace Hopper") && items[0].detail().contains("Mastodon") && items[0].detail().contains("1 file"), "{}", items[0].detail());
        assert!(items[0].tag_text().contains("plain text files"));
        assert!(items[0].warning().is_none());
        items[1].att_on[1] = false;
        // a problem while fetching shows as a warning
        let mut broken = demo::saved(&posts[3]);
        broken.thread_error = Some("offline".into());
        broken.failed_media.push(crate::social::note::FailedMedia { url: "https://x.test/a.jpg".into(), error: "gone".into(), post: 0, item: 0 });
        let w = social_item(broken, "Mastodon", false).warning().unwrap();
        assert!(w.contains("offline") && w.contains("1 picture or video could not be downloaded"), "{w}");
        let o = Options { folder: PathBuf::from("Social"), tags: parse_tags("social, reading"), keep_original: false, people_dir: "Adressbuch".into() };
        let (made, errs) = import(&mut v, &items, &o);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(made.len(), 2);
        let thread = std::fs::read_to_string(v.abs(&made[0])).unwrap();
        assert!(thread.contains("type: social-post\n") && thread.contains("post-id: \"demo-thread-1\"\n") && thread.contains("thread-ids: [\"demo-thread-1\", \"demo-thread-2\", \"demo-thread-3\"]\n"), "{thread}");
        assert!(thread.contains("tags: [social, reading]\n") && thread.contains("- **Thread:** 3 posts by @grace@fosstodon.org\n"), "{thread}");
        assert!(thread.contains("- **Source:** [https://demo.social/@grace/demo-thread-1](https://demo.social/@grace/demo-thread-1)\n"), "{thread}");
        assert!(thread.contains("**3/3**") && thread.contains("![[z_media/"), "{thread}");
        let single = std::fs::read_to_string(v.abs(&made[1])).unwrap();
        assert_eq!(single.matches("![[z_media/").count(), 1, "the unticked picture is not saved: {single}");
        assert!(single.contains("*The same hill a minute later, more purple*"), "its description stays: {single}");
        let saved: Vec<_> = std::fs::read_dir(v.root.join("z_media")).unwrap().flatten().flat_map(|y| std::fs::read_dir(y.path()).unwrap().flatten().flat_map(|m| std::fs::read_dir(m.path()).unwrap().flatten())).collect();
        assert_eq!(saved.len(), 2, "one file for the thread, one for the post");
        // replies of others: off by default, then on
        let mut with = social_item(demo::saved(&posts[4]), "Mastodon", false);
        assert!(matches!(with.source, Source::Social { replies: false, .. }), "off by default");
        assert!(!thread.contains("## Replies") && !thread.contains("replies:"));
        if let Source::Social { replies, .. } = &mut with.source {
            *replies = true;
        }
        with.title = "with replies".into();
        let (m3, errs) = import(&mut v, &[with], &o);
        assert!(errs.is_empty(), "{errs:?}");
        let t3 = std::fs::read_to_string(v.abs(&m3[0])).unwrap();
        assert!(t3.contains("replies: 3\n") && t3.contains("## Replies") && t3.contains("Plain files, yes") && t3.contains(">> **[Grace Hopper"), "{t3}");
        // the same thread again, and one post of it alone: both are marked, the other post is not
        v.rescan();
        let mut again = vec![social_item(demo::saved(&posts[4]), "Mastodon", false), social_item(demo::saved(&demo::thread()[1]), "Mastodon", false), social_item(demo::saved(&posts[0]), "Mastodon", false)];
        mark_duplicates(&v, &mut again);
        assert_eq!(again[0].already.as_deref(), Some(made[0].as_path()));
        assert_eq!(again[1].already.as_deref(), Some(made[0].as_path()), "a post that is part of a saved thread");
        assert!(!again[0].include && !again[1].include);
        assert!(again[2].already.is_none() && again[2].include);
    }

    // spec: IMP-008
    #[test]
    fn a_thunderbird_drag_with_an_empty_file_list() {
        // what Thunderbird (through the X11 bridge) sends: no file, a link, a file name
        let root = std::env::temp_dir().join(format!("shale-tbdrop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let prof = root.join("x.default");
        std::fs::create_dir_all(prof.join("Mail/Local Folders")).unwrap();
        std::fs::write(prof.join("prefs.js"), "user_pref(\"mail.server.server1.hostname\", \"Local Folders\");\nuser_pref(\"mail.server.server1.directory-rel\", \"[ProfD]Mail/Local Folders\");\n").unwrap();
        let other = "From - x\nSubject: other\n\nbody\n\n";
        let raw = String::from_utf8(crate::mail::tests::sample_eml()).unwrap();
        std::fs::write(prof.join("Mail/Local Folders/Inbox"), format!("{other}{raw}")).unwrap();
        let utf16 = |s: &str| s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>();
        let link = format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", other.len());
        let parts = vec![
            ("text/uri-list".to_string(), vec![]),
            ("text/x-moz-url".to_string(), utf16(&format!("{link}\nNeubau"))),
            ("application/x-moz-file-promise-url".to_string(), link.clone().into_bytes()),
            ("application/x-moz-file-promise-dest-filename".to_string(), b"Neubau Hoersaal B.eml".to_vec()),
            ("text/plain".to_string(), link.clone().into_bytes()),
        ];
        let (items, notes) = items_from_parts_in(&parts, std::slice::from_ref(&root));
        assert_eq!(items.len(), 1, "{notes:?}");
        assert!(items[0].title.ends_with("Stand 3"), "{}", items[0].title);
        match &items[0].source {
            Source::Email { mail, original } => {
                assert_eq!(mail.attachments.len(), 3);
                assert_eq!(original.0, "Neubau Hoersaal B.eml");
                assert!(!original.1.starts_with(b"From "), "the mbox line is taken off");
            }
            _ => panic!(),
        }
        // not found: explained, no text note made of the link
        let parts = vec![("text/plain".to_string(), b"imap-message://ada@imap.x/INBOX#5".to_vec())];
        let (items, notes) = items_from_parts_in(&parts, std::slice::from_ref(&root));
        assert!(items.is_empty() && notes[0].contains("only a link"), "{notes:?}");
        // nothing at all: shows what was sent
        let (items, notes) = items_from_parts_in(&[("text/uri-list".into(), vec![]), ("STRING".into(), vec![])], &[]);
        assert!(items.is_empty() && notes.iter().any(|n| n.contains("text/uri-list: (empty)")), "{notes:?}");
        // the normal cases still work
        let (items, _) = items_from_parts_in(&[("text/uri-list".into(), vec![]), ("text/plain;charset=utf-8".into(), b"Notiz".to_vec())], &[]);
        assert_eq!(items[0].kind(), "Text");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: IMP-009
    #[test]
    fn what_other_programs_drop() {
        let p = std::env::temp_dir().join(format!("shale drop {}.eml", std::process::id()));
        std::fs::write(&p, crate::mail::tests::sample_eml()).unwrap();
        let uri = format!("file://{}\r\n", p.to_string_lossy().replace(' ', "%20"));
        let (items, notes) = items_from_drop("text/uri-list", uri.as_bytes());
        assert_eq!((items.len(), notes.len()), (1, 0), "{notes:?}");
        assert_eq!(items[0].kind(), "E-mail");
        // UTF-16 link + title, as some programs send it
        let moz: Vec<u8> = format!("{}\nTitle", uri.trim()).encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(items_from_drop("text/x-moz-url", &moz).0.len(), 1);
        // a whole message
        let (items, _) = items_from_drop("message/rfc822", &crate::mail::tests::sample_eml());
        assert_eq!(items[0].kind(), "E-mail");
        assert!(items[0].title.ends_with("Stand 3"));
        // calendar data (as dragged out of a calendar program)
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x1\r\nDTSTART:20261202T090000Z\r\nDTEND:20261202T100000Z\r\nSUMMARY:Abnahme Hörsaal\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (items, _) = items_from_drop("text/calendar", ics.as_bytes());
        assert_eq!((items.len(), &*items[0].kind()), (1, "Event"));
        assert_eq!(items_from_drop("text/plain", ics.as_bytes()).0[0].kind(), "Event");
        // only a link into the mail program: explained
        let (items, notes) = items_from_drop("text/plain", b"imap-message://ada@mail.uni.example/INBOX#4711");
        assert!(items.is_empty() && notes[0].contains("Save As"), "{notes:?}");
        let (items, notes) = items_from_drop("text/uri-list", b"mailbox:///home/ada/Mail/Inbox?number=12");
        assert!(items.is_empty() && notes[0].contains(".eml"));
        let _ = std::fs::remove_file(&p);
    }
}
