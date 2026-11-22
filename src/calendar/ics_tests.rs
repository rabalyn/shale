//! Property tests of the iCalendar reader and writer (CAL-006, CAL-012, CAL-013, CAL-033,
//! DEV-042): what `to_ics` writes is read back as it was, every line is folded to 75 octets, the
//! repeat rules the dialog makes are read back, the occurrences of an event are in their window,
//! and no text makes the reader panic. The example tests are in `ics.rs`.
//!
//! The times are drawn from the months January, February, June and July (days 1 to 15), where
//! none of the zones used changes its clock: a wall clock time that happens twice or not at all
//! has no round trip, and that is a property of the zone, not of the file.

use proptest::prelude::*;

use crate::calendar::ics::{occurrences, parse_calendar, parse_ics, parse_repeat, repeat_rule, to_ics, to_ics_with, Alarm, Event, Freq, Repeat, RepeatEnd};
use crate::calendar::time::days_from_civil;
use crate::property_test_support::{config, config_with};

/// How the times of a generated event are written.
#[derive(Clone, Copy, Debug)]
enum Kind {
    AllDay,
    Utc,
    Zoned(&'static str),
    Floating,
}

const DAY: i64 = 86_400;

/// Text of the awkward kind: commas, semicolons, backslashes, quotes, colons, new lines, a literal
/// backslash and `n`, non-ASCII of one, three and four octets; sometimes long enough to be folded.
fn text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        6 => "[a-zA-Z0-9 ]{1,6}",
        2 => Just(",".to_string()),
        2 => Just(";".to_string()),
        2 => Just("\\".to_string()),
        2 => Just("\n".to_string()),
        1 => Just("\r".to_string()),
        1 => Just("\\n".to_string()),
        1 => Just(":".to_string()),
        1 => Just("\"".to_string()),
        1 => Just("\t".to_string()),
        2 => Just("é".to_string()),
        1 => Just("日本".to_string()),
        1 => Just("😀".to_string()),
    ];
    prop_oneof![8 => prop::collection::vec(piece.clone(), 0..8), 1 => prop::collection::vec(piece, 40..160)].prop_map(|p| p.concat())
}

/// A person as the reader gives it back: `Name <mail>`, a mail address or a name.
fn person() -> impl Strategy<Value = String> {
    let name = "[A-Za-zé]([A-Za-zé ,]{0,8}[A-Za-zé])?";
    let mail = "[a-z]{1,6}@[a-z]{1,6}\\.de";
    prop_oneof![Just(String::new()), (name, mail).prop_map(|(n, m)| format!("{n} <{m}>")), mail.prop_map(|m| m.to_string()), name.prop_map(|n| n.to_string())]
}

/// The kind, start and end of an event's times.
fn when() -> impl Strategy<Value = (Kind, i64, i64)> {
    let kind = prop_oneof![Just(Kind::AllDay), Just(Kind::Utc), Just(Kind::Zoned("Europe/Berlin")), Just(Kind::Zoned("America/New_York")), Just(Kind::Zoned("Asia/Tokyo")), Just(Kind::Zoned("Australia/Sydney")), Just(Kind::Floating)];
    (kind, 2000i32..2040, prop::sample::select(vec![1u32, 2, 6, 7]), 1u32..16, 0i64..DAY, 1i64..(3 * DAY), 1i64..4).prop_map(|(kind, y, m, d, secs, len, days)| {
        let day = days_from_civil(y, m, d) * DAY;
        match kind {
            Kind::AllDay => (kind, day, day + days * DAY),
            _ => (kind, day + secs, day + secs + len),
        }
    })
}

