//! iCalendar (RFC 5545) events: reading and writing VEVENTs, and expanding repeating events.
//!
//! Supported: line folding, text escapes, DTSTART/DTEND/DURATION in UTC, with TZID (system
//! zoneinfo) or floating (local); all-day dates; SUMMARY, LOCATION, DESCRIPTION, URL, ORGANIZER,
//! ATTENDEE, STATUS, UID; RRULE with FREQ DAILY/WEEKLY/MONTHLY/YEARLY, INTERVAL, COUNT, UNTIL,
//! BYDAY (weekly); EXDATE and RDATE; overridden occurrences (RECURRENCE-ID); reminders (VALARM),
//! CATEGORIES, TRANSP.
//!
//! **Nothing that was read is lost when an event is written back (CAL-012).** An event keeps the
//! lines it was read from ([`Event::src`]) and the time zone definitions of its file
//! ([`Event::zones`]). Writing compares each field with what those lines say: a field that was not
//! changed is written as the very line that was read (parameters, alarms, properties Shale does not
//! know and all), and only a changed field is written anew. A time keeps its form: a time in a
//! named zone stays in that zone, so a repeating event keeps its clock time over summer time.

use crate::calendar::time::{self as t, civil_from_days, days_from_civil};

/// A reminder of an event (VALARM).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Alarm {
    /// Seconds before the start (negative: after it). None: a reminder of another kind (at a fixed
    /// time, or counted from the end), which is kept as it is.
    pub before: Option<i64>,
    /// The reminder as it was read (`BEGIN:VALARM` … `END:VALARM`); empty for one made or changed here.
    pub raw: Vec<String>,
}

impl Alarm {
    /// A new reminder this many seconds before the start.
    pub fn before_start(secs: i64) -> Alarm {
        Alarm { before: Some(secs), raw: vec![] }
    }
}

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
    /// The named zone the times are written in ("Europe/Berlin"); None: UTC (or `floating`).
    pub tzid: Option<String>,
    /// The times are written without a zone: the clock time wherever the reader is.
    pub floating: bool,
    /// Extra occurrence starts (RDATE; same units as `start`). Read only: the lines are kept as they are.
    pub rdates: Vec<i64>,
    pub categories: Vec<String>,
    /// Shown as free time (TRANSP:TRANSPARENT); otherwise as busy.
    pub transparent: bool,
    pub alarms: Vec<Alarm>,
    /// The event as it was read: its lines (unfolded) between `BEGIN:VEVENT` and `END:VEVENT`.
    /// Empty for an event made here. See the module comment.
    pub src: Vec<String>,
    /// The time zone definitions (VTIMEZONE, each with its `BEGIN` and `END` lines) of the file
    /// the event was read from that the event uses.
    pub zones: Vec<Vec<String>>,
}

impl Event {
    /// A time of this event on its own wall clock: (day number, seconds since midnight).
    pub fn wall(&self, v: i64) -> (i64, i64) {
        if self.all_day {
            (v.div_euclid(86_400), 0)
        } else {
            match &self.tzid {
                Some(tz) => t::utc_to_zoned(tz, v),
                None => t::utc_to_local(v),
            }
        }
    }

    /// The opposite of [`Event::wall`].
    pub fn at_wall(&self, day: i64, secs: i64) -> i64 {
        if self.all_day {
            day * 86_400
        } else {
            match &self.tzid {
                Some(tz) => t::zoned_to_utc(tz, day, secs),
                None => t::local_to_utc(day, secs),
            }
        }
    }

    /// `v` moved by `delta` seconds on the event's wall clock (so a move by whole days keeps the clock time).
    pub fn shift(&self, v: i64, delta: i64) -> i64 {
        let (d, s) = self.wall(v);
        let w = d * 86_400 + s + delta;
        self.at_wall(w.div_euclid(86_400), w.rem_euclid(86_400))
    }

    /// How far the start of `new` is from the start of `self`, on the wall clock of `self` (so
    /// writing the same moment in another zone is no move).
    pub fn wall_delta(&self, new: &Event) -> i64 {
        if self.all_day != new.all_day {
            return 0;
        }
        let (a, b) = (self.wall(self.start), self.wall(new.start));
        (b.0 * 86_400 + b.1) - (a.0 * 86_400 + a.1)
    }

    /// One date of this repeating event as an event of its own (an override, RECURRENCE-ID):
    /// everything the series says (title, place, notes, reminders, people), but no repeat. `start` is the date's start as the
    /// series has it; the copy can then be changed.
    pub fn override_at(&self, start: i64, end: i64) -> Event {
        Event { start, end, rrule: None, exdates: vec![], rdates: vec![], recurrence_id: Some(start), src: without_fields(&self.src, &["RRULE", "EXRULE", "EXDATE", "RDATE", "RECURRENCE-ID"]), ..self.clone() }
    }
}

#[cfg(test)]
impl Event {
    pub fn start_day(&self) -> i64 {
        if self.all_day {
            self.start.div_euclid(86_400)
        } else {
            t::utc_to_local(self.start).0
        }
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
    let secs = num(tpart.get(0..2)?)? * 3600 + num(tpart.get(2..4)?)? * 60 + num(tpart.get(4..6)?)?;
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
                let unit = match (c, in_time) {
                    ('W', _) => 604_800,
                    ('D', _) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
                // a number too big for a duration is no duration (not an overflow)
                total = total.checked_add(n.checked_mul(unit)?)?;
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
    let cn = if name.is_empty() {
        String::new()
    } else if name.contains(',') {
        format!(";CN=\"{name}\"")
    } else {
        format!(";CN={name}")
    };
    if mail.is_empty() {
        format!("{prop}{cn}:mailto:")
    } else {
        format!("{prop}{cn}:mailto:{mail}")
    }
}

/// A whole iCalendar file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Calendar {
    pub events: Vec<Event>,
    /// What else the file holds, as lines: other components (tasks, journals) and calendar
    /// properties Shale does not write itself. Written back as they are.
    pub other: Vec<String>,
}

/// All VEVENTs of an iCalendar text.
pub fn parse_ics(text: &str) -> Vec<Event> {
    parse_calendar(text).events
}

/// Where the block that begins at `lines[start]` (a `BEGIN:` line) ends: the index after its
/// `END:` line, blocks inside it counted. A block that is never closed runs to the end.
fn block_end(lines: &[String], start: usize) -> usize {
    let mut depth = 0usize;
    for (i, l) in lines.iter().enumerate().skip(start) {
        let head = l.get(..6).unwrap_or("");
        if head.eq_ignore_ascii_case("BEGIN:") {
            depth += 1;
        } else if l.get(..4).unwrap_or("").eq_ignore_ascii_case("END:") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return i + 1;
            }
        }
    }
    lines.len()
}

