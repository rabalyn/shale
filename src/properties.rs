//! How the front matter of a note is shown: a display model that is independent of the editor and
//! of egui (EDIT-060 to EDIT-063).
//!
//! * [`summary`] is the one line of a folded front matter: `email · Anna Müller · 2026-11-12 · #work #mail`.
//! * [`Card::parse`] reads the properties into a typed card ([`Kind`]: e-mail, event, Word import,
//!   generic) that the reading view draws.
//! * [`is_technical`] says which keys are ids and print settings that no summary and no card body shows.
//!
//! Nothing here interprets dates or numbers beyond cutting a date out of a stamp; the values are the
//! text of the file (`markdown::front_matter_values`).

use crate::import::people;
use crate::markdown;

/// What a note's front matter describes (EDIT-062).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `type: email`
    Email,
    /// `type: event`
    Event,
    /// any other note with a `print-*` key: a Word import
    Word,
    /// everything else: key and value rows
    Generic,
}

/// Keys that are ids or settings for other programs, never shown in a summary or the body of a card
/// (EDIT-061). `print-*` keys are technical as a whole.
pub fn is_technical(key: &str) -> bool {
    let k = key.trim().to_lowercase().replace('_', "-");
    k.starts_with("print-") || matches!(k.as_str(), "message-id" | "in-reply-to" | "references" | "event-uid" | "uid" | "id" | "source" | "numbered-headings")
}

/// A person of an address list: `"Müller, Hans" <h@x.de>` is name `Hans Müller`, email `h@x.de`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub email: String,
}

impl Person {
    /// What is shown: the name, the address when there is no name.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.email
        } else {
            &self.name
        }
    }
}

/// The people in a value such as a `from` or `to` property (EDIT-062).
pub fn people_in(value: &str) -> Vec<Person> {
    people::parse_addresses(value).into_iter().map(|(name, email)| Person { name, email }).collect()
}

/// The date part of a stamp: `2026-11-12 14:30` and `2026-11-12T14:30` give `2026-11-12`.
pub fn date_part(stamp: &str) -> &str {
    let s = stamp.trim();
    s.split([' ', 'T']).next().unwrap_or(s)
}

/// The front matter of a note as a card (EDIT-062).
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub kind: Kind,
    /// every property in file order: key in lower case, values
    entries: Vec<(String, Vec<String>)>,
}

impl Card {
    /// The card of `text`'s front matter; `None` when the note has none or it holds no property.
    pub fn parse(text: &str) -> Option<Card> {
        let entries = markdown::front_matter_values(text);
        if entries.is_empty() {
            return None;
        }
        let kind = match entries.iter().find(|(k, _)| k == "type").and_then(|(_, v)| v.first()).map(|v| v.trim().to_lowercase()).as_deref() {
            Some("email") => Kind::Email,
            Some("event") => Kind::Event,
            _ if entries.iter().any(|(k, _)| k.starts_with("print-")) => Kind::Word,
            _ => Kind::Generic,
        };
        Some(Card { kind, entries })
    }

    /// The values of a property (`key` in lower case).
    pub fn values(&self, key: &str) -> &[String] {
        self.entries.iter().find(|(k, _)| k == key).map_or(&[], |(_, v)| v.as_slice())
    }

