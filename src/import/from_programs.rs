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

/// The files of a `text/uri-list` and what could not be taken: one URI per line (CRLF or LF),
/// empty lines and `#` comments skipped.
fn uri_list(text: &str) -> (Vec<PathBuf>, Vec<String>) {
    let mut paths = Vec::new();
    let mut notes = Vec::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        match media::file_uri_path(l) {
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
    items_from_parts_in(parts, &crate::import::tb_store::default_roots(), &crate::social::download)
}

/// Fetches a web address: the bytes and the content type, if the server named one.
type Download<'a> = &'a dyn Fn(&str) -> Result<(Vec<u8>, Option<String>), String>;

/// The file name an address ends in (`https://x.org/a/b%20c.png?s=1` → `b c.png`), if it has one.
fn url_file_name(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let (_, path) = path.split_once("://")?.1.split_once('/')?;
    let name = media::percent_decode(path.trim_end_matches('/').rsplit('/').next()?);
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The first http(s) address of text, one per line.
fn web_urls(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| l.starts_with("http://") || l.starts_with("https://")).map(String::from).collect()
}

/// A media item for a drag out of a web browser (IMP-032): a picture it handed over, else a picture,
/// PDF or audio file downloaded from the address it offered. None if there is no such thing; an
/// Err if the download failed.
fn media_from_parts(parts: &[(String, Vec<u8>)], download: Download) -> Option<Result<Item, String>> {
    let text_of_kind = |want: &str| parts.iter().find(|(m, _)| norm(m) == want).map(|(_, d)| text_of(d)).unwrap_or_default();
    let promised = text_of_kind("application/x-moz-file-promise-dest-filename").trim().trim_end_matches('\0').to_string();
    let promised = (!promised.is_empty()).then_some(promised);
    let promise_url = web_urls(&text_of_kind("application/x-moz-file-promise-url"));
    let mut urls = promise_url.clone();
    urls.extend(web_urls(&text_of_kind("text/uri-list")));
    urls.extend(web_urls(text_of_kind("text/x-moz-url").lines().next().unwrap_or("")));
    let first_url = urls.first().cloned();
    let named = |url: Option<&str>| promised.clone().or_else(|| url.and_then(url_file_name));
    // a picture the browser handed over
    if let Some((mime, data)) = parts.iter().find(|(m, d)| (norm(m).starts_with("image/") || norm(m).starts_with("video/")) && !d.is_empty()) {
        let video = norm(mime).starts_with("video/");
        let ext = media::ext_for_mime(mime).unwrap_or(if video { "mp4" } else { "png" });
        let name = named(first_url.as_deref()).filter(|n| media::media_kind(Path::new(n)).is_some()).unwrap_or_else(|| format!("{}.{ext}", if video { "video" } else { "image" }));
        let kind = media::media_kind(Path::new(&name))?;
        return Some(Ok(media_item(kind, name, MediaData::Bytes(data.clone()))));
    }
    // an address of a file
    let offered_image = parts.iter().any(|(m, _)| norm(m).starts_with("image/") || norm(m).starts_with("video/"));
    let url = urls.iter().find(|u| url_file_name(u).is_some_and(|n| media::media_kind(Path::new(&n)).is_some()) || offered_image || promise_url.contains(u))?;
    let (data, content_type) = match download(url) {
        Ok(r) => r,
        Err(e) => return Some(Err(format!("Could not download “{url}”: {e}"))),
    };
    let mut name = named(Some(url)).unwrap_or_else(|| "image".to_string());
    if media::media_kind(Path::new(&name)).is_none() {
        let Some(ext) = content_type.as_deref().and_then(media::ext_for_mime) else {
            return Some(Err(format!("“{url}” is not a picture, PDF, audio or video file{}.", content_type.map(|c| format!(" (the server says {c})")).unwrap_or_default())));
        };
        name = format!("{name}.{ext}");
    }
    let kind = media::media_kind(Path::new(&name))?;
    Some(Ok(media_item(kind, name, MediaData::Bytes(data))))
}

fn items_from_parts_in(parts: &[(String, Vec<u8>)], tb_roots: &[PathBuf], download: Download) -> (Vec<Item>, Vec<String>) {
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
    // a picture, PDF, audio or video file out of a web browser
    let mut download_error = None;
    match media_from_parts(parts, download) {
        Some(Ok(item)) => return (vec![item], vec![]),
        Some(Err(e)) => download_error = Some(e),
        None => {}
    }
    // plain text
    for k in ["text/plain;charset=utf-8", "utf8_string", "text/plain", "string"] {
        if let Some(d) = get(k) {
            let (items, n) = items_from_drop("text/plain", d);
            if !items.is_empty() {
                return (items, n.into_iter().chain(download_error).collect());
            }
        }
    }
    notes.extend(download_error);
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

    fn no_download(_: &str) -> Result<(Vec<u8>, Option<String>), String> {
        panic!("no download expected")
    }

    // spec: IMP-003
    #[test]
    fn dropped_text_becomes_a_note() {
        let d = std::env::temp_dir().join(format!("shale-import-text-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut v = Vault::open(&d).unwrap();
        let (items, _) = items_from_drop("text/plain", "Beamer tauschen\nim Hörsaal".as_bytes());
        assert_eq!(items.len(), 1);
        let o = Options { folder: PathBuf::from("Projekte"), tags: vec!["status".into()], keep_original: true, keep_docx: true, people_dir: "Adressbuch".into() };
        let Imported { notes: made, errors: errs, .. } = import(&mut v, &items, &o);
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
        let (items, notes) = items_from_parts_in(&parts, std::slice::from_ref(&root), &no_download);
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
        let (items, notes) = items_from_parts_in(&parts, std::slice::from_ref(&root), &no_download);
        assert!(items.is_empty() && notes[0].contains("only a link"), "{notes:?}");
        // nothing at all: shows what was sent
        let (items, notes) = items_from_parts_in(&[("text/uri-list".into(), vec![]), ("STRING".into(), vec![])], &[], &no_download);
        assert!(items.is_empty() && notes.iter().any(|n| n.contains("text/uri-list: (empty)")), "{notes:?}");
        // the normal cases still work
        let (items, _) = items_from_parts_in(&[("text/uri-list".into(), vec![]), ("text/plain;charset=utf-8".into(), b"Notiz".to_vec())], &[], &no_download);
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
            assert_eq!(media::file_uri_path(&uri), Some(PathBuf::from(p)), "{uri}");
        }
        assert_eq!(file_uri(Path::new("/a b")), "file:///a%20b");
    }

    fn media_of(item: &Item) -> (String, Vec<u8>) {
        match &item.source {
            Source::Media { name, data: MediaData::Bytes(b), .. } => (name.clone(), b.clone()),
            _ => panic!("not a media item: {:?}", item.source),
        }
    }

    // spec: IMP-032
    #[test]
    fn a_picture_dragged_out_of_a_browser_is_a_media_item() {
        let utf16 = |s: &str| s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>();
        // the browser hands over the picture itself: used as it is, named by the offered file name
        let parts = vec![
            ("text/uri-list".to_string(), b"https://example.org/img/cat%20photo.jpg?w=1\r\n".to_vec()),
            ("application/x-moz-file-promise-dest-filename".to_string(), b"cat.jpg".to_vec()),
            ("text/plain".to_string(), b"https://example.org/img/cat%20photo.jpg".to_vec()),
            ("image/png".to_string(), b"PNGDATA".to_vec()),
        ];
        let (items, notes) = items_from_parts_in(&parts, &[], &no_download);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!((items[0].kind().to_string(), media_of(&items[0])), ("Image".into(), ("cat.jpg".into(), b"PNGDATA".to_vec())));
        // no file name offered: the last part of the address, else image.<ext>
        let parts = vec![("text/uri-list".to_string(), b"https://example.org/img/cat%20photo.jpg?w=1".to_vec()), ("image/jpeg".to_string(), b"J".to_vec())];
        assert_eq!(media_of(&items_from_parts_in(&parts, &[], &no_download).0[0]).0, "cat photo.jpg");
        let parts = vec![("image/webp".to_string(), b"W".to_vec()), ("text/plain".to_string(), b"hello".to_vec())];
        assert_eq!(media_of(&items_from_parts_in(&parts, &[], &no_download).0[0]).0, "image.webp");
        // only the address: downloaded (the file promise of Firefox, the address of text/uri-list, the first line of text/x-moz-url)
        let fetched = std::cell::RefCell::new(Vec::new());
        let download = |u: &str| -> Result<(Vec<u8>, Option<String>), String> {
            fetched.borrow_mut().push(u.to_string());
            Ok((b"BYTES".to_vec(), Some("image/gif".into())))
        };
        let parts = vec![("application/x-moz-file-promise-url".to_string(), b"https://cdn.example.org/get?id=7".to_vec()), ("text/plain".to_string(), b"https://cdn.example.org/get?id=7".to_vec())];
        let (items, notes) = items_from_parts_in(&parts, &[], &download);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(media_of(&items[0]), ("get.gif".into(), b"BYTES".to_vec()), "the content type gives the extension");
        let parts = vec![("text/uri-list".to_string(), b"https://example.org/files/plan.pdf\r\n".to_vec())];
        let (items, _) = items_from_parts_in(&parts, &[], &download);
        assert_eq!((items[0].kind().to_string(), media_of(&items[0]).0), ("PDF".into(), "plan.pdf".into()), "a PDF address needs no image kind");
        let parts = vec![("text/x-moz-url".to_string(), utf16("https://example.org/a/song.mp3\nA song"))];
        assert_eq!(items_from_parts_in(&parts, &[], &download).0[0].kind(), "Audio");
        let parts = vec![("video/webm".to_string(), b"V".to_vec())];
        let (items, _) = items_from_parts_in(&parts, &[], &no_download);
        assert_eq!((items[0].kind().to_string(), media_of(&items[0]).0), ("Video".into(), "video.webm".into()));
        let parts = vec![("text/uri-list".to_string(), b"https://example.org/clips/a.MP4".to_vec())];
        assert_eq!(items_from_parts_in(&parts, &[], &download).0[0].kind(), "Video");
        assert_eq!(*fetched.borrow(), vec!["https://cdn.example.org/get?id=7", "https://example.org/files/plan.pdf", "https://example.org/a/song.mp3", "https://example.org/clips/a.MP4"]);
        // a link to an ordinary page is no download: a text note as before
        let parts = vec![("text/uri-list".to_string(), b"https://example.org/page\r\n".to_vec()), ("text/plain".to_string(), b"https://example.org/page".to_vec())];
        let (items, _) = items_from_parts_in(&parts, &[], &no_download);
        assert_eq!(items[0].kind(), "Text");
    }

    // spec: IMP-032
    #[test]
    fn a_failed_download_is_reported_and_the_drop_falls_back_to_text() {
        let parts = vec![("text/uri-list".to_string(), b"https://example.org/a/cat.png\r\n".to_vec()), ("text/plain".to_string(), b"https://example.org/a/cat.png".to_vec())];
        let (items, notes) = items_from_parts_in(&parts, &[], &|_| Err("the server answered with status 404".into()));
        assert_eq!(items[0].kind(), "Text", "the address as a text note, like before");
        assert!(notes.iter().any(|n| n.contains("Could not download “https://example.org/a/cat.png”") && n.contains("404")), "{notes:?}");
        // an address that is no picture although a picture was offered
        let parts = vec![("text/uri-list".to_string(), b"https://example.org/view".to_vec()), ("image/png".to_string(), vec![])];
        let (items, notes) = items_from_parts_in(&parts, &[], &|_| Ok((b"<html>".to_vec(), Some("text/html".into()))));
        assert!(items.is_empty() && notes.iter().any(|n| n.contains("is not a picture, PDF, audio or video file") && n.contains("text/html")), "{notes:?}");
    }

    // spec: IMP-032
    #[test]
    fn a_picture_comes_from_a_real_address() {
        // the real downloader against a tiny local server
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut c, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let _ = c.read(&mut buf);
            c.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\nConnection: close\r\n\r\nPNG!").unwrap();
        });
        let url = format!("http://127.0.0.1:{port}/pics/dot");
        let parts = vec![("application/x-moz-file-promise-url".to_string(), url.clone().into_bytes())];
        let (items, notes) = items_from_parts_in(&parts, &[], &crate::social::download);
        server.join().unwrap();
        assert_eq!(media_of(&items[0]), ("dot.png".into(), b"PNG!".to_vec()), "{notes:?}");
    }
}