/// A repeat rule of the kind the event dialog makes (`end` is a day number near 2000 to 2041).
fn repeat() -> impl Strategy<Value = Repeat> {
    let freq = prop::sample::select(Freq::ALL.to_vec());
    let end = prop_oneof![Just(RepeatEnd::Never), (1u32..1000).prop_map(RepeatEnd::Count), (days_from_civil(2000, 1, 1)..days_from_civil(2042, 1, 1)).prop_map(RepeatEnd::Until)];
    (freq, 1u32..12, prop::array::uniform7(any::<bool>()), end).prop_map(|(freq, interval, days, end)| Repeat { freq, interval, days: if freq == Freq::Weekly { days } else { [false; 7] }, end })
}

/// An event with every field Shale writes: `src`, `zones`, `href`, `etag` and the read-only `rdates`
/// are left empty or set only when `with_rdates`.
fn event(with_rdates: bool) -> impl Strategy<Value = Event> {
    let texts =
        (text(), text(), text(), "(https://example\\.com/[a-z]{0,8})?", prop::sample::select(vec!["", "CONFIRMED", "TENTATIVE", "CANCELLED"]), person(), prop::collection::vec(person().prop_filter("a person", |p| !p.is_empty()), 0..3));
    let extras = ("[a-z0-9]{1,8}@shale", prop::collection::vec(text().prop_map(|c| c.trim().to_string()).prop_filter("not empty", |c| !c.is_empty()), 0..3), any::<bool>(), prop::collection::vec(-3 * DAY..3 * DAY, 0..3));
    let series = (prop::option::of(repeat()), prop::collection::vec(1i64..10, 0..3), prop::option::of(1i64..10), prop::collection::vec(-3i64..30, 0..3));
    (when(), texts, extras, series).prop_map(move |((kind, start, end), (summary, description, location, url, status, organizer, attendees), (uid, categories, transparent, alarms), (rep, ex, rid, rd))| {
        let mut e = Event {
            uid,
            summary,
            description,
            location,
            url,
            organizer,
            attendees,
            status: status.to_string(),
            start,
            end,
            all_day: matches!(kind, Kind::AllDay),
            tzid: if let Kind::Zoned(z) = kind { Some(z.to_string()) } else { None },
            floating: matches!(kind, Kind::Floating),
            categories,
            transparent,
            alarms: alarms.into_iter().map(Alarm::before_start).collect(),
            exdates: ex.into_iter().map(|k| start + k * DAY).collect(),
            recurrence_id: rid.map(|k| start + k * DAY),
            rdates: if with_rdates { rd.into_iter().map(|k| start + k * DAY).collect() } else { vec![] },
            ..Default::default()
        };
        e.rrule = rep.map(|r| repeat_rule(&r, &e));
        e
    })
}

/// What a read event has beyond what was written: its source lines, zone definitions and the
/// lines of its reminders (CAL-012: kept for writing back, not part of the event's content).
fn content(mut e: Event) -> Event {
    e.src.clear();
    e.zones.clear();
    for a in &mut e.alarms {
        a.raw.clear();
    }
    e
}

