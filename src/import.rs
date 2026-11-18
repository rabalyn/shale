//! Importing things into the vault as tagged notes: e-mails (with their attachments and,
//! optionally, the original message), calendar events, any other files, and dropped text.
//! Each e-mail / event becomes one note named "<date> <subject>", in a chosen folder, with
//! the tags in the note's properties so the tag list finds it.

use crate::calendar::ics::{self, Event};
use crate::calendar::store::Occ;
use crate::calendar::time as t;
use crate::mail::{self, Mail};
use crate::media;
use crate::vault::{rel_to_string, Vault};
use std::path::{Path, PathBuf};

pub mod dialog;
pub mod docx;
mod docx_look;
#[cfg(test)]
mod docx_look_tests;
#[cfg(test)]
pub(crate) mod docx_tests;
/// What another program hands over in a drop (a whole message, calendar data, links, text,
/// Thunderbird lookups): only Wayland delivers those kinds (IMP-011); `wl_drop.rs` is their one source.
#[cfg(target_os = "linux")]
mod from_programs;
pub mod people;
pub mod tagwords;
#[cfg(all(test, target_os = "linux"))]
mod tb_fixture_tests;
// Thunderbird's own mail folders, looked up for dropped messages (Wayland, IMP-008)
#[cfg(target_os = "linux")]
pub mod tb_store;
#[cfg(target_os = "linux")]
pub mod wl_drop;
#[cfg(target_os = "linux")]
pub use from_programs::{file_uri, items_from_parts};
#[cfg(all(target_os = "linux", test))]
pub use from_programs::{items_from_drop, text_of};

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
    /// an image, PDF or audio file, saved as a file in the media folder, no note (IMP-031);
    /// `name`: its file name as it came
    Media {
        kind: media::Kind,
        name: String,
        data: MediaData,
    },
    /// a Markdown file, imported as a note with its text as it is (IMP-033); `lift`: its headings
    /// move up so the highest is `#` (IMP-034), only when it has none and the box is ticked
    Markdown {
        name: String,
        text: String,
        lift: bool,
    },
    /// a Word file, imported as a note by pandoc (IMP-035). Without `pandoc` it is saved as an
    /// attachment (IMP-035). `lift`: the headings move up when the note has no `#` (IMP-034).
    /// `ready`: the result of the run when the dialog has made it ahead of time in a job (else
    /// `import` runs pandoc itself)
    Docx {
        name: String,
        path: PathBuf,
        pandoc: Option<PathBuf>,
        lift: bool,
        ready: Option<Result<std::sync::Arc<docx::Prepared>, String>>,
    },
    /// text dropped from another program (Wayland, IMP-011)
    #[cfg(target_os = "linux")]
    Text(String),
    /// a Mastodon / Bluesky post (or thread) with its pictures; `network` is "Mastodon" / "Bluesky"
    Social {
        saved: crate::social::note::SavedPost,
        network: &'static str,
        replies: bool,
    },
}

/// Where the bytes of a media item come from.
#[derive(Clone, Debug)]
pub enum MediaData {
    /// a file on this computer
    Path(PathBuf),
    /// bytes a program handed over or that were downloaded
    #[cfg(any(target_os = "linux", test))]
    Bytes(Vec<u8>),
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
    pub people: Vec<crate::import::people::Person>,
}

impl Item {
    pub fn kind(&self) -> std::borrow::Cow<'static, str> {
        match &self.source {
            Source::Email { .. } => "E-mail".into(),
            Source::Event { .. } => "Event".into(),
            Source::Files(_) => "Files".into(),
            Source::Markdown { .. } => "Markdown note".into(),
            Source::Docx { pandoc: Some(_), .. } => "Word document".into(),
            Source::Docx { pandoc: None, .. } => "Files".into(),
            Source::Media { kind, .. } => kind.label().into(),
            #[cfg(target_os = "linux")]
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
            Source::Media { name, .. } => name.clone(),
            Source::Markdown { text, .. } => text.clone(),
            Source::Docx { path, .. } => path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            #[cfg(target_os = "linux")]
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
                format!("{} · {calendar}", crate::calendar::ui::when_text(&occ_of(event)))
            }
            Source::Files(p) => p.iter().map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect::<Vec<_>>().join(", "),
            Source::Media { name, data, .. } => match data {
                MediaData::Path(p) => format!("{name} · from {}", p.parent().map(|d| d.display().to_string()).unwrap_or_default()),
                #[cfg(any(target_os = "linux", test))]
                MediaData::Bytes(b) => format!("{name} · {}", dialog::size_text(b.len())),
            },
            Source::Markdown { name, text, .. } => format!("{name} · {} characters", text.chars().count()),
            Source::Docx { name, path, .. } => format!("{name} · from {}", path.parent().map(|d| d.display().to_string()).unwrap_or_default()),
            #[cfg(target_os = "linux")]
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
        if let Source::Docx { pandoc: None, .. } = &self.source {
            return Some(format!("{}. The file is saved as an attachment.", docx::MISSING));
        }
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

/// What the Import does with Word files (IMP-035, IMP-036).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Word {
    /// the setting is off: a `.docx` is an attachment like any other file (IMP-031)
    #[default]
    Off,
    /// the setting is on; the pandoc that was found, if any
    On(Option<PathBuf>),
}