    /// The first non-empty value of a property.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values(key).iter().map(|v| v.trim()).find(|v| !v.is_empty())
    }

    /// The tags without a leading `#`.
    pub fn tags(&self) -> Vec<String> {
        self.entries.iter().filter(|(k, _)| k == "tags" || k == "tag").flat_map(|(_, v)| v.iter()).flat_map(|v| v.split([',', ' '])).map(|t| t.trim().trim_start_matches('#').to_string()).filter(|t| !t.is_empty()).collect()
    }

    /// An event's time: `2026-11-12 14:30 – 15:30`, `2026-11-12 14:30 – 2026-11-13 09:00`,
    /// `2026-11-12 (all day)`, `2026-11-12 – 2026-11-14 (all day)`.
    pub fn when(&self) -> Option<String> {
        let start = self.get("start")?;
        let all_day = self.get("all-day").is_some_and(|v| matches!(v.to_lowercase().as_str(), "true" | "yes"));
        let end = self.get("end").filter(|e| *e != start);
        Some(match (all_day, end) {
            (true, None) => format!("{} (all day)", date_part(start)),
            (true, Some(e)) if date_part(e) == date_part(start) => format!("{} (all day)", date_part(start)),
            (true, Some(e)) => format!("{} – {} (all day)", date_part(start), date_part(e)),
            (false, None) => start.to_string(),
            (false, Some(e)) if date_part(e) == date_part(start) => format!("{start} – {}", e.trim_start_matches(date_part(e)).trim_start_matches(['T', ' '])),
            (false, Some(e)) => format!("{start} – {e}"),
        })
    }

    /// The page of a Word import in one chip: `A4 · 2.5 cm margins · 11 pt` (what the file does not
    /// say is left out). The margins are `top right bottom left` in mm: four equal sides read as one
    /// number, otherwise all four are named.
    pub fn page_chip(&self) -> Option<String> {
        let mut parts = Vec::new();
        parts.extend(self.get("print-paper").map(String::from));
        if let Some(m) = self.get("print-margins") {
            let cm = |mm: &str| mm.parse::<f64>().ok().map(|v| format!("{}", (v).round() / 10.0));
            let sides: Vec<Option<String>> = m.split_whitespace().map(cm).collect();
            if sides.len() == 4 && sides.iter().all(Option::is_some) {
                let s: Vec<String> = sides.into_iter().flatten().collect();
                parts.push(if s.iter().all(|x| *x == s[0]) { format!("{} cm margins", s[0]) } else { format!("margins {} cm", s.join(" / ")) });
            }
        }
        parts.extend(self.get("print-font-size").map(|s| format!("{s} pt")));
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    /// The rows of a generic card, and of the rest of any card: every property that is not technical,
    /// not the type and not the tags, as (key, values joined by `, `), values that are empty left out.
    pub fn rows(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .filter(|(k, v)| !is_technical(k) && k != "type" && k != "tags" && k != "tag" && v.iter().any(|x| !x.trim().is_empty()))
            .map(|(k, v)| (k.clone(), v.iter().map(|x| x.trim()).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ")))
            .collect()
    }

    /// The type as written (`email`, `event`, …) or `word` for a Word import without a type.
    pub fn type_name(&self) -> Option<String> {
        self.get("type").map(str::to_lowercase).or_else(|| (self.kind == Kind::Word).then(|| "word".to_string()))
    }
}

/// The one line of a folded front matter (EDIT-060): the type first, then the most telling
/// properties of that type, the tags last as `#tag`, joined by ` · `. An e-mail gives sender and
/// date, an event its start and place, a Word import its page chip, any other note its title or
/// its first rows. Technical keys (EDIT-061) never appear. `None` when `text` has no front matter
/// or nothing to say.
pub fn summary(text: &str) -> Option<String> {
    let card = Card::parse(text)?;
    let mut parts: Vec<String> = Vec::new();
    parts.extend(card.type_name());
    match card.kind {
        Kind::Email => {
            parts.extend(card.get("from").map(|f| people_in(f).first().map_or_else(|| f.to_string(), |p| p.label().to_string())));
            parts.extend(card.get("date").map(|d| date_part(d).to_string()));
        }
        Kind::Event => {
            parts.extend(card.when());
            parts.extend(card.get("location").map(String::from));
        }
        Kind::Word => parts.extend(card.page_chip()),
        Kind::Generic => match card.get("title") {
            Some(t) => parts.push(t.to_string()),
            None => parts.extend(card.rows().into_iter().take(2).map(|(_, v)| v)),
        },
    }
    let tags = card.tags();
    if !tags.is_empty() {
        parts.push(tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIL: &str = "---\ntype: email\nsubject: \"Plan\"\nfrom: \"Anna Müller <anna@x.de>\"\nto: \"Bob <bob@x.de>, carol@y.de\"\ndate: 2026-11-12 14:30\nmessage-id: \"<abc@x.de>\"\ntags: [work, mail]\n---\n\n# Plan\n";
    const EVENT: &str = "---\ntype: event\ntitle: Standup\nstart: 2026-11-12 14:30\nend: 2026-11-12 15:00\nlocation: Room 4\ncalendar: Work\nevent-uid: \"u1\"\ntags: [work]\n---\n";
    const WORD: &str = "---\nprint-paper: A4\nprint-margins: 25 25 25 25\nprint-font: serif\nprint-font-size: 11\nsource: a.docx\ntags: [docx]\n---\n";

    // spec: EDIT-060
    #[test]
    fn a_mail_is_summed_up_as_type_sender_date_and_tags() {
        assert_eq!(summary(MAIL).as_deref(), Some("email · Anna Müller · 2026-11-12 · #work #mail"));
    }

    // spec: EDIT-060, EDIT-067
    #[test]
    fn a_quoted_display_name_of_a_sender_shows_without_escapes() {
        let text = "---\ntype: email\nfrom: \"\\\"Nils Mundhenke\\\" <nils@example.org>\"\ndate: 2026-11-12 14:30\n---\n";
        assert_eq!(summary(text).as_deref(), Some("email · Nils Mundhenke · 2026-11-12"));
    }

    // spec: EDIT-060
    #[test]
    fn an_event_is_summed_up_with_its_time_and_place() {
        assert_eq!(summary(EVENT).as_deref(), Some("event · 2026-11-12 14:30 – 15:00 · Room 4 · #work"));
    }

    // spec: EDIT-060, EDIT-062
    #[test]
    fn a_word_import_is_summed_up_by_its_page() {
        assert_eq!(summary(WORD).as_deref(), Some("word · A4 · 2.5 cm margins · 11 pt · #docx"));
    }

    // spec: EDIT-060
    #[test]
    fn another_note_shows_its_title_or_its_first_rows() {
        assert_eq!(summary("---\ntitle: Idea\nstatus: open\n---\n").as_deref(), Some("Idea"));
        assert_eq!(summary("---\nstatus: open\nowner: Ann\nmood: ok\n---\n").as_deref(), Some("open · Ann"));
        assert_eq!(summary("---\ntype: recipe\nserves: 4\ntags: [#food]\n---\n").as_deref(), Some("recipe · 4 · #food"));
    }

    // spec: EDIT-060
    #[test]
    fn no_front_matter_or_an_empty_one_has_no_summary() {
        assert_eq!(summary("# Just text\n"), None);
        assert_eq!(summary("---\n---\n\ntext"), None);
        assert_eq!(summary("---\nmessage-id: x\nsource: y\n---\n"), None, "technical keys alone say nothing");
    }

    // spec: EDIT-061
    #[test]
    fn technical_keys_are_never_in_the_summary_or_the_rows() {
        for k in ["message-id", "event-uid", "source", "print-paper", "print-heading-color", "Message_ID", "uid", "in-reply-to"] {
            assert!(is_technical(k), "{k}");
        }
        for k in ["from", "start", "title", "location", "tags"] {
            assert!(!is_technical(k), "{k}");
        }
        let s = summary(MAIL).unwrap();
        assert!(!s.contains("abc@x.de"), "{s}");
        let c = Card::parse(EVENT).unwrap();
        assert!(c.rows().iter().all(|(k, _)| k != "event-uid"));
        assert_eq!(c.values("event-uid"), ["u1"], "the model still has it for the all-properties view");
    }

    // spec: EDIT-062
    #[test]
    fn the_kind_follows_the_type_and_the_print_keys() {
        assert_eq!(Card::parse(MAIL).unwrap().kind, Kind::Email);
        assert_eq!(Card::parse(EVENT).unwrap().kind, Kind::Event);
        assert_eq!(Card::parse(WORD).unwrap().kind, Kind::Word);
        assert_eq!(Card::parse("---\nstatus: x\n---\n").unwrap().kind, Kind::Generic);
        assert_eq!(Card::parse("---\ntype: email\nprint-paper: A4\n---\n").unwrap().kind, Kind::Email, "a type wins");
        assert!(Card::parse("text").is_none());
    }

    // spec: EDIT-062
    #[test]
    fn the_page_chip_names_paper_margins_and_size() {
        assert_eq!(Card::parse(WORD).unwrap().page_chip().as_deref(), Some("A4 · 2.5 cm margins · 11 pt"));
        let c = Card::parse("---\nprint-paper: 297x210mm\nprint-margins: 20 25 20 30\n---\n").unwrap();
        assert_eq!(c.page_chip().as_deref(), Some("297x210mm · margins 2 / 2.5 / 2 / 3 cm"));
        let c = Card::parse("---\nprint-font: sans\n---\n").unwrap();
        assert_eq!(c.page_chip(), None);
        assert_eq!(Card::parse(MAIL).unwrap().page_chip(), None);
    }

    // spec: EDIT-062
    #[test]
    fn an_events_time_is_one_phrase() {
        let when = |f: &str| Card::parse(&format!("---\ntype: event\n{f}\n---\n")).unwrap().when();
        assert_eq!(when("start: 2026-11-12 14:30\nend: 2026-11-13 09:00").as_deref(), Some("2026-11-12 14:30 – 2026-11-13 09:00"));
        assert_eq!(when("start: 2026-11-12\nend: 2026-11-12\nall-day: true").as_deref(), Some("2026-11-12 (all day)"));
        assert_eq!(when("start: 2026-11-12\nend: 2026-11-14\nall-day: true").as_deref(), Some("2026-11-12 – 2026-11-14 (all day)"));
        assert_eq!(when("start: 2026-11-12 10:00").as_deref(), Some("2026-11-12 10:00"));
        assert_eq!(when("title: x"), None);
    }

    // spec: EDIT-062
    #[test]
    fn people_and_dates_are_cut_out_of_their_values() {
        let p = people_in("\"Müller, Hans\" <h@x.de>, info@y.de");
        assert_eq!(p.iter().map(Person::label).collect::<Vec<_>>(), ["Hans Müller", "Info"]);
        assert_eq!(p[0].email, "h@x.de");
        assert_eq!(date_part("2026-11-12 14:30"), "2026-11-12");
        assert_eq!(date_part("2026-11-12T14:30"), "2026-11-12");
        assert_eq!(date_part("2026-11-12"), "2026-11-12");
    }

    // spec: EDIT-062
    #[test]
    fn generic_rows_leave_out_type_tags_technical_and_empty_values() {
        let c = Card::parse("---\ntype: recipe\nserves: 4\nsource: web\nnote:\naliases: [a, b]\ntags: [x]\n---\n").unwrap();
        assert_eq!(c.rows(), vec![("serves".to_string(), "4".to_string()), ("aliases".to_string(), "a, b".to_string())]);
        assert_eq!(c.tags(), ["x"]);
    }
}
