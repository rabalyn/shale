//! The other content of the benchmarks (BUILD-029): Word files, drawings, mail, calendar events,
//! social posts, audio. Everything comes out of a fixed seed, so two runs read the same bytes.

use super::vault_gen::Rng;
use serde_json::{json, Value};
use std::fmt::Write as _;

const WORDS: &[&str] = &[
    "garden", "plan", "river", "meeting", "recipe", "budget", "project", "reading", "travel", "idea", "design", "history", "kitchen", "winter", "network", "library", "system", "memory", "problem", "answer", "season", "journal", "review",
    "draft", "outline", "summary", "research", "paper", "chapter", "theory", "method", "result", "question", "goal", "habit", "health", "music", "photo", "forest", "mountain", "market", "coffee", "invoice", "report", "customer",
    "supplier", "schedule", "deadline", "release", "feature", "server", "cache", "queue", "pattern", "lesson",
];

fn words(r: &mut Rng, n: usize) -> String {
    (0..n).map(|_| WORDS[r.below(WORDS.len())]).collect::<Vec<_>>().join(" ")
}

fn sentences(r: &mut Rng, n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        if i > 0 {
            s.push(' ');
        }
        let n = 6 + r.below(8);
        let w = words(r, n);
        let mut c = w.chars();
        s.extend(c.next().map(|f| f.to_ascii_uppercase()));
        s.push_str(c.as_str());
        s.push('.');
    }
    s
}

// ---- a zip and a Word file -------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    !data.iter().fold(!0u32, |c, b| table[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8))
}