/// Make Word items of the `.docx` files that `items_from_paths` left in its "Files" items, when the
/// Word import is on (IMP-035); the other files stay where they are.
pub fn word_items(items: &mut Vec<Item>, word: &Word) {
    let Word::On(pandoc) = word else { return };
    let is_docx = |p: &PathBuf| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("docx"));
    let mut out: Vec<Item> = Vec::new();
    for it in items.drain(..) {
        match it.source {
            Source::Files(paths) if paths.iter().any(is_docx) => {
                let (words, rest): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(is_docx);
                out.extend(words.into_iter().map(|p| docx::docx_item(p, pandoc.clone())));
                if !rest.is_empty() {
                    out.push(files_item(rest));
                }
            }
            source => out.push(Item { source, ..it }),
        }
    }
    *items = out;
}

/// An item for a Markdown file (IMP-033): a note named like the file, its text unchanged unless
/// the headings are lifted (IMP-034), which is ticked when the text has headings but no `#`.
pub fn markdown_item(name: String, text: String) -> Item {
    let title = safe_title(&Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.clone()));
    let lift = crate::markdown::lift_headings(&text).is_some();
    Item { source: Source::Markdown { name, text, lift }, include: true, title, att_on: vec![], already: None, people: vec![] }
}

/// An item for a picture, PDF or audio file; its title is the file name the file is saved under.
pub fn media_item(kind: media::Kind, name: String, data: MediaData) -> Item {
    Item { source: Source::Media { kind, name: name.clone(), data }, include: true, title: name, att_on: vec![], already: None, people: vec![] }
}

fn calendar_items(ics: &[u8], calendar: &str, file_name: &str) -> Vec<Item> {
    let text = String::from_utf8_lossy(ics);
    let events = ics::parse_ics(&text);
    let single = events.iter().filter(|e| e.recurrence_id.is_none()).count() <= 1;
    events
        .into_iter()
        .filter(|e| e.recurrence_id.is_none())
        .map(|e| {
            // the calendar file is kept with the note only when it holds just this event
            let original = if single { Some((file_name.to_string(), ics.to_vec())) } else { Some((format!("{}.ics", safe_title(&e.summary)), ics::to_ics("", std::slice::from_ref(&e)).into_bytes())) };
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
            "md" | "markdown" => match crate::fsutil::read_to_string(p) {
                Ok(text) => items.push(markdown_item(name, text)),
                Err(e) => notes.push(format!("Could not read “{name}”: {e}")),
            },
            _ if p.is_file() => match media::media_kind(p) {
                Some(kind) => items.push(media_item(kind, name, MediaData::Path(p.clone()))),
                None => plain.push(p.clone()),
            },
            _ => notes.push(format!("“{name}” was not found.")),
        }
    }
    if !plain.is_empty() {
        items.push(files_item(plain));
    }
    (items, notes)
}

