//! Drops from other programs, as Wayland delivers them (`wl_drop.rs`): the kinds the other
//! program offers (a whole message, calendar data, links, text) and what it sent for each. On
//! macOS and Windows the window toolkit delivers dropped files only, which `items_from_paths` takes
//! (IMP-011); this module is compiled on Linux only.

use super::*;

fn text_item(text: String) -> Item {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("Text");
    let title = safe_title(&format!("{} {}", ymd_text(today_ymd()), first.chars().take(60).collect::<String>()));
    Item { source: Source::Text(text), include: true, title, att_on: vec![], already: None, people: vec![] }
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

/// The files of a `text/uri-list` and what could not be taken: one URI per line (CRLF or LF),
/// empty lines and `#` comments skipped.
fn uri_list(text: &str) -> (Vec<PathBuf>, Vec<String>) {
    let mut paths = Vec::new();
    let mut notes = Vec::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        match file_uri_path(l) {
            Some(p) => paths.push(p),
            None if is_mail_link(l) => notes.push(MAIL_LINK_HELP.to_string()),
            None => notes.push(format!("“{l}” is not a file on this computer.")),
        }
    }
    notes.dedup();
    (paths, notes)
}

/// `file://` URI of a path, as a file manager writes it (everything but letters, digits and
/// `-_.~/` percent-encoded).
pub fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
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
    items_from_parts_in(parts, &crate::import::tb_store::default_roots())
}

fn items_from_parts_in(parts: &[(String, Vec<u8>)], tb_roots: &[PathBuf]) -> (Vec<Item>, Vec<String>) {
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
        return match crate::import::tb_store::find_in(tb_roots, &links, file_name.as_deref()) {
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
        let (paths, mut notes) = uri_list(text);
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

#[cfg(test)]
mod tests {
    use super::*;

    // spec: IMP-003
    #[test]
    fn dropped_text_becomes_a_note() {
        let d = std::env::temp_dir().join(format!("shale-import-text-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut v = Vault::open(&d).unwrap();
        let (items, _) = items_from_drop("text/plain", "Beamer tauschen\nim Hörsaal".as_bytes());
        assert_eq!(items.len(), 1);
        let o = Options { folder: PathBuf::from("Projekte"), tags: vec!["status".into()], keep_original: true, people_dir: "Adressbuch".into() };
        let (made, errs) = import(&mut v, &items, &o);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(made.len(), 1);
        let s = std::fs::read_to_string(v.abs(&made[0])).unwrap();
        assert!(s.contains("Beamer tauschen\nim Hörsaal"));
        assert!(made[0].to_string_lossy().ends_with("Beamer tauschen.md"));
        let _ = std::fs::remove_dir_all(&d);
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
    // spec: IMP-014
    #[test]
    fn imp_014_a_uri_list_is_read_line_by_line() {
        let text = "# a comment\r\nfile:///tmp/a%20b.md\r\n\r\n  file://localhost/tmp/c.md  \nfile:///tmp/%C3%A4%C3%B6.md\nfile://otherhost/tmp/d.md\nhttps://example.org/x\nhttps://example.org/x\nimap-message://ada@mail/INBOX#1\nfile:///tmp/bad%zz.md\nfile:///tmp/end%41";
        let (paths, notes) = uri_list(text);
        let want: Vec<PathBuf> = ["/tmp/a b.md", "/tmp/c.md", "/tmp/äö.md", "/tmp/bad%zz.md", "/tmp/endA"].iter().map(PathBuf::from).collect();
        assert_eq!(paths, want);
        assert_eq!(notes.len(), 3, "the same reason twice in a row comes once: {notes:?}");
        assert!(notes[0].contains("file://otherhost/tmp/d.md") && notes[1].contains("https://example.org/x"));
        assert!(notes[2].contains("Save As"), "{notes:?}");
        assert_eq!(uri_list("").0.len() + uri_list("# only\r\n").0.len(), 0);
        assert!(uri_list("file:relative").1[0].contains("not a file"));
        // a list with nothing usable gives no items, only the reasons
        let (items, notes) = items_from_drop("text/uri-list", b"https://example.org/x\r\n");
        assert!(items.is_empty() && notes.len() == 1);
    }

    // spec: IMP-014
    #[test]
    fn imp_014_a_path_survives_the_trip_through_a_uri() {
        for p in ["/tmp/plain.md", "/tmp/a b/ü ö#1%.md", "/tmp/colon:semi;amp&.md", "/tmp/日本語.md"] {
            let uri = file_uri(Path::new(p));
            assert!(uri.is_ascii() && !uri.contains(' ') && uri.starts_with("file:///"), "{uri}");
            assert_eq!(file_uri_path(&uri), Some(PathBuf::from(p)), "{uri}");
        }
        assert_eq!(file_uri(Path::new("/a b")), "file:///a%20b");
    }
}