/// A zip file of stored (not compressed) entries.
pub fn zip(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let head = |out: &mut Vec<u8>, sig: u32, central: bool| {
            out.extend(sig.to_le_bytes());
            if central {
                out.extend(20u16.to_le_bytes());
            }
            out.extend([20, 0, 0, 0, 0, 0]);
            out.extend([0, 0, 0x21, 0]);
            out.extend(crc.to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
        };
        head(&mut out, 0x0403_4b50, false);
        out.extend(name.as_bytes());
        out.extend(data);
        head(&mut central, 0x0201_4b50, true);
        central.extend([0u8; 10]);
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let (cd_offset, cd_size) = (out.len() as u32, central.len() as u32);
    out.extend(central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend(cd_size.to_le_bytes());
    out.extend(cd_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/></w:style></w:styles>"#;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A Word file of at least `min_bytes` of text: headings, paragraphs with bold and italic runs,
/// lists and tables.
pub fn docx(seed: u64, min_bytes: usize) -> Vec<u8> {
    let mut r = Rng::new(seed);
    let mut body = String::new();
    let para = |body: &mut String, style: Option<&str>, runs: &[(&str, bool, bool)]| {
        body.push_str("<w:p>");
        if let Some(s) = style {
            let _ = write!(body, "<w:pPr><w:pStyle w:val=\"{s}\"/></w:pPr>");
        }
        for (t, b, i) in runs {
            body.push_str("<w:r>");
            if *b || *i {
                let _ = write!(body, "<w:rPr>{}{}</w:rPr>", if *b { "<w:b/>" } else { "" }, if *i { "<w:i/>" } else { "" });
            }
            let _ = write!(body, "<w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(t));
        }
        body.push_str("</w:p>");
    };
    para(&mut body, Some("Heading1"), &[(&format!("Report {}", words(&mut r, 3)), false, false)]);
    while body.len() < min_bytes {
        let h = format!("{} {}", words(&mut r, 2), r.below(100));
        para(&mut body, Some("Heading2"), &[(&h, false, false)]);
        for _ in 0..2 + r.below(4) {
            let (a, b, c) = (sentences(&mut r, 2), words(&mut r, 3), sentences(&mut r, 3));
            para(&mut body, None, &[(&a, false, false), (&b, true, false), (&c, false, r.chance(30))]);
        }
        if r.chance(20) {
            body.push_str("<w:tbl><w:tblPr/><w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid>");
            for _ in 0..3 + r.below(4) {
                body.push_str("<w:tr>");
                for _ in 0..3 {
                    let cell = words(&mut r, 2);
                    let _ = write!(body, "<w:tc><w:p><w:r><w:t>{cell}</w:t></w:r></w:p></w:tc>");
                }
                body.push_str("</w:tr>");
            }
            body.push_str("</w:tbl>");
            para(&mut body, None, &[("", false, false)]);
        }
    }
    let document = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>");
    zip(&[
        ("[Content_Types].xml", br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#.to_vec()),
        ("_rels/.rels", br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec()),
        ("word/_rels/document.xml.rels", br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.to_vec()),
        ("word/styles.xml", STYLES.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
    ])
}

// ---- a drawing -------------------------------------------------------------------------------

/// An Excalidraw file with `n` elements: boxes, ellipses, arrows, free lines and texts.
pub fn drawing(seed: u64, n: usize) -> String {
    let mut r = Rng::new(seed);
    let elements: Vec<Value> = (0..n)
        .map(|i| {
            let (x, y) = ((r.below(4000)) as f64, (r.below(3000)) as f64);
            let kind = ["rectangle", "ellipse", "arrow", "line", "text", "diamond"][i % 6];
            let mut e = json!({"id": format!("el{i}"), "type": kind, "x": x, "y": y, "width": 40.0 + r.below(160) as f64, "height": 30.0 + r.below(100) as f64, "strokeColor": "#1e1e1e", "backgroundColor": if r.chance(40) { "#a5d8ff" } else { "transparent" }, "fillStyle": "solid", "strokeWidth": 2, "roughness": 1, "opacity": 100, "seed": r.below(1 << 20), "version": 1, "versionNonce": 7, "isDeleted": false});
            match i % 6 {
                2 | 3 => e["points"] = json!([[0.0, 0.0], [60.0, 20.0], [120.0, -10.0]]),
                4 => {
                    e["text"] = json!(words(&mut r, 3));
                    e["fontSize"] = json!(20);
                }
                _ => {}
            }
            e
        })
        .collect();
    json!({"type": "excalidraw", "version": 2, "source": "shale-bench", "elements": elements, "appState": {"viewBackgroundColor": "#ffffff"}, "files": {}}).to_string()
}

// ---- mail ------------------------------------------------------------------------------------

const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// An RFC 2822 date of `secs` since 1970 (UTC).
pub fn rfc2822(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (y, m, d) = crate::calendar::time::civil_from_days(days);
    format!("{}, {d:02} {} {y} {:02}:{:02}:{:02} +0000", DAYS[days.rem_euclid(7) as usize], MONTHS[m as usize - 1], rest / 3600, rest % 3600 / 60, rest % 60)
}

/// Message `i` of a mailbox: plain text, 1 in 20 with an attachment, 1 in 10 HTML.
pub fn eml(i: usize) -> (Vec<u8>, i64) {
    let mut r = Rng::new(0xA11CE ^ (i as u64).wrapping_mul(0x9E37_79B9));
    let date = 1_780_000_000 + i as i64 * 600;
    let from = format!("{} {} <{}{}@example.com>", words(&mut r, 1), words(&mut r, 1), words(&mut r, 1), r.below(500));
    let subject = format!("{} {} #{i}", words(&mut r, 2), words(&mut r, 2));
    let paragraphs = 2 + r.below(6);
    let body: String = (0..paragraphs)
        .map(|_| {
            let n = 3 + r.below(4);
            sentences(&mut r, n) + "\r\n\r\n"
        })
        .collect();
    let mut m = format!("From: {from}\r\nTo: Ada <ada@example.com>\r\nSubject: {subject}\r\nDate: {}\r\nMessage-ID: <bench-{i}@example.com>\r\nMIME-Version: 1.0\r\n", rfc2822(date));
    if i.is_multiple_of(20) {
        let blob: String = (0..40).map(|k| format!("{:08x}", (k * 2_654_435_761u64 + i as u64) as u32)).collect::<Vec<_>>().join("\r\n");
        let _ = write!(m, "Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}--b1\r\nContent-Type: application/octet-stream; name=\"data{i}.bin\"\r\nContent-Disposition: attachment; filename=\"data{i}.bin\"\r\nContent-Transfer-Encoding: 7bit\r\n\r\n{blob}\r\n--b1--\r\n");
    } else if i % 10 == 1 {
        let _ = write!(m, "Content-Type: text/html; charset=utf-8\r\n\r\n<html><body><p>{}</p><p><b>{}</b></p></body></html>\r\n", body.replace("\r\n\r\n", "</p><p>"), words(&mut r, 4));
    } else {
        let _ = write!(m, "Content-Type: text/plain; charset=utf-8\r\n\r\n{body}");
    }
    (m.into_bytes(), date)
}

// ---- calendar --------------------------------------------------------------------------------

/// An iCalendar file with `n` events: a quarter repeat weekly, a tenth daily with a count, a few
/// monthly with excluded days, the rest single; some all-day.
pub fn ics(n: usize) -> String {
    let mut r = Rng::new(0xCA1);
    let mut s = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//shale-bench//EN\r\n");
    let base = crate::calendar::time::days_from_civil(2026, 1, 1);
    for i in 0..n {
        let day = base + r.below(365) as i64;
        let (y, m, d) = crate::calendar::time::civil_from_days(day);
        let _ = write!(s, "BEGIN:VEVENT\r\nUID:bench-{i}@example.com\r\nSUMMARY:{}\r\nDESCRIPTION:{}\r\nLOCATION:Room {}\r\n", words(&mut r, 3), sentences(&mut r, 2), r.below(40));
        if i % 7 == 0 {
            let _ = write!(s, "DTSTART;VALUE=DATE:{y:04}{m:02}{d:02}\r\nDTEND;VALUE=DATE:{y:04}{m:02}{:02}\r\n", d.min(27) + 1);
        } else {
            let h = 7 + r.below(11);
            let _ = write!(s, "DTSTART:{y:04}{m:02}{d:02}T{h:02}0000Z\r\nDTEND:{y:04}{m:02}{d:02}T{:02}3000Z\r\n", h);
        }
        match i % 20 {
            0..=4 => s.push_str("RRULE:FREQ=WEEKLY;COUNT=40\r\n"),
            5 | 6 => s.push_str("RRULE:FREQ=DAILY;COUNT=60\r\n"),
            7 => s.push_str("RRULE:FREQ=MONTHLY;BYMONTHDAY=15\r\n"),
            _ => {}
        }
        s.push_str("END:VEVENT\r\n");
    }
    s.push_str("END:VCALENDAR\r\n");
    s
}

// ---- social ----------------------------------------------------------------------------------

/// A Mastodon status as the server sends it.
pub fn mastodon_status(i: usize) -> Value {
    let mut r = Rng::new(0x50C1 ^ i as u64);
    let n = 2 + r.below(3);
    let text = sentences(&mut r, n);
    json!({
        "id": format!("{}", 110_000_000 + i), "url": format!("https://demo.social/@u{}/{}", i % 97, 110_000_000 + i), "created_at": "2026-11-10T09:30:00.000Z",
        "content": format!("<p>{text} <a href=\"https://demo.social/tags/{0}\" class=\"hashtag\">#<span>{0}</span></a> <a href=\"https://example.org/{1}\">example.org/{1}</a></p>", words(&mut r, 1), i),
        "spoiler_text": "", "replies_count": r.below(20), "reblogs_count": r.below(50), "favourites_count": r.below(400), "in_reply_to_id": null, "media_attachments": [], "reblog": null, "card": null,
        "account": {"id": format!("{}", i % 97), "username": format!("u{}", i % 97), "acct": format!("u{}@demo.social", i % 97), "display_name": format!("User {}", i % 97), "avatar": ""}
    })
}

/// A Bluesky post view as the server sends it.
pub fn bluesky_post(i: usize) -> Value {
    let mut r = Rng::new(0xB5C1 ^ i as u64);
    let n = 2 + r.below(3);
    let text = sentences(&mut r, n);
    json!({
        "uri": format!("at://did:plc:u{}/app.bsky.feed.post/{i}", i % 97), "cid": format!("bafy{i}"),
        "author": {"did": format!("did:plc:u{}", i % 97), "handle": format!("u{}.bsky.social", i % 97), "displayName": format!("User {}", i % 97), "avatar": ""},
        "record": {"$type": "app.bsky.feed.post", "text": text, "createdAt": "2026-11-10T09:30:00.000Z"},
        "replyCount": r.below(20), "repostCount": r.below(50), "likeCount": r.below(400), "indexedAt": "2026-11-10T09:30:01.000Z"
    })
}

/// The HTML of a Mastodon post of `i`.
pub fn post_html(i: usize) -> String {
    let mut r = Rng::new(0x47 ^ i as u64);
    format!("<p>{} <a href=\"https://demo.social/@u{}\" class=\"u-url mention\">@<span>u{}</span></a> <a href=\"https://example.org/{i}\" rel=\"nofollow\">example.org/{i}</a><br>{} &amp; more</p><p><strong>{}</strong> <em>{}</em> <code>{}</code></p>", sentences(&mut r, 3), i % 97, i % 97, sentences(&mut r, 1), words(&mut r, 2), words(&mut r, 2), words(&mut r, 1))
}

// ---- audio -----------------------------------------------------------------------------------

/// A WAV file of `secs` seconds of stereo 16-bit audio at 44.1 kHz: a few sine tones.
pub fn wav(secs: usize) -> Vec<u8> {
    let (rate, frames) = (44_100usize, 44_100 * secs);
    let mut out = Vec::with_capacity(44 + frames * 4);
    out.extend(b"RIFF");
    out.extend(((36 + frames * 4) as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend([1, 0, 2, 0]);
    out.extend((rate as u32).to_le_bytes());
    out.extend(((rate * 4) as u32).to_le_bytes());
    out.extend([4, 0, 16, 0]);
    out.extend(b"data");
    out.extend(((frames * 4) as u32).to_le_bytes());
    for t in 0..frames {
        let x = t as f64 / rate as f64;
        let v = ((x * 440.0 * std::f64::consts::TAU).sin() * 0.4 + (x * 660.0 * std::f64::consts::TAU).sin() * 0.2) * 20_000.0;
        let s = (v as i16).to_le_bytes();
        out.extend(s);
        out.extend(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: BUILD-029
    #[test]
    fn the_content_is_deterministic_and_what_the_programs_read() {
        assert_eq!(docx(1, 5000), docx(1, 5000));
        assert_ne!(docx(1, 5000), docx(2, 5000));
        let z = docx(3, 50_000);
        assert!(z.len() > 50_000 && z.starts_with(b"PK\x03\x04"));
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let scene = crate::drawing::Scene::parse(&drawing(1, 120)).unwrap();
        assert_eq!(scene.live().count(), 120);
        let (raw, date) = eml(0);
        assert!(String::from_utf8_lossy(&raw).contains("filename=\"data0.bin\""), "every twentieth has an attachment");
        assert_eq!(eml(5).0, eml(5).0);
        assert!(date > 0);
        assert_eq!(rfc2822(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        let events = crate::calendar::ics::parse_ics(&ics(100));
        assert_eq!(events.len(), 100);
        assert!(events.iter().any(|e| e.rrule.is_some()) && events.iter().any(|e| e.all_day));
        assert!(crate::social::mastodon::parse_status(&mastodon_status(3), "https://demo.social").is_some());
        assert!(crate::social::bluesky::parse_post_view(&bluesky_post(3)).is_some());
        assert!(crate::social::markup::html_to_markdown(&post_html(1)).contains("example.org"));
        let w = wav(1);
        assert_eq!(w.len(), 44 + 44_100 * 4);
    }
}
