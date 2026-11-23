//! The properties Shale keeps in a note's front matter: `created:`, `modified:` and `priority:`
//! (VAULT-045 to VAULT-047, ADR-0034).
//!
//! * [`stamp`] is what a save from Shale does to the text: `created:` once, `modified:` when the
//!   body changed.
//! * [`created`], [`modified`] and [`priority`] read them, with the fallbacks of the board sort
//!   (BOARD-016): the `YYMMDD_` name prefix (VAULT-025) and the file time.
//!
//! Times are UTC seconds; they are written as ISO 8601 with the time zone, `2026-11-23T14:05:09+01:00`.

use std::path::Path;

use crate::calendar::time::{civil_from_days, days_from_civil, days_in_month, local_offset, local_to_utc};
use crate::markdown;

/// The priority of a note, the five levels of Obsidian Tasks. The order is by importance:
/// `Lowest < Low < Medium < High < Highest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Lowest,
    Low,
    Medium,
    High,
    Highest,
}

impl Priority {
    /// The level a value names (any case); another value is none (VAULT-047).
    pub fn parse(value: &str) -> Option<Priority> {
        match value.trim().to_lowercase().as_str() {
            "highest" => Some(Priority::Highest),
            "high" => Some(Priority::High),
            "medium" => Some(Priority::Medium),
            "low" => Some(Priority::Low),
            "lowest" => Some(Priority::Lowest),
            _ => None,
        }
    }

    /// The word as written in the front matter.
    pub fn name(self) -> &'static str {
        match self {
            Priority::Highest => "highest",
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
            Priority::Lowest => "lowest",
        }
    }
}

/// The first non-empty value of a front matter property (`key` in lower case).
fn value(text: &str, key: &str) -> Option<String> {
    markdown::front_matter_values(text).into_iter().find(|(k, _)| k == key).and_then(|(_, v)| v.into_iter().map(|x| x.trim().to_string()).find(|x| !x.is_empty()))
}

/// The `priority:` of a note's text; none when it has none or the value is not one of the levels.
pub fn priority(text: &str) -> Option<Priority> {
    value(text, "priority").and_then(|v| Priority::parse(&v))
}

/// The creation time of a note: `created:`, else the `YYMMDD_` prefix of its name (midnight), else
/// the file time (UTC seconds).
pub fn created(rel: &Path, text: &str, file_time: Option<i64>) -> Option<i64> {
    value(text, "created").and_then(|v| parse_timestamp(&v)).or_else(|| prefix_time(rel)).or(file_time)
}

/// The time of the last change of a note's body: `modified:`, else the file time.
pub fn modified(text: &str, file_time: Option<i64>) -> Option<i64> {
    value(text, "modified").and_then(|v| parse_timestamp(&v)).or(file_time)
}

/// Midnight of the day a name's `YYMMDD_` prefix says (20YY), local time.
fn prefix_time(rel: &Path) -> Option<i64> {
    let name = rel.file_name()?.to_string_lossy();
    let p = crate::vault::date_prefix_of(&name)?;
    let n = |a: usize| p[a..a + 2].parse::<u32>().ok();
    let (y, m, d) = (2000 + n(0)? as i32, n(2)?, n(4)?);
    ((1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)).then(|| local_to_utc(days_from_civil(y, m, d), 0))
}

/// The time of `2026-11-23T14:05:09+01:00`; `Z`, `+0100` and `+01` zones, a space for the `T`, no
/// seconds, and a fraction are read too. Without a zone it is local time, a date alone is midnight.
pub fn parse_timestamp(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = match s.split_once(['T', ' ']) {
        Some((d, r)) => (d, Some(r.trim())),
        None => (s, None),
    };
    let mut ymd = date.splitn(3, '-');
    let y: i32 = ymd.next()?.parse().ok()?;
    let (m, d): (u32, u32) = (ymd.next()?.parse().ok()?, ymd.next()?.parse().ok()?);
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    let Some(rest) = rest else { return Some(local_to_utc(days, 0)) };
    let zone_at = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(zone_at);
    let mut hms = clock.trim().splitn(3, ':');
    let h: i64 = hms.next()?.parse().ok()?;
    let min: i64 = hms.next()?.parse().ok()?;
    let sec: i64 = match hms.next() {
        Some(x) => x.split('.').next()?.parse().ok()?,
        None => 0,
    };
    if h > 23 || min > 59 || sec > 60 {
        return None;
    }
    let sod = h * 3600 + min * 60 + sec;
    let zone = zone.trim();
    if zone.is_empty() {
        return Some(local_to_utc(days, sod));
    }
    if zone.eq_ignore_ascii_case("z") {
        return Some(days * 86_400 + sod);
    }
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let digits: String = zone[1..].chars().filter(|c| *c != ':').collect();
    if !digits.chars().all(|c| c.is_ascii_digit()) || !matches!(digits.len(), 2 | 4) {
        return None;
    }
    let (zh, zm): (i64, i64) = (digits[..2].parse().ok()?, if digits.len() == 4 { digits[2..].parse().ok()? } else { 0 });
    Some(days * 86_400 + sod - sign * (zh * 3600 + zm * 60))
}