/// The lines inside a block (without its `BEGIN` and, if it has one, its `END` line).
fn block_inner(block: &[String]) -> &[String] {
    let closed = block.len() > 1 && block.last().is_some_and(|l| l.get(..4).unwrap_or("").eq_ignore_ascii_case("END:"));
    &block[1..block.len() - closed as usize]
}

/// The name a time zone definition is known by (its `TZID` line).
fn zone_id(block: &[String]) -> Option<String> {
    block.iter().map(|l| split_line(l)).find(|(n, _, _)| n == "TZID").map(|(_, _, v)| v.trim().to_string())
}

/// An iCalendar text: its events, and everything else in it (see [`Calendar`]).
pub fn parse_calendar(text: &str) -> Calendar {
    let lines = unfold(text);
    let mut cal = Calendar::default();
    let mut zones: Vec<Vec<String>> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (name, _, value) = split_line(&lines[i]);
        let kind = value.trim().to_ascii_uppercase();
        if name == "BEGIN" && kind != "VCALENDAR" {
            let end = block_end(&lines, i);
            let block = &lines[i..end];
            match kind.as_str() {
                "VEVENT" => cal.events.push(parse_event(block_inner(block))),
                "VTIMEZONE" => zones.push(block.to_vec()),
                _ => cal.other.extend(block.iter().cloned()),
            }
            i = end;
            continue;
        }
        if !matches!(name.as_str(), "BEGIN" | "END" | "VERSION" | "PRODID" | "X-WR-CALNAME") {
            cal.other.push(lines[i].clone());
        }
        i += 1;
    }
    // each event keeps the zone definitions it uses, so it can be written back on its own
    for e in &mut cal.events {
        e.zones = zones.iter().filter(|z| zone_id(z).is_some_and(|id| !id.is_empty() && e.src.iter().any(|l| l.contains(&id)))).cloned().collect();
    }
    cal
}

/// A list value split at the commas that are not escaped.
fn split_list(v: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in v.chars() {
        if esc {
            out.last_mut().unwrap().push(c);
            esc = false;
        } else if c == ',' {
            out.push(String::new());
        } else {
            esc = c == '\\';
            out.last_mut().unwrap().push(c);
        }
    }
    out
}

/// A reminder from its lines (`BEGIN:VALARM` … `END:VALARM`).
fn parse_alarm(block: &[String]) -> Alarm {
    let mut before = None;
    let mut i = 1;
    while i < block.len() {
        let (name, params, value) = split_line(&block[i]);
        if name == "BEGIN" {
            i = block_end(block, i);
            continue;
        }
        if name == "TRIGGER" {
            let other_kind = params.iter().any(|(k, v)| (k == "VALUE" && v.eq_ignore_ascii_case("DATE-TIME")) || (k == "RELATED" && v.eq_ignore_ascii_case("END")));
            before = if other_kind { None } else { parse_duration(value.trim()).map(|d| -d) };
        }
        i += 1;
    }
    Alarm { before, raw: block.to_vec() }
}

/// The lines of an event without the properties named (the parts inside it, such as reminders, stay).
fn without_fields(src: &[String], names: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let (name, _, _) = split_line(&src[i]);
        if name == "BEGIN" {
            let end = block_end(src, i);
            out.extend(src[i..end].iter().cloned());
            i = end;
            continue;
        }
        if !names.contains(&name.as_str()) {
            out.push(src[i].clone());
        }
        i += 1;
    }
    out
}

/// One event from the lines inside its `BEGIN:VEVENT` … `END:VEVENT`.
fn parse_event(lines: &[String]) -> Event {
    let mut e = Event { src: lines.to_vec(), ..Default::default() };
    let mut duration: Option<i64> = None;
    let mut i = 0;
    while i < lines.len() {
        let (name, params, value) = split_line(&lines[i]);
        if name == "BEGIN" {
            // a part inside the event: a reminder is understood, anything else is only kept
            let end = block_end(lines, i);
            if value.trim().eq_ignore_ascii_case("VALARM") {
                e.alarms.push(parse_alarm(&lines[i..end]));
            }
            i = end;
            continue;
        }
        i += 1;
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
                    e.tzid = if ad { None } else { params.iter().find(|(k, _)| k == "TZID").map(|(_, z)| z.clone()) };
                    e.floating = !ad && e.tzid.is_none() && !value.trim().ends_with('Z');
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
            "RDATE" => {
                // a period (`start/end`) is not understood; its line is kept all the same
                for v in value.split(',').filter(|v| !v.contains('/')) {
                    if let Some((d, _)) = parse_dt(v, &params) {
                        e.rdates.push(d);
                    }
                }
            }
            "RECURRENCE-ID" => e.recurrence_id = parse_dt(&value, &params).map(|x| x.0),
            "CATEGORIES" => e.categories.extend(split_list(&value).iter().map(|c| unescape(c).trim().to_string()).filter(|c| !c.is_empty())),
            "TRANSP" => e.transparent = value.trim().eq_ignore_ascii_case("TRANSPARENT"),
            _ => {}
        }
    }
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
    e
}

// ---------------------------------------------------------------- the repeat rule, for editing

/// How often a simple repeat happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Freq {
    pub const ALL: [Freq; 4] = [Freq::Daily, Freq::Weekly, Freq::Monthly, Freq::Yearly];
    fn word(self) -> &'static str {
        match self {
            Freq::Daily => "DAILY",
            Freq::Weekly => "WEEKLY",
            Freq::Monthly => "MONTHLY",
            Freq::Yearly => "YEARLY",
        }
    }
}

/// When a repeat stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatEnd {
    Never,
    /// The last day it may happen on (a day number on the event's wall clock).
    Until(i64),
    /// After this many times.
    Count(u32),
}

/// A repeat rule the event dialog can show and change: every `interval` days, weeks (on `days`,
/// Monday first), months or years, until `end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repeat {
    pub freq: Freq,
    pub interval: u32,
    pub days: [bool; 7],
    pub end: RepeatEnd,
}

const DAY_CODES: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

