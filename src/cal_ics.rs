//! iCalendar (RFC 5545) events: reading and writing VEVENTs, and expanding repeating events.
//!
//! Supported: line folding, text escapes, DTSTART/DTEND/DURATION in UTC, with TZID (system
//! zoneinfo) or floating (local); all-day dates; SUMMARY, LOCATION, DESCRIPTION, URL, ORGANIZER,
//! ATTENDEE, STATUS, UID; RRULE with FREQ DAILY/WEEKLY/MONTHLY/YEARLY, INTERVAL, COUNT, UNTIL,
//! BYDAY (weekly); EXDATE; overridden occurrences (RECURRENCE-ID).

use crate::cal_time::{self as t, civil_from_days, days_from_civil};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub url: String,
    pub organizer: String,
    pub attendees: Vec<String>,
    pub status: String,
    /// Timed events: UTC seconds. All-day events: local day number × 86400.
    pub start: i64,
    /// Exclusive end, same units as `start`.
    pub end: i64,
    pub all_day: bool,
    pub rrule: Option<String>,
    /// Excluded occurrence starts (same units as `start`).
    pub exdates: Vec<i64>,
    /// Set on an override of one occurrence of a repeating event.
    pub recurrence_id: Option<i64>,
    /// CalDAV: where the event lives on the server (absolute URL), and its version tag.
    pub href: Option<String>,
    pub etag: Option<String>,
}

#[cfg(test)]
impl Event {
    pub fn start_day(&self) -> i64 {
        if self.all_day { self.start.div_euclid(86_400) } else { t::utc_to_local(self.start).0 }
    }
    /// Last day the event touches (inclusive).
    pub fn end_day(&self) -> i64 {
        if self.all_day {
            ((self.end - 1).div_euclid(86_400)).max(self.start_day())
        } else {
            t::utc_to_local((self.end - 1).max(self.start)).0
        }
    }
}

/// One occurrence of an event (repeats expanded).
#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    pub start: i64,
    pub end: i64,
}

fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

/// "NAME;PARAM=x;P2=y:value" → (name, params, value)
fn split_line(line: &str) -> (String, Vec<(String, String)>, String) {
    // the value starts at the first ':' that is not inside a quoted parameter value
    let mut in_q = false;
    let mut colon = line.len();
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_q = !in_q,
            ':' if !in_q => {
                colon = i;
                break;
            }
            _ => {}
        }
    }
    let head = &line[..colon];
    let value = line.get(colon + 1..).unwrap_or("").to_string();
    let mut parts = head.split(';');
    let name = parts.next().unwrap_or("").to_ascii_uppercase();
    let params = parts.filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))).collect();
    (name, params, value)
}

/// Parse a DATE or DATE-TIME value. Returns (value, is_all_day).
fn parse_dt(value: &str, params: &[(String, String)]) -> Option<(i64, bool)> {
    let v = value.trim();
    let date_only = params.iter().any(|(k, val)| k == "VALUE" && val.eq_ignore_ascii_case("DATE")) || (v.len() == 8 && !v.contains('T'));
    let num = |s: &str| s.parse::<i64>().ok();
    let y = num(v.get(0..4)?)? as i32;
    let m = num(v.get(4..6)?)? as u32;
    let d = num(v.get(6..8)?)? as u32;
    let days = days_from_civil(y, m, d);
    if date_only {
        return Some((days * 86_400, true));
    }
    let tpart = v.get(9..15)?;
    let secs = num(&tpart[0..2])? * 3600 + num(&tpart[2..4])? * 60 + num(&tpart[4..6])?;
    if v.ends_with('Z') {
        return Some((days * 86_400 + secs, false));
    }
    match params.iter().find(|(k, _)| k == "TZID") {
        Some((_, tz)) => Some((t::zoned_to_utc(tz, days, secs), false)),
        None => Some((t::local_to_utc(days, secs), false)),
    }
}