/// `2026-11-23T14:05:09+01:00` for a UTC time, in the computer's time zone.
pub fn format_timestamp(utc: i64) -> String {
    let off = local_offset(utc);
    let wall = utc + off;
    let (y, m, d) = civil_from_days(wall.div_euclid(86_400));
    let sod = wall.rem_euclid(86_400);
    let (sign, a) = if off < 0 { ('-', -off) } else { ('+', off) };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}", sod / 3600, sod % 3600 / 60, sod % 60, a / 3600, a % 3600 / 60)
}

/// The text after the front matter.
fn body_of(text: &str) -> &str {
    let n = markdown::front_matter_lines(text);
    let at: usize = text.split_inclusive('\n').take(n).map(str::len).sum();
    &text[at..]
}

/// `text` with the property `key: val` set: the line is replaced, or added at the end of the front
/// matter, which a note without one gets (`---`, the line, `---`, at the top).
fn set_property(text: &str, key: &str, val: &str) -> String {
    let n = markdown::front_matter_lines(text);
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    if n == 0 {
        return format!("---{nl}{key}: {val}{nl}---{nl}{text}");
    }
    let mut out = String::with_capacity(text.len() + 40);
    let mut done = false;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let is_key = i > 0 && i < n - 1 && line.split_once(':').is_some_and(|(k, _)| k.trim_end().eq_ignore_ascii_case(key) && !k.starts_with(char::is_whitespace));
        if is_key && !done {
            let eol = &line[line.trim_end_matches(['\r', '\n']).len()..];
            out.push_str(&format!("{key}: {val}{eol}"));
            done = true;
        } else if i == n - 1 && !done {
            out.push_str(&format!("{key}: {val}{nl}"));
            out.push_str(line);
            done = true;
        } else {
            out.push_str(line);
        }
    }
    out
}

/// What a save from Shale writes for `text` (VAULT-045, VAULT-046): `disk` is the note as it is on
/// disk now (none for a new file), `now` the time of the save. `created:` is the one on disk, else
/// the one in `text`, else `now`; it is never changed. `modified:` is `now` when the body differs from
/// the one on disk, else it stays. The front matter alone never moves it. `None`: nothing to change,
/// also for a blank text.
pub fn stamp(text: &str, disk: Option<&str>, now: i64) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    let at = format_timestamp(now);
    let created = disk.and_then(|d| value(d, "created")).or_else(|| value(text, "created")).unwrap_or_else(|| at.clone());
    let modified = if body_of(text) != disk.map_or("", body_of) { Some(at) } else { value(text, "modified").or_else(|| disk.and_then(|d| value(d, "modified"))) };
    let mut out = set_property(text, "created", &created);
    if let Some(m) = modified {
        out = set_property(&out, "modified", &m);
    }
    (out != text).then_some(out)
}