/// The rule of `e` as a [`Repeat`]. None when it has no rule, or one with parts the dialog does
/// not know (the third Tuesday, every 5th of the month …): such a rule is shown and kept as it is.
pub fn parse_repeat(e: &Event) -> Option<Repeat> {
    let mut r = Repeat { freq: Freq::Daily, interval: 1, days: [false; 7], end: RepeatEnd::Never };
    let mut have_freq = false;
    for (k, v) in rule_parts(e.rrule.as_deref()?) {
        match k.as_str() {
            "FREQ" => {
                r.freq = *Freq::ALL.iter().find(|f| f.word().eq_ignore_ascii_case(&v))?;
                have_freq = true;
            }
            "INTERVAL" => r.interval = v.parse().ok().filter(|n| *n >= 1)?,
            "COUNT" => r.end = RepeatEnd::Count(v.parse().ok()?),
            "UNTIL" => {
                let (u, _) = parse_dt(&v, &[])?;
                // a rule's end in UTC is an instant; a date or a time without zone is on the wall clock already
                r.end = RepeatEnd::Until(if v.trim().ends_with('Z') && !e.all_day { e.wall(u).0 } else { u.div_euclid(86_400) });
            }
            "BYDAY" => {
                for d in v.split(',') {
                    r.days[DAY_CODES.iter().position(|c| c.eq_ignore_ascii_case(d.trim()))?] = true;
                }
            }
            "WKST" if v.eq_ignore_ascii_case("MO") => {}
            _ => return None,
        }
    }
    if !have_freq || (r.freq != Freq::Weekly && r.days.iter().any(|d| *d)) {
        return None;
    }
    Some(r)
}

/// The rule text of a [`Repeat`] for the event `e` (the end of the rule is written in the form
/// the event's times have).
pub fn repeat_rule(r: &Repeat, e: &Event) -> String {
    let mut s = format!("FREQ={}", r.freq.word());
    if r.interval > 1 {
        s.push_str(&format!(";INTERVAL={}", r.interval));
    }
    if r.freq == Freq::Weekly && r.days.iter().any(|d| *d) {
        let days: Vec<&str> = DAY_CODES.iter().zip(r.days).filter(|(_, on)| *on).map(|(c, _)| *c).collect();
        s.push_str(&format!(";BYDAY={}", days.join(",")));
    }
    match r.end {
        RepeatEnd::Never => {}
        RepeatEnd::Count(n) => s.push_str(&format!(";COUNT={n}")),
        RepeatEnd::Until(day) => {
            let (y, m, d) = civil_from_days(day);
            if e.all_day {
                s.push_str(&format!(";UNTIL={y:04}{m:02}{d:02}"));
            } else if e.floating {
                s.push_str(&format!(";UNTIL={y:04}{m:02}{d:02}T235959"));
            } else {
                // the last second of that day on the event's wall clock, as UTC
                s.push_str(&format!(";UNTIL={}", utc_stamp(e.at_wall(day, 86_399))));
            }
        }
    }
    s
}

fn rule_parts(rule: &str) -> Vec<(String, String)> {
    rule.split(';').filter_map(|p| p.split_once('=')).map(|(k, v)| (k.to_ascii_uppercase(), v.to_string())).collect()
}

/// Occurrences of `e` that overlap [from, to) (in `e`'s units). Non-repeating events give at
/// most one. At most 2000 occurrences are generated per event.
pub fn occurrences(e: &Event, from: i64, to: i64) -> Vec<Occurrence> {
    let mut out = rule_occurrences(e, from, to);
    // extra dates (RDATE) happen as well, unless they are excluded or are a repeat already
    let len = (e.end - e.start).max(0);
    for r in &e.rdates {
        if *r < to && r + len.max(1) > from && !e.exdates.contains(r) && !out.iter().any(|o| o.start == *r) {
            out.push(Occurrence { start: *r, end: r + len });
        }
    }
    if !e.rdates.is_empty() {
        out.sort_by_key(|o| o.start);
    }
    out
}

fn rule_occurrences(e: &Event, from: i64, to: i64) -> Vec<Occurrence> {
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
    // work on the event's wall clock (its named zone, else this computer's) so repeats keep
    // their clock time over summer-time changes
    let (base_day, base_secs) = e.wall(e.start);
    let to_units = |day: i64| e.at_wall(day, base_secs);
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
                let days: Vec<u32> = if byday.is_empty() {
                    vec![t::weekday(base_day)]
                } else {
                    let mut d = byday.clone();
                    d.sort();
                    d
                };
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
                if bd <= t::days_in_month(y, m) {
                    push(days_from_civil(y, m, bd), &mut out)
                } else {
                    true
                }
            }
            "YEARLY" => {
                let y = by + (k * interval) as i32;
                if bd <= t::days_in_month(y, bm) {
                    push(days_from_civil(y, bm, bd), &mut out)
                } else {
                    true
                }
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

/// `20261112T130000Z`
fn utc_stamp(v: i64) -> String {
    let (y, m, d) = civil_from_days(v.div_euclid(86_400));
    let s = v.rem_euclid(86_400);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", s / 3600, s % 3600 / 60, s % 60)
}

/// `20261112T130000` for a wall clock time.
fn wall_stamp((day, s): (i64, i64)) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}", s / 3600, s % 3600 / 60, s % 60)
}

/// A time of the event `e` as the rest of a property line (parameters, `:` and the value), in the
/// form the event's times have: a date, a time in its named zone, a time without zone, or UTC.
fn fmt_time(e: &Event, v: i64) -> String {
    if e.all_day {
        let (y, m, d) = civil_from_days(v.div_euclid(86_400));
        return format!(";VALUE=DATE:{y:04}{m:02}{d:02}");
    }
    match &e.tzid {
        Some(tz) => {
            let name = if tz.contains([':', ';', ',']) { format!("\"{}\"", tz.replace('"', "")) } else { tz.clone() };
            format!(";TZID={name}:{}", wall_stamp(t::utc_to_zoned(tz, v)))
        }
        None if e.floating => format!(":{}", wall_stamp(t::utc_to_local(v))),
        None => format!(":{}", utc_stamp(v)),
    }
}