/// "P1DT2H30M" / "PT45M" / "P2W" → seconds
fn parse_duration(v: &str) -> Option<i64> {
    let neg = v.starts_with('-');
    let v = v.trim_start_matches(['+', '-']).strip_prefix('P')?;
    let mut total = 0i64;
    let mut num = String::new();
    let mut in_time = false;
    for c in v.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += n * match (c, in_time) {
                    ('W', _) => 604_800,
                    ('D', _) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    Some(if neg { -total } else { total })
}

fn cn_or_mail(params: &[(String, String)], value: &str) -> String {
    let mail = value.trim_start_matches("mailto:").trim_start_matches("MAILTO:");
    match params.iter().find(|(k, _)| k == "CN") {
        Some((_, cn)) if !mail.is_empty() => format!("{cn} <{mail}>"),
        Some((_, cn)) => cn.clone(),
        None => mail.to_string(),
    }
}

/// "Name <mail>" (as read) back to `ORGANIZER;CN="Name":mailto:mail`, keeping the real address.
fn person_line(prop: &str, who: &str) -> String {
    let who = who.trim();
    let (name, mail) = match (who.rfind('<'), who.ends_with('>')) {
        (Some(p), true) => (who[..p].trim(), who[p + 1..who.len() - 1].trim()),
        _ if who.contains('@') && !who.contains(' ') => ("", who),
        _ => (who, ""),
    };
    // the reader splits parameters at ';' and the value at the first ':'
    let name = name.trim_matches('"').replace('"', "'").replace([';', ':'], " ");
    let cn = if name.is_empty() { String::new() } else if name.contains(',') { format!(";CN=\"{name}\"") } else { format!(";CN={name}") };
    if mail.is_empty() {
        format!("{prop}{cn}:mailto:")
    } else {
        format!("{prop}{cn}:mailto:{mail}")
    }
}

/// All VEVENTs of an iCalendar text.
pub fn parse_ics(text: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut cur: Option<Event> = None;
    let mut duration: Option<i64> = None;
    let mut depth_other = 0; // inside VALARM etc.
    for line in unfold(text) {
        let (name, params, value) = split_line(&line);
        match (name.as_str(), value.to_ascii_uppercase().as_str()) {
            ("BEGIN", "VEVENT") => {
                cur = Some(Event::default());
                duration = None;
                continue;
            }
            ("END", "VEVENT") => {
                if let Some(mut e) = cur.take() {
                    if e.end <= e.start {
                        e.end = match duration {
                            Some(d) => e.start + d,
                            None if e.all_day => e.start + 86_400,
                            None => e.start,
                        };
                    }
                    if e.uid.is_empty() {
                        e.uid = format!("{}-{}", e.start, e.summary);
                    }
                    out.push(e);
                }
                continue;
            }
            ("BEGIN", _) if cur.is_some() => {
                depth_other += 1;
                continue;
            }
            ("END", _) if cur.is_some() && depth_other > 0 => {
                depth_other -= 1;
                continue;
            }
            _ => {}
        }
        let Some(e) = cur.as_mut() else { continue };
        if depth_other > 0 {
            continue;
        }
        match name.as_str() {
            "UID" => e.uid = value,
            "SUMMARY" => e.summary = unescape(&value),
            "DESCRIPTION" => e.description = unescape(&value),
            "LOCATION" => e.location = unescape(&value),
            "URL" => e.url = value,
            "STATUS" => e.status = value,
            "ORGANIZER" => e.organizer = cn_or_mail(&params, &value),
            "ATTENDEE" => e.attendees.push(cn_or_mail(&params, &value)),
            "DTSTART" => {
                if let Some((v, ad)) = parse_dt(&value, &params) {
                    e.start = v;
                    e.all_day = ad;
                }
            }
            "DTEND" => {
                if let Some((v, _)) = parse_dt(&value, &params) {
                    e.end = v;
                }
            }
            "DURATION" => duration = parse_duration(&value),
            "RRULE" => e.rrule = Some(value),
            "EXDATE" => {
                for v in value.split(',') {
                    if let Some((d, _)) = parse_dt(v, &params) {
                        e.exdates.push(d);
                    }
                }
            }
            "RECURRENCE-ID" => e.recurrence_id = parse_dt(&value, &params).map(|x| x.0),
            _ => {}
        }
    }
    out
}

fn rule_parts(rule: &str) -> Vec<(String, String)> {
    rule.split(';').filter_map(|p| p.split_once('=')).map(|(k, v)| (k.to_ascii_uppercase(), v.to_string())).collect()
}

/// Occurrences of `e` that overlap [from, to) (in `e`'s units). Non-repeating events give at
/// most one. At most 2000 occurrences are generated per event.
pub fn occurrences(e: &Event, from: i64, to: i64) -> Vec<Occurrence> {
    let len = (e.end - e.start).max(0);
    let overlaps = |s: i64| s < to && s + len.max(1) > from;
    let Some(rule) = &e.rrule else {
        return if overlaps(e.start) { vec![Occurrence { start: e.start, end: e.end }] } else { vec![] };
    };
    let parts = rule_parts(rule);
    let get = |k: &str| parts.iter().find(|p| p.0 == k).map(|p| p.1.clone());
    let freq = get("FREQ").unwrap_or_default().to_ascii_uppercase();
    let interval = get("INTERVAL").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let count = get("COUNT").and_then(|v| v.parse::<usize>().ok());
    let until = get("UNTIL").and_then(|v| parse_dt(&v, &[]).map(|x| if e.all_day { x.0.div_euclid(86_400) * 86_400 + 86_399 } else { x.0 }));
    let byday: Vec<u32> = get("BYDAY")
        .map(|v| {
            v.split(',')
                .filter_map(|d| match d.trim_start_matches(|c: char| c == '+' || c == '-' || c.is_ascii_digit()) {
                    "MO" => Some(0),
                    "TU" => Some(1),
                    "WE" => Some(2),
                    "TH" => Some(3),
                    "FR" => Some(4),
                    "SA" => Some(5),
                    "SU" => Some(6),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    // work in local wall time so repeats keep their clock time over summer-time changes
    let (base_day, base_secs) = if e.all_day { (e.start.div_euclid(86_400), 0) } else { t::utc_to_local(e.start) };
    let to_units = |day: i64| if e.all_day { day * 86_400 } else { t::local_to_utc(day, base_secs) };
    let (by, bm, bd) = civil_from_days(base_day);
    let mut out = Vec::new();
    let mut made = 0usize;
    let mut push = |day: i64, out: &mut Vec<Occurrence>| -> bool {
        if day < base_day {
            return true;
        }
        let s = to_units(day);
        if let Some(u) = until {
            if s > u {
                return false;
            }
        }
        made += 1;
        if let Some(c) = count {
            if made > c {
                return false;
            }
        }
        if s >= to && count.is_none() {
            return false;
        }
        if overlaps(s) && !e.exdates.contains(&s) {
            out.push(Occurrence { start: s, end: s + len });
        }
        made < 2000
    };
    let mut k: i64 = 0;
    loop {
        let go_on = match freq.as_str() {
            "DAILY" => push(base_day + k * interval, &mut out),
            "WEEKLY" => {
                let week_start = base_day - t::weekday(base_day) as i64 + k * 7 * interval;
                let days: Vec<u32> = if byday.is_empty() { vec![t::weekday(base_day)] } else { let mut d = byday.clone(); d.sort(); d };
                let mut ok = true;
                for wd in days {
                    if !push(week_start + wd as i64, &mut out) {
                        ok = false;
                        break;
                    }
                }
                ok
            }
            "MONTHLY" => {
                let (y, m) = t::add_months(by, bm, (k * interval) as i32);
                if bd <= t::days_in_month(y, m) { push(days_from_civil(y, m, bd), &mut out) } else { true }
            }
            "YEARLY" => {
                let y = by + (k * interval) as i32;
                if bd <= t::days_in_month(y, bm) { push(days_from_civil(y, bm, bd), &mut out) } else { true }
            }
            _ => {
                if overlaps(e.start) {
                    out.push(Occurrence { start: e.start, end: e.end });
                }
                false
            }
        };
        k += 1;
        if !go_on || k > 5000 {
            break;
        }
    }
    out
}

fn fmt_dt(v: i64, all_day: bool) -> String {
    if all_day {
        let (y, m, d) = civil_from_days(v.div_euclid(86_400));
        format!(";VALUE=DATE:{y:04}{m:02}{d:02}")
    } else {
        let (y, m, d) = civil_from_days(v.div_euclid(86_400));
        let s = v.rem_euclid(86_400);
        format!(":{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", s / 3600, s % 3600 / 60, s % 60)
    }
}

fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut n = 0;
    for c in line.chars() {
        if n + c.len_utf8() > 74 {
            out.push_str("\r\n ");
            n = 1;
        }
        out.push(c);
        n += c.len_utf8();
    }
    out
}

/// A complete iCalendar file with these events.
pub fn to_ics(name: &str, events: &[Event]) -> String {
    let mut s = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Shale//Calendar//EN\r\n");
    s.push_str(&fold(&format!("X-WR-CALNAME:{}", escape(name))));
    s.push_str("\r\n");
    let stamp = fmt_dt(t::now(), false);
    for e in events {
        let mut lines = vec![
            "BEGIN:VEVENT".to_string(),
            format!("UID:{}", e.uid),
            format!("DTSTAMP{stamp}"),
            format!("DTSTART{}", fmt_dt(e.start, e.all_day)),
            format!("DTEND{}", fmt_dt(e.end, e.all_day)),
            format!("SUMMARY:{}", escape(&e.summary)),
        ];
        if !e.location.is_empty() {
            lines.push(format!("LOCATION:{}", escape(&e.location)));
        }
        if !e.description.is_empty() {
            lines.push(format!("DESCRIPTION:{}", escape(&e.description)));
        }
        if !e.url.is_empty() {
            lines.push(format!("URL:{}", e.url));
        }
        if !e.organizer.is_empty() {
            lines.push(person_line("ORGANIZER", &e.organizer));
        }
        for a in &e.attendees {
            lines.push(person_line("ATTENDEE", a));
        }
        if let Some(r) = &e.rrule {
            lines.push(format!("RRULE:{r}"));
        }
        for x in &e.exdates {
            lines.push(format!("EXDATE{}", fmt_dt(*x, e.all_day)));
        }
        lines.push("END:VEVENT".into());
        for l in lines {
            s.push_str(&fold(&l));
            s.push_str("\r\n");
        }
    }
    s.push_str("END:VCALENDAR\r\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a1\r\nDTSTART:20261112T130000Z\r\nDTEND:20261112T143000Z\r\nSUMMARY:Planning\\, part 2\r\nLOCATION:Room 4\r\nDESCRIPTION:Line one\\nLine two that is quite long and is fol\r\n ded onto the next line\r\nORGANIZER;CN=Ada:mailto:ada@example.com\r\nATTENDEE;CN=\"Bob B\":mailto:bob@example.com\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nDESCRIPTION:ignored\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a2\r\nDTSTART;VALUE=DATE:20261120\r\nDTEND;VALUE=DATE:20261123\r\nSUMMARY:Trip\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a3\r\nDTSTART;TZID=Europe/Berlin:20260105T090000\r\nDURATION:PT15M\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6\r\nEXDATE;TZID=Europe/Berlin:20260107T090000\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_events() {
        let ev = parse_ics(SAMPLE);
        assert_eq!(ev.len(), 3);
        let a = &ev[0];
        assert_eq!(a.summary, "Planning, part 2");
        assert_eq!(a.location, "Room 4");
        assert_eq!(a.description, "Line one\nLine two that is quite long and is folded onto the next line");
        assert_eq!(a.organizer, "Ada <ada@example.com>");
        assert_eq!(a.attendees, vec!["Bob B <bob@example.com>".to_string()]);
        // written back with the real addresses (CalDAV servers keep what we send)
        let again = parse_ics(&to_ics("x", &[a.clone()]));
        assert_eq!(again[0].organizer, "Ada <ada@example.com>");
        assert_eq!(again[0].attendees, a.attendees);
        let odd = Event { organizer: "Müller, Hans <h@x.de>".into(), attendees: vec!["solo@x.de".into(), "Nur Name".into()], ..a.clone() };
        let back = parse_ics(&to_ics("x", &[odd]));
        assert_eq!(back[0].organizer, "Müller, Hans <h@x.de>");
        assert_eq!(back[0].attendees[0], "solo@x.de");
        assert!(back[0].attendees[1].starts_with("Nur Name"));
        assert_eq!(a.end - a.start, 90 * 60);
        assert_eq!(a.start, days_from_civil(2026, 11, 12) * 86_400 + 13 * 3600);
        let trip = &ev[1];
        assert!(trip.all_day);
        assert_eq!(trip.start_day(), days_from_civil(2026, 11, 20));
        assert_eq!(trip.end_day(), days_from_civil(2026, 11, 22), "DTEND date is exclusive");
        assert_eq!(ev[2].end - ev[2].start, 15 * 60, "DURATION");
    }

    #[test]
    fn weekly_repeats_with_exdate_and_count() {
        let ev = parse_ics(SAMPLE);
        let standup = &ev[2];
        let occ = occurrences(standup, 0, i64::MAX / 2);
        // 6 occurrences (Mon/Wed from 5 Jan), one excluded
        assert_eq!(occ.len(), 5);
        let days: Vec<i64> = occ.iter().map(|o| t::utc_to_local(o.start).0).collect();
        assert_eq!(days[0], days_from_civil(2026, 1, 5));
        assert_eq!(days[1], days_from_civil(2026, 1, 12), "Wed 7 Jan is excluded");
    }

    #[test]
    fn other_frequencies_and_ranges() {
        let base = Event { uid: "x".into(), summary: "Pay rent".into(), start: days_from_civil(2026, 1, 31) * 86_400, end: days_from_civil(2026, 2, 1) * 86_400, all_day: true, rrule: Some("FREQ=MONTHLY".into()), ..Default::default() };
        let from = days_from_civil(2026, 1, 1) * 86_400;
        let to = days_from_civil(2027, 1, 1) * 86_400;
        let occ = occurrences(&base, from, to);
        assert_eq!(occ.len(), 7, "months with a 31st in 2026");
        let daily = Event { rrule: Some("FREQ=DAILY;INTERVAL=2;UNTIL=20260210".into()), ..base.clone() };
        let d = occurrences(&daily, from, to);
        assert_eq!(d.len(), 6, "31 Jan, 2, 4, 6, 8, 10 Feb");
        let yearly = Event { start: days_from_civil(2024, 2, 29) * 86_400, end: days_from_civil(2024, 3, 1) * 86_400, rrule: Some("FREQ=YEARLY".into()), ..base.clone() };
        assert_eq!(occurrences(&yearly, days_from_civil(2024, 1, 1) * 86_400, days_from_civil(2033, 1, 1) * 86_400).len(), 3, "29 Feb only in leap years");
        let single = Event { rrule: None, ..base.clone() };
        assert_eq!(occurrences(&single, from, to).len(), 1);
        assert!(occurrences(&single, to, to + 86_400).is_empty());
    }

    #[test]
    fn writes_and_reads_back() {
        let ev = parse_ics(SAMPLE);
        let text = to_ics("Test, cal", &ev);
        assert!(text.contains("X-WR-CALNAME:Test\\, cal"));
        assert!(text.lines().all(|l| l.trim_end_matches('\r').len() <= 75), "lines are folded");
        let back = parse_ics(&text);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].summary, ev[0].summary);
        assert_eq!(back[0].description, ev[0].description);
        assert_eq!(back[0].start, ev[0].start);
        assert_eq!(back[1].all_day, true);
        assert_eq!(back[1].end, ev[1].end);
        assert_eq!(back[2].rrule, ev[2].rrule);
        assert_eq!(occurrences(&back[2], 0, i64::MAX / 2).len(), 5);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("PT45M"), Some(2700));
        assert_eq!(parse_duration("P1DT2H"), Some(93_600));
        assert_eq!(parse_duration("P2W"), Some(1_209_600));
        assert_eq!(parse_duration("-PT15M"), Some(-900));
    }
}