/// For tests of other features: `text` as it was before a save added `created:` and `modified:` (a front
/// matter that holds nothing else goes again).
#[cfg(test)]
pub(crate) fn unstamped(text: &str) -> String {
    let n = markdown::front_matter_lines(text);
    if n == 0 {
        return text.to_string();
    }
    let is_stamp = |l: &str| l.split_once(':').is_some_and(|(k, _)| matches!(k.trim().to_lowercase().as_str(), "created" | "modified") && !k.starts_with(char::is_whitespace));
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let kept: Vec<&str> = lines[..n].iter().copied().enumerate().filter(|(i, l)| *i == 0 || *i == n - 1 || !is_stamp(l)).map(|(_, l)| l).collect();
    let rest: String = lines[n..].concat();
    if kept.len() == 2 {
        rest
    } else {
        kept.concat() + &rest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: VAULT-045
    #[test]
    fn the_test_helper_takes_the_stamp_off_again() {
        let text = "---\ntitle: x\ncreated: a\nmodified: b\n---\nbody\n";
        assert_eq!(unstamped(text), "---\ntitle: x\n---\nbody\n");
        assert_eq!(unstamped(&stamp("body\n", None, NOW).unwrap()), "body\n");
        assert_eq!(unstamped("plain\n"), "plain\n");
    }

    const NOW: i64 = 1_795_000_000;

    fn at(secs: i64) -> String {
        format_timestamp(secs)
    }

    // spec: VAULT-047
    #[test]
    fn the_five_levels_parse_in_any_case_and_are_ordered_by_importance() {
        for (word, p) in [("highest", Priority::Highest), ("High", Priority::High), (" MEDIUM ", Priority::Medium), ("low", Priority::Low), ("Lowest", Priority::Lowest)] {
            assert_eq!(Priority::parse(word), Some(p));
            assert_eq!(Priority::parse(p.name()), Some(p), "the name reads back");
        }
        assert!(Priority::Lowest < Priority::Low && Priority::Low < Priority::Medium && Priority::Medium < Priority::High && Priority::High < Priority::Highest);
        assert_eq!(Priority::parse("urgent"), None);
        assert_eq!(Priority::parse(""), None);
    }

    // spec: VAULT-047
    #[test]
    fn a_priority_is_read_from_the_front_matter_and_an_unknown_value_is_none() {
        assert_eq!(priority("---\npriority: high\n---\nbody"), Some(Priority::High));
        assert_eq!(priority("---\nPriority: \"Lowest\"\n---\n"), Some(Priority::Lowest));
        assert_eq!(priority("---\npriority: urgent\n---\n"), None);
        assert_eq!(priority("---\ntitle: x\n---\n"), None);
        assert_eq!(priority("priority: high\nno front matter"), None);
    }

    // spec: VAULT-045
    #[test]
    fn timestamps_round_trip_and_read_the_zones_of_iso_8601() {
        assert_eq!(parse_timestamp(&at(NOW)), Some(NOW));
        assert_eq!(parse_timestamp("2026-11-23T14:05:09Z"), Some(days_from_civil(2026, 11, 23) * 86_400 + 14 * 3600 + 5 * 60 + 9));
        let utc = days_from_civil(2026, 11, 23) * 86_400 + 13 * 3600;
        assert_eq!(parse_timestamp("2026-11-23T14:00:00+01:00"), Some(utc));
        assert_eq!(parse_timestamp("2026-11-23 14:00+0100"), Some(utc));
        assert_eq!(parse_timestamp("2026-11-23T14:00:00.250+01"), Some(utc));
        assert_eq!(parse_timestamp("2026-11-23T08:00:00-05:00"), Some(utc));
        assert_eq!(parse_timestamp("2026-11-23T14:00"), Some(local_to_utc(days_from_civil(2026, 11, 23), 14 * 3600)));
        assert_eq!(parse_timestamp("2026-11-23"), Some(local_to_utc(days_from_civil(2026, 11, 23), 0)));
        for bad in ["", "soon", "2026-13-01", "2026-02-30", "2026-11-23T25:00", "2026-11-23T14:00+9", "2026-11-23T14"] {
            assert_eq!(parse_timestamp(bad), None, "{bad}");
        }
        let shown = at(NOW);
        assert!(shown.len() == 25 && shown.as_bytes()[10] == b'T' && (shown.as_bytes()[19] == b'+' || shown.as_bytes()[19] == b'-'), "{shown}");
    }

    // spec: VAULT-045, VAULT-046
    #[test]
    fn readers_fall_back_to_the_name_prefix_and_the_file_time() {
        let with = format!("---\ncreated: {}\nmodified: {}\n---\n", at(NOW), at(NOW + 60));
        let rel = Path::new("261117_Plan.md");
        assert_eq!(created(rel, &with, Some(5)), Some(NOW));
        assert_eq!(modified(&with, Some(5)), Some(NOW + 60));
        let prefix = Some(local_to_utc(days_from_civil(2026, 11, 17), 0));
        assert_eq!(created(rel, "body", Some(5)), prefix, "the prefix before the file time");
        assert_eq!(created(Path::new("dir/Plan.md"), "body", Some(5)), Some(5), "no prefix: the file time");
        assert_eq!(created(Path::new("261399_Plan.md"), "body", Some(5)), Some(5), "an impossible date is no prefix");
        assert_eq!(created(Path::new("Plan.md"), "body", None), None);
        assert_eq!(created(rel, "---\ncreated: soon\n---\n", Some(5)), prefix, "an unreadable value falls back");
        assert_eq!(modified("body", Some(5)), Some(5));
        assert_eq!(modified("body", None), None);
    }

    // spec: VAULT-045, VAULT-046
    #[test]
    fn the_first_save_of_a_note_writes_created_and_modified_at_the_top() {
        let out = stamp("# Plan\nbody\n", None, NOW).unwrap();
        let t = at(NOW);
        assert_eq!(out, format!("---\ncreated: {t}\nmodified: {t}\n---\n# Plan\nbody\n"));
        assert_eq!(body_of(&out), "# Plan\nbody\n", "the body is not touched");
        assert_eq!(stamp("", None, NOW), None, "a blank note stays blank");
        assert_eq!(stamp("  \n", None, NOW), None);
    }

    // spec: VAULT-045
    #[test]
    fn a_later_save_keeps_created_even_when_the_buffer_has_none_or_another() {
        let first = stamp("one\n", None, NOW).unwrap();
        let created_first = value(&first, "created").unwrap();
        // the buffer is still the text without the properties, the disk has them
        let second = stamp("one\ntwo\n", Some(&first), NOW + 500).unwrap();
        assert_eq!(value(&second, "created").unwrap(), created_first);
        // the buffer has another created: the disk's wins
        let edited = "---\ncreated: 2001-01-01T00:00:00Z\n---\none\ntwo\nthree\n";
        let third = stamp(edited, Some(&second), NOW + 900).unwrap();
        assert_eq!(value(&third, "created").unwrap(), created_first, "never changed afterwards");
    }

    // spec: VAULT-046
    #[test]
    fn a_body_change_sets_modified_and_a_front_matter_change_does_not() {
        let first = stamp("one\n", None, NOW).unwrap();
        let t0 = value(&first, "modified").unwrap();
        // body change
        let body = stamp("one\ntwo\n", Some(&first), NOW + 3600).unwrap();
        assert_eq!(value(&body, "modified").unwrap(), at(NOW + 3600));
        assert_ne!(t0, at(NOW + 3600));
        // front matter only, the buffer already holds the properties: nothing to change
        let fm = format!("---\ncreated: {}\nmodified: {t0}\npriority: high\ndrop: done\n---\none\n", at(NOW));
        assert_eq!(stamp(&fm, Some(&first), NOW + 7200), None);
        // front matter only, buffer without the properties: modified comes back from the disk
        let out = stamp("---\npriority: low\n---\none\n", Some(&first), NOW + 7200).unwrap();
        assert_eq!(value(&out, "modified").unwrap(), t0);
        assert_eq!(value(&out, "priority").unwrap(), "low");
        // no change at all: the disk text is what is written
        assert_eq!(stamp("one\n", Some(&first), NOW + 9000).as_deref(), Some(first.as_str()));
    }

    // spec: VAULT-045, VAULT-046
    #[test]
    fn existing_front_matter_keeps_its_lines_and_line_ends() {
        let t = at(NOW);
        let out = stamp("---\ntype: email\ntags: [a]\n---\n\nHello\n", None, NOW).unwrap();
        assert_eq!(out, format!("---\ntype: email\ntags: [a]\ncreated: {t}\nmodified: {t}\n---\n\nHello\n"));
        let out = stamp("---\r\ntitle: x\r\n---\r\nHello\r\n", None, NOW).unwrap();
        assert_eq!(out, format!("---\r\ntitle: x\r\ncreated: {t}\r\nmodified: {t}\r\n---\r\nHello\r\n"));
        let out = stamp("no fm\r\nline\r\n", None, NOW).unwrap();
        assert!(out.starts_with("---\r\ncreated: "), "{out:?}");
        // an old modified is replaced in its place
        let old = "---\nmodified: 2020-01-01T00:00:00Z\ntitle: x\ncreated: 2020-01-01T00:00:00Z\n---\nold\n";
        let out = stamp(&old.replace("old", "new"), Some(old), NOW).unwrap();
        assert_eq!(out, format!("---\nmodified: {t}\ntitle: x\ncreated: 2020-01-01T00:00:00Z\n---\nnew\n"));
        // a front matter that is not closed is no front matter
        let out = stamp("---\nnot closed\ntext\n", None, NOW).unwrap();
        assert!(out.ends_with("---\n---\nnot closed\ntext\n"), "{out:?}");
    }
}