/// `-PT15M` for a reminder 15 minutes before the start.
fn fmt_trigger(before: i64) -> String {
    let n = before.abs();
    let body = if n == 0 {
        "PT0M".to_string()
    } else if n % 86_400 == 0 {
        format!("P{}D", n / 86_400)
    } else if n % 3600 == 0 {
        format!("PT{}H", n / 3600)
    } else if n % 60 == 0 {
        format!("PT{}M", n / 60)
    } else {
        format!("PT{n}S")
    };
    format!("{}{body}", if before > 0 { "-" } else { "" })
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

/// The fields Shale writes itself, in the order a new event gets them. Each is one or more
/// property lines; `DTSTART` stands for the times (`DTSTART`, `DTEND`, `DURATION`).
const FIELDS: [&str; 14] = ["UID", "DTSTART", "RECURRENCE-ID", "SUMMARY", "LOCATION", "DESCRIPTION", "URL", "STATUS", "ORGANIZER", "ATTENDEE", "CATEGORIES", "TRANSP", "RRULE", "EXDATE"];

/// The field a property line belongs to (None: a property Shale only keeps).
fn field_of(name: &str) -> Option<&'static str> {
    match name {
        "DTEND" | "DURATION" => Some("DTSTART"),
        n => FIELDS.iter().find(|f| **f == n).copied(),
    }
}

/// Does `e` have the same value for a field as `b` (what its source lines say)?
fn same_field(field: &str, e: &Event, b: &Event) -> bool {
    match field {
        "UID" => e.uid == b.uid,
        "DTSTART" => (e.start, e.end, e.all_day, &e.tzid, e.floating) == (b.start, b.end, b.all_day, &b.tzid, b.floating),
        "RECURRENCE-ID" => e.recurrence_id == b.recurrence_id,
        "SUMMARY" => e.summary == b.summary,
        "LOCATION" => e.location == b.location,
        "DESCRIPTION" => e.description == b.description,
        "URL" => e.url == b.url,
        "STATUS" => e.status == b.status,
        "ORGANIZER" => e.organizer == b.organizer,
        "ATTENDEE" => e.attendees == b.attendees,
        "CATEGORIES" => e.categories == b.categories,
        "TRANSP" => e.transparent == b.transparent,
        "RRULE" => e.rrule == b.rrule,
        "EXDATE" => e.exdates == b.exdates,
        _ => true,
    }
}

/// The lines of a field written anew from the event.
fn field_lines(field: &str, e: &Event) -> Vec<String> {
    let text = |name: &str, v: &str| if v.is_empty() { vec![] } else { vec![format!("{name}:{}", escape(v))] };
    match field {
        "UID" => vec![format!("UID:{}", e.uid)],
        "DTSTART" => vec![format!("DTSTART{}", fmt_time(e, e.start)), format!("DTEND{}", fmt_time(e, e.end))],
        "RECURRENCE-ID" => e.recurrence_id.map(|r| format!("RECURRENCE-ID{}", fmt_time(e, r))).into_iter().collect(),
        "SUMMARY" => vec![format!("SUMMARY:{}", escape(&e.summary))],
        "LOCATION" => text("LOCATION", &e.location),
        "DESCRIPTION" => text("DESCRIPTION", &e.description),
        "URL" => {
            if e.url.is_empty() {
                vec![]
            } else {
                vec![format!("URL:{}", e.url)]
            }
        }
        "STATUS" => {
            if e.status.is_empty() {
                vec![]
            } else {
                vec![format!("STATUS:{}", e.status)]
            }
        }
        "ORGANIZER" => {
            if e.organizer.is_empty() {
                vec![]
            } else {
                vec![person_line("ORGANIZER", &e.organizer)]
            }
        }
        "ATTENDEE" => e.attendees.iter().map(|a| person_line("ATTENDEE", a)).collect(),
        "CATEGORIES" => {
            if e.categories.is_empty() {
                vec![]
            } else {
                vec![format!("CATEGORIES:{}", e.categories.iter().map(|c| escape(c)).collect::<Vec<_>>().join(","))]
            }
        }
        "TRANSP" => {
            if e.transparent {
                vec!["TRANSP:TRANSPARENT".into()]
            } else {
                vec![]
            }
        }
        "RRULE" => e.rrule.iter().map(|r| format!("RRULE:{r}")).collect(),
        "EXDATE" => e.exdates.iter().map(|x| format!("EXDATE{}", fmt_time(e, *x))).collect(),
        _ => vec![],
    }
}

/// The lines of a reminder: as it was read if it was not changed, else a plain one that shows a message.
fn alarm_lines(a: &Alarm, e: &Event) -> Vec<String> {
    if !a.raw.is_empty() && parse_alarm(&a.raw).before == a.before {
        return a.raw.clone();
    }
    let Some(before) = a.before else { return vec![] };
    let what = if e.summary.trim().is_empty() { "Reminder" } else { e.summary.trim() };
    vec!["BEGIN:VALARM".into(), "ACTION:DISPLAY".into(), format!("DESCRIPTION:{}", escape(what)), format!("TRIGGER:{}", fmt_trigger(before)), "END:VALARM".into()]
}

/// The lines of one event, `BEGIN:VEVENT` to `END:VEVENT` (not folded). See the module comment:
/// what was read and not changed is written as it was read.
fn event_lines(e: &Event, now: &str) -> Vec<String> {
    let base = (!e.src.is_empty()).then(|| parse_event(&e.src));
    let kept = |f: &str| base.as_ref().is_some_and(|b| same_field(f, e, b));
    let changed = base.as_ref().is_none_or(|b| b.alarms != e.alarms) || FIELDS.iter().any(|f| !kept(f));
    let mut out = vec!["BEGIN:VEVENT".to_string()];
    let mut written: Vec<&str> = Vec::new();
    let (mut stamped, mut modified) = (false, false);
    let mut i = 0;
    while i < e.src.len() {
        let line = &e.src[i];
        let (name, _, value) = split_line(line);
        if name == "BEGIN" {
            // reminders are written from the event below; any other part inside is kept
            let end = block_end(&e.src, i);
            if !value.trim().eq_ignore_ascii_case("VALARM") {
                out.extend(e.src[i..end].iter().cloned());
            }
            i = end;
            continue;
        }
        i += 1;
        match field_of(&name) {
            Some(f) if kept(f) => out.push(line.clone()),
            Some(f) => {
                if !written.contains(&f) {
                    written.push(f);
                    out.extend(field_lines(f, e));
                }
            }
            None if name == "DTSTAMP" => {
                stamped = true;
                out.push(if changed { format!("DTSTAMP:{now}") } else { line.clone() });
            }
            None if name == "LAST-MODIFIED" && changed => {
                modified = true;
                out.push(format!("LAST-MODIFIED:{now}"));
            }
            None if name == "END" => {} // a stray end line would close the event early
            None => out.push(line.clone()),
        }
    }
    for f in FIELDS {
        if !kept(f) && !written.contains(&f) {
            out.extend(field_lines(f, e));
        }
    }
    if !stamped {
        out.push(format!("DTSTAMP:{now}"));
    }
    if changed && base.is_some() && !modified {
        out.push(format!("LAST-MODIFIED:{now}"));
    }
    for a in &e.alarms {
        out.extend(alarm_lines(a, e));
    }
    out.push("END:VEVENT".into());
    out
}