proptest! {
    #![proptest_config(config())]

    // spec: CAL-012
    // Compared: every field Shale writes (the content of the event). Not meant to survive: `rdates`
    // (only kept in the lines an event was read from, so an event made here does not write them),
    // `href` and `etag` (where the event lives on a server) and the source lines and zone
    // definitions the reader adds.
    #[test]
    fn cal_012_what_to_ics_writes_parse_ics_reads_back(events in prop::collection::vec(event(false), 0..4)) {
        let events: Vec<Event> = events.into_iter().enumerate().map(|(i, e)| Event { uid: format!("{i}-{}", e.uid), ..e }).collect();
        let read: Vec<Event> = parse_ics(&to_ics("Name, é;\\", &events)).into_iter().map(content).collect();
        prop_assert_eq!(read, events);
    }

    // spec: CAL-012
    // Written again, what was read is the same event (the second write keeps it line for line).
    #[test]
    fn cal_012_a_file_written_from_what_was_read_reads_the_same(events in prop::collection::vec(event(false), 0..3)) {
        let once = parse_calendar(&to_ics("x", &events));
        let twice = parse_calendar(&to_ics_with("x", &once.events, &once.other));
        prop_assert_eq!(once.events.into_iter().map(content).collect::<Vec<_>>(), twice.events.into_iter().map(content).collect::<Vec<_>>());
    }

    // spec: CAL-033
    #[test]
    fn cal_033_every_written_line_is_at_most_75_octets_and_no_character_is_split(events in prop::collection::vec(event(false), 1..3)) {
        let out = to_ics("Name é 日本 😀", &events);
        prop_assert!(out.ends_with("\r\n"));
        for line in out.split("\r\n") {
            prop_assert!(line.len() <= 75, "{} octets: {:?}", line.len(), line);
        }
        // a fold is a line break and one space: the lines that continue another are not empty
        prop_assert!(out.split("\r\n").all(|l| l != " "));
        // nothing is split in the middle of a character: unfolded, the text of every field is whole
        let unfolded = out.replace("\r\n ", "");
        for e in &events {
            let summary = e.summary.replace('\\', "\\\\").replace(';', r"\;").replace(',', "\\,").replace('\n', "\\n");
            prop_assert!(unfolded.contains(&format!("SUMMARY:{summary}\r\n")), "{:?}", e.summary);
        }
    }

    // spec: CAL-013
    #[test]
    fn cal_013_a_repeat_rule_the_dialog_makes_is_read_back_and_written_the_same((e, r) in (event(false), repeat())) {
        let rule = repeat_rule(&r, &e);
        let e = Event { rrule: Some(rule.clone()), ..e };
        let back = parse_repeat(&e);
        prop_assert_eq!(back.as_ref(), Some(&r), "{}", rule);
        prop_assert_eq!(repeat_rule(&back.unwrap(), &e), rule);
    }
}

proptest! {
    #![proptest_config(config_with(256))]

    // spec: CAL-006
    // The rule of the code: an occurrence is in the window [from, to) when it starts before `to`
    // and its end (at least a second after the start) is after `from`.
    #[test]
    fn cal_006_occurrences_lie_in_the_window_sorted_and_without_duplicates(e in event(true), from in -3 * DAY..60 * DAY, span in 1i64..(90 * DAY)) {
        let (from, to) = (e.start + from, e.start + from + span);
        let occ = occurrences(&e, from, to);
        for o in &occ {
            prop_assert!(o.start < to && o.start + (o.end - o.start).max(1) > from, "{:?} outside [{}, {})", o, from, to);
            prop_assert_eq!(o.end - o.start, (e.end - e.start).max(0));
        }
        prop_assert!(occ.windows(2).all(|w| w[0].start < w[1].start), "not sorted without duplicates: {:?}", occ);
        prop_assert!(occ.len() <= 2000 + e.rdates.len());
    }

    // spec: CAL-006
    #[test]
    fn cal_006_a_window_split_in_two_gives_the_same_occurrences(e in event(true), from in -3 * DAY..60 * DAY, span in 2i64..(90 * DAY), cut in 1i64..1000) {
        let (from, to) = (e.start + from, e.start + from + span);
        let mid = from + span * cut / 1000;
        let mut both: Vec<i64> = occurrences(&e, from, mid).into_iter().chain(occurrences(&e, mid, to)).map(|o| o.start).collect();
        both.sort();
        both.dedup();
        let whole: Vec<i64> = occurrences(&e, from, to).into_iter().map(|o| o.start).collect();
        prop_assert_eq!(both, whole);
    }

    // spec: CAL-006
    #[test]
    fn cal_006_a_rule_with_a_count_makes_at_most_that_many(e in event(false), n in 1u32..40) {
        let e = Event { rrule: Some(format!("FREQ=DAILY;COUNT={n}")), exdates: vec![], ..e };
        let all = occurrences(&e, e.start - DAY, e.start + 100 * DAY);
        prop_assert!(all.len() <= n as usize, "{} of {}", all.len(), n);
    }
}