/// Find the people of e-mails and events and their contact notes in the people folder `dir`.
pub fn find_people(vault: &Vault, dir: &str, items: &mut [Item]) {
    let contacts = crate::import::people::Contacts::index(vault, dir);
    for it in items.iter_mut() {
        if !it.people.is_empty() {
            continue;
        }
        it.people = match &it.source {
            Source::Email { mail, .. } => crate::import::people::of_mail(mail, &contacts),
            Source::Event { event, .. } => crate::import::people::of_event(event, &contacts),
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

/// A property that is a list, written as a block (`key:` and one `  - "item"` line each), so that
/// an item may hold commas. Empty list → an empty value (the property is left out by `front`).
pub(crate) fn list(items: &[String]) -> String {
    items.iter().map(|i| format!("\n  - {}", yaml(i))).collect()
}

pub(crate) fn front(lines: &[(&str, String)], tags: &[String]) -> String {
    let mut s = String::from("---\n");
    for (k, v) in lines {
        if v.starts_with('\n') {
            // a block list: the value brings its own lines (`list`)
            s.push_str(&format!("{k}:{v}\n"));
        } else if !v.is_empty() {
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
    /// keep the original .docx in the import folder (IMP-038)
    pub keep_docx: bool,
    /// where contact notes are (and new ones are made)
    pub people_dir: String,
    /// also write the properties of an e-mail / event as a list in the note's text (IMP-052)
    pub header_list: bool,
}

fn link_line(rel: &Path) -> String {
    format!("- {}", media::link_for(rel))
}

/// The note for an e-mail (attachment links already made). The properties are the one record of
/// the header (IMP-050, IMP-051); `header_list` also writes them as a list below the title (IMP-052).
pub fn email_note(mail: &Mail, tags: &[String], attachments: &[PathBuf], original: Option<&Path>, people: &[String], header_list: bool) -> String {
    let subject = if mail.subject.trim().is_empty() { "(no subject)" } else { mail.subject.trim() };
    let date = mail.date.map(|d| local_stamp(d, false)).unwrap_or_default();
    let mut s = front(
        &[
            ("type", "email".into()),
            ("subject", yaml(subject)),
            ("from", yaml(&mail.from)),
            ("to", if mail.to.is_empty() { String::new() } else { yaml(&mail.to) }),
            ("cc", if mail.cc.is_empty() { String::new() } else { yaml(&mail.cc) }),
            ("date", if date.is_empty() && !mail.date_raw.is_empty() { yaml(&mail.date_raw) } else { date.clone() }),
            ("message-id", if mail.message_id.is_empty() { String::new() } else { yaml(&mail.message_id) }),
            ("people", list(people)),
            ("original", original.map(|o| yaml(&format!("[[{}]]", rel_to_string(o)))).unwrap_or_default()),
        ],
        tags,
    );
    s.push_str(&format!("\n# {}\n\n", safe_title(subject)));
    if header_list {
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
    }
    let original_line = original.filter(|_| header_list);
    if !attachments.is_empty() || original_line.is_some() {
        if header_list {
            s.push('\n');
        }
        s.push_str("## Attachments\n\n");
        for a in attachments {
            s.push_str(&link_line(a));
            s.push('\n');
        }
        if let Some(o) = original_line {
            s.push_str(&format!("- Original message: [[{}]]\n", rel_to_string(o)));
        }
        s.push('\n');
    }
    s.push_str("## Message\n\n");
    s.push_str(mail.text.trim());
    s.push('\n');
    s
}

pub fn event_note(e: &Event, calendar: &str, tags: &[String], original: Option<&Path>, people: &[String], header_list: bool) -> String {
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
            ("repeats", e.rrule.as_ref().map(|r| yaml(&crate::calendar::ui::rrule_text(r))).unwrap_or_default()),
            ("organizer", if e.organizer.is_empty() { String::new() } else { yaml(&e.organizer) }),
            ("attendees", list(&e.attendees)),
            ("status", if e.status.is_empty() { String::new() } else { yaml(&e.status.to_lowercase()) }),
            ("url", if e.url.is_empty() { String::new() } else { yaml(&e.url) }),
            ("people", list(people)),
            ("original", original.map(|o| yaml(&format!("[[{}]]", rel_to_string(o)))).unwrap_or_default()),
        ],
        tags,
    );
    s.push_str(&format!("\n# {}\n\n", safe_title(if e.summary.is_empty() { "Event" } else { &e.summary })));
    if header_list {
        s.push_str(&format!("- **When:** {}\n", crate::calendar::ui::when_text(&occ_of(e))));
        if !people.is_empty() {
            s.push_str(&format!("- **People:** {}\n", people.join(", ")));
        }
        if let Some(r) = &e.rrule {
            s.push_str(&format!("- **Repeats:** {}\n", crate::calendar::ui::rrule_text(r)));
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
    }
    if !e.description.trim().is_empty() {
        if header_list {
            s.push_str("\n## Notes\n\n");
        }
        s.push_str(e.description.trim());
        s.push('\n');
    }
    s
}

/// What an import made.
#[derive(Debug, Default)]
pub struct Imported {
    /// the new notes
    pub notes: Vec<PathBuf>,
    /// the files that were saved as they are in the media folder (IMP-031)
    pub files: Vec<PathBuf>,
    /// messages about anything that failed
    pub errors: Vec<String>,
}

/// The file name a media item is saved under: the title, with the extension of the original name.
fn media_file_name(title: &str, name: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return name.to_string();
    }
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let has = Path::new(title).extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(&ext));
    if ext.is_empty() || has {
        title.to_string()
    } else {
        format!("{title}.{ext}")
    }
}

/// The note for files that are saved as attachments, with a link to each.
fn files_note(root: &Path, paths: &[PathBuf], title: &str, tags: &[String]) -> Result<String, String> {
    let date = today_ymd();
    let mut s = front(&[("type", "files".into()), ("date", ymd_text(date))], tags);
    s.push_str(&format!("\n# {title}\n\n"));
    for p in paths {
        let rel = media::import_file(root, p, date).map_err(|e| format!("could not copy “{}”: {e}", p.display()))?;
        s.push_str(&link_line(&rel));
        s.push('\n');
    }
    Ok(s)
}

/// Make the notes (and copy attachments / files into the media folder); pictures, PDFs and audio
/// files become files there and no note.
pub fn import(vault: &mut Vault, items: &[Item], o: &Options) -> Imported {
    let mut made = Vec::new();
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let root = vault.root.clone();
    for it in items.iter().filter(|i| i.include) {
        if let Source::Media { name, data, .. } = &it.source {
            let file = media_file_name(&it.title, name);
            let date = today_ymd();
            let saved = match data {
                MediaData::Path(p) if file == *name => media::import_file(&root, p, date),
                MediaData::Path(p) => crate::fsutil::read(p).and_then(|b| media::save_bytes(&root, &file, &b, date)),
                #[cfg(any(target_os = "linux", test))]
                MediaData::Bytes(b) => media::save_bytes(&root, &file, b, date),
            };
            match saved {
                Ok(rel) => files.push(rel),
                Err(e) => errors.push(format!("could not save “{file}”: {e}")),
            }
            continue;
        }
        if let Source::Docx { pandoc: Some(_), .. } = &it.source {
            match docx::import_item(vault, it, o) {
                Ok(rel) => made.push(rel),
                Err(e) => errors.push(e),
            }
            continue;
        }
        let title = if safe_title(&it.title).is_empty() { "Imported".to_string() } else { safe_title(&it.title) };
        // contact notes (made when needed) and links to them
        let mut people: Vec<String> = Vec::new();
        for p in it.people.iter().filter(|p| p.on) {
            match crate::import::people::ensure_note(vault, &o.people_dir, p) {
                Ok(rel) => {
                    let link = crate::import::people::link_to(vault, &rel);
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
                    email_note(mail, &o.tags, &atts, orig.as_deref(), &people, o.header_list)
                }
                Source::Event { event, calendar, original } => {
                    let orig = match (o.keep_original, original) {
                        (true, Some((name, data))) => Some(media::save_bytes(&root, name, data, event_ymd(event)).map_err(|e| format!("could not save the calendar file: {e}"))?),
                        _ => None,
                    };
                    event_note(event, calendar, &o.tags, orig.as_deref(), &people, o.header_list)
                }
                Source::Files(paths) => files_note(&root, paths, &title, &o.tags)?,
                Source::Docx { path, .. } => files_note(&root, std::slice::from_ref(path), &title, &o.tags)?,
                Source::Markdown { text, lift, .. } => match crate::markdown::lift_headings(text).filter(|_| *lift) {
                    Some(lifted) => lifted,
                    None => text.clone(),
                },
                #[cfg(target_os = "linux")]
                Source::Text(text) => {
                    let mut s = front(&[("type", "text".into()), ("date", ymd_text(today_ymd()))], &o.tags);
                    s.push_str(&format!("\n# {title}\n\n{}\n", text.trim()));
                    s
                }
                Source::Media { .. } => unreachable!("media items are saved above"),
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
                let rel = vault.new_note_path_max(&o.folder, &title, crate::fsutil::MADE_NAME_MAX);
                match vault.create_file(&rel, &text) {
                    Ok(r) => made.push(r),
                    Err(e) => errors.push(format!("“{title}”: could not write the note: {e}")),
                }
            }
            Err(e) => errors.push(format!("“{title}”: {e}")),
        }
    }
    Imported { notes: made, files, errors }
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
        let o = Options { folder: PathBuf::from("Projekte/Neubau B"), tags: parse_tags("neubau-b, plaene"), keep_original: true, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false };
        let Imported { notes: made, errors: errs, .. } = import(&mut v, &items, &o);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(made, vec![PathBuf::from("Projekte/Neubau B/2026-11-13 Neubau Hörsaal B – Pläne Stand 3.md")]);
        let text = std::fs::read_to_string(v.abs(&made[0])).unwrap();
        assert!(text.starts_with("---\ntype: email\nsubject: \"Neubau Hörsaal B – Pläne Stand 3\"\n"), "{text}");
        assert!(text.contains("message-id: \"<abc123@uni.example>\"\n"));
        assert!(text.contains("tags: [neubau-b, plaene]\n"));
        assert!(text.contains("- [[z_media/2026/11/261113_Grundriß EG.pdf]]\n"), "{text}");
        assert!(text.contains("- [[z_media/2026/11/261113_Alte Mail.eml]]\n"));
        // the header is written once, as properties (IMP-050, IMP-051)
        assert!(text.contains("original: \"[[z_media/2026/11/261113_Neubau Hörsaal B.eml]]\"\n"), "{text}");
        assert!(!text.contains("**From:**") && !text.contains("**Date:**") && !text.contains("Original message:"), "{text}");
        assert!(text.contains("\n---\n\n# Neubau Hörsaal B – Pläne Stand 3\n\n## Attachments\n\n- [[z_media"), "properties, title, attachments: {text}");
        assert!(!text.contains("inline-1.png"));
        assert!(text.contains("## Message\n\nHallo zusammen,"));
        assert!(v.root.join("z_media/2026/11/261113_Grundriß EG.pdf").exists());
        assert!(!text.contains("**People:**"), "nobody ticked yet (people are found by find_people)");
        // with people: the sender gets a contact note and is linked
        let mut again = items.clone();
        again[0].title = "second".into();
        find_people(&v, "Adressbuch", &mut again);
        assert_eq!(again[0].people.iter().map(|p| (p.name.as_str(), p.on)).collect::<Vec<_>>(), vec![("Hans Müller", true), ("Medientechnik", false), ("Bauamt", false)]);
        let Imported { notes: made2, errors: errs, .. } = import(&mut v, &again, &o);
        assert!(errs.is_empty(), "{errs:?}");
        let text2 = std::fs::read_to_string(v.abs(&made2[0])).unwrap();
        assert!(text2.contains("people:\n  - \"[[Hans Müller]]\"\n"), "{text2}");
        assert!(!text2.contains("**People:**"), "{text2}");
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
    fn events_and_files() {
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
        let mut items = vec![event_item(e.clone(), "Work".into(), Some(("x.ics".into(), ics::to_ics("", std::slice::from_ref(&e)).into_bytes())))];
        assert_eq!(items[0].title, "2026-12-02 Baubesprechung");
        // a file next to it
        let src = v.root.join("..").join(format!("shale-import-plan-{}.pdf", std::process::id()));
        std::fs::write(&src, b"%PDF-1.4").unwrap();
        let (pdf, _) = items_from_paths(std::slice::from_ref(&src));
        assert_eq!(pdf[0].kind(), "PDF", "a PDF is a file of its own (IMP-031), not part of a Files note");
        let txt = v.root.join("..").join(format!("shale-import-plan-{}.txt", std::process::id()));
        std::fs::write(&txt, b"plan").unwrap();
        let (more, _) = items_from_paths(std::slice::from_ref(&txt));
        items.extend(more);
        let o = Options { folder: PathBuf::from("Projekte"), tags: vec!["status".into()], keep_original: true, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false };
        let Imported { notes: made, errors: errs, .. } = import(&mut v, &items, &o);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(made.len(), 2);
        let ev = std::fs::read_to_string(v.abs(&made[0])).unwrap();
        assert!(ev.contains("type: event\n") && ev.contains("start: 2026-12-02 10:00\n") && ev.contains("event-uid: \"ev-1@x\"\n"), "{ev}");
        assert!(ev.contains("location: \"Raum 1.12\"\n") && ev.contains("attendees:\n  - \"A\"\n  - \"B\"\n"), "{ev}");
        assert!(ev.contains("original: \"[[z_media/2026/12/261202_x.ics]]\"\n"), "{ev}");
        assert!(ev.ends_with("---\n\n# Baubesprechung\n\nAgenda: Medientechnik\n"), "properties, title, notes: {ev}");
        assert!(!ev.contains("**Where:**") && !ev.contains("## Notes") && !ev.contains("Calendar file:"), "{ev}");
        let f = std::fs::read_to_string(v.abs(&made[1])).unwrap();
        assert!(f.contains("type: files") && f.contains(".txt]]"), "{f}");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&txt);
    }

    fn full_event() -> Event {
        let day = t::days_from_civil(2026, 12, 2);
        Event {
            uid: "ev-2@x".into(),
            summary: "Jour fixe".into(),
            location: "Raum 1.12".into(),
            description: "Agenda\n\nPunkt 1".into(),
            url: "https://meet.example.org/j".into(),
            organizer: "Anna Beispiel".into(),
            attendees: vec!["Müller, Hans".into(), "B".into()],
            status: "CONFIRMED".into(),
            rrule: Some("FREQ=WEEKLY".into()),
            start: t::local_to_utc(day, 10 * 3600),
            end: t::local_to_utc(day, 11 * 3600),
            ..Default::default()
        }
    }

    // spec: IMP-050, IMP-051, IMP-052
    #[test]
    fn the_header_of_an_event_is_written_once_as_properties_and_as_a_list_only_on_request() {
        let e = full_event();
        let people = vec!["[[Anna Beispiel]]".to_string()];
        let orig = PathBuf::from("z_media/2026/12/261202_x.ics");
        let note = event_note(&e, "Work", &["t".into()], Some(&orig), &people, false);
        for line in [
            "repeats: \"weekly\"\n",
            "organizer: \"Anna Beispiel\"\n",
            "attendees:\n  - \"Müller, Hans\"\n  - \"B\"\n",
            "status: \"confirmed\"\n",
            "url: \"https://meet.example.org/j\"\n",
            "people:\n  - \"[[Anna Beispiel]]\"\n",
            "original: \"[[z_media/2026/12/261202_x.ics]]\"\n",
            "location: \"Raum 1.12\"\n",
            "tags: [t]\n---\n",
        ] {
            assert!(note.contains(line), "{line:?} missing in {note}");
        }
        assert!(note.ends_with("\n# Jour fixe\n\nAgenda\n\nPunkt 1\n"), "title, then the notes, tidied: {note}");
        assert!(!note.contains("**"), "no header list: {note}");
        // the list is read back by the properties parser, an item may hold a comma
        let values = crate::markdown::front_matter_values(&note);
        let attendees = values.iter().find(|(k, _)| k == "attendees").unwrap();
        assert_eq!(attendees.1, vec!["Müller, Hans", "B"]);
        // on request the list comes back as it was
        let listed = event_note(&e, "Work", &["t".into()], Some(&orig), &people, true);
        for line in [
            "- **When:** ",
            "- **People:** [[Anna Beispiel]]\n",
            "- **Repeats:** weekly\n",
            "- **Where:** Raum 1.12\n",
            "- **Organizer:** Anna Beispiel\n",
            "- **Status:** confirmed\n",
            "- **Attendees:** Müller, Hans, B\n",
            "- **Link:** https://meet.example.org/j\n",
            "- Calendar file: [[z_media/2026/12/261202_x.ics]]\n",
            "\n## Notes\n\nAgenda",
        ] {
            assert!(listed.contains(line), "{line:?} missing in {listed}");
        }
        assert!(listed.starts_with(&note[..note.find("\n# ").unwrap()]), "the properties are the same");
    }

    // spec: IMP-050, IMP-052
    #[test]
    fn an_email_goes_from_its_properties_to_the_title_the_attachments_and_the_message() {
        let mail = Mail { subject: "Plan".into(), from: "A <a@x>".into(), to: "B <b@x>".into(), date_raw: "Thu, 13 Nov 2026 10:22:33 +0100".into(), text: "Hallo\n\nWelt\n-- \nAnna\n".into(), ..Default::default() };
        let atts = [PathBuf::from("z_media/2026/11/261113_foto.jpg"), PathBuf::from("z_media/2026/11/261113_plan.pdf")];
        let orig = PathBuf::from("z_media/2026/11/261113_Plan.eml");
        let note = email_note(&mail, &[], &atts, Some(&orig), &[], false);
        assert!(note.contains("date: \"Thu, 13 Nov 2026 10:22:33 +0100\"\n"), "an unreadable date is kept as written: {note}");
        assert!(note.ends_with("\n---\n\n# Plan\n\n## Attachments\n\n- ![[z_media/2026/11/261113_foto.jpg]]\n- [[z_media/2026/11/261113_plan.pdf]]\n\n## Message\n\nHallo\n\nWelt\n-- \nAnna\n"), "{note}");
        assert!(!note.contains("**"), "{note}");
        let listed = email_note(&mail, &[], &atts, Some(&orig), &["[[A]]".into()], true);
        assert!(listed.contains("\n# Plan\n\n- **From:** A <a@x>\n- **To:** B <b@x>\n- **Date:** Thu, 13 Nov 2026 10:22:33 +0100\n- **People:** [[A]]\n\n## Attachments\n\n"), "{listed}");
        assert!(listed.contains("- Original message: [[z_media/2026/11/261113_Plan.eml]]\n\n## Message"), "{listed}");
        // without attachments there is no section
        let bare = email_note(&mail, &[], &[], None, &[], false);
        assert!(bare.contains("# Plan\n\n## Message\n\nHallo"), "{bare}");
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
        let o = Options { folder: PathBuf::from("Social"), tags: parse_tags("social, reading"), keep_original: false, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false };
        let Imported { notes: made, errors: errs, .. } = import(&mut v, &items, &o);
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
        let Imported { notes: m3, errors: errs, .. } = import(&mut v, &[with], &o);
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

    // spec: IMP-033, IMP-034
    #[test]
    fn markdown_files_become_notes_and_their_headings_are_lifted_when_the_box_is_ticked() {
        let mut v = temp_vault("markdown");
        let src = v.root.join("..").join(format!("shale-import-mdsrc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(&src).unwrap();
        let deep = "---\ntitle: x\n---\n### Intro\ntext\n```\n### code\n```\n#### Part\n";
        for (n, t) in [("Deep.md", deep), ("Top notes.markdown", "# Top\n## Sub\n"), ("Plain.MD", "no headings\n"), ("other.txt", "t")] {
            std::fs::write(src.join(n), t).unwrap();
        }
        let paths: Vec<PathBuf> = ["Deep.md", "Top notes.markdown", "Plain.MD", "other.txt"].iter().map(|n| src.join(n)).collect();
        let (mut items, notes) = items_from_paths(&paths);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(items.iter().map(|i| i.kind().to_string()).collect::<Vec<_>>(), vec!["Markdown note", "Markdown note", "Markdown note", "Files"], "other files stay attachments");
        assert_eq!(items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["Deep", "Top notes", "Plain", "other"]);
        assert!(items[0].detail().starts_with("Deep.md"), "{}", items[0].detail());
        assert!(items[0].tag_text().contains("Intro"));
        // the lift box is ticked for the one with headings but no #, and for it only
        let lifts: Vec<bool> = items.iter().take(3).map(|i| matches!(i.source, Source::Markdown { lift: true, .. })).collect();
        assert_eq!(lifts, vec![true, false, false]);
        let o = Options { folder: PathBuf::from("In"), tags: vec!["ignored".into()], keep_original: false, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false };
        let Imported { notes: made, errors, .. } = import(&mut v, &items[..3], &o);
        assert!(errors.is_empty(), "{errors:?}");
        let read = |i: usize| std::fs::read_to_string(v.abs(&made[i])).unwrap();
        assert_eq!(made.iter().map(|m| rel_to_string(m)).collect::<Vec<_>>(), vec!["In/Deep.md", "In/Top notes.md", "In/Plain.md"], "named like the file, no date prefix");
        assert_eq!(read(0), "---\ntitle: x\n---\n# Intro\ntext\n```\n### code\n```\n## Part\n", "ticked: lifted, code and front matter untouched");
        assert_eq!((read(1).as_str(), read(2).as_str()), ("# Top\n## Sub\n", "no headings\n"), "the text as it is");
        // unticked: the text unchanged
        if let Source::Markdown { lift, .. } = &mut items[0].source {
            *lift = false;
        }
        items[0].title = "Deep again".into();
        let Imported { notes: made, .. } = import(&mut v, &items[..1], &o);
        assert_eq!(std::fs::read_to_string(v.abs(&made[0])).unwrap(), deep);
        let _ = std::fs::remove_dir_all(&src);
    }

    // spec: IMP-031
    #[test]
    fn pictures_pdfs_and_audio_are_items_of_their_own_and_saved_as_files_without_a_note() {
        let mut v = temp_vault("media");
        let src = v.root.join("..").join(format!("shale-import-mediasrc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(&src).unwrap();
        for (n, d) in [("Cat.PNG", &b"png"[..]), ("anim.gif", b"gif"), ("plan.pdf", b"%PDF"), ("song.flac", b"fl"), ("clip.MKV", b"mk"), ("notes.txt", b"t1"), ("more.dat", b"t2")] {
            std::fs::write(src.join(n), d).unwrap();
        }
        let paths: Vec<PathBuf> = ["Cat.PNG", "anim.gif", "plan.pdf", "song.flac", "clip.MKV", "notes.txt", "more.dat"].iter().map(|n| src.join(n)).collect();
        let (mut items, notes) = items_from_paths(&paths);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(items.iter().map(|i| i.kind().to_string()).collect::<Vec<_>>(), vec!["Image", "Image", "PDF", "Audio", "Video", "Files"], "the other files stay one note");
        assert_eq!(items[0].title, "Cat.PNG", "the file name is the title");
        assert_eq!(media::media_kind(Path::new("x.MP3")), Some(media::Kind::Audio));
        assert_eq!(media::media_kind(Path::new("x.txt")), None);
        // a renamed title keeps the extension; a file of the vault is linked where it is
        items[0].title = "A cat".into();
        items[1].title = "anim.gif".into();
        let o = Options { folder: PathBuf::from("Projekte"), tags: vec!["x".into()], keep_original: false, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false };
        items.truncate(5);
        let Imported { notes, files, errors } = import(&mut v, &items, &o);
        assert!(errors.is_empty() && notes.is_empty(), "no note for media: {errors:?} {notes:?}");
        let names: Vec<String> = files.iter().map(|f| rel_to_string(f)).collect();
        let (y, m, d) = media::local_date();
        let stamp = format!("z_media/{y:04}/{m:02}/{:02}{m:02}{d:02}_", y.rem_euclid(100));
        assert_eq!(names, vec![format!("{stamp}A cat.PNG"), format!("{stamp}anim.gif"), format!("{stamp}plan.pdf"), format!("{stamp}song.flac"), format!("{stamp}clip.MKV")]);
        assert_eq!(std::fs::read(v.abs(&files[0])).unwrap(), b"png");
        assert!(!v.root.join("Projekte").exists(), "the notes folder is not made for files");
        // bytes (a download) are saved under their name; unticked items are left out
        let mut bytes = media_item(media::Kind::Image, "web.jpg".into(), MediaData::Bytes(b"jpg".to_vec()));
        let mut off = media_item(media::Kind::Audio, "off.mp3".into(), MediaData::Bytes(b"x".to_vec()));
        off.include = false;
        let Imported { files, .. } = import(&mut v, &[bytes.clone(), off], &o);
        assert_eq!(files.len(), 1);
        assert_eq!(std::fs::read(v.abs(&files[0])).unwrap(), b"jpg");
        assert_eq!(bytes.kind(), "Image");
        assert!(bytes.detail().contains("web.jpg") && bytes.detail().contains("KB"));
        // a name typed without the extension gets it
        bytes.title = "holiday".into();
        let Imported { files, .. } = import(&mut v, &[bytes], &o);
        assert!(rel_to_string(&files[0]).ends_with("_holiday.jpg"), "{files:?}");
        // media next to a note item: the note is made, the file saved
        let mixed = vec![event_item(Event { uid: "u".into(), summary: "Termin".into(), start: 0, end: 3600, ..Default::default() }, "Work".into(), None), media_item(media::Kind::Pdf, "a.pdf".into(), MediaData::Bytes(b"%PDF".to_vec()))];
        let r = import(&mut v, &mixed, &o);
        assert_eq!((r.notes.len(), r.files.len()), (1, 1));
        let _ = std::fs::remove_dir_all(&src);
    }

    // spec: VAULT-027
    #[test]
    fn imported_notes_and_files_get_portable_names_within_127_bytes() {
        let mut v = temp_vault("portable");
        let day = t::days_from_civil(2026, 12, 2);
        let long_subject = format!("Re: Plan? {}", "wörter ".repeat(60));
        let e = |summary: &str| Event { uid: format!("ev-{}@x", summary.len()), summary: summary.into(), start: t::local_to_utc(day, 10 * 3600), end: t::local_to_utc(day, 11 * 3600), ..Default::default() };
        let mut mail = crate::mail::parse(&crate::mail::tests::sample_eml());
        mail.subject = long_subject;
        let src = std::env::temp_dir().join(format!("shale-import-portable-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(&src).unwrap();
        let file = src.join("Notes: 1?.txt");
        std::fs::write(&file, b"x").unwrap();
        let mut items = vec![
            email_item(mail, ("Mail: original?.eml".into(), b"raw".to_vec())),
            event_item(e("Meeting: Q4 *final*"), "Work".into(), Some(("Cal: x?.ics".into(), b"BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n".to_vec()))),
            markdown_item("Idea: A?.md".into(), "# Idea\n".into()),
            media_item(media::Kind::Pdf, "Plan: v2?.pdf".into(), MediaData::Bytes(b"%PDF".to_vec())),
        ];
        items.extend(items_from_paths(std::slice::from_ref(&file)).0);
        let o = Options { folder: PathBuf::from("In"), tags: vec![], keep_original: true, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false };
        let Imported { notes, files, errors } = import(&mut v, &items, &o);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(notes.len(), 4, "{notes:?}");
        for n in notes.iter().chain(&files) {
            let name = n.file_name().unwrap().to_string_lossy().into_owned();
            assert!(!name.contains([':', '?', '*', '<', '>']) && name.len() <= 127, "{name} ({} bytes)", name.len());
            assert!(v.root.join(n).exists(), "{n:?}");
        }
        let names: Vec<String> = notes.iter().map(|n| n.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert!(names.contains(&"2026-12-02 Meeting- Q4 -final-.md".to_string()), "{names:?}");
        assert!(names.contains(&"Idea- A-.md".to_string()), "{names:?}");
        assert!(files.iter().any(|f| f.to_string_lossy().ends_with("_Plan- v2-.pdf")), "{files:?}");
        // the links in the notes name the files as they really are
        let text = |n: &PathBuf| std::fs::read_to_string(v.abs(n)).unwrap();
        let ev = notes.iter().find(|n| n.to_string_lossy().contains("Meeting")).unwrap();
        assert!(text(ev).contains("_Cal- x-.ics]]"), "{}", text(ev));
        let em = notes.iter().find(|n| n.to_string_lossy().contains("Re- Plan")).unwrap();
        assert!(text(em).contains("_Mail- original-.eml]]"), "{}", text(em));
        let fl = notes.iter().find(|n| n.to_string_lossy().contains("Notes- 1-")).unwrap();
        assert!(text(fl).contains("_Notes- 1-.txt]]"), "{}", text(fl));
        let _ = std::fs::remove_dir_all(&src);
    }
}