/// `+0100`
fn fmt_offset(secs: i64) -> String {
    format!("{}{:02}{:02}", if secs < 0 { '-' } else { '+' }, secs.abs() / 3600, secs.abs() % 3600 / 60)
}

/// "the last Sunday of March" as (month, `-1SU`): how the yearly change of a zone is said.
fn change_rule(day: i64) -> (u32, String) {
    let (y, m, d) = civil_from_days(day);
    let nth = if d + 7 > t::days_in_month(y, m) { -1 } else { (d as i64 - 1) / 7 + 1 };
    (m, format!("{nth}{}", DAY_CODES[t::weekday(day) as usize]))
}

/// A time zone definition for a zone this computer knows, good for the years around `around`
/// (written for an event made here; programs that know the zone's name use their own rules).
fn make_zone(tzid: &str, around: i64) -> Option<Vec<String>> {
    let offset = t::zone_offset(tzid, around)?;
    let (year, _, _) = civil_from_days(around.div_euclid(86_400));
    let year_of = |y: i32| t::zone_changes(tzid, days_from_civil(y, 1, 1) * 86_400, days_from_civil(y + 1, 1, 1) * 86_400);
    let mut out = vec!["BEGIN:VTIMEZONE".to_string(), format!("TZID:{tzid}")];
    let changes = year_of(year);
    if changes.is_empty() {
        out.extend(["BEGIN:STANDARD".to_string(), "DTSTART:19700101T000000".into(), format!("TZOFFSETFROM:{}", fmt_offset(offset)), format!("TZOFFSETTO:{}", fmt_offset(offset)), "END:STANDARD".into()]);
    }
    let next = year_of(year + 1);
    for (at, before, after) in changes {
        let kind = if after > before { "DAYLIGHT" } else { "STANDARD" };
        let wall = at + before;
        let (month, by_day) = change_rule(wall.div_euclid(86_400));
        out.push(format!("BEGIN:{kind}"));
        out.push(format!("DTSTART:{}", wall_stamp((wall.div_euclid(86_400), wall.rem_euclid(86_400)))));
        out.push(format!("TZOFFSETFROM:{}", fmt_offset(before)));
        out.push(format!("TZOFFSETTO:{}", fmt_offset(after)));
        // the same rule next year: it repeats (true for the zones in use today)
        if next.iter().any(|(a, b, c)| (*b, *c) == (before, after) && change_rule((a + b).div_euclid(86_400)) == (month, by_day.clone())) {
            out.push(format!("RRULE:FREQ=YEARLY;BYMONTH={month};BYDAY={by_day}"));
        }
        out.push(format!("END:{kind}"));
    }
    out.push("END:VTIMEZONE".into());
    Some(out)
}

/// A complete iCalendar file with these events.
pub fn to_ics(name: &str, events: &[Event]) -> String {
    to_ics_with(name, events, &[])
}