/// Lines of an iCalendar file, whole, cut and wrong.
fn ics_like() -> impl Strategy<Value = String> {
    let line = prop_oneof![
        Just("BEGIN:VCALENDAR".to_string()),
        Just("END:VCALENDAR".to_string()),
        Just("BEGIN:VEVENT".to_string()),
        Just("END:VEVENT".to_string()),
        Just("BEGIN:VALARM".to_string()),
        Just("END:VALARM".to_string()),
        Just("BEGIN:VTIMEZONE".to_string()),
        Just("END:VTIMEZONE".to_string()),
        Just("DTSTART:20261112T130000Z".to_string()),
        Just("DTSTART;TZID=Europe/Berlin:20260105T090000".to_string()),
        Just("DTSTART;VALUE=DATE:20261120".to_string()),
        Just("RRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6".to_string()),
        Just("TRIGGER:-PT15M".to_string()),
        "DTSTART(;[A-Z]{3,5}=[A-Za-z/\"]{0,8})?:[0-9TZ+-]{0,17}",
        "DTSTART:2026[0-9]{4}T[0-9é]{3,6}Z?",
        "DURATION:P[0-9]{10,20}[WDHMS]",
        "(DTEND|EXDATE|RDATE|RECURRENCE-ID)(;[A-Z]{3,5}=[A-Za-z/\"]{0,8})?:[0-9TZ+,/-]{0,40}",
        "(DURATION|TRIGGER(;RELATED=END)?):[-+PTWDHMS0-9]{0,24}",
        "RRULE:[A-Z]{3,5}=[A-Z0-9,+-]{0,10}(;[A-Z]{3,5}=[A-Z0-9,+-]{0,12}){0,3}",
        "[A-Z-]{1,12}([;:=][ -~]{0,20}){0,3}",
        "\\PC{0,30}",
    ];
    let sep = prop_oneof![Just("\r\n"), Just("\n"), Just("\r\n "), Just("\n\t"), Just("\r")];
    let lines = |n: std::ops::Range<usize>| prop::collection::vec((line.clone(), sep.clone()), n).prop_map(|v| v.into_iter().map(|(l, s)| l + s).collect::<String>());
    // most of the time the lines are inside an event, where they are read
    prop_oneof![3 => lines(0..14).prop_map(|body| format!("BEGIN:VEVENT\r\n{body}END:VEVENT\r\n")), 1 => lines(0..30)]
}

proptest! {
    #![proptest_config(config())]

    // spec: CAL-006
    #[test]
    fn cal_006_the_reader_never_panics_on_any_text(t in prop_oneof![ics_like(), "\\PC{0,300}".prop_map(String::from)]) {
        let cal = parse_calendar(&t);
        prop_assert_eq!(parse_ics(&t), cal.events.clone());
        for e in &cal.events {
            let _ = parse_repeat(e);
            let _ = occurrences(e, e.start.saturating_sub(DAY), e.start.saturating_add(400 * DAY));
        }
        // what was read can be written
        let _ = to_ics_with("x", &cal.events, &cal.other);
    }
}

// spec: CAL-006
#[test]
fn cal_006_a_time_with_a_non_ascii_digit_or_a_duration_too_big_is_ignored_not_a_panic() {
    // found by the property above: the reader cut a time at byte 2 of `1é0…`, and multiplied a huge number of weeks
    let e = parse_ics("BEGIN:VEVENT\nUID:a\nDTSTART:20261112T1é000Z\nEND:VEVENT").remove(0);
    assert_eq!(e.start, 0, "a time that is no time is not read");
    let e = parse_ics("BEGIN:VEVENT\nUID:a\nDTSTART:20261112T130000Z\nDURATION:P99999999999999999W\nEND:VEVENT").remove(0);
    assert_eq!(e.end, e.start, "a duration that is too big is no duration");
}