/// A complete iCalendar file with these events and the `other` lines of the file they came from
/// ([`Calendar::other`]). The time zone definitions the events use come first: the ones that
/// were read, and one made here for a zone that has none.
pub fn to_ics_with(name: &str, events: &[Event], other: &[String]) -> String {
    let mut lines = vec!["BEGIN:VCALENDAR".to_string(), "VERSION:2.0".into(), "PRODID:-//Shale//Calendar//EN".into(), format!("X-WR-CALNAME:{}", escape(name))];
    let mut zones: Vec<Vec<String>> = Vec::new();
    for z in events.iter().flat_map(|e| &e.zones) {
        if !zones.contains(z) {
            zones.push(z.clone());
        }
    }
    for e in events.iter().filter(|e| !e.all_day) {
        if let Some(tz) = &e.tzid {
            if !zones.iter().any(|z| zone_id(z).as_deref() == Some(tz.as_str())) {
                zones.extend(make_zone(tz, e.start));
            }
        }
    }
    lines.extend(zones.into_iter().flatten());
    lines.extend(other.iter().cloned());
    let now = utc_stamp(t::now());
    for e in events {
        lines.extend(event_lines(e, &now));
    }
    lines.push("END:VCALENDAR".into());
    let mut s = String::new();
    for l in lines {
        s.push_str(&fold(&l));
        s.push_str("\r\n");
    }
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a1\r\nDTSTART:20261112T130000Z\r\nDTEND:20261112T143000Z\r\nSUMMARY:Planning\\, part 2\r\nLOCATION:Room 4\r\nDESCRIPTION:Line one\\nLine two that is quite long and is fol\r\n ded onto the next line\r\nORGANIZER;CN=Ada:mailto:ada@example.com\r\nATTENDEE;CN=\"Bob B\":mailto:bob@example.com\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nDESCRIPTION:ignored\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a2\r\nDTSTART;VALUE=DATE:20261120\r\nDTEND;VALUE=DATE:20261123\r\nSUMMARY:Trip\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a3\r\nDTSTART;TZID=Europe/Berlin:20260105T090000\r\nDURATION:PT15M\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6\r\nEXDATE;TZID=Europe/Berlin:20260107T090000\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    // spec: CAL-006
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
        let again = parse_ics(&to_ics("x", std::slice::from_ref(a)));
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

    // spec: CAL-006
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

    // spec: CAL-006
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

    // spec: CAL-005, DATA-005
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
        assert!(back[1].all_day);
        assert_eq!(back[1].end, ev[1].end);
        assert_eq!(back[2].rrule, ev[2].rrule);
        assert_eq!(occurrences(&back[2], 0, i64::MAX / 2).len(), 5);
    }

    // spec: CAL-006
    #[test]
    fn durations() {
        assert_eq!(parse_duration("PT45M"), Some(2700));
        assert_eq!(parse_duration("P1DT2H"), Some(93_600));
        assert_eq!(parse_duration("P2W"), Some(1_209_600));
        assert_eq!(parse_duration("-PT15M"), Some(-900));
    }

    /// An event as another program writes it: a named zone with its definition, a rule, an excluded and an
    /// extra date, people with their answers, two reminders Shale would not make, properties Shale does not
    /// know, a changed single date, and a task in the same file.
    pub(crate) const RICH: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//EN\r\nCALSCALE:GREGORIAN\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nX-LIC-LOCATION:Europe/Berlin\r\nBEGIN:DAYLIGHT\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nTZNAME:CEST\r\nDTSTART:19700329T020000\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nEND:DAYLIGHT\r\nBEGIN:STANDARD\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nTZNAME:CET\r\nDTSTART:19701025T030000\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:rich-1\r\nDTSTAMP:20260101T000000Z\r\nCREATED:20251201T080000Z\r\nLAST-MODIFIED:20251202T080000Z\r\nSEQUENCE:3\r\nDTSTART;TZID=Europe/Berlin:20260105T090000\r\nDTEND;TZID=Europe/Berlin:20260105T093000\r\nSUMMARY;LANGUAGE=de:Standup\r\nLOCATION;ALTREP=\"http://x.example/room\":Room 4\r\nDESCRIPTION:Daily\\, short\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;WKST=MO\r\nEXDATE;TZID=Europe/Berlin:20260107T090000\r\nRDATE;TZID=Europe/Berlin:20260110T090000\r\nSTATUS:TENTATIVE\r\nTRANSP:TRANSPARENT\r\nCLASS:PRIVATE\r\nCATEGORIES:Work,Team\r\nORGANIZER;CN=Ada:mailto:ada@example.com\r\nATTENDEE;CN=Bob;PARTSTAT=ACCEPTED;ROLE=REQ-PARTICIPANT:mailto:bob@example.com\r\nX-APPLE-TRAVEL-ADVISORY-BEHAVIOR:AUTOMATIC\r\nBEGIN:VALARM\r\nACTION:EMAIL\r\nTRIGGER:-PT15M\r\nSUMMARY:Soon\r\nDESCRIPTION:Standup soon\r\nATTENDEE:mailto:ada@example.com\r\nX-WR-ALARMUID:abc\r\nEND:VALARM\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER;VALUE=DATE-TIME:20260105T080000Z\r\nDESCRIPTION:fixed\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:rich-1\r\nDTSTAMP:20260101T000000Z\r\nRECURRENCE-ID;TZID=Europe/Berlin:20260112T090000\r\nDTSTART;TZID=Europe/Berlin:20260112T100000\r\nDTEND;TZID=Europe/Berlin:20260112T103000\r\nSUMMARY:Standup (late)\r\nEND:VEVENT\r\nBEGIN:VTODO\r\nUID:todo-1\r\nSUMMARY:Buy milk\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    /// The lines of a file that carry content (not the frame Shale writes itself), sorted.
    fn content(text: &str) -> Vec<String> {
        let mut v: Vec<String> = unfold(text).into_iter().filter(|l| !matches!(l.as_str(), "BEGIN:VCALENDAR" | "END:VCALENDAR" | "VERSION:2.0") && !l.starts_with("PRODID:") && !l.starts_with("X-WR-CALNAME:")).collect();
        v.sort();
        v
    }

    /// What is in `a` and not in `b`, counting repeats.
    fn missing(a: &[String], b: &[String]) -> Vec<String> {
        let mut b = b.to_vec();
        a.iter().filter(|l| b.iter().position(|x| x == *l).map(|i| b.remove(i)).is_none()).cloned().collect()
    }

    // spec: CAL-012
    #[test]
    fn an_event_that_was_not_changed_is_written_back_line_for_line() {
        let cal = parse_calendar(RICH);
        assert_eq!(cal.events.len(), 2);
        assert_eq!(cal.other, vec!["CALSCALE:GREGORIAN", "BEGIN:VTODO", "UID:todo-1", "SUMMARY:Buy milk", "END:VTODO"]);
        let e = &cal.events[0];
        assert_eq!((e.tzid.as_deref(), e.floating, e.status.as_str(), e.transparent), (Some("Europe/Berlin"), false, "TENTATIVE", true));
        assert_eq!(e.categories, vec!["Work", "Team"]);
        assert_eq!(e.alarms.iter().map(|a| a.before).collect::<Vec<_>>(), vec![Some(900), None], "15 minutes before, and one at a fixed time");
        assert_eq!((e.exdates.len(), e.rdates.len(), e.zones.len()), (1, 1, 1));
        assert!(cal.events[1].recurrence_id.is_some() && cal.events[1].rrule.is_none());
        let out = to_ics_with("x", &cal.events, &cal.other);
        assert_eq!(content(&out), content(RICH), "every line is there again, and nothing else");
        // also when it is written on its own, as a CalDAV save does
        let alone = to_ics("", &cal.events);
        assert!(missing(&content(RICH), &content(&alone)).iter().all(|l| cal.other.contains(l)), "{:?}", missing(&content(RICH), &content(&alone)));
        // and reading what was written gives the same events
        let again = parse_calendar(&out);
        assert_eq!(again.events, cal.events);
        assert_eq!(again.other, cal.other);
    }

    // spec: CAL-012
    #[test]
    fn changing_a_field_rewrites_that_field_only() {
        let cal = parse_calendar(RICH);
        let mut e = cal.events[0].clone();
        e.summary = "Daily".into();
        e.status.clear();
        e.alarms.push(Alarm::before_start(2 * 3600));
        let out = to_ics("", &[e.clone(), cal.events[1].clone()]);
        let (before, after) = (content(RICH), content(&out));
        let gone = missing(&before, &after);
        let expect_gone = ["SUMMARY;LANGUAGE=de:Standup", "STATUS:TENTATIVE", "DTSTAMP:20260101T000000Z", "LAST-MODIFIED:20251202T080000Z", "CALSCALE:GREGORIAN", "BEGIN:VTODO", "UID:todo-1", "SUMMARY:Buy milk", "END:VTODO"];
        assert_eq!(gone.len(), expect_gone.len(), "{gone:?}");
        assert!(expect_gone.iter().all(|l| gone.iter().any(|g| g == l)), "{gone:?}");
        let new = missing(&after, &before);
        assert!(new.contains(&"SUMMARY:Daily".to_string()) && new.contains(&"TRIGGER:-PT2H".to_string()) && new.contains(&"ACTION:DISPLAY".to_string()), "{new:?}");
        assert!(new.iter().any(|l| l.starts_with("DTSTAMP:")) && new.iter().any(|l| l.starts_with("LAST-MODIFIED:")), "{new:?}");
        // the people with their answers, the foreign reminders, the unknown properties and the changed single date are untouched
        for keep in [
            "ATTENDEE;CN=Bob;PARTSTAT=ACCEPTED;ROLE=REQ-PARTICIPANT:mailto:bob@example.com",
            "ACTION:EMAIL",
            "X-WR-ALARMUID:abc",
            "CLASS:PRIVATE",
            "SEQUENCE:3",
            "LOCATION;ALTREP=\"http://x.example/room\":Room 4",
            "RECURRENCE-ID;TZID=Europe/Berlin:20260112T090000",
            "TRIGGER;VALUE=DATE-TIME:20260105T080000Z",
        ] {
            assert!(after.iter().any(|l| l == keep), "{keep} in {out}");
        }
        let back = parse_ics(&out);
        assert_eq!((back[0].summary.as_str(), back[0].status.as_str()), ("Daily", ""));
        assert_eq!(back[0].alarms.iter().map(|a| a.before).collect::<Vec<_>>(), vec![Some(900), None, Some(7200)]);
        // a reminder that was changed is written anew, one that was removed is gone
        let mut e2 = cal.events[0].clone();
        e2.alarms[0].before = Some(600);
        e2.alarms.remove(1);
        let out = to_ics("", &[e2]);
        assert!(out.contains("TRIGGER:-PT10M") && !out.contains("ACTION:EMAIL") && !out.contains("DESCRIPTION:fixed"), "{out}");
    }

    // spec: CAL-012, CAL-009
    #[test]
    fn times_keep_their_zone_and_repeats_keep_their_clock_time() {
        if !t::zone_known("Europe/Berlin") {
            return; // no zoneinfo on this system
        }
        let cal = parse_calendar(RICH);
        let mut e = cal.events[0].clone();
        // 09:00 in Berlin in winter and in summer, whatever this computer's zone is
        let jan = occurrences(&e, days_from_civil(2026, 1, 19) * 86_400, days_from_civil(2026, 1, 20) * 86_400);
        let jul = occurrences(&e, days_from_civil(2026, 7, 6) * 86_400, days_from_civil(2026, 7, 7) * 86_400);
        assert_eq!(jan[0].start, days_from_civil(2026, 1, 19) * 86_400 + 8 * 3600);
        assert_eq!(jul[0].start, days_from_civil(2026, 7, 6) * 86_400 + 7 * 3600);
        // the extra date happens, the excluded one does not
        let first = occurrences(&e, days_from_civil(2026, 1, 5) * 86_400, days_from_civil(2026, 1, 12) * 86_400);
        assert_eq!(first.iter().map(|o| e.wall(o.start).0 - days_from_civil(2026, 1, 5)).collect::<Vec<_>>(), vec![0, 5]);
        // an hour later: written in the same zone, with the definition that was read, and the excluded date as given
        e.start += 3600;
        e.end += 3600;
        e.exdates.push(e.at_wall(days_from_civil(2026, 1, 14), 10 * 3600));
        let out = to_ics("", std::slice::from_ref(&e));
        assert!(out.contains("DTSTART;TZID=Europe/Berlin:20260105T100000\r\n") && out.contains("DTEND;TZID=Europe/Berlin:20260105T103000\r\n"), "{out}");
        assert!(out.contains("EXDATE;TZID=Europe/Berlin:20260114T100000\r\n") && out.contains("X-LIC-LOCATION:Europe/Berlin"), "{out}");
        assert_eq!(out.matches("BEGIN:VTIMEZONE").count(), 1);
        // an event made here in a named zone gets a definition, with the yearly change as a rule
        let new = Event { uid: "n1".into(), summary: "New".into(), start: e.start, end: e.end, tzid: Some("Europe/Berlin".into()), rrule: Some("FREQ=WEEKLY".into()), ..Default::default() };
        let out = to_ics("", std::slice::from_ref(&new));
        for l in [
            "BEGIN:VTIMEZONE",
            "TZID:Europe/Berlin",
            "DTSTART:20260329T020000",
            "TZOFFSETFROM:+0100",
            "TZOFFSETTO:+0200",
            "RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU",
            "RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU",
            "DTSTART;TZID=Europe/Berlin:20260105T100000",
        ] {
            assert!(out.contains(&format!("{l}\r\n")), "{l} in {out}");
        }
        assert_eq!(parse_ics(&out)[0].start, new.start);
        // one date of the series as an event of its own: everything the series says, no repeat
        let one = cal.events[0].override_at(jan[0].start, jan[0].end);
        let out = to_ics("", std::slice::from_ref(&one));
        assert!(out.contains("RECURRENCE-ID;TZID=Europe/Berlin:20260119T090000\r\n") && out.contains("DTSTART;TZID=Europe/Berlin:20260119T090000\r\n"), "{out}");
        assert!(!out.contains("RRULE:FREQ=WEEKLY") && !out.contains("EXDATE") && out.contains("PARTSTAT=ACCEPTED") && out.contains("ACTION:EMAIL"), "{out}");
    }

    // spec: CAL-013
    #[test]
    fn simple_rules_are_read_for_editing_and_others_are_left_alone() {
        let ev = |rule: &str, all_day: bool| Event { start: days_from_civil(2026, 1, 5) * 86_400, end: days_from_civil(2026, 1, 6) * 86_400, all_day, rrule: Some(rule.into()), ..Default::default() };
        let r = parse_repeat(&ev("FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6", true)).unwrap();
        assert_eq!(r, Repeat { freq: Freq::Weekly, interval: 1, days: [true, false, true, false, false, false, false], end: RepeatEnd::Count(6) });
        assert_eq!(repeat_rule(&r, &ev("x", true)), "FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6");
        let r = parse_repeat(&ev("FREQ=DAILY;INTERVAL=2;UNTIL=20260210", true)).unwrap();
        assert_eq!((r.freq, r.interval, r.end), (Freq::Daily, 2, RepeatEnd::Until(days_from_civil(2026, 2, 10))));
        assert_eq!(repeat_rule(&r, &ev("x", true)), "FREQ=DAILY;INTERVAL=2;UNTIL=20260210");
        // a timed event: the last day is that whole day on the event's clock, written as UTC
        let timed = Event { tzid: None, ..ev("x", false) };
        let rule = repeat_rule(&Repeat { freq: Freq::Monthly, interval: 1, days: [false; 7], end: RepeatEnd::Until(days_from_civil(2026, 6, 30)) }, &timed);
        assert!(rule.starts_with("FREQ=MONTHLY;UNTIL=2026") && rule.ends_with('Z'), "{rule}");
        assert_eq!(parse_repeat(&Event { rrule: Some(rule), ..timed.clone() }).unwrap().end, RepeatEnd::Until(days_from_civil(2026, 6, 30)));
        // rules with parts the dialog has no field for
        for other in ["FREQ=MONTHLY;BYDAY=2TU", "FREQ=MONTHLY;BYMONTHDAY=5", "FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU", "FREQ=WEEKLY;WKST=SU", "FREQ=HOURLY", "INTERVAL=2", "FREQ=DAILY;BYDAY=MO"] {
            assert_eq!(parse_repeat(&ev(other, true)), None, "{other}");
        }
        assert_eq!(parse_repeat(&Event::default()), None);
        assert_eq!((fmt_trigger(900), fmt_trigger(86_400), fmt_trigger(0), fmt_trigger(-600), fmt_trigger(90)), ("-PT15M".into(), "-P1D".into(), "PT0M".into(), "PT10M".into(), "-PT90S".into()));
    }

    // spec: CAL-012, DATA-005
    #[test]
    fn random_changes_never_lose_a_line_that_was_not_changed() {
        struct Rng(u64);
        impl Rng {
            fn next(&mut self) -> u64 {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                self.0
            }
            fn pick(&mut self, n: u64) -> u64 {
                self.next() % n
            }
        }
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        let mut pool: Vec<Event> = parse_ics(RICH).into_iter().chain(parse_ics(SAMPLE)).collect();
        pool.push(Event { uid: "made-here".into(), summary: "New".into(), start: 1_800_000_000, end: 1_800_003_600, ..Default::default() });
        let words = ["Plan", "A, b; c", "Zeile 1\nZeile 2", "", "ünïcode ✓", "back\\slash"];
        for round in 0..400 {
            let i = rng.pick(pool.len() as u64) as usize;
            let before = pool[i].clone();
            let mut e = before.clone();
            let mut touched: Vec<&str> = vec![];
            for _ in 0..1 + rng.pick(3) {
                match rng.pick(11) {
                    0 => {
                        e.summary = words[rng.pick(6) as usize].into();
                        touched.push("SUMMARY");
                    }
                    1 => {
                        e.location = words[rng.pick(6) as usize].into();
                        touched.push("LOCATION");
                    }
                    2 => {
                        e.description = words[rng.pick(6) as usize].into();
                        touched.push("DESCRIPTION");
                    }
                    3 => {
                        // whole days, so the clock time stays (and never falls into a summer-time gap)
                        let days = (1 + rng.pick(5) as i64) * 86_400;
                        (e.start, e.end) = (e.shift(e.start, days), e.shift(e.end, days));
                        touched.push("DTSTART");
                    }
                    4 => {
                        e.status = ["", "CONFIRMED", "CANCELLED"][rng.pick(3) as usize].into();
                        touched.push("STATUS");
                    }
                    5 => {
                        e.transparent = !e.transparent;
                        touched.push("TRANSP");
                    }
                    6 => {
                        e.categories = (0..rng.pick(3)).map(|k| format!("Cat {k}, x")).collect();
                        touched.push("CATEGORIES");
                    }
                    7 if e.recurrence_id.is_none() => {
                        e.rrule = [None, Some("FREQ=DAILY;COUNT=3".to_string()), Some("FREQ=WEEKLY;BYDAY=TU".to_string())][rng.pick(3) as usize].clone();
                        touched.push("RRULE");
                    }
                    8 if e.rrule.is_some() => {
                        e.exdates.push(e.shift(e.start, 7 * 86_400 * (1 + rng.pick(4) as i64)));
                        touched.push("EXDATE");
                    }
                    9 => e.alarms.push(Alarm::before_start(60 * rng.pick(600) as i64)),
                    10 if !e.alarms.is_empty() => {
                        e.alarms.remove(rng.pick(e.alarms.len() as u64) as usize);
                    }
                    _ => {}
                }
            }
            let out = to_ics("", std::slice::from_ref(&e));
            assert!(out.lines().all(|l| l.len() <= 75), "round {round}: lines are folded");
            let back = parse_ics(&out).pop().unwrap_or_else(|| panic!("round {round}: no event in {out}"));
            // 1. the event reads back as it was written
            let fields = |x: &Event| (x.uid.clone(), x.summary.clone(), x.location.clone(), x.description.clone(), x.start, x.end, x.all_day, x.tzid.clone(), x.floating, x.status.clone(), x.transparent, x.categories.clone());
            assert_eq!(fields(&back), fields(&e), "round {round}: {out}");
            assert_eq!((&back.rrule, &back.exdates, &back.rdates, back.recurrence_id, &back.organizer, &back.attendees), (&e.rrule, &e.exdates, &e.rdates, e.recurrence_id, &e.organizer, &e.attendees), "round {round}: {out}");
            assert_eq!(back.alarms.iter().map(|a| a.before).collect::<Vec<_>>(), e.alarms.iter().map(|a| a.before).collect::<Vec<_>>(), "round {round}: {out}");
            // 2. every line of a field that was not touched, and every line Shale does not know, is there as it was
            let written = unfold(&out);
            let mut k = 0;
            while k < before.src.len() {
                let line = &before.src[k];
                let (name, _, value) = split_line(line);
                if name == "BEGIN" {
                    let end = block_end(&before.src, k);
                    if !value.eq_ignore_ascii_case("VALARM") {
                        assert!(before.src[k..end].iter().all(|l| written.contains(l)), "round {round}: a part inside the event was lost: {out}");
                    }
                    k = end;
                    continue;
                }
                k += 1;
                let changed = !same_field("", &e, &before) || e != before;
                let stamp = matches!(name.as_str(), "DTSTAMP" | "LAST-MODIFIED") && changed;
                if !stamp && !field_of(&name).is_some_and(|f| touched.contains(&f)) {
                    assert!(written.contains(line), "round {round}: lost {line:?} (touched {touched:?}) in {out}");
                }
            }
            // 3. reminders that were not removed are the ones that were read
            for a in e.alarms.iter().filter(|a| !a.raw.is_empty()) {
                assert!(a.raw.iter().all(|l| written.contains(l)), "round {round}: a reminder was rewritten: {out}");
            }
            // 4. writing what was read again changes nothing more
            let stable = |text: &str| content(text).into_iter().filter(|l| !l.starts_with("DTSTAMP:") && !l.starts_with("LAST-MODIFIED:")).collect::<Vec<_>>();
            assert_eq!(stable(&to_ics("", std::slice::from_ref(&back))), stable(&out), "round {round}");
            pool[i] = back;
        }
    }
}
